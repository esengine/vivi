use vivi_parser::ast::TypeName;

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Entity,
}

impl Ty {
    pub fn from_ast(ty: &TypeName) -> Self {
        match ty {
            TypeName::I32 => Ty::I32,
            TypeName::I64 => Ty::I64,
            TypeName::F32 => Ty::F32,
            TypeName::F64 => Ty::F64,
            TypeName::Bool => Ty::Bool,
            TypeName::Entity => Ty::Entity,
        }
    }

    pub fn byte_size(&self) -> u32 {
        match self {
            Ty::I32 | Ty::F32 | Ty::Bool | Ty::Entity => 4,
            Ty::I64 | Ty::F64 => 8,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::I32 | Ty::I64 | Ty::F32 | Ty::F64)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Ty::F32 | Ty::F64)
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Ty::I32 | Ty::I64)
    }
}

impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::I32 => write!(f, "i32"),
            Ty::I64 => write!(f, "i64"),
            Ty::F32 => write!(f, "f32"),
            Ty::F64 => write!(f, "f64"),
            Ty::Bool => write!(f, "bool"),
            Ty::Entity => write!(f, "Entity"),
        }
    }
}
