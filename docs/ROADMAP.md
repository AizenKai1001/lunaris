# Lunaris roadmap (2026-10-24)

The direction, in the owner's words: Lunaris should be **the most optimized and fastest game engine there is**, in every way (a potato laptop must run it well), with everything drawn and watched **by the engine itself (no browser anywhere)**, with an editor that copies what is good in Unreal (UI, Blueprints) while staying simple and unique, and with **documentation, benchmarks and metrics** that compare it honestly to other engines.

Rules that go with it: engine work is committed often and on its own (never together with any game's files); the docs are game-neutral (the engine is open source); nothing here may depend on a browser; a claim of speed or realism only counts with a number or a check behind it.

## 1. Performance: the fastest engine

**Define the target first.** A "potato" is written down so it can be measured: 2 cores at about 2 GHz, 4 GB of RAM, an integrated GPU (or the software rasteriser as the floor). Budgets to hold on it: a 720p scene at 60 fps on the `medium` quality, a small scene's resident memory, start-up to first frame, scene load, the binary size.

**Measure before changing anything.**
* A `lunaris bench` command and `tools/bench/` scripts that print one table: ECS (spawn, iterate, query, add/remove component), the sim tick at 10k / 100k / 1M entities, physics, rays, navigation, scene load, particles, audio, CPU reference render, GPU frame time per quality and resolution, resident memory, start-up, binary size. Every run records the machine (cores, RAM, GPU) and the git commit.
* The potato run: the same table with the process pinned to 2 cores and a 4 GB memory limit, once with the GPU renderer and once with the software adapter.
* Other engines, only where a fair comparison exists: the public Rust `ecs_bench_suite` (bevy_ecs, hecs, legion, shipyard, specs) gets a Lunaris adapter; published numbers are quoted with their source and date; nothing is estimated.

**Then optimize by the profile, never by guess.** Candidates, in the order the numbers will probably justify: cache-friendly component layout and iteration, SIMD in the hot loops, the job scheduler (chunk size, thread count by core count), allocation-free ticks, GPU-driven culling and indirect draws, instancing, occlusion culling, texture streaming and compression (BC already exists), pipeline and shader caches, a "potato mode" with dynamic resolution and an automatic quality ladder, lazy initialisation for start-up, a binary scene format for load time, power use when idle. A performance gate in CI fails a change that makes a benchmark slower than its stored baseline.

Output: `docs/PERFORMANCE.md` (method, how to reproduce, results tables, comparisons, history).

## 2. The dashboard, drawn by the engine

The Linux test machine's dashboard is replaced by a Lunaris program (a scene plus small data feeders: CPU, GPU, memory, disks, services, containers) drawn with the engine's UI kit by the engine's renderer in a native window: no browser, no Conky. Steps: a feeder that publishes the machine's metrics as engine resources, the dashboard scene, a player that runs it full screen on a Linux test machine's display, then the existing panels one by one until the old dashboard can be switched off.

## 3. The editor: what to take from Unreal, in Lunaris' own look

**The audit table is written: [EDITOR_AUDIT](EDITOR_AUDIT.md) (2026-10-25), with the gaps ranked.** It covers (Unreal editor part, Lunaris equivalent today, gap): outliner, details panel, content browser, viewport gizmos and snapping, play-in-editor, undo and redo, the Blueprint graph (exists: Blueprint tab), scripting (exists: Script tab), the UI designer (exists: UI tab), material editor, animation and sequencer, particle editor, the profiler view, project settings. Gaps become tasks, each built with the same rule: simple first, optional depth after. Images the editor needs come from GPT image generation, never drawn by hand.

## 4. Documentation, comparison and community

An index of the docs, one honest comparison page per other engine (what can be compared, what cannot, the numbers and their sources), a getting-started that works from a clean checkout, and the changelog. A Discord server and posts on X are the owner's to open and publish; drafts are prepared, nothing is posted without him.

## 5. State on 2026-10-26

Done since the first version of this page: `lunaris bench` and the gate (`tools/perf_gate.py`, `tools/perf_baseline.json`), the potato run on a pinned 2-core machine, the comparison with the Rust ECS crates (`docs/PERFORMANCE.md`); the editor audit and its first gaps closed (Paint mode, snapping, the eye column, the Output Log: `docs/EDITOR_AUDIT.md`, `docs/EDITOR.md`); lighting and shadows for night scenes (`Lighting`, soft shadows, point-light shadows), macro variation for meshes (`MacroVary`); and a series of measured speed-ups found by timing inside a real city server and client rather than in a bench: the static-world gather, the nav cell cache (opt-in), the probe step, `despawn_trees`, the batching of a procedural grid's zone spawns, the characters' obstacle grid (reused and patched), the hidden-widget set (each with its numbers in `docs/PERFORMANCE.md`). The engine-drawn dashboard (section 2) ran on a Linux test machine for a day; its scene is not in this repository and drew the artificial-life culture, so it is switched off until it gets a new middle panel. GPU picture times are in the bench (`lunaris bench --gpu`, see PERFORMANCE). Not done: a run on real low-end hardware (and on the software adapter), GPU frame times of a window's frames (the bench measures stills), the material editor, dockable editor panels, the comparison pages for Unity and Unreal.


Particles (2026-10-26): `ParticleSystem`, a Niagara-style system (emitters, curves, forces, curl noise, collision with planes and the world's colliders, events, ribbons, solids, a light per particle, soft particles, a budget and level of detail, and emitters simulated on the card with the CPU as the fallback), with a JSON format that `check` validates, a library of effects (`examples/fx`, a voxel set in `examples/fx/voxel`) and the numbers in [PERFORMANCE](PERFORMANCE.md); [FX](FX.md) is the reference and [COMPARISON](COMPARISON.md) sets it next to Niagara and Hanabi. Next for it: the editor's Effects tab, mesh particles, events and ribbons on the card.
*The artificial-life work (cells, populations, grown bodies; the `lunaris-alife` crate, its GPU culture and its docs) was removed on 2026-10-25 at the owner's order. It is in the git history before that commit.*
