//! Additional impls about `Cc<T>` to make it easier to use.

use crate::{Cc, Trace};
use std::cmp::Ordering;

impl<T: Default + Trace + 'static> Default for Cc<T> {
    #[inline]
    fn default() -> Cc<T> {
        Cc::new(Default::default())
    }
}

impl<T: PartialEq> PartialEq for Cc<T> {
    #[inline]
    fn eq(&self, other: &Cc<T>) -> bool {
        **self == **other
    }

    #[inline]
    fn ne(&self, other: &Cc<T>) -> bool {
        **self != **other
    }
}

impl<T: Eq> Eq for Cc<T> {}

impl<T: PartialOrd> PartialOrd for Cc<T> {
    #[inline]
    fn partial_cmp(&self, other: &Cc<T>) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }

    #[inline]
    fn lt(&self, other: &Cc<T>) -> bool {
        **self < **other
    }

    #[inline]
    fn le(&self, other: &Cc<T>) -> bool {
        **self <= **other
    }

    #[inline]
    fn gt(&self, other: &Cc<T>) -> bool {
        **self > **other
    }

    #[inline]
    fn ge(&self, other: &Cc<T>) -> bool {
        **self >= **other
    }
}

impl<T: Ord> Ord for Cc<T> {
    #[inline]
    fn cmp(&self, other: &Cc<T>) -> Ordering {
        (**self).cmp(&**other)
    }
}
