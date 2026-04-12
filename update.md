# Updating Global Toolchain + Cranelift Backend

When updating `rustc_codegen_cranelift` to a new nightly (i.e. after pulling upstream changes
that bump `rust-toolchain.toml`), follow these steps **in order** to avoid ending up with a
default toolchain that has no matching cranelift backend.

## 1. Check the target nightly version

```bash
cat rust-toolchain.toml
# e.g. channel = "nightly-2026-04-01"
```

## 2. Install the new nightly toolchain (don't switch yet)

```bash
rustup toolchain install nightly-2026-04-01 --component rust-src rustc-dev llvm-tools rustfmt clippy rust-analyzer
```

## 3. Build the cranelift backend against the new toolchain

The `rust-toolchain.toml` in this repo pins the correct nightly, so `./y.sh` will
automatically use it.

```bash
./y.sh prepare  # only needed on first build or after git clean
./y.sh build
```

This produces `dist/cargo-clif`, `dist/rustc-clif`, etc.

## 4. (Optional) Install the rustup cranelift component

If you prefer using `rustup component add` instead of the self-built `dist/` binaries:

```bash
rustup component add rustc-codegen-cranelift-preview --toolchain nightly-2026-04-01
```

## 5. Switch global default to the new nightly

Only do this **after** step 3 (or 4) completes — switching before means your global
toolchain will try to use a cranelift that doesn't exist yet or is built for a different
nightly.

```bash
rustup default nightly-2026-04-01
```

## 6. Verify

```bash
rustc --version
# should show the new nightly

# If using self-built backend:
./dist/cargo-clif build  # in any project

# If using rustup component:
CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift cargo build -Zcodegen-backend
```

## Using cranelift globally via `.cargo/config.toml`

To enable cranelift for all `dev` builds without per-project config, add to
`~/.cargo/config.toml`:

```toml
[unstable]
codegen-backend = true

[profile.dev]
codegen-backend = "cranelift"
```

This requires the `rustc-codegen-cranelift-preview` rustup component (step 4).
For the self-built backend, use `dist/cargo-clif` directly instead.
