pub(crate) mod collect;
mod ref_count;

#[cfg(test)]
mod tests;

use crate::cc::RawCc;
use crate::ref_count::RefCount;
use crate::Trace;
use crate::Tracer;
use collect::ThreadedObjectSpace;
use parking_lot::lock_api::RwLockReadGuard;
use parking_lot::RawRwLock;
use std::marker::PhantomData;
use std::ops::Deref;

/// A multi-thread reference-counting pointer that integrates with cyclic
/// garbage collection.
///
/// [`ThreadedCc`](type.ThreadedCc.html) is similar to [`Cc`](type.Cc.html).
/// It works with multi-thread but is significantly slower than
/// [`Cc`](type.Cc.html).
///
/// To construct a [`ThreadedCc`](type.ThreadedCc.html), use
/// [`ThreadedObjectSpace::create`](struct.ThreadedObjectSpace.html#method.create).
pub type ThreadedCc<T> = RawCc<T, ThreadedObjectSpace>;

/// Wraps a borrowed reference to [`ThreadedCc`](type.ThreadedCc.html).
///
/// The wrapper automatically takes a lock that prevents the collector from
/// running. This ensures that when the collector is running, there are no
/// borrowed references of [`ThreadedCc`](type.ThreadedCc.html). Therefore
/// [`ThreadedCc`](type.ThreadedCc.html)s can be seen as temporarily immutable,
/// even if they might have interior mutability. The collector relies on this
/// for correctness.
pub struct ThreadedCcRef<'a, T: ?Sized> {
    // Prevent the collector from running when a reference is present.
    locked: RwLockReadGuard<'a, RawRwLock, ()>,

    // Provide access to the parent `Acc`.
    parent: &'a ThreadedCc<T>,

    // !Send + !Sync.
    _phantom: PhantomData<*mut ()>,
}

// safety: similar to `std::sync::Arc`
unsafe impl<T: Send + Sync> Send for ThreadedCc<T> {}
unsafe impl<T: Send + Sync> Sync for ThreadedCc<T> {}

impl<T: ?Sized> ThreadedCc<T> {
    /// Immutably borrows the wrapped value.
    ///
    /// The borrow lasts until the returned value exits scope.
    pub fn borrow(&self) -> ThreadedCcRef<'_, T> {
        ThreadedCcRef {
            locked: self.inner().ref_count.locked().unwrap(),
            parent: self,
            _phantom: PhantomData,
        }
    }
}

impl<'a, T: ?Sized> Deref for ThreadedCcRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let _ = &self.locked;
        self.parent.inner().deref()
    }
}

impl<T: Trace> Trace for ThreadedCc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        T::is_type_tracked()
    }

    // No as_any. This enforces locking via ThreadedCcRef.
}

impl Trace for ThreadedCc<dyn Trace> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }

    // No as_any. This enforces locking via ThreadedCcRef.
}

impl Trace for ThreadedCc<dyn Trace + Send> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }

    // No as_any. This enforces locking via ThreadedCcRef.
}

impl Trace for ThreadedCc<dyn Trace + Send + Sync> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }

    // No as_any. This enforces locking via ThreadedCcRef.
}
