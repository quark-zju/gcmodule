use crate::cc::AbstractCc;
mod collect;
use collect::AccObjectSpace;

#[cfg(test)]
mod tests;

/// An atomic reference-counting pointer that integrates
/// with cyclic garbage collection.
///
/// [`Acc`](struct.Acc.html) is similar to [`Cc`](struct.Cc.html). It is slower
/// but can work in multiple threads.
pub type Acc<T> = AbstractCc<T, AccObjectSpace>;

// safety: similar to `std::sync::Arc`
unsafe impl<T: Send + Sync> Send for Acc<T> {}
unsafe impl<T: Send + Sync> Sync for Acc<T> {}
