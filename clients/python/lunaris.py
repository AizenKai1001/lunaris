"""Drive a Lunaris engine from Python. Standard library only.

    from lunaris import Engine
    with Engine() as eng:                       # starts `lunaris serve` and talks JSON-RPC over its pipes
        eng.define_component(name="Health", fields=[{"name": "hp", "type": "f32", "default": 100}])
        eng.spawn(count=3, components={"Transform": {}, "Health": {}})
        print(eng.query(with_=["Health"])["total"])   # a trailing/leading underscore is removed: with_ -> "with"
        eng.step(ticks=60)
        frame = eng.render(width=320, height=180)     # frame["png"] holds the PNG bytes

Every tool the engine has is a method (`eng.tools` lists them with their descriptions). Failed calls raise `LunarisError`
with the engine's stable `code`, a `path` into your arguments and a `hint`. The executable is found from the argument,
the `LUNARIS_EXE` environment variable, or `target/release/lunaris(.exe)` next to this repository.
"""
from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from itertools import count
from pathlib import Path
from typing import Any

__all__ = ["Engine", "Embedded", "Remote", "connect", "LunarisError"]


class LunarisError(Exception):
    """A tool call the engine refused. `code` is stable and meant for programs, `hint` is meant for people and agents."""

    def __init__(self, message: str, code: str = "", path: str = "", hint: str = "", data: dict | None = None):
        super().__init__(message if not hint else f"{message} (hint: {hint})")
        self.message, self.code, self.path, self.hint, self.data = message, code, path, hint, data or {}


def _find_exe(exe: str | None) -> str:
    if exe:
        return exe
    if os.environ.get("LUNARIS_EXE"):
        return os.environ["LUNARIS_EXE"]
    repo = Path(__file__).resolve().parents[2]
    for name in ("lunaris.exe", "lunaris"):
        p = repo / "target" / "release" / name
        if p.exists():
            return str(p)
    raise FileNotFoundError("lunaris executable not found: build it (cargo build --release -p lunaris) or set LUNARIS_EXE")


class Engine:
    def __init__(self, exe: str | None = None, root: str | os.PathLike = ".", seed: int = 1, threads: int | None = None):
        cmd = [_find_exe(exe), "serve", "--root", str(root), "--seed", str(seed)]
        if threads:
            cmd += ["--threads", str(threads)]
        self._proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, encoding="utf-8", bufsize=1)
        self._ids = count(1)
        self.tools: dict[str, dict] = {}
        listing = self._rpc("tools/list", {})
        self.tools = {t["name"]: t for t in listing["tools"]}

    # -- transport
    def _rpc(self, method: str, params: dict) -> Any:
        if self._proc.poll() is not None:
            raise RuntimeError("the lunaris process has exited")
        rid = next(self._ids)
        self._proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}) + "\n")
        self._proc.stdin.flush()
        line = self._proc.stdout.readline()
        if not line:
            raise RuntimeError("the lunaris process closed its output")
        reply = json.loads(line)
        if "error" in reply:
            err = reply["error"]
            d = err.get("data") or {}
            raise LunarisError(d.get("message", err.get("message", "error")), d.get("code", ""), d.get("path", ""), d.get("hint", ""), d)
        return reply["result"]

    def call(self, tool: str, **args: Any) -> dict:
        """Run one tool. Argument names ending or starting with `_` lose it (so Python keywords like `with` work)."""
        clean = {k.strip("_"): v for k, v in args.items()}
        result = self._rpc("lunaris." + tool, clean)
        image = result.get("image") if isinstance(result, dict) else None
        if isinstance(image, dict) and "data" in image:
            result = dict(result)
            result["png"] = base64.b64decode(image.pop("data"))
        return result

    def __getattr__(self, name: str):
        if name.startswith("_") or name not in self.__dict__.get("tools", {}):
            raise AttributeError(f"no tool named `{name}` (tools: {', '.join(sorted(self.__dict__.get('tools', {})))})")
        return lambda **args: self.call(name, **args)

    # -- lifecycle
    def close(self) -> None:
        if self._proc.poll() is None:
            try:
                self._proc.stdin.close()
                self._proc.wait(timeout=5)
            except Exception:
                self._proc.kill()
        for pipe in (self._proc.stdin, self._proc.stdout):
            try:
                pipe.close()
            except Exception:
                pass

    def __enter__(self) -> "Engine":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()


def _find_ffi(lib: str | None) -> str:
    if lib:
        return lib
    if os.environ.get("LUNARIS_FFI"):
        return os.environ["LUNARIS_FFI"]
    repo = Path(__file__).resolve().parents[2]
    for name in ("lunaris_ffi.dll", "liblunaris_ffi.so", "liblunaris_ffi.dylib"):
        p = repo / "target" / "release" / name
        if p.exists():
            return str(p)
    raise FileNotFoundError("the lunaris_ffi library was not found: build it (cargo build --release -p lunaris-ffi) or set LUNARIS_FFI")


class Embedded(Engine):
    """The engine inside this Python process: no child process, no pipes. It loads the `lunaris_ffi` shared library with ctypes
    (the C interface in crates/lunaris-ffi/include/lunaris.h) and has exactly the methods of `Engine`."""

    def __init__(self, root: str | os.PathLike = ".", seed: int = 1, lib: str | None = None):
        import ctypes

        self._lib = ctypes.CDLL(_find_ffi(lib))
        self._lib.lunaris_engine_new.restype = ctypes.c_void_p
        self._lib.lunaris_engine_new.argtypes = [ctypes.c_uint64, ctypes.c_char_p]
        self._lib.lunaris_engine_free.restype = None
        self._lib.lunaris_engine_free.argtypes = [ctypes.c_void_p]
        self._lib.lunaris_rpc.restype = ctypes.c_void_p  # a raw pointer, so it can be given back to the library
        self._lib.lunaris_rpc.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self._lib.lunaris_string_free.restype = None
        self._lib.lunaris_string_free.argtypes = [ctypes.c_void_p]
        self._ctypes = ctypes
        self._handle = self._lib.lunaris_engine_new(seed, str(Path(root).resolve()).encode("utf-8"))
        if not self._handle:
            raise RuntimeError("lunaris_engine_new failed")
        self._ids = count(1)
        self.tools = {t["name"]: t for t in self._rpc("tools/list", {})["tools"]}

    def _rpc(self, method: str, params: dict) -> Any:
        if not self._handle:
            raise RuntimeError("the engine was closed")
        request = json.dumps({"jsonrpc": "2.0", "id": next(self._ids), "method": method, "params": params}).encode("utf-8")
        ptr = self._lib.lunaris_rpc(self._handle, request)
        if not ptr:
            raise RuntimeError("the engine gave no reply")
        try:
            reply = json.loads(self._ctypes.string_at(ptr).decode("utf-8"))
        finally:
            self._lib.lunaris_string_free(ptr)
        if "error" in reply:
            err = reply["error"]
            d = err.get("data") or {}
            raise LunarisError(d.get("message", err.get("message", "error")), d.get("code", ""), d.get("path", ""), d.get("hint", ""), d)
        return reply["result"]

    def close(self) -> None:
        if self._handle:
            self._lib.lunaris_engine_free(self._handle)
            self._handle = None

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass


class Remote(Engine):
    """Work in a world that is already running, next to the person using it: `lunaris edit scene.json --session-file s.json`
    starts the browser editor and writes the connection details to `s.json`; `connect("s.json")` then gives the same methods as
    `Engine`, and every edit you make shows up on the person's page (and theirs in your queries). Tool calls from the page and
    from you take turns, so nothing is lost. `close()` only drops the connection; it never stops the editor."""

    def __init__(self, session: str | os.PathLike | dict | None = None, *, rpc: str | None = None, token: str | None = None):
        if session is None and rpc is None:
            session = os.environ.get("LUNARIS_SESSION")
        if session is not None:
            info = session if isinstance(session, dict) else json.loads(Path(session).read_text(encoding="utf-8"))
            rpc, token = info["rpc"], info["token"]
        if not rpc or not token:
            raise ValueError("connect needs a session file (from `lunaris edit --session-file`), or rpc= and token=, or LUNARIS_SESSION")
        self._rpc_url, self._token = rpc, token
        self._opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # a local server: never through a proxy
        self._ids = count(1)
        self.tools = {t["name"]: t for t in self._rpc("tools/list", {})["tools"]}

    def _rpc(self, method: str, params: dict) -> Any:
        body = json.dumps({"jsonrpc": "2.0", "id": next(self._ids), "method": method, "params": params}).encode("utf-8")
        req = urllib.request.Request(self._rpc_url, data=body, headers={"Content-Type": "application/json", "X-Lunaris-Token": self._token})
        try:
            with self._opener.open(req, timeout=120) as resp:
                reply = json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            raise RuntimeError(f"the editor refused the request ({e.code}): {e.read().decode('utf-8', 'replace')}") from None
        except urllib.error.URLError as e:
            raise RuntimeError(f"cannot reach the editor at {self._rpc_url}: {e.reason}") from None
        if "error" in reply:
            err = reply["error"]
            d = err.get("data") or {}
            raise LunarisError(d.get("message", err.get("message", "error")), d.get("code", ""), d.get("path", ""), d.get("hint", ""), d)
        return reply["result"]

    def close(self) -> None:
        pass

    def __enter__(self) -> "Remote":
        return self


def connect(session: str | os.PathLike | dict | None = None, *, rpc: str | None = None, token: str | None = None) -> Remote:
    """Join a running editor's world (see `Remote`)."""
    return Remote(session, rpc=rpc, token=token)


if __name__ == "__main__":
    with Engine() as eng:
        print(f"{len(eng.tools)} tools; engine {eng.describe()['version']}", file=sys.stderr)
        for name, t in sorted(eng.tools.items()):
            print(f"{name:18} {t['description'].split('.')[0]}")
