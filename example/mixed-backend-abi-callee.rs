//! Callee half of a cross-backend ABI test: this crate is built with one codegen backend and
//! called from `mixed-backend-abi.rs`, which is built with the other one.

#![feature(explicit_tail_calls, rust_cold_cc, rust_preserve_none_cc, rust_tail_cc)]
#![allow(improper_ctypes_definitions, incomplete_features)]

#[inline]
pub fn mix(vals: &[u64]) -> u64 {
    vals.iter().fold(0, |acc, &v| acc.wrapping_mul(1_000_003).wrapping_add(v))
}

#[inline]
pub fn fold(x: u128) -> u64 {
    x as u64 ^ ((x >> 64) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

macro_rules! define_many_args {
    ($abi:literal) => {
        // More arguments than there are registers in any convention.
            #[inline(never)]
            pub extern $abi fn many_args(
                a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64, a7: u64, a8: u64,
                a9: u64, a10: u64, a11: u64, a12: u64, a13: u64, a14: u64, a15: u64, a16: u64,
                a17: u64, a18: u64, a19: u64, a20: u64, a21: u64, a22: u64, a23: u64, a24: u64,
                a25: u64, f0: f64, f1: f64, f2: f64, f3: f64, f4: f64, f5: f64, f6: f64, f7: f64,
                f8: f64, f9: f64,
            ) -> f64 {
                let ints = mix(&[
                    a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17,
                    a18, a19, a20, a21, a22, a23, a24, a25,
                ]);
                ints as f64 + f0 + f1 * 2.0 + f2 * 3.0 + f3 * 4.0 + f4 * 5.0 + f5 * 6.0 + f6 * 7.0
                    + f7 * 8.0 + f8 * 9.0 + f9 * 10.0
            }

    };
}

macro_rules! define_abi {
    ($module:ident, $abi:literal, $(#[$repr:meta])*) => {
        pub mod $module {
            use super::{fold, mix};

            // `ScalarPair`s in the Rust ABI.
            $(#[$repr])*
            #[derive(Clone, Copy)]
            pub struct Wide(pub u128, pub u64);
            $(#[$repr])*
            #[derive(Clone, Copy)]
            pub struct Narrow(pub u64, pub u128);

            $(#[$repr])*
            #[repr(align(16))]
            #[derive(Clone, Copy)]
            pub struct Align16(pub u64, pub u64);
            $(#[$repr])*
            #[derive(Clone, Copy)]
            pub struct WrapU128(pub u128);
            $(#[$repr])*
            #[derive(Clone, Copy)]
            pub struct U8U128(pub u8, pub u128);
            $(#[$repr])*
            #[derive(Clone, Copy)]
            pub struct U128F64(pub u128, pub f64);
            $(#[$repr])*
            #[derive(Clone, Copy)]
            pub union U128Union {
                pub x: u128,
                pub y: [u64; 2],
            }

            // An aggregate with the alignment of an i128 goes on the stack, after a single
            // slot with seven integer arguments, aligned to 16 bytes.
            macro_rules! seven_ints_struct {
                ($name:ident, $ty:ty, $s:ident => $fold:expr) => {
                    #[inline(never)]
                    pub extern $abi fn $name(
                        a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, $s: $ty, z: u64,
                    ) -> u64 {
                        mix(&[a, b, c, d, e, f, g, $fold, z])
                    }
                };
            }

            seven_ints_struct!(seven_ints_wide_int, Wide, w => fold(w.0) ^ w.1);
            seven_ints_struct!(seven_ints_narrow_int, Narrow, n => n.0 ^ fold(n.1));
            seven_ints_struct!(seven_ints_align16_int, Align16, s => s.0 ^ s.1);
            seven_ints_struct!(seven_ints_wrap_int, WrapU128, s => fold(s.0));
            seven_ints_struct!(seven_ints_u8_u128_int, U8U128, s => s.0 as u64 ^ fold(s.1));
            seven_ints_struct!(
                seven_ints_u128_f64_int, U128F64, s => fold(s.0) ^ s.1.to_bits()
            );
            seven_ints_struct!(seven_ints_union_int, U128Union, s => fold(unsafe { s.x }));

            // The x86_64 SysV i128 goes on the stack with r9 left free for `y`.
            #[inline(never)]
            pub extern $abi fn five_ints_i128_int(
                a: u64, b: u64, c: u64, d: u64, e: u64, x: u128, y: u64,
            ) -> u64 {
                mix(&[a, b, c, d, e, fold(x), y])
            }

            // On aarch64 x7 is the only register left for the i128, so it goes on the stack and
            // `y` follows it there.
            #[inline(never)]
            pub extern $abi fn seven_ints_i128_int(
                a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, x: u128, y: u64,
            ) -> u64 {
                mix(&[a, b, c, d, e, f, g, fold(x), y])
            }

            // i128 starts at an even register on AAPCS64 but not on Apple.
            #[inline(never)]
            pub extern $abi fn int_i128_int_i128_int(
                a: u64, x: u128, b: u64, y: u128, c: u64,
            ) -> u64 {
                mix(&[a, fold(x), b, fold(y), c])
            }

            #[inline(never)]
            pub extern $abi fn four_ints_wide_int(
                a: u64, b: u64, c: u64, d: u64, w: Wide, z: u64,
            ) -> u64 {
                mix(&[a, b, c, d, fold(w.0), w.1, z])
            }

            #[inline(never)]
            pub extern $abi fn five_ints_wide_int(
                a: u64, b: u64, c: u64, d: u64, e: u64, w: Wide, z: u64,
            ) -> u64 {
                mix(&[a, b, c, d, e, fold(w.0), w.1, z])
            }

            #[inline(never)]
            pub extern $abi fn four_ints_narrow_int(
                a: u64, b: u64, c: u64, d: u64, n: Narrow, z: u64,
            ) -> u64 {
                mix(&[a, b, c, d, n.0, fold(n.1), z])
            }

            #[inline(never)]
            pub extern $abi fn five_ints_narrow_int(
                a: u64, b: u64, c: u64, d: u64, e: u64, n: Narrow, z: u64,
            ) -> u64 {
                mix(&[a, b, c, d, e, n.0, fold(n.1), z])
            }

            // Small stack slots are packed on Apple aarch64 but not elsewhere.
            #[inline(never)]
            pub extern $abi fn stack_alignment(
                a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64,
                i: u8, x: u128, j: u8, y: u128,
            ) -> u64 {
                mix(&[a, b, c, d, e, f, g, h, i as u64, fold(x), j as u64, fold(y)])
            }

            define_many_args!($abi);

            #[inline(never)]
            pub extern $abi fn ret_i128(a: u64) -> u128 {
                ((a as u128) << 64) | (a as u128).wrapping_mul(3)
            }

            #[inline(never)]
            pub extern $abi fn ret_wide(a: u64) -> Wide {
                Wide(ret_i128(a), a.wrapping_mul(5))
            }

            #[inline(never)]
            pub extern $abi fn ret_narrow(a: u64) -> Narrow {
                Narrow(a.wrapping_mul(7), ret_i128(a))
            }
        }
    };
}

define_abi!(rust, "Rust",);
define_abi!(c, "C", #[repr(C)]);
define_abi!(rust_cold, "rust-cold",);
define_abi!(tail, "tail",);

/// A function with the `preserve_most` convention on x86_64 and aarch64 that uses many registers,
/// including the ones that the convention preserves and the vector ones that it doesn't.
pub mod cold {
    use std::hint::black_box;

    #[inline(never)]
    pub extern "rust-cold" fn burn(seed: u64) -> u64 {
        let mut values = [0u64; 24];
        for (i, value) in values.iter_mut().enumerate() {
            *value = black_box(seed.wrapping_mul(i as u64 + 3));
        }
        let mut acc = 0u64;
        for i in 0..24 {
            acc = acc.wrapping_mul(31).wrapping_add(values[i] ^ values[23 - i]);
        }
        acc
    }

    #[inline(never)]
    pub extern "rust-cold" fn burn_floats(seed: f64) -> f64 {
        let mut values = [0f64; 24];
        for (i, value) in values.iter_mut().enumerate() {
            *value = black_box(seed * (i as f64 + 1.5));
        }
        let mut acc = 0f64;
        for i in 0..24 {
            acc = acc * 0.5 + values[i] - values[23 - i] * 0.25;
        }
        acc
    }
}

// LLVM can't compile `i128` arguments in this convention on x86_64.
pub mod preserve_none {
    use super::mix;

    define_many_args!("rust-preserve-none");

    #[inline(never)]
    pub extern "rust-preserve-none" fn small(a: u8, b: u16, c: u64, d: u32, e: bool) -> u64 {
        mix(&[a as u64, b as u64, c, d as u64, e as u64])
    }

    #[inline(never)]
    pub extern "rust-preserve-none" fn small_late(
        a: u64,
        b: u64,
        c: u64,
        d: u64,
        e: u64,
        f: u64,
        g: u64,
        h: u8,
        i: u64,
    ) -> u64 {
        mix(&[a, b, c, d, e, f, g, h as u64, i])
    }

    #[derive(Clone, Copy)]
    pub struct Pair(pub u64, pub f64);

    #[inline(never)]
    pub extern "rust-preserve-none" fn pair(a: u64, p: Pair, b: u64) -> Pair {
        Pair(mix(&[a, p.0, b]), p.1 + a as f64 + b as f64)
    }

    #[derive(Clone, Copy)]
    pub struct Big(pub [u64; 5]);

    #[inline(never)]
    pub extern "rust-preserve-none" fn big(a: u64, x: Big, b: u64, y: Big) -> Big {
        let mut res = x.0;
        for (i, r) in res.iter_mut().enumerate() {
            *r = mix(&[a, *r, y.0[4 - i], b]);
        }
        Big(res)
    }
}

/// The signature of every hop of the chain of tail calls between the two crates.
pub type Link = extern "rust-preserve-none" fn(
    u64,
    u64,
    usize,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    u64,
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
    f64,
) -> u64;

/// A hop of a chain that bounces between the two crates with tail calls. The ints and floats
/// change on every hop, so every argument is checked.
#[inline(never)]
pub extern "rust-preserve-none" fn chain(
    n: u64,
    acc: u64,
    next: usize,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: u64,
    h: u64,
    f0: f64,
    f1: f64,
    f2: f64,
    f3: f64,
    f4: f64,
    f5: f64,
    f6: f64,
    f7: f64,
    f8: f64,
    f9: f64,
) -> u64 {
    if n == 0 {
        return acc + a + b + c + d + e + f + g + h;
    }
    let acc = acc + a + h + (f0 + f9) as u64;
    let next: Link = unsafe { std::mem::transmute(next) };
    become next(
        n - 1,
        acc,
        chain as *const () as usize,
        a + 1,
        b,
        c,
        d,
        e,
        f,
        g,
        h + 2,
        f0 + 1.0,
        f1,
        f2,
        f3,
        f4,
        f5,
        f6,
        f7,
        f8,
        f9 + 0.5,
    )
}
