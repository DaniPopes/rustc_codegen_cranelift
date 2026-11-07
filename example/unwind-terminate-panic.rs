// Panicking in an `extern "C"` function on a thread nothing catches above must abort with
// "panic in a function that cannot unwind", as with LLVM.

use std::ffi::c_void;

unsafe extern "C" {
    fn pthread_create(
        thread: *mut usize,
        attr: *const c_void,
        start: extern "C" fn(*mut c_void) -> *mut c_void,
        arg: *mut c_void,
    ) -> i32;
    fn pthread_join(thread: usize, retval: *mut *mut c_void) -> i32;
}

extern "C" fn callback(_: *mut c_void) -> *mut c_void {
    panic!("in raw thread");
}

fn main() {
    unsafe {
        let mut thread = 0;
        assert_eq!(
            pthread_create(&mut thread, std::ptr::null(), callback, std::ptr::null_mut()),
            0
        );
        pthread_join(thread, std::ptr::null_mut());
    }
}
