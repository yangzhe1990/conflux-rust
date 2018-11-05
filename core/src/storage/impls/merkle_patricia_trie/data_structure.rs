use self::access_mode::*;
use super::{
    super::errors::*,
    merkle::*,
    return_after_use::ReturnAfterUse,
    slab::{EntryTrait, Slab},
};
use core::slice;
use std::{
    cmp::{min, Ord},
    collections::BTreeSet,
    marker::{Send, Sync},
    mem::{replace, swap},
    ptr::null_mut,
    sync::{RwLock, RwLockReadGuard},
    vec::Vec,
};

impl NodeMemoryAllocator {
    /// In disk hybrid solution, the nodes in memory are merely LRU cache of
    /// non-leaf nodes. So the memory consumption is 128B * number of nodes.
    /// 5GB ~ 31_250_000 nodes. There are costs to maintain references to disk
    /// and cache recency... so TODO(yz): maybe
    pub const MAX_IN_MEM_TRIE_NODES_DISK_HYBRID: u32 = 26_000_000;
    /// If we do not swap out any node onto disk, the maximum tolerable nodes is
    /// about 43.2M, where there is about 7.2M leaf node. The total memory
    /// consumption is about 43.2 * 128 - 7.2 * 64 MB ~= 5GB. It can hold new
    /// items for about 1 hour assuming 2k updates per second.
    pub const MAX_IN_MEM_TRIE_NODES_MEM_ONLY: u32 = 43_200_000;
    pub const START_CAPACITY: u32 = 1_000_000;
}

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

// TODO(yz): statically check the size to be 64B
// TODO(yz): TrieNode should leave one byte so that it can be used to indicate a
// free slot in memory region.
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
    value_size: u16,
    value: MaybeInPlaceByteArray,
    pub merkle_hash: MerkleHash,
}

/// Compiler is not sure about the pointer in MaybeInPlaceByteArray fields.
/// It's Send because TrieNode is move only and it's impossible to change any
/// part of it without &mut.
unsafe impl Send for TrieNode {}
/// Compiler is not sure about the pointer in MaybeInPlaceByteArray fields.
/// We do not allow a &TrieNode to be able to change anything the pointer
/// is pointing to, therefore TrieNode is Sync.
unsafe impl Sync for TrieNode {}

union MaybeInPlaceByteArray {
    in_place: [u8; 8],
    // TODO(yz): statically assert that the ptr has size of at most 8.
    // TODO(yz): to initialize and destruct, convert from/into Vec.
    // TODO(yz): introduce a type to pass into template which manages the
    // conversion from/to  Vec<A>. The type should also take the offset of
    // u16 size from the TrieNode struct to manage memory buffer, and
    // should offer a type for ptr here.
    /// Only raw pointer is 8B.
    ptr: *mut u8,
}

impl Default for MaybeInPlaceByteArray {
    fn default() -> Self {
        Self {
            in_place: Default::default(),
        }
    }
}

impl MaybeInPlaceByteArray {
    /// Take ptr out and clear ptr.
    // FIXME: hide this method.
    unsafe fn ptr_into_vec(&mut self, size: usize) -> Vec<u8> {
        let vec = Vec::from_raw_parts(self.ptr, size, size);
        self.ptr = null_mut();
        vec
    }

    // FIXME: hide this method.
    unsafe fn ptr_slice(&self, size: usize) -> &[u8] {
        slice::from_raw_parts(self.ptr, size)
    }

    unsafe fn ptr_slice_mut(&mut self, size: usize) -> &mut [u8] {
        slice::from_raw_parts_mut(self.ptr, size)
    }

    fn get_slice_mut(&mut self, size: u16) -> &mut [u8] {
        let is_ptr = size > Self::MAX_INPLACE_SIZE;
        let size = size as usize;
        unsafe {
            if is_ptr {
                self.ptr_slice_mut(size)
            } else {
                &mut self.in_place[0..size]
            }
        }
    }

    fn get_slice(&self, size: u16) -> &[u8] {
        let is_ptr = size > Self::MAX_INPLACE_SIZE;
        let size = size as usize;
        unsafe {
            if is_ptr {
                self.ptr_slice(size)
            } else {
                &self.in_place[0..size]
            }
        }
    }

    fn into_vec(&mut self, size: u16) -> Vec<u8> {
        let is_ptr = size > Self::MAX_INPLACE_SIZE;
        let size = size as usize;
        unsafe {
            if is_ptr {
                self.ptr_into_vec(size)
            } else {
                Vec::from(&self.in_place[0..size])
            }
        }
    }

    /// The @size: u16 parameter reminds the caller to check if the vector is
    /// over sized.
    fn new(value: Box<[u8]>, size: u16) -> Self {
        if size > Self::MAX_INPLACE_SIZE {
            Self {
                ptr: Box::into_raw(value) as *mut u8,
            }
        } else {
            let mut x = Self {
                in_place: Default::default(),
            };
            unsafe {
                x.in_place[0..size.into()].copy_from_slice(&*value);
            }
            x
        }
    }

    fn new_uninitialized(size: u16) -> Self {
        Self::new(vec![0u8; size as usize].into_boxed_slice(), size)
    }

    fn copy_from(value: &[u8], size: u16) -> Self {
        if size > Self::MAX_INPLACE_SIZE {
            Self {
                ptr: Box::into_raw(Box::<[u8]>::from(value)) as *mut u8,
            }
        } else {
            let mut x: Self = Self {
                in_place: Default::default(),
            };
            unsafe {
                x.in_place[0..size.into()].copy_from_slice(value);
            }
            x
        }
    }
}

impl MaybeInPlaceByteArray {
    const MAX_INPLACE_SIZE: u16 = 8;
    const MAX_SIZE: u16 = 0xffff;
}

pub const CHILDREN_COUNT: usize = 16;
// TODO(yz): statically check the size to be 64B
/// The children table for a non-leaf trie node.
type ChildrenTable = [MaybeNodeRef; CHILDREN_COUNT];

type NodeMemoryRegion = Vec<TrieNode>;

// FIXME: implement EntryTrait.
type TrieNodeSlabEntry = super::slab::Entry<TrieNode>;
type Allocator = Slab<TrieNode, TrieNodeSlabEntry>;
type VacantEntry<'a> =
    super::slab::VacantEntry<'a, TrieNode, TrieNodeSlabEntry>;
pub type AllocatorRef<'trie> = RwLockReadGuard<'trie, Allocator>;
pub type AllocatorRefRef<'trie> = &'trie AllocatorRef<'trie>;

pub struct NodeMemoryAllocator {
    /// The number of available memory for nodes.
    idle_size: u32,
    /// The max number of nodes.
    size_limit: u32,
    // FIXME: implement EntryTrait for TrieNode
    allocator: RwLock<Allocator>,
}

/// The MSB is used to indicate if a node is in mem or on disk,
/// the rest 31 bits specifies the index of the node in the
/// memory region.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct MaybeNodeRef {
    index: u32,
}

impl Default for MaybeNodeRef {
    fn default() -> Self { Self { index: Self::NULL } }
}

impl MaybeNodeRef {
    const MAX_INDEX: u32 = 0x7fffffff;
    const NULL: u32 = 0;
    pub const NULL_NODE: MaybeNodeRef = MaybeNodeRef { index: Self::NULL };
    const ON_DISK_BIT: u32 = 0x80000000;
}

impl From<MaybeNodeRef> for Option<NodeRef> {
    fn from(x: MaybeNodeRef) -> Self {
        if x.index == MaybeNodeRef::NULL {
            Option::None
        } else if MaybeNodeRef::ON_DISK_BIT & x.index != 0 {
            Option::Some(NodeRef::OnDisk {
                index: (MaybeNodeRef::ON_DISK_BIT ^ x.index) as usize,
            })
        } else {
            Option::Some(NodeRef::InMemory {
                index: (MaybeNodeRef::MAX_INDEX ^ x.index) as usize,
            })
        }
    }
}

impl From<NodeRef> for MaybeNodeRef {
    fn from(node: NodeRef) -> Self {
        match node {
            NodeRef::InMemory { index } => Self {
                index: (index as u32) ^ MaybeNodeRef::MAX_INDEX,
            },
            NodeRef::OnDisk { index } => Self {
                index: (index as u32) ^ MaybeNodeRef::ON_DISK_BIT,
            },
        }
    }
}

impl From<Option<NodeRef>> for MaybeNodeRef {
    fn from(maybe_node: Option<NodeRef>) -> Self {
        match maybe_node {
            None => MaybeNodeRef::NULL_NODE,
            Some(node) => node.into(),
        }
    }
}

// Manages access to a child TrieNode. Converted from MaybeOwnedNode.
// TODO(yz): It must provide a way to check for COW tag, indicating if this node
// is created in the current commit. NodeRef is move because it controls access
// to TrieNode.
#[derive(Clone, Eq, PartialOrd, PartialEq, Ord)]
pub enum NodeRef {
    InMemory { index: usize },
    OnDisk { index: usize },
}

type OwnedChildrenTable = Option<Box<ChildrenTable>>;

/// Key length should be multiple of 8.
// TODO(yz): align key @8B with mask.
type KeyPart<'a> = &'a [u8];
const EMPTY_KEY_PART: KeyPart = &[];

/// Implement section.

impl Drop for TrieNode {
    fn drop(&mut self) {
        unsafe {
            let size = self.value_size;
            if size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE {
                self.value.ptr_into_vec(size.into());
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
            path_slice: self.path.get_slice(size),
            end_mask: self.path_end_mask,
        }
    }

    unsafe fn clear_path(&mut self) {
        let size = self.get_compressed_path_size();
        if size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE {
            self.path.ptr_into_vec(size.into());
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
        self.path = MaybeInPlaceByteArray::copy_from(
            path_slice,
            path_slice.len() as u16,
        );
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
        let size = self.value_size;
        if size > 0 {
            self.value.get_slice(size)
        } else {
            &[]
        }
    }

    fn value(&self) -> Option<Box<[u8]>> {
        let size = self.value_size;
        if size > 0 {
            Option::Some(self.value.get_slice(size).into())
        } else {
            Option::None
        }
    }

    /// Take value out of self.
    /// This method can only be called by replace_value / delete_value because
    /// empty node must be removed and pass compression must be maintained.
    // FIXME: hide this method
    fn value_into_vec(&mut self) -> Option<Vec<u8>> {
        let size = self.value_size;
        let maybe_value: Option<Vec<u8>>;
        if size > 0 {
            maybe_value = Some(self.value.into_vec(size));
            self.value_size = 0;
        } else {
            maybe_value = None;
        }
        maybe_value
    }

    fn replace_value_unchecked(&mut self, value: &[u8]) -> Option<Vec<u8>> {
        let old_value = self.value_into_vec();
        if old_value.is_none() {
            self.number_of_children_plus_value += 1;
        }
        let value_size = value.len() as u16;
        self.value_size = value_size;
        self.value = MaybeInPlaceByteArray::copy_from(value, value_size);

        old_value
    }

    fn check_value_size(value: &[u8]) -> Result<()> {
        let value_size = value.len();
        if value_size > MaybeInPlaceByteArray::MAX_SIZE as usize {
            // TODO(yz): value too long.
            return Err(Error::from_kind(ErrorKind::MPTInvalidValue));
        }
        if value_size == 0 {
            // TODO(yz): value is empty.
            return Err(Error::from_kind(ErrorKind::MPTInvalidValue));
        }

        Ok(())
    }
}

//struct CompressedPath<'a> {
//    path_slice: &'a [u8],
// FIXME: is it indeed useful?
//#[derive(Default)]
struct CompressedPath {
    path_slice: Box<[u8]>,
    end_mask: u8,
}

pub struct CompressedPathRef<'a> {
    pub path_slice: &'a [u8],
    pub end_mask: u8,
}

#[derive(Default)]
struct CompressedPathRaw {
    path_size: u16,
    path: MaybeInPlaceByteArray,
    end_mask: u8,
}

impl CompressedPathRaw {
    fn new(path_slice: &[u8], end_mask: u8) -> Self {
        let path_size = path_slice.len() as u16;
        Self {
            path_size: path_size,
            path: MaybeInPlaceByteArray::copy_from(path_slice, path_size),
            end_mask: end_mask,
        }
    }

    fn new_uninitialized(path_size: u16, end_mask: u8) -> Self {
        Self {
            path_size: path_size,
            path: MaybeInPlaceByteArray::new_uninitialized(path_size),
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

                    if Self::first_nibble((path_slice[i] ^ key[i])) == 0 {
                        // "First half" matched
                        matched_path = CompressedPathRaw::new(
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
                    }
                }
            }
        }
    }
}

/// The actions for the logical trie. Since we maintain a multiple version trie
/// the action must be translated into trie node operations, which may vary
/// depends on whether the node is owned by current version, etc.
enum TrieNodeAction {
    MODIFY,
    DELETE,
    MERGE_PATH {
        child_index: u8,
        child_node_ref: NodeRef,
    },
    DELETE_CHILDREN_TABLE,
}

/// Update
impl TrieNode {
    /// new_value can only be set according to the situation.
    /// children_table can only be replaced when there is no children in both
    /// old and new table.
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
                    ret.replace_value_unchecked(value);
                }
                None => {
                    ret.delete_value_unchecked();
                }
            },
            None => {
                ret.value_size = self.value_size;
                ret.value = MaybeInPlaceByteArray::copy_from(
                    self.value.get_slice(self.value_size),
                    self.value_size,
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
            CompressedPathRaw::new_uninitialized(concated_size, path.end_mask);

        {
            let slice = new_path.path.get_slice_mut(concated_size);
            if (prefix.end_mask == 0) {
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
    unsafe fn delete_value_unchecked(&mut self) -> Vec<u8> {
        let ret = self.value_into_vec().unwrap();

        self.number_of_children_plus_value -= 1;

        ret
    }

    /// Returns: old_value, is_self_about_to_delete, replacement_node_for_self
    fn check_delete_value(&self) -> Result<TrieNodeAction> {
        if self.has_value() {
            let number_of_children_plus_value =
                self.number_of_children_plus_value - 1;
            Ok(match number_of_children_plus_value {
                0 => TrieNodeAction::DELETE,
                1 => self.merge_path_action(),
                _ => TrieNodeAction::MODIFY,
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
                return TrieNodeAction::MERGE_PATH {
                    child_index: i as u8,
                    child_node_ref: Option::<NodeRef>::from(
                        children_table_ref[i],
                    )
                    .unwrap(),
                };
            }
        }
        unreachable!()
    }

    fn merge_path_action_after_child_deletion(
        &self, child_index: u8,
    ) -> TrieNodeAction {
        let children_table_ref = self.get_children_table_unchecked();
        for i in 0..CHILDREN_COUNT {
            if i != child_index as usize
                && children_table_ref[i] != MaybeNodeRef::NULL_NODE
            {
                return TrieNodeAction::MERGE_PATH {
                    child_index: i as u8,
                    child_node_ref: Option::<NodeRef>::from(
                        children_table_ref[i],
                    )
                    .unwrap(),
                };
            }
        }
        unreachable!()
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

    // FIXME: replace_child should be called on something that the
    // SubTrieVisitor owns because it modifies its FIXME: children_table in
    // any cases. FIXME: However it doesn't imply that when if falls into
    // the situation that a path compression is FIXME: necessary, the child
    // node is also owned.
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
                        TrieNodeAction::DELETE_CHILDREN_TABLE
                    } else {
                        self.merge_path_action_after_child_deletion(child_index)
                    }
                }
                _ => TrieNodeAction::MODIFY,
            }
        } else {
            TrieNodeAction::MODIFY
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
            node_ref: NodeRef::InMemory { index: 0 },
        }
    }
}

pub type OwnedNodeSet = BTreeSet<NodeRef>;

impl CowNodeRef {
    pub fn new_uninitialized_node<'trie>(
        allocator: AllocatorRefRef<'trie>, owned_node_set: &mut OwnedNodeSet,
    ) -> Result<(Self, VacantEntry<'trie>)> {
        let (node_ref, new_entry) = NodeMemoryAllocator::new_node(allocator)?;
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

    pub fn new<'trie>(
        node_ref: NodeRef, owned_node_set: &'trie OwnedNodeSet,
    ) -> Self {
        Self {
            owned: owned_node_set.contains(&node_ref),
            moved: false,
            node_ref: node_ref,
        }
    }

    fn into_owned<'trie>(
        &mut self, allocator: AllocatorRefRef<'trie>,
        owned_node_set: &'trie mut OwnedNodeSet,
    ) -> Result<Option<(&'trie TrieNode, VacantEntry<'trie>)>>
    {
        if (self.owned) {
            // TODO(yz): check if unused at all.
            //            let node: &'trie mut TrieNode;
            //            match self.node_ref {
            //                NodeRef::InMemory { ref mut index } => {
            //                    node =
            // NodeMemoryAllocator::get_node_mut(allocator, index);
            //                }
            //                _ => unreachable!(),
            //            }
            Ok(None)
        } else {
            let old_node =
                NodeMemoryAllocator::node_as_ref(allocator, &self.node_ref);

            // TODO(yz): maybe use Self::new_uninitialized_node().
            let (node_ref, new_entry) =
                NodeMemoryAllocator::new_node(allocator)?;
            owned_node_set.insert(node_ref.clone());
            self.node_ref = node_ref;

            Ok(Some((old_node, new_entry)))
        }
    }

    fn delete_node(&mut self, allocator: &NodeMemoryAllocator) {
        if self.owned {
            allocator.free_node(&mut self.node_ref);
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

    /// Only called on Merkle computation.
    fn load_merkles_from_children<'trie>(
        &self, trie: &'trie MultiVersionMerklePatriciaTrie,
        allocator: AllocatorRefRef<'trie>, owned_node_set: &'trie OwnedNodeSet,
        trie_node: &'trie TrieNode,
    ) -> MaybeMerkleTable
    {
        match trie_node.children_table {
            None => None,
            Some(ref table) => {
                let mut merkles = ChildrenMerkleTable::default();
                for i in 0..CHILDREN_COUNT {
                    let maybe_child_node: Option<NodeRef> = table[i].into();
                    merkles[i] = match maybe_child_node {
                        None => super::merkle::MERKLE_NULL_NODE,
                        Some(node_ref) => {
                            let mut cow_child_node =
                                Self::new(node_ref, owned_node_set);
                            let mut trie_node =
                                NodeMemoryAllocator::node_as_mut(
                                    allocator,
                                    &mut cow_child_node.node_ref,
                                );
                            let merkle = cow_child_node.get_or_compute_merkle(
                                trie,
                                allocator,
                                owned_node_set,
                                trie_node,
                            );

                            // The child node isn't replaced so it's safe to not
                            // insert into to parent node.
                            cow_child_node.into_child();

                            merkle
                        }
                    }
                }
                Some(merkles)
            }
        }
    }

    fn set_merkle(
        &mut self, children_merkles: MaybeMerkleTable, trie_node: &mut TrieNode,
    ) -> MerkleHash {
        let merkle = compute_merkle(
            trie_node.compressed_path_ref(),
            children_merkles,
            trie_node.value_as_slice(),
        );
        trie_node.merkle_hash = merkle;

        merkle
    }

    /// get if unowned, compute if owned.
    pub fn get_or_compute_merkle<'trie>(
        &mut self, trie: &'trie MultiVersionMerklePatriciaTrie,
        allocator: AllocatorRefRef<'trie>, owned_node_set: &'trie OwnedNodeSet,
        trie_node: &mut TrieNode,
    ) -> MerkleHash
    {
        if self.owned {
            let mut children_merkles = self.load_merkles_from_children(
                trie,
                allocator,
                owned_node_set,
                trie_node,
            );
            self.set_merkle(children_merkles, trie_node)
        } else {
            trie_node.merkle_hash
        }
    }

    // FIXME: why mut TrieNode when Cow doesn't own it?
    unsafe fn delete_value_if_owned(
        &self, trie_node: &mut TrieNode,
    ) -> Vec<u8> {
        if self.owned {
            // FIXME: delete value unchecked.
            trie_node.delete_value_unchecked()
        } else {
            trie_node.value().unwrap().into()
        }
    }

    fn cow_set_compressed_path<'trie>(
        &mut self, allocator: AllocatorRefRef<'trie>,
        owned_node_set: &'trie mut OwnedNodeSet, path: CompressedPathRaw,
        trie_node: &mut TrieNode,
    ) -> Result<()>
    {
        let copied = self.into_owned(allocator, owned_node_set)?;
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

    fn cow_delete_value_unchecked<'trie>(
        &mut self, allocator: AllocatorRefRef<'trie>,
        owned_node_set: &'trie mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<Vec<u8>>
    {
        let copied = self.into_owned(allocator, owned_node_set)?;
        Ok(match copied {
            None => unsafe { trie_node.delete_value_unchecked() },
            Some((old, new_entry)) => {
                new_entry.insert(unsafe {
                    old.copy_and_replace_fields(Some(None), None, None)
                });
                old.value().unwrap().into()
            }
        })
    }

    fn cow_replace_value_unchecked<'trie>(
        &mut self, allocator: AllocatorRefRef<'trie>,
        owned_node_set: &'trie mut OwnedNodeSet, trie_node: &mut TrieNode,
        value: &[u8],
    ) -> Result<Option<Vec<u8>>>
    {
        let copied = self.into_owned(allocator, owned_node_set)?;
        Ok(match copied {
            None => unsafe { trie_node.replace_value_unchecked(value) },
            Some((old, new_entry)) => {
                new_entry.insert(unsafe {
                    old.copy_and_replace_fields(Some(Some(value)), None, None)
                });
                old.value().map(|value| value.into_vec())
            }
        })
    }

    fn cow_delete_children_table<'trie>(
        &mut self, allocator: AllocatorRefRef<'trie>,
        owned_node_set: &'trie mut OwnedNodeSet, trie_node: &mut TrieNode,
    ) -> Result<()>
    {
        let copied = self.into_owned(allocator, owned_node_set)?;
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

    fn cow_replace_child<'trie>(
        &mut self, allocator: AllocatorRefRef<'trie>,
        owned_node_set: &'trie mut OwnedNodeSet, trie_node: &mut TrieNode,
        child_index: u8, child_node: MaybeNodeRef,
    ) -> Result<()>
    {
        let copied = self.into_owned(allocator, owned_node_set)?;
        match copied {
            None => unsafe {
                trie_node.set_child(child_index, child_node);
            },
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

/// TO convert a MaybeOwnedNode into a TrieNode, external information
/// is needed: disk db object and memory region object. A cache policy object is
/// also required
// TODO(yz): update doc.

// FIXME: use allocator instead of trie.
pub struct SubTrieVisitor<'trie> {
    trie_ref: &'trie MultiVersionMerklePatriciaTrie,
    root: CowNodeRef,

    // TODO: make sure that the NodeRef as key in cow_node_set doesn't change.
    // e.g. if some owned node is in memory, it should not be promoted into
    // memory without updating owned_node_set.
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

    fn memory_allocator(&self) -> &'trie NodeMemoryAllocator {
        &self.get_trie_ref().node_memory_allocator
    }

    fn memory_allocator_borrow(&self) -> AllocatorRef<'trie> {
        self.memory_allocator().allocator.read().unwrap()
    }

    pub fn get<'key>(&self, key: KeyPart<'key>) -> Result<Box<[u8]>> {
        let allocator = self.memory_allocator_borrow();
        let mut node_cow = self.root.clone();
        let mut key = key;
        loop {
            let trie_node = NodeMemoryAllocator::node_as_ref(
                &allocator,
                &node_cow.node_ref,
            );
            match trie_node.walk::<Read>(key) {
                WalkStop::Arrived => {
                    return trie_node.value().map_or_else(
                        || Err(Error::from_kind(ErrorKind::MPTKeyNotFound)),
                        |value| Ok(value),
                    );
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
                    return Err(Error::from_kind(ErrorKind::MPTKeyNotFound));
                }
            }
        }
    }

    /// The visitor can only be used once to modify.
    /// Returns (deleted value, is root node replaced, the current root node for
    /// the subtree).
    pub fn delete<'key>(
        mut self, key: KeyPart<'key>,
    ) -> Result<(Vec<u8>, bool, MaybeNodeRef)> {
        let allocator = self.memory_allocator_borrow();
        let mut node_cow = replace(&mut self.root, Default::default());
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        // FIXME: map_split?
        let trie_node_mut = NodeMemoryAllocator::node_as_mut(
            &allocator,
            &mut node_cow.node_ref,
        );
        match trie_node_mut.walk::<Read>(key) {
            WalkStop::Arrived => {
                // If value doesn't exists, returns invalid key error.
                let action = trie_node_mut.check_delete_value()?;
                match action {
                    TrieNodeAction::DELETE => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow.delete_value_if_owned(trie_node_mut)
                        };
                        // FIXME: deal with deletion while holding the
                        // trie_node_mut.
                        node_cow.delete_node(self.memory_allocator());
                        // FIXME: maybe unify NULL_NODE into
                        // node_cow.into_child()?
                        Ok((value, true, MaybeNodeRef::NULL_NODE))
                    }
                    TrieNodeAction::MERGE_PATH {
                        child_index,
                        child_node_ref,
                    } => {
                        // The current node is going to be dropped if owned.
                        let value = unsafe {
                            node_cow.delete_value_if_owned(trie_node_mut)
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
                            new_path = NodeMemoryAllocator::node_as_ref(
                                &allocator,
                                &child_node_cow.node_ref,
                            )
                            .path_prepended(path_prefix, child_index);
                        }
                        // FIXME: error processing for OOM.
                        let child_trie_node = NodeMemoryAllocator::node_as_mut(
                            &allocator,
                            &mut child_node_cow.node_ref,
                        );
                        child_node_cow.cow_set_compressed_path(
                            &allocator,
                            self.owned_node_set.get_mut(),
                            new_path,
                            child_trie_node,
                        );

                        // FIXME: how to represent that trie_node_mut is invalid
                        // after call to node_mut.delete_node?
                        // FIXME: trie_node_mut should be considered ref of
                        // node_mut.
                        node_cow.delete_node(self.memory_allocator());

                        Ok((value, true, child_node_cow.into_child()))
                    }
                    TrieNodeAction::MODIFY => {
                        let node_changed = !node_cow.owned;
                        let value = node_cow.cow_delete_value_unchecked(
                            &allocator,
                            self.owned_node_set.get_mut(),
                            trie_node_mut,
                        )?;

                        Ok((value, node_changed, node_cow.into_child()))
                    }
                    _ => unreachable!(),
                }
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                let (value, mut child_replaced, mut new_child_node) = self
                    .new_visitor_for_subtree(child_node)
                    .delete(key_remaining)?;
                if child_replaced {
                    let action = trie_node_mut
                        .check_replace_child(child_index, new_child_node);
                    match action {
                        TrieNodeAction::MERGE_PATH {
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
                                new_path = NodeMemoryAllocator::node_as_ref(
                                    &allocator,
                                    &child_node_cow.node_ref,
                                )
                                .path_prepended(path_prefix, child_index);
                            }
                            let child_trie_node =
                                NodeMemoryAllocator::node_as_mut(
                                    &allocator,
                                    &mut child_node_cow.node_ref,
                                );
                            child_node_cow.cow_set_compressed_path(
                                &allocator,
                                self.owned_node_set.get_mut(),
                                new_path,
                                child_trie_node,
                            );

                            // FIXME: how to represent that trie_node_mut is
                            // invalid after call to node_mut.delete_node?
                            // FIXME: trie_node_mut should be considered ref of
                            // node_mut.
                            node_cow.delete_node(self.memory_allocator());

                            Ok((value, true, child_node_cow.into_child()))
                        }
                        TrieNodeAction::DELETE_CHILDREN_TABLE => {
                            let node_changed = !node_cow.owned;
                            node_cow.cow_delete_children_table(
                                &allocator,
                                self.owned_node_set.get_mut(),
                                trie_node_mut,
                            );

                            Ok((value, node_changed, node_cow.into_child()))
                        }
                        TrieNodeAction::MODIFY => {
                            let node_changed = !node_cow.owned;
                            node_cow.cow_replace_child(
                                &allocator,
                                self.owned_node_set.get_mut(),
                                trie_node_mut,
                                child_index,
                                new_child_node,
                            );

                            Ok((value, node_changed, node_cow.into_child()))
                        }
                        _ => unreachable!(),
                    }
                } else {
                    Ok((value, false, node_cow.into_child()))
                }
            }

            _ => Err(Error::from_kind(ErrorKind::MPTKeyNotFound)),
        }
    }

    // Assume that the obtained TrieNode will be set valid value (non-empty)
    // later on.
    /// Insert a valid value into MPT.
    /// The visitor can only be used once to modify.
    unsafe fn insert_checked_value<'key>(
        mut self, key: KeyPart<'key>, value: &[u8],
    ) -> Result<(bool, MaybeNodeRef)> {
        let allocator = self.memory_allocator_borrow();
        let mut node_cow = replace(&mut self.root, Default::default());
        // TODO(yz): be compliant to borrow rule and avoid duplicated

        let trie_node_mut = NodeMemoryAllocator::node_as_mut(
            &allocator,
            &mut node_cow.node_ref,
        );
        match trie_node_mut.walk::<Write>(key) {
            WalkStop::Arrived => {
                let node_changed = !node_cow.owned;
                node_cow.cow_replace_value_unchecked(
                    &allocator,
                    self.owned_node_set.get_mut(),
                    trie_node_mut,
                    value,
                );

                Ok((node_changed, node_cow.into_child()))
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                let (mut child_changed, mut new_child_node) = self
                    .new_visitor_for_subtree(child_node)
                    .insert_checked_value(key_remaining, value)?;

                // No node deletion can happen
                if child_changed {
                    let node_changed = !node_cow.owned;
                    node_cow.cow_replace_child(
                        &allocator,
                        self.owned_node_set.get_mut(),
                        trie_node_mut,
                        child_index,
                        new_child_node,
                    );

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
                let (mut new_node_cow, mut new_node_entry) =
                    CowNodeRef::new_uninitialized_node(
                        &allocator,
                        self.owned_node_set.get_mut(),
                    )?;
                let mut new_node = TrieNode::default();
                // set compressed path.
                new_node.set_compressed_path(matched_path);

                node_cow.cow_set_compressed_path(
                    &allocator,
                    self.owned_node_set.get_mut(),
                    unmatched_path_remaining,
                    trie_node_mut,
                );

                new_node
                    .set_child(unmatched_child_index, node_cow.into_child());

                // TODO(yz): remove duplicated code.
                match key_child_index {
                    None => {
                        // Insert value at the current node
                        new_node.replace_value_unchecked(value);
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
                        new_child_node.replace_value_unchecked(value);
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
                new_child_node.replace_value_unchecked(value);
                child_node_entry.insert(new_child_node);

                let node_changed = !node_cow.owned;
                node_cow.cow_replace_child(
                    &allocator,
                    self.owned_node_set.get_mut(),
                    trie_node_mut,
                    child_index,
                    child_node_cow.into_child(),
                );

                Ok((node_changed, node_cow.into_child()))
            }
        }
    }

    pub fn set<'key>(
        mut self, key: KeyPart<'key>, value: &[u8],
    ) -> Result<MaybeNodeRef> {
        TrieNode::check_value_size(key)?;
        TrieNode::check_value_size(value)?;
        let new_root: MaybeNodeRef;
        unsafe {
            new_root = self.insert_checked_value(key, value)?.1;
        }
        Ok((new_root))
    }
}

impl Default for NodeMemoryAllocator {
    fn default() -> Self {
        // TODO(yz): use MAX_IN_MEM_TRIE_NODES_DISK_HYBRID.
        Self::new_with_idle_and_size_limit(
            Self::START_CAPACITY,
            Self::MAX_IN_MEM_TRIE_NODES_MEM_ONLY,
        )
        .unwrap()
    }
}

impl NodeMemoryAllocator {
    fn new_with_idle_and_size_limit(
        idle_size: u32, size_limit: u32,
    ) -> Result<Self> {
        Ok(Self {
            idle_size: idle_size,
            size_limit: size_limit,
            allocator: RwLock::new(Slab::with_capacity(idle_size as usize)),
        })
    }

    pub fn get_allocator<'a>(&'a self) -> AllocatorRef<'a> {
        self.allocator.read().unwrap()
    }

    // Methods that requires mut borrow of slab.
    pub fn enlarge(&self) -> Result<()> {
        // TODO(yz): no unwrap to LockResult here?
        let mut allocator_mut = self.allocator.write().unwrap();
        let new_size = allocator_mut.len() + self.idle_size as usize;
        let size_limit = self.size_limit as usize;
        if new_size * 2 >= size_limit {
            allocator_mut.reserve_exact(size_limit)?;
        } else {
            allocator_mut.reserve(new_size)?;
        }
        Ok(())
    }

    pub fn node_as_ref<'a>(
        allocator: AllocatorRefRef<'a>, node: &NodeRef,
    ) -> &'a TrieNode {
        match node {
            NodeRef::InMemory { index } => {
                NodeMemoryAllocator::get_in_memory_node_ref(allocator, index)
            }
            _ => unimplemented!(),
        }
    }

    pub fn node_as_mut<'a>(
        allocator: AllocatorRefRef<'a>, node: &mut NodeRef,
    ) -> &'a mut TrieNode {
        match node {
            NodeRef::InMemory { index } => {
                NodeMemoryAllocator::get_in_memory_node_mut(allocator, index)
            }
            _ => unimplemented!(),
        }
    }

    // Methods that doesn't requires mut borrow of slab.
    pub fn get_in_memory_node_ref<'a>(
        allocator: AllocatorRefRef<'a>, index: &usize,
    ) -> &'a TrieNode {
        unsafe { allocator.get_unchecked(*index) }
    }

    fn get_in_memory_node_mut<'a>(
        allocator: AllocatorRefRef<'a>, index: &mut usize,
    ) -> &'a mut TrieNode {
        unsafe { allocator.get_unchecked_mut(*index) }
    }

    fn new_node<'a>(
        allocator: AllocatorRefRef<'a>,
    ) -> Result<(NodeRef, VacantEntry<'a>)> {
        let vacant_entry = allocator.vacant_entry()?;
        let node = NodeRef::InMemory {
            index: vacant_entry.key(),
        };
        Ok((node, vacant_entry))
    }

    fn free_node(&self, node: &mut NodeRef) {
        match *node {
            NodeRef::InMemory { index } => {
                self.allocator.read().unwrap().remove(index).unwrap();
            }
            NodeRef::OnDisk { index } => unimplemented!(),
        }
    }
}
