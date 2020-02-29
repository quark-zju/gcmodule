use crate::rc::Rc;

/// Type-erased `Rc<T>` with interfaces needed by GC.
pub trait RcDyn {}

impl<T: ?Sized> RcDyn for Rc<T> {}
