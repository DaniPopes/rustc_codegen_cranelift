// Tests that the `\x01` prefix of a symbol name, which makes LLVM use the name verbatim, is honored.

use std::ffi::{CStr, c_char};

macro_rules! externs {
    ($strlen:literal, $function:literal, $data:literal) => {
        unsafe extern "C" {
            #[link_name = $strlen]
            fn imported_strlen(s: *const c_char) -> usize;
            #[link_name = $function]
            fn imported_function() -> u32;
            #[link_name = $data]
            static IMPORTED_DATA: u32;
        }
    };
}

// Targets that add a global prefix to symbol names need it spelled out in a verbatim name.
#[cfg(any(target_vendor = "apple", all(windows, target_arch = "x86")))]
externs!("\u{1}_strlen", "\u{1}_verbatim_function", "\u{1}_VERBATIM_DATA");
#[cfg(not(any(target_vendor = "apple", all(windows, target_arch = "x86"))))]
externs!("\u{1}strlen", "\u{1}verbatim_function", "\u{1}VERBATIM_DATA");

unsafe extern "C" {
    #[link_name = "strlen"]
    fn plain_strlen(s: *const c_char) -> usize;
}

#[unsafe(no_mangle)]
extern "C" fn verbatim_function() -> u32 {
    42
}

#[unsafe(no_mangle)]
static VERBATIM_DATA: u32 = 1337;

fn main() {
    let s: &CStr = c"abc";
    unsafe {
        assert_eq!(imported_strlen(s.as_ptr()), 3);
        assert_eq!(plain_strlen(s.as_ptr()), 3);
        assert_eq!(imported_function(), 42);
        assert_eq!(IMPORTED_DATA, 1337);
    }
}
