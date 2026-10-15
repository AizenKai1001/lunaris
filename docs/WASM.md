# Sandboxed WebAssembly systems: your own code, in any language, with no trust required

Graph systems (`docs/GRAPH.md`) cover most gameplay. When you want **compiled code of your own inside the simulation**, you have two choices: a native plugin (`docs/PLUGINS.md`: full speed, but it can do anything a program can, so it is off unless the person running the engine switches it on) and a **WebAssembly module**: any language that compiles to WebAssembly (Rust, C, C++, Zig, AssemblyScript, Go/TinyGo, C#, ...) or the text format written by hand. A module runs inside a sandbox, so **no permission switch is needed**, and an AI agent may define one.

```json
{ "name": "seek", "wasm": "examples/wasm/swarm.wasm",
  "each": ["Transform", "Velocity"],
  "fields": ["Transform.pos[0]", "Transform.pos[2]", "Velocity.lin[0]", "Velocity.lin[2]"],
  "writes": ["Velocity.lin[0]", "Velocity.lin[2]"],
  "res": ["target.x", "target.z", "seek.k"], "fuel": 20000000 }
```

Every tick, for every entity that has the `each` components (and none of `without`), the engine collects the listed `fields` into arrays of `double` (256 entities at a time), calls the module's update function once per batch, and writes back the fields listed in `writes` (converted to each field's type; a non-finite number is refused and recorded as the system's error). It is shown in `systems` as `"kind": "wasm"` and runs in `order` with the graph systems. `.wasm` (binary) and `.wat` (text) files both work, from anywhere inside the workspace folder.

## What the sandbox guarantees

- **No access to anything.** A module may import only `lunaris.rand`. No files, no clock, no network, no host functions; any other import is refused when the system is defined, with the reason.
- **Fuel.** Each call gets an instruction budget (`fuel`, default 20 million, 1,000 to 4 billion). An endless loop ends with an error that says how many instructions it used; the world is left untouched.
- **Memory.** At most 64 MiB of linear memory; asking for more fails inside the module. Endless recursion ends in a trap.
- **Errors, not crashes.** A trap, an out-of-fuel, a non-zero result or a bad write is recorded as the system's `last_error` (visible in `systems` and `check`); the batch's writes are dropped.
- **Deterministic.** WebAssembly arithmetic is exactly defined (the engine runs it on an interpreter in its deterministic profile, NaNs canonicalised), `rand` comes from the engine, and **every tick starts a fresh instance**: no state survives from one tick to the next, so a recording replays exactly and a snapshot can be restored anywhere. The module file's hash is part of the state hash. The sample module and the equivalent graph system give **bit-identical** results on any thread count (`crates/lunaris/tests/wasm.rs`: 6,000 entities, 90 ticks, despawns included).

## The module's side of the contract (version 1)

The module exports its linear memory as `memory`, a function `lunaris_wasm_abi() -> i32` that returns 1, and the update function `lunaris_update(batch: i32) -> i32` (another name with `"export": "update_enemies"`, so one module can hold several systems). The result is 0 for success; anything else is the system's error. It may import `(import "lunaris" "rand" (func (param i32 i64) (result f64)))`: the engine's deterministic random number in [0, 1) for (seed, tick, the entity in this batch row, salt), the same as `{"rand": salt}` in a graph.

`batch` is an address in the module's memory where the engine laid out this call's data (little-endian):

| offset | type | field |
|---|---|---|
| 0 | u32 | `abi` (1) |
| 4 | u32 | `struct_size` (64) |
| 8 | u32 | `count`: entities in this batch (at most 256) |
| 12 | u32 | `nfields` |
| 16 | u32 | `nres` |
| 24 | u64 | `tick` |
| 32 | f64 | `dt` |
| 40 | u64 | `seed` |
| 48 | u32 | `uids`: address of `count` u64 |
| 52 | u32 | `fields`: address of `nfields` u32 addresses, each of `count` f64 (write the ones named in `writes`) |
| 56 | u32 | `res`: address of `nres` f64 (the resources listed in `res`, read once per tick) |
| 60 | u32 | `despawn`: address of `count` bytes, zero on entry; set one to 1 to delete that entity |

The engine grows the module's memory by the pages it needs for the batch, so the module has to allocate nothing.

## The sample (Rust, 900 bytes)

`examples/wasm/swarm` is a `no_std` Rust `cdylib` of about 60 lines that does what `examples/plugins/swarm_graph.json` does as a graph and `examples/plugins/swarm` as a native plugin, with the arithmetic in the same order:

```
python -I tools/build_wasm.py        # needs: rustup target add wasm32-unknown-unknown; writes examples/wasm/swarm.wasm
lunaris edit examples/orb_dash.lunaris.json
```

The built module is committed, so running the engine and its tests needs no WebAssembly toolchain. The same system written by hand in the text format (every number is an `f64` in the module's memory; this one sets the first field of every entity to 42):

```wat
(module
  (memory (export "memory") 1)
  (func (export "lunaris_wasm_abi") (result i32) i32.const 1)
  (func (export "lunaris_update") (param $b i32) (result i32)
    (local $arr i32) (local $i i32)
    (local.set $arr (i32.load (i32.load offset=52 (local.get $b))))          ;; address of field 0's array
    (block $done (loop $each
      (br_if $done (i32.ge_u (local.get $i) (i32.load offset=8 (local.get $b))))   ;; i >= count
      (f64.store (i32.add (local.get $arr) (i32.shl (local.get $i) (i32.const 3))) (f64.const 42))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $each)))
    i32.const 0))
```

## Speed

On 200,000 entities and one thread a graph system on the batch vm adds 7.4 ms per tick; the sandboxed module adds 32.5 ms (about 4.4 times: it runs on an interpreter, and the fields are copied into and out of its memory). Native plugins stay the fast road (about the speed of the vm, faster with `"parallel": true`). The sandbox is for code you do not trust or cannot compile into the engine, in the language you like, for thousands to tens of thousands of entities per tick.

## Rules and limits

- The module sees only the fields and resources the system lists (by index) and its own batch: no neighbour queries, no spawning. Despawn is the one structural change; for the rest use graph systems or drive the engine from outside.
- Modules are limited to 16 MB; fields and resources to 256 each.
- The interpreter's portable loop dispatch is used on purpose: the default dispatch grows the native stack with every loop iteration, which would make an endless loop a crash instead of an error (found by the test that runs one).
