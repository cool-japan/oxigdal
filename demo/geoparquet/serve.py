#!/usr/bin/env python3
"""Static file server with HTTP Range support for the GeoParquet Live demo.

Stock ``python3 -m http.server`` ignores ``Range`` headers and answers
``200 OK`` with the whole body; the WASM fetch layer (correctly) rejects
that as a length mismatch.  This drop-in replacement serves single byte
ranges — including suffix ranges like ``bytes=-8`` — with ``206 Partial
Content`` and a proper ``Content-Range``, which is exactly what the
offline sample path (``?src=./sample/jpn-sample.parquet``) needs.

Usage:  python3 serve.py [port]        (default port 8080)
"""

import os
import re
import sys
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

RANGE_RE = re.compile(r"bytes=(\d*)-(\d*)$")


class RangeRequestHandler(SimpleHTTPRequestHandler):
    """SimpleHTTPRequestHandler plus single-range 206 responses."""

    protocol_version = "HTTP/1.1"

    def send_head(self):  # noqa: N802 (stdlib naming)
        self.range = None
        spec = self.headers.get("Range")
        if spec:
            m = RANGE_RE.match(spec.strip())
            if m and (m.group(1) or m.group(2)):
                self.range = (m.group(1), m.group(2))
        if self.range is None:
            return super().send_head()

        path = self.translate_path(self.path)
        if os.path.isdir(path) or not os.path.exists(path):
            return super().send_head()

        try:
            f = open(path, "rb")
        except OSError:
            self.send_error(404, "File not found")
            return None

        size = os.fstat(f.fileno()).st_size
        first_s, last_s = self.range
        if first_s == "":                      # suffix range: bytes=-N
            length = min(int(last_s), size)
            first, last = size - length, size - 1
        else:
            first = int(first_s)
            last = int(last_s) if last_s else size - 1
            last = min(last, size - 1)
        if first >= size or first > last:
            f.close()
            self.send_response(416, "Range Not Satisfiable")
            self.send_header("Content-Range", f"bytes */{size}")
            self.end_headers()
            return None

        self.send_response(206, "Partial Content")
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Range", f"bytes {first}-{last}/{size}")
        self.send_header("Content-Length", str(last - first + 1))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        f.seek(first)
        self.range_remaining = last - first + 1
        return f

    def copyfile(self, source, outputfile):
        if self.range is None:
            return super().copyfile(source, outputfile)
        remaining = self.range_remaining
        while remaining > 0:
            chunk = source.read(min(65536, remaining))
            if not chunk:
                break
            outputfile.write(chunk)
            remaining -= len(chunk)
        return None

    def end_headers(self):
        # SharedArrayBuffer-free demo: no COOP/COEP needed; just no-cache
        # so local edits show up immediately.
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
    handler = partial(RangeRequestHandler, directory=os.path.dirname(os.path.abspath(__file__)))
    server = ThreadingHTTPServer(("127.0.0.1", port), handler)
    print(f"GeoParquet Live demo: http://127.0.0.1:{port}/")
    print(f"Offline sample path:  http://127.0.0.1:{port}/?src=./sample/jpn-sample.parquet")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
