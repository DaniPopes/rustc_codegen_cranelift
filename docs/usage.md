# Usage

rustc_codegen_cranelift can be used as a near-drop-in replacement for `cargo build` or `cargo run` for existing projects.

Assuming `$cg_clif_dir` is the directory you cloned this repo into and you followed the instructions (`y.sh prepare` and `y.sh build` or `test.sh`).

## Cargo

In the directory with your project (where you can do the usual `cargo build`), run:

```bash
$ $cg_clif_dir/dist/cargo-clif build
```

This will build your project with rustc_codegen_cranelift instead of the usual LLVM backend.

## Optimized crates are compiled by LLVM

Cranelift's optimizer is far weaker than LLVM's. When a crate is compiled with `-Copt-level` 2, 3, `s` or `z` (for example through a `[profile.dev.package.hot] opt-level = 3` entry), rustc_codegen_cranelift hands the whole crate to rustc's built-in LLVM backend. Crates at `opt-level` 0 and 1 are compiled by Cranelift. No configuration is needed. LLVM does the codegen of a delegated crate and honors `-Cllvm-args` and `-Ctarget-cpu`, and its object files link with the ones from Cranelift. `-Clto` only works when every upstream crate, including the sysroot, was built by LLVM (see `--sysroot llvm`), as Cranelift objects contain no bitcode.

Set `CG_CLIF_LLVM_OPT_LEVEL` to the lowest `opt-level` that is delegated: `off`, `1`, `2`, `3`, `s` or `z`. Ordered by optimization effort the levels are `0 < 1 < z < s < 2 < 3`, and the default is `z`. `off` compiles everything with Cranelift, which `./y.sh build` and `./y.sh test` use unless the variable is set. Cargo does not track the variable, so rebuild after changing it.

`-Cllvm-args` options starting with `-` belong to LLVM, and Cranelift ignores them. Other options are rustc_codegen_cranelift options, like `jit-mode`, and an unknown one is an error. JIT mode is never delegated.

`cfg(target_feature)` comes from the backend that compiles the crate, so a delegated crate can see more target features than a Cranelift crate of the same build, for example with `-Ctarget-cpu=native`.

Delegation needs a rustc that contains the LLVM backend. A dylib built with the default `llvm-delegate` cargo feature links against `rustc_codegen_llvm`, and rustc fails to load it with an `undefined symbol` error if rustc does not have it. Build with `--no-default-features` for such a rustc.

## Rustc

> You should prefer using the Cargo method.

```bash
$ $cg_clif_dir/dist/rustc-clif my_crate.rs
```

## Jit mode

> ⚠⚠⚠ The JIT mode is highly experimental. It may be slower than AOT compilation due to lack of incremental compilation. It may also be hard to setup if you have cargo dependencies. ⚠⚠⚠

In jit mode cg_clif will immediately execute your code without creating an executable file.

> This requires all dependencies to be available as dynamic library.
> The jit mode will probably need cargo integration to make this possible.

```bash
$ $cg_clif_dir/dist/cargo-clif jit
```

or

```bash
$ $cg_clif_dir/dist/rustc-clif -Cllvm-args=jit-mode -Cprefer-dynamic my_crate.rs
```

## Shell

These are a few functions that allow you to easily run rust code from the shell using cg_clif as jit.

```bash
function jit_naked() {
    echo "$@" | $cg_clif_dir/dist/rustc-clif - -Zunstable-options -Cllvm-args=jit-mode -Cprefer-dynamic
}

function jit() {
    jit_naked "fn main() { $@ }"
}

function jit_calc() {
    jit 'println!("0x{:x}", ' $@ ');';
}
```
