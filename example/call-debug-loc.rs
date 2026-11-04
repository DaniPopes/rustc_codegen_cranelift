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
            return lines.next().unwrap().trim().to_owned();
        }
    }
    panic!("no frame for `{function}` in backtrace:\n{backtrace}");
}

fn main() {
    // The frame of a call reports the location of the call itself, not of the whole expression.
    let backtrace = Chain
        .step()
        .step()
        .capture();
    let location = frame_location(&backtrace, "call_debug_loc::main");
    assert!(location.ends_with("call-debug-loc.rs:33:10"), "{location}\n{backtrace}");
}
