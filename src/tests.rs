use crate::testutil::test_small_graph;
use crate::{collect, Cc, Trace, Tracer};
use crate::{debug, with_thread_object_space};
use std::cell::Cell;
use std::cell::RefCell;
use std::ops::Deref;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

#[test]
fn test_simple_untracked() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    struct X(&'static str);
    impl Trace for X {
        fn is_type_tracked() -> bool {
            false
        }
    }
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
            assert_eq!(collect::count_thread_tracked(), 0);
        }
        assert!(!DROPPED.load(SeqCst));
    }
    assert!(DROPPED.load(SeqCst));
}

#[test]
fn test_simple_tracked() {
    static DROPPED: AtomicBool = AtomicBool::new(false);
    struct X(&'static str);
    impl Trace for X {
        fn is_type_tracked() -> bool {
            true
        }
    }
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
            assert_eq!(collect::count_thread_tracked(), 1);
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
        assert_eq!(collect::count_thread_tracked(), 2);
    }
    assert_eq!(collect::collect_thread_cycles(), 2);
    assert_eq!(collect::count_thread_tracked(), 0);
}

#[test]
fn test_simple_non_trait_cycles() {
    // cycles without using trait objects.
    type C = Cc<RefCell<Option<Box<T>>>>;
    #[derive(Default, Clone)]
    struct T(C);
    use crate::Tracer;
    impl Trace for T {
        fn trace(&self, t: &mut Tracer) {
            self.0.trace(t);
        }
        fn is_type_tracked() -> bool {
            true
        }
    }
    {
        let t1: T = Default::default();
        debug::NEXT_DEBUG_NAME.with(|n| n.set(1));
        let t2: T = T(Cc::new(RefCell::new(Some(Box::new(t1.clone())))));
        (*t1.0.borrow_mut()) = Some(Box::new(t2.clone()));

        // The collector runs if RefCell is borrowed.
        let _borrowed = t1.0.borrow_mut();
        assert_eq!(collect::collect_thread_cycles(), 0);
    }
    assert_eq!(collect::collect_thread_cycles(), 2);
}

#[test]
fn test_weakref_without_cycles() {
    let log = debug::capture_log(|| {
        let s1 = Cc::new("S".to_string());
        let w1 = s1.downgrade();
        let s2 = w1.upgrade().unwrap();
        let w2 = w1.clone();
        assert_eq!(s2.strong_count(), 2);
        assert_eq!(s2.weak_count(), 2);
        assert_eq!(w2.strong_count(), 2);
        assert_eq!(w2.weak_count(), 2);
        drop(s1);
        drop(s2);
        let w3 = w2.clone();
        assert!(w3.upgrade().is_none());
        assert!(w2.upgrade().is_none());
        assert!(w1.upgrade().is_none());
        assert_eq!(w3.strong_count(), 0);
        assert_eq!(w3.weak_count(), 3);
    });
    assert_eq!(
        log,
        r#"
0: new (CcBox), new-weak (1), new-strong (2), clone-weak (2), drop (1), drop (0), drop (T), clone-weak (3), drop-weak (2), drop-weak (1), drop-weak (0), drop (CcBox)"#
    );
}

#[test]
fn test_weakref_with_cycles() {
    let log = debug::capture_log(|| {
        debug::NEXT_DEBUG_NAME.with(|n| n.set(1));
        let a: Cc<RefCell<Vec<Box<dyn Trace>>>> = Cc::new(RefCell::new(Vec::new()));
        assert_eq!(a.strong_count(), 1);
        debug::NEXT_DEBUG_NAME.with(|n| n.set(2));
        let b: Cc<RefCell<Vec<Box<dyn Trace>>>> = Cc::new(RefCell::new(Vec::new()));
        a.borrow_mut().push(Box::new(b.clone()));
        b.borrow_mut().push(Box::new(a.clone()));
        assert_eq!(a.strong_count(), 2);
        assert_eq!(a.weak_count(), 0);
        let wa = a.downgrade();
        assert_eq!(a.weak_count(), 1);
        let wa1 = wa.clone();
        assert_eq!(a.weak_count(), 2);
        let wb = b.downgrade();
        assert_eq!(wa.strong_count(), 2);
        assert_eq!(wa.weak_count(), 2);
        assert_eq!(wb.weak_count(), 1);
        drop(a);
        drop(b);
        assert!(wa.upgrade().is_some());
        assert!(wb.upgrade().is_some());
        assert_eq!(collect::collect_thread_cycles(), 2);
        assert!(wa.upgrade().is_none());
        assert!(wa1.upgrade().is_none());
        assert!(wb.upgrade().is_none());
        assert!(wb.clone().upgrade().is_none());
        assert_eq!(wa.weak_count(), 2);
        assert_eq!(wa.strong_count(), 0);
    });
    assert_eq!(
        log,
        r#"
1: new (CcBoxWithGcHeader)
2: new (CcBoxWithGcHeader), clone (2)
1: clone (2), new-weak (1), clone-weak (2)
2: new-weak (1)
1: drop (1)
2: drop (1)
1: new-strong (2), drop (1)
2: new-strong (2), drop (1)
collect: collect_thread_cycles
2: gc_traverse
1: trace, gc_traverse
2: trace
collect: 2 unreachable objects
2: gc_clone (2)
1: gc_clone (2)
2: drop (T)
1: drop (1), drop (T)
2: drop (1), drop (0)
1: drop (0)
2: clone-weak (2), drop-weak (1), drop-weak (0), drop (CcBoxWithGcHeader)
1: drop-weak (1), drop-weak (0), drop (CcBoxWithGcHeader)"#
    );
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

#[test]
#[cfg_attr(miri, ignore)]
fn test_update_with() {
    // Update on a unique value.
    let log = debug::capture_log(|| {
        let mut cc = Cc::new(30);
        cc.update_with(|i| *i = *i + 1);
        assert_eq!(cc.deref(), &31);
    });
    assert_eq!(log, "\n0: new (CcBox), drop (0), drop (T), drop (CcBox)");

    // Update on a non-unique value.
    let log = debug::capture_log(|| {
        debug::NEXT_DEBUG_NAME.with(|n| n.set(0));
        let cc1 = Cc::new(30);
        let mut cc2 = cc1.clone();
        debug::NEXT_DEBUG_NAME.with(|n| n.set(3));
        cc2.update_with(|i| *i = *i + 1);
        assert_eq!(cc1.deref(), &30);
        assert_eq!(cc2.deref(), &31);
    });
    assert_eq!(
        log,
        r#"
0: new (CcBox), clone (2)
3: new (CcBox)
0: drop (1)
3: drop (0), drop (T), drop (CcBox)
0: drop (0), drop (T), drop (CcBox)"#
    );

    // Update on a tracked, non-unique value.
    let log = debug::capture_log(|| {
        #[derive(Clone)]
        struct V(usize);
        impl Trace for V {
            fn is_type_tracked() -> bool {
                true
            }
        }

        debug::NEXT_DEBUG_NAME.with(|n| n.set(0));
        let cc1: Cc<V> = Cc::new(V(30));
        let mut cc2 = cc1.clone();
        debug::NEXT_DEBUG_NAME.with(|n| n.set(3));
        cc2.update_with(|i| i.0 = i.0 + 1);
        assert_eq!(cc1.deref().0, 30);
        assert_eq!(cc2.deref().0, 31);
    });
    assert_eq!(
        log,
        r#"
0: new (CcBoxWithGcHeader), clone (2)
3: new (CcBoxWithGcHeader)
0: drop (1)
3: drop (0), drop (T), drop (CcBoxWithGcHeader)
0: drop (0), drop (T), drop (CcBoxWithGcHeader)"#
    );
}

#[derive(Default)]
struct DuplicatedVisits {
    a: RefCell<Option<Box<dyn Trace>>>,
    extra_times: Cell<usize>,
}
impl Trace for DuplicatedVisits {
    fn trace(&self, tracer: &mut Tracer) {
        // incorrectly visit "a" twice.
        self.a.trace(tracer);
        for _ in 0..self.extra_times.get() {
            self.a.trace(tracer);
        }
    }
    fn is_type_tracked() -> bool {
        true
    }
}
impl panic::UnwindSafe for DuplicatedVisits {}

fn capture_panic_message<R, F: Fn() -> R + panic::UnwindSafe>(func: F) -> String {
    match panic::catch_unwind(func) {
        Ok(_) => "(no panic happened)".to_string(),
        Err(e) => {
            if let Some(s) = e.downcast_ref::<String>() {
                return s.clone();
            } else if let Some(s) = e.downcast_ref::<&'static str>() {
                return s.to_string();
            } else {
                "(panic information is not a string)".to_string()
            }
        }
    }
}

#[test]
fn test_trace_impl_double_visits() {
    let v: Cc<DuplicatedVisits> = Default::default();
    v.extra_times.set(1);
    *(v.a.borrow_mut()) = Some(Box::new(v.clone()));

    let message = capture_panic_message(|| collect::collect_thread_cycles());
    assert!(message.contains("bug: unexpected ref-count after dropping cycles"));

    // The `CcBox<_>` was "forced dropped" as a side effect.
    // So accessing `v` becomes invalid.
    // For performance reasons, this is a debug assertion.
    #[cfg(debug_assertions)]
    {
        let message = capture_panic_message(move || {
            let _ = v.deref();
        });
        assert!(message.contains("bug: accessing a dropped CcBox detected"));
    }
}

#[test]
#[ignore = "causes memory leak, thus causing valgrind to error"]
fn leak() {
    let a = Cc::new(1);
    let b = Cc::new((a.clone(), 1));
    with_thread_object_space(|s| s.leak());
    assert_eq!(crate::count_thread_tracked(), 0);
    assert_eq!(*a, 1);
    let _ = b;
}

#[cfg(not(miri))]
quickcheck::quickcheck! {
    fn test_quickcheck_16_vertex_graph(edges: Vec<u8>, atomic_bits: u16, collect_bits: u16) -> bool {
        test_small_graph(16, &edges, atomic_bits, collect_bits);
        true
    }
}
