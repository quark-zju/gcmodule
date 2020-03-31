use std::cell::Cell;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};

pub(crate) trait Usize {
    fn new(value: usize) -> Self;
    fn get(&self) -> usize;
    #[inline]
    fn get_relaxed(&self) -> usize {
        self.get()
    }
    fn fetch_add(&self, value: usize) -> usize;
    fn fetch_sub(&self, value: usize) -> usize;
    fn fetch_or(&self, value: usize) -> usize;
}

impl Usize for Cell<usize> {
    #[inline]
    fn new(value: usize) -> Self {
        Cell::new(value)
    }

    #[inline]
    fn get(&self) -> usize {
        Cell::get(self)
    }

    #[inline]
    fn fetch_add(&self, value: usize) -> usize {
        let previous_value = self.get();
        self.set(previous_value + value);
        previous_value
    }

    #[inline]
    fn fetch_sub(&self, value: usize) -> usize {
        let previous_value = self.get();
        self.set(previous_value - value);
        previous_value
    }

    #[inline]
    fn fetch_or(&self, value: usize) -> usize {
        let previous_value = self.get();
        self.set(previous_value | value);
        previous_value
    }
}

impl Usize for AtomicUsize {
    #[inline]
    fn new(value: usize) -> Self {
        AtomicUsize::new(value)
    }

    #[inline]
    fn get(&self) -> usize {
        self.load(Acquire)
    }

    #[inline]
    fn get_relaxed(&self) -> usize {
        self.load(Relaxed)
    }

    #[inline]
    fn fetch_add(&self, value: usize) -> usize {
        AtomicUsize::fetch_add(self, value, AcqRel)
    }

    #[inline]
    fn fetch_sub(&self, value: usize) -> usize {
        AtomicUsize::fetch_sub(self, value, AcqRel)
    }

    #[inline]
    fn fetch_or(&self, value: usize) -> usize {
        AtomicUsize::fetch_or(self, value, AcqRel)
    }
}
