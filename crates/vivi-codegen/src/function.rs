use std::collections::{HashMap, HashSet};
use vivi_parser::ast::*;
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::FnSignature;
use wasm_encoder::Function;

use crate::expr::{ExprCtx, LocalVar};
use crate::sourcemap::FuncMappings;
use crate::system::{compile_stmts_with_mappings, build_local_groups};

/// Compile a user-defined function into a WASM function.
/// Reuses the same ExprCtx + compile_stmt path as systems.
pub fn compile_user_fn(
    sig: &FnSignature,
    body: &[Stmt],
    layout: &MemoryLayout,
    fn_index_map: &HashMap<String, u32>,
    void_fns: &HashSet<String>,
    globals: &HashMap<String, crate::expr::GlobalVar>,
    source: &str,
    func_mappings: &mut FuncMappings,
) -> Function {
    let param_count = sig.params.len() as u32;

    // ExprCtx with entity_index_local=0 (unused for fns, but harmless)
    let empty_params = vec![];
    let mut ctx = ExprCtx::new(layout, &empty_params, 0, fn_index_map, void_fns, globals);

    // Pre-populate locals with function parameters (indices 0..param_count)
    for (i, (name, ty)) in sig.params.iter().enumerate() {
        ctx.locals.insert(name.clone(), LocalVar { index: i as u32, ty: ty.clone() });
    }
    ctx.next_local = param_count;

    let mut instrs = Vec::new();

    compile_stmts_with_mappings(body, &mut ctx, &mut instrs, source, func_mappings);

    instrs.push(wasm_encoder::Instruction::End);

    // Build local declarations (only for locals beyond params)
    let local_groups = build_local_groups(&ctx, param_count);

    let mut func = Function::new(local_groups);
    for instr in &instrs {
        func.instruction(instr);
    }
    func
}
