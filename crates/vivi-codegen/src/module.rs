use std::collections::{HashMap, HashSet};
use vivi_parser::ast::{Item, Program};
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::{FieldValue, ResolvedProgram};
use vivi_sema::types::Ty;
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, ImportSection,
    Instruction, MemorySection, MemoryType, Module, NameMap, NameSection, TypeSection, ValType,
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
    // All unique systems (init + tick, deduplicated)
    let mut all_system_names: Vec<String> = Vec::new();
    for name in &resolved.world_init_systems {
        if !all_system_names.contains(name) {
            all_system_names.push(name.clone());
        }
    }
    for name in &resolved.world_systems {
        if !all_system_names.contains(name) {
            all_system_names.push(name.clone());
        }
    }
    let system_count = all_system_names.len();

    // WASM function index layout:
    // [0 .. import_count)                          imported (extern) functions
    // [import_count .. import_count+fn_count)      user fns
    // [import_count+fn_count .. +system_count)     all system fns
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

    // Build globals map for codegen
    let globals_map: HashMap<String, crate::expr::GlobalVar> = resolved.globals.iter()
        .map(|g| (g.name.clone(), crate::expr::GlobalVar { offset: g.offset, ty: g.ty.clone() }))
        .collect();

    // Build function return type map for accurate type inference in codegen
    let mut fn_return_types: HashMap<String, vivi_sema::types::Ty> = HashMap::new();
    for sig in &resolved.functions {
        if let Some(ty) = &sig.return_ty {
            fn_return_types.insert(sig.name.clone(), ty.clone());
        }
    }
    for efn in &resolved.extern_fns {
        if let Some(ty) = &efn.return_ty {
            fn_return_types.insert(efn.name.clone(), ty.clone());
        }
    }

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
        let func = compile_user_fn(sig, &ast_fn.body, &resolved.layout, &fn_index_map, &void_fns, &globals_map, &fn_return_types, src, &mut fm);
        codes.function(&func);
        module_mappings.functions.push(fm);
    }

    // All system functions (init systems + tick systems, deduplicated)
    let system_base = (import_count + fn_count) as u32;
    let mut system_index_map: HashMap<String, u32> = HashMap::new();
    for (i, sys_name) in all_system_names.iter().enumerate() {
        system_index_map.insert(sys_name.clone(), system_base + i as u32);
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
        let func = if let Some(each) = &ast_system.each {
            compile_system(sys_info, &each.body, &resolved.layout, &fn_index_map, &void_fns, &globals_map, &fn_return_types, src, &mut fm)
        } else {
            // Bare system: compile as system without entity loop (has layout for spawn)
            compile_system(sys_info, &ast_system.body, &resolved.layout, &fn_index_map, &void_fns, &globals_map, &fn_return_types, src, &mut fm)
        };
        codes.function(&func);
        module_mappings.functions.push(fm);
    }

    // init + tick
    module_mappings.functions.push(FuncMappings::default());
    module_mappings.functions.push(FuncMappings::default());

    // init: call init systems, then write entity templates
    let init_system_indices: Vec<u32> = resolved
        .world_init_systems
        .iter()
        .map(|name| system_index_map[name])
        .collect();
    let init_func = compile_init(&init_system_indices, &resolved.entities, &resolved.globals, &resolved.layout);
    codes.function(&init_func);

    // tick: call tick systems
    let tick_system_indices: Vec<u32> = resolved
        .world_systems
        .iter()
        .map(|name| system_index_map[name])
        .collect();
    let tick_func = compile_tick(&tick_system_indices);
    codes.function(&tick_func);

    module.section(&codes);

    // -- Name section (debug info: function and local variable names) --
    let mut names = NameSection::new();
    let mut func_names = NameMap::new();
    let mut func_idx = 0u32;

    // Import function names
    for efn in &resolved.extern_fns {
        func_names.append(func_idx, &efn.name);
        func_idx += 1;
    }

    // User function names
    for sig in &resolved.functions {
        func_names.append(func_idx, &sig.name);
        func_idx += 1;
    }

    // System function names
    for sys_name in &resolved.world_systems {
        func_names.append(func_idx, &format!("system_{sys_name}"));
        func_idx += 1;
    }

    func_names.append(func_idx, "init");
    func_names.append(func_idx + 1, "tick");

    names.functions(&func_names);
    module.section(&names);

    // -- Source map URL (must be LAST section) --
    // The sourceMappingURL custom section payload must be a WASM-encoded string:
    // a LEB128 length prefix followed by the UTF-8 URL bytes.
    // V8's DecodeSoSourceMappingURLSection() calls consume_utf8_string() which
    // expects this format. Without the length prefix, V8 silently fails to parse
    // the URL and never fetches the source map.
    if source.is_some() {
        let url = b"app.wasm.map";
        let mut data = Vec::with_capacity(1 + url.len());
        data.push(url.len() as u8); // LEB128 length prefix (single byte for len < 128)
        data.extend_from_slice(url);
        let custom = wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed("sourceMappingURL"),
            data: std::borrow::Cow::Owned(data),
        };
        module.section(&custom);
    }

    (module.finish(), module_mappings)
}

fn compile_init(
    init_system_indices: &[u32],
    entities: &[vivi_sema::EntityInfo],
    globals: &[vivi_sema::resolve::GlobalInfo],
    layout: &MemoryLayout,
) -> Function {
    let mut func = Function::new(vec![]);

    // Set entity_count = number of static entity templates
    let entity_count = entities.len() as i32;
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(entity_count));
    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0, align: 2, memory_index: 0,
    }));

    // Initialize globals
    for g in globals {
        func.instruction(&Instruction::I32Const(g.offset as i32));
        match &g.init_value {
            FieldValue::F32(v) => {
                func.instruction(&Instruction::F32Const(*v));
                func.instruction(&Instruction::F32Store(wasm_encoder::MemArg {
                    offset: 0, align: 2, memory_index: 0,
                }));
            }
            FieldValue::I32(v) => {
                func.instruction(&Instruction::I32Const(*v));
                func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 0, align: 2, memory_index: 0,
                }));
            }
            FieldValue::Bool(v) => {
                func.instruction(&Instruction::I32Const(if *v { 1 } else { 0 }));
                func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 0, align: 2, memory_index: 0,
                }));
            }
        }
    }

    // Write static entity template data
    for (entity_idx, entity) in entities.iter().enumerate() {
        for ec in &entity.components {
            let comp_layout = layout.get_component(&ec.component).unwrap();
            for (fname, fval) in &ec.field_values {
                let fl = comp_layout.fields.iter().find(|f| f.name == *fname).unwrap();
                let addr = fl.offset + (entity_idx as u32) * fl.element_size;
                func.instruction(&Instruction::I32Const(addr as i32));
                match fval {
                    FieldValue::F32(v) => {
                        func.instruction(&Instruction::F32Const(*v));
                        func.instruction(&Instruction::F32Store(wasm_encoder::MemArg {
                            offset: 0, align: 2, memory_index: 0,
                        }));
                    }
                    FieldValue::I32(v) => {
                        func.instruction(&Instruction::I32Const(*v));
                        func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                            offset: 0, align: 2, memory_index: 0,
                        }));
                    }
                    FieldValue::Bool(v) => {
                        func.instruction(&Instruction::I32Const(if *v { 1 } else { 0 }));
                        func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
                            offset: 0, align: 2, memory_index: 0,
                        }));
                    }
                }
            }
        }
    }

    // Call init systems (e.g. SpawnGalaxy)
    for &idx in init_system_indices {
        func.instruction(&Instruction::Call(idx));
    }

    func.instruction(&Instruction::End);
    func
}

fn compile_tick(tick_system_indices: &[u32]) -> Function {
    let mut func = Function::new(vec![]);
    for &idx in tick_system_indices {
        func.instruction(&Instruction::Call(idx));
    }
    func.instruction(&Instruction::End);
    func
}

