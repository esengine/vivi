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
    init {
        spawn Player {
            Position { x: 0.0, y: 0.0 }
            Velocity { dx: 1.0, dy: 0.5 }
        }
    }
    systems {
        Movement
    }
}
```

## Features

- **ECS as language primitives** — `component`, `system`, `query`, `world`, `entity`, `spawn` are keywords, not macros or generics
- **Compiles to WASM** — output runs in browsers, wasmtime, or any WASM runtime
- **Full web target** — `--target web` generates a complete bundle (wasm + runtime.js + index.html + source map)
- **Interpreter mode** — `vivi run` executes programs directly without compiling to WASM
- **Chrome DevTools debugging** — source maps let you step through `.vivi` files in the browser Sources panel
- **WASM name section** — real function names appear in the debugger, not numeric indices
- **Struct-of-Arrays memory layout** — components stored as SoA in WASM linear memory for cache-friendly iteration
- **Standard host API** — built-in canvas, debug, input, time, and random modules via `extern` blocks
- **4.6x faster than optimized JS** — in pure computation benchmarks (galaxy-bench)
- **Fast compilation** — full pipeline completes in ~15 microseconds
- **Minimal syntax** — no semicolons, newline-separated, `and`/`or`/`not` for logic

## Build

Requires Rust 1.75+.

```bash
cargo build --release
```

## Usage

```bash
# Compile .vivi to .wasm
vivi build input.vivi -o output.wasm

# Generate full web bundle (wasm + runtime.js + index.html + source map)
vivi build input.vivi --target web -o dist/

# Run in interpreter mode
vivi run input.vivi --ticks 100 --dump-state

# Custom entity capacity (default: 10,000)
vivi build input.vivi -o output.wasm --max-entities 50000
```

The output WASM module exports:
- `init()` — initialize the ECS world and execute `world.init` block
- `tick()` — execute all systems once
- `memory` — linear memory containing all component data

### Web Target

With `--target web`, Vivi generates a self-contained web application. No JavaScript to write — the runtime is auto-generated:

```bash
vivi build examples/galaxy.vivi --target web -o dist/
# Open dist/index.html in a browser
```

The generated runtime handles WASM loading, the game loop, and all standard host API bindings.

## Language

### Keywords

| Construct | Purpose |
|-----------|---------|
| `component` | Define a component type with typed fields |
| `system` | Define a system with `query`/`each` or as a bare system (no query) |
| `world` | Define a world with `init` and `systems` blocks |
| `entity` | Declare a static entity template |
| `spawn` | Create an entity at runtime with component values |
| `fn` | User-defined function with parameters and return type |
| `extern` | Declare imported host functions with module name |

### Statements

`let`, `if`/`else`, `while`, `return`, `spawn`, assignment (`=`)

### Expressions

Arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`==`, `!=`, `<`, `>`, `<=`, `>=`), logic (`and`, `or`, `not`), field access (`pos.x`), function calls (`sin(angle)`)

### Type System

| Type | Size | Description |
|------|------|-------------|
| `i32` | 4 bytes | 32-bit signed integer |
| `i64` | 8 bytes | 64-bit signed integer |
| `f32` | 4 bytes | 32-bit float |
| `f64` | 8 bytes | 64-bit float |
| `bool` | 4 bytes | boolean (stored as i32) |
| `Entity` | 4 bytes | opaque entity handle |

## Architecture

```
Source (.vivi) --> Lexer --> Parser --> AST --> Sema --> Codegen --> .wasm
                                                  \-> Interp  --> direct execution
```

| Crate | Role |
|-------|------|
| `vivi-lexer` | Tokenization ([logos](https://github.com/maciejhirsz/logos)) |
| `vivi-parser` | Recursive descent parser, AST definition |
| `vivi-sema` | Type checking, name resolution, SoA memory layout |
| `vivi-codegen` | WASM binary generation, source maps, name section ([wasm-encoder](https://github.com/bytecodealliance/wasm-tools)) |
| `vivi-interp` | Tree-walking interpreter for `vivi run` |
| `vivi-web` | Web target generator (runtime.js + index.html) |
| `vivi-cli` | Command-line interface ([clap](https://github.com/clap-rs/clap)) |
| `std/host/*.js` | Standard host API modules, auto-embedded via `build.rs` |

### Memory Layout

Components use Struct-of-Arrays layout in WASM linear memory:

```
Offset 0:       entity_count (i32)
Offset 4:       Position_x[MAX_ENTITIES]   (f32[])
Offset 40004:   Position_y[MAX_ENTITIES]   (f32[])
Offset 80004:   Velocity_dx[MAX_ENTITIES]  (f32[])
Offset 120004:  Velocity_dy[MAX_ENTITIES]  (f32[])
```

`MAX_ENTITIES` = 10,000 by default (configurable with `--max-entities`).

## Examples

| File | Description |
|------|-------------|
| `examples/movement.vivi` | Basic ECS — position + velocity system |
| `examples/demo.vivi` | Canvas rendering with entities |
| `examples/galaxy.vivi` | 5,000 stars with gravity, spawn, bare systems |
| `examples/galaxy-bench.vivi` | Pure computation benchmark (4.6x faster than JS) |

## Benchmarks

Measured with [criterion](https://github.com/bheisler/criterion.rs) via wasmtime (Release, x86_64):

| Entities | Time per tick | Per entity |
|----------|--------------|------------|
| 100 | 112 ns | 1.12 ns |
| 1,000 | 937 ns | 0.94 ns |
| 5,000 | 3.89 us | 0.78 ns |
| 10,000 | 7.91 us | 0.79 ns |

Galaxy benchmark (5,000 stars, gravity + movement): **4.6x faster** than equivalent optimized JavaScript.

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

- [x] Phase 1 — Core compiler pipeline (component, system, query, world, arithmetic)
- [x] Phase 2 — `fn`, `extern`, `entity`, `spawn`, bare systems, `world init`
- [x] Phase 3 — Interpreter, `--target web`, source maps, standard host API
- [ ] Phase 4 — WebGL rendering, despawn, infinite world chunks

## License

MIT
