use crate::trace::{Trace, Tracer};
use std::any::Any;

/// Mark types as acyclic. Opt-out the cycle collector.
///
/// See [`Trace::is_type_tracked`](trait.Trace.html#method.is_type_tracked) for details.
/// In general, types including trait objects (directly or indirectly) should
/// not be acyclic.
///
/// ## Examples
///
/// ```
/// use gcmodule::trace_acyclic;
///
/// struct X(u32);
/// struct Y(String);
/// struct Z<T>(fn (T));
///
/// trace_acyclic!(X);
/// trace_acyclic!(Y);
/// trace_acyclic!(<T> Z<T>);
/// ```
macro_rules! trace_acyclic {
    ( <$( $g:ident ),*> $( $t: tt )* ) => {
        impl<$( $g: 'static ),*> $crate::Trace for $($t)* {
            #[inline]
            fn is_type_tracked() -> bool where Self: Sized { false }
            fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
        }
    };
    ( $( $t: ty ),* ) => {
        $( trace_acyclic!(<> $t); )*
    };
}

/// Implement [`Trace`](trait.Trace.html) for simple container types.
///
/// ## Examples
///
/// ```
/// use gcmodule::Trace;
/// use gcmodule::trace_fields;
///
/// struct X<T1, T2> { a: T1, b: T2 };
/// struct Y<T>(Box<T>);
/// struct Z(Box<dyn Trace>);
///
/// trace_fields!(
///     X<T1, T2> { a: T1, b: T2 }
///     Y<T> { 0: T }
///     Z { 0 }
/// );
/// ```
macro_rules! trace_fields {
    ( $( $type:ty { $( $field:tt $(: $tp:ident )? ),* } )* ) => {
        $(
            impl< $( $( $tp: $crate::Trace )? ),* > $crate::Trace for $type {
                fn trace(&self, tracer: &mut $crate::Tracer) {
                    let _ = tracer;
                    $( (&self . $field ).trace(tracer); )*
                }
                #[inline]
                fn is_type_tracked() -> bool {
                    $( $( if $tp::is_type_tracked() { return true } )? )*
                    false
                }
                fn as_any(&self) -> Option<&dyn std::any::Any> { Some(self) }
            }
        )*
    };
}

trace_acyclic!(bool, char, f32, f64, i16, i32, i64, i8, isize, u16, u32, u64, u8, usize);
trace_acyclic!(());
trace_acyclic!(String, &'static str);

mod tuples {
    trace_fields!(
        (A, B) { 0: A, 1: B }
        (A, B, C) { 0: A, 1: B, 2: C }
        (A, B, C, D) { 0: A, 1: B, 2: C, 3: D }
        (A, B, C, D, E) { 0: A, 1: B, 2: C, 3: D, 4: E }
    );
}

mod boxed {
    use super::*;

    impl<T: Trace> Trace for Box<T> {
        fn trace(&self, tracer: &mut Tracer) {
            self.as_ref().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl Trace for Box<dyn Trace> {
        fn trace(&self, tracer: &mut Tracer) {
            self.as_ref().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            // Trait objects can have complex non-atomic structure.
            true
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl Trace for Box<dyn Trace + Send> {
        fn trace(&self, tracer: &mut Tracer) {
            self.as_ref().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            true
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl Trace for Box<dyn Trace + Send + Sync> {
        fn trace(&self, tracer: &mut Tracer) {
            self.as_ref().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            true
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }
}

mod cell {
    use super::*;
    use std::cell;

    impl<T: Copy + Trace> Trace for cell::Cell<T> {
        fn trace(&self, tracer: &mut Tracer) {
            self.get().trace(tracer);
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl<T: Trace> Trace for cell::RefCell<T> {
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

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }
}

mod collections {
    use super::*;
    use std::collections;
    use std::hash;

    impl<K: Trace, V: Trace> Trace for collections::BTreeMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            for (k, v) in self {
                k.trace(tracer);
                v.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            K::is_type_tracked() && V::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl<K: Eq + hash::Hash + Trace, V: Trace> Trace for collections::HashMap<K, V> {
        fn trace(&self, tracer: &mut Tracer) {
            for (k, v) in self {
                k.trace(tracer);
                v.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            K::is_type_tracked() && V::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl<T: Trace> Trace for collections::LinkedList<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl<T: Trace> Trace for collections::VecDeque<T> {
        fn trace(&self, tracer: &mut Tracer) {
            for t in self {
                t.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
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

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }
}

// See https://github.com/rust-lang/rust/issues/56105#issuecomment-465709105
#[allow(unknown_lints)]
#[allow(coherence_leak_check)]
mod func {
    trace_acyclic!(<X> fn() -> X);

    trace_acyclic!(<A, X> fn(&A) -> X);
    trace_acyclic!(<A, X> fn(A) -> X);

    trace_acyclic!(<A, B, X> fn(&A, &B) -> X);
    trace_acyclic!(<A, B, X> fn(A, &B) -> X);
    trace_acyclic!(<A, B, X> fn(&A, B) -> X);
    trace_acyclic!(<A, B, X> fn(A, B) -> X);

    trace_acyclic!(<A, B, C, X> fn(&A, &B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, &B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(&A, B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, B, &C) -> X);
    trace_acyclic!(<A, B, C, X> fn(&A, &B, C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, &B, C) -> X);
    trace_acyclic!(<A, B, C, X> fn(&A, B, C) -> X);
    trace_acyclic!(<A, B, C, X> fn(A, B, C) -> X);

    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, &C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, C, &D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, &C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, &B, C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, &B, C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(&A, B, C, D) -> X);
    trace_acyclic!(<A, B, C, D, X> fn(A, B, C, D) -> X);

    trace_acyclic!(<A, B, C, D, E, X> fn(A, B, C, D, E) -> X);
    trace_acyclic!(<A, B, C, D, E, F, X> fn(A, B, C, D, E, F) -> X);
}

mod ffi {
    use std::ffi;

    trace_acyclic!(ffi::CString, ffi::NulError, ffi::OsString);
}

mod net {
    use std::net;

    trace_acyclic!(
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

        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }
}

mod path {
    use std::path;

    trace_acyclic!(path::PathBuf);
}

mod process {
    use std::process;

    trace_acyclic!(
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

    trace_acyclic!(<T> rc::Rc<T>);
    trace_acyclic!(<T> rc::Weak<T>);
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

        fn is_type_tracked() -> bool {
            T::is_type_tracked() && U::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }
}

mod sync {
    use super::*;
    use std::sync;

    trace_acyclic!(<T> sync::Arc<T>);

    impl<T: Trace> Trace for sync::Mutex<T> {
        fn trace(&self, tracer: &mut Tracer) {
            // For the same reason as RefCell
            if let Ok(x) = self.lock() {
                x.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }

    impl<T: Trace> Trace for sync::RwLock<T> {
        fn trace(&self, tracer: &mut Tracer) {
            // For the same reason as RefCell
            // The reson why use `.write()` instead of `.read()` is that read access is also a outstanding reference
            if let Ok(x) = self.write() {
                x.trace(tracer);
            }
        }

        #[inline]
        fn is_type_tracked() -> bool {
            T::is_type_tracked()
        }

        fn as_any(&self) -> Option<&dyn Any> {
            Some(self)
        }
    }
}

mod thread {
    use std::thread;

    trace_acyclic!(<T> thread::JoinHandle<T>);
    trace_acyclic!(<T> thread::LocalKey<T>);
    trace_acyclic!(thread::Thread);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cc;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn test_is_type_tracked() {
        assert!(!u8::is_type_tracked());
        assert!(!<f32 as Trace>::is_type_tracked());
        assert!(!String::is_type_tracked());
        assert!(!Option::<u32>::is_type_tracked());
        assert!(!Vec::<u8>::is_type_tracked());
        assert!(!<(bool, f64)>::is_type_tracked());
        assert!(!Cell::<u32>::is_type_tracked());
        assert!(!RefCell::<String>::is_type_tracked());
        assert!(Box::<dyn Trace>::is_type_tracked());
        assert!(RefCell::<Box::<dyn Trace>>::is_type_tracked());
        assert!(RefCell::<Vec::<Box::<dyn Trace>>>::is_type_tracked());
        assert!(Vec::<RefCell::<Box::<dyn Trace>>>::is_type_tracked());
        assert!(!Cc::<u8>::is_type_tracked());
        assert!(!Vec::<Cc::<u8>>::is_type_tracked());

        assert!(!<fn(u8) -> u8>::is_type_tracked());
        assert!(!<fn(&u8) -> u8>::is_type_tracked());
    }

    #[test]
    fn test_is_cyclic_type_tracked() {
        type C1 = RefCell<Option<Rc<Box<S1>>>>;
        struct S1(C1);
        impl Trace for S1 {
            fn trace(&self, t: &mut Tracer) {
                self.0.trace(t);
            }
            fn is_type_tracked() -> bool {
                // This is not an infinite loop because Rc is not tracked.
                C1::is_type_tracked()
            }
        }

        type C2 = RefCell<Option<Cc<Box<S2>>>>;
        struct S2(C2);
        impl Trace for S2 {
            fn trace(&self, t: &mut Tracer) {
                self.0.trace(t);
            }
            fn is_type_tracked() -> bool {
                // C2::is_type_tracked() can cause an infinite loop.
                true
            }
        }

        assert!(!S1::is_type_tracked());
        assert!(S2::is_type_tracked());
    }
}
