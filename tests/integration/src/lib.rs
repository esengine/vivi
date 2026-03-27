#[cfg(test)]
mod tests {
    use vivi_sema::layout::MAX_ENTITIES;
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
}
