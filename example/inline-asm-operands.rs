// Inline asm operands in registers wider than a general purpose register.

#![cfg_attr(target_arch = "aarch64", feature(f16, f128))]

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use std::arch::asm;
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use std::hint::black_box;
#[cfg(target_arch = "x86_64")]
use std::mem::MaybeUninit;
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
use std::mem::transmute;

fn main() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        if is_x86_feature_detected!("avx") {
            write_word();
            ymm_operands();
        }
        if is_x86_feature_detected!("avx512f") {
            zmm_operands();
            kreg_operands();
        }
        if is_x86_feature_detected!("avx512bw") {
            kreg_operands_bw();
        }
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        vreg_operands();
    }
}

/// Writes four 64-bit words through a `ymm` and a `xmm` register.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn write_word() {
    unsafe {
        let [l0, l1, l2, l3] = black_box([1u64, 2, 3, 4]);
        let mut dst = [0u64; 4];
        let _lo: __m256i;
        let _hi: __m128i;
        asm!(
            "vmovq {lo:x}, {l0}",
            "vpinsrq {lo:x}, {lo:x}, {l1}, 1",
            "vmovq {hi:x}, {l2}",
            "vpinsrq {hi:x}, {hi:x}, {l3}, 1",
            "vinsertf128 {lo:y}, {lo:y}, {hi:x}, 1",
            "vmovdqu ymmword ptr [{dst}], {lo:y}",
            dst = in(reg) dst.as_mut_ptr(),
            l0 = in(reg) l0,
            l1 = in(reg) l1,
            l2 = in(reg) l2,
            l3 = in(reg) l3,
            lo = out(ymm_reg) _lo,
            hi = out(xmm_reg) _hi,
            options(nostack, preserves_flags),
        );
        assert_eq!(dst, [1, 2, 3, 4]);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx")]
unsafe fn ymm_operands() {
    unsafe {
        let input: __m256i = transmute(black_box([1u64, 2, 3, 4]));

        // `vperm2f128` with immediate 1 swaps the two 128-bit halves.
        let output: __m256i;
        asm!(
            "vperm2f128 {y}, {y}, {y}, 1",
            y = inout(ymm_reg) input => output,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m256i, [u64; 4]>(output), [3, 4, 1, 2]);

        // Explicit registers.
        let swapped: __m256i;
        asm!(
            "vperm2f128 ymm4, ymm3, ymm3, 1",
            in("ymm3") input,
            out("ymm4") swapped,
            out("ymm5") _,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m256i, [u64; 4]>(swapped), [3, 4, 1, 2]);

        // Narrower types in a `ymm` register.
        let xmm: __m128i = transmute(black_box([5u64, 6]));
        let xmm_out: __m128i;
        asm!(
            "vpshufd {o:x}, {i:x}, 0x4e",
            i = in(ymm_reg) xmm,
            o = lateout(ymm_reg) xmm_out,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m128i, [u64; 2]>(xmm_out), [6, 5]);

        let float: f64;
        asm!(
            "vmovapd {o:x}, {i:x}",
            i = in(ymm_reg) 2.5f64,
            o = lateout(ymm_reg) float,
            options(nostack, preserves_flags),
        );
        assert_eq!(float, 2.5);

        let uninit: MaybeUninit<__m256i>;
        asm!(
            "vmovdqa {o}, {i}",
            i = in(ymm_reg) input,
            o = lateout(ymm_reg) uninit,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m256i, [u64; 4]>(uninit.assume_init()), [1, 2, 3, 4]);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn zmm_operands() {
    unsafe {
        let input: __m512i = transmute(black_box([1u64, 2, 3, 4, 5, 6, 7, 8]));
        let reverse: __m512i = transmute(black_box([7u64, 6, 5, 4, 3, 2, 1, 0]));
        let reversed = [8, 7, 6, 5, 4, 3, 2, 1];

        let output: __m512i;
        asm!(
            "vpermq {a}, {idx}, {a}",
            a = inout(zmm_reg) input => output,
            idx = in(zmm_reg) reverse,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m512i, [u64; 8]>(output), reversed);

        let output: __m512i;
        asm!(
            "vpermq {o}, {idx}, {i}",
            i = in(zmm_reg) input,
            idx = in(zmm_reg) reverse,
            o = lateout(zmm_reg) output,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m512i, [u64; 8]>(output), reversed);

        // Explicit registers, including ones only reachable with AVX-512.
        let high: __m512i;
        let high_inout: __m512i;
        asm!(
            "vpermq zmm31, zmm3, zmm2",
            "vpermq zmm17, zmm3, zmm17",
            in("zmm2") input,
            in("zmm3") reverse,
            out("zmm31") high,
            inout("zmm17") input => high_inout,
            out("zmm18") _,
            out("xmm19") _,
            out("ymm20") _,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m512i, [u64; 8]>(high), reversed);
        assert_eq!(transmute::<__m512i, [u64; 8]>(high_inout), reversed);

        // Narrower types in a `zmm` register.
        let ymm: __m256i = transmute(black_box([9u64, 10, 11, 12]));
        let ymm_out: __m256i;
        asm!(
            "vperm2f128 {o:y}, {i:y}, {i:y}, 1",
            i = in(zmm_reg) ymm,
            o = lateout(zmm_reg) ymm_out,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m256i, [u64; 4]>(ymm_out), [11, 12, 9, 10]);

        let xmm: __m128i = transmute(black_box([13u64, 14]));
        let xmm_out: __m128i;
        asm!(
            "vpshufd {o:x}, {i:x}, 0x4e",
            i = in(zmm_reg) xmm,
            o = lateout(zmm_reg) xmm_out,
            options(nostack, preserves_flags),
        );
        assert_eq!(transmute::<__m128i, [u64; 2]>(xmm_out), [14, 13]);

        let float: f32;
        asm!(
            "vmovaps {o:x}, {i:x}",
            i = in(zmm_reg) 1.5f32,
            o = lateout(zmm_reg) float,
            options(nostack, preserves_flags),
        );
        assert_eq!(float, 1.5);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn kreg_operands() {
    unsafe {
        let shifted: u16;
        asm!(
            "kshiftlw {k}, {k}, 4",
            k = inout(kreg) black_box(0x1234u16) => shifted,
            options(nostack, preserves_flags),
        );
        assert_eq!(shifted, 0x2340);

        let shifted: u8;
        asm!(
            "kshiftlw {k}, {k}, 1",
            k = inout(kreg) black_box(0x81u8) => shifted,
            options(nostack, preserves_flags),
        );
        assert_eq!(shifted, 0x02);

        let copy: u16;
        asm!(
            "kmovw k5, k4",
            "kshiftrw k5, k5, 8",
            in("k4") black_box(0xabcdu16),
            out("k5") copy,
            options(nostack, preserves_flags),
        );
        assert_eq!(copy, 0xab);

        // Clobbers of mask registers and of the AVX-512 only vector registers.
        asm!(
            "kxorw k1, k1, k1",
            "kxnorw k2, k2, k2",
            "vpxord zmm16, zmm16, zmm16",
            "vpxord zmm30, zmm30, zmm30",
            out("k0") _,
            out("k1") _,
            out("k2") _,
            out("k7") _,
            out("zmm16") _,
            out("zmm30") _,
            options(nostack, preserves_flags),
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn kreg_operands_bw() {
    unsafe {
        let shifted: u64;
        asm!(
            "kshiftlq {k}, {k}, 4",
            k = inout(kreg) black_box(0x0123_4567_89ab_cdefu64) => shifted,
            options(nostack, preserves_flags),
        );
        assert_eq!(shifted, 0x1234_5678_9abc_def0);

        let shifted: u32;
        asm!(
            "kshiftld {k}, {k}, 8",
            k = inout(kreg) black_box(0x0123_4567u32) => shifted,
            options(nostack, preserves_flags),
        );
        assert_eq!(shifted, 0x2345_6700);

        let copy: u64;
        asm!(
            "kmovq k6, k3",
            in("k3") black_box(0xfedc_ba98_7654_3210u64),
            out("k6") copy,
            options(nostack, preserves_flags),
        );
        assert_eq!(copy, 0xfedc_ba98_7654_3210);
    }
}

#[cfg(target_arch = "aarch64")]
unsafe fn vreg_operands() {
    unsafe {
        let out: u8;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg) 0x5au8, o = out(vreg) out);
        assert_eq!(out, 0x5a);

        let out: u16;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg) 0x5aa5u16, o = out(vreg) out);
        assert_eq!(out, 0x5aa5);

        let out: u32;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg) 0x1234_5678u32, o = out(vreg) out);
        assert_eq!(out, 0x1234_5678);

        let out: u64;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg) 0x0123_4567_89ab_cdefu64, o = out(vreg) out);
        assert_eq!(out, 0x0123_4567_89ab_cdef);

        let out: f32;
        asm!("fadd {o:s}, {i:s}, {i:s}", i = in(vreg) 1.25f32, o = out(vreg) out);
        assert_eq!(out, 2.5);

        let out: f64;
        asm!("fadd {o:d}, {i:d}, {i:d}", i = in(vreg) 1.25f64, o = out(vreg) out);
        assert_eq!(out, 2.5);

        let out: f16;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg) 1.5f16, o = out(vreg) out);
        assert_eq!(out, 1.5f16);

        let out: f128;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg) 1.5f128, o = out(vreg) out);
        assert_eq!(out, 1.5f128);

        // 64-bit and 128-bit vectors.
        let input: uint8x8_t = transmute(black_box([1u8, 2, 3, 4, 5, 6, 7, 8]));
        let out: uint8x8_t;
        asm!("rev64 {o:v}.8b, {i:v}.8b", i = in(vreg) input, o = out(vreg) out);
        assert_eq!(transmute::<uint8x8_t, [u8; 8]>(out), [8, 7, 6, 5, 4, 3, 2, 1]);

        let input: float32x2_t = transmute(black_box([1.0f32, 2.0]));
        let out: float32x2_t;
        asm!("rev64 {o:v}.2s, {i:v}.2s", i = in(vreg) input, o = out(vreg) out);
        assert_eq!(transmute::<float32x2_t, [f32; 2]>(out), [2.0, 1.0]);

        let input: uint64x2_t = transmute(black_box([1u64, 2]));
        let out: uint64x2_t;
        asm!("ext {o:v}.16b, {i:v}.16b, {i:v}.16b, 8", i = in(vreg) input, o = out(vreg) out);
        assert_eq!(transmute::<uint64x2_t, [u64; 2]>(out), [2, 1]);

        let input: float64x2_t = transmute(black_box([1.0f64, 2.0]));
        let out: float64x2_t;
        asm!("ext {o:v}.16b, {i:v}.16b, {i:v}.16b, 8", i = in(vreg) input, o = out(vreg) out);
        assert_eq!(transmute::<float64x2_t, [f64; 2]>(out), [2.0, 1.0]);

        // `inout` and explicit registers.
        let mut x = black_box(0x0102_0304_0506_0708u64);
        asm!("rev64 {x:v}.8b, {x:v}.8b", x = inout(vreg) x);
        assert_eq!(x, 0x0807_0605_0403_0201);

        let input: uint64x2_t = transmute(black_box([3u64, 4]));
        let out: uint64x2_t;
        asm!("ext v31.16b, v3.16b, v3.16b, 8", in("v3") input, out("v31") out);
        assert_eq!(transmute::<uint64x2_t, [u64; 2]>(out), [4, 3]);

        let out: u32;
        asm!("mov v2.16b, v9.16b", in("v9") 0xdead_beefu32, out("v2") out);
        assert_eq!(out, 0xdead_beef);

        let out: u16;
        asm!("mov {o:v}.16b, {i:v}.16b", i = in(vreg_low16) 0xbeefu16, o = out(vreg_low16) out);
        assert_eq!(out, 0xbeef);

        let out: uint64x2_t;
        asm!(
            "ext {o:v}.16b, {i:v}.16b, {i:v}.16b, 8",
            i = in(vreg_low16) input,
            o = out(vreg_low16) out,
        );
        assert_eq!(transmute::<uint64x2_t, [u64; 2]>(out), [4, 3]);

        // Clobbers of callee saved vector registers.
        asm!("movi v8.16b, 0", "movi v15.16b, 0", out("v8") _, out("v15") _);
    }
}
