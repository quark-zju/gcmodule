mod collect;
mod ref_count;

#[cfg(test)]
mod tests;

use crate::cc::AbstractCc;
use crate::ref_count::RefCount;
use crate::Trace;
use crate::Tracer;
use collect::AccObjectSpace;
use parking_lot::lock_api::RwLockReadGuard;
use parking_lot::RawRwLock;
use std::marker::PhantomData;
use std::ops::Deref;

/// An atomic reference-counting pointer that integrates
/// with cyclic garbage collection.
///
/// [`Acc`](struct.Acc.html) is similar to [`Cc`](struct.Cc.html). It is slower
/// but can work in multiple threads.
pub type Acc<T> = AbstractCc<T, AccObjectSpace>;

/// Reference to `Acc<T>`.
pub struct AccRef<'a, T: ?Sized> {
    // Prevent the collector from running when a reference is present.
    locked: RwLockReadGuard<'a, RawRwLock, ()>,

    // Provide access to the parent `Acc`.
    parent: &'a Acc<T>,

    // !Send + !Sync.
    _phantom: PhantomData<*mut ()>,
}

// safety: similar to `std::sync::Arc`
unsafe impl<T: Send + Sync> Send for Acc<T> {}
unsafe impl<T: Send + Sync> Sync for Acc<T> {}

impl<T: ?Sized> Acc<T> {
    pub fn read(&self) -> AccRef<'_, T> {
        AccRef {
            locked: self.inner().ref_count.locked().unwrap(),
            parent: self,
            _phantom: PhantomData,
        }
    }
}

impl<'a, T: ?Sized> Deref for AccRef<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let _ = &self.locked;
        self.parent.inner().deref()
    }
}

impl<T: Trace> Trace for Acc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        T::is_type_tracked()
    }

    // No as_any. Enforce locking via AccRef.
}

impl Trace for Acc<dyn Trace> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }

    // No as_any. Enforce locking via AccRef.
}
