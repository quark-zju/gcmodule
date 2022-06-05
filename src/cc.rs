use crate::collect;
use crate::collect::AbstractObjectSpace;
use crate::collect::ObjectSpace;
use crate::debug;
use crate::ref_count::RefCount;
use crate::trace::Trace;
use crate::trace::Tracer;
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
// Types tracked by the cycle collector:
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

/// The data shared by multiple `RawCc<T, O>` pointers.
#[repr(C)]
pub(crate) struct RawCcBox<T: ?Sized, O: AbstractObjectSpace> {
    pub(crate) ref_count: O::RefCount,

    #[cfg(test)]
    pub(crate) name: String,

    value: UnsafeCell<ManuallyDrop<T>>,
}

/// The real layout if `T` is tracked by the collector. The main APIs still use
/// the `CcBox` type. This type is only used for allocation and deallocation.
///
/// This is a private type.
#[repr(C)]
pub struct RawCcBoxWithGcHeader<T: ?Sized, O: AbstractObjectSpace> {
    header: O::Header,
    cc_box: RawCcBox<T, O>,
}

/// A single-threaded reference-counting pointer that integrates
/// with cyclic garbage collection.
///
/// See [module level documentation](index.html) for more details.
///
/// [`Cc`](type.Cc.html) is not thread-safe. It does not implement `Send`
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
pub type Cc<T> = RawCc<T, ObjectSpace>;

/// Weak reference of [`Cc`](type.Cc.html).
pub type Weak<T> = RawWeak<T, ObjectSpace>;

/// Low-level type for [`Cc<T>`](type.Cc.html).
pub struct RawCc<T: ?Sized, O: AbstractObjectSpace>(NonNull<RawCcBox<T, O>>);

/// Low-level type for [`Weak<T>`](type.Weak.html).
pub struct RawWeak<T: ?Sized, O: AbstractObjectSpace>(NonNull<RawCcBox<T, O>>);

// `ManuallyDrop<T>` does not implement `UnwindSafe`. But `CcBox::drop` does
// make sure `T` is dropped. If `T` is unwind-safe, so does `CcBox<T>`.
impl<T: UnwindSafe + ?Sized> UnwindSafe for RawCcBox<T, ObjectSpace> {}

// `NonNull` does not implement `UnwindSafe`. But `Cc` and `Weak` only use it
// as a "const" pointer. If `T` is unwind-safe, so does `Cc<T>`.
impl<T: UnwindSafe + ?Sized, O: AbstractObjectSpace> UnwindSafe for RawCc<T, O> {}
impl<T: UnwindSafe + ?Sized, O: AbstractObjectSpace> UnwindSafe for RawWeak<T, O> {}

/// Type-erased `Cc<T>` with interfaces needed by GC.
///
/// This is a private type.
pub trait CcDyn {
    /// Returns the reference count for cycle detection.
    fn gc_ref_count(&self) -> usize;

    /// Visit referents for cycle detection.
    fn gc_traverse(&self, tracer: &mut Tracer);

    /// Get an cloned `Cc<dyn Trace>`. This has 2 purposes:
    /// - Keep a reference so `CcBox<T>` is not released in the next step.
    ///   So metadata like `ref_count` can still be read.
    /// - Operate on the object.
    fn gc_clone(&self) -> Box<dyn GcClone>;

    #[cfg(feature = "debug")]
    /// Name used in collect.rs.
    fn gc_debug_name(&self) -> String {
        "?".to_string()
    }
}

/// Type-erased gc_clone result.
///
/// This is a private type.
pub trait GcClone {
    /// Force drop the value T.
    fn gc_drop_t(&self);

    /// Returns the reference count. This is useful for verification.
    fn gc_ref_count(&self) -> usize;
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
    fn gc_clone(&self) -> Box<dyn GcClone> {
        panic!("bug: CcDummy::gc_clone should never be called");
    }
}

impl<T: Trace> Cc<T> {
    /// Constructs a new [`Cc<T>`](type.Cc.html) in a thread-local storage.
    ///
    /// To collect cycles, use [`collect_thread_cycles`](fn.collect_thread_cycles.html).
    pub fn new(value: T) -> Cc<T> {
        collect::THREAD_OBJECT_SPACE.with(|space| Self::new_in_space(value, space))
    }
}

impl<T: Trace, O: AbstractObjectSpace> RawCc<T, O> {
    /// Constructs a new [`Cc<T>`](type.Cc.html) in the given
    /// [`ObjectSpace`](struct.ObjectSpace.html).
    ///
    /// To collect cycles, call `ObjectSpace::collect_cycles()`.
    pub(crate) fn new_in_space(value: T, space: &O) -> Self {
        let is_tracked = T::is_type_tracked();
        let cc_box = RawCcBox {
            ref_count: space.new_ref_count(is_tracked),
            value: UnsafeCell::new(ManuallyDrop::new(value)),
            #[cfg(test)]
            name: debug::NEXT_DEBUG_NAME.with(|n| n.get().to_string()),
        };
        let ccbox_ptr: *mut RawCcBox<T, O> = if is_tracked {
            // Create a GcHeader before the CcBox. This is similar to cpython.
            let header = space.empty_header();
            let cc_box_with_header = RawCcBoxWithGcHeader { header, cc_box };
            let mut boxed = Box::new(cc_box_with_header);
            // Fix-up fields in GcHeader. This is done after the creation of the
            // Box so the memory addresses are stable.
            space.insert(&mut boxed.header, &boxed.cc_box);
            debug_assert_eq!(
                mem::size_of::<O::Header>() + mem::size_of::<RawCcBox<T, O>>(),
                mem::size_of::<RawCcBoxWithGcHeader<T, O>>()
            );
            let ptr: *mut RawCcBox<T, O> = &mut boxed.cc_box;
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
        debug_assert_eq!(result.ref_count(), 1);
        result
    }

    /// Convert to `RawCc<dyn Trace>`.
    pub fn into_dyn(self) -> RawCc<dyn Trace, O> {
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
            let mut fat_ptr: [usize; 2] = mem::transmute(self.inner().deref() as &dyn Trace);
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
    pub fn update_with(&mut self, mut update_func: impl FnMut(&mut T)) {
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

impl<T: ?Sized, O: AbstractObjectSpace> RawCcBox<T, O> {
    #[inline]
    fn header_ptr(&self) -> *const () {
        self.header() as *const _ as _
    }

    #[inline]
    fn header(&self) -> &O::Header {
        debug_assert!(self.is_tracked());
        // safety: See `Cc::new`. GcHeader is before CcBox for tracked objects.
        unsafe { cast_ref(self, -(mem::size_of::<O::Header>() as isize)) }
    }

    #[inline]
    fn is_tracked(&self) -> bool {
        self.ref_count.is_tracked()
    }

    #[inline]
    fn is_dropped(&self) -> bool {
        self.ref_count.is_dropped()
    }

    #[inline]
    fn inc_ref(&self) -> usize {
        self.ref_count.inc_ref()
    }

    #[inline]
    fn dec_ref(&self) -> usize {
        self.ref_count.dec_ref()
    }

    #[inline]
    fn ref_count(&self) -> usize {
        self.ref_count.ref_count()
    }

    #[inline]
    fn weak_count(&self) -> usize {
        self.ref_count.weak_count()
    }

    #[inline]
    fn set_dropped(&self) -> bool {
        self.ref_count.set_dropped()
    }

    #[inline]
    pub(crate) fn drop_t(&self) {
        let already_dropped = self.set_dropped();
        if !already_dropped {
            debug::log(|| (self.debug_name(), "drop (T)"));
            // safety: is_dropped() check ensures T is only dropped once. Other
            // places (ex. gc collector) ensure that T is no longer accessed.
            unsafe { ManuallyDrop::drop(&mut *(self.value.get())) };
        }
    }

    pub(crate) fn trace_t(&self, tracer: &mut Tracer) {
        if !self.is_tracked() {
            return;
        }
        debug::log(|| (self.debug_name(), "trace"));
        // For other non-`Cc<T>` container types, `trace` visit referents,
        // is recursive, and does not call `tracer` directly. For `Cc<T>`,
        // `trace` stops here, is non-recursive, and does apply `tracer`
        // to the actual `GcHeader`. It's expected that the upper layer
        // calls `gc_traverse` on everything (not just roots).
        tracer(self.header_ptr());
    }

    pub(crate) fn debug_name(&self) -> String {
        #[cfg(test)]
        {
            self.name.clone()
        }
        #[cfg(not(test))]
        {
            #[allow(unused_mut)]
            let mut result = format!("{} at {:p}", std::any::type_name::<T>(), &self.value);

            #[cfg(all(feature = "debug", feature = "nightly"))]
            {
                if !self.is_dropped() && crate::debug::GC_DROPPING.with(|t| !t.get()) {
                    let debug = self.deref().optional_debug();
                    if !debug.is_empty() {
                        result += &format!(" {}", debug);
                    }
                }
            }

            return result;
        }
    }
}

#[cfg(all(feature = "debug", feature = "nightly"))]
pub(crate) trait OptionalDebug {
    fn optional_debug(&self) -> String;
}

#[cfg(all(feature = "debug", feature = "nightly"))]
impl<T: ?Sized> OptionalDebug for T {
    default fn optional_debug(&self) -> String {
        "".to_string()
    }
}

#[cfg(all(feature = "debug", feature = "nightly"))]
impl<T: std::fmt::Debug + ?Sized> OptionalDebug for T {
    fn optional_debug(&self) -> String {
        format!("{:?}", self)
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> RawCc<T, O> {
    /// Obtains a "weak reference", a non-owning pointer.
    pub fn downgrade(&self) -> RawWeak<T, O> {
        let inner = self.inner();
        inner.ref_count.inc_weak();
        debug::log(|| {
            (
                inner.debug_name(),
                format!("new-weak ({})", inner.ref_count.weak_count()),
            )
        });
        RawWeak(self.0)
    }

    /// Gets the reference count not considering weak references.
    #[inline]
    pub fn strong_count(&self) -> usize {
        self.ref_count()
    }

    /// Returns `true` if the two `Cc`s point to the same allocation
    #[inline]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        this.0.as_ptr() == other.0.as_ptr()
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> RawWeak<T, O> {
    /// Attempts to obtain a "strong reference".
    ///
    /// Returns `None` if the value has already been dropped.
    pub fn upgrade(&self) -> Option<RawCc<T, O>> {
        let inner = self.inner();
        // Make the below operation "atomic".
        let _locked = inner.ref_count.locked();
        if inner.is_dropped() {
            None
        } else {
            inner.inc_ref();
            debug::log(|| {
                (
                    inner.debug_name(),
                    format!("new-strong ({})", inner.ref_count.ref_count()),
                )
            });
            Some(RawCc(self.0))
        }
    }

    /// Gets the reference count not considering weak references.
    #[inline]
    pub fn strong_count(&self) -> usize {
        self.inner().ref_count()
    }

    /// Get the weak (non-owning) reference count.
    #[inline]
    pub fn weak_count(&self) -> usize {
        self.inner().weak_count()
    }

    /// Returns `true` if the two `Weak`s point to the same allocation
    #[inline]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        this.0.as_ptr() == other.0.as_ptr()
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> RawCc<T, O> {
    #[inline]
    pub(crate) fn inner(&self) -> &RawCcBox<T, O> {
        // safety: CcBox lifetime maintained by ref count. Pointer is valid.
        unsafe { self.0.as_ref() }
    }

    /// `trace` without `T: Trace` bound.
    ///
    /// Useful for structures with `Cc<T>` fields where `T` does not implement
    /// `Trace`. For example, `struct S(Cc<Box<dyn MyTrait>>)`. To implement
    /// `Trace` for `S`, it can use `Cc::trace(&self.0, tracer)`.
    #[inline]
    pub fn trace(&self, tracer: &mut Tracer) {
        self.inner().trace_t(tracer);
    }

    #[inline]
    fn inc_ref(&self) -> usize {
        self.inner().inc_ref()
    }

    #[inline]
    fn dec_ref(&self) -> usize {
        self.inner().dec_ref()
    }

    #[inline]
    pub(crate) fn ref_count(&self) -> usize {
        self.inner().ref_count()
    }

    /// Get the weak (non-owning) reference count.
    #[inline]
    pub fn weak_count(&self) -> usize {
        self.inner().weak_count()
    }

    pub(crate) fn debug_name(&self) -> String {
        self.inner().debug_name()
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> RawWeak<T, O> {
    #[inline]
    fn inner(&self) -> &RawCcBox<T, O> {
        // safety: CcBox lifetime maintained by ref count. Pointer is valid.
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> Clone for RawCc<T, O> {
    #[inline]
    fn clone(&self) -> Self {
        // In theory self.inner().ref_count.locked() is needed.
        // Practically this is an atomic operation that cannot be split so locking
        // becomes optional.
        // let _locked = self.inner().ref_count.locked();
        self.inc_ref();
        debug::log(|| (self.debug_name(), format!("clone ({})", self.ref_count())));
        Self(self.0)
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> Clone for RawWeak<T, O> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = self.inner();
        let ref_count = &inner.ref_count;
        ref_count.inc_weak();
        debug::log(|| {
            (
                inner.debug_name(),
                format!("clone-weak ({})", ref_count.weak_count()),
            )
        });
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

impl<T: ?Sized, O: AbstractObjectSpace> Deref for RawCcBox<T, O> {
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

fn drop_ccbox<T: ?Sized, O: AbstractObjectSpace>(cc_box: *mut RawCcBox<T, O>) {
    // safety: See Cc::new. The pointer was created by Box::into_raw.
    let cc_box: Box<RawCcBox<T, O>> = unsafe { Box::from_raw(cc_box) };
    let is_tracked = cc_box.is_tracked();
    if is_tracked {
        // The real object is CcBoxWithGcHeader. Drop that instead.
        // safety: See Cc::new for CcBoxWithGcHeader.
        let gc_box: Box<RawCcBoxWithGcHeader<T, O>> = unsafe { cast_box(cc_box) };
        O::remove(&gc_box.header);
        // Drop T if it hasn't been dropped yet.
        // This needs to be after O::remove so the collector won't have a
        // chance to read dropped content.
        gc_box.cc_box.drop_t();
        debug::log(|| (gc_box.cc_box.debug_name(), "drop (CcBoxWithGcHeader)"));
        drop(gc_box);
    } else {
        // Drop T if it hasn't been dropped yet.
        cc_box.drop_t();
        debug::log(|| (cc_box.debug_name(), "drop (CcBox)"));
        drop(cc_box);
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> Drop for RawCc<T, O> {
    fn drop(&mut self) {
        let ptr: *mut RawCcBox<T, O> = self.0.as_ptr();
        let inner = self.inner();
        // Block threaded collector. This is needed because "drop()" is a
        // complex operation. The whole operation needs to be "atomic".
        let _locked = inner.ref_count.locked();
        let old_ref_count = self.dec_ref();
        debug::log(|| (self.debug_name(), format!("drop ({})", self.ref_count())));
        debug_assert!(old_ref_count >= 1);
        if old_ref_count == 1 {
            if self.weak_count() == 0 {
                // safety: CcBox lifetime maintained by ref count.
                drop_ccbox(ptr);
            } else {
                inner.drop_t();
            }
        }
    }
}

impl<T: ?Sized, O: AbstractObjectSpace> Drop for RawWeak<T, O> {
    fn drop(&mut self) {
        let ptr: *mut RawCcBox<T, O> = self.0.as_ptr();
        let inner = self.inner();
        let ref_count = &inner.ref_count;
        // Block threaded collector to "freeze" the ref count, for safety.
        let _locked = ref_count.locked();
        let old_ref_count = ref_count.ref_count();
        let old_weak_count = ref_count.dec_weak();
        debug::log(|| {
            (
                inner.debug_name(),
                format!("drop-weak ({})", ref_count.weak_count()),
            )
        });
        debug_assert!(old_weak_count >= 1);
        if old_ref_count == 0 && old_weak_count == 1 {
            // safety: CcBox lifetime maintained by ref count.
            drop_ccbox(ptr);
        }
    }
}

impl<T: Trace + ?Sized, O: AbstractObjectSpace> CcDyn for RawCcBox<T, O> {
    fn gc_ref_count(&self) -> usize {
        self.ref_count()
    }

    fn gc_traverse(&self, tracer: &mut Tracer) {
        debug::log(|| (self.debug_name(), "gc_traverse"));
        T::trace(self.deref(), tracer)
    }

    fn gc_clone(&self) -> Box<dyn GcClone> {
        self.ref_count.inc_ref();
        debug::log(|| {
            let msg = format!("gc_clone ({})", self.ref_count());
            (self.debug_name(), msg)
        });
        // safety: The pointer is compatible. The mutability is different only
        // to satisfy NonNull (NonNull::new requires &mut). The returned value
        // is still "immutable". &self can also never be nonnull.
        let ptr: NonNull<RawCcBox<T, O>> =
            unsafe { NonNull::new_unchecked(self as *const _ as *mut _) };
        let cc = RawCc::<T, O>(ptr);
        Box::new(cc)
    }

    #[cfg(feature = "debug")]
    fn gc_debug_name(&self) -> String {
        self.debug_name()
    }
}

impl<T: Trace + ?Sized, O: AbstractObjectSpace> GcClone for RawCc<T, O> {
    fn gc_ref_count(&self) -> usize {
        self.ref_count()
    }

    fn gc_drop_t(&self) {
        self.inner().drop_t()
    }
}

impl<T: Trace> Trace for Cc<T> {
    fn trace(&self, tracer: &mut Tracer) {
        Cc::<T>::trace(self, tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        T::is_type_tracked()
    }
}

impl Trace for Cc<dyn Trace> {
    fn trace(&self, tracer: &mut Tracer) {
        Cc::<dyn Trace>::trace(self, tracer)
    }

    #[inline]
    fn is_type_tracked() -> bool {
        // Trait objects can be anything.
        true
    }
}

#[cfg(feature = "nightly")]
impl<T: ?Sized + std::marker::Unsize<U>, U: ?Sized, O: AbstractObjectSpace>
    std::ops::CoerceUnsized<RawCc<U, O>> for RawCc<T, O>
{
}

#[inline]
unsafe fn cast_ref<T: ?Sized, R>(value: &T, offset_bytes: isize) -> &R {
    let ptr: *const T = value;
    let ptr: *const u8 = ptr as _;
    let ptr = ptr.offset(offset_bytes);
    &*(ptr as *const R)
}

#[inline]
unsafe fn cast_box<T: ?Sized, O: AbstractObjectSpace>(
    value: Box<RawCcBox<T, O>>,
) -> Box<RawCcBoxWithGcHeader<T, O>> {
    let mut ptr: *const RawCcBox<T, O> = Box::into_raw(value);

    // ptr can be "thin" (1 pointer) or "fat" (2 pointers).
    // Change the first byte to point to the GcHeader.
    let pptr: *mut *const RawCcBox<T, O> = &mut ptr;
    let pptr: *mut *const O::Header = pptr as _;
    *pptr = (*pptr).offset(-1);
    let ptr: *mut RawCcBoxWithGcHeader<T, O> = mem::transmute(ptr);
    Box::from_raw(ptr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::Linked;

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

        let v4: &dyn CcDyn = v2.inner().header().value();
        assert_eq!(v4.gc_ref_count(), 2);
    }

    #[cfg(feature = "nightly")]
    #[test]
    fn test_unsize_coerce() {
        let _v: Cc<dyn Trace> = Cc::new(vec![1u8, 2, 3]);
    }
}
