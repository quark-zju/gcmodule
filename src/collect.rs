// The main idea comes from cpython 3.8's `gcmodule.c` [1].
//
// [1]: https://github.com/python/cpython/blob/v3.8.0/Modules/gcmodule.c

// NOTE: Consider adding generation support if necessary. It won't be too hard.

use crate::cc::CcDummy;
use crate::cc::CcDyn;
use crate::cc::GcHeader;
use crate::debug;
use std::cell::RefCell;
use std::ops::DerefMut;
use std::pin::Pin;

/// Collect cyclic garbage in the current thread.
/// Return the number of objects collected.
pub fn collect_thread_cycles() -> usize {
    debug::log(|| ("collect", "collect_thread_cycles"));
    GC_LIST.with(|list| {
        let list: *mut GcHeader = {
            let mut list = list.borrow_mut();
            list.deref_mut().deref_mut()
        };
        collect_list(list)
    })
}

thread_local!(pub(crate) static GC_LIST: RefCell<Pin<Box<GcHeader>>> = RefCell::new(new_gc_list()));

/// Create an empty linked list with a dummy GcHeader.
fn new_gc_list() -> Pin<Box<GcHeader>> {
    let mut pinned = Box::pin(GcHeader {
        prev: std::ptr::null_mut(),
        next: std::ptr::null_mut(),
        value: Box::new(CcDummy),
    });
    let header: &mut GcHeader = pinned.deref_mut();
    header.prev = header;
    header.next = header;
    pinned
}

/// Scan the specified linked list. Collect cycles.
fn collect_list(list: *mut GcHeader) -> usize {
    update_refs(list);
    subtract_refs(list);
    release_unreachable(list)
}

/// Visit the linked list.
fn visit_list(list: *mut GcHeader, mut func: impl FnMut(&mut GcHeader)) {
    // Skip the first dummy entry.
    let mut ptr = unsafe { (*list).next };
    while ptr != list {
        let header: &mut GcHeader = unsafe { ptr.as_mut() }.unwrap();
        ptr = header.next;
        func(header);
    }
}

const PREV_MASK_COLLECTING: usize = 1;
const PREV_SHIFT: u32 = 1;

/// Temporarily use `GcHeader.prev` as `gc_ref_count`.
/// Idea comes from https://bugs.python.org/issue33597.
fn update_refs(list: *mut GcHeader) {
    visit_list(list, |header| {
        let ref_count = header.value.gc_ref_count();
        header.prev = ((ref_count << PREV_SHIFT) | PREV_MASK_COLLECTING) as _;
    });
}

/// Subtract ref counts in `GcHeader.prev` by calling the non-recursive
/// `Trace::trace` on every track objects.
///
/// After this, potential unreachable objects will have ref count down
/// to 0. If vertexes in a connected component _all_ have ref count 0,
/// they are unreachable and can be released.
fn subtract_refs(list: *mut GcHeader) {
    let mut tracer = |header: &mut GcHeader| {
        if is_collecting(header) {
            debug_assert!(!is_unreachable(header));
            edit_gc_ref_count(header, -1);
        }
    };
    visit_list(list, |header| {
        header.value.gc_traverse(&mut tracer);
    });
}

/// Mark objects as reachable recursively. So ref count 0 means unreachable
/// values. This also removes the COLLECTING flag for reachable objects so
/// unreachable objects all have the COLLECTING flag set.
fn mark_reachable(list: *mut GcHeader) {
    fn revive(header: &mut GcHeader) {
        // hasn't visited?
        if is_collecting(header) {
            unset_collecting(header);
            if is_unreachable(header) {
                edit_gc_ref_count(header, 1); // revive
            }
            header.value.gc_traverse(&mut revive); // revive recursively
        }
    }
    visit_list(list, |header| {
        if is_collecting(header) && !is_unreachable(header) {
            unset_collecting(header);
            header.value.gc_traverse(&mut revive)
        }
    });
}

/// Release unreachable objects in the linked list.
fn release_unreachable(list: *mut GcHeader) -> usize {
    // Mark reachable objects. For example, A refers B. A's gc_ref_count
    // is 1 while B's gc_ref_count is 0. In this case B should be revived
    // by A's non-zero gc_ref_count.
    mark_reachable(list);

    let mut count = 0;

    // Count unreachable objects. This is an optimization to avoid realloc.
    visit_list(list, |header| {
        if is_unreachable(header) {
            count += 1;
        }
    });

    debug::log(|| ("collect", format!("{} unreachable objects", count)));

    // Build a list about what to drop and release.
    let mut to_drop: Vec<Box<dyn CcDyn>> = Vec::with_capacity(count);
    visit_list(list, |header| {
        if is_unreachable(header) {
            to_drop.push(header.value.gc_prepare_drop());
        }
    });

    // Restore "prev" so "gc_untrack" can work.
    restore_prev(list);

    // Call `T::drop`. Do not release `CcBox<T>` yet since `ref_count` is still
    // needed for `CcBox<T>::drop`.
    for value in to_drop.iter_mut() {
        value.gc_force_drop_without_release();
    }

    // Release the memory of `CcBox<T>`.
    for mut value in to_drop {
        value.gc_mark_for_release();
        drop(value); // This will trigger the memory release.
    }

    count
}

/// Restore `GcHeader.prev` as a pointer used in the linked list.
fn restore_prev(list: *mut GcHeader) {
    let mut prev = list;
    visit_list(list, |header| {
        header.prev = prev;
        prev = header;
    });
}

fn is_unreachable(header: &GcHeader) -> bool {
    let prev = header.prev as usize;
    is_collecting(header) && (prev >> PREV_SHIFT) == 0
}

fn is_collecting(header: &GcHeader) -> bool {
    let prev = header.prev as usize;
    (prev & PREV_MASK_COLLECTING) != 0
}

fn unset_collecting(header: &mut GcHeader) {
    let prev = header.prev as usize;
    header.prev = ((prev & PREV_MASK_COLLECTING) ^ prev) as _;
}

fn edit_gc_ref_count(header: &mut GcHeader, delta: isize) {
    let prev = header.prev as isize;
    header.prev = (prev + (1 << PREV_SHIFT) * delta) as _;
}
