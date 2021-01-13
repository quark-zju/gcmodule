//! Thread-local logs for testing purpose.
use std::cell::Cell;
use std::cell::RefCell;
use std::ops::Deref;
use std::ops::DerefMut;

thread_local!(pub(crate) static LOG: RefCell<String> = Default::default());
thread_local!(pub(crate) static LAST_NAME: RefCell<String> = Default::default());
thread_local!(pub(crate) static ENABLED: Cell<bool> = Default::default());
thread_local!(pub(crate) static NEXT_DEBUG_NAME: Cell<usize> = Default::default());
thread_local!(pub(crate) static VERBOSE: bool = std::env::var("VERBOSE").is_ok());

/// Enable debug log for the given scope. Return the debug log.
pub(crate) fn capture_log(mut func: impl FnMut()) -> String {
    NEXT_DEBUG_NAME.with(|n| n.set(0));
    LAST_NAME.with(|n| n.borrow_mut().clear());
    ENABLED.with(|e| e.set(true));
    func();
    ENABLED.with(|e| e.set(false));
    LOG.with(|log| {
        let result = log.borrow().to_string();
        log.borrow_mut().clear();
        result
    })
}

pub(crate) fn log<S1: ToString, S2: ToString>(func: impl Fn() -> (S1, S2)) {
    let enabled = ENABLED.with(|e| e.get());
    if enabled {
        LOG.with(|log| {
            let (name, message) = func();
            let name = name.to_string();
            let message = message.to_string();
            let mut log = log.borrow_mut();
            LAST_NAME.with(|last_name| {
                let same_name = last_name.borrow().deref() == &name;
                if same_name {
                    log.push_str(", ");
                    log.push_str(&message);
                } else {
                    log.push_str("\n");
                    log.push_str(&name);
                    log.push_str(": ");
                    log.push_str(&message);
                    *(last_name.borrow_mut().deref_mut()) = name;
                }
            });
        })
    } else if VERBOSE.with(|verbose| *verbose) {
        let (name, message) = func();
        let t = std::thread::current().id();
        let name = format!("{:?}-{}", t, name.to_string());
        eprintln!("debug::log {} {}", name.to_string(), message.to_string());
    }
}
