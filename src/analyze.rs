//! SSA analysis

use std::borrow::Cow;

use rustc_abi::{FieldsShape, TagEncoding, Variants};
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
                // The borrowed memory is not that of the local if it is behind a pointer.
                && !place.is_indirect()
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
/// keep it only while their value is live, which lets `_2 = move _1` use the slot of `_1`. Such a
/// local that is copied into or out of a field of another one, as in `_2 = Ok(move _1)` and
/// `_1 = move (_2 as Ok).0`, may also be placed at that field.
///
/// Returns the place of every local that shares a slot. All other locals are left to
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
        let borrowed = ssa_analyzed[local] == SsaKind::NotSsa;
        if borrowed && always_live_locals.contains(local) {
            continue;
        }
        let ty = fx.monomorphize(fx.mir.local_decls[local].ty);
        if ssa_analyzed[local].is_ssa(fx, ty) {
            continue;
        }
        let layout = fx.layout_of(ty);
        // Zero sized and huge locals are left to `CPlace::new_stack_slot`.
        if layout.is_sized() && layout.size.bytes() != 0 && layout.size.bytes() < MAX_SHARED_SIZE {
            candidates.push(Candidate {
                local,
                size: layout.size.bytes(),
                align: layout.align.abi.bytes(),
                borrowed,
            });
        }
    }
    if candidates.len() < 2 {
        return shared_slots;
    }

    let Conflicts { conflicts, moves } =
        ConflictCollector::new(fx.mir.local_decls.len(), &candidates)
            .collect(fx, &always_live_locals);
    let placer = Placer::new(&candidates, &conflicts, &moves);

    let slots = placer.assign_slots(placer.merge_moved_candidates());
    for slot in slots.iter().filter(|slot| slot.len() > 1) {
        let align = slot.iter().map(|&(candidate, _)| candidates[candidate].align).max().unwrap();
        let size = placer.extent(slot).next_multiple_of(align);
        let ptr = fx.create_stack_slot(u32::try_from(size).unwrap(), u32::try_from(align).unwrap());
        for &(candidate, offset) in slot {
            shared_slots[candidates[candidate].local] =
                Some(ptr.offset_i64(fx, i64::try_from(offset).unwrap()));
        }
    }

    shared_slots
}

/// Stack slots are created for sizes below this, which also keeps rounding up to the alignment
/// within `u32`.
const MAX_SHARED_SIZE: u64 = 1 << 31;

struct Candidate {
    local: Local,
    size: u64,
    align: u64,
    /// Whether the address of the local is taken.
    borrowed: bool,
}

/// A candidate that is copied from another one, to another one, or into or out of a field of
/// another one, where `inner` is `offset` bytes into `outer`. The copy is skipped if they are
/// placed so.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Move {
    inner: usize,
    outer: usize,
    offset: u64,
}

/// Puts candidates that do not conflict into slots. The members of a group or a slot come with
/// their offsets.
struct Placer<'a> {
    candidates: &'a [Candidate],
    conflicts: &'a BitMatrix<usize, usize>,
    moves: &'a [Move],
    /// For every candidate, the candidates it is copied from or to, and where it starts relative
    /// to each. They must start there if they overlap, as the copy is skipped then.
    starts: Vec<Vec<(usize, i64)>>,
}

impl<'a> Placer<'a> {
    fn new(
        candidates: &'a [Candidate],
        conflicts: &'a BitMatrix<usize, usize>,
        moves: &'a [Move],
    ) -> Self {
        let mut starts = vec![Vec::new(); candidates.len()];
        for &Move { inner, outer, offset } in moves {
            starts[inner].push((outer, offset.cast_signed()));
            starts[outer].push((inner, -offset.cast_signed()));
        }
        Placer { candidates, conflicts, moves, starts }
    }

    /// Whether `a` at `a_start` and `b` at `b_start` may overlap: there is no conflict between
    /// them, unless `ignore_conflict`, and a copy between them is skipped.
    fn may_overlap(
        &self,
        (a, a_start): (usize, u64),
        (b, b_start): (usize, u64),
        ignore_conflict: bool,
    ) -> bool {
        let a_end = a_start + self.candidates[a].size;
        let b_end = b_start + self.candidates[b].size;
        if a_end <= b_start || b_end <= a_start {
            return true;
        }
        (ignore_conflict || !self.conflicts.contains(a, b))
            && self.starts[a].iter().all(|&(other, delta)| {
                other != b || a_start.cast_signed() == b_start.cast_signed() + delta
            })
    }

    /// The size of the slot or group with the given members.
    fn extent(&self, members: &[(usize, u64)]) -> u64 {
        members
            .iter()
            .map(|&(candidate, offset)| offset + self.candidates[candidate].size)
            .max()
            .unwrap()
    }

    /// Starts with every candidate in a group of its own and merges the groups of candidates
    /// that are copied into or out of a field of another one, or to each other, so that the
    /// copy is skipped. The members of a merged group keep their offsets from each other.
    fn merge_moved_candidates(&self) -> Vec<Vec<(usize, u64)>> {
        let mut groups = (0..self.candidates.len()).map(|c| vec![(c, 0)]).collect::<Vec<_>>();
        let mut group_of_candidate = (0..self.candidates.len()).collect::<Vec<_>>();

        for &Move { inner, outer, offset } in self.moves {
            let (inner_group, outer_group) = (group_of_candidate[inner], group_of_candidate[outer]);
            if inner_group == outer_group {
                continue;
            }
            // Place the inner group such that `inner` is at `offset` in `outer`. If that moves
            // the start of the inner group before the start of the outer group, move the outer
            // group instead.
            let offset_of = |group: &[(usize, u64)], candidate| {
                group.iter().find(|&&(member, _)| member == candidate).unwrap().1.cast_signed()
            };
            let shift = offset_of(&groups[outer_group], outer) + offset.cast_signed()
                - offset_of(&groups[inner_group], inner);
            let start = groups[inner_group].iter().map(|&(_, o)| o.cast_signed() + shift).min();
            let base = start.unwrap().min(0);
            let inner_members = groups[inner_group]
                .iter()
                .map(|&(member, o)| (member, (o.cast_signed() + shift - base).cast_unsigned()))
                .collect::<Vec<_>>();
            let outer_members = groups[outer_group]
                .iter()
                .map(|&(member, o)| (member, (o.cast_signed() - base).cast_unsigned()))
                .collect::<Vec<_>>();

            let merged = [inner_members.as_slice(), outer_members.as_slice()].concat();
            let is_aligned =
                merged.iter().all(|&(member, o)| o % self.candidates[member].align == 0);
            let is_sound = inner_members.iter().all(|&inner_member| {
                outer_members
                    .iter()
                    .all(|&outer_member| self.may_overlap(inner_member, outer_member, false))
            });
            if !is_aligned || self.extent(&merged) >= MAX_SHARED_SIZE || !is_sound {
                continue;
            }

            groups[inner_group].clear();
            for &(member, _) in &inner_members {
                group_of_candidate[member] = outer_group;
            }
            groups[outer_group] = merged;
        }

        groups.retain(|group| !group.is_empty());
        groups
    }

    /// Greedily puts every group, the largest first, into the first slot without a conflicting
    /// member.
    fn assign_slots(&self, mut groups: Vec<Vec<(usize, u64)>>) -> Vec<Vec<(usize, u64)>> {
        groups.sort_by_cached_key(|group| std::cmp::Reverse(self.extent(group)));
        let mut offset_of_candidate = vec![0; self.candidates.len()];
        for &(member, offset) in groups.iter().flatten() {
            offset_of_candidate[member] = offset;
        }

        // The row of a slot's first member holds the union of the conflicts of all its members.
        let mut slot_conflicts = self.conflicts.clone();
        let mut slots = Vec::<Vec<(usize, u64)>>::new();
        let mut slot_of_candidate = vec![None; self.candidates.len()];
        for group in groups {
            let fits = |slot: usize| {
                let leader = slots[slot].first().map(|&(leader, _)| leader);
                group.iter().all(|&(member, offset)| {
                    leader.is_none_or(|leader| !slot_conflicts.contains(leader, member))
                        && self.starts[member].iter().all(|&(other, _)| {
                            slot_of_candidate[other] != Some(slot)
                                || self.may_overlap(
                                    (member, offset),
                                    (other, offset_of_candidate[other]),
                                    true,
                                )
                        })
                })
            };
            let slot = (0..slots.len()).find(|&slot| fits(slot)).unwrap_or_else(|| {
                slots.push(Vec::new());
                slots.len() - 1
            });
            let leader = slots[slot].first().map_or(group[0].0, |&(leader, _)| leader);
            for &(member, _) in &group {
                slot_conflicts.union_rows(member, leader);
                slot_of_candidate[member] = Some(slot);
            }
            slots[slot].extend(group);
        }
        slots
    }
}

/// Finds the candidates that must not share a slot. Like rustc's coroutine layout, it keeps one
/// bit matrix row per candidate rather than per local.
///
/// A candidate whose address is never taken is in use while its value is live. Writing it
/// conflicts with every candidate that is live afterwards, and with every candidate the same
/// statement reads, as the write may come before the read. The exceptions are the copies of a
/// whole local, as in `dest = move src`, and of a local into or out of a field of another one, as
/// in `dest = Some(copy src)` and `dest = move (src as Some).0`. These copy in one go, and the
/// two places hold the same value afterwards, so `src` and `dest` may be placed on top of each
/// other, which skips the copy. Later writes to either of them conflict with the other as usual.
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
    /// The candidates in `in_use` that a statement copies, which it does not conflict with.
    copied_in_use: Vec<usize>,
}

impl ConflictCollector {
    fn new(local_count: usize, candidates: &[Candidate]) -> Self {
        let mut value_candidate_of_local = IndexVec::from_elem_n(None, local_count);
        let mut borrowed_candidate_of_local = IndexVec::from_elem_n(None, local_count);
        for (candidate, &Candidate { local, borrowed, .. }) in candidates.iter().enumerate() {
            if borrowed {
                borrowed_candidate_of_local[local] = Some(candidate);
            } else {
                value_candidate_of_local[local] = Some(candidate);
            }
        }
        ConflictCollector {
            value_candidate_of_local,
            borrowed_candidate_of_local,
            conflicts: BitMatrix::new(candidates.len(), candidates.len()),
            in_use: DenseBitSet::new_empty(candidates.len()),
            copied_in_use: Vec::new(),
        }
    }

    fn collect<'tcx>(
        mut self,
        fx: &FunctionCx<'_, '_, 'tcx>,
        always_live_locals: &DenseBitSet<Local>,
    ) -> Conflicts {
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
        let mut moves = Vec::new();
        let mut statement_moves = Vec::new();
        let mut copied = Vec::new();

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
            self.add_conflicts(&live, &def_uses, &[], &storage_live);
            live_results.analysis.apply_primary_terminator_effect(&mut live, terminator, location);

            for (statement_index, statement) in data.statements.iter().enumerate().rev() {
                let location = Location { block, statement_index };
                def_uses.clear();
                def_uses.visit_statement(statement, location);

                // The candidates that `statement` copies, and does not write over first.
                copied.clear();
                statement_moves.clear();
                moves_of(fx, &self.value_candidate_of_local, statement, &mut statement_moves);
                for &(source, mv) in &statement_moves {
                    if def_uses.uses.iter().filter(|&&local| local == source).count() == 1 {
                        moves.push(mv);
                        copied.push(self.value_candidate_of_local[source].unwrap());
                        def_uses.uses.retain(|&local| local != source);
                    }
                }

                self.add_conflicts(&live, &def_uses, &copied, &storage_live);
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
            storage_changes.clear();

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

        // A candidate that is copied to different fields of another one conflicts with it, as it
        // cannot be at both.
        moves.sort_unstable();
        moves.dedup();
        for pair in moves.windows(2) {
            if pair[0].inner == pair[1].inner && pair[0].outer == pair[1].outer {
                conflicts.insert(pair[0].inner, pair[0].outer);
                conflicts.insert(pair[0].outer, pair[0].inner);
            }
        }
        moves.retain(|mv| !conflicts.contains(mv.inner, mv.outer));

        Conflicts { conflicts, moves }
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
    /// it, and the borrowed candidates whose storage is live there. The candidates in `copied`
    /// hold the same value as what is written, so they do not conflict with it.
    fn add_conflicts(
        &mut self,
        live: &DenseBitSet<Local>,
        def_uses: &DefUses,
        copied: &[usize],
        storage_live: &DenseBitSet<usize>,
    ) {
        let writes_value =
            def_uses.defs.iter().any(|&local| self.value_candidate_of_local[local].is_some());
        if !writes_value && storage_live.is_empty() {
            return;
        }
        self.fill_in_use(live);
        self.copied_in_use.clear();
        self.copied_in_use.extend(copied.iter().copied().filter(|&c| self.in_use.remove(c)));
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
        for &candidate in &self.copied_in_use {
            self.in_use.insert(candidate);
        }
        if !storage_live.is_empty() {
            for candidate in self.in_use.iter() {
                self.conflicts.union_row_with(storage_live, candidate);
            }
        }
    }
}

/// The result of `ConflictCollector`.
struct Conflicts {
    /// A symmetric matrix of the candidates that must not share a slot.
    conflicts: BitMatrix<usize, usize>,
    /// The copies that are skipped if the candidates are placed as given.
    moves: Vec<Move>,
}

/// Returns the local that `statement` copies as a whole, to the local it assigns, into a field of
/// it, or out of a field of it, with the `Move` that this makes between their candidates.
/// Copies that write over the field before they read it are left out.
fn moves_of<'tcx>(
    fx: &FunctionCx<'_, '_, 'tcx>,
    value_candidate_of_local: &IndexVec<Local, Option<usize>>,
    statement: &Statement<'tcx>,
    moves: &mut Vec<(Local, Move)>,
) {
    let layout_of = |local: Local| fx.layout_of(fx.monomorphize(fx.mir.local_decls[local].ty));
    let Assign(assign) = &statement.kind else { return };
    let Some(dest) = assign.0.as_local() else { return };
    let Some(dest_candidate) = value_candidate_of_local[dest] else { return };

    match &assign.1 {
        Rvalue::Aggregate(kind, operands) => {
            let dest_layout = layout_of(dest);
            let (variant, layout) = match **kind {
                AggregateKind::Adt(_, variant, _, _, None) => {
                    (variant, dest_layout.for_variant(fx, variant))
                }
                AggregateKind::Tuple | AggregateKind::Closure(..) => (FIRST_VARIANT, dest_layout),
                _ => return,
            };
            if !matches!(layout.fields, FieldsShape::Arbitrary { .. }) {
                return;
            }
            // The tag is written after the fields.
            let tag_field = match dest_layout.variants {
                Variants::Multiple { tag_field, tag_encoding: TagEncoding::Direct, .. } => {
                    Some(tag_field)
                }
                Variants::Multiple {
                    tag_field,
                    tag_encoding: TagEncoding::Niche { untagged_variant, .. },
                    ..
                } if variant != untagged_variant => Some(tag_field),
                _ => None,
            };
            let tag = tag_field.map(|tag_field| {
                let start = dest_layout.fields.offset(tag_field.index()).bytes();
                start..start + dest_layout.field(fx, tag_field.index()).size.bytes()
            });

            for (field, operand) in operands.iter_enumerated() {
                let (Operand::Move(place) | Operand::Copy(place)) = operand else { continue };
                let Some(source) = place.as_local() else { continue };
                let Some(source_candidate) = value_candidate_of_local[source] else { continue };
                let offset = layout.fields.offset(field.index()).bytes();
                let size = layout.field(fx, field.index()).size.bytes();
                let overlaps_tag =
                    tag.as_ref().is_some_and(|tag| offset < tag.end && tag.start < offset + size);
                if source_candidate != dest_candidate
                    && size == layout_of(source).size.bytes()
                    && !overlaps_tag
                {
                    let mv = Move { inner: source_candidate, outer: dest_candidate, offset };
                    moves.push((source, mv));
                }
            }
        }
        Rvalue::Use(Operand::Move(place) | Operand::Copy(place), _) => {
            let source = place.local;
            let Some(source_candidate) = value_candidate_of_local[source] else { return };
            let mut layout = layout_of(source);
            let mut offset = 0;
            for elem in place.projection {
                match elem {
                    PlaceElem::Field(field, _) => {
                        if !matches!(layout.fields, FieldsShape::Arbitrary { .. }) {
                            return;
                        }
                        offset += layout.fields.offset(field.index()).bytes();
                        layout = layout.field(fx, field.index());
                    }
                    PlaceElem::Downcast(_, variant) => layout = layout.for_variant(fx, variant),
                    _ => return,
                }
            }
            if source_candidate != dest_candidate && layout.size == layout_of(dest).size {
                let mv = if place.projection.is_empty() {
                    Move { inner: source_candidate, outer: dest_candidate, offset: 0 }
                } else {
                    Move { inner: dest_candidate, outer: source_candidate, offset }
                };
                moves.push((source, mv));
            }
        }
        _ => {}
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

    fn visit_terminator(&mut self, terminator: &Terminator<'tcx>, location: Location) {
        self.super_terminator(terminator, location);
        // The callee may write to an argument that it is moved by reference, and drop glue to the
        // place it drops.
        match &terminator.kind {
            TerminatorKind::Call { args, .. } | TerminatorKind::TailCall { args, .. } => {
                for arg in args {
                    if let Operand::Move(place) = &arg.node
                        && !place.is_indirect()
                    {
                        self.defs.push(place.local);
                    }
                }
            }
            TerminatorKind::Drop { place, .. } if !place.is_indirect() => {
                self.defs.push(place.local);
            }
            _ => {}
        }
    }
}
