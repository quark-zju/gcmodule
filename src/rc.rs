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
    pub(crate) value: Option<Box<dyn RcDyn>>,
}

struct RcBox<T: ?Sized> {
    pub(crate) gc_header: Option<Pin<Box<GcHeader>>>,
    pub(crate) ref_count: Cell<usize>,
    value: T,
}

pub struct Rc<T: ?Sized>(NonNull<RcBox<T>>);

impl<T> Rc<T> {
    pub fn new(value: T) -> Rc<T> {
        let rc_box = RcBox {
            gc_header: None,
            ref_count: Cell::new(1),
            value,
        };
        let ptr = Box::into_raw(Box::new(rc_box));
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        Self(ptr)
    }
}

impl<T: ?Sized> Rc<T> {
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
        inner.gc_header.is_some()
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
        let mut gc_header = None;
        std::mem::swap(&mut gc_header, &mut inner.gc_header);
        let mut gc_header = gc_header.unwrap();
        debug_assert!(gc_header.value.is_some());
        debug_assert!(!gc_header.prev.is_null());
        debug_assert!(!gc_header.next.is_null());
        unsafe {
            (*(gc_header.prev)).next = gc_header.next;
            (*(gc_header.next)).prev = gc_header.prev;
        }
        // triggers 'drop()'
    }
}

impl<T: Trace + 'static> Rc<T> {
    fn gc_track(&mut self, prev: &mut Pin<Box<GcHeader>>) {
        if self.is_tracked() {
            return;
        }
        let cloned = self.clone();
        let mut inner = self.inner_mut();
        let next = prev.next;
        let header = Box::pin(GcHeader {
            prev: prev.deref_mut(),
            next,
            value: Some(Box::new(cloned)),
        });
        inner.gc_header = Some(header);
        // FIXME: Set prev, next accordingly.
        // unsafe { next.as_mut() }.unwrap().prev = inner.gc_header;
        // prev.next = inner.gc_header;
    }
}

impl<T: ?Sized> Clone for Rc<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.inc_ref();
        Self(self.0)
    }
}

impl<T: ?Sized> Deref for Rc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner().value
    }
}

impl<T: ?Sized> Drop for Rc<T> {
    fn drop(&mut self) {
        self.dec_ref();
        match self.ref_count() {
            0 => {
                debug_assert!(
                    self.inner().gc_header.is_none()
                        || self.inner().gc_header.as_ref().unwrap().value.is_none()
                );
                unsafe {
                    (self.0.as_mut() as *mut RcBox<T>).drop_in_place();
                }
            }
            1 if self.inner().gc_header.is_some() => {
                self.gc_untrack();
            }
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() {
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
}
