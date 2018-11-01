// TODO(yz): evict to key-value store on disk when out-of-memory.
// TODO(yz): current implementation is single version.
use self::data_structure::*;

/// The memory consumption of MerklePatriciaTrie is
/// 64B * (number of node (TrieNode) + number of non-leaf node
/// (OwnedChildrenTable) )
///
/// TODO(yz): explain more about how MAX_TRIE_NODES is chosen.
pub struct MultiVersionMerklePatriciaTrie {
    root: TrieNode, // TODO(yz): maybe lifetime of root can be shorter?
    node_memory_allocator: NodeMemoryAllocator,
    // TODO(yz): do we manage ChildrenTable in a memory region?
}

pub(super) mod data_structure;
pub(super) mod merkle;

/// Fork to slab in order to compact data.
mod slab;
