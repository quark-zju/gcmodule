//! Test utilities.

use crate::{collect, debug, Cc, Trace, Tracer};
use std::cell::Cell;
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;

thread_local!(static NEXT_TRACKED_OVERRIDE: Cell<bool> = Cell::new(true));

/// Track count of drop(). Store result in AtomicUsize.
/// The bool value controls whether this type is tracked.
pub struct DropCounter<T>(T, Arc<AtomicUsize>);
impl<T: Trace> Trace for DropCounter<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.0.trace(tracer);
    }
    fn is_type_tracked() -> bool {
        NEXT_TRACKED_OVERRIDE.with(|a| a.get())
    }
}
impl<T> Drop for DropCounter<T> {
    fn drop(&mut self) {
        self.1.fetch_add(1, SeqCst);
    }
}

pub(crate) fn create_objects(
    n: usize,
    atomic_bits: u16,
    drop_count: Arc<AtomicUsize>,
) -> Vec<Cc<DropCounter<RefCell<Vec<Box<dyn Trace>>>>>> {
    assert!(n <= 16);
    let is_tracked = |n| -> bool { (atomic_bits >> n) & 1 == 0 };
    (0..n)
        .map(|i| {
            debug::NEXT_DEBUG_NAME.with(|n| n.set(i));
            NEXT_TRACKED_OVERRIDE.with(|a| a.set(is_tracked(i)));
            Cc::new(DropCounter(RefCell::new(Vec::new()), drop_count.clone()))
        })
        .collect()
}

/// Test a graph of n (n <= 16) nodes, with specified edges between nodes.
///
/// `atomic_bits` is a bit mask. If the i-th bit is set, then the i-th vertex
/// opts out cycle collector.
///
/// `collect_bits` is a bit mask. If the i-th bit is set, then try to collect
/// after dropping the i-th value.
pub fn test_small_graph(n: usize, edges: &[u8], atomic_bits: u16, collect_bits: u16) {
    assert!(n <= 16);
    let is_tracked = |n| -> bool { (atomic_bits >> n) & 1 == 0 };
    let drop_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let mut edge_descs: Vec<Vec<usize>> = vec![Vec::new(); n];
    {
        let values = create_objects(n, atomic_bits, drop_count.clone());
        for &edge in edges {
            let from_index = ((edge as usize) >> 4) % n;
            let to_index = ((edge as usize) & 15) % n;
            match (is_tracked(from_index), is_tracked(to_index)) {
                // Okay: tracked value can include either tracked or untracked
                // values.
                (_, true) => (),
                // Both are untracked. To avoid cycles. Only allow references
                // in one direction.
                (false, false) => {
                    if from_index >= to_index {
                        continue;
                    }
                }
                // Skip: cannot put a tracked value inside an untracked value.
                (true, false) => continue,
            }
            let mut to = values[to_index].0.borrow_mut();
            to.push(Box::new(values[from_index].clone()));
            edge_descs[to_index].push(from_index);
        }
        for (i, _value) in values.into_iter().enumerate() {
            if ((collect_bits >> i) & 1) != 0 {
                collect::collect_thread_cycles();
            }
        }
    }
    let old_dropped = drop_count.load(SeqCst);
    let collected = collect::collect_thread_cycles();
    let new_dropped = drop_count.load(SeqCst);
    assert!(
        collected + old_dropped <= new_dropped,
        "collected ({}) + old_dropped ({}) > new_dropped ({}) edges: {:?}",
        collected,
        old_dropped,
        new_dropped,
        edge_descs,
    );
    let dropped = drop_count.load(SeqCst);
    assert_eq!(drop_count.load(SeqCst), n, "dropped ({}) != n ({}) edges: {:?}", dropped, n, edge_descs);
}
