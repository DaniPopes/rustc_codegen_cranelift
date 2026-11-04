// Checks that `cfg(target_feature)` reflects `-Ctarget-cpu` and `-Ctarget-feature`. Each
// `--cfg` selects the flags the test is compiled with.

#![allow(unexpected_cfgs)]

#[cfg(all(target_arch = "x86_64", x86_64_v3))]
const _: () = {
    assert!(cfg!(target_feature = "avx2"));
    assert!(cfg!(target_feature = "bmi2"));
    assert!(cfg!(target_feature = "fma"));
    assert!(cfg!(target_feature = "sse4.2"));
    assert!(!cfg!(target_feature = "avx512f"));
};

// `avx2` implies `avx` and `sse4.2`.
#[cfg(all(target_arch = "x86_64", plus_avx2))]
const _: () = {
    assert!(cfg!(target_feature = "avx2"));
    assert!(cfg!(target_feature = "avx"));
    assert!(cfg!(target_feature = "sse4.2"));
    assert!(!cfg!(target_feature = "bmi2"));
};

#[cfg(all(target_arch = "x86_64", x86_64_v3_minus_avx2))]
const _: () = {
    assert!(!cfg!(target_feature = "avx2"));
    assert!(cfg!(target_feature = "avx"));
    assert!(cfg!(target_feature = "bmi2"));
};

// Cranelift doesn't know this cpu.
#[cfg(all(target_arch = "x86_64", znver5))]
const _: () = {
    assert!(cfg!(target_feature = "avx512f"));
    assert!(cfg!(target_feature = "avx512vl"));
};

#[cfg(all(target_arch = "aarch64", apple_m1))]
const _: () = {
    assert!(cfg!(target_feature = "neon"));
    assert!(cfg!(target_feature = "lse"));
    assert!(cfg!(target_feature = "aes"));
};

fn main() {}
