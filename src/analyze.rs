//! SSA analysis

use rustc_abi::{Endian, Float, Primitive};
use rustc_index::IndexVec;
use rustc_middle::mir::StatementKind::*;
use rustc_target::spec::Arch;

use crate::prelude::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SsaKind {
    NotSsa,
    MaybeSsa,
}

impl SsaKind {
    pub(crate) fn is_ssa<'tcx>(self, fx: &FunctionCx<'_, '_, 'tcx>, ty: Ty<'tcx>) -> bool {
        self == SsaKind::MaybeSsa
            && (fx.clif_type(ty).is_some()
                || fx.clif_pair_type(ty).is_some()
                || simd_ssa_type(fx, fx.layout_of(ty)).is_some())
    }
}

/// The Cranelift type of the SSA variable holding a value of the given layout.
pub(crate) fn ssa_clif_type<'tcx>(
    fx: &FunctionCx<'_, '_, 'tcx>,
    layout: TyAndLayout<'tcx>,
) -> Option<Type> {
    fx.clif_type(layout.ty).or_else(|| simd_ssa_type(fx, layout))
}

/// The Cranelift vector type used to hold a SIMD value in an SSA variable, if it can be held in
/// one. Wider vectors and vectors Cranelift does not fully support stay in memory.
fn simd_ssa_type<'tcx>(fx: &FunctionCx<'_, '_, 'tcx>, layout: TyAndLayout<'tcx>) -> Option<Type> {
    let tcx = fx.tcx;
    let BackendRepr::SimdVector { element, count } = layout.backend_repr else {
        return None;
    };
    // Only single-field structs: a SIMD type wrapping an array, or a type wrapping such a type.
    // Fields of these are handled by `CPlace::place_field` and `CValue::value_field`.
    let ty::Adt(adt, _) = layout.ty.kind() else {
        return None;
    };
    if !adt.is_struct() || adt.non_enum_variant().fields.len() != 1 {
        return None;
    }
    if !layout.ty.is_simd() && simd_ssa_type(fx, layout.field(fx, 0)).is_none() {
        return None;
    }
    // Cranelift orders the lanes of vectors in memory by the target endianness.
    if tcx.data_layout.endian != Endian::Little || count.as_u32() < 2 {
        return None;
    }
    // Cranelift only supports 64-bit vectors on AArch64.
    if !matches!(
        (&tcx.sess.target.arch, layout.size.bytes()),
        (Arch::X86_64 | Arch::AArch64, 16) | (Arch::AArch64, 8)
    ) {
        return None;
    }
    if !matches!(
        element.primitive(),
        Primitive::Int(int, _) if int.size().bytes() <= 8
    ) && !matches!(
        element.primitive(),
        Primitive::Float(Float::F32 | Float::F64) | Primitive::Pointer(_)
    ) {
        return None;
    }
    Some(crate::intrinsics::clif_vector_type(tcx, layout))
}

pub(crate) fn analyze(fx: &FunctionCx<'_, '_, '_>) -> IndexVec<Local, SsaKind> {
    let mut flag_map =
        fx.mir.local_decls.iter().map(|_| SsaKind::MaybeSsa).collect::<IndexVec<Local, SsaKind>>();

    for bb in fx.mir.basic_blocks.iter() {
        for stmt in bb.statements.iter() {
            if let Assign(place_and_rval) = &stmt.kind
                && let Rvalue::Ref(_, _, place) | Rvalue::RawPtr(_, place) = &place_and_rval.1
            {
                flag_map[place.local] = SsaKind::NotSsa;
            }
        }
        // Dropping a value takes its address.
        if let TerminatorKind::Drop { place, .. } = &bb.terminator().kind {
            flag_map[place.local] = SsaKind::NotSsa;
        }
    }

    flag_map
}
