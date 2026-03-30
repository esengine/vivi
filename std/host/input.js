// Vivi Standard Library — Input Driver
// Syncs keyboard/mouse state into shared memory buffer.

// __INPUT_BUF_OFFSET is set by vivi-web before this file is included.
let __inputKeys = new Uint8Array(256);
let __mouseDown = 0;

document.addEventListener('keydown', (e) => {
    if (e.keyCode < 256) __inputKeys[e.keyCode] = 1;
});

document.addEventListener('keyup', (e) => {
    if (e.keyCode < 256) __inputKeys[e.keyCode] = 0;
});

canvas.addEventListener('mousedown', () => { __mouseDown = 1; });
canvas.addEventListener('mouseup', () => { __mouseDown = 0; });

function __vivi_sync_input(mem) {
    const view = new DataView(mem.buffer);
    const base = view.getInt32(__INPUT_BUF_OFFSET, true);
    view.setFloat32(base, mouseX, true);
    view.setFloat32(base + 4, mouseY, true);
    view.setInt32(base + 8, __mouseDown, true);
    for (let i = 0; i < 256; i++) {
        view.setInt32(base + 12 + i * 4, __inputKeys[i], true);
    }
}
