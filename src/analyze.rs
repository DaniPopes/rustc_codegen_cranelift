//! SSA analysis

use std::borrow::Cow;

use rustc_index::IndexVec;
use rustc_index::bit_set::{BitMatrix, DenseBitSet};
use rustc_middle::mir::StatementKind::*;
use rustc_middle::mir::visit::{PlaceContext, Visitor};
use rustc_mir_dataflow::Analysis;
use rustc_mir_dataflow::impls::{
    DefUse, MaybeLiveLocals, MaybeStorageLive, always_storage_live_locals,
};

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

/// Assigns locals that are never in use at the same time to a shared stack slot.
///
/// Locals whose address is taken keep their slot for as long as their storage is live, so nothing
/// is shared between them when the MIR has no storage markers. Locals whose address is never taken
/// keep it only while their value is live, which lets `_2 = move _1` use the slot of `_1`.
///
/// Returns the slot of every local that shares one. All other locals are left to
/// `make_local_place`.
pub(crate) fn share_stack_slots<'tcx>(
    fx: &mut FunctionCx<'_, '_, 'tcx>,
    ssa_analyzed: &IndexVec<Local, SsaKind>,
) -> IndexVec<Local, Option<Pointer>> {
    let mut shared_slots = IndexVec::from_elem_n(None, fx.mir.local_decls.len());

    let always_live_locals = always_storage_live_locals(fx.mir);

    // The locals that get their own stack slot today.
    let mut candidates = Vec::new();
    for local in fx.mir.vars_and_temps_iter() {
        if ssa_analyzed[local] == SsaKind::NotSsa && always_live_locals.contains(local) {
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

    let mut value_candidate_of_local = IndexVec::from_elem_n(None, fx.mir.local_decls.len());
    let mut borrowed_candidate_of_local = IndexVec::from_elem_n(None, fx.mir.local_decls.len());
    for (candidate, &(local, _, _)) in candidates.iter().enumerate() {
        if ssa_analyzed[local] == SsaKind::NotSsa {
            borrowed_candidate_of_local[local] = Some(candidate);
        } else {
            value_candidate_of_local[local] = Some(candidate);
        }
    }

    let mut conflicts = ConflictCollector::new(
        value_candidate_of_local,
        borrowed_candidate_of_local,
        candidates.len(),
    )
    .collect(fx, &always_live_locals);

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

/// Finds the candidates that must not share a slot. Like rustc's coroutine layout, it keeps one
/// bit matrix row per candidate rather than per local.
///
/// A candidate whose address is never taken is in use while its value is live. Writing it
/// conflicts with every candidate that is live afterwards, and with every candidate the same
/// statement reads, as the write may come before the read. Only a whole local `dest = move src`
/// or `dest = copy src` copies in one go, so `src` and `dest` may share a slot when `src` dies
/// there.
///
/// A candidate whose address is taken is in use while its storage is live. It conflicts with every
/// candidate that is in use at some point where its storage is live.
struct ConflictCollector {
    /// The candidates whose address is never taken.
    value_candidate_of_local: IndexVec<Local, Option<usize>>,
    /// The candidates whose address is taken.
    borrowed_candidate_of_local: IndexVec<Local, Option<usize>>,
    conflicts: BitMatrix<usize, usize>,
    /// The candidates whose address is never taken and that are live at the current location.
    in_use: DenseBitSet<usize>,
}

impl ConflictCollector {
    fn new(
        value_candidate_of_local: IndexVec<Local, Option<usize>>,
        borrowed_candidate_of_local: IndexVec<Local, Option<usize>>,
        count: usize,
    ) -> Self {
        ConflictCollector {
            value_candidate_of_local,
            borrowed_candidate_of_local,
            conflicts: BitMatrix::new(count, count),
            in_use: DenseBitSet::new_empty(count),
        }
    }

    /// Returns a symmetric matrix of the candidates that conflict.
    fn collect(
        mut self,
        fx: &FunctionCx<'_, '_, '_>,
        always_live_locals: &DenseBitSet<Local>,
    ) -> BitMatrix<usize, usize> {
        let mir = fx.mir;
        let count = self.in_use.domain_size();

        let live_results = MaybeLiveLocals.iterate_to_fixpoint(fx.tcx, mir, None);
        let storage_results =
            self.borrowed_candidate_of_local.iter().any(Option::is_some).then(|| {
                MaybeStorageLive::new(Cow::Borrowed(always_live_locals))
                    .iterate_to_fixpoint(fx.tcx, mir, None)
            });

        // The borrowed candidates whose storage is live at the current location, and the
        // `StorageLive` and `StorageDead` statements of the block that changed it.
        let mut storage_live = DenseBitSet::new_empty(count);
        let mut storage_changes = Vec::new();
        let mut def_uses = DefUses::default();

        for &block in mir.basic_blocks.reverse_postorder() {
            let data = &mir.basic_blocks[block];

            // Walk the block forwards to the storage live at its end.
            if let Some(storage_results) = &storage_results {
                storage_live.clear();
                for local in storage_results.entry_states[block].iter() {
                    if let Some(candidate) = self.borrowed_candidate_of_local[local] {
                        storage_live.insert(candidate);
                    }
                }
                for (statement_index, statement) in data.statements.iter().enumerate() {
                    if let StorageLive(local) | StorageDead(local) = statement.kind
                        && let Some(candidate) = self.borrowed_candidate_of_local[local]
                    {
                        storage_changes.push((
                            statement_index,
                            candidate,
                            storage_live.contains(candidate),
                        ));
                        if matches!(statement.kind, StorageLive(_)) {
                            // Two borrowed locals are in use at the same time only if the later
                            // `StorageLive` ran while the other local was live.
                            storage_live.insert(candidate);
                            self.conflicts.union_row_with(&storage_live, candidate);
                        } else {
                            storage_live.remove(candidate);
                        }
                    }
                }
            }

            // Walk it backwards, from the live locals at its end.
            let mut live = live_results.entry_states[block].clone();

            let terminator = data.terminator();
            let location = Location { block, statement_index: data.statements.len() };
            def_uses.clear();
            def_uses.visit_terminator(terminator, location);
            self.add_conflicts(&live, &def_uses, &storage_live);
            live_results.analysis.apply_primary_terminator_effect(&mut live, terminator, location);

            for (statement_index, statement) in data.statements.iter().enumerate().rev() {
                let location = Location { block, statement_index };
                def_uses.clear();
                def_uses.visit_statement(statement, location);
                if let Assign(assign) = &statement.kind
                    && let (dest, Rvalue::Use(Operand::Move(src) | Operand::Copy(src), _)) =
                        &**assign
                    && dest.as_local().is_some()
                    && let Some(src) = src.as_local()
                {
                    def_uses.uses.retain(|&local| local != src);
                }
                self.add_conflicts(&live, &def_uses, &storage_live);
                live_results
                    .analysis
                    .apply_primary_statement_effect(&mut live, statement, location);

                if let Some(&(index, candidate, was_live)) = storage_changes.last()
                    && index == statement_index
                {
                    storage_changes.pop();
                    if was_live {
                        storage_live.insert(candidate);
                    } else {
                        storage_live.remove(candidate);
                    }
                }
            }

            // The locals that are live on entry are in use at the same time.
            if block == START_BLOCK {
                self.fill_in_use(&live);
                for candidate in self.in_use.iter() {
                    self.conflicts.union_row_with(&self.in_use, candidate);
                }
            }
        }

        // `union_row_with` only fills the row of the candidate that is in use.
        let mut conflicts = self.conflicts.clone();
        for candidate in 0..count {
            for other in self.conflicts.iter(candidate) {
                conflicts.insert(other, candidate);
            }
        }
        conflicts
    }

    /// Sets `in_use` to the candidates whose address is never taken and whose value is live.
    fn fill_in_use(&mut self, live: &DenseBitSet<Local>) {
        self.in_use.clear();
        for local in live.iter() {
            if let Some(candidate) = self.value_candidate_of_local[local] {
                self.in_use.insert(candidate);
            }
        }
    }

    /// Adds the conflicts of a location with the given locals live after it, written and read by
    /// it, and the borrowed candidates whose storage is live there.
    fn add_conflicts(
        &mut self,
        live: &DenseBitSet<Local>,
        def_uses: &DefUses,
        storage_live: &DenseBitSet<usize>,
    ) {
        let writes_value =
            def_uses.defs.iter().any(|&local| self.value_candidate_of_local[local].is_some());
        if !writes_value && storage_live.is_empty() {
            return;
        }
        self.fill_in_use(live);
        for &local in &def_uses.defs {
            if let Some(def) = self.value_candidate_of_local[local] {
                self.conflicts.union_row_with(&self.in_use, def);
                for &local in &def_uses.uses {
                    if let Some(used) = self.value_candidate_of_local[local] {
                        self.conflicts.insert(def, used);
                    }
                }
                self.in_use.insert(def);
            }
        }
        if !storage_live.is_empty() {
            for candidate in self.in_use.iter() {
                self.conflicts.union_row_with(storage_live, candidate);
            }
        }
    }
}

/// The locals a statement or terminator writes, partly or as a whole, and the locals it reads.
#[derive(Default)]
struct DefUses {
    defs: Vec<Local>,
    uses: Vec<Local>,
}

impl DefUses {
    fn clear(&mut self) {
        self.defs.clear();
        self.uses.clear();
    }

    fn record(&mut self, place: Place<'_>, context: PlaceContext) {
        match DefUse::for_place(place, context) {
            DefUse::Def | DefUse::PartialWrite => self.defs.push(place.local),
            DefUse::Use => self.uses.push(place.local),
            DefUse::NonUse => {}
        }
    }
}

impl<'tcx> Visitor<'tcx> for DefUses {
    fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, location: Location) {
        self.record(*place, context);
        self.visit_projection(place.as_ref(), context, location);
    }

    fn visit_local(&mut self, local: Local, context: PlaceContext, _location: Location) {
        self.record(local.into(), context);
    }
}
