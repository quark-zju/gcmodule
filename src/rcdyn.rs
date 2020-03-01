use crate::rc::Rc;
use crate::trace::Trace;

/// Type-erased `Rc<T>` with interfaces needed by GC.
pub trait RcDyn {}

impl<T: Trace> RcDyn for Rc<T> {}
