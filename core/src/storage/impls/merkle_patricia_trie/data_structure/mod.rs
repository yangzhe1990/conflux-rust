pub mod children_table;
pub mod node_ref;

#[cfg(test)]
mod tests;

pub use self::{
    children_table::CHILDREN_COUNT,
    node_ref::{NodeRefDeltaMpt, NodeRefDeltaMptCompact},
};

use self::{access_mode::*, children_table::*};
use super::{
    super::{
        super::super::db::COL_DELTA_TRIE, errors::*,
        state_manager::AtomicCommitTransaction,
    },
    cache::algorithm::lfru::LFRUHandle,
    guarded_value::*,
    maybe_in_place_byte_array::MaybeInPlaceByteArray,
    merkle::*,
    node_memory_manager::*,
    return_after_use::ReturnAfterUse,
};
use parking_lot::RwLockWriteGuard;
use rlp::*;
use std::{
    cmp::min,
    collections::BTreeSet,
    fmt::{Debug, Formatter},
    hint::unreachable_unchecked,
    marker::{Send, Sync},
    ops::Deref,
    vec::Vec,
};

// TODO(yz): choose the best place to attach the consts.
impl TrieNode {
    const BITS_0_3_MASK: u8 = 0x0f;
    const BITS_4_7_MASK: u8 = 0xf0;

    fn first_nibble(x: u8) -> u8 { x & Self::BITS_0_3_MASK }

    fn second_nibble(x: u8) -> u8 { (x & Self::BITS_4_7_MASK) >> 4 }

    pub fn set_second_nibble(x: u8, second_nibble: u8) -> u8 {
        Self::first_nibble(x) | (second_nibble << 4)
    }
}

// TODO(yz): statically check the size to be "around" 64B + 64B (merkle_hash)
// TODO(yz): TrieNode should leave one byte so that it can be used to indicate a
// free slot in memory region, in order to implement EntryTrait.
/// A node consists of an optional compressed path (concept of Patricia
/// Trie), an optional ChildrenTable (if the node is intermediate), an
/// optional value attached, and the Merkle hash for subtree.
#[derive(Default)]
pub struct TrieNode {
    /// The number of children plus one if there is value attached.
    /// After a delete operation, when there is no value attached to this
    /// path, and if there is only one child left, path compression
    /// should apply. Path compression can only happen when
    /// number_of_children_plus_value drop from 2 to 1.
    // TODO(yz): refactor out this value. Move the number_of_children counter
    // to children_table.
    number_of_children_plus_value: u8,

    /// CompactPath section. The CompactPath if defined as separate struct
    /// would consume 16B, while the current layout plus the
    /// previous u8 field consumes 16B in total and keep integers
    /// aligned.

    /// Can be: "no mask" (0x00), "second half" (BITS_4_7_MASK), "first half"
    /// (BITS_0_3_MASK). When there is only one half-byte in path and it's
    /// the "first half", both path_begin_mask and path_end_mask are set.
    /// This field is not used in comparison because matching one more
    /// half-byte at the beginning doesn't matter.
    // TODO(yz): remove since it's unused, now it's always 0.
    path_begin_mask: u8,
    /// Can be: "no mask" (0x00), "first half" (BITS_0_3_MASK).
    /// When there is only one half-byte in path and it's the "second half",
    /// only path_begin_mask is set, path_end_mask is set to "no mask",
    /// because the first byte of path actually keeps the missing
    /// "first half" so that it still matches to the corresponding byte of the
    /// key.
    path_end_mask: u8,
    /// 4 bits per step.
    /// We limit the maximum key steps by u16.
    path_steps: u16,
    path: MaybeInPlaceByteArray,
    // End of CompactPath section
    // TODO(yz): maybe unpack the fields from ChildrenTableDeltaMpt to save
    // memory. In this case create temporary ChildrenTableDeltaMpt for
    // update / iteration.
    children_table: ChildrenTableDeltaMpt,
    // Rust automatically moves the value_size field in order to minimize the
    // total size of the struct.
    /// We limit the maximum value length by u16. If it proves insufficient,
    /// manage the length and content separately.
    value_size: u32,
    value: MaybeInPlaceByteArray,

    /// The merkle hash without the compressed path.
    pub merkle_hash: MerkleHash,

    pub cache_algo_data: LFRUHandle<LFRUPosT>,
}

/// Compiler is not sure about the pointer in MaybeInPlaceByteArray fields.
/// It's Send because TrieNode is move only and it's impossible to change any
/// part of it without &mut.
unsafe impl Send for TrieNode {}
/// Compiler is not sure about the pointer in MaybeInPlaceByteArray fields.
/// We do not allow a &TrieNode to be able to change anything the pointer
/// is pointing to, therefore TrieNode is Sync.
unsafe impl Sync for TrieNode {}

type NodeMemoryRegion = Vec<TrieNode>;

// FIXME: implement EntryTrait for TrieNode.
pub type TrieNodeSlabEntry = super::slab::Entry<TrieNode>;
pub type VacantEntry<'a> =
    super::slab::VacantEntry<'a, TrieNode, TrieNodeSlabEntry>;

/// Key length should be multiple of 8.
// TODO(yz): align key @8B with mask.
type KeyPart<'a> = &'a [u8];
const EMPTY_KEY_PART: KeyPart = &[];

/// Implement section.

impl Drop for TrieNode {
    fn drop(&mut self) {
        unsafe {
            let size = self.value_size as usize;
            if size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE {
                self.value.ptr_into_vec(size);
            }

            self.clear_path();
        }
    }
}

impl TrieNode {
    const MAX_VALUE_SIZE: usize = 0xfffffffe;
    /// A special value to use in Delta Mpt to indicate that the value is
    /// deleted.
    const VALUE_TOMBSTONE: u32 = 0xffffffff;

    fn get_compressed_path_size(&self) -> u16 {
        (self.path_steps / 2)
            + (((self.path_begin_mask | self.path_end_mask) != 0) as u16)
    }

    fn compressed_path_ref(&self) -> CompressedPathRef {
        let size = self.get_compressed_path_size();
        CompressedPathRef {
            path_slice: self.path.get_slice(size as usize),
            end_mask: self.path_end_mask,
        }
    }

    unsafe fn clear_path(&mut self) {
        let size = self.get_compressed_path_size() as usize;
        if size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE {
            self.path.ptr_into_vec(size);
        }
    }

    fn compute_path_steps(path_size: u16, end_mask: u8) -> u16 {
        path_size * 2 - (end_mask != 0) as u16
    }

    fn copy_compressed_path(&mut self, new_path: CompressedPathRef) {
        let path_slice = new_path.path_slice;

        // Remove old path.
        unsafe {
            self.clear_path();
        }

        self.path_steps = Self::compute_path_steps(
            path_slice.len() as u16,
            new_path.end_mask,
        );
        self.path_end_mask = new_path.end_mask;
        self.path =
            MaybeInPlaceByteArray::copy_from(path_slice, path_slice.len());
    }

    fn set_compressed_path(&mut self, path: CompressedPathRaw) {
        // Remove old path.
        unsafe {
            self.clear_path();
        }

        self.path_steps =
            Self::compute_path_steps(path.path_size, path.end_mask);
        self.path_end_mask = path.end_mask;
        self.path = path.path;
    }

    fn has_value(&self) -> bool { self.value_size > 0 }

    fn get_children_count(&self) -> u8 {
        self.children_table.get_children_count()
    }

    fn value_as_slice(&self) -> Option<&[u8]> {
        let size = self.value_size;
        if size == 0 {
            None
        } else if size == Self::VALUE_TOMBSTONE {
            Some(&[])
        } else {
            Some(self.value.get_slice(size as usize))
        }
    }

    fn value_clone(&self) -> Option<Box<[u8]>> {
        let size = self.value_size;
        if size == 0 {
            None
        } else if size == Self::VALUE_TOMBSTONE {
            Some(Box::<[u8]>::default())
        } else {
            Some(self.value.get_slice(size as usize).into())
        }
    }

    /// Take value out of self.
    /// This method can only be called by replace_value / delete_value because
    /// empty node must be removed and pass compression must be maintained.
    // FIXME: move this method into a special session.
    fn value_into_boxed_slice(&mut self) -> Option<Box<[u8]>> {
        let size = self.value_size;
        let maybe_value;
        if size == 0 {
            maybe_value = None;
        } else {
            if size == Self::VALUE_TOMBSTONE {
                maybe_value = Some(Box::<[u8]>::default())
            } else {
                maybe_value = Some(self.value.into_boxed_slice(size as usize));
            }
            self.value_size = 0;
        }
        maybe_value
    }

    fn replace_value_valid(&mut self, valid_value: &[u8]) -> Option<Box<[u8]>> {
        let old_value = self.value_into_boxed_slice();
        let value_size = valid_value.len();
        if value_size == 0 {
            self.value_size = Self::VALUE_TOMBSTONE;
        } else {
            self.value =
                MaybeInPlaceByteArray::copy_from(valid_value, value_size);
            self.value_size = value_size as u32;
        }

        old_value
    }

    fn check_value_size(value: &[u8]) -> Result<()> {
        let value_size = value.len();
        if value_size > Self::MAX_VALUE_SIZE {
            // TODO(yz): value too long.
            return Err(Error::from_kind(ErrorKind::MPTInvalidValue));
        }
        // We may use empty value to represent special state, such as tombstone.
        // Therefore We don't check for emptiness.

        Ok(())
    }

    fn check_key_size(access_key: &[u8]) -> Result<()> {
        let key_size = access_key.len();
        if key_size > MaybeInPlaceByteArray::MAX_SIZE_U16 {
            // TODO(yz): key too long.
            return Err(Error::from_kind(ErrorKind::MPTInvalidKey));
        }
        if key_size == 0 {
            // TODO(yz): key is empty.
            return Err(Error::from_kind(ErrorKind::MPTInvalidKey));
        }

        Ok(())
    }
}

trait CompressedPathTrait {
    type SelfType;

    fn path_slice(&self) -> &[u8];
    fn end_mask(&self) -> u8;
}

impl<'a> CompressedPathTrait for &'a [u8] {
    type SelfType = Self;

    fn path_slice(&self) -> &[u8] { self }

    fn end_mask(&self) -> u8 { 0 }
}

impl<'a> CompressedPathTrait for CompressedPathRef<'a> {
    type SelfType = Self;

    fn path_slice(&self) -> &[u8] { self.path_slice }

    fn end_mask(&self) -> u8 { self.end_mask }
}

impl CompressedPathTrait for CompressedPathRaw {
    type SelfType = Self;

    fn path_slice(&self) -> &[u8] {
        self.path.get_slice(self.path_size as usize)
    }

    fn end_mask(&self) -> u8 { self.end_mask }
}

#[derive(Debug, PartialEq)]
pub struct CompressedPathRef<'a> {
    pub path_slice: &'a [u8],
    pub end_mask: u8,
}

#[derive(Default)]
pub struct CompressedPathRaw {
    path_size: u16,
    path: MaybeInPlaceByteArray,
    end_mask: u8,
}

impl<'a> From<&'a [u8]> for CompressedPathRaw {
    fn from(x: &'a [u8]) -> Self { CompressedPathRaw::new(x, 0) }
}

impl CompressedPathRaw {
    fn concat<X: CompressedPathTrait, Y: CompressedPathTrait>(
        x: &X, y: &Y,
    ) -> Self {
        let x_slice;
        if x.end_mask() != 0 {
            let s = x.path_slice();
            x_slice = &s[0..s.len() - 1];
        } else {
            x_slice = x.path_slice();
        }
        let y_slice = y.path_slice();
        let size = x_slice.len() + y_slice.len();

        let mut path;
        if size <= MaybeInPlaceByteArray::MAX_INPLACE_SIZE {
            path = MaybeInPlaceByteArray::copy_from(x_slice, size);
            path.get_slice_mut(size)
                [x_slice.len()..x_slice.len() + y_slice.len()]
                .clone_from_slice(y_slice);
        } else {
            path = MaybeInPlaceByteArray::copy_from(
                &([x_slice, y_slice].concat()),
                size,
            );
        }

        Self {
            path_size: size as u16,
            path: path,
            end_mask: y.end_mask(),
        }
    }

    fn new(path_slice: &[u8], end_mask: u8) -> Self {
        let path_size = path_slice.len();
        Self {
            path_size: path_size as u16,
            path: MaybeInPlaceByteArray::copy_from(path_slice, path_size),
            end_mask: end_mask,
        }
    }

    fn new_and_apply_mask(path_slice: &[u8], end_mask: u8) -> Self {
        let path_size = path_slice.len();
        let mut ret = Self {
            path_size: path_size as u16,
            path: MaybeInPlaceByteArray::copy_from(path_slice, path_size),
            end_mask: end_mask,
        };
        ret.path.get_slice_mut(path_size)[path_size - 1] &= end_mask;

        ret
    }

    fn new_zeroed(path_size: u16, end_mask: u8) -> Self {
        Self {
            path_size: path_size,
            path: MaybeInPlaceByteArray::new_zeroed(path_size as usize),
            end_mask: end_mask,
        }
    }
}

enum WalkStop<'key> {
    // path matching fails at some point. Want the new path_steps,
    // path_end_mask, ..., etc Basically, a new node should be created to
    // replace the current node from parent children table;
    // modify this node or create a new node to insert as children of new
    // node, (update path) then
    // the child that should be followed is nil at the new node.
    // if put single version, this node changes, this node replaced, parent
    // update child and merkle. Before merkle update, this node must be saved
    // in mem or into disk db (not that expensive). if get / delete (not
    // found)
    PathDiverted {
        /// Key may terminate on the path.
        key_child_index: Option<u8>,
        key_remaining: KeyPart<'key>,
        matched_path: CompressedPathRaw,
        unmatched_child_index: u8,
        unmatched_path_remaining: CompressedPathRaw,
    },

    // If exactly at this node.
    // if put, update this node
    // if delete, may cause deletion / path compression (delete this node,
    // parent update child, update path of original child node)
    Arrived,

    Descent {
        key_remaining: KeyPart<'key>,
        child_index: u8,
        child_node: NodeRefDeltaMpt,
    },

    // To descent, however child doesn't exists:
    // to modify this node or create a new node to replace this node (update
    // child) Then create a new node for remaining key_part (we don't care
    // about begin_mask). if put single version, this node changes, parent
    // update merkle. if get / delete (not found)
    ChildNotFound {
        key_remaining: KeyPart<'key>,
        child_index: u8,
    },
}

impl<'key> WalkStop<'key> {
    fn child_not_found_uninitialized() -> Self {
        WalkStop::ChildNotFound {
            key_remaining: Default::default(),
            child_index: 0,
        }
    }

    fn path_diverted_uninitialized() -> Self {
        WalkStop::PathDiverted {
            key_child_index: None,
            key_remaining: Default::default(),
            matched_path: Default::default(),
            unmatched_child_index: 0,
            unmatched_path_remaining: Default::default(),
        }
    }
}

mod access_mode {
    pub trait AccessMode {
        fn is_read_only() -> bool;
    }

    pub struct Read {}
    pub struct Write {}

    impl AccessMode for Read {
        fn is_read_only() -> bool { return true; }
    }

    impl AccessMode for Write {
        fn is_read_only() -> bool { return false; }
    }
}

/// Traverse.
impl TrieNode {
    // TODO(yz): write test.
    /// The start of key is always aligned with compressed path of
    /// current node, e.g. if compressed path starts at the second-half, so
    /// should be key.
    fn walk<'key, AM: AccessMode>(&self, key: KeyPart<'key>) -> WalkStop<'key> {
        let path = self.compressed_path_ref();
        let path_slice = path.path_slice;

        // Compare bytes till the last full byte. The first byte is always
        // included because even if it's the second-half, it must be
        // already matched before entering this TrieNode.
        let memcmp_len = min(
            path_slice.len() - ((path.end_mask != 0) as usize),
            key.len(),
        );

        for i in 0..memcmp_len {
            if path_slice[i] != key[i] {
                if AM::is_read_only() {
                    return WalkStop::path_diverted_uninitialized();
                } else {
                    let matched_path: CompressedPathRaw;
                    let key_child_index: u8;
                    let key_remaining: &[u8];
                    let unmatched_child_index: u8;
                    let unmatched_path_remaining: &[u8];

                    if Self::first_nibble(path_slice[i] ^ key[i]) == 0 {
                        // "First half" matched
                        matched_path = CompressedPathRaw::new_and_apply_mask(
                            &path_slice[0..i + 1],
                            Self::first_nibble(!0),
                        );

                        key_child_index = Self::second_nibble(key[i]);
                        key_remaining = &key[i + 1..];
                        unmatched_child_index =
                            Self::second_nibble(path_slice[i]);
                        unmatched_path_remaining = &path_slice[i + 1..];
                    } else {
                        matched_path =
                            CompressedPathRaw::new(&path_slice[0..i], 0);
                        key_child_index = Self::first_nibble(key[i]);
                        key_remaining = &key[i..];
                        unmatched_child_index =
                            Self::first_nibble(path_slice[i]);
                        unmatched_path_remaining = &path_slice[i..];
                    }
                    return WalkStop::PathDiverted {
                        key_child_index: Some(key_child_index),
                        key_remaining: key_remaining.into(),
                        matched_path: matched_path,
                        unmatched_child_index: unmatched_child_index,
                        unmatched_path_remaining: CompressedPathRaw::new(
                            unmatched_path_remaining,
                            self.path_end_mask,
                        ),
                    };
                }
            }
        }
        // Key is fully consumed, get value attached.
        if key.len() == memcmp_len {
            // Compressed path isn't fully consumed.
            if path_slice.len() > memcmp_len {
                if AM::is_read_only() {
                    return WalkStop::path_diverted_uninitialized();
                } else {
                    return WalkStop::PathDiverted {
                        // key_remaining is empty, and key_child_index doesn't
                        // make sense, but we need to
                        // mark it.
                        key_remaining: Default::default(),
                        key_child_index: None,
                        matched_path: CompressedPathRaw::new(
                            &path_slice[0..memcmp_len],
                            0,
                        ),
                        unmatched_child_index: Self::first_nibble(
                            path_slice[memcmp_len],
                        ),
                        unmatched_path_remaining: CompressedPathRaw::new(
                            &path_slice[memcmp_len..],
                            self.path_end_mask,
                        ),
                    };
                }
            } else {
                return WalkStop::Arrived;
            }
        } else {
            // Key is not fully consumed.

            // When path is fully consumed, check if child exists under
            // child_index.
            let child_index;
            let key_remaining;

            if path_slice.len() == memcmp_len {
                // Compressed path is fully consumed. Descend into one child.
                child_index = Self::first_nibble(key[memcmp_len]);
                key_remaining = &key[memcmp_len..];
            } else {
                // One half byte remaining to match with path. Consume it in the
                // key.
                if Self::first_nibble(path_slice[memcmp_len] ^ key[memcmp_len])
                    != 0
                {
                    // Mismatch.
                    if AM::is_read_only() {
                        return WalkStop::path_diverted_uninitialized();
                    } else {
                        return WalkStop::PathDiverted {
                            key_child_index: Some(Self::first_nibble(
                                key[memcmp_len],
                            )),
                            key_remaining: &key[memcmp_len..],
                            matched_path: CompressedPathRaw::new(
                                &path_slice[0..memcmp_len],
                                0,
                            ),
                            unmatched_child_index: Self::first_nibble(
                                path_slice[memcmp_len],
                            ),
                            unmatched_path_remaining: CompressedPathRaw::new(
                                &path_slice[memcmp_len..],
                                self.path_end_mask,
                            ),
                        };
                    }
                } else {
                    child_index = Self::second_nibble(key[memcmp_len]);
                    key_remaining = &key[memcmp_len + 1..];
                }
            }

            match self.get_child(child_index) {
                Option::None => {
                    if AM::is_read_only() {
                        return WalkStop::child_not_found_uninitialized();
                    }
                    return WalkStop::ChildNotFound {
                        key_remaining: key_remaining,
                        child_index: child_index,
                    };
                }
                Option::Some(child_node) => {
                    return WalkStop::Descent {
                        key_remaining: key_remaining,
                        child_node: child_node.into(),
                        child_index: child_index,
                    };
                }
            }
        }
    }
}

/// The actions for the logical trie. Since we maintain a multiple version trie
/// the action must be translated into trie node operations, which may vary
/// depends on whether the node is owned by current version, etc.
enum TrieNodeAction {
    Modify,
    Delete,
    MergePath {
        child_index: u8,
        child_node_ref: NodeRefDeltaMpt,
    },
    DeleteChildrenTable,
}

/// Update
impl TrieNode {
    pub fn new(
        merkle: &MerkleHash, children_table: ChildrenTableDeltaMpt,
        maybe_value: Option<Vec<u8>>, compressed_path: CompressedPathRaw,
    ) -> TrieNode
    {
        let mut ret = TrieNode::default();

        ret.merkle_hash = *merkle;
        ret.children_table = children_table;
        match maybe_value {
            None => {}
            Some(value) => {
                ret.replace_value_valid(value.as_ref());
            }
        }
        ret.set_compressed_path(compressed_path);

        ret
    }

    /// new_value can only be set according to the situation.
    /// children_table can only be replaced when there is no children in both
    /// old and new table.
    ///
    /// unsafe because:
    /// 1. precondition on children_table;
    /// 2. delete value assumes that self contains some value.
    unsafe fn copy_and_replace_fields(
        &self, new_value: Option<Option<&[u8]>>,
        new_path: Option<CompressedPathRaw>,
        children_table: Option<ChildrenTableDeltaMpt>,
    ) -> TrieNode
    {
        let mut ret = TrieNode::default();

        match new_value {
            Some(maybe_value) => match maybe_value {
                Some(value) => {
                    ret.replace_value_valid(value);
                }
                None => {
                    ret.delete_value_unchecked();
                }
            },
            None => {
                let value_size = self.value_size as usize;
                ret.value_size = self.value_size;
                ret.value = MaybeInPlaceByteArray::copy_from(
                    self.value.get_slice(value_size),
                    value_size,
                );
            }
        }

        match new_path {
            Some(path) => ret.set_compressed_path(path),
            None => ret.copy_compressed_path(self.compressed_path_ref()),
        }

        match children_table {
            Some(table) => ret.children_table = table,
            None => ret.children_table = self.children_table.clone(),
        }

        ret
    }

    fn path_prepended(
        &self, prefix: CompressedPathRaw, child_index: u8,
    ) -> CompressedPathRaw {
        let prefix_size = prefix.path_slice().len();
        let path_size = self.get_compressed_path_size();
        // TODO(yz): it happens to be the same no matter what end_mask is,
        // because u8 = 2 nibbles. When we switch to u32 as path unit
        // the concated size may vary.
        let concated_size = prefix_size as u16 + path_size;

        let path = self.compressed_path_ref();

        let mut new_path =
            CompressedPathRaw::new_zeroed(concated_size, path.end_mask);

        {
            let slice = new_path.path.get_slice_mut(concated_size as usize);
            if prefix.end_mask() == 0 {
                slice[0..prefix_size].copy_from_slice(prefix.path_slice());
                slice[prefix_size..].copy_from_slice(path.path_slice);
            } else {
                slice[0..prefix_size - 1]
                    .copy_from_slice(&prefix.path_slice()[0..prefix_size - 1]);
                slice[prefix_size - 1] = Self::set_second_nibble(
                    prefix.path_slice()[prefix_size - 1],
                    child_index,
                );
                slice[prefix_size..].copy_from_slice(path.path_slice);
            }
        }

        new_path
    }

    /// Delete value when we know that it already exists.
    unsafe fn delete_value_unchecked(&mut self) -> Box<[u8]> {
        self.value_into_boxed_slice().unwrap()
    }

    /// Returns: old_value, is_self_about_to_delete, replacement_node_for_self
    fn check_delete_value(&self) -> Result<TrieNodeAction> {
        if self.has_value() {
            Ok(match self.get_children_count() {
                0 => TrieNodeAction::Delete,
                1 => self.merge_path_action(),
                _ => TrieNodeAction::Modify,
            })
        } else {
            Err(ErrorKind::MPTKeyNotFound.into())
        }
    }

    fn merge_path_action(&self) -> TrieNodeAction {
        for (i, node_ref) in self.children_table.iter() {
            return TrieNodeAction::MergePath {
                child_index: i,
                child_node_ref: (*node_ref).into(),
            };
        }
        unsafe { unreachable_unchecked() }
    }

    fn merge_path_action_after_child_deletion(
        &self, child_index: u8,
    ) -> TrieNodeAction {
        for (i, node_ref) in self.children_table.iter() {
            if i != child_index {
                return TrieNodeAction::MergePath {
                    child_index: i,
                    child_node_ref: (*node_ref).into(),
                };
            }
        }
        unsafe { unreachable_unchecked() }
    }

    fn get_child(&self, child_index: u8) -> Option<NodeRefDeltaMptCompact> {
        self.children_table.get_child(child_index)
    }

    unsafe fn set_first_child_unchecked(
        &mut self, child_index: u8, child: NodeRefDeltaMptCompact,
    ) {
        self.children_table =
            ChildrenTableDeltaMpt::new_from_one_child(child_index, child);
    }

    unsafe fn add_new_child_unchecked(
        &mut self, child_index: u8, child: NodeRefDeltaMptCompact,
    ) {
        self.children_table = CompactedChildrenTable::insert_child_unchecked(
            self.children_table.to_ref(),
            child_index,
            child,
        );
    }

    /// Unsafe because it's assumed that the child_index already exists.
    unsafe fn delete_child_unchecked(&mut self, child_index: u8) {
        self.children_table = CompactedChildrenTable::delete_child_unchecked(
            self.children_table.to_ref(),
            child_index,
        );
    }

    /// Unsafe because it's assumed that the child_index already exists.
    unsafe fn replace_child_unchecked(
        &mut self, child_index: u8, new_child_node: NodeRefDeltaMptCompact,
    ) {
        self.children_table
            .set_child_unchecked(child_index, new_child_node);
    }

    /// Returns old_child, is_self_about_to_delete, replacement_node_for_self
    fn check_replace_or_delete_child_action(
        &self, child_index: u8, new_child_node: Option<NodeRefDeltaMptCompact>,
    ) -> TrieNodeAction {
        if new_child_node.is_none() {
            match self.get_children_count() {
                1 => {
                    // There is no children left after deletion.
                    TrieNodeAction::DeleteChildrenTable
                }
                2 => self.merge_path_action_after_child_deletion(child_index),
                _ => TrieNodeAction::Modify,
            }
        } else {
            TrieNodeAction::Modify
        }
    }
}

// TODO(yz): move to file for merkle_patricia_trie.
use super::MultiVersionMerklePatriciaTrie;

/// CowNodeRef facilities access and modification to trie nodes in multi-version
/// MPT. It offers read-only access to the original trie node, and creates an
/// unique owned trie node once there is any modification. The ownership is
/// maintained centralized in owned_node_set passed into many methods as
/// argument.
pub struct CowNodeRef {
    // TODO(yz): remove moved because it's fine to only use owned.
    moved: bool,
    owned: bool,
    pub node_ref: NodeRefDeltaMpt,
}

pub type OwnedNodeSet = BTreeSet<NodeRefDeltaMpt>;

impl CowNodeRef {
    pub fn new_uninitialized_node<'a>(
        allocator: AllocatorRefRef<'a>, owned_node_set: &mut OwnedNodeSet,
    ) -> Result<(Self, VacantEntry<'a>)> {
        let (node_ref, new_entry) = NodeMemoryManager::new_node(allocator)?;
        owned_node_set.insert(node_ref.clone());

        Ok((
            Self {
                owned: true,
                moved: false,
                node_ref: node_ref,
            },
            new_entry,
        ))
    }

    pub fn new(
        node_ref: NodeRefDeltaMpt, owned_node_set: &OwnedNodeSet,
    ) -> Self {
        Self {
            owned: owned_node_set.contains(&node_ref),
            moved: false,
            node_ref: node_ref,
        }
    }

    /// Steal the value of Self, and leave the value into a state that's ready
    /// to drop. The purpose of this method is only to circumvent Rust's
    /// ownership check.
    fn steal(&mut self) -> Self {
        let ret = Self {
            moved: self.moved,
            owned: self.owned,
            node_ref: self.node_ref.clone(),
        };

        self.moved = true;
        ret
    }

    fn convert_to_owned<'a>(
        &mut self, node_memory_manager: &'a NodeMemoryManager,
        allocator: AllocatorRefRef<'a>, owned_node_set: &mut OwnedNodeSet,
    ) -> Result<Option<VacantEntry<'a>>>
    {
        if self.owned {
            Ok(None)
        } else {
            // TODO(yz): maybe use Self::new_uninitialized_node().
            let (node_ref, new_entry) =
                NodeMemoryManager::new_node(&allocator)?;
            owned_node_set.insert(node_ref.clone());
            self.node_ref = node_ref;

            Ok(Some(new_entry))
        }
    }

    pub fn delete_node(mut self, node_memory_manager: &NodeMemoryManager) {
        if self.owned {
            node_memory_manager.free_node(&mut self.node_ref);
            self.owned = false;
        }
        self.moved = true;
    }

    // TODO(yz): call into_child on unowned node should be an error.
    pub fn into_child(mut self) -> Option<NodeRefDeltaMptCompact> {
        if !self.moved {
            self.moved = true;
            Some(self.node_ref.clone().into())
        } else {
            None
        }
    }

    fn commit_children(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        commit_transaction: &mut AtomicCommitTransaction,
        cache_manager: &mut CacheManager, allocator_ref: AllocatorRefRef,
    ) -> Result<()>
    {
        for (i, node_ref_mut) in trie_node.children_table.iter_mut() {
            let node_ref = node_ref_mut.clone();
            let mut cow_child_node = Self::new(node_ref.into(), owned_node_set);
            let trie_node = trie
                .get_node_memory_manager()
                .node_as_mut_with_cache_manager(
                    allocator_ref,
                    &mut cow_child_node.node_ref,
                    cache_manager,
                )?;
            let was_owned = cow_child_node.commit(
                trie,
                owned_node_set,
                trie_node,
                commit_transaction,
                cache_manager,
                allocator_ref,
            )?;

            // An owned child TrieNode now have a new NodeRef.
            // Unowned child TrieNode isn't changed so it's safe
            // to not reset children
            // table in parent node.
            let new_node_ref = cow_child_node.into_child();
            if was_owned {
                *node_ref_mut = new_node_ref.clone().unwrap();
            }
        }
        Ok(())
    }

    fn set_merkle(
        &mut self, children_merkles: MaybeMerkleTableRef,
        trie_node: &mut TrieNode,
    ) -> MerkleHash
    {
        let path_merkle = compute_merkle(
            trie_node.compressed_path_ref(),
            children_merkles,
            trie_node.value_as_slice(),
        );
        trie_node.merkle_hash = path_merkle;

        path_merkle
    }

    /// Get if unowned, compute if owned.
    pub fn get_or_compute_merkle(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, allocator_ref: AllocatorRefRef,
    ) -> Result<MerkleHash>
    {
        if self.owned {
            // FIXME: refactor.
            // It's safe because owned nodes are not owned by cache manager.
            let trie_node = unsafe {
                trie.get_node_memory_manager()
                    .node_as_mut(allocator_ref, &mut self.node_ref)?
                    .into_value()
            };
            let children_merkles = self.get_or_compute_children_merkles(
                trie,
                owned_node_set,
                trie_node,
                allocator_ref,
            )?;

            let merkle = self.set_merkle(children_merkles.as_ref(), trie_node);

            Ok(merkle)
        } else {
            let trie_node = trie
                .get_node_memory_manager()
                .node_as_ref(allocator_ref, &self.node_ref)?;
            Ok(trie_node.merkle_hash)
        }
    }

    // FIXME: get allocator outside recursion. do the same for commit.
    fn get_or_compute_children_merkles(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        allocator_ref: AllocatorRefRef,
    ) -> Result<MaybeMerkleTable>
    {
        match trie_node.children_table.get_children_count() {
            0 => Ok(None),
            _ => {
                let mut merkles = ChildrenMerkleTable::default();
                for (i, maybe_node_ref_mut) in
                    trie_node.children_table.iter_non_skip()
                {
                    match maybe_node_ref_mut {
                        None => merkles[i as usize] = MERKLE_NULL_NODE,
                        Some(node_ref_mut) => {
                            let mut cow_child_node = Self::new(
                                (*node_ref_mut).into(),
                                owned_node_set,
                            );
                            let merkle = cow_child_node.get_or_compute_merkle(
                                trie,
                                owned_node_set,
                                allocator_ref,
                            )?;
                            // There is no change to the child_node so the
                            // return value is dropped.
                            cow_child_node.into_child();

                            merkles[i as usize] = merkle;
                        }
                    }
                }
                Ok(Some(merkles))
            }
        }
    }

    // FIXME: unit test.
    fn iterate_internal<TrieNodeRef: Deref<Target = TrieNode>>(
        &self, owned_node_set: &OwnedNodeSet,
        trie: &MultiVersionMerklePatriciaTrie, allocator_ref: AllocatorRefRef,
        trie_node: TrieNodeRef, key_prefix: CompressedPathRaw,
        values: &mut Vec<(Vec<u8>, Box<[u8]>)>,
    ) -> Result<()>
    {
        if trie_node.has_value() {
            assert_eq!(key_prefix.end_mask(), 0);
            values.push((
                key_prefix.path_slice().to_vec(),
                trie_node.value_clone().unwrap(),
            ));
        }

        let children_table = trie_node.children_table.clone();
        // Free the lock for trie_node.
        // FIXME: try to share the lock.
        drop(trie_node);
        for (i, node_ref) in children_table.iter() {
            let mut cow_child_node =
                Self::new((*node_ref).into(), owned_node_set);
            let child_node = trie
                .get_node_memory_manager()
                .node_as_ref(allocator_ref, &mut cow_child_node.node_ref)?;
            let key_prefix = CompressedPathRaw::concat(
                &key_prefix,
                &child_node.compressed_path_ref(),
            );
            cow_child_node.iterate_internal(
                owned_node_set,
                trie,
                allocator_ref,
                child_node,
                key_prefix,
                values,
            )?;
        }

        Ok(())
    }

    /// Recursively commit dirty nodes.
    pub fn commit(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        commit_transaction: &mut AtomicCommitTransaction,
        cache_manager: &mut CacheManager, allocator_ref: AllocatorRefRef,
    ) -> Result<bool>
    {
        if self.owned {
            self.commit_children(
                trie,
                owned_node_set,
                trie_node,
                commit_transaction,
                cache_manager,
                allocator_ref,
            )?;

            let db_key = commit_transaction.info.row_number.value;
            commit_transaction.transaction.put(
                COL_DELTA_TRIE,
                commit_transaction.info.row_number.to_string().as_bytes(),
                trie_node.rlp_bytes().as_slice(),
            );
            commit_transaction.info.row_number =
                commit_transaction.info.row_number.get_next()?;

            let slot = match &self.node_ref {
                NodeRefDeltaMpt::Dirty { index } => *index,
                _ => unsafe { unreachable_unchecked() },
            };
            owned_node_set.remove(&self.node_ref);
            self.node_ref = NodeRefDeltaMpt::Committed { db_key: db_key };
            owned_node_set.insert(self.node_ref.clone());

            cache_manager.insert_to_node_ref_map(
                db_key,
                slot,
                trie.get_node_memory_manager(),
            );

            Ok(true)
        } else {
            Ok(false)
        }
    }

    // FIXME: why mut TrieNode when Cow doesn't own it?
    unsafe fn delete_value_unchecked_if_owned(
        &mut self, trie_node: &mut TrieNode,
    ) -> Box<[u8]> {
        if self.owned {
            trie_node.delete_value_unchecked()
        } else {
            trie_node.value_clone().unwrap()
        }
    }

    fn cow_set_compressed_path(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, path: CompressedPathRaw,
        trie_node: &mut TrieNode,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(None, Some(path), None)
                });
            }
            None => {
                trie_node.set_compressed_path(path);
            }
        }
        Ok(())
    }

    fn cow_delete_value_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<Box<[u8]>>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        Ok(match copied {
            None => unsafe { trie_node.delete_value_unchecked() },
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(Some(None), None, None)
                });
                trie_node.value_clone().unwrap()
            }
        })
    }

    fn cow_replace_value_valid(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        value: &[u8],
    ) -> Result<Option<Box<[u8]>>>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        Ok(match copied {
            None => trie_node.replace_value_valid(value),
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(
                        Some(Some(value)),
                        None,
                        None,
                    )
                });
                trie_node.value_clone()
            }
        })
    }

    fn cow_delete_children_table(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                trie_node.children_table = ChildrenTableDeltaMpt::default();
            }
            Some(new_entry) => {
                new_entry.insert(unsafe {
                    trie_node.copy_and_replace_fields(
                        None,
                        None,
                        Some(ChildrenTableDeltaMpt::default()),
                    )
                });
            }
        }
        Ok(())
    }

    unsafe fn cow_replace_child_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        child_index: u8, child_node: NodeRefDeltaMptCompact,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                trie_node.replace_child_unchecked(child_index, child_node);
            }
            Some(new_entry) => {
                let mut new_trie_node =
                    trie_node.copy_and_replace_fields(None, None, None);
                new_trie_node.replace_child_unchecked(child_index, child_node);
                new_entry.insert(new_trie_node);
            }
        }

        Ok(())
    }

    unsafe fn cow_delete_child_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        child_index: u8,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                trie_node.delete_child_unchecked(child_index);
            }
            Some(new_entry) => {
                let mut new_trie_node =
                    trie_node.copy_and_replace_fields(None, None, None);
                new_trie_node.delete_child_unchecked(child_index);
                new_entry.insert(new_trie_node);
            }
        }

        Ok(())
    }

    // FIXME: How to replace duplicated code?
    unsafe fn cow_add_new_child_unchecked(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        child_index: u8, child_node: NodeRefDeltaMptCompact,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied = self.convert_to_owned(
            node_memory_manager,
            &allocator,
            owned_node_set,
        )?;
        match copied {
            None => {
                trie_node.add_new_child_unchecked(child_index, child_node);
            }
            Some(new_entry) => {
                let mut new_trie_node =
                    trie_node.copy_and_replace_fields(None, None, None);
                new_trie_node.add_new_child_unchecked(child_index, child_node);
                new_entry.insert(new_trie_node);
            }
        }

        Ok(())
    }
}

impl Clone for CowNodeRef {
    /// Cloned CowNodeRef doesn't own the node together.
    fn clone(&self) -> Self {
        Self {
            owned: false,
            moved: false,
            node_ref: self.node_ref.clone(),
        }
    }
}

impl Drop for CowNodeRef {
    /// Assert that the CowNodeRef doesn't own something.
    fn drop(&mut self) {
        assert_eq!(self.moved || !self.owned, true);
    }
}

pub struct SubTrieVisitor<'trie> {
    root: CowNodeRef,

    trie_ref: &'trie MultiVersionMerklePatriciaTrie,

    /// We use ReturnAfterUse because only one SubTrieVisitor(the deepest) can
    /// hold the mutable reference of owned_node_set.
    owned_node_set: ReturnAfterUse<'trie, OwnedNodeSet>,
}

impl<'trie> SubTrieVisitor<'trie> {
    pub fn new(
        trie_ref: &'trie MultiVersionMerklePatriciaTrie, root: NodeRefDeltaMpt,
        owned_node_set: &'trie mut Option<OwnedNodeSet>,
    ) -> Self
    {
        Self {
            trie_ref: trie_ref,
            root: CowNodeRef::new(root, owned_node_set.as_ref().unwrap()),
            owned_node_set: ReturnAfterUse::new(owned_node_set),
        }
    }

    fn new_visitor_for_subtree<'a>(
        &'a mut self, child_node: NodeRefDeltaMpt,
    ) -> SubTrieVisitor<'a>
    where 'trie: 'a {
        let trie_ref = self.trie_ref;
        let cow_child_node =
            CowNodeRef::new(child_node, self.owned_node_set.get_ref());
        SubTrieVisitor {
            trie_ref: trie_ref,
            root: cow_child_node,
            owned_node_set: ReturnAfterUse::new_from_origin(
                &mut self.owned_node_set,
            ),
        }
    }

    fn get_trie_ref(&self) -> &'trie MultiVersionMerklePatriciaTrie {
        self.trie_ref
    }

    fn node_memory_manager(&self) -> &'trie NodeMemoryManager {
        &self.get_trie_ref().get_node_memory_manager()
    }

    fn memory_allocator_borrow(&self) -> AllocatorRef<'trie> {
        self.node_memory_manager().get_allocator()
    }

    fn get_trie_node<'a>(
        &self, key: KeyPart, allocator_ref: AllocatorRefRef<'a>,
    ) -> Result<
        Option<GuardedValue<RwLockWriteGuard<'a, CacheManager>, &'a TrieNode>>,
    >
    where 'trie: 'a {
        let mut node_cow = self.root.clone();
        let mut key = key;
        loop {
            let trie_node = self
                .node_memory_manager()
                .node_as_ref(allocator_ref, &node_cow.node_ref)?;
            match trie_node.walk::<Read>(key) {
                WalkStop::Arrived => {
                    return Ok(Some(trie_node));
                }
                WalkStop::Descent {
                    key_remaining,
                    child_index: _,
                    child_node,
                } => {
                    node_cow = CowNodeRef::new(
                        child_node,
                        self.owned_node_set.get_ref(),
                    );
                    key = key_remaining;
                }
                _ => {
                    return Ok(None);
                }
            }
        }
    }

    pub fn get(&self, key: KeyPart) -> Result<Option<Box<[u8]>>> {
        let allocator = self.node_memory_manager().get_allocator();
        let maybe_trie_node = self.get_trie_node(key, &allocator)?;

        Ok(match maybe_trie_node {
            None => None,
            Some(trie_node) => trie_node.value_clone(),
        })
    }

    pub fn get_merkle_hash_wo_compressed_path(
        &self, key: KeyPart,
    ) -> Result<Option<MerkleHash>> {
        let allocator = self.node_memory_manager().get_allocator();
        let maybe_trie_node = self.get_trie_node(key, &allocator)?;

        match maybe_trie_node {
            None => Ok(None),
            Some(trie_node) => {
                if trie_node.get_compressed_path_size() == 0 {
                    Ok(Some(trie_node.merkle_hash))
                } else {
                    let maybe_value = trie_node.value_clone();
                    let merkles = match trie_node
                        .children_table
                        .get_children_count()
                    {
                        0 => None,
                        _ => {
                            let mut merkles = ChildrenMerkleTable::default();
                            let children_table =
                                trie_node.children_table.clone();
                            drop(trie_node);
                            for (i, maybe_node_ref) in
                                children_table.iter_non_skip()
                            {
                                merkles[i as usize] = match maybe_node_ref {
                                    None => super::merkle::MERKLE_NULL_NODE,
                                    Some(node_ref) => self
                                        .trie_ref
                                        .get_merkle(Some((*node_ref).into()))?
                                        .unwrap(),
                                };
                            }

                            Some(merkles)
                        }
                    };

                    Ok(Some(compute_node_merkle(
                        merkles.as_ref(),
                        maybe_value.as_ref().map(|value| value.as_ref()),
                    )))
                }
            }
        }
    }

    /// The visitor can only be used once to modify.
    /// Returns (deleted value, is root node replaced, the current root node for
    /// the subtree).
    pub fn delete(
        mut self, key: KeyPart,
    ) -> Result<(Option<Box<[u8]>>, bool, Option<NodeRefDeltaMptCompact>)> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = self.root.steal();
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        // FIXME: map_split?
        let mut trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;
        match trie_node_mut.walk::<Read>(key) {
            WalkStop::Arrived => {
                // If value doesn't exists, returns invalid key error.
                let result = trie_node_mut.check_delete_value();
                if result.is_err() {
                    return Ok((None, false, node_cow.into_child()));
                }
                let action = result.unwrap();
                match action {
                    TrieNodeAction::Delete => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow.delete_value_unchecked_if_owned(
                                &mut trie_node_mut,
                            )
                        };
                        // FIXME: deal with deletion while holding the
                        // trie_node_mut.
                        node_cow.delete_node(self.node_memory_manager());
                        Ok((Some(value), true, None))
                    }
                    TrieNodeAction::MergePath {
                        child_index,
                        child_node_ref,
                    } => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow.delete_value_unchecked_if_owned(
                                &mut trie_node_mut,
                            )
                        };

                        let new_path: CompressedPathRaw;
                        let mut child_node_cow: CowNodeRef;
                        {
                            let path_prefix = CompressedPathRaw::new(
                                trie_node_mut
                                    .compressed_path_ref()
                                    .path_slice(),
                                trie_node_mut.compressed_path_ref().end_mask(),
                            );
                            // COW modify child,
                            child_node_cow = CowNodeRef::new(
                                child_node_ref,
                                self.owned_node_set.get_ref(),
                            );
                            // FIXME: try to share the lock.
                            drop(trie_node_mut);
                            new_path = node_memory_manager
                                .node_as_ref(
                                    &allocator,
                                    &child_node_cow.node_ref,
                                )?
                                .path_prepended(path_prefix, child_index);
                        }
                        // FIXME: error processing for OOM.
                        let mut child_trie_node = node_memory_manager
                            .node_as_mut(
                                &allocator,
                                &mut child_node_cow.node_ref,
                            )?;
                        child_node_cow.cow_set_compressed_path(
                            &node_memory_manager,
                            self.owned_node_set.get_mut(),
                            new_path,
                            &mut child_trie_node,
                        )?;

                        // FIXME: how to represent that trie_node_mut is invalid
                        // after call to node_mut.delete_node?
                        // FIXME: trie_node_mut should be considered ref of
                        // node_mut.
                        node_cow.delete_node(self.node_memory_manager());

                        Ok((Some(value), true, child_node_cow.into_child()))
                    }
                    TrieNodeAction::Modify => {
                        let node_changed = !node_cow.owned;
                        let value = node_cow.cow_delete_value_unchecked(
                            &node_memory_manager,
                            self.owned_node_set.get_mut(),
                            &mut trie_node_mut,
                        )?;

                        Ok((Some(value), node_changed, node_cow.into_child()))
                    }
                    _ => unsafe { unreachable_unchecked() },
                }
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                drop(trie_node_mut);
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .delete(key_remaining);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let mut trie_node_mut = node_memory_manager
                    .node_as_mut(&allocator, &mut node_cow.node_ref)?;
                let (value, child_replaced, new_child_node) = result.unwrap();
                if child_replaced {
                    let action = trie_node_mut
                        .check_replace_or_delete_child_action(
                            child_index,
                            new_child_node,
                        );
                    match action {
                        TrieNodeAction::MergePath {
                            child_index,
                            child_node_ref,
                        } => {
                            // FIXME: how to reuse code?
                            let new_path: CompressedPathRaw;
                            let mut child_node_cow: CowNodeRef;
                            {
                                let path_prefix = CompressedPathRaw::new(
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .path_slice(),
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .end_mask(),
                                );
                                // COW modify child,
                                child_node_cow = CowNodeRef::new(
                                    child_node_ref,
                                    self.owned_node_set.get_ref(),
                                );
                                drop(trie_node_mut);
                                new_path = node_memory_manager
                                    .node_as_ref(
                                        &allocator,
                                        &child_node_cow.node_ref,
                                    )?
                                    .path_prepended(path_prefix, child_index);
                            }
                            let mut child_trie_node = node_memory_manager
                                .node_as_mut(
                                    &allocator,
                                    &mut child_node_cow.node_ref,
                                )?;
                            child_node_cow.cow_set_compressed_path(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                new_path,
                                &mut child_trie_node,
                            )?;

                            // FIXME: how to represent that trie_node_mut is
                            // invalid after call to node_mut.delete_node?
                            // FIXME: trie_node_mut should be considered ref of
                            // node_mut.
                            node_cow.delete_node(self.node_memory_manager());

                            Ok((value, true, child_node_cow.into_child()))
                        }
                        TrieNodeAction::DeleteChildrenTable => {
                            let node_changed = !node_cow.owned;
                            node_cow.cow_delete_children_table(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                &mut trie_node_mut,
                            )?;

                            Ok((value, node_changed, node_cow.into_child()))
                        }
                        TrieNodeAction::Modify => unsafe {
                            let node_changed = !node_cow.owned;
                            match new_child_node {
                                None => {
                                    node_cow.cow_delete_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                    )?;
                                }
                                Some(replacement) => {
                                    node_cow.cow_replace_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                        replacement,
                                    )?;
                                }
                            }

                            Ok((value, node_changed, node_cow.into_child()))
                        },
                        _ => unsafe { unreachable_unchecked() },
                    }
                } else {
                    Ok((value, false, node_cow.into_child()))
                }
            }

            _ => Ok((None, false, node_cow.into_child())),
        }
    }

    /// The visitor can only be used once to modify.
    /// Returns (deleted value, is root node replaced, the current root node for
    /// the subtree).
    pub fn delete_all(
        mut self, key: KeyPart, key_remaining: KeyPart,
    ) -> Result<(
        Option<Vec<(Vec<u8>, Box<[u8]>)>>,
        bool,
        Option<NodeRefDeltaMptCompact>,
    )> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = self.root.steal();
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        // FIXME: map_split?
        let trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;

        let key_prefix: CompressedPathRaw;
        match trie_node_mut.walk::<Read>(key_remaining) {
            WalkStop::ChildNotFound {
                key_remaining,
                child_index,
            } => return Ok((None, false, node_cow.into_child())),
            WalkStop::Arrived => {
                // To enumerate the subtree.
                key_prefix = key.into();
            }
            WalkStop::PathDiverted {
                key_child_index,
                key_remaining,
                matched_path,
                unmatched_child_index,
                unmatched_path_remaining,
            } => {
                if key_child_index.is_none() {
                    return Ok((None, false, node_cow.into_child()));
                }
                // To enumerate the subtree.
                key_prefix =
                    CompressedPathRaw::concat(&key, &unmatched_path_remaining);
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                drop(trie_node_mut);
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .delete_all(key, key_remaining);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let mut trie_node_mut = node_memory_manager
                    .node_as_mut(&allocator, &mut node_cow.node_ref)?;
                let (value, child_replaced, new_child_node) = result.unwrap();
                // FIXME: copied from delete(). Try to reuse code?
                if child_replaced {
                    let action = trie_node_mut
                        .check_replace_or_delete_child_action(
                            child_index,
                            new_child_node,
                        );
                    match action {
                        TrieNodeAction::MergePath {
                            child_index,
                            child_node_ref,
                        } => {
                            let new_path: CompressedPathRaw;
                            let mut child_node_cow: CowNodeRef;
                            {
                                let path_prefix = CompressedPathRaw::new(
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .path_slice(),
                                    trie_node_mut
                                        .compressed_path_ref()
                                        .end_mask(),
                                );
                                // COW modify child,
                                child_node_cow = CowNodeRef::new(
                                    child_node_ref,
                                    self.owned_node_set.get_ref(),
                                );
                                drop(trie_node_mut);
                                new_path = node_memory_manager
                                    .node_as_ref(
                                        &allocator,
                                        &child_node_cow.node_ref,
                                    )?
                                    .path_prepended(path_prefix, child_index);
                            }
                            let mut child_trie_node = node_memory_manager
                                .node_as_mut(
                                    &allocator,
                                    &mut child_node_cow.node_ref,
                                )?;
                            child_node_cow.cow_set_compressed_path(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                new_path,
                                &mut child_trie_node,
                            )?;

                            // FIXME: how to represent that trie_node_mut is
                            // invalid after call to node_mut.delete_node?
                            // FIXME: trie_node_mut should be considered ref of
                            // node_mut.
                            node_cow.delete_node(self.node_memory_manager());

                            return Ok((
                                value,
                                true,
                                child_node_cow.into_child(),
                            ));
                        }
                        TrieNodeAction::DeleteChildrenTable => {
                            let node_changed = !node_cow.owned;
                            node_cow.cow_delete_children_table(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                &mut trie_node_mut,
                            )?;

                            return Ok((
                                value,
                                node_changed,
                                node_cow.into_child(),
                            ));
                        }
                        TrieNodeAction::Modify => unsafe {
                            let node_changed = !node_cow.owned;
                            match new_child_node {
                                None => {
                                    node_cow.cow_delete_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                    )?;
                                }
                                Some(replacement) => {
                                    node_cow.cow_replace_child_unchecked(
                                        &node_memory_manager,
                                        self.owned_node_set.get_mut(),
                                        &mut trie_node_mut,
                                        child_index,
                                        replacement,
                                    )?;
                                }
                            }

                            return Ok((
                                value,
                                node_changed,
                                node_cow.into_child(),
                            ));
                        },
                        _ => unsafe { unreachable_unchecked() },
                    }
                } else {
                    return Ok((value, false, node_cow.into_child()));
                }
            }
        }

        let mut old_values = vec![];
        node_cow.iterate_internal(
            self.owned_node_set.get_ref(),
            self.get_trie_ref(),
            &allocator,
            trie_node_mut,
            key_prefix,
            &mut old_values,
        )?;
        node_cow.delete_node(self.node_memory_manager());

        Ok((Some(old_values), true, None))
    }

    // In a method we visit node one or 2 times but borrow-checker prevent
    // holding and access other fields so it's visited multiple times.
    // FIXME: Check if we did something like this.
    // It's correct behavior if we first
    // access this node, recurse into children, then access it again, because
    // the accesses in subtree and other threads may in extreme cases evict
    // this node from cache.

    // Assume that the obtained TrieNode will be set valid value (non-empty)
    // later on.
    /// Insert a valid value into MPT.
    /// The visitor can only be used once to modify.
    unsafe fn insert_checked_value<'key>(
        mut self, key: KeyPart<'key>, value: &[u8],
    ) -> Result<(bool, NodeRefDeltaMptCompact)> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = self.root.steal();
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        let mut trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;
        match trie_node_mut.walk::<Write>(key) {
            WalkStop::Arrived => {
                let node_changed = !node_cow.owned;
                node_cow.cow_replace_value_valid(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    &mut trie_node_mut,
                    value,
                )?;

                Ok((node_changed, node_cow.into_child().unwrap()))
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                drop(trie_node_mut);
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .insert_checked_value(key_remaining, value);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let mut trie_node_mut = node_memory_manager
                    .node_as_mut(&allocator, &mut node_cow.node_ref)?;
                let (child_changed, new_child_node) = result.unwrap();

                if child_changed {
                    let node_changed = !node_cow.owned;
                    node_cow.cow_replace_child_unchecked(
                        &node_memory_manager,
                        self.owned_node_set.get_mut(),
                        &mut trie_node_mut,
                        child_index,
                        new_child_node,
                    )?;

                    Ok((node_changed, node_cow.into_child().unwrap()))
                } else {
                    Ok((false, node_cow.into_child().unwrap()))
                }
            }
            WalkStop::PathDiverted {
                key_child_index,
                key_remaining,
                matched_path,
                unmatched_child_index,
                unmatched_path_remaining,
            } => {
                // create a new node to replace self with compressed
                // path = matched_path, modify current
                // node with the remaining path, and attach it as child to the
                // replacement node create a new node for
                // insertion (if key_remaining is non-empty), set it to child,
                // with key_remaining.
                let (new_node_cow, new_node_entry) =
                    CowNodeRef::new_uninitialized_node(
                        &allocator,
                        self.owned_node_set.get_mut(),
                    )?;
                let mut new_node = TrieNode::default();
                // set compressed path.
                new_node.set_compressed_path(matched_path);

                node_cow.cow_set_compressed_path(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    unmatched_path_remaining,
                    &mut trie_node_mut,
                )?;

                // It's safe because we know that this is the first child.
                new_node.set_first_child_unchecked(
                    unmatched_child_index,
                    // It's safe to unwrap because we know that it's not none.
                    node_cow.into_child().unwrap(),
                );

                // TODO(yz): remove duplicated code.
                match key_child_index {
                    None => {
                        // Insert value at the current node
                        new_node.replace_value_valid(value);
                    }
                    Some(child_index) => {
                        // TODO(yz): Maybe create CowNodeRef on NULL then
                        // cow_set_value then set path.
                        let (child_node_cow, child_node_entry) =
                            CowNodeRef::new_uninitialized_node(
                                &allocator,
                                self.owned_node_set.get_mut(),
                            )?;
                        let mut new_child_node = TrieNode::default();
                        // set compressed path.
                        new_child_node.copy_compressed_path(
                            CompressedPathRef {
                                path_slice: key_remaining,
                                end_mask: 0,
                            },
                        );
                        new_child_node.replace_value_valid(value);
                        child_node_entry.insert(new_child_node);

                        // It's safe because it's guaranteed that
                        // key_child_index != unmatched_child_index
                        new_node.add_new_child_unchecked(
                            child_index,
                            // It's safe to unwrap here because it's not null
                            // node.
                            child_node_cow.into_child().unwrap(),
                        );
                    }
                }
                new_node_entry.insert(new_node);
                Ok((true, new_node_cow.into_child().unwrap()))
            }
            WalkStop::ChildNotFound {
                key_remaining,
                child_index,
            } => {
                // TODO(yz): Maybe create CowNodeRef on NULL then cow_set_value
                // then set path.
                let (child_node_cow, child_node_entry) =
                    CowNodeRef::new_uninitialized_node(
                        &allocator,
                        self.owned_node_set.get_mut(),
                    )?;
                let mut new_child_node = TrieNode::default();
                // set compressed path.
                new_child_node.copy_compressed_path(CompressedPathRef {
                    path_slice: key_remaining,
                    end_mask: 0,
                });
                new_child_node.replace_value_valid(value);
                child_node_entry.insert(new_child_node);

                let node_changed = !node_cow.owned;
                node_cow.cow_add_new_child_unchecked(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    &mut trie_node_mut,
                    child_index,
                    child_node_cow.into_child().unwrap(),
                )?;

                Ok((node_changed, node_cow.into_child().unwrap()))
            }
        }
    }

    pub fn set(self, key: KeyPart, value: &[u8]) -> Result<NodeRefDeltaMpt> {
        TrieNode::check_key_size(key)?;
        TrieNode::check_value_size(value)?;
        let new_root;
        unsafe {
            new_root = self.insert_checked_value(key, value)?.1;
        }
        Ok(new_root.into())
    }
}

impl Encodable for TrieNode {
    fn rlp_append(&self, s: &mut RlpStream) {
        // Format: [ merkle, children_table ([] or [*16], value (maybe empty) ]
        // ( + [compressed_path] )
        s.begin_unbounded_list()
            .append(&self.merkle_hash)
            .append(&self.children_table.to_ref())
            .append(&self.value_as_slice());

        let compressed_path_ref = self.compressed_path_ref();
        if compressed_path_ref.path_slice.len() > 0 {
            s.append(&compressed_path_ref);
        }

        s.complete_unbounded_list();
    }
}

impl Decodable for TrieNode {
    fn decode(rlp: &Rlp) -> ::std::result::Result<Self, DecoderError> {
        let compressed_path;
        if rlp.item_count()? != 4 {
            compressed_path = CompressedPathRaw::new(&[], 0);
        } else {
            compressed_path = rlp.val_at(3)?;
        }

        Ok(TrieNode::new(
            &rlp.val_at::<Vec<u8>>(0)?.as_slice().into(),
            rlp.val_at::<ChildrenTableManagedDeltaMpt>(1)?.into(),
            rlp.val_at::<Option<Vec<u8>>>(2)?,
            compressed_path,
        ))
    }
}

impl<'a> CompressedPathRef<'a> {
    // TODO(yz): the format can be optimized.
    pub fn rlp_append_parts(&self, s: &mut RlpStream) {
        s.append(&self.end_mask).append(&self.path_slice);
    }
}

impl<'a> Encodable for CompressedPathRef<'a> {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(2);
        self.rlp_append_parts(s);
    }
}

impl Decodable for CompressedPathRaw {
    // TODO(yz): the format can be optimized.
    fn decode(rlp: &Rlp) -> ::std::result::Result<Self, DecoderError> {
        Ok(CompressedPathRaw::new(
            rlp.val_at::<Vec<u8>>(1)?.as_slice(),
            rlp.val_at(0)?,
        ))
    }
}

impl PartialEq for TrieNode {
    fn eq(&self, other: &Self) -> bool {
        self.value_as_slice() == other.value_as_slice()
            && self.children_table == other.children_table
            && self.merkle_hash == other.merkle_hash
            && self.compressed_path_ref() == other.compressed_path_ref()
    }
}

impl Debug for TrieNode {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "TrieNode{{ merkle: {:?}, value: {:?}, children_table: {:?}, compressed_path: {:?} }}",
               self.merkle_hash, self.value_as_slice(),
               &self.children_table, self.compressed_path_ref())
    }
}
