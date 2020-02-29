use crate::trace::{Trace, Tracer};

/// Mark types as "untracked". Untracked types opt-out the cycle collector.
#[macro_export]
macro_rules! untrack {
    ( <$( $g: tt ),*> $( $t: tt )* ) => {
            impl<$( $g ),*> $crate::trace::Trace for $($t)* {
                fn is_type_tracked(&self) -> bool { false }
            }
    };
    ( $( $t: ty ),* ) => {
        $(
            impl $crate::trace::Trace for $t {
                fn is_type_tracked(&self) -> bool { false }
            }
        )*
    };
}

untrack!(bool, char, f32, f64, i16, i32, i64, i8, isize, str, u16, u32, u64, u8, usize);
untrack!(());
untrack!(String, &'static str);

mod slice {
    use super::*;

    impl<'a, T: Trace> Trace for &'a mut [T] {
        fn trace(&self, tracer: &mut Tracer) {
            for t in &self[..] {
                t.trace(tracer);
            }
        }
    }
}

mod boxed {
    use super::*;

    impl<T: Trace + ?Sized> Trace for Box<T> {
        fn trace(&self, tracer: &mut Tracer) {
            (**self).trace(tracer);
        }
    }
}
mod cell {
    use super::*;
    use std::cell;

    impl<T: Copy + Trace + ?Sized> Trace for cell::Cell<T> {
        fn trace(&self, tracer: &mut Tracer) {
            self.get().trace(tracer);
        }
    }

    impl<T: Trace + ?Sized> Trace for cell::RefCell<T> {
        fn trace(&self, tracer: &mut Tracer) {
            // If the RefCell is currently borrowed we
            // assume there's an outstanding reference to this
            // cycle so it's ok if we don't trace through it.
            // If the borrow gets leaked somehow then we're going
            // to leak the cycle.
            if let Ok(x) = self.try_borrow() {
                x.trace(tracer);
            }
        }
    }
}

mod collections {
    use super::*;
    use std::collections;
    use std::hash;

    impl<K, V: Trace> Trace for collections::BTreeMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            for (_, v) in self {
                v.trace(tracer);
            }
        }
    }

    impl<K: Eq + hash::Hash + Trace, V: Trace> Trace for collections::HashMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            for (_, v) in self {
                v.trace(tracer);
            }
        }
    }

    impl<T: Trace> Trace for collections::LinkedList<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }
    }

    impl<T: Trace> Trace for collections::VecDeque<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }
    }
}

mod vec {
    use super::*;
    impl<T: Trace> Trace for Vec<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }
    }
}

mod func {
    untrack!(<X> fn() -> X);
    untrack!(<A, X> fn(A) -> X);
    untrack!(<A, B, X> fn(A, B) -> X);
    untrack!(<A, B, C, X> fn(A, B, C) -> X);
    untrack!(<A, B, C, D, X> fn(A, B, C, D) -> X);
    untrack!(<A, B, C, D, E, X> fn(A, B, C, D, E) -> X);
    untrack!(<A, B, C, D, E, F, X> fn(A, B, C, D, E, F) -> X);
}

mod ffi {
    use std::ffi;

    untrack!(
        ffi::CStr,
        ffi::CString,
        ffi::NulError,
        ffi::OsStr,
        ffi::OsString
    );
}

mod net {
    use std::net;

    untrack!(
        net::AddrParseError,
        net::Ipv4Addr,
        net::Ipv6Addr,
        net::SocketAddrV4,
        net::SocketAddrV6,
        net::TcpListener,
        net::TcpStream,
        net::UdpSocket
    );
}

mod option {
    use super::*;

    impl<T: Trace> Trace for Option<T> {
        fn trace(&self, tracer: &mut Tracer) {
            if let Some(ref t) = *self {
                t.trace(tracer);
            }
        }
    }
}

mod path {
    use std::path;

    untrack!(path::Path, path::PathBuf);
}

mod process {
    use std::process;

    untrack!(
        process::Child,
        process::ChildStderr,
        process::ChildStdin,
        process::ChildStdout,
        process::Command,
        process::ExitStatus,
        process::Output,
        process::Stdio
    );
}

mod rc {
    use std::rc;

    untrack!(<T> rc::Rc<T>);
    untrack!(<T> rc::Weak<T>);
}

mod result {
    use super::*;

    impl<T: Trace, U: Trace> Trace for Result<T, U> {
        fn trace(&self, tracer: &mut Tracer) {
            match *self {
                Ok(ref t) => t.trace(tracer),
                Err(ref u) => u.trace(tracer),
            }
        }
    }
}

mod sync {
    use std::sync;

    untrack!(<T> sync::Arc<T>);
    untrack!(<T> sync::Mutex<T>);
    untrack!(<T> sync::RwLock<T>);
}

mod thread {
    use std::thread;

    untrack!(<T> thread::JoinHandle<T>);
    untrack!(<T> thread::LocalKey<T>);
    untrack!(thread::Thread);
}
