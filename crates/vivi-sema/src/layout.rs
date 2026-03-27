use crate::resolve::ComponentInfo;
use crate::types::Ty;

pub const MAX_ENTITIES: u32 = 10000;

/// Describes where a component field's SoA array lives in linear memory.
#[derive(Debug, Clone)]
pub struct FieldLayout {
    pub name: String,
    pub ty: Ty,
    pub offset: u32,       // byte offset from memory start
    pub element_size: u32, // bytes per element (4 for f32/i32, 8 for f64/i64)
}

/// Layout for all fields of a component.
#[derive(Debug, Clone)]
pub struct ComponentLayout {
    pub name: String,
    pub fields: Vec<FieldLayout>,
}

/// Full memory layout for all components.
#[derive(Debug, Clone)]
pub struct MemoryLayout {
    pub components: Vec<ComponentLayout>,
    pub entity_count_offset: u32,
    pub total_bytes: u32,
}

impl MemoryLayout {
    /// Compute SoA memory layout.
    /// Layout:
    ///   [0..4]  entity_count: i32
    ///   [4..]   field arrays, each MAX_ENTITIES * element_size bytes
    pub fn compute(components: &[ComponentInfo]) -> Self {
        let entity_count_offset = 0u32;
        let mut offset = 4u32; // after entity_count

        let mut comp_layouts = Vec::new();
        for comp in components {
            let mut field_layouts = Vec::new();
            for field in &comp.fields {
                let element_size = field.ty.byte_size();
                field_layouts.push(FieldLayout {
                    name: field.name.clone(),
                    ty: field.ty.clone(),
                    offset,
                    element_size,
                });
                offset += MAX_ENTITIES * element_size;
            }
            comp_layouts.push(ComponentLayout {
                name: comp.name.clone(),
                fields: field_layouts,
            });
        }

        MemoryLayout {
            components: comp_layouts,
            entity_count_offset,
            total_bytes: offset,
        }
    }

    pub fn get_component(&self, name: &str) -> Option<&ComponentLayout> {
        self.components.iter().find(|c| c.name == name)
    }

    /// Required WASM memory pages (64KB each).
    pub fn required_pages(&self) -> u32 {
        (self.total_bytes + 65535) / 65536
    }
}
