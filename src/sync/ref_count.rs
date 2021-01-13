use crate::ref_count::{RefCount, REF_COUNT_MASK_DROPPED, REF_COUNT_MASK_TRACKED, REF_COUNT_SHIFT};
use parking_lot::lock_api::RwLockReadGuard;
use parking_lot::RawRwLock;
use parking_lot::RwLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};
use std::sync::Arc;

pub struct ThreadedRefCount {
    ref_count: AtomicUsize,
    weak_count: AtomicUsize,
    pub(crate) collector_lock: Arc<RwLock<()>>,
}

impl ThreadedRefCount {
    #[inline]
    pub(crate) fn new(tracked: bool, collector_lock: Arc<RwLock<()>>) -> Self {
        Self {
            collector_lock: collector_lock,
            ref_count: AtomicUsize::new(
                (1 << REF_COUNT_SHIFT) | if tracked { REF_COUNT_MASK_TRACKED } else { 0 },
            ),
            weak_count: AtomicUsize::new(0),
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
        Some(self.collector_lock.read_recursive())
    }

    #[inline]
    fn inc_weak(&self) -> usize {
        self.weak_count.fetch_add(1, AcqRel)
    }

    #[inline]
    fn dec_weak(&self) -> usize {
        self.weak_count.fetch_sub(1, AcqRel)
    }

    #[inline]
    fn weak_count(&self) -> usize {
        self.weak_count.load(Acquire)
    }
}
