use std::collections::{HashMap, HashSet};
use vivi_parser::ast::{Item, Program};
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::{FieldValue, ResolvedProgram};
use vivi_sema::types::Ty;
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, MemorySection, MemoryType, Module, TypeSection, ValType,
};

use crate::function::compile_user_fn;
use crate::sourcemap::{FuncMappings, ModuleMappings};
use crate::system::compile_system;

fn ty_to_valtype(ty: &Ty) -> ValType {
    match ty {
        Ty::F32 => ValType::F32,
        Ty::F64 => ValType::F64,
        Ty::I64 => ValType::I64,
        Ty::I32 | Ty::Bool | Ty::Entity => ValType::I32,
    }
}

pub fn generate_wasm(program: &Program, resolved: &ResolvedProgram) -> Vec<u8> {
    generate_wasm_with_mappings(program, resolved, None).0
}

pub fn generate_wasm_with_sourcemap(
    program: &Program,
    resolved: &ResolvedProgram,
    source: &str,
) -> (Vec<u8>, ModuleMappings) {
    generate_wasm_with_mappings(program, resolved, Some(source))
}

fn generate_wasm_with_mappings(
    program: &Program,
    resolved: &ResolvedProgram,
    source: Option<&str>,
) -> (Vec<u8>, ModuleMappings) {
    let mut module = Module::new();

    let import_count = resolved.extern_fns.len();
    let fn_count = resolved.functions.len();
    let system_count = resolved.world_systems.len();

    // WASM function index layout:
    // [0 .. import_count)                          imported (extern) functions
    // [import_count .. import_count+fn_count)      user fns
    // [import_count+fn_count .. +system_count)     system fns
    // import_count+fn_count+system_count           init
    // import_count+fn_count+system_count+1         tick

    let mut fn_index_map: HashMap<String, u32> = HashMap::new();

    // -- Type section --
    let mut types = TypeSection::new();
    let mut type_indices: Vec<u32> = Vec::new(); // type index per function

    // Types for extern fns
    for efn in &resolved.extern_fns {
        let idx = type_indices.len() as u32;
        let params: Vec<ValType> = efn.params.iter().map(|(_, ty)| ty_to_valtype(ty)).collect();
        let results: Vec<ValType> = efn.return_ty.as_ref().map_or(vec![], |ty| vec![ty_to_valtype(ty)]);
        types.ty().function(params, results);
        type_indices.push(idx);
        fn_index_map.insert(efn.name.clone(), fn_index_map.len() as u32);
    }

    // Types for user fns
    for sig in &resolved.functions {
        let idx = type_indices.len() as u32;
        let params: Vec<ValType> = sig.params.iter().map(|(_, ty)| ty_to_valtype(ty)).collect();
        let results: Vec<ValType> = sig.return_ty.as_ref().map_or(vec![], |ty| vec![ty_to_valtype(ty)]);
        types.ty().function(params, results);
        type_indices.push(idx);
        fn_index_map.insert(sig.name.clone(), fn_index_map.len() as u32);
    }

    // Type for void () -> () (systems, init, tick)
    let void_type_idx = type_indices.len() as u32;
    types.ty().function(vec![], vec![]);

    module.section(&types);

    // -- Import section --
    if !resolved.extern_fns.is_empty() {
        let mut imports = ImportSection::new();
        for (i, efn) in resolved.extern_fns.iter().enumerate() {
            imports.import(
                &efn.module_name,
                &efn.name,
                wasm_encoder::EntityType::Function(type_indices[i]),
            );
        }
        module.section(&imports);
    }

    // -- Function section (local functions only, not imports) --
    let mut functions = FunctionSection::new();

    // User functions
    for i in 0..fn_count {
        functions.function(type_indices[import_count + i]);
    }

    // System functions
    for _ in 0..system_count {
        functions.function(void_type_idx);
    }

    // init + tick
    functions.function(void_type_idx);
    functions.function(void_type_idx);

    module.section(&functions);

    // -- Memory section --
    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: resolved.layout.required_pages() as u64,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    // -- Export section --
    let mut exports = ExportSection::new();
    let init_func_idx = (import_count + fn_count + system_count) as u32;
    let tick_func_idx = init_func_idx + 1;
    exports.export("init", ExportKind::Func, init_func_idx);
    exports.export("tick", ExportKind::Func, tick_func_idx);
    exports.export("memory", ExportKind::Memory, 0);
    module.section(&exports);

    // -- Code section (local functions only) --
    let mut codes = CodeSection::new();

    // Build void function set
    let mut void_fns: HashSet<String> = HashSet::new();
    for efn in &resolved.extern_fns {
        if efn.return_ty.is_none() {
            void_fns.insert(efn.name.clone());
        }
    }
    for sig in &resolved.functions {
        if sig.return_ty.is_none() {
            void_fns.insert(sig.name.clone());
        }
    }

    let mut module_mappings = ModuleMappings::default();
    let src = source.unwrap_or("");

    // User functions
    for sig in &resolved.functions {
        let ast_fn = program
            .items
            .iter()
            .find_map(|item| {
                if let Item::Fn(f) = item {
                    if f.name == sig.name { Some(f) } else { None }
                } else {
                    None
                }
            })
            .unwrap();
        let mut fm = FuncMappings::default();
        let func = compile_user_fn(sig, &ast_fn.body, &fn_index_map, &void_fns, src, &mut fm);
        codes.function(&func);
        module_mappings.functions.push(fm);
    }

    // System functions
    let system_base = (import_count + fn_count) as u32;
    for sys_name in &resolved.world_systems {
        let sys_info = resolved.systems.iter().find(|s| s.name == *sys_name).unwrap();
        let ast_system = program
            .items
            .iter()
            .find_map(|item| {
                if let Item::System(s) = item {
                    if s.name == *sys_name { Some(s) } else { None }
                } else {
                    None
                }
            })
            .unwrap();
        let mut fm = FuncMappings::default();
        let func = compile_system(sys_info, &ast_system.each.body, &resolved.layout, &fn_index_map, &void_fns, src, &mut fm);
        codes.function(&func);
        module_mappings.functions.push(fm);
    }

    // init + tick (no source mappings for these generated functions)
    module_mappings.functions.push(FuncMappings::default()); // init
    module_mappings.functions.push(FuncMappings::default()); // tick

    let init_func = compile_init(&resolved.entities, &resolved.layout);
    codes.function(&init_func);

    let tick_func = compile_tick(system_base, system_count);
    codes.function(&tick_func);

    module.section(&codes);

    (module.finish(), module_mappings)
}

fn compile_init(
    entities: &[vivi_sema::EntityInfo],
    layout: &MemoryLayout,
) -> Function {
    let mut func = Function::new(vec![]);

    // Set entity_count = number of entity templates
    let entity_count = entities.len() as i32;
    func.instruction(&Instruction::I32Const(0)); // address of entity_count
    func.instruction(&Instruction::I32Const(entity_count));
    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Write each entity's component field values into memory
    for (entity_idx, entity) in entities.iter().enumerate() {
        for ec in &entity.components {
            let comp_layout = layout.get_component(&ec.component).unwrap();
            for (fname, fval) in &ec.field_values {
                let fl = comp_layout.fields.iter().find(|f| f.name == *fname).unwrap();
                // address = fl.offset + entity_idx * fl.element_size
                let addr = fl.offset + (entity_idx as u32) * fl.element_size;

                func.instruction(&Instruction::I32Const(addr as i32));
                match fval {
                    FieldValue::F32(v) => {
                        func.instruction(&Instruction::F32Const(*v));
                        func.instruction(&Instruction::F32Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    }
                    FieldValue::I32(v) => {
                        func.instruction(&Instruction::I32Const(*v));
                        func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    }
                    FieldValue::Bool(v) => {
                        func.instruction(&Instruction::I32Const(if *v { 1 } else { 0 }));
                        func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 2,
                            memory_index: 0,
                        }));
                    }
                }
            }
        }
    }

    func.instruction(&Instruction::End);
    func
}

fn compile_tick(system_base: u32, system_count: usize) -> Function {
    let mut func = Function::new(vec![]);
    for i in 0..system_count {
        func.instruction(&Instruction::Call(system_base + i as u32));
    }
    func.instruction(&Instruction::End);
    func
}
