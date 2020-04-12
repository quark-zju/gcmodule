//! Additional impls about `AbstractCc<T, O>` to make it easier to use.

use crate::cc::RawCc;
use crate::collect::ObjectSpace as O;
use crate::Cc;
use crate::Trace;
use std::cmp::Ordering;
use std::fmt;

impl<T: Default + Trace> Default for Cc<T> {
    #[inline]
    fn default() -> Cc<T> {
        Self::new(Default::default())
    }
}

impl<T: PartialEq> PartialEq for RawCc<T, O> {
    #[inline]
    fn eq(&self, other: &RawCc<T, O>) -> bool {
        **self == **other
    }

    #[inline]
    fn ne(&self, other: &RawCc<T, O>) -> bool {
        **self != **other
    }
}

impl<T: Eq> Eq for RawCc<T, O> {}

impl<T: PartialOrd> PartialOrd for RawCc<T, O> {
    #[inline]
    fn partial_cmp(&self, other: &RawCc<T, O>) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }

    #[inline]
    fn lt(&self, other: &RawCc<T, O>) -> bool {
        **self < **other
    }

    #[inline]
    fn le(&self, other: &RawCc<T, O>) -> bool {
        **self <= **other
    }

    #[inline]
    fn gt(&self, other: &RawCc<T, O>) -> bool {
        **self > **other
    }

    #[inline]
    fn ge(&self, other: &RawCc<T, O>) -> bool {
        **self >= **other
    }
}

impl<T: Ord> Ord for RawCc<T, O> {
    #[inline]
    fn cmp(&self, other: &RawCc<T, O>) -> Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: fmt::Debug> fmt::Debug for RawCc<T, O> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Cc({:?})", **self)
    }
}

impl<T: fmt::Debug> fmt::Display for RawCc<T, O> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (**self).fmt(f)
    }
}
