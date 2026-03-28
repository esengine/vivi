// Vivi Standard Library — Render Driver
// Reads draw commands written by std/vivi/render.vivi from shared memory.
// Auto-detects WebGL, falls back to Canvas 2D.
//
// To upgrade rendering: replace this file. No compiler changes needed.

const DRAW_BUF_COUNT_ADDR = 900000;
const DRAW_BUF_ADDR = 900004;

// ---- WebGL backend ----
let _gl = null;
let _glBuf = null;
let _vertData = null;

function _initWebGL() {
    _gl = canvas.getContext('webgl', { antialias: false });
    if (!_gl) return false;

    const vs = _gl.createShader(_gl.VERTEX_SHADER);
    _gl.shaderSource(vs, 'attribute vec2 p;attribute vec4 c;uniform vec2 r;varying vec4 v;void main(){vec2 s=p/r*2.0-1.0;gl_Position=vec4(s.x,-s.y,0,1);v=c;}');
    _gl.compileShader(vs);

    const fs = _gl.createShader(_gl.FRAGMENT_SHADER);
    _gl.shaderSource(fs, 'precision mediump float;varying vec4 v;void main(){gl_FragColor=v;}');
    _gl.compileShader(fs);

    const pg = _gl.createProgram();
    _gl.attachShader(pg, vs);
    _gl.attachShader(pg, fs);
    _gl.linkProgram(pg);
    _gl.useProgram(pg);
    _gl.uniform2f(_gl.getUniformLocation(pg, 'r'), canvas.width, canvas.height);

    _glBuf = _gl.createBuffer();
    _gl.bindBuffer(_gl.ARRAY_BUFFER, _glBuf);
    _gl.bufferData(_gl.ARRAY_BUFFER, 600000 * 6 * 4, _gl.DYNAMIC_DRAW);

    const ap = _gl.getAttribLocation(pg, 'p');
    const ac = _gl.getAttribLocation(pg, 'c');
    _gl.enableVertexAttribArray(ap);
    _gl.vertexAttribPointer(ap, 2, _gl.FLOAT, false, 24, 0);
    _gl.enableVertexAttribArray(ac);
    _gl.vertexAttribPointer(ac, 4, _gl.FLOAT, false, 24, 8);
    _gl.viewport(0, 0, canvas.width, canvas.height);

    _vertData = new Float32Array(600000 * 6);
    return true;
}

function _flushGL(mem) {
    const view = new DataView(mem.buffer);
    const count = view.getInt32(DRAW_BUF_COUNT_ADDR, true);
    let vc = 0;
    const d = _vertData;
    const cw = canvas.width, ch = canvas.height;

    for (let i = 0; i < count; i++) {
        const off = DRAW_BUF_ADDR + i * 32;
        const kind = view.getInt32(off + 28, true);
        let x, y, w, h, r, g, b;
        if (kind === 1) {
            x = 0; y = 0; w = cw; h = ch; r = 10/255; g = 10/255; b = 26/255;
        } else {
            x = view.getFloat32(off, true);
            y = view.getFloat32(off + 4, true);
            w = view.getFloat32(off + 8, true);
            h = view.getFloat32(off + 12, true);
            r = view.getInt32(off + 16, true) / 255;
            g = view.getInt32(off + 20, true) / 255;
            b = view.getInt32(off + 24, true) / 255;
        }
        const v = vc * 6;
        d[v]=x; d[v+1]=y; d[v+2]=r; d[v+3]=g; d[v+4]=b; d[v+5]=1;
        d[v+6]=x+w; d[v+7]=y; d[v+8]=r; d[v+9]=g; d[v+10]=b; d[v+11]=1;
        d[v+12]=x+w; d[v+13]=y+h; d[v+14]=r; d[v+15]=g; d[v+16]=b; d[v+17]=1;
        d[v+18]=x; d[v+19]=y; d[v+20]=r; d[v+21]=g; d[v+22]=b; d[v+23]=1;
        d[v+24]=x+w; d[v+25]=y+h; d[v+26]=r; d[v+27]=g; d[v+28]=b; d[v+29]=1;
        d[v+30]=x; d[v+31]=y+h; d[v+32]=r; d[v+33]=g; d[v+34]=b; d[v+35]=1;
        vc += 6;
    }
    if (vc > 0) {
        _gl.bufferSubData(_gl.ARRAY_BUFFER, 0, d.subarray(0, vc * 6));
        _gl.drawArrays(_gl.TRIANGLES, 0, vc);
    }
}

// ---- Canvas 2D fallback ----
function _flush2D(mem) {
    const view = new DataView(mem.buffer);
    const count = view.getInt32(DRAW_BUF_COUNT_ADDR, true);
    for (let i = 0; i < count; i++) {
        const off = DRAW_BUF_ADDR + i * 32;
        const kind = view.getInt32(off + 28, true);
        if (kind === 1) {
            ctx.fillStyle = '#0a0a1a';
            ctx.fillRect(0, 0, canvas.width, canvas.height);
        } else {
            ctx.fillStyle = `rgb(${view.getInt32(off+16,true)},${view.getInt32(off+20,true)},${view.getInt32(off+24,true)})`;
            ctx.fillRect(view.getFloat32(off,true), view.getFloat32(off+4,true), view.getFloat32(off+8,true), view.getFloat32(off+12,true));
        }
    }
}

// ---- Auto-select backend ----
const __vivi_flush_draws = _initWebGL() ? _flushGL : _flush2D;
