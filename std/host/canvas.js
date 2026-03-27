// Vivi Standard Library — Canvas API
// Provides: clear_screen, set_color, draw_rect, draw_circle

function clear_screen() {
    ctx.fillStyle = '#0a0a1a';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
}

function set_color(r, g, b) {
    currentColor = `rgb(${r},${g},${b})`;
}

function draw_rect(x, y, w, h) {
    ctx.fillStyle = currentColor;
    ctx.fillRect(x, y, w, h);
}

function draw_circle(x, y, r) {
    ctx.fillStyle = currentColor;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fill();
}
