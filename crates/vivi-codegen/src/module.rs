use std::collections::HashMap;
use vivi_parser::ast::{Item, Program};
use vivi_sema::resolve::ResolvedProgram;
use vivi_sema::types::Ty;
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction, MemorySection,
    MemoryType, Module, TypeSection, ValType,
};

use crate::function::compile_user_fn;
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
    let mut module = Module::new();

    let fn_count = resolved.functions.len();
    let system_count = resolved.world_systems.len();

    // Build function index map: fn_name -> wasm function index
    // Layout: [0..fn_count) user fns, [fn_count..fn_count+system_count) systems,
    //         fn_count+system_count = init, fn_count+system_count+1 = tick
    let mut fn_index_map: HashMap<String, u32> = HashMap::new();
    for (i, sig) in resolved.functions.iter().enumerate() {
        fn_index_map.insert(sig.name.clone(), i as u32);
    }

    // -- Type section --
    let mut types = TypeSection::new();

    // Type for each user fn
    for sig in &resolved.functions {
        let params: Vec<ValType> = sig.params.iter().map(|(_, ty)| ty_to_valtype(ty)).collect();
        let results: Vec<ValType> = sig.return_ty.as_ref().map_or(vec![], |ty| vec![ty_to_valtype(ty)]);
        types.ty().function(params, results);
    }

    // Type for system functions and init/tick: () -> ()
    let void_type_idx = fn_count as u32;
    types.ty().function(vec![], vec![]);

    module.section(&types);

    // -- Function section --
    let mut functions = FunctionSection::new();

    // User functions
    for i in 0..fn_count {
        functions.function(i as u32); // type index = fn index
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
    let init_func_idx = (fn_count + system_count) as u32;
    let tick_func_idx = (fn_count + system_count + 1) as u32;
    exports.export("init", ExportKind::Func, init_func_idx);
    exports.export("tick", ExportKind::Func, tick_func_idx);
    exports.export("memory", ExportKind::Memory, 0);
    module.section(&exports);

    // -- Code section --
    let mut codes = CodeSection::new();

    // Compile user functions
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
        let func = compile_user_fn(sig, &ast_fn.body, &fn_index_map);
        codes.function(&func);
    }

    // Compile system functions
    let system_base = fn_count as u32;
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
        let func = compile_system(sys_info, &ast_system.each.body, &resolved.layout, &fn_index_map);
        codes.function(&func);
    }

    // init
    codes.function(&compile_init());

    // tick: calls system functions
    let tick_func = compile_tick(system_base, system_count);
    codes.function(&tick_func);

    module.section(&codes);

    module.finish()
}

fn compile_init() -> Function {
    let mut func = Function::new(vec![]);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
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
