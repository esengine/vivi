// Vivi Standard Library — Render Driver
// Reads draw commands written by std/vivi/render.vivi from shared memory.
// Paired with: std/vivi/render.vivi (Vivi side writes, this JS side reads)
//
// Auto-included when user code uses `use std.render`.

const DRAW_BUF_COUNT_ADDR = 900000;
const DRAW_BUF_ADDR = 900004;

function __vivi_flush_draws(mem) {
    const view = new DataView(mem.buffer);
    const count = view.getInt32(DRAW_BUF_COUNT_ADDR, true);
    for (let i = 0; i < count; i++) {
        const off = DRAW_BUF_ADDR + i * 32;
        const kind = view.getInt32(off + 28, true);
        if (kind === 1) {
            ctx.fillStyle = '#0a0a1a';
            ctx.fillRect(0, 0, canvas.width, canvas.height);
        } else {
            const x = view.getFloat32(off, true);
            const y = view.getFloat32(off + 4, true);
            const w = view.getFloat32(off + 8, true);
            const h = view.getFloat32(off + 12, true);
            const r = view.getInt32(off + 16, true);
            const g = view.getInt32(off + 20, true);
            const b = view.getInt32(off + 24, true);
            ctx.fillStyle = `rgb(${r},${g},${b})`;
            ctx.fillRect(x, y, w, h);
        }
    }
}
