// Compiled at opt-level 0, so by Cranelift. Calls into crates compiled by Cranelift and by LLVM.

use std::sync::atomic::Ordering;

use llvm_delegate_clif::{CLIF_STATIC, CLIF_TLS, DROPS};
use llvm_delegate_llvm::{LLVM_STATIC, LLVM_TLS};

fn main() {
    assert_eq!(llvm_delegate_llvm::llvm_add(1, 2), 103);
    assert_eq!(llvm_delegate_llvm::llvm_inline(2), 7);
    assert_eq!(llvm_delegate_clif::clif_inline(2), 6);

    let location = llvm_delegate_llvm::llvm_location();
    assert_eq!((location.file(), location.line()), (file!(), line!() - 1));
    let location = llvm_delegate_clif::clif_location();
    assert_eq!((location.file(), location.line()), (file!(), line!() - 1));
    let location = llvm_delegate_llvm::llvm_clif_location();
    assert_eq!(location.file(), llvm_delegate_llvm::FILE);

    assert_eq!(llvm_delegate_clif::clif_shape(4).area(), 16);
    assert_eq!(llvm_delegate_llvm::llvm_shape(4).area(), 8);
    assert_eq!(llvm_delegate_llvm::llvm_shape_area(&*llvm_delegate_clif::clif_shape(3)), 9);
    assert_eq!(llvm_delegate_llvm::llvm_shape_area(&*llvm_delegate_llvm::llvm_shape(3)), 4);

    assert_eq!(llvm_delegate_clif::clif_generic("a"), "clif<a>");
    assert_eq!(llvm_delegate_llvm::llvm_generic("a"), "llvm<a>");
    assert_eq!(llvm_delegate_llvm::llvm_clif_generic(), "clif<7>");
    assert_eq!(llvm_delegate_clif::clif_generic(7u8), "clif<7>");

    assert_eq!(llvm_delegate_llvm::llvm_touch_statics(), (1, 1, 11, 21));
    assert_eq!(llvm_delegate_llvm::llvm_touch_statics(), (2, 2, 12, 22));
    assert_eq!((CLIF_STATIC.load(Ordering::SeqCst), LLVM_STATIC.load(Ordering::SeqCst)), (2, 2));
    assert_eq!((CLIF_TLS.get(), LLVM_TLS.get()), (12, 22));
    std::thread::spawn(|| {
        assert_eq!(llvm_delegate_llvm::llvm_touch_statics(), (3, 3, 11, 21));
    })
    .join()
    .unwrap();

    assert_eq!(llvm_delegate_llvm::llvm_call(&|| llvm_delegate_clif::clif_add(1, 1)), 4);
    assert_eq!(llvm_delegate_clif::clif_call(&|| llvm_delegate_llvm::llvm_add(1, 1)), 103);
    assert_eq!(llvm_delegate_llvm::llvm_call_clif(&|| llvm_delegate_llvm::llvm_add(1, 1)), 105);
    assert_eq!(DROPS.swap(0, Ordering::SeqCst), 3);

    #[cfg(unwind)]
    unwinding();
}

#[cfg(unwind)]
fn unwinding() {
    use std::panic::catch_unwind;

    std::panic::set_hook(Box::new(|_| {}));

    // Cranelift catches a panic raised by LLVM code.
    assert!(catch_unwind(|| llvm_delegate_llvm::llvm_panic()).is_err());
    assert_eq!(DROPS.swap(0, Ordering::SeqCst), 1);

    // The panic of a Cranelift closure unwinds through an LLVM frame.
    assert!(
        catch_unwind(|| llvm_delegate_llvm::llvm_call(&|| llvm_delegate_clif::clif_panic()))
            .is_err()
    );
    assert_eq!(DROPS.swap(0, Ordering::SeqCst), 2);

    // The panic of an LLVM closure unwinds through a Cranelift frame.
    assert!(
        catch_unwind(|| llvm_delegate_clif::clif_call(&|| llvm_delegate_llvm::llvm_panic()))
            .is_err()
    );
    assert_eq!(DROPS.swap(0, Ordering::SeqCst), 2);

    // LLVM catches a panic raised by Cranelift code.
    assert!(llvm_delegate_llvm::llvm_catches_clif_panic());
    assert_eq!(DROPS.swap(0, Ordering::SeqCst), 2);

    // A Cranelift panic unwinds through LLVM and Cranelift frames to Cranelift.
    assert!(
        catch_unwind(|| llvm_delegate_llvm::llvm_call_clif(&|| llvm_delegate_clif::clif_panic()))
            .is_err()
    );
    assert_eq!(DROPS.swap(0, Ordering::SeqCst), 2);
}
