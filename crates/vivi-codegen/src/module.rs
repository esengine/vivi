use vivi_parser::ast::{Item, Program};
use vivi_sema::layout::MemoryLayout;
use vivi_sema::resolve::ResolvedProgram;
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction, MemorySection,
    MemoryType, Module, TypeSection,
};

use crate::system::compile_system;

pub fn generate_wasm(program: &Program, resolved: &ResolvedProgram) -> Vec<u8> {
    let mut module = Module::new();

    // -- Type section --
    let mut types = TypeSection::new();

    // Type 0: () -> () for system functions and tick
    types.ty().function(vec![], vec![]);

    // Type 1: () -> () for init (same signature)
    // We reuse type 0

    module.section(&types);

    // -- Function section --
    let mut functions = FunctionSection::new();

    // Collect system functions in world order
    let system_count = resolved.world_systems.len();

    // Function indices:
    // 0..system_count-1: system functions
    // system_count: init function
    // system_count+1: tick function
    for _ in 0..system_count {
        functions.function(0); // type 0: () -> ()
    }
    functions.function(0); // init: () -> ()
    functions.function(0); // tick: () -> ()

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
    let init_func_idx = system_count as u32;
    let tick_func_idx = system_count as u32 + 1;
    exports.export("init", ExportKind::Func, init_func_idx);
    exports.export("tick", ExportKind::Func, tick_func_idx);
    exports.export("memory", ExportKind::Memory, 0);
    module.section(&exports);

    // -- Code section --
    let mut codes = CodeSection::new();

    // Compile each system function
    for sys_name in &resolved.world_systems {
        let sys_info = resolved
            .systems
            .iter()
            .find(|s| s.name == *sys_name)
            .unwrap();

        // Find the AST system to get the each body
        let ast_system = program
            .items
            .iter()
            .find_map(|item| {
                if let Item::System(s) = item {
                    if s.name == *sys_name {
                        Some(s)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap();

        let func = compile_system(sys_info, &ast_system.each.body, &resolved.layout);
        codes.function(&func);
    }

    // Init function: set entity_count = 0
    let init_func = compile_init(&resolved.layout);
    codes.function(&init_func);

    // Tick function: call each system
    let tick_func = compile_tick(system_count);
    codes.function(&tick_func);

    module.section(&codes);

    module.finish()
}

fn compile_init(_layout: &MemoryLayout) -> Function {
    let mut func = Function::new(vec![]);
    // Set entity_count to 0
    func.instruction(&Instruction::I32Const(0)); // address
    func.instruction(&Instruction::I32Const(0)); // value
    func.instruction(&Instruction::I32Store(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

fn compile_tick(system_count: usize) -> Function {
    let mut func = Function::new(vec![]);
    for i in 0..system_count {
        func.instruction(&Instruction::Call(i as u32));
    }
    func.instruction(&Instruction::End);
    func
}
