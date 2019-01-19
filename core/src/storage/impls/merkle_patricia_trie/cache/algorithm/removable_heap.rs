use super::{super::super::super::errors::*, MyInto, PrimitiveNum};
use std::{cmp::Ordering, marker::PhantomData, mem, ptr, vec::Vec};

// To make it easy for any value type to implement HeapValueUtil.
pub struct HeapHandle<PosT> {
    pos: PosT,
}

impl<PosT: PrimitiveNum> HeapHandle<PosT> {
    pub const NULL_POS: i32 = -1;

    pub fn get_pos(&self) -> PosT { self.pos }
}

pub trait ValueWithHeapHandle<PosT: PrimitiveNum> {
    type KeyType;

    fn get_handle_mut(&mut self) -> &mut HeapHandle<PosT>;

    // Update heap handle for value being moved in heap.
    fn set_heap_handle(&mut self, pos: PosT) {
        self.get_handle_mut().set_heap_handle(pos);
    }

    fn set_heap_removed(&mut self) { self.get_handle_mut().set_heap_removed(); }
}

impl<PosT: PrimitiveNum> ValueWithHeapHandle<PosT> for HeapHandle<PosT> {
    type KeyType = ();

    fn get_handle_mut(&mut self) -> &mut HeapHandle<PosT> { self }

    // Update heap handle for value being moved in heap.
    fn set_heap_handle(&mut self, pos: PosT) { self.pos = pos; }

    fn set_heap_removed(&mut self) { self.pos = PosT::from(Self::NULL_POS); }
}

impl<PosT: PrimitiveNum> Default for HeapHandle<PosT> {
    fn default() -> Self {
        Self {
            pos: PosT::from(Self::NULL_POS),
        }
    }
}

/// The value util should only be passed for each action. The problem of holding
/// it for the lifetime of the heap is that the "reference" to some data may
/// prevent modification in other part of the system.
pub trait HeapValueUtil<ValueType, PosT: PrimitiveNum> {
    type KeyType: Ord + Clone;

    // Update heap handle for value being moved in heap.
    fn set_heap_handle(&mut self, value: &mut ValueType, pos: PosT);
    // A special one to set the heap handle for the value being changed by Hole.
    // FIXME(yz): check that in all cases when it's called with special
    // cache_util, the hole operates the most-recently-accessed element.
    // Test LFRU.
    fn set_heap_handle_final(&mut self, value: &mut ValueType, pos: PosT);
    fn set_heap_removed(&mut self, value: &mut ValueType);

    fn get_key_for_comparison<'v>(
        &self, value: &'v ValueType,
    ) -> &'v Self::KeyType;
}

pub struct TrivialValueWithHeapHandle<ValueType, PosT: PrimitiveNum> {
    pub value: ValueType,
    handle: HeapHandle<PosT>,
}

pub struct TrivialHeapValueUtil<
    ValueType: ValueWithHeapHandle<PosT>,
    PosT: PrimitiveNum,
> where ValueType::KeyType: Ord + Clone
{
    __marker_pos_t: PhantomData<PosT>,
    __marker_value_type: PhantomData<ValueType>,
}

impl<ValueType, PosT: PrimitiveNum>
    TrivialValueWithHeapHandle<ValueType, PosT>
{
    pub fn new(value: ValueType) -> Self {
        Self {
            value: value,
            handle: unsafe { mem::uninitialized() },
        }
    }
}

impl<ValueType: PartialEq, PosT: PrimitiveNum> PartialEq
    for TrivialValueWithHeapHandle<ValueType, PosT>
{
    fn eq(&self, other: &Self) -> bool { self.value.eq(&other.value) }
}

impl<ValueType: Eq, PosT: PrimitiveNum> Eq
    for TrivialValueWithHeapHandle<ValueType, PosT>
{
}

impl<ValueType: PartialOrd, PosT: PrimitiveNum> PartialOrd
    for TrivialValueWithHeapHandle<ValueType, PosT>
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<ValueType: Ord, PosT: PrimitiveNum> Ord
    for TrivialValueWithHeapHandle<ValueType, PosT>
{
    fn cmp(&self, other: &Self) -> Ordering { self.value.cmp(&other.value) }
}

impl<ValueType, PosT: PrimitiveNum> ValueWithHeapHandle<PosT>
    for TrivialValueWithHeapHandle<ValueType, PosT>
{
    type KeyType = ValueType;

    fn get_handle_mut(&mut self) -> &mut HeapHandle<PosT> { &mut self.handle }
}

impl<ValueType, PosT: PrimitiveNum> AsRef<ValueType>
    for TrivialValueWithHeapHandle<ValueType, PosT>
{
    fn as_ref(&self) -> &ValueType { &self.value }
}

// Derive doesn't work because it unreasonably requires that ValueType is
// default.
impl<PosT: PrimitiveNum, ValueType: ValueWithHeapHandle<PosT>> Default
    for TrivialHeapValueUtil<ValueType, PosT>
where ValueType::KeyType: Ord + Clone
{
    fn default() -> Self {
        Self {
            __marker_pos_t: PhantomData,
            __marker_value_type: PhantomData,
        }
    }
}

impl<
        PosT: PrimitiveNum,
        KeyType: Ord + Clone,
        ValueType: ValueWithHeapHandle<PosT, KeyType = KeyType> + AsRef<KeyType>,
    > HeapValueUtil<ValueType, PosT> for TrivialHeapValueUtil<ValueType, PosT>
{
    type KeyType = KeyType;

    fn set_heap_handle(&mut self, value: &mut ValueType, pos: PosT) {
        value.set_heap_handle(pos);
    }

    fn set_heap_handle_final(&mut self, value: &mut ValueType, pos: PosT) {
        value.set_heap_handle(pos);
    }

    fn set_heap_removed(&mut self, value: &mut ValueType) {
        value.set_heap_removed();
    }

    fn get_key_for_comparison<'v>(&self, value: &'v ValueType) -> &'v KeyType {
        value.as_ref()
    }
}

pub struct RemovableHeap<PosT: PrimitiveNum, ValueType> {
    /// The array holds the heap. The array may keep more elements after the
    /// heap.
    ///
    /// Typical use of the non-heap part is to hold elements which are removed
    /// from heap but may be maintained and push to heap again.
    array: Vec<ValueType>,
    heap_size: PosT,
}

impl<PosT: PrimitiveNum, ValueType> RemovableHeap<PosT, ValueType> {
    pub fn new(capacity: PosT) -> Self {
        if capacity == PosT::from(HeapHandle::<PosT>::NULL_POS) {
            panic!("LRU: capacity {:?} is too large!", capacity)
        }

        Self {
            array: Vec::with_capacity(capacity.into()),
            heap_size: PosT::from(0),
        }
    }

    pub fn get_heap_size(&self) -> PosT { self.heap_size }

    pub unsafe fn set_heap_size_unchecked(&mut self, size: PosT) {
        self.heap_size = size;
    }

    pub fn get_array_mut(&mut self) -> &mut Vec<ValueType> { &mut self.array }

    pub unsafe fn get_unchecked(&self, pos: PosT) -> &ValueType {
        self.array.get_unchecked(MyInto::<usize>::into(pos))
    }

    pub unsafe fn get_unchecked_mut(&mut self, pos: PosT) -> &mut ValueType {
        self.array.get_unchecked_mut(MyInto::<usize>::into(pos))
    }

    /// Replace an element with hole, and place the removed element at the end
    /// of array (non-heap part).
    ///
    /// Unsafe because pos and capacity are unchecked.
    pub unsafe fn hole_push_back_and_swap_unchecked<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, pos: PosT, hole: &mut Hole<ValueType>,
        value_util: &mut ValueUtilT,
    ) -> PosT
    {
        let array_pos = self.array.len();

        self.array.set_len(array_pos + 1);
        let array_pos = PosT::from(array_pos);
        if pos != array_pos {
            ptr::copy_nonoverlapping(
                self.get_unchecked(pos),
                self.get_unchecked_mut(array_pos),
                1,
            );
            value_util
                .set_heap_handle(self.get_unchecked_mut(array_pos), array_pos);
        }
        hole.pointer_pos = self.get_unchecked_mut(pos);

        array_pos
    }
}

trait OrderChecker<
    ValueType,
    KeyType: Ord + Clone,
    PosT: PrimitiveNum,
    ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
>
{
    // Settle the heap pos for the value and return None if the order is
    // correct, otherwise return an order checker object for further order
    // corrections.
    fn new_checked(
        heap_base: *mut ValueType, pos: PosT, heap_size: PosT,
        key_comparison: KeyType, value_util: &mut ValueUtilT,
    ) -> Option<(Self, *mut ValueType)>
    where
        Self: Sized,
    {
        let mut order_checker =
            Self::new(heap_base, pos, heap_size, key_comparison, value_util);

        if let Some(pointer_parent) = order_checker.calculate_next(value_util) {
            Some((order_checker, pointer_parent))
        } else {
            value_util.set_heap_handle(
                unsafe { &mut *order_checker.pointer_pos() },
                pos,
            );
            None
        }
    }

    fn new(
        heap_base: *mut ValueType, pos: PosT, heap_size: PosT,
        key_comparison: KeyType, value_util: &mut ValueUtilT,
    ) -> Self;

    fn calculate_next(
        &mut self, value_util: &ValueUtilT,
    ) -> Option<*mut ValueType>;

    fn current_pos(&self) -> PosT;

    fn pointer_pos(&self) -> *mut ValueType;
}

struct UpOrderChecker<
    ValueType,
    KeyType: Ord + Clone,
    PosT: PrimitiveNum,
    ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
> {
    key_comparison: KeyType,
    heap_base: *mut ValueType,
    pointer_pos: *mut ValueType,
    pos: PosT,
    _util_marker: PhantomData<ValueUtilT>,
}

impl<
        ValueType,
        KeyType: Ord + Clone,
        PosT: PrimitiveNum,
        ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
    > OrderChecker<ValueType, KeyType, PosT, ValueUtilT>
    for UpOrderChecker<ValueType, KeyType, PosT, ValueUtilT>
{
    fn new(
        heap_base: *mut ValueType, pos: PosT, heap_size: PosT,
        key_comparison: KeyType, value_util: &mut ValueUtilT,
    ) -> Self
    {
        let pointer_pos = unsafe { heap_base.offset(pos.into()) };

        Self {
            key_comparison: key_comparison,
            heap_base: heap_base,
            pointer_pos: pointer_pos,
            pos: pos,
            _util_marker: PhantomData,
        }
    }

    fn calculate_next(
        &mut self, value_util: &ValueUtilT,
    ) -> Option<*mut ValueType> {
        if self.pos == PosT::from(0) {
            None
        } else {
            let parent = (self.pos - PosT::from(1)) / PosT::from(2);
            let pointer_parent =
                unsafe { self.heap_base.offset(parent.into()) };

            if self
                .key_comparison
                .lt(value_util
                    .get_key_for_comparison(unsafe { &*pointer_parent }))
            {
                self.pos = parent;
                self.pointer_pos = pointer_parent;

                Some(pointer_parent)
            } else {
                None
            }
        }
    }

    fn current_pos(&self) -> PosT { self.pos }

    fn pointer_pos(&self) -> *mut ValueType { self.pointer_pos }
}

struct DownOrderChecker<
    ValueType,
    KeyType: Ord + Clone,
    PosT: PrimitiveNum,
    ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
> {
    key_comparison: KeyType,
    heap_base: *mut ValueType,
    pointer_pos: *mut ValueType,
    pos: PosT,
    pos_limit: PosT,
    max_right_child: PosT,
    _util_marker: PhantomData<ValueUtilT>,
}

impl<
        ValueType,
        KeyType: Ord + Clone,
        PosT: PrimitiveNum,
        ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
    > OrderChecker<ValueType, KeyType, PosT, ValueUtilT>
    for DownOrderChecker<ValueType, KeyType, PosT, ValueUtilT>
{
    fn new(
        heap_base: *mut ValueType, pos: PosT, heap_size: PosT,
        key_comparison: KeyType, value_util: &mut ValueUtilT,
    ) -> Self
    {
        let pointer_pos = unsafe { heap_base.offset(pos.into()) };

        Self {
            key_comparison: key_comparison,
            heap_base: heap_base,
            pointer_pos: pointer_pos,
            pos: pos,
            pos_limit: heap_size / PosT::from(2),
            max_right_child: heap_size - PosT::from(1),
            _util_marker: PhantomData,
        }
    }

    fn calculate_next(
        &mut self, value_util: &ValueUtilT,
    ) -> Option<*mut ValueType> {
        if self.pos >= self.pos_limit {
            return None;
        }
        let left_child = self.pos * PosT::from(2) + PosT::from(1);

        let pointer_left_child =
            unsafe { self.heap_base.offset(left_child.into()) };
        let left_child_key_comparison =
            value_util.get_key_for_comparison(unsafe { &*pointer_left_child });

        let mut best_child = left_child;
        let mut pointer_best_child = pointer_left_child;
        let mut best_child_key = left_child_key_comparison;

        if left_child < self.max_right_child {
            let right_child = left_child + PosT::from(1);

            let pointer_right_child =
                unsafe { self.heap_base.offset(right_child.into()) };
            let right_child_key_comparison = value_util
                .get_key_for_comparison(unsafe { &*pointer_right_child });

            if right_child_key_comparison < best_child_key {
                best_child = right_child;
                pointer_best_child = pointer_right_child;
                best_child_key = right_child_key_comparison;
            }
        }

        if best_child_key.lt(&self.key_comparison) {
            self.pos = best_child;
            self.pointer_pos = pointer_best_child;

            Some(pointer_best_child)
        } else {
            None
        }
    }

    fn current_pos(&self) -> PosT { self.pos }

    fn pointer_pos(&self) -> *mut ValueType { self.pointer_pos }
}

pub struct Hole<ValueType> {
    pub pointer_pos: *mut ValueType,
    pub value: ValueType,
}

impl<ValueType> Hole<ValueType> {
    pub fn new(pointer_pos: *mut ValueType) -> Self {
        Self {
            pointer_pos: pointer_pos,
            value: unsafe { ptr::read(pointer_pos) },
        }
    }

    pub fn new_from_value_ptr_read(
        pointer_pos: *mut ValueType, value: &ValueType,
    ) -> Self {
        Self {
            pointer_pos: pointer_pos,
            value: unsafe { ptr::read(value) },
        }
    }

    pub fn finalize<
        PosT: PrimitiveNum,
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        mut self, pos: PosT, value_updater: &mut ValueUtilT,
    ) {
        unsafe {
            value_updater.set_heap_handle_final(&mut self.value, pos);
            ptr::write(self.pointer_pos, self.value);
        };
    }

    pub fn move_to<
        PosT: PrimitiveNum,
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, pointer_new_pos: *mut ValueType, pos: PosT,
        value_updater: &mut ValueUtilT,
    )
    {
        unsafe {
            value_updater.set_heap_handle(&mut *pointer_new_pos, pos);
            ptr::copy_nonoverlapping(pointer_new_pos, self.pointer_pos, 1);
            self.pointer_pos = pointer_new_pos;
        }
    }
}

impl<PosT: PrimitiveNum, ValueType> RemovableHeap<PosT, ValueType> {
    /// Insert an element carried in a hole into heap.
    ///
    /// Unsafe because the capacity isn't checked.
    pub unsafe fn insert_with_hole_unchecked<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, mut hole: Hole<ValueType>, value_util: &mut ValueUtilT,
    ) -> PosT {
        let heap_size = self.heap_size;
        self.hole_push_back_and_swap_unchecked(
            heap_size, &mut hole, value_util,
        );

        self.sift_up_with_hole(heap_size, hole, value_util);
        self.heap_size += PosT::from(1);

        heap_size
    }

    pub fn insert<ValueUtilT: HeapValueUtil<ValueType, PosT>>(
        &mut self, value: ValueType, value_util: &mut ValueUtilT,
    ) -> Result<PosT> {
        if self.array.capacity() == self.array.len() {
            return Err(ErrorKind::OutOfCapacity.into());
        }

        let mut hole: Hole<ValueType> = unsafe { mem::uninitialized() };
        hole.value = value;

        let pos = unsafe { self.insert_with_hole_unchecked(hole, value_util) };

        Ok(pos)
    }

    /// Unsafe because the emptiness is unchecked.
    pub unsafe fn replace_head_unchecked_with_hole<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, hole: Hole<ValueType>, replaced: &mut ValueType,
        value_util: &mut ValueUtilT,
    )
    {
        ptr::copy_nonoverlapping(
            self.get_unchecked_mut(PosT::from(0)),
            replaced,
            1,
        );

        self.sift_down_with_hole(PosT::from(0), hole, value_util);
    }

    /// The value is set to removed from heap. However if user provides a value
    /// from the non-heap part of self.array, user may want the heap handle
    /// to point to the non-heap location.
    ///
    /// Unsafe because the emptiness is unchecked.
    pub unsafe fn replace_head_unchecked<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, value: &mut ValueType, value_util: &mut ValueUtilT,
    ) {
        let hole =
            Hole::new_from_value_ptr_read(self.array.as_mut_ptr(), value);

        self.replace_head_unchecked_with_hole(hole, value, value_util);
        value_util.set_heap_removed(value);
    }

    pub fn pop_head<ValueUtilT: HeapValueUtil<ValueType, PosT>>(
        &mut self, value_util: &mut ValueUtilT,
    ) -> Option<ValueType> {
        if self.heap_size == PosT::from(0) {
            None
        } else {
            unsafe {
                self.heap_size -= PosT::from(1);
                let mut ret = Some(mem::uninitialized());
                let last_element_pos = self.heap_size;
                if self.heap_size == PosT::from(0) {
                    ptr::copy_nonoverlapping(
                        self.get_unchecked_mut(PosT::from(0)),
                        ret.as_mut().unwrap(),
                        1,
                    );
                } else {
                    let hole = Hole::new_from_value_ptr_read(
                        self.get_unchecked_mut(PosT::from(0)),
                        self.get_unchecked_mut(last_element_pos),
                    );
                    self.replace_head_unchecked_with_hole(
                        hole,
                        ret.as_mut().unwrap(),
                        value_util,
                    );
                }

                let new_len = self.array.len() - 1;
                if PosT::from(new_len) != last_element_pos {
                    ptr::copy_nonoverlapping(
                        self.get_unchecked(PosT::from(new_len)),
                        self.get_unchecked_mut(last_element_pos),
                        1,
                    );
                }
                self.array.set_len(new_len);

                value_util.set_heap_removed(ret.as_mut().unwrap());
                ret
            }
        }
    }

    /// Unsafe because pos is unchecked, and because of using of hole.
    pub unsafe fn replace_at_unchecked_with_hole<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, pos: PosT, hole: Hole<ValueType>, replaced: &mut ValueType,
        value_util: &mut ValueUtilT,
    )
    {
        ptr::copy_nonoverlapping(self.get_unchecked_mut(pos), replaced, 1);

        if value_util.get_key_for_comparison(replaced)
            < value_util.get_key_for_comparison(&hole.value)
        {
            self.sift_down_with_hole(pos, hole, value_util);
        } else {
            self.sift_up_with_hole(pos, hole, value_util);
        }
    }

    /// Unsafe because the pos is unchecked.
    pub unsafe fn replace_at_unchecked<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, pos: PosT, value: &mut ValueType,
        value_util: &mut ValueUtilT,
    )
    {
        let hole =
            Hole::new_from_value_ptr_read(self.get_unchecked_mut(pos), value);

        self.replace_at_unchecked_with_hole(pos, hole, value, value_util);
        value_util.set_heap_removed(value);
    }

    /// Unsafe because the pos is unchecked.
    pub unsafe fn remove_at_unchecked<
        ValueUtilT: HeapValueUtil<ValueType, PosT>,
    >(
        &mut self, pos: PosT, value_util: &mut ValueUtilT,
    ) -> ValueType {
        let mut removed = mem::uninitialized();
        let hole_pos = if self.heap_size > pos {
            self.heap_size -= PosT::from(1);
            let last_element_pos = self.heap_size;
            let hole = Hole::new_from_value_ptr_read(
                self.get_unchecked_mut(PosT::from(0)),
                self.get_unchecked_mut(last_element_pos),
            );

            self.replace_at_unchecked_with_hole(
                pos,
                hole,
                &mut removed,
                value_util,
            );
            last_element_pos
        } else {
            let value_to_remove = self.get_unchecked_mut(pos);
            ptr::copy_nonoverlapping(value_to_remove, &mut removed, 1);
            pos
        };
        let new_len = self.array.len() - 1;
        if PosT::from(new_len) != hole_pos {
            ptr::copy_nonoverlapping(
                self.get_unchecked(PosT::from(new_len)),
                self.get_unchecked_mut(hole_pos),
                1,
            );
        }
        self.array.set_len(new_len);

        value_util.set_heap_removed(&mut removed);
        removed
    }

    pub fn sift_up_with_hole<ValueUtilT: HeapValueUtil<ValueType, PosT>>(
        &mut self, pos: PosT, hole: Hole<ValueType>,
        value_util: &mut ValueUtilT,
    )
    {
        let up_order_checker = UpOrderChecker::new(
            self.array.as_mut_ptr(),
            pos,
            self.heap_size,
            value_util.get_key_for_comparison(&hole.value).clone(),
            value_util,
        );
        self.sift_with_hole(pos, hole, up_order_checker, value_util)
    }

    pub fn sift_down_with_hole<ValueUtilT: HeapValueUtil<ValueType, PosT>>(
        &mut self, pos: PosT, hole: Hole<ValueType>,
        value_util: &mut ValueUtilT,
    )
    {
        let down_order_checker = DownOrderChecker::new(
            self.array.as_mut_ptr(),
            pos,
            self.heap_size,
            value_util.get_key_for_comparison(&hole.value).clone(),
            value_util,
        );
        self.sift_with_hole(pos, hole, down_order_checker, value_util)
    }

    fn sift_with_hole<
        KeyType: Ord + Clone,
        OrderCheckerT: OrderChecker<ValueType, KeyType, PosT, ValueUtilT>,
        ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
    >(
        &mut self, mut pos: PosT, mut hole: Hole<ValueType>,
        mut order_checker: OrderCheckerT, value_util: &mut ValueUtilT,
    )
    {
        while let Some(pointer_new_pos) =
            order_checker.calculate_next(value_util)
        {
            hole.move_to(pointer_new_pos, pos, value_util);
            pos = order_checker.current_pos();
        }
        hole.finalize(pos, value_util);
    }

    /// User may call this function when user increased value at pos.
    pub fn sift_down<ValueUtilT: HeapValueUtil<ValueType, PosT>>(
        &mut self, pos: PosT, value_util: &mut ValueUtilT,
    ) -> bool {
        let maybe_order_checker = DownOrderChecker::new_checked(
            self.array.as_mut_ptr(),
            pos,
            self.heap_size,
            unsafe {
                value_util
                    .get_key_for_comparison(self.get_unchecked(pos))
                    .clone()
            },
            value_util,
        );
        self.sift(pos, maybe_order_checker, value_util)
    }

    /// User may call this function when user decreased value at pos.
    pub fn sift_up<ValueUtilT: HeapValueUtil<ValueType, PosT>>(
        &mut self, pos: PosT, value_util: &mut ValueUtilT,
    ) -> bool {
        let maybe_order_checker = UpOrderChecker::new_checked(
            self.array.as_mut_ptr(),
            pos,
            self.heap_size,
            unsafe {
                value_util
                    .get_key_for_comparison(self.get_unchecked(pos))
                    .clone()
            },
            value_util,
        );
        self.sift(pos, maybe_order_checker, value_util)
    }

    fn sift<
        KeyType: Ord + Clone,
        OrderCheckerT: OrderChecker<ValueType, KeyType, PosT, ValueUtilT>,
        ValueUtilT: HeapValueUtil<ValueType, PosT, KeyType = KeyType>,
    >(
        &mut self, pos: PosT,
        maybe_order_checker: Option<(OrderCheckerT, *mut ValueType)>,
        value_util: &mut ValueUtilT,
    ) -> bool
    {
        match maybe_order_checker {
            None => false,
            Some((order_checker, pointer_new_pos)) => {
                // The order checker holds the next position to check.
                let mut hole =
                    Hole::new(unsafe { self.get_unchecked_mut(pos) });
                hole.move_to(pointer_new_pos, pos, value_util);
                self.sift_with_hole(
                    order_checker.current_pos(),
                    hole,
                    order_checker,
                    value_util,
                );
                true
            }
        }
    }
}