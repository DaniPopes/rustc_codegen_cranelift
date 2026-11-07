// Locals whose storage is never live at the same time share a stack slot, so the frame of a
// function with many large locals in disjoint scopes stays small. Every frame of the recursion
// below has 23 buffers of `N` bytes, but at most three are live at once. Without slot sharing the
// recursion needs about 3 MiB of stack and overflows the 1 MiB thread stack; with sharing it needs
// about 0.4 MiB. The checks make sure values survive wherever two locals do overlap.

use std::hint::black_box;
use std::thread;

const N: usize = 4096;
const DEPTH: usize = 32;
const STACK_SIZE: usize = 1 << 20;

fn pattern(seed: u8, i: usize) -> u8 {
    seed.wrapping_mul(31).wrapping_add(i as u8)
}

#[inline(never)]
fn fill(buf: &mut [u8; N], seed: u8) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = pattern(seed, i);
    }
}

#[inline(never)]
fn check(buf: &[u8; N], seed: u8) {
    for (i, b) in buf.iter().enumerate() {
        assert_eq!(*b, pattern(seed, i), "seed {seed} byte {i}");
    }
}

struct Guard {
    buf: [u8; N],
    seed: u8,
}

impl Guard {
    fn new(seed: u8) -> Guard {
        let mut buf = [0; N];
        fill(&mut buf, seed);
        Guard { buf, seed }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        check(&self.buf, self.seed);
    }
}

macro_rules! scopes {
    ($seed:expr; $($n:literal)*) => {$({
        let mut buf = [0; N];
        fill(&mut buf, $seed.wrapping_add($n));
        check(black_box(&buf), $seed.wrapping_add($n));
    })*};
}

#[inline(never)]
fn recurse(depth: usize, seed: u8) -> usize {
    if depth == 0 {
        return 0;
    }
    let mut total = 1;

    scopes!(seed; 1 2 3 4 5 6 7 8);

    // Two buffers live at the same time, with a recursive call in between.
    {
        let mut a = [0; N];
        let mut b = [0; N];
        fill(&mut a, seed.wrapping_add(10));
        fill(&mut b, seed.wrapping_add(11));
        total += recurse(depth - 1, seed.wrapping_add(1));
        check(&a, seed.wrapping_add(10));
        check(&b, seed.wrapping_add(11));
    }

    scopes!(seed; 12 13 14 15);

    // A buffer that is live across a loop whose body reuses a slot in every iteration.
    {
        let mut acc = [0; N];
        fill(&mut acc, seed.wrapping_add(20));
        for i in 0..4u8 {
            let mut tmp = [0; N];
            fill(&mut tmp, seed.wrapping_add(21 + i));
            check(black_box(&tmp), seed.wrapping_add(21 + i));
            check(&acc, seed.wrapping_add(20));
        }
        check(&acc, seed.wrapping_add(20));
    }

    scopes!(seed; 30 31 32 33);

    // A panic caught while a buffer is live: the cleanup path drops `Guard`, which checks its
    // contents, and buffers of earlier scopes may share a slot with it.
    #[cfg(panic = "unwind")]
    {
        let mut live = [0; N];
        fill(&mut live, seed.wrapping_add(40));
        let result = std::panic::catch_unwind(|| {
            scopes!(seed; 41 42);
            let _guard = Guard::new(seed.wrapping_add(43));
            let mut tmp = [0; N];
            fill(&mut tmp, seed.wrapping_add(44));
            if black_box(true) {
                std::panic::resume_unwind(Box::new(()));
            }
            check(&tmp, seed.wrapping_add(44));
        });
        assert!(result.is_err());
        check(&live, seed.wrapping_add(40));
    }
    #[cfg(not(panic = "unwind"))]
    drop(Guard::new(seed.wrapping_add(43)));

    scopes!(seed; 50 51);

    total
}

fn main() {
    let total = thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(|| recurse(black_box(DEPTH), 1))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(total, DEPTH);
}
