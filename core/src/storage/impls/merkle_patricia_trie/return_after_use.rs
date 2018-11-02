use std::mem::swap;

pub struct ReturnAfterUse<'a, T: 'a> {
    origin: Option<&'a mut Option<T>>,
    current: Option<T>,
}

impl<'a, T: Default> Default for ReturnAfterUse<'a, T> {
    fn default() -> Self {
        Self {
            origin: None,
            current: Some(T::default()),
        }
    }
}

impl<'a, T> Drop for ReturnAfterUse<'a, T> {
    fn drop(&mut self) {
        match &mut self.origin {
            Some(origin_mut) => swap(*origin_mut, &mut self.current),
            _ => unreachable!(),
        }
    }
}

impl<'a, T> ReturnAfterUse<'a, T> {
    pub fn new(option: &'a mut Option<T>) -> Self {
        let mut ret = Self {
            origin: None,
            current: None,
        };
        swap(&mut ret.current, option);
        ret.origin = Some(option);

        ret
    }

    pub fn new_from_origin<'b>(
        origin: &'b mut ReturnAfterUse<'a, T>,
    ) -> ReturnAfterUse<'b, T>
    where 'a: 'b {
        Self::new(&mut origin.current)
    }

    pub fn get_ref(&self) -> &T { return self.current.as_ref().unwrap(); }

    pub fn get_mut(&mut self) -> &mut T {
        return self.current.as_mut().unwrap();
    }
}
