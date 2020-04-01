use parking_lot::lock_api::RwLockReadGuard;
use parking_lot::RawRwLock;
use std::cell::Cell;

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
