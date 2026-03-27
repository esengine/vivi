pub mod expr;
pub mod module;
pub mod system;

#[derive(Clone)]
pub(crate) struct FieldLayoutClone {
    pub offset: u32,
    pub element_size: u32,
}

pub use module::generate_wasm;
