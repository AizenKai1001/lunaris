"""Register the Lunaris engine as an MCP server in Claude Code (user scope), so that any Claude Code session can build and look at games.

    python -I tools/install_mcp.py            builds the release binary, copies it to a `lunaris-mcp` folder next to the repository, registers `lunaris`
    python -I tools/install_mcp.py --update   only rebuilds and copies (after changing the engine; restart Claude Code sessions to reload)

The binary is copied away from `target/` because Windows will not let `cargo build` replace a program that is running, and a Claude Code
session keeps its MCP server running. The server uses the repository folder as its workspace (scenes, textures and models are read from
and saved to it) and draws pictures with the GPU renderer in high quality when the machine has a GPU (`quality: reference` for exact CPU
pictures).
"""
import os
import shutil
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
DEST_DIR = os.path.abspath(os.path.join(ROOT, "..", "lunaris-mcp"))
DEST = os.path.join(DEST_DIR, "lunaris.exe" if os.name == "nt" else "lunaris")
SRC = os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")

subprocess.run(["cargo", "build", "--release", "-p", "lunaris"], cwd=ROOT, check=True)
os.makedirs(DEST_DIR, exist_ok=True)
try:
    shutil.copy2(SRC, DEST)
    exe = DEST
except PermissionError:
    # a session is using the old copy: put the new one beside it; the next registration (or a restart) picks it up
    exe = os.path.join(DEST_DIR, "lunaris-new.exe")
    shutil.copy2(SRC, exe)
    print("the running copy is locked; wrote", exe, "(close Claude Code sessions that use lunaris, then rename it to lunaris.exe)")
print("binary:", exe)
if "--update" not in sys.argv:
    subprocess.run(["claude", "mcp", "remove", "lunaris", "--scope", "user"], capture_output=True, shell=(os.name == "nt"))
    p = subprocess.run(["claude", "mcp", "add", "--scope", "user", "lunaris", "--", exe, "serve", "--root", ROOT], capture_output=True, text=True, shell=(os.name == "nt"))
    print(p.stdout.strip() or p.stderr.strip())
    sys.exit(p.returncode)
