use super::merkle::MerkleHash;
use core::slice;
use std::{
    cmp::min,
    io::{Error, ErrorKind::NotFound},
    marker::{Send, Sync},
    mem::{self, replace},
    ops::{Deref, DerefMut},
    ptr::null_mut,
    result::Result,
    vec::Vec,
};

impl NodeMemoryAllocator {
    /// In disk hybrid solution, the nodes in memory are merely LRU cache of
    /// non-leaf nodes. So the memory consumption is 128B * number of nodes.
    /// 5GB ~ 31_250_000 nodes. There are costs to maintain references to disk
    /// and cache recency... so TODO(yz): maybe
    pub const MAX_IN_MEM_TRIE_NODES_DISK_HYBRID: usize = 26_000_000;
    /// If we do not swap out any node onto disk, the maximum tolerable nodes is
    /// about 43.2M, where there is about 7.2M leaf node. The total memory
    /// consumption is about 43.2 * 128 - 7.2 * 64 MB ~= 5GB. It can hold new
    /// items for about 1 hour assuming 2k updates per second.
    pub const MAX_IN_MEM_TRIE_NODES_MEM_ONLY: usize = 43_200_000;
}

// TODO(yz): choose the best type to attach the consts.
impl TrieNode {
    const BITS_0_3_MASK: u8 = 0x0f;
    const BITS_4_7_MASK: u8 = 0xf0;
}

// TODO(yz): statically check the size to be 64B
// TODO(yz): TrieNode should leave one byte so that it can be used to indicate a
// free slot in memory region.
/// A node consists of an optional compressed path (concept of Patricia
/// Trie), an optional ChildrenTable (if the node is intermediate), an
/// optional value attached, and the Merkle hash for subtree.
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
    merkle_hash: MerkleHash,
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
    fn new(value: Vec<u8>, size: u16) -> Self {
        if size > Self::MAX_INPLACE_SIZE {
            Self {
                ptr: Box::into_raw(value.into_boxed_slice()) as *mut u8,
            }
        } else {
            let mut x: Self = Self {
                in_place: Default::default(),
            };
            unsafe {
                x.in_place[0..size.into()].copy_from_slice(value.as_slice());
            }
            x
        }
    }
}

impl MaybeInPlaceByteArray {
    const MAX_INPLACE_SIZE: u16 = 8;
    const MAX_SIZE: u16 = 0xffff;
}

// TODO(yz): statically check the size to be 64B
/// The children table for a non-leaf trie node.
type ChildrenTable = [MaybeNodeRef; 16];

type NodeMemoryRegion = Vec<TrieNode>;

pub struct NodeMemoryAllocator {
    count: u32,
    // TODO(yz): use MAX_TRIE_NODES_DISK_HYBRID.
    nodes: NodeMemoryRegion,
}

/// The MSB is used to indicate if a node is in mem or on disk,
/// the rest 31 bits specifies the index of the node in the
/// memory region.
#[derive(Copy, Clone, Eq, PartialEq)]
struct MaybeNodeRef {
    index: u32,
}

impl Default for MaybeNodeRef {
    fn default() -> Self { Self { index: Self::NULL } }
}

impl MaybeNodeRef {
    const MAX_INDEX: u32 = 0x7fffffff;
    const NULL: u32 = 0;
    const NULL_NODE: MaybeNodeRef = MaybeNodeRef { index: Self::NULL };
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

// Manages access to a child TrieNode. Converted from MaybeOwnedNode.
// It should also contain a COW tag to indicate if this node is created in the
// current commit.
#[derive(Copy, Clone)]
enum NodeRef {
    InMemory { index: usize },
    OnDisk { index: usize },
}

// What if memory alloc table happens and changes?
// After deletion, content of some node is moved into new place.
// Anything pointing to it should be updated.
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
        }

        unsafe {
            let size = self.get_compressed_path_size();
            if size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE {
                self.path.ptr_into_vec(size.into());
            }
        }
    }
}

impl TrieNode {
    fn get_compressed_path_size(&self) -> u16 {
        (self.path_steps / 2)
            + (((self.path_begin_mask | self.path_end_mask) != 0) as u16)
    }

    fn compressed_path(&self) -> CompressedPath {
        let size = self.get_compressed_path_size();
        CompressedPath {
            path_slice: self.path.get_slice(size),
            end_mask: self.path_end_mask,
        }
    }

    fn value(&self) -> Option<&[u8]> {
        let size = self.value_size;
        if size > 0 {
            Option::Some(&self.value.get_slice(size))
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

    fn replace_value(
        &mut self, value: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, Error> {
        let value_size = value.len();
        if value_size > MaybeInPlaceByteArray::MAX_SIZE as usize {
            // TODO(yz): define our own error.
            return Err(Error::new(
                NotFound,
                "Value too long when trying to insert value into TrieNode.",
            ));
        }
        if value_size == 0 {
            // TODO(yz): define our own error.
            return Err(Error::new(
                NotFound,
                "Value is empty when trying to insert value into TrieNode.",
            ));
        }
        let old_value = self.value_into_vec();
        if old_value.is_none() {
            self.number_of_children_plus_value += 1;
        }
        let value_size = value_size as u16;
        self.value_size = value_size;
        self.value = MaybeInPlaceByteArray::new(value, value_size);

        Ok(old_value)
    }

    /// merkle hash for new nodes are computed in batch, because it doesn't make
    /// sense to keep merkle hash between executions in an epoch.
    fn update_merkle_hash(&mut self) { unimplemented!() }
}

struct CompressedPath<'a> {
    path_slice: &'a [u8],
    end_mask: u8,
}

enum WalkStop<'key, 'trie> {
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
        key_remaining: KeyPart<'key>,
        maybe_matched_path: Option<CompressedPath<'trie>>,
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

impl<'key, 'trie> WalkStop<'key, 'trie> {
    const CHILD_NOT_FOUND: WalkStop<'key, 'trie> = WalkStop::ChildNotFound {
        key_remaining: &[],
        child_index: 0,
    };
    const PATH_DIVERTED: WalkStop<'key, 'trie> = WalkStop::PathDiverted {
        // I wish I could use Default::default() here.
        key_remaining: &[],
        maybe_matched_path: Option::None,
    };
}

trait AccessMode {
    fn is_read_only() -> bool;
}

struct Read;
struct Write;

impl AccessMode for Read {
    fn is_read_only() -> bool { return true; }
}

impl AccessMode for Write {
    fn is_read_only() -> bool { return false; }
}

/// Traverse.
impl TrieNode {
    // TODO(yz): write test.
    /// The start of key is always aligned with compressed path of
    /// current node, e.g. if compressed path starts at the second-half, so
    /// should be key.
    fn walk<'trie, 'key, AM: AccessMode>(
        &'trie self, key: KeyPart<'key>,
    ) -> WalkStop<'key, 'trie> {
        let path = self.compressed_path();
        let path_slice = path.path_slice;

        // Compare bytes till the last full byte. The first byte is always
        // included because even if it's the second-half, it must be
        // already matched before entering this TrieNode.
        let memcmp_len = min(
            path_slice.len() - ((path.end_mask != 0) as usize),
            key.len(),
        );

        for i in 0..memcmp_len {
            // TODO(yz): check if compiler skips range check. Probably not
            // because "-" in memcmp_len calculation may underflow.
            if path_slice[i] != key[i] {
                if AM::is_read_only() {
                    return WalkStop::PATH_DIVERTED;
                } else {
                    let matched_path = if (path_slice[i] ^ key[i])
                        & Self::BITS_0_3_MASK
                        == 0
                    {
                        // "First half" matched
                        CompressedPath {
                            path_slice: &path_slice[0..i + 1],
                            end_mask: Self::BITS_0_3_MASK,
                        }
                    } else {
                        CompressedPath {
                            path_slice: &path_slice[0..i],
                            end_mask: 0,
                        }
                    };
                    return WalkStop::PathDiverted {
                        key_remaining: &key[i..],
                        maybe_matched_path: Option::Some(matched_path),
                    };
                }
            }
        }
        // Key is fully consumed, get value attached.
        if key.len() == memcmp_len {
            // Compressed path isn't fully consumed.
            if path_slice.len() > memcmp_len {
                if AM::is_read_only() {
                    return WalkStop::PATH_DIVERTED;
                } else {
                    return WalkStop::PathDiverted {
                        key_remaining: &key[memcmp_len..],
                        maybe_matched_path: Option::Some(CompressedPath {
                            path_slice: &path_slice[0..memcmp_len],
                            end_mask: 0,
                        }),
                    };
                }
            } else {
                return WalkStop::Arrived;
            }
        } else {
            // Key is not fully consumed.

            // If descent into child, if child exists under child_index.
            let child_index;
            let key_remaining;

            if path_slice.len() == memcmp_len {
                // Compressed path is fully consumed. Descend into one child.
                child_index = key[memcmp_len] >> 4;
                key_remaining = &key[memcmp_len..];
            } else {
                // One half byte remaining to match with path. Consume it in the
                // key.
                if (path_slice[memcmp_len] ^ key[memcmp_len])
                    & Self::BITS_0_3_MASK
                    != 0
                {
                    // Mismatch.
                    if AM::is_read_only() {
                        return WalkStop::PATH_DIVERTED;
                    } else {
                        return WalkStop::PathDiverted {
                            key_remaining: &key[memcmp_len..],
                            maybe_matched_path: Option::Some(CompressedPath {
                                path_slice: &path_slice[0..memcmp_len],
                                end_mask: 0,
                            }),
                        };
                    }
                }

                child_index = key[memcmp_len] & Self::BITS_4_7_MASK;
                key_remaining = &key[memcmp_len + 1..];
            }

            // TODO(yz): Is compiler able to skip range check?
            match self.get_child(child_index).into() {
                Option::None => {
                    if AM::is_read_only() {
                        return WalkStop::CHILD_NOT_FOUND;
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

/// Update
impl TrieNode {
    /// Returns old_value, is_self_about_to_delete, replacement_node_for_self
    fn delete_value(&mut self) -> (Option<Vec<u8>>, bool, MaybeNodeRef) {
        let old_value = self.value_into_vec();
        if old_value.is_some() {
            self.number_of_children_plus_value -= 1;
            match self.number_of_children_plus_value {
                0 => {
                    // FIXME: delete self, is it necessary to inform parent or
                    // return new state?
                }
                1 => {
                    // FIXME: delete self, update the only child.
                }
                _ => {}
            };
        }
        (old_value, false, MaybeNodeRef::NULL_NODE)
    }

    fn get_child(&self, child_index: u8) -> MaybeNodeRef {
        return self
            .children_table
            .as_ref()
            .map_or(MaybeNodeRef::NULL_NODE, |table| {
                table[child_index as usize]
            });
    }

    /// Returns old_child, is_self_about_to_delete, replacement_node_for_self
    fn replace_child(
        &mut self, child_index: u8, child_node: MaybeNodeRef,
    ) -> (MaybeNodeRef, bool, MaybeNodeRef) {
        let mut delta = 0;
        if child_node != MaybeNodeRef::NULL_NODE {
            delta += 1;
        }
        let old_child = replace(
            &mut self
                .children_table
                .get_or_insert_with(|| Default::default())
                [child_index as usize],
            child_node,
        );
        if old_child != MaybeNodeRef::NULL_NODE {
            delta -= 1;
        }
        self.number_of_children_plus_value =
            (self.number_of_children_plus_value as i16 + delta) as u8;
        if delta == -1 {
            match self.number_of_children_plus_value {
                0 => {
                    // It's not possible to have 0 child 0 value after delete a
                    // child, because the previous state
                    // must be 1 child 0 value, which can not exist.
                }
                1 => {
                    // FIXME: The state can be 1 child 0 value or 0 child 1
                    // value, FIXME: the previous state is 2
                    // child 0 value or 1 child 1 value.
                    // FIXME: in one case, path compression, in another case,
                    // delete children_table.
                }
                _ => {}
            };
        }
        (old_child, false, MaybeNodeRef::NULL_NODE)
    }
}

// TODO(yz): move to file for merkle_patricia_trie.
use super::MultiVersionMerklePatriciaTrie;

/// TO convert a MaybeOwnedNode into a TrieNode, external information
/// is needed: disk db object and memory region object. A cache policy object is
/// also required. TODO(yz): update doc.

struct SubTrieVisitor<TrieRef> {
    trie: TrieRef,
    root: NodeRef,
}

impl<TrieRef: Deref<Target = MultiVersionMerklePatriciaTrie>>
    SubTrieVisitor<TrieRef>
{
    fn new(trie: TrieRef, root: NodeRef) -> Self {
        Self {
            trie: trie,
            root: root,
        }
    }

    fn memory_allocator_as_ref(&self) -> &NodeMemoryAllocator {
        &(*self.trie).node_memory_allocator
    }

    fn memory_region_as_ref(&self) -> &NodeMemoryRegion {
        self.memory_allocator_as_ref().nodes.as_ref()
    }

    fn node_as_ref(&self, node: NodeRef) -> &TrieNode {
        match node {
            NodeRef::InMemory { index } => &self.memory_region_as_ref()[index],
            _ => unimplemented!(),
        }
    }

    fn get<'key>(&self, key: KeyPart<'key>) -> Result<&[u8], Error> {
        let mut node = self.node_as_ref(self.root);
        let mut key = key;
        loop {
            match node.walk::<Write>(key) {
                WalkStop::Arrived => {
                    // TODO(yz): Set proper error.
                    return node.value().map_or_else(
                        || Err(Error::new(NotFound, "")),
                        |value| Ok(value),
                    );
                }
                WalkStop::Descent {
                    key_remaining,
                    child_index: _,
                    child_node,
                } => {
                    node = self.node_as_ref(child_node);
                    key = key_remaining;
                }
                _ => {
                    // TODO(yz): Set proper error.
                    return Err(Error::new(NotFound, ""));
                }
            }
        }
    }
}

impl<TrieRef: DerefMut<Target = MultiVersionMerklePatriciaTrie>>
    SubTrieVisitor<TrieRef>
{
    fn memory_allocator_as_mut(&mut self) -> &mut NodeMemoryAllocator {
        &mut (*self.trie).node_memory_allocator
    }

    fn memory_region_as_mut(&mut self) -> &mut NodeMemoryRegion {
        self.memory_allocator_as_mut().nodes.as_mut()
    }

    fn node_as_mut(&mut self, node: NodeRef) -> &mut TrieNode {
        match node {
            NodeRef::InMemory { index } => {
                &mut self.memory_region_as_mut()[index]
            }
            _ => unimplemented!(),
        }
    }

    fn free_node(&mut self) {
        let node = self.root;
        self.memory_allocator_as_mut().free_node(node);
    }

    fn delete<'key>(
        &mut self, key: KeyPart<'key>,
    ) -> Result<(Vec<u8>, bool, MaybeNodeRef), Error> {
        let node_ref = self.root;
        // FIXME: be compliant to borrow rule and avoid duplicated computation
        match self.node_as_ref(node_ref).walk::<Read>(key) {
            WalkStop::Arrived => {
                let (maybe_value, self_to_delete, replacement) =
                // FIXME: be compliant to borrow rule and avoid duplicated computation
                    self.node_as_mut(node_ref).delete_value();
                if self_to_delete {
                    self.free_node();
                }
                maybe_value.map_or_else(
                    // TODO(yz): Set proper error.
                    || Err(Error::new(NotFound, "")),
                    |value| Ok((value, self_to_delete, replacement)),
                )
            }
            WalkStop::Descent {
                key_remaining,
                child_node,
                child_index,
            } => {
                let (value, mut child_changed, mut new_child_node) =
                    SubTrieVisitor::<&mut MultiVersionMerklePatriciaTrie>::new(
                        &mut *self.trie,
                        child_node,
                    )
                    .delete(key_remaining)?;
                if child_changed {
                    let (old_child, self_to_delete, replacement) = self
                        .node_as_mut(node_ref)
                        .replace_child(child_index, new_child_node);
                    if self_to_delete {
                        if self_to_delete {
                            self.free_node();
                        }
                        Ok((value, self_to_delete, replacement))
                    } else {
                        Ok((value, false, MaybeNodeRef::NULL_NODE))
                    }
                } else {
                    Ok((value, false, MaybeNodeRef::NULL_NODE))
                }
            }

            // TODO(yz): Set proper error.
            _ => Err(Error::new(NotFound, "")),
        }
    }

    fn put<'key>(&mut self, key: KeyPart<'key>, value: &[u8]) {
        unimplemented!()
    }
}

impl NodeMemoryAllocator {
    fn free_node(&mut self, node_ref: NodeRef) { unimplemented!() }
}
