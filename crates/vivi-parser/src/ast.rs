use std::ops::Range;

pub type Span = Range<usize>;

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Component(ComponentDef),
    System(SystemDef),
    World(WorldDef),
    Fn(FnDef),
    Extern(ExternBlock),
    Entity(EntityDef),
    Global(GlobalDef),
    Use(UseDecl),
}

#[derive(Debug, Clone)]
pub struct UseDecl {
    pub path: Vec<String>, // e.g. ["std", "render"]
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct GlobalDef {
    pub name: String,
    pub ty: TypeName,
    pub init_value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ComponentDef {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: TypeName,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeName {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Entity,
}

impl TypeName {
    pub fn byte_size(&self) -> u32 {
        match self {
            TypeName::I32 => 4,
            TypeName::I64 => 8,
            TypeName::F32 => 4,
            TypeName::F64 => 8,
            TypeName::Bool => 4, // stored as i32 in wasm
            TypeName::Entity => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemDef {
    pub name: String,
    pub query: Option<QueryDef>,
    pub each: Option<EachBlock>,
    /// Bare statements for systems without query/each (run once per tick)
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct QueryDef {
    pub entries: Vec<QueryEntry>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct QueryEntry {
    pub access: Access,
    pub component: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Access {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub struct EachBlock {
    pub params: Vec<EachParam>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EachParam {
    pub name: String,
    pub component: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Assign(AssignStmt),
    Let(LetStmt),
    If(IfStmt),
    While(WhileStmt),
    ForLoop(ForLoopStmt),
    Spawn(SpawnStmt),
    Despawn(Span),
    Expr(Expr),
    Return(Option<Expr>, Span),
}

#[derive(Debug, Clone)]
pub struct ForLoopStmt {
    pub var: String,
    pub start: Expr,
    pub end: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SpawnStmt {
    pub components: Vec<SpawnComponent>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct SpawnComponent {
    pub component: String,
    pub fields: Vec<(String, Expr)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct AssignStmt {
    pub target: Expr,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub name: String,
    pub ty: Option<TypeName>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub condition: Expr,
    pub then_body: Vec<Stmt>,
    pub else_body: Option<Vec<Stmt>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub condition: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    IntLit(i64, Span),
    FloatLit(f64, Span),
    BoolLit(bool, Span),
    Ident(String, Span),
    FieldAccess(Box<Expr>, String, Span),
    Call(String, Vec<Expr>, Span),
    BinOp(Box<Expr>, BinOp, Box<Expr>, Span),
    UnaryOp(UnaryOp, Box<Expr>, Span),
}

impl Expr {
    pub fn span(&self) -> &Span {
        match self {
            Expr::IntLit(_, s) => s,
            Expr::FloatLit(_, s) => s,
            Expr::BoolLit(_, s) => s,
            Expr::Ident(_, s) => s,
            Expr::FieldAccess(_, _, s) => s,
            Expr::Call(_, _, s) => s,
            Expr::BinOp(_, _, _, s) => s,
            Expr::UnaryOp(_, _, s) => s,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<FnParam>,
    pub return_ty: Option<TypeName>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnParam {
    pub name: String,
    pub ty: TypeName,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExternBlock {
    pub module_name: String, // "host" by default
    pub functions: Vec<ExternFn>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ExternFn {
    pub name: String,
    pub params: Vec<FnParam>,
    pub return_ty: Option<TypeName>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EntityDef {
    pub name: String,
    pub components: Vec<EntityComponent>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EntityComponent {
    pub component: String,
    pub fields: Vec<(String, Expr)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct WorldDef {
    pub name: String,
    pub init_systems: Vec<String>,
    pub systems: Vec<String>,
    pub span: Span,
}
