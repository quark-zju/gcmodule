use gcmodule::{Cc, Trace, Tracer};
use gcmodule_derive::Trace as DeriveTrace;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_empty() {
    #[derive(DeriveTrace)]
    struct S0;

    #[derive(DeriveTrace)]
    enum E0 {}

    #[derive(DeriveTrace)]
    enum E1 {
        _A,
    }
}

#[test]
fn test_named_struct() {
    #[derive(DeriveTrace)]
    struct S0 {
        a: u8,
        b: String,
        c: &'static str,
    }
    assert!(!S0::is_type_tracked());

    #[derive(DeriveTrace)]
    struct S1 {
        a: Option<Box<dyn Trace>>,
        b: (u32, u64),
    }
    assert!(S1::is_type_tracked());
}

#[test]
fn test_type_parameters() {
    #[derive(DeriveTrace)]
    struct S0<T: Trace> {
        a: Option<T>,
    }
    assert!(!S0::<u8>::is_type_tracked());
    assert!(S0::<Box<dyn Trace>>::is_type_tracked());

    #[derive(DeriveTrace)]
    struct S1<T: Trace> {
        a: Option<Rc<T>>,
    }
    assert!(!S1::<u8>::is_type_tracked());
    assert!(!S1::<Box<dyn Trace>>::is_type_tracked());
}

#[test]
fn test_field_skip() {
    #[derive(DeriveTrace)]
    struct S2 {
        #[trace(skip)]
        _a: Option<Box<dyn Trace>>,
        _b: (u32, u64),
    }
    assert!(!S2::is_type_tracked());
}

#[test]
fn test_container_skip() {
    #[derive(DeriveTrace)]
    #[trace(skip)]
    struct S0 {
        _a: Option<Box<dyn Trace>>,
        _b: (u32, u64),
    }
    assert!(!S0::is_type_tracked());

    #[derive(DeriveTrace)]
    #[trace(skip)]
    union U0 {
        _b: (u32, u64),
    }
    assert!(!U0::is_type_tracked());

    #[derive(DeriveTrace)]
    #[trace(skip)]
    enum E0 {
        _A(Option<Box<dyn Trace>>),
        _B(u32, u64),
    }
    assert!(!E0::is_type_tracked());
}

#[test]
fn test_recursive_struct() {
    #[derive(DeriveTrace)]
    struct A {
        b: Box<dyn Trace>,
        #[trace(tracking(ignore))]
        a: Box<A>,
    }
    assert!(A::is_type_tracked());

    #[derive(DeriveTrace)]
    struct B {
        #[trace(tracking(ignore))]
        b: Box<B>,
    }
    assert!(!B::is_type_tracked());

    #[derive(DeriveTrace)]
    #[trace(tracking(force))]
    struct C {
        c: (Box<C>, Box<dyn Trace>),
    }
    assert!(C::is_type_tracked());
}

#[test]
fn test_unnamed_struct() {
    #[derive(DeriveTrace)]
    struct S0(u8, String);
    assert!(!S0::is_type_tracked());

    #[derive(DeriveTrace)]
    struct S1(u8, Box<dyn Trace>);
    assert!(S1::is_type_tracked());
}

#[test]
fn test_real_cycles() {
    #[derive(DeriveTrace, Default)]
    struct S(RefCell<Option<Box<dyn Trace>>>);
    {
        let s1: Cc<S> = Default::default();
        let s2: Cc<S> = Default::default();
        let s3: Cc<S> = Default::default();
        *(s1.0.borrow_mut()) = Some(Box::new(s2.clone()));
        *(s2.0.borrow_mut()) = Some(Box::new(s3.clone()));
        *(s3.0.borrow_mut()) = Some(Box::new(s1.clone()));
    }
    assert_eq!(gcmodule::collect_thread_cycles(), 3);
}

#[test]
fn test_with() {
    struct Child;

    fn trace_child(_child: &Child, _tracer: &mut Tracer) {}

    #[derive(DeriveTrace)]
    struct Parent(#[trace(with(trace_child))] Child);
}
