use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use vivi_sema::layout::DEFAULT_MAX_ENTITIES as MAX_ENTITIES;
use wasmtime::*;

const SOURCE: &str = r#"
component Position {
    x: f32
    y: f32
}

component Velocity {
    dx: f32
    dy: f32
}

system Movement {
    query {
        write Position
        read Velocity
    }
    each(pos: Position, vel: Velocity) {
        pos.x = pos.x + vel.dx
        pos.y = pos.y + vel.dy
    }
}

world Game {
    systems {
        Movement
    }
}
"#;

fn compile_vivi(source: &str) -> Vec<u8> {
    let program = vivi_parser::parse(source).unwrap();
    let resolved = vivi_sema::resolve(&program, source).unwrap();
    vivi_codegen::generate_wasm(&program, &resolved)
}

fn setup_instance(engine: &Engine, wasm_bytes: &[u8], entity_count: u32) -> (Store<()>, Func, Memory) {
    let module = Module::new(engine, wasm_bytes).unwrap();
    let mut store = Store::new(engine, ());
    let instance = Instance::new(&mut store, &module, &[]).unwrap();

    let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
    let tick = instance.get_func(&mut store, "tick").unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();

    init.call(&mut store, ()).unwrap();

    // Set entity count and initialize data
    let pos_x_off = 4usize;
    let pos_y_off = 4 + (MAX_ENTITIES as usize) * 4;
    let vel_dx_off = 4 + (MAX_ENTITIES as usize) * 8;
    let vel_dy_off = 4 + (MAX_ENTITIES as usize) * 12;

    let data = memory.data_mut(&mut store);
    data[0..4].copy_from_slice(&(entity_count as i32).to_le_bytes());

    for i in 0..entity_count as usize {
        let off = i * 4;
        data[pos_x_off + off..pos_x_off + off + 4].copy_from_slice(&(i as f32).to_le_bytes());
        data[pos_y_off + off..pos_y_off + off + 4].copy_from_slice(&(i as f32).to_le_bytes());
        data[vel_dx_off + off..vel_dx_off + off + 4].copy_from_slice(&1.0f32.to_le_bytes());
        data[vel_dy_off + off..vel_dy_off + off + 4].copy_from_slice(&0.5f32.to_le_bytes());
    }

    (store, tick, memory)
}

fn bench_tick(c: &mut Criterion) {
    let wasm_bytes = compile_vivi(SOURCE);
    let engine = Engine::default();

    let mut group = c.benchmark_group("tick");

    for &count in &[100, 1000, 5000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("entities", count),
            &count,
            |b, &count| {
                let (mut store, tick, _memory) = setup_instance(&engine, &wasm_bytes, count);
                b.iter(|| {
                    tick.call(black_box(&mut store), &[], &mut []).unwrap();
                });
            },
        );
    }

    group.finish();
}

fn bench_compile(c: &mut Criterion) {
    c.bench_function("compile_movement", |b| {
        b.iter(|| {
            compile_vivi(black_box(SOURCE));
        });
    });
}

criterion_group!(benches, bench_tick, bench_compile);
criterion_main!(benches);
