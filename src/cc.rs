use crate::collect;
use crate::debug;
use crate::trace::Trace;
use crate::trace::Tracer;
use std::any::Any;
use std::cell::Cell;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::ptr::NonNull;

/// Internal metadata used by the cycle collector.
pub struct GcHeader {
    pub(crate) next: *mut GcHeader,
    pub(crate) prev: *mut GcHeader,

    // NOTE: `value` is mainly for type erasure. Is there a way to avoid `value`
    // to reduce memory footprint without hurting performance?
    pub(crate) value: Box<dyn CcDyn>,
}

struct CcBox<T: ?Sized> {
    pub(crate) gc_header: *mut GcHeader,
    pub(crate) ref_count: Cell<usize>,
    #[cfg(test)]
    pub(crate) name: String,
    value: ManuallyDrop<T>,
}

/// A single-threaded reference-counting pointer that integrates
/// with cyclic garbage collection.
///
/// See [module level documentation](index.html) for more details.
pub struct Cc<T: ?Sized>(NonNull<CcBox<T>>);

const REF_COUNT_MARKED_FOR_DROP: usize = usize::max_value();
const REF_COUNT_MARKED_FOR_FREE: usize = REF_COUNT_MARKED_FOR_DROP - 1;

/// Type-erased `Cc<T>` with interfaces needed by GC.
pub(crate) trait CcDyn {
    /// Returns the reference count for cycle detection.
    fn gc_ref_count(&self) -> usize;

    /// Visit referents for cycle detection.
    fn gc_traverse(&self, tracer: &mut Tracer);

    /// Mark for drop. Transfer ownship of `Box<dyn CcDyn>` from `self`.
    /// Must call `gc_force_drop_without_release` for the next step.
    fn gc_prepare_drop(&mut self) -> Box<dyn CcDyn>;

    /// Call customized drop logic (`T::drop`) without releasing memory.
    /// Remove self from the GC list.
    /// Must call `gc_mark_for_release` for the next step.
    fn gc_force_drop_without_release(&mut self);

    /// Mark for releasing memory.
    /// At this point there should be only one owner of the `CcBox<T>`, which is
    /// the `Box<dyn CcDyn>` returned by `gc_prepare_drop`. Dropping that owner
    /// will release the memory of `CcBox<T>`.
    fn gc_mark_for_release(&mut self);
}

/// A dummy implementation without drop side-effects.
pub(crate) struct CcDummy;

impl CcDyn for CcDummy {
    fn gc_ref_count(&self) -> usize {
        1
    }
    fn gc_traverse(&self, _tracer: &mut Tracer) {}
    fn gc_prepare_drop(&mut self) -> Box<dyn CcDyn> {
        Box::new(Self)
    }
    fn gc_force_drop_without_release(&mut self) {}
    fn gc_mark_for_release(&mut self) {}
}

impl<T: Trace> Cc<T> {
    /// Constructs a new [`Cc<T>`](struct.Cc.html).
    pub fn new(value: T) -> Cc<T> {
        let rc_box = CcBox {
            gc_header: std::ptr::null_mut(),
            ref_count: Cell::new(1),
            value: ManuallyDrop::new(value),
            #[cfg(test)]
            name: debug::NEXT_DEBUG_NAME.with(|n| n.get().to_string()),
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
        debug::log(|| (result.debug_name(), "new"));
        result
    }
}

impl<T: ?Sized> Cc<T> {
    #[inline]
    fn inner(&self) -> &CcBox<T> {
        unsafe { self.0.as_ref() }
    }

    #[inline]
    fn inner_mut(&mut self) -> &mut CcBox<T> {
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
        debug::log(|| (self.debug_name(), "untrack"));
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

    pub(crate) fn debug_name(&self) -> &str {
        #[cfg(test)]
        {
            &self.inner().name
        }
        #[cfg(not(test))]
        {
            unreachable!()
        }
    }
}

impl<T: Trace> Cc<T> {
    fn gc_track(&mut self, prev: &mut Pin<Box<GcHeader>>) {
        if self.is_tracked() {
            return;
        }
        debug::log(|| (self.debug_name(), "track"));
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

impl<T> Clone for Cc<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.inc_ref();
        debug::log(|| (self.debug_name(), format!("clone ({})", self.ref_count())));
        Self(self.0)
    }
}

impl<T: ?Sized> Deref for Cc<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner().value
    }
}

impl<T: ?Sized> Drop for Cc<T> {
    fn drop(&mut self) {
        match self.ref_count() {
            1 => {
                // ref_count will be 0. Drop and release memory.
                debug_assert!(!self.is_tracked());
                debug::log(|| (self.debug_name(), "drop (0)"));
                unsafe {
                    let mut rc_box: Box<CcBox<T>> = Box::from_raw(self.0.as_mut());
                    ManuallyDrop::drop(&mut rc_box.value);
                    drop(rc_box);
                }
            }
            2 if self.is_tracked() => {
                // ref_count will be 1, held by the CcDyn in GcHeader.
                // Opt-out GC and ref_count will be 0.
                debug::log(|| (self.debug_name(), "drop (1, tracked)"));
                self.dec_ref();
                self.gc_untrack();
            }
            REF_COUNT_MARKED_FOR_DROP => {
                // Do nothing. Drop is being done by gc_force_drop_without_release().
                debug::log(|| ("?", "drop (ignored)"));
            }
            REF_COUNT_MARKED_FOR_FREE => {
                // T was dropped by gc_force_drop_without_release.
                // Just release the memory.
                let rc_box: Box<CcBox<T>> = unsafe { Box::from_raw(self.0.as_mut()) };
                debug::log(|| ("?", "drop (release)"));
                drop(rc_box);
            }
            0 => {
                panic!("bug: ref_count should not be 0");
            }
            _ => {
                self.dec_ref();
                debug::log(|| (self.debug_name(), format!("drop ({})", self.ref_count())));
            }
        }
    }
}

impl<T: Trace> CcDyn for Cc<T> {
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
        debug::log(|| (self.debug_name(), "gc_traverse"));
        self.deref().trace(tracer)
    }

    fn gc_prepare_drop(&mut self) -> Box<dyn CcDyn> {
        debug::log(|| (self.debug_name(), "gc_prepare_drop"));
        debug_assert!(self.is_tracked());
        self.inner().ref_count.set(REF_COUNT_MARKED_FOR_DROP);
        let mut result: Box<dyn CcDyn> = Box::new(CcDummy);
        std::mem::swap(&mut result, unsafe {
            &mut (*self.inner_mut().gc_header).value
        });
        result
    }

    fn gc_force_drop_without_release(&mut self) {
        debug_assert!(self.is_tracked());
        debug_assert!(self.ref_count() == REF_COUNT_MARKED_FOR_DROP);
        self.gc_untrack();
        debug::log(|| (self.debug_name(), "gc_force_drop"));
        let inner = self.inner_mut();
        unsafe { ManuallyDrop::drop(&mut inner.value) };
    }

    fn gc_mark_for_release(&mut self) {
        debug::log(|| ("?", "gc_mark_for_release"));
        debug_assert!(!self.is_tracked());
        debug_assert!(self.ref_count() == REF_COUNT_MARKED_FOR_DROP);
        self.inner().ref_count.set(REF_COUNT_MARKED_FOR_FREE);
    }
}

impl<T: Trace + ?Sized> Trace for Cc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        debug::log(|| (self.debug_name(), "trace"));
        // For other non-`Cc<T>` container types, `trace` visit referents,
        // is recursive, and does not call `tracer` directly. For `Cc<T>`,
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
        T::is_type_tracked(self.deref())
    }

    fn as_any(&self) -> Option<&dyn Any> {
        T::as_any(self.deref())
    }
}

#[cfg(feature = "nightly")]
impl<T: ?Sized + std::marker::Unsize<U>, U: ?Sized> std::ops::CoerceUnsized<Cc<U>> for Cc<T> {}
