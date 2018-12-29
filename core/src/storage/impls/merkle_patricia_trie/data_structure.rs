use self::access_mode::*;
use super::{
    super::{
        super::super::db::COL_DELTA_TRIE, errors::*,
        state_manager::AtomicCommitTransaction,
    },
    cache::algorithm::lfru::LFRUHandle,
    maybe_in_place_byte_array::MaybeInPlaceByteArray,
    merkle::*,
    node_memory_manager::*,
    node_ref_map::NodeRefDeltaMPT,
    return_after_use::ReturnAfterUse,
};
use rlp::*;
use std::{
    cmp::min,
    collections::BTreeSet,
    hint::unreachable_unchecked,
    marker::{Send, Sync},
    mem::replace,
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
    children_table: OwnedChildrenTable,
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

pub const CHILDREN_COUNT: usize = 16;
/// The children table for a non-leaf trie node.
pub type ChildrenTable = [MaybeNodeRef; CHILDREN_COUNT];

// TODO(yz): 2^16 = 2B. 2B + actual children to save space.
/// For delta MPT the index type is DeltaMptDbKey, for persistent MPT see
/// node_ref_map.rs (not implemented yet).
struct CompactedChildrenTable<IndexT> {
    table: Box<[IndexT]>,
}

type NodeMemoryRegion = Vec<TrieNode>;

// FIXME: implement EntryTrait for TrieNode.
pub type TrieNodeSlabEntry = super::slab::Entry<TrieNode>;
pub type VacantEntry<'a> =
    super::slab::VacantEntry<'a, TrieNode, TrieNodeSlabEntry>;

pub type OwnedChildrenTable = Option<Box<ChildrenTable>>;

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

    fn value_as_slice(&self) -> &[u8] {
        let size = self.value_size as usize;
        if size != 0 {
            self.value.get_slice(size)
        } else {
            &[]
        }
    }

    fn value_clone(&self) -> Option<Box<[u8]>> {
        let size = self.value_size as usize;
        if size != 0 {
            Option::Some(self.value.get_slice(size).into())
        } else {
            Option::None
        }
    }

    /// Take value out of self.
    /// This method can only be called by replace_value / delete_value because
    /// empty node must be removed and pass compression must be maintained.
    // FIXME: hide this method
    fn value_into_boxed_slice(&mut self) -> Option<Box<[u8]>> {
        let size = self.value_size as usize;
        let maybe_value;
        if size != 0 {
            maybe_value = Some(self.value.into_boxed_slice(size));
            self.value_size = 0;
        } else {
            maybe_value = None;
        }
        maybe_value
    }

    fn replace_value_valid(&mut self, valid_value: &[u8]) -> Option<Box<[u8]>> {
        let old_value = self.value_into_boxed_slice();
        if old_value.is_none() {
            self.number_of_children_plus_value += 1;
        }
        let value_size = valid_value.len();
        self.value_size = value_size as u32;
        self.value = MaybeInPlaceByteArray::copy_from(valid_value, value_size);

        old_value
    }

    fn check_value_size(value: &[u8]) -> Result<()> {
        let value_size = value.len();
        if value_size > MaybeInPlaceByteArray::MAX_SIZE_U32 {
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

impl CompressedPathRaw {
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
        child_node: NodeRef,
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

                        // TODO(yz): move shift operator and magic number into a
                        // better place.
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

            match Option::<NodeRef>::from(self.get_child(child_index)) {
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
                        child_node: child_node,
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
        child_node_ref: NodeRef,
    },
    DeleteChildrenTable,
}

/// Update
impl TrieNode {
    pub fn new(
        merkle: &MerkleHash, children_table: OwnedChildrenTable, value: &[u8],
        compressed_path: CompressedPathRaw,
    ) -> TrieNode
    {
        let mut ret = TrieNode::default();

        ret.merkle_hash = *merkle;

        ret.number_of_children_plus_value = match children_table.as_ref() {
            None => 0,
            Some(ref table) => {
                let mut count = 0;
                for node in table.as_ref() {
                    if *node != MaybeNodeRef::NULL_NODE {
                        count += 1;
                    }
                }
                count
            }
        };

        ret.children_table = children_table;
        ret.replace_value_valid(value);
        ret.set_compressed_path(compressed_path);

        ret
    }

    /// new_value can only be set according to the situation.
    /// children_table can only be replaced when there is no children in both
    /// old and new table.
    ///
    /// unsafe because:
    /// 1. precondition on children_table;
    /// 2. number_of_children_plus_value is only incrementally calculated.
    /// 3. delete value assumes that self contains some value.
    unsafe fn copy_and_replace_fields(
        &self, new_value: Option<Option<&[u8]>>,
        new_path: Option<CompressedPathRaw>,
        children_table: Option<OwnedChildrenTable>,
    ) -> TrieNode
    {
        let mut ret = TrieNode::default();
        ret.number_of_children_plus_value = self.number_of_children_plus_value;

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
        &self, prefix: CompressedPathRef, child_index: u8,
    ) -> CompressedPathRaw {
        let prefix_size = prefix.path_slice.len();
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
            if prefix.end_mask == 0 {
                slice[0..prefix_size].copy_from_slice(prefix.path_slice);
                slice[prefix_size..].copy_from_slice(path.path_slice);
            } else {
                slice[0..prefix_size - 1]
                    .copy_from_slice(&prefix.path_slice[0..prefix_size - 1]);
                slice[prefix_size - 1] = Self::set_second_nibble(
                    prefix.path_slice[prefix_size - 1],
                    child_index,
                );
                slice[prefix_size..].copy_from_slice(path.path_slice);
            }
        }

        new_path
    }

    /// Delete value when we know that it already exists.
    unsafe fn delete_value_unchecked(&mut self) -> Box<[u8]> {
        let ret = self.value_into_boxed_slice().unwrap();

        self.number_of_children_plus_value -= 1;

        ret
    }

    /// Returns: old_value, is_self_about_to_delete, replacement_node_for_self
    fn check_delete_value(&self) -> Result<TrieNodeAction> {
        if self.has_value() {
            let number_of_children_plus_value =
                self.number_of_children_plus_value - 1;
            Ok(match number_of_children_plus_value {
                0 => TrieNodeAction::Delete,
                1 => self.merge_path_action(),
                _ => TrieNodeAction::Modify,
            })
        } else {
            Err(ErrorKind::MPTKeyNotFound.into())
        }
    }

    fn get_children_table_unchecked(&self) -> &ChildrenTable {
        self.children_table.as_ref().unwrap().as_ref()
    }

    fn get_children_table_unchecked_mut(&mut self) -> &mut ChildrenTable {
        self.children_table.as_mut().unwrap().as_mut()
    }

    fn merge_path_action(&self) -> TrieNodeAction {
        let children_table_ref = self.get_children_table_unchecked();
        for i in 0..CHILDREN_COUNT {
            if children_table_ref[i] != MaybeNodeRef::NULL_NODE {
                return TrieNodeAction::MergePath {
                    child_index: i as u8,
                    child_node_ref: Option::<NodeRef>::from(
                        children_table_ref[i],
                    )
                    .unwrap(),
                };
            }
        }
        unsafe { unreachable_unchecked() }
    }

    fn merge_path_action_after_child_deletion(
        &self, child_index: u8,
    ) -> TrieNodeAction {
        let children_table_ref = self.get_children_table_unchecked();
        for i in 0..CHILDREN_COUNT {
            if i != child_index as usize
                && children_table_ref[i] != MaybeNodeRef::NULL_NODE
            {
                return TrieNodeAction::MergePath {
                    child_index: i as u8,
                    child_node_ref: Option::<NodeRef>::from(
                        children_table_ref[i],
                    )
                    .unwrap(),
                };
            }
        }
        unsafe { unreachable_unchecked() }
    }

    // Children table.
    fn get_child(&self, child_index: u8) -> MaybeNodeRef {
        return self
            .children_table
            .as_ref()
            .map_or(MaybeNodeRef::NULL_NODE, |table| {
                table[child_index as usize]
            });
    }

    /// set_child doesn't need to deal with resource management, because the
    /// replaced child is either NULL_NODE, or whose ownership are stolen in
    /// Trie operation.
    fn set_child(&mut self, child_index: u8, new_child_node: MaybeNodeRef) {
        let mut delta = 0;
        let old_child: MaybeNodeRef;
        if new_child_node != MaybeNodeRef::NULL_NODE {
            delta += 1;
        }
        old_child = replace(
            &mut self
                .children_table
                .get_or_insert_with(|| Default::default())
                [child_index as usize],
            new_child_node,
        );
        if old_child != MaybeNodeRef::NULL_NODE {
            delta -= 1;
        }
        self.number_of_children_plus_value =
            (self.number_of_children_plus_value as i16 + delta) as u8;
    }

    /// Returns old_child, is_self_about_to_delete, replacement_node_for_self
    fn check_replace_child(
        &self, child_index: u8, new_child_node: MaybeNodeRef,
    ) -> TrieNodeAction {
        let mut delta = 0;
        let old_child: MaybeNodeRef;
        if new_child_node != MaybeNodeRef::NULL_NODE {
            delta += 1;
        }
        if self.get_child(child_index) != MaybeNodeRef::NULL_NODE {
            delta -= 1;
        }
        if delta == -1 {
            match self.number_of_children_plus_value {
                count @ 1...2 => {
                    // It's not possible for non-root node to have 0 child 0
                    // value after delete a child, because
                    // the previous state must be 1 child 0
                    // value, which can not exist.
                    if count == 1 || self.has_value() {
                        // There is no children.
                        TrieNodeAction::DeleteChildrenTable
                    } else {
                        self.merge_path_action_after_child_deletion(child_index)
                    }
                }
                _ => TrieNodeAction::Modify,
            }
        } else {
            TrieNodeAction::Modify
        }
    }
}

// TODO(yz): move to file for merkle_patricia_trie.
use super::MultiVersionMerklePatriciaTrie;

// If deleted, when the SubTrieVisitor goes out of scope, the node is deleted.
// There is no need of allocator for deletion.
// Allocation can happen as creation of this object.
// OwnedChecker is only required when creating the CowNodeRef.
pub struct CowNodeRef {
    // TODO(yz): if moved no deletion happens when SubTrieVisitor goes out of
    // scope. Maybe this is not the best way but we will see. TODO(yz): if
    // this is not the best way maybe we keep a reference to allocator to make
    // sure that it definitely result into a deletion if not used.
    moved: bool,
    owned: bool,
    pub node_ref: NodeRef,
}

impl Default for CowNodeRef {
    fn default() -> Self {
        Self {
            moved: false,
            owned: false,
            node_ref: NodeRef::Dirty {
                index: NodeRefDeltaMPT::NULL_SLOT,
            },
        }
    }
}

pub type OwnedNodeSet = BTreeSet<NodeRef>;

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

    pub fn new(node_ref: NodeRef, owned_node_set: &OwnedNodeSet) -> Self {
        Self {
            owned: owned_node_set.contains(&node_ref),
            moved: false,
            node_ref: node_ref,
        }
    }

    fn into_owned<'a>(
        &mut self, node_memory_manager: &NodeMemoryManager,
        allocator: AllocatorRefRef<'a>, owned_node_set: &mut OwnedNodeSet,
    ) -> Result<Option<(&'a TrieNode, VacantEntry<'a>)>>
    {
        if self.owned {
            Ok(None)
        } else {
            let old_node =
                node_memory_manager.node_as_ref(allocator, &self.node_ref)?;

            // TODO(yz): maybe use Self::new_uninitialized_node().
            let (node_ref, new_entry) =
                NodeMemoryManager::new_node(&allocator)?;
            owned_node_set.insert(node_ref.clone());
            self.node_ref = node_ref;

            Ok(Some((old_node, new_entry)))
        }
    }

    pub fn delete_node(&mut self, node_memory_manager: &NodeMemoryManager) {
        if self.owned {
            node_memory_manager.free_node(&mut self.node_ref);
            self.owned = false;
        }
        self.moved = true;
    }

    pub fn into_child(&mut self) -> MaybeNodeRef {
        if !self.moved {
            self.moved = true;
            self.node_ref.clone().into()
        } else {
            MaybeNodeRef::NULL_NODE
        }
    }

    fn commit_children(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        commit_transaction: &mut AtomicCommitTransaction,
        cache_manager: &mut CacheManager,
    ) -> Result<()>
    {
        match trie_node.children_table {
            None => {}
            Some(ref mut table) => {
                let allocator = trie.get_node_memory_manager().get_allocator();

                let merkles = ChildrenMerkleTable::default();
                for i in 0..CHILDREN_COUNT {
                    let maybe_child_node: Option<NodeRef> = table[i].into();
                    match maybe_child_node {
                        None => {}
                        Some(node_ref) => {
                            let mut cow_child_node =
                                Self::new(node_ref, owned_node_set);
                            let trie_node = trie
                                .get_node_memory_manager()
                                .node_as_mut_with_cache_manager(
                                    &allocator,
                                    &mut cow_child_node.node_ref,
                                    cache_manager,
                                )?;
                            let was_owned = cow_child_node.commit(
                                trie,
                                owned_node_set,
                                trie_node,
                                commit_transaction,
                                cache_manager,
                            )?;

                            // A owned child TrieNode now have a new NodeRef.
                            // Unowned child TrieNode isn't changed so it's safe
                            // to not reset children
                            // table in parent node.
                            let new_node_ref = cow_child_node.into_child();
                            if was_owned {
                                table[i] = new_node_ref;
                            }
                        }
                    }
                }
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
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<MerkleHash>
    {
        if self.owned {
            let children_merkles = self.get_or_compute_children_merkles(
                trie,
                owned_node_set,
                trie_node,
            )?;

            let merkle = self.set_merkle(children_merkles.as_ref(), trie_node);

            Ok(merkle)
        } else {
            Ok(trie_node.merkle_hash)
        }
    }

    fn get_or_compute_children_merkles(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<MaybeMerkleTable>
    {
        match trie_node.children_table {
            None => Ok(None),
            Some(ref mut table) => {
                let allocator = trie.get_node_memory_manager().get_allocator();

                let mut merkles = ChildrenMerkleTable::default();
                for i in 0..CHILDREN_COUNT {
                    let maybe_child_node: Option<NodeRef> = table[i].into();
                    merkles[i] = match maybe_child_node {
                        None => super::merkle::MERKLE_NULL_NODE,
                        Some(node_ref) => {
                            let mut cow_child_node =
                                Self::new(node_ref, owned_node_set);
                            let trie_node =
                                trie.get_node_memory_manager().node_as_mut(
                                    &allocator,
                                    &mut cow_child_node.node_ref,
                                )?;
                            let merkle = cow_child_node.get_or_compute_merkle(
                                trie,
                                owned_node_set,
                                trie_node,
                            )?;
                            // There is no change to the child_node so the
                            // return value is dropped.
                            cow_child_node.into_child();

                            merkle
                        }
                    }
                }
                Ok(Some(merkles))
            }
        }
    }

    /// Recursively commit dirty nodes.
    pub fn commit(
        &mut self, trie: &MultiVersionMerklePatriciaTrie,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        commit_transaction: &mut AtomicCommitTransaction,
        cache_manager: &mut CacheManager,
    ) -> Result<bool>
    {
        if self.owned {
            self.commit_children(
                trie,
                owned_node_set,
                trie_node,
                commit_transaction,
                cache_manager,
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
                NodeRef::Dirty { index } => *index,
                _ => unsafe { unreachable_unchecked() },
            };
            owned_node_set.remove(&self.node_ref);
            self.node_ref = NodeRef::Committed { db_key: db_key };
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
        let copied =
            self.into_owned(node_memory_manager, &allocator, owned_node_set)?;
        match copied {
            Some((old, new_entry)) => {
                new_entry.insert(unsafe {
                    old.copy_and_replace_fields(None, Some(path), None)
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
        let copied =
            self.into_owned(node_memory_manager, &allocator, owned_node_set)?;
        Ok(match copied {
            None => unsafe { trie_node.delete_value_unchecked() },
            Some((old, new_entry)) => {
                new_entry.insert(unsafe {
                    old.copy_and_replace_fields(Some(None), None, None)
                });
                old.value_clone().unwrap()
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
        let copied =
            self.into_owned(node_memory_manager, &allocator, owned_node_set)?;
        Ok(match copied {
            None => trie_node.replace_value_valid(value),
            Some((old, new_entry)) => {
                new_entry.insert(unsafe {
                    old.copy_and_replace_fields(Some(Some(value)), None, None)
                });
                old.value_clone()
            }
        })
    }

    fn cow_delete_children_table(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied =
            self.into_owned(node_memory_manager, &allocator, owned_node_set)?;
        match copied {
            None => {
                trie_node.children_table = None;
            }
            Some((old, new_entry)) => {
                new_entry.insert(unsafe {
                    old.copy_and_replace_fields(None, None, Some(None))
                });
            }
        }
        Ok(())
    }

    fn cow_replace_child(
        &mut self, node_memory_manager: &NodeMemoryManager,
        owned_node_set: &mut OwnedNodeSet, trie_node: &mut TrieNode,
        child_index: u8, child_node: MaybeNodeRef,
    ) -> Result<()>
    {
        let allocator = node_memory_manager.get_allocator();
        let copied =
            self.into_owned(node_memory_manager, &allocator, owned_node_set)?;
        match copied {
            None => {
                trie_node.set_child(child_index, child_node);
            }
            Some((old, new_entry)) => {
                let mut new_trie_node =
                    unsafe { old.copy_and_replace_fields(None, None, None) };
                new_trie_node.set_child(child_index, child_node);
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
    trie_ref: &'trie MultiVersionMerklePatriciaTrie,
    root: CowNodeRef,

    /// We use ReturnAfterUse because only one SubTrieVisitor(the deepest) can
    /// hold the mutable reference of owned_node_set.
    owned_node_set: ReturnAfterUse<'trie, OwnedNodeSet>,
}

impl<'trie> SubTrieVisitor<'trie> {
    pub fn new(
        trie_ref: &'trie MultiVersionMerklePatriciaTrie, root: NodeRef,
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
        &'a mut self, child_node: NodeRef,
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
    ) -> Result<Option<&'a TrieNode>> {
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
                    let merkles = match trie_node.children_table {
                        None => None,
                        Some(ref table) => {
                            let mut merkles = ChildrenMerkleTable::default();
                            for i in 0..CHILDREN_COUNT {
                                merkles[i] =
                                    match self.trie_ref.get_merkle(table[i])? {
                                        None => super::merkle::MERKLE_NULL_NODE,
                                        Some(merkle) => merkle,
                                    };
                            }

                            Some(merkles)
                        }
                    };

                    Ok(Some(compute_node_merkle(
                        merkles.as_ref(),
                        trie_node.value_as_slice(),
                    )))
                }
            }
        }
    }

    /// The visitor can only be used once to modify.
    /// Returns (deleted value, is root node replaced, the current root node for
    /// the subtree).
    pub fn delete(
        &mut self, key: KeyPart,
    ) -> Result<(Option<Box<[u8]>>, bool, MaybeNodeRef)> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = replace(&mut self.root, Default::default());
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        // FIXME: map_split?
        let trie_node_mut = node_memory_manager
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
                            node_cow
                                .delete_value_unchecked_if_owned(trie_node_mut)
                        };
                        // FIXME: deal with deletion while holding the
                        // trie_node_mut.
                        node_cow.delete_node(self.node_memory_manager());
                        // FIXME: maybe unify NULL_NODE into
                        // node_cow.into_child()?
                        Ok((Some(value), true, MaybeNodeRef::NULL_NODE))
                    }
                    TrieNodeAction::MergePath {
                        child_index,
                        child_node_ref,
                    } => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow
                                .delete_value_unchecked_if_owned(trie_node_mut)
                        };

                        let new_path: CompressedPathRaw;
                        let mut child_node_cow: CowNodeRef;
                        {
                            let path_prefix =
                                trie_node_mut.compressed_path_ref();
                            // COW modify child,
                            child_node_cow = CowNodeRef::new(
                                child_node_ref,
                                self.owned_node_set.get_ref(),
                            );
                            new_path = node_memory_manager
                                .node_as_ref(
                                    &allocator,
                                    &child_node_cow.node_ref,
                                )?
                                .path_prepended(path_prefix, child_index);
                        }
                        // FIXME: error processing for OOM.
                        let child_trie_node = node_memory_manager.node_as_mut(
                            &allocator,
                            &mut child_node_cow.node_ref,
                        )?;
                        child_node_cow.cow_set_compressed_path(
                            &node_memory_manager,
                            self.owned_node_set.get_mut(),
                            new_path,
                            child_trie_node,
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
                            trie_node_mut,
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
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .delete(key_remaining);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let (value, child_replaced, new_child_node) = result.unwrap();
                if child_replaced {
                    let action = trie_node_mut
                        .check_replace_child(child_index, new_child_node);
                    match action {
                        TrieNodeAction::MergePath {
                            child_index,
                            child_node_ref,
                        } => {
                            // FIXME: how to reuse code?
                            let new_path: CompressedPathRaw;
                            let mut child_node_cow: CowNodeRef;
                            {
                                let path_prefix =
                                    trie_node_mut.compressed_path_ref();
                                // COW modify child,
                                child_node_cow = CowNodeRef::new(
                                    child_node_ref,
                                    self.owned_node_set.get_ref(),
                                );
                                new_path = node_memory_manager
                                    .node_as_ref(
                                        &allocator,
                                        &child_node_cow.node_ref,
                                    )?
                                    .path_prepended(path_prefix, child_index);
                            }
                            let child_trie_node = node_memory_manager
                                .node_as_mut(
                                    &allocator,
                                    &mut child_node_cow.node_ref,
                                )?;
                            child_node_cow.cow_set_compressed_path(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                new_path,
                                child_trie_node,
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
                                trie_node_mut,
                            )?;

                            Ok((value, node_changed, node_cow.into_child()))
                        }
                        TrieNodeAction::Modify => {
                            let node_changed = !node_cow.owned;
                            node_cow.cow_replace_child(
                                &node_memory_manager,
                                self.owned_node_set.get_mut(),
                                trie_node_mut,
                                child_index,
                                new_child_node,
                            )?;

                            Ok((value, node_changed, node_cow.into_child()))
                        }
                        _ => unsafe { unreachable_unchecked() },
                    }
                } else {
                    Ok((value, false, node_cow.into_child()))
                }
            }

            _ => Ok((None, false, node_cow.into_child())),
        }
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
    ) -> Result<(bool, MaybeNodeRef)> {
        let node_memory_manager = self.node_memory_manager();
        let allocator = node_memory_manager.get_allocator();
        let mut node_cow = replace(&mut self.root, Default::default());
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        let trie_node_mut = node_memory_manager
            .node_as_mut(&allocator, &mut node_cow.node_ref)?;
        match trie_node_mut.walk::<Write>(key) {
            WalkStop::Arrived => {
                let node_changed = !node_cow.owned;
                node_cow.cow_replace_value_valid(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    trie_node_mut,
                    value,
                )?;

                Ok((node_changed, node_cow.into_child()))
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                let result = self
                    .new_visitor_for_subtree(child_node)
                    .insert_checked_value(key_remaining, value);
                if result.is_err() {
                    node_cow.into_child();
                    return result;
                }
                let (child_changed, new_child_node) = result.unwrap();

                // No node deletion can happen
                if child_changed {
                    let node_changed = !node_cow.owned;
                    node_cow.cow_replace_child(
                        &node_memory_manager,
                        self.owned_node_set.get_mut(),
                        trie_node_mut,
                        child_index,
                        new_child_node,
                    )?;

                    Ok((node_changed, node_cow.into_child()))
                } else {
                    Ok((false, node_cow.into_child()))
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
                let (mut new_node_cow, new_node_entry) =
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
                    trie_node_mut,
                )?;

                new_node
                    .set_child(unmatched_child_index, node_cow.into_child());

                // TODO(yz): remove duplicated code.
                match key_child_index {
                    None => {
                        // Insert value at the current node
                        new_node.replace_value_valid(value);
                    }
                    Some(child_index) => {
                        // TODO(yz): Maybe create CowNodeRef on NULL then
                        // cow_set_value then set path.
                        let (mut child_node_cow, child_node_entry) =
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

                        new_node.set_child(
                            child_index,
                            child_node_cow.into_child(),
                        );
                    }
                }
                new_node_entry.insert(new_node);
                Ok((true, new_node_cow.into_child()))
            }
            WalkStop::ChildNotFound {
                key_remaining,
                child_index,
            } => {
                // TODO(yz): Maybe create CowNodeRef on NULL then cow_set_value
                // then set path.
                let (mut child_node_cow, child_node_entry) =
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
                node_cow.cow_replace_child(
                    &node_memory_manager,
                    self.owned_node_set.get_mut(),
                    trie_node_mut,
                    child_index,
                    child_node_cow.into_child(),
                )?;

                Ok((node_changed, node_cow.into_child()))
            }
        }
    }

    pub fn set(self, key: KeyPart, value: &[u8]) -> Result<MaybeNodeRef> {
        TrieNode::check_key_size(key)?;
        TrieNode::check_value_size(value)?;
        let new_root: MaybeNodeRef;
        unsafe {
            new_root = self.insert_checked_value(key, value)?.1;
        }
        Ok(new_root)
    }
}

impl Encodable for TrieNode {
    fn rlp_append(&self, s: &mut RlpStream) {
        // Format: [ merkle, children_table ([] or [*16], value (maybe empty) ]
        // ( + [compressed_path] )
        s.begin_unbounded_list()
            .append(&self.merkle_hash)
            .append(&OwnedChildrenTableRef {
                children_table_ref: &self.children_table,
            })
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
            rlp.val_at::<OwnedChildrenTableWrapper>(1)?
                .owned_children_table,
            rlp.val_at::<Vec<u8>>(2)?.as_slice(),
            compressed_path,
        ))
    }
}

pub struct OwnedChildrenTableWrapper {
    pub owned_children_table: OwnedChildrenTable,
}

pub struct OwnedChildrenTableRef<'a> {
    pub children_table_ref: &'a OwnedChildrenTable,
}

impl<'a> Encodable for OwnedChildrenTableRef<'a> {
    fn rlp_append(&self, s: &mut RlpStream) {
        match self.children_table_ref {
            Some(ref owned_children_table) => {
                s.append_list(owned_children_table.as_ref().as_ref());
            }
            None => {
                s.begin_list(0);
            }
        }
    }
}

impl Decodable for OwnedChildrenTableWrapper {
    fn decode(rlp: &Rlp) -> ::std::result::Result<Self, DecoderError> {
        Ok(OwnedChildrenTableWrapper {
            owned_children_table: if rlp.item_count()? > 1 {
                let mut children_table = Box::<ChildrenTable>::default();
                let parsed_children_table = rlp.as_list()?;
                // Prevent copy_from_slice from asserting.
                if parsed_children_table.len() != CHILDREN_COUNT {
                    return Err(DecoderError::RlpIncorrectListLen);
                }
                children_table
                    .copy_from_slice(parsed_children_table.as_slice());
                Some(children_table)
            } else {
                None
            },
        })
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
