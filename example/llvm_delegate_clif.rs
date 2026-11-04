// Compiled at opt-level 0, so by Cranelift. The upstream crate of `llvm_delegate_llvm.rs`.

use std::cell::Cell;
use std::fmt::Display;
use std::panic::Location;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const FILE: &str = file!();

pub static CLIF_STATIC: AtomicUsize = AtomicUsize::new(0);

pub static DROPS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    pub static CLIF_TLS: Cell<u32> = const { Cell::new(10) };
}

pub struct DropGuard;

impl Drop for DropGuard {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

pub trait Shape {
    fn area(&self) -> u64;
}

struct Square(u64);

impl Shape for Square {
    fn area(&self) -> u64 {
        self.0 * self.0
    }
}

pub fn clif_add(a: u64, b: u64) -> u64 {
    a + b
}

#[inline]
pub fn clif_inline(x: u32) -> u32 {
    x * 3
}

#[track_caller]
pub fn clif_location() -> &'static Location<'static> {
    Location::caller()
}

pub fn clif_shape(side: u64) -> Box<dyn Shape> {
    Box::new(Square(side))
}

pub fn clif_generic<T: Display>(value: T) -> String {
    format!("clif<{value}>")
}

pub fn clif_call(f: &dyn Fn() -> u64) -> u64 {
    let _guard = DropGuard;
    f() + 1
}

#[cfg(unwind)]
pub fn clif_panic() -> ! {
    let _guard = DropGuard;
    panic!("panic in the Cranelift crate");
}
