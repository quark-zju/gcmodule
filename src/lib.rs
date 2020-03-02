#![deny(missing_docs)]
#![cfg_attr(feature = "nightly", feature(coerce_unsized), feature(unsize))]

//! Reference cycle garbage collection inspired by
//! [cpython](https://github.com/python/cpython/).
//!
//! The type [`Cc<T>`](struct.Cc.html) provides shared ownership of a value of type `T`,
//! similar to `std::rc::Rc<T>`. Unlike `Rc<T>`, [`collect_thread_cycles`](fn.collect_thread_cycles.html)
//! can be used to drop unreachable values that form circular references.
//!
//! ## Cloning references
//!
//! Similar to `Rc<T>`, use `clone()` to get cloned references.
//!
//! ```
//! use gcmodule::Cc;
//! let foo = Cc::new(vec![1, 2, 3]);
//! let foo_cloned = foo.clone();
//! // foo and foo_cloned both point to the same `vec![1, 2, 3]`.
//! assert!(std::ptr::eq(&foo[0], &foo_cloned[0]));
//! ```
//!
//! ## Collecting cycles
//!
//! Use [`collect_thread_cycles()`](fn.collect_thread_cycles.html) to collect thread-local garbage.
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
//! gcmodule::collect_thread_cycles();  // This will drop a and b.
//! ```
//!
//! ## Definiting new types
//!
//! `Cc<T>` requires [`Trace`](trait.Trace.html) implemented for `T` so the
//! collector knows how values are referred.
//!
//! ### Acyclic types
//!
//! Types that do not store references to other objects, or only store
//! references to atomic types (such as numbers or strings), can opt-out
//! cyclic garbage collection for performance. This can be done by using
//! the [`untrack!`](macro.untrack.html) macro:
//!
//! ```
//! use gcmodule::{untrack, Cc};
//! struct Foo(String);
//! struct Bar;
//! untrack!(Foo, Bar); // Opt-out cycle collector. Ref-count still works.
//!
//! let foo = Cc::new(Foo("abc".to_string()));
//! let bar = Cc::new(Bar);
//! let foo_cloned = foo.clone(); // Share the same `"abc"` with `foo`.
//! drop(foo); // The ref count of `"abc"` drops from 2 to 1.
//! drop(foo_cloned); // `"abc"` will be dropped here..
//! # drop(bar);
//! ```
//!
//! ### Container types
//!
//! Types that store references to other objects, need to implement the
//! `Trace` trait. For example:
//!
//! ```
//! use gcmodule::{Cc, Trace, Tracer};
//! struct Foo<T1, T2>(T1, T2, u8);
//!
//! impl<T1: Trace, T2: Trace> Trace for Foo<T1, T2> {
//!     fn trace(&self, tracer: &mut Tracer) {
//!         self.0.trace(tracer);
//!         self.1.trace(tracer);
//!     }
//! }
//!
//! let foo = Cc::new(Foo(Foo(Cc::new(1), 2, 3), Cc::new("abc"), 10));
//! # drop(foo);
//! ```

mod cc;
mod cc_impls;
mod collect;
#[cfg(test)]
mod debug;
#[cfg(test)]
mod tests;
#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
mod trace;
mod trace_impls;

pub use cc::Cc;
pub use collect::collect_thread_cycles;
pub use trace::{Trace, Tracer};

#[cfg(not(test))]
mod debug {
    pub(crate) fn log<S1: ToString, S2: ToString>(_func: impl Fn() -> (S1, S2)) {}
}
