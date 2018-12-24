use self::{merkle::*, node_memory_manager::*, node_ref_map::*};
use super::errors::*;
use kvdb::KeyValueDB;
use parking_lot::RwLock;
use primitives::EpochId;
use std::{collections::HashMap, sync::Arc};

pub mod cache;
pub(in super::super) mod data_structure;
pub(self) mod maybe_in_place_byte_array;
pub mod merkle;
pub(in super::super) mod node_memory_manager;
pub(self) mod node_ref_map;
pub(super) mod return_after_use;
pub(super) mod row_number;

/// Fork of upstre  am slab in order to compact data and to provide internal
/// mutability.
mod slab;

pub struct MultiVersionMerklePatriciaTrie {
    /// We don't distinguish an epoch which doesn't exists from an epoch which
    /// contains nothing.
    /// This version map is incomplete as the rest map lives in disk db.
    root_by_version: RwLock<HashMap<EpochId, NodeRef>>,
    /// The nodes in memory should be considered a cache for MPT.
    /// However for delta_trie the disk_db contains MPT nodes which are swapped
    /// out from memory because persistence isn't necessary.
    ///
    /// Note that we don't manage ChildrenTable in allocator because it's
    /// variable-length.
    ///
    /// The node memory manager holds reference to db on disk which stores MPT
    /// nodes.
    node_memory_manager: NodeMemoryManager,
}

impl MultiVersionMerklePatriciaTrie {
    pub fn new(kvdb: Arc<KeyValueDB>) -> Self {
        Self {
            root_by_version: Default::default(),
            node_memory_manager: NodeMemoryManager::new(kvdb),
        }
    }

    pub fn get_root_at_epoch(&self, epoch_id: EpochId) -> Option<NodeRef> {
        self.root_by_version.read().get(&epoch_id).cloned()
    }

    pub fn set_epoch_root(&self, epoch_id: EpochId, root: NodeRef) {
        self.root_by_version.write().insert(epoch_id, root);
    }

    pub fn loaded_root_at_epoch(
        &self, epoch_id: EpochId, db_key: DeltaMptDbKey,
    ) -> NodeRef {
        let root = NodeRef::Committed { db_key: db_key };
        self.set_epoch_root(epoch_id, root.clone());

        root
    }

    pub fn get_node_memory_manager(&self) -> &NodeMemoryManager {
        &self.node_memory_manager
    }

    pub fn get_merkle(
        &self, maybe_node: MaybeNodeRef,
    ) -> Result<Option<MerkleHash>> {
        let maybe_node: Option<NodeRef> = maybe_node.into();
        match maybe_node {
            Some(node) => Ok(Some(
                self.node_memory_manager
                    .node_as_ref(
                        &self.node_memory_manager.get_allocator(),
                        &node,
                    )?
                    .merkle_hash,
            )),
            None => Ok(None),
        }
    }
}
