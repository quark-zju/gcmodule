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

    // Ideally this can be "type Locked<'a> = ..." so there is no need to
    // duplicate the function to make parking_lot optional. However it's not in
    // stable Rust yet. See https://github.com/rust-lang/rust/issues/44265.
    #[cfg(not(feature = "sync"))]
    #[inline]
    fn locked(&self) -> () {
        ()
    }

    #[cfg(feature = "sync")]
    #[inline]
    fn locked(
        &self,
    ) -> Option<parking_lot::lock_api::RwLockReadGuard<'_, parking_lot::RawRwLock, ()>> {
        None
    }
}

pub struct SingleThreadRefCount(Cell<usize>);

impl SingleThreadRefCount {
    pub fn new(tracked: bool) -> Self {
        let value = (1 << REF_COUNT_SHIFT) | if tracked { REF_COUNT_MASK_TRACKED } else { 0 };
        Self(Cell::new(value))
    }
}

impl RefCount for SingleThreadRefCount {
    #[inline]
    fn is_tracked(&self) -> bool {
        Cell::get(&self.0) & REF_COUNT_MASK_TRACKED != 0
    }

    #[inline]
    fn is_dropped(&self) -> bool {
        Cell::get(&self.0) & REF_COUNT_MASK_DROPPED != 0
    }

    #[inline]
    fn set_dropped(&self) -> bool {
        let value = Cell::get(&self.0);
        self.0.set(value | REF_COUNT_MASK_DROPPED);
        value & REF_COUNT_MASK_DROPPED != 0
    }

    #[inline]
    fn ref_count(&self) -> usize {
        self.0.get() >> REF_COUNT_SHIFT
    }

    #[inline]
    fn inc_ref(&self) -> usize {
        let value = Cell::get(&self.0);
        self.0.set(value + (1 << REF_COUNT_SHIFT));
        value >> REF_COUNT_SHIFT
    }

    #[inline]
    fn dec_ref(&self) -> usize {
        let value = Cell::get(&self.0);
        self.0.set(value - (1 << REF_COUNT_SHIFT));
        value >> REF_COUNT_SHIFT
    }
}
