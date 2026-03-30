<p align="center">
  <h1 align="center">Vivi</h1>
  <p align="center">一门以 ECS 为语法的编程语言。</p>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="#快速开始">快速开始</a> · <a href="#语言概览">语言概览</a> · <a href="examples/">示例</a>
</p>

---

Vivi 编译到 WebAssembly。组件、系统、查询、世界是语言原语——不是库，不是宏，不是泛型。写 ECS 逻辑，得到一个 `.wasm`，浏览器和服务端都能跑。

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

比优化过的 JavaScript **快 4.6 倍**。80 万实体实时帧率。

## 快速开始

```bash
# 安装
cargo install --path crates/vivi-cli

# 编译到 WASM
vivi build game.vivi -o game.wasm

# 或生成完整 Web 应用（不写一行 JS）
vivi build game.vivi --target web -o dist/

# 或用解释器直接运行
vivi run game.vivi --ticks 10 --dump-state
```

## 语言概览

### 组件

附加到实体上的数据。没有方法，没有继承——只有字段。

```vivi
component Health {
    hp: i32
    max_hp: i32
}
```

类型：`i32`、`i64`、`f32`、`f64`、`bool`、`Entity`

### 系统

操作实体的逻辑。两种形式：

```vivi
// 查询系统——每个匹配实体执行一次
system Damage {
    query { write Health }
    each(h: Health) {
        h.hp = h.hp - 1
        if h.hp < 0 {
            despawn
        }
    }
}

// 裸系统——每 tick 执行一次
system GameTick {
    score = score + 1
}
```

### 创建与销毁

运行时创建和销毁实体。

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

`despawn` 在 `each` 循环中移除当前实体（交换删除）。

### 世界

声明哪些系统在初始化时运行，哪些每帧运行。

```vivi
world Game {
    init { Spawner }
    systems { Physics, Damage, GameTick }
}
```

### 函数

```vivi
fn clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min { return min }
    if value > max { return max }
    return value
}
```

### 全局变量

跨 tick 持久化的状态。

```vivi
global score: i32 = 0
global gravity: f32 = 9.8
```

### 外部函数

导入宿主函数——仅用于 WASM 物理上做不到的事（随机数、时间、I/O）。

```vivi
extern host {
    fn random() -> f32
    fn get_time() -> f32
}
```

### 标准库

```vivi
use std.math     // clamp, wrap, lerp, min_f32, max_f32, abs_f32
use std.render   // set_color, draw_rect, clear_screen（缓冲渲染）
```

`std.render` 把绘制命令写到共享内存。JS 运行时读取后用 WebGL 渲染——tick 期间零 host 调用。

### 内存操作

直接 WASM 内存读写，用于高级场景。

```vivi
mem_store_f32(addr, 3.14)
let val: f32 = mem_load_f32(addr)
```

编译器提供 `__heap_base`——组件数据之后的第一个安全地址。

## Web 构建

```bash
vivi build game.vivi --target web -o dist/
```

生成 `app.wasm` + `runtime.js` + `index.html` + source map。不需要写 JavaScript。浏览器打开，F12 → Sources → 在 `.vivi` 文件上设断点。

## 性能

通过 wasmtime 测量（Release 模式，x86_64）：

| 实体数量 | 每 tick | 每实体 |
|---------|--------|--------|
| 1,000 | 937 ns | 0.94 ns |
| 10,000 | 7.91 μs | 0.79 ns |
| 100,000 | 302 μs | 3.0 ns |

纯计算比优化过的 JavaScript（SoA + TypedArrays）**快 4.6 倍**。

[宇宙 demo](examples/universe.vivi) 在浏览器中渲染 80 万颗 3D 透视投影星体。

## 架构

```
.vivi → 词法 → 语法 → AST → 语义 → 代码生成 → .wasm
                                  ↘ 解释器   → 直接执行
```

| Crate | 职责 |
|-------|------|
| `vivi-lexer` | 词法分析（[logos](https://github.com/maciejhirsz/logos)） |
| `vivi-parser` | 递归下降解析器 + `use` 解析 |
| `vivi-sema` | 类型检查、名称解析、SoA 布局 |
| `vivi-codegen` | WASM 生成 + source map（[wasm-encoder](https://github.com/bytecodealliance/wasm-tools)） |
| `vivi-interp` | 树遍历解释器 |
| `vivi-web` | Web 构建生成器 |
| `vivi-lsp` | 语言服务（跳转定义、悬停、补全） |

5,300 行 Rust。21 个测试。零 warning。

### 内存布局

WASM 线性内存中的 Struct-of-Arrays。缓存友好，SIMD 就绪。

```
[0]          entity_count: i32
[4..]        Position_x[MAX]  Position_y[MAX]  Velocity_dx[MAX]  ...
[heap_base]  自由内存（全局变量、渲染缓冲区、用户数据）
```

## 示例

| 文件 | 演示内容 |
|------|---------|
| [`movement.vivi`](examples/movement.vivi) | 最小 ECS |
| [`demo.vivi`](examples/demo.vivi) | Canvas 渲染 |
| [`galaxy.vivi`](examples/galaxy.vivi) | 5K 星体、引力、spawn |
| [`buffered-render.vivi`](examples/buffered-render.vivi) | 零拷贝渲染 |
| [`universe.vivi`](examples/universe.vivi) | 80 万 3D 星体 |

## 编辑器支持

**VS Code**：安装 `editors/vscode/` 下的扩展——语法高亮、跳转定义、悬停、自动补全。

```bash
# 编译语言服务器
cargo build --release -p vivi-lsp
```

## 路线图

- [x] 核心编译器（词法、语法、语义、代码生成、解释器）
- [x] ECS 原语（component、system、query、spawn、despawn）
- [x] 函数、extern、全局变量、内存操作
- [x] 模块系统、标准库（math、render）
- [x] Web 构建、source map、WebGL、LSP
- [ ] 分块无限世界
- [ ] WASI 支持
- [ ] 包管理器

## 许可证

MIT
