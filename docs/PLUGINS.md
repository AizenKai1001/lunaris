# Native plugins: your own code inside the tick

Graph systems (`docs/GRAPH.md`) cover most gameplay, and anything can drive the engine from outside (`docs/EMBEDDING.md`). When you want **your own compiled code running inside the simulation**, in the language you like, write a **plugin**: a shared library (`.dll`, `.so`, `.dylib`) with two exported C functions. A system names it:

```json
{ "name": "seek", "plugin": "target/plugin-swarm/release/swarm.dll", "parallel": true,
  "each": ["Transform", "Velocity"],
  "fields": ["Transform.pos[0]", "Transform.pos[2]", "Velocity.lin[0]", "Velocity.lin[2]"],
  "writes": ["Velocity.lin[0]", "Velocity.lin[2]"],
  "res": ["target.x", "target.z", "seek.k"] }
```

Every tick, for every entity that has the `each` components (and none of `without`), the engine collects the listed `fields` into plain arrays of `double` (256 entities at a time), calls the library once per batch, and writes back the fields listed in `writes` (converted to each field's own type; a non-finite number is refused and recorded as the system's error, like a graph `set`). The plugin never sees the world: only these arrays, `dt`, `tick`, `seed`, the values of the `res` resources, a deterministic `rand`, and a `despawn` flag per entity. It is shown in `systems` as `"kind": "plugin"` and runs in `order` with the graph systems.

## Safety first

Native code can do anything a program can, so:

- **Plugins are off.** Nothing loads unless the *host* allows it: `lunaris serve --allow-plugins` (or any command with that flag), the environment variable `LUNARIS_ALLOW_PLUGINS=1`, or `Engine::allow_plugins(true)` from Rust. No tool, MCP call or scene can switch it on; an agent that asks for a plugin gets an error that says how a person can allow it.
- The library must be **inside the workspace folder** (a relative path without `..`).
- It is loaded when the system is defined (so a bad path, a missing export or a wrong interface version is an error at that moment, with a hint). Redefining the system reloads it; a plugin stays loaded while any system uses it.
- A plugin that crashes takes the engine down with it. Test plugins on throwaway worlds.

## The interface

`crates/lunaris-sim/include/lunaris_plugin.h` is the whole contract (version 1):

```c
uint32_t lunaris_plugin_abi(void);              // return LUNARIS_PLUGIN_ABI (1)
int32_t  lunaris_update(LunarisBatch* batch);   // 0 = ok; anything else is recorded as the system's error (the batch's writes are dropped)
```

`LunarisBatch` carries `count` (at most 256), `nfields`, `nres`, `tick`, `dt`, `seed`, `uids[count]`, `fields[nfields][count]` (in the order of the system's `fields`), `res[nres]`, `despawn[count]` (zero on entry; set a byte to 1 to delete that entity) and `rand(batch, row, salt)`: the engine's deterministic per-entity random number in [0, 1), the same one `{"rand": salt}` gives in a graph. A different export name is allowed: `"export": "update_enemies"` (default `lunaris_update`), so one library can hold several systems.

## The rules that keep it reproducible

The engine's promises (same scene, seed and commands give the same state hash, on any machine and any thread count; recordings replay exactly) hold if the plugin is a **pure function of what it is given**: no clock, no threads, no random numbers of its own, no global or static state that survives between calls, no reading files. Use `rand` for randomness. Then a plugin and an equivalent graph system give **bit-identical results**; the test `crates/lunaris/tests/plugin.rs` proves it for the sample, over 6,000 entities, 90 ticks, despawns included, on 1, 4 and 8 threads.

`"parallel": true` lets the engine call the function from several threads at once, on different batches (needs reentrant code; the default is one thread). The library file's hash is part of the state hash: a recording made with one build of a plugin does not replay with another, which is what you want.

## The sample (Rust)

`examples/plugins/swarm` is a dependency-free Rust `cdylib` of about 60 lines. It does what `examples/plugins/swarm_graph.json` does as a graph (accelerate towards a target, jitter with `rand`, vanish on arrival), with the arithmetic in the same order:

```
cargo build --release --manifest-path examples/plugins/swarm/Cargo.toml
lunaris edit examples/orb_dash.lunaris.json --allow-plugins            # or serve / verify / run ...
```

The same interface works from C, C++, Zig, Go (cgo), C# NativeAOT and anything else that can export C functions; the header is all you need.

## Speed

For arithmetic this small the plugin is about as fast as the batch vm on one thread (gathering and scattering the fields dominates), and faster with `"parallel": true`: on 1,000,000 entities, about 4 ms per tick against about 7 ms for the vm, both on 8 threads. The point is not beating the vm on tiny graphs: it is that **anything** you can write in a compiled language (lookup tables, noise, pathfinding on your own data, SIMD) can run inside the deterministic tick. Plugins see only the entity's own fields; neighbour queries and structural changes other than `despawn` are for graph systems and for code driving the engine from outside.
