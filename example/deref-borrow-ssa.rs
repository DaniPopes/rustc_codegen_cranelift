// `borrow_field` only borrows memory behind `self`, so `self` has to stay in a register. The CLIF
// of `borrow_field` must not have a stack slot. See `aot.deref-borrow-ssa` in
// `build_system/tests.rs`.

#![crate_type = "lib"]

pub struct Pair {
    pub first: u64,
    pub second: u64,
}

impl Pair {
    #[unsafe(no_mangle)]
    pub fn borrow_field(&self) -> &u64 {
        &self.second
    }
}
