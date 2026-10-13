# Fixture for the browser-extension test: a page that sets a session cookie, and a file that
# needs it, served with Range support and a Content-Disposition name. Logs every request.
import http.server, sys

BODY = bytes((i * 7) % 251 for i in range(4_000_000))
open(sys.argv[1], "wb").write(BODY)  # the expected file, to compare against

class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def do_GET(self):
        cookie = self.headers.get("Cookie", "")
        print(f"{self.path} range={self.headers.get('Range')} cookie={'sid=s3cret' in cookie} "
              f"referer={self.headers.get('Referer')} ua={(self.headers.get('User-Agent') or '')[:30]}", flush=True)
        if self.path == "/":
            page = b'<html><body><a id="dl" href="/files/report.bin">Download the report</a></body></html>'
            self.send_response(200)
            self.send_header("Set-Cookie", "sid=s3cret; Path=/")
            self.send_header("Content-Type", "text/html")
            self.send_header("Content-Length", str(len(page)))
            self.end_headers()
            self.wfile.write(page)
            return
        if self.path != "/files/report.bin" or "sid=s3cret" not in cookie:
            self.send_response(403); self.send_header("Content-Length", "0"); self.end_headers(); return
        start, end = 0, len(BODY) - 1
        r = self.headers.get("Range")
        if r and r.startswith("bytes="):
            a, b = r[6:].split("-")
            start, end = int(a), (int(b) if b else len(BODY) - 1)
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{len(BODY)}")
        else:
            self.send_response(200)
        self.send_header("Content-Disposition", 'attachment; filename="Quarterly Report.bin"')
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("ETag", '"r1"')
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(end - start + 1))
        self.end_headers()
        try:
            self.wfile.write(BODY[start:end + 1])
        except (BrokenPipeError, ConnectionResetError):
            pass

http.server.ThreadingHTTPServer(("127.0.0.1", 8765), H).serve_forever()
