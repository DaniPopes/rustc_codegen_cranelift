// Tests for the stack space used by inlined calls. They only hold for a backend that shares the
// stack slots of inlined calls, so they are not run with LLVM.

use std::hint::black_box;
use std::marker::PhantomData;

// A chain of `#[inline(always)]` functions inside a recursive cycle. Every call of `Rec::call` goes
// through 64 wrappers. Without inlining each wrapper has a frame of its own.

trait Chain {
    fn call(n: u64) -> u64;
}

struct Wrap<T>(PhantomData<T>);

impl<T: Chain> Chain for Wrap<T> {
    #[inline(always)]
    fn call(n: u64) -> u64 {
        T::call(n) + 1
    }
}

type Wrap4<T> = Wrap<Wrap<Wrap<Wrap<T>>>>;
type Wrap16<T> = Wrap4<Wrap4<Wrap4<Wrap4<T>>>>;
type Wrap64<T> = Wrap16<Wrap16<Wrap16<Wrap16<T>>>>;

struct Rec;

impl Chain for Rec {
    fn call(n: u64) -> u64 {
        if n == 0 { 0 } else { Wrap64::<Rec>::call(n - 1) }
    }
}

const CHAIN_DEPTH: u64 = 1000;
const CHAIN_STACK_SIZE: usize = 256 * 1024;

fn run_with_stack_size<R: Send + 'static>(
    stack_size: usize,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    std::thread::Builder::new().stack_size(stack_size).spawn(f).unwrap().join().unwrap()
}

fn inline_always_chain_in_recursion() {
    let result = run_with_stack_size(CHAIN_STACK_SIZE, || Wrap64::<Rec>::call(CHAIN_DEPTH));
    assert_eq!(result, 64 * (CHAIN_DEPTH + 1));
}

// Callees called in sequence from the same function. Their stack slots are never live at the same
// time, so they share the space. Without sharing every level of the recursion needs 16 * 2 KiB.

macro_rules! buffer_users {
    ($($name:ident),*) => {
        $(
            #[inline(always)]
            fn $name(x: u64) -> u64 {
                let mut buffer = [0u8; 2048];
                buffer[x as usize % 2048] = x as u8;
                black_box(&mut buffer);
                buffer.iter().map(|&b| u64::from(b)).sum()
            }
        )*

        fn recurse_after_buffer_users(n: u64) -> u64 {
            if n == 0 {
                return 0;
            }
            let mut sum = 0;
            $(sum += $name(n);)*
            sum + black_box(recurse_after_buffer_users)(n - 1)
        }
    };
}

buffer_users!(u0, u1, u2, u3, u4, u5, u6, u7, u8, u9, u10, u11, u12, u13, u14, u15);

const BUFFER_DEPTH: u64 = 100;
const BUFFER_STACK_SIZE: usize = 1024 * 1024;

fn inline_always_sibling_stack_slots_share_space() {
    let result =
        run_with_stack_size(BUFFER_STACK_SIZE, || recurse_after_buffer_users(BUFFER_DEPTH));
    let expected = (1..=BUFFER_DEPTH).map(|n| 16 * u64::from(n as u8)).sum::<u64>();
    assert_eq!(result, expected);
}

fn main() {
    inline_always_chain_in_recursion();
    inline_always_sibling_stack_slots_share_space();
}
