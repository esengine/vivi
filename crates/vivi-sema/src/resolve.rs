use std::collections::HashMap;
use vivi_parser::ast::*;

use crate::layout::MemoryLayout;
use crate::types::Ty;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("{message}")]
pub struct SemaError {
    pub message: String,
    #[label("{label}")]
    pub span: std::ops::Range<usize>,
    pub label: String,
    #[source_code]
    pub source_code: String,
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub ty: Ty,
}

#[derive(Debug, Clone)]
pub struct ComponentInfo {
    pub name: String,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub name: String,
    pub query: Vec<QueryEntryInfo>,
    pub each_params: Vec<EachParamInfo>,
}

#[derive(Debug, Clone)]
pub struct QueryEntryInfo {
    pub access: Access,
    pub component: String,
}

#[derive(Debug, Clone)]
pub struct EachParamInfo {
    pub name: String,
    pub component: String,
}

#[derive(Debug, Clone)]
pub struct FnSignature {
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub return_ty: Option<Ty>,
}

#[derive(Debug, Clone)]
pub struct ExternFnInfo {
    pub module_name: String,
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub return_ty: Option<Ty>,
}

#[derive(Debug, Clone)]
pub struct EntityInfo {
    pub name: String,
    pub components: Vec<EntityComponentInfo>,
}

#[derive(Debug, Clone)]
pub struct EntityComponentInfo {
    pub component: String,
    pub field_values: Vec<(String, FieldValue)>,
}

#[derive(Debug, Clone)]
pub enum FieldValue {
    F32(f32),
    I32(i32),
    Bool(bool),
}

#[derive(Debug)]
pub struct ResolvedProgram {
    pub components: Vec<ComponentInfo>,
    pub systems: Vec<SystemInfo>,
    pub functions: Vec<FnSignature>,
    pub extern_fns: Vec<ExternFnInfo>,
    pub entities: Vec<EntityInfo>,
    pub world_systems: Vec<String>,
    pub layout: MemoryLayout,
}

pub fn resolve(program: &Program, source: &str) -> Result<ResolvedProgram, SemaError> {
    let mut components: HashMap<String, ComponentInfo> = HashMap::new();
    let mut functions: Vec<FnSignature> = Vec::new();
    let mut extern_fns: Vec<ExternFnInfo> = Vec::new();
    let mut entities: Vec<EntityInfo> = Vec::new();
    let mut fn_map: HashMap<String, FnSignature> = HashMap::new();
    let mut systems: Vec<SystemInfo> = Vec::new();
    let mut world_systems: Vec<String> = Vec::new();
    let mut component_order: Vec<String> = Vec::new();

    // First pass: collect components
    for item in &program.items {
        if let Item::Component(comp) = item {
            if components.contains_key(&comp.name) {
                return Err(SemaError {
                    message: format!("duplicate component `{}`", comp.name),
                    span: comp.span.clone(),
                    label: "defined here".into(),
                    source_code: source.to_string(),
                });
            }
            let fields: Vec<FieldInfo> = comp
                .fields
                .iter()
                .map(|f| FieldInfo {
                    name: f.name.clone(),
                    ty: Ty::from_ast(&f.ty),
                })
                .collect();
            component_order.push(comp.name.clone());
            components.insert(
                comp.name.clone(),
                ComponentInfo {
                    name: comp.name.clone(),
                    fields,
                },
            );
        }
    }

    // Collect extern functions
    for item in &program.items {
        if let Item::Extern(ext) = item {
            for efn in &ext.functions {
                let params: Vec<(String, Ty)> = efn
                    .params
                    .iter()
                    .map(|p| (p.name.clone(), Ty::from_ast(&p.ty)))
                    .collect();
                let return_ty = efn.return_ty.as_ref().map(Ty::from_ast);
                let sig = FnSignature {
                    name: efn.name.clone(),
                    params: params.clone(),
                    return_ty: return_ty.clone(),
                };
                fn_map.insert(efn.name.clone(), sig);
                extern_fns.push(ExternFnInfo {
                    module_name: ext.module_name.clone(),
                    name: efn.name.clone(),
                    params,
                    return_ty,
                });
            }
        }
    }

    // Second pass: collect function signatures
    for item in &program.items {
        if let Item::Fn(fn_def) = item {
            if fn_map.contains_key(&fn_def.name) {
                return Err(SemaError {
                    message: format!("duplicate function `{}`", fn_def.name),
                    span: fn_def.span.clone(),
                    label: "defined here".into(),
                    source_code: source.to_string(),
                });
            }
            let params: Vec<(String, Ty)> = fn_def
                .params
                .iter()
                .map(|p| (p.name.clone(), Ty::from_ast(&p.ty)))
                .collect();
            let return_ty = fn_def.return_ty.as_ref().map(Ty::from_ast);
            let sig = FnSignature {
                name: fn_def.name.clone(),
                params,
                return_ty,
            };
            fn_map.insert(fn_def.name.clone(), sig.clone());
            functions.push(sig);
        }
    }

    // Type check function bodies
    for item in &program.items {
        if let Item::Fn(fn_def) = item {
            let sig = &fn_map[&fn_def.name];
            let mut locals: HashMap<String, Ty> = HashMap::new();
            for (name, ty) in &sig.params {
                locals.insert(name.clone(), ty.clone());
            }
            type_check_fn_body(
                &fn_def.body,
                &mut locals,
                sig.return_ty.as_ref(),
                &fn_map,
                source,
            )?;
        }
    }

    // Third pass: resolve systems
    for item in &program.items {
        if let Item::System(sys) = item {
            if let (Some(query), Some(each)) = (&sys.query, &sys.each) {
                // System with query + each
                let mut query_entries = Vec::new();
                for entry in &query.entries {
                    if !components.contains_key(&entry.component) {
                        return Err(SemaError {
                            message: format!("unknown component `{}`", entry.component),
                            span: entry.span.clone(),
                            label: "not defined".into(),
                            source_code: source.to_string(),
                        });
                    }
                    query_entries.push(QueryEntryInfo {
                        access: entry.access.clone(),
                        component: entry.component.clone(),
                    });
                }

                let mut each_params = Vec::new();
                for param in &each.params {
                    let in_query = query_entries.iter().any(|q| q.component == param.component);
                    if !in_query {
                        return Err(SemaError {
                            message: format!(
                                "each parameter `{}` references component `{}` not in query",
                                param.name, param.component
                            ),
                            span: param.span.clone(),
                            label: "not in query".into(),
                            source_code: source.to_string(),
                        });
                    }
                    each_params.push(EachParamInfo {
                        name: param.name.clone(),
                        component: param.component.clone(),
                    });
                }

                let mut locals = HashMap::new();
                type_check_body(
                    &each.body,
                    &each_params,
                    &components,
                    &mut locals,
                    &fn_map,
                    source,
                )?;

                systems.push(SystemInfo {
                    name: sys.name.clone(),
                    query: query_entries,
                    each_params,
                });
            } else {
                // Bare system: no query/each, just statements
                let mut locals = HashMap::new();
                type_check_fn_body(
                    &sys.body,
                    &mut locals,
                    None,
                    &fn_map,
                    source,
                )?;

                systems.push(SystemInfo {
                    name: sys.name.clone(),
                    query: vec![],
                    each_params: vec![],
                });
            }
        }
    }

    // Collect entity templates
    for item in &program.items {
        if let Item::Entity(entity) = item {
            let mut comp_infos = Vec::new();
            for ec in &entity.components {
                let comp = components.get(&ec.component).ok_or_else(|| SemaError {
                    message: format!("unknown component `{}` in entity `{}`", ec.component, entity.name),
                    span: ec.span.clone(),
                    label: "not defined".into(),
                    source_code: source.to_string(),
                })?;
                let mut field_values = Vec::new();
                for (fname, fexpr) in &ec.fields {
                    let fi = comp.fields.iter().find(|f| f.name == *fname).ok_or_else(|| SemaError {
                        message: format!("component `{}` has no field `{fname}`", ec.component),
                        span: ec.span.clone(),
                        label: "no such field".into(),
                        source_code: source.to_string(),
                    })?;
                    let val = match fexpr {
                        vivi_parser::ast::Expr::FloatLit(v, _) => {
                            if fi.ty != Ty::F32 {
                                return Err(SemaError {
                                    message: format!("field `{fname}` is `{}`, got float literal", fi.ty),
                                    span: ec.span.clone(),
                                    label: "type mismatch".into(),
                                    source_code: source.to_string(),
                                });
                            }
                            FieldValue::F32(*v as f32)
                        }
                        vivi_parser::ast::Expr::IntLit(v, _) => {
                            if fi.ty != Ty::I32 {
                                return Err(SemaError {
                                    message: format!("field `{fname}` is `{}`, got int literal", fi.ty),
                                    span: ec.span.clone(),
                                    label: "type mismatch".into(),
                                    source_code: source.to_string(),
                                });
                            }
                            FieldValue::I32(*v as i32)
                        }
                        vivi_parser::ast::Expr::BoolLit(v, _) => FieldValue::Bool(*v),
                        // Handle negative literals: -1.5, -42
                        vivi_parser::ast::Expr::UnaryOp(
                            vivi_parser::ast::UnaryOp::Neg,
                            inner,
                            _,
                        ) => match inner.as_ref() {
                            vivi_parser::ast::Expr::FloatLit(v, _) => {
                                if fi.ty != Ty::F32 {
                                    return Err(SemaError {
                                        message: format!("field `{fname}` is `{}`, got float literal", fi.ty),
                                        span: ec.span.clone(),
                                        label: "type mismatch".into(),
                                        source_code: source.to_string(),
                                    });
                                }
                                FieldValue::F32(-(*v as f32))
                            }
                            vivi_parser::ast::Expr::IntLit(v, _) => {
                                if fi.ty != Ty::I32 {
                                    return Err(SemaError {
                                        message: format!("field `{fname}` is `{}`, got int literal", fi.ty),
                                        span: ec.span.clone(),
                                        label: "type mismatch".into(),
                                        source_code: source.to_string(),
                                    });
                                }
                                FieldValue::I32(-(*v as i32))
                            }
                            _ => {
                                return Err(SemaError {
                                    message: "entity field values must be literals".into(),
                                    span: ec.span.clone(),
                                    label: "expected literal".into(),
                                    source_code: source.to_string(),
                                });
                            }
                        },
                        _ => {
                            return Err(SemaError {
                                message: "entity field values must be literals".into(),
                                span: ec.span.clone(),
                                label: "expected literal".into(),
                                source_code: source.to_string(),
                            });
                        }
                    };
                    field_values.push((fname.clone(), val));
                }
                comp_infos.push(EntityComponentInfo {
                    component: ec.component.clone(),
                    field_values,
                });
            }
            entities.push(EntityInfo {
                name: entity.name.clone(),
                components: comp_infos,
            });
        }
    }

    // Third pass: world
    for item in &program.items {
        if let Item::World(world) = item {
            for sys_name in &world.systems {
                let found = systems.iter().any(|s| s.name == *sys_name);
                if !found {
                    return Err(SemaError {
                        message: format!("unknown system `{sys_name}` in world"),
                        span: world.span.clone(),
                        label: "here".into(),
                        source_code: source.to_string(),
                    });
                }
                world_systems.push(sys_name.clone());
            }
        }
    }

    let ordered_components: Vec<ComponentInfo> = component_order
        .iter()
        .map(|name| components[name].clone())
        .collect();
    let layout = MemoryLayout::compute(&ordered_components);

    Ok(ResolvedProgram {
        components: ordered_components,
        systems,
        functions,
        extern_fns,
        entities,
        world_systems,
        layout,
    })
}

/// Type context shared by all type-checking functions.
struct TypeCtx<'a> {
    params: &'a [EachParamInfo],
    components: &'a HashMap<String, ComponentInfo>,
    locals: &'a mut HashMap<String, Ty>,
    functions: &'a HashMap<String, FnSignature>,
    return_ty: Option<&'a Ty>,
    source: &'a str,
}

fn type_check_body(
    stmts: &[Stmt],
    params: &[EachParamInfo],
    components: &HashMap<String, ComponentInfo>,
    locals: &mut HashMap<String, Ty>,
    functions: &HashMap<String, FnSignature>,
    source: &str,
) -> Result<(), SemaError> {
    let mut ctx = TypeCtx { params, components, locals, functions, return_ty: None, source };
    check_stmts(stmts, &mut ctx)
}

fn type_check_fn_body(
    stmts: &[Stmt],
    locals: &mut HashMap<String, Ty>,
    return_ty: Option<&Ty>,
    functions: &HashMap<String, FnSignature>,
    source: &str,
) -> Result<(), SemaError> {
    let empty: Vec<EachParamInfo> = vec![];
    let empty_comps: HashMap<String, ComponentInfo> = HashMap::new();
    let mut ctx = TypeCtx {
        params: &empty,
        components: &empty_comps,
        locals,
        functions,
        return_ty,
        source,
    };
    check_stmts(stmts, &mut ctx)
}

fn check_stmts(stmts: &[Stmt], ctx: &mut TypeCtx) -> Result<(), SemaError> {
    for stmt in stmts {
        match stmt {
            Stmt::Assign(assign) => {
                let lhs_ty = infer_type(&assign.target, ctx)?;
                let rhs_ty = infer_type(&assign.value, ctx)?;
                if lhs_ty != rhs_ty {
                    return Err(SemaError {
                        message: format!(
                            "type mismatch in assignment: expected `{lhs_ty}`, found `{rhs_ty}`"
                        ),
                        span: assign.span.clone(),
                        label: format!("expected `{lhs_ty}`"),
                        source_code: ctx.source.to_string(),
                    });
                }
            }
            Stmt::Let(let_stmt) => {
                let val_ty = infer_type(&let_stmt.value, ctx)?;
                let ty = if let Some(ast_ty) = &let_stmt.ty {
                    let declared = Ty::from_ast(ast_ty);
                    if declared != val_ty {
                        return Err(SemaError {
                            message: format!(
                                "type mismatch in let: declared `{declared}`, value is `{val_ty}`"
                            ),
                            span: let_stmt.span.clone(),
                            label: format!("expected `{declared}`"),
                            source_code: ctx.source.to_string(),
                        });
                    }
                    declared
                } else {
                    val_ty
                };
                ctx.locals.insert(let_stmt.name.clone(), ty);
            }
            Stmt::If(if_stmt) => {
                infer_type(&if_stmt.condition, ctx)?;
                check_stmts(&if_stmt.then_body, ctx)?;
                if let Some(else_body) = &if_stmt.else_body {
                    check_stmts(else_body, ctx)?;
                }
            }
            Stmt::While(while_stmt) => {
                infer_type(&while_stmt.condition, ctx)?;
                check_stmts(&while_stmt.body, ctx)?;
            }
            Stmt::Expr(expr) => {
                infer_type(expr, ctx)?;
            }
            Stmt::Return(Some(expr), span) => {
                let ty = infer_type(expr, ctx)?;
                if let Some(ret_ty) = ctx.return_ty {
                    if ty != *ret_ty {
                        return Err(SemaError {
                            message: format!(
                                "return type mismatch: expected `{ret_ty}`, found `{ty}`"
                            ),
                            span: span.clone(),
                            label: format!("expected `{ret_ty}`"),
                            source_code: ctx.source.to_string(),
                        });
                    }
                }
            }
            Stmt::Return(None, _) => {}
        }
    }
    Ok(())
}

fn infer_type(expr: &Expr, ctx: &TypeCtx) -> Result<Ty, SemaError> {
    match expr {
        Expr::IntLit(_, _) => Ok(Ty::I32),
        Expr::FloatLit(_, _) => Ok(Ty::F32),
        Expr::BoolLit(_, _) => Ok(Ty::Bool),
        Expr::Ident(name, span) => {
            if let Some(ty) = ctx.locals.get(name) {
                Ok(ty.clone())
            } else if ctx.params.iter().any(|p| p.name == *name) {
                Ok(Ty::Entity) // placeholder for component ref
            } else {
                Err(SemaError {
                    message: format!("undefined variable `{name}`"),
                    span: span.clone(),
                    label: "not found".into(),
                    source_code: ctx.source.to_string(),
                })
            }
        }
        Expr::FieldAccess(obj, field, span) => {
            if let Expr::Ident(param_name, _) = obj.as_ref() {
                if let Some(param) = ctx.params.iter().find(|p| p.name == *param_name) {
                    let comp = &ctx.components[&param.component];
                    if let Some(fi) = comp.fields.iter().find(|f| f.name == *field) {
                        return Ok(fi.ty.clone());
                    } else {
                        return Err(SemaError {
                            message: format!(
                                "component `{}` has no field `{field}`",
                                param.component
                            ),
                            span: span.clone(),
                            label: "no such field".into(),
                            source_code: ctx.source.to_string(),
                        });
                    }
                }
                Err(SemaError {
                    message: format!("undefined variable `{param_name}`"),
                    span: span.clone(),
                    label: "not found".into(),
                    source_code: ctx.source.to_string(),
                })
            } else {
                Err(SemaError {
                    message: "field access only supported on component parameters".into(),
                    span: span.clone(),
                    label: "unsupported".into(),
                    source_code: ctx.source.to_string(),
                })
            }
        }
        Expr::Call(name, args, span) => {
            let sig = ctx.functions.get(name).ok_or_else(|| SemaError {
                message: format!("undefined function `{name}`"),
                span: span.clone(),
                label: "not found".into(),
                source_code: ctx.source.to_string(),
            })?;
            if args.len() != sig.params.len() {
                return Err(SemaError {
                    message: format!(
                        "function `{name}` expects {} arguments, got {}",
                        sig.params.len(),
                        args.len()
                    ),
                    span: span.clone(),
                    label: "wrong number of arguments".into(),
                    source_code: ctx.source.to_string(),
                });
            }
            for (i, arg) in args.iter().enumerate() {
                let arg_ty = infer_type(arg, ctx)?;
                let (param_name, param_ty) = &sig.params[i];
                if arg_ty != *param_ty {
                    return Err(SemaError {
                        message: format!(
                            "argument `{param_name}` of `{name}`: expected `{param_ty}`, got `{arg_ty}`"
                        ),
                        span: arg.span().clone(),
                        label: format!("expected `{param_ty}`"),
                        source_code: ctx.source.to_string(),
                    });
                }
            }
            // Void functions return I32(0) as placeholder — callers in expression
            // context would be caught by type mismatch at the assignment/binop level.
            Ok(sig.return_ty.clone().unwrap_or(Ty::I32))
        }
        Expr::BinOp(left, op, right, span) => {
            let left_ty = infer_type(left, ctx)?;
            let right_ty = infer_type(right, ctx)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if left_ty.is_numeric() && right_ty.is_numeric() {
                        if left_ty == right_ty {
                            Ok(left_ty)
                        } else if left_ty.is_float() || right_ty.is_float() {
                            Ok(if left_ty == Ty::F64 || right_ty == Ty::F64 {
                                Ty::F64
                            } else {
                                Ty::F32
                            })
                        } else {
                            Ok(left_ty)
                        }
                    } else {
                        Err(SemaError {
                            message: format!(
                                "arithmetic on non-numeric types `{left_ty}` and `{right_ty}`"
                            ),
                            span: span.clone(),
                            label: "type mismatch".into(),
                            source_code: ctx.source.to_string(),
                        })
                    }
                }
                BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq
                | BinOp::GtEq => Ok(Ty::Bool),
                BinOp::And | BinOp::Or => Ok(Ty::Bool),
            }
        }
        Expr::UnaryOp(op, inner, _span) => {
            let ty = infer_type(inner, ctx)?;
            match op {
                UnaryOp::Neg => Ok(ty),
                UnaryOp::Not => Ok(Ty::Bool),
            }
        }
    }
}
