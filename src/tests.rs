use crate::debug;
use crate::{collect, Cc, Trace, Tracer};
use quickcheck::quickcheck;
use std::cell::RefCell;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;

#[test]
fn test_simple_untracked() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    struct X(&'static str);
    crate::untrack!(X);
    impl Drop for X {
        fn drop(&mut self) {
            DROPPED.store(true, SeqCst);
        }
    }
    {
        let v1 = Cc::new(X("abc"));
        {
            let v2 = v1.clone();
            assert_eq!(v1.deref().0, v2.deref().0);
        }
        assert!(!DROPPED.load(SeqCst));
    }
    assert!(DROPPED.load(SeqCst));
}

#[test]
fn test_simple_tracked() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    struct X(&'static str);
    impl Trace for X {}
    impl Drop for X {
        fn drop(&mut self) {
            DROPPED.store(true, SeqCst);
        }
    }
    {
        let v1 = Cc::new(X("abc"));
        {
            let v2 = v1.clone();
            assert_eq!(v1.deref().0, v2.deref().0);
        }
        assert!(!DROPPED.load(SeqCst));
    }
    assert!(DROPPED.load(SeqCst));
}

#[test]
fn test_simple_cycles() {
    assert_eq!(collect::collect_thread_cycles(), 0);
    {
        let a: Cc<RefCell<Vec<Box<dyn Trace>>>> = Cc::new(RefCell::new(Vec::new()));
        let b: Cc<RefCell<Vec<Box<dyn Trace>>>> = Cc::new(RefCell::new(Vec::new()));
        assert_eq!(collect::collect_thread_cycles(), 0);
        {
            let mut a = a.borrow_mut();
            a.push(Box::new(b.clone()));
        }
        {
            let mut b = b.borrow_mut();
            b.push(Box::new(a.clone()));
        }
        assert_eq!(collect::collect_thread_cycles(), 0);
    }
    assert_eq!(collect::collect_thread_cycles(), 2);
}

/// Track count of drop().
struct DropCounter<T>(T, Arc<AtomicUsize>, bool);
impl<T: Trace> Trace for DropCounter<T> {
    fn trace(&self, tracer: &mut Tracer) {
        self.0.trace(tracer);
    }
    fn is_type_tracked(&self) -> bool {
        self.2
    }
}
impl<T> Drop for DropCounter<T> {
    fn drop(&mut self) {
        self.1.fetch_add(1, SeqCst);
    }
}

/// Test a graph of n (n <= 16) nodes, with specified edges between nodes.
///
/// `atomic_bits` is a bit mask. If the i-th bit is set, then the i-th vertex
/// opts out cycle collector.
///
/// `collect_bits` is a bit mask. If the i-th bit is set, then try to collect
/// after dropping the i-th value.
fn test_small_graph(n: usize, edges: &[u8], atomic_bits: u16, collect_bits: u16) {
    assert!(n <= 16);
    let is_tracked = |n| -> bool { (atomic_bits >> n) & 1 == 0 };
    let drop_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let mut edge_descs: Vec<Vec<usize>> = vec![Vec::new(); n];
    {
        let values: Vec<Cc<DropCounter<RefCell<Vec<Box<dyn Trace>>>>>> = (0..n)
            .map(|i| {
                debug::NEXT_DEBUG_NAME.with(|n| n.set(i));
                Cc::new(DropCounter(
                    RefCell::new(Vec::new()),
                    drop_count.clone(),
                    is_tracked(i),
                ))
            })
            .collect();
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
    assert!(
        drop_count.load(SeqCst) == n,
        "dropped ({}) != n ({}) edges: {:?}",
        dropped,
        n,
        edge_descs,
    );
}

#[test]
fn test_drop_by_ref_count() {
    let log = debug::capture_log(|| test_small_graph(3, &[], 0, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: track, clone (2), new
2: track, clone (2), new
0: drop (1, tracked), untrack, drop (0)
1: drop (1, tracked), untrack, drop (0)
2: drop (1, tracked), untrack, drop (0)
collect: collect_thread_cycles, 0 unreachable objects"#
    );
}

#[test]
fn test_self_referential() {
    let log = debug::capture_log(|| test_small_graph(1, &[0x00, 0x00, 0x00], 0, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new, clone (3), clone (4), clone (5), drop (4)
collect: collect_thread_cycles
0: gc_traverse, trace, trace, trace
collect: 1 unreachable objects
0: gc_prepare_drop, untrack, gc_force_drop
?: drop (ignored), drop (ignored), drop (ignored), gc_mark_for_release, drop (release)"#
    );
}

#[test]
fn test_3_object_cycle() {
    // 0 -> 1 -> 2 -> 0
    let log = debug::capture_log(|| test_small_graph(3, &[0x01, 0x12, 0x20], 0, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: track, clone (2), new
2: track, clone (2), new
0: clone (3)
1: clone (3)
2: clone (3)
0: drop (2)
1: drop (2)
2: drop (2)
collect: collect_thread_cycles
2: gc_traverse
1: trace, gc_traverse
0: trace, gc_traverse
2: trace
collect: 3 unreachable objects
2: gc_prepare_drop
1: gc_prepare_drop
0: gc_prepare_drop
2: untrack, gc_force_drop
?: drop (ignored)
1: untrack, gc_force_drop
?: drop (ignored)
0: untrack, gc_force_drop
?: drop (ignored), gc_mark_for_release, drop (release), gc_mark_for_release, drop (release), gc_mark_for_release, drop (release)"#
    );
}

#[test]
fn test_2_object_cycle_with_another_incoming_reference() {
    let log = debug::capture_log(|| test_small_graph(3, &[0x02, 0x20, 0x10], 0, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: track, clone (2), new
2: track, clone (2), new
0: clone (3)
2: clone (3)
1: clone (3)
0: drop (2)
1: drop (2)
2: drop (2)
collect: collect_thread_cycles
2: gc_traverse
0: trace
1: gc_traverse
0: gc_traverse
2: trace
1: trace
collect: 3 unreachable objects
2: gc_prepare_drop
1: gc_prepare_drop
0: gc_prepare_drop
2: untrack, gc_force_drop
?: drop (ignored)
1: untrack, gc_force_drop
0: untrack, gc_force_drop
?: drop (ignored), drop (ignored), gc_mark_for_release, drop (release), gc_mark_for_release, drop (release), gc_mark_for_release, drop (release)"#
    );
}

#[test]
fn test_2_object_cycle_with_another_outgoing_reference() {
    let log = debug::capture_log(|| test_small_graph(3, &[0x02, 0x20, 0x01], 0, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: track, clone (2), new
2: track, clone (2), new
0: clone (3)
2: clone (3)
0: clone (4), drop (3)
1: drop (1, tracked), untrack, drop (0)
0: drop (2)
2: drop (2)
collect: collect_thread_cycles
2: gc_traverse
0: trace, gc_traverse
2: trace
collect: 2 unreachable objects
2: gc_prepare_drop
0: gc_prepare_drop
2: untrack, gc_force_drop
?: drop (ignored)
0: untrack, gc_force_drop
?: drop (ignored), gc_mark_for_release, drop (release), gc_mark_for_release, drop (release)"#
    );
}

/// Mixed tracked and untracked values.
#[test]
fn test_simple_mixed_graph() {
    let log = debug::capture_log(|| test_small_graph(2, &[0, 0x00], 0b10, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: new
0: clone (3), clone (4), drop (3)
1: drop (0)
collect: collect_thread_cycles
0: gc_traverse, trace, trace
collect: 1 unreachable objects
0: gc_prepare_drop, untrack, gc_force_drop
?: drop (ignored), drop (ignored), gc_mark_for_release, drop (release)"#
    );

    let log = debug::capture_log(|| test_small_graph(2, &[0, 0x10], 0b10, 0));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: new
0: clone (3)
1: clone (2)
0: drop (2)
1: drop (1)
collect: collect_thread_cycles
0: gc_traverse, trace
1: trace
collect: 1 unreachable objects
0: gc_prepare_drop, untrack, gc_force_drop
?: drop (ignored)
1: drop (0)
?: gc_mark_for_release, drop (release)"#
    )
}

#[test]
fn test_collect_multi_times() {
    // Keep 0, 1. 1: [0]; Drop 0, then Drop 1.
    let edges = [0x01];
    let log = debug::capture_log(|| test_small_graph(2, &edges, 0, 3));
    assert_eq!(
        log,
        r#"
0: track, clone (2), new
1: track, clone (2), new
0: clone (3)
collect: collect_thread_cycles
1: gc_traverse
0: trace, gc_traverse
1: gc_traverse
0: trace, gc_traverse
collect: 0 unreachable objects
0: drop (2)
collect: collect_thread_cycles
1: gc_traverse
0: trace, gc_traverse
1: gc_traverse
0: trace, gc_traverse
collect: 0 unreachable objects
1: drop (1, tracked), untrack, drop (0)
0: drop (1, tracked), untrack, drop (0)
collect: collect_thread_cycles, 0 unreachable objects"#
    );
}

#[test]
fn test_dyn_downcast() {
    let v: Cc<dyn Trace> = Cc::new(vec![1u8, 2, 3]).into_dyn();
    let downcasted: &Vec<u8> = v.downcast_ref().unwrap();
    assert_eq!(downcasted, &vec![1, 2, 3]);
}

#[cfg(feature = "nightly")]
#[test]
fn test_unsize_coerce() {
    let _v: Cc<dyn Trace> = Cc::new(vec![1u8, 2, 3]);
}

quickcheck! {
    fn test_quickcheck_16_vertex_graph(edges: Vec<u8>, atomic_bits: u16, collect_bits: u16) -> bool {
        test_small_graph(16, &edges, atomic_bits, collect_bits);
        true
    }
}
