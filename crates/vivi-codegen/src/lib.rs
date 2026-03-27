pub mod expr;
pub mod function;
pub mod module;
pub mod sourcemap;
pub mod system;

pub use module::{generate_wasm, generate_wasm_with_sourcemap};
pub use sourcemap::{generate_source_map, resolve_mappings, source_mapping_url_section};
