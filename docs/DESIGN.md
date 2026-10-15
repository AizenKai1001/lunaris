# Lunaris Engine: design

## Goals, in priority order

1. **Operable by an AI agent as easily as by a person.** The agent should never need the source code to use the engine.
2. **Deterministic and reproducible.** Any bug an agent finds must be replayable bit for bit.
3. **As fast as the hardware allows** on the work that matters for the games we build (many entities, GPU-driven rendering).
4. **2D and 3D together**, one transform, one world, one renderer.
5. **Any language, plus visual nodes**, without making the node graph a second-class or opaque format.

Non-goals: matching Unreal's feature list, console certification, a marketplace. The aim is a better *workflow* and a faster core for a focused set of games.

## Architecture

```
 clients:   editor (nodes + code)   AI agent (MCP)   CLI / CI   Python
                         \               |             |          /
                          +--------  Command + query API  -------+     lunaris-api   (tools, JSON-RPC, MCP)
                                          |
                      +------------------ Sim ------------------+      lunaris-sim   (fixed step, systems, commands,
                      |  fixed-step loop, seeded RNG, systems,  |                      undo, snapshots, record/replay)
                      |  recording, snapshots by clone          |
                      +------------------ World ----------------+      lunaris-ecs   (archetypes, schemas, queries, scenes)
                                          |
                      renderer, physics, audio, assets, scripting host    (later milestones, all behind the same API)
```

### Decisions

**D1. Components are plain data with a schema.** A component is a flat record of scalars and fixed arrays
(`f32 f64 i32 u32 i64 u64 bool str uid`). Strings are interned (the component stores a `u32`); references to other entities
are stable uids. Consequences, all wanted: components copy with `memcpy`, snapshots are `memcpy`, hashing walks fields in
schema order, a script in any language can read the same bytes through a described layout, and an AI can define a new
component at runtime. Cost: no nested structs or `Vec`s inside components (use another entity or a table).

**D2. Two ways to name an entity.** Inside the engine an `Entity` is `(index, generation)` for speed. Outside (API, scenes,
recordings, component references) it is a stable `uid` (u64, never reused, assigned deterministically). The world keeps the map.

**D3. Archetype storage, type-erased columns.** Entities with the same component set share a table; each component is a
contiguous column. Iterating a query touches only the columns it needs. Adding/removing a component moves the entity to
another archetype (edges are cached). Spawning with a known component set writes directly into the final archetype.

**D4. The simulation is a pure function.** `state(t+1) = systems(state(t), commands(t), seed)`. Systems take `(world, ctx)`
and nothing else; all state lives in the world, so a simulation can be cloned (snapshot / fork), serialised (scene) and
replayed. Per-entity randomness is keyed by `(seed, tick, uid, salt)`, so results never depend on storage order.

**D5. One tool set, many transports.** `Engine::call(tool, json)` is the only way to do anything. MCP, JSON-RPC, the CLI and
the (future) editor are thin wrappers. A feature is not done until it is a tool with a schema and a hint-carrying error.

**D6. Everything is undoable or says why not.** Each command returns its inverse; batches are atomic (all or nothing, one
undo step). `define_component` is irreversible and says so.

**D7. Text scenes.** `{ "lunaris": 1, "components": [...], "entities": [...] }`, ordered by uid, so saving twice gives the same
bytes and `git diff` shows exactly what changed. Loading is atomic.

**D8. Zero dependencies for the core.** JSON, hashing, RNG and the ECS are in-tree, so the core can be audited and is fast
to build. Third-party crates are used at the edges only (graphics, windowing, audio, physics, image decode).

### Determinism contract

Guaranteed on one machine and across machines for the same binary: integer math, `+ - * / sqrt` on f32/f64 (IEEE-754,
no fast-math, no implicit FMA), the RNG, hashing, command order. Not guaranteed across platforms: libm functions
(`sin`, `cos`, `exp`, `pow`); simulation code uses the deterministic math module `lunaris_sim::detmath` (`sin cos tan atan atan2 exp ln`, built from `+ - * / sqrt` only; graph systems get them as `sin cos tan atan atan2 exp ln pow hypot`) and never libm. Systems must not
sum floats across entities in storage order.

## Status and roadmap

| Milestone | Content | State |
|---|---|---|
| M0 | JSON, ECS, simulation, commands/undo, record/replay, tool API, MCP server, CLI, bench | **done** |
| M1a | **Reference renderer** (deterministic CPU rasteriser): 2D sprites, sprite sheets, tile maps, 3D meshes, glTF models, HUD text, lights, textures, entity-id and depth buffers, PNG; parallel bands (identical picture for any thread count) | **done** |
| M1b | **GPU renderer** on wgpu (`lunaris-gpu`): a classic mode drawn from the same `Scene` as the reference and validated against it pixel by pixel, and a high-quality mode (PBR, procedural sky, cascaded shadows, AO, bloom, fog, MSAA, tone mapping); used by `lunaris play`, the `render` tool and the editor (`docs/GPU.md`) | **done** (Vulkan and DirectX 12 tested on this PC; Metal is in the build but untested) |
| M1c | **Graph systems**: logic as JSON graphs (`docs/GRAPH.md`), resources, input, events, contact systems | **done** |
| M2a | Deterministic parallel chunks for native and graph systems; `raycast`, `nearby`, `contacts`, `minimap` | **done** |
| M2b | **Batch vm** for graph systems (3.4x the interpreter, differential-tested against it) | **done** |
| M2c | Declared reads/writes so independent systems can run concurrently | |
| M3a | **Embedding**: the engine as a C library (`lunaris-ffi`), usable from C#, Python ctypes and any C FFI; `docs/EMBEDDING.md` | **done** |
| M3b | **Native plugins** inside the tick: shared libraries in any compiled language, batch interface, deterministic, host opt-in (`docs/PLUGINS.md`) | **done** (Rust sample, bit-identical to its graph twin) |
| M3c | **Sandboxed WebAssembly host** (`lunaris-wasm`, the `wasmi` interpreter, deterministic profile): any language, fuel budget, memory cap, no imports but `rand`, a fresh instance every tick; bit-identical to the graph and the native plugin in tests (`docs/WASM.md`) | **done** (Luau is not planned: graphs and WebAssembly cover it) |
| M4 | **Editor**: the Lunaris Editor, a native desktop application (egui over wgpu) with hierarchy, scene view, inspector, node view and node editing, play with sound; **native player** window | **done** (Windows) |
| M5a | Physics-lite (spheres, boxes, impulses, friction, triggers, contacts) | **done**; Jolt behind the same components later |
| M5c | **Character controller** (`CharacterBody`): a capsule moved by the engine after the graph systems (walk, jump, step height, slope limit, wall sliding, ground snapping), against every solid collider through a grid; deterministic, playtested (`examples/walker.lunaris.json`, `examples/tests/walker.test.json`) | **done** |
| M0b | **Cross-OS determinism verified**: all 19 example scenes give the same state hash after 600 ticks and the same CPU reference-picture hash on Windows (MSVC) and Linux (rustc 1.99); `tools/golden_hashes.txt` records them and `check_all` compares. The same exercise found one portability bug (the workspace path sandbox treated `C:/x` as relative on Linux), now one shared check, `lunaris_json::safe_relative` | **done** |
| M5d | **Navigation** (`NavAgent`, `path` tool): A* over a grid built from the colliders (stairs, platforms, drops, open floors), agents that follow paths and replan, deterministic and hashed (`docs/NAVIGATION.md`, `examples/maze.lunaris.json`) | **done** (no slopes from meshes, no agent avoidance) |
| M4c | **On-screen interface** (`Ui`): rectangles, bars, borders, labels and mouse buttons drawn by both renderers (GPU matches the reference), clicks as one-tick flags for graph systems (`docs/UI.md`, `examples/menu.lunaris.json`) | **done** (no layout, images or rounded corners yet) |
| M5b | Audio device mixing, asset baking and streaming, skeletal animation | partly (synthesised effects; no streaming, no skins) |
| M6a | **Lockstep multiplayer** (`lunaris-net`): a relay host and clients that exchange only inputs; every copy steps the same inputs in the same order, a start-up world hash and a state hash every 30 ticks catch any drift; `lunaris host`, `play --join`, `join`; two-player example `duel` (`docs/NETWORK.md`) | **done** (LAN or VPN; no encryption or per-player cameras; no reconnection in lockstep, see M6b) |
| M6b | **Rollback netcode** (`--rollback`): every copy plays at once with the guess "no change" for inputs that have not arrived, saves a `Sim::checkpoint` per tick (about 8 microseconds), goes back and plays again when a guess was wrong, never further than a host-announced window (8 ticks), compares state hashes only on settled ticks; a **lobby**, **spectators** and **rejoin by replay** of the host's input log (key changes only, memory-limited), an empty seat frozen after a grace period, a **latency simulator** (`LatencyProxy`); `docs/NETWORK.md` | **done** (up to 16 players; no host migration, no encryption). State replication for very large worlds and an authoritative server: not started |
| M4b | **One world for a person and an agent**: the `revision` counter exists; a live shared session between the native editor and an agent is **not built yet** (the browser version that had it was removed) | open |
| M5c | **Hierarchy** (`Parent` + `LocalTransform`, replay-exact), `MeshRenderer.emissive`, gameplay math (`atan2`, `pow`, `exp`, `ln`, `smoothstep` ... in `detmath`, shared by the interpreter and the vm) | **done** |
| M8 | **Playtests as data**: the `verify` tool and `lunaris verify` (steps, inputs, expectations, replay check; scratch engine) | **done** |
| M7 | **Minds**: language models play bodies and games through the same tools (`lunaris mind`, OpenAI-style client, permissions, replay without the model) | **done**; waiting for the user's OK to point it at a real model |

### Visual nodes

Graph systems are JSON; the editor draws them as nodes and edits them through right-click menus on the tree (a value feeding a pin
is the expression in that slot, so there is no free-form wiring to get out of sync). The back ends are the batch vm and the
interpreter; WASM and native back ends remain to come.

### Visual nodes (original sketch)

A node graph is a *projection* of a typed dataflow text format ("Lunaris IR"): each node has a stable id, a type, inputs and
outputs, and the file is ordinary text that diffs and merges. The editor draws it as nodes; an AI edits the text; both
compile to the same bytecode as code in other languages. This avoids the problems of opaque binary graph assets.

### Rendering

The reference renderer (`lunaris-render`) is a CPU rasteriser written for determinism: only IEEE `+ - * / sqrt` and `detmath`
are used, edge functions run in f64 (huge near-plane-clipped triangles need it), entities are drawn in uid order, and
shared edges follow a top-left rule so translucent neighbours never blend twice. Colours are lit and blended in a gamma-2.0
space and square-rooted on output. It gives agents and CI an image without a GPU or a window, and gives the GPU
renderer a ground truth.

The GPU renderer (`lunaris-gpu`, wgpu 30) draws the same `Scene` in two modes. *Classic* is the reference renderer's maths in
shaders (same working space, same lighting formula, nearest-neighbour textures) and is compared with it pixel by pixel in tests.
*High* is the one built to look good: reversed-Z depth, an HDR RGBA16F target with 4x MSAA, GGX/Smith/Schlick shading with a
procedural sky as the environment, three sun shadow cascades, a depth prepass feeding 16-sample ambient occlusion, a
13-tap bloom chain, per-sample filmic tone mapping (so bright edges stay anti-aliased), sRGB output with dithering. Steered by data:
the `Environment` component and each mesh's `metallic` / `roughness` / `texture` / `uv_scale` / `bump` (`docs/GPU.md`).
GPU pictures are not bit-identical between machines, so determinism-sensitive paths (hashes, replays, golden images) keep
using the reference renderer: the `render` tool's `quality: reference`.

### Rendering (original design sketch)

One renderer for 2D and 3D: sprites are instanced quads with a layer; 3D meshes are instanced and culled on the GPU. A render
graph orders passes. Headless is first-class: the `render` tool returns the frame as an image and a structured list of what
is visible (uids and screen rectangles), so an agent can verify what it built without guessing.
