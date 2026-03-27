use std::collections::HashMap;
use vivi_parser::ast::*;
use vivi_sema::resolve::FnSignature;
use vivi_sema::types::Ty;
use wasm_encoder::{Function, Instruction, ValType};

use crate::expr::LocalVar;

/// Compile a user-defined function into a WASM function.
pub fn compile_user_fn(
    sig: &FnSignature,
    body: &[Stmt],
    fn_index_map: &HashMap<String, u32>,
) -> Function {
    // Function params are WASM function parameters (not locals).
    // Additional locals come from let statements.
    let param_count = sig.params.len() as u32;

    // Build locals map: params are at indices 0..param_count
    let mut locals: HashMap<String, LocalVar> = HashMap::new();
    for (i, (name, ty)) in sig.params.iter().enumerate() {
        locals.insert(name.clone(), LocalVar { index: i as u32, ty: ty.clone() });
    }

    let mut instrs: Vec<Instruction<'static>> = Vec::new();
    let mut next_local = param_count;

    // Compile body
    for stmt in body {
        compile_fn_stmt(stmt, &mut locals, &mut next_local, fn_index_map, &mut instrs);
    }

    // If function has no explicit return at the end, WASM needs an end.
    // For void functions this is fine. For non-void, sema should ensure return exists.
    instrs.push(Instruction::End);

    // Collect extra locals (beyond params)
    let mut i32_locals = 0u32;
    let mut f32_locals = 0u32;
    for local in locals.values() {
        if local.index >= param_count {
            match local.ty {
                Ty::F32 => f32_locals += 1,
                _ => i32_locals += 1,
            }
        }
    }
    let mut local_groups = Vec::new();
    if i32_locals > 0 { local_groups.push((i32_locals, ValType::I32)); }
    if f32_locals > 0 { local_groups.push((f32_locals, ValType::F32)); }

    let mut func = Function::new(local_groups);
    for instr in &instrs {
        func.instruction(instr);
    }
    func
}

fn compile_fn_stmt(
    stmt: &Stmt,
    locals: &mut HashMap<String, LocalVar>,
    next_local: &mut u32,
    fn_index_map: &HashMap<String, u32>,
    instrs: &mut Vec<Instruction<'static>>,
) {
    match stmt {
        Stmt::Assign(assign) => {
            if let Expr::Ident(name, _) = &assign.target {
                if let Some(local) = locals.get(name) {
                    let index = local.index;
                    compile_fn_expr(&assign.value, locals, fn_index_map, instrs);
                    instrs.push(Instruction::LocalSet(index));
                } else {
                    panic!("assignment to undefined variable `{name}` in fn codegen");
                }
            } else {
                panic!("invalid assignment target in fn");
            }
        }
        Stmt::Let(let_stmt) => {
            let ty = if let Some(ast_ty) = &let_stmt.ty {
                Ty::from_ast(ast_ty)
            } else {
                infer_fn_expr_ty(&let_stmt.value, locals)
            };
            let index = *next_local;
            *next_local += 1;
            locals.insert(let_stmt.name.clone(), LocalVar { index, ty });
            compile_fn_expr(&let_stmt.value, locals, fn_index_map, instrs);
            instrs.push(Instruction::LocalSet(index));
        }
        Stmt::If(if_stmt) => {
            compile_fn_expr(&if_stmt.condition, locals, fn_index_map, instrs);
            instrs.push(Instruction::If(wasm_encoder::BlockType::Empty));
            for s in &if_stmt.then_body {
                compile_fn_stmt(s, locals, next_local, fn_index_map, instrs);
            }
            if let Some(else_body) = &if_stmt.else_body {
                instrs.push(Instruction::Else);
                for s in else_body {
                    compile_fn_stmt(s, locals, next_local, fn_index_map, instrs);
                }
            }
            instrs.push(Instruction::End);
        }
        Stmt::While(while_stmt) => {
            instrs.push(Instruction::Block(wasm_encoder::BlockType::Empty));
            instrs.push(Instruction::Loop(wasm_encoder::BlockType::Empty));
            compile_fn_expr(&while_stmt.condition, locals, fn_index_map, instrs);
            instrs.push(Instruction::I32Eqz);
            instrs.push(Instruction::BrIf(1));
            for s in &while_stmt.body {
                compile_fn_stmt(s, locals, next_local, fn_index_map, instrs);
            }
            instrs.push(Instruction::Br(0));
            instrs.push(Instruction::End);
            instrs.push(Instruction::End);
        }
        Stmt::Return(Some(expr), _) => {
            compile_fn_expr(expr, locals, fn_index_map, instrs);
            instrs.push(Instruction::Return);
        }
        Stmt::Return(None, _) => {
            instrs.push(Instruction::Return);
        }
        Stmt::Expr(expr) => {
            compile_fn_expr(expr, locals, fn_index_map, instrs);
            instrs.push(Instruction::Drop);
        }
    }
}

fn compile_fn_expr(
    expr: &Expr,
    locals: &HashMap<String, LocalVar>,
    fn_index_map: &HashMap<String, u32>,
    instrs: &mut Vec<Instruction<'static>>,
) {
    match expr {
        Expr::IntLit(v, _) => instrs.push(Instruction::I32Const(*v as i32)),
        Expr::FloatLit(v, _) => instrs.push(Instruction::F32Const(*v as f32)),
        Expr::BoolLit(v, _) => instrs.push(Instruction::I32Const(if *v { 1 } else { 0 })),
        Expr::Ident(name, _) => {
            let local = locals.get(name).unwrap_or_else(|| panic!("undefined `{name}` in fn expr"));
            instrs.push(Instruction::LocalGet(local.index));
        }
        Expr::Call(name, args, _) => {
            for arg in args {
                compile_fn_expr(arg, locals, fn_index_map, instrs);
            }
            let idx = fn_index_map[name];
            instrs.push(Instruction::Call(idx));
        }
        Expr::BinOp(left, op, right, _) => {
            compile_fn_expr(left, locals, fn_index_map, instrs);
            compile_fn_expr(right, locals, fn_index_map, instrs);
            let is_float = is_float_fn_expr(left, locals);
            let instr = match op {
                BinOp::Add => pick(is_float, Instruction::F32Add, Instruction::I32Add),
                BinOp::Sub => pick(is_float, Instruction::F32Sub, Instruction::I32Sub),
                BinOp::Mul => pick(is_float, Instruction::F32Mul, Instruction::I32Mul),
                BinOp::Div => pick(is_float, Instruction::F32Div, Instruction::I32DivS),
                BinOp::Eq => pick(is_float, Instruction::F32Eq, Instruction::I32Eq),
                BinOp::NotEq => pick(is_float, Instruction::F32Ne, Instruction::I32Ne),
                BinOp::Lt => pick(is_float, Instruction::F32Lt, Instruction::I32LtS),
                BinOp::Gt => pick(is_float, Instruction::F32Gt, Instruction::I32GtS),
                BinOp::LtEq => pick(is_float, Instruction::F32Le, Instruction::I32LeS),
                BinOp::GtEq => pick(is_float, Instruction::F32Ge, Instruction::I32GeS),
                BinOp::And => Instruction::I32And,
                BinOp::Or => Instruction::I32Or,
            };
            instrs.push(instr);
        }
        Expr::UnaryOp(op, inner, _) => match op {
            UnaryOp::Neg => {
                if is_float_fn_expr(inner, locals) {
                    instrs.push(Instruction::F32Const(0.0));
                    compile_fn_expr(inner, locals, fn_index_map, instrs);
                    instrs.push(Instruction::F32Sub);
                } else {
                    instrs.push(Instruction::I32Const(0));
                    compile_fn_expr(inner, locals, fn_index_map, instrs);
                    instrs.push(Instruction::I32Sub);
                }
            }
            UnaryOp::Not => {
                compile_fn_expr(inner, locals, fn_index_map, instrs);
                instrs.push(Instruction::I32Eqz);
            }
        },
        Expr::FieldAccess(_, _, _) => {
            panic!("field access not supported in user functions");
        }
    }
}

fn is_float_fn_expr(expr: &Expr, locals: &HashMap<String, LocalVar>) -> bool {
    match expr {
        Expr::FloatLit(_, _) => true,
        Expr::IntLit(_, _) | Expr::BoolLit(_, _) => false,
        Expr::Ident(name, _) => locals.get(name).map_or(false, |l| l.ty.is_float()),
        Expr::Call(_, _, _) => true, // For now, assume calls return float. Sema validated types.
        Expr::BinOp(left, op, _, _) => match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => is_float_fn_expr(left, locals),
            _ => false,
        },
        Expr::UnaryOp(UnaryOp::Neg, inner, _) => is_float_fn_expr(inner, locals),
        Expr::UnaryOp(UnaryOp::Not, _, _) => false,
        Expr::FieldAccess(_, _, _) => false,
    }
}

fn infer_fn_expr_ty(expr: &Expr, locals: &HashMap<String, LocalVar>) -> Ty {
    match expr {
        Expr::FloatLit(_, _) => Ty::F32,
        Expr::IntLit(_, _) => Ty::I32,
        Expr::BoolLit(_, _) => Ty::Bool,
        Expr::Ident(name, _) => locals.get(name).map_or(Ty::I32, |l| l.ty.clone()),
        _ => Ty::I32,
    }
}

fn pick(is_float: bool, f: Instruction<'static>, i: Instruction<'static>) -> Instruction<'static> {
    if is_float { f } else { i }
}
