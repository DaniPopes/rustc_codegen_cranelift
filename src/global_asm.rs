//! The AOT driver uses [`cranelift_object`] to write object files suitable for linking into a
//! standalone executable.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rustc_ast::{InlineAsmOptions, InlineAsmTemplatePiece};
use rustc_codegen_ssa::traits::{AsmCodegenMethods, GlobalAsmOperandRef};
use rustc_data_structures::fx::FxIndexSet;
use rustc_middle::mir::interpret::{GlobalAlloc, PointerArithmetic, Scalar as ConstScalar};
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::layout::{
    FnAbiError, FnAbiOfHelpers, FnAbiRequest, HasTyCtxt, HasTypingEnv, LayoutError, LayoutOfHelpers,
};
use rustc_session::Session;
use rustc_target::asm::InlineAsmArch;

use crate::llvm_fallback::{LlvmWrappers, SUBPROCESS_AVAILABLE};
use crate::prelude::*;

pub(crate) struct GlobalAsmContext<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub global_asm: &'a mut String,
}

impl<'tcx> AsmCodegenMethods<'tcx> for GlobalAsmContext<'_, 'tcx> {
    fn codegen_global_asm(
        &mut self,
        template: &[InlineAsmTemplatePiece],
        operands: &[GlobalAsmOperandRef<'tcx>],
        options: InlineAsmOptions,
        _line_spans: &[Span],
        _extra_rust_target_features: &[String],
    ) {
        codegen_global_asm_inner(self.tcx, self.global_asm, template, operands, options);
    }

    fn mangled_name(&self, instance: Instance<'tcx>) -> String {
        let symbol_name = self.tcx.symbol_name(instance).name.to_owned();
        if self.tcx.sess.target.is_like_darwin { format!("_{symbol_name}") } else { symbol_name }
    }
}

impl<'tcx> LayoutOfHelpers<'tcx> for GlobalAsmContext<'_, 'tcx> {
    #[inline]
    fn handle_layout_err(&self, err: LayoutError<'tcx>, span: Span, ty: Ty<'tcx>) -> ! {
        if let LayoutError::SizeOverflow(_)
        | LayoutError::InvalidSimd { .. }
        | LayoutError::ReferencesError(_) = err
        {
            self.tcx.sess.dcx().span_fatal(span, err.to_string())
        } else {
            self.tcx
                .sess
                .dcx()
                .span_fatal(span, format!("failed to get layout for `{}`: {}", ty, err))
        }
    }
}

impl<'tcx> FnAbiOfHelpers<'tcx> for GlobalAsmContext<'_, 'tcx> {
    #[inline]
    fn handle_fn_abi_err(
        &self,
        err: FnAbiError<'tcx>,
        span: Span,
        fn_abi_request: FnAbiRequest<'tcx>,
    ) -> ! {
        FullyMonomorphizedLayoutCx(self.tcx).handle_fn_abi_err(err, span, fn_abi_request)
    }
}

impl<'tcx> HasTyCtxt<'tcx> for GlobalAsmContext<'_, 'tcx> {
    fn tcx<'b>(&'b self) -> TyCtxt<'tcx> {
        self.tcx
    }
}

impl<'tcx> rustc_abi::HasDataLayout for GlobalAsmContext<'_, 'tcx> {
    fn data_layout(&self) -> &rustc_abi::TargetDataLayout {
        &self.tcx.data_layout
    }
}

impl<'tcx> HasTypingEnv<'tcx> for GlobalAsmContext<'_, 'tcx> {
    fn typing_env(&self) -> ty::TypingEnv<'tcx> {
        ty::TypingEnv::fully_monomorphized()
    }
}

fn codegen_global_asm_inner<'tcx>(
    tcx: TyCtxt<'tcx>,
    global_asm: &mut String,
    template: &[InlineAsmTemplatePiece],
    operands: &[GlobalAsmOperandRef<'tcx>],
    options: InlineAsmOptions,
) {
    let is_x86 = matches!(tcx.sess.asm_arch.unwrap(), InlineAsmArch::X86 | InlineAsmArch::X86_64);

    if is_x86 {
        if !options.contains(InlineAsmOptions::ATT_SYNTAX) {
            global_asm.push_str("\n.intel_syntax noprefix\n");
        } else {
            global_asm.push_str("\n.att_syntax\n");
        }
    }
    for piece in template {
        match *piece {
            InlineAsmTemplatePiece::String(ref s) => global_asm.push_str(s),
            InlineAsmTemplatePiece::Placeholder { operand_idx, modifier: _, span } => {
                use rustc_codegen_ssa::back::symbol_export::escape_symbol_name;
                match operands[operand_idx] {
                    GlobalAsmOperandRef::Const { value, ty } => {
                        match value {
                            ConstScalar::Int(int) => {
                                let string = rustc_codegen_ssa::common::asm_const_to_str(
                                    tcx,
                                    span,
                                    int,
                                    FullyMonomorphizedLayoutCx(tcx).layout_of(ty),
                                );
                                global_asm.push_str(&string);
                            }

                            ConstScalar::Ptr(ptr, _) => {
                                if cfg!(not(feature = "inline_asm_sym")) {
                                    tcx.dcx().span_err(
                                        span,
                                        "asm! and global_asm! sym operands are not yet supported",
                                    );
                                }

                                let (prov, offset) = ptr.prov_and_relative_offset();
                                let global_alloc = tcx.global_alloc(prov.alloc_id());
                                let symbol = match global_alloc {
                                    GlobalAlloc::Function { instance } => {
                                        // FIXME handle the case where the function was made private to the
                                        // current codegen unit
                                        tcx.symbol_name(instance)
                                    }
                                    GlobalAlloc::Static(def_id) => {
                                        let instance = Instance::mono(tcx, def_id);
                                        tcx.symbol_name(instance)
                                    }
                                    GlobalAlloc::Memory(_)
                                    | GlobalAlloc::VTable(..)
                                    | GlobalAlloc::TypeId { .. } => unreachable!(),
                                };
                                let symbol_name = if tcx.sess.target.is_like_darwin {
                                    format!("_{}", symbol.name)
                                } else {
                                    symbol.name.to_owned()
                                };
                                global_asm.push_str(&escape_symbol_name(tcx, &symbol_name, span));

                                if offset != Size::ZERO {
                                    let offset = tcx.sign_extend_to_target_isize(offset.bytes());
                                    write!(global_asm, "{offset:+}").unwrap();
                                }
                            }
                        }
                    }
                    GlobalAsmOperandRef::SymThreadLocalStatic { def_id } => {
                        if cfg!(not(feature = "inline_asm_sym")) {
                            tcx.dcx().span_err(
                                span,
                                "asm! and global_asm! sym operands are not yet supported",
                            );
                        }

                        let instance = Instance::mono(tcx, def_id);
                        let symbol = tcx.symbol_name(instance);
                        let symbol_name = if tcx.sess.target.is_like_darwin {
                            format!("_{}", symbol.name)
                        } else {
                            symbol.name.to_owned()
                        };

                        global_asm.push_str(&escape_symbol_name(tcx, &symbol_name, span));
                    }
                }
            }
        }
    }

    global_asm.push('\n');
    if is_x86 {
        global_asm.push_str(".att_syntax\n\n");
    }
}

#[derive(Debug)]
pub(crate) struct GlobalAsmConfig {
    assembler: PathBuf,
    target: String,
    target_cpu: Option<String>,
    target_feature: String,
}

impl GlobalAsmConfig {
    pub(crate) fn new(sess: &Session) -> Self {
        GlobalAsmConfig {
            assembler: crate::toolchain::get_toolchain_binary(sess, "as"),
            target: match &sess.opts.target_triple {
                rustc_target::spec::TargetTuple::TargetTuple(tuple) => tuple.clone(),
                rustc_target::spec::TargetTuple::TargetJson { path_for_rustdoc, .. } => {
                    path_for_rustdoc.to_str().unwrap().to_owned()
                }
            },
            target_cpu: sess.opts.cg.target_cpu.clone(),
            target_feature: sess.opts.cg.target_feature.clone(),
        }
    }
}

/// Lang items needed by the `#![no_core]` crate compiled by the LLVM backend.
const NO_CORE_PRELUDE: &str = r####"
#[lang = "pointee_sized"]
pub trait PointeeSized {}
#[lang = "meta_sized"]
pub trait MetaSized: PointeeSized {}
#[lang = "sized"]
pub trait Sized: MetaSized {}
#[lang = "freeze"]
pub unsafe auto trait Freeze {}
#[lang = "unpin"]
pub auto trait Unpin {}
#[lang = "unsafe_unpin"]
pub unsafe auto trait UnsafeUnpin {}
#[lang = "copy"]
pub trait Copy {}
impl Copy for bool {}
impl Copy for i8 {}
impl Copy for i16 {}
impl Copy for i32 {}
impl Copy for i64 {}
impl Copy for i128 {}
impl Copy for isize {}
impl Copy for u8 {}
impl Copy for u16 {}
impl Copy for u32 {}
impl Copy for u64 {}
impl Copy for u128 {}
impl Copy for usize {}
impl Copy for f16 {}
impl Copy for f32 {}
impl Copy for f64 {}
impl Copy for f128 {}
impl<T: PointeeSized> Copy for *const T {}
impl<T: PointeeSized> Copy for *mut T {}
"####;

/// Compiles `global_asm` and `llvm_wrappers` into an object file using the LLVM backend.
pub(crate) fn compile_global_asm(
    config: &GlobalAsmConfig,
    global_asm: String,
    llvm_wrappers: LlvmWrappers,
    global_asm_object_file: &Path,
) -> Result<(), String> {
    assert!(!global_asm.is_empty() || !llvm_wrappers.is_empty());

    // Remove all LLVM style comments
    let mut global_asm = global_asm
        .lines()
        .map(|line| if let Some(index) = line.find("//") { &line[0..index] } else { line })
        .collect::<Vec<_>>()
        .join("\n");
    global_asm.push('\n');

    // Assemble `global_asm`
    if !SUBPROCESS_AVAILABLE {
        assert!(llvm_wrappers.is_empty());

        let mut child = Command::new(&config.assembler)
            .arg("-o")
            .arg(&global_asm_object_file)
            .stdin(Stdio::piped())
            .spawn()
            .expect("Failed to spawn `as`.");
        child.stdin.take().unwrap().write_all(global_asm.as_bytes()).unwrap();
        let status = child.wait().expect("Failed to wait for `as`.");
        if !status.success() {
            return Err(format!("Failed to assemble `{}`", global_asm));
        }
    } else {
        let mut source = String::new();

        let mut feature_gates = FxIndexSet::default();
        feature_gates.extend(
            [
                "decl_macro",
                "no_core",
                "rustc_attrs",
                "f16",
                "f128",
                "lang_items",
                "auto_traits",
                "repr_simd",
                "simd_ffi",
                "link_llvm_intrinsics",
            ]
            .map(str::to_owned),
        );
        for wrapper in llvm_wrappers.values() {
            feature_gates.extend(wrapper.feature_gates.iter().cloned());
        }
        let feature_gates = feature_gates.into_iter().collect::<Vec<_>>().join(", ");
        writeln!(source, "#![feature({feature_gates})]").unwrap();
        source.push_str(
            "#![allow(internal_features, non_snake_case)]\n\
             #![allow(improper_ctypes, improper_ctypes_definitions)]\n\
             #![no_core]\n",
        );
        source.push_str(NO_CORE_PRELUDE);

        if !global_asm.is_empty() {
            // Escape { and }
            let global_asm = global_asm.replace('{', "{{").replace('}', "}}");

            source.push_str(
                r####"
                #[rustc_builtin_macro]
                #[rustc_macro_transparency = "semiopaque"]
                macro global_asm() { /* compiler built-in */ }
                global_asm!(r###"
                "####,
            );
            source.push_str(&global_asm);
            source.push_str(
                r####"
                "###);
                "####,
            );
        }

        for wrapper in llvm_wrappers.values() {
            source.push_str(&wrapper.source);
        }

        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            // Avoid a warning about the jobserver fd not being passed
            .env_remove("CARGO_MAKEFLAGS")
            .arg("--target")
            .arg(&config.target)
            .arg("--crate-type")
            .arg("staticlib")
            .arg("--emit")
            .arg("obj")
            .arg("-o")
            .arg(&global_asm_object_file)
            .arg("-")
            .arg("-Abad_asm_style")
            .arg("-Zcodegen-backend=llvm")
            // JSON targets currently require `-Zunstable-options`
            // Tracking issue: https://github.com/rust-lang/rust/issues/151528
            .arg("-Zunstable-options");
        if !llvm_wrappers.is_empty() {
            command.arg("-Copt-level=1");
            // The wrappers enable the target features of the functions calling them on top of
            // those of the session.
            if let Some(target_cpu) = &config.target_cpu {
                command.arg(format!("-Ctarget-cpu={target_cpu}"));
            }
            if !config.target_feature.is_empty() {
                command.arg(format!("-Ctarget-feature={}", config.target_feature));
            }
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to spawn `as`.");
        child.stdin.take().unwrap().write_all(source.as_bytes()).unwrap();
        let output = child.wait_with_output().expect("Failed to wait for `as`.");
        if !output.status.success() {
            return Err(format!(
                "Failed to assemble `{}`\n{}",
                source,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    Ok(())
}
