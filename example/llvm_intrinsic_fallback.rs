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
        test_x86_sse42();
        if is_x86_feature_detected!("sha")
            && is_x86_feature_detected!("sse4.1")
            && is_x86_feature_detected!("ssse3")
        {
            test_x86_sha();
        }
        if is_x86_feature_detected!("aes") {
            test_x86_aes();
        }
        if is_x86_feature_detected!("pclmulqdq") {
            test_x86_pclmulqdq();
        }
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
        if std::arch::is_aarch64_feature_detected!("aes")
            && std::arch::is_aarch64_feature_detected!("sha2")
            && std::arch::is_aarch64_feature_detected!("crc")
        {
            test_neon_crypto();
        }
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

// Intrinsics that cg_clif used to lower with inline assembly.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sha,sse4.1,ssse3")]
unsafe fn test_x86_sha() {
    let a: [u32; 4] = [0x01234567, 0x89abcdef, 0xfedcba98, 0x76543210];
    let b: [u32; 4] = [0x11111111, 0x22222222, 0x33333333, 0x44444444];
    let k: [u32; 4] = [0xdeadbeef, 0xcafebabe, 0x0badf00d, 0x8badf00d];
    let (a, b, k) = (transmute(a), transmute(b), transmute(k));
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha256rnds2_epu32(a, b, k)),
        [0x46156a98_u32, 0x16a4c8e2_u32, 0xdfaf0432_u32, 0x21e5920e_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha256msg1_epu32(a, b)),
        [0x3e8111b3_u32, 0x8a2bdf80_u32, 0x217eee4b_u32, 0xda987654_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha256msg2_epu32(a, b)),
        [0x0116789a_u32, 0x346789aa_u32, 0xf23abfcf_u32, 0xac41776d_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha1rnds4_epu32::<2>(a, b)),
        [0x49e48447_u32, 0x3d5ae1f0_u32, 0x27f1b18f_u32, 0x3bffb33b_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha1msg1_epu32(a, b)),
        [0x32107654_u32, 0xcdef89ab_u32, 0xffffffff_u32, 0xffffffff_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha1msg2_epu32(a, b)),
        [0x17da8e43_u32, 0x3175b9fd_u32, 0xb9fd3175_u32, 0x8ace0246_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(_mm_sha1nexte_epu32(a, b)),
        [0x11111111_u32, 0x22222222_u32, 0x33333333_u32, 0x61d950c8_u32]
    );
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "aes")]
unsafe fn test_x86_aes() {
    let a: [u8; 16] = std::array::from_fn(|i| (i * 17 + 3) as u8);
    let k: [u8; 16] = std::array::from_fn(|i| (255 - i * 7) as u8);
    let (a, k) = (transmute(a), transmute(k));
    assert_eq!(
        transmute::<_, [u8; 16]>(_mm_aesenc_si128(a, k)),
        [
            0x55_u8, 0x84_u8, 0x48_u8, 0x76_u8, 0x65_u8, 0x26_u8, 0xfd_u8, 0x40_u8, 0x60_u8,
            0xbf_u8, 0x6e_u8, 0x84_u8, 0xe6_u8, 0x7f_u8, 0xed_u8, 0x57_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(_mm_aesenclast_si128(a, k)),
        [
            0x84_u8, 0x92_u8, 0x64_u8, 0x9d_u8, 0x43_u8, 0x02_u8, 0x74_u8, 0xcb_u8, 0xfa_u8,
            0x21_u8, 0x86_u8, 0x68_u8, 0x21_u8, 0x5e_u8, 0x64_u8, 0x38_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(_mm_aesdec_si128(a, k)),
        [
            0xd3_u8, 0x4d_u8, 0xab_u8, 0xf9_u8, 0x5d_u8, 0x48_u8, 0xb5_u8, 0x78_u8, 0xe1_u8,
            0x8e_u8, 0xe3_u8, 0xb8_u8, 0x5f_u8, 0x44_u8, 0xcf_u8, 0x53_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(_mm_aesdeclast_si128(a, k)),
        [
            0x2a_u8, 0x58_u8, 0xe9_u8, 0x57_u8, 0xf5_u8, 0x47_u8, 0xfe_u8, 0x94_u8, 0x09_u8,
            0x9e_u8, 0x7b_u8, 0xd8_u8, 0xf4_u8, 0xb8_u8, 0x79_u8, 0xb2_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(_mm_aesimc_si128(a)),
        [
            0x91_u8, 0xda_u8, 0xbf_u8, 0xf0_u8, 0xad_u8, 0xde_u8, 0x9b_u8, 0xe4_u8, 0x19_u8,
            0x52_u8, 0x37_u8, 0x78_u8, 0x0f_u8, 0x01_u8, 0x92_u8, 0x40_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(_mm_aeskeygenassist_si128::<0x1b>(a)),
        [
            0xa0_u8, 0x6a_u8, 0xf9_u8, 0xda_u8, 0x71_u8, 0xf9_u8, 0xda_u8, 0xa0_u8, 0x8a_u8,
            0xe1_u8, 0xa1_u8, 0x77_u8, 0xfa_u8, 0xa1_u8, 0x77_u8, 0x8a_u8
        ]
    );
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
unsafe fn test_x86_sse42() {
    assert_eq!(_mm_crc32_u8(0x12345678, 0xab), 0xc0912609);
    assert_eq!(_mm_crc32_u16(0x12345678, 0xabcd), 0xaae32043);
    assert_eq!(_mm_crc32_u32(0x12345678, 0xabcdef01), 0x39bcb2dd);
    assert_eq!(_mm_crc32_u64(0x12345678, 0xabcdef0123456789), 0xecd7500f as u64);

    let a: [u8; 16] = *b"hello, world abc";
    let b: [u8; 16] = *b"o, wo\0\0\0\0\0\0\0\0\0\0\0";
    let (a, b) = (transmute(a), transmute(b));
    assert_eq!(_mm_cmpestri::<{ _SIDD_CMP_EQUAL_ORDERED }>(b, 5, a, 16), 4);
    let r = _mm_cmpestrm::<{ _SIDD_CMP_EQUAL_ANY | _SIDD_UBYTE_OPS }>(b, 5, a, 16);
    assert_eq!(
        transmute::<_, [u8; 16]>(r),
        [
            0xf0_u8, 0x11_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8,
            0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8, 0x00_u8
        ]
    );

    let f: [f32; 4] = [1.5, -2.5, 3.4999, 1e20];
    assert_eq!(transmute::<_, [i32; 4]>(_mm_cvtps_epi32(transmute(f))), [2, -2, 3, i32::MIN]);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "pclmulqdq")]
unsafe fn test_x86_pclmulqdq() {
    let a: [u64; 2] = [0x0123456789abcdef, 0xfedcba9876543210];
    let b: [u64; 2] = [0x1111222233334444, 0x8000000000000001];
    let (a, b) = (transmute(a), transmute(b));
    assert_eq!(
        transmute::<_, [u64; 2]>(_mm_clmulepi64_si128::<0x00>(a, b)),
        [0x001324020431307c_u64, 0x001304220431105c_u64]
    );
    assert_eq!(
        transmute::<_, [u64; 2]>(_mm_clmulepi64_si128::<0x01>(a, b)),
        [0x0f1c3a1c15200c40_u64, 0x0f1c1a3c15202c60_u64]
    );
    assert_eq!(
        transmute::<_, [u64; 2]>(_mm_clmulepi64_si128::<0x10>(a, b)),
        [0x8123456789abcdef_u64, 0x0091a2b3c4d5e6f7_u64]
    );
    assert_eq!(
        transmute::<_, [u64; 2]>(_mm_clmulepi64_si128::<0x11>(a, b)),
        [0xfedcba9876543210_u64, 0x7f6e5d4c3b2a1908_u64]
    );
}

// Intrinsics that cg_clif used to lower with inline assembly.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "aes,sha2,crc,neon")]
unsafe fn test_neon_crypto() {
    let a: uint8x16_t = transmute(std::array::from_fn::<u8, 16, _>(|i| (i * 17 + 3) as u8));
    let k: uint8x16_t = transmute(std::array::from_fn::<u8, 16, _>(|i| (255 - i * 7) as u8));
    assert_eq!(
        transmute::<_, [u8; 16]>(vaeseq_u8(a, k)),
        [
            0xb0_u8, 0x5f_u8, 0xfa_u8, 0x22_u8, 0x49_u8, 0x4a_u8, 0x50_u8, 0x86_u8, 0x29_u8,
            0x1b_u8, 0x48_u8, 0x8d_u8, 0x43_u8, 0xce_u8, 0x65_u8, 0xfe_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(vaesdq_u8(a, k)),
        [
            0x55_u8, 0x86_u8, 0x9b_u8, 0xc6_u8, 0x1d_u8, 0x83_u8, 0xb8_u8, 0x81_u8, 0x5d_u8,
            0x4f_u8, 0x19_u8, 0xe7_u8, 0x8c_u8, 0xa7_u8, 0x78_u8, 0x93_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(vaesmcq_u8(a)),
        [
            0x29_u8, 0x72_u8, 0x07_u8, 0x58_u8, 0x75_u8, 0x36_u8, 0x43_u8, 0x0c_u8, 0xa1_u8,
            0xfa_u8, 0x8f_u8, 0xd0_u8, 0x4d_u8, 0x1e_u8, 0xd0_u8, 0x5f_u8
        ]
    );
    assert_eq!(
        transmute::<_, [u8; 16]>(vaesimcq_u8(a)),
        [
            0x91_u8, 0xda_u8, 0xbf_u8, 0xf0_u8, 0xad_u8, 0xde_u8, 0x9b_u8, 0xe4_u8, 0x19_u8,
            0x52_u8, 0x37_u8, 0x78_u8, 0x0f_u8, 0x01_u8, 0x92_u8, 0x40_u8
        ]
    );

    let x: uint32x4_t = transmute([0x01234567_u32, 0x89abcdef, 0xfedcba98, 0x76543210]);
    let y: uint32x4_t = transmute([0x11111111_u32, 0x22222222, 0x33333333, 0x44444444]);
    let z: uint32x4_t = transmute([0xdeadbeef_u32, 0xcafebabe, 0x0badf00d, 0x8badf00d]);
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha1cq_u32(x, 0x9abcdef0, y)),
        [0xe8918bc7_u32, 0x4e80cbf5_u32, 0x9e780fd9_u32, 0x73c4d5de_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha1mq_u32(x, 0x9abcdef0, y)),
        [0x342fcf90_u32, 0x9de19e0f_u32, 0x97530db9_u32, 0x73c4d5de_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha1pq_u32(x, 0x9abcdef0, y)),
        [0x0c254203_u32, 0x4e80e851_u32, 0x3851ea74_u32, 0x34567892_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha1su0q_u32(x, y, z)),
        [0x21524110_u32, 0x35014541_u32, 0xe4605b84_u32, 0xdfdbe03f_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha1su1q_u32(x, y)),
        [0x4602ce8a_u32, 0x7531fdb9_u32, 0x7531fdb9_u32, 0x60adf934_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha256hq_u32(x, y, z)),
        [0x1cc12692_u32, 0x86ec03fa_u32, 0xd95b5f64_u32, 0x24142679_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha256h2q_u32(x, y, z)),
        [0xcca4a607_u32, 0x019106f9_u32, 0x3b57f5c7_u32, 0x3a8c503f_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha256su0q_u32(x, y)),
        [0x3e8111b3_u32, 0x8a2bdf80_u32, 0x217eee4b_u32, 0xda987654_u32]
    );
    assert_eq!(
        transmute::<_, [u32; 4]>(vsha256su1q_u32(x, y, z)),
        [0x694b3768_u32, 0x0304a101_u32, 0x40646a1d_u32, 0x19a251c9_u32]
    );
    assert_eq!(vsha1h_u32(0x80000001), 0x60000000);

    assert_eq!(__crc32b(0x12345678, 0xab), 0x1fc8b738);
    assert_eq!(__crc32h(0x12345678, 0xabcd), 0x5abec668);
    assert_eq!(__crc32w(0x12345678, 0xabcdef01), 0xab7b7b55);
    assert_eq!(__crc32d(0x12345678, 0xabcdef0123456789), 0x44b8d6fb);
    assert_eq!(__crc32cb(0x12345678, 0xab), 0xc0912609);
    assert_eq!(__crc32ch(0x12345678, 0xabcd), 0xaae32043);
    assert_eq!(__crc32cw(0x12345678, 0xabcdef01), 0x39bcb2dd);
    assert_eq!(__crc32cd(0x12345678, 0xabcdef0123456789), 0xecd7500f);

    assert_eq!(
        vmull_p64(0x0123456789abcdef, 0xfedcba9876543210),
        0x00e038d8688850b040a0789828c810f0
    );
    let a8: [u8; 8] = [3, 0xff, 0x80, 7, 0, 0x55, 0xaa, 1];
    let b8: [u8; 8] = [5, 0xff, 2, 0x81, 9, 0xaa, 0x33, 0x80];
    let r: [u16; 8] = transmute(vmull_p8(transmute(a8), transmute(b8)));
    assert_eq!(
        r,
        [
            0x000f_u16, 0x5555_u16, 0x0100_u16, 0x0387_u16, 0x0000_u16, 0x2222_u16, 0x1e1e_u16,
            0x0080_u16
        ]
    );

    let f: [f32; 4] = [1.5, -2.5, 3.4999, 2.5];
    let f: float32x4_t = transmute(f);
    assert_eq!(transmute::<_, [i32; 4]>(vcvtnq_s32_f32(f)), [2, -2, 3, 2]);
    assert_eq!(
        transmute::<_, [f32; 4]>(vrecpeq_f32(f)),
        [0.6660156, -0.39941406, 0.2861328, 0.39941406]
    );
    let g: float32x4_t = transmute([2.0f32, 0.5, 1.0, 3.0]);
    assert_eq!(transmute::<_, [f32; 4]>(vrecpsq_f32(f, g)), [-1.0, 3.25, -1.4999001, -5.5]);
}
