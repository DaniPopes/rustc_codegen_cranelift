// A forced unwind (`pthread_exit`) must pass through the terminate landing pad of an
// `extern "C"` function instead of aborting, as with LLVM.

#![no_main]

unsafe extern "C-unwind" {
    fn pthread_exit(value: *mut core::ffi::c_void) -> !;
}

unsafe extern "C" fn call_pthread_exit() {
    unsafe { pthread_exit(core::ptr::null_mut()) };
}

#[unsafe(no_mangle)]
unsafe extern "C-unwind" fn main(_argc: core::ffi::c_int, _argv: *mut *mut core::ffi::c_char) {
    unsafe { call_pthread_exit() };
}
