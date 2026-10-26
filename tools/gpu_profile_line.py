"""Print the pass times that tools/build_gfx_coast.py (or any script printing `measured`) reported, one pass a line, biggest first. Reads stdin."""
import ast
import sys

for line in sys.stdin:
    if "'gpu_ms.total'" not in line:
        continue
    d = ast.literal_eval("{" + line.split("{", 1)[1].rsplit("}", 1)[0] + "}")
    rows = sorted(((k[7:], v) for k, v in d.items() if k.startswith(("gpu_ms.", "cpu_ms."))), key=lambda r: -r[1])
    print(line.split(" gpu:")[0], "ms (GPU passes and CPU phases):")
    for k, v in rows:
        print(f"  {k:<16}{v:7.2f}")
