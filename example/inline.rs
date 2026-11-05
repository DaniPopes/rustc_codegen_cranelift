// Tests for the behavior of inlined calls.

use std::cell::Cell;
use std::hint::black_box;
use std::panic::Location;

// Callees with stack slots of different sizes and alignments, inlined at several call sites.

#[repr(align(16))]
struct Aligned(u8);

#[inline(always)]
fn small_slots(x: u64) -> u64 {
    let bytes = [x as u8; 3];
    let word = Cell::new(x as u32);
    black_box((&bytes, &word));
    u64::from(bytes[2]) + u64::from(word.get())
}

#[inline(always)]
fn aligned_slots(x: u64) -> u64 {
    let aligned = Aligned(x as u8);
    let wide = Cell::new(u128::from(x) << 64);
    black_box((&aligned, &wide));
    assert_eq!(&aligned as *const Aligned as usize % 16, 0);
    assert_eq!(wide.as_ptr() as usize % 16, 0);
    u64::from(aligned.0) + (wide.get() >> 64) as u64
}

#[inline(always)]
fn nested_slots(x: u64) -> u64 {
    let local = Cell::new(x);
    black_box(&local);
    aligned_slots(small_slots(local.get())) + local.get()
}

fn small_expected(x: u64) -> u64 {
    u64::from(x as u8) + u64::from(x as u32)
}

fn aligned_expected(x: u64) -> u64 {
    u64::from(x as u8) + x
}

fn inline_always_stack_slots() {
    for x in [0, 1, 7, 100, 255, 256, u32::MAX as u64] {
        let sum = small_slots(x) + aligned_slots(x.wrapping_add(1)) + small_slots(x);
        assert_eq!(sum, 2 * small_expected(x) + aligned_expected(x.wrapping_add(1)));
        assert_eq!(nested_slots(x), aligned_expected(small_expected(x)) + x);
    }
}

// Calls through the inlined bodies keep working.

#[inline(always)]
#[track_caller]
fn tracked() -> &'static Location<'static> {
    Location::caller()
}

#[inline(always)]
#[track_caller]
fn nested_tracked() -> &'static Location<'static> {
    tracked()
}

fn inline_always_track_caller() {
    let location = tracked();
    assert_eq!((location.file(), location.line(), location.column()), (file!(), line!() - 1, 20));
    let location = nested_tracked();
    assert_eq!((location.file(), location.line(), location.column()), (file!(), line!() - 1, 20));
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn asm_add(a: u64, b: u64) -> u64 {
    let sum;
    unsafe {
        std::arch::asm!(
            "lea {sum}, [{a} + {b}]", a = in(reg) a, b = in(reg) b, sum = lateout(reg) sum,
        );
    }
    sum
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn asm_add(a: u64, b: u64) -> u64 {
    let sum;
    unsafe {
        std::arch::asm!(
            "add {sum}, {a}, {b}", a = in(reg) a, b = in(reg) b, sum = lateout(reg) sum,
        );
    }
    sum
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[inline(always)]
fn asm_add(a: u64, b: u64) -> u64 {
    a + b
}

fn inline_always_asm() {
    let x = black_box(40);
    assert_eq!(asm_add(x, 2) + asm_add(x, 3) * 2, 42 + 43 * 2);
}

struct Guard<'a>(&'a Cell<u32>);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[inline(always)]
fn panics_if(panic: bool, drops: &Cell<u32>) -> u64 {
    let _guard = Guard(drops);
    if panic {
        panic!("inlined");
    }
    7
}

#[inline(always)]
fn calls_panics_if(panic: bool, drops: &Cell<u32>) -> u64 {
    let _guard = Guard(drops);
    panics_if(panic, drops) + 1
}

#[cfg(panic = "unwind")]
fn inline_always_unwinding() {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let drops = Cell::new(0);
    let ok = catch_unwind(AssertUnwindSafe(|| calls_panics_if(black_box(false), &drops)));
    assert_eq!(ok.ok(), Some(8));
    assert_eq!(drops.get(), 2);

    drops.set(0);
    let err = catch_unwind(AssertUnwindSafe(|| calls_panics_if(black_box(true), &drops)));
    assert!(err.is_err());
    assert_eq!(drops.get(), 2);
}

#[cfg(not(panic = "unwind"))]
fn inline_always_unwinding() {
    let drops = Cell::new(0);
    assert_eq!(calls_panics_if(black_box(false), &drops), 8);
    assert_eq!(drops.get(), 2);
}

fn main() {
    inline_always_stack_slots();
    inline_always_track_caller();
    inline_always_asm();
    inline_always_unwinding();
}
