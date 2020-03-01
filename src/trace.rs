use crate::cc::GcHeader;

pub type Tracer<'a> = dyn FnMut(&mut GcHeader) + 'a;

pub trait Trace {
    /// Traverse through objects referred by this value.
    fn trace(&self, _tracer: &mut Tracer) {}

    /// Whether this type should be tracked by the cycle collector.
    /// This provides an optimization that makes atomic types opt
    /// out the cycle collector.
    ///
    /// This is ideally an associated constant. However that is
    /// impossible due to compiler limitations.
    /// See https://doc.rust-lang.org/error-index.html#E0038.
    fn is_type_tracked(&self) -> bool {
        true
    }

    /// Name used for debugging purpose.
    #[inline(never)]
    fn debug_name(&self) -> &str {
        "<unnamed>"
    }
}
