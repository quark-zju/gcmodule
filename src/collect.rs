// The main idea comes from cpython 3.8's `gcmodule.c` [1].
//
// [1]: https://github.com/python/cpython/blob/v3.8.0/Modules/gcmodule.c

// NOTE: Consider adding generation support if necessary. It won't be too hard.

use crate::cc::GcHeader;
use crate::debug;
use crate::Cc;
use crate::Trace;
use std::cell::RefCell;
use std::ops::Deref;
use std::pin::Pin;

/// Collect cyclic garbage in the current thread.
/// Return the number of objects collected.
pub fn collect_thread_cycles() -> usize {
    debug::log(|| ("collect", "collect_thread_cycles"));
    GC_LIST.with(|list| {
        let list: &GcHeader = { &list.borrow() };
        collect_list(list)
    })
}

thread_local!(pub(crate) static GC_LIST: RefCell<Pin<Box<GcHeader>>> = RefCell::new(new_gc_list()));

/// Create an empty linked list with a dummy GcHeader.
fn new_gc_list() -> Pin<Box<GcHeader>> {
    let pinned = Box::pin(GcHeader::empty());
    let header: &GcHeader = pinned.deref();
    header.prev.set(header);
    header.next.set(header);
    pinned
}

/// Scan the specified linked list. Collect cycles.
fn collect_list(list: &GcHeader) -> usize {
    update_refs(list);
    subtract_refs(list);
    release_unreachable(list)
}

/// Visit the linked list.
fn visit_list<'a>(list: &'a GcHeader, mut func: impl FnMut(&'a GcHeader)) {
    // Skip the first dummy entry.
    let mut ptr = list.next.get();
    while ptr != list {
        // The linked list is maintained so the pointer is valid.
        let header: &GcHeader = unsafe { &*ptr };
        ptr = header.next.get();
        func(header);
    }
}

const PREV_MASK_COLLECTING: usize = 1;
const PREV_SHIFT: u32 = 1;

/// Temporarily use `GcHeader.prev` as `gc_ref_count`.
/// Idea comes from https://bugs.python.org/issue33597.
fn update_refs(list: &GcHeader) {
    visit_list(list, |header| {
        let ref_count = header.value().gc_ref_count();
        let shifted = (ref_count << PREV_SHIFT) | PREV_MASK_COLLECTING;
        header.prev.set(shifted as _);
    });
}

/// Subtract ref counts in `GcHeader.prev` by calling the non-recursive
/// `Trace::trace` on every track objects.
///
/// After this, potential unreachable objects will have ref count down
/// to 0. If vertexes in a connected component _all_ have ref count 0,
/// they are unreachable and can be released.
fn subtract_refs(list: &GcHeader) {
    let mut tracer = |header: &GcHeader| {
        if is_collecting(header) {
            debug_assert!(!is_unreachable(header));
            edit_gc_ref_count(header, -1);
        }
    };
    visit_list(list, |header| {
        header.value().gc_traverse(&mut tracer);
    });
}

/// Mark objects as reachable recursively. So ref count 0 means unreachable
/// values. This also removes the COLLECTING flag for reachable objects so
/// unreachable objects all have the COLLECTING flag set.
fn mark_reachable(list: &GcHeader) {
    fn revive(header: &GcHeader) {
        // hasn't visited?
        if is_collecting(header) {
            unset_collecting(header);
            if is_unreachable(header) {
                edit_gc_ref_count(header, 1); // revive
            }
            header.value().gc_traverse(&mut revive); // revive recursively
        }
    }
    visit_list(list, |header| {
        if is_collecting(header) && !is_unreachable(header) {
            unset_collecting(header);
            header.value().gc_traverse(&mut revive)
        }
    });
}

/// Release unreachable objects in the linked list.
fn release_unreachable(list: &GcHeader) -> usize {
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

    // Build a list of what to drop. The collecting steps change the linked list
    // so `visit_list` cannot be used.
    //
    // Here we keep extra references to the `CcBox<T>` to keep them alive. This
    // ensures metadata fields like `ref_count` is available.
    let mut to_drop: Vec<Cc<dyn Trace>> = Vec::with_capacity(count);
    visit_list(list, |header| {
        if is_unreachable(header) {
            to_drop.push(header.value().gc_clone());
        }
    });

    // Restore "prev" so deleting nodes from the linked list can work.
    restore_prev(list);

    // Drop `T` without releasing memory of `CcBox<T>`. This might trigger some
    // recursive drops of other `Cc<T>`. `CcBox<T>` need to stay alive so
    // `Cc<T>::drop` can read the ref count metadata.
    for value in to_drop.iter() {
        value.inner().drop_t();
    }

    // At this point the only references to the `CcBox<T>`s are inside the
    // `to_drop` list. Dropping `to_drop` would release the memory.
    for value in to_drop.iter() {
        let ref_count = value.ref_count();
        assert_eq!(
            ref_count, 1,
            "bug: unexpected ref-count after dropping cycles\n{}",
            "This usually indicates a buggy Trace or Drop implementation."
        );
    }

    count
}

/// Restore `GcHeader.prev` as a pointer used in the linked list.
fn restore_prev(list: &GcHeader) {
    let mut prev = list;
    visit_list(list, |header| {
        header.prev.set(prev);
        prev = header;
    });
}

fn is_unreachable(header: &GcHeader) -> bool {
    let prev = header.prev.get() as usize;
    is_collecting(header) && (prev >> PREV_SHIFT) == 0
}

fn is_collecting(header: &GcHeader) -> bool {
    let prev = header.prev.get() as usize;
    (prev & PREV_MASK_COLLECTING) != 0
}

fn unset_collecting(header: &GcHeader) {
    let prev = header.prev.get() as usize;
    let new_prev = (prev & PREV_MASK_COLLECTING) ^ prev;
    header.prev.set(new_prev as _);
}

fn edit_gc_ref_count(header: &GcHeader, delta: isize) {
    let prev = header.prev.get() as isize;
    let new_prev = prev + (1 << PREV_SHIFT) * delta;
    header.prev.set(new_prev as _);
}
