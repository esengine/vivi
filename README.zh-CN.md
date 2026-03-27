# Vivi

面向 ECS 的领域特定语言，编译到 WebAssembly。

Vivi 将 ECS 概念——组件、系统、查询、世界——作为**语言原语**，而非库抽象。语法极简一致，为人类和 AI 共同设计。

## 示例

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

## 特性

- **ECS 作为语言原语** — `component`、`system`、`query`、`world` 是关键字，不是宏或泛型
- **编译到 WASM** — 输出可在浏览器、wasmtime 或任何 WASM 运行时执行
- **Struct-of-Arrays 内存布局** — 组件以 SoA 方式存储在 WASM 线性内存中，缓存友好
- **编译极快** — 完整编译流水线约 15 微秒完成
- **运行极快** — 每实体每 tick 不到 1 纳秒（10,000 实体约 8 微秒）
- **极简语法** — 无分号，换行分隔，逻辑运算用 `and`/`or`/`not`

## 构建

需要 Rust 1.75+。

```bash
cargo build --release
```

## 使用

```bash
# 编译 .vivi 到 .wasm
vivi build examples/movement.vivi -o output.wasm
```

输出的 WASM 模块导出：
- `init()` — 初始化 ECS 世界
- `tick()` — 执行所有系统一次
- `memory` — 包含所有组件数据的线性内存

### 在浏览器中使用

```javascript
const { instance } = await WebAssembly.instantiate(wasmBytes);
const { init, tick, memory } = instance.exports;

init();
// 向 memory.buffer 写入实体数据 ...
tick();
// 从 memory.buffer 读取更新后的数据 ...
```

## 编译器架构

```
源码 (.vivi) → 词法分析 → 语法分析 → AST → 语义分析 → WASM 代码生成 → .wasm
```

| Crate | 职责 |
|-------|------|
| `vivi-lexer` | 词法分析（[logos](https://github.com/maciejhirsz/logos)） |
| `vivi-parser` | 递归下降解析器，AST 定义 |
| `vivi-sema` | 类型检查、名称解析、SoA 内存布局计算 |
| `vivi-codegen` | WASM 二进制生成（[wasm-encoder](https://github.com/bytecodealliance/wasm-tools)） |
| `vivi-cli` | 命令行接口（[clap](https://github.com/clap-rs/clap)） |

### 内存布局

组件使用 Struct-of-Arrays 布局存储在 WASM 线性内存中：

```
偏移 0:       entity_count (i32)
偏移 4:       Position_x[MAX_ENTITIES]   (f32[])
偏移 40004:   Position_y[MAX_ENTITIES]   (f32[])
偏移 80004:   Velocity_dx[MAX_ENTITIES]  (f32[])
偏移 120004:  Velocity_dy[MAX_ENTITIES]  (f32[])
```

`MAX_ENTITIES` = 10,000（编译期常量）。

## 类型系统

| 类型 | 大小 | 说明 |
|------|------|------|
| `i32` | 4 字节 | 32 位有符号整数 |
| `i64` | 8 字节 | 64 位有符号整数 |
| `f32` | 4 字节 | 32 位浮点数 |
| `f64` | 8 字节 | 64 位浮点数 |
| `bool` | 4 字节 | 布尔值（存储为 i32） |
| `Entity` | 4 字节 | 不透明实体句柄 |

## 性能基准

使用 [criterion](https://github.com/bheisler/criterion.rs) 通过 wasmtime 测量（Release 模式，x86_64）：

| 实体数量 | 每 tick 耗时 | 每实体耗时 |
|---------|-------------|-----------|
| 100 | 112 ns | 1.12 ns |
| 1,000 | 937 ns | 0.94 ns |
| 5,000 | 3.89 us | 0.78 ns |
| 10,000 | 7.91 us | 0.79 ns |

## 测试

```bash
# 单元测试（词法分析器、解析器）
cargo test --workspace

# 集成测试（编译 + wasmtime 运行验证）
cargo test -p vivi-integration-tests

# 性能基准测试
cargo bench -p vivi-integration-tests
```

## 路线图

- [x] Phase 1 — 最小原型（component、system、query、world、算术运算）
- [ ] Phase 2 — `entity` 模板、`extern fn`（WASM imports）、自定义 `fn`
- [ ] Phase 3 — 运行时实体创建/销毁、条件系统执行
- [ ] Phase 4 — 调度器、并行系统执行提示

## 许可证

MIT
