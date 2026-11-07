// Explicit tail calls and the `rust-preserve-none` and `tail` ABIs. Every chain is deep enough to
// overflow the stack if the tail calls were lowered to normal calls.

#![feature(explicit_tail_calls, rust_preserve_none_cc, rust_tail_cc)]
#![allow(incomplete_features, tail_call_track_caller)]

const DEPTH: u64 = 10_000_000;

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct Big([u64; 4]);

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(C)]
struct Huge([u64; 12]);

// Direct recursion with arguments and a return value that are passed indirectly.
#[inline(never)]
extern "rust-preserve-none" fn count_big(n: u64, a: Big, b: Big) -> Big {
    if n == 0 {
        return Big([a.0[0], b.0[0], a.0[1] + b.0[1], 0]);
    }
    // Swap `a` and `b` to check that the arguments are copied before the caller's are overwritten.
    become count_big(n - 1, b, Big([a.0[0] + 1, a.0[1], a.0[2], a.0[3]]))
}

fn count_big_reference(mut n: u64, mut a: Big, mut b: Big) -> Big {
    while n != 0 {
        (a, b) = (b, Big([a.0[0] + 1, a.0[1], a.0[2], a.0[3]]));
        n -= 1;
    }
    Big([a.0[0], b.0[0], a.0[1] + b.0[1], 0])
}

// Dispatch through a table of function pointers, like an interpreter.
struct State {
    sum: u64,
}

type Handler = extern "rust-preserve-none" fn(&mut State, u64, usize, Big) -> u64;

static TABLE: [Handler; 3] = [add, sub, done];

extern "rust-preserve-none" fn add(state: &mut State, n: u64, op: usize, extra: Big) -> u64 {
    state.sum += extra.0[0];
    next(state, n, op, extra)
}

extern "rust-preserve-none" fn sub(state: &mut State, n: u64, op: usize, extra: Big) -> u64 {
    state.sum -= extra.0[1];
    next(state, n, op, extra)
}

extern "rust-preserve-none" fn done(state: &mut State, _n: u64, _op: usize, _extra: Big) -> u64 {
    state.sum
}

extern "rust-preserve-none" fn next(state: &mut State, n: u64, op: usize, extra: Big) -> u64 {
    if n == 0 {
        become TABLE[2](state, n, op, extra)
    }
    let handler = TABLE[(op + n as usize) % 3];
    become handler(state, n - 1, op, extra)
}

fn table_reference(mut sum: u64, mut n: u64, op: usize, extra: Big) -> u64 {
    let mut handler = 0;
    loop {
        match handler {
            0 => sum += extra.0[0],
            1 => sum -= extra.0[1],
            _ => return sum,
        }
        handler = if n == 0 { 2 } else { (op + n as usize) % 3 };
        n = n.saturating_sub(1);
    }
}

// Calls through a trait object.
trait Step {
    extern "rust-preserve-none" fn step(&self, n: u64, acc: u64) -> u64;
}

struct Inc(u64);

impl Step for Inc {
    extern "rust-preserve-none" fn step(&self, n: u64, acc: u64) -> u64 {
        acc + n * self.0
    }
}

extern "rust-preserve-none" fn step_dyn(step: &dyn Step, n: u64, acc: u64) -> u64 {
    become step.step(n, acc)
}

// `extern "tail"` callers and callees may have different arguments, including ones passed on the
// stack, and the stack space of the arguments is reused.
extern "tail" fn tail_ping(n: u64, a: Huge, b: u8) -> u64 {
    if n == 0 {
        return a.0.iter().sum::<u64>() + u64::from(b);
    }
    let mut a = a;
    a.0[(n % 12) as usize] += 1;
    become tail_pong(n - 1, b, a, Big([n, 0, 0, 0]), 1)
}

extern "tail" fn tail_pong(n: u64, b: u8, a: Huge, _unused: Big, _unused2: u8) -> u64 {
    if n == 0 {
        return a.0.iter().sum::<u64>() + u64::from(b);
    }
    become tail_ping(n - 1, a, b)
}

#[inline(never)]
extern "tail" fn tail_ret(n: u64, acc: u64) -> Huge {
    if n == 0 {
        return Huge([acc; 12]);
    }
    become tail_ret(n - 1, acc + 1)
}

// The Rust and C ABIs: the caller pops the stack arguments, so the arguments of a tail call are
// written over the ones of the caller.
#[inline(never)]
fn rust_abi(n: u64, a: Big) -> Big {
    if n == 0 {
        return a;
    }
    become rust_abi(n - 1, Big([a.0[0] + 1, a.0[1], a.0[2], a.0[3]]))
}

// More integer arguments than fit in registers, and arguments of every size on the stack.
#[inline(never)]
fn rust_abi_stack(
    n: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: u64,
    h: u64,
    i: u8,
    big: Huge,
) -> u64 {
    if n == 0 {
        return a
            + b * 2
            + c * 3
            + d * 4
            + e * 5
            + f * 6
            + g * 7
            + h * 8
            + u64::from(i)
            + big.0[0]
            + big.0[11];
    }
    // Rotate the arguments to check that they are copied before any of them is overwritten.
    become rust_abi_stack(
        n - 1,
        b,
        c,
        d,
        e,
        f,
        g,
        h,
        a,
        i.wrapping_add(1),
        Huge([
            big.0[11],
            big.0[1],
            big.0[2],
            big.0[3],
            big.0[4],
            big.0[5],
            big.0[6],
            big.0[7],
            big.0[8],
            big.0[9],
            big.0[10],
            big.0[0] + 1,
        ]),
    )
}

fn rust_abi_stack_reference(
    mut n: u64,
    mut a: u64,
    mut b: u64,
    mut c: u64,
    mut d: u64,
    mut e: u64,
    mut f: u64,
    mut g: u64,
    mut h: u64,
    mut i: u8,
    mut big: Huge,
) -> u64 {
    while n != 0 {
        (a, b, c, d, e, f, g, h) = (b, c, d, e, f, g, h, a);
        i = i.wrapping_add(1);
        big = Huge([
            big.0[11],
            big.0[1],
            big.0[2],
            big.0[3],
            big.0[4],
            big.0[5],
            big.0[6],
            big.0[7],
            big.0[8],
            big.0[9],
            big.0[10],
            big.0[0] + 1,
        ]);
        n -= 1;
    }
    a + b * 2 + c * 3 + d * 4 + e * 5 + f * 6 + g * 7 + h * 8 + u64::from(i) + big.0[0] + big.0[11]
}

extern "C" fn c_abi_stack(
    n: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
    g: u64,
    h: u64,
    big: Huge,
    other: Huge,
) -> u64 {
    if n == 0 {
        return a + b + c + d + e + f + g + h + big.0[0] * 3 + other.0[0] * 5 + other.0[11];
    }
    become c_abi_stack(n - 1, b, c, d, e, f, g, h, a + 1, other, Huge([big.0[0] + 1; 12]))
}

fn c_abi_stack_reference(
    mut n: u64,
    mut a: u64,
    mut b: u64,
    mut c: u64,
    mut d: u64,
    mut e: u64,
    mut f: u64,
    mut g: u64,
    mut h: u64,
    mut big: Huge,
    mut other: Huge,
) -> u64 {
    while n != 0 {
        (a, b, c, d, e, f, g, h) = (b, c, d, e, f, g, h, a + 1);
        (big, other) = (other, Huge([big.0[0] + 1; 12]));
        n -= 1;
    }
    a + b + c + d + e + f + g + h + big.0[0] * 3 + other.0[0] * 5 + other.0[11]
}

extern "C" fn c_abi_indirect(n: u64, f: extern "C" fn(u64, u64) -> u64, acc: u64) -> u64 {
    if n == 0 {
        return acc;
    }
    become c_abi_indirect_next(n - 1, f, f(acc, n))
}

extern "C" fn c_abi_indirect_next(n: u64, f: extern "C" fn(u64, u64) -> u64, acc: u64) -> u64 {
    become c_abi_indirect(n, f, acc)
}

extern "C" fn mix(acc: u64, n: u64) -> u64 {
    (acc ^ n).wrapping_mul(31)
}

#[track_caller]
fn tracked(n: u64) -> u64 {
    n + u64::from(std::panic::Location::caller().line() != 0)
}

#[inline(never)]
fn tail_call_tracked(n: u64) -> u64 {
    become tracked(n)
}

fn main() {
    let a = Big([0, 1, 2, 3]);
    let b = Big([10, 20, 30, 40]);
    assert_eq!(count_big(DEPTH, a, b), count_big_reference(DEPTH, a, b));
    assert_eq!(count_big(DEPTH + 1, a, b), count_big_reference(DEPTH + 1, a, b));

    let extra = Big([3, 1, 0, 0]);
    for op in 0..3 {
        let mut state = State { sum: 1 << 40 };
        let result = TABLE[0](&mut state, DEPTH, op, extra);
        assert_eq!(result, table_reference(1 << 40, DEPTH, op, extra));
        assert_eq!(state.sum, result);
    }

    assert_eq!(step_dyn(&Inc(2), 21, 5), 47);

    assert_eq!(tail_ping(DEPTH, Huge([1; 12]), 7), 12 + 7 + DEPTH / 2);
    assert_eq!(tail_ret(DEPTH, 5), Huge([5 + DEPTH; 12]));

    assert_eq!(rust_abi(DEPTH, a), Big([DEPTH, 1, 2, 3]));
    let big = Huge([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    assert_eq!(
        rust_abi_stack(DEPTH + 3, 1, 2, 3, 4, 5, 6, 7, 8, 9, big),
        rust_abi_stack_reference(DEPTH + 3, 1, 2, 3, 4, 5, 6, 7, 8, 9, big),
    );
    assert_eq!(
        c_abi_stack(DEPTH + 1, 1, 2, 3, 4, 5, 6, 7, 8, big, Huge([9; 12])),
        c_abi_stack_reference(DEPTH + 1, 1, 2, 3, 4, 5, 6, 7, 8, big, Huge([9; 12])),
    );
    let mut acc = 7;
    for n in (1..=DEPTH).rev() {
        acc = mix(acc, n);
    }
    assert_eq!(c_abi_indirect(DEPTH, mix, 7), acc);
    assert_eq!(tail_call_tracked(41), 42);
}
