// tidy-alphabetical-start
// Note: please avoid adding other feature gates where possible
#![feature(rustc_private)]
// Only used to define intrinsics in `compiler_builtins.rs`.
#![cfg_attr(feature = "jit", feature(f16))]
#![cfg_attr(feature = "jit", feature(f128))]
// Note: please avoid adding other feature gates where possible
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]
#![warn(unused_lifetimes)]
// tidy-alphabetical-end

extern crate rustc_abi;
extern crate rustc_ast;
extern crate rustc_attr_ir;
extern crate rustc_codegen_llvm;
extern crate rustc_codegen_ssa;
extern crate rustc_const_eval;
extern crate rustc_data_structures;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_index;
extern crate rustc_log;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
#[cfg(feature = "jit")]
extern crate rustc_structures;
extern crate rustc_symbol_mangling;
extern crate rustc_target;

// This prevents duplicating functions and statics that are already part of the host rustc process.
#[allow(unused_extern_crates)]
extern crate rustc_driver;

use std::any::Any;
use std::env;
use std::sync::Arc;

use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings::{self, Configurable};
use rustc_codegen_llvm::LlvmCodegenBackend;
use rustc_codegen_ssa::traits::CodegenBackend;
use rustc_codegen_ssa::{CompiledModules, CrateInfo, TargetConfig, back};
use rustc_data_structures::unord::UnordSet;
use rustc_log::tracing::info;
use rustc_middle::dep_graph::WorkProductMap;
use rustc_session::config::{NATIVE_CPU, OutputFilenames};
use rustc_session::{CodegenBackendInit, EarlySession, IncrCompSession, Session};
use rustc_span::{Symbol, sym};
use rustc_target::spec::{Arch, CfgAbi, Env, Os};

pub use crate::config::*;
use crate::prelude::*;

mod abi;
mod allocator;
mod analyze;
mod base;
mod cast;
mod codegen_f16_f128;
mod codegen_i128;
mod common;
mod compiler_builtins;
mod config;
mod constant;
mod debuginfo;
mod discriminant;
mod driver;
mod global_asm;
mod inline_asm;
mod intrinsics;
mod linkage;
mod main_shim;
mod num;
mod optimize;
mod pointer;
mod pretty_clif;
mod toolchain;
mod unsize;
mod unwind_module;
mod value_and_place;
mod vtable;

mod prelude {
    pub(crate) use cranelift_codegen::Context;
    pub(crate) use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
    pub(crate) use cranelift_codegen::ir::function::Function;
    pub(crate) use cranelift_codegen::ir::{
        AbiParam, Block, FuncRef, Inst, InstBuilder, MemFlagsData, Signature, SourceLoc, StackSlot,
        StackSlotData, StackSlotKind, TrapCode, Type, Value, types,
    };
    pub(crate) use cranelift_module::{self, DataDescription, FuncId, Linkage, Module};
    pub(crate) use rustc_abi::{BackendRepr, FIRST_VARIANT, FieldIdx, Scalar, Size, VariantIdx};
    pub(crate) use rustc_data_structures::fx::FxHashMap;
    pub(crate) use rustc_hir::def_id::{DefId, LOCAL_CRATE};
    pub(crate) use rustc_index::Idx;
    pub(crate) use rustc_middle::mir::{self, *};
    pub(crate) use rustc_middle::ty::layout::{LayoutOf, TyAndLayout};
    pub(crate) use rustc_middle::ty::{
        self, FloatTy, Instance, InstanceKind, IntTy, Ty, TyCtxt, UintTy,
    };
    pub(crate) use rustc_span::{Span, bug, span_bug};

    pub(crate) use crate::abi::*;
    pub(crate) use crate::base::{codegen_operand, codegen_place};
    pub(crate) use crate::cast::*;
    pub(crate) use crate::common::*;
    pub(crate) use crate::debuginfo::{DebugContext, UnwindContext};
    pub(crate) use crate::pointer::Pointer;
    pub(crate) use crate::value_and_place::{CPlace, CValue};
}

struct PrintOnPanic<F: Fn() -> String>(F);
impl<F: Fn() -> String> Drop for PrintOnPanic<F> {
    fn drop(&mut self) {
        if ::std::thread::panicking() {
            println!("{}", (self.0)());
        }
    }
}

pub struct CraneliftCodegenBackend {
    // Set by `init` if not already set. (E.g. by cg_clif.)
    pub config: Option<BackendConfig>,
    /// Used to report the same target features as the LLVM backend would. Its `init` is run by our
    /// own `init`.
    llvm: Box<dyn CodegenBackend>,
}

impl CodegenBackend for CraneliftCodegenBackend {
    fn name(&self) -> &'static str {
        "cranelift"
    }

    fn init(&mut self, sess: &EarlySession) -> CodegenBackendInit {
        use rustc_session::config::{InstrumentCoverage, LtoCli};

        match (sess.target.requires_lto, sess.early_lto()) {
            (true, _) | (false, LtoCli::Yes | LtoCli::Fat | LtoCli::NoParam | LtoCli::Thin) => {
                sess.dcx().fatal("LTO is not supported by rustc_codegen_cranelift")
            }
            (false, LtoCli::Unspecified | LtoCli::No) => {}
        }

        if sess.opts.cg.instrument_coverage() != InstrumentCoverage::No {
            sess.dcx()
                .fatal("`-Cinstrument-coverage` is LLVM specific and not supported by Cranelift");
        }

        // Set `config` if not already set.
        let config = self.config.get_or_insert_with(|| {
            BackendConfig::from_opts(&sess.opts.cg.llvm_args)
                .unwrap_or_else(|err| sess.dcx().fatal(err))
        });

        if config.jit_mode && !sess.opts.output_types.should_codegen() {
            sess.dcx().fatal("JIT mode doesn't work with `cargo check`");
        }

        // LLVM's `init` configures LLVM using `-Cllvm-args`, which it fails on for `jit-mode`.
        if !config.jit_mode {
            self.llvm.init(sess);
        }

        CodegenBackendInit {
            global_backend_features: vec![],
            // FIXME workaround for rust-lang/rustc_codegen_cranelift#1717
            replaced_intrinsics: vec![sym::fma],
            fallback_intrinsics: vec![sym::type_id_eq],
            thin_lto_supported: false,
        }
    }

    fn target_config(&self, sess: &EarlySession) -> TargetConfig {
        let mut target_config = if self.config.as_ref().is_some_and(|config| config.jit_mode) {
            fixed_target_config(sess)
        } else {
            self.llvm.target_config(sess)
        };

        // FIXME(f16_f128): `rustc_codegen_llvm` currently disables support on Windows GNU
        // targets due to GCC using a different ABI than LLVM. Therefore `f16` and `f128`
        // won't be available when using a LLVM-built sysroot.
        let has_reliable_f16_f128 = !(sess.target.arch == Arch::X86_64
            && sess.target.os == Os::Windows
            && sess.target.env == Env::Gnu
            && sess.target.cfg_abi != CfgAbi::Llvm);

        // FIXME(f128): f128 math operations need f128 math symbols, which currently aren't always
        // filled in by compiler-builtins. The only libc that provides these currently is glibc.
        let has_reliable_f128_math = has_reliable_f16_f128 && sess.target.env == Env::Gnu;

        // `rustc_codegen_cranelift` polyfills functionality not yet available in Cranelift.
        target_config.has_reliable_f16 = has_reliable_f16_f128;
        target_config.has_reliable_f16_math = has_reliable_f16_f128;
        target_config.has_reliable_f16b = false;
        target_config.has_reliable_f128 = has_reliable_f16_f128;
        target_config.has_reliable_f128_math = has_reliable_f128_math;
        target_config
    }

    fn print_version(&self) {
        println!("Cranelift version: {}", cranelift_codegen::VERSION);
    }

    fn has_mnemonic(&self, sess: &Session, mnemonic: &str) -> bool {
        // All Cranelift supported targets support ret except for s390x
        mnemonic == "ret" && sess.target.arch != Arch::S390x
    }

    fn target_cpu(&self, sess: &Session) -> String {
        self.llvm.target_cpu(sess)
    }

    fn codegen_crate(&self, tcx: TyCtxt<'_>) -> Box<dyn Any> {
        info!("codegen crate {}", tcx.crate_name(LOCAL_CRATE));
        let config = self.config.as_ref().unwrap();
        if config.jit_mode {
            #[cfg(feature = "jit")]
            driver::jit::run_jit(tcx, self.target_cpu(tcx.sess), config.jit_args.clone());

            #[cfg(not(feature = "jit"))]
            tcx.dcx().fatal("jit support was disabled when compiling rustc_codegen_cranelift");
        } else {
            Box::new(rustc_codegen_ssa::base::codegen_crate(driver::aot::AotDriver, tcx))
        }
    }

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        sess: &Session,
        incr_comp_session: Option<&IncrCompSession>,
        _outputs: &OutputFilenames,
        crate_info: &CrateInfo,
    ) -> (CompiledModules, WorkProductMap) {
        ongoing_codegen
            .downcast::<rustc_codegen_ssa::back::write::OngoingCodegen<driver::aot::AotDriver>>()
            .unwrap()
            .join(sess, incr_comp_session, crate_info)
    }
}

/// Determine if the Cranelift ir verifier should run.
///
/// Returns true when `-Zverify-llvm-ir` is passed, the `CG_CLIF_ENABLE_VERIFIER` env var is set to
/// 1 or when cg_clif is compiled with debug assertions enabled or false otherwise.
fn enable_verifier(sess: &Session) -> bool {
    sess.verify_llvm_ir()
        || cfg!(debug_assertions)
        || env::var("CG_CLIF_ENABLE_VERIFIER").as_deref() == Ok("1")
}

fn target_tuple(sess: &Session) -> target_lexicon::Triple {
    // Use versioned target tuple to make `OperatingSystem::MacOSX(...)`
    // contain a value, which we use when emitting `LC_BUILD_VERSION`.
    match back::versioned_llvm_target(sess).parse() {
        Ok(tuple) => tuple,
        Err(err) => sess.dcx().fatal(format!("target not recognized: {}", err)),
    }
}

/// The target config used when LLVM isn't available to expand the target cpu into target
/// features. Only a fixed set of target features is reported.
fn fixed_target_config(sess: &EarlySession) -> TargetConfig {
    let target_features = match sess.target.arch {
        Arch::X86_64 if sess.target.os != Os::None => {
            // x86_64 mandates SSE2 support and rustc requires the x87 feature to be enabled
            vec![sym::fxsr, sym::sse, sym::sse2, Symbol::intern("x87")]
        }
        Arch::AArch64 => match &sess.target.os {
            Os::None => vec![],
            // On macOS the aes, sha2 and sha3 features are enabled by default and ring
            // fails to compile on macOS when they are not present.
            Os::MacOs => vec![sym::neon, sym::aes, sym::sha2, sym::sha3],
            // AArch64 mandates Neon support
            _ => vec![sym::neon],
        },
        _ => vec![],
    };
    TargetConfig {
        internal_target_features: UnordSet::from_iter(target_features),
        has_reliable_f16: true,
        has_reliable_f16_math: true,
        has_reliable_f16b: true,
        has_reliable_f128: true,
        has_reliable_f128_math: true,
    }
}

/// Rust target features and the Cranelift ISA flags they enable.
const X86_64_CRANELIFT_FEATURES: &[(&str, &str)] = &[
    ("avx", "has_avx"),
    ("avx2", "has_avx2"),
    ("avx512bitalg", "has_avx512bitalg"),
    ("avx512dq", "has_avx512dq"),
    ("avx512f", "has_avx512f"),
    ("avx512vbmi", "has_avx512vbmi"),
    ("avx512vl", "has_avx512vl"),
    ("avx512vnni", "has_avx512vnni"),
    ("avxvnni", "has_avx_vnni"),
    ("bmi1", "has_bmi1"),
    ("bmi2", "has_bmi2"),
    ("cmpxchg16b", "has_cmpxchg16b"),
    ("fma", "has_fma"),
    ("lzcnt", "has_lzcnt"),
    ("popcnt", "has_popcnt"),
    ("sse3", "has_sse3"),
    ("sse4.1", "has_sse41"),
    ("sse4.2", "has_sse42"),
    ("ssse3", "has_ssse3"),
];

const AARCH64_CRANELIFT_FEATURES: &[(&str, &str)] = &[
    ("dotprod", "has_dotprod"),
    ("fp16", "has_fp16"),
    ("i8mm", "has_i8mm"),
    ("lse", "has_lse"),
    ("paca", "has_pauth"),
];

fn build_isa(sess: &Session, jit: bool) -> Arc<dyn TargetIsa + 'static> {
    use target_lexicon::BinaryFormat;

    let target_tuple = crate::target_tuple(sess);

    let mut flags_builder = settings::builder();
    flags_builder.set("is_pic", if jit { "false" } else { "true" }).unwrap();
    let enable_verifier = if enable_verifier(sess) { "true" } else { "false" };
    flags_builder.set("enable_verifier", enable_verifier).unwrap();
    flags_builder.set("regalloc_checker", enable_verifier).unwrap();

    let frame_ptr =
        { sess.target.options.frame_pointer }.ratchet(sess.opts.cg.force_frame_pointers);
    let preserve_frame_pointer = frame_ptr != rustc_target::spec::FramePointer::MayOmit;
    flags_builder
        .set("preserve_frame_pointers", if preserve_frame_pointer { "true" } else { "false" })
        .unwrap();

    let tls_model = match target_tuple.binary_format {
        BinaryFormat::Elf => "elf_gd",
        BinaryFormat::Macho => "macho",
        BinaryFormat::Coff => "coff",
        _ => "none",
    };
    flags_builder.set("tls_model", tls_model).unwrap();

    flags_builder.set("enable_llvm_abi_extensions", "true").unwrap();

    if let Some(align) = sess.opts.unstable_opts.min_function_alignment {
        flags_builder
            .set("log2_min_function_alignment", &align.bytes().ilog2().to_string())
            .unwrap();
    }

    use rustc_session::config::OptLevel;
    match sess.opts.optimize {
        OptLevel::No => {
            flags_builder.set("opt_level", "none").unwrap();
        }
        OptLevel::Less
        | OptLevel::More
        | OptLevel::Size
        | OptLevel::SizeMin
        | OptLevel::Aggressive => {
            flags_builder.set("opt_level", "speed_and_size").unwrap();
        }
    }

    if let target_lexicon::OperatingSystem::Windows = target_tuple.operating_system {
        // FIXME remove dependency on this from the Rust ABI. cc bytecodealliance/wasmtime#9510
        flags_builder.enable("enable_multi_ret_implicit_sret").unwrap();
    }

    if let target_lexicon::Architecture::S390x = target_tuple.architecture {
        // FIXME remove dependency on this from the Rust ABI. cc bytecodealliance/wasmtime#9510
        flags_builder.enable("enable_multi_ret_implicit_sret").unwrap();
    }

    if let target_lexicon::Architecture::Aarch64(_)
    | target_lexicon::Architecture::Riscv64(_)
    | target_lexicon::Architecture::X86_64 = target_tuple.architecture
    {
        // Windows depends on stack probes to grow the committed part of the stack.
        // On other platforms it helps prevents stack smashing.
        flags_builder.enable("enable_probestack").unwrap();
        flags_builder.set("probestack_strategy", "inline").unwrap();
    } else {
        // __cranelift_probestack is not provided and inline stack probes are only supported on
        // AArch64, Riscv64 and x86_64.
        flags_builder.set("enable_probestack", "false").unwrap();
    }

    let flags = settings::Flags::new(flags_builder);

    let mut isa_builder = if sess.opts.cg.target_cpu.as_deref() == Some(NATIVE_CPU) {
        cranelift_native::builder_with_options(true).unwrap()
    } else {
        cranelift_codegen::isa::lookup(target_tuple.clone()).unwrap_or_else(|err| {
            sess.dcx().fatal(format!("can't compile for {}: {}", target_tuple, err));
        })
    };

    // The features computed by rustc already include the ones implied by the target cpu and
    // `-Ctarget-feature`, so don't pass the cpu name to Cranelift. In JIT mode LLVM isn't used to
    // compute them.
    let cranelift_features: &[(&str, &str)] = match target_tuple.architecture {
        _ if jit => &[],
        target_lexicon::Architecture::X86_64 => X86_64_CRANELIFT_FEATURES,
        target_lexicon::Architecture::Aarch64(_) => AARCH64_CRANELIFT_FEATURES,
        _ => &[],
    };
    for &(rust_feature, cranelift_flag) in cranelift_features {
        let enabled = sess.internal_target_features.contains(&Symbol::intern(rust_feature));
        isa_builder.set(cranelift_flag, if enabled { "true" } else { "false" }).unwrap();
    }

    match isa_builder.finish(flags) {
        Ok(target_isa) => target_isa,
        Err(err) => sess.dcx().fatal(format!("failed to build TargetIsa: {}", err)),
    }
}

/// This is the entrypoint for a hot plugged rustc_codegen_cranelift
#[unsafe(no_mangle)]
pub fn __rustc_codegen_backend() -> Box<dyn CodegenBackend> {
    Box::new(CraneliftCodegenBackend { config: None, llvm: LlvmCodegenBackend::new() })
}
