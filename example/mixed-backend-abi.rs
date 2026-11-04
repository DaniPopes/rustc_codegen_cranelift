//! Caller half of a cross-backend ABI test, see `mixed-backend-abi-callee.rs`.

#![allow(improper_ctypes_definitions)]

use std::hint::black_box;

use callee::{fold, mix};

macro_rules! check_abi {
    ($module:ident) => {{
        use callee::$module::*;

        let x = black_box(0x0123_4567_89ab_cdef_0fed_cba9_8765_4321u128);
        let y = black_box(0x1111_2222_3333_4444_5555_6666_7777_8888u128);
        let [a, b, c, d, e, f, g, h] = black_box([1u64, 2, 3, 4, 5, 6, 7, 8]);
        let (i, j) = black_box((9u8, 10u8));
        let w = Wide(x, 11);
        let n = Narrow(12, y);

        assert_eq!(
            five_ints_i128_int(a, b, c, d, e, x, 9),
            mix(&[a, b, c, d, e, fold(x), 9]),
            "{}::five_ints_i128_int",
            stringify!($module)
        );
        assert_eq!(
            seven_ints_i128_int(a, b, c, d, e, f, g, x, 9),
            mix(&[a, b, c, d, e, f, g, fold(x), 9]),
            "{}::seven_ints_i128_int",
            stringify!($module)
        );
        assert_eq!(
            int_i128_int_i128_int(a, x, b, y, c),
            mix(&[a, fold(x), b, fold(y), c]),
            "{}::int_i128_int_i128_int",
            stringify!($module)
        );
        assert_eq!(
            four_ints_wide_int(a, b, c, d, w, 13),
            mix(&[a, b, c, d, fold(x), 11, 13]),
            "{}::four_ints_wide_int",
            stringify!($module)
        );
        assert_eq!(
            five_ints_wide_int(a, b, c, d, e, w, 13),
            mix(&[a, b, c, d, e, fold(x), 11, 13]),
            "{}::five_ints_wide_int",
            stringify!($module)
        );
        assert_eq!(
            four_ints_narrow_int(a, b, c, d, n, 13),
            mix(&[a, b, c, d, 12, fold(y), 13]),
            "{}::four_ints_narrow_int",
            stringify!($module)
        );
        assert_eq!(
            five_ints_narrow_int(a, b, c, d, e, n, 13),
            mix(&[a, b, c, d, e, 12, fold(y), 13]),
            "{}::five_ints_narrow_int",
            stringify!($module)
        );
        assert_eq!(
            stack_alignment(a, b, c, d, e, f, g, h, i, x, j, y),
            mix(&[a, b, c, d, e, f, g, h, i as u64, fold(x), j as u64, fold(y)]),
            "{}::stack_alignment",
            stringify!($module)
        );

        let r = ret_i128(a);
        assert_eq!(r, ((a as u128) << 64) | (a as u128 * 3), "{}::ret_i128", stringify!($module));
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
