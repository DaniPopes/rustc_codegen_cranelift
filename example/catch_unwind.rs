// Checks that panics unwind. This requires compiling with `-Cpanic=unwind`.

use std::panic::catch_unwind;
use std::sync::Mutex;

// The JIT doesn't support thread locals.
static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn log(entry: &'static str) {
    LOG.lock().unwrap().push(entry);
}

fn take_log() -> Vec<&'static str> {
    std::mem::take(&mut *LOG.lock().unwrap())
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
        panics(2);
    }
}

#[inline(never)]
fn panics(code: u32) -> ! {
    std::panic::panic_any(code);
}

fn code(payload: Box<dyn std::any::Any + Send>) -> u32 {
    *payload.downcast::<u32>().unwrap()
}

fn main() {
    if cfg!(panic = "abort") {
        println!("skipping catch_unwind test as -Cpanic=abort is used");
        return;
    }

    // Don't print the messages of the expected panics.
    std::panic::set_hook(Box::new(|_| {}));

    assert_eq!(catch_unwind(|| 42).unwrap(), 42);

    assert_eq!(code(catch_unwind(|| panics(1)).unwrap_err()), 1);

    // Destructors run in reverse declaration order while unwinding.
    let result = catch_unwind(|| {
        let _a = LogOnDrop("a");
        let _b = LogOnDrop("b");
        panics(1);
    });
    assert_eq!(code(result.unwrap_err()), 1);
    assert_eq!(take_log(), ["b", "a"]);

    // A panic in a destructor doesn't prevent the remaining values from being dropped.
    let result = catch_unwind(|| {
        let _a = LogOnDrop("a");
        let _tuple = (LogOnDrop("t0"), PanicOnDrop("t1"), LogOnDrop("t2"));
    });
    assert_eq!(code(result.unwrap_err()), 2);
    assert_eq!(take_log(), ["t0", "t1", "t2", "a"]);

    // Panics can be caught repeatedly and nested.
    let result = catch_unwind(|| {
        let _outer = LogOnDrop("outer");
        let inner = catch_unwind(|| {
            let _inner = LogOnDrop("inner");
            panics(1);
        });
        assert_eq!(code(inner.unwrap_err()), 1);
        panics(3);
    });
    assert_eq!(code(result.unwrap_err()), 3);
    assert_eq!(take_log(), ["inner", "outer"]);
}
