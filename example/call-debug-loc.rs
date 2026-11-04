// run-pass

use std::backtrace::Backtrace;

struct Chain;

impl Chain {
    fn step(self) -> Self {
        self
    }

    #[inline(never)]
    fn capture(self) -> String {
        Backtrace::force_capture().to_string()
    }
}

fn frame_location(backtrace: &str, function: &str) -> String {
    let mut lines = backtrace.lines();
    while let Some(line) = lines.next() {
        if line.contains(function) {
            return lines.next().expect("frame without location").trim().to_owned();
        }
    }
    panic!("no frame for `{function}` in backtrace:\n{backtrace}");
}

#[rustfmt::skip]
fn main() {
    // The cg_clif sysroot cannot capture backtraces on Windows yet.
    if cfg!(windows) {
        return;
    }

    // The frame of a call reports the location of the call itself, not of the whole expression.
    let backtrace = Chain
        .step()
        .step()
        .capture();
    let expected = format!("call-debug-loc.rs:{}:10", line!() - 1);
    let location = frame_location(&backtrace, "call_debug_loc::main");
    assert!(location.ends_with(&expected), "{location}\n{backtrace}");
}
