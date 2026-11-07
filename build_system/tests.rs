use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build_sysroot::{SysrootConfig, SysrootKind};
use crate::path::{Dirs, RelPath};
use crate::prepare::{GitRepo, apply_patches};
use crate::rustc_info::get_default_sysroot;
use crate::shared_utils::rustflags_from_env;
use crate::utils::{CargoProject, Compiler, LogGroup, ensure_empty_dir, spawn_and_wait};
use crate::{CodegenBackend, build_sysroot, config};

static BUILD_EXAMPLE_OUT_DIR: RelPath = RelPath::build("example");

struct TestCase {
    config: &'static str,
    cmd: TestCaseCmd,
}

enum TestCaseCmd {
    Custom { func: &'static dyn Fn(&TestRunner<'_>) },
    BuildLib { source: &'static str, crate_types: &'static str },
    BuildBin { source: &'static str },
    BuildBinAndRun { source: &'static str, args: &'static [&'static str] },
    JitBin { source: &'static str, args: &'static str },
}

impl TestCase {
    // FIXME reduce usage of custom test case commands
    const fn custom(config: &'static str, func: &'static dyn Fn(&TestRunner<'_>)) -> Self {
        Self { config, cmd: TestCaseCmd::Custom { func } }
    }

    const fn build_lib(
        config: &'static str,
        source: &'static str,
        crate_types: &'static str,
    ) -> Self {
        Self { config, cmd: TestCaseCmd::BuildLib { source, crate_types } }
    }

    const fn build_bin(config: &'static str, source: &'static str) -> Self {
        Self { config, cmd: TestCaseCmd::BuildBin { source } }
    }

    const fn build_bin_and_run(
        config: &'static str,
        source: &'static str,
        args: &'static [&'static str],
    ) -> Self {
        Self { config, cmd: TestCaseCmd::BuildBinAndRun { source, args } }
    }

    const fn jit_bin(config: &'static str, source: &'static str, args: &'static str) -> Self {
        Self { config, cmd: TestCaseCmd::JitBin { source, args } }
    }
}

const NO_SYSROOT_SUITE: &[TestCase] = &[
    TestCase::build_lib("build.mini_core", "example/mini_core.rs", "lib,dylib"),
    TestCase::custom("build.eh_frame_encodings", &|runner| {
        if !runner.panic_unwind_support {
            eprintln!("Unwinding is not supported on this target");
            return;
        }
        let objdump = get_default_sysroot(&runner.bootstrap_host_compiler.rustc)
            .join("lib/rustlib")
            .join(&runner.bootstrap_host_compiler.target)
            .join("bin/llvm-objdump");
        if !objdump.exists() {
            eprintln!("llvm-objdump is not installed (rustup component llvm-tools)");
            return;
        }

        let out_dir = BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs);
        for target in
            ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
        {
            let emit_object = |mut cmd: Command, name: &str| {
                let object = out_dir.join(format!("eh_frame_encodings_{target}_{name}.o"));
                cmd.args(["--target", target, "--crate-type", "lib", "--crate-name", "mini_core"])
                    .args(["--edition=2024", "-Zforce-unstable-if-unmarked", "-Cpanic=unwind"])
                    .args(["--emit", "obj", "-o"])
                    .arg(&object)
                    .arg("example/mini_core.rs");
                spawn_and_wait(cmd);
                cie_pointer_encodings(&objdump, &object)
            };

            let mut clif_cmd = Command::new(&runner.target_compiler.rustc);
            clif_cmd.args(&runner.target_compiler.rustflags);
            let clif = emit_object(clif_cmd, "clif");
            let llvm = emit_object(Command::new(&runner.bootstrap_host_compiler.rustc), "llvm");
            assert!(!llvm.is_empty(), "{target}: LLVM didn't emit a CIE");
            assert_eq!(clif, llvm, "{target}: CIE pointer encodings differ from LLVM");
        }
    }),
    TestCase::build_lib("build.example", "example/example.rs", "lib"),
    TestCase::jit_bin("jit.mini_core_hello_world", "example/mini_core_hello_world.rs", "abc bcd"),
    TestCase::build_bin_and_run(
        "aot.mini_core_hello_world",
        "example/mini_core_hello_world.rs",
        &["abc", "bcd"],
    ),
];

const BASE_SYSROOT_SUITE: &[TestCase] = &[
    TestCase::build_bin_and_run(
        "aot.arbitrary_self_types_pointers_and_wrappers",
        "example/arbitrary_self_types_pointers_and_wrappers.rs",
        &[],
    ),
    TestCase::jit_bin("jit.catch_unwind", "example/catch_unwind.rs", ""),
    TestCase::jit_bin("jit.std_example", "example/std_example.rs", "arg"),
    TestCase::build_bin_and_run("aot.std_example", "example/std_example.rs", &["arg"]),
    TestCase::build_bin_and_run("aot.catch_unwind", "example/catch_unwind.rs", &[]),
    TestCase::build_bin_and_run("aot.dst_field_align", "example/dst-field-align.rs", &[]),
    TestCase::build_bin_and_run(
        "aot.subslice-patterns-const-eval",
        "example/subslice-patterns-const-eval.rs",
        &[],
    ),
    TestCase::build_bin_and_run(
        "aot.track-caller-attribute",
        "example/track-caller-attribute.rs",
        &[],
    ),
    TestCase::build_bin_and_run("aot.float-minmax-pass", "example/float-minmax-pass.rs", &[]),
    TestCase::custom("aot.powi_libcall_signature", &|runner| {
        let mut cmd = runner.rustc_command(["example/powi-libcall-signature.rs"]);
        let output = cmd.output().unwrap();
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        assert!(!output.status.success(), "expected compilation to fail, got success");
        assert!(
            combined.contains("attempt to declare `__powisf2`"),
            "expected signature mismatch error, got:\n{combined}"
        );
        assert!(
            !combined.contains("internal compiler error") && !combined.contains("panicked at"),
            "expected graceful error, not ICE:\n{combined}"
        );
    }),
    TestCase::custom("aot.tls_conflicting_declarations", &|runner| {
        let variants: &[(&str, bool)] = if runner.target_compiler.target.contains("windows") {
            // The other variants are rejected earlier on Windows with a duplicate-symbol error,
            // before reaching the TLS declaration check.
            &[("plain", true)]
        } else {
            &[("plain", true), ("matching", false), ("reverse", true)]
        };

        for &(variant, expect_conflict) in variants {
            let mut cmd = runner.rustc_command([
                "example/tls-conflicting-declarations.rs",
                "--emit=obj",
                "--check-cfg=cfg(matching)",
                "--check-cfg=cfg(reverse)",
            ]);
            if variant != "plain" {
                cmd.arg("--cfg").arg(variant);
            }

            let output = cmd.output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            let has_conflict = stderr.contains(
                "conflicting thread-local and non-thread-local declarations for `EXPORTED`",
            );

            assert_eq!(has_conflict, expect_conflict, "{variant}: unexpected stderr:\n{stderr}");
            assert_eq!(
                output.status.success(),
                !expect_conflict,
                "{variant}: unexpected compilation status:\n{stderr}"
            );
            assert!(
                !stderr.contains("internal compiler error") && !stderr.contains("panicked at"),
                "{variant}: compiler ICE'd:\n{stderr}"
            );
        }
    }),
    TestCase::custom("aot.unwind", &|runner| {
        if !runner.panic_unwind_support {
            eprintln!("Unwinding is not supported on this target");
            return;
        }
        runner.run_rustc(["example/unwind.rs"]);
        runner.run_out_command("unwind", &[]);

        // A panic in a destructor during cleanup and a panic escaping from an `extern "C"`
        // function have to abort.
        for (arg, message) in [
            ("abort-in-drop", "panic in a destructor during cleanup"),
            ("abort-extern-c", "panic in a function that cannot unwind"),
        ] {
            let mut cmd = runner
                .target_compiler
                .run_with_runner(BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs).join("unwind"));
            let output = cmd.arg(arg).output().unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);

            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                const SIGABRT: i32 = 6;
                assert_eq!(
                    output.status.signal(),
                    Some(SIGABRT),
                    "{arg}: unexpected exit:\n{stderr}"
                );
            }
            assert!(!output.status.success(), "{arg}: expected abort, got success");
            assert!(stderr.contains(message), "{arg}: expected `{message}`, got:\n{stderr}");
        }
    }),
    TestCase::custom("aot.unwind-lsda", &|runner| {
        if !runner.panic_unwind_support || !runner.is_native {
            eprintln!("[SKIP] unwind-lsda (unsupported target, panic=abort or cross compiling)");
            return;
        }
        let objdump = get_default_sysroot(&runner.bootstrap_host_compiler.rustc)
            .join("lib/rustlib")
            .join(&runner.bootstrap_host_compiler.target)
            .join("bin/llvm-objdump");
        if !objdump.exists() {
            eprintln!("llvm-objdump is not installed (rustup component llvm-tools)");
            return;
        }

        let out_dir = BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs);
        let emit_object = |mut cmd: Command, name: &str| {
            let object = out_dir.join(format!("unwind_lsda_{name}.o"));
            cmd.args(["--crate-type", "lib", "--crate-name", "unwind_lsda"])
                .args(["--edition=2024", "-Cpanic=unwind", "-Cmetadata=lsda"])
                .args(["--emit", "obj", "-o"])
                .arg(&object)
                .arg("example/unwind-lsda.rs");
            spawn_and_wait(cmd);
            exception_tables(&objdump, &object)
        };

        let mut clif_cmd = Command::new(&runner.target_compiler.rustc);
        clif_cmd.args(&runner.target_compiler.rustflags);
        let clif = emit_object(clif_cmd, "clif");
        let llvm = emit_object(Command::new(&runner.bootstrap_host_compiler.rustc), "llvm");

        // Only `shared_cleanup` has landing pads, so only it has an LSDA and an FDE with a
        // personality function. Its calls with the same cleanup share one call site entry, so
        // there are no more entries with a landing pad than in LLVM's.
        assert_eq!(clif.fdes_with_personality, llvm.fdes_with_personality);
        assert_eq!(llvm.fdes_with_personality, 1);
        assert_eq!(clif.landing_pad_call_sites.len(), 1);
        assert!(
            clif.landing_pad_call_sites <= llvm.landing_pad_call_sites,
            "more call sites with landing pads than LLVM: {clif:?} vs {llvm:?}"
        );
    }),
    TestCase::build_bin_and_run("aot.issue-72793", "example/issue-72793.rs", &[]),
    TestCase::build_bin("aot.issue-59326", "example/issue-59326.rs"),
    TestCase::build_bin_and_run("aot.gen_block_iterate", "example/gen_block_iterate.rs", &[]),
    TestCase::build_bin_and_run("aot.raw-dylib", "example/raw-dylib.rs", &[]),
    TestCase::custom("aot.unwind-terminate", &|runner| {
        // Needs unwinding and a pthread-based forced unwind.
        if !runner.panic_unwind_support || !runner.target_compiler.target.contains("linux") {
            eprintln!("[SKIP] unwind-terminate (unsupported target or panic=abort)");
            return;
        }

        runner.run_rustc(["example/unwind-terminate-forced.rs"]);
        runner.run_out_command("unwind-terminate-forced", &[]);

        runner.run_rustc(["example/unwind-terminate-panic.rs"]);
        let output = runner
            .target_compiler
            .run_with_runner(
                BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs).join("unwind-terminate-panic"),
            )
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "expected an abort, got success");
        assert!(
            stderr.contains("panic in a function that cannot unwind"),
            "expected the terminate landing pad to run, got:\n{stderr}"
        );
    }),
    TestCase::custom("test.sysroot", &|runner| {
        apply_patches(
            &runner.dirs,
            "sysroot_tests",
            &runner.stdlib_source.join("library"),
            &SYSROOT_TESTS_SRC.to_path(&runner.dirs),
        );

        SYSROOT_TESTS.clean(&runner.dirs);

        let mut target_compiler = runner.target_compiler.clone();
        // coretests and alloctests produce a bunch of warnings. When running
        // in rust's CI warnings are denied, so we have to override that here.
        target_compiler.rustflags.push("--cap-lints=allow".to_owned());
        // The standard library may have been compiled with -Zrandomize-layout.
        target_compiler.rustflags.extend(["--cfg".to_owned(), "randomized_layouts".to_owned()]);

        if runner.is_native {
            let mut test_cmd = SYSROOT_TESTS.test(&target_compiler, &runner.dirs);
            test_cmd.args(["-p", "coretests", "-p", "alloctests", "--tests", "--", "-q"]);
            spawn_and_wait(test_cmd);
        } else {
            eprintln!("Cross-Compiling: Not running tests");
            let mut build_cmd = SYSROOT_TESTS.build(&target_compiler, &runner.dirs);
            build_cmd.args(["-p", "coretests", "-p", "alloctests", "--tests"]);
            spawn_and_wait(build_cmd);
        }
    }),
];

pub(crate) static RAND_REPO: GitRepo = GitRepo::github(
    "rust-random",
    "rand",
    "1f4507a8e1cf8050e4ceef95eeda8f64645b6719",
    &[],
    "981f8bf489338978",
    "rand",
);

static RAND: CargoProject = CargoProject::new(RAND_REPO.source_dir(), "rand_target");

pub(crate) static REGEX_REPO: GitRepo = GitRepo::github(
    "rust-lang",
    "regex",
    "061ee815ef2c44101dba7b0b124600fcb03c1912",
    &[],
    "dc26aefbeeac03ca",
    "regex",
);

static REGEX: CargoProject = CargoProject::new(REGEX_REPO.source_dir(), "regex_target");

pub(crate) static GRAVIOLA_REPO: GitRepo = GitRepo::github(
    "ctz",
    "graviola",
    "7763d0cc617d6f5f66c3bc0fe9b3d8581d781b6a", // v0.4.1
    &["thirdparty/cavp", "thirdparty/wycheproof"],
    "7fa5a75b9fb1ac40",
    "graviola",
);

static GRAVIOLA: CargoProject = CargoProject::new(GRAVIOLA_REPO.source_dir(), "graviola_target");

static PORTABLE_SIMD_SRC: RelPath = RelPath::build("portable-simd");

static PORTABLE_SIMD: CargoProject = CargoProject::new(PORTABLE_SIMD_SRC, "portable-simd_target");

static SYSROOT_TESTS_SRC: RelPath = RelPath::build("sysroot_tests");

static SYSROOT_TESTS: CargoProject = CargoProject::new(SYSROOT_TESTS_SRC, "sysroot_tests_target");

static STDARCH_SRC: RelPath = RelPath::build("stdarch");

static STDARCH: CargoProject = CargoProject::new(STDARCH_SRC, "stdarch_target");
static STDARCH_RUST_PROGRAMS: CargoProject =
    CargoProject::new(RelPath::build("stdarch/rust_programs"), "stdarch_rust_programs_target");

const EXTENDED_SYSROOT_SUITE: &[TestCase] = &[
    TestCase::custom("test.rust-random/rand", &|runner| {
        RAND_REPO.patch(&runner.dirs);

        RAND.clean(&runner.dirs);

        if runner.is_native {
            let mut test_cmd = RAND.test(&runner.target_compiler, &runner.dirs);
            test_cmd.arg("--workspace").arg("--").arg("-q");
            spawn_and_wait(test_cmd);
        } else {
            eprintln!("Cross-Compiling: Not running tests");
            let mut build_cmd = RAND.build(&runner.target_compiler, &runner.dirs);
            build_cmd.arg("--workspace").arg("--tests");
            spawn_and_wait(build_cmd);
        }
    }),
    TestCase::custom("test.regex", &|runner| {
        REGEX_REPO.patch(&runner.dirs);

        REGEX.clean(&runner.dirs);

        if runner.is_native {
            let mut run_cmd = REGEX.test(&runner.target_compiler, &runner.dirs);
            // regex-capi and regex-debug don't have any tests. Nor do they contain any code
            // that is useful to test with cg_clif. Skip building them to reduce test time.
            run_cmd.args([
                "-p",
                "regex",
                "-p",
                "regex-syntax",
                "--release",
                "--all-targets",
                "--",
                "-q",
            ]);
            spawn_and_wait(run_cmd);

            let mut run_cmd = REGEX.test(&runner.target_compiler, &runner.dirs);
            // don't run integration tests for regex-autonata. they take like 2min each without
            // much extra coverage of simd usage.
            run_cmd.args(["-p", "regex-automata", "--release", "--lib", "--", "-q"]);
            spawn_and_wait(run_cmd);
        } else {
            eprintln!("Cross-Compiling: Not running tests");
            let mut build_cmd = REGEX.build(&runner.target_compiler, &runner.dirs);
            build_cmd.arg("--tests");
            spawn_and_wait(build_cmd);
        }
    }),
    TestCase::custom("test.graviola", &|runner| {
        let (arch, _) = runner.target_compiler.target.split_once('-').unwrap();

        if !["aarch64", "x86_64"].contains(&arch) {
            eprintln!("Skipping `graviola` tests: unsupported target");
            return;
        }

        GRAVIOLA_REPO.patch(&runner.dirs);
        GRAVIOLA.clean(&runner.dirs);

        if runner.is_native {
            let mut test_cmd = GRAVIOLA.test(&runner.target_compiler, &runner.dirs);

            // FIXME: Disable AVX-512 until intrinsics are supported.
            test_cmd.env("GRAVIOLA_CPU_DISABLE_avx512f", "1");
            test_cmd.env("GRAVIOLA_CPU_DISABLE_avx512bw", "1");
            test_cmd.env("GRAVIOLA_CPU_DISABLE_avx512vl", "1");

            test_cmd.args(["-p", "graviola", "--lib", "--", "-q"]);
            spawn_and_wait(test_cmd);
        } else {
            eprintln!("Cross-Compiling: Not running tests");
            let mut build_cmd = GRAVIOLA.build(&runner.target_compiler, &runner.dirs);
            build_cmd.args(["-p", "graviola", "--lib"]);
            spawn_and_wait(build_cmd);
        }
    }),
    TestCase::custom("test.portable-simd", &|runner| {
        apply_patches(
            &runner.dirs,
            "portable-simd",
            &runner.stdlib_source.join("library/portable-simd"),
            &PORTABLE_SIMD_SRC.to_path(&runner.dirs),
        );

        PORTABLE_SIMD.clean(&runner.dirs);

        if runner.is_native {
            let mut test_cmd = PORTABLE_SIMD.test(&runner.target_compiler, &runner.dirs);
            if !runner.target_compiler.target.contains("windows-gnu") {
                test_cmd.arg("--features=proptest/f16");
            }
            // FIXME remove --tests once examples work: https://github.com/rust-lang/portable-simd/issues/470
            // FIXME missing arm64 intrinsics for swizzle_dyn
            test_cmd.arg("-q").arg("--tests").arg("--").arg("--skip").arg("swizzle_dyn");
            spawn_and_wait(test_cmd);
        } else {
            eprintln!("Cross-Compiling: Not running tests");
            let mut build_cmd = PORTABLE_SIMD.build(&runner.target_compiler, &runner.dirs);
            if !runner.target_compiler.target.contains("windows-gnu") {
                build_cmd.arg("--features=proptest/f16");
            }
            // FIXME use --all-targets once examples work: https://github.com/rust-lang/portable-simd/issues/470
            build_cmd.arg("--tests");
            spawn_and_wait(build_cmd);
        }
    }),
    TestCase::custom("test.stdarch-core", &|runner| {
        let target = runner.target_compiler.target.as_str();
        let (arch, _) = target.split_once('-').unwrap();

        // FIXME: Add support for remaining architectures.
        if !["aarch64"].contains(&arch) {
            eprintln!("Skipping `stdarch-core` tests: unsupported target");
            return;
        }

        apply_patches(
            &runner.dirs,
            "stdarch",
            &runner.stdlib_source.join("library/stdarch"),
            &STDARCH_SRC.to_path(&runner.dirs),
        );

        STDARCH.clean(&runner.dirs);

        let skip = format!("build_system/stdarch_skip/{arch}_core.txt");
        let skip = fs::read_to_string(runner.dirs.source_dir.join(skip)).unwrap_or_default();

        let skip_functions = skip
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join(",");

        if runner.is_native {
            let mut test_cmd = STDARCH.test(&runner.target_compiler, &runner.dirs);

            test_cmd.env("TARGET", target);
            test_cmd.env("STDARCH_TEST_SKIP_FUNCTION", &skip_functions);

            test_cmd.args(["-p", "core_arch", "--tests", "--", "-q"]);
            spawn_and_wait(test_cmd);
        } else {
            eprintln!("Cross-Compiling: Not running tests");
            let mut build_cmd = STDARCH.build(&runner.target_compiler, &runner.dirs);

            build_cmd.env("TARGET", target);

            build_cmd.args(["-p", "core_arch", "--tests"]);
            spawn_and_wait(build_cmd);
        }
    }),
    TestCase::custom("test.stdarch-intrinsics", &|runner| {
        let target = runner.target_compiler.target.as_str();
        let (arch, _) = target.split_once('-').unwrap();

        // FIXME: Add support for remaining architectures.
        let input = match arch {
            "aarch64" => "intrinsics_data/arm_intrinsics.json",
            _ => {
                eprintln!("Skipping `stdarch-intrinsics` tests: unsupported target");
                return;
            }
        };

        apply_patches(
            &runner.dirs,
            "stdarch",
            &runner.stdlib_source.join("library/stdarch"),
            &STDARCH_SRC.to_path(&runner.dirs),
        );

        STDARCH_RUST_PROGRAMS.clean(&runner.dirs);

        if !runner.is_native {
            eprintln!("Cross-Compiling: Not running tests");
            return;
        }

        let cc = if target.contains("apple") { "clang" } else { "gcc" };

        let mut generate_cmd = STDARCH.run(&runner.target_compiler, &runner.dirs);
        generate_cmd.current_dir(STDARCH.source_dir(&runner.dirs));
        generate_cmd.args([
            "-p",
            "intrinsic-test",
            "--",
            input,
            "--target",
            target,
            "--cc-arg-style",
            cc,
            "--skip",
            &format!("crates/intrinsic-test/missing_{arch}_common.txt"),
            "--skip",
            &format!("crates/intrinsic-test/missing_{arch}_{cc}.txt"),
        ]);

        for skip in [
            format!("build_system/stdarch_skip/{arch}_intrinsics_common.txt"),
            format!("build_system/stdarch_skip/{arch}_intrinsics_{cc}.txt"),
        ] {
            let skip = runner.dirs.source_dir.join(skip);

            if skip.exists() {
                generate_cmd.arg("--skip").arg(skip);
            }
        }

        spawn_and_wait(generate_cmd);

        let mut test_cmd = STDARCH_RUST_PROGRAMS.test(&runner.target_compiler, &runner.dirs);
        test_cmd.args(["--workspace", "--tests", "--no-fail-fast", "--", "-q"]);
        spawn_and_wait(test_cmd);
    }),
];

pub(crate) fn run_tests(
    dirs: &Dirs,
    sysroot_config: &SysrootConfig,
    use_unstable_features: bool,
    skip_tests: &[&str],
    cg_clif_dylib: &CodegenBackend,
    bootstrap_host_compiler: &Compiler,
    rustup_toolchain_name: Option<&str>,
    target_tuple: String,
) {
    let stdlib_source =
        get_default_sysroot(&bootstrap_host_compiler.rustc).join("lib/rustlib/src/rust");
    assert!(stdlib_source.exists());

    if config::get_bool("testsuite.no_sysroot") && !skip_tests.contains(&"testsuite.no_sysroot") {
        let target_compiler = build_sysroot::build_sysroot(
            dirs,
            &SysrootConfig { sysroot_kind: SysrootKind::None, ..*sysroot_config },
            cg_clif_dylib,
            bootstrap_host_compiler,
            rustup_toolchain_name,
            target_tuple.clone(),
        );

        let runner = TestRunner::new(
            dirs.clone(),
            target_compiler,
            bootstrap_host_compiler,
            use_unstable_features,
            sysroot_config.panic_unwind_support,
            skip_tests,
            bootstrap_host_compiler.target == target_tuple,
            stdlib_source.clone(),
        );

        let path = BUILD_EXAMPLE_OUT_DIR.to_path(dirs);
        ensure_empty_dir(&path);

        runner.run_testsuite(NO_SYSROOT_SUITE);
    } else {
        eprintln!("[SKIP] no_sysroot tests");
    }

    let run_base_sysroot = config::get_bool("testsuite.base_sysroot")
        && !skip_tests.contains(&"testsuite.base_sysroot");
    let run_extended_sysroot = config::get_bool("testsuite.extended_sysroot")
        && !skip_tests.contains(&"testsuite.extended_sysroot");

    if run_base_sysroot || run_extended_sysroot {
        let target_compiler = build_sysroot::build_sysroot(
            dirs,
            sysroot_config,
            cg_clif_dylib,
            bootstrap_host_compiler,
            rustup_toolchain_name,
            target_tuple.clone(),
        );

        let mut runner = TestRunner::new(
            dirs.clone(),
            target_compiler,
            bootstrap_host_compiler,
            use_unstable_features,
            sysroot_config.panic_unwind_support,
            skip_tests,
            bootstrap_host_compiler.target == target_tuple,
            stdlib_source,
        );

        if run_base_sysroot {
            runner.run_testsuite(BASE_SYSROOT_SUITE);
        } else {
            eprintln!("[SKIP] base_sysroot tests");
        }

        if run_extended_sysroot {
            // Rust's build system denies a couple of lints that trigger on several of the test
            // projects. Changing the code to fix them is not worth it, so just silence all lints.
            runner.target_compiler.rustflags.push("--cap-lints=allow".to_owned());
            runner.run_testsuite(EXTENDED_SYSROOT_SUITE);
        } else {
            eprintln!("[SKIP] extended_sysroot tests");
        }
    }
}

struct TestRunner<'a> {
    is_native: bool,
    jit_supported: bool,
    panic_unwind_support: bool,
    skip_tests: &'a [&'a str],
    dirs: Dirs,
    target_compiler: Compiler,
    bootstrap_host_compiler: &'a Compiler,
    stdlib_source: PathBuf,
}

impl<'a> TestRunner<'a> {
    fn new(
        dirs: Dirs,
        mut target_compiler: Compiler,
        bootstrap_host_compiler: &'a Compiler,
        use_unstable_features: bool,
        panic_unwind_support: bool,
        skip_tests: &'a [&'a str],
        is_native: bool,
        stdlib_source: PathBuf,
    ) -> Self {
        target_compiler.rustflags.extend(rustflags_from_env("RUSTFLAGS"));
        target_compiler.rustdocflags.extend(rustflags_from_env("RUSTDOCFLAGS"));

        let jit_supported =
            use_unstable_features && is_native && !target_compiler.target.contains("windows");

        Self {
            is_native,
            jit_supported,
            panic_unwind_support,
            skip_tests,
            dirs,
            target_compiler,
            bootstrap_host_compiler,
            stdlib_source,
        }
    }

    fn run_testsuite(&self, tests: &[TestCase]) {
        for TestCase { config, cmd } in tests {
            let (tag, testname) = config.split_once('.').unwrap();
            let tag = tag.to_uppercase();
            let is_jit_test = tag == "JIT";

            let _guard = if !config::get_bool(config)
                || (is_jit_test && !self.jit_supported)
                || self.skip_tests.contains(config)
            {
                eprintln!("[{tag}] {testname} (skipped)");
                continue;
            } else {
                let guard = LogGroup::guard(&format!("[{tag}] {testname}"));
                eprintln!("[{tag}] {testname}");
                guard
            };

            match *cmd {
                TestCaseCmd::Custom { func } => func(self),
                TestCaseCmd::BuildLib { source, crate_types } => {
                    self.run_rustc([source, "--crate-type", crate_types]);
                }
                TestCaseCmd::BuildBin { source } => {
                    self.run_rustc([source]);
                }
                TestCaseCmd::BuildBinAndRun { source, args } => {
                    self.run_rustc([source]);
                    self.run_out_command(
                        source.split('/').last().unwrap().split('.').next().unwrap(),
                        args,
                    );
                }
                TestCaseCmd::JitBin { source, args } => {
                    let mut jit_cmd = self.rustc_command([
                        "-Zunstable-options",
                        "-Cllvm-args=jit-mode",
                        "-Cprefer-dynamic",
                        source,
                        "--cfg",
                        "jit",
                    ]);
                    if !args.is_empty() {
                        jit_cmd.env("CG_CLIF_JIT_ARGS", args);
                    }
                    spawn_and_wait(jit_cmd);
                }
            }
        }
    }

    #[must_use]
    fn rustc_command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(&self.target_compiler.rustc);
        cmd.args(&self.target_compiler.rustflags);
        cmd.arg("-L");
        cmd.arg(format!("crate={}", BUILD_EXAMPLE_OUT_DIR.to_path(&self.dirs).display()));
        cmd.arg("--out-dir");
        cmd.arg(BUILD_EXAMPLE_OUT_DIR.to_path(&self.dirs));
        cmd.arg("-Cdebuginfo=2");
        cmd.arg("--target");
        cmd.arg(&self.target_compiler.target);
        if !self.panic_unwind_support {
            cmd.arg("-Cpanic=abort");
        }
        cmd.arg("--check-cfg=cfg(jit)");
        cmd.arg("--check-cfg=cfg(target_has_reliable_f128)");
        cmd.arg("--edition=2024");
        // implicitly passed when building inside the rust repo
        cmd.arg("-Zforce-unstable-if-unmarked");
        cmd.args(args);
        cmd
    }

    fn run_rustc<I, S>(&self, args: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        spawn_and_wait(self.rustc_command(args));
    }

    fn run_out_command(&self, name: &str, args: &[&str]) {
        let mut cmd = self
            .target_compiler
            .run_with_runner(BUILD_EXAMPLE_OUT_DIR.to_path(&self.dirs).join(name));

        cmd.args(args);

        spawn_and_wait(cmd);
    }
}

/// Returns the pointer encodings of the personality, LSDA and FDE pointers for all CIEs in
/// `.eh_frame` of `object`, keyed by their augmentation letter.
fn cie_pointer_encodings(objdump: &Path, object: &Path) -> BTreeMap<char, BTreeSet<String>> {
    let output = Command::new(objdump).arg("--dwarf=frames").arg(object).output().unwrap();
    assert!(output.status.success(), "llvm-objdump failed on {}", object.display());
    let output = String::from_utf8(output.stdout).unwrap();

    let mut encodings = BTreeMap::<_, BTreeSet<_>>::new();
    let mut augmentation = "";
    for line in output.lines().map(str::trim) {
        if let Some(value) = line.strip_prefix("Augmentation:") {
            augmentation = value.trim().trim_matches('"');
        } else if let Some(data) = line.strip_prefix("Augmentation data:") {
            let mut data =
                data.split_whitespace().map(|byte| u8::from_str_radix(byte, 16).unwrap());
            // The first letter is `z`, which means that the augmentation has data.
            for letter in augmentation.chars().skip(1) {
                match letter {
                    'L' | 'R' => {
                        encodings
                            .entry(letter)
                            .or_default()
                            .insert(format!("{:#04x}", data.next().unwrap()));
                    }
                    'P' => {
                        let encoding = data.next().unwrap();
                        encodings.entry(letter).or_default().insert(format!("{encoding:#04x}"));
                        // Skip the personality pointer, which is not part of the encoding.
                        let pointer_size = match encoding & 0x0f {
                            0x02 | 0x0a => 2,
                            0x03 | 0x0b => 4,
                            0x00 | 0x04 | 0x0c => 8,
                            _ => panic!("unknown pointer encoding {encoding:#04x}"),
                        };
                        data.nth(pointer_size - 1);
                    }
                    _ => {}
                }
            }
        }
    }
    encodings
}

#[derive(Debug)]
struct ExceptionTables {
    /// The number of FDEs whose CIE has a personality function.
    fdes_with_personality: usize,
    /// For every LSDA, the number of call site entries with a landing pad, sorted.
    landing_pad_call_sites: Vec<usize>,
}

/// Returns a summary of the FDEs and LSDAs in `object`.
fn exception_tables(objdump: &Path, object: &Path) -> ExceptionTables {
    let objdump_output = |arg: &str| {
        let output = Command::new(objdump).arg(arg).arg(object).output().unwrap();
        assert!(output.status.success(), "llvm-objdump failed on {}", object.display());
        String::from_utf8(output.stdout).unwrap()
    };

    // `llvm-objdump --dwarf=frames` prints `<offset> <length> <id> CIE`, followed by its
    // augmentation, and `<offset> <length> <id> FDE cie=<offset> ...` for every FDE.
    let frames = objdump_output("--dwarf=frames");
    let mut cies_with_personality = BTreeSet::new();
    let mut fde_cies = vec![];
    let mut cie_offset = "";
    for line in frames.lines().map(str::trim) {
        if line.ends_with(" CIE") {
            cie_offset = line.split_whitespace().next().unwrap();
        } else if let Some(augmentation) = line.strip_prefix("Augmentation:") {
            if augmentation.contains('P') {
                cies_with_personality.insert(cie_offset);
            }
        } else if let Some((_, fde)) = line.split_once(" FDE cie=") {
            fde_cies.push(fde.split_whitespace().next().unwrap());
        }
    }
    let fdes_with_personality =
        fde_cies.iter().filter(|cie| cies_with_personality.contains(*cie)).count();

    // Every LSDA is in its own section of the hex dump.
    let contents = objdump_output("-s");
    let mut lsdas = vec![];
    for line in contents.lines() {
        if line.starts_with("Contents of section") {
            lsdas.push(line.contains(" .gcc_except_table").then(Vec::new));
        } else if let Some(Some(lsda)) = lsdas.last_mut() {
            // ` 0000 ff031501 10200100 ...  ascii`
            let hex = line.get(6..41).unwrap_or_default();
            let digits = hex.bytes().filter(u8::is_ascii_hexdigit).collect::<Vec<_>>();
            lsda.extend(digits.chunks(2).map(|byte| {
                u8::from_str_radix(std::str::from_utf8(byte).unwrap(), 16).unwrap()
            }));
        }
    }

    let mut landing_pad_call_sites = lsdas
        .into_iter()
        .flatten()
        .map(|lsda| lsda_landing_pad_call_sites(&lsda))
        .collect::<Vec<_>>();
    landing_pad_call_sites.sort();
    ExceptionTables { fdes_with_personality, landing_pad_call_sites }
}

/// Returns the number of call site entries with a landing pad in an LSDA.
fn lsda_landing_pad_call_sites(mut lsda: &[u8]) -> usize {
    fn uleb128(bytes: &mut &[u8]) -> u64 {
        let mut value = 0;
        for shift in (0..).step_by(7) {
            let (&byte, rest) = bytes.split_first().unwrap();
            *bytes = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
        }
        value
    }

    let [lp_start_encoding, ttype_encoding, ..] = *lsda else { panic!("LSDA is too short") };
    assert_eq!(lp_start_encoding, 0xff, "unexpected landing pad base");
    lsda = &lsda[2..];
    if ttype_encoding != 0xff {
        uleb128(&mut lsda);
    }
    let call_site_encoding = lsda[0];
    assert_eq!(call_site_encoding, 0x01, "unexpected call site encoding");
    lsda = &lsda[1..];
    let call_sites_len = usize::try_from(uleb128(&mut lsda)).unwrap();
    let mut call_sites = &lsda[..call_sites_len];

    let mut landing_pad_call_sites = 0;
    while !call_sites.is_empty() {
        let _start = uleb128(&mut call_sites);
        let _length = uleb128(&mut call_sites);
        let landing_pad = uleb128(&mut call_sites);
        let _action = uleb128(&mut call_sites);
        landing_pad_call_sites += usize::from(landing_pad != 0);
    }
    landing_pad_call_sites
}
