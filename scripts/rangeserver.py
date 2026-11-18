#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Local fault-injection HTTP server for BeepBeep engine tests (plan §30-31).

Serves a deterministic random file at /file.bin with Range/ETag support and optional faults:
  --size MB         file size (default 64)
  --no-range        ignore Range headers (always 200 full body)
  --drop RATE       probability a response is cut off mid-stream (connection reset)
  --err RATE        probability of an HTTP 503 instead of data
  --stall RATE      probability a response stops sending mid-stream but keeps the socket open
  --rate KBPS       per-connection throughput cap
  --mutate-after N  change the file contents (and ETag) after N requests
Prints the SHA-256 of the served file on start.
"""
import argparse, hashlib, http.server, os, random, socket, socketserver, threading, time

ap = argparse.ArgumentParser()
ap.add_argument("--port", type=int, default=8765)
ap.add_argument("--size", type=int, default=64)
ap.add_argument("--no-range", action="store_true")
ap.add_argument("--drop", type=float, default=0.0)
ap.add_argument("--err", type=float, default=0.0)
ap.add_argument("--stall", type=float, default=0.0)
ap.add_argument("--rate", type=int, default=0)
ap.add_argument("--mutate-after", type=int, default=0)
args = ap.parse_args()

def make(seed):
    rnd = random.Random(seed)
    return rnd.randbytes(args.size * 1024 * 1024)

state = {"data": make(1), "etag": '"v1"', "requests": 0}
lock = threading.Lock()
print("sha256", hashlib.sha256(state["data"]).hexdigest(), flush=True)

class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def do_GET(self):
        with lock:
            state["requests"] += 1
            if args.mutate_after and state["requests"] == args.mutate_after:
                state["data"], state["etag"] = make(2), '"v2"'
                print("mutated; new sha256", hashlib.sha256(state["data"]).hexdigest(), flush=True)
            data, etag = state["data"], state["etag"]
        if random.random() < args.err:
            self.send_response(503); self.send_header("Content-Length", "0"); self.end_headers(); return
        total = len(data)
        rng = self.headers.get("Range")
        if_range = self.headers.get("If-Range")
        start, end, status = 0, total - 1, 200
        if rng and not args.no_range and (if_range is None or if_range == etag):
            a, b = rng.replace("bytes=", "").split("-")
            start = int(a); end = min(int(b) if b else total - 1, total - 1)
            if start >= total:
                self.send_response(416); self.send_header("Content-Range", f"bytes */{total}")
                self.send_header("Content-Length", "0"); self.end_headers(); return
            status = 206
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", 'attachment; filename="test-file.bin"')
        self.send_header("ETag", etag)
        if not args.no_range: self.send_header("Accept-Ranges", "bytes")
        if status == 206: self.send_header("Content-Range", f"bytes {start}-{end}/{total}")
        self.send_header("Content-Length", str(end - start + 1))
        self.end_headers()
        body = memoryview(data)[start:end + 1]
        cut = len(body) * random.uniform(0.1, 0.9) if random.random() < args.drop else None
        stall = len(body) * random.uniform(0.1, 0.9) if cut is None and random.random() < args.stall else None
        chunk = 64 * 1024
        sent = 0
        try:
            while sent < len(body):
                n = min(chunk, len(body) - sent)
                if cut is not None and sent + n > cut:
                    self.wfile.write(body[sent:int(cut)]); self.wfile.flush()
                    self.connection.shutdown(socket.SHUT_RDWR)  # a real reset, not a half-open socket
                    self.close_connection = True
                    return
                if stall is not None and sent + n > stall:
                    time.sleep(60)  # hold the socket open and send nothing
                    return
                self.wfile.write(body[sent:sent + n])
                sent += n
                if args.rate: time.sleep(n / (args.rate * 1024))
        except (BrokenPipeError, ConnectionResetError):
            pass

    do_HEAD = do_GET

class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True

S(("127.0.0.1", args.port), H).serve_forever()
