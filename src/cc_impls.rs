//! Additional impls about `AbstractCc<T, O>` to make it easier to use.

use crate::cc::AbstractCc;
use crate::collect::ObjectSpace;
use crate::Cc;
use crate::Trace;
use std::cmp::Ordering;

impl<T: Default + Trace> Default for Cc<T> {
    #[inline]
    fn default() -> Cc<T> {
        Self::new(Default::default())
    }
}

impl<T: PartialEq, O: ObjectSpace> PartialEq for AbstractCc<T, O> {
    #[inline]
    fn eq(&self, other: &AbstractCc<T, O>) -> bool {
        **self == **other
    }

    #[inline]
    fn ne(&self, other: &AbstractCc<T, O>) -> bool {
        **self != **other
    }
}

impl<T: Eq, O: ObjectSpace> Eq for AbstractCc<T, O> {}

impl<T: PartialOrd, O: ObjectSpace> PartialOrd for AbstractCc<T, O> {
    #[inline]
    fn partial_cmp(&self, other: &AbstractCc<T, O>) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }

    #[inline]
    fn lt(&self, other: &AbstractCc<T, O>) -> bool {
        **self < **other
    }

    #[inline]
    fn le(&self, other: &AbstractCc<T, O>) -> bool {
        **self <= **other
    }

    #[inline]
    fn gt(&self, other: &AbstractCc<T, O>) -> bool {
        **self > **other
    }

    #[inline]
    fn ge(&self, other: &AbstractCc<T, O>) -> bool {
        **self >= **other
    }
}

impl<T: Ord, O: ObjectSpace> Ord for AbstractCc<T, O> {
    #[inline]
    fn cmp(&self, other: &AbstractCc<T, O>) -> Ordering {
        (**self).cmp(&**other)
    }
}
