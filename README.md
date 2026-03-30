<p align="center">
  <h1 align="center">Vivi</h1>
  <p align="center">A programming language where ECS is the syntax.</p>
</p>

<p align="center">
  <a href="README.zh-CN.md">中文</a> · <a href="#quick-start">Quick Start</a> · <a href="#language-tour">Language Tour</a> · <a href="examples/">Examples</a>
</p>

---

Vivi compiles to WebAssembly. Components, systems, queries, and worlds are language primitives — not libraries, not macros, not generics. Write ECS logic, get a `.wasm` that runs in browsers and servers.

```vivi
component Position { x: f32, y: f32 }
component Velocity { dx: f32, dy: f32 }

system Physics {
    query { write Position, read Velocity }
    each(pos: Position, vel: Velocity) {
        pos.x = pos.x + vel.dx
        pos.y = pos.y + vel.dy
    }
}
```

**4.6x faster** than equivalent optimized JavaScript. 800K entities at real-time frame rates.

## Quick Start

```bash
# Install
cargo install --path crates/vivi-cli

# Compile to WASM
vivi build game.vivi -o game.wasm

# Or generate a complete web app (zero JS to write)
vivi build game.vivi --target web -o dist/

# Or run directly with the interpreter
vivi run game.vivi --ticks 10 --dump-state
```

## Language Tour

### Components

Data attached to entities. No methods, no inheritance — just fields.

```vivi
component Health {
    hp: i32
    max_hp: i32
}
```

Types: `i32`, `i64`, `f32`, `f64`, `bool`, `Entity`

### Systems

Logic that operates on entities. Two forms:

```vivi
// Query system — runs once per matching entity
system Damage {
    query { write Health }
    each(h: Health) {
        h.hp = h.hp - 1
        if h.hp < 0 {
            despawn
        }
    }
}

// Bare system — runs once per tick
system GameTick {
    score = score + 1
}
```

### Spawn & Despawn

Create and destroy entities at runtime.

```vivi
system Spawner {
    let i: i32 = 0
    while i < 1000 {
        spawn {
            Position { x: random() * 800.0, y: random() * 600.0 }
            Health { hp: 100, max_hp: 100 }
        }
        i = i + 1
    }
}
```

`despawn` removes the current entity inside an `each` loop (swap-with-last).

### World

Declares which systems run at init and which run every tick.

```vivi
world Game {
    init { Spawner }
    systems { Physics, Damage, GameTick }
}
```

### Functions

```vivi
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min { return min }
    if value > max { return max }
    return value
}
```

### Globals

State that persists across ticks.

```vivi
global score: i32 = 0
global gravity: f32 = 9.8
```

### Extern

Import host functions — only for things WASM physically cannot do (random, time, I/O).

```vivi
extern host {
    fn random() -> f32
    fn get_time() -> f32
}
```

### Standard Library

```vivi
use std.math     // clamp, wrap, lerp, min_f32, max_f32, abs_f32
use std.render   // set_color, draw_rect, clear_screen (buffered rendering)
```

`std.render` writes draw commands to shared memory. The JS runtime reads them and renders with WebGL — zero host calls during tick.

### Memory Intrinsics

Direct WASM memory access for advanced use cases.

```vivi
mem_store_f32(addr, 3.14)
let val: f32 = mem_load_f32(addr)
```

The compiler provides `__heap_base` — first safe address after all component data.

## Web Target

```bash
vivi build game.vivi --target web -o dist/
```

Generates `app.wasm` + `runtime.js` + `index.html` + source map. No JavaScript to write. Open in browser, F12 → Sources → set breakpoints on `.vivi` files.

## Performance

Measured via wasmtime (Release, x86_64):

| Entities | Per tick | Per entity |
|----------|---------|------------|
| 1,000 | 937 ns | 0.94 ns |
| 10,000 | 7.91 μs | 0.79 ns |
| 100,000 | 302 μs | 3.0 ns |

**4.6x faster** than optimized JavaScript (SoA + TypedArrays) in pure computation.

The [universe demo](examples/universe.vivi) renders 800K stars with 3D perspective projection in the browser.

## Architecture

```
.vivi → Lexer → Parser → AST → Sema → Codegen → .wasm
                                    ↘ Interp  → direct execution
```

| Crate | Role |
|-------|------|
| `vivi-lexer` | Tokenization ([logos](https://github.com/maciejhirsz/logos)) |
| `vivi-parser` | Recursive descent parser + `use` resolution |
| `vivi-sema` | Type checking, name resolution, SoA layout |
| `vivi-codegen` | WASM generation + source maps ([wasm-encoder](https://github.com/bytecodealliance/wasm-tools)) |
| `vivi-interp` | Tree-walking interpreter |
| `vivi-web` | Web bundle generator |
| `vivi-lsp` | Language server (go-to-definition, hover, completion) |

5,300 lines of Rust. 21 tests. Zero warnings.

### Memory Layout

Struct-of-Arrays in WASM linear memory. Cache-friendly, SIMD-ready.

```
[0]          entity_count: i32
[4..]        Position_x[MAX]  Position_y[MAX]  Velocity_dx[MAX]  ...
[heap_base]  free memory (globals, render buffer, user data)
```

## Examples

| File | What it demonstrates |
|------|---------------------|
| [`movement.vivi`](examples/movement.vivi) | Minimal ECS |
| [`demo.vivi`](examples/demo.vivi) | Canvas rendering |
| [`galaxy.vivi`](examples/galaxy.vivi) | 5K stars, gravity, spawn |
| [`buffered-render.vivi`](examples/buffered-render.vivi) | Zero-copy rendering |
| [`universe.vivi`](examples/universe.vivi) | 800K 3D stars |

## Editor Support

**VS Code**: Install the extension from `editors/vscode/` — syntax highlighting, go-to-definition, hover, auto-completion.

```bash
# Build the language server
cargo build --release -p vivi-lsp
```

## Roadmap

- [x] Core compiler (lexer, parser, sema, codegen, interpreter)
- [x] ECS primitives (component, system, query, spawn, despawn)
- [x] Functions, extern, globals, memory intrinsics
- [x] Module system, standard library (math, render)
- [x] Web target, source maps, WebGL, LSP
- [ ] Chunk-based infinite worlds
- [ ] WASI support
- [ ] Package manager

## License

MIT
