# Lunaris next to other engines

Only what can be compared fairly is compared here. A number is either **measured by us with a command in this repository** (and says so), or **quoted with its source and what it is**; nothing is estimated, and where there is no fair comparison the page says so. Dates are when the number was taken.

## Bevy (and the other Rust ECS crates)

Comparable today: the **entity-component-system** itself. The same workloads (those of the public `ecs_bench_suite`), the current crate versions, one timer, one thread, run by `tools/bench/ecs_compare` (commands and the adapters' source are in [PERFORMANCE.md](PERFORMANCE.md), section 2). On 2026-10-25 (result 3 there): iterating a million entities, Lunaris 1.26 ns an entity against `bevy_ecs` 0.15 at 2.16 and `hecs` at 1.47; creating 10 000 entities with four components, Lunaris 25.2 ns, `bevy_ecs` 29.6, `hecs` 30.3; adding and removing a component, Lunaris 53.6, `bevy_ecs` 81.1, `hecs` 45.5. Memory an entity costs with those four components (100 bytes of data): Lunaris 148 bytes, `bevy_ecs` 176, `hecs` 116.

Not comparable (and not claimed): the rest of Bevy. Bevy's renderer, UI, asset pipeline and scheduler are not measured against anything here, and the suite's scheduling, multi-threaded and serialization workloads are not ported yet. The adapters are written by the Lunaris side in the idiomatic way for each crate; a faster adapter for any of them is welcome.

## Godot

Comparable: the size of what you ship. Godot's own documentation gives no figure for its default export; one community write-up of Godot 4.4 (updated for 4.5) reports a default Windows export of **92.8 MB** and a default Web export of 42.0 MB, and about 21 MB for a Windows build compiled with size optimizations and unused modules removed ([Optimizing a build for size](https://docs.godotengine.org/en/stable/engine_details/development/compiling/optimizing_for_size.html) for the options, [the write-up](https://popcar.bearblog.dev/how-to-minify-godots-build-size/) for the numbers; one project, one machine). The Lunaris command-line program, stripped, is **20.3 MB** on Linux (measured 2026-10-25; it holds the tool API, MCP server, both renderers, physics, scripting and networking, and no editor). That is **not** a like-for-like comparison: different platforms, different feature sets, and a Godot export carries things this program does not. It says only that the order of magnitude is the same as a size-optimized Godot build.

Not compared: rendering quality, tooling, 2D, animation, the editor, everything else. Lunaris is much younger and much smaller in scope.

## Unity and Unreal Engine

No fair measurement exists yet, so no number is given. Both are closed-source commercial engines whose published figures come from their own marketing or from projects of different content; a comparison needs the same scene built in each, on the same machine, and that has not been done. The editor audit against Unreal's editor is the other piece (the Profiler and the Blueprint, UI and Script tabs follow Unreal's and VS Code's looks; see [EDITOR.md](EDITOR.md)).

## Particle systems (Niagara and Bevy Hanabi)

Compared here: **design**, not speed. Lunaris' `ParticleSystem` ([FX](FX.md)) follows the model Unreal's Niagara taught (emitters that are stacks of spawn, initialise, update and render; curves over a particle's life; events between emitters; budgets and level of detail), and, like Bevy's `bevy_hanabi`, can simulate on the card with a compute pass. What each says of itself, with the source, taken 2026-10-26:

| | Niagara (Unreal) | `bevy_hanabi` (Bevy) | Lunaris `ParticleSystem` |
|---|---|---|---|
| Where particles are simulated | CPU or GPU, chosen per emitter | GPU compute only ("a modern GPU-based particle system", [README](https://github.com/djeedai/bevy_hanabi)) | CPU in the picture (a server never does it), or the card per emitter (`simulate: gpu`); a renderer with no card uses the CPU for every emitter |
| Events between emitters (a death or a hit spawns particles) | **CPU simulation only**: Epic's event documentation says events do not work with GPU simulation ([Epic](https://dev.epicgames.com/documentation/unreal-engine/events-and-event-handlers-in-niagara-effects-for-unreal-engine)) | the README does not mention events or sub-emitters | CPU emitters only, the same constraint: an emitter on the card cannot send or receive events, and the format says so when you write it |
| Collision | planes and the world through the physics scene on the CPU; on the GPU against the depth buffer (third-party reports, not confirmed against Epic's text) | against plane, cube and sphere shapes and the depth buffer (README) | planes, balls and boxes of the effect; and the world's own colliders (boxes, balls, capsules, mesh colliders, terrain) by a ray for each step a particle takes, on the CPU; planes only on the card |
| Ribbons, light per particle, flipbooks, soft particles | yes (renderers of an emitter) | trails and ribbons listed; the rest not stated | ribbons, small solids, flipbooks (life, random, rate), a light for each of the brightest particles, soft particles, hard-edged `nearest` pictures: CPU emitters; sprites with flipbooks and soft particles on the card |
| How an effect is written | node-based editor, assets | Rust code (Expressions API) | a JSON file, checked with messages that say where a mistake is (`check`); no editor tab yet |

What is **not** compared, because nothing fair exists: particles per second in any of them. Epic and Hanabi publish no figure that says what hardware and content it was for. Ours are in [PERFORMANCE](PERFORMANCE.md): 19 ns to step a particle without forces and 73 ns with curl noise on one thread; 30 campfires (2,358 live particles) cost 0.44 ms a picture for the whole CPU side; and a mind of 12,000 motes costs 2.48 ms on the CPU and nothing the processor can measure on the card. Unity's VFX Graph and Godot's particles are not covered here at all (not read for this page).

## What would make this page better

The same scene (a few thousand lit meshes, physics, a character) built in each engine and timed on one machine with the same method; a CPU-only run on a 2-core machine; resident memory at start-up for each. Until then the honest summary is: Lunaris' ECS is competitive with the fastest Rust crates on these workloads, its start-up and memory for the core are small, and everything beyond the core is not measured against anyone.
