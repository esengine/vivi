use std::collections::{HashMap, HashSet};
use vivi_parser::ast::*;
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::SystemInfo;
use vivi_sema::types::Ty;
use wasm_encoder::{Function, Instruction, ValType};

use crate::expr::ExprCtx;
use crate::sourcemap::{FuncMappings, RawMapping};

/// Compile a system's `each` body into a WASM function.
/// Compile a system. If it has each_params, wraps in entity loop. Otherwise runs once.
pub fn compile_system(
    sys: &SystemInfo,
    stmts: &[Stmt],
    layout: &MemoryLayout,
    fn_index_map: &HashMap<String, u32>,
    void_fns: &HashSet<String>,
    globals: &HashMap<String, crate::expr::GlobalVar>,
    fn_return_types: &HashMap<String, Ty>,
    source: &str,
    func_mappings: &mut FuncMappings,
) -> Function {
    let is_bare = sys.each_params.is_empty();
    let entity_index_local: u32 = 0;
    let mut ctx = ExprCtx::new(layout, &sys.each_params, entity_index_local, fn_index_map, void_fns, globals, fn_return_types);

    let mut instrs: Vec<Instruction<'static>> = Vec::new();

    if is_bare {
        // Bare system: just compile statements, no entity loop
        for stmt in stmts {
            record_stmt_mapping(stmt, instrs.len(), source, func_mappings);
            compile_stmt(stmt, &mut ctx, &mut instrs);
        }
    } else {
        // System with each: entity loop
        instrs.push(Instruction::I32Const(0));
        instrs.push(Instruction::LocalSet(entity_index_local));

        instrs.push(Instruction::Block(wasm_encoder::BlockType::Empty));
        instrs.push(Instruction::Loop(wasm_encoder::BlockType::Empty));

        instrs.push(Instruction::LocalGet(entity_index_local));
        instrs.push(Instruction::I32Const(0));
        instrs.push(Instruction::I32Load(wasm_encoder::MemArg {
            offset: 0, align: 2, memory_index: 0,
        }));
        instrs.push(Instruction::I32GeS);
        instrs.push(Instruction::BrIf(1));

        for stmt in stmts {
            record_stmt_mapping(stmt, instrs.len(), source, func_mappings);
            compile_stmt(stmt, &mut ctx, &mut instrs);
        }

        instrs.push(Instruction::LocalGet(entity_index_local));
        instrs.push(Instruction::I32Const(1));
        instrs.push(Instruction::I32Add);
        instrs.push(Instruction::LocalSet(entity_index_local));

        // Branch back to loop
        instrs.push(Instruction::Br(0));

        instrs.push(Instruction::End); // end loop
        instrs.push(Instruction::End); // end block
    }

    instrs.push(Instruction::End); // end function

    // Build locals: entity_index (i32) + user locals in index order
    let mut local_groups: Vec<(u32, ValType)> = vec![(1, ValType::I32)]; // entity_index
    local_groups.extend(build_local_groups(&ctx, 1));

    let mut func = Function::new(local_groups);
    for instr in &instrs {
        func.instruction(instr);
    }

    func
}

/// Compile a list of statements with source map tracking.
pub fn compile_stmts_with_mappings(
    stmts: &[Stmt],
    ctx: &mut ExprCtx,
    instrs: &mut Vec<Instruction<'static>>,
    source: &str,
    func_mappings: &mut FuncMappings,
) {
    for stmt in stmts {
        record_stmt_mapping(stmt, instrs.len(), source, func_mappings);
        compile_stmt(stmt, ctx, instrs);
    }
}

/// Build WASM local declaration groups from ExprCtx, skipping indices < skip_count.
/// Locals must be declared in index order to match alloc_local's sequential assignment.
pub fn build_local_groups(ctx: &ExprCtx, skip_count: u32) -> Vec<(u32, ValType)> {
    // Collect locals sorted by index
    let mut sorted: Vec<_> = ctx.locals.values()
        .filter(|l| l.index >= skip_count)
        .collect();
    sorted.sort_by_key(|l| l.index);

    // Group consecutive same-type locals
    let mut groups: Vec<(u32, ValType)> = Vec::new();
    for local in sorted {
        let vt = match local.ty {
            Ty::F32 => ValType::F32,
            Ty::F64 => ValType::F64,
            Ty::I64 => ValType::I64,
            _ => ValType::I32,
        };
        if let Some(last) = groups.last_mut() {
            if last.1 == vt {
                last.0 += 1;
                continue;
            }
        }
        groups.push((1, vt));
    }
    groups
}

fn compile_stmt(stmt: &Stmt, ctx: &mut ExprCtx, instrs: &mut Vec<Instruction<'static>>) {
    match stmt {
        Stmt::Assign(assign) => {
            if let Expr::FieldAccess(obj, field, _) = &assign.target {
                ctx.compile_field_store(obj, field, &assign.value, instrs);
            } else if let Expr::Ident(name, _) = &assign.target {
                if let Some(local) = ctx.locals.get(name) {
                    let index = local.index;
                    ctx.compile_expr(&assign.value, instrs);
                    instrs.push(Instruction::LocalSet(index));
                } else if let Some(gvar) = ctx.globals.get(name) {
                    // Global variable assignment: store to memory
                    let offset = gvar.offset;
                    let ty = gvar.ty.clone();
                    instrs.push(Instruction::I32Const(offset as i32));
                    ctx.compile_expr(&assign.value, instrs);
                    instrs.push(ctx.store_instr(&ty));
                } else {
                    panic!("assignment to undefined variable `{name}`");
                }
            } else {
                panic!("invalid assignment target");
            }
        }
        Stmt::Let(let_stmt) => {
            // Determine type from annotation or infer from value
            let ty = if let Some(ast_ty) = &let_stmt.ty {
                Ty::from_ast(ast_ty)
            } else {
                // Infer: float literal → f32, int literal → i32, etc.
                ctx.infer_expr_ty(&let_stmt.value)
            };
            let index = ctx.alloc_local(let_stmt.name.clone(), ty);
            ctx.compile_expr(&let_stmt.value, instrs);
            instrs.push(Instruction::LocalSet(index));
        }
        Stmt::If(if_stmt) => {
            ctx.compile_expr(&if_stmt.condition, instrs);
            instrs.push(Instruction::If(wasm_encoder::BlockType::Empty));
            for s in &if_stmt.then_body {
                compile_stmt(s, ctx, instrs);
            }
            if let Some(else_body) = &if_stmt.else_body {
                instrs.push(Instruction::Else);
                for s in else_body {
                    compile_stmt(s, ctx, instrs);
                }
            }
            instrs.push(Instruction::End);
        }
        Stmt::While(while_stmt) => {
            // block { loop { br_if (not cond) 1; body; br 0; } }
            instrs.push(Instruction::Block(wasm_encoder::BlockType::Empty));
            instrs.push(Instruction::Loop(wasm_encoder::BlockType::Empty));

            ctx.compile_expr(&while_stmt.condition, instrs);
            instrs.push(Instruction::I32Eqz);
            instrs.push(Instruction::BrIf(1)); // break if condition is false

            for s in &while_stmt.body {
                compile_stmt(s, ctx, instrs);
            }

            instrs.push(Instruction::Br(0)); // continue loop
            instrs.push(Instruction::End); // end loop
            instrs.push(Instruction::End); // end block
        }
        Stmt::Spawn(spawn) => {
            compile_spawn(spawn, ctx, instrs);
        }
        Stmt::Despawn(_) => {
            compile_despawn(ctx, instrs);
        }
        Stmt::Expr(expr) => {
            ctx.compile_expr(expr, instrs);
            if !is_void_call(expr, ctx.void_fns) {
                instrs.push(Instruction::Drop);
            }
        }
        Stmt::Return(Some(expr), _) => {
            ctx.compile_expr(expr, instrs);
            instrs.push(Instruction::Return);
        }
        Stmt::Return(None, _) => {
            instrs.push(Instruction::Return);
        }
    }
}

fn is_void_call(expr: &Expr, void_fns: &HashSet<String>) -> bool {
    if let Expr::Call(name, _, _) = expr {
        void_fns.contains(name) || name.starts_with("mem_store")
    } else {
        false
    }
}

fn compile_spawn(
    spawn: &SpawnStmt,
    ctx: &mut ExprCtx,
    instrs: &mut Vec<Instruction<'static>>,
) {
    // Allocate a local for the new entity index if not already done
    let spawn_idx_local = if ctx.locals.contains_key("__spawn_idx") {
        ctx.locals["__spawn_idx"].index
    } else {
        ctx.alloc_local("__spawn_idx".to_string(), Ty::I32)
    };

    // Load current entity_count into spawn_idx_local
    instrs.push(Instruction::I32Const(0)); // address of entity_count
    instrs.push(Instruction::I32Load(wasm_encoder::MemArg {
        offset: 0, align: 2, memory_index: 0,
    }));
    instrs.push(Instruction::LocalSet(spawn_idx_local));

    // Write each component field
    for sc in &spawn.components {
        let comp_layout = ctx.layout.get_component(&sc.component).unwrap();
        for (fname, fexpr) in &sc.fields {
            let fl = comp_layout.fields.iter().find(|f| f.name == *fname).unwrap();
            // address = fl.offset + spawn_idx * fl.element_size
            instrs.push(Instruction::I32Const(fl.offset as i32));
            instrs.push(Instruction::LocalGet(spawn_idx_local));
            instrs.push(Instruction::I32Const(fl.element_size as i32));
            instrs.push(Instruction::I32Mul);
            instrs.push(Instruction::I32Add);
            // evaluate value expression
            ctx.compile_expr(fexpr, instrs);
            // store
            let mem = wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 };
            match &fl.ty {
                Ty::F32 => instrs.push(Instruction::F32Store(mem)),
                Ty::I32 | Ty::Bool | Ty::Entity => instrs.push(Instruction::I32Store(mem)),
                Ty::F64 => instrs.push(Instruction::F64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 })),
                Ty::I64 => instrs.push(Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 })),
            }
        }
    }

    // Increment entity_count: memory[0] = spawn_idx + 1
    instrs.push(Instruction::I32Const(0));
    instrs.push(Instruction::LocalGet(spawn_idx_local));
    instrs.push(Instruction::I32Const(1));
    instrs.push(Instruction::I32Add);
    instrs.push(Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0, align: 2, memory_index: 0,
    }));
}

fn compile_despawn(
    ctx: &mut ExprCtx,
    instrs: &mut Vec<Instruction<'static>>,
) {
    let mem4 = wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 };

    // For each component, for each field: copy last entity's data to current entity
    for comp_layout in &ctx.layout.components {
        for fl in &comp_layout.fields {
            let elem = fl.element_size;

            // dst_addr = fl.offset + entity_index * element_size
            instrs.push(Instruction::I32Const(fl.offset as i32));
            instrs.push(Instruction::LocalGet(ctx.entity_index_local));
            instrs.push(Instruction::I32Const(elem as i32));
            instrs.push(Instruction::I32Mul);
            instrs.push(Instruction::I32Add);

            // src_addr = fl.offset + (entity_count - 1) * element_size
            instrs.push(Instruction::I32Const(fl.offset as i32));
            instrs.push(Instruction::I32Const(0));
            instrs.push(Instruction::I32Load(mem4));
            instrs.push(Instruction::I32Const(1));
            instrs.push(Instruction::I32Sub);
            instrs.push(Instruction::I32Const(elem as i32));
            instrs.push(Instruction::I32Mul);
            instrs.push(Instruction::I32Add);

            // load from src, store to dst
            match &fl.ty {
                Ty::F32 => {
                    instrs.push(Instruction::F32Load(mem4));
                    instrs.push(Instruction::F32Store(mem4));
                }
                Ty::I32 | Ty::Bool | Ty::Entity => {
                    instrs.push(Instruction::I32Load(mem4));
                    instrs.push(Instruction::I32Store(mem4));
                }
                Ty::F64 => {
                    let mem8 = wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 };
                    instrs.push(Instruction::F64Load(mem8));
                    instrs.push(Instruction::F64Store(mem8));
                }
                Ty::I64 => {
                    let mem8 = wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 };
                    instrs.push(Instruction::I64Load(mem8));
                    instrs.push(Instruction::I64Store(mem8));
                }
            }
        }
    }

    // Decrement entity_count: memory[0] = memory[0] - 1
    instrs.push(Instruction::I32Const(0));
    instrs.push(Instruction::I32Const(0));
    instrs.push(Instruction::I32Load(mem4));
    instrs.push(Instruction::I32Const(1));
    instrs.push(Instruction::I32Sub);
    instrs.push(Instruction::I32Store(mem4));

    // Decrement entity_index so the loop re-processes the swapped-in entity
    instrs.push(Instruction::LocalGet(ctx.entity_index_local));
    instrs.push(Instruction::I32Const(1));
    instrs.push(Instruction::I32Sub);
    instrs.push(Instruction::LocalSet(ctx.entity_index_local));
}

pub fn span_to_line_col(source: &str, offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub fn record_stmt_mapping(
    stmt: &Stmt,
    instr_index: usize,
    source: &str,
    mappings: &mut FuncMappings,
) {
    let span_start = match stmt {
        Stmt::Assign(s) => s.span.start,
        Stmt::Let(s) => s.span.start,
        Stmt::If(s) => s.span.start,
        Stmt::While(s) => s.span.start,
        Stmt::Spawn(s) => s.span.start,
        Stmt::Despawn(span) => span.start,
        Stmt::Expr(e) => e.span().start,
        Stmt::Return(_, span) => span.start,
    };
    let (line, col) = span_to_line_col(source, span_start);
    mappings.entries.push(RawMapping {
        instr_index: instr_index as u32,
        source_line: line,
        source_col: col,
    });
}
