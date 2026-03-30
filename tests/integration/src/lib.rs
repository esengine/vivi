#[cfg(test)]
mod tests {
    use vivi_sema::layout::DEFAULT_MAX_ENTITIES as MAX_ENTITIES;
    use wasmtime::*;

    fn compile_vivi(source: &str) -> Vec<u8> {
        let program = vivi_parser::parse(source).expect("parse failed");
        let resolved = vivi_sema::resolve(&program, source).expect("sema failed");
        vivi_codegen::generate_wasm(&program, &resolved)
    }

    #[test]
    fn test_movement_system() {
        let source = r#"
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
        let wasm_bytes = compile_vivi(source);

        // Run with wasmtime
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).expect("failed to create module");
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).expect("failed to instantiate");

        let init = instance
            .get_typed_func::<(), ()>(&mut store, "init")
            .expect("init not found");
        let tick = instance
            .get_typed_func::<(), ()>(&mut store, "tick")
            .expect("tick not found");
        let memory = instance
            .get_memory(&mut store, "memory")
            .expect("memory not found");

        // Call init
        init.call(&mut store, ()).expect("init failed");

        // Manually set up 1 entity with position (10.0, 20.0) and velocity (1.0, 2.0)
        // Memory layout:
        //   [0..4]    entity_count: i32
        //   [4..]     Position_x[MAX_ENTITIES]   (f32 array)
        //   [4 + MAX*4..] Position_y[MAX_ENTITIES]
        //   [4 + MAX*8..] Velocity_dx[MAX_ENTITIES]
        //   [4 + MAX*12..] Velocity_dy[MAX_ENTITIES]

        let data = memory.data_mut(&mut store);

        // Set entity_count = 1
        data[0..4].copy_from_slice(&1i32.to_le_bytes());

        let pos_x_offset = 4usize;
        let pos_y_offset = 4 + (MAX_ENTITIES as usize) * 4;
        let vel_dx_offset = 4 + (MAX_ENTITIES as usize) * 8;
        let vel_dy_offset = 4 + (MAX_ENTITIES as usize) * 12;

        // Position[0] = (10.0, 20.0)
        data[pos_x_offset..pos_x_offset + 4].copy_from_slice(&10.0f32.to_le_bytes());
        data[pos_y_offset..pos_y_offset + 4].copy_from_slice(&20.0f32.to_le_bytes());

        // Velocity[0] = (1.0, 2.0)
        data[vel_dx_offset..vel_dx_offset + 4].copy_from_slice(&1.0f32.to_le_bytes());
        data[vel_dy_offset..vel_dy_offset + 4].copy_from_slice(&2.0f32.to_le_bytes());

        // Call tick
        tick.call(&mut store, ()).expect("tick failed");

        // Read back Position
        let data = memory.data(&store);
        let new_x = f32::from_le_bytes(data[pos_x_offset..pos_x_offset + 4].try_into().unwrap());
        let new_y = f32::from_le_bytes(data[pos_y_offset..pos_y_offset + 4].try_into().unwrap());

        assert!(
            (new_x - 11.0).abs() < 1e-6,
            "expected x=11.0, got {new_x}"
        );
        assert!(
            (new_y - 22.0).abs() < 1e-6,
            "expected y=22.0, got {new_y}"
        );
    }

    #[test]
    fn test_multiple_ticks() {
        let source = r#"
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
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let pos_x_offset = 4usize;
        let pos_y_offset = 4 + (MAX_ENTITIES as usize) * 4;
        let vel_dx_offset = 4 + (MAX_ENTITIES as usize) * 8;
        let vel_dy_offset = 4 + (MAX_ENTITIES as usize) * 12;

        // Set up 2 entities
        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&2i32.to_le_bytes());

        // Entity 0: pos(0,0), vel(3,4)
        data[pos_x_offset..pos_x_offset + 4].copy_from_slice(&0.0f32.to_le_bytes());
        data[pos_y_offset..pos_y_offset + 4].copy_from_slice(&0.0f32.to_le_bytes());
        data[vel_dx_offset..vel_dx_offset + 4].copy_from_slice(&3.0f32.to_le_bytes());
        data[vel_dy_offset..vel_dy_offset + 4].copy_from_slice(&4.0f32.to_le_bytes());

        // Entity 1: pos(100,200), vel(-1,-2)
        data[pos_x_offset + 4..pos_x_offset + 8].copy_from_slice(&100.0f32.to_le_bytes());
        data[pos_y_offset + 4..pos_y_offset + 8].copy_from_slice(&200.0f32.to_le_bytes());
        data[vel_dx_offset + 4..vel_dx_offset + 8].copy_from_slice(&(-1.0f32).to_le_bytes());
        data[vel_dy_offset + 4..vel_dy_offset + 8].copy_from_slice(&(-2.0f32).to_le_bytes());

        // Run 3 ticks
        for _ in 0..3 {
            tick.call(&mut store, ()).unwrap();
        }

        let data = memory.data(&store);
        let x0 = f32::from_le_bytes(data[pos_x_offset..pos_x_offset + 4].try_into().unwrap());
        let y0 = f32::from_le_bytes(data[pos_y_offset..pos_y_offset + 4].try_into().unwrap());
        let x1 = f32::from_le_bytes(data[pos_x_offset + 4..pos_x_offset + 8].try_into().unwrap());
        let y1 = f32::from_le_bytes(data[pos_y_offset + 4..pos_y_offset + 8].try_into().unwrap());

        assert!((x0 - 9.0).abs() < 1e-6, "entity 0 x: expected 9.0, got {x0}");
        assert!((y0 - 12.0).abs() < 1e-6, "entity 0 y: expected 12.0, got {y0}");
        assert!((x1 - 97.0).abs() < 1e-6, "entity 1 x: expected 97.0, got {x1}");
        assert!((y1 - 194.0).abs() < 1e-6, "entity 1 y: expected 194.0, got {y1}");
    }

    #[test]
    fn test_mixed_i32_f32_fields() {
        // Component with both i32 and f32 fields
        let source = r#"
component Stats {
    health: i32
    speed: f32
}

system Regen {
    query {
        write Stats
    }
    each(s: Stats) {
        s.health = s.health + 1
        s.speed = s.speed + 0.5
    }
}

world Game {
    systems {
        Regen
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let health_off = 4usize;
        let speed_off = 4 + (MAX_ENTITIES as usize) * 4;

        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        data[health_off..health_off + 4].copy_from_slice(&100i32.to_le_bytes());
        data[speed_off..speed_off + 4].copy_from_slice(&3.0f32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();
        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let health = i32::from_le_bytes(data[health_off..health_off + 4].try_into().unwrap());
        let speed = f32::from_le_bytes(data[speed_off..speed_off + 4].try_into().unwrap());

        assert_eq!(health, 102, "expected health=102, got {health}");
        assert!((speed - 4.0).abs() < 1e-6, "expected speed=4.0, got {speed}");
    }

    #[test]
    fn test_let_local_variable() {
        // Use let to store intermediate value
        let source = r#"
component Position {
    x: f32
    y: f32
}

component Velocity {
    dx: f32
    dy: f32
}

system ScaledMovement {
    query {
        write Position
        read Velocity
    }
    each(pos: Position, vel: Velocity) {
        let scale: f32 = 2.0
        pos.x = pos.x + vel.dx * scale
        pos.y = pos.y + vel.dy * scale
    }
}

world Game {
    systems {
        ScaledMovement
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let pos_x_off = 4usize;
        let pos_y_off = 4 + (MAX_ENTITIES as usize) * 4;
        let vel_dx_off = 4 + (MAX_ENTITIES as usize) * 8;
        let vel_dy_off = 4 + (MAX_ENTITIES as usize) * 12;

        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        data[pos_x_off..pos_x_off + 4].copy_from_slice(&10.0f32.to_le_bytes());
        data[pos_y_off..pos_y_off + 4].copy_from_slice(&20.0f32.to_le_bytes());
        data[vel_dx_off..vel_dx_off + 4].copy_from_slice(&3.0f32.to_le_bytes());
        data[vel_dy_off..vel_dy_off + 4].copy_from_slice(&5.0f32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let x = f32::from_le_bytes(data[pos_x_off..pos_x_off + 4].try_into().unwrap());
        let y = f32::from_le_bytes(data[pos_y_off..pos_y_off + 4].try_into().unwrap());

        // 10 + 3*2 = 16, 20 + 5*2 = 30
        assert!((x - 16.0).abs() < 1e-6, "expected x=16.0, got {x}");
        assert!((y - 30.0).abs() < 1e-6, "expected y=30.0, got {y}");
    }

    #[test]
    fn test_while_loop() {
        // Use while to add velocity multiple times within one tick
        let source = r#"
component Position {
    x: f32
    y: f32
}

component Counter {
    steps: i32
    unused: i32
}

system StepMovement {
    query {
        write Position
        write Counter
    }
    each(pos: Position, cnt: Counter) {
        let i: i32 = 0
        while i < cnt.steps {
            pos.x = pos.x + 1.0
            i = i + 1
        }
    }
}

world Game {
    systems {
        StepMovement
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let pos_x_off = 4usize;
        let pos_y_off = 4 + (MAX_ENTITIES as usize) * 4;
        let counter_steps_off = 4 + (MAX_ENTITIES as usize) * 8;
        let counter_unused_off = 4 + (MAX_ENTITIES as usize) * 12;

        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        data[pos_x_off..pos_x_off + 4].copy_from_slice(&0.0f32.to_le_bytes());
        data[pos_y_off..pos_y_off + 4].copy_from_slice(&0.0f32.to_le_bytes());
        data[counter_steps_off..counter_steps_off + 4].copy_from_slice(&5i32.to_le_bytes());
        data[counter_unused_off..counter_unused_off + 4].copy_from_slice(&0i32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let x = f32::from_le_bytes(data[pos_x_off..pos_x_off + 4].try_into().unwrap());

        assert!((x - 5.0).abs() < 1e-6, "expected x=5.0, got {x}");
    }

    #[test]
    fn test_type_mismatch_rejected() {
        // Assignment f32 = i32 should fail at sema
        let source = r#"
component Position {
    x: f32
    y: f32
}

system Bad {
    query {
        write Position
    }
    each(pos: Position) {
        pos.x = 42
    }
}

world Game {
    systems {
        Bad
    }
}
"#;
        let program = vivi_parser::parse(source).expect("parse failed");
        let result = vivi_sema::resolve(&program, source);
        assert!(result.is_err(), "expected type mismatch error");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("type mismatch"),
            "expected 'type mismatch' in error, got: {}",
            err.message
        );
    }

    #[test]
    fn test_fn_clamp_in_system() {
        let source = r#"
component Position {
    x: f32
    y: f32
}

component Velocity {
    dx: f32
    dy: f32
}

fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min { return min }
    if value > max { return max }
    return value
}

system ClampedMovement {
    query {
        write Position
        read Velocity
    }
    each(pos: Position, vel: Velocity) {
        pos.x = clamp(pos.x + vel.dx, 0.0, 50.0)
        pos.y = clamp(pos.y + vel.dy, 0.0, 50.0)
    }
}

world Game {
    systems {
        ClampedMovement
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let pos_x_off = 4usize;
        let pos_y_off = 4 + (MAX_ENTITIES as usize) * 4;
        let vel_dx_off = 4 + (MAX_ENTITIES as usize) * 8;
        let vel_dy_off = 4 + (MAX_ENTITIES as usize) * 12;

        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        // pos = (48, 5), vel = (10, -20)
        data[pos_x_off..pos_x_off + 4].copy_from_slice(&48.0f32.to_le_bytes());
        data[pos_y_off..pos_y_off + 4].copy_from_slice(&5.0f32.to_le_bytes());
        data[vel_dx_off..vel_dx_off + 4].copy_from_slice(&10.0f32.to_le_bytes());
        data[vel_dy_off..vel_dy_off + 4].copy_from_slice(&(-20.0f32).to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let x = f32::from_le_bytes(data[pos_x_off..pos_x_off + 4].try_into().unwrap());
        let y = f32::from_le_bytes(data[pos_y_off..pos_y_off + 4].try_into().unwrap());

        // 48+10=58 → clamped to 50, 5-20=-15 → clamped to 0
        assert!((x - 50.0).abs() < 1e-6, "expected x=50.0 (clamped), got {x}");
        assert!((y - 0.0).abs() < 1e-6, "expected y=0.0 (clamped), got {y}");
    }

    #[test]
    fn test_fn_call_arg_type_error() {
        let source = r#"
fn add_f32(a: f32, b: f32) -> f32 {
    return a + b
}

component Position {
    x: f32
    y: f32
}

system Bad {
    query { write Position }
    each(pos: Position) {
        pos.x = add_f32(pos.x, 1)
    }
}

world Game {
    systems { Bad }
}
"#;
        let program = vivi_parser::parse(source).expect("parse failed");
        let result = vivi_sema::resolve(&program, source);
        assert!(result.is_err(), "expected argument type error");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("expected `f32`"),
            "expected type error mentioning f32, got: {}",
            err.message
        );
    }

    #[test]
    fn test_fn_calling_fn() {
        // fn calling another fn
        let source = r#"
component Value {
    n: f32
    unused: f32
}

fn square(x: f32) -> f32 {
    return x * x
}

fn sum_of_squares(a: f32, b: f32) -> f32 {
    return square(a) + square(b)
}

system Compute {
    query { write Value }
    each(v: Value) {
        v.n = sum_of_squares(3.0, 4.0)
    }
}

world Game {
    systems { Compute }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let n_off = 4usize;

        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        data[n_off..n_off + 4].copy_from_slice(&0.0f32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let n = f32::from_le_bytes(data[n_off..n_off + 4].try_into().unwrap());

        // 3^2 + 4^2 = 9 + 16 = 25
        assert!((n - 25.0).abs() < 1e-6, "expected n=25.0, got {n}");
    }

    #[test]
    fn test_despawn() {
        // Spawn 4 entities, 2 with negative health.
        // A system despawns entities with health < 0.
        // After one tick, 2 entities should remain.
        let source = r#"
component Health {
    hp: i32
    unused: i32
}

system RemoveDead {
    query {
        read Health
    }
    each(h: Health) {
        if h.hp < 0 {
            despawn
        }
    }
}

world Game {
    systems {
        RemoveDead
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let hp_off = 4usize;
        let unused_off = 4 + (MAX_ENTITIES as usize) * 4;

        let data = memory.data_mut(&mut store);
        // Set entity_count = 4
        data[0..4].copy_from_slice(&4i32.to_le_bytes());

        // Entity 0: hp = 10 (alive)
        data[hp_off..hp_off + 4].copy_from_slice(&10i32.to_le_bytes());
        data[unused_off..unused_off + 4].copy_from_slice(&0i32.to_le_bytes());
        // Entity 1: hp = -5 (dead)
        data[hp_off + 4..hp_off + 8].copy_from_slice(&(-5i32).to_le_bytes());
        data[unused_off + 4..unused_off + 8].copy_from_slice(&0i32.to_le_bytes());
        // Entity 2: hp = 20 (alive)
        data[hp_off + 8..hp_off + 12].copy_from_slice(&20i32.to_le_bytes());
        data[unused_off + 8..unused_off + 12].copy_from_slice(&0i32.to_le_bytes());
        // Entity 3: hp = -1 (dead)
        data[hp_off + 12..hp_off + 16].copy_from_slice(&(-1i32).to_le_bytes());
        data[unused_off + 12..unused_off + 16].copy_from_slice(&0i32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let entity_count = i32::from_le_bytes(data[0..4].try_into().unwrap());
        assert_eq!(entity_count, 2, "expected 2 entities after despawn, got {entity_count}");

        // Verify remaining entities all have positive hp
        for i in 0..entity_count as usize {
            let hp = i32::from_le_bytes(
                data[hp_off + i * 4..hp_off + i * 4 + 4].try_into().unwrap(),
            );
            assert!(hp > 0, "entity {i} should have positive hp, got {hp}");
        }
    }

    #[test]
    fn test_global_variable() {
        // A global counter increments by 1 for each entity each tick.
        // With 3 entities and 2 ticks, the counter should be 6.
        let source = r#"
global counter: i32 = 0

component Tag {
    id: i32
    unused: i32
}

system Count {
    query {
        read Tag
    }
    each(t: Tag) {
        counter = counter + 1
    }
}

world Game {
    systems {
        Count
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        // Set up 3 entities
        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&3i32.to_le_bytes());

        // Run 2 ticks
        tick.call(&mut store, ()).unwrap();
        tick.call(&mut store, ()).unwrap();

        // The global counter is stored after all component data.
        // Components: Tag has 2 fields (id: i32, unused: i32)
        // Layout: [0..4] entity_count
        //         [4 .. 4 + MAX*4] Tag_id
        //         [4 + MAX*4 .. 4 + MAX*8] Tag_unused
        //         [4 + MAX*8 ..] globals
        let global_offset = 4 + (MAX_ENTITIES as usize) * 8;
        let data = memory.data(&store);
        let counter = i32::from_le_bytes(
            data[global_offset..global_offset + 4].try_into().unwrap(),
        );
        // 3 entities * 2 ticks = 6
        assert_eq!(counter, 6, "expected counter=6, got {counter}");
    }

    #[test]
    fn test_use_std_math() {
        // Use std.math's clamp function via the module system
        let source = r#"
use std.math

component Value {
    x: f32
    unused: f32
}

system ClampValues {
    query {
        write Value
    }
    each(v: Value) {
        v.x = clamp(v.x, 0.0, 100.0)
    }
}

world Game {
    systems {
        ClampValues
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let x_off = 4usize;

        // Set up 1 entity with x = 999.0 (should be clamped to 100.0)
        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        data[x_off..x_off + 4].copy_from_slice(&999.0f32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let x = f32::from_le_bytes(data[x_off..x_off + 4].try_into().unwrap());
        assert!(
            (x - 100.0).abs() < 1e-6,
            "expected x=100.0 (clamped), got {x}"
        );

        // Now set x = -50.0 (should be clamped to 0.0)
        let data = memory.data_mut(&mut store);
        data[x_off..x_off + 4].copy_from_slice(&(-50.0f32).to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let x = f32::from_le_bytes(data[x_off..x_off + 4].try_into().unwrap());
        assert!(
            (x - 0.0).abs() < 1e-6,
            "expected x=0.0 (clamped), got {x}"
        );
    }

    #[test]
    fn test_spawn_in_system() {
        // Spawn 3 entities in a bare init system, verify entity_count = 3 after init.
        let source = r#"
component Position {
    x: f32
    y: f32
}

system Setup {
    spawn {
        Position { x: 1.0, y: 2.0 }
    }
    spawn {
        Position { x: 3.0, y: 4.0 }
    }
    spawn {
        Position { x: 5.0, y: 6.0 }
    }
}

world Game {
    init {
        Setup
    }
    systems {
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let entity_count = i32::from_le_bytes(data[0..4].try_into().unwrap());
        assert_eq!(entity_count, 3, "expected 3 entities after init, got {entity_count}");

        // Verify spawned field values
        let pos_x_off = 4usize;
        let pos_y_off = 4 + (MAX_ENTITIES as usize) * 4;

        let x0 = f32::from_le_bytes(data[pos_x_off..pos_x_off + 4].try_into().unwrap());
        let y0 = f32::from_le_bytes(data[pos_y_off..pos_y_off + 4].try_into().unwrap());
        let x1 = f32::from_le_bytes(data[pos_x_off + 4..pos_x_off + 8].try_into().unwrap());
        let y1 = f32::from_le_bytes(data[pos_y_off + 4..pos_y_off + 8].try_into().unwrap());
        let x2 = f32::from_le_bytes(data[pos_x_off + 8..pos_x_off + 12].try_into().unwrap());
        let y2 = f32::from_le_bytes(data[pos_y_off + 8..pos_y_off + 12].try_into().unwrap());

        assert!((x0 - 1.0).abs() < 1e-6, "entity 0 x: expected 1.0, got {x0}");
        assert!((y0 - 2.0).abs() < 1e-6, "entity 0 y: expected 2.0, got {y0}");
        assert!((x1 - 3.0).abs() < 1e-6, "entity 1 x: expected 3.0, got {x1}");
        assert!((y1 - 4.0).abs() < 1e-6, "entity 1 y: expected 4.0, got {y1}");
        assert!((x2 - 5.0).abs() < 1e-6, "entity 2 x: expected 5.0, got {x2}");
        assert!((y2 - 6.0).abs() < 1e-6, "entity 2 y: expected 6.0, got {y2}");
    }

    #[test]
    fn test_bare_system() {
        // A bare system (no query/each) that increments a global counter.
        // Run 3 ticks, read the counter via a component field, verify it's 3.
        let source = r#"
global counter: i32 = 0

component Result {
    value: i32
    unused: i32
}

system Increment {
    counter = counter + 1
}

system CopyCounter {
    query { write Result }
    each(r: Result) {
        r.value = counter
    }
}

world Game {
    systems {
        Increment
        CopyCounter
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        // Set up 1 entity so CopyCounter has something to iterate
        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());

        // Run 3 ticks
        tick.call(&mut store, ()).unwrap();
        tick.call(&mut store, ()).unwrap();
        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let value_off = 4usize;
        let value = i32::from_le_bytes(data[value_off..value_off + 4].try_into().unwrap());
        assert_eq!(value, 3, "expected counter=3 after 3 ticks, got {value}");
    }

    #[test]
    fn test_init_block() {
        // World with init { Setup } and systems { Process }.
        // Setup spawns 2 entities, Process increments their values.
        let source = r#"
component Data {
    value: i32
    unused: i32
}

system Setup {
    spawn {
        Data { value: 10, unused: 0 }
    }
    spawn {
        Data { value: 20, unused: 0 }
    }
}

system Process {
    query { write Data }
    each(d: Data) {
        d.value = d.value + 5
    }
}

world Game {
    init {
        Setup
    }
    systems {
        Process
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        // After init, Setup should have spawned 2 entities
        let data = memory.data(&store);
        let entity_count = i32::from_le_bytes(data[0..4].try_into().unwrap());
        assert_eq!(entity_count, 2, "expected 2 entities after init, got {entity_count}");

        let value_off = 4usize;
        let v0 = i32::from_le_bytes(data[value_off..value_off + 4].try_into().unwrap());
        let v1 = i32::from_le_bytes(data[value_off + 4..value_off + 8].try_into().unwrap());
        assert_eq!(v0, 10, "entity 0 value after init: expected 10, got {v0}");
        assert_eq!(v1, 20, "entity 1 value after init: expected 20, got {v1}");

        // Run 1 tick: Process adds 5 to each
        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let v0 = i32::from_le_bytes(data[value_off..value_off + 4].try_into().unwrap());
        let v1 = i32::from_le_bytes(data[value_off + 4..value_off + 8].try_into().unwrap());
        assert_eq!(v0, 15, "entity 0 value after tick: expected 15, got {v0}");
        assert_eq!(v1, 25, "entity 1 value after tick: expected 25, got {v1}");
    }

    #[test]
    fn test_boolean_ops() {
        // Test `and`, `or`, `not` operators in conditions.
        let source = r#"
component Flags {
    a: bool
    b: bool
}

component Results {
    and_result: i32
    or_result: i32
    not_result: i32
}

system Logic {
    query {
        read Flags
        write Results
    }
    each(f: Flags, r: Results) {
        if f.a and f.b {
            r.and_result = 1
        } else {
            r.and_result = 0
        }
        if f.a or f.b {
            r.or_result = 1
        } else {
            r.or_result = 0
        }
        if not f.a {
            r.not_result = 1
        } else {
            r.not_result = 0
        }
    }
}

world Game {
    systems {
        Logic
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        // Layout: Flags(a: bool, b: bool), Results(and_result: i32, or_result: i32, not_result: i32)
        let flags_a_off = 4usize;
        let flags_b_off = 4 + (MAX_ENTITIES as usize) * 4;
        let results_and_off = 4 + (MAX_ENTITIES as usize) * 8;
        let results_or_off = 4 + (MAX_ENTITIES as usize) * 12;
        let results_not_off = 4 + (MAX_ENTITIES as usize) * 16;

        // Set up 2 entities:
        // Entity 0: a=true, b=false  -> and=0, or=1, not=0
        // Entity 1: a=true, b=true   -> and=1, or=1, not=0
        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&2i32.to_le_bytes());

        data[flags_a_off..flags_a_off + 4].copy_from_slice(&1i32.to_le_bytes());
        data[flags_b_off..flags_b_off + 4].copy_from_slice(&0i32.to_le_bytes());

        data[flags_a_off + 4..flags_a_off + 8].copy_from_slice(&1i32.to_le_bytes());
        data[flags_b_off + 4..flags_b_off + 8].copy_from_slice(&1i32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);

        // Entity 0: a=true, b=false -> and=0, or=1, not_a=0
        let and0 = i32::from_le_bytes(data[results_and_off..results_and_off + 4].try_into().unwrap());
        let or0 = i32::from_le_bytes(data[results_or_off..results_or_off + 4].try_into().unwrap());
        let not0 = i32::from_le_bytes(data[results_not_off..results_not_off + 4].try_into().unwrap());
        assert_eq!(and0, 0, "entity 0: true and false should be 0, got {and0}");
        assert_eq!(or0, 1, "entity 0: true or false should be 1, got {or0}");
        assert_eq!(not0, 0, "entity 0: not true should be 0, got {not0}");

        // Entity 1: a=true, b=true -> and=1, or=1, not_a=0
        let and1 = i32::from_le_bytes(data[results_and_off + 4..results_and_off + 8].try_into().unwrap());
        let or1 = i32::from_le_bytes(data[results_or_off + 4..results_or_off + 8].try_into().unwrap());
        let not1 = i32::from_le_bytes(data[results_not_off + 4..results_not_off + 8].try_into().unwrap());
        assert_eq!(and1, 1, "entity 1: true and true should be 1, got {and1}");
        assert_eq!(or1, 1, "entity 1: true or true should be 1, got {or1}");
        assert_eq!(not1, 0, "entity 1: not true should be 0, got {not1}");
    }

    #[test]
    fn test_despawn_rejected_in_bare_system() {
        // despawn in a bare system should be rejected by sema.
        let source = r#"
component Tag {
    id: i32
    unused: i32
}

system BadSystem {
    despawn
}

world Game {
    systems {
        BadSystem
    }
}
"#;
        let program = vivi_parser::parse(source).expect("parse failed");
        let result = vivi_sema::resolve(&program, source);
        assert!(result.is_err(), "expected sema error for despawn in bare system");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("each block"),
            "expected error about 'each block', got: {}",
            err.message
        );
    }

    #[test]
    fn test_type_cast() {
        // Test i32() and f32() cast built-in functions.
        // A system reads an f32 field, casts to i32, stores in i32 field,
        // then reads the i32 field, casts to f32, stores in another f32 field.
        let source = r#"
component Input {
    float_val: f32
    int_val: i32
}

component Output {
    truncated: i32
    converted: f32
}

system CastTest {
    query {
        read Input
        write Output
    }
    each(inp: Input, out: Output) {
        out.truncated = i32(inp.float_val)
        out.converted = f32(inp.int_val)
    }
}

world Game {
    systems {
        CastTest
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        // Layout: Input(float_val: f32, int_val: i32), Output(truncated: i32, converted: f32)
        let float_val_off = 4usize;
        let int_val_off = 4 + (MAX_ENTITIES as usize) * 4;
        let truncated_off = 4 + (MAX_ENTITIES as usize) * 8;
        let converted_off = 4 + (MAX_ENTITIES as usize) * 12;

        let data = memory.data_mut(&mut store);
        data[0..4].copy_from_slice(&1i32.to_le_bytes());
        data[float_val_off..float_val_off + 4].copy_from_slice(&3.75f32.to_le_bytes());
        data[int_val_off..int_val_off + 4].copy_from_slice(&42i32.to_le_bytes());

        tick.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let truncated = i32::from_le_bytes(data[truncated_off..truncated_off + 4].try_into().unwrap());
        let converted = f32::from_le_bytes(data[converted_off..converted_off + 4].try_into().unwrap());

        // i32(3.75) should truncate to 3
        assert_eq!(truncated, 3, "expected i32(3.75)=3, got {truncated}");
        // f32(42) should convert to 42.0
        assert!((converted - 42.0).abs() < 1e-6, "expected f32(42)=42.0, got {converted}");
    }

    #[test]
    fn test_for_loop() {
        // Test for loop: for i in 0..5 { } sets component values based on loop index.
        // A bare system uses a for loop to spawn 5 entities with incrementing x values.
        let source = r#"
component Position {
    x: f32
    y: f32
}

system Setup {
    for i in 0..5 {
        spawn {
            Position { x: f32(i), y: 0.0 }
        }
    }
}

world Game {
    init {
        Setup
    }
    systems {
    }
}
"#;
        let wasm_bytes = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let memory = instance.get_memory(&mut store, "memory").unwrap();

        init.call(&mut store, ()).unwrap();

        let data = memory.data(&store);
        let entity_count = i32::from_le_bytes(data[0..4].try_into().unwrap());
        assert_eq!(entity_count, 5, "expected 5 entities from for loop, got {entity_count}");

        let pos_x_off = 4usize;
        for i in 0..5 {
            let off = pos_x_off + i * 4;
            let x = f32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            assert!(
                (x - i as f32).abs() < 1e-6,
                "entity {i}: expected x={}.0, got {x}",
                i
            );
        }
    }

    #[test]
    fn test_modulo_operator() {
        let source = r#"
component Data {
    val: i32
    rem: i32
}

entity E1 {
    Data { val: 17, rem: 0 }
}

system Compute {
    query { write Data }
    each(d: Data) {
        d.rem = d.val % 5
    }
}

world Main {
    systems { Compute }
}
"#;
        let wasm = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        init.call(&mut store, ()).unwrap();
        tick.call(&mut store, ()).unwrap();

        let mem = instance.get_memory(&mut store, "memory").unwrap();
        let data = mem.data(&store);
        // 17 % 5 = 2
        let rem_offset = 4 + MAX_ENTITIES as usize * 4 + 0 * 4; // Data.rem field after Data.val
        let result = i32::from_le_bytes(data[rem_offset..rem_offset + 4].try_into().unwrap());
        assert_eq!(result, 2);
    }

    #[test]
    fn test_global_array() {
        let source = r#"
component Result {
    sum: i32
    third: i32
}

global values: [i32; 4] = [10, 20, 30, 40]

entity E1 {
    Result { sum: 0, third: 0 }
}

system Compute {
    query { write Result }
    each(r: Result) {
        r.sum = values[0] + values[1] + values[2] + values[3]
        r.third = values[2]
        values[0] = 99
    }
}

world Main {
    systems { Compute }
}
"#;
        let wasm = compile_vivi(source);
        let engine = Engine::default();
        let module = Module::new(&engine, &wasm).unwrap();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[]).unwrap();

        let init = instance.get_typed_func::<(), ()>(&mut store, "init").unwrap();
        let tick = instance.get_typed_func::<(), ()>(&mut store, "tick").unwrap();
        init.call(&mut store, ()).unwrap();
        tick.call(&mut store, ()).unwrap();

        let mem = instance.get_memory(&mut store, "memory").unwrap();
        let data = mem.data(&store);

        // Result.sum = 10 + 20 + 30 + 40 = 100
        let sum_offset = 4; // first field of first (only) component
        let sum = i32::from_le_bytes(data[sum_offset..sum_offset + 4].try_into().unwrap());
        assert_eq!(sum, 100);

        // Result.third = values[2] = 30
        let third_offset = 4 + MAX_ENTITIES as usize * 4;
        let third = i32::from_le_bytes(data[third_offset..third_offset + 4].try_into().unwrap());
        assert_eq!(third, 30);
    }
}
