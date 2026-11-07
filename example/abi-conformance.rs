// Calls between code compiled by different backends, for what abi-cafe cannot express: C-unwind,
// C variadics and SIMD vectors.
//
// The build system compiles this file twice, once with `--cfg callee` into an rlib that defines
// every function below, and once with `--cfg caller` into the executable that declares them as
// `extern` and calls them. Both crates are built from the same list of cases, one with Cranelift
// and one with LLVM, in both directions. Every argument and return value is derived from its
// position, so a value that ends up in the wrong register or stack slot is reported by name.
//
// Defining a variadic function is not supported by Cranelift, so these cases are only built with
// `--cfg variadic` when LLVM is the callee.

#![feature(c_variadic_int128, simd_ffi, f16, f128, portable_simd)]
#![allow(improper_ctypes, improper_ctypes_definitions, non_camel_case_types)]
#![allow(dead_code, unused_mut, unused_variables, unused_features, private_interfaces)]

use std::fmt::Debug;
use std::simd::{
    f32x4, f32x8, f64x2, f64x4, i8x16, i16x8, i32x4, i64x2, u8x16, u8x32, u16x8, u32x4, u64x2,
};
use std::sync::atomic::{AtomicUsize, Ordering};

static FAILURES: AtomicUsize = AtomicUsize::new(0);

fn mix(seed: u64) -> u64 {
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// A value that can be rebuilt from a seed on both sides of a call.
trait Val: Copy + PartialEq + Debug {
    fn val(seed: u64) -> Self;
}

macro_rules! impl_val_int {
    ($($ty:ident)*) => {$(
        impl Val for $ty {
            fn val(seed: u64) -> Self {
                ((mix(seed) as u128) << 64 | mix(seed ^ 0xFFFF) as u128) as $ty
            }
        }
    )*};
}
impl_val_int!(u8 i8 u16 i16 u32 i32 u64 i64 u128 i128 usize isize);

macro_rules! impl_val_float {
    ($($ty:ident)*) => {$(
        impl Val for $ty {
            fn val(seed: u64) -> Self {
                (mix(seed) % 1000) as $ty + 0.5
            }
        }
    )*};
}
impl_val_float!(f16 f32 f64 f128);

impl Val for bool {
    fn val(seed: u64) -> Self {
        mix(seed) & 1 == 1
    }
}

impl Val for () {
    fn val(_: u64) -> Self {}
}

impl Val for *const u8 {
    fn val(seed: u64) -> Self {
        (mix(seed) & 0xFFFF_FFF0) as *const u8
    }
}

impl<A: Val, B: Val> Val for (A, B) {
    fn val(seed: u64) -> Self {
        (A::val(seed * 2), B::val(seed * 2 + 1))
    }
}

impl<T: Val, const N: usize> Val for [T; N] {
    fn val(seed: u64) -> Self {
        std::array::from_fn(|i| T::val(seed * 64 + i as u64))
    }
}

impl<T: Val> Val for Option<T> {
    fn val(seed: u64) -> Self {
        if mix(seed) & 7 == 0 { None } else { Some(T::val(seed)) }
    }
}

macro_rules! impl_val_simd {
    ($($ty:ident)*) => {$(
        impl Val for $ty {
            fn val(seed: u64) -> Self {
                Self::from_array(std::array::from_fn(|i| Val::val(seed * 64 + i as u64)))
            }
        }
    )*};
}
impl_val_simd!(f32x4 f32x8 f64x2 f64x4 i8x16 i16x8 i32x4 i64x2 u8x16 u8x32 u16x8 u32x4 u64x2);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct HfaF32x3(f32, f32, f32);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct MixedFloatInt(f64, u64);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct Big([u64; 5]);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C, align(16))]
struct Align16(u64, u64);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct IntPair(u64, u64);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct FloatPair(f32, f32);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct DoublePair(f64, f64);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct WrapU128(u128);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct WrapF128(f128);

macro_rules! impl_val_two_fields {
    ($($ty:ident)*) => {$(
        impl Val for $ty {
            fn val(seed: u64) -> Self {
                $ty(Val::val(seed * 2), Val::val(seed * 2 + 1))
            }
        }
    )*};
}
impl_val_two_fields!(MixedFloatInt IntPair FloatPair DoublePair Align16);

macro_rules! impl_val_one_field {
    ($($ty:ident)*) => {$(
        impl Val for $ty {
            fn val(seed: u64) -> Self {
                $ty(Val::val(seed))
            }
        }
    )*};
}
impl_val_one_field!(WrapU128 WrapF128 Big);

impl Val for HfaF32x3 {
    fn val(seed: u64) -> Self {
        HfaF32x3(Val::val(seed * 4), Val::val(seed * 4 + 1), Val::val(seed * 4 + 2))
    }
}

const RETURN_SEED: u64 = 1_000_000;

fn check<T: Val>(case: &str, arg: &str, got: T, seed: u64) {
    let expected = T::val(seed);
    if got != expected {
        eprintln!("{case}: {arg} is {got:?}, expected {expected:?}");
        FAILURES.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(callee)]
#[unsafe(no_mangle)]
pub extern "C" fn callee_failures() -> usize {
    FAILURES.load(Ordering::Relaxed)
}

/// Define a function that checks all its arguments, and returns a known value, in the callee and
/// call it with the arguments the callee expects in the caller.
macro_rules! cases {
    ($($abi:literal fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty;)*) => {
        $(
            #[cfg(callee)]
            #[unsafe(no_mangle)]
            pub extern $abi fn $name($($arg: $ty),*) -> $ret {
                let mut _seed = 0;
                $(
                    check(stringify!($name), stringify!($arg), $arg, _seed);
                    _seed += 1;
                )*
                <$ret as Val>::val(RETURN_SEED)
            }
        )*

        #[cfg(caller)]
        fn call_cases() {
            $({
                unsafe extern $abi {
                    fn $name($($arg: $ty),*) -> $ret;
                }
                let mut _seed = 0;
                $(
                    let $arg = <$ty as Val>::val(_seed);
                    _seed += 1;
                )*
                let ret = unsafe { $name($($arg),*) };
                check(stringify!($name), "the return value", ret, RETURN_SEED);
            })*
        }
    };
}

/// Like `cases!`, but the arguments after the semicolon are passed as variadic arguments.
macro_rules! variadic_cases {
    ($caller:ident; $(fn $name:ident($($arg:ident: $ty:ty),*; $($vty:ty),*) -> $ret:ty;)*) => {
        $(
            #[cfg(all(callee, variadic))]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $name($($arg: $ty,)* mut va: ...) -> $ret {
                let mut _seed = 0;
                $(
                    check(stringify!($name), stringify!($arg), $arg, _seed);
                    _seed += 1;
                )*
                $(
                    check(stringify!($name), "a variadic argument", unsafe { va.next_arg::<$vty>() }, _seed);
                    _seed += 1;
                )*
                <$ret as Val>::val(RETURN_SEED)
            }
        )*

        #[cfg(all(caller, variadic))]
        fn $caller() {
            $({
                unsafe extern "C" {
                    fn $name($($arg: $ty,)* ...) -> $ret;
                }
                let mut _seed = 0;
                $(
                    let $arg = <$ty as Val>::val(_seed);
                    _seed += 1;
                )*
                let ret = unsafe { $name($($arg,)* $({
                    _seed += 1;
                    <$vty as Val>::val(_seed - 1)
                }),*) };
                check(stringify!($name), "the return value", ret, RETURN_SEED);
            })*
        }
    };
}

cases! {
    // The known register assignment mismatch: the 128-bit integer does not fit in the remaining
    // integer registers on x86_64 SysV and is passed on the stack, while the next argument still
    // uses the register that is left.
    "C" fn u128_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: u128, y: u64) -> ();
    "C" fn i128_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: i128, y: u8, z: f64) -> ();
    "C" fn u128_after_seven_u64(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: u128, y: u64) -> ();
    "Rust" fn rust_u128_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: u128, y: u64) -> ();
    "Rust" fn rust_pair_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: (u128, u64), y: u64) -> ();
    "C" fn u128_pair_return(a: u64, b: u64, c: u64, d: u64, e: u64, x: u128, y: u64) -> u128;

    // An aggregate that needs more registers than are left is passed on the stack as a whole, and
    // the registers that are left stay free (x86_64) or are used up (aarch64).
    "C" fn int_pair_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: IntPair, y: u64) -> ();
    "C" fn int_pair_after_seven_u64(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: IntPair, y: u64) -> ();
    "C" fn mixed_pair_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: MixedFloatInt, y: u64, z: f64) -> ();
    "C" fn mixed_pair_after_seven_u64(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: MixedFloatInt, y: u64, z: f64) -> ();
    "C" fn float_pair_after_seven_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, x: FloatPair, y: f64) -> ();
    "C" fn double_pair_after_seven_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, x: DoublePair, y: f64) -> ();
    "C" fn hfa_after_six_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, x: HfaF32x3, y: f64) -> ();
    "C" fn mixed_pair_after_seven_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, x: MixedFloatInt, y: u64, z: f64) -> ();
    "C" fn align16_after_one_u64(a: u64, x: Align16, y: u64) -> ();
    "C" fn align16_after_three_u64(a: u64, b: u64, c: u64, x: Align16, y: u64) -> ();

    // An aggregate that is passed in memory because no registers are left must keep its alignment
    // of 16 bytes on the stack.
    "C" fn align16_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: Align16, y: u64) -> ();
    "C" fn align16_after_seven_u64(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: Align16, y: u64) -> ();
    "C" fn wrapped_u128_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: WrapU128, y: u64) -> ();
    "C" fn wrapped_u128_after_seven_u64(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: WrapU128, y: u64) -> ();
    "C" fn wrapped_f128_after_nine_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: f64, x: WrapF128, y: f64) -> ();
    "Rust" fn rust_align16_after_seven_u64(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: Align16, y: u64) -> Align16;

    // The same shapes with the `C-unwind` ABI, which must be call-compatible with `C`.
    "C-unwind" fn unwind_scalars(a: u8, b: i16, c: u32, d: i64, e: u128, f: f32, g: f64) -> u64;
    "C-unwind" fn unwind_u128_after_five_u64(a: u64, b: u64, c: u64, d: u64, e: u64, x: u128, y: u64) -> ();
    "C-unwind" fn unwind_many_floats(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: f64, j: f32, k: u8) -> f64;
    "C-unwind" fn unwind_aggregates(a: HfaF32x3, b: MixedFloatInt, c: Big, d: u8) -> MixedFloatInt;
    "C-unwind" fn unwind_return_big(a: u64) -> Big;
    "C-unwind" fn unwind_f16_f128(a: f16, b: f128, c: f16, d: f32) -> f16;
    "C-unwind" fn unwind_simd(a: f32x4, b: u8, c: i32x4) -> f32x4;

    // Float registers run out and the vector no longer fits in a register.
    "C" fn simd_f32x4(a: f32x4) -> f32x4;
    "C" fn simd_i8x16(a: i8x16, b: u8x16) -> i8x16;
    "C" fn simd_f64x2(a: f64x2, b: i64x2, c: u64x2) -> f64x2;
    "C" fn simd_int_mix(a: u8, b: f32x4, c: u64, d: u16x8, e: i32x4, f: u32) -> u64x2;
    "C" fn simd_float_mix(a: f64, b: f32x4, c: f32, d: f64x2, e: f64) -> f32x4;
    "C" fn simd_after_7_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, x: f32x4, y: f64) -> f32x4;
    "C" fn simd_after_8_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, x: f32x4, y: f64, z: u64) -> f32x4;
    "C" fn simd_after_9_f64(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: f64, x: f32x4, y: f64, z: u64) -> f32x4;
    "C" fn simd_eight_vectors(a: f32x4, b: f32x4, c: f32x4, d: f32x4, e: f32x4, f: f32x4, g: f32x4, h: f32x4, i: f32x4, j: f64) -> f32x4;
    "C" fn simd_after_ints(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64, i: u64, x: u8x16, y: u64) -> u8x16;
    "C" fn simd_stack_pack(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64, i: u8, j: f32x4, k: u16, l: u8) -> ();
    "C" fn simd_with_f16_f128(a: f16, b: f32x4, c: f128, d: f64x2, e: f16) -> f64x2;
    "Rust" fn rust_simd_f32x4(a: f32x4, b: u8, c: f32x4) -> f32x4;
    "Rust" fn rust_simd_f32x8(a: f32x8, b: u8, c: f32x8) -> f32x8;
    "Rust" fn rust_simd_u8x32(a: u8x32, b: f64x4, c: u64) -> u8x32;
    "Rust" fn rust_simd_after_ints(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64, x: f64x4, y: u64) -> f64x4;
    "Rust" fn rust_simd_many(a: f32x4, b: f32x4, c: f32x4, d: f32x4, e: f32x4, f: f32x4, g: f32x4, h: f32x4, i: f32x4, j: u8) -> ();

    // Types that do not have the same representation in Rust and C, passed with the Rust ABI.
    "Rust" fn rust_option_u128(a: u8, b: Option<u128>, c: Option<f64>, d: Option<u8>, e: u64) -> Option<u128>;
    "Rust" fn rust_arrays(a: [u8; 3], b: [u16; 5], c: [f32; 3], d: [u128; 2], e: [u8; 0], f: u64) -> [u32; 7];
    "Rust" fn rust_pairs(a: (u8, u128), b: (f32, u8), c: (u64, f64), d: (f64, f64), e: (i128, u8)) -> (u64, u128);
}

variadic_cases! {
    call_variadic_cases;

    // The number of integer registers used by the variadic arguments, and the point where they
    // spill to the stack. Apple arm64 passes all of them on the stack.
    fn variadic_none(a: u32;) -> ();
    fn variadic_ints_1(a: u32; i32) -> ();
    fn variadic_ints_3(a: u8; u32, i64, u64) -> u64;
    fn variadic_small_ints(a: u32; i32, u32, i32, u32, i32, u32, i32, u32, i32, u32, i32, u32) -> ();
    fn variadic_ints_5(a: u32; u64, u64, u64, u64, u64) -> ();
    fn variadic_ints_6(a: u32; u64, u64, u64, u64, u64, u64) -> ();
    fn variadic_ints_7(a: u32; u64, u64, u64, u64, u64, u64, u64) -> ();
    fn variadic_ints_9(a: u32; u64, u64, u64, u64, u64, u64, u64, u64, u64) -> ();
    fn variadic_pointers(a: u32; *const u8, usize, isize, *const u8) -> *const u8;

    // Fixed arguments that already use up registers before the variadic arguments start.
    fn variadic_fixed_eight_ints(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64; u64, u64) -> ();
    fn variadic_fixed_pair(a: u64, b: (u128, u64), c: u8; u32, i64) -> u64;
}

// `__int128` is available for all 64-bit targets we support, and is passed in a register pair
// that must not be split between registers and the stack.
variadic_cases! {
    call_variadic_i128_cases;

    fn variadic_u128(a: u32; u128, i128) -> u128;
    fn variadic_u128_after_four_u64(a: u32; u64, u64, u64, u64, u128, u64) -> ();
    fn variadic_u128_after_five_u64(a: u32; u64, u64, u64, u64, u64, u128, u64) -> ();
    fn variadic_u128_after_six_u64(a: u32; u64, u64, u64, u64, u64, u64, u128, u64) -> ();
}

// Cranelift can't call variadic functions with float arguments on x86_64, as it can't set the
// number of vector registers in `al`.
#[cfg(variadic_float)]
variadic_cases! {
    call_variadic_float_cases;

    fn variadic_floats_1(a: u32; f64) -> ();
    fn variadic_floats_8(a: u32; f64, f64, f64, f64, f64, f64, f64, f64) -> ();
    fn variadic_floats_9(a: u32; f64, f64, f64, f64, f64, f64, f64, f64, f64) -> ();
    fn variadic_floats_10(a: u32; f64, f64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64;
    fn variadic_alternating(a: u32; u64, f64, u64, f64, u64, f64, u64, f64, u64, f64, u64, f64, u64, f64) -> ();
    fn variadic_fixed_floats(a: f64, b: f32, c: u8; u64, f64) -> ();
    fn variadic_fixed_eight_floats(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, g: f64, h: f64; u64, f64) -> ();
    fn variadic_fixed_f16(a: f16, b: u16, c: i8; i32, f64) -> ();
}

#[cfg(all(variadic_float, not(any(target_vendor = "apple", target_env = "msvc"))))]
variadic_cases! {
    call_variadic_f128_cases;

    fn variadic_f128(a: u32; f128, f64, f128) -> f128;
    fn variadic_f128_after_seven_f64(a: u32; f64, f64, f64, f64, f64, f64, f64, f128, f64) -> ();
}

#[cfg(caller)]
extern crate abi_callee;

#[cfg(caller)]
fn main() {
    unsafe extern "C" {
        fn callee_failures() -> usize;
    }

    call_cases();
    #[cfg(variadic)]
    {
        call_variadic_cases();
        call_variadic_i128_cases();
    }
    #[cfg(variadic_float)]
    {
        call_variadic_float_cases();
        #[cfg(not(any(target_vendor = "apple", target_env = "msvc")))]
        call_variadic_f128_cases();
    }
    let failures = FAILURES.load(Ordering::Relaxed) + unsafe { callee_failures() };
    if failures != 0 {
        eprintln!("{failures} ABI mismatches");
        std::process::exit(1);
    }
}
