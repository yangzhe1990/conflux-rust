use super::{super::removable_heap::*, *};
use rand::Rng;
use std::mem;

fn initialize_heap(
    capacity: u32, non_heap_size: u32,
) -> RemovableHeap<u32, TrivialValueWithHeapHandle<i32, u32>> {
    let mut rng = get_rng_for_test();
    let mut heap =
        RemovableHeap::<u32, TrivialValueWithHeapHandle<i32, u32>>::new(
            capacity,
        );
    let mut heap_util = TrivialHeapValueUtil::default();
    for i in 0..non_heap_size {
        let mut hole: Hole<TrivialValueWithHeapHandle<i32, u32>> =
            unsafe { mem::uninitialized() };
        hole.value = TrivialValueWithHeapHandle::<i32, u32>::new(
            rng.gen_range(-100, -1),
        );
        unsafe {
            heap.hole_push_back_and_swap_unchecked(0, &mut hole, &mut heap_util)
        };
        hole.finalize(0, &mut heap_util);
    }

    for i in non_heap_size..capacity {
        heap.insert(
            TrivialValueWithHeapHandle::<i32, u32>::new(
                rng.gen_range(0, 1000000),
            ),
            &mut heap_util,
        )
        .unwrap();
    }

    heap
}

fn check_and_sort_heap(
    heap: &mut RemovableHeap<u32, TrivialValueWithHeapHandle<i32, u32>>,
    capacity: u32, non_heap_size: u32,
)
{
    let mut heap_util = TrivialHeapValueUtil::default();
    for i in 0..capacity {
        assert_eq!(
            i,
            unsafe { heap.get_unchecked_mut(i) }
                .get_handle_mut()
                .get_pos()
        );
    }

    let value = heap.pop_head(&mut heap_util).unwrap();
    assert_eq!(true, value.value >= 0);
    for i in 1..capacity - non_heap_size {
        let new_value = heap.pop_head(&mut heap_util).unwrap();
        assert_eq!(true, new_value >= value);
    }
}

#[test]
fn test_sort_with_heap_handle_check() {
    let mut heap = initialize_heap(50000u32, 0u32);
    check_and_sort_heap(&mut heap, 50000u32, 0u32);
    let mut heap = initialize_heap(50000u32, 1u32);
    check_and_sort_heap(&mut heap, 50000u32, 1u32);
    let mut heap = initialize_heap(50000u32, 100u32);
    check_and_sort_heap(&mut heap, 50000u32, 100u32);
}

#[test]
fn test_removal_and_sort_with_heap_handle_check() {}

// TODO: test removal + insertion; test removal(keep in after-heap part)
