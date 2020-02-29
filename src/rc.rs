use std::cell::Cell;
use std::ops::Deref;
use std::ptr::NonNull;

static FLAG_TRACKED: usize = 1;
static FLAG_BIT_COUNT: u32 = 1;

struct RcBox<T: ?Sized> {
    // Lowest FLAG_BIT_COUNT bits are used for flags.
    ref_count_with_flags: Cell<usize>,
    value: T,
}

pub struct Rc<T: ?Sized>(NonNull<RcBox<T>>);

impl<T> Rc<T> {
    pub fn new(value: T) -> Rc<T> {
        let ref_count_with_flags = Cell::new(1 << FLAG_BIT_COUNT);
        let rc_box = RcBox {
            ref_count_with_flags,
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
    fn inc_ref(&self) {
        let inner = self.inner();
        let new_count = inner.ref_count_with_flags.get() + (1 << FLAG_BIT_COUNT);
        inner.ref_count_with_flags.set(new_count);
    }

    #[inline]
    fn dec_ref(&self) {
        let inner = self.inner();
        let new_count = inner.ref_count_with_flags.get() - (1 << FLAG_BIT_COUNT);
        inner.ref_count_with_flags.set(new_count);
    }

    #[inline]
    fn is_tracked(&self) -> bool {
        let inner = self.inner();
        inner.ref_count_with_flags.get() & FLAG_TRACKED != 0
    }

    #[inline]
    fn set_tracked(&self) {
        let inner = self.inner();
        let new_count = inner.ref_count_with_flags.get() | FLAG_TRACKED;
        inner.ref_count_with_flags.set(new_count);
    }

    #[inline]
    pub(crate) fn ref_count(&self) -> usize {
        let inner = self.inner();
        inner.ref_count_with_flags.get() >> FLAG_BIT_COUNT
    }
}

impl<T: ?Sized> Clone for Rc<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.inc_ref();
        if !self.is_tracked() {
            // TODO: Track this.
            // self.set_tracked();
        }
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
        if self.ref_count() == 0 {
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
