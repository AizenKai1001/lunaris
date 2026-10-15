"""Build the sample WebAssembly systems from Rust and copy them next to the examples.

    python -I tools/build_wasm.py        (needs the `wasm32-unknown-unknown` Rust target: rustup target add wasm32-unknown-unknown)

The built modules are committed (a few hundred bytes), so running the engine and its tests does not need the target.
"""
import os
import shutil
import subprocess

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
target = os.path.join(ROOT, "target", "wasm-swarm")
subprocess.run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "--manifest-path", os.path.join(ROOT, "examples", "wasm", "swarm", "Cargo.toml"), "--target-dir", target], check=True, cwd=ROOT)
built = os.path.join(target, "wasm32-unknown-unknown", "release", "swarm_wasm.wasm")
dest = os.path.join(ROOT, "examples", "wasm", "swarm.wasm")
shutil.copyfile(built, dest)
print("wrote", dest, os.path.getsize(dest), "bytes")
