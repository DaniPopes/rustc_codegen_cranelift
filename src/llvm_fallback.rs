//! Fallback for `llvm.*` intrinsics that have no native lowering.
//!
//! For every call to such an intrinsic, a small `#![no_core]` Rust wrapper that calls the
//! intrinsic is generated. The wrappers of a codegen unit are compiled by the LLVM backend in the
//! same subprocess that assembles the global asm of the codegen unit (see
//! [`crate::global_asm::compile_global_asm`]). The Cranelift function then calls the wrapper like
//! it calls inline asm wrappers, but with the C ABI: scalars and 128-bit vectors are passed and
//! returned by value, everything else lives in stack slots whose addresses are passed to the
//! wrapper. Literal integers are baked into the wrapper as LLVM requires some arguments
//! (`immarg`) to be constants.

use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hash, Hasher};

use rustc_data_structures::fx::FxIndexMap;
use rustc_middle::ty::TypingEnv;
use rustc_span::Spanned;
use rustc_target::spec::Arch;

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
                let repr = adt.repr();
                if repr.packed() || repr.align.is_some() {
                    return None;
                }
                let name = format!("T{}", self.structs.len());
                // Reserve the name before printing the fields, which may be structs too.
                self.structs.insert(ty, name.clone());
                let mut decl = String::new();
                if repr.simd() {
                    let (lane_count, lane_ty) = ty.simd_size_and_type(self.tcx);
                    if !matches!(lane_ty.kind(), ty::Int(_) | ty::Uint(_) | ty::Float(_)) {
                        return None;
                    }
                    let lane_ty = self.print(lane_ty)?;
                    writeln!(decl, "#[repr(simd)] pub struct {name}([{lane_ty}; {lane_count}]);")
                        .unwrap();
                } else {
                    if !adt.is_struct() || repr.transparent() || repr.scalable.is_some() {
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
    // These depend on the frame or the state of the function that calls them.
    const FRAME_DEPENDENT: &[&str] = &[
        "llvm.eh.",
        "llvm.frameaddress",
        "llvm.read_register",
        "llvm.returnaddress",
        "llvm.sponentry",
        "llvm.stackrestore",
        "llvm.stacksave",
        "llvm.va_",
        "llvm.write_register",
        "llvm.x86.xbegin",
    ];
    if !fx.llvm_fallback || FRAME_DEPENDENT.iter().any(|prefix| intrinsic.starts_with(prefix)) {
        return false;
    }

    let mut printer = TypePrinter::new(fx.tcx);
    let mut params = String::new();
    let mut wrapper_params = String::new();
    let mut call_args = String::new();
    // The arguments passed to the wrapper, and whether they are passed in registers.
    let mut wrapper_arg_kinds = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        let arg_ty = fx.monomorphize(arg.node.ty(fx.mir, fx.tcx));
        let Some(arg_ty_str) = printer.print(arg_ty) else { return false };
        write!(params, "a{i}: {arg_ty_str}, ").unwrap();

        let const_val = crate::constant::mir_operand_get_const_val(fx, &arg.node);
        let literal = match (arg_ty.kind(), const_val) {
            // Negation isn't available without `core`, so cast the bit pattern instead.
            (ty::Int(int_ty), Some(val)) => Some(format!(
                "{}_{} as {arg_ty_str}",
                val.to_uint(val.size()),
                int_ty.to_unsigned().name_str()
            )),
            (ty::Uint(_), Some(val)) => Some(format!("{}_{arg_ty_str}", val.to_uint(val.size()))),
            (ty::Bool, Some(val)) => Some(val.try_to_bool().unwrap().to_string()),
            _ => None,
        };
        if let Some(literal) = literal {
            write!(call_args, "{literal}, ").unwrap();
        } else if passed_by_value(fx, fx.layout_of(arg_ty)) {
            write!(wrapper_params, "a{i}: {arg_ty_str}, ").unwrap();
            write!(call_args, "a{i}, ").unwrap();
            wrapper_arg_kinds.push((i, true));
        } else {
            write!(wrapper_params, "p{i}: *const {arg_ty_str}, ").unwrap();
            write!(call_args, "*p{i}, ").unwrap();
            wrapper_arg_kinds.push((i, false));
        }
    }
    let Some(ret_ty_str) = printer.print(ret.layout().ty) else { return false };
    let ret_is_unit = ret.layout().ty.is_unit();
    let ret_by_value = ret_is_unit || passed_by_value(fx, ret.layout());

    let (target_features, feature_gates) = enclosing_target_features(fx);

    let mut header = String::from("use super::Copy;\n");
    header.push_str(&printer.decls);
    writeln!(
        header,
        "unsafe extern \"llvm-intrinsic\" {{\n    \
             #[link_name = \"{intrinsic}\"]\n    \
             fn f({params}) -> {ret_ty_str};\n}}"
    )
    .unwrap();
    if !target_features.is_empty() {
        writeln!(header, "#[target_feature(enable = \"{target_features}\")]").unwrap();
    }
    let wrapper_fn = if ret_is_unit {
        format!("({wrapper_params}) {{ f({call_args}) }}")
    } else if ret_by_value {
        format!("({wrapper_params}) -> {ret_ty_str} {{ f({call_args}) }}")
    } else {
        format!("({wrapper_params}ret: *mut {ret_ty_str}) {{ *ret = f({call_args}); }}")
    };

    // The name only depends on the codegen unit and the wrapper itself, which deduplicates
    // wrappers within the codegen unit.
    let mut hasher = DefaultHasher::new();
    (fx.cgu_name.as_str(), &header, &wrapper_fn).hash(&mut hasher);
    let wrapper_name = format!(
        "__llvm_intrinsic_{}_{}_{:016x}",
        fx.cgu_name.as_str().replace('.', "__").replace('-', "_"),
        intrinsic.replace('.', "_"),
        hasher.finish(),
    );
    if !fx.llvm_wrappers.contains_key(&wrapper_name) {
        let source = format!(
            "mod {wrapper_name}_mod {{\n{header}\
             #[unsafe(no_mangle)]\n\
             pub unsafe extern \"C\" fn {wrapper_name}{wrapper_fn}\n}}\n"
        );
        fx.llvm_wrappers.insert(wrapper_name.clone(), LlvmWrapper { source, feature_gates });
    }

    // Pass the arguments in registers or through stack slots, as the wrapper expects.
    let ptr_layout = fx.layout_of(Ty::new_mut_ptr(fx.tcx, fx.tcx.types.u8));
    let mut wrapper_args = Vec::new();
    for (i, by_value) in wrapper_arg_kinds {
        let arg = codegen_operand(fx, &args[i].node);
        if by_value {
            wrapper_args.push(arg);
        } else {
            let slot = CPlace::new_stack_slot(fx, arg.layout());
            slot.write_cvalue(fx, arg);
            wrapper_args.push(CValue::by_val(slot.to_ptr().get_addr(fx), ptr_layout));
        }
    }
    if ret_by_value {
        crate::abi::codegen_extern_c_call(fx, &wrapper_name, &wrapper_args, ret);
    } else {
        let ret_slot = CPlace::new_stack_slot(fx, ret.layout());
        wrapper_args.push(CValue::by_val(ret_slot.to_ptr().get_addr(fx), ptr_layout));
        let unit_place = CPlace::new_stack_slot(fx, fx.layout_of(fx.tcx.types.unit));
        crate::abi::codegen_extern_c_call(fx, &wrapper_name, &wrapper_args, unit_place);

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
        if features.split(',').any(|feature| feature == name) {
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

/// Whether values of the type are passed to and returned from wrappers in registers. Everything
/// else, in particular vectors wider than 128 bits whose ABI depends on the target features, is
/// passed by pointer.
fn passed_by_value<'tcx>(fx: &FunctionCx<'_, '_, 'tcx>, layout: TyAndLayout<'tcx>) -> bool {
    match *layout.ty.kind() {
        ty::Bool | ty::RawPtr(..) | ty::Float(FloatTy::F32 | FloatTy::F64) => true,
        ty::Int(IntTy::I128) | ty::Uint(UintTy::U128) => false,
        ty::Int(_) | ty::Uint(_) => true,
        // Single lane vectors are passed as scalars by Cranelift but as vectors by LLVM. Only
        // x86_64 and AArch64 are known to agree on vectors, and only AArch64 has 64-bit ones.
        ty::Adt(adt, _) if adt.repr().simd() && layout.ty.simd_size_and_type(fx.tcx).0 > 1 => {
            match fx.tcx.sess.target.arch {
                Arch::X86_64 => layout.size.bytes() == 16,
                Arch::AArch64 => matches!(layout.size.bytes(), 8 | 16),
                _ => false,
            }
        }
        _ => false,
    }
}
