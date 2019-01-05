use super::node_ref::NodeRefDeltaMptCompact;
use rlp::*;
use std::{fmt::*, marker::PhantomData, mem, ptr::null_mut, slice};

pub trait NodeRefTrait:
    Clone + Encodable + Decodable + PartialEq + Debug
{
}

/// NodeRefT for delta MPT and persistent MPT can be different.
#[derive(Clone)]
pub struct CompactedChildrenTable<NodeRefT: NodeRefTrait> {
    /// Stores whether each child exists.
    bitmap: u16,
    /// Stores the number of children in children table.
    children_count: u8,
    /// Stores the existing children ordered.
    table_ptr: *mut NodeRefT,
}

pub const CHILDREN_COUNT: usize = 16;

impl NodeRefTrait for NodeRefDeltaMptCompact {}
pub type ChildrenTableDeltaMpt = CompactedChildrenTable<NodeRefDeltaMptCompact>;
pub type ChildrenTableManagedDeltaMpt = ChildrenTable<NodeRefDeltaMptCompact>;

impl<NodeRefT: NodeRefTrait> Default for CompactedChildrenTable<NodeRefT> {
    fn default() -> Self {
        Self {
            bitmap: 0,
            children_count: 0,
            table_ptr: null_mut(),
        }
    }
}

impl<NodeRefT: NodeRefTrait> Debug for CompactedChildrenTable<NodeRefT> {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "ChildrenTable{{ {:?} }}", self.as_ref())
    }
}

impl<NodeRefT: NodeRefTrait> CompactedChildrenTable<NodeRefT> {
    pub fn get_children_count(&self) -> u8 { self.children_count }

    pub fn get_child(&self, index: u8) -> Option<NodeRefT> {
        if Self::has_index(self.bitmap, index) {
            Some(unsafe {
                (*self
                    .table_ptr
                    .offset(Self::lower_bound(self.bitmap, index) as isize))
                .clone()
            })
        } else {
            None
        }
    }

    /// Unsafe because child must already exist at the index.
    pub unsafe fn set_child_unchecked(&mut self, index: u8, value: NodeRefT) {
        (*self
            .table_ptr
            .offset(Self::lower_bound(self.bitmap, index) as isize)) =
            value.clone();
    }

    pub fn new_from_one_child(index: u8, value: NodeRefT) -> Self {
        Self {
            bitmap: Self::bit(index.into()),
            children_count: 1,
            table_ptr: unsafe {
                Self::managed_slice_into_raw(vec![value].into_boxed_slice())
            },
        }
    }

    pub unsafe fn insert_child_unchecked(
        old_table: ChildrenTableRef<NodeRefT>, index: u8, value: NodeRefT,
    ) -> Self {
        let insertion_pos = Self::lower_bound(old_table.bitmap, index);
        Self {
            bitmap: old_table.bitmap | Self::bit(index.into()),
            children_count: old_table.table.len() as u8 + 1,
            table_ptr: Self::managed_slice_into_raw(
                [
                    &old_table.table[0..insertion_pos],
                    &[value],
                    &old_table.table[insertion_pos..],
                ]
                .concat()
                .into_boxed_slice(),
            ),
        }
    }

    pub unsafe fn delete_child_unchecked(
        old_table: ChildrenTableRef<NodeRefT>, index: u8,
    ) -> Self {
        let deletion_pos = Self::lower_bound(old_table.bitmap, index);
        Self {
            bitmap: old_table.bitmap & (!Self::bit(index.into())),
            children_count: old_table.table.len() as u8 - 1,
            table_ptr: Self::managed_slice_into_raw(
                [
                    &old_table.table[0..deletion_pos],
                    &old_table.table[deletion_pos + 1..],
                ]
                .concat()
                .into_boxed_slice(),
            ),
        }
    }
}

impl<NodeRefT: NodeRefTrait> Drop for CompactedChildrenTable<NodeRefT> {
    fn drop(&mut self) {
        drop(unsafe {
            Vec::from_raw_parts(
                self.table_ptr,
                self.children_count.into(),
                self.children_count.into(),
            )
        });
    }
}

impl<NodeRefT: NodeRefTrait> CompactedChildrenTable<NodeRefT> {
    pub fn into_managed(self) -> ChildrenTable<NodeRefT> {
        ChildrenTable {
            bitmap: self.bitmap,
            table: unsafe {
                Vec::from_raw_parts(
                    self.table_ptr,
                    self.children_count.into(),
                    self.children_count.into(),
                )
                .into_boxed_slice()
            },
        }
    }

    unsafe fn managed_slice_into_raw(
        mut managed: Box<[NodeRefT]>,
    ) -> *mut NodeRefT {
        let ret = managed.as_mut_ptr();

        mem::forget(managed);
        ret
    }

    pub fn from_managed(managed: ChildrenTable<NodeRefT>) -> Self {
        let children_count = managed.table.len() as u8;
        Self {
            bitmap: managed.bitmap,
            table_ptr: unsafe { Self::managed_slice_into_raw(managed.table) },
            children_count: children_count,
        }
    }

    pub fn as_ref(&self) -> ChildrenTableRef<NodeRefT> {
        ChildrenTableRef {
            table: unsafe {
                slice::from_raw_parts(
                    self.table_ptr,
                    self.children_count.into(),
                )
            },
            bitmap: self.bitmap,
        }
    }
}

impl<NodeRefT: NodeRefTrait> CompactedChildrenTable<NodeRefT> {
    fn bit(index: u16) -> u16 { 1 << index }

    fn has_index(bitmap: u16, index: u8) -> bool {
        Self::bit(index.into()) & bitmap != 0
    }

    fn lower_bits(index: u16) -> u16 { (1 << index) - 1 }

    fn all_bits() -> u16 { !0 }

    fn count_bits(bitmap: u16) -> u16 {
        let mut count = (bitmap & 0b0101010101010101)
            + ((bitmap >> 1) & 0b0101010101010101);
        count =
            (count & 0b0011001100110011) + ((count >> 2) & 0b0011001100110011);
        count =
            (count & 0b0000111100001111) + ((count >> 4) & 0b0000111100001111);
        (count & 0b0000000011111111) + (count >> 8)
    }

    fn lowest_bit_at(bitmap: u16) -> u8 {
        Self::count_bits(1 ^ bitmap ^ (bitmap - 1)) as u8
    }

    fn remove_lowest_bit(bitmap: u16) -> u16 { bitmap & (bitmap - 1) }

    fn lower_bound(bitmap: u16, index: u8) -> usize {
        Self::count_bits(bitmap & Self::lower_bits(index.into())).into()
    }
}

impl<NodeRefT: NodeRefTrait> CompactedChildrenTable<NodeRefT> {
    pub fn iter(&self) -> CompactedChildrenTableIterator<NodeRefT> {
        CompactedChildrenTableIterator {
            elements: self.table_ptr,
            bitmap: self.bitmap,
            __marker: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> CompactedChildrenTableIteratorMut<NodeRefT> {
        CompactedChildrenTableIteratorMut {
            elements: self.table_ptr,
            bitmap: self.bitmap,
            __marker: PhantomData,
        }
    }

    pub fn iter_non_skip(
        &self,
    ) -> CompactedChildrenTableIteratorNonSkip<NodeRefT> {
        CompactedChildrenTableIteratorNonSkip {
            next_child_index: 0,
            elements: self.table_ptr,
            bitmap: self.bitmap,
            __marker: PhantomData,
        }
    }

    pub fn iter_non_skip_mut(
        &mut self,
    ) -> CompactedChildrenTableIteratorNonSkipMut<NodeRefT> {
        CompactedChildrenTableIteratorNonSkipMut {
            next_child_index: 0,
            elements: self.table_ptr,
            bitmap: self.bitmap,
            __marker: PhantomData,
        }
    }
}

impl<NodeRefT: NodeRefTrait> PartialEq for CompactedChildrenTable<NodeRefT> {
    fn eq(&self, other: &Self) -> bool { self.as_ref() == other.as_ref() }
}

pub struct CompactedChildrenTableIteratorNonSkip<'a, NodeRefT> {
    next_child_index: u8,
    elements: *const NodeRefT,
    bitmap: u16,
    __marker: PhantomData<&'a NodeRefT>,
}

impl<'a, NodeRefT> CompactedChildrenTableIteratorNonSkip<'a, NodeRefT> {
    fn get_current_element(&self) -> &'a NodeRefT { unsafe { &*self.elements } }

    fn advance_elements(&mut self) {
        unsafe {
            self.elements = self.elements.offset(1);
        }
    }
}

impl<'a, NodeRefT: NodeRefTrait> Iterator
    for CompactedChildrenTableIteratorNonSkip<'a, NodeRefT>
{
    type Item = (u8, Option<&'a NodeRefT>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_child_index as usize == CHILDREN_COUNT {
            return None;
        }

        let ret;
        if CompactedChildrenTable::<NodeRefT>::has_index(
            self.bitmap,
            self.next_child_index,
        ) {
            ret =
                Some((self.next_child_index, Some(self.get_current_element())));
            self.advance_elements();
        } else {
            ret = Some((self.next_child_index, None));
        }
        self.next_child_index += 1;

        ret
    }
}

pub struct CompactedChildrenTableIteratorNonSkipMut<'a, NodeRefT> {
    next_child_index: u8,
    elements: *mut NodeRefT,
    bitmap: u16,
    __marker: PhantomData<&'a mut NodeRefT>,
}

impl<'a, NodeRefT> CompactedChildrenTableIteratorNonSkipMut<'a, NodeRefT> {
    fn get_current_element(&self) -> &'a mut NodeRefT {
        unsafe { &mut *self.elements }
    }

    fn advance_elements(&mut self) {
        unsafe {
            self.elements = self.elements.offset(1);
        }
    }
}

impl<'a, NodeRefT: NodeRefTrait> Iterator
    for CompactedChildrenTableIteratorNonSkipMut<'a, NodeRefT>
{
    type Item = (u8, Option<&'a mut NodeRefT>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_child_index as usize == CHILDREN_COUNT {
            return None;
        }

        let ret;
        if CompactedChildrenTable::<NodeRefT>::has_index(
            self.bitmap,
            self.next_child_index,
        ) {
            ret =
                Some((self.next_child_index, Some(self.get_current_element())));
            self.advance_elements();
        } else {
            ret = Some((self.next_child_index, None));
        }
        self.next_child_index += 1;

        ret
    }
}

pub struct CompactedChildrenTableIterator<'a, NodeRefT> {
    elements: *const NodeRefT,
    bitmap: u16,
    __marker: PhantomData<&'a NodeRefT>,
}

impl<'a, NodeRefT> CompactedChildrenTableIterator<'a, NodeRefT> {
    fn get_current_element(&self) -> &'a NodeRefT { unsafe { &*self.elements } }

    fn advance_elements(&mut self) {
        unsafe {
            self.elements = self.elements.offset(1);
        }
    }
}

// FIXME: try to reuse code for next.

impl<'a, NodeRefT: NodeRefTrait> Iterator
    for CompactedChildrenTableIterator<'a, NodeRefT>
{
    type Item = (u8, &'a NodeRefT);

    fn next(&mut self) -> Option<Self::Item> {
        let ret;
        if self.bitmap != 0 {
            ret = Some((
                CompactedChildrenTable::<NodeRefT>::lowest_bit_at(self.bitmap),
                self.get_current_element(),
            ));
        } else {
            return None;
        }

        self.advance_elements();
        self.bitmap =
            CompactedChildrenTable::<NodeRefT>::remove_lowest_bit(self.bitmap);

        ret
    }
}

pub struct CompactedChildrenTableIteratorMut<'a, NodeRefT> {
    elements: *mut NodeRefT,
    bitmap: u16,
    __marker: PhantomData<&'a mut NodeRefT>,
}

impl<'a, NodeRefT> CompactedChildrenTableIteratorMut<'a, NodeRefT> {
    fn get_current_element(&self) -> &'a mut NodeRefT {
        unsafe { &mut *self.elements }
    }

    fn advance_elements(&mut self) {
        unsafe {
            self.elements = self.elements.offset(1);
        }
    }
}

impl<'a, NodeRefT: NodeRefTrait> Iterator
    for CompactedChildrenTableIteratorMut<'a, NodeRefT>
{
    type Item = (u8, &'a mut NodeRefT);

    fn next(&mut self) -> Option<Self::Item> {
        let ret;
        if self.bitmap != 0 {
            ret = Some((
                CompactedChildrenTable::<NodeRefT>::lowest_bit_at(self.bitmap),
                self.get_current_element(),
            ));
        } else {
            return None;
        }

        self.advance_elements();
        self.bitmap =
            CompactedChildrenTable::<NodeRefT>::remove_lowest_bit(self.bitmap);

        ret
    }
}

pub struct ChildrenTable<NodeRefT: NodeRefTrait> {
    /// Stores the existing children ordered.
    table: Box<[NodeRefT]>,
    /// Stores whether each child exists.
    bitmap: u16,
}

impl<NodeRefT: NodeRefTrait> Default for ChildrenTable<NodeRefT> {
    fn default() -> Self {
        Self {
            table: vec![].into_boxed_slice(),
            bitmap: 0,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct ChildrenTableRef<'a, NodeRefT: NodeRefTrait> {
    table: &'a [NodeRefT],
    bitmap: u16,
}

impl<NodeRefT: NodeRefTrait> From<CompactedChildrenTable<NodeRefT>>
    for ChildrenTable<NodeRefT>
{
    fn from(x: CompactedChildrenTable<NodeRefT>) -> Self { x.into_managed() }
}

impl<NodeRefT: NodeRefTrait> From<ChildrenTable<NodeRefT>>
    for CompactedChildrenTable<NodeRefT>
{
    fn from(x: ChildrenTable<NodeRefT>) -> Self { Self::from_managed(x) }
}

impl<'a, NodeRefT: NodeRefTrait> Encodable for ChildrenTableRef<'a, NodeRefT> {
    fn rlp_append(&self, s: &mut RlpStream) {
        match self.table.len() {
            0 => s.begin_list(0),
            // Skip bitmap if list has length of 16.
            16 => s.append_list(self.table),
            _ => s.begin_list(2).append(&self.bitmap).append_list(self.table),
        };
    }
}

impl<NodeRefT: NodeRefTrait> Decodable for ChildrenTable<NodeRefT> {
    fn decode(rlp: &Rlp) -> ::std::result::Result<Self, DecoderError> {
        let item_count = rlp.item_count()?;
        Ok(match item_count {
            0...1 => Self::default(),
            16 => Self {
                bitmap: !0u16,
                table: rlp.as_list::<NodeRefT>()?.into_boxed_slice(),
            },
            _ => Self {
                bitmap: rlp.val_at::<u16>(0)?,
                table: rlp.list_at::<NodeRefT>(1)?.into_boxed_slice(),
            },
        })
    }
}
