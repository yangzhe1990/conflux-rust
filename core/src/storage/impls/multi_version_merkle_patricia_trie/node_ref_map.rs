use super::{
    cache::algorithm::{CacheAlgoDataTrait, CacheAlgorithm},
    node_memory_manager::*,
    row_number::RowNumberUnderlyingType,
};
use std::{marker::PhantomData, vec::Vec};

#[derive(Clone)]
pub enum TrieCacheSlotOrCacheAlgoData<CacheAlgoDataT: CacheAlgoDataTrait> {
    TrieCacheSlot(ActualSlabIndex),
    CacheAlgoData(CacheAlgoDataT),
}

// TODO(yz): Rename class and explain how this class interact with the lifecycle
// of trie node.
/// CacheableNodeRef maintains the information of cached node and possibly
/// non-cached children of cached node.
///
/// Only CacheableNodeRef for Delta MPT is currently implemented.
///
/// CacheableNodeRef for persistent MPT may add a field for storage access key,
/// and a reference count. For persistent MPT, NodeRef (storage access key) for
/// non-cached node might be kept, to be able to read child node directly from
/// disk, otherwise the program have to read the current node again to first
/// obtain the CacheableNodeRef for the non-cached child node. However a
/// reference count is necessary to prevent NodeRef for non-cached node from
/// staying forever in the memory.
#[derive(Clone)]
pub struct CacheableNodeRefDeltaMpt<CacheAlgoDataT: CacheAlgoDataTrait> {
    cached: TrieCacheSlotOrCacheAlgoData<CacheAlgoDataT>,
}

impl<CacheAlgoDataT: CacheAlgoDataTrait>
    CacheableNodeRefDeltaMpt<CacheAlgoDataT>
{
    pub fn new(cached: TrieCacheSlotOrCacheAlgoData<CacheAlgoDataT>) -> Self {
        Self { cached: cached }
    }

    pub fn get_cache_info(
        &self,
    ) -> &TrieCacheSlotOrCacheAlgoData<CacheAlgoDataT> {
        &self.cached
    }

    pub fn get_slot(&self) -> Option<ActualSlabIndex> {
        match self.cached {
            TrieCacheSlotOrCacheAlgoData::CacheAlgoData(_) => None,
            TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot) => Some(slot),
        }
    }
}

/// Generally, the db key of MPT node is the merkle hash, however it consumes
/// too much memory. For Delta MPT, the total number of nodes at 1000tps are
/// relative small compared to memory consumed by Cached TrieNodes, and we don't
/// need to persist, therefore we could use "row number" as db key.
pub type DeltaMptDbKey = RowNumberUnderlyingType;

pub struct NodeRefMapDeltaMpt<
    CacheAlgoDataT: CacheAlgoDataTrait,
    CacheAlgorithmT: CacheAlgorithm<CacheAlgoData = CacheAlgoDataT, CacheIndex = DeltaMptDbKey>,
> {
    /// Only in unusual situation base_row_number could go higher than 0.
    base_row_number: DeltaMptDbKey,
    map: Vec<Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>>,
    __marker_algorithm: PhantomData<CacheAlgorithmT>,
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>
{
    /// We allow at most 200M (most recent) nodes for cache of delta trie.
    /// Assuming 2h lifetime for Delta MPT it's around 27k new node per second.
    const MAX_CAPACITY: DeltaMptDbKey = 200_000_000;

    pub fn new(start_size: usize) -> Self {
        let mut mpt = Self {
            // Explicitly specify one item so that only the fields are default
            // initialized.
            map: Default::default(),
            base_row_number: Default::default(),
            __marker_algorithm: Default::default(),
        };

        mpt.map.reserve(start_size);

        mpt
    }
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > Default for NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>
{
    fn default() -> Self {
        Self::new(
            NodeMemoryManager::<CacheAlgoDataT, CacheAlgorithmT>::START_CAPACITY
                as usize,
        )
    }
}

// Type alias for clarity.
pub trait NodeRefMapTrait {
    type StorageAccessKey;
    type NodeRef;
    type MaybeCacheSlotIndex;
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > NodeRefMapTrait for NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>
{
    type MaybeCacheSlotIndex = ActualSlabIndex;
    type NodeRef = CacheableNodeRefDeltaMpt<CacheAlgoDataT>;
    type StorageAccessKey = DeltaMptDbKey;
}

impl<
        CacheAlgoDataT: CacheAlgoDataTrait,
        CacheAlgorithmT: CacheAlgorithm<
            CacheAlgoData = CacheAlgoDataT,
            CacheIndex = DeltaMptDbKey,
        >,
    > NodeRefMapDeltaMpt<CacheAlgoDataT, CacheAlgorithmT>
{
    unsafe fn get_unchecked(
        &self, key: DeltaMptDbKey,
    ) -> &Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>> {
        self.map.get_unchecked(Self::key_to_subscription(key))
    }

    fn key_to_subscription(key: DeltaMptDbKey) -> usize {
        (key % Self::MAX_CAPACITY) as usize
    }

    fn reset(
        &mut self, key: DeltaMptDbKey,
        node_memory_manager: &NodeMemoryManager<
            CacheAlgoDataT,
            CacheAlgorithmT,
        >,
        cache_algorithm: &mut CacheAlgorithmT,
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
            *self.map.get_unchecked_mut(Self::key_to_subscription(key)) = None;
        }
    }

    pub fn insert(
        &mut self, key: DeltaMptDbKey, slot: ActualSlabIndex,
        node_memory_manager: &NodeMemoryManager<
            CacheAlgoDataT,
            CacheAlgorithmT,
        >,
        cache_algorithm: &mut CacheAlgorithmT,
    ) -> Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>
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
            self.map.resize(new_len, None);
        }
        self.set_cache_info(
            key,
            Some(CacheableNodeRefDeltaMpt::new(
                TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot),
            )),
        );
        unsafe { self.get_unchecked(key).clone() }
    }

    pub fn get(
        &self, key: DeltaMptDbKey,
    ) -> Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>> {
        if key >= self.base_row_number
            && key
                < self.base_row_number
                    + self.map.len() as RowNumberUnderlyingType
        {
            unsafe { self.get_unchecked(key).clone() }
        } else {
            None
        }
    }

    pub fn set_cache_info(
        &mut self, key: DeltaMptDbKey,
        cache_info: Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>,
    ) -> Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>
    {
        let node_ref = unsafe {
            self.map.get_unchecked_mut(Self::key_to_subscription(key))
        };
        let old_slot = node_ref.clone();
        *node_ref = cache_info;

        old_slot
    }

    pub unsafe fn delete(
        &mut self, key: DeltaMptDbKey,
    ) -> Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>> {
        self.set_cache_info(key, None)
    }
}
