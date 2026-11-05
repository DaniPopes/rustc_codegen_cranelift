//! Caller half of a cross-backend ABI test, see `mixed-backend-abi-callee.rs`.

#![allow(improper_ctypes_definitions)]

use std::hint::black_box;

use callee::{fold, mix};

macro_rules! check {
    ($module:ident, $f:ident($($arg:expr),*), $expected:expr) => {
        assert_eq!($f($($arg),*), $expected, "{}::{}", stringify!($module), stringify!($f))
    };
}

macro_rules! check_abi {
    ($module:ident) => {{
        use callee::$module::*;

        let x = black_box(0x0123_4567_89ab_cdef_0fed_cba9_8765_4321u128);
        let y = black_box(0x1111_2222_3333_4444_5555_6666_7777_8888u128);
        let [a, b, c, d, e, f, g, h] = black_box([1u64, 2, 3, 4, 5, 6, 7, 8]);
        let (i, j) = black_box((9u8, 10u8));
        let w = Wide(x, 11);
        let n = Narrow(12, y);

        check!($module, five_ints_i128_int(a, b, c, d, e, x, 9), mix(&[a, b, c, d, e, fold(x), 9]));
        check!(
            $module,
            seven_ints_i128_int(a, b, c, d, e, f, g, x, 9),
            mix(&[a, b, c, d, e, f, g, fold(x), 9])
        );
        check!($module, int_i128_int_i128_int(a, x, b, y, c), mix(&[a, fold(x), b, fold(y), c]));
        check!($module, four_ints_wide_int(a, b, c, d, w, 13), mix(&[a, b, c, d, fold(x), 11, 13]));
        check!(
            $module,
            five_ints_wide_int(a, b, c, d, e, w, 13),
            mix(&[a, b, c, d, e, fold(x), 11, 13])
        );
        check!(
            $module,
            four_ints_narrow_int(a, b, c, d, n, 13),
            mix(&[a, b, c, d, 12, fold(y), 13])
        );
        check!(
            $module,
            five_ints_narrow_int(a, b, c, d, e, n, 13),
            mix(&[a, b, c, d, e, 12, fold(y), 13])
        );
        check!(
            $module,
            stack_alignment(a, b, c, d, e, f, g, h, i, x, j, y),
            mix(&[a, b, c, d, e, f, g, h, i as u64, fold(x), j as u64, fold(y)])
        );

        let (a16, wrap) = (Align16(a, b), WrapU128(x));
        let (u8_u128, u128_f64) = (U8U128(i, y), U128F64(x, 1.5));
        let union = U128Union { x: y };

        check!(
            $module,
            seven_ints_wide_int(a, b, c, d, e, f, g, w, 13),
            mix(&[a, b, c, d, e, f, g, fold(x) ^ 11, 13])
        );
        check!(
            $module,
            seven_ints_narrow_int(a, b, c, d, e, f, g, n, 13),
            mix(&[a, b, c, d, e, f, g, 12 ^ fold(y), 13])
        );
        check!(
            $module,
            seven_ints_align16_int(a, b, c, d, e, f, g, a16, 13),
            mix(&[a, b, c, d, e, f, g, a ^ b, 13])
        );
        check!(
            $module,
            seven_ints_wrap_int(a, b, c, d, e, f, g, wrap, 13),
            mix(&[a, b, c, d, e, f, g, fold(x), 13])
        );
        check!(
            $module,
            seven_ints_u8_u128_int(a, b, c, d, e, f, g, u8_u128, 13),
            mix(&[a, b, c, d, e, f, g, i as u64 ^ fold(y), 13])
        );
        check!(
            $module,
            seven_ints_u128_f64_int(a, b, c, d, e, f, g, u128_f64, 13),
            mix(&[a, b, c, d, e, f, g, fold(x) ^ 1.5f64.to_bits(), 13])
        );
        check!(
            $module,
            seven_ints_union_int(a, b, c, d, e, f, g, union, 13),
            mix(&[a, b, c, d, e, f, g, fold(y), 13])
        );

        check!($module, ret_i128(a), ((a as u128) << 64) | (a as u128 * 3));
        let Wide(r, s) = ret_wide(a);
        assert_eq!((r, s), (ret_i128(a), a * 5), "{}::ret_wide", stringify!($module));
        let Narrow(s, r) = ret_narrow(a);
        assert_eq!((s, r), (a * 7, ret_i128(a)), "{}::ret_narrow", stringify!($module));
    }};
}

fn main() {
    check_abi!(rust);
    check_abi!(c);
}
