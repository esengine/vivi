#!/usr/bin/env python3
import http.server
import os

class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        if self.path.endswith('.wasm'):
            self.send_header('SourceMap', self.path + '.map')
        self.send_header('Cache-Control', 'no-cache')
        super().end_headers()

    def guess_type(self, path):
        if path.endswith('.wasm'):
            return 'application/wasm'
        return super().guess_type(path)

print("http://localhost:8000")
http.server.HTTPServer(("", 8000), Handler).serve_forever()
