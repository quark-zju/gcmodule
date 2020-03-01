use crate::collect;
use crate::trace::Trace;
use crate::trace::Tracer;
use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::ptr::NonNull;

pub struct GcHeader {
    pub(crate) next: *mut GcHeader,
    pub(crate) prev: *mut GcHeader,
    pub(crate) value: Box<dyn RcDyn>,
}

struct RcBox<T: ?Sized> {
    pub(crate) gc_header: *mut GcHeader,
    pub(crate) ref_count: Cell<usize>,
    value: ManuallyDrop<T>,
}

pub struct Rc<T: Trace + 'static>(NonNull<RcBox<T>>);

const REF_COUNT_MARKED_FOR_DROP: usize = usize::max_value();
const REF_COUNT_MARKED_FOR_FREE: usize = REF_COUNT_MARKED_FOR_DROP - 1;

/// Type-erased `Rc<T>` with interfaces needed by GC.
pub(crate) trait RcDyn {
    /// Returns the reference count for cycle detection.
    fn gc_ref_count(&self) -> usize;

    /// Visit referents for cycle detection.
    fn gc_traverse(&self, tracer: &mut Tracer);

    /// Mark for drop. Transfer ownship of `Box<dyn RcDyn>` from `self`.
    /// Must call `gc_force_drop_without_release` for the next step.
    fn gc_prepare_drop(&mut self) -> Box<dyn RcDyn>;

    /// Call customized drop logic (`T::drop`) without releasing memory.
    /// Remove self from the GC list.
    /// Must call `gc_mark_for_release` for the next step.
    fn gc_force_drop_without_release(&mut self);

    /// Mark for releasing memory.
    /// At this point there should be only one owner of the `RcBox<T>`, which is
    /// the `Box<dyn RcDyn>` returned by `gc_prepare_drop`. Dropping that owner
    /// will release the memory of `RcBox<T>`.
    fn gc_mark_for_release(&mut self);
}

/// A dummy implementation without drop side-effects.
pub(crate) struct RcDummy;

impl RcDyn for RcDummy {
    fn gc_ref_count(&self) -> usize {
        1
    }
    fn gc_traverse(&self, _tracer: &mut Tracer) {}
    fn gc_prepare_drop(&mut self) -> Box<dyn RcDyn> {
        Box::new(Self)
    }
    fn gc_force_drop_without_release(&mut self) {}
    fn gc_mark_for_release(&mut self) {}
}

impl<T: Trace + 'static> Rc<T> {
    pub fn new(value: T) -> Rc<T> {
        let rc_box = RcBox {
            gc_header: std::ptr::null_mut(),
            ref_count: Cell::new(1),
            value: ManuallyDrop::new(value),
        };
        let ptr = Box::into_raw(Box::new(rc_box));
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        let mut result = Self(ptr);
        // Opt-in GC if this type should be tracked.
        if result.is_type_tracked() {
            collect::GC_LIST.with(|ref_head| {
                let mut head = ref_head.borrow_mut();
                result.gc_track(&mut head);
            });
        }
        result
    }

    #[inline]
    fn inner(&self) -> &RcBox<T> {
        unsafe { self.0.as_ref() }
    }

    #[inline]
    fn inner_mut(&mut self) -> &mut RcBox<T> {
        unsafe { self.0.as_mut() }
    }

    #[inline]
    fn inc_ref(&self) {
        let inner = self.inner();
        let new_count = inner.ref_count.get() + 1;
        inner.ref_count.set(new_count);
    }

    #[inline]
    fn dec_ref(&self) {
        let inner = self.inner();
        let new_count = inner.ref_count.get() - 1;
        inner.ref_count.set(new_count);
    }

    #[inline]
    fn is_tracked(&self) -> bool {
        let inner = self.inner();
        !inner.gc_header.is_null()
    }

    #[inline]
    fn ref_count(&self) -> usize {
        let inner = self.inner();
        inner.ref_count.get()
    }

    fn gc_untrack(&mut self) {
        if !self.is_tracked() {
            return;
        }
        let inner = self.inner_mut();
        let mut gc_header = unsafe { Box::from_raw(inner.gc_header) };
        inner.gc_header = std::ptr::null_mut();
        debug_assert!(!gc_header.prev.is_null());
        debug_assert!(!gc_header.next.is_null());
        unsafe {
            (*(gc_header.prev)).next = gc_header.next;
            (*(gc_header.next)).prev = gc_header.prev;
        }
        // triggers 'drop()'
    }

    fn gc_track(&mut self, prev: &mut Pin<Box<GcHeader>>) {
        if self.is_tracked() {
            return;
        }
        let cloned = self.clone();
        let mut inner = self.inner_mut();
        let next = prev.next;
        let header = Box::new(GcHeader {
            prev: prev.deref_mut(),
            next,
            value: Box::new(cloned),
        });
        inner.gc_header = Box::into_raw(header);
        unsafe { next.as_mut() }.unwrap().prev = inner.gc_header;
        prev.next = inner.gc_header;
    }
}

impl<T: Trace + 'static> Clone for Rc<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.inc_ref();
        Self(self.0)
    }
}

impl<T: Trace + 'static> Deref for Rc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner().value
    }
}

impl<T: Trace + 'static> Drop for Rc<T> {
    fn drop(&mut self) {
        match self.ref_count() {
            1 => {
                // ref_count will be 0. Drop and release memory.
                debug_assert!(!self.is_tracked());
                unsafe {
                    let mut rc_box: Box<RcBox<T>> = Box::from_raw(self.0.as_mut());
                    ManuallyDrop::drop(&mut rc_box.value);
                    drop(rc_box);
                }
            }
            2 if self.is_tracked() => {
                // ref_count will be 1, held by the RcDyn in GcHeader.
                // Opt-out GC and ref_count will be 0.
                self.dec_ref();
                self.gc_untrack();
            }
            REF_COUNT_MARKED_FOR_DROP => {
                // Do nothing. Drop is being done by gc_force_drop_without_release().
            }
            REF_COUNT_MARKED_FOR_FREE => {
                // T was dropped by gc_force_drop_without_release.
                // Just release the memory.
                let rc_box: Box<RcBox<T>> = unsafe { Box::from_raw(self.0.as_mut()) };
                drop(rc_box);
            }
            0 => {
                panic!("bug: ref_count should not be 0");
            }
            _ => {
                self.dec_ref();
            }
        }
    }
}

impl<T: Trace> RcDyn for Rc<T> {
    fn gc_ref_count(&self) -> usize {
        let mut count = self.inner().ref_count.get();
        if self.is_tracked() {
            // Exclude the refcount kept by GcHeader.
            // So if the cycle collector dry runs dec_ref, unreachable
            // objects will have 0 as their ref_counts.
            count -= 1;
        }
        count
    }

    fn gc_traverse(&self, tracer: &mut Tracer) {
        self.deref().trace(tracer)
    }

    fn gc_prepare_drop(&mut self) -> Box<dyn RcDyn> {
        debug_assert!(self.is_tracked());
        self.inner().ref_count.set(REF_COUNT_MARKED_FOR_DROP);
        let mut result: Box<dyn RcDyn> = Box::new(RcDummy);
        std::mem::swap(&mut result, unsafe {
            &mut (*self.inner_mut().gc_header).value
        });
        result
    }

    fn gc_force_drop_without_release(&mut self) {
        debug_assert!(self.is_tracked());
        debug_assert!(self.ref_count() == REF_COUNT_MARKED_FOR_DROP);
        self.gc_untrack();
        let inner = self.inner_mut();
        unsafe { ManuallyDrop::drop(&mut inner.value) };
    }

    fn gc_mark_for_release(&mut self) {
        debug_assert!(!self.is_tracked());
        debug_assert!(self.ref_count() == REF_COUNT_MARKED_FOR_DROP);
        self.inner().ref_count.set(REF_COUNT_MARKED_FOR_FREE);
    }
}

impl<T: Trace> Trace for Rc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        // For other non-`Rc<T>` container types, `trace` visit referents,
        // is recursive, and does not call `tracer` directly. For `Rc<T>`,
        // `trace` stops here, is non-recursive, and does apply `tracer`
        // to the actual `GcHeader`. It's expected that the upper layer
        // calls `gc_traverse` on everything (not just roots).
        if self.is_tracked() {
            if let Some(header) = unsafe { self.inner().gc_header.as_mut() } {
                tracer(header);
            }
        }
    }

    fn is_type_tracked(&self) -> bool {
        self.deref().is_type_tracked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn test_simple_untracked() {
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        static DROPPED: AtomicBool = AtomicBool::new(false);
        struct X(&'static str);
        crate::untrack!(X);
        impl Drop for X {
            fn drop(&mut self) {
                DROPPED.store(true, SeqCst);
            }
        }
        {
            let v1 = Rc::new(X("abc"));
            {
                let v2 = v1.clone();
                assert_eq!(v1.deref().0, v2.deref().0);
            }
            assert!(!DROPPED.load(SeqCst));
        }
        assert!(DROPPED.load(SeqCst));
    }

    #[test]
    fn test_simple_tracked() {
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        static DROPPED: AtomicBool = AtomicBool::new(false);
        struct X(&'static str);
        impl Trace for X {}
        impl Drop for X {
            fn drop(&mut self) {
                DROPPED.store(true, SeqCst);
            }
        }
        {
            let v1 = Rc::new(X("abc"));
            {
                let v2 = v1.clone();
                assert_eq!(v1.deref().0, v2.deref().0);
            }
            assert!(!DROPPED.load(SeqCst));
        }
        assert!(DROPPED.load(SeqCst));
    }

    #[test]
    fn test_simple_cycles() {
        assert_eq!(collect::collect_cycles(), 0);
        {
            let a: Rc<RefCell<Vec<Box<dyn Trace>>>> = Rc::new(RefCell::new(Vec::new()));
            let b: Rc<RefCell<Vec<Box<dyn Trace>>>> = Rc::new(RefCell::new(Vec::new()));
            assert_eq!(collect::collect_cycles(), 0);
            {
                let mut a = a.borrow_mut();
                a.push(Box::new(b.clone()));
            }
            {
                let mut b = b.borrow_mut();
                b.push(Box::new(a.clone()));
            }
            assert_eq!(collect::collect_cycles(), 0);
        }
        assert_eq!(collect::collect_cycles(), 2);
    }
}
