"""Download a URL to a file with progress, resuming nothing, no tricks: python tools/realmap/dl.py URL OUTFILE"""
import os
import sys
import urllib.request

url, out = sys.argv[1], sys.argv[2]
req = urllib.request.Request(url, headers={"User-Agent": "lunaris-realmap/0.1"})
with urllib.request.urlopen(req, timeout=300) as r, open(out, "wb") as f:
    total = int(r.headers.get("Content-Length", 0))
    n = 0
    while True:
        b = r.read(1 << 20)
        if not b:
            break
        f.write(b)
        n += len(b)
        if n % (25 << 20) < (1 << 20):
            print(f"{n / 1e6:.0f} / {total / 1e6:.0f} MB", flush=True)
print("done", n / 1e6, "MB")
