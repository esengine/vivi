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

#[derive(Debug)]
pub struct ResolvedProgram {
    pub components: Vec<ComponentInfo>,
    pub systems: Vec<SystemInfo>,
    pub world_systems: Vec<String>,
    pub layout: MemoryLayout,
}

pub fn resolve(program: &Program, source: &str) -> Result<ResolvedProgram, SemaError> {
    let mut components: HashMap<String, ComponentInfo> = HashMap::new();
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

    // Second pass: resolve systems
    for item in &program.items {
        if let Item::System(sys) = item {
            let mut query_entries = Vec::new();

            for entry in &sys.query.entries {
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

            // Validate each params reference queried components
            let mut each_params = Vec::new();
            for param in &sys.each.params {
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

            // Type check each body
            type_check_body(
                &sys.each.body,
                &each_params,
                &components,
                source,
            )?;

            systems.push(SystemInfo {
                name: sys.name.clone(),
                query: query_entries,
                each_params,
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
        world_systems,
        layout,
    })
}

fn type_check_body(
    stmts: &[Stmt],
    params: &[EachParamInfo],
    components: &HashMap<String, ComponentInfo>,
    source: &str,
) -> Result<(), SemaError> {
    for stmt in stmts {
        match stmt {
            Stmt::Assign(assign) => {
                let _lhs_ty = infer_expr_type(&assign.target, params, components, source)?;
                let _rhs_ty = infer_expr_type(&assign.value, params, components, source)?;
                // For Phase 1, just check both sides type-check.
                // Full type compatibility checking can come later.
            }
            Stmt::Let(let_stmt) => {
                let _ty = infer_expr_type(&let_stmt.value, params, components, source)?;
            }
            Stmt::If(if_stmt) => {
                let _cond_ty =
                    infer_expr_type(&if_stmt.condition, params, components, source)?;
                type_check_body(&if_stmt.then_body, params, components, source)?;
                if let Some(else_body) = &if_stmt.else_body {
                    type_check_body(else_body, params, components, source)?;
                }
            }
            Stmt::While(while_stmt) => {
                let _cond_ty =
                    infer_expr_type(&while_stmt.condition, params, components, source)?;
                type_check_body(&while_stmt.body, params, components, source)?;
            }
            Stmt::Expr(expr) => {
                let _ty = infer_expr_type(expr, params, components, source)?;
            }
            Stmt::Return(Some(expr), _) => {
                let _ty = infer_expr_type(expr, params, components, source)?;
            }
            Stmt::Return(None, _) => {}
        }
    }
    Ok(())
}

fn infer_expr_type(
    expr: &Expr,
    params: &[EachParamInfo],
    components: &HashMap<String, ComponentInfo>,
    source: &str,
) -> Result<Ty, SemaError> {
    match expr {
        Expr::IntLit(_, _) => Ok(Ty::I32),
        Expr::FloatLit(_, _) => Ok(Ty::F32),
        Expr::BoolLit(_, _) => Ok(Ty::Bool),
        Expr::Ident(name, span) => {
            // Check if it's an each parameter
            if params.iter().any(|p| p.name == *name) {
                // Component reference - this is a struct, but we'll handle it in field access
                Ok(Ty::I32) // placeholder; actual type resolved at field access
            } else {
                Err(SemaError {
                    message: format!("undefined variable `{name}`"),
                    span: span.clone(),
                    label: "not found".into(),
                    source_code: source.to_string(),
                })
            }
        }
        Expr::FieldAccess(obj, field, span) => {
            if let Expr::Ident(param_name, _) = obj.as_ref() {
                let param = params.iter().find(|p| p.name == *param_name);
                if let Some(param) = param {
                    let comp = &components[&param.component];
                    let field_info = comp.fields.iter().find(|f| f.name == *field);
                    if let Some(fi) = field_info {
                        Ok(fi.ty.clone())
                    } else {
                        Err(SemaError {
                            message: format!(
                                "component `{}` has no field `{field}`",
                                param.component
                            ),
                            span: span.clone(),
                            label: "no such field".into(),
                            source_code: source.to_string(),
                        })
                    }
                } else {
                    Err(SemaError {
                        message: format!("undefined variable `{param_name}`"),
                        span: span.clone(),
                        label: "not found".into(),
                        source_code: source.to_string(),
                    })
                }
            } else {
                Err(SemaError {
                    message: "field access only supported on component parameters".into(),
                    span: span.clone(),
                    label: "unsupported".into(),
                    source_code: source.to_string(),
                })
            }
        }
        Expr::BinOp(left, op, right, span) => {
            let left_ty = infer_expr_type(left, params, components, source)?;
            let right_ty = infer_expr_type(right, params, components, source)?;
            match op {
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                    if left_ty.is_numeric() && right_ty.is_numeric() {
                        // Use the "wider" type
                        if left_ty == right_ty {
                            Ok(left_ty)
                        } else if left_ty.is_float() || right_ty.is_float() {
                            Ok(if left_ty == Ty::F64 || right_ty == Ty::F64 {
                                Ty::F64
                            } else {
                                Ty::F32
                            })
                        } else {
                            Ok(left_ty) // both integer, use left
                        }
                    } else {
                        Err(SemaError {
                            message: format!(
                                "arithmetic on non-numeric types `{left_ty}` and `{right_ty}`"
                            ),
                            span: span.clone(),
                            label: "type mismatch".into(),
                            source_code: source.to_string(),
                        })
                    }
                }
                BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq
                | BinOp::GtEq => Ok(Ty::Bool),
                BinOp::And | BinOp::Or => Ok(Ty::Bool),
            }
        }
        Expr::UnaryOp(op, inner, _span) => {
            let ty = infer_expr_type(inner, params, components, source)?;
            match op {
                UnaryOp::Neg => Ok(ty),
                UnaryOp::Not => Ok(Ty::Bool),
            }
        }
    }
}
