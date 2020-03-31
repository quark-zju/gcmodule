//! Additional impls about `AbstractCc<T, I>` to make it easier to use.

use crate::cc::AbstractCc;
use crate::mutable_usize::Usize;
use crate::Cc;
use crate::Trace;
use std::cmp::Ordering;

impl<T: Default + Trace> Default for Cc<T> {
    #[inline]
    fn default() -> Cc<T> {
        Self::new(Default::default())
    }
}

impl<T: PartialEq, I: Usize> PartialEq for AbstractCc<T, I> {
    #[inline]
    fn eq(&self, other: &AbstractCc<T, I>) -> bool {
        **self == **other
    }

    #[inline]
    fn ne(&self, other: &AbstractCc<T, I>) -> bool {
        **self != **other
    }
}

impl<T: Eq, I: Usize> Eq for AbstractCc<T, I> {}

impl<T: PartialOrd, I: Usize> PartialOrd for AbstractCc<T, I> {
    #[inline]
    fn partial_cmp(&self, other: &AbstractCc<T, I>) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }

    #[inline]
    fn lt(&self, other: &AbstractCc<T, I>) -> bool {
        **self < **other
    }

    #[inline]
    fn le(&self, other: &AbstractCc<T, I>) -> bool {
        **self <= **other
    }

    #[inline]
    fn gt(&self, other: &AbstractCc<T, I>) -> bool {
        **self > **other
    }

    #[inline]
    fn ge(&self, other: &AbstractCc<T, I>) -> bool {
        **self >= **other
    }
}

impl<T: Ord, I: Usize> Ord for AbstractCc<T, I> {
    #[inline]
    fn cmp(&self, other: &AbstractCc<T, I>) -> Ordering {
        (**self).cmp(&**other)
    }
}
