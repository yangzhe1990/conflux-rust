use super::{
    lru::{LRUHandle, LRU},
    removable_heap::{HeapHandle, HeapValueUtil, Hole, RemovableHeap},
    CacheAccessResult, CacheAlgoDataAdapter, CacheAlgoDataTrait,
    CacheAlgorithm, CacheIndexTrait, CacheStoreUtil, MyInto, PrimitiveNum,
};
use std::{hint, mem};

/// In LFRU we keep an LRU to maintain frequency for alpha * cache_slots
/// recently visited elements. When inserting the most recent element, evict the
/// least frequent element from LFU, and insert the element with frequency
/// maintained in LRU. As long as an element stays in LRU the frequency
/// doesn't restart from 0.
///
/// The double link list algorithm for LFU can not extend if an element starts
/// from frequency greater than 0, and another downside is using much more
/// memory. We use heap to maintain frequency.
pub struct LFRU<PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait> {
    size: PosT,
    capacity: PosT,
    frequency_lru: LRU<PosT, LFRUHandle<PosT>>,
    frequency_heap: RemovableHeap<PosT, LFRUMetadata<PosT, CacheIndexT>>,
}

type FrequencyType = u16;
const MAX_VISIT_COUNT: FrequencyType = ::std::u16::MAX;

/// LFRUHandle points to the location where frequency data is stored. A non-null
/// pos means that the object is maintained in LRU.
#[derive(Clone, Copy)]
pub struct LFRUHandle<PosT: PrimitiveNum> {
    pos: PosT,
}

impl<PosT: PrimitiveNum> CacheAlgoDataTrait for LFRUHandle<PosT> {}

impl<PosT: PrimitiveNum> LFRUHandle<PosT> {
    const NULL_POS: i32 = -1;

    fn placement_new_handle(&mut self, pos: PosT) { self.set_handle(pos); }

    fn placement_new_evicted(&mut self) { self.set_evicted(); }

    fn is_lru_hit(&self) -> bool { self.pos != PosT::from(Self::NULL_POS) }

    fn is_lfu_hit<CacheIndexT: CacheIndexTrait>(
        &self, heap: &RemovableHeap<PosT, LFRUMetadata<PosT, CacheIndexT>>,
    ) -> bool {
        self.pos < heap.get_heap_size()
            && self.pos != PosT::from(Self::NULL_POS)
    }

    fn set_evicted(&mut self) { self.pos = PosT::from(Self::NULL_POS); }

    fn get_handle(&self) -> PosT { self.pos }

    fn set_handle(&mut self, pos: PosT) { self.pos = pos; }
}

impl<PosT: PrimitiveNum> Default for LFRUHandle<PosT> {
    fn default() -> Self {
        Self {
            pos: PosT::from(Self::NULL_POS),
        }
    }
}

impl<PosT: PrimitiveNum> CacheIndexTrait for LFRUHandle<PosT> {}

struct LFRUMetadata<PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait> {
    frequency: FrequencyType,
    lru_handle: LRUHandle<PosT>,
    cache_index: CacheIndexT,
}

impl<PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait>
    LFRUMetadata<PosT, CacheIndexT>
{
    fn inc_visit_counter(&mut self) {
        if self.frequency != MAX_VISIT_COUNT {
            self.frequency += 1
        }
    }
}

struct LFRUMetadataHeapUtil<
    'a: 'b,
    'b,
    PosT: 'a + PrimitiveNum,
    CacheIndexT: CacheIndexTrait,
    CacheStoreUtilT: 'b
        + CacheStoreUtil<
            CacheAlgoData = LFRUHandle<PosT>,
            ElementIndex = CacheIndexT,
        >,
> {
    frequency_lru: &'a mut LRU<PosT, LFRUHandle<PosT>>,
    cache_store_util: &'b mut CacheStoreUtilT,
}

impl<
        'a,
        'b,
        PosT: PrimitiveNum,
        CacheIndexT: CacheIndexTrait,
        CacheStoreUtilT: CacheStoreUtil<
            CacheAlgoData = LFRUHandle<PosT>,
            ElementIndex = CacheIndexT,
        >,
    > HeapValueUtil<LFRUMetadata<PosT, CacheIndexT>, PosT>
    for LFRUMetadataHeapUtil<'a, 'b, PosT, CacheIndexT, CacheStoreUtilT>
{
    type KeyType = FrequencyType;

    fn set_heap_handle(
        &mut self, value: &mut LFRUMetadata<PosT, CacheIndexT>, pos: PosT,
    ) {
        unsafe {
            self.frequency_lru
                .get_cache_index_mut(value.lru_handle)
                .set_handle(pos);
            CacheAlgoDataAdapter::new_mut(
                self.cache_store_util,
                value.cache_index,
            )
            .placement_new_handle(pos);
        }
    }

    fn set_heap_handle_final(
        &mut self, value: &mut LFRUMetadata<PosT, CacheIndexT>, pos: PosT,
    ) {
        unsafe {
            self.frequency_lru
                .get_cache_index_mut(value.lru_handle)
                .set_handle(pos);
            CacheAlgoDataAdapter::new_mut_most_recently_accessed(
                self.cache_store_util,
                value.cache_index,
            )
            .placement_new_handle(pos);
        }
    }

    fn set_heap_removed(
        &mut self, value: &mut LFRUMetadata<PosT, CacheIndexT>,
    ) {
        unsafe {
            self.frequency_lru
                .get_cache_index_mut(value.lru_handle)
                .set_handle(PosT::from(HeapHandle::<PosT>::NULL_POS));
            CacheAlgoDataAdapter::new_mut(
                self.cache_store_util,
                value.cache_index,
            )
            .placement_new_evicted();
        }
    }

    fn get_key_for_comparison<'v>(
        &self, value: &'v LFRUMetadata<PosT, CacheIndexT>,
    ) -> &'v Self::KeyType {
        &value.frequency
    }
}

type CacheStoreUtilLRUHit<'a, PosT, CacheIndexT> =
    &'a mut Vec<LFRUMetadata<PosT, CacheIndexT>>;

impl<'a, PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait> CacheStoreUtil
    for CacheStoreUtilLRUHit<'a, PosT, CacheIndexT>
{
    type CacheAlgoData = LRUHandle<PosT>;
    type ElementIndex = LFRUHandle<PosT>;

    fn get(&self, element_index: Self::ElementIndex) -> LRUHandle<PosT> {
        self[MyInto::<usize>::into(element_index.get_handle())].lru_handle
    }

    fn set(
        &mut self, element_index: Self::ElementIndex,
        algo_data: &LRUHandle<PosT>,
    )
    {
        self[MyInto::<usize>::into(element_index.get_handle())].lru_handle =
            *algo_data
    }
}

// TODO: in rust 2018, it's not necessary to write Type: 'a.
struct CacheStoreUtilLRUMiss<
    'a,
    'b,
    PosT: PrimitiveNum + 'a + 'b,
    CacheIndexT: CacheIndexTrait + 'a + 'b,
> {
    metadata: CacheStoreUtilLRUHit<'a, PosT, CacheIndexT>,
    new_metadata: &'b mut LFRUMetadata<PosT, CacheIndexT>,
}

impl<'a, 'b, PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait>
    CacheStoreUtilLRUMiss<'a, 'b, PosT, CacheIndexT>
{
    fn new(
        lfru_metadata: CacheStoreUtilLRUHit<'a, PosT, CacheIndexT>,
        cache_index: CacheIndexT,
        new_metadata: &'b mut LFRUMetadata<PosT, CacheIndexT>,
    ) -> Self
    {
        *new_metadata = LFRUMetadata::<PosT, CacheIndexT> {
            frequency: 1,
            lru_handle: Default::default(),
            cache_index: cache_index,
        };

        Self {
            new_metadata: new_metadata,
            metadata: lfru_metadata,
        }
    }
}

impl<'a, 'b, PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait> CacheStoreUtil
    for CacheStoreUtilLRUMiss<'a, 'b, PosT, CacheIndexT>
{
    type CacheAlgoData = LRUHandle<PosT>;
    type ElementIndex = LFRUHandle<PosT>;

    fn get(&self, element_index: Self::ElementIndex) -> LRUHandle<PosT> {
        self.metadata.get(element_index)
    }

    fn get_most_recently_accessed(
        &self, element_index: Self::ElementIndex,
    ) -> LRUHandle<PosT> {
        self.new_metadata.lru_handle
    }

    fn set(
        &mut self, element_index: Self::ElementIndex,
        algo_data: &LRUHandle<PosT>,
    )
    {
        self.metadata.set(element_index, algo_data);
    }

    fn set_most_recently_accessed(
        &mut self, element_index: <Self as CacheStoreUtil>::ElementIndex,
        algo_data: &LRUHandle<PosT>,
    )
    {
        self.new_metadata.lru_handle = *algo_data;
    }
}

impl<PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait>
    CacheAlgorithm<PosT, CacheIndexT> for LFRU<PosT, CacheIndexT>
{
    type CacheAlgoData = LFRUHandle<PosT>;

    fn access<
        CacheStoreUtilT: CacheStoreUtil<
            ElementIndex = CacheIndexT,
            CacheAlgoData = LFRUHandle<PosT>,
        >,
    >(
        &mut self, cache_index: CacheIndexT,
        cache_store_util: &mut CacheStoreUtilT,
    ) -> CacheAccessResult<CacheIndexT>
    {
        let lfru_handle =
            cache_store_util.get_most_recently_accessed(cache_index);
        let is_lru_hit = lfru_handle.is_lru_hit();

        if is_lru_hit {
            self.frequency_lru
                .access(lfru_handle, &mut self.frequency_heap.get_array_mut());

            // Increase LFU visit counter.
            unsafe {
                self.frequency_heap
                    .get_unchecked_mut(lfru_handle.get_handle())
                    .inc_visit_counter()
            };

            let has_space = self.has_space();
            let (heap, mut heap_util) =
                self.heap_and_heap_util(cache_store_util);

            if lfru_handle.is_lfu_hit(&heap) {
                heap.sift_down(lfru_handle.get_handle(), &mut heap_util);
                CacheAccessResult::Hit
            } else {
                // Hit in LRU but not in LFU. The heap may not be full because
                // of deletion.
                unsafe {
                    let lfru_metadata_ptr = heap
                        .get_unchecked_mut(lfru_handle.get_handle())
                        as *mut LFRUMetadata<PosT, CacheIndexT>;

                    let mut hole = Hole::new(lfru_metadata_ptr);

                    if has_space {
                        let heap_size = heap.get_heap_size();
                        hole.move_to(
                            heap.get_unchecked_mut(heap_size),
                            heap_size,
                            &mut heap_util,
                        );
                        heap.set_heap_size_unchecked(heap_size + PosT::from(1));

                        heap.sift_up_with_hole(heap_size, hole, &mut heap_util);

                        CacheAccessResult::MissInsert
                    } else {
                        hole.move_to(
                            heap.get_unchecked_mut(PosT::from(0)),
                            PosT::from(0),
                            &mut heap_util,
                        );
                        heap.sift_down_with_hole(
                            PosT::from(0),
                            hole,
                            &mut heap_util,
                        );
                        CacheAccessResult::MissReplaced {
                            evicted: (*lfru_metadata_ptr).cache_index,
                        }
                    }
                }
            }
        } else {
            // lfru_handle equals NULL_POS.
            if self.frequency_lru.has_space() {
                let mut hole: Hole<LFRUMetadata<PosT, CacheIndexT>> =
                    unsafe { mem::uninitialized() };
                {
                    let mut lru_cache_store_util = CacheStoreUtilLRUMiss::new(
                        self.frequency_heap.get_array_mut(),
                        cache_index,
                        &mut hole.value,
                    );

                    self.frequency_lru
                        .access(lfru_handle, &mut lru_cache_store_util);
                }

                let has_space = self.has_space();
                let (heap, mut heap_util) =
                    self.heap_and_heap_util(cache_store_util);
                if has_space {
                    unsafe {
                        heap.insert_with_hole_unchecked(hole, &mut heap_util)
                    };

                    CacheAccessResult::MissInsert
                } else {
                    let pos = unsafe {
                        heap.hole_push_back_and_swap_unchecked(
                            PosT::from(0),
                            &mut hole,
                        )
                    };
                    heap.sift_down_with_hole(
                        PosT::from(0),
                        hole,
                        &mut heap_util,
                    );

                    CacheAccessResult::MissReplaced {
                        evicted: unsafe {
                            heap.get_unchecked_mut(pos).cache_index
                        },
                    }
                }
            } else {
                let mut hole: Hole<LFRUMetadata<PosT, CacheIndexT>> =
                    unsafe { mem::uninitialized() };
                let lru_access_result;
                {
                    let mut lru_cache_store_util = CacheStoreUtilLRUMiss::new(
                        self.frequency_heap.get_array_mut(),
                        cache_index,
                        &mut hole.value,
                    );
                    lru_access_result = self
                        .frequency_lru
                        .access(lfru_handle, &mut lru_cache_store_util);
                }

                let (heap, mut heap_util) =
                    self.heap_and_heap_util(cache_store_util);

                match lru_access_result {
                    CacheAccessResult::MissReplaced {
                        evicted: lru_evicted,
                    } => {
                        let evicted_lfru_metadata_ptr = unsafe {
                            heap.get_unchecked_mut(lru_evicted.pos)
                                as *mut LFRUMetadata<PosT, CacheIndexT>
                        };
                        hole.pointer_pos = evicted_lfru_metadata_ptr;

                        if lru_evicted.is_lfu_hit(heap) {
                            // The element removed from LRU also lives in LFU.
                            // Replace it with the newly accessed item.

                            unsafe {
                                heap.replace_at_unchecked_with_hole(
                                    lru_evicted.pos,
                                    hole,
                                    &mut *evicted_lfru_metadata_ptr,
                                    &mut heap_util,
                                )
                            };
                        } else {
                            // The element removed from LRU lives outside LFU.
                            // Replace the least frequently visited with the
                            // newly accessed item.

                            hole.move_to(
                                unsafe {
                                    heap.get_unchecked_mut(PosT::from(0))
                                },
                                PosT::from(0),
                                &mut heap_util,
                            );

                            heap.sift_down_with_hole(
                                PosT::from(0),
                                hole,
                                &mut heap_util,
                            );
                        }
                        CacheAccessResult::MissReplaced {
                            evicted: unsafe {
                                (*evicted_lfru_metadata_ptr).cache_index
                            },
                        }
                    }
                    _ => unsafe { hint::unreachable_unchecked() },
                }
            }
        }
    }

    fn delete<
        CacheStoreUtilT: CacheStoreUtil<
            ElementIndex = CacheIndexT,
            CacheAlgoData = LFRUHandle<PosT>,
        >,
    >(
        &mut self, cache_index: CacheIndexT,
        cache_store_util: &mut CacheStoreUtilT,
    )
    {
        let lfru_handle =
            cache_store_util.get_most_recently_accessed(cache_index);
        self.frequency_lru
            .delete(lfru_handle, &mut self.frequency_heap.get_array_mut());
        // Remove from heap.
        let (heap, mut heap_util) = self.heap_and_heap_util(cache_store_util);
        unsafe {
            heap.remove_at_unchecked(lfru_handle.get_handle(), &mut heap_util);
        }
    }
}

impl<PosT: PrimitiveNum, CacheIndexT: CacheIndexTrait> LFRU<PosT, CacheIndexT> {
    pub fn new(capacity: PosT, lru_capacity: PosT) -> Self {
        Self {
            size: PosT::from(0),
            capacity: capacity,
            frequency_heap: RemovableHeap::new(lru_capacity),
            frequency_lru: LRU::new(lru_capacity),
        }
    }

    fn heap_and_heap_util<
        'a,
        'b,
        CacheStoreUtilT: CacheStoreUtil<
            ElementIndex = CacheIndexT,
            CacheAlgoData = LFRUHandle<PosT>,
        >,
    >(
        &'a mut self, cache_store_util: &'b mut CacheStoreUtilT,
    ) -> (
        &mut RemovableHeap<PosT, LFRUMetadata<PosT, CacheIndexT>>,
        LFRUMetadataHeapUtil<'a, 'b, PosT, CacheIndexT, CacheStoreUtilT>,
    ) {
        (
            &mut self.frequency_heap,
            LFRUMetadataHeapUtil::<PosT, CacheIndexT, CacheStoreUtilT> {
                frequency_lru: &mut self.frequency_lru,
                cache_store_util: cache_store_util,
            },
        )
    }

    pub fn has_space(&self) -> bool { self.capacity != self.size }

    pub fn is_full(&self) -> bool { self.capacity == self.size }
}
