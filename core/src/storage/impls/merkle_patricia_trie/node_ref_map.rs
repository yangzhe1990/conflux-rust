use super::{
    node_memory_manager::*, row_number::RowNumberUnderlyingType, MaybeNodeRef,
};
use std::vec::Vec;

/// NodeRef for persistent MPT may add a field for storage access key, and a
/// reference count. For persistent MPT, NodeRef (storage access key) for
/// non-cached node might be kept, to be able to read child node directly from
/// disk, otherwise the program have to read the current node again to first
/// obtain the NodeRef for the non-cached child node. However a reference count
/// is necessary to prevent NodeRef for non-cached node from staying forever in
/// the memory.
///
/// Only NodeRef for Delta MPT is implemented.
#[derive(Clone)]
pub struct NodeRefDeltaMPT {
    trie_cache_slot: ActualSlabIndex,
}

impl NodeRefDeltaMPT {
    pub const NULL_SLOT: ActualSlabIndex = MaybeNodeRef::MAX_VALUE;

    pub fn to_slot(&self) -> Option<ActualSlabIndex> {
        if self.trie_cache_slot == Self::NULL_SLOT {
            None
        } else {
            Some(self.trie_cache_slot)
        }
    }
}

impl Default for NodeRefDeltaMPT {
    fn default() -> Self {
        Self {
            trie_cache_slot: Self::NULL_SLOT,
        }
    }
}

/// Generally, the db key of MPT node is the merkle hash, however it consumes
/// too much memory. For Delta MPT, the total number of nodes at 1000tps are
/// relative small compared to memory consumed by Cached TrieNodes, and we don't
/// need to persist, therefore we could use "row number" as db key.
pub type DeltaMptDbKey = RowNumberUnderlyingType;

pub struct NodeRefMapDeltaMPT {
    /// Only in unusual situation base_row_number could go higher than 0.
    base_row_number: DeltaMptDbKey,
    map: Vec<NodeRefDeltaMPT>,
}

impl NodeRefMapDeltaMPT {
    /// We allow at most 200M (most recent) nodes for cache of delta trie.
    /// Assuming 2h lifetime for Delta MPT it's around 27k new node per second.
    const MAX_CAPACITY: DeltaMptDbKey = 200_000_000;
}

impl Default for NodeRefMapDeltaMPT {
    fn default() -> Self {
        let mut default = Self {
            base_row_number: Default::default(),
            map: Default::default(),
        };

        default
            .map
            .reserve(NodeMemoryManager::START_CAPACITY as usize);

        default
    }
}

// Type alias for clarity.
pub trait NodeRefMapTrait {
    type StorageAccessKey;
    type NodeRef;
    type MaybeCacheSlotIndex;
}

impl NodeRefMapTrait for NodeRefMapDeltaMPT {
    type MaybeCacheSlotIndex = ActualSlabIndex;
    type NodeRef = NodeRefDeltaMPT;
    type StorageAccessKey = DeltaMptDbKey;
}

impl NodeRefMapDeltaMPT {
    unsafe fn get_unchecked(&self, key: DeltaMptDbKey) -> &NodeRefDeltaMPT {
        self.map.get_unchecked(Self::key_to_subscription(key))
    }

    fn key_to_subscription(key: DeltaMptDbKey) -> usize {
        (key % Self::MAX_CAPACITY) as usize
    }

    fn reset(
        &mut self, key: DeltaMptDbKey, node_memory_manager: &NodeMemoryManager,
        cache_algorithm: &mut CacheAlgorithmDeltaMPT,
    )
    {
        let maybe_slot = unsafe { self.delete(key) };
        if maybe_slot.is_none() {
            return;
        }
        let slot = maybe_slot.unwrap();

        unsafe {
            node_memory_manager.delete_from_cache(
                cache_algorithm,
                self,
                key,
                slot,
            );
            *self.map.get_unchecked_mut(Self::key_to_subscription(key)) =
                NodeRefDeltaMPT::default();
        }
    }

    pub fn insert(
        &mut self, key: DeltaMptDbKey, slot: ActualSlabIndex,
        node_memory_manager: &NodeMemoryManager,
        cache_algorithm: &mut CacheAlgorithmDeltaMPT,
    ) -> Option<NodeRefDeltaMPT>
    {
        if key < self.base_row_number {
            return None;
        }
        let size = self.map.len() as RowNumberUnderlyingType;
        if key >= self.base_row_number + Self::MAX_CAPACITY {
            let new_row_number = key + 1 - Self::MAX_CAPACITY;
            let mut base_row_number = self.base_row_number;
            loop {
                self.reset(
                    base_row_number,
                    node_memory_manager,
                    cache_algorithm,
                );
                base_row_number += 1;
                if base_row_number == new_row_number {
                    break;
                }
            }
            self.base_row_number = base_row_number;
        } else if size < Self::MAX_CAPACITY && key >= size {
            let new_len = (if Self::MAX_CAPACITY / 2 > key {
                (key + 1) * 2
            } else {
                Self::MAX_CAPACITY
            }) as usize;
            self.map.reserve_exact(new_len);
            self.map.resize(new_len, NodeRefDeltaMPT::default());
        }
        self.set_cache_slot(key, slot);
        Some(unsafe { self.get_unchecked(key).clone() })
    }

    pub fn get(&self, key: DeltaMptDbKey) -> Option<NodeRefDeltaMPT> {
        if key >= self.base_row_number
            && key
                < self.base_row_number
                    + self.map.len() as RowNumberUnderlyingType
        {
            Some(unsafe { self.get_unchecked(key).clone() })
        } else {
            None
        }
    }

    fn set_cache_slot(
        &mut self, key: DeltaMptDbKey, slot: ActualSlabIndex,
    ) -> Option<ActualSlabIndex> {
        let node_ref = unsafe {
            self.map.get_unchecked_mut(Self::key_to_subscription(key))
        };
        let old_slot = node_ref.to_slot();
        node_ref.trie_cache_slot = slot;

        old_slot
    }

    pub unsafe fn delete(
        &mut self, key: DeltaMptDbKey,
    ) -> Option<ActualSlabIndex> {
        self.set_cache_slot(key, NodeRefDeltaMPT::NULL_SLOT)
    }
}
