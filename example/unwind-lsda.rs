// Compiled to an object, whose exception tables the build system compares with LLVM's.

#![crate_type = "lib"]

unsafe extern "Rust" {
    fn may_unwind();
}

pub struct Guard;

impl Drop for Guard {
    #[inline(never)]
    fn drop(&mut self) {
        unsafe { may_unwind() }
    }
}

// Without landing pads there is no LSDA and the FDE uses a CIE without personality function.
#[unsafe(no_mangle)]
pub fn no_landing_pads() {
    unsafe {
        may_unwind();
        may_unwind();
    }
}

// All calls share the cleanup of the guard.
#[unsafe(no_mangle)]
pub fn shared_cleanup() {
    let _guard = Guard;
    unsafe {
        may_unwind();
        may_unwind();
        may_unwind();
        may_unwind();
        may_unwind();
    }
}
