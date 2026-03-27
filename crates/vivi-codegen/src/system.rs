use std::collections::{HashMap, HashSet};
use vivi_parser::ast::*;
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::SystemInfo;
use vivi_sema::types::Ty;
use wasm_encoder::{Function, Instruction, ValType};

use crate::expr::ExprCtx;

/// Compile a system's `each` body into a WASM function.
/// The function iterates over all entities and executes the body for each.
pub fn compile_system(
    sys: &SystemInfo,
    each_stmts: &[Stmt],
    layout: &MemoryLayout,
    fn_index_map: &HashMap<String, u32>,
    void_fns: &HashSet<String>,
) -> Function {
    let entity_index_local: u32 = 0;
    let mut ctx = ExprCtx::new(layout, &sys.each_params, entity_index_local, fn_index_map, void_fns);

    let mut instrs: Vec<Instruction<'static>> = Vec::new();

    // Pre-scan for let statements to count needed locals
    let extra_locals = count_let_stmts(each_stmts);

    // Initialize loop counter to 0
    instrs.push(Instruction::I32Const(0));
    instrs.push(Instruction::LocalSet(entity_index_local));

    // block { loop {
    //   if entity_index >= entity_count: break
    //   ... body ...
    //   entity_index++
    //   br loop
    // } }
    instrs.push(Instruction::Block(wasm_encoder::BlockType::Empty));
    instrs.push(Instruction::Loop(wasm_encoder::BlockType::Empty));

    // Load entity_count from memory[0]
    instrs.push(Instruction::LocalGet(entity_index_local));
    instrs.push(Instruction::I32Const(0));
    instrs.push(Instruction::I32Load(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    instrs.push(Instruction::I32GeS);
    instrs.push(Instruction::BrIf(1));

    // Compile each statement in the body
    for stmt in each_stmts {
        compile_stmt(stmt, &mut ctx, &mut instrs);
    }

    // Increment entity index
    instrs.push(Instruction::LocalGet(entity_index_local));
    instrs.push(Instruction::I32Const(1));
    instrs.push(Instruction::I32Add);
    instrs.push(Instruction::LocalSet(entity_index_local));

    // Branch back to loop
    instrs.push(Instruction::Br(0));

    instrs.push(Instruction::End); // end loop
    instrs.push(Instruction::End); // end block
    instrs.push(Instruction::End); // end function

    // Build locals: entity_index (i32) + user locals
    // We declare enough i32 and f32 locals to cover all let bindings.
    // For simplicity, declare all extra locals as both i32 and f32 groups.
    let mut local_groups: Vec<(u32, ValType)> = vec![(1, ValType::I32)]; // entity_index
    if extra_locals > 0 {
        // Count how many of each type were allocated
        let mut i32_count = 0u32;
        let mut f32_count = 0u32;
        for local in ctx.locals.values() {
            match local.ty {
                Ty::F32 => f32_count += 1,
                Ty::F64 => {} // would need f64 locals
                _ => i32_count += 1,
            }
        }
        if i32_count > 0 {
            local_groups.push((i32_count, ValType::I32));
        }
        if f32_count > 0 {
            local_groups.push((f32_count, ValType::F32));
        }
    }

    let mut func = Function::new(local_groups);
    for instr in &instrs {
        func.instruction(instr);
    }

    func
}

fn compile_stmt(stmt: &Stmt, ctx: &mut ExprCtx, instrs: &mut Vec<Instruction<'static>>) {
    match stmt {
        Stmt::Assign(assign) => {
            if let Expr::FieldAccess(obj, field, _) = &assign.target {
                ctx.compile_field_store(obj, field, &assign.value, instrs);
            } else if let Expr::Ident(name, _) = &assign.target {
                // Local variable assignment
                if let Some(local) = ctx.locals.get(name) {
                    let index = local.index;
                    ctx.compile_expr(&assign.value, instrs);
                    instrs.push(Instruction::LocalSet(index));
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
                infer_expr_ty_simple(&let_stmt.value, ctx)
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
        Stmt::Expr(expr) => {
            ctx.compile_expr(expr, instrs);
            if !is_void_call(expr, ctx.void_fns) {
                instrs.push(Instruction::Drop);
            }
        }
        Stmt::Return(_, _) => {
            // In system context, return exits the function
            instrs.push(Instruction::Return);
        }
    }
}

fn count_let_stmts(stmts: &[Stmt]) -> u32 {
    let mut count = 0u32;
    for stmt in stmts {
        match stmt {
            Stmt::Let(_) => count += 1,
            Stmt::If(if_stmt) => {
                count += count_let_stmts(&if_stmt.then_body);
                if let Some(else_body) = &if_stmt.else_body {
                    count += count_let_stmts(else_body);
                }
            }
            Stmt::While(while_stmt) => {
                count += count_let_stmts(&while_stmt.body);
            }
            _ => {}
        }
    }
    count
}

/// Simple type inference for codegen (sema already validated types).
fn infer_expr_ty_simple(expr: &Expr, ctx: &ExprCtx) -> Ty {
    match expr {
        Expr::FloatLit(_, _) => Ty::F32,
        Expr::IntLit(_, _) => Ty::I32,
        Expr::BoolLit(_, _) => Ty::Bool,
        Expr::Ident(name, _) => {
            ctx.locals.get(name).map_or(Ty::I32, |l| l.ty.clone())
        }
        Expr::FieldAccess(obj, field, _) => {
            if let Expr::Ident(param_name, _) = obj.as_ref() {
                if let Some(param) = ctx.params.iter().find(|p| p.name == *param_name) {
                    let comp = ctx.layout.get_component(&param.component).unwrap();
                    return comp.fields.iter().find(|f| f.name == *field).unwrap().ty.clone();
                }
            }
            Ty::I32
        }
        Expr::BinOp(left, op, _, _) => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => infer_expr_ty_simple(left, ctx),
            _ => Ty::Bool,
        },
        Expr::Call(_, _, _) => Ty::F32, // sema validated; return type used at call site
        Expr::UnaryOp(UnaryOp::Neg, inner, _) => infer_expr_ty_simple(inner, ctx),
        Expr::UnaryOp(UnaryOp::Not, _, _) => Ty::Bool,
    }
}

fn is_void_call(expr: &Expr, void_fns: &HashSet<String>) -> bool {
    if let Expr::Call(name, _, _) = expr {
        void_fns.contains(name)
    } else {
        false
    }
}
