// TODO(yz): evict to key-value store on disk when out-of-memory.
use self::data_structure::*;
use execution::EpochId;
use std::{collections::HashMap, sync::RwLock};

pub(super) mod data_structure;
pub(super) mod merkle;
pub(super) mod return_after_use;

/// Fork to slab in order to compact data.
mod slab;

/// The memory consumption of MerklePatriciaTrie is
/// 64B * (number of node (TrieNode) + number of non-leaf node
/// (OwnedChildrenTable) )
///
/// TODO(yz): explain more about how MAX_TRIE_NODES is chosen.
pub struct MultiVersionMerklePatriciaTrie {
    // We don't distinguish an epoch which doesn't exists from an epoch which
    // contains nothing.
    root_by_version: RwLock<HashMap<EpochId, NodeRef>>,
    node_memory_allocator: NodeMemoryAllocator,
    // TODO(yz): do we manage ChildrenTable in the allocator?
}

impl MultiVersionMerklePatriciaTrie {
    pub fn get_root_at_epoch(&self, epoch_id: EpochId) -> Option<NodeRef> {
        self.root_by_version.read().unwrap().get(&epoch_id).cloned()
    }

    pub fn commit_epoch_root(&self, epoch_id: EpochId, root: NodeRef) {
        self.root_by_version.write().unwrap().insert(epoch_id, root);
    }
}
