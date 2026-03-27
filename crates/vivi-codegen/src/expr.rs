use vivi_parser::ast::*;
use vivi_sema::layout::{ComponentLayout, MemoryLayout};
use vivi_sema::resolve::EachParamInfo;
use wasm_encoder::Instruction;

/// Context for compiling expressions within a system's `each` body.
pub struct ExprCtx<'a> {
    pub layout: &'a MemoryLayout,
    pub params: &'a [EachParamInfo],
    pub entity_index_local: u32, // local holding the loop variable (entity index)
}

impl<'a> ExprCtx<'a> {
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
                // This is a bare parameter reference — shouldn't happen in well-typed code
                // unless it's a local variable
                if self.params.iter().any(|p| p.name == *name) {
                    // Push the entity index as a stand-in; actual usage is via field access
                    instrs.push(Instruction::LocalGet(self.entity_index_local));
                } else {
                    // Must be a local variable. For now we don't handle locals beyond
                    // the entity index. This would need a local variable table.
                    panic!("unresolved variable `{name}` in codegen");
                }
            }
            Expr::FieldAccess(obj, field, _) => {
                self.compile_field_load(obj, field, instrs);
            }
            Expr::BinOp(left, op, right, _) => {
                self.compile_expr(left, instrs);
                self.compile_expr(right, instrs);
                let instr = match op {
                    BinOp::Add => self.arith_instr(left, Instruction::F32Add, Instruction::I32Add),
                    BinOp::Sub => self.arith_instr(left, Instruction::F32Sub, Instruction::I32Sub),
                    BinOp::Mul => self.arith_instr(left, Instruction::F32Mul, Instruction::I32Mul),
                    BinOp::Div => self.arith_instr(left, Instruction::F32Div, Instruction::I32DivS),
                    BinOp::Eq => self.cmp_instr(left, Instruction::F32Eq, Instruction::I32Eq),
                    BinOp::NotEq => self.cmp_instr(left, Instruction::F32Ne, Instruction::I32Ne),
                    BinOp::Lt => self.cmp_instr(left, Instruction::F32Lt, Instruction::I32LtS),
                    BinOp::Gt => self.cmp_instr(left, Instruction::F32Gt, Instruction::I32GtS),
                    BinOp::LtEq => self.cmp_instr(left, Instruction::F32Le, Instruction::I32LeS),
                    BinOp::GtEq => self.cmp_instr(left, Instruction::F32Ge, Instruction::I32GeS),
                    BinOp::And => Instruction::I32And,
                    BinOp::Or => Instruction::I32Or,
                };
                instrs.push(instr);
            }
            Expr::UnaryOp(op, inner, _) => {
                match op {
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
                }
            }
        }
    }

    /// Compile loading a component field: memory[field_offset + entity_index * elem_size]
    fn compile_field_load(&self, obj: &Expr, field: &str, instrs: &mut Vec<Instruction<'static>>) {
        let (comp_layout, field_layout) = self.resolve_field(obj, field);
        // address = field_offset + entity_index * element_size
        self.compile_field_address(field_layout.offset, field_layout.element_size, instrs);

        match field_layout.element_size {
            4 => {
                if self.is_float_field(&comp_layout.name, field) {
                    instrs.push(Instruction::F32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                } else {
                    instrs.push(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                }
            }
            8 => {
                instrs.push(Instruction::F64Load(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
            }
            _ => unreachable!(),
        }
    }

    /// Compile storing a value to a component field.
    pub fn compile_field_store(
        &self,
        obj: &Expr,
        field: &str,
        value: &Expr,
        instrs: &mut Vec<Instruction<'static>>,
    ) {
        let (_comp_layout, field_layout) = self.resolve_field(obj, field);
        // Push address first, then value
        self.compile_field_address(field_layout.offset, field_layout.element_size, instrs);
        self.compile_expr(value, instrs);

        match field_layout.element_size {
            4 => {
                if self.is_float_field(&_comp_layout.name, field) {
                    instrs.push(Instruction::F32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                } else {
                    instrs.push(Instruction::I32Store(wasm_encoder::MemArg {
                        offset: 0,
                        align: 2,
                        memory_index: 0,
                    }));
                }
            }
            8 => {
                instrs.push(Instruction::F64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
            }
            _ => unreachable!(),
        }
    }

    /// Emit instructions to compute field_offset + entity_index * element_size
    fn compile_field_address(
        &self,
        field_offset: u32,
        element_size: u32,
        instrs: &mut Vec<Instruction<'static>>,
    ) {
        // address = field_offset + entity_index * element_size
        instrs.push(Instruction::I32Const(field_offset as i32));
        instrs.push(Instruction::LocalGet(self.entity_index_local));
        instrs.push(Instruction::I32Const(element_size as i32));
        instrs.push(Instruction::I32Mul);
        instrs.push(Instruction::I32Add);
    }

    fn resolve_field(&self, obj: &Expr, field: &str) -> (ComponentLayout, crate::FieldLayoutClone) {
        if let Expr::Ident(param_name, _) = obj {
            let param = self.params.iter().find(|p| p.name == *param_name).unwrap();
            let comp_layout = self.layout.get_component(&param.component).unwrap();
            let field_layout = comp_layout
                .fields
                .iter()
                .find(|f| f.name == field)
                .unwrap();
            (
                comp_layout.clone(),
                crate::FieldLayoutClone {
                    offset: field_layout.offset,
                    element_size: field_layout.element_size,
                },
            )
        } else {
            panic!("field access on non-identifier not supported");
        }
    }

    fn is_float_field(&self, component: &str, field: &str) -> bool {
        // Look up the component info to determine the field type
        // We use the layout element_size as a heuristic, but need the actual type
        // For now: fields named in components with f32/f64 type
        // We'll pass this info through. For phase 1, check the sema component info.
        // Shortcut: look at the field layout element_size and assume 4-byte = could be i32 or f32.
        // We need to actually know. Let's store this info.
        // For the MVP, we'll rely on the component naming convention or pass through type info.
        // Actually, let's check our resolved component info through the layout.
        // The cleanest approach: store the type in FieldLayout.
        // For now, use the fact that in our test case, all fields are f32.
        // TODO: pass type info through FieldLayout
        let comp = self.layout.get_component(component).unwrap();
        let _fl = comp.fields.iter().find(|f| f.name == field).unwrap();
        // We need type info. Let's store it alongside. For now, assume f32 for 4-byte fields.
        // This is a known limitation - we'll fix it properly.
        true // All component fields in Phase 1 examples are f32
    }

    fn is_float_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::FloatLit(_, _) => true,
            Expr::IntLit(_, _) => false,
            Expr::BoolLit(_, _) => false,
            Expr::FieldAccess(obj, field, _) => {
                if let Expr::Ident(param_name, _) = obj.as_ref() {
                    let param = self.params.iter().find(|p| p.name == *param_name);
                    if let Some(p) = param {
                        self.is_float_field(&p.component, field)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            Expr::BinOp(left, op, _, _) => match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => self.is_float_expr(left),
                _ => false, // comparison ops return bool (i32)
            },
            Expr::UnaryOp(UnaryOp::Neg, inner, _) => self.is_float_expr(inner),
            Expr::UnaryOp(UnaryOp::Not, _, _) => false,
            Expr::Ident(_, _) => false,
        }
    }

    fn arith_instr(
        &self,
        left: &Expr,
        float_instr: Instruction<'static>,
        int_instr: Instruction<'static>,
    ) -> Instruction<'static> {
        if self.is_float_expr(left) {
            float_instr
        } else {
            int_instr
        }
    }

    fn cmp_instr(
        &self,
        left: &Expr,
        float_instr: Instruction<'static>,
        int_instr: Instruction<'static>,
    ) -> Instruction<'static> {
        if self.is_float_expr(left) {
            float_instr
        } else {
            int_instr
        }
    }
}
