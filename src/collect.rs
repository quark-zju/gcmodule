use crate::rc::GcHeader;
use std::cell::RefCell;
use std::ops::DerefMut;
use std::pin::Pin;

thread_local!(pub(crate) static GC_LIST: RefCell<Pin<Box<GcHeader>>> = RefCell::new(new_gc_list()));

fn new_gc_list() -> Pin<Box<GcHeader>> {
    let mut pinned = Box::pin(GcHeader {
        prev: std::ptr::null_mut(),
        next: std::ptr::null_mut(),
        value: None,
    });
    let header: &mut GcHeader = pinned.deref_mut();
    header.prev = header;
    header.next = header;
    pinned
}
