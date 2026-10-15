"""Every file path the documentation mentions in `backticks` must exist (so the docs cannot silently rot).

    python -I tools/check_docs.py
"""
import glob
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
docs = [os.path.join(ROOT, "README.md")] + sorted(glob.glob(os.path.join(ROOT, "docs", "*.md")))
pat = re.compile(r"`((?:docs|examples|tools|clients|crates)/[A-Za-z0-9_./\-]+?)`")
# shorthand that is not a path: example names without the extension, and the MCP method `tools/list`
SHORTHAND = {"examples/orb_dash", "examples/moon_hopper", "tools/list"}
missing = {}
for d in docs:
    text = open(d, encoding="utf-8").read()
    for m in pat.finditer(text):
        path = m.group(1).rstrip("/.,")
        if "*" in path or path in SHORTHAND:
            continue
        if not os.path.exists(os.path.join(ROOT, path)):
            missing.setdefault(path, set()).add(os.path.basename(d))
for p, where in sorted(missing.items()):
    print(f"MISSING {p}   (mentioned in {', '.join(sorted(where))})")
print(f"{len(missing)} missing path(s) in {len(docs)} documents")
sys.exit(1 if missing else 0)
