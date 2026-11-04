//! Callee half of a cross-backend ABI test: this crate is built with one codegen backend and
//! called from `mixed-backend-abi.rs`, which is built with the other one.

#![allow(improper_ctypes_definitions)]

#[inline]
pub fn mix(vals: &[u64]) -> u64 {
    vals.iter().fold(0, |acc, &v| acc.wrapping_mul(1_000_003).wrapping_add(v))
}

#[inline]
pub fn fold(x: u128) -> u64 {
    x as u64 ^ ((x >> 64) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
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

            #[derive(Clone, Copy)]
            #[repr(align(16))]
            $(#[$repr])*
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
