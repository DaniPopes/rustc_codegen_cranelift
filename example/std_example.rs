#![allow(internal_features)]
#![cfg_attr(target_has_reliable_f128, feature(f128))]
#![feature(cfg_target_has_reliable_f16_f128)]
#![feature(core_intrinsics)]
#![feature(coroutine_trait)]
#![feature(coroutines)]
#![feature(portable_simd)]
#![feature(repr_simd)]
#![feature(tuple_trait)]
#![feature(unboxed_closures)]

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
use std::hint::black_box;
use std::io::Write;
use std::ops::Coroutine;

fn main() {
    println!("{:?}", std::env::args().collect::<Vec<_>>());

    let mutex = std::sync::Mutex::new(());
    let _guard = mutex.lock().unwrap();

    let _ = ::std::iter::repeat('a' as u8).take(10).collect::<Vec<_>>();
    let stderr = ::std::io::stderr();
    let mut stderr = stderr.lock();

    std::thread::spawn(move || {
        println!("Hello from another thread!");
    });

    writeln!(stderr, "some {} text", "<unknown>").unwrap();

    let _ = std::process::Command::new("true").env("c", "d").spawn();

    println!("cargo:rustc-link-lib=z");

    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {});

    let _eq = LoopState::Continue(()) == LoopState::Break(());

    // Make sure ByValPair values with differently sized components are correctly passed
    map(None::<(u8, Box<Instruction>)>);

    println!("{}", 2.3f32.exp());
    println!("{}", 2.3f32.exp2());
    println!("{}", 2.3f32.abs());
    println!("{}", 2.3f32.sqrt());
    println!("{}", 2.3f32.floor());
    println!("{}", 2.3f32.ceil());
    println!("{}", 2.3f32.min(1.0));
    println!("{}", 2.3f32.max(1.0));
    println!("{}", 2.3f32.powi(2));
    println!("{}", 2.3f32.log2());
    assert_eq!(2.3f32.copysign(-1.0), -2.3f32);
    println!("{}", 2.3f32.powf(2.0));

    assert_eq!(i64::MAX.checked_mul(2), None);

    assert_eq!(-128i8, (-128i8).saturating_sub(1));
    assert_eq!(127i8, 127i8.saturating_sub(-128));
    assert_eq!(-128i8, (-128i8).saturating_add(-128));
    assert_eq!(127i8, 127i8.saturating_add(1));

    assert_eq!(0b0000000000000000000000000010000010000000000000000000000000000000_0000000000100000000000000000000000001000000000000100000000000000u128.leading_zeros(), 26);
    assert_eq!(0b0000000000000000000000000010000000000000000000000000000000000000_0000000000000000000000000000000000001000000000000000000010000000u128.trailing_zeros(), 7);
    assert_eq!(
        core::intrinsics::saturating_sub(0, -170141183460469231731687303715884105728i128),
        170141183460469231731687303715884105727i128
    );

    std::hint::black_box(std::hint::black_box(7571400400375753350092698930310845914i128) * 10);
    assert!(0i128.checked_div(2i128).is_some());
    assert!(0u128.checked_div(2u128).is_some());
    assert_eq!(1u128 + 2, 3);

    assert_eq!(0b100010000000000000000000000000000u128 >> 10, 0b10001000000000000000000u128);
    assert_eq!(0xFEDCBA987654321123456789ABCDEFu128 >> 64, 0xFEDCBA98765432u128);
    assert_eq!(0xFEDCBA987654321123456789ABCDEFu128 as i128 >> 64, 0xFEDCBA98765432i128);

    let tmp = 353985398u128;
    assert_eq!(tmp * 932490u128, 330087843781020u128);

    let tmp = -0x1234_5678_9ABC_DEF0i64;
    assert_eq!(tmp as i128, -0x1234_5678_9ABC_DEF0i128);

    // Check that all u/i128 <-> float casts work correctly.
    let houndred_u128 = 100u128;
    let houndred_i128 = 100i128;
    let houndred_f32 = 100.0f32;
    let houndred_f64 = 100.0f64;
    assert_eq!(houndred_u128 as f32, 100.0);
    assert_eq!(houndred_u128 as f64, 100.0);
    assert_eq!(houndred_f32 as u128, 100);
    assert_eq!(houndred_f64 as u128, 100);
    assert_eq!(houndred_i128 as f32, 100.0);
    assert_eq!(houndred_i128 as f64, 100.0);
    assert_eq!(houndred_f32 as i128, 100);
    assert_eq!(houndred_f64 as i128, 100);
    assert_eq!(1u128.rotate_left(2), 4);
    #[cfg(target_has_reliable_f128)]
    assert_eq!(std::hint::black_box(300.0f128) as u8, 255);

    assert_eq!(black_box(f32::NAN) as i128, 0);
    assert_eq!(black_box(f32::NAN) as u128, 0);

    // Test signed 128bit comparing
    let max = usize::MAX as i128;
    if 100i128 < 0i128 || 100i128 > max {
        panic!();
    }

    test_checked_mul();

    test_simd_ops();

    let _a = 1u32 << 2u8;

    let empty: [i32; 0] = [];
    assert!(empty.is_sorted());

    println!("{:?}", std::intrinsics::caller_location());

    #[cfg(target_arch = "x86_64")]
    unsafe {
        test_simd();
    }

    Box::pin(
        #[coroutine]
        move |mut _task_context| {
            yield ();
        },
    )
    .as_mut()
    .resume(0);

    #[derive(Copy, Clone)]
    enum Nums {
        NegOne = -1,
    }

    let kind = Nums::NegOne;
    assert_eq!(-1i128, kind as i128);

    let options = [1u128];
    match options[0] {
        1 => (),
        0 => loop {},
        v => panic(v),
    };

    if black_box(false) {
        // Based on https://github.com/rust-lang/rust/blob/2f320a224e827b400be25966755a621779f797cc/src/test/ui/debuginfo/debuginfo_with_uninhabitable_field_and_unsized.rs
        let _ = Foo::<dyn Send>::new();

        #[allow(dead_code)]
        struct Foo<T: ?Sized> {
            base: Never,
            value: T,
        }

        impl<T: ?Sized> Foo<T> {
            pub fn new() -> Box<Foo<T>> {
                todo!()
            }
        }

        enum Never {}
    }

    #[cfg(not(target_arch = "s390x"))] // s390x doesn't have vector instructions enabled by default
    foo(I64X2([0, 0]));

    transmute_wide_pointer();

    rust_call_abi();

    // #[cfg(target_arch = "x86_64")]
    // inline_asm_call_custom_abi();

    const fn no_str() -> Option<Box<str>> {
        None
    }

    static STATIC_WITH_MAYBE_NESTED_BOX: &Option<Box<str>> = &no_str();

    println!("{:?}", STATIC_WITH_MAYBE_NESTED_BOX);
}

fn panic(_: u128) {
    panic!();
}

use std::mem::transmute;

#[cfg(target_pointer_width = "32")]
type TwoPtrs = i64;
#[cfg(target_pointer_width = "64")]
type TwoPtrs = i128;

fn transmute_wide_pointer() -> TwoPtrs {
    unsafe { transmute::<_, TwoPtrs>("true !") }
}

extern "rust-call" fn rust_call_abi_callee<T: std::marker::Tuple>(_: T) {}

fn rust_call_abi() {
    rust_call_abi_callee(());
    rust_call_abi_callee((1, 2));
}

#[cfg_attr(target_arch = "s390x", allow(dead_code))]
#[repr(simd)]
struct I64X2([i64; 2]);

#[cfg_attr(target_arch = "s390x", allow(dead_code))]
#[allow(improper_ctypes_definitions)]
extern "C" fn foo(_a: I64X2) {}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
#[cfg(not(jit))]
unsafe fn test_crc32() {
    assert!(is_x86_feature_detected!("sse4.2"));

    let a = 42u32;
    let b = 0xdeadbeefu64;

    assert_eq!(_mm_crc32_u8(a, b as u8), 4135334616);
    assert_eq!(_mm_crc32_u16(a, b as u16), 1200687288);
    assert_eq!(_mm_crc32_u32(a, b as u32), 2543798776);
    assert_eq!(_mm_crc32_u64(a as u64, b as u64), 241952147);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_simd() {
    unsafe {
        assert!(is_x86_feature_detected!("sse2"));

        let x = _mm_setzero_si128();
        let y = _mm_set1_epi16(7);
        let or = _mm_or_si128(x, y);
        let cmp_eq = _mm_cmpeq_epi8(y, y);
        let cmp_lt = _mm_cmplt_epi8(y, y);

        let (zero0, zero1) = std::mem::transmute::<_, (u64, u64)>(x);
        assert_eq!((zero0, zero1), (0, 0));
        assert_eq!(std::mem::transmute::<_, [u16; 8]>(or), [7, 7, 7, 7, 7, 7, 7, 7]);
        assert_eq!(
            std::mem::transmute::<_, [u16; 8]>(cmp_eq),
            [0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff]
        );
        assert_eq!(std::mem::transmute::<_, [u16; 8]>(cmp_lt), [0, 0, 0, 0, 0, 0, 0, 0]);

        test_mm_slli_si128();
        test_mm_movemask_epi8();
        test_mm256_movemask_epi8();
        test_mm_add_epi8();
        test_mm_add_pd();
        test_mm_cvtepi8_epi16();
        #[cfg(not(jit))]
        test_mm_cvtps_epi32();
        test_mm_cvttps_epi32();
        test_mm_cvtsi128_si64();

        #[cfg(not(jit))]
        test_mm_cvtps_ph();

        test_mm_extract_epi8();
        test_mm_insert_epi16();
        test_mm_shuffle_epi8();

        #[cfg(not(jit))]
        test_mm_cmpestri();

        test_mm256_shuffle_epi8();
        test_mm256_permute2x128_si256();
        test_mm256_permutevar8x32_epi32();

        #[rustfmt::skip]
    let mask1 = _mm_movemask_epi8(dbg!(_mm_setr_epi8(255u8 as i8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)));
        assert_eq!(mask1, 1);

        #[cfg(not(jit))]
        test_crc32();

        #[cfg(not(jit))]
        test_xmm_roundtrip();
        #[cfg(not(jit))]
        if is_x86_feature_detected!("avx") {
            test_ymm_roundtrip();
        }
        #[cfg(not(jit))]
        if is_x86_feature_detected!("avx512f") {
            test_zmm_roundtrip();
        }

        #[cfg(not(jit))]
        {
            if is_x86_feature_detected!("pclmulqdq") {
                test_mm_clmulepi64_si128();
            }
            if is_x86_feature_detected!("vpclmulqdq") {
                test_mm256_clmulepi64_epi128();
            }
            if is_x86_feature_detected!("vpclmulqdq") && is_x86_feature_detected!("avx512f") {
                test_mm512_clmulepi64_epi128();
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_mm_slli_si128() {
    #[rustfmt::skip]
    let a = _mm_setr_epi8(
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    );
    let r = _mm_slli_si128(a, 1);
    let e = _mm_setr_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
    assert_eq_m128i(r, e);

    #[rustfmt::skip]
    let a = _mm_setr_epi8(
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    );
    let r = _mm_slli_si128(a, 15);
    let e = _mm_setr_epi8(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1);
    assert_eq_m128i(r, e);

    #[rustfmt::skip]
    let a = _mm_setr_epi8(
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    );
    let r = _mm_slli_si128(a, 16);
    assert_eq_m128i(r, _mm_set1_epi8(0));
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_mm_movemask_epi8() {
    #[rustfmt::skip]
    let a = _mm_setr_epi8(
        0b1000_0000u8 as i8, 0b0, 0b1000_0000u8 as i8, 0b01,
        0b0101, 0b1111_0000u8 as i8, 0, 0,
        0, 0, 0b1111_0000u8 as i8, 0b0101,
        0b01, 0b1000_0000u8 as i8, 0b0, 0b1000_0000u8 as i8,
    );
    let r = _mm_movemask_epi8(a);
    assert_eq!(r, 0b10100100_00100101);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn test_mm256_movemask_epi8() {
    let a = _mm256_set1_epi8(-1);
    let r = _mm256_movemask_epi8(a);
    let e = -1;
    assert_eq!(r, e);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_mm_add_epi8() {
    let a = _mm_setr_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
    #[rustfmt::skip]
    let b = _mm_setr_epi8(
        16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
    );
    let r = _mm_add_epi8(a, b);
    #[rustfmt::skip]
    let e = _mm_setr_epi8(
        16, 18, 20, 22, 24, 26, 28, 30, 32, 34, 36, 38, 40, 42, 44, 46,
    );
    assert_eq_m128i(r, e);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_mm_add_pd() {
    let a = _mm_setr_pd(1.0, 2.0);
    let b = _mm_setr_pd(5.0, 10.0);
    let r = _mm_add_pd(a, b);
    assert_eq_m128d(r, _mm_setr_pd(6.0, 12.0));
}

#[cfg(target_arch = "x86_64")]
fn assert_eq_m128i(x: std::arch::x86_64::__m128i, y: std::arch::x86_64::__m128i) {
    unsafe {
        assert_eq!(std::mem::transmute::<_, [u8; 16]>(x), std::mem::transmute::<_, [u8; 16]>(y));
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
pub fn assert_eq_m128d(a: __m128d, b: __m128d) {
    if _mm_movemask_pd(_mm_cmpeq_pd(a, b)) != 0b11 {
        panic!("{:?} != {:?}", a, b);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
pub fn assert_eq_m256i(a: __m256i, b: __m256i) {
    unsafe {
        assert_eq!(std::mem::transmute::<_, [u64; 4]>(a), std::mem::transmute::<_, [u64; 4]>(b))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
pub fn assert_eq_m512i(a: __m512i, b: __m512i) {
    unsafe {
        assert_eq!(std::mem::transmute::<_, [u64; 8]>(a), std::mem::transmute::<_, [u64; 8]>(b))
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_mm_cvtsi128_si64() {
    unsafe {
        let r = _mm_cvtsi128_si64(std::mem::transmute::<[i64; 2], _>([5, 0]));
        assert_eq!(r, 5);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn test_mm_cvtepi8_epi16() {
    let a = _mm_set1_epi8(10);
    let r = _mm_cvtepi8_epi16(a);
    let e = _mm_set1_epi16(10);
    assert_eq_m128i(r, e);
    let a = _mm_set1_epi8(-10);
    let r = _mm_cvtepi8_epi16(a);
    let e = _mm_set1_epi16(-10);
    assert_eq_m128i(r, e);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn test_mm_extract_epi8() {
    #[rustfmt::skip]
    let a = _mm_setr_epi8(
        -1, 1, 2, 3, 4, 5, 6, 7,
        8, 9, 10, 11, 12, 13, 14, 15
    );
    let r1 = _mm_extract_epi8(a, 0);
    let r2 = _mm_extract_epi8(a, 3);
    assert_eq!(r1, 0xFF);
    assert_eq!(r2, 3);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_mm_insert_epi16() {
    let a = _mm_setr_epi16(0, 1, 2, 3, 4, 5, 6, 7);
    let r = _mm_insert_epi16(a, 9, 0);
    let e = _mm_setr_epi16(9, 1, 2, 3, 4, 5, 6, 7);
    assert_eq_m128i(r, e);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn test_mm_shuffle_epi8() {
    #[rustfmt::skip]
        let a = _mm_setr_epi8(
            1, 2, 3, 4, 5, 6, 7, 8,
            9, 10, 11, 12, 13, 14, 15, 16,
        );
    #[rustfmt::skip]
        let b = _mm_setr_epi8(
            4, 128_u8 as i8, 4, 3,
            24, 12, 6, 19,
            12, 5, 5, 10,
            4, 1, 8, 0,
        );
    let expected = _mm_setr_epi8(5, 0, 5, 4, 9, 13, 7, 4, 13, 6, 6, 11, 5, 2, 9, 1);
    let r = _mm_shuffle_epi8(a, b);
    assert_eq_m128i(r, expected);
}

// Currently one cannot `load` a &[u8] that is less than 16
// in length. This makes loading strings less than 16 in length
// a bit difficult. Rather than `load` and mutate the __m128i,
// it is easier to memcpy the given string to a local slice with
// length 16 and `load` the local slice.
#[cfg(not(jit))]
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
unsafe fn str_to_m128i(s: &[u8]) -> __m128i {
    unsafe {
        assert!(s.len() <= 16);
        let slice = &mut [0u8; 16];
        std::ptr::copy_nonoverlapping(s.as_ptr(), slice.as_mut_ptr(), s.len());
        _mm_loadu_si128(slice.as_ptr() as *const _)
    }
}

#[cfg(not(jit))]
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
unsafe fn test_mm_cmpestri() {
    unsafe {
        let a = str_to_m128i(b"bar - garbage");
        let b = str_to_m128i(b"foobar");
        let i = _mm_cmpestri::<_SIDD_CMP_EQUAL_ORDERED>(a, 3, b, 6);
        assert_eq!(3, i);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn test_mm256_shuffle_epi8() {
    #[rustfmt::skip]
    let a = _mm256_setr_epi8(
        1, 2, 3, 4, 5, 6, 7, 8,
        9, 10, 11, 12, 13, 14, 15, 16,
        17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32,
    );
    #[rustfmt::skip]
    let b = _mm256_setr_epi8(
        4, 128u8 as i8, 4, 3, 24, 12, 6, 19,
        12, 5, 5, 10, 4, 1, 8, 0,
        4, 128u8 as i8, 4, 3, 24, 12, 6, 19,
        12, 5, 5, 10, 4, 1, 8, 0,
    );
    #[rustfmt::skip]
    let expected = _mm256_setr_epi8(
        5, 0, 5, 4, 9, 13, 7, 4,
        13, 6, 6, 11, 5, 2, 9, 1,
        21, 0, 21, 20, 25, 29, 23, 20,
        29, 22, 22, 27, 21, 18, 25, 17,
    );
    let r = _mm256_shuffle_epi8(a, b);
    assert_eq_m256i(r, expected);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn test_mm256_permute2x128_si256() {
    let a = _mm256_setr_epi64x(100, 200, 500, 600);
    let b = _mm256_setr_epi64x(300, 400, 700, 800);
    let r = _mm256_permute2x128_si256::<0b00_01_00_11>(a, b);
    let e = _mm256_setr_epi64x(700, 800, 500, 600);
    assert_eq_m256i(r, e);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn test_mm256_permutevar8x32_epi32() {
    let a = _mm256_setr_epi32(100, 200, 300, 400, 500, 600, 700, 800);
    let idx = _mm256_setr_epi32(7, 6, 5, 4, 3, 2, 1, 0);
    let r = _mm256_setr_epi32(800, 700, 600, 500, 400, 300, 200, 100);
    let e = _mm256_permutevar8x32_epi32(a, idx);
    assert_eq_m256i(r, e);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "pclmulqdq")]
#[cfg(not(jit))]
unsafe fn test_mm_clmulepi64_si128() {
    // Constants taken from https://software.intel.com/sites/default/files/managed/72/cc/clmul-wp-rev-2.02-2014-04-20.pdf
    let a = _mm_set_epi64x(0x7b5b546573745665, 0x63746f725d53475d);
    let b = _mm_set_epi64x(0x4869285368617929, 0x5b477565726f6e5d);
    let r00 = _mm_set_epi64x(0x1d4d84c85c3440c0, 0x929633d5d36f0451u64.cast_signed());
    let r01 = _mm_set_epi64x(0x1bd17c8d556ab5a1, 0x7fa540ac2a281315);
    let r10 = _mm_set_epi64x(0x1a2bf6db3a30862f, 0xbabf262df4b7d5c9u64.cast_signed());
    let r11 = _mm_set_epi64x(0x1d1e1f2c592e7c45, 0xd66ee03e410fd4edu64.cast_signed());

    assert_eq_m128i(_mm_clmulepi64_si128::<0x00>(a, b), r00);
    assert_eq_m128i(_mm_clmulepi64_si128::<0x10>(a, b), r01);
    assert_eq_m128i(_mm_clmulepi64_si128::<0x01>(a, b), r10);
    assert_eq_m128i(_mm_clmulepi64_si128::<0x11>(a, b), r11);

    let a0 = _mm_set_epi64x(0x0000000000000000, 0x8000000000000000u64.cast_signed());
    let r = _mm_set_epi64x(0x4000000000000000, 0x0000000000000000);
    assert_eq_m128i(_mm_clmulepi64_si128::<0x00>(a0, a0), r);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "vpclmulqdq")]
#[cfg(not(jit))]
unsafe fn test_mm256_clmulepi64_epi128() {
    let a = _mm256_setr_epi64x(1, 2, 3, 4);
    let b = _mm256_setr_epi64x(5, 6, 7, 8);

    assert_eq_m256i(_mm256_clmulepi64_epi128::<0x00>(a, b), _mm256_setr_epi64x(5, 0, 9, 0));
    assert_eq_m256i(_mm256_clmulepi64_epi128::<0x01>(a, b), _mm256_setr_epi64x(10, 0, 28, 0));
    assert_eq_m256i(_mm256_clmulepi64_epi128::<0x10>(a, b), _mm256_setr_epi64x(6, 0, 24, 0));
    assert_eq_m256i(_mm256_clmulepi64_epi128::<0x11>(a, b), _mm256_setr_epi64x(12, 0, 32, 0));
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "vpclmulqdq,avx512f")]
#[cfg(not(jit))]
unsafe fn test_mm512_clmulepi64_epi128() {
    let a = _mm512_setr_epi64(1, 2, 3, 4, 5, 6, 7, 8);
    let b = _mm512_setr_epi64(9, 10, 11, 12, 13, 14, 15, 16);

    assert_eq_m512i(
        _mm512_clmulepi64_epi128::<0x00>(a, b),
        _mm512_setr_epi64(9, 0, 29, 0, 57, 0, 45, 0),
    );
    assert_eq_m512i(
        _mm512_clmulepi64_epi128::<0x10>(a, b),
        _mm512_setr_epi64(10, 0, 20, 0, 54, 0, 112, 0),
    );
    assert_eq_m512i(
        _mm512_clmulepi64_epi128::<0x11>(a, b),
        _mm512_setr_epi64(20, 0, 48, 0, 36, 0, 128, 0),
    );
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[cfg(not(jit))]
unsafe fn test_mm_cvtps_epi32() {
    unsafe {
        let floats: [f32; 4] = [1.5, -2.5, i32::MAX as f32 + 1.0, f32::NAN];

        let float_vec = _mm_loadu_ps(floats.as_ptr());
        let int_vec = _mm_cvtps_epi32(float_vec);

        let mut ints: [i32; 4] = [0; 4];
        _mm_storeu_si128(ints.as_mut_ptr() as *mut __m128i, int_vec);

        // this is very different from `floats.map(|f| f as i32)`!
        let expected_ints: [i32; 4] = [2, -2, i32::MIN, i32::MIN];

        assert_eq!(ints, expected_ints);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn test_mm_cvttps_epi32() {
    unsafe {
        let floats: [f32; 4] = [1.5, -2.5, i32::MAX as f32 + 1.0, f32::NAN];

        let float_vec = _mm_loadu_ps(floats.as_ptr());
        let int_vec = _mm_cvttps_epi32(float_vec);

        let mut ints: [i32; 4] = [0; 4];
        _mm_storeu_si128(ints.as_mut_ptr() as *mut __m128i, int_vec);

        // this is very different from `floats.map(|f| f as i32)`!
        let expected_ints: [i32; 4] = [1, -2, i32::MIN, i32::MIN];

        assert_eq!(ints, expected_ints);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "f16c")]
#[cfg(not(jit))]
unsafe fn test_mm_cvtps_ph() {
    const F16_ONE: i16 = 0x3c00;
    const F16_TWO: i16 = 0x4000;
    const F16_THREE: i16 = 0x4200;
    const F16_FOUR: i16 = 0x4400;

    let a = _mm_set_ps(1.0, 2.0, 3.0, 4.0);
    let r = _mm_cvtps_ph::<_MM_FROUND_CUR_DIRECTION>(a);
    let e = _mm_set_epi16(0, 0, 0, 0, F16_ONE, F16_TWO, F16_THREE, F16_FOUR);
    assert_eq_m128i(r, e);
}

#[cfg(target_arch = "x86_64")]
#[cfg(not(jit))]
unsafe fn test_xmm_roundtrip() {
    unsafe {
        let input = [1u8; 16];
        let mut output = [0u8; 16];

        std::arch::asm!(
            "movups {xmm}, [{input}]",
            "movups [{output}], {xmm}",
            input = in(reg) input.as_ptr(),
            output = in(reg) output.as_mut_ptr(),
            xmm = out(xmm_reg) _,
        );

        assert_eq!(input, output);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
#[cfg(not(jit))]
unsafe fn test_ymm_roundtrip() {
    unsafe {
        let input = [1u8; 32];
        let mut output = [0u8; 32];

        std::arch::asm!(
            "vmovups {ymm}, [{input}]",
            "vmovups [{output}], {ymm}",
            input = in(reg) input.as_ptr(),
            output = in(reg) output.as_mut_ptr(),
            ymm = out(ymm_reg) _,
        );

        assert_eq!(input, output);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
#[cfg(not(jit))]
unsafe fn test_zmm_roundtrip() {
    unsafe {
        let input = [1u8; 64];
        let mut output = [0u8; 64];

        std::arch::asm!(
            "vmovups {zmm}, [{input}]",
            "vmovups [{output}], {zmm}",
            input = in(reg) input.as_ptr(),
            output = in(reg) output.as_mut_ptr(),
            zmm = out(zmm_reg) _,
        );

        assert_eq!(input, output);
    }
}

fn test_checked_mul() {
    let u: Option<u8> = u8::from_str_radix("1000", 10).ok();
    assert_eq!(u, None);

    assert_eq!(1u8.checked_mul(255u8), Some(255u8));
    assert_eq!(255u8.checked_mul(255u8), None);
    assert_eq!(1i8.checked_mul(127i8), Some(127i8));
    assert_eq!(127i8.checked_mul(127i8), None);
    assert_eq!((-1i8).checked_mul(-127i8), Some(127i8));
    assert_eq!(1i8.checked_mul(-128i8), Some(-128i8));
    assert_eq!((-128i8).checked_mul(-128i8), None);

    assert_eq!(1u64.checked_mul(u64::MAX), Some(u64::MAX));
    assert_eq!(u64::MAX.checked_mul(u64::MAX), None);
    assert_eq!(1i64.checked_mul(i64::MAX), Some(i64::MAX));
    assert_eq!(i64::MAX.checked_mul(i64::MAX), None);
    assert_eq!((-1i64).checked_mul(i64::MIN + 1), Some(i64::MAX));
    assert_eq!(1i64.checked_mul(i64::MIN), Some(i64::MIN));
    assert_eq!(i64::MIN.checked_mul(i64::MIN), None);
}

#[derive(PartialEq)]
enum LoopState {
    Continue(()),
    Break(()),
}

pub enum Instruction {
    Increment,
    Loop,
}

fn map(a: Option<(u8, Box<Instruction>)>) -> Option<Box<Instruction>> {
    match a {
        None => None,
        Some((_, instr)) => Some(instr),
    }
}

// FIXME enable once inline asm sym references are stabilized in cg_clif
// #[cfg(target_arch = "x86_64")]
// fn inline_asm_call_custom_abi() {
//     use std::arch::{asm, naked_asm};
//
//     #[unsafe(naked)]
//     unsafe extern "custom" fn double() {
//         naked_asm!("add rax, rax", "ret");
//     }
//
//     let mut x: u64 = 21;
//     unsafe { asm!("call {}", sym double, inout("rax") x) };
//     assert_eq!(x, 42);
// }

/// Checks the SIMD operations that are lowered to whole-vector Cranelift instructions against the
/// results of LLVM.
fn test_simd_ops() {
    use std::simd::prelude::*;

    macro_rules! t {
        ($e:expr, $expected:expr) => {
            assert_eq!(format!("{:?}", $e), $expected, "{}", stringify!($e));
        };
    }

    fn bb<T>(x: T) -> T {
        black_box(x)
    }

    let u8a =
        bb(u8x16::from_array([0, 1, 2, 3, 127, 128, 129, 200, 250, 255, 7, 8, 9, 10, 11, 12]));
    let u8b = bb(u8x16::from_array([255, 1, 5, 3, 1, 128, 127, 100, 10, 1, 7, 9, 8, 11, 10, 12]));
    let i8a =
        bb(i8x16::from_array([0, 1, -2, 3, 127, -128, -127, 100, -100, -1, 7, 8, -9, 10, -11, 12]));
    let i8b =
        bb(i8x16::from_array([-1, 1, 5, -3, 1, -128, 127, -100, 10, 1, -7, 9, 8, -11, 10, 12]));
    let u16a = bb(u16x8::from_array([0, 1, 0x7fff, 0x8000, 0xffff, 1000, 65000, 3]));
    let u16b = bb(u16x8::from_array([0xffff, 1, 1, 0x8000, 2, 64000, 1000, 4]));
    let i16a = bb(i16x8::from_array([0, 1, 0x7fff, -0x8000, -1, 1000, -30000, 3]));
    let i16b = bb(i16x8::from_array([-1, 1, 1, -0x8000, 2, -32000, 10000, -4]));
    let u32a = bb(u32x4::from_array([0, 0x8000_0000, 0xffff_ffff, 123456789]));
    let u32b = bb(u32x4::from_array([1, 0x8000_0000, 2, 987654321]));
    let i32a = bb(i32x4::from_array([i32::MIN, i32::MAX, -1, 123456789]));
    let i32b = bb(i32x4::from_array([-1, 2, i32::MIN, -987654321]));
    let u64a = bb(u64x2::from_array([0xffff_ffff_ffff_ffff, 0x1234_5678_9abc_def0]));
    let u64b = bb(u64x2::from_array([3, 0xfedc_ba98_7654_3210]));
    let i64a = bb(i64x2::from_array([i64::MIN, -5]));
    let i64b = bb(i64x2::from_array([-1, 0x1234_5678_9abc_def0]));
    let f32a = bb(f32x4::from_array([1.5, -0.0, f32::INFINITY, 3.0]));
    let f32b = bb(f32x4::from_array([-2.25, 0.0, f32::INFINITY, f32::NAN]));
    let f32c = bb(f32x4::from_array([-0.0, -0.0, 7.0, 0.5]));
    let f64a = bb(f64x2::from_array([-0.0, 1.0e300]));
    let f64b = bb(f64x2::from_array([0.0, 1.0e300]));
    let f64c = bb(f64x2::from_array([f64::NAN, -2.5]));

    // The bits of a NaN that is not an input are architecture specific.
    macro_rules! nb {
        ($e:expr) => {{
            let v = $e;
            v.is_nan().select(Simd::splat(!0), v.to_bits())
        }};
    }

    // Integer arithmetic wraps and does not depend on the lane order.
    t!((u8a + u8b), "[255, 2, 7, 6, 128, 0, 0, 44, 4, 0, 14, 17, 17, 21, 21, 24]");
    t!((u8a - u8b), "[1, 0, 253, 0, 126, 0, 2, 100, 240, 254, 0, 255, 1, 255, 1, 0]");
    t!((u8a * u8b), "[0, 1, 10, 9, 127, 0, 255, 32, 196, 255, 49, 72, 72, 110, 110, 144]");
    t!((i8a + i8b), "[-1, 2, 3, 0, -128, 0, 0, 0, -90, 0, 0, 17, -1, -1, -1, 24]");
    t!((i8a - i8b), "[1, 0, -7, 6, 126, 0, 2, -56, -110, -2, 14, -1, -17, 21, -21, 0]");
    t!((i8a * i8b), "[0, 1, -10, -9, 127, 0, -1, -16, 24, -1, -49, 72, -72, -110, -110, -112]");
    t!((u16a + u16b), "[65535, 2, 32768, 0, 1, 65000, 464, 7]");
    t!((u16a - u16b), "[1, 0, 32766, 0, 65533, 2536, 64000, 65535]");
    t!((u16a * u16b), "[0, 1, 32767, 0, 65534, 36864, 53824, 12]");
    t!((i16a + i16b), "[-1, 2, -32768, 0, 1, -31000, -20000, -1]");
    t!((i16a * i16b), "[0, 1, 32767, 0, -2, -18432, 23808, -12]");
    t!((u32a + u32b), "[1, 0, 1, 1111111110]");
    t!((u32a - u32b), "[4294967295, 0, 4294967293, 3430769764]");
    t!((u32a * u32b), "[0, 0, 4294967294, 4227814277]");
    t!((i32a + i32b), "[2147483647, -2147483647, 2147483647, -864197532]");
    t!((i32a - i32b), "[-2147483647, 2147483645, 2147483647, 1111111110]");
    t!((i32a * i32b), "[-2147483648, -2, -2147483648, 67153019]");
    t!((u64a + u64b), "[2, 1229782938247303424]");
    t!((u64a - u64b), "[18446744073709551612, 1393753996680277216]");
    t!((u64a * u64b), "[18446744073709551613, 2552847189736476416]");
    t!((i64a + i64b), "[9223372036854775807, 1311768467463790315]");
    t!((i64a * i64b), "[-9223372036854775808, -6558842337318951600]");
    t!((u8a & u8b), "[0, 1, 0, 3, 1, 128, 1, 64, 10, 1, 7, 8, 8, 10, 10, 12]");
    t!((u8a | u8b), "[255, 1, 7, 3, 127, 128, 255, 236, 250, 255, 7, 9, 9, 11, 11, 12]");
    t!((u8a ^ u8b), "[255, 0, 7, 0, 126, 0, 254, 172, 240, 254, 0, 1, 1, 1, 1, 0]");
    t!((i32a & i32b), "[-2147483648, 2, -2147483648, 83985669]");
    t!((u64a | u64b), "[18446744073709551615, 18373840925148118768]");
    t!((i64a ^ i64b), "[9223372036854775807, -1311768467463790325]");
    t!((-i8a), "[0, -1, 2, -3, -127, -128, 127, -100, 100, 1, -7, -8, 9, -10, 11, -12]");
    t!((-i16a), "[0, -1, -32767, -32768, 1, -1000, 30000, -3]");
    t!((-i32a), "[-2147483648, -2147483647, 1, -123456789]");
    t!((-i64a), "[-9223372036854775808, 5]");

    // Signed zeros, infinities and NaNs.
    t!((f32a + f32b).to_bits(), "[3208642560, 0, 2139095040, 2143289344]");
    t!(nb!(f32a - f32b), "[1081081856, 2147483648, 4294967295, 4294967295]");
    t!((f32a * f32b).to_bits(), "[3226992640, 2147483648, 2139095040, 2143289344]");
    t!(nb!(f32a / f32c), "[4286578688, 4294967295, 2139095040, 1086324736]");
    t!((f32a + f32c).to_bits(), "[1069547520, 2147483648, 2139095040, 1080033280]");
    t!((f32c - f32b).to_bits(), "[1074790400, 2147483648, 4286578688, 2143289344]");
    t!((f32a + f32b).is_nan().to_bitmask(), "8");
    t!((f32a - f32b).is_nan().to_bitmask(), "12");
    t!((f32a * f32c).is_nan().to_bitmask(), "0");
    t!((f32a / f32b).is_nan().to_bitmask(), "14");
    t!((-f32a).to_bits(), "[3217031168, 0, 4286578688, 3225419776]");
    t!((-f32b).is_nan().to_bitmask(), "8");
    t!((f64a + f64b).to_bits(), "[0, 9099492520756278684]");
    t!((f64a - f64b).to_bits(), "[9223372036854775808, 0]");
    t!((f64a * f64b).to_bits(), "[9223372036854775808, 9218868437227405312]");
    t!(nb!(f64b / f64a), "[18446744073709551615, 4607182418800017408]");
    t!((f64a + f64c).is_nan().to_bitmask(), "1");
    t!((-f64a).to_bits(), "[0, 18318360957983683996]");
    t!((-f64b).to_bits(), "[9223372036854775808, 18318360957983683996]");

    // Comparisons give all ones or all zeros lanes, and signed and unsigned compare differently.
    t!(u8a.simd_eq(u8b).to_simd(), "[0, -1, 0, -1, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0, 0, -1]");
    t!(u8a.simd_ne(u8b).to_simd(), "[-1, 0, -1, 0, -1, 0, -1, -1, -1, -1, 0, -1, -1, -1, -1, 0]");
    t!(u8a.simd_lt(u8b).to_simd(), "[-1, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0, -1, 0, -1, 0, 0]");
    t!(u8a.simd_le(u8b).to_simd(), "[-1, -1, -1, -1, 0, -1, 0, 0, 0, 0, -1, -1, 0, -1, 0, -1]");
    t!(u8a.simd_gt(u8b).to_simd(), "[0, 0, 0, 0, -1, 0, -1, -1, -1, -1, 0, 0, -1, 0, -1, 0]");
    t!(u8a.simd_ge(u8b).to_simd(), "[0, -1, 0, -1, -1, -1, -1, -1, -1, -1, -1, 0, -1, 0, -1, -1]");
    t!(i8a.simd_lt(i8b).to_simd(), "[0, 0, -1, 0, 0, 0, -1, 0, -1, -1, 0, -1, -1, 0, -1, 0]");
    t!(i8a.simd_le(i8b).to_simd(), "[0, -1, -1, 0, 0, -1, -1, 0, -1, -1, 0, -1, -1, 0, -1, -1]");
    t!(i8a.simd_gt(i8b).to_simd(), "[-1, 0, 0, -1, -1, 0, 0, -1, 0, 0, -1, 0, 0, -1, 0, 0]");
    t!(i8a.simd_ge(i8b).to_simd(), "[-1, -1, 0, -1, -1, -1, 0, -1, 0, 0, -1, 0, 0, -1, 0, -1]");
    t!(u16a.simd_lt(u16b).to_simd(), "[-1, 0, 0, 0, 0, -1, 0, -1]");
    t!(i16a.simd_lt(i16b).to_simd(), "[0, 0, 0, 0, -1, 0, -1, 0]");
    t!(i16a.simd_ge(i16b).to_simd(), "[-1, -1, -1, -1, 0, -1, 0, -1]");
    t!(u32a.simd_lt(u32b).to_simd(), "[-1, 0, 0, -1]");
    t!(u32a.simd_ge(u32b).to_simd(), "[0, -1, -1, 0]");
    t!(i32a.simd_lt(i32b).to_simd(), "[-1, 0, 0, 0]");
    t!(i32a.simd_eq(i32b).to_simd(), "[0, 0, 0, 0]");
    t!(u64a.simd_gt(u64b).to_simd(), "[-1, 0]");
    t!(i64a.simd_gt(i64b).to_simd(), "[0, 0]");
    t!(i64a.simd_le(i64b).to_simd(), "[-1, -1]");
    t!(f32a.simd_eq(f32b).to_simd(), "[0, -1, -1, 0]");
    t!(f32a.simd_ne(f32b).to_simd(), "[-1, 0, 0, -1]");
    t!(f32a.simd_lt(f32b).to_simd(), "[0, 0, 0, 0]");
    t!(f32a.simd_le(f32b).to_simd(), "[0, -1, -1, 0]");
    t!(f32a.simd_gt(f32b).to_simd(), "[-1, 0, 0, 0]");
    t!(f32a.simd_ge(f32b).to_simd(), "[-1, -1, -1, 0]");
    t!(f32a.simd_eq(f32c).to_simd(), "[0, -1, 0, 0]");
    t!(f32a.simd_ne(f32c).to_simd(), "[-1, 0, -1, -1]");
    t!(f64a.simd_eq(f64b).to_simd(), "[-1, -1]");
    t!(f64a.simd_ne(f64c).to_simd(), "[-1, -1]");
    t!(f64b.simd_lt(f64c).to_simd(), "[0, 0]");
    t!(f64b.simd_ge(f64c).to_simd(), "[0, -1]");

    // Selecting with a comparison mask.
    t!(
        u8a.simd_lt(u8b).select(u8a, u8b),
        "[0, 1, 2, 3, 1, 128, 127, 100, 10, 1, 7, 8, 8, 10, 10, 12]"
    );
    t!(
        i8a.simd_lt(i8b).select(i8a, i8b),
        "[-1, 1, -2, -3, 1, -128, -127, -100, -100, -1, -7, 8, -9, -11, -11, 12]"
    );
    t!(i16a.simd_gt(i16b).select(i16a, i16b), "[0, 1, 32767, -32768, 2, 1000, 10000, 3]");
    t!(u32a.simd_ge(u32b).select(u32a, u32b), "[1, 2147483648, 4294967295, 987654321]");
    t!(i64a.simd_gt(i64b).select(i64a, i64b), "[-1, 1311768467463790320]");
    t!(
        f32a.simd_lt(f32b).select(f32a, f32c).to_bits(),
        "[2147483648, 2147483648, 1088421888, 1056964608]"
    );
    t!(f64a.simd_ne(f64b).select(f64a, f64c).is_nan().to_bitmask(), "1");
    t!(
        f64b.simd_ge(f64c).select(f64b, f64a).to_bits(),
        "[9223372036854775808, 9094988921128908188]"
    );

    // Saturating arithmetic.
    t!(
        u8a.saturating_add(u8b),
        "[255, 2, 7, 6, 128, 255, 255, 255, 255, 255, 14, 17, 17, 21, 21, 24]"
    );
    t!(u8a.saturating_sub(u8b), "[0, 0, 0, 0, 126, 0, 2, 100, 240, 254, 0, 0, 1, 0, 1, 0]");
    t!(i8a.saturating_add(i8b), "[-1, 2, 3, 0, 127, -128, 0, 0, -90, 0, 0, 17, -1, -1, -1, 24]");
    t!(
        i8a.saturating_sub(i8b),
        "[1, 0, -7, 6, 126, 0, -128, 127, -110, -2, 14, -1, -17, 21, -21, 0]"
    );
    t!(u16a.saturating_add(u16b), "[65535, 2, 32768, 65535, 65535, 65000, 65535, 7]");
    t!(u16a.saturating_sub(u16b), "[0, 0, 32766, 0, 65533, 0, 64000, 0]");
    t!(i16a.saturating_add(i16b), "[-1, 2, 32767, -32768, 1, -31000, -20000, -1]");
    t!(i16a.saturating_sub(i16b), "[1, 0, 32766, 0, -3, 32767, -32768, 7]");
    t!(u32a.saturating_add(u32b), "[1, 4294967295, 4294967295, 1111111110]");
    t!(i32a.saturating_sub(i32b), "[-2147483647, 2147483645, 2147483647, 1111111110]");
    t!(i64a.saturating_add(i64b), "[-9223372036854775808, 1311768467463790315]");

    // Shuffles.
    t!(
        simd_swizzle!(u8a, [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0]),
        "[12, 11, 10, 9, 8, 7, 255, 250, 200, 129, 128, 127, 3, 2, 1, 0]"
    );
    t!(
        simd_swizzle!(u8a, u8b, [0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23]),
        "[0, 255, 1, 1, 2, 5, 3, 3, 127, 1, 128, 128, 129, 127, 200, 100]"
    );
    t!(
        simd_swizzle!(u8a, u8b, [31, 0, 16, 5, 5, 30, 1, 2, 9, 9, 17, 18, 29, 28, 3, 4]),
        "[12, 0, 255, 128, 128, 10, 1, 2, 255, 255, 1, 5, 11, 8, 3, 127]"
    );
    t!(
        simd_swizzle!(u16a, u16b, [7, 8, 0, 9, 1, 15, 2, 14]),
        "[3, 65535, 0, 1, 1, 4, 32767, 1000]"
    );
    t!(simd_swizzle!(u32a, [3, 0, 2, 1]), "[123456789, 0, 4294967295, 2147483648]");
    t!(simd_swizzle!(u32a, [1, 1, 1, 1]), "[2147483648, 2147483648, 2147483648, 2147483648]");
    t!(simd_swizzle!(u32a, u32b, [0, 1, 4, 5]), "[0, 2147483648, 1, 2147483648]");
    t!(simd_swizzle!(u32a, u32b, [6, 3, 2, 4]), "[2, 123456789, 4294967295, 1]");
    t!(simd_swizzle!(i32a, i32b, [4, 5, 0, 1]), "[-1, 2, -2147483648, 2147483647]");
    t!(simd_swizzle!(u64a, u64b, [3, 0]), "[18364758544493064720, 18446744073709551615]");
    t!(
        simd_swizzle!(f32a, f32c, [7, 0, 5, 2]).to_bits(),
        "[1056964608, 1069547520, 2147483648, 2139095040]"
    );
    t!(simd_swizzle!(f64a, f64b, [2, 1]).to_bits(), "[0, 9094988921128908188]");
    t!(u32a.rotate_elements_left::<1>(), "[2147483648, 4294967295, 123456789, 0]");
    t!(
        u8a.rotate_elements_right::<5>(),
        "[8, 9, 10, 11, 12, 0, 1, 2, 3, 127, 128, 129, 200, 250, 255, 7]"
    );
    t!(u32a.reverse(), "[123456789, 4294967295, 2147483648, 0]");
    let (lo, hi) = u32a.interleave(u32b);
    t!(lo, "[0, 1, 2147483648, 2147483648]");
    t!(hi, "[4294967295, 2, 123456789, 987654321]");
    t!(
        simd_swizzle!(u32a, u32b, [0, 1, 2, 3, 4, 5, 6, 7]),
        "[0, 2147483648, 4294967295, 123456789, 1, 2147483648, 2, 987654321]"
    );
    t!(
        simd_swizzle!(u32a, u32b, [7, 6, 5, 4, 3, 2, 1, 0]),
        "[987654321, 2, 2147483648, 1, 123456789, 4294967295, 2147483648, 0]"
    );
    t!(
        simd_swizzle!(u32a, [3, 2, 1, 0, 0, 1, 2, 3]),
        "[123456789, 4294967295, 2147483648, 0, 0, 2147483648, 4294967295, 123456789]"
    );
    t!(simd_swizzle!(u8a, u8b, [31, 30, 29, 28, 3, 2, 1, 0]), "[12, 10, 11, 8, 3, 2, 1, 0]");

    // Inserting and extracting lanes.
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(u8a), 15, 99u8) },
        "[0, 1, 2, 3, 127, 128, 129, 200, 250, 255, 7, 8, 9, 10, 11, 99]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(u8a), 0, 98u8) },
        "[98, 1, 2, 3, 127, 128, 129, 200, 250, 255, 7, 8, 9, 10, 11, 12]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(i16a), 5, -77i16) },
        "[0, 1, 32767, -32768, -1, -77, -30000, 3]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(u32a), 3, 0xdead_beef_u32) },
        "[0, 2147483648, 4294967295, 3735928559]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(i64a), 1, 0x7fff_ffff_ffff_ffff_i64) },
        "[-9223372036854775808, 9223372036854775807]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(f32a), 2, -1.5f32) }.to_bits(),
        "[1069547520, 2147483648, 3217031168, 1077936128]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(bb(f64a), 0, f64::INFINITY) }.to_bits(),
        "[9218868437227405312, 9094988921128908188]"
    );
    t!([u8a[0], u8a[7], u8a[15]], "[0, 200, 12]");
    t!([f32a[1].to_bits(), f32a[3].to_bits()], "[2147483648, 1077936128]");
    t!([i64a[0], i64a[1]], "[-9223372036854775808, -5]");

    // Splatting.
    t!(
        u8x16::splat(bb(0xa5)),
        "[165, 165, 165, 165, 165, 165, 165, 165, 165, 165, 165, 165, 165, 165, 165, 165]"
    );
    t!(i16x8::splat(bb(-3)), "[-3, -3, -3, -3, -3, -3, -3, -3]");
    t!(u32x4::splat(bb(0x8000_0001)), "[2147483649, 2147483649, 2147483649, 2147483649]");
    t!(u64x2::splat(bb(0x1122_3344_5566_7788)), "[1234605616436508552, 1234605616436508552]");
    t!(f32x4::splat(bb(-0.0)).to_bits(), "[2147483648, 2147483648, 2147483648, 2147483648]");
    t!(f64x2::splat(bb(2.5)).to_bits(), "[4612811918334230528, 4612811918334230528]");

    // Copies and transmutes.
    let u32_array = bb([1u32, 0x8000_0002, 3, 0x4000_0004]);
    let from_array = u32x4::from_array(u32_array);
    t!(from_array, "[1, 2147483650, 3, 1073741828]");
    // Byte order dependent.
    #[cfg(target_endian = "little")]
    t!(
        unsafe { std::mem::transmute::<u32x4, u8x16>(bb(u32a)) },
        "[0, 0, 0, 0, 0, 0, 0, 128, 255, 255, 255, 255, 21, 205, 91, 7]"
    );
    // Byte order dependent.
    #[cfg(target_endian = "little")]
    t!(
        unsafe { std::mem::transmute::<u8x16, u64x2>(bb(u8a)) },
        "[14447970362580926720, 867798387104677882]"
    );
    // Byte order dependent.
    #[cfg(target_endian = "little")]
    t!(
        unsafe { std::mem::transmute::<i16x8, f32x4>(bb(i16a)) }.to_bits(),
        "[65536, 2147516415, 65601535, 232144]"
    );
    // Byte order dependent.
    #[cfg(target_endian = "little")]
    t!(
        unsafe { std::mem::transmute::<f64x2, [u32; 4]>(bb(f64a)) },
        "[0, 2147483648, 2281731484, 2117592124]"
    );
    let moved = bb(u8a);
    t!(moved, "[0, 1, 2, 3, 127, 128, 129, 200, 250, 255, 7, 8, 9, 10, 11, 12]");

    // Vectors wider than 128 bits are processed in 128-bit chunks.
    let u32x8a =
        bb(u32x8::from_array([0, 0x8000_0000, 0xffff_ffff, 123456789, 5, 6, 7, 0x7fff_ffff]));
    let u32x8b = bb(u32x8::from_array([1, 0x8000_0000, 2, 987654321, 50, 60, 70, 80]));
    let f32x8a = bb(f32x8::from_array([1.5, -0.0, f32::INFINITY, 3.0, -4.0, 0.25, 8.0, -0.0]));
    let f32x8b = bb(f32x8::from_array([-2.25, 0.0, f32::INFINITY, 0.5, 4.0, 0.25, -8.0, 0.0]));
    let u8x32a = bb(u8x32::from_array(std::array::from_fn(|i| (i * 9 + 200) as u8)));
    let u8x32b = bb(u8x32::from_array(std::array::from_fn(|i| (i * 5 + 3) as u8)));
    let u32x16a =
        bb(u32x16::from_array(std::array::from_fn(|i| (i as u32 * 0x1111_1111) ^ 0x8000_0001)));
    let u32x16b = bb(u32x16::from_array(std::array::from_fn(|i| i as u32 * 3 + 1)));
    t!(u32x8a + u32x8b, "[1, 0, 1, 1111111110, 55, 66, 77, 2147483727]");
    t!(u32x8a * u32x8b, "[0, 0, 4294967294, 4227814277, 250, 360, 490, 4294967216]");
    t!(u32x8a.simd_lt(u32x8b).to_simd(), "[-1, 0, 0, -1, -1, -1, -1, 0]");
    t!(u32x8a.simd_lt(u32x8b).select(u32x8a, u32x8b), "[0, 2147483648, 2, 123456789, 5, 6, 7, 80]");
    t!(
        nb!(f32x8a - f32x8b),
        "[1081081856, 2147483648, 4294967295, 1075838976, 3238002688, 0, 1098907648, 2147483648]"
    );
    t!(f32x8a.simd_ge(f32x8b).to_simd(), "[-1, -1, -1, -1, 0, -1, -1, -1]");
    t!(
        (-f32x8a).to_bits(),
        "[3217031168, 0, 4286578688, 3225419776, 1082130432, 3196059648, 3238002688, 0]"
    );
    t!(
        u8x32a.saturating_add(u8x32b),
        "[203, 217, 231, 245, 255, 255, 255, 45, 59, 73, 87, 101, 115, 129, 143, 157, 171, 185, 199, 213, 227, 241, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255]"
    );
    t!(
        u8x32a.saturating_sub(u8x32b),
        "[197, 201, 205, 209, 213, 217, 221, 0, 0, 0, 0, 0, 0, 0, 0, 1, 5, 9, 13, 17, 21, 25, 29, 33, 37, 41, 45, 49, 53, 57, 61, 65]"
    );
    t!(
        u32x16a ^ u32x16b,
        "[2147483648, 2433814804, 2720145956, 3006477112, 3292808264, 3579139396, 3865470580, 4151801696, 143165584, 429496708, 715827892, 1002159000, 1288490216, 1574821364, 1861152452, 2147483600]"
    );
    t!(
        u32x16a.simd_gt(u32x16b).select(u32x16b, u32x16a),
        "[1, 4, 7, 10, 13, 16, 19, 22, 25, 28, 31, 34, 37, 40, 43, 46]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(u32x16a, 13, 77u32) },
        "[2147483649, 2433814800, 2720145955, 3006477106, 3292808261, 3579139412, 3865470567, 4151801718, 143165577, 429496728, 715827883, 1002159034, 1288490189, 77, 1861152495, 2147483646]"
    );
    t!(
        u32x16a.reverse(),
        "[2147483646, 1861152495, 1574821340, 1288490189, 1002159034, 715827883, 429496728, 143165577, 4151801718, 3865470567, 3579139412, 3292808261, 3006477106, 2720145955, 2433814800, 2147483649]"
    );
    t!(
        simd_swizzle!(u32x16a, [15, 3, 2, 1, 0, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]),
        "[2147483646, 3006477106, 2720145955, 2433814800, 2147483649, 3292808261, 3579139412, 3865470567, 4151801718, 143165577, 429496728, 715827883, 1002159034, 1288490189, 1574821340, 1861152495]"
    );
    t!(
        simd_swizzle!(u32x16a, u32x16b, [0, 4, 8, 12, 16, 20, 24, 28, 1, 5, 9, 13, 17, 21, 25, 29]),
        "[2147483649, 3292808261, 143165577, 1288490189, 1, 13, 25, 37, 2433814800, 3579139412, 429496728, 1574821340, 4, 16, 28, 40]"
    );
    t!(
        unsafe { std::intrinsics::simd::simd_insert(u32x8a, 6, 0x1357_9bdf_u32) },
        "[0, 2147483648, 4294967295, 123456789, 5, 6, 324508639, 2147483647]"
    );
    t!(u32x8a.reverse(), "[2147483647, 7, 6, 5, 123456789, 4294967295, 2147483648, 0]");
    t!(
        u8x32a.rotate_elements_left::<3>(),
        "[227, 236, 245, 254, 7, 16, 25, 34, 43, 52, 61, 70, 79, 88, 97, 106, 115, 124, 133, 142, 151, 160, 169, 178, 187, 196, 205, 214, 223, 200, 209, 218]"
    );
    t!(u32x8::splat(bb(9)), "[9, 9, 9, 9, 9, 9, 9, 9]");
    t!(
        u64x8::splat(bb(0x0102_0304_0506_0708)),
        "[72623859790382856, 72623859790382856, 72623859790382856, 72623859790382856, 72623859790382856, 72623859790382856, 72623859790382856, 72623859790382856]"
    );
    // Byte order dependent.
    #[cfg(target_endian = "little")]
    t!(
        unsafe { std::mem::transmute::<u32x8, [u8; 32]>(u32x8a) },
        "[0, 0, 0, 0, 0, 0, 0, 128, 255, 255, 255, 255, 21, 205, 91, 7, 5, 0, 0, 0, 6, 0, 0, 0, 7, 0, 0, 0, 255, 255, 255, 127]"
    );

    // Vectors narrower than 128 bits keep using the per-lane code.
    let u8x8a = bb(u8x8::from_array([0, 1, 127, 128, 200, 255, 9, 10]));
    let u8x8b = bb(u8x8::from_array([255, 1, 1, 128, 100, 1, 9, 11]));
    let u32x2a = bb(u32x2::from_array([0xffff_ffff, 7]));
    t!(u8x8a + u8x8b, "[255, 2, 128, 0, 44, 0, 18, 21]");
    t!(u8x8a.simd_lt(u8x8b).to_simd(), "[-1, 0, 0, 0, 0, 0, 0, -1]");
    t!(u8x8a.simd_lt(u8x8b).select(u8x8a, u8x8b), "[0, 1, 1, 128, 100, 1, 9, 10]");
    t!(u32x2a.reverse(), "[7, 4294967295]");
    t!(unsafe { std::intrinsics::simd::simd_insert(u32x2a, 1, 5u32) }, "[4294967295, 5]");
}
