use std::collections::{HashMap, HashSet};
use vivi_parser::ast::*;
use vivi_sema::layout::{FieldLayout, MemoryLayout};
use vivi_sema::resolve::EachParamInfo;
use vivi_sema::types::Ty;
use wasm_encoder::Instruction;

/// A local variable in the WASM function.
#[derive(Clone)]
pub struct LocalVar {
    pub index: u32,
    pub ty: Ty,
}

/// A global variable stored at a fixed memory offset.
#[derive(Clone)]
pub struct GlobalVar {
    pub offset: u32,
    pub ty: Ty,
}

/// Context for compiling expressions within a system's `each` body.
pub struct ExprCtx<'a> {
    pub layout: &'a MemoryLayout,
    pub params: &'a [EachParamInfo],
    pub entity_index_local: u32,
    pub locals: HashMap<String, LocalVar>,
    pub next_local: u32,
    pub fn_index_map: &'a HashMap<String, u32>,
    pub void_fns: &'a HashSet<String>,
    pub globals: &'a HashMap<String, GlobalVar>,
    pub fn_return_types: &'a HashMap<String, Ty>,
}

impl<'a> ExprCtx<'a> {
    pub fn new(
        layout: &'a MemoryLayout,
        params: &'a [EachParamInfo],
        entity_index_local: u32,
        fn_index_map: &'a HashMap<String, u32>,
        void_fns: &'a HashSet<String>,
        globals: &'a HashMap<String, GlobalVar>,
        fn_return_types: &'a HashMap<String, Ty>,
    ) -> Self {
        Self {
            layout,
            params,
            entity_index_local,
            locals: HashMap::new(),
            next_local: entity_index_local + 1,
            fn_index_map,
            void_fns,
            globals,
            fn_return_types,
        }
    }

    /// Allocate a new local variable, returns its index.
    pub fn alloc_local(&mut self, name: String, ty: Ty) -> u32 {
        let index = self.next_local;
        self.next_local += 1;
        self.locals.insert(name, LocalVar { index, ty });
        index
    }

    /// Emit instructions to compute an expression, leaving result on the WASM stack.
    pub fn compile_expr(&self, expr: &Expr, instrs: &mut Vec<Instruction<'static>>) {
        match expr {
            Expr::IntLit(v, _) => {
                instrs.push(Instruction::I32Const(*v as i32));
            }
            Expr::FloatLit(v, _) => {
                instrs.push(Instruction::F32Const(*v as f32));
            }
            Expr::BoolLit(v, _) => {
                instrs.push(Instruction::I32Const(if *v { 1 } else { 0 }));
            }
            Expr::Ident(name, _) => {
                if let Some(local) = self.locals.get(name) {
                    instrs.push(Instruction::LocalGet(local.index));
                } else if let Some(gvar) = self.globals.get(name) {
                    // Load global from memory
                    instrs.push(Instruction::I32Const(gvar.offset as i32));
                    instrs.push(self.load_instr(&gvar.ty));
                } else if self.params.iter().any(|p| p.name == *name) {
                    instrs.push(Instruction::LocalGet(self.entity_index_local));
                } else {
                    panic!("unresolved variable `{name}` in codegen");
                }
            }
            Expr::FieldAccess(obj, field, _) => {
                self.compile_field_load(obj, field, instrs);
            }
            Expr::Call(name, args, _) => {
                if !self.try_compile_builtin(name, args, instrs) {
                    for arg in args {
                        self.compile_expr(arg, instrs);
                    }
                    let idx = self.fn_index_map[name];
                    instrs.push(Instruction::Call(idx));
                }
            }
            Expr::BinOp(left, op, right, _) => {
                self.compile_expr(left, instrs);
                self.compile_expr(right, instrs);
                let is_float = self.is_float_expr(left);
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
                    if self.is_float_expr(inner) {
                        instrs.push(Instruction::F32Const(0.0));
                        self.compile_expr(inner, instrs);
                        instrs.push(Instruction::F32Sub);
                    } else {
                        instrs.push(Instruction::I32Const(0));
                        self.compile_expr(inner, instrs);
                        instrs.push(Instruction::I32Sub);
                    }
                }
                UnaryOp::Not => {
                    self.compile_expr(inner, instrs);
                    instrs.push(Instruction::I32Eqz);
                }
            },
        }
    }

    fn compile_field_load(&self, obj: &Expr, field: &str, instrs: &mut Vec<Instruction<'static>>) {
        let fl = self.resolve_field(obj, field);
        self.compile_field_address(fl.offset, fl.element_size, instrs);
        instrs.push(self.load_instr(&fl.ty));
    }

    pub fn compile_field_store(
        &self,
        obj: &Expr,
        field: &str,
        value: &Expr,
        instrs: &mut Vec<Instruction<'static>>,
    ) {
        let fl = self.resolve_field(obj, field);
        self.compile_field_address(fl.offset, fl.element_size, instrs);
        self.compile_expr(value, instrs);
        instrs.push(self.store_instr(&fl.ty));
    }

    fn compile_field_address(
        &self,
        field_offset: u32,
        element_size: u32,
        instrs: &mut Vec<Instruction<'static>>,
    ) {
        instrs.push(Instruction::I32Const(field_offset as i32));
        instrs.push(Instruction::LocalGet(self.entity_index_local));
        instrs.push(Instruction::I32Const(element_size as i32));
        instrs.push(Instruction::I32Mul);
        instrs.push(Instruction::I32Add);
    }

    fn resolve_field(&self, obj: &Expr, field: &str) -> FieldLayout {
        if let Expr::Ident(param_name, _) = obj {
            let param = self.params.iter().find(|p| p.name == *param_name).unwrap();
            let comp_layout = self.layout.get_component(&param.component).unwrap();
            comp_layout
                .fields
                .iter()
                .find(|f| f.name == field)
                .unwrap()
                .clone()
        } else {
            panic!("field access on non-identifier not supported");
        }
    }

    fn load_instr(&self, ty: &Ty) -> Instruction<'static> {
        let mem = wasm_encoder::MemArg { offset: 0, align: if ty.byte_size() == 8 { 3 } else { 2 }, memory_index: 0 };
        match ty {
            Ty::F32 => Instruction::F32Load(mem),
            Ty::F64 => Instruction::F64Load(mem),
            Ty::I32 | Ty::Bool | Ty::Entity => Instruction::I32Load(mem),
            Ty::I64 => Instruction::I64Load(mem),
        }
    }

    pub fn store_instr(&self, ty: &Ty) -> Instruction<'static> {
        let mem = wasm_encoder::MemArg { offset: 0, align: if ty.byte_size() == 8 { 3 } else { 2 }, memory_index: 0 };
        match ty {
            Ty::F32 => Instruction::F32Store(mem),
            Ty::F64 => Instruction::F64Store(mem),
            Ty::I32 | Ty::Bool | Ty::Entity => Instruction::I32Store(mem),
            Ty::I64 => Instruction::I64Store(mem),
        }
    }

    /// Compile built-in memory intrinsics. Returns true if handled.
    fn try_compile_builtin(
        &self,
        name: &str,
        args: &[Expr],
        instrs: &mut Vec<Instruction<'static>>,
    ) -> bool {
        let mem4 = wasm_encoder::MemArg { offset: 0, align: 2, memory_index: 0 };
        match name {
            "mem_store_i32" if args.len() == 2 => {
                self.compile_expr(&args[0], instrs); // addr
                self.compile_expr(&args[1], instrs); // value
                instrs.push(Instruction::I32Store(mem4));
                true
            }
            "mem_store_f32" if args.len() == 2 => {
                self.compile_expr(&args[0], instrs);
                self.compile_expr(&args[1], instrs);
                instrs.push(Instruction::F32Store(mem4));
                true
            }
            "mem_load_i32" if args.len() == 1 => {
                self.compile_expr(&args[0], instrs);
                instrs.push(Instruction::I32Load(mem4));
                true
            }
            "mem_load_f32" if args.len() == 1 => {
                self.compile_expr(&args[0], instrs);
                instrs.push(Instruction::F32Load(mem4));
                true
            }
            _ => false,
        }
    }

    /// Simple type inference for codegen (sema already validated types).
    pub fn infer_expr_ty(&self, expr: &Expr) -> Ty {
        match expr {
            Expr::FloatLit(_, _) => Ty::F32,
            Expr::IntLit(_, _) => Ty::I32,
            Expr::BoolLit(_, _) => Ty::Bool,
            Expr::Ident(name, _) => {
                if let Some(l) = self.locals.get(name) {
                    l.ty.clone()
                } else if let Some(g) = self.globals.get(name) {
                    g.ty.clone()
                } else {
                    Ty::I32
                }
            }
            Expr::FieldAccess(obj, field, _) => {
                if let Expr::Ident(param_name, _) = obj.as_ref() {
                    if let Some(param) = self.params.iter().find(|p| p.name == *param_name) {
                        let comp = self.layout.get_component(&param.component).unwrap();
                        return comp.fields.iter().find(|f| f.name == *field).unwrap().ty.clone();
                    }
                }
                Ty::I32
            }
            Expr::BinOp(left, op, _, _) => match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => self.infer_expr_ty(left),
                _ => Ty::Bool,
            },
            Expr::Call(name, _, _) => {
                if name == "mem_load_f32" { return Ty::F32; }
                if name == "mem_load_i32" || name.starts_with("mem_store") { return Ty::I32; }
                self.fn_return_types.get(name).cloned().unwrap_or(Ty::I32)
            }
            Expr::UnaryOp(UnaryOp::Neg, inner, _) => self.infer_expr_ty(inner),
            Expr::UnaryOp(UnaryOp::Not, _, _) => Ty::Bool,
        }
    }

    /// Determine if an expression produces a float value.
    fn is_float_expr(&self, expr: &Expr) -> bool {
        self.infer_expr_ty(expr).is_float()
    }
}

fn pick(is_float: bool, float_instr: Instruction<'static>, int_instr: Instruction<'static>) -> Instruction<'static> {
    if is_float { float_instr } else { int_instr }
}
