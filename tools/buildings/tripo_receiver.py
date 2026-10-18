"""Receives Tripo exports from the built-in browser (tripo_export.js), which cannot download
files: POST /upload (multipart, field `file`) saves into a folder, GET serves it.

    python3 tools/buildings/tripo_receiver.py <folder> [port, default 8766]

One receiver per machine is enough; if the port is taken, one is already running (check
`lsof -iTCP:8766 -sTCP:LISTEN`) and saves into its own folder."""
import functools, http.server, os, sys
from email.parser import BytesParser
from email.policy import default

DIR = sys.argv[1]
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8766


class Handler(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Access-Control-Allow-Origin', '*')
        self.send_header('Access-Control-Allow-Private-Network', 'true')
        super().end_headers()

    def do_POST(self):
        body = self.rfile.read(int(self.headers['Content-Length']))
        head = b'Content-Type: ' + self.headers['Content-Type'].encode() + b'\r\n\r\n'
        saved = []
        for part in BytesParser(policy=default).parsebytes(head + body).iter_parts():
            if part.get_param('name', header='content-disposition') == 'file':
                name = os.path.basename(part.get_filename() or 'upload.bin')
                data = part.get_payload(decode=True)
                with open(os.path.join(DIR, name), 'wb') as f:
                    f.write(data)
                saved.append(f'{name} {len(data)}')
        self.send_response(200)
        self.send_header('Content-Type', 'text/plain')
        self.end_headers()
        self.wfile.write(('saved ' + ', '.join(saved)).encode())


os.makedirs(DIR, exist_ok=True)
http.server.ThreadingHTTPServer(('127.0.0.1', PORT), functools.partial(Handler, directory=DIR)).serve_forever()
