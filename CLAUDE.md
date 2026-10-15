# Lunaris

A from-scratch Rust game engine (2D + 3D) that AI agents and people drive through the same tool API. Read `README.md` for what exists, `docs/DESIGN.md` for why.

## Two ways to be here

**Building a game with the engine** (you have the `lunaris` MCP tools): start with the `guide` tool, then `describe`, `schema`, `files`. The loop is build, `check`, `render`, `step`. Add an `Environment` entity and give meshes materials for a good-looking picture; `model_info` tells you what a model file contains (size, animation clips). `docs/AI_GUIDE.md` is the same guide in full; `examples/` has scenes to learn from (`vista`, `animation`, `orb_dash`, `moon_hopper`, `coin_chase`). The project skill `lunaris-game-builder` has the checklist.

**Working on the engine itself** (this repository):

```
cargo build --release -p lunaris              # the binary: target/release/lunaris.exe
cargo test --workspace                         # unit + integration tests (GPU tests skip themselves without a GPU)
python -I tools/check_all.py [--quick]         # everything: build, tests, MCP, clients, the editor window, playtests, bots, soak
python -I tools/install_mcp.py --update        # rebuild and refresh the copy the `lunaris` MCP server runs from (a `lunaris-mcp` folder next to the repository)
```

- Crates: `lunaris-json`, `lunaris-ecs`, `lunaris-sim` (core, no third-party dependencies), `lunaris-api` (the tools), `lunaris-render` (reference CPU renderer + `gather_scene`), `lunaris-gpu` (wgpu renderer: classic mode must match the reference pixel for pixel, high mode is for looks), `lunaris-mind`, `lunaris-wasm` (sandboxed WebAssembly host), `lunaris-net` (lockstep multiplayer: only inputs travel, every copy steps the same ticks), `lunaris-audio` (WAV, mixer, reverb, the acoustic ray tracer; sound is output only and never touches a hash; `lunaris_api::Director` drives it from a running engine), `lunaris-sim::nav` (pathfinding + `NavAgent`), `lunaris-sim::prefab` (entity templates), `lunaris-ffi`, `lunaris` (CLI, native player).
- Determinism is a feature: the simulation and the reference renderer use only IEEE `+ - * / sqrt` and `detmath`; per-entity randomness is keyed by `(seed, tick, uid, salt)`. The GPU renderer is allowed to differ between machines; nothing that hashes or replays may depend on it.
- Every tool is advertised in `tool_defs()` and dispatched in `Engine::call`; a test ties the two lists together. A new component is registered in `register_standard_components` and documented in its `component!` doc comment (that text is what `schema` shows).
- Shaders are WGSL in `crates/lunaris-gpu/src/shaders/`; change the Rust struct and the WGSL struct together (uniform layouts are checked only by the GPU tests).
- Docs live in `docs/`; `tools/check_docs.py` fails if a document mentions a file that does not exist.
- Commit locally with the no-reply identity and without a Co-Authored-By line; never push.
