use std::cell::Cell;
use std::ops::Deref;
use std::ptr::NonNull;

struct RcBox<T: ?Sized> {
    ref_count: Cell<usize>,
    value: T,
}

pub struct Rc<T: ?Sized>(NonNull<RcBox<T>>);

impl<T> Rc<T> {
    pub fn new(value: T) -> Rc<T> {
        let ref_count = Cell::new(1);
        let rc_box = RcBox { ref_count, value };
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
    fn inc_ref(&self) {
        let inner = self.inner();
        inner.ref_count.set(inner.ref_count.get() + 1);
    }

    #[inline]
    fn dec_ref(&self) {
        let inner = self.inner();
        inner.ref_count.set(inner.ref_count.get() - 1);
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
        if self.inner().ref_count.get() == 0 {
            unsafe {
                (self.0.as_mut() as *mut RcBox<T>).drop_in_place();
            }
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
