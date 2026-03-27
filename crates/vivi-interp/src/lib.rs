use std::collections::HashMap;
use vivi_parser::ast::*;
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::{
    EachParamInfo, EntityInfo, FieldValue, FnSignature, ResolvedProgram, SystemInfo,
};
use vivi_sema::types::Ty;

#[derive(Debug, Clone)]
pub enum Value {
    I32(i32),
    F32(f32),
    F64(f64),
    Bool(bool),
}

impl Value {
    pub fn as_i32(&self) -> i32 {
        match self {
            Value::I32(v) => *v,
            Value::Bool(v) => if *v { 1 } else { 0 },
            _ => panic!("expected i32, got {:?}", self),
        }
    }

    pub fn as_f32(&self) -> f32 {
        match self {
            Value::F32(v) => *v,
            _ => panic!("expected f32, got {:?}", self),
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::I32(v) => *v != 0,
            Value::Bool(v) => *v,
            Value::F32(v) => *v != 0.0,
            Value::F64(v) => *v != 0.0,
        }
    }
}

pub type ExternHandler = Box<dyn Fn(Vec<Value>) -> Option<Value>>;

enum Flow {
    Continue,
    Return(Option<Value>),
    Despawn,
}

pub struct Interpreter {
    pub memory: Vec<u8>,
    layout: MemoryLayout,
    entities: Vec<EntityInfo>,
    world_init_systems: Vec<String>,
    world_systems: Vec<String>,
    system_infos: Vec<SystemInfo>,
    system_bodies: HashMap<String, Vec<Stmt>>,
    #[allow(dead_code)]
    fn_sigs: HashMap<String, FnSignature>,
    fn_bodies: HashMap<String, Vec<Stmt>>,
    fn_params: HashMap<String, Vec<FnParam>>,
    extern_handlers: HashMap<String, ExternHandler>,
    #[allow(dead_code)]
    component_fields: HashMap<String, Vec<(String, Ty)>>,
}

impl Interpreter {
    pub fn new(program: &Program, resolved: &ResolvedProgram) -> Self {
        let total_bytes = resolved.layout.total_bytes as usize;
        let memory = vec![0u8; total_bytes];

        let mut system_bodies = HashMap::new();
        let mut fn_bodies = HashMap::new();
        let mut fn_params = HashMap::new();

        for item in &program.items {
            match item {
                Item::System(sys) => {
                    if let Some(each) = &sys.each {
                        system_bodies.insert(sys.name.clone(), each.body.clone());
                    } else {
                        system_bodies.insert(sys.name.clone(), sys.body.clone());
                    }
                }
                Item::Fn(f) => {
                    fn_bodies.insert(f.name.clone(), f.body.clone());
                    fn_params.insert(f.name.clone(), f.params.clone());
                }
                _ => {}
            }
        }

        let mut fn_sigs = HashMap::new();
        for sig in &resolved.functions {
            fn_sigs.insert(sig.name.clone(), sig.clone());
        }
        for efn in &resolved.extern_fns {
            fn_sigs.insert(
                efn.name.clone(),
                FnSignature {
                    name: efn.name.clone(),
                    params: efn.params.clone(),
                    return_ty: efn.return_ty.clone(),
                },
            );
        }

        let mut component_fields = HashMap::new();
        for comp in &resolved.components {
            let fields: Vec<(String, Ty)> = comp
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect();
            component_fields.insert(comp.name.clone(), fields);
        }

        Self {
            memory,
            layout: resolved.layout.clone(),
            entities: resolved.entities.clone(),
            world_init_systems: resolved.world_init_systems.clone(),
            world_systems: resolved.world_systems.clone(),
            system_infos: resolved.systems.clone(),
            system_bodies,
            fn_sigs,
            fn_bodies,
            fn_params,
            extern_handlers: HashMap::new(),
            component_fields,
        }
    }

    pub fn register_extern(&mut self, name: &str, handler: ExternHandler) {
        self.extern_handlers.insert(name.to_string(), handler);
    }

    pub fn init(&mut self) {
        // Write entity count
        let count = self.entities.len() as i32;
        self.memory[0..4].copy_from_slice(&count.to_le_bytes());

        // Write entity template data
        for (idx, entity) in self.entities.iter().enumerate() {
            for ec in &entity.components {
                let comp_layout = self.layout.get_component(&ec.component).unwrap();
                for (fname, fval) in &ec.field_values {
                    let fl = comp_layout.fields.iter().find(|f| f.name == *fname).unwrap();
                    let addr = (fl.offset + (idx as u32) * fl.element_size) as usize;
                    match fval {
                        FieldValue::F32(v) => {
                            self.memory[addr..addr + 4].copy_from_slice(&v.to_le_bytes());
                        }
                        FieldValue::I32(v) => {
                            self.memory[addr..addr + 4].copy_from_slice(&v.to_le_bytes());
                        }
                        FieldValue::Bool(v) => {
                            let val: i32 = if *v { 1 } else { 0 };
                            self.memory[addr..addr + 4].copy_from_slice(&val.to_le_bytes());
                        }
                    }
                }
            }
        }

        // Run init systems
        let init_systems: Vec<String> = self.world_init_systems.clone();
        for sys_name in &init_systems {
            self.run_system(sys_name);
        }
    }

    pub fn tick(&mut self) {
        let systems: Vec<String> = self.world_systems.clone();
        for sys_name in &systems {
            self.run_system(sys_name);
        }
    }

    fn run_system(&mut self, name: &str) {
        let sys_info = self.system_infos.iter().find(|s| s.name == *name).unwrap().clone();
        let body = self.system_bodies[name].clone();

        if sys_info.each_params.is_empty() && sys_info.query.is_empty() {
            // Bare system: run once, no entity loop
            let mut locals: HashMap<String, Value> = HashMap::new();
            self.exec_stmts(&body, &mut locals, None, 0);
        } else {
            // System with query/each: iterate entities
            let mut entity_idx: i32 = 0;
            loop {
                let entity_count =
                    i32::from_le_bytes(self.memory[0..4].try_into().unwrap());
                if entity_idx >= entity_count {
                    break;
                }
                let mut locals: HashMap<String, Value> = HashMap::new();
                let flow = self.exec_stmts(
                    &body,
                    &mut locals,
                    Some(&sys_info.each_params),
                    entity_idx as u32,
                );
                match flow {
                    Flow::Return(_) => break,
                    Flow::Despawn => {
                        // Don't increment: the swapped-in entity is now at entity_idx
                    }
                    Flow::Continue => {
                        entity_idx += 1;
                    }
                }
            }
        }
    }

    fn exec_stmts(
        &mut self,
        stmts: &[Stmt],
        locals: &mut HashMap<String, Value>,
        each_params: Option<&[EachParamInfo]>,
        entity_idx: u32,
    ) -> Flow {
        for stmt in stmts {
            match self.exec_stmt(stmt, locals, each_params, entity_idx) {
                Flow::Continue => {}
                flow @ Flow::Return(_) => return flow,
                Flow::Despawn => return Flow::Despawn,
            }
        }
        Flow::Continue
    }

    fn exec_stmt(
        &mut self,
        stmt: &Stmt,
        locals: &mut HashMap<String, Value>,
        each_params: Option<&[EachParamInfo]>,
        entity_idx: u32,
    ) -> Flow {
        match stmt {
            Stmt::Assign(assign) => {
                let value = self.eval_expr(&assign.value, locals, each_params, entity_idx);
                match &assign.target {
                    Expr::FieldAccess(obj, field, _) => {
                        if let Expr::Ident(param_name, _) = obj.as_ref() {
                            self.write_field(param_name, field, each_params.unwrap(), entity_idx, &value);
                        }
                    }
                    Expr::Ident(name, _) => {
                        locals.insert(name.clone(), value);
                    }
                    _ => panic!("invalid assignment target"),
                }
                Flow::Continue
            }
            Stmt::Let(let_stmt) => {
                let value = self.eval_expr(&let_stmt.value, locals, each_params, entity_idx);
                locals.insert(let_stmt.name.clone(), value);
                Flow::Continue
            }
            Stmt::If(if_stmt) => {
                let cond = self.eval_expr(&if_stmt.condition, locals, each_params, entity_idx);
                if cond.is_truthy() {
                    self.exec_stmts(&if_stmt.then_body, locals, each_params, entity_idx)
                } else if let Some(else_body) = &if_stmt.else_body {
                    self.exec_stmts(else_body, locals, each_params, entity_idx)
                } else {
                    Flow::Continue
                }
            }
            Stmt::While(while_stmt) => {
                loop {
                    let cond = self.eval_expr(&while_stmt.condition, locals, each_params, entity_idx);
                    if !cond.is_truthy() {
                        break;
                    }
                    match self.exec_stmts(&while_stmt.body, locals, each_params, entity_idx) {
                        Flow::Continue => {}
                        flow @ Flow::Return(_) => return flow,
                        Flow::Despawn => return Flow::Despawn,
                    }
                }
                Flow::Continue
            }
            Stmt::Return(Some(expr), _) => {
                let value = self.eval_expr(expr, locals, each_params, entity_idx);
                Flow::Return(Some(value))
            }
            Stmt::Return(None, _) => Flow::Return(None),
            Stmt::Spawn(spawn) => {
                self.exec_spawn(spawn, locals, each_params, entity_idx);
                Flow::Continue
            }
            Stmt::Despawn(_) => {
                self.exec_despawn(entity_idx);
                Flow::Despawn
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr, locals, each_params, entity_idx);
                Flow::Continue
            }
        }
    }

    fn eval_expr(
        &mut self,
        expr: &Expr,
        locals: &HashMap<String, Value>,
        each_params: Option<&[EachParamInfo]>,
        entity_idx: u32,
    ) -> Value {
        match expr {
            Expr::IntLit(v, _) => Value::I32(*v as i32),
            Expr::FloatLit(v, _) => Value::F32(*v as f32),
            Expr::BoolLit(v, _) => Value::Bool(*v),
            Expr::Ident(name, _) => {
                if let Some(val) = locals.get(name) {
                    val.clone()
                } else {
                    Value::I32(entity_idx as i32)
                }
            }
            Expr::FieldAccess(obj, field, _) => {
                if let Expr::Ident(param_name, _) = obj.as_ref() {
                    self.read_field(param_name, field, each_params.unwrap(), entity_idx)
                } else {
                    panic!("field access on non-ident");
                }
            }
            Expr::Call(name, args, _) => {
                let arg_values: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval_expr(a, locals, each_params, entity_idx))
                    .collect();
                self.call_fn(name, arg_values)
            }
            Expr::BinOp(left, op, right, _) => {
                let lv = self.eval_expr(left, locals, each_params, entity_idx);
                let rv = self.eval_expr(right, locals, each_params, entity_idx);
                self.eval_binop(&lv, op, &rv)
            }
            Expr::UnaryOp(op, inner, _) => {
                let v = self.eval_expr(inner, locals, each_params, entity_idx);
                match op {
                    UnaryOp::Neg => match v {
                        Value::I32(n) => Value::I32(-n),
                        Value::F32(n) => Value::F32(-n),
                        Value::F64(n) => Value::F64(-n),
                        _ => panic!("cannot negate {:?}", v),
                    },
                    UnaryOp::Not => Value::Bool(!v.is_truthy()),
                }
            }
        }
    }

    fn exec_spawn(
        &mut self,
        spawn: &vivi_parser::ast::SpawnStmt,
        locals: &HashMap<String, Value>,
        each_params: Option<&[vivi_sema::resolve::EachParamInfo]>,
        entity_idx: u32,
    ) {
        let idx = i32::from_le_bytes(self.memory[0..4].try_into().unwrap()) as u32;

        // Pre-collect layout info to avoid borrow conflicts
        let field_addrs: Vec<(usize, usize, vivi_sema::types::Ty)> = spawn
            .components
            .iter()
            .flat_map(|sc| {
                let comp_layout = self.layout.get_component(&sc.component).unwrap();
                sc.fields.iter().enumerate().map(move |(i, (fname, _))| {
                    let fl = comp_layout.fields.iter().find(|f| f.name == *fname).unwrap();
                    let addr = (fl.offset + idx * fl.element_size) as usize;
                    (i, addr, fl.ty.clone())
                })
            })
            .collect();

        // Evaluate and store
        let all_exprs: Vec<&vivi_parser::ast::Expr> = spawn
            .components
            .iter()
            .flat_map(|sc| sc.fields.iter().map(|(_, e)| e))
            .collect();

        for (i, (_, addr, _ty)) in field_addrs.iter().enumerate() {
            let value = self.eval_expr(all_exprs[i], locals, each_params, entity_idx);
            match value {
                Value::F32(v) => self.memory[*addr..*addr + 4].copy_from_slice(&v.to_le_bytes()),
                Value::I32(v) => self.memory[*addr..*addr + 4].copy_from_slice(&v.to_le_bytes()),
                Value::Bool(v) => {
                    let val: i32 = if v { 1 } else { 0 };
                    self.memory[*addr..*addr + 4].copy_from_slice(&val.to_le_bytes());
                }
                Value::F64(v) => self.memory[*addr..*addr + 8].copy_from_slice(&v.to_le_bytes()),
            }
        }

        let new_count = (idx + 1) as i32;
        self.memory[0..4].copy_from_slice(&new_count.to_le_bytes());
    }

    fn exec_despawn(&mut self, entity_idx: u32) {
        let entity_count = i32::from_le_bytes(self.memory[0..4].try_into().unwrap()) as u32;
        let last = entity_count - 1;

        // For each component, for each field: copy last entity's data to current position
        for comp_layout in &self.layout.components.clone() {
            for fl in &comp_layout.fields {
                let src_addr = (fl.offset + last * fl.element_size) as usize;
                let dst_addr = (fl.offset + entity_idx * fl.element_size) as usize;
                let size = fl.element_size as usize;
                // Copy byte by byte to avoid borrow issues with overlapping slices
                if src_addr != dst_addr {
                    for b in 0..size {
                        self.memory[dst_addr + b] = self.memory[src_addr + b];
                    }
                }
            }
        }

        // Decrement entity_count
        let new_count = (entity_count - 1) as i32;
        self.memory[0..4].copy_from_slice(&new_count.to_le_bytes());
    }

    fn call_fn(&mut self, name: &str, args: Vec<Value>) -> Value {
        // Check extern handlers first
        if let Some(handler) = self.extern_handlers.get(name) {
            return handler(args).unwrap_or(Value::I32(0));
        }

        // User function
        let body = self.fn_bodies.get(name).cloned();
        if let Some(body) = body {
            let params = self.fn_params[name].clone();
            let mut locals: HashMap<String, Value> = HashMap::new();
            for (i, param) in params.iter().enumerate() {
                locals.insert(param.name.clone(), args[i].clone());
            }
            match self.exec_stmts(&body, &mut locals, None, 0) {
                Flow::Return(Some(val)) => val,
                Flow::Return(None) | Flow::Continue | Flow::Despawn => Value::I32(0),
            }
        } else {
            // Unregistered extern — return default
            Value::I32(0)
        }
    }

    fn eval_binop(&self, lv: &Value, op: &BinOp, rv: &Value) -> Value {
        match (lv, rv) {
            (Value::F32(a), Value::F32(b)) => match op {
                BinOp::Add => Value::F32(a + b),
                BinOp::Sub => Value::F32(a - b),
                BinOp::Mul => Value::F32(a * b),
                BinOp::Div => Value::F32(a / b),
                BinOp::Eq => Value::Bool((a - b).abs() < f32::EPSILON),
                BinOp::NotEq => Value::Bool((a - b).abs() >= f32::EPSILON),
                BinOp::Lt => Value::Bool(a < b),
                BinOp::Gt => Value::Bool(a > b),
                BinOp::LtEq => Value::Bool(a <= b),
                BinOp::GtEq => Value::Bool(a >= b),
                BinOp::And => Value::Bool(*a != 0.0 && *b != 0.0),
                BinOp::Or => Value::Bool(*a != 0.0 || *b != 0.0),
            },
            (Value::I32(a), Value::I32(b)) => match op {
                BinOp::Add => Value::I32(a + b),
                BinOp::Sub => Value::I32(a - b),
                BinOp::Mul => Value::I32(a * b),
                BinOp::Div => Value::I32(a / b),
                BinOp::Eq => Value::Bool(a == b),
                BinOp::NotEq => Value::Bool(a != b),
                BinOp::Lt => Value::Bool(a < b),
                BinOp::Gt => Value::Bool(a > b),
                BinOp::LtEq => Value::Bool(a <= b),
                BinOp::GtEq => Value::Bool(a >= b),
                BinOp::And => Value::Bool(*a != 0 && *b != 0),
                BinOp::Or => Value::Bool(*a != 0 || *b != 0),
            },
            // Mixed: compare i32 field with f32 literal (e.g. while i < cnt.steps)
            (Value::I32(a), Value::F32(b)) => {
                self.eval_binop(&Value::F32(*a as f32), op, &Value::F32(*b))
            }
            (Value::F32(a), Value::I32(b)) => {
                self.eval_binop(&Value::F32(*a), op, &Value::F32(*b as f32))
            }
            _ => panic!("unsupported binop on {:?} and {:?}", lv, rv),
        }
    }

    fn read_field(
        &self,
        param_name: &str,
        field: &str,
        each_params: &[EachParamInfo],
        entity_idx: u32,
    ) -> Value {
        let param = each_params.iter().find(|p| p.name == param_name).unwrap();
        let comp_layout = self.layout.get_component(&param.component).unwrap();
        let fl = comp_layout.fields.iter().find(|f| f.name == field).unwrap();
        let addr = (fl.offset + entity_idx * fl.element_size) as usize;

        match &fl.ty {
            Ty::F32 => {
                let bytes: [u8; 4] = self.memory[addr..addr + 4].try_into().unwrap();
                Value::F32(f32::from_le_bytes(bytes))
            }
            Ty::I32 | Ty::Bool | Ty::Entity => {
                let bytes: [u8; 4] = self.memory[addr..addr + 4].try_into().unwrap();
                Value::I32(i32::from_le_bytes(bytes))
            }
            Ty::F64 => {
                let bytes: [u8; 8] = self.memory[addr..addr + 8].try_into().unwrap();
                Value::F64(f64::from_le_bytes(bytes))
            }
            Ty::I64 => {
                let bytes: [u8; 8] = self.memory[addr..addr + 8].try_into().unwrap();
                Value::I32(i64::from_le_bytes(bytes) as i32)
            }
        }
    }

    fn write_field(
        &mut self,
        param_name: &str,
        field: &str,
        each_params: &[EachParamInfo],
        entity_idx: u32,
        value: &Value,
    ) {
        let param = each_params.iter().find(|p| p.name == param_name).unwrap();
        let comp_layout = self.layout.get_component(&param.component).unwrap();
        let fl = comp_layout.fields.iter().find(|f| f.name == field).unwrap();
        let addr = (fl.offset + entity_idx * fl.element_size) as usize;

        match value {
            Value::F32(v) => self.memory[addr..addr + 4].copy_from_slice(&v.to_le_bytes()),
            Value::I32(v) => self.memory[addr..addr + 4].copy_from_slice(&v.to_le_bytes()),
            Value::Bool(v) => {
                let val: i32 = if *v { 1 } else { 0 };
                self.memory[addr..addr + 4].copy_from_slice(&val.to_le_bytes());
            }
            Value::F64(v) => self.memory[addr..addr + 8].copy_from_slice(&v.to_le_bytes()),
        }
    }

    /// Dump all entity state to a formatted string.
    pub fn dump_state(&self) -> String {
        let entity_count = i32::from_le_bytes(self.memory[0..4].try_into().unwrap());
        let mut out = format!("Entity count: {entity_count}\n");

        for idx in 0..entity_count as u32 {
            out.push_str(&format!("\n  Entity {idx}:\n"));
            for comp_layout in &self.layout.components {
                out.push_str(&format!("    {}:", comp_layout.name));
                for fl in &comp_layout.fields {
                    let addr = (fl.offset + idx * fl.element_size) as usize;
                    let val_str = match &fl.ty {
                        Ty::F32 => {
                            let v = f32::from_le_bytes(
                                self.memory[addr..addr + 4].try_into().unwrap(),
                            );
                            format!("{v:.2}")
                        }
                        Ty::I32 | Ty::Bool | Ty::Entity => {
                            let v = i32::from_le_bytes(
                                self.memory[addr..addr + 4].try_into().unwrap(),
                            );
                            format!("{v}")
                        }
                        Ty::F64 => {
                            let v = f64::from_le_bytes(
                                self.memory[addr..addr + 8].try_into().unwrap(),
                            );
                            format!("{v:.2}")
                        }
                        Ty::I64 => {
                            let v = i64::from_le_bytes(
                                self.memory[addr..addr + 8].try_into().unwrap(),
                            );
                            format!("{v}")
                        }
                    };
                    out.push_str(&format!(" {}={val_str}", fl.name));
                }
                out.push('\n');
            }
        }
        out
    }
}
