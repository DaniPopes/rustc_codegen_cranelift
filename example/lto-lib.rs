// Upstream crate of the `aot.lto` test.

use std::arch::global_asm;

#[cfg(target_vendor = "apple")]
global_asm!(".data", ".globl _lto_lib_asm_value", "_lto_lib_asm_value:", ".long 40");
#[cfg(not(target_vendor = "apple"))]
global_asm!(".data", ".globl lto_lib_asm_value", "lto_lib_asm_value:", ".long 40");

unsafe extern "C" {
    static lto_lib_asm_value: u32;
}

#[inline(never)]
pub fn answer() -> u32 {
    unsafe { lto_lib_asm_value + 2 }
}

pub fn greeting(name: &str) -> String {
    format!("Hello, {name}!")
}
