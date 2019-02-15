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

    pub fn get_slot(&self) -> Option<&ActualSlabIndex> {
        match &self.cached {
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

    pub fn new(size: DeltaMptDbKey) -> Self {
        Self {
            // Explicitly specify one item so that only the fields are default
            // initialized.
            map: vec![None; size as usize],
            base_row_number: Default::default(),
            __marker_algorithm: Default::default(),
        }
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
    fn default() -> Self { Self::new(Self::MAX_CAPACITY) }
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
        self.map.get_unchecked(self.key_to_subscription(key))
    }

    unsafe fn get_unchecked_mut(
        &mut self, key: DeltaMptDbKey,
    ) -> &mut Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>> {
        let offset = self.key_to_subscription(key);
        self.map.get_unchecked_mut(offset)
    }

    fn key_to_subscription(&self, key: DeltaMptDbKey) -> usize {
        (key as usize) % self.map.len()
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
        let maybe_cache_info = self.get(key);
        if maybe_cache_info.is_none() {
            return;
        }
        let cache_info = maybe_cache_info.unwrap().clone();

        unsafe {
            node_memory_manager.delete_from_cache(
                cache_algorithm,
                self,
                key,
                cache_info,
            );
            *self.get_unchecked_mut(key) = None;
        }
    }

    /// Insert may fail due to capacity issue. In this implementation we fail
    /// the insertion for very small(old) db_key. The error is recoverable:
    /// caller should skip calling cache access and ignore the error.
    pub fn insert(
        &mut self, key: DeltaMptDbKey, slot: ActualSlabIndex,
        node_memory_manager: &NodeMemoryManager<
            CacheAlgoDataT,
            CacheAlgorithmT,
        >,
        cache_algorithm: &mut CacheAlgorithmT,
    ) -> Result<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>
    {
        if key < self.base_row_number {
            bail!(ErrorKind::TooOldToCache);
        }
        let size = self.map.len() as RowNumberUnderlyingType;
        if key >= self.base_row_number + size {
            // The final state is that the key is the maximum in the data
            // structure so that the number of deleted items are minimized.
            let new_row_number = key + 1 - size;
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
        }

        self.set_cache_info(
            key,
            Some(CacheableNodeRefDeltaMpt::new(
                TrieCacheSlotOrCacheAlgoData::TrieCacheSlot(slot),
            )),
        );
        Ok(unsafe { self.get_unchecked(key) }.clone().unwrap())
    }

    /// The cache_info is only valid when the element still lives in cache.
    /// Therefore we return the reference to the cache_info to represent the
    /// lifetime requirement.
    pub fn get(
        &self, key: DeltaMptDbKey,
    ) -> Option<&CacheableNodeRefDeltaMpt<CacheAlgoDataT>> {
        if key >= self.base_row_number
            && key
                < self.base_row_number
                    + self.map.len() as RowNumberUnderlyingType
        {
            unsafe { self.get_unchecked(key).as_ref() }
        } else {
            None
        }
    }

    pub fn set_cache_info(
        &mut self, key: DeltaMptDbKey,
        cache_info: Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>,
    ) -> Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>>
    {
        let node_ref = unsafe { self.get_unchecked_mut(key) };
        let old_slot = node_ref.clone();
        *node_ref = cache_info;

        old_slot
    }

    pub fn delete(
        &mut self, key: DeltaMptDbKey,
    ) -> Option<CacheableNodeRefDeltaMpt<CacheAlgoDataT>> {
        self.set_cache_info(key, None)
    }
}

use super::{
    super::errors::*,
    cache::algorithm::{CacheAlgoDataTrait, CacheAlgorithm},
    node_memory_manager::*,
    row_number::RowNumberUnderlyingType,
};
use std::{marker::PhantomData, vec::Vec};
