# Using Lunaris from any language

Everything the engine does is a **tool**: a name and a JSON object in, a JSON object out (`tools/list` or `docs/AI_GUIDE.md` lists them). There are three ways to reach the tools, and all of them run the same engine and give the same results (same seed and commands, same state hash):

| Way | Good for | How |
|---|---|---|
| **Child process** (`lunaris serve`) | Any language that can start a process and read lines. This is also **MCP**, so AI agents use it directly. | One JSON-RPC 2.0 object per line on stdin and stdout. Python and JavaScript clients are in `clients/`. See `docs/MCP.md`. |
| **Library in your process** (`lunaris_ffi`) | C, C++, C#, Go, Zig, Rust, Java, Lua, Python (ctypes) ... when you do not want a second process. | A C interface of five functions, below. |
| **Rust crates** | Rust programs. | Link `lunaris-api` and call `Engine::call(tool, &args)`. |

## The C interface

Build it with `cargo build --release -p lunaris-ffi`; you get `target/release/lunaris_ffi.dll` (`liblunaris_ffi.so` / `.dylib` elsewhere). The header is `crates/lunaris-ffi/include/lunaris.h`:

```c
LunarisEngine* lunaris_engine_new(uint64_t seed, const char* root_dir);
void           lunaris_engine_free(LunarisEngine* engine);
char*          lunaris_call(LunarisEngine* engine, const char* tool, const char* args_json);  // {"result": ...} or {"error": ...}
char*          lunaris_rpc(LunarisEngine* engine, const char* request_json);                   // any JSON-RPC / MCP message
void           lunaris_string_free(char* s);                                                   // give back every returned string
const char*    lunaris_version(void);
```

Rules: all text is UTF-8; every returned `char*` goes back to `lunaris_string_free` once; one engine can be shared between threads (calls take turns); nothing unwinds into your code. An error reply looks like `{"error": {"code": -32602, "message": "...", "data": {"code": "unknown_component", "path": "components.Transfrom", "hint": "did you mean `Transform`? ..."}}}`: `data.code` is the stable code to branch on, `data.hint` says how to fix the call.

A render comes back as base64 PNG in `result.image.data`; decode it and write a file (every client below does).

### Python (no child process)

```python
from lunaris import Embedded            # clients/python/lunaris.py, standard library only (ctypes)
with Embedded(root=".", seed=1) as eng:  # the same methods as Engine
    eng.spawn(components={"Transform": {}})
    print(eng.step(ticks=60)["hash"])
```

### C#

`examples/ffi/csharp` is a small console program (P/Invoke with `System.Text.Json`, nothing to install): it builds a floor, a ball and a camera, steps 3 seconds, reads where the ball came to rest, shows an error with its code and hint, and writes a picture. Build and run it with the .NET SDK (the library must exist first):

```
cargo build --release -p lunaris-ffi
dotnet run --project examples/ffi/csharp -c Release -- <path to the repository>
```

### Other languages

Any language with a C FFI works the same way: declare the five functions, pass UTF-8 strings, parse the JSON reply. For languages without one, use the child process: it is one JSON object per line.

## Your own code inside the tick

Compiled code in any language can run inside the simulation as a **native plugin** (`docs/PLUGINS.md`): a shared library with two exported C functions, named by a graph system, off unless the host allows it. A sandboxed WASM host (scripts that cannot crash the engine) is still on the roadmap; today logic is **graph systems** (JSON, `docs/GRAPH.md`), plugins, and anything driving the engine from outside through the tools above.
