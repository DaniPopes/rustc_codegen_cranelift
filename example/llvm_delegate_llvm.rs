// Compiled at opt-level 3, so by LLVM through the delegation. Depends on `llvm_delegate_clif.rs`,
// which is compiled by Cranelift.

use std::cell::Cell;
use std::fmt::Display;
use std::panic::Location;
use std::sync::atomic::{AtomicUsize, Ordering};

use llvm_delegate_clif::{CLIF_STATIC, CLIF_TLS, DropGuard, Shape};

pub const FILE: &str = file!();

pub static LLVM_STATIC: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    pub static LLVM_TLS: Cell<u32> = const { Cell::new(20) };
}

struct Triangle(u64);

impl Shape for Triangle {
    fn area(&self) -> u64 {
        self.0 * self.0 / 2
    }
}

#[inline(never)]
pub fn llvm_add(a: u64, b: u64) -> u64 {
    llvm_delegate_clif::clif_add(a, b) + 100
}

#[inline]
pub fn llvm_inline(x: u32) -> u32 {
    llvm_delegate_clif::clif_inline(x) + 1
}

#[track_caller]
pub fn llvm_location() -> &'static Location<'static> {
    Location::caller()
}

pub fn llvm_clif_location() -> &'static Location<'static> {
    llvm_delegate_clif::clif_location()
}

pub fn llvm_shape(side: u64) -> Box<dyn Shape> {
    Box::new(Triangle(side))
}

pub fn llvm_shape_area(shape: &dyn Shape) -> u64 {
    shape.area()
}

pub fn llvm_generic<T: Display>(value: T) -> String {
    format!("llvm<{value}>")
}

pub fn llvm_clif_generic() -> String {
    llvm_delegate_clif::clif_generic(7u8)
}

pub fn llvm_touch_statics() -> (usize, usize, u32, u32) {
    CLIF_STATIC.fetch_add(1, Ordering::SeqCst);
    LLVM_STATIC.fetch_add(1, Ordering::SeqCst);
    CLIF_TLS.set(CLIF_TLS.get() + 1);
    LLVM_TLS.set(LLVM_TLS.get() + 1);
    (
        CLIF_STATIC.load(Ordering::SeqCst),
        LLVM_STATIC.load(Ordering::SeqCst),
        CLIF_TLS.get(),
        LLVM_TLS.get(),
    )
}

pub fn llvm_call(f: &dyn Fn() -> u64) -> u64 {
    let _guard = DropGuard;
    f() + 2
}

pub fn llvm_call_clif(f: &dyn Fn() -> u64) -> u64 {
    llvm_delegate_clif::clif_call(f) + 2
}

#[cfg(unwind)]
pub fn llvm_panic() -> ! {
    let _guard = DropGuard;
    panic!("panic in the LLVM crate");
}

#[cfg(unwind)]
pub fn llvm_catches_clif_panic() -> bool {
    let _guard = DropGuard;
    std::panic::catch_unwind(|| llvm_delegate_clif::clif_panic()).is_err()
}
