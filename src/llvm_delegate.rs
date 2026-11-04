//! Hands the whole codegen of optimized crates to rustc's built-in LLVM backend.
//!
//! Cranelift's optimizer is far weaker than LLVM's. A crate that is compiled with an optimizing
//! `-Copt-level` therefore gets its code from `rustc_codegen_llvm`, and Cranelift keeps compiling
//! the crates that are built without optimizations. Every `CodegenBackend` method is forwarded to
//! whichever backend `init` selected, so a delegated crate is built like any other LLVM crate,
//! including its `-Cllvm-args`, target features, LTO and linking.
//!
//! The decision is taken per rustc invocation, from `-Copt-level`. It can be changed with
//! `CG_CLIF_LLVM_OPT_LEVEL`, which takes the lowest opt-level that is delegated:
//! `off`, `1`, `2`, `3`, `s` or `z`. Ordered by optimization effort the levels are
//! `0 < 1 < z < s < 2 < 3`. The default is `z`, so every optimizing level is delegated.
//!
//! JIT mode (`-Cllvm-args=jit-mode`) is never delegated.
//!
//! The `llvm-delegate` cargo feature links this dylib against `rustc_codegen_llvm`, which the
//! dynamic loader resolves in `librustc_driver` when rustc loads the backend. A rustc built
//! without the LLVM backend can't load such a dylib. Build cg_clif without the feature for it, and
//! every crate is then compiled by Cranelift.

use std::any::Any;
use std::env;

use rustc_codegen_ssa::traits::CodegenBackend;
use rustc_codegen_ssa::{CompiledModules, CrateInfo, TargetConfig};
use rustc_metadata::EncodedMetadata;
use rustc_metadata::creader::MetadataLoaderDyn;
use rustc_middle::dep_graph::WorkProductMap;
use rustc_middle::ty::TyCtxt;
use rustc_middle::util::Providers;
use rustc_session::config::{OptLevel, OutputFilenames, PrintRequest};
use rustc_session::{CodegenBackendInit, EarlySession, IncrCompSession, Session};
use rustc_structures::CrateType;

use crate::{BackendConfig, CraneliftCodegenBackend};

const OPT_LEVEL_ENV: &str = "CG_CLIF_LLVM_OPT_LEVEL";

/// An optimization level, ordered by optimization effort.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Effort {
    None,
    Less,
    SizeMin,
    Size,
    Default,
    Aggressive,
}

impl Effort {
    fn of(level: OptLevel) -> Self {
        match level {
            OptLevel::No => Effort::None,
            OptLevel::Less => Effort::Less,
            OptLevel::SizeMin => Effort::SizeMin,
            OptLevel::Size => Effort::Size,
            OptLevel::More => Effort::Default,
            OptLevel::Aggressive => Effort::Aggressive,
        }
    }
}

/// The lowest [`Effort`] that is delegated to LLVM, or `None` if nothing is.
fn threshold() -> Result<Option<Effort>, String> {
    match env::var(OPT_LEVEL_ENV).as_deref() {
        Err(env::VarError::NotPresent) | Ok("z") => Ok(Some(Effort::SizeMin)),
        Ok("off") => Ok(None),
        Ok("1") => Ok(Some(Effort::Less)),
        Ok("s") => Ok(Some(Effort::Size)),
        Ok("2") => Ok(Some(Effort::Default)),
        Ok("3") => Ok(Some(Effort::Aggressive)),
        Ok(other) => Err(format!(
            "invalid {OPT_LEVEL_ENV}=`{other}`, expected `off`, `1`, `2`, `3`, `s` or `z`"
        )),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{OPT_LEVEL_ENV} is not unicode")),
    }
}

#[cfg(feature = "llvm-delegate")]
fn new_llvm() -> Option<Box<dyn CodegenBackend>> {
    Some(rustc_codegen_llvm::LlvmCodegenBackend::new())
}

#[cfg(not(feature = "llvm-delegate"))]
fn new_llvm() -> Option<Box<dyn CodegenBackend>> {
    None
}

/// The `CodegenBackend` that cg_clif registers with rustc.
pub(crate) struct LlvmDelegateBackend {
    cranelift: CraneliftCodegenBackend,
    /// Set by `init` if this invocation is delegated to LLVM.
    llvm: Option<Box<dyn CodegenBackend>>,
}

impl LlvmDelegateBackend {
    pub(crate) fn new() -> Self {
        LlvmDelegateBackend { cranelift: CraneliftCodegenBackend { config: None }, llvm: None }
    }

    /// The backend that compiles this crate.
    fn active(&self) -> &dyn CodegenBackend {
        match &self.llvm {
            Some(llvm) => &**llvm,
            None => &self.cranelift,
        }
    }
}

impl CodegenBackend for LlvmDelegateBackend {
    fn name(&self) -> &'static str {
        self.active().name()
    }

    fn init(&mut self, sess: &EarlySession) -> CodegenBackendInit {
        let config = self.cranelift.config.get_or_insert_with(|| {
            BackendConfig::from_opts(&sess.opts.cg.llvm_args)
                .unwrap_or_else(|err| sess.dcx().fatal(err))
        });
        let threshold = threshold().unwrap_or_else(|err| sess.dcx().fatal(err));

        if !config.jit_mode
            && let Some(threshold) = threshold
            && Effort::of(sess.opts.optimize) >= threshold
        {
            self.llvm = new_llvm();
            if let Some(llvm) = &mut self.llvm {
                return llvm.init(sess);
            }
        }

        self.cranelift.init(sess)
    }

    fn print(&self, req: &PrintRequest, out: &mut String, sess: &Session) {
        self.active().print(req, out, sess)
    }

    fn target_config(&self, sess: &EarlySession) -> TargetConfig {
        self.active().target_config(sess)
    }

    fn supported_crate_types(&self, sess: &Session) -> Vec<CrateType> {
        self.active().supported_crate_types(sess)
    }

    fn print_passes(&self) {
        // This runs before `init`, and only LLVM has passes.
        if let Some(llvm) = new_llvm() {
            llvm.print_passes();
        }
    }

    fn print_version(&self) {
        // This runs before `init`, so print the versions of both backends.
        self.cranelift.print_version();
        if let Some(llvm) = new_llvm() {
            llvm.print_version();
        }
    }

    fn has_zstd(&self) -> bool {
        self.active().has_zstd()
    }

    fn has_mnemonic(&self, sess: &Session, mnemonic: &str) -> bool {
        self.active().has_mnemonic(sess, mnemonic)
    }

    fn metadata_loader(&self) -> Box<MetadataLoaderDyn> {
        self.active().metadata_loader()
    }

    fn provide(&self, providers: &mut Providers) {
        self.active().provide(providers)
    }

    fn target_cpu(&self, sess: &Session) -> String {
        self.active().target_cpu(sess)
    }

    fn codegen_crate(&self, tcx: TyCtxt<'_>) -> Box<dyn Any> {
        self.active().codegen_crate(tcx)
    }

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        sess: &Session,
        incr_comp_session: Option<&IncrCompSession>,
        outputs: &OutputFilenames,
        crate_info: &CrateInfo,
    ) -> (CompiledModules, WorkProductMap) {
        self.active().join_codegen(ongoing_codegen, sess, incr_comp_session, outputs, crate_info)
    }

    fn link(
        &self,
        sess: &Session,
        compiled_modules: CompiledModules,
        crate_info: CrateInfo,
        metadata: EncodedMetadata,
        outputs: &OutputFilenames,
    ) {
        self.active().link(sess, compiled_modules, crate_info, metadata, outputs)
    }
}
