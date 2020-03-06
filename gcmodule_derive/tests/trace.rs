use gcmodule::{Cc, Trace};
use gcmodule_derive::Trace;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_named_struct() {
    #[derive(Trace)]
    struct S0 {
        a: u8,
        b: String,
        c: &'static str,
    }
    assert!(!S0::is_type_tracked());

    #[derive(Trace)]
    struct S1 {
        a: Option<Box<dyn Trace>>,
        b: (u32, u64),
    }
    assert!(S1::is_type_tracked());
}

#[test]
fn test_type_parameters() {
    #[derive(Trace)]
    struct S0<T: Trace> {
        a: Option<T>,
    }
    assert!(!S0::<u8>::is_type_tracked());
    assert!(S0::<Box<dyn Trace>>::is_type_tracked());

    #[derive(Trace)]
    struct S1<T: Trace> {
        a: Option<Rc<T>>,
    }
    assert!(!S1::<u8>::is_type_tracked());
    assert!(!S1::<Box<dyn Trace>>::is_type_tracked());
}

#[test]
fn test_skip() {
    #[derive(Trace)]
    struct S2 {
        #[trace(skip)]
        _a: Option<Box<dyn Trace>>,
        b: (u32, u64),
    }
    assert!(!S2::is_type_tracked());
}

#[test]
fn test_unnamed_struct() {
    #[derive(Trace)]
    struct S0(u8, String);
    assert!(!S0::is_type_tracked());

    #[derive(Trace)]
    struct S1(u8, Box<dyn Trace>);
    assert!(S1::is_type_tracked());
}

#[test]
fn test_real_cycles() {
    #[derive(Trace, Default)]
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
