mod cc;
mod collect;
#[cfg(test)]
mod debug;
#[cfg(test)]
mod tests;
mod trace;
mod trace_impls;

pub use cc::Cc;
pub use collect::collect_cycles;
pub use trace::{Trace, Tracer};

#[cfg(not(test))]
mod debug {
    pub(crate) fn log<S1: ToString, S2: ToString>(_func: impl Fn() -> (S1, S2)) {}
}
