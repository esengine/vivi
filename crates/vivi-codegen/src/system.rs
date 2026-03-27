use vivi_parser::ast::*;
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::SystemInfo;
use wasm_encoder::{Function, Instruction, ValType};

use crate::expr::ExprCtx;

/// Compile a system's `each` body into a WASM function.
/// The function iterates over all entities and executes the body for each.
pub fn compile_system(
    sys: &SystemInfo,
    each_stmts: &[Stmt],
    layout: &MemoryLayout,
) -> Function {
    let mut func = Function::new(vec![(1, ValType::I32)]); // one local: entity index (i32)
    let entity_index_local: u32 = 0;

    let ctx = ExprCtx {
        layout,
        params: &sys.each_params,
        entity_index_local,
    };

    let mut instrs: Vec<Instruction<'static>> = Vec::new();

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
    instrs.push(Instruction::I32Const(0)); // entity_count offset
    instrs.push(Instruction::I32Load(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    instrs.push(Instruction::I32GeS);
    instrs.push(Instruction::BrIf(1)); // break out of block

    // Compile each statement in the body
    for stmt in each_stmts {
        compile_stmt(stmt, &ctx, &mut instrs);
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

    for instr in &instrs {
        func.instruction(instr);
    }

    func
}

fn compile_stmt(stmt: &Stmt, ctx: &ExprCtx, instrs: &mut Vec<Instruction<'static>>) {
    match stmt {
        Stmt::Assign(assign) => {
            // Target must be a field access
            if let Expr::FieldAccess(obj, field, _) = &assign.target {
                ctx.compile_field_store(obj, field, &assign.value, instrs);
            } else {
                panic!("assignment target must be a field access");
            }
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
        Stmt::Expr(expr) => {
            ctx.compile_expr(expr, instrs);
            instrs.push(Instruction::Drop); // discard result
        }
        _ => {
            // let, while, return - not needed for Phase 1 MVP
        }
    }
}
