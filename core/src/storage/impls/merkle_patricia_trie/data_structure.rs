use super::merkle::MerkleHash;
use alloc::alloc::dealloc;
use core::alloc::Layout;
use std::{
    assert_eq, cell::Cell, cmp::min, marker::PhantomData, mem, ptr::NonNull,
    slice, vec::Vec,
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
    /// Can be: "no mask" (0x00), first half.
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
    child_node: OwnedChildrenTable,
    // Rust automatically moves the value_size field in order to minimize the
    // total size of the struct.
    /// We limit the maximum value length by u16. If it proves insufficient,
    /// manage the length and content separately.
    value_size: u16,
    value: MaybeInPlaceByteArray,
    merkle_hash: MerkleHash,
}

#[allow(unions_with_drop_fields)]
union MaybeInPlaceByteArray {
    in_place: [u8; 8],
    // TODO(yz): statically assert that the ptr has size of at most 8.
    // TODO(yz): to initialize and destruct, convert from/into Vec.
    // TODO(yz): introduce a type to pass into template which manages the
    // conversion from/to  Vec<A>. The type should also take the offset of
    // u16 size from the TrieNode struct to manage memory buffer, and
    // should offer a type for ptr here.
    ptr: NonNull<u8>,
}

impl MaybeInPlaceByteArray {
    const MAX_INPLACE_SIZE: u16 = 8;
}

// TODO(yz): statically check the size to be 64B
/// The children table for a non-leaf trie node.
type ChildrenTable = [OwnedNode; 16];

type NodeMemoryRegion = Vec<TrieNode>;

pub struct NodeMemoryAllocator {
    count: u32,
    // TODO(yz): use MAX_TRIE_NODES_DISK_HYBRID.
    node: NodeMemoryRegion,
}

struct OwnedNode {
    memory_region: NonNull<NodeMemoryRegion>,
    // TODO(yz): use the MSB to indicate if a node is in mem or on disk,
    // the rest 31 bits specifies the index of the node in the
    // memory region.
    index: u32,
}

type OwnedChildrenTable = Box<ChildrenTable>;

/// Key length should be multiple of 8.
// TODO(yz): align key @8B with mask.
type KeyPart<'a> = &'a [u8];
const EMPTY_KEY_PART: KeyPart = &[];

/// Implement section.

impl OwnedNode {
    fn memory_region_as_ref(&self) -> &NodeMemoryRegion {
        unsafe { self.memory_region.as_ref() }
    }

    fn memory_region_as_mut(&mut self) -> &mut NodeMemoryRegion {
        unsafe { self.memory_region.as_mut() }
    }
}

impl AsRef<TrieNode> for OwnedNode {
    fn as_ref(&self) -> &TrieNode {
        &self.memory_region_as_ref()[self.index as usize]
    }
}

impl AsMut<TrieNode> for OwnedNode {
    fn as_mut(&mut self) -> &mut TrieNode {
        let index = self.index as usize;
        &mut self.memory_region_as_mut()[index]
    }
}

impl Drop for TrieNode {
    fn drop(&mut self) {
        unsafe {
            let size = self.value_size;
            if (size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE) {
                dealloc(
                    self.value.ptr.as_ptr(),
                    Layout::from_size_align_unchecked(
                        size.into(),
                        mem::align_of::<u8>(),
                    ),
                );
            }
        }

        unsafe {
            let size = self.get_path_size();
            if (size > MaybeInPlaceByteArray::MAX_INPLACE_SIZE) {
                dealloc(
                    self.path.ptr.as_ptr(),
                    Layout::from_size_align_unchecked(
                        size.into(),
                        mem::align_of::<u8>(),
                    ),
                );
            }
        }
    }
}

impl TrieNode {
    fn get_path_size(&self) -> u16 {
        (self.path_steps / 2)
            + (((self.path_begin_mask | self.path_end_mask) != 0) as u16)
    }

    unsafe fn get_path_slice(&self) -> &[u8] {
        let len = self.get_path_size();
        if (len > MaybeInPlaceByteArray::MAX_INPLACE_SIZE) {
            unsafe {
                return slice::from_raw_parts_mut(
                    self.path.ptr.as_ptr(),
                    len.into(),
                );
            }
        } else {
            return &self.path.in_place[0..len.into()];
        }
    }
}

/// Traverse.
impl TrieNode {
    // TODO(yz): write test.
    fn walk_to_child_or_die<'a>(
        &self, key: KeyPart<'a>,
    ) -> (Option<&TrieNode>, KeyPart<'a>) {
        // Compare till the last full byte.

        let path_slice = unsafe { self.get_path_slice() };

        // Compare bytes till the last full byte. The first byte is always
        // included because even if it's the second-half, it must be
        // already matched before entering this TrieNode.
        let memcmp_len = min(
            path_slice.len() - ((self.path_end_mask != 0) as usize),
            key.len(),
        );

        for i in 0..memcmp_len {
            // TODO(yz): check if compiler skips range check. Probably not
            // because "-" in memcmp_len calculation may underflow.
            if (path_slice[i] != key[i]) {
                return (Option::None, key);
            }
        }

        // Key is fully consumed, get value attached.
        if (key.len() == memcmp_len) {
            // Compressed path isn't fully consumed.
            if (path_slice.len() > memcmp_len) {
                return (Option::None, EMPTY_KEY_PART);
            } else {
                // The value we are looking for should be attached to this node,
                // if any.
                return (Option::Some(self), EMPTY_KEY_PART);
            }
        } else {
            // Key is not fully consumed.

            let mut child_index;
            let mut key_part_for_child;

            if (path_slice.len() == memcmp_len) {
                // Compressed path is fully consumed. Descend into one child.
                child_index = key[memcmp_len] >> 4;
                key_part_for_child = &key[memcmp_len..];
            } else {
                // One half byte remaining to match with path. Consume it in the
                // key.
                if ((path_slice[memcmp_len] ^ key[memcmp_len])
                    & Self::BITS_0_3_MASK
                    != 0)
                {
                    // Mismatch.
                    return (Option::None, key);
                }

                child_index = key[memcmp_len] & Self::BITS_4_7_MASK;
                key_part_for_child = &key[memcmp_len + 1..];
            }

            // TODO(yz): Is compiler able to skip range check?
            return (
                Option::Some((*self.child_node)[child_index as usize].as_ref()),
                key_part_for_child,
            );
        }
    }

    /// The start of key should be always aligned with compressed path of
    /// current node, e.g. if compressed path starts at the second-half, so
    /// should be key.
    fn walk_to_child_or_create<'a>(
        &self, key: KeyPart<'a>,
    ) -> (&mut TrieNode, KeyPart<'a>) {
        unimplemented!()
    }
}
