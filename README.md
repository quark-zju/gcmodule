# gcmodule

Garbage collection inspired by [CPython](https://github.com/python/cpython/)'s implementation.

This library provides a type `Cc<T>`. It provides shared reference-counting pointer, similar to stdlib `Rc<T>`. Unlike `Rc`, reference cycles in `Cc` can be collected.

## Example

```rust
use gcmodule::{Cc, Trace};
use std::cell::RefCell;

type List = Cc<RefCell<Vec<Box<dyn Trace>>>>;
let a: List = Default::default();
let b: List = Default::default();
a.borrow_mut().push(Box::new(b.clone()));
b.borrow_mut().push(Box::new(a.clone())); // Form a cycle.
drop(a);
drop(b); // Internal values are not dropped due to the cycle.
gcmodule::collect_thread_cycles(); // Internal values are dropped.
```

## Similar Projects

### [bacon-rajan-cc](https://github.com/fitzgen/bacon-rajan-cc) v0.3

- Both are reference count, with cyclic garbage collection.
- Both are single-threaded, and stop-the-world.
- Main APIs like `Cc<T>` and `Trace` are similar, or even compatible.
- `gcmodule` is conceptually simpler. There is no need for the "colors" concept.
- `gcmodule` does not require extra space for bookkeeping, if all objects are freed by reference counting. See [this commit message](https://github.com/quark-zju/gcmodule/commit/b825bc45ac008e26ada3c13daa3efa34334f8273) for some details.
