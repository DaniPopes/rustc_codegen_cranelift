use std::path::Path;

use crate::build_sysroot::SysrootConfig;
use crate::path::Dirs;
use crate::prepare::GitRepo;
use crate::utils::{CargoProject, Compiler};
use crate::{CodegenBackend, abi_cafe_gen, build_sysroot};

/// How many tests to run in one abi-cafe invocation.
const TESTS_PER_RUN: usize = 16;

static ABI_CAFE_REPO: GitRepo = GitRepo::github(
    "Gankra",
    "abi-cafe",
    "94d38030419eb00a1ba80e5e2b4d763dcee58db4",
    &[],
    "6efb4457893c8670",
    "abi-cafe",
);

static ABI_CAFE: CargoProject = CargoProject::new(ABI_CAFE_REPO.source_dir(), "abi_cafe_target");

pub(crate) fn run(
    sysroot_config: &SysrootConfig,
    dirs: &Dirs,
    cg_clif_dylib: &CodegenBackend,
    rustup_toolchain_name: Option<&str>,
    bootstrap_host_compiler: &Compiler,
) {
    std::fs::create_dir_all(&dirs.download_dir).unwrap();
    ABI_CAFE_REPO.fetch(dirs);
    ABI_CAFE_REPO.patch(dirs);

    eprintln!("Building sysroot for abi-cafe");
    build_sysroot::build_sysroot(
        dirs,
        sysroot_config,
        cg_clif_dylib,
        bootstrap_host_compiler,
        rustup_toolchain_name,
        bootstrap_host_compiler.target.clone(),
    );

    eprintln!("Running abi-cafe");

    let pairs: &[_] =
        if cfg!(not(any(target_os = "macos", all(target_os = "windows", target_env = "msvc")))) {
            &["rustc_calls_cgclif", "cgclif_calls_rustc", "cgclif_calls_cc", "cc_calls_cgclif"]
        } else {
            &["rustc_calls_cgclif", "cgclif_calls_rustc"]
        };

    let generated_tests = abi_cafe_gen::generate(dirs);
    let mut tests = test_names(&ABI_CAFE.source_dir(dirs).join("include/tests"));
    tests.extend(test_names(&generated_tests));
    tests.sort();

    // Every test builds two static libraries that each contain the standard library. Delete them
    // after a few tests to keep the disk usage at a few GB instead of the tens of GB that all
    // tests together take.
    let temp_dir = ABI_CAFE.source_dir(dirs).join("target/temp");
    let mut failed = false;
    for chunk in tests.chunks(TESTS_PER_RUN) {
        let mut cmd = ABI_CAFE.run(bootstrap_host_compiler, dirs);
        cmd.arg("--");

        cmd.arg("--debug");

        cmd.arg("--add-tests").arg(&generated_tests);
        cmd.arg("--tests").arg(chunk.join(","));

        cmd.arg("--rules").arg(dirs.source_dir.join("scripts/abi-cafe-rules.toml"));

        // stdcall, vectorcall and such don't work yet
        cmd.arg("--conventions").arg("c").arg("--conventions").arg("rust");

        for pair in pairs {
            cmd.arg("--pairs").arg(pair);
        }

        cmd.arg("--add-rustc-codegen-backend");
        match cg_clif_dylib {
            CodegenBackend::Local(path) => {
                cmd.arg(format!("cgclif:{}", path.display()));
            }
            CodegenBackend::Builtin(name) => {
                cmd.arg(format!("cgclif:{name}"));
            }
        }

        cmd.current_dir(ABI_CAFE.source_dir(dirs));

        failed |= !cmd.spawn().unwrap().wait().unwrap().success();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    if failed {
        std::process::exit(1);
    }
}

/// The names of the tests in a directory of abi-cafe test files.
fn test_names(dir: &Path) -> Vec<String> {
    let mut names = vec![];
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            names.extend(test_names(&path));
        } else {
            names.push(
                path.file_name().unwrap().to_str().unwrap().split('.').next().unwrap().to_owned(),
            );
        }
    }
    names
}
