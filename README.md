# Vivi

An ECS-oriented domain-specific language that compiles to WebAssembly.

Vivi makes ECS concepts — components, systems, queries, worlds — **language primitives**, not library abstractions. The syntax is minimal and consistent, designed for both humans and AI to read and generate.

## Example

```
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
```

## Features

- **ECS as language primitives** — `component`, `system`, `query`, `world` are keywords, not macros or generics
- **Compiles to WASM** — output runs in browsers, wasmtime, or any WASM runtime
- **Struct-of-Arrays memory layout** — components stored as SoA in WASM linear memory for cache-friendly iteration
- **Fast compilation** — full pipeline completes in ~15 microseconds
- **Fast runtime** — <1 ns per entity per system tick (10,000 entities in ~8 us)
- **Minimal syntax** — no semicolons, newline-separated, `and`/`or`/`not` for logic

## Build

Requires Rust 1.75+.

```bash
cargo build --release
```

## Usage

```bash
# Compile .vivi to .wasm
vivi build examples/movement.vivi -o output.wasm
```

The output WASM module exports:
- `init()` — initialize the ECS world
- `tick()` — execute all systems once
- `memory` — linear memory containing all component data

### Use in Browser

```javascript
const { instance } = await WebAssembly.instantiate(wasmBytes);
const { init, tick, memory } = instance.exports;

init();
// write entity data into memory.buffer ...
tick();
// read updated data from memory.buffer ...
```

## Architecture

```
Source (.vivi) → Lexer → Parser → AST → Sema → WASM Codegen → .wasm
```

| Crate | Role |
|-------|------|
| `vivi-lexer` | Tokenization ([logos](https://github.com/maciejhirsz/logos)) |
| `vivi-parser` | Recursive descent parser, AST definition |
| `vivi-sema` | Type checking, name resolution, SoA memory layout |
| `vivi-codegen` | WASM binary generation ([wasm-encoder](https://github.com/bytecodealliance/wasm-tools)) |
| `vivi-cli` | Command-line interface ([clap](https://github.com/clap-rs/clap)) |

### Memory Layout

Components use Struct-of-Arrays layout in WASM linear memory:

```
Offset 0:       entity_count (i32)
Offset 4:       Position_x[MAX_ENTITIES]   (f32[])
Offset 40004:   Position_y[MAX_ENTITIES]   (f32[])
Offset 80004:   Velocity_dx[MAX_ENTITIES]  (f32[])
Offset 120004:  Velocity_dy[MAX_ENTITIES]  (f32[])
```

`MAX_ENTITIES` = 10,000 (compile-time constant).

## Type System

| Type | Size | Description |
|------|------|-------------|
| `i32` | 4 bytes | 32-bit signed integer |
| `i64` | 8 bytes | 64-bit signed integer |
| `f32` | 4 bytes | 32-bit float |
| `f64` | 8 bytes | 64-bit float |
| `bool` | 4 bytes | boolean (stored as i32) |
| `Entity` | 4 bytes | opaque entity handle |

## Benchmarks

Measured with [criterion](https://github.com/bheisler/criterion.rs) via wasmtime (Release, x86_64):

| Entities | Time per tick | Per entity |
|----------|--------------|------------|
| 100 | 112 ns | 1.12 ns |
| 1,000 | 937 ns | 0.94 ns |
| 5,000 | 3.89 us | 0.78 ns |
| 10,000 | 7.91 us | 0.79 ns |

## Testing

```bash
# Unit tests (lexer, parser)
cargo test --workspace

# Integration tests (compile + run in wasmtime)
cargo test -p vivi-integration-tests

# Benchmarks
cargo bench -p vivi-integration-tests
```

## Roadmap

- [x] Phase 1 — Minimal prototype (component, system, query, world, arithmetic)
- [ ] Phase 2 — `entity` templates, `extern fn` (WASM imports), user-defined `fn`
- [ ] Phase 3 — Runtime entity spawn/destroy, conditional system execution
- [ ] Phase 4 — Scheduler, parallel system execution hints

## License

MIT
