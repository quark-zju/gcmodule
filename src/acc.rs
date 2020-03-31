use crate::cc::AbstractCc;
use crate::cc::CcDyn;
use crate::cc::GcHeader;
use crate::cc::GcHeaderWithExtras;
use crate::collect;
use crate::collect::ObjectSpace;
use crate::Trace;
use parking_lot::ReentrantMutex;
use std::mem;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

/// An atomic reference-counting pointer that integrates
/// with cyclic garbage collection.
///
/// [`Acc`](struct.Acc.html) is similar to [`Cc`](struct.Cc.html). It is slower
/// but can work in multiple threads.
pub type Acc<T> = AbstractCc<T, AccObjectSpace>;

// safety: similar to `std::sync::Arc`
unsafe impl<T: Send + Sync> Send for Acc<T> {}
unsafe impl<T: Send + Sync> Sync for Acc<T> {}

pub struct AccObjectSpace {
    /// Linked list to the tracked objects.
    list: Pin<Box<GcHeader>>,
    lock: Arc<ReentrantMutex<()>>,
}

// safety: accesses are protected by mutex
unsafe impl Send for AccObjectSpace {}
unsafe impl Sync for AccObjectSpace {}

impl ObjectSpace for AccObjectSpace {
    type RefCount = AtomicUsize;
    type Extras = Arc<ReentrantMutex<()>>;

    fn insert(&self, header: &GcHeaderWithExtras<Self>, value: &dyn CcDyn) {
        debug_assert!(Arc::ptr_eq(&header.extras, &self.lock));
        let _locked = self.lock.lock();
        let header: &GcHeader = &header.gc_header;
        let prev: &GcHeader = &self.list;
        debug_assert!(header.next.get().is_null());
        let next = prev.next.get();
        header.prev.set(prev.deref());
        header.next.set(next);
        unsafe {
            // safety: The linked list is maintained, and pointers are valid.
            (&*next).prev.set(header);
            // safety: To access vtable pointer. Test by test_gc_header_value.
            let fat_ptr: [*mut (); 2] = mem::transmute(value);
            header.ccdyn_vptr.set(fat_ptr[1]);
        }
        prev.next.set(header);
    }

    #[inline]
    fn remove(header: &GcHeaderWithExtras<Self>) {
        let _locked = header.extras.lock();
        let header: &GcHeader = &header.gc_header;
        debug_assert!(!header.next.get().is_null());
        debug_assert!(!header.prev.get().is_null());
        let next = header.next.get();
        let prev = header.prev.get();
        // safety: The linked list is maintained. Pointers in it are valid.
        unsafe {
            (*prev).next.set(next);
            (*next).prev.set(prev);
        }
        header.next.set(std::ptr::null_mut());
    }

    fn default_extras(&self) -> Self::Extras {
        self.lock.clone()
    }
}

impl Default for AccObjectSpace {
    /// Constructs an empty [`AccObjectSpace`](struct.AccObjectSpace.html).
    fn default() -> Self {
        let header = collect::new_gc_list();
        Self {
            list: header,
            lock: Arc::new(ReentrantMutex::new(())),
        }
    }
}

impl AccObjectSpace {
    /// Count objects tracked by this [`ObjectSpace`](struct.ObjectSpace.html).
    pub fn count_tracked(&self) -> usize {
        let _locked = self.lock.lock();
        let list: &GcHeader = &self.list;
        let mut count = 0;
        collect::visit_list(list, |_| count += 1);
        count
    }

    /// Collect cyclic garbage tracked by this [`ObjectSpace`](struct.ObjectSpace.html).
    /// Return the number of objects collected.
    pub fn collect_cycles(&self) -> usize {
        let _locked = self.lock.lock();
        let list: &GcHeader = &self.list;
        collect::collect_list(list)
    }

    /// Constructs a new [`Acc<T>`](struct.Acc.html) in this
    /// [`AccObjectSpace`](struct.AccObjectSpace.html).
    ///
    /// The returned [`Acc<T>`](struct.Cc.html) can refer to other
    ///  `Acc`s in the same [`AccObjectSpace`](struct.AccObjectSpace.html).
    ///
    /// If an `Acc` refers to another `Acc` in another
    /// [`AccObjectSpace`](struct.AccObjectSpace.html), the cyclic collector
    /// will not be able to collect cycles.
    pub fn create<T: Trace>(&self, value: T) -> Acc<T> {
        // Lock will be taken by ObjectSpace::insert.
        Acc::new_in_space(value, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug;
    use crate::Trace;
    use std::sync::mpsc::channel;
    use std::sync::Mutex;
    use std::thread::spawn;

    type List = Acc<Mutex<Vec<Box<dyn Trace + Send + Sync>>>>;

    fn test_cross_thread_cycle(n: usize) {
        let list: Arc<Mutex<Vec<List>>> = Arc::new(Mutex::new(Vec::with_capacity(n)));
        let space = Arc::new(AccObjectSpace::default());
        assert_eq!(space.count_tracked(), 0);

        let spawn_thread = |name| {
            let value = Mutex::new(Vec::new());
            let space = space.clone();
            let list = list.clone();
            spawn(move || {
                debug::NEXT_DEBUG_NAME.with(|n| n.set(name));
                let this: List = space.create(value);
                let mut list = list.lock().unwrap();
                for other in list.iter_mut() {
                    let cloned_other = other.clone();
                    let cloned_this = this.clone();
                    this.lock().unwrap().push(Box::new(cloned_other));
                    other.lock().unwrap().push(Box::new(cloned_this));
                }
                list.push(this);
            })
        };

        let threads: Vec<_> = (0..n).map(|i| spawn_thread(i)).collect();
        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(space.count_tracked(), n);
        assert_eq!(space.collect_cycles(), 0);

        drop(list);
        assert_eq!(space.collect_cycles(), n);
    }

    #[test]
    fn test_2_thread_cycle() {
        test_cross_thread_cycle(2);
    }

    #[test]
    fn test_17_thread_cycle() {
        test_cross_thread_cycle(17);
    }

    #[test]
    fn test_100_thread_cycle() {
        test_cross_thread_cycle(100);
    }

    fn test_racy_threads(
        thread_count: usize,
        iteration_count: usize,
        create_cycles_bits: u32,
        collect_cycles_bits: u32,
    ) {
        let space = Arc::new(AccObjectSpace::default());
        let mut tx_list = Vec::with_capacity(thread_count);
        let mut rx_list = Vec::with_capacity(thread_count);
        for _ in 0..thread_count {
            let (tx, rx) = channel();
            tx_list.push(tx);
            rx_list.push(rx);
        }

        let threads: Vec<_> = rx_list
            .into_iter()
            .enumerate()
            .map(|(i, rx)| {
                let space = space.clone();
                let tx_list = tx_list.clone();
                spawn(move || {
                    for k in 0..iteration_count {
                        {
                            debug::NEXT_DEBUG_NAME.with(|n| n.set((i + 1) * 1000 + k));
                            let value = Mutex::new(Vec::new());
                            let acc: List = Acc::new_in_space(value, &space);
                            {
                                let mut locked = acc.lock().unwrap();
                                while let Ok(received) = rx.try_recv() {
                                    locked.push(received);
                                }
                            }
                            if (create_cycles_bits >> i) & 1 == 1 {
                                for j in 0..thread_count {
                                    if j % (i + 1) == 0 {
                                        let _ = tx_list[j].send(Box::new(acc.clone()));
                                    }
                                }
                            }
                        }

                        if (collect_cycles_bits >> i) & 1 == 1 {
                            space.collect_cycles();
                        }
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        space.collect_cycles();
        assert_eq!(space.count_tracked(), 0);
    }

    #[test]
    fn test_racy_threads_racy_drops() {
        test_racy_threads(32, 1000, 0, 0);
    }

    #[test]
    fn test_racy_threads_collects() {
        test_racy_threads(8, 100, 0xff, 0xff);
    }

    #[test]
    fn test_racy_threads_mixed_collects() {
        test_racy_threads(8, 100, 0b11110000, 0b10101010);
    }
}
