#![deny(missing_docs)]
#![cfg_attr(feature = "nightly", feature(coerce_unsized), feature(unsize))]

//! Reference cycle garbage collection inspired by
//! [cpython](https://github.com/python/cpython/).
//!
//! The type [`Cc<T>`](struct.Cc.html) provides shared ownership of a value of type `T`,
//! similar to `std::rc::Rc<T>`. Unlike `Rc<T>`, [`collect_thread_cycles`](fn.collect_thread_cycles.html)
//! can be used to drop unreachable values that form circular references.
//!
//! # Examples
//!
//! ## Cloning references
//!
//! Similar to `Rc<T>`, use `clone()` to get cloned references.
//!
//! ```
//! use gcmodule::Cc;
//! let foo = Cc::new(vec![1, 2, 3]);
//! let foo_cloned = foo.clone();
//!
//! // foo and foo_cloned both point to the same `vec![1, 2, 3]`.
//! assert!(std::ptr::eq(&foo[0], &foo_cloned[0]));
//! ```
//!
//! ## Collecting cycles
//!
//! Use [`collect_thread_cycles()`](fn.collect_thread_cycles.html) to collect
//! thread-local garbage.
//!
//! Use [`count_thread_tracked()`](fn.count_thread_tracked.html) to count how
//! many objects are tracked by the collector.
//!
//! ```
//! use gcmodule::{Cc, Trace};
//! use std::cell::RefCell;
//! {
//!     type List = Cc<RefCell<Vec<Box<dyn Trace>>>>;
//!     let a: List = Default::default();
//!     let b: List = Default::default();
//!     a.borrow_mut().push(Box::new(b.clone()));
//!     b.borrow_mut().push(Box::new(a.clone()));
//! }
//!
//! // a and b form circular references. The objects they point to are not
//! // dropped automatically, despite both variables run out of scope.
//!
//! assert_eq!(gcmodule::count_thread_tracked(), 2);   // 2 values are tracked.
//! assert_eq!(gcmodule::collect_thread_cycles(), 2);  // This will drop a and b.
//! assert_eq!(gcmodule::count_thread_tracked(), 0);   // no values are tracked.
//! ```
//!
//! ## Defining new types
//!
//! [`Cc<T>`](struct.Cc.html) requires [`Trace`](trait.Trace.html) implemented
//! for `T` so the collector knows how values are referred. That can usually
//! be done by `#[derive(Trace)]`.
//!
//! ### Acyclic types
//!
//! If a type is acyclic (cannot form reference circles about [`Cc`](struct.Cc.html)),
//! [`Trace::is_type_tracked()`](trait.Trace.html#method.is_type_tracked) will return `false`.
//!
//! ```
//! use gcmodule::{Cc, Trace};
//!
//! #[derive(Trace)]
//! struct Foo(String);
//!
//! #[derive(Trace)]
//! struct Bar;
//!
//! assert!(!Foo::is_type_tracked()); // Acyclic
//! assert!(!Bar::is_type_tracked()); // Acyclic
//!
//! let foo = Cc::new(Foo("abc".to_string()));
//! let bar = Cc::new(Bar);
//! let foo_cloned = foo.clone(); // Share the same `"abc"` with `foo`.
//! assert_eq!(gcmodule::count_thread_tracked(), 0); // The collector tracks nothing.
//! drop(foo); // The ref count of `"abc"` drops from 2 to 1.
//! drop(foo_cloned); // `"abc"` will be dropped here..
//! # drop(bar);
//! ```
//!
//! ### Container types
//!
//! Whether a container type is acyclic or not depends on its fields. Usually,
//! types without referring to trait objects or itself are considered acyclic.
//!
//! ```
//! use gcmodule::{Cc, Trace};
//!
//! #[derive(Trace)]
//! struct Foo<T1: Trace, T2: Trace>(T1, T2, u8);
//!
//! // `a` is not tracked - types are acyclic.
//! let a = Cc::new(Foo(Foo(Cc::new(1), 2, 3), Cc::new("abc"), 10));
//! assert_eq!(gcmodule::count_thread_tracked(), 0);
//!
//! // `b` is tracked because it contains a trait object.
//! let b = Cc::new(Foo(Box::new(1) as Box<dyn Trace>, 2, 3));
//! assert_eq!(gcmodule::count_thread_tracked(), 1);
//! ```
//!
//! The `#[trace(skip)]` attribute can be used to skip tracking specified fields
//! in a structure.
//!
//! ```
//! use gcmodule::{Cc, Trace};
//!
//! struct AlienStruct; // Does not implement Trace
//!
//! #[derive(Trace)]
//! struct Foo {
//!     field: String,
//!
//!     #[trace(skip)]
//!     alien: AlienStruct, // Field skipped in Trace implementation.
//! }
//! ```
//!
//! # Technical Details
//!
//! ## Memory Layouts
//!
//! [`Cc<T>`](struct.Cc.html) uses different memory layouts depending on `T`.
//!
//! ### Untracked types
//!
//! If [`<T as Trace>::is_type_tracked()`](trait.Trace.html#method.is_type_tracked)
//! returns `false`, the layout is similar to `Rc<T>` but without a `weak_count`:
//!
//! ```plain,ignore
//! Shared T                    Pointer
//! +------------------+     .-- Cc<T>
//! | ref_count: usize | <--<
//! |------------------|     '-- Cc<T>::clone()
//! | T (shared data)  | <--- Cc<T>::deref()
//! +------------------+
//! ```
//!
//! ### Tracked types
//!
//! If [`<T as Trace>::is_type_tracked()`](trait.Trace.html#method.is_type_tracked)
//! returns `true`, the layout has an extra `GcHeader` that makes the value visible
//! in a thread-local linked list:
//!
//! ```plain,ignore
//! Shared T with GcHeader
//! +------------------+
//! | gc_prev: pointer | ---> GcHeader in a linked list.
//! | gc_next: pointer |
//! | vptr<T>: pointer | ---> Pointer to the `&T as &dyn Trace` virtual table.
//! |------------------|
//! | ref_count: usize | <--- Cc<T>
//! | ---------------- |
//! | T (shared data)  | <--- Cc<T>::deref()
//! +------------------+
//! ```
//!
//! ## Incorrect `Trace` implementation
//!
//! While most public APIs provided by this library looks safe, incorrectly
//! implementing the [`Trace`](trait.Trace.html) trait has consequences.
//!
//! This library should cause no undefined behaviors (UB) even with incorrect
//! [`Trace`](trait.Trace.html) implementation on _debug_ build.
//!
//! Below are some consequences of a wrong [`Trace`](trait.Trace.html)
//! implementation.
//!
//! ### Memory leak
//!
//! If [`Trace::trace`](trait.Trace.html#method.trace) does not visit all
//! referred values, the collector might fail to detect cycles, and take
//! no actions on cycles. That causes memory leak.
//!
//! ### Panic
//!
//! If [`Trace::trace`](trait.Trace.html#method.trace) visits more values
//! than it should (for example, visit indirectly referred values, or visit
//! a directly referred value multiple times), the collector can detect
//! such issues and panic the thread with the message:
//!
//! ```plain,ignore
//! bug: unexpected ref-count after dropping cycles
//! This usually indicates a buggy Trace or Drop implementation.
//! ```
//!
//! ### Undefined behavior (UB)
//!
//! After the above panic (`bug: unexpected ref-count after dropping cycles`),
//! dereferencing a garbage-collected [`Cc<T>`](struct.Cc.html) will trigger
//! `panic!` or UB depending on whether it's a debug build or not.
//!
//! On debug build, sanity checks are added at `Cc::<T>::deref()`.
//! It will panic if `T` was garbage-collected:
//!
//! ```plain,ignore
//! bug: accessing a dropped CcBox detected
//! ```
//!
//! In other words, no UB on debug build.
//!
//! On release build the dereference would access dropped values, which is an
//! undefined behavior. Again, the UB can only happen if the [`Trace::trace`](trait.Trace.html#method.trace)
//! is implemented wrong, and panic will happen before the UB.

mod cc;
mod cc_impls;
mod collect;
#[cfg(test)]
mod debug;
mod ref_count;
#[cfg(feature = "sync")]
//mod sync;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
mod trace;
mod trace_impls;

pub use cc::Cc;
pub use collect::{collect_thread_cycles, count_thread_tracked, CcObjectSpace};
pub use trace::{Trace, Tracer};

/// Derive [`Trace`](trait.Trace.html) implementation for a structure.
///
/// # Examples
///
/// ```
/// use gcmodule::{Cc, Trace};
///
/// #[derive(Trace)]
/// struct S1(u32, String);
///
/// #[derive(Trace)]
/// struct S2<T1: Trace, T2: Trace>(T1, T2, u8);
///
/// #[derive(Trace)]
/// struct S3<T: Trace> {
///     a: S1,
///     b: Option<S2<T, u8>>,
///
///     #[trace(skip)]
///     c: AlienStruct,  // c is not tracked by the collector.
/// }
///
/// struct AlienStruct;
/// ```
#[cfg(feature = "derive")]
pub use gcmodule_derive::Trace;

#[cfg(not(test))]
mod debug {
    use std::cell::Cell;
    thread_local!(pub(crate) static NEXT_DEBUG_NAME: Cell<usize> = Default::default());
    pub(crate) fn log<S1: ToString, S2: ToString>(_func: impl Fn() -> (S1, S2)) {}
}
