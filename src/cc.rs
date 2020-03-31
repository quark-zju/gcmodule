use crate::collect;
use crate::debug;
use crate::trace::Trace;
use crate::trace::Tracer;
use std::any::Any;
use std::cell::Cell;
use std::cell::UnsafeCell;
use std::mem;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::panic::UnwindSafe;
use std::ptr::NonNull;

// Types not tracked by the cycle collector:
//
//     CcBox<T>
//     +-----------+ <---+--- Cc<T> (pointer)
//     | ref_count |     |
//     +-----------+     +--- Cc<T> (pointer)
//     | T (data)  |
//     +-----------+
//
// Types not tracked by the cycle collector:
//
//     CcBoxWithHeader<T>
//     +----------------------+
//     | GcHeader | next      | (GcHeader is in a linked list)
//     |          | prev      |
//     |          | vptr<T>   |
//     +----------------------+ <---+--- Cc<T> (pointer)
//     | CcBox<T> | ref_count |     |
//     |          | T (data)  |     +--- Cc<T> (pointer)
//     +----------------------+

/// Internal metadata used by the cycle collector.
#[repr(C)]
pub struct GcHeader {
    pub(crate) next: Cell<*const GcHeader>,
    pub(crate) prev: Cell<*const GcHeader>,

    /// Vtable of (`&CcBox<T> as &dyn CcDyn`)
    pub(crate) ccdyn_vptr: Cell<*mut ()>,
}

/// The data shared by multiple `Cc<T>` pointers.
#[repr(C)]
pub(crate) struct CcBox<T: ?Sized> {
    /// The lowest REF_COUNT_SHIFT bits are used for metadata.
    /// The higher bits are used for ref count.
    pub(crate) ref_count: Cell<usize>,

    #[cfg(test)]
    pub(crate) name: String,
    value: UnsafeCell<ManuallyDrop<T>>,
}

/// The real layout if `T` is tracked by the collector. The main APIs still use
/// the `CcBox` type. This type is only used for allocation and deallocation.
#[repr(C)]
pub(crate) struct CcBoxWithGcHeader<T: ?Sized> {
    pub(crate) gc_header: GcHeader,
    cc_box: CcBox<T>,
}

/// A single-threaded reference-counting pointer that integrates
/// with cyclic garbage collection.
///
/// See [module level documentation](index.html) for more details.
///
/// [`Cc`](struct.Cc.html) is not thread-safe. It does not implement `Send`
/// or `Sync`:
///
/// ```compile_fail
/// use std::ops::Deref;
/// use gcmodule::Cc;
/// let cc = Cc::new(5);
/// std::thread::spawn(move || {
///     println!("{}", cc.deref());
/// });
/// ```
pub struct Cc<T: ?Sized>(NonNull<CcBox<T>>);

// `ManuallyDrop<T>` does not implement `UnwindSafe`. But `CcBox::drop` does
// make sure `T` is dropped. If `T` is unwind-safe, so does `CcBox<T>`.
impl<T: UnwindSafe + ?Sized> UnwindSafe for CcBox<T> {}

// `NonNull` does not implement `UnwindSafe`. But `Cc` only uses it
// as a "const" pointer. If `T` is unwind-safe, so does `Cc<T>`.
impl<T: UnwindSafe + ?Sized> UnwindSafe for Cc<T> {}

/// Whether a `GcHeader` exists before the `CcBox<T>`.
const REF_COUNT_MASK_TRACKED: usize = 0b1;

/// Whether `T` in the `CcBox<T>` has been dropped.
const REF_COUNT_MASK_DROPPED: usize = 0b10;

/// Number of bits used for metadata.
const REF_COUNT_SHIFT: i32 = 2;

/// Type-erased `Cc<T>` with interfaces needed by GC.
pub(crate) trait CcDyn {
    /// Returns the reference count for cycle detection.
    fn gc_ref_count(&self) -> usize;

    /// Visit referents for cycle detection.
    fn gc_traverse(&self, tracer: &mut Tracer);

    /// Get an cloned `Cc<dyn Trace>`. This has 2 purposes:
    /// - Keep a reference so `CcBox<T>` is not released in the next step.
    ///   So metadata like `ref_count` can still be read.
    /// - Operate on the object.
    fn gc_clone(&self) -> Cc<dyn Trace>;
}

/// A dummy implementation without drop side-effects.
pub(crate) struct CcDummy;

impl CcDummy {
    pub(crate) fn ccdyn_vptr() -> *mut () {
        let mut dummy = CcDummy;
        // safety: To access vtable pointer. Stable API cannot do it.
        let fat_ptr: [*mut (); 2] = unsafe { mem::transmute(&mut dummy as &mut dyn CcDyn) };
        fat_ptr[1]
    }
}

impl CcDyn for CcDummy {
    fn gc_ref_count(&self) -> usize {
        1
    }
    fn gc_traverse(&self, _tracer: &mut Tracer) {}
    fn gc_clone(&self) -> Cc<dyn Trace> {
        panic!("bug: CcDummy::gc_clone should never be called");
    }
}

impl GcHeader {
    /// Insert into a linked list.
    fn insert_into_linked_list(&self, prev: &GcHeader, value: &dyn CcDyn) {
        debug_assert!(self.next.get().is_null());
        let next = prev.next.get();
        self.prev.set(prev.deref());
        self.next.set(next);
        unsafe {
            // safety: The linked list is maintained, and pointers are valid.
            (&*next).prev.set(self);
            // safety: To access vtable pointer. Test by test_gc_header_value.
            let fat_ptr: [*mut (); 2] = mem::transmute(value);
            self.ccdyn_vptr.set(fat_ptr[1]);
        }
        prev.next.set(self);
    }

    /// Remove from a linked list.
    fn remove_from_linked_list(&self) {
        debug_assert!(!self.next.get().is_null());
        debug_assert!(!self.prev.get().is_null());
        let next = self.next.get();
        let prev = self.prev.get();
        // safety: The linked list is maintained. Pointers in it are valid.
        unsafe {
            (*prev).next.set(next);
            (*next).prev.set(prev);
        }
        self.next.set(std::ptr::null_mut());
    }

    /// Create an empty header.
    pub(crate) fn empty() -> Self {
        Self {
            next: Cell::new(std::ptr::null()),
            prev: Cell::new(std::ptr::null()),
            ccdyn_vptr: Cell::new(CcDummy::ccdyn_vptr()),
        }
    }

    /// Get the trait object to operate on the actual `CcBox`.
    pub(crate) fn value(&self) -> &dyn CcDyn {
        // safety: To build trait object from self and vtable pointer.
        // Test by test_gc_header_value_consistency().
        unsafe {
            let fat_ptr: (*const (), *mut ()) = (
                (self as *const GcHeader).offset(1) as _,
                self.ccdyn_vptr.get(),
            );
            mem::transmute(fat_ptr)
        }
    }
}

impl<T: Trace> Cc<T> {
    /// Constructs a new [`Cc<T>`](struct.Cc.html).
    pub fn new(value: T) -> Cc<T> {
        let is_tracked = T::is_type_tracked();
        let cc_box = CcBox {
            ref_count: Cell::new(
                (1 << REF_COUNT_SHIFT)
                    + if is_tracked {
                        REF_COUNT_MASK_TRACKED
                    } else {
                        0
                    },
            ),
            value: UnsafeCell::new(ManuallyDrop::new(value)),
            #[cfg(test)]
            name: debug::NEXT_DEBUG_NAME.with(|n| n.get().to_string()),
        };
        let ccbox_ptr: *mut CcBox<T> = if is_tracked {
            // Create a GcHeader before the CcBox. This is similar to cpython.
            let gc_header = GcHeader::empty();
            let cc_box_with_header = CcBoxWithGcHeader { gc_header, cc_box };
            let mut boxed = Box::new(cc_box_with_header);
            // Fix-up fields in GcHeader. This is done after the creation of the
            // Box so the memory addresses are stable.
            collect::GC_LIST.with(|ref_head| {
                let head = ref_head.borrow();
                boxed
                    .gc_header
                    .insert_into_linked_list(&head, &boxed.cc_box);
            });
            debug_assert_eq!(
                mem::size_of::<GcHeader>() + mem::size_of::<CcBox<T>>(),
                mem::size_of::<CcBoxWithGcHeader<T>>()
            );
            let ptr: *mut CcBox<T> = &mut boxed.cc_box;
            Box::leak(boxed);
            ptr
        } else {
            Box::into_raw(Box::new(cc_box))
        };
        // safety: ccbox_ptr cannot be null from the above code.
        let non_null = unsafe { NonNull::new_unchecked(ccbox_ptr) };
        let result = Self(non_null);
        if is_tracked {
            debug::log(|| (result.debug_name(), "new (CcBoxWithGcHeader)"));
        } else {
            debug::log(|| (result.debug_name(), "new (CcBox)"));
        }
        result
    }

    /// Convert to `Cc<dyn Trace>`.
    pub fn into_dyn(self) -> Cc<dyn Trace> {
        #[cfg(feature = "nightly")]
        {
            // Requires CoerceUnsized, which is currently unstable.
            self
        }

        // safety: Trait object magic. Test by test_dyn_downcast.
        #[cfg(not(feature = "nightly"))]
        unsafe {
            // XXX: This depends on rust internals. But it works on stable.
            // Replace this with CoerceUnsized once that becomes stable.
            // Cc<dyn Trace> has 2 usize values: The first one is the same
            // as Cc<T>. The second one is the vtable. The vtable pointer
            // is the same as the second pointer of `&dyn Trace`.
            let mut fat_ptr: [usize; 2] = mem::transmute(self.deref() as &dyn Trace);
            let self_ptr: usize = mem::transmute(self);
            fat_ptr[0] = self_ptr;
            mem::transmute(fat_ptr)
        }
    }
}

impl<T: Trace + Clone> Cc<T> {
    /// Update the value `T` in a copy-on-write way.
    ///
    /// If the ref count is 1, the value is updated in-place.
    /// Otherwise a new `Cc<T>` will be created.
    pub fn cow_update(&mut self, mut update_func: impl FnMut(&mut T)) {
        let need_clone = self.ref_count() > 1;
        if need_clone {
            let mut value = <Cc<T>>::deref(self).clone();
            update_func(&mut value);
            *self = Cc::new(value);
        } else {
            let value_ptr: *mut ManuallyDrop<T> = self.inner().value.get();
            let value_mut: &mut T = unsafe { &mut *value_ptr }.deref_mut();
            update_func(value_mut);
        }
    }
}

impl<T: ?Sized> CcBox<T> {
    #[inline]
    fn is_tracked(&self) -> bool {
        (self.ref_count.get() & REF_COUNT_MASK_TRACKED) != 0
    }

    #[inline]
    fn is_dropped(&self) -> bool {
        (self.ref_count.get() & REF_COUNT_MASK_DROPPED) != 0
    }

    #[inline]
    fn gc_header(&self) -> &GcHeader {
        debug_assert!(self.is_tracked());
        // safety: See `Cc::new`. GcHeader is before CcBox for tracked objects.
        unsafe { cast_ref(self, -(mem::size_of::<GcHeader>() as isize)) }
    }

    #[inline]
    fn inc_ref(&self) {
        let new_count = self.ref_count.get() + (1 << REF_COUNT_SHIFT);
        self.ref_count.set(new_count);
    }

    #[inline]
    fn dec_ref(&self) {
        let new_count = self.ref_count.get() - (1 << REF_COUNT_SHIFT);
        self.ref_count.set(new_count);
    }

    #[inline]
    fn ref_count(&self) -> usize {
        self.ref_count.get() >> REF_COUNT_SHIFT
    }

    #[inline]
    fn set_dropped(&self) {
        let new_value = self.ref_count.get() | REF_COUNT_MASK_DROPPED;
        self.ref_count.set(new_value);
        debug_assert!(self.is_dropped());
    }

    #[inline]
    pub(crate) fn drop_t(&self) {
        if !self.is_dropped() {
            // This might trigger recursive drops. So set the "dropped" bit
            // first to prevent infinite recursive drops.
            self.set_dropped();
            debug::log(|| (self.debug_name(), "drop (T)"));
            // safety: is_dropped() check ensures T is only dropped once. Other
            // places (ex. gc collector) ensure that T is no longer accessed.
            unsafe { ManuallyDrop::drop(&mut *(self.value.get())) };
        }
    }

    fn trace_t(&self, tracer: &mut Tracer) {
        if !self.is_tracked() {
            return;
        }
        debug::log(|| (self.debug_name(), "trace"));
        // For other non-`Cc<T>` container types, `trace` visit referents,
        // is recursive, and does not call `tracer` directly. For `Cc<T>`,
        // `trace` stops here, is non-recursive, and does apply `tracer`
        // to the actual `GcHeader`. It's expected that the upper layer
        // calls `gc_traverse` on everything (not just roots).
        tracer(self.gc_header());
    }

    pub(crate) fn debug_name(&self) -> &str {
        #[cfg(test)]
        {
            self.name.as_ref()
        }
        #[cfg(not(test))]
        {
            unreachable!()
        }
    }
}

impl<T: ?Sized> Cc<T> {
    #[inline]
    pub(crate) fn inner(&self) -> &CcBox<T> {
        // safety: CcBox lifetime maintained by ref count. Pointer is valid.
        unsafe { self.0.as_ref() }
    }

    #[inline]
    fn inc_ref(&self) {
        self.inner().inc_ref()
    }

    #[inline]
    fn dec_ref(&self) {
        self.inner().dec_ref()
    }

    #[inline]
    pub(crate) fn ref_count(&self) -> usize {
        self.inner().ref_count()
    }

    pub(crate) fn debug_name(&self) -> &str {
        self.inner().debug_name()
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
        self.inner().deref()
    }
}

impl<T: ?Sized> Deref for CcBox<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        debug_assert!(
            !self.is_dropped(),
            concat!(
                "bug: accessing a dropped CcBox detected\n",
                "This usually happens after ignoring another panic triggered by the collector."
            )
        );
        // safety: CcBox (and its value) lifetime maintained by ref count.
        // If `Trace` is implemented correctly then the GC won't drop_t()
        // incorrectly and this pointer is valid. Otherwise the above
        // assertion can prevent UBs on debug build.
        unsafe { &*self.value.get() }
    }
}

fn drop_ccbox<T: ?Sized>(cc_box: &mut CcBox<T>) {
    // safety: See Cc::new. The pointer was created by Box::into_raw.
    let cc_box: Box<CcBox<T>> = unsafe { Box::from_raw(cc_box) };
    let is_tracked = cc_box.is_tracked();
    // Drop T if it hasn't been dropped yet.
    cc_box.drop_t();
    if is_tracked {
        // The real object is CcBoxWithGcHeader. Drop that instead.
        debug::log(|| (cc_box.debug_name(), "drop (CcBoxWithGcHeader)"));
        // safety: See Cc::new for CcBoxWithGcHeader.
        let gc_box: Box<CcBoxWithGcHeader<T>> = unsafe { cast_box(cc_box) };
        gc_box.gc_header.remove_from_linked_list();
        drop(gc_box);
    } else {
        debug::log(|| (cc_box.debug_name(), "drop (CcBox)"));
        drop(cc_box);
    }
}

impl<T: ?Sized> Drop for Cc<T> {
    fn drop(&mut self) {
        self.dec_ref();
        debug::log(|| (self.debug_name(), format!("drop ({})", self.ref_count())));
        if self.ref_count() == 0 {
            // safety: CcBox lifetime maintained by ref count.
            drop_ccbox(unsafe { self.0.as_mut() });
        }
    }
}

impl<T: Trace> CcDyn for CcBox<T> {
    fn gc_ref_count(&self) -> usize {
        self.ref_count()
    }

    fn gc_traverse(&self, tracer: &mut Tracer) {
        debug::log(|| (self.debug_name(), "gc_traverse"));
        T::trace(self.deref(), tracer)
    }

    fn gc_clone(&self) -> Cc<dyn Trace> {
        self.inc_ref();
        debug::log(|| {
            let msg = format!("gc_clone ({})", self.ref_count());
            (self.debug_name(), msg)
        });
        // safety: The pointer is compatible. The mutability is different only
        // to satisfy NonNull (NonNull::new requires &mut). The returned value
        // is still "immutable".
        let ptr: NonNull<CcBox<T>> = unsafe { mem::transmute(self) };
        Cc::<T>(ptr).into_dyn()
    }
}

impl<T: Trace> Trace for Cc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        T::is_type_tracked()
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Trace::as_any(self.deref())
    }
}

impl Trace for Cc<dyn Trace> {
    fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Trace::as_any(self.deref())
    }
}

impl Cc<dyn Trace> {
    /// Attempt to downcast to the specified type.
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.deref().as_any().and_then(|any| any.downcast_ref())
    }

    /// Attempt to downcast to the specified `Cc<T>` type.
    pub fn downcast<T: Trace>(self) -> Result<Cc<T>, Cc<dyn Trace>> {
        if self.downcast_ref::<T>().is_some() {
            // safety: type T is checked above. The first pointer of the fat
            // pointer (Cc<dyn Trace>) matches the raw CcBox pointer.
            let fat_ptr: (*mut CcBox<T>, *mut ()) = unsafe { mem::transmute(self) };
            let non_null = unsafe { NonNull::new_unchecked(fat_ptr.0) };
            let result: Cc<T> = Cc(non_null);
            Ok(result)
        } else {
            Err(self)
        }
    }
}

#[cfg(feature = "nightly")]
impl<T: ?Sized + std::marker::Unsize<U>, U: ?Sized> std::ops::CoerceUnsized<Cc<U>> for Cc<T> {}

#[inline]
unsafe fn cast_ref<T: ?Sized, R>(value: &T, offset_bytes: isize) -> &R {
    let ptr: *const T = value;
    let ptr: *const u8 = ptr as _;
    let ptr = ptr.offset(offset_bytes);
    &*(ptr as *const R)
}

#[inline]
unsafe fn cast_box<T: ?Sized>(value: Box<CcBox<T>>) -> Box<CcBoxWithGcHeader<T>> {
    let mut ptr: *const CcBox<T> = Box::into_raw(value);

    // ptr can be "thin" (1 pointer) or "fat" (2 pointers).
    // Change the first byte to point to the GcHeader.
    let pptr: *mut *const CcBox<T> = &mut ptr;
    let pptr: *mut *const GcHeader = pptr as _;
    *pptr = (*pptr).offset(-1);
    let ptr: *mut CcBoxWithGcHeader<T> = mem::transmute(ptr);
    Box::from_raw(ptr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Check that `GcHeader::value()` returns a working trait object.
    #[test]
    fn test_gc_header_value() {
        let v1: Cc<Box<dyn Trace>> = Cc::new(Box::new(1));
        assert_eq!(v1.ref_count(), 1);

        let v2 = v1.clone();
        assert_eq!(v1.ref_count(), 2);
        assert_eq!(v2.ref_count(), 2);

        let v3: &dyn CcDyn = v1.inner() as &dyn CcDyn;
        assert_eq!(v3.gc_ref_count(), 2);

        let v4: &dyn CcDyn = v2.inner().gc_header().value();
        assert_eq!(v4.gc_ref_count(), 2);
    }

    #[test]
    fn test_dyn_downcast() {
        let v: Cc<Vec<u8>> = Cc::new(vec![1u8, 2, 3]);
        let v: Cc<dyn Trace> = v.into_dyn();
        let downcasted: &Vec<u8> = v.downcast_ref().unwrap();
        assert_eq!(downcasted, &vec![1, 2, 3]);

        let v = v.downcast::<usize>().map(|_| panic!()).unwrap_err();
        let v: Cc<Vec<u8>> = v.downcast().map_err(|_| panic!()).unwrap();
        assert_eq!(v.deref(), &vec![1, 2, 3]);
    }

    #[cfg(feature = "nightly")]
    #[test]
    fn test_unsize_coerce() {
        let _v: Cc<dyn Trace> = Cc::new(vec![1u8, 2, 3]);
    }
}
