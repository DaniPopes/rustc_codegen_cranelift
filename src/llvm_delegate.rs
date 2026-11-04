//! Hands the whole codegen of optimized crates to rustc's built-in LLVM backend.
//!
//! A crate that is compiled with an optimizing `-Copt-level` gets its code from
//! `rustc_codegen_llvm`, and Cranelift keeps compiling the rest. Every `CodegenBackend` method is
//! forwarded to whichever backend `init` selected, so a delegated crate is built like any other
//! LLVM crate. See `docs/usage.md` for the threshold and `CG_CLIF_LLVM_OPT_LEVEL`.

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

/// Orders optimization levels by optimization effort.
fn effort(level: OptLevel) -> u8 {
    match level {
        OptLevel::No => 0,
        OptLevel::Less => 1,
        OptLevel::SizeMin => 2,
        OptLevel::Size => 3,
        OptLevel::More => 4,
        OptLevel::Aggressive => 5,
    }
}

/// The lowest [`OptLevel`] that is delegated to LLVM, or `None` if nothing is.
///
/// The default is `z`, which delegates `z`, `s`, 2 and 3 but not 1.
fn threshold() -> Result<Option<OptLevel>, String> {
    match env::var(OPT_LEVEL_ENV).as_deref() {
        Err(env::VarError::NotPresent) | Ok("z") => Ok(Some(OptLevel::SizeMin)),
        Ok("off") => Ok(None),
        Ok("1") => Ok(Some(OptLevel::Less)),
        Ok("s") => Ok(Some(OptLevel::Size)),
        Ok("2") => Ok(Some(OptLevel::More)),
        Ok("3") => Ok(Some(OptLevel::Aggressive)),
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

// Forwards every method of `CodegenBackend`: one that is missing here would silently use the trait
// default even when LLVM is active.
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
            && effort(sess.opts.optimize) >= effort(threshold)
        {
            if let Some(mut llvm) = new_llvm() {
                let init = llvm.init(sess);
                self.llvm = Some(llvm);
                return init;
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
