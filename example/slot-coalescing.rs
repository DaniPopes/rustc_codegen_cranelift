// Locals whose address is never taken share a stack slot while their values are not live at the
// same time, so a chain like `let b = a; let c = b;` needs one slot instead of three. Every frame
// of the recursion below moves a buffer of `N` bytes through eight locals, with a recursive call
// while the last one is live. Without coalescing it needs about 3 MiB of stack and overflows the
// 1 MiB thread stack; with it it needs about 0.5 MiB. The checks make sure values survive wherever
// two locals do overlap.

use std::hint::black_box;
use std::sync::Mutex;
use std::thread;

const N: usize = 4096;
const DEPTH: usize = 100;
const STACK_SIZE: usize = 1 << 20;

fn pattern(seed: u8, i: usize) -> u8 {
    seed.wrapping_mul(31).wrapping_add(i as u8)
}

struct Big([u8; N]);

impl Big {
    #[inline(never)]
    fn new(seed: u8) -> Big {
        let mut big = Big([0; N]);
        for (i, b) in big.0.iter_mut().enumerate() {
            *b = pattern(seed, i);
        }
        big
    }

    #[inline(never)]
    fn check(&self, seed: u8) {
        for (i, b) in self.0.iter().enumerate() {
            assert_eq!(*b, pattern(seed, i), "seed {seed} byte {i}");
        }
    }
}

#[inline(never)]
fn consume(big: Big, seed: u8) {
    big.check(seed);
}

/// Moves a value into a new local, in a way the MIR optimizations leave alone.
macro_rules! hop {
    ($seed:expr; $($from:ident => $to:ident)*) => {$(
        let $to = if black_box(true) { $from } else { Big::new($seed ^ 0xff) };
    )*};
}

#[inline(never)]
fn recurse(depth: usize, seed: u8) -> usize {
    if depth == 0 {
        return 0;
    }
    let v0 = Big::new(seed);
    hop!(seed; v0 => v1 v1 => v2 v2 => v3 v3 => v4 v4 => v5 v5 => v6 v6 => v7 v7 => v8);
    let total = 1 + recurse(depth - 1, seed.wrapping_add(1));
    consume(v8, seed);
    total
}

/// Logs the order in which guards are dropped, and checks their contents when they are.
static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct Guard {
    big: Big,
    seed: u8,
}

impl Guard {
    fn new(seed: u8) -> Guard {
        Guard { big: Big::new(seed), seed }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.big.check(self.seed);
        LOG.lock().unwrap().push(self.seed);
    }
}

fn take_log() -> Vec<u8> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

/// A value that is moved through several locals in a loop, which are overwritten in the next
/// iteration.
#[inline(never)]
fn move_in_loop(seed: u8) {
    let mut cur = Big::new(seed);
    let mut cur_seed = seed;
    for i in 0..5u8 {
        cur.check(cur_seed);
        let a = cur;
        let b = Big::new(seed.wrapping_add(100 + i));
        let c = a;
        // `b` is live while `c` is written.
        b.check(seed.wrapping_add(100 + i));
        if black_box(i % 2 == 0) {
            cur = c;
        } else {
            cur = b;
            cur_seed = seed.wrapping_add(100 + i);
        }
    }
    consume(cur, cur_seed);
}

/// A value that is moved on one branch and still used, and dropped, on the other.
#[inline(never)]
fn move_conditionally(take: bool, seed: u8) {
    let a = Guard::new(seed);
    let other = Guard::new(seed.wrapping_add(1));
    if black_box(take) {
        let b = a;
        let c = b;
        c.big.check(seed);
        let d = Guard::new(seed.wrapping_add(2));
        d.big.check(seed.wrapping_add(2));
        drop(c);
    } else {
        a.big.check(seed);
        let d = Guard::new(seed.wrapping_add(3));
        d.big.check(seed.wrapping_add(3));
    }
    other.big.check(seed.wrapping_add(1));
}

#[inline(never)]
fn drop_order() {
    take_log();
    {
        let a = Guard::new(1);
        let b = a;
        let c = Guard::new(2);
        let d = b;
        let e = Guard::new(3);
        let f = c;
        drop(e);
        let _ = (d, f);
    }
    assert_eq!(take_log(), [3, 1, 2]);

    move_conditionally(true, 10);
    assert_eq!(take_log(), [10, 12, 11]);
    move_conditionally(false, 20);
    assert_eq!(take_log(), [23, 21, 20]);
}

/// A panic is caught while locals that share a slot are live, and the cleanup path drops them.
#[cfg(panic = "unwind")]
#[inline(never)]
fn catch_panic(seed: u8) {
    take_log();
    let live = Guard::new(seed);
    let live = live;
    let result = std::panic::catch_unwind(|| {
        let a = Guard::new(seed.wrapping_add(1));
        let b = a;
        let c = Guard::new(seed.wrapping_add(2));
        let d = b;
        if black_box(true) {
            std::panic::resume_unwind(Box::new(()));
        }
        drop((c, d));
    });
    assert!(result.is_err());
    live.big.check(seed);
    assert_eq!(take_log(), [seed.wrapping_add(1), seed.wrapping_add(2)]);
    drop(live);
    assert_eq!(take_log(), [seed]);
}

fn main() {
    let total = thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(|| recurse(black_box(DEPTH), 1))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(total, DEPTH);

    move_in_loop(7);
    drop_order();
    #[cfg(panic = "unwind")]
    catch_panic(50);
}
