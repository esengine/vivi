// Vivi Standard Library — Host Runtime Initialization
// This file is auto-included by the Vivi compiler for --target web builds.

const canvas = document.getElementById('vivi-canvas');
canvas.width = CANVAS_WIDTH;
canvas.height = CANVAS_HEIGHT;
const ctx = canvas.getContext('2d');

let currentColor = '#ffffff';
let mouseX = 0;
let mouseY = 0;

canvas.addEventListener('mousemove', (e) => {
    const rect = canvas.getBoundingClientRect();
    mouseX = e.clientX - rect.left;
    mouseY = e.clientY - rect.top;
});
