# Connecting an AI agent to Lunaris

`lunaris serve` speaks the Model Context Protocol over stdin/stdout (newline-delimited JSON-RPC 2.0), so any MCP client can use it.

## Claude Code

```bash
cargo build --release -p lunaris
claude mcp add lunaris -- /path/to/lunaris/target/release/lunaris serve --root /path/to/workspace
```

`--root` is the only folder the file tools (`save_scene`, `load_scene`, `record_stop`, `replay`) can touch. `--seed N` sets the
simulation seed (default 1).

## A first session

1. `describe` - tick, entities, components, systems, snapshots, state hash. (`revision` is the cheap one: a counter that moves whenever anything may have changed the world, for noticing other clients' edits.)
2. `schema` - every component with its fields and types.
3. `define_component` - invent a component: `{"name":"Health","fields":[{"name":"hp","type":"f32","default":100}]}`.
4. `spawn` - `{"count":100,"components":{"Transform":{},"Velocity":{"lin":[1,0,0]},"Health":{}}}`.
5. `snapshot` `{"name":"before"}`, then `step` `{"ticks":600}`; the result carries the new `hash`.
6. `query` - `{"with":["Health"],"where":[{"component":"Health","field":"hp","op":"<","value":50}],"limit":20}`.
7. `restore` `{"name":"before"}` to try something else, `undo`/`redo` for edits.
8. `record_start` ... edits and steps ... `record_stop` `{"path":"runs/a.json"}`, then `replay` to prove the run is reproducible.
9. `save_scene` `{"path":"scenes/level.lunaris.json"}`; scenes are plain text.
10. `check` after building something: it lists what would otherwise be a blank picture or a value stuck at 0 (no camera, a drawn thing without a Transform, a missing model or texture, a tile map with the wrong number of tiles, graph systems that failed, resources nothing sets), each with a severity, the entity and a hint.

## Errors

A failed tool call returns `isError: true` with a JSON body:

```json
{ "code": "unknown_component", "message": "no component named `Transfrom`",
  "hint": "did you mean `Transform`? known components: Name, Transform, Velocity, Wander", "path": "Transfrom" }
```

`code` is stable and meant for programs; `hint` is meant for the agent.

## Direct JSON-RPC

Besides MCP, `lunaris.<tool>` methods return the bare result:

```json
{"jsonrpc":"2.0","id":1,"method":"lunaris.step","params":{"ticks":60}}
```

## Any language

The protocol is one JSON object per line on stdin/stdout (a line may be at most 64 MiB; a longer one is answered with error -32600 `request too large` and the next request is served normally, so put big data in a workspace file and pass its path), so any language that can start a process can drive the engine. Two thin clients are included, each about 100 lines with no dependencies, and both turn every tool in `tools/list` into a method:

```python
from lunaris import Engine                        # clients/python/lunaris.py
with Engine() as eng:
    eng.spawn(count=3, components={"Transform": {}, "Velocity": {"lin": [1, 0, 0]}})
    print(eng.step(ticks=60)["hash"])
    open("frame.png", "wb").write(eng.render(width=320, height=180)["png"])
```

```js
import { Engine } from './clients/js/lunaris.mjs';
const eng = await Engine.start();
await eng.spawn({ count: 3, components: { Transform: {} } });
console.log((await eng.step({ ticks: 60 })).hash);
await eng.close();
```

Failed calls raise `LunarisError` (Python) / reject with it (JavaScript), carrying the engine's `code`, `path` and `hint`. Tests: `python -I clients/python/test_lunaris.py` and `node --test clients/js/test.mjs`. For any other language, write a request line such as `{"jsonrpc":"2.0","id":1,"method":"lunaris.step","params":{"ticks":60}}` to the process and read one line back; a Rust program can link `lunaris-api` and call `Engine::call` directly, and any language with a C FFI can load the in-process library (`docs/EMBEDDING.md`). Running your own code *inside* the simulation (WASM) is on the roadmap.

## Tools

Run `lunaris tools` for the live list with descriptions, or call `tools/list` for the JSON Schemas.
