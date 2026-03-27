#!/usr/bin/env python3
"""Simple HTTP server with correct MIME types for WASM source maps."""
import http.server
import socketserver

class WasmHandler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        '.wasm': 'application/wasm',
        '.map': 'application/json',
        '.vivi': 'text/plain',
    }

PORT = 8000
with socketserver.TCPServer(("", PORT), WasmHandler) as httpd:
    print(f"Serving at http://localhost:{PORT}")
    print("Open Chrome DevTools → Sources to debug .vivi files")
    httpd.serve_forever()
