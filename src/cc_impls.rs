//! Additional impls about `AbstractCc<T, O>` to make it easier to use.

use crate::cc::RawCc;
use crate::collect::ObjectSpace as O;
use crate::Cc;
use crate::Trace;
use std::cmp::Ordering;
use std::fmt;
use std::hash;
use std::ops::Deref;

impl<T: Default + Trace> Default for Cc<T> {
    #[inline]
    fn default() -> Cc<T> {
        Self::new(Default::default())
    }
}

impl<T: PartialEq + ?Sized> PartialEq for RawCc<T, O> {
    #[inline]
    fn eq(&self, other: &RawCc<T, O>) -> bool {
        **self == **other
    }

    #[inline]
    fn ne(&self, other: &RawCc<T, O>) -> bool {
        **self != **other
    }
}

impl<T: hash::Hash + ?Sized> hash::Hash for RawCc<T, O> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T: Eq + ?Sized> Eq for RawCc<T, O> {}

impl<T: PartialOrd + ?Sized> PartialOrd for RawCc<T, O> {
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

impl<T: Ord + ?Sized> Ord for RawCc<T, O> {
    #[inline]
    fn cmp(&self, other: &RawCc<T, O>) -> Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: fmt::Debug + ?Sized> fmt::Debug for RawCc<T, O> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("Cc").field(&self.inner().deref()).finish()
    }
}

impl<T: fmt::Display + ?Sized> fmt::Display for RawCc<T, O> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: ?Sized> fmt::Pointer for RawCc<T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.inner().deref(), f)
    }
}
