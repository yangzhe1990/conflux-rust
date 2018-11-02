// TODO(yz): evict to key-value store on disk when out-of-memory.
use self::{data_structure::*, merkle::*};
use execution::EpochId;
use std::{collections::HashMap, sync::RwLock};

pub(super) mod data_structure;
pub mod merkle;
pub(super) mod return_after_use;

/// Fork to slab in order to compact data.
mod slab;

/// The memory consumption of MerklePatriciaTrie is
/// 64B * (number of node (TrieNode) + number of non-leaf node
/// (OwnedChildrenTable) )
///
/// TODO(yz): explain more about how MAX_TRIE_NODES is chosen.
#[derive(Default)]
pub struct MultiVersionMerklePatriciaTrie {
    // We don't distinguish an epoch which doesn't exists from an epoch which
    // contains nothing.
    root_by_version: RwLock<HashMap<EpochId, NodeRef>>,
    // FIXME: remove pub.
    pub node_memory_allocator: NodeMemoryAllocator,
    // TODO(yz): do we manage ChildrenTable in the allocator?
}

impl MultiVersionMerklePatriciaTrie {
    pub fn get_root_at_epoch(&self, epoch_id: EpochId) -> Option<NodeRef> {
        self.root_by_version.read().unwrap().get(&epoch_id).cloned()
    }

    pub fn commit_epoch_root(&self, epoch_id: EpochId, root: NodeRef) {
        self.root_by_version.write().unwrap().insert(epoch_id, root);
    }

    pub fn get_allocator<'a>(&'a self) -> AllocatorRef<'a> {
        self.node_memory_allocator.get_allocator()
    }

    pub fn get_merkle_at_node(&self, node: MaybeNodeRef) -> MerkleHash {
        match self.get_merkle(&self.get_allocator(), node) {
            Some(hash) => hash,
            None => MERKLE_NULL_NODE,
        }
    }

    fn get_merkle<'a>(
        &'a self, allocator: AllocatorRefRef<'a>, maybe_node: MaybeNodeRef,
    ) -> Option<MerkleHash> {
        let mut maybe_node: Option<NodeRef> = maybe_node.into();
        match maybe_node {
            Some(node) => Some(
                NodeMemoryAllocator::node_as_ref(allocator, &node).merkle_hash,
            ),
            None => None,
        }
        // FIXME: the node memory should take a NodeRef instead.
    }
}
