"""Loopback-only browser file-drop fixture; no external uploads or dependencies."""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

root = Path(sys.argv[1])
page = b'''<!doctype html><meta charset="utf-8"><title>Seek owned drag upload check</title>
<style>body{font:18px Segoe UI;background:#fafafa;margin:20px}input{display:block;border:2px dashed #777;width:94%;height:120px;padding:10px;box-sizing:border-box}#zone{margin-top:30px;border:2px dashed #777;height:200px;padding:10px}pre{white-space:pre-wrap}</style>
<h2>Seek owned drag upload check</h2><p>Native file input</p><input id="upload" type="file" multiple>
<div id="zone" tabindex="0">Drop files here (HTML DataTransfer target)</div><pre id="result">No files received</pre>
<script>
async function record(kind, files) {
 let rows=[];
 for(const f of files){let bytes=await f.arrayBuffer();let hash=await crypto.subtle.digest('SHA-256',bytes);rows.push({name:f.name,size:f.size,sha256:Array.from(new Uint8Array(hash),b=>b.toString(16).padStart(2,'0')).join('')});}
 let value={kind,files:rows};document.querySelector('#result').textContent=JSON.stringify(value,null,2);
 await fetch('/record',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(value)});
}
upload.onchange=()=>record('input',upload.files);
zone.ondragover=e=>{e.preventDefault();e.dataTransfer.dropEffect='copy';};
zone.onpaste=e=>{e.preventDefault();record('paste',e.clipboardData.files);};
zone.ondrop=e=>{e.preventDefault();record('drop',e.dataTransfer.files);};
</script>'''


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.end_headers()
        self.wfile.write(page)

    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        if self.path != '/record' or length > 16384:
            self.send_error(400)
            return
        value = json.loads(self.rfile.read(length))
        with (root / 'browser-records.ndjson').open('a', encoding='utf-8') as out:
            out.write(json.dumps(value) + '\n')
        self.send_response(204)
        self.end_headers()

    def log_message(self, *_):
        pass


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
(root / 'server.json').write_text(json.dumps({'port': server.server_port}))
server.serve_forever()
