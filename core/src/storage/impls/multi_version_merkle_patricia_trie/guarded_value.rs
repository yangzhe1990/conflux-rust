use std::ops::{Deref, DerefMut};

pub struct GuardedValue<GuardType, ValueType> {
    guard: GuardType,
    value: ValueType,
}

impl<GuardType, ValueType> GuardedValue<GuardType, ValueType> {
    pub fn new(guard: GuardType, value: ValueType) -> Self {
        Self {
            guard: guard,
            value: value,
        }
    }

    /// Not yet useful but defined for completeness.
    pub fn new_with_fn<F: FnOnce(&GuardType) -> ValueType>(
        guard: GuardType, f: F,
    ) -> Self {
        unimplemented!()
    }

    pub fn into(self) -> (GuardType, ValueType) { (self.guard, self.value) }
}

impl<GuardType, ValueType: Clone> GuardedValue<GuardType, ValueType> {
    /// There is no guarantee for the validity of value especially when
    /// ValueType is reference type.
    pub unsafe fn get_value(&self) -> ValueType { self.value.clone() }
}

impl<'a, GuardType, ValueType> Deref
    for GuardedValue<GuardType, &'a ValueType>
{
    type Target = ValueType;

    fn deref(&self) -> &Self::Target { self.value }
}

impl<'a, GuardType, ValueType> Deref
    for GuardedValue<GuardType, &'a mut ValueType>
{
    type Target = ValueType;

    fn deref(&self) -> &Self::Target { self.value }
}

impl<'a, GuardType, ValueType> DerefMut
    for GuardedValue<GuardType, &'a mut ValueType>
{
    fn deref_mut(&mut self) -> &mut Self::Target { self.value }
}
