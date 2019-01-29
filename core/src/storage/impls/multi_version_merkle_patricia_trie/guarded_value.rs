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

    /// Unsafe because the guard is dropped at the end of the statement. Using
    /// of value is only safe within the statement.
    pub unsafe fn into_value(self) -> ValueType { self.value }
}

impl<GuardType, ValueType: Clone> GuardedValue<GuardType, ValueType> {
    /// There is no guarantee for the validity of value especially when
    /// ValueType is reference type.
    pub unsafe fn get_value(&self) -> ValueType { self.value.clone() }
}

impl<GuardType, ValueType: Deref> Deref for GuardedValue<GuardType, ValueType> {
    type Target = ValueType::Target;

    fn deref(&self) -> &Self::Target { self.value.deref() }
}

impl<GuardType, ValueType: DerefMut> DerefMut
    for GuardedValue<GuardType, ValueType>
{
    fn deref_mut(&mut self) -> &mut Self::Target { self.value.deref_mut() }
}
