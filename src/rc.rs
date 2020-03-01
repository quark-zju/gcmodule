use crate::collect;
use crate::rcdyn::RcDyn;
use crate::trace::Trace;
use std::cell::Cell;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::ptr::NonNull;

pub(crate) struct GcHeader {
    pub(crate) next: *mut GcHeader,
    pub(crate) prev: *mut GcHeader,
    pub(crate) value: Box<dyn RcDyn>,
}

struct RcBox<T: ?Sized> {
    pub(crate) gc_header: *mut GcHeader,
    pub(crate) ref_count: Cell<usize>,
    value: T,
}

pub struct Rc<T: Trace + 'static>(NonNull<RcBox<T>>);

impl<T: Trace + 'static> Rc<T> {
    pub fn new(value: T) -> Rc<T> {
        let rc_box = RcBox {
            gc_header: std::ptr::null_mut(),
            ref_count: Cell::new(1),
            value,
        };
        let ptr = Box::into_raw(Box::new(rc_box));
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        Self(ptr)
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
        debug_assert!(self.ref_count() > 0);
        self.dec_ref();
        match self.ref_count() {
            0 => {
                debug_assert!(!self.is_tracked());
                unsafe {
                    let _drop = Box::from_raw(self.0.as_mut());
                }
            }
            1 if self.is_tracked() => {
                self.gc_untrack();
            }
            _ => {
                // Opt-in GC if this type is tracked.
                if self.is_type_tracked() {
                    collect::GC_LIST.with(|ref_head| {
                        let mut head = ref_head.borrow_mut();
                        self.gc_track(&mut head);
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
