//! Caller half of a cross-backend ABI test, see `mixed-backend-abi-callee.rs`.

#![feature(explicit_tail_calls, rust_preserve_none_cc)]
#![allow(improper_ctypes_definitions, incomplete_features)]

use std::hint::black_box;

use callee::{fold, mix};

macro_rules! check {
    ($module:ident, $f:ident($($arg:expr),*), $expected:expr) => {
        assert_eq!($f($($arg),*), $expected, "{}::{}", stringify!($module), stringify!($f))
    };
}

macro_rules! check_many_args {
    ($module:ident) => {{
        use callee::$module::many_args;

        let ints = black_box([1u64; 26]);
        let [a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18, a19, a20, a21, a22, a23, a24, a25] =
            ints.map(|x| x + 1);
        let [f0, f1, f2, f3, f4, f5, f6, f7, f8, f9] = black_box([0.5f64; 10]);
        let expected = mix(&[
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18,
            a19, a20, a21, a22, a23, a24, a25,
        ]) as f64
            + f0 + f1 * 2.0 + f2 * 3.0 + f3 * 4.0 + f4 * 5.0 + f5 * 6.0 + f6 * 7.0 + f7 * 8.0
            + f8 * 9.0 + f9 * 10.0;
        let result = many_args(
            a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15, a16, a17, a18,
            a19, a20, a21, a22, a23, a24, a25, f0, f1, f2, f3, f4, f5, f6, f7, f8, f9,
        );
        assert_eq!(result, expected, "{}::many_args", stringify!($module));
    }};
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

        let a16 = Align16(a, b);
        let wrap = WrapU128(x);
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

        check_many_args!($module);

        check!($module, ret_i128(a), ((a as u128) << 64) | (a as u128 * 3));
        let Wide(r, s) = ret_wide(a);
        assert_eq!((r, s), (ret_i128(a), a * 5), "{}::ret_wide", stringify!($module));
        let Narrow(s, r) = ret_narrow(a);
        assert_eq!((s, r), (a * 7, ret_i128(a)), "{}::ret_narrow", stringify!($module));
    }};
}

fn check_preserve_none() {
    use callee::mix;
    use callee::preserve_none::*;

    check_many_args!(preserve_none);

    // Small integers use other registers than the large ones.
    let (a, b) = black_box((5u64, 9u64));
    let (s, t, u, v) = black_box((250u8, 65000u16, 3_000_000_000u32, true));
    assert_eq!(
        small(s, t, a, u, v),
        mix(&[s as u64, t as u64, a, u as u64, v as u64]),
        "preserve_none::small"
    );
    assert_eq!(
        small_late(a, b, a, b, a, b, a, s, a + b),
        mix(&[a, b, a, b, a, b, a, s as u64, a + b]),
        "preserve_none::small_late"
    );

    let Pair(x, y) = pair(a, Pair(3, 1.5), b);
    assert_eq!((x, y), (mix(&[a, 3, b]), 1.5 + 5.0 + 9.0), "preserve_none::pair");

    let x = Big(black_box([1, 2, 3, 4, 5]));
    let y = Big(black_box([6, 7, 8, 9, 10]));
    let mut want = x.0;
    for (i, w) in want.iter_mut().enumerate() {
        *w = mix(&[a, *w, y.0[4 - i], b]);
    }
    assert_eq!(big(a, x, b, y).0, want, "preserve_none::big");
}

/// The caller's half of the chain of tail calls that bounces between the two crates.
extern "rust-preserve-none" fn ping(
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
    let acc = acc + b + g + (f1 + f8) as u64;
    let next: callee::Link = unsafe { std::mem::transmute(next) };
    become next(
        n - 1,
        acc,
        ping as *const () as usize,
        a,
        b + 1,
        c,
        d,
        e,
        f,
        g + 3,
        h,
        f0,
        f1 + 2.0,
        f2,
        f3,
        f4,
        f5,
        f6,
        f7,
        f8 + 0.25,
        f9,
    )
}

fn check_tail_calls() {
    const N: u64 = 2_000_000;
    let start = black_box(1u64);
    let float = black_box(1.0f64);
    let result = callee::chain(
        N,
        0,
        ping as *const () as usize,
        start,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
        float,
        2.0,
        3.0,
        4.0,
        5.0,
        6.0,
        7.0,
        8.0,
        9.0,
        10.0,
    );

    // The same chain without the calls: `chain` runs on even hops and `ping` on odd ones.
    let (mut acc, mut a, mut b, mut g, mut h) = (0u64, 1u64, 2u64, 7u64, 8u64);
    let (mut f0, mut f1, mut f8, mut f9) = (1.0f64, 2.0f64, 9.0f64, 10.0f64);
    for hop in 0..N {
        if hop % 2 == 0 {
            acc += a + h + (f0 + f9) as u64;
            a += 1;
            h += 2;
            f0 += 1.0;
            f9 += 0.5;
        } else {
            acc += b + g + (f1 + f8) as u64;
            b += 1;
            g += 3;
            f1 += 2.0;
            f8 += 0.25;
        }
    }
    let expected = acc + a + b + 3 + 4 + 5 + 6 + g + h;
    assert_eq!(result, expected, "tail call chain between the backends");
}

fn main() {
    check_abi!(rust);
    check_abi!(c);
    check_preserve_none();
    check_tail_calls();
}
