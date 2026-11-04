// The calls to `inlined_by_attribute` have to be gone from the CLIF of `inline_check_caller`, the
// call to `never_inlined` has to stay. The call to `small_leaf` is only inlined when optimizing.
// See `aot.inline-calls` in `build_system/tests.rs`.

#![crate_type = "lib"]

#[inline(always)]
pub fn inlined_by_attribute(x: u64) -> u64 {
    x.wrapping_mul(3)
}

#[inline]
pub fn small_leaf(x: u64) -> u64 {
    x ^ 0x55
}

#[inline(never)]
pub fn never_inlined(x: u64) -> u64 {
    x.wrapping_add(5)
}

#[unsafe(no_mangle)]
pub fn inline_check_caller(x: u64) -> u64 {
    never_inlined(small_leaf(inlined_by_attribute(inlined_by_attribute(x))))
}
