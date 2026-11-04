// Checks that panics unwind correctly. This only works when compiled with `-Cpanic=unwind`.
//
// When run with the argument `abort-in-drop` or `abort-extern-c`, this instead panics in a way
// that has to abort the process.

use std::cell::RefCell;
use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any, resume_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

// Thread locals are not supported by the JIT, so use statics.
static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static DROPPED: AtomicU32 = AtomicU32::new(0);

fn log(entry: &'static str) {
    LOG.lock().unwrap().push(entry);
}

fn take_log() -> Vec<&'static str> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

fn count_drop() {
    DROPPED.fetch_add(1, Ordering::SeqCst);
}

fn reset_dropped() {
    DROPPED.store(0, Ordering::SeqCst);
}

fn dropped() -> u32 {
    DROPPED.load(Ordering::SeqCst)
}

struct LogOnDrop(&'static str);

impl Drop for LogOnDrop {
    fn drop(&mut self) {
        log(self.0);
    }
}

struct PanicOnDrop(&'static str);

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        log(self.0);
        panic!("panic in drop of {}", self.0);
    }
}

#[inline(never)]
fn panics(msg: &'static str) -> ! {
    panic!("{}", msg);
}

fn message(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        msg
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg
    } else {
        "<non-string payload>"
    }
}

fn catch_unwind_returns_value() {
    assert_eq!(catch_unwind(|| black_box(42)).unwrap(), 42);

    let err = catch_unwind(|| -> i32 { panics("boom") }).unwrap_err();
    assert_eq!(message(&*err), "boom");

    let err = catch_unwind(|| -> i32 { panic!("formatted {}", black_box(1)) }).unwrap_err();
    assert_eq!(message(&*err), "formatted 1");
}

fn drop_order_during_unwind() {
    take_log();
    let result = catch_unwind(|| {
        let _a = LogOnDrop("a");
        let _b = LogOnDrop("b");
        {
            let _c = LogOnDrop("c");
        }
        let _d = LogOnDrop("d");
        panics("unwind");
    });
    assert!(result.is_err());
    assert_eq!(take_log(), ["c", "d", "b", "a"]);
}

fn drop_temporaries_and_moved_values() {
    take_log();
    let result = catch_unwind(|| {
        let moved = LogOnDrop("moved");
        let kept = LogOnDrop("kept");
        drop(black_box(moved));
        let _ = black_box(Some(LogOnDrop("temporary"))).map(|_| -> () { panics("in closure") });
        drop(kept);
    });
    assert!(result.is_err());
    assert_eq!(take_log(), ["moved", "temporary", "kept"]);
}

fn panic_in_drop_drops_remaining_elements() {
    take_log();
    let result = catch_unwind(|| {
        let _v = vec![LogOnDrop("v0"), LogOnDrop("v1"), LogOnDrop("v2")];
        let _boxed: Box<[LogOnDrop]> = Box::new([LogOnDrop("b0"), LogOnDrop("b1")]);
        let elems = Box::new([LogOnDrop("e0"), LogOnDrop("e1")]);
        drop(black_box(elems));
        let _tuple = (LogOnDrop("t0"), PanicOnDrop("t1"), LogOnDrop("t2"));
    });
    assert_eq!(message(&*result.unwrap_err()), "panic in drop of t1");
    assert_eq!(take_log(), ["e0", "e1", "t0", "t1", "t2", "b0", "b1", "v0", "v1", "v2"]);

    let result = catch_unwind(|| {
        let _v = vec![LogOnDrop("0"), LogOnDrop("1")];
        let _slice = vec![LogOnDrop("s0"), LogOnDrop("s1")];
        drop(vec![LogOnDrop("x0")]);
        let _p = PanicOnDrop("p");
        let _tail = vec![LogOnDrop("tail")];
    });
    assert_eq!(message(&*result.unwrap_err()), "panic in drop of p");
    assert_eq!(take_log(), ["x0", "tail", "p", "s0", "s1", "0", "1"]);
}

fn nested_catch() {
    take_log();
    let result = catch_unwind(|| {
        let _outer = LogOnDrop("outer");
        let inner = catch_unwind(|| {
            let _inner = LogOnDrop("inner");
            panics("inner panic");
        });
        assert_eq!(message(&*inner.unwrap_err()), "inner panic");
        log("after inner");
        panics("outer panic");
    });
    assert_eq!(message(&*result.unwrap_err()), "outer panic");
    assert_eq!(take_log(), ["inner", "after inner", "outer"]);
}

fn catch_inside_drop_while_unwinding() {
    struct CatchInDrop;

    impl Drop for CatchInDrop {
        fn drop(&mut self) {
            let err = catch_unwind(|| {
                let _guard = LogOnDrop("drop guard");
                panics("panic in drop");
            })
            .unwrap_err();
            assert_eq!(message(&*err), "panic in drop");
            log("caught in drop");
        }
    }

    take_log();
    let result = catch_unwind(|| {
        let _a = LogOnDrop("a");
        let _catch = CatchInDrop;
        let _b = LogOnDrop("b");
        panics("original");
    });
    assert_eq!(message(&*result.unwrap_err()), "original");
    assert_eq!(take_log(), ["b", "drop guard", "caught in drop", "a"]);
}

fn resume_unwind_and_payloads() {
    let err = catch_unwind(|| resume_unwind(Box::new(1234_u32))).unwrap_err();
    assert_eq!(*err.downcast::<u32>().unwrap(), 1234);

    let err = catch_unwind(|| panic_any(String::from("owned"))).unwrap_err();
    assert_eq!(*err.downcast::<String>().unwrap(), "owned");

    take_log();
    let result = catch_unwind(|| {
        let _guard = LogOnDrop("guard");
        let err = catch_unwind(|| panics("first")).unwrap_err();
        resume_unwind(err);
    });
    assert_eq!(message(&*result.unwrap_err()), "first");
    assert_eq!(take_log(), ["guard"]);
}

fn closures_and_dyn() {
    take_log();
    let f: Box<dyn FnOnce()> = Box::new({
        let guard = LogOnDrop("captured");
        move || {
            let _guard = guard;
            panics("in boxed closure");
        }
    });
    let result = catch_unwind(AssertUnwindSafe(f));
    assert_eq!(message(&*result.unwrap_err()), "in boxed closure");
    assert_eq!(take_log(), ["captured"]);

    trait Trait {
        fn call(&self);
    }

    struct Impl(&'static str);

    impl Trait for Impl {
        fn call(&self) {
            let _guard = LogOnDrop("in method");
            panics(self.0);
        }
    }

    let obj: &dyn Trait = black_box(&Impl("in dyn method"));
    let result = catch_unwind(AssertUnwindSafe(|| obj.call()));
    assert_eq!(message(&*result.unwrap_err()), "in dyn method");
    assert_eq!(take_log(), ["in method"]);
}

unsafe extern "C-unwind" fn c_unwind_callee(depth: u32) {
    let _guard = LogOnDrop("c-unwind frame");
    if depth == 0 {
        panics("from c-unwind");
    }
    unsafe { c_unwind_callee(depth - 1) };
}

fn extern_c_unwind() {
    take_log();
    let err = catch_unwind(|| unsafe { c_unwind_callee(black_box(2)) }).unwrap_err();
    assert_eq!(message(&*err), "from c-unwind");
    assert_eq!(take_log(), ["c-unwind frame"; 3]);

    let ptr = black_box(c_unwind_callee as unsafe extern "C-unwind" fn(u32));
    let err = catch_unwind(|| unsafe { ptr(1) }).unwrap_err();
    assert_eq!(message(&*err), "from c-unwind");
    assert_eq!(take_log(), ["c-unwind frame"; 2]);
}

fn thread_join_and_scope() {
    take_log();
    let handle = thread::spawn(|| {
        let _guard = LogOnDrop("thread guard");
        panics("in thread");
    });
    assert_eq!(message(&*handle.join().unwrap_err()), "in thread");
    assert_eq!(take_log(), ["thread guard"]);

    let counter = AtomicU32::new(0);
    let result = catch_unwind(AssertUnwindSafe(|| {
        thread::scope(|scope| {
            scope.spawn(|| {
                counter.fetch_add(1, Ordering::SeqCst);
                panics("in scoped thread");
            });
        });
    }));
    assert_eq!(message(&*result.unwrap_err()), "a scoped thread panicked");
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    let mutex = std::sync::Arc::new(std::sync::Mutex::new(0));
    let mutex2 = mutex.clone();
    let _ = thread::spawn(move || {
        let _lock = mutex2.lock().unwrap();
        panics("poison");
    })
    .join();
    assert!(mutex.lock().is_err());
}

#[inline(never)]
fn callee_saved_across_unwinding_call(
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: f64,
    h: f64,
    panic_rounds: bool,
) -> u64 {
    let (mut sum, mut fsum) = (a ^ b ^ c ^ d ^ e ^ f, g + h);
    for round in 0..4_u64 {
        let result = catch_unwind(AssertUnwindSafe(|| {
            if panic_rounds && black_box(round) % 2 == 0 {
                panics("callee saved");
            }
        }));
        assert_eq!(result.is_err(), panic_rounds && round % 2 == 0);
        sum = sum.wrapping_mul(31).wrapping_add(a + b + c + d + e + f + round);
        fsum += g * h;
    }
    sum.wrapping_add(fsum as u64)
}

fn callee_saved_registers() {
    let (a, b, c, d, e, f) = black_box((1, 2, 3, 4, 5, 6));
    let (g, h) = black_box((1.5, 2.5));
    let without_panics = callee_saved_across_unwinding_call(a, b, c, d, e, f, g, h, false);
    let with_panics = callee_saved_across_unwinding_call(a, b, c, d, e, f, g, h, true);
    assert_eq!(with_panics, without_panics);
    assert_eq!((a, b, c, d, e, f, g, h), (1, 2, 3, 4, 5, 6, 1.5, 2.5));
}

struct Counted;

impl Drop for Counted {
    fn drop(&mut self) {
        count_drop();
    }
}

#[inline(never)]
fn maybe_panic(i: u32, panic_at: u32) {
    if black_box(i) == panic_at {
        panics("large function");
    }
}

macro_rules! call_site {
    ($i:expr, $panic_at:expr) => {
        let _counted = Counted;
        maybe_panic($i, $panic_at);
    };
}

macro_rules! repeat4 {
    ($mac:ident, $n:expr, $panic_at:expr) => {
        $mac!($n * 4, $panic_at);
        $mac!($n * 4 + 1, $panic_at);
        $mac!($n * 4 + 2, $panic_at);
        $mac!($n * 4 + 3, $panic_at);
    };
}

macro_rules! repeat16 {
    ($n:expr, $panic_at:expr) => {
        repeat4!(call_site, $n * 4, $panic_at);
        repeat4!(call_site, $n * 4 + 1, $panic_at);
        repeat4!(call_site, $n * 4 + 2, $panic_at);
        repeat4!(call_site, $n * 4 + 3, $panic_at);
    };
}

macro_rules! repeat256 {
    ($panic_at:expr) => {
        repeat16!(0, $panic_at);
        repeat16!(1, $panic_at);
        repeat16!(2, $panic_at);
        repeat16!(3, $panic_at);
        repeat16!(4, $panic_at);
        repeat16!(5, $panic_at);
        repeat16!(6, $panic_at);
        repeat16!(7, $panic_at);
        repeat16!(8, $panic_at);
        repeat16!(9, $panic_at);
        repeat16!(10, $panic_at);
        repeat16!(11, $panic_at);
        repeat16!(12, $panic_at);
        repeat16!(13, $panic_at);
        repeat16!(14, $panic_at);
        repeat16!(15, $panic_at);
    };
}

/// The landing pads and call sites of this function are far enough from its start that their
/// offsets in the LSDA don't fit in a single uleb128 byte.
#[inline(never)]
fn large_function(panic_at: u32) {
    repeat256!(panic_at);
}

fn lsda_with_large_offsets() {
    for panic_at in [0, 1, 127, 128, 129, 200, 255] {
        reset_dropped();
        let result = catch_unwind(|| large_function(panic_at));
        assert_eq!(message(&*result.unwrap_err()), "large function");
        // Every `Counted` created up to and including the panicking call is dropped.
        assert_eq!(dropped(), panic_at + 1);
    }

    reset_dropped();
    large_function(u32::MAX);
    assert_eq!(dropped(), 256);
}

fn catch_unwind_in_loop() {
    let mut caught = 0;
    for i in 0..1000 {
        if catch_unwind(|| {
            if black_box(i) % 3 == 0 {
                panics("loop");
            }
        })
        .is_err()
        {
            caught += 1;
        }
    }
    assert_eq!(caught, 334);
}

fn arithmetic_and_indexing_panics() {
    let err = catch_unwind(|| black_box(1) / black_box(0)).unwrap_err();
    assert_eq!(message(&*err), "attempt to divide by zero");

    let v = vec![1, 2, 3];
    let err = catch_unwind(|| v[black_box(5)]).unwrap_err();
    assert_eq!(message(&*err), "index out of bounds: the len is 3 but the index is 5");

    let err = catch_unwind(|| black_box(None::<u8>).unwrap()).unwrap_err();
    assert_eq!(message(&*err), "called `Option::unwrap()` on a `None` value");

    let cell = RefCell::new(1);
    let _borrow = cell.borrow_mut();
    assert!(catch_unwind(AssertUnwindSafe(|| *cell.borrow())).is_err());
}

fn panicking_iterators_and_clones() {
    struct PanicClone(u32);

    impl Clone for PanicClone {
        fn clone(&self) -> Self {
            if self.0 == 2 {
                panics("clone");
            }
            PanicClone(self.0)
        }
    }

    impl Drop for PanicClone {
        fn drop(&mut self) {
            count_drop();
        }
    }

    reset_dropped();
    let v = vec![PanicClone(0), PanicClone(1), PanicClone(2), PanicClone(3)];
    assert!(catch_unwind(AssertUnwindSafe(|| v.clone())).is_err());
    // The two successfully cloned elements are dropped.
    assert_eq!(dropped(), 2);
    drop(v);
    assert_eq!(dropped(), 6);

    reset_dropped();
    let result = catch_unwind(|| {
        (0..5)
            .map(|i| {
                let _counted = Counted;
                if i == 3 {
                    panics("map");
                }
                Box::new(i)
            })
            .collect::<Vec<_>>()
    });
    assert!(result.is_err());
    assert_eq!(dropped(), 4);
}

fn panic_hook() {
    static HOOK_CALLS: AtomicU32 = AtomicU32::new(0);

    let old = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        assert_eq!(info.payload_as_str(), Some("hook"));
        HOOK_CALLS.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(catch_unwind(|| panics("hook")).is_err());
    assert_eq!(HOOK_CALLS.load(Ordering::SeqCst), 1);
    std::panic::set_hook(old);
}

fn run_unwind_tests() {
    // Don't print the messages of the many expected panics.
    std::panic::set_hook(Box::new(|_| {}));

    catch_unwind_returns_value();
    drop_order_during_unwind();
    drop_temporaries_and_moved_values();
    panic_in_drop_drops_remaining_elements();
    nested_catch();
    catch_inside_drop_while_unwinding();
    resume_unwind_and_payloads();
    closures_and_dyn();
    extern_c_unwind();
    thread_join_and_scope();
    callee_saved_registers();
    lsda_with_large_offsets();
    catch_unwind_in_loop();
    arithmetic_and_indexing_panics();
    panicking_iterators_and_clones();
    panic_hook();
}

extern "C" fn panic_in_extern_c() {
    panics("panic in extern C");
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        // A panic in a destructor while already unwinding has to abort.
        Some("abort-in-drop") => {
            let _drop = PanicOnDrop("abort-in-drop");
            panics("first panic");
        }
        // A panic can't escape from an `extern "C"` function.
        Some("abort-extern-c") => panic_in_extern_c(),
        _ => {
            if cfg!(panic = "abort") {
                println!("skipping unwind tests as -Cpanic=abort is used");
                return;
            }
            run_unwind_tests();
        }
    }
}
