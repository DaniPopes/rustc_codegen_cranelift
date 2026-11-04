//! SSA analysis

use std::borrow::Cow;

use rustc_index::IndexVec;
use rustc_index::bit_set::{BitMatrix, DenseBitSet};
use rustc_middle::mir::StatementKind::*;
use rustc_mir_dataflow::impls::{MaybeStorageLive, always_storage_live_locals};
use rustc_mir_dataflow::{Analysis, ResultsVisitor, visit_results};

use crate::prelude::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SsaKind {
    NotSsa,
    MaybeSsa,
}

impl SsaKind {
    pub(crate) fn is_ssa<'tcx>(self, fx: &FunctionCx<'_, '_, 'tcx>, ty: Ty<'tcx>) -> bool {
        self == SsaKind::MaybeSsa && (fx.clif_type(ty).is_some() || fx.clif_pair_type(ty).is_some())
    }
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
    }

    flag_map
}

/// Assigns locals whose storage is never live at the same time to a shared stack slot.
///
/// Returns the slot of every local that shares one. All other locals are left to
/// `make_local_place`. Only locals that are not always storage live take part, so
/// nothing is shared when the MIR has no storage markers.
pub(crate) fn share_stack_slots<'tcx>(
    fx: &mut FunctionCx<'_, '_, 'tcx>,
    ssa_analyzed: &IndexVec<Local, SsaKind>,
) -> IndexVec<Local, Option<Pointer>> {
    let mut shared_slots = IndexVec::from_elem_n(None, fx.mir.local_decls.len());

    let always_live_locals = always_storage_live_locals(fx.mir);

    // The locals that get their own stack slot today.
    let mut candidates = Vec::new();
    for local in fx.mir.vars_and_temps_iter() {
        if always_live_locals.contains(local) {
            continue;
        }
        let ty = fx.monomorphize(fx.mir.local_decls[local].ty);
        if ssa_analyzed[local].is_ssa(fx, ty) {
            continue;
        }
        let layout = fx.layout_of(ty);
        // Zero sized and huge locals are left to `CPlace::new_stack_slot`.
        if layout.is_sized() && layout.size.bytes() != 0 && layout.size.bytes() < MAX_SHARED_SIZE {
            candidates.push((local, layout.size.bytes(), layout.align.abi.bytes()));
        }
    }
    if candidates.len() < 2 {
        return shared_slots;
    }
    // The first member of a slot is then its largest.
    candidates.sort_by_key(|&(_, size, _)| std::cmp::Reverse(size));

    let mut candidate_of_local = IndexVec::from_elem_n(None, fx.mir.local_decls.len());
    for (candidate, &(local, _, _)) in candidates.iter().enumerate() {
        candidate_of_local[local] = Some(candidate);
    }

    let results = MaybeStorageLive::new(Cow::Borrowed(&always_live_locals))
        .iterate_to_fixpoint(fx.tcx, fx.mir, None);
    let mut visitor = StorageConflictVisitor {
        candidate_of_local,
        conflicts: BitMatrix::new(candidates.len(), candidates.len()),
    };
    visit_results(
        fx.mir,
        fx.mir.basic_blocks.reverse_postorder().iter().copied(),
        &results,
        &mut visitor,
    );
    let mut conflicts = visitor.conflicts;

    // Greedily put every candidate into the first slot without a conflicting member. The row of
    // a slot's first member holds the union of the conflicts of all its members.
    let mut slots = Vec::<Vec<usize>>::new();
    for candidate in 0..candidates.len() {
        match slots.iter_mut().find(|members| !conflicts.contains(members[0], candidate)) {
            Some(members) => {
                conflicts.union_rows(candidate, members[0]);
                members.push(candidate);
            }
            None => slots.push(vec![candidate]),
        }
    }

    for members in slots.iter().filter(|members| members.len() > 1) {
        let align = members.iter().map(|&member| candidates[member].2).max().unwrap();
        let size = candidates[members[0]].1.next_multiple_of(align);
        let ptr = fx.create_stack_slot(u32::try_from(size).unwrap(), u32::try_from(align).unwrap());
        for &member in members {
            shared_slots[candidates[member].0] = Some(ptr);
        }
    }

    shared_slots
}

/// Stack slots are created for sizes below this, which also keeps rounding up to the alignment
/// within `u32`.
const MAX_SHARED_SIZE: u64 = 1 << 31;

/// Records which candidates have their storage live at the same time. Like rustc's coroutine
/// layout, it keeps one bit matrix row per candidate rather than per local.
struct StorageConflictVisitor {
    candidate_of_local: IndexVec<Local, Option<usize>>,
    conflicts: BitMatrix<usize, usize>,
}

impl<'a, 'tcx> ResultsVisitor<'tcx, MaybeStorageLive<'a>> for StorageConflictVisitor {
    fn visit_after_primary_statement_effect(
        &mut self,
        state: &DenseBitSet<Local>,
        statement: &Statement<'tcx>,
        _location: Location,
    ) {
        // Two locals are live at the same time only if the later `StorageLive` ran while the
        // other local was live.
        if let StorageLive(local) = statement.kind
            && let Some(candidate) = self.candidate_of_local[local]
        {
            for live_local in state.iter() {
                if let Some(live_candidate) = self.candidate_of_local[live_local] {
                    self.conflicts.insert(candidate, live_candidate);
                    self.conflicts.insert(live_candidate, candidate);
                }
            }
        }
    }
}
