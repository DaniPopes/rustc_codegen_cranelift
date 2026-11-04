//! Fallback for `llvm.*` intrinsics that have no native lowering.
//!
//! For every call to such an intrinsic, a small `#![no_core]` Rust wrapper that calls the
//! intrinsic is generated. The wrappers of a codegen unit are compiled by the LLVM backend in the
//! same subprocess that assembles the global asm of the codegen unit (see
//! [`crate::global_asm::compile_global_asm`]). The Cranelift function then calls the wrapper like
//! it calls inline asm wrappers: each argument that isn't a literal integer and the return value
//! live in stack slots whose addresses are passed to the wrapper. Literal integers are baked into
//! the wrapper as LLVM requires some arguments (`immarg`) to be constants.

use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};

use rustc_data_structures::fx::FxIndexMap;
use rustc_middle::ty::TypingEnv;
use rustc_span::Spanned;

use crate::prelude::*;

/// Whether `llvm.*` intrinsics can be compiled by a LLVM subprocess. This is the case when the
/// global asm is compiled by `rustc -Zcodegen-backend=llvm` and not by the GNU assembler.
pub(crate) const SUBPROCESS_AVAILABLE: bool = option_env!("CG_CLIF_FORCE_GNU_AS").is_none();

/// A generated `#![no_core]` module calling a single `llvm.*` intrinsic.
pub(crate) struct LlvmWrapper {
    /// The full source of the module.
    pub(crate) source: String,
    /// The `#![feature(..)]` gates needed by the module.
    pub(crate) feature_gates: Vec<String>,
}

/// The wrappers of a codegen unit, keyed by their unique symbol name.
pub(crate) type LlvmWrappers = FxIndexMap<String, LlvmWrapper>;

/// Prints the types of LLVM intrinsic arguments and return values as `#![no_core]` Rust.
struct TypePrinter<'tcx> {
    tcx: TyCtxt<'tcx>,
    /// Definitions of the structs created for SIMD vectors and ADTs.
    decls: String,
    /// Names of the structs defined in `decls`.
    structs: FxIndexMap<Ty<'tcx>, String>,
}

impl<'tcx> TypePrinter<'tcx> {
    fn new(tcx: TyCtxt<'tcx>) -> Self {
        TypePrinter { tcx, decls: String::new(), structs: FxIndexMap::default() }
    }

    /// Prints `ty` such that it has the same layout as `ty` and the same LLVM representation after
    /// the autocasting done for LLVM intrinsics. Returns `None` for unsupported types.
    fn print(&mut self, ty: Ty<'tcx>) -> Option<String> {
        Some(match *ty.kind() {
            ty::Bool => "bool".to_owned(),
            ty::Int(int_ty) => int_ty.name_str().to_owned(),
            ty::Uint(uint_ty) => uint_ty.name_str().to_owned(),
            ty::Float(float_ty) => float_ty.name_str().to_owned(),
            ty::RawPtr(pointee, mutbl)
                if pointee.is_sized(self.tcx, TypingEnv::fully_monomorphized()) =>
            {
                format!("*{} u8", mutbl.ptr_str())
            }
            ty::Tuple(fields) => {
                let mut res = "(".to_owned();
                for field in fields {
                    write!(res, "{}, ", self.print(field)?).unwrap();
                }
                res.push(')');
                res
            }
            ty::Adt(adt, args) => {
                if let Some(name) = self.structs.get(&ty) {
                    return Some(name.clone());
                }
                let name = format!("T{}", self.structs.len());
                let mut decl = String::new();
                if adt.repr().simd() {
                    let (lane_count, lane_ty) = ty.simd_size_and_type(self.tcx);
                    if !matches!(lane_ty.kind(), ty::Int(_) | ty::Uint(_) | ty::Float(_)) {
                        return None;
                    }
                    let lane_ty = self.print(lane_ty)?;
                    writeln!(decl, "#[repr(simd)] pub struct {name}([{lane_ty}; {lane_count}]);")
                        .unwrap();
                } else {
                    let repr = adt.repr();
                    if !adt.is_struct()
                        || repr.packed()
                        || repr.transparent()
                        || repr.align.is_some()
                        || repr.scalable.is_some()
                    {
                        return None;
                    }
                    if repr.c() {
                        decl.push_str("#[repr(C)] ");
                    }
                    write!(decl, "pub struct {name}(").unwrap();
                    for field in adt.non_enum_variant().fields.iter() {
                        let field_ty = self.tcx.normalize_erasing_regions(
                            TypingEnv::fully_monomorphized(),
                            field.ty(self.tcx, args),
                        );
                        write!(decl, "{}, ", self.print(field_ty)?).unwrap();
                    }
                    decl.push_str(");\n");
                }
                writeln!(decl, "impl Copy for {name} {{}}").unwrap();
                self.decls.push_str(&decl);
                self.structs.insert(ty, name.clone());
                name
            }
            _ => return None,
        })
    }
}

/// Calls `intrinsic` through a wrapper compiled by the LLVM backend. Returns `false` without
/// emitting any code if this isn't possible. The caller then has to handle the intrinsic call
/// otherwise.
pub(crate) fn codegen_llvm_fallback_call<'tcx>(
    fx: &mut FunctionCx<'_, '_, 'tcx>,
    intrinsic: &str,
    args: &[Spanned<mir::Operand<'tcx>>],
    ret: CPlace<'tcx>,
) -> bool {
    if !fx.llvm_fallback {
        return false;
    }

    let mut printer = TypePrinter::new(fx.tcx);
    let mut params = String::new();
    let mut wrapper_params = String::new();
    let mut call_args = String::new();
    let mut by_ptr_args = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let arg_ty = fx.monomorphize(arg.node.ty(fx.mir, fx.tcx));
        let Some(arg_ty_str) = printer.print(arg_ty) else { return false };
        write!(params, "a{i}: {arg_ty_str}, ").unwrap();

        let const_val = match arg_ty.kind() {
            ty::Int(_) | ty::Uint(_) => crate::constant::mir_operand_get_const_val(fx, &arg.node),
            _ => None,
        };
        if let Some(const_val) = const_val {
            let size = const_val.size();
            match arg_ty.kind() {
                ty::Int(_) => write!(call_args, "{}_{arg_ty_str}, ", const_val.to_int(size)),
                _ => write!(call_args, "{}_{arg_ty_str}, ", const_val.to_uint(size)),
            }
            .unwrap();
        } else {
            write!(wrapper_params, "p{i}: *const {arg_ty_str}, ").unwrap();
            write!(call_args, "*p{i}, ").unwrap();
            by_ptr_args.push(i);
        }
    }

    let ret_ty_str = if ret.layout().ty.is_unit() {
        None
    } else {
        let Some(ret_ty_str) = printer.print(ret.layout().ty) else { return false };
        Some(ret_ty_str)
    };

    let (target_features, feature_gates) = enclosing_target_features(fx);

    let mut body = String::new();
    writeln!(body, "use super::Copy;").unwrap();
    body.push_str(&printer.decls);
    writeln!(body, "unsafe extern \"llvm-intrinsic\" {{").unwrap();
    write!(body, "    #[link_name = \"{intrinsic}\"]\n    fn f({params})").unwrap();
    if let Some(ret_ty_str) = &ret_ty_str {
        write!(body, " -> {ret_ty_str}").unwrap();
    }
    writeln!(body, ";\n}}").unwrap();
    writeln!(body, "#[unsafe(no_mangle)]").unwrap();
    if !target_features.is_empty() {
        writeln!(body, "#[target_feature(enable = \"{target_features}\")]").unwrap();
    }
    if let Some(ret_ty_str) = &ret_ty_str {
        write!(wrapper_params, "ret: *mut {ret_ty_str}").unwrap();
        writeln!(
            body,
            "pub unsafe extern \"C\" fn WRAPPER({wrapper_params}) {{ *ret = f({call_args}); }}"
        )
        .unwrap();
    } else {
        writeln!(
            body,
            "pub unsafe extern \"C\" fn WRAPPER({wrapper_params}) {{ f({call_args}); }}"
        )
        .unwrap();
    }

    // The name only depends on the codegen unit and the wrapper itself, which deduplicates
    // wrappers within the codegen unit.
    let mut hasher = DefaultHasher::new();
    body.hash(&mut hasher);
    let wrapper_name = format!(
        "__llvm_intrinsic_{}_{}_{:016x}",
        fx.cgu_name.as_str().replace('.', "__").replace('-', "_"),
        intrinsic.replace('.', "_"),
        hasher.finish(),
    );
    if !fx.llvm_wrappers.contains_key(&wrapper_name) {
        let body = body.replace("WRAPPER", &wrapper_name);
        let source = format!("mod {wrapper_name}_mod {{\n{body}}}\n");
        fx.llvm_wrappers.insert(wrapper_name.clone(), LlvmWrapper { source, feature_gates });
    }

    // Pass the arguments and the return value through stack slots.
    let mut wrapper_args = Vec::new();
    for i in by_ptr_args {
        let arg = codegen_operand(fx, &args[i].node);
        let slot = CPlace::new_stack_slot(fx, arg.layout());
        slot.write_cvalue(fx, arg);
        wrapper_args.push(slot.to_ptr().get_addr(fx));
    }
    let ret_slot = ret_ty_str.is_some().then(|| CPlace::new_stack_slot(fx, ret.layout()));
    if let Some(ret_slot) = ret_slot {
        wrapper_args.push(ret_slot.to_ptr().get_addr(fx));
    }

    let sig = Signature {
        call_conv: fx.target_config.default_call_conv,
        params: vec![AbiParam::new(fx.pointer_type); wrapper_args.len()],
        returns: vec![],
    };
    let func_id = fx.module.declare_function(&wrapper_name, Linkage::Import, &sig).unwrap();
    let func_ref = fx.module.declare_func_in_func(func_id, fx.bcx.func);
    if fx.clif_comments.enabled() {
        fx.add_comment(func_ref, intrinsic);
    }
    fx.bcx.ins().call(func_ref, &wrapper_args);

    if let Some(ret_slot) = ret_slot {
        let value = ret_slot.to_cvalue(fx);
        ret.write_cvalue(fx, value);
    }

    true
}

/// The target features enabled for the function being compiled, together with the feature gates
/// required to enable them.
fn enclosing_target_features(fx: &FunctionCx<'_, '_, '_>) -> (String, Vec<String>) {
    let rust_target_features = fx.tcx.sess.target.rust_target_features();
    let mut features = String::new();
    let mut gates = Vec::new();
    for feature in &fx.tcx.codegen_instance_attrs(fx.instance.def).target_features {
        let name = feature.name.as_str();
        let Some((_, stability, _)) =
            rust_target_features.iter().find(|(feature_name, _, _)| *feature_name == name)
        else {
            continue;
        };
        if stability.toggle_allowed().is_err() {
            continue;
        }
        if !features.is_empty() {
            features.push(',');
        }
        features.push_str(name);
        if let Some(gate) = stability.requires_nightly(false) {
            gates.push(gate.as_str().to_owned());
        }
    }
    (features, gates)
}
