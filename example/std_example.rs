#![allow(internal_features)]
#![cfg_attr(target_has_reliable_f128, feature(f128))]
#![feature(cfg_target_has_reliable_f16_f128)]
#![feature(core_intrinsics)]
#![feature(coroutine_trait)]
#![feature(coroutines)]
#![feature(repr_simd)]
#![feature(tuple_trait)]
#![feature(unboxed_closures)]

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
use std::hint::black_box;
use std::intrinsics::simd::{simd_extract, simd_extract_dyn, simd_insert, simd_insert_dyn};
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

    let _a = 1u32 << 2u8;

    let empty: [i32; 0] = [];
    assert!(empty.is_sorted());

    println!("{:?}", std::intrinsics::caller_location());

    test_simd_values();

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

#[repr(simd)]
#[derive(Copy, Clone)]
struct U8X16([u8; 16]);

#[repr(simd)]
#[derive(Copy, Clone)]
struct U8X8([u8; 8]);

#[repr(simd)]
#[derive(Copy, Clone)]
struct U32X4([u32; 4]);

#[repr(simd)]
#[derive(Copy, Clone)]
struct F32X4([f32; 4]);

#[derive(Copy, Clone)]
struct WrappedU8X16(U8X16);

struct DropWrappedU8X16(U8X16);

static DROPPED_LANE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

impl Drop for DropWrappedU8X16 {
    fn drop(&mut self) {
        DROPPED_LANE
            .store(unsafe { simd_extract(self.0, 5) }, std::sync::atomic::Ordering::Relaxed);
    }
}

#[inline(never)]
fn simd_value_with_drop_glue(v: DropWrappedU8X16, extract: bool) -> u8 {
    if black_box(extract) { unsafe { simd_extract(v.0, 2) } } else { 0 }
}

#[inline(never)]
fn simd_value_by_value(v: U8X16, lane: u32) -> (U8X16, u8) {
    unsafe { (simd_insert_dyn(v, lane, 0xaa_u8), simd_extract_dyn(v, lane)) }
}

/// Lane access and transmutes of SIMD values with all lanes different, such that mixing up lanes
/// or reinterpreting them in the wrong order is noticed.
fn test_simd_values() {
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = (i as u8 + 1).wrapping_mul(0x1d) ^ 0x5c;
    }
    let v = black_box(U8X16(bytes));

    unsafe {
        for i in 0..16 {
            assert_eq!(simd_extract_dyn::<_, u8>(v, black_box(i as u32)), bytes[i]);
        }
        assert_eq!(simd_extract::<_, u8>(v, 0), bytes[0]);
        assert_eq!(simd_extract::<_, u8>(v, 15), bytes[15]);

        // Insert with a constant and a dynamic index.
        let mut expected = bytes;
        expected[3] = 0xe1;
        expected[12] = 0xd2;
        let w = simd_insert(v, 3, 0xe1_u8);
        let w = simd_insert_dyn(w, black_box(12), 0xd2_u8);
        assert_eq!(std::mem::transmute::<_, [u8; 16]>(w), expected);
        let expected_w = expected;

        let (w2, old) = simd_value_by_value(w, black_box(12));
        assert_eq!(old, 0xd2);
        expected[12] = 0xaa;
        assert_eq!(std::mem::transmute::<_, [u8; 16]>(w2), expected);

        #[cfg(target_endian = "little")]
        {
            assert_eq!(std::mem::transmute::<_, u128>(w), u128::from_le_bytes(expected_w));
            let expected = u128::from_le_bytes(bytes);
            assert_eq!(std::mem::transmute::<_, u128>(v), expected);
            assert_eq!(
                std::mem::transmute::<_, (u64, u64)>(v),
                (expected as u64, (expected >> 64) as u64)
            );

            // Reinterpret the lanes as wider integers and floats.
            let words = std::mem::transmute::<_, U32X4>(v);
            for i in 0..4 {
                let word = u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap());
                assert_eq!(simd_extract_dyn::<_, u32>(words, i as u32), word);
            }
            let words = simd_insert_dyn(words, black_box(2), 0x3f80_0000_u32);
            let floats = std::mem::transmute::<_, F32X4>(words);
            assert_eq!(simd_extract::<_, f32>(floats, 2), 1.0);
            assert_eq!(
                simd_extract::<_, f32>(floats, 1).to_bits(),
                u32::from_le_bytes(bytes[4..8].try_into().unwrap())
            );
            assert_eq!(std::mem::transmute::<_, u128>(floats) >> 96, expected >> 96);

            // Build a vector from a number.
            let from_num = std::mem::transmute::<u128, U8X16>(expected.rotate_left(8));
            assert_eq!(simd_extract::<_, u8>(from_num, 1), bytes[0]);
            assert_eq!(simd_extract::<_, u8>(from_num, 0), bytes[15]);
        }

        // A struct wrapping a vector.
        let wrapped = WrappedU8X16(w);
        let inner = black_box(wrapped).0;
        assert_eq!(std::mem::transmute::<_, [u8; 16]>(inner), expected_w);

        // A value with drop glue takes its address when dropped.
        let extracted = simd_value_with_drop_glue(DropWrappedU8X16(w), true);
        let dropped = DROPPED_LANE.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!((extracted, dropped), (expected_w[2], expected_w[5]));

        // A vector whose address is taken stays in memory.
        let r = black_box(&w);
        assert_eq!(simd_extract::<_, u8>(*r, 3), 0xe1);

        // 64-bit vectors, a vector register on some targets.
        let small = black_box(U8X8([1, 2, 3, 4, 5, 6, 7, 8]));
        let small = simd_insert_dyn(small, black_box(5), 0x77_u8);
        assert_eq!(std::mem::transmute::<_, [u8; 8]>(small), [1, 2, 3, 4, 5, 0x77, 7, 8]);
        assert_eq!(simd_extract::<_, u8>(small, 7), 8);
        #[cfg(target_endian = "little")]
        assert_eq!(std::mem::transmute::<_, u64>(small), 0x0807_7705_0403_0201);
    }
}

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
