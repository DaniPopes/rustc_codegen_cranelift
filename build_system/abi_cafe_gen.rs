//! Generation of the abi-cafe tests that are too repetitive to write by hand.
//!
//! The tests probe how arguments and return values get assigned to registers and stack slots. Each
//! `Exhaust*` test places one multi-register or oddly shaped type after a growing number of
//! integer or float arguments, so that the type sits right where the integer or float registers
//! run out, for every possible number of registers already in use. A backend that consumes a
//! leftover register the type could not use, or that packs stack slots differently, shows up as a
//! trailing argument whose value is wrong. The `RandomMix*` tests are random signatures drawn from
//! the same pool of types with a fixed seed.

use std::collections::HashSet;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use crate::path::Dirs;
use crate::utils::ensure_empty_dir;

const PRIMITIVES: &[&str] = &[
    "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "u128", "i128", "f16", "f32", "f64",
    "f128", "bool", "ptr",
];

/// Types at the boundary of the register assignment rules of at least one ABI. The aggregates are
/// defined in `scripts/abi-cafe-tests/shapes`.
const PROBES: &[&str] = &[
    "u8",
    "i8",
    "i16",
    "u32",
    "bool",
    "f32",
    "f16",
    "u128",
    "i128",
    "f128",
    "U8x3",
    "U8x9",
    "U16x8",
    "U16x9",
    "U32x3",
    "U32U64",
    "U64x2",
    "U64x3",
    "F32x2",
    "F32x3",
    "F32x4",
    "F64x2",
    "F64x3",
    "F64x4",
    "F32U32",
    "U8F32",
    "F64U64",
    "U128U64",
    "U64U128",
    "U8U128",
    "U128F64",
    "Align16U64",
    "Align16U8",
    "F128x1",
    "F16x4",
    "OptionI128",
    "OptionF64",
    "ResultU64U32",
    "UnionU128F64",
    "NestedF32x2F32",
    "ZstBetween",
    "ArrayU64x2",
];

/// The most arguments placed before a probe. This exhausts the 6 integer and 8 float registers of
/// x86_64 SysV and the 8 integer and 8 float registers of aarch64, with one more argument to spare.
const MAX_FILL: usize = 9;

/// Types that Cranelift currently gets wrong and that would make every random mix fail. Aggregates
/// with an alignment of 16 bytes that are passed on the stack are placed at an 8-byte aligned
/// offset, and a 128-bit integer that doesn't fit in the remaining integer registers takes up the
/// register that is left. The `Exhaust` tests of these are marked as busted.
const RANDOM_MIX_EXCLUDED: &[&str] = &[
    "u128",
    "i128",
    "Align16U64",
    "Align16U8",
    "F128x1",
    "OptionI128",
    "U128F64",
    "U128U64",
    "U64U128",
    "U8U128",
    "UnionU128F64",
    "ZstBetween",
];

const RANDOM_MIX_SEEDS: u64 = 8;
const RANDOM_MIX_FUNCS: usize = 40;

/// Write the hand-written and generated abi-cafe tests into one directory and return it.
pub(crate) fn generate(dirs: &Dirs) -> PathBuf {
    let static_tests = dirs.source_dir.join("scripts/abi-cafe-tests");
    let out = dirs.build_dir.join("abi-cafe-tests");
    ensure_empty_dir(&out);

    for entry in fs::read_dir(static_tests.join("shapes")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), out.join(entry.file_name())).unwrap();
    }

    for probe in PROBES {
        let mut test = Test::new(&static_tests);
        exhaust(&mut test, probe);
        test.write(&out, &format!("Exhaust_{probe}"));
    }

    for seed in 1..=RANDOM_MIX_SEEDS {
        let mut test = Test::new(&static_tests);
        random_mix(&mut test, seed);
        test.write(&out, &format!("RandomMix{seed}"));
    }

    out
}

fn exhaust(test: &mut Test<'_>, probe: &str) {
    for fill in 0..=MAX_FILL {
        let ints = vec!["u64"; fill];
        let floats = vec!["f64"; fill];

        // The probe followed by a small integer and a float. If the probe consumed a register it
        // did not use, the integer is not where the callee expects it.
        test.func(&format!("int{fill}"), &concat(&[&ints, &[probe, "u64", "f64"]]), None);
        test.func(&format!("float{fill}"), &concat(&[&floats, &[probe, "f64", "u64"]]), None);

        // Two probes, of which only the first may still fit in registers.
        test.func(
            &format!("int{fill}_twice"),
            &concat(&[&ints, &[probe, "u8", probe, "u16", "f32", "u64"]]),
            None,
        );
        test.func(
            &format!("float{fill}_twice"),
            &concat(&[&floats, &[probe, "f32", probe, "u16", "f64", "u64"]]),
            None,
        );

        // Small arguments after the probe. Once on the stack, some ABIs pack them tightly and
        // others give each a full slot.
        test.func(
            &format!("int{fill}_stack_pack"),
            &concat(&[&ints, &[probe, "u8", "i16", "u8", "i8", "u32", "bool", "f32", "u64"]]),
            None,
        );
        test.func(
            &format!("float{fill}_stack_pack"),
            &concat(&[&floats, &[probe, "f32", "f32", "u16", "u8", "f64", "i8", "u64"]]),
            None,
        );

        // A hidden return pointer takes a register too.
        test.func(&format!("int{fill}_return"), &concat(&[&ints, &[probe, "u64"]]), Some(probe));
        test.func(
            &format!("float{fill}_return"),
            &concat(&[&floats, &[probe, "f64"]]),
            Some(probe),
        );
    }

    // Both register classes run out at once, with either class first.
    for ints in 4..=MAX_FILL {
        for floats in [7, 8, MAX_FILL] {
            let ints = vec!["u64"; ints];
            let floats = vec!["f64"; floats];
            test.func(
                &format!("ints{}_floats{}", ints.len(), floats.len()),
                &concat(&[&ints, &floats, &[probe, "u64", "f64"]]),
                None,
            );
            test.func(
                &format!("floats{}_ints{}", floats.len(), ints.len()),
                &concat(&[&floats, &ints, &[probe, "f64", "u64"]]),
                None,
            );
        }
    }
}

fn random_mix(test: &mut Test<'_>, seed: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let pool: Vec<_> = [PRIMITIVES, PROBES]
        .concat()
        .into_iter()
        .filter(|ty| !RANDOM_MIX_EXCLUDED.contains(ty))
        .collect();
    for idx in 0..RANDOM_MIX_FUNCS {
        let len = 2 + rng.below(19);
        let inputs: Vec<_> = (0..len).map(|_| pool[rng.below(pool.len())]).collect();
        let output = (rng.below(3) != 0).then(|| pool[rng.below(pool.len())]);
        test.func(&format!("mix{idx}"), &inputs, output);
    }
}

fn concat<'a>(parts: &[&[&'a str]]) -> Vec<&'a str> {
    parts.iter().flat_map(|part| part.iter().copied()).collect()
}

struct Test<'a> {
    static_tests: &'a Path,
    defs: Vec<String>,
    seen_defs: HashSet<String>,
    seen_types: HashSet<String>,
    funcs: String,
}

impl<'a> Test<'a> {
    fn new(static_tests: &'a Path) -> Self {
        Test {
            static_tests,
            defs: vec![],
            seen_defs: HashSet::new(),
            seen_types: HashSet::new(),
            funcs: String::new(),
        }
    }

    fn func(&mut self, name: &str, inputs: &[&str], output: Option<&str>) {
        for ty in inputs.iter().chain(output.as_ref()) {
            self.define(ty);
        }

        writeln!(self.funcs, "fn \"{name}\" {{").unwrap();
        writeln!(self.funcs, "    inputs {{").unwrap();
        for ty in inputs {
            writeln!(self.funcs, "        _ \"{ty}\"").unwrap();
        }
        writeln!(self.funcs, "    }}").unwrap();
        if let Some(ty) = output {
            writeln!(self.funcs, "    outputs {{ _ \"{ty}\"; }}").unwrap();
        }
        writeln!(self.funcs, "}}\n").unwrap();
    }

    /// Add the definition of an aggregate with the helper types it needs, skipping those that an
    /// earlier type already defined.
    fn define(&mut self, ty: &str) {
        if PRIMITIVES.contains(&ty) || !self.seen_types.insert(ty.to_owned()) {
            return;
        }
        let path = self.static_tests.join("shapes").join(format!("{ty}.procgen.kdl"));
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for block in source.split("\n\n") {
            let mut def = String::new();
            for line in block.lines().filter(|line| !line.starts_with("//")) {
                def.push_str(line);
                def.push('\n');
            }
            if !def.trim().is_empty() && self.seen_defs.insert(def.clone()) {
                self.defs.push(def);
            }
        }
    }

    fn write(&self, out: &Path, name: &str) {
        let mut source = self.defs.join("\n");
        source.push('\n');
        source.push_str(&self.funcs);
        fs::write(out.join(format!("{name}.kdl")), source).unwrap();
    }
}

/// xorshift64*, enough for a repeatable spread of signatures.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % bound
    }
}
