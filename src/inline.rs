//! Inlining of direct calls between the functions of a codegen unit at the CLIF level.
//!
//! The pass runs after all functions of a codegen unit have been translated to CLIF and before any
//! of them is compiled. Callees are processed before their callers, so every function that gets
//! inlined already has its own inlined calls flattened into it. Inlined functions are never
//! removed from the codegen unit, as other code may still refer to them.
//!
//! The stack slots of an inlined callee are only live while the inlined body runs and the bodies
//! of two call sites never run at the same time, so all stack slots introduced by inlining into one
//! function are overlaid on a single slot, sized for the largest call site.

use std::borrow::Cow;
use std::mem;

use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::inline::{Inline, InlineCommand};
use cranelift_codegen::ir::{
    ArgumentPurpose, ExternalName, GlobalValue, GlobalValueData, InstructionData, Opcode,
};
use rustc_attr_ir::InlineAttr;
use rustc_errors::DiagCtxtHandle;

use crate::base::CodegenedFunction;
use crate::prelude::*;

/// Without `#[inline(always)]` only callees that don't call anything and have at most this many
/// instructions, not counting nops and jumps, are inlined, and only when optimizing.
const MAX_LEAF_INSTS: usize = 100;

/// The namespace of the user external names of functions. See `UserFuncName::user(0, ..)` in
/// `codegen_fn`.
const FUNC_NAMESPACE: u32 = 0;

#[derive(Clone, Copy)]
struct Summary {
    insts: usize,
    has_calls: bool,
    /// Whether the function can't be inlined: it depends on its own frame, tail calls or has a
    /// by-value aggregate parameter that a call copies but inlining would not.
    pinned: bool,
}

impl Summary {
    fn new(func: &Function) -> Summary {
        let mut summary = Summary {
            insts: 0,
            has_calls: false,
            pinned: func
                .signature
                .params
                .iter()
                .any(|param| matches!(param.purpose, ArgumentPurpose::StructArgument(_))),
        };
        for block in func.layout.blocks() {
            for inst in func.layout.block_insts(block) {
                let opcode = func.dfg.insts[inst].opcode();
                // cg_clif emits many of these and Cranelift removes them.
                if !matches!(opcode, Opcode::Nop | Opcode::Jump) {
                    summary.insts += 1;
                }
                summary.has_calls |= opcode.is_call();
                summary.pinned |= matches!(
                    opcode,
                    Opcode::ReturnCall
                        | Opcode::ReturnCallIndirect
                        | Opcode::GetFramePointer
                        | Opcode::GetStackPointer
                        | Opcode::GetReturnAddress
                );
            }
        }
        summary
    }
}

#[derive(Clone, Copy)]
struct Policy {
    optimize: bool,
}

impl Policy {
    /// Whether a call to a function with this body is worth inlining.
    fn should_inline(self, attr: &InlineAttr, summary: Summary) -> bool {
        !summary.pinned
            && match attr {
                InlineAttr::Never => false,
                InlineAttr::Always | InlineAttr::Force { .. } => true,
                InlineAttr::None | InlineAttr::Hint => {
                    self.optimize && !summary.has_calls && summary.insts <= MAX_LEAF_INSTS
                }
            }
    }

    /// Whether a function might get inlined once its own calls are inlined. Inlining makes a
    /// function larger, so this is only false for functions that stay too large.
    fn may_inline(self, attr: &InlineAttr, summary: Summary) -> bool {
        self.should_inline(attr, Summary { has_calls: false, ..summary })
    }
}

pub(crate) fn inline_cgu(
    dcx: DiagCtxtHandle<'_>,
    optimize: bool,
    functions: &mut [CodegenedFunction],
) {
    let policy = Policy { optimize };

    let index = functions
        .iter()
        .enumerate()
        .map(|(idx, function)| (function.func_id, idx))
        .collect::<FxHashMap<_, _>>();
    let mut summaries = functions.iter().map(|f| Summary::new(&f.func)).collect::<Vec<_>>();
    let (order, callees) = callees_first_order(policy, functions, &summaries, &index);

    let mut processed = vec![false; functions.len()];
    let mut ctx = Context::new();
    for caller in order {
        if callees[caller].is_empty() {
            processed[caller] = true;
            continue;
        }
        ctx.func = mem::replace(&mut functions[caller].func, Function::new());
        let own_slots = ctx.func.sized_stack_slots.len();
        let own_insts = ctx.func.dfg.num_insts();

        let mut call_sites = CallSites {
            policy,
            functions,
            index: &index,
            summaries: &summaries,
            processed: &processed,
            caller,
            slot_ranges: Vec::new(),
            global_values: Vec::new(),
        };
        let inlined_any = ctx
            .inline(&mut call_sites)
            .unwrap_or_else(|err| dcx.fatal(format!("failed to inline calls: {err:?}")));
        let CallSites { slot_ranges, global_values, .. } = call_sites;

        if inlined_any {
            fix_inlined_symbol_names(&mut ctx.func, functions, &global_values);
            overlay_inlined_stack_slots(&mut ctx.func, own_slots, &slot_ranges);
            if functions[caller].func_debug_cx.is_some() {
                attribute_inlined_insts_to_call_sites(&mut ctx.func, own_insts);
            }
            summaries[caller] = Summary::new(&ctx.func);
        }
        functions[caller].func = mem::replace(&mut ctx.func, Function::new());
        processed[caller] = true;
    }
}

/// The functions of the codegen unit with every function that may be inlined into another one
/// ordered before its callers, and for each function the functions that it calls and that may be
/// inlined. Edges are only followed to functions that may be inlined, so every cycle consists of
/// such functions, and the edge that closes a cycle is skipped.
fn callees_first_order(
    policy: Policy,
    functions: &[CodegenedFunction],
    summaries: &[Summary],
    index: &FxHashMap<FuncId, usize>,
) -> (Vec<usize>, Vec<Vec<usize>>) {
    let callees = functions
        .iter()
        .map(|function| {
            let mut callees = Vec::new();
            for block in function.func.layout.blocks() {
                for inst in function.func.layout.block_insts(block) {
                    if let Some(callee) = direct_callee(&function.func, inst, index)
                        && policy.may_inline(&functions[callee].inline, summaries[callee])
                        && !callees.contains(&callee)
                    {
                        callees.push(callee);
                    }
                }
            }
            callees
        })
        .collect::<Vec<_>>();

    let mut order = Vec::with_capacity(functions.len());
    let mut visited = vec![false; functions.len()];
    let mut stack = Vec::new();
    // Start at functions that are not inlined. Then a cycle is entered at a function that is
    // inlined and the call to it that closes the cycle stays.
    let roots = (0..functions.len())
        .filter(|&idx| !policy.may_inline(&functions[idx].inline, summaries[idx]))
        .chain(0..functions.len());
    for root in roots {
        if mem::replace(&mut visited[root], true) {
            continue;
        }
        stack.push((root, 0));
        while let Some((node, next)) = stack.last_mut() {
            let node = *node;
            if let Some(&callee) = callees[node].get(*next) {
                *next += 1;
                if !mem::replace(&mut visited[callee], true) {
                    stack.push((callee, 0));
                }
            } else {
                order.push(node);
                stack.pop();
            }
        }
    }
    (order, callees)
}

/// The index of the function of this codegen unit that `inst` calls directly, if any.
fn direct_callee(func: &Function, inst: Inst, index: &FxHashMap<FuncId, usize>) -> Option<usize> {
    let (InstructionData::Call { opcode: Opcode::Call, func_ref, .. }
    | InstructionData::TryCall { func_ref, .. }) = func.dfg.insts[inst]
    else {
        return None;
    };
    let ExternalName::User(name) = func.dfg.ext_funcs[func_ref].name else {
        return None;
    };
    let name = &func.params.user_named_funcs()[name];
    if name.namespace != FUNC_NAMESPACE {
        return None;
    }
    index.get(&FuncId::from_u32(name.index)).copied()
}

struct CallSites<'a> {
    policy: Policy,
    functions: &'a [CodegenedFunction],
    index: &'a FxHashMap<FuncId, usize>,
    summaries: &'a [Summary],
    processed: &'a [bool],
    caller: usize,
    /// The first stack slot and the number of stack slots of each inlined call site.
    slot_ranges: Vec<(u32, u32)>,
    /// The first global value and the callee of each inlined call site.
    global_values: Vec<(u32, usize)>,
}

impl Inline for CallSites<'_> {
    fn inline(
        &mut self,
        caller: &Function,
        call_inst: Inst,
        _call_opcode: Opcode,
        callee: FuncRef,
        _call_args: &[Value],
    ) -> InlineCommand<'_> {
        let Some(callee_idx) = direct_callee(caller, call_inst, self.index) else {
            return InlineCommand::KeepCall;
        };
        let callee_fn = &self.functions[callee_idx];
        let callee_sig = caller.dfg.ext_funcs[callee].signature;
        // Calls into a cycle of functions that may be inlined can reach a function that is not
        // flattened yet. Those and recursive calls stay calls.
        if callee_idx == self.caller
            || !self.processed[callee_idx]
            || !self.policy.should_inline(&callee_fn.inline, self.summaries[callee_idx])
            || !caller.dfg.ext_funcs[callee].colocated
            || caller.dfg.signatures[callee_sig] != callee_fn.func.signature
        {
            return InlineCommand::KeepCall;
        }

        // Cranelift appends the stack slots of the callee to those of the caller.
        let slots = callee_fn.func.sized_stack_slots.len();
        if slots != 0 {
            self.slot_ranges.push((caller.sized_stack_slots.len() as u32, slots as u32));
        }
        if !callee_fn.func.global_values.is_empty() {
            self.global_values.push((caller.global_values.len() as u32, callee_idx));
        }
        InlineCommand::Inline { callee: Cow::Borrowed(&callee_fn.func), visit_callee: false }
    }
}

/// Cranelift 0.137 copies the symbols of the global values of an inlined callee into the caller as
/// is, but their names refer to the user external names of the callee. Point them at the same names
/// in the caller. This has to go once Cranelift does that itself.
fn fix_inlined_symbol_names(
    func: &mut Function,
    functions: &[CodegenedFunction],
    global_values: &[(u32, usize)],
) {
    for &(start, callee) in global_values {
        let callee = &functions[callee].func;
        for (offset, data) in callee.global_values.values().enumerate() {
            let GlobalValueData::Symbol { name: ExternalName::User(callee_name), .. } = data else {
                continue;
            };
            let name = callee.params.user_named_funcs()[*callee_name].clone();
            let name = func.params.ensure_user_func_name(name);
            let gv = GlobalValue::from_u32(start + offset as u32);
            let GlobalValueData::Symbol { name: caller_name, .. } = &mut func.global_values[gv]
            else {
                unreachable!();
            };
            *caller_name = ExternalName::User(name);
        }
    }
}

/// Replace the stack slots that inlining appended after the first `own_slots` slots with a single
/// slot shared by all inlined call sites. cg_clif doesn't emit debug tags and stack maps, which
/// would refer to the stack slots too.
fn overlay_inlined_stack_slots(func: &mut Function, own_slots: usize, slot_ranges: &[(u32, u32)]) {
    if slot_ranges.is_empty() {
        return;
    }

    let mut offsets = vec![0; func.sized_stack_slots.len() - own_slots];
    let mut size = 0;
    let mut align_shift = 0;
    for &(start, len) in slot_ranges {
        let mut site_size = 0u32;
        for slot in start..start + len {
            let data = &func.sized_stack_slots[StackSlot::from_u32(slot)];
            site_size = site_size.next_multiple_of(1 << data.align_shift);
            offsets[slot as usize - own_slots] = site_size as i32;
            site_size += data.size;
            align_shift = align_shift.max(data.align_shift);
        }
        size = size.max(site_size);
    }

    let own = func.sized_stack_slots.values().take(own_slots).cloned().collect::<Vec<_>>();
    func.sized_stack_slots.clear();
    for slot in own {
        func.sized_stack_slots.push(slot);
    }
    let overlay = func.sized_stack_slots.push(StackSlotData {
        kind: StackSlotKind::ExplicitSlot,
        size: size.next_multiple_of(1 << align_shift),
        align_shift,
        key: None,
    });

    let mut cursor = FuncCursor::new(func);
    while cursor.next_block().is_some() {
        while let Some(inst) = cursor.next_inst() {
            let InstructionData::StackAddr { stack_slot, offset, .. } =
                &mut cursor.func.dfg.insts[inst]
            else {
                continue;
            };
            if stack_slot.index() >= own_slots {
                *offset = (i32::from(*offset) + offsets[stack_slot.index() - own_slots]).into();
                *stack_slot = overlay;
            }
        }
    }
}

/// Cranelift doesn't carry source locations over into inlined instructions. Give them the location
/// of the call they replace instead of none, which would show up as the start of the function.
fn attribute_inlined_insts_to_call_sites(func: &mut Function, own_insts: usize) {
    let func = &mut func.stencil;
    let mut call_site = Default::default();
    for block in func.layout.blocks() {
        for inst in func.layout.block_insts(block) {
            if inst.index() >= own_insts {
                func.srclocs[inst] = call_site;
            } else if !func.srclocs[inst].is_default() {
                call_site = func.srclocs[inst];
            }
        }
    }
}
