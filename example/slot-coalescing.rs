// Locals whose address is never taken share a stack slot while their values are not live at the
// same time, so a chain like `let b = a; let c = b;` needs one slot instead of three. Every frame
// of the recursion in `recurse` moves a buffer of `N` bytes through eight locals, with a recursive
// call while the last one is live. Without coalescing it needs about 3 MiB of stack and
// overflows the 1 MiB thread stack; with it it needs about 0.45 MiB. `recurse_wrapped` moves a
// buffer into and out of enum variants, which are placed at the field, so each frame needs one
// buffer instead of two. The checks make sure values survive wherever two locals do overlap.

#![feature(custom_mir, core_intrinsics)]

use std::hint::black_box;
use std::intrinsics::mir::*;
use std::num::NonZeroU8;
use std::ops::ControlFlow;
use std::sync::Mutex;
use std::thread;

const N: usize = 4096;
const DEPTH: usize = 100;
const STACK_SIZE: usize = 1 << 20;
const WRAPPED_DEPTH: usize = 160;

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

/// Like `recurse`, but moves the buffer into and out of enum variants, so each level needs the
/// space of one buffer, not two.
#[inline(never)]
fn recurse_wrapped(depth: usize, seed: u8) -> usize {
    if depth == 0 {
        return 0;
    }
    let v = Big::new(seed);
    let r: Result<Big, u8> = if black_box(true) { Ok(v) } else { Err(0) };
    let o = Some(r);
    let c: ControlFlow<u8, Option<Result<Big, u8>>> = ControlFlow::Continue(o);
    let w = match c {
        ControlFlow::Continue(w) => w,
        ControlFlow::Break(_) => None,
    };
    let u = match w {
        Some(u) => u,
        None => Err(1),
    };
    let v = match u {
        Ok(v) => v,
        Err(_) => Big::new(seed ^ 0xff),
    };
    let total = 1 + recurse_wrapped(depth - 1, seed.wrapping_add(1));
    consume(v, seed);
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

/// Values moved into and out of enum variants, including ones whose tag is a niche in a field.
struct Held {
    big: Big,
    flag: bool,
}

enum Slot {
    Full(Held),
    Other(Small),
    Empty,
}

struct Small([u8; 64]);

#[inline(never)]
fn wrapped_values(seed: u8) {
    let held = Held { big: Big::new(seed), flag: true };
    let slot = if black_box(true) { Slot::Full(held) } else { Slot::Empty };
    let option = Some(slot);
    let Some(Slot::Full(held)) = option else { panic!() };
    held.big.check(seed);
    assert!(held.flag);

    // The tag of `Other` is a niche in the bytes of `Full`, which the field of `Other` must not
    // cover.
    let mut small = Small([0; 64]);
    for (i, b) in small.0.iter_mut().enumerate() {
        *b = pattern(seed, i);
    }
    let slot = if black_box(true) { Slot::Other(small) } else { Slot::Empty };
    let Slot::Other(small) = slot else { panic!() };
    for (i, b) in small.0.iter().enumerate() {
        assert_eq!(*b, pattern(seed, i));
    }

    // `Some` is the variant without a tag, and its field holds the niche of `None`.
    let big = Big::new(seed);
    let nonzero = black_box(NonZeroU8::new(3).unwrap());
    let niche = if black_box(true) { Some((big, nonzero)) } else { None };
    let Some((big, nonzero)) = niche else { panic!() };
    big.check(seed);
    assert_eq!(nonzero.get(), 3);
    let none: Option<(Big, NonZeroU8)> = if black_box(false) { Some((big, nonzero)) } else { None };
    assert!(none.is_none());
}

#[inline(never)]
fn wrapped_drop_order() {
    take_log();
    {
        let a = Guard::new(1);
        let b = Some(a);
        let c = Guard::new(2);
        let d = Ok::<_, ()>(c);
        let Some(a) = b else { panic!() };
        let e = Guard::new(3);
        let Ok(c) = d else { panic!() };
        drop(e);
        let _ = (c, a);
    }
    assert_eq!(take_log(), [3, 2, 1]);

    {
        let a = Guard::new(4);
        let b = if black_box(false) { ControlFlow::Continue(a) } else { ControlFlow::Break(()) };
        let c = Guard::new(5);
        let ControlFlow::Break(()) = b else { panic!() };
        drop(c);
    }
    assert_eq!(take_log(), [5, 4]);
}

/// A value that is copied to a second local while the first is live, and then written through
/// the second. The two must not share a slot, whichever way the second is written.
#[derive(Clone, Copy)]
#[repr(C)]
struct Cp {
    head: u64,
    rest: [u64; 31],
}

#[derive(Clone, Copy)]
#[repr(C)]
struct NzCp(Cp, NonZeroU8);

struct Dg([u64; 8]);

impl Drop for Dg {
    fn drop(&mut self) {
        self.0 = [0xdead; 8];
    }
}

#[inline(never)]
fn cp(seed: u64) -> Cp {
    let mut rest = [0; 31];
    for (i, r) in rest.iter_mut().enumerate() {
        *r = seed.wrapping_mul(31).wrapping_add(i as u64);
    }
    Cp { head: seed, rest }
}

#[inline(never)]
fn cp_sum(c: Cp) -> u64 {
    c.rest.iter().fold(c.head, |a, &x| a.wrapping_mul(31).wrapping_add(x))
}

#[inline(never)]
fn nz_sum(c: NzCp) -> u64 {
    cp_sum(c.0).wrapping_mul(3).wrapping_add(c.1.get() as u64)
}

#[inline(never)]
fn dg(seed: u64) -> Dg {
    Dg([seed; 8])
}

#[inline(never)]
fn nz_cp(c: Cp) -> NzCp {
    NzCp(c, NonZeroU8::new(9).unwrap())
}

#[inline(never)]
fn dg_sum(d: Dg) -> u64 {
    d.0.iter().fold(0, |a, &x| a.wrapping_mul(31).wrapping_add(x))
}

/// Overwrites its argument, which the caller may pass by reference without a copy.
#[inline(never)]
fn cp_mutate(mut c: Cp) -> Cp {
    c.rest[0] ^= 0x1234;
    c.head += 1;
    c
}

#[inline(never)]
fn nz_mutate(mut c: NzCp) -> NzCp {
    c.1 = NonZeroU8::new(77).unwrap();
    c.0.rest[1] ^= 5;
    c
}

#[inline(never)]
fn src_write_field(seed: u64) -> u64 {
    let a = cp(seed);
    let mut b = a;
    b.head = 1;
    b.rest[3] = 2;
    cp_sum(a).wrapping_mul(3).wrapping_add(cp_sum(b))
}

#[inline(never)]
fn src_callee_mutates(seed: u64) -> u64 {
    let a = cp(seed);
    let b = a;
    let y = cp_mutate(b);
    cp_sum(a).wrapping_mul(3).wrapping_add(cp_sum(y))
}

#[inline(never)]
fn src_whole_rewrite(seed: u64) -> u64 {
    let a = cp(seed);
    let mut b = a;
    b = cp(seed + 1);
    cp_sum(a).wrapping_mul(3).wrapping_add(cp_sum(b))
}

#[inline(never)]
fn expected_write_field(seed: u64) -> u64 {
    let (a, mut b) = (cp(seed), cp(seed));
    b.head = 1;
    b.rest[3] = 2;
    cp_sum(a).wrapping_mul(3).wrapping_add(cp_sum(b))
}

#[inline(never)]
fn expected_callee_mutates(seed: u64) -> u64 {
    let (a, b) = (cp(seed), cp(seed));
    cp_sum(a).wrapping_mul(3).wrapping_add(cp_sum(cp_mutate(b)))
}

#[inline(never)]
fn expected_whole_rewrite(seed: u64) -> u64 {
    cp_sum(cp(seed)).wrapping_mul(3).wrapping_add(cp_sum(cp(seed + 1)))
}

#[inline(never)]
fn expected_mir_write_field(seed: u64) -> u64 {
    let (a, mut b) = (cp(seed), cp(seed));
    b.head = 1;
    cp_sum(a).wrapping_mul(3).wrapping_add(cp_sum(b))
}

#[inline(never)]
fn expected_field_callee_mutates(seed: u64) -> u64 {
    let nz = || NzCp(cp(seed), NonZeroU8::new(9).unwrap());
    nz_sum(nz_mutate(nz())).wrapping_mul(3).wrapping_add(nz_sum(nz()))
}

#[inline(never)]
fn expected_niche(seed: u64) -> u64 {
    nz_sum(NzCp(cp(seed), NonZeroU8::new(9).unwrap()))
}

/// The same with the MIR written out, as the MIR optimizations leave it: `b = copy a; b.head = 1;
/// use(copy a)`.
#[custom_mir(dialect = "runtime", phase = "optimized")]
#[inline(never)]
fn mir_write_field(seed: u64) -> u64 {
    mir! {
        let a: Cp;
        let b: Cp;
        let r1: u64;
        let r2: u64;
        let t: u64;
        {
            Call(a = cp(seed), ReturnTo(bb1), UnwindUnreachable())
        }
        bb1 = {
            b = a;
            b.head = 1;
            Call(r1 = cp_sum(a), ReturnTo(bb2), UnwindUnreachable())
        }
        bb2 = {
            Call(r2 = cp_sum(b), ReturnTo(bb3), UnwindUnreachable())
        }
        bb3 = {
            t = r1 * 3;
            RET = t + r2;
            Return()
        }
    }
}

/// `b = copy a; y = callee(move b); use(copy a)`, where the callee writes the slot of `b`.
#[custom_mir(dialect = "runtime", phase = "optimized")]
#[inline(never)]
fn mir_callee_mutates(seed: u64) -> u64 {
    mir! {
        let a: Cp;
        let b: Cp;
        let y: Cp;
        let r1: u64;
        let r2: u64;
        let t: u64;
        {
            Call(a = cp(seed), ReturnTo(bb1), UnwindUnreachable())
        }
        bb1 = {
            b = a;
            Call(y = cp_mutate(Move(b)), ReturnTo(bb2), UnwindUnreachable())
        }
        bb2 = {
            Call(r1 = cp_sum(a), ReturnTo(bb3), UnwindUnreachable())
        }
        bb3 = {
            Call(r2 = cp_sum(y), ReturnTo(bb4), UnwindUnreachable())
        }
        bb4 = {
            t = r1 * 3;
            RET = t + r2;
            Return()
        }
    }
}

/// `b = copy a; b = call(); use(copy a)`, where the call writes the slot of `b`.
#[custom_mir(dialect = "runtime", phase = "optimized")]
#[inline(never)]
fn mir_call_destination(seed: u64) -> u64 {
    mir! {
        let a: Cp;
        let b: Cp;
        let r1: u64;
        let r2: u64;
        let t: u64;
        {
            Call(a = cp(seed), ReturnTo(bb1), UnwindUnreachable())
        }
        bb1 = {
            b = a;
            Call(b = cp(1), ReturnTo(bb2), UnwindUnreachable())
        }
        bb2 = {
            Call(r1 = cp_sum(a), ReturnTo(bb3), UnwindUnreachable())
        }
        bb3 = {
            Call(r2 = cp_sum(b), ReturnTo(bb4), UnwindUnreachable())
        }
        bb4 = {
            t = r1 * 3;
            RET = t + r2;
            Return()
        }
    }
}

/// `x = copy a; o = Some(copy x); SetDiscriminant(o, None); use(copy x)`, where the tag of `o`
/// is a niche in the bytes of `x`.
#[custom_mir(dialect = "runtime", phase = "optimized")]
#[inline(never)]
fn mir_set_discriminant(seed: u64) -> u64 {
    mir! {
        let a: Cp;
        let x: NzCp;
        let o: Option<NzCp>;
        let r: u64;
        {
            Call(a = cp(seed), ReturnTo(bb1), UnwindUnreachable())
        }
        bb1 = {
            Call(x = nz_cp(a), ReturnTo(bb2), UnwindUnreachable())
        }
        bb2 = {
            o = Some(x);
            SetDiscriminant(o, 0);
            Call(r = nz_sum(x), ReturnTo(bb3), UnwindUnreachable())
        }
        bb3 = {
            RET = r;
            Return()
        }
    }
}

/// `y = (o as Some).0` out of a copy of `x`, written by a callee, while `x` is used again.
#[custom_mir(dialect = "runtime", phase = "optimized")]
#[inline(never)]
fn mir_field_callee_mutates(seed: u64) -> u64 {
    mir! {
        let a: Cp;
        let x: NzCp;
        let o: Option<NzCp>;
        let y: NzCp;
        let y2: NzCp;
        let r1: u64;
        let r2: u64;
        let t: u64;
        {
            Call(a = cp(seed), ReturnTo(bb1), UnwindUnreachable())
        }
        bb1 = {
            Call(x = nz_cp(a), ReturnTo(bb1b), UnwindUnreachable())
        }
        bb1b = {
            o = Some(x);
            y = Field::<NzCp>(Variant(o, 1), 0);
            Call(y2 = nz_mutate(Move(y)), ReturnTo(bb2), UnwindUnreachable())
        }
        bb2 = {
            Call(r1 = nz_sum(y2), ReturnTo(bb3), UnwindUnreachable())
        }
        bb3 = {
            Call(r2 = nz_sum(x), ReturnTo(bb4), UnwindUnreachable())
        }
        bb4 = {
            t = r1 * 3;
            RET = t + r2;
            Return()
        }
    }
}

/// `b = copy a; drop(b); use(a)`, where the drop glue writes to the slot of `b`.
#[custom_mir(dialect = "runtime", phase = "optimized")]
#[inline(never)]
fn mir_drop_writes(seed: u64) -> u64 {
    mir! {
        let a: Dg;
        let b: Dg;
        let r: u64;
        {
            Call(a = dg(seed), ReturnTo(bb0), UnwindUnreachable())
        }
        bb0 = {
            b = Move(a);
            Drop(b, ReturnTo(bb1), UnwindUnreachable())
        }
        bb1 = {
            Call(r = dg_sum(Move(a)), ReturnTo(bb2), UnwindUnreachable())
        }
        bb2 = {
            RET = r;
            Return()
        }
    }
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
    let total = thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(|| recurse_wrapped(black_box(WRAPPED_DEPTH), 1))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(total, WRAPPED_DEPTH);

    for seed in [3, 1000, u64::MAX / 3] {
        assert_eq!(src_write_field(seed), expected_write_field(seed));
        assert_eq!(src_callee_mutates(seed), expected_callee_mutates(seed));
        assert_eq!(src_whole_rewrite(seed), expected_whole_rewrite(seed));
        assert_eq!(mir_write_field(seed), expected_mir_write_field(seed));
        assert_eq!(mir_callee_mutates(seed), expected_callee_mutates(seed));
        assert_eq!(
            mir_call_destination(seed),
            cp_sum(cp(seed)).wrapping_mul(3).wrapping_add(cp_sum(cp(1)))
        );
        assert_eq!(mir_set_discriminant(seed), expected_niche(seed));
        assert_eq!(mir_field_callee_mutates(seed), expected_field_callee_mutates(seed));
        assert_eq!(mir_drop_writes(seed), dg_sum(Dg([seed; 8])));
    }

    move_in_loop(7);
    wrapped_values(9);
    wrapped_drop_order();
    drop_order();
    #[cfg(panic = "unwind")]
    catch_panic(50);
}
