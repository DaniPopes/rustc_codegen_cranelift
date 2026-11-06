// Calls into an upstream crate and into std with LTO enabled. Both crates also contain global asm.

extern crate lto_lib;

use std::arch::global_asm;

#[cfg(target_vendor = "apple")]
global_asm!(".data", ".globl _lto_bin_asm_value", "_lto_bin_asm_value:", ".long 1");
#[cfg(not(target_vendor = "apple"))]
global_asm!(".data", ".globl lto_bin_asm_value", "lto_bin_asm_value:", ".long 1");

unsafe extern "C" {
    static lto_bin_asm_value: u32;
}

fn main() {
    assert_eq!(lto_lib::answer() + unsafe { lto_bin_asm_value }, 43);
    assert_eq!(lto_lib::greeting("LTO"), "Hello, LTO!");
    assert_eq!((0..10u64).collect::<Vec<_>>().iter().sum::<u64>(), 45);
}
