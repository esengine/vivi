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

## 特性

- **ECS 作为语言原语** — `component`、`system`、`query`、`world`、`entity`、`spawn` 是关键字，不是宏或泛型
- **编译到 WASM** — 输出可在浏览器、wasmtime 或任何 WASM 运行时执行
- **完整 Web 目标** — `--target web` 生成完整的 Web 构建包（wasm + runtime.js + index.html + source map）
- **解释器模式** — `vivi run` 直接执行程序，无需编译到 WASM
- **Chrome DevTools 调试** — Source Map 支持让你在浏览器 Sources 面板中单步调试 `.vivi` 文件
- **WASM 名称段** — 调试器中显示真实函数名，而非数字索引
- **Struct-of-Arrays 内存布局** — 组件以 SoA 方式存储在 WASM 线性内存中，缓存友好
- **标准宿主 API** — 内置 canvas、debug、input、time、random 模块，通过 `extern` 块声明
- **比优化 JS 快 4.6 倍** — 纯计算基准测试（galaxy-bench）
- **编译极快** — 完整编译流水线约 15 微秒完成
- **极简语法** — 无分号，换行分隔，逻辑运算用 `and`/`or`/`not`

## 构建

需要 Rust 1.75+。

```bash
cargo build --release
```

## 使用

```bash
# 编译 .vivi 到 .wasm
vivi build input.vivi -o output.wasm

# 生成完整 Web 构建包（wasm + runtime.js + index.html + source map）
vivi build input.vivi --target web -o dist/

# 解释器模式运行
vivi run input.vivi --ticks 100 --dump-state

# 自定义实体容量（默认：10,000）
vivi build input.vivi -o output.wasm --max-entities 50000
```

输出的 WASM 模块导出：
- `init()` — 初始化 ECS 世界并执行 `world.init` 块
- `tick()` — 执行所有系统一次
- `memory` — 包含所有组件数据的线性内存

### Web 目标

使用 `--target web`，Vivi 生成自包含的 Web 应用。无需编写 JavaScript — 运行时自动生成：

```bash
vivi build examples/galaxy.vivi --target web -o dist/
# 在浏览器中打开 dist/index.html
```

生成的运行时负责 WASM 加载、游戏循环和所有标准宿主 API 绑定。

## 语言

### 关键字

| 构造 | 用途 |
|------|------|
| `component` | 定义带类型字段的组件类型 |
| `system` | 定义带 `query`/`each` 的系统，或裸系统（无查询） |
| `world` | 定义带 `init` 和 `systems` 块的世界 |
| `entity` | 声明静态实体模板 |
| `spawn` | 在运行时创建带组件值的实体 |
| `fn` | 自定义函数，支持参数和返回类型 |
| `extern` | 声明带模块名的宿主导入函数 |

### 语句

`let`、`if`/`else`、`while`、`return`、`spawn`、赋值（`=`）

### 表达式

算术（`+`、`-`、`*`、`/`、`%`）、比较（`==`、`!=`、`<`、`>`、`<=`、`>=`）、逻辑（`and`、`or`、`not`）、字段访问（`pos.x`）、函数调用（`sin(angle)`）

### 类型系统

| 类型 | 大小 | 说明 |
|------|------|------|
| `i32` | 4 字节 | 32 位有符号整数 |
| `i64` | 8 字节 | 64 位有符号整数 |
| `f32` | 4 字节 | 32 位浮点数 |
| `f64` | 8 字节 | 64 位浮点数 |
| `bool` | 4 字节 | 布尔值（存储为 i32） |
| `Entity` | 4 字节 | 不透明实体句柄 |

## 编译器架构

```
源码 (.vivi) --> 词法分析 --> 语法分析 --> AST --> 语义分析 --> 代码生成 --> .wasm
                                                         \-> 解释器  --> 直接执行
```

| Crate | 职责 |
|-------|------|
| `vivi-lexer` | 词法分析（[logos](https://github.com/maciejhirsz/logos)） |
| `vivi-parser` | 递归下降解析器，AST 定义 |
| `vivi-sema` | 类型检查、名称解析、SoA 内存布局计算 |
| `vivi-codegen` | WASM 二进制生成、Source Map、名称段（[wasm-encoder](https://github.com/bytecodealliance/wasm-tools)） |
| `vivi-interp` | 树遍历解释器，用于 `vivi run` |
| `vivi-web` | Web 目标生成器（runtime.js + index.html） |
| `vivi-cli` | 命令行接口（[clap](https://github.com/clap-rs/clap)） |
| `std/host/*.js` | 标准宿主 API 模块，通过 `build.rs` 自动嵌入 |

### 内存布局

组件使用 Struct-of-Arrays 布局存储在 WASM 线性内存中：

```
偏移 0:       entity_count (i32)
偏移 4:       Position_x[MAX_ENTITIES]   (f32[])
偏移 40004:   Position_y[MAX_ENTITIES]   (f32[])
偏移 80004:   Velocity_dx[MAX_ENTITIES]  (f32[])
偏移 120004:  Velocity_dy[MAX_ENTITIES]  (f32[])
```

`MAX_ENTITIES` = 10,000（默认值，可通过 `--max-entities` 配置）。

## 示例程序

| 文件 | 说明 |
|------|------|
| `examples/movement.vivi` | 基础 ECS — 位置 + 速度系统 |
| `examples/demo.vivi` | Canvas 渲染与实体 |
| `examples/galaxy.vivi` | 5,000 颗星体，重力模拟、spawn、裸系统 |
| `examples/galaxy-bench.vivi` | 纯计算基准测试（比 JS 快 4.6 倍） |

## 性能基准

使用 [criterion](https://github.com/bheisler/criterion.rs) 通过 wasmtime 测量（Release 模式，x86_64）：

| 实体数量 | 每 tick 耗时 | 每实体耗时 |
|---------|-------------|-----------|
| 100 | 112 ns | 1.12 ns |
| 1,000 | 937 ns | 0.94 ns |
| 5,000 | 3.89 us | 0.78 ns |
| 10,000 | 7.91 us | 0.79 ns |

Galaxy 基准测试（5,000 颗星体，重力 + 运动）：比等效优化 JavaScript **快 4.6 倍**。

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

- [x] Phase 1 — 核心编译器流水线（component、system、query、world、算术运算）
- [x] Phase 2 — `fn`、`extern`、`entity`、`spawn`、裸系统、`world init`
- [x] Phase 3 — 解释器、`--target web`、Source Map、标准宿主 API
- [ ] Phase 4 — WebGL 渲染、despawn、无限世界分块

## 许可证

MIT
