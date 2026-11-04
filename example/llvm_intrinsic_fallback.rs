// Calls LLVM intrinsics that cg_clif doesn't lower itself. They are compiled by the LLVM backend.
//
// The test passes when compiled with the LLVM backend too, which is where the expected values come
// from.

#![allow(internal_features, unsafe_op_in_unsafe_fn)]
#![feature(link_llvm_intrinsics)]

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
use std::mem::transmute;

unsafe extern "llvm-intrinsic" {
    #[link_name = "llvm.sadd.sat.i8"]
    fn sadd_sat_i8(a: i8, b: i8) -> i8;
    #[link_name = "llvm.sadd.sat.i64"]
    fn sadd_sat_i64(a: i64, b: i64) -> i64;
    #[link_name = "llvm.ctlz.i32"]
    fn ctlz_i32(a: i32, is_zero_poison: bool) -> i32;
}

fn main() {
    test_scalar_intrinsics();

    #[cfg(target_arch = "x86_64")]
    unsafe {
        test_pmulhuw();
        if is_x86_feature_detected!("avx2") {
            test_avx2_shift_variable();
        }
        if is_x86_feature_detected!("avx512f") {
            test_avx512_ternarylogic();
        }
        if is_x86_feature_detected!("avx512f") {
            test_avx512_mask_cmp_pd();
            test_avx512_compressstoreu();
        }
        if is_x86_feature_detected!("avx512vbmi") {
            test_avx512_permutex2var_epi8();
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        test_neon_tbx();
        test_neon_sqxtn();
        test_neon_ushl();
        test_neon_fmaxnm();
        test_neon_ld4_lane();
        test_neon_ld4_dup();
        test_neon_st1_x2();
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn test_pmulhuw() {
    let a: [u16; 8] = [0xffff, 0x8000, 3, 0x1234, 0, 7, 0xfffe, 0x0101];
    let b: [u16; 8] = [0xffff, 0x0003, 0x5555, 0x4321, 9, 0x9000, 0x8001, 0xff00];
    let r: [u16; 8] = transmute(_mm_mulhi_epu16(transmute(a), transmute(b)));
    let expected: [u16; 8] = std::array::from_fn(|i| ((a[i] as u32 * b[i] as u32) >> 16) as u16);
    assert_eq!(r, expected);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn test_avx2_shift_variable() {
    let a: [u32; 8] = [0x8000_0001, 0xffff_ffff, 0x1234_5678, 0xdead_beef, 1, 2, 0xf0f0_f0f0, 3];
    let count: [u32; 8] = [0, 1, 4, 31, 32, 33, 8, 0xffff_ffff];

    let r: [u32; 8] = transmute(_mm256_srlv_epi32(transmute(a), transmute(count)));
    assert_eq!(r, [0x8000_0001, 0x7fff_ffff, 0x0123_4567, 1, 0, 0, 0x00f0_f0f0, 0]);

    let r: [u32; 8] = transmute(_mm256_sllv_epi32(transmute(a), transmute(count)));
    assert_eq!(r, [0x8000_0001, 0xffff_fffe, 0x2345_6780, 0x8000_0000, 0, 0, 0xf0f0_f000, 0]);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn test_avx512_ternarylogic() {
    let a: [u64; 8] = [
        0x0123_4567_89ab_cdef,
        0xffff_0000_ffff_0000,
        0xf0f0_f0f0_f0f0_f0f0,
        0,
        u64::MAX,
        0x5555_5555_5555_5555,
        3,
        0x8000_0000_0000_0001,
    ];
    let b: [u64; 8] = [
        0xfedc_ba98_7654_3210,
        0xff00_ff00_ff00_ff00,
        0xcccc_cccc_cccc_cccc,
        u64::MAX,
        0,
        0x3333_3333_3333_3333,
        5,
        0x4000_0000_0000_0002,
    ];
    let c: [u64; 8] = [
        0x0f0f_f0f0_1234_5678,
        0xaaaa_aaaa_aaaa_aaaa,
        0xaaaa_aaaa_aaaa_aaaa,
        0x0123_4567_89ab_cdef,
        0x1111_1111_1111_1111,
        0xf00f_f00f_f00f_f00f,
        6,
        0x2000_0000_0000_0004,
    ];

    // Three-way xor.
    let r: [u64; 8] =
        transmute(_mm512_ternarylogic_epi64::<0x96>(transmute(a), transmute(b), transmute(c)));
    let expected: [u64; 8] = std::array::from_fn(|i| a[i] ^ b[i] ^ c[i]);
    assert_eq!(r, expected);

    // `a ? b : c`, which is not symmetric in its operands.
    let r: [u64; 8] =
        transmute(_mm512_ternarylogic_epi64::<0xca>(transmute(a), transmute(b), transmute(c)));
    let expected: [u64; 8] = std::array::from_fn(|i| (a[i] & b[i]) | (!a[i] & c[i]));
    assert_eq!(r, expected);
}

// A write mask argument and an immediate.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn test_avx512_mask_cmp_pd() {
    let a: [f64; 8] = [1.0, 2.0, 3.0, f64::NAN, -1.0, 5.0, 6.0, 7.5];
    let b: [f64; 8] = [1.0, 3.0, 2.0, 4.0, -2.0, f64::NAN, 6.5, 7.0];
    let mask = 0b1101_1011;
    let r = _mm512_mask_cmp_pd_mask::<_CMP_LT_OQ>(mask, transmute(a), transmute(b));
    let expected = (0..8).fold(0u8, |acc, i| acc | (((a[i] < b[i]) as u8) << i)) & mask;
    assert_eq!(r, expected);
    assert_eq!(r, 0b0100_0010);

    // The mask is a literal on only one of the paths, so it must not be baked into the wrapper.
    let mut mask = 0xff;
    if std::hint::black_box(false) {
        mask = 0b1101_1011;
    }
    let r = _mm512_mask_cmp_pd_mask::<_CMP_LT_OQ>(mask, transmute(a), transmute(b));
    assert_eq!(r, 0b0100_0010);
    assert_eq!(
        _mm512_mask_cmp_pd_mask::<_CMP_LT_OQ>(
            std::hint::black_box(0x0f),
            transmute(a),
            transmute(b)
        ),
        0b0000_0010
    );
}

// A pointer argument, a mask argument and no return value.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn test_avx512_compressstoreu() {
    let a: [i32; 16] = std::array::from_fn(|i| 100 + 3 * i as i32);
    let mut out = [-1i32; 16];
    let mask = 0b1010_0110_0001_1001u16;
    _mm512_mask_compressstoreu_epi32(out.as_mut_ptr().cast(), mask, transmute(a));
    let mut expected = [-1i32; 16];
    let mut n = 0;
    for i in 0..16 {
        if mask & (1 << i) != 0 {
            expected[n] = a[i];
            n += 1;
        }
    }
    assert_eq!(out, expected);
    assert_eq!(n, 7);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512vbmi")]
unsafe fn test_avx512_permutex2var_epi8() {
    let a: [u8; 64] = std::array::from_fn(|i| i as u8);
    let b: [u8; 64] = std::array::from_fn(|i| 0x80 + i as u8);
    let idx: [u8; 64] = std::array::from_fn(|i| ((i * 37 + 5) % 128) as u8);
    let r: [u8; 64] =
        transmute(_mm512_permutex2var_epi8(transmute(a), transmute(idx), transmute(b)));
    let expected: [u8; 64] = std::array::from_fn(|i| {
        let j = idx[i] as usize;
        if j < 64 { a[j] } else { b[j - 64] }
    });
    assert_eq!(r, expected);
}

#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_tbx() {
    let table: [u8; 16] = [10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25];
    let fallback: [u8; 16] =
        [100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115];
    let idx: [u8; 16] = [15, 0, 16, 3, 255, 7, 1, 20, 2, 14, 9, 17, 5, 5, 31, 8];
    let r: [u8; 16] = transmute(vqtbx1q_u8(transmute(fallback), transmute(table), transmute(idx)));
    assert_eq!(r, [25, 10, 102, 13, 104, 17, 11, 107, 12, 24, 19, 111, 15, 15, 114, 18]);
}

#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_sqxtn() {
    let a: [i16; 8] = [0, 1, -1, 127, 128, -128, -129, 30000];
    let r: [i8; 8] = transmute(vqmovn_s16(transmute(a)));
    assert_eq!(r, [0, 1, -1, 127, 127, -128, -128, 127]);

    let a: [i32; 4] = [-70000, 70000, 32767, -32768];
    let r: [i16; 4] = transmute(vqmovn_s32(transmute(a)));
    assert_eq!(r, [-32768, 32767, 32767, -32768]);
}

#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_ushl() {
    let a: [u8; 8] = [0x81, 0x81, 0x81, 0xff, 0x10, 0x01, 0x80, 0x7f];
    let shift: [i8; 8] = [1, -1, 0, -7, 4, 7, -8, 9];
    let r: [u8; 8] = transmute(vshl_u8(transmute(a), transmute(shift)));
    assert_eq!(r, [0x02, 0x40, 0x81, 1, 0, 0x80, 0, 0]);
}

#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_fmaxnm() {
    let a: [f32; 2] = [f32::NAN, 1.5];
    let b: [f32; 2] = [2.0, f32::NAN];
    let r: [f32; 2] = transmute(vmaxnm_f32(transmute(a), transmute(b)));
    assert_eq!(r, [2.0, 1.5]);

    let a: [f32; 2] = [-3.0, 7.0];
    let b: [f32; 2] = [-4.0, 8.0];
    let r: [f32; 2] = transmute(vmaxnm_f32(transmute(a), transmute(b)));
    assert_eq!(r, [-3.0, 8.0]);
}

#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_ld4_lane() {
    let mem: [i8; 4] = [-1, -2, -3, -4];
    let v0: [i8; 16] = std::array::from_fn(|i| i as i8);
    let v1: [i8; 16] = std::array::from_fn(|i| 20 + i as i8);
    let v2: [i8; 16] = std::array::from_fn(|i| 40 + i as i8);
    let v3: [i8; 16] = std::array::from_fn(|i| 60 + i as i8);
    let src = int8x16x4_t(transmute(v0), transmute(v1), transmute(v2), transmute(v3));

    let r = vld4q_lane_s8::<5>(mem.as_ptr(), src);
    let r: [[i8; 16]; 4] = transmute([r.0, r.1, r.2, r.3]);
    for (i, lane) in r.iter().enumerate() {
        for (j, &v) in lane.iter().enumerate() {
            let expected = if j == 5 { mem[i] } else { (20 * i + j) as i8 };
            assert_eq!(v, expected, "vector {i} lane {j}");
        }
    }
}

#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_st1_x2() {
    let v0: [i8; 16] = std::array::from_fn(|i| i as i8 + 1);
    let v1: [i8; 16] = std::array::from_fn(|i| -(i as i8) - 1);
    let mut out = [0i8; 32];
    vst1q_s8_x2(out.as_mut_ptr(), int8x16x2_t(transmute(v0), transmute(v1)));
    assert_eq!(out[..16], v0);
    assert_eq!(out[16..], v1);
}

// A struct of vectors as the first printed type.
#[cfg(target_arch = "aarch64")]
unsafe fn test_neon_ld4_dup() {
    let mem: [i8; 4] = [-1, 2, -3, 4];
    let r = vld4q_dup_s8(mem.as_ptr());
    let r: [[i8; 16]; 4] = transmute([r.0, r.1, r.2, r.3]);
    for (i, lane) in r.iter().enumerate() {
        assert_eq!(*lane, [mem[i]; 16]);
    }
}

// Negative and boolean literals, with and without a constant operand.
fn test_scalar_intrinsics() {
    unsafe {
        let one = std::hint::black_box(1i8);
        assert_eq!(sadd_sat_i8(-128, -1), -128);
        assert_eq!(sadd_sat_i8(-100, -100), -128);
        assert_eq!(sadd_sat_i8(100, 100), 127);
        assert_eq!(sadd_sat_i8(-128, one), -127);
        assert_eq!(sadd_sat_i8(one, -2), -1);
        assert_eq!(sadd_sat_i64(-1, i64::MIN), i64::MIN);
        assert_eq!(sadd_sat_i64(i64::MIN, 5), i64::MIN + 5);
        assert_eq!(sadd_sat_i64(std::hint::black_box(i64::MAX), 1), i64::MAX);
        assert_eq!(ctlz_i32(std::hint::black_box(0x00f0_0000), true), 8);
        assert_eq!(ctlz_i32(std::hint::black_box(0), false), 32);
    }
}
