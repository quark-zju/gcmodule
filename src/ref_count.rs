use parking_lot::lock_api::RwLockReadGuard;
use parking_lot::RawRwLock;
use parking_lot::RwLock;
use std::cell::Cell;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};
use std::sync::Arc;

/// Whether a `GcHeader` exists before the `CcBox<T>`.
pub(crate) const REF_COUNT_MASK_TRACKED: usize = 0b1;

/// Whether `T` in the `CcBox<T>` has been dropped.
pub(crate) const REF_COUNT_MASK_DROPPED: usize = 0b10;

/// Number of bits used for metadata.
pub(crate) const REF_COUNT_SHIFT: i32 = 2;

pub trait RefCount: 'static {
    fn is_tracked(&self) -> bool;
    fn is_dropped(&self) -> bool;
    fn inc_ref(&self) -> usize;
    fn dec_ref(&self) -> usize;
    fn ref_count(&self) -> usize;
    fn set_dropped(&self) -> bool;

    #[inline]
    fn locked(&self) -> Option<RwLockReadGuard<'_, RawRwLock, ()>> {
        None
    }
}

impl RefCount for Cell<usize> {
    #[inline]
    fn is_tracked(&self) -> bool {
        Cell::get(self) & REF_COUNT_MASK_TRACKED != 0
    }

    #[inline]
    fn is_dropped(&self) -> bool {
        Cell::get(self) & REF_COUNT_MASK_DROPPED != 0
    }

    #[inline]
    fn set_dropped(&self) -> bool {
        let value = Cell::get(self);
        self.set(value | REF_COUNT_MASK_DROPPED);
        value & REF_COUNT_MASK_DROPPED != 0
    }

    #[inline]
    fn ref_count(&self) -> usize {
        self.get() >> REF_COUNT_SHIFT
    }

    #[inline]
    fn inc_ref(&self) -> usize {
        let value = Cell::get(self);
        self.set(value + (1 << REF_COUNT_SHIFT));
        value >> REF_COUNT_SHIFT
    }

    #[inline]
    fn dec_ref(&self) -> usize {
        let value = Cell::get(self);
        self.set(value - (1 << REF_COUNT_SHIFT));
        value >> REF_COUNT_SHIFT
    }
}

pub struct ThreadedRefCount {
    ref_count: AtomicUsize,
    pub(crate) collecting: Arc<RwLock<()>>,
}

impl ThreadedRefCount {
    #[inline]
    pub(crate) fn new(tracked: bool, collecting: Arc<RwLock<()>>) -> Self {
        Self {
            collecting: collecting,
            ref_count: AtomicUsize::new(
                (1 << REF_COUNT_SHIFT) | if tracked { REF_COUNT_MASK_TRACKED } else { 0 },
            ),
        }
    }
}

impl RefCount for ThreadedRefCount {
    #[inline]
    fn is_tracked(&self) -> bool {
        self.ref_count.load(Relaxed) & REF_COUNT_MASK_TRACKED != 0
    }

    #[inline]
    fn is_dropped(&self) -> bool {
        self.ref_count.load(Acquire) & REF_COUNT_MASK_DROPPED != 0
    }

    #[inline]
    fn set_dropped(&self) -> bool {
        let old_value = self.ref_count.fetch_or(REF_COUNT_MASK_DROPPED, AcqRel);
        old_value & REF_COUNT_MASK_DROPPED != 0
    }

    #[inline]
    fn ref_count(&self) -> usize {
        self.ref_count.load(Acquire) >> REF_COUNT_SHIFT
    }

    #[inline]
    fn inc_ref(&self) -> usize {
        self.ref_count.fetch_add(1 << REF_COUNT_SHIFT, AcqRel) >> REF_COUNT_SHIFT
    }

    #[inline]
    fn dec_ref(&self) -> usize {
        self.ref_count.fetch_sub(1 << REF_COUNT_SHIFT, AcqRel) >> REF_COUNT_SHIFT
    }

    #[inline]
    fn locked(&self) -> Option<RwLockReadGuard<'_, RawRwLock, ()>> {
        Some(self.collecting.read_recursive())
    }
}
