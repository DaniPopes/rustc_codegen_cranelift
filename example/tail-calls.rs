// Explicit tail calls and the `rust-preserve-none` and `tail` ABIs. Every chain is deep enough to
// overflow the stack if the tail calls were lowered to normal calls.

#![feature(explicit_tail_calls, rust_preserve_none_cc, rust_tail_cc)]
#![allow(incomplete_features, tail_call_track_caller)]

const DEPTH: u64 = 10_000_000;

#[derive(Clone, Copy, PartialEq, Debug)]
struct Big([u64; 4]);

#[derive(Clone, Copy, PartialEq, Debug)]
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

// `become` with a calling convention without guaranteed tail calls is lowered to a normal call.
#[inline(never)]
fn rust_abi(n: u64, a: Big) -> Big {
    if n == 0 {
        return a;
    }
    become rust_abi(n - 1, Big([a.0[0] + 1, a.0[1], a.0[2], a.0[3]]))
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

    assert_eq!(rust_abi(1000, a), Big([1000, 1, 2, 3]));
    assert_eq!(tail_call_tracked(41), 42);
}
