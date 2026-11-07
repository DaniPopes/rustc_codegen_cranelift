// Constants with equal contents share one copy: within a codegen unit, the backend defines them
// once, and between codegen units, the linker merges equal C strings.

mod a {
    #[inline(never)]
    pub fn c_string() -> *const u8 {
        c"mergeable constants test string".as_ptr().cast()
    }

    #[inline(never)]
    pub fn first_table() -> *const [u32; 6] {
        &[11, 22, 33, 44, 55, 66]
    }

    #[inline(never)]
    pub fn second_table() -> *const [u32; 6] {
        &[11, 22, 33, 44, 55, 66]
    }
}

mod b {
    #[inline(never)]
    pub fn c_string() -> *const u8 {
        c"mergeable constants test string".as_ptr().cast()
    }
}

fn main() {
    assert_eq!(a::first_table(), a::second_table());

    // COFF linkers do not merge strings.
    if !cfg!(windows) {
        assert_eq!(a::c_string(), b::c_string());
    }
}
