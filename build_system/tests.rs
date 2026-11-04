use std::collections::BTreeSet;
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
    TestCase::build_lib("build.example", "example/example.rs", "lib"),
    TestCase::jit_bin("jit.mini_core_hello_world", "example/mini_core_hello_world.rs", "abc bcd"),
    TestCase::build_bin_and_run(
        "aot.mini_core_hello_world",
        "example/mini_core_hello_world.rs",
        &["abc", "bcd"],
    ),
    // Mach-O only supports 255 sections, so data objects that share a custom section have to be
    // emitted into one section.
    TestCase::custom("aot.macho_many_custom_sections", &|runner| {
        let mut source = String::from(
            "#![feature(no_core, lang_items)]\n\
             #![allow(internal_features)]\n\
             #![no_core]\n\
             #[lang = \"pointee_sized\"] trait PointeeSized {}\n\
             #[lang = \"meta_sized\"] trait MetaSized: PointeeSized {}\n\
             #[lang = \"sized\"] trait Sized: MetaSized {}\n\
             #[lang = \"sync\"] unsafe trait Sync {}\n\
             #[lang = \"drop_glue\"] unsafe fn drop_glue<T: ?Sized>(_: &mut T) {}\n\
             unsafe impl Sync for u8 {}\n",
        );
        for i in 0..300 {
            source.push_str(&format!(
                "#[used] #[unsafe(link_section = \"__DATA,__mod_init_func,mod_init_funcs\")] \
                 static S{i}: u8 = 0;\n"
            ));
        }
        let out_dir = BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs);
        let source_path = out_dir.join("macho_many_custom_sections.rs");
        fs::write(&source_path, source).unwrap();

        let cmd = runner.rustc_command_for_target(
            "aarch64-apple-darwin",
            [source_path.as_os_str(), OsStr::new("--crate-type=lib"), OsStr::new("--emit=obj")],
        );
        spawn_and_wait(cmd);
    }),
];

const BASE_SYSROOT_SUITE: &[TestCase] = &[
    TestCase::build_bin_and_run(
        "aot.arbitrary_self_types_pointers_and_wrappers",
        "example/arbitrary_self_types_pointers_and_wrappers.rs",
        &[],
    ),
    TestCase::jit_bin("jit.std_example", "example/std_example.rs", "arg"),
    TestCase::build_bin_and_run("aot.std_example", "example/std_example.rs", &["arg"]),
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
    TestCase::custom("aot.abi_conformance", &|runner| {
        let target = &runner.target_compiler.target;
        // The cases assume a 64-bit x86_64 or aarch64 ABI that is not the Windows one.
        if !(target.starts_with("x86_64") || target.starts_with("aarch64"))
            || target.contains("windows")
        {
            eprintln!("Skipping `abi_conformance` test: unsupported target");
            return;
        }

        let out_dir = BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs).join("abi-conformance");
        let source = "example/abi-conformance.rs";

        // Calls in both directions between the backends, and between each backend and itself to
        // check the test case.
        let mut failed = false;
        for (callee, caller) in
            [("llvm", "llvm"), ("clif", "clif"), ("llvm", "clif"), ("clif", "llvm")]
        {
            eprintln!("abi-conformance: {callee} callee, {caller} caller");
            let out_dir = out_dir.join(format!("{callee}_{caller}"));
            ensure_empty_dir(&out_dir);

            let rustc = |backend: &str, args: &[&str]| {
                let mut cmd = runner.rustc_command_in(&out_dir, &runner.target_compiler.target, [source]);
                cmd.arg("--check-cfg=cfg(callee, caller, variadic, variadic_float)");
                if backend == "llvm" {
                    cmd.arg("-Zcodegen-backend=llvm");
                }
                // Cranelift can't define variadic functions.
                if callee == "llvm" {
                    cmd.args(["--cfg", "variadic"]);
                    // Cranelift can't call variadic functions with float arguments on x86_64.
                    if caller == "llvm" || runner.target_compiler.target.starts_with("aarch64") {
                        cmd.args(["--cfg", "variadic_float"]);
                    }
                }
                cmd.args(args);
                spawn_and_wait(cmd);
            };

            rustc(
                callee,
                &["--cfg", "callee", "--crate-type", "rlib", "--crate-name", "abi_callee"],
            );
            rustc(caller, &["--cfg", "caller", "--crate-name", "abi_caller"]);

            {
                let mut cmd = runner.target_compiler.run_with_runner(out_dir.join("abi_caller"));
                let output = cmd.output().unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprint!("{stderr}");

                // Every mismatch is reported as `<case>: <details>`.
                let mismatches: BTreeSet<_> = stderr
                    .lines()
                    .filter_map(|line| line.split_once(": "))
                    .map(|(case, _)| case.to_owned())
                    .collect();
                let busted = busted_abi_conformance_cases(
                    &runner.dirs,
                    &runner.target_compiler.target,
                    &format!("{callee}_{caller}"),
                );
                let unexpected: Vec<_> = mismatches.difference(&busted).collect();
                let fixed: Vec<_> = busted.difference(&mismatches).collect();
                if !unexpected.is_empty()
                    || !fixed.is_empty()
                    || (!output.status.success() && mismatches.is_empty())
                {
                    eprintln!(
                        "abi-conformance: unexpected mismatches: {unexpected:?}, \
                         cases in scripts/abi-conformance-busted.txt that now pass: {fixed:?}"
                    );
                    failed = true;
                }
            }
        }
        if failed {
            std::process::exit(1);
        }
    }),
    TestCase::build_bin_and_run("aot.issue-72793", "example/issue-72793.rs", &[]),
    // Call every function of a crate built with one backend from a crate built with the other one.
    TestCase::custom("aot.mixed_backend_abi", &|runner| {
        let llvm = "-Zcodegen-backend=llvm";
        for (name, callee_flag, caller_flag) in
            [("llvm", Some(llvm), None), ("clif", None, Some(llvm))]
        {
            let callee = format!("callee_{name}");
            let mut args =
                vec!["example/mixed-backend-abi-callee.rs", "--crate-type=rlib", "--crate-name"];
            args.push(&callee);
            args.extend(callee_flag);
            runner.run_rustc(args);

            let caller = format!("mixed_backend_abi_{name}");
            let callee_rlib =
                BUILD_EXAMPLE_OUT_DIR.to_path(&runner.dirs).join(format!("lib{callee}.rlib"));
            let callee_extern = format!("callee={}", callee_rlib.display());
            let mut args = vec!["example/mixed-backend-abi.rs", "--crate-name"];
            args.push(&caller);
            args.extend(["--extern", &callee_extern]);
            args.extend(caller_flag);
            runner.run_rustc(args);
            runner.run_out_command(&caller, &[]);
        }
    }),
    TestCase::build_bin("aot.issue-59326", "example/issue-59326.rs"),
    TestCase::build_bin_and_run("aot.gen_block_iterate", "example/gen_block_iterate.rs", &[]),
    TestCase::build_bin_and_run("aot.raw-dylib", "example/raw-dylib.rs", &[]),
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

/// The cases of `example/abi-conformance.rs` that are known to mismatch for a target and a
/// callee and caller backend pair.
fn busted_abi_conformance_cases(dirs: &Dirs, target: &str, backends: &str) -> BTreeSet<String> {
    let busted = fs::read_to_string(dirs.source_dir.join("scripts/abi-conformance-busted.txt"));
    busted
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (target_prefix, line_backends, case) =
                (parts.next()?, parts.next()?, parts.next()?);
            (target.starts_with(target_prefix) && line_backends == backends)
                .then(|| case.to_owned())
        })
        .collect()
}

struct TestRunner<'a> {
    is_native: bool,
    jit_supported: bool,
    panic_unwind_support: bool,
    skip_tests: &'a [&'a str],
    dirs: Dirs,
    target_compiler: Compiler,
    stdlib_source: PathBuf,
}

impl<'a> TestRunner<'a> {
    fn new(
        dirs: Dirs,
        mut target_compiler: Compiler,
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
        self.rustc_command_for_target(&self.target_compiler.target, args)
    }

    #[must_use]
    fn rustc_command_for_target<I, S>(&self, target: &str, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.rustc_command_in(&BUILD_EXAMPLE_OUT_DIR.to_path(&self.dirs), target, args)
    }

    #[must_use]
    fn rustc_command_in<I, S>(&self, out_dir: &Path, target: &str, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(&self.target_compiler.rustc);
        cmd.args(&self.target_compiler.rustflags);
        cmd.arg("-L");
        cmd.arg(format!("crate={}", out_dir.display()));
        cmd.arg("--out-dir");
        cmd.arg(out_dir);
        cmd.arg("-Cdebuginfo=2");
        cmd.arg("--target");
        cmd.arg(target);
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
