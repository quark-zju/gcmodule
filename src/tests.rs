use crate::debug;
use crate::testutil::test_small_graph;
use crate::{collect, Cc, Trace};
use quickcheck::quickcheck;
use std::cell::RefCell;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

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

#[test]
fn test_drop_by_ref_count() {
    let log = debug::capture_log(|| test_small_graph(3, &[], 0, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader)
1: new (CcBoxWithGcHeader)
2: new (CcBoxWithGcHeader)
0: drop (0), drop (T), drop (CcBoxWithGcHeader)
1: drop (0), drop (T), drop (CcBoxWithGcHeader)
2: drop (0), drop (T), drop (CcBoxWithGcHeader)
collect: collect_thread_cycles, 0 unreachable objects"#
    );
}

#[test]
fn test_self_referential() {
    let log = debug::capture_log(|| test_small_graph(1, &[0x00, 0x00, 0x00], 0, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader), clone (2), clone (3), clone (4), drop (3)
collect: collect_thread_cycles
0: gc_traverse, trace, trace, trace
collect: 1 unreachable objects
0: gc_clone (4), drop (T), drop (3), drop (2), drop (1), drop (0), drop (CcBoxWithGcHeader)"#
    );
}

#[test]
fn test_3_object_cycle() {
    // 0 -> 1 -> 2 -> 0
    let log = debug::capture_log(|| test_small_graph(3, &[0x01, 0x12, 0x20], 0, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader)
1: new (CcBoxWithGcHeader)
2: new (CcBoxWithGcHeader)
0: clone (2)
1: clone (2)
2: clone (2)
0: drop (1)
1: drop (1)
2: drop (1)
collect: collect_thread_cycles
2: gc_traverse
1: trace, gc_traverse
0: trace, gc_traverse
2: trace
collect: 3 unreachable objects
2: gc_clone (2)
1: gc_clone (2)
0: gc_clone (2)
2: drop (T)
1: drop (1), drop (T)
0: drop (1), drop (T)
2: drop (1), drop (0), drop (CcBoxWithGcHeader)
1: drop (0), drop (CcBoxWithGcHeader)
0: drop (0), drop (CcBoxWithGcHeader)"#
    );
}

#[test]
fn test_2_object_cycle_with_another_incoming_reference() {
    let log = debug::capture_log(|| test_small_graph(3, &[0x02, 0x20, 0x10], 0, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader)
1: new (CcBoxWithGcHeader)
2: new (CcBoxWithGcHeader)
0: clone (2)
2: clone (2)
1: clone (2)
0: drop (1)
1: drop (1)
2: drop (1)
collect: collect_thread_cycles
2: gc_traverse
0: trace
1: gc_traverse
0: gc_traverse
2: trace
1: trace
collect: 3 unreachable objects
2: gc_clone (2)
1: gc_clone (2)
0: gc_clone (2)
2: drop (T)
0: drop (1)
1: drop (T)
0: drop (T)
2: drop (1)
1: drop (1)
2: drop (0), drop (CcBoxWithGcHeader)
1: drop (0), drop (CcBoxWithGcHeader)
0: drop (0), drop (CcBoxWithGcHeader)"#
    );
}

#[test]
fn test_2_object_cycle_with_another_outgoing_reference() {
    let log = debug::capture_log(|| test_small_graph(3, &[0x02, 0x20, 0x01], 0, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader)
1: new (CcBoxWithGcHeader)
2: new (CcBoxWithGcHeader)
0: clone (2)
2: clone (2)
0: clone (3), drop (2)
1: drop (0), drop (T)
0: drop (1)
1: drop (CcBoxWithGcHeader)
2: drop (1)
collect: collect_thread_cycles
2: gc_traverse
0: trace, gc_traverse
2: trace
collect: 2 unreachable objects
2: gc_clone (2)
0: gc_clone (2)
2: drop (T)
0: drop (1), drop (T)
2: drop (1), drop (0), drop (CcBoxWithGcHeader)
0: drop (0), drop (CcBoxWithGcHeader)"#
    );
}

/// Mixed tracked and untracked values.
#[test]
fn test_simple_mixed_graph() {
    let log = debug::capture_log(|| test_small_graph(2, &[0, 0x00], 0b10, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader)
1: new (CcBox)
0: clone (2), clone (3), drop (2)
1: drop (0), drop (T), drop (CcBox)
collect: collect_thread_cycles
0: gc_traverse, trace, trace
collect: 1 unreachable objects
0: gc_clone (3), drop (T), drop (2), drop (1), drop (0), drop (CcBoxWithGcHeader)"#
    );

    let log = debug::capture_log(|| test_small_graph(2, &[0, 0x10], 0b10, 0));
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader)
1: new (CcBox)
0: clone (2)
1: clone (2)
0: drop (1)
1: drop (1)
collect: collect_thread_cycles
0: gc_traverse, trace
collect: 1 unreachable objects
0: gc_clone (2), drop (T), drop (1)
1: drop (0), drop (T), drop (CcBox)
0: drop (0), drop (CcBoxWithGcHeader)"#
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
0: new (CcBoxWithGcHeader)
1: new (CcBoxWithGcHeader)
0: clone (2)
collect: collect_thread_cycles
1: gc_traverse
0: trace, gc_traverse
1: gc_traverse
0: trace, gc_traverse
collect: 0 unreachable objects
0: drop (1)
collect: collect_thread_cycles
1: gc_traverse
0: trace, gc_traverse
1: gc_traverse
0: trace, gc_traverse
collect: 0 unreachable objects
1: drop (0), drop (T)
0: drop (0), drop (T), drop (CcBoxWithGcHeader)
1: drop (CcBoxWithGcHeader)
collect: collect_thread_cycles, 0 unreachable objects"#
    );
}

quickcheck! {
    fn test_quickcheck_16_vertex_graph(edges: Vec<u8>, atomic_bits: u16, collect_bits: u16) -> bool {
        test_small_graph(16, &edges, atomic_bits, collect_bits);
        true
    }
}
