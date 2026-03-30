// Vivi Standard Library — Render Driver
// Reads draw commands from std/vivi/render.vivi shared memory buffer.
// Uses WebGL GL_POINTS for high performance, Canvas 2D fallback.

// __HEAP_BASE_OFFSET is set by vivi-web before this file is included.
// Read the actual heap_base value from WASM memory at that offset.
function __getDrawBufAddrs(mem) {
    const view = new DataView(mem.buffer);
    const heap_base = view.getInt32(__HEAP_BASE_OFFSET, true);
    return { countAddr: heap_base, bufAddr: heap_base + 4 };
}
let __drawAddrs = null;

// ---- WebGL backend (GL_POINTS — one vertex per star) ----
let _gl = null;
let _glPosData, _glSizeData, _glColData;
let _glPosBuf, _glSizeBuf, _glColBuf;

function _initWebGL() {
    _gl = canvas.getContext('webgl', { antialias: false });
    if (!_gl) return false;

    const vs = _gl.createShader(_gl.VERTEX_SHADER);
    _gl.shaderSource(vs, 'attribute vec2 p;attribute float s;attribute vec3 c;uniform vec2 r;varying vec3 v;void main(){vec2 n=p/r*2.0-1.0;gl_Position=vec4(n.x,-n.y,0,1);gl_PointSize=s;v=c;}');
    _gl.compileShader(vs);

    const fs = _gl.createShader(_gl.FRAGMENT_SHADER);
    _gl.shaderSource(fs, 'precision mediump float;varying vec3 v;void main(){vec2 d=gl_PointCoord-0.5;float a=1.0-smoothstep(0.3,0.5,length(d));gl_FragColor=vec4(v*a,a);}');
    _gl.compileShader(fs);

    const pg = _gl.createProgram();
    _gl.attachShader(pg, vs); _gl.attachShader(pg, fs);
    _gl.linkProgram(pg); _gl.useProgram(pg);
    _gl.uniform2f(_gl.getUniformLocation(pg, 'r'), canvas.width, canvas.height);

    _glPosBuf = _gl.createBuffer();
    _glSizeBuf = _gl.createBuffer();
    _glColBuf = _gl.createBuffer();

    const M = 800000;
    _glPosData = new Float32Array(M * 2);
    _glSizeData = new Float32Array(M);
    _glColData = new Float32Array(M * 3);

    _gl.enable(_gl.BLEND);
    _gl.blendFunc(_gl.SRC_ALPHA, _gl.ONE);
    _gl.viewport(0, 0, canvas.width, canvas.height);

    const ap = _gl.getAttribLocation(pg, 'p');
    const as2 = _gl.getAttribLocation(pg, 's');
    const ac = _gl.getAttribLocation(pg, 'c');

    _gl.bindBuffer(_gl.ARRAY_BUFFER, _glPosBuf);
    _gl.bufferData(_gl.ARRAY_BUFFER, _glPosData.byteLength, _gl.DYNAMIC_DRAW);
    _gl.enableVertexAttribArray(ap);
    _gl.vertexAttribPointer(ap, 2, _gl.FLOAT, false, 0, 0);

    _gl.bindBuffer(_gl.ARRAY_BUFFER, _glSizeBuf);
    _gl.bufferData(_gl.ARRAY_BUFFER, _glSizeData.byteLength, _gl.DYNAMIC_DRAW);
    _gl.enableVertexAttribArray(as2);
    _gl.vertexAttribPointer(as2, 1, _gl.FLOAT, false, 0, 0);

    _gl.bindBuffer(_gl.ARRAY_BUFFER, _glColBuf);
    _gl.bufferData(_gl.ARRAY_BUFFER, _glColData.byteLength, _gl.DYNAMIC_DRAW);
    _gl.enableVertexAttribArray(ac);
    _gl.vertexAttribPointer(ac, 3, _gl.FLOAT, false, 0, 0);

    return true;
}

function _flushGL(mem) {
    if (!__drawAddrs) __drawAddrs = __getDrawBufAddrs(mem);
    const view = new DataView(mem.buffer);
    const count = view.getInt32(__drawAddrs.countAddr, true);

    _gl.clearColor(0.005, 0.005, 0.02, 1);
    _gl.clear(_gl.COLOR_BUFFER_BIT);

    let n = 0;
    for (let i = 0; i < count; i++) {
        const off = __drawAddrs.bufAddr + i * 32;
        if (view.getInt32(off + 28, true) === 1) continue; // skip clear commands
        const x = view.getFloat32(off, true);
        const y = view.getFloat32(off + 4, true);
        const w = view.getFloat32(off + 8, true);
        const r = view.getInt32(off + 16, true);
        const g = view.getInt32(off + 20, true);
        const b = view.getInt32(off + 24, true);
        _glPosData[n * 2] = x;
        _glPosData[n * 2 + 1] = y;
        _glSizeData[n] = Math.max(w * 2, 1);
        _glColData[n * 3] = r / 255;
        _glColData[n * 3 + 1] = g / 255;
        _glColData[n * 3 + 2] = b / 255;
        n++;
    }

    if (n > 0) {
        _gl.bindBuffer(_gl.ARRAY_BUFFER, _glPosBuf);
        _gl.bufferSubData(_gl.ARRAY_BUFFER, 0, _glPosData.subarray(0, n * 2));
        _gl.bindBuffer(_gl.ARRAY_BUFFER, _glSizeBuf);
        _gl.bufferSubData(_gl.ARRAY_BUFFER, 0, _glSizeData.subarray(0, n));
        _gl.bindBuffer(_gl.ARRAY_BUFFER, _glColBuf);
        _gl.bufferSubData(_gl.ARRAY_BUFFER, 0, _glColData.subarray(0, n * 3));
        _gl.drawArrays(_gl.POINTS, 0, n);
    }
}

// ---- Canvas 2D fallback ----
function _flush2D(mem) {
    if (!__drawAddrs) __drawAddrs = __getDrawBufAddrs(mem);
    const view = new DataView(mem.buffer);
    const count = view.getInt32(__drawAddrs.countAddr, true);
    for (let i = 0; i < count; i++) {
        const off = __drawAddrs.bufAddr + i * 32;
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

const __vivi_flush_draws = _initWebGL() ? _flushGL : _flush2D;
