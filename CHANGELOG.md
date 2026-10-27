# Changelog

What changed in Lunaris and when, by day, from the first commit on 2026-10-14. The engine is at version 0.0.x and has made no release or tag yet, so there are no version headings: this is the history of `main`, in the words of what a person using the engine gets. The commit messages have the detail, [docs/](docs/README.md) says how each part works, and [docs/PERFORMANCE.md](docs/PERFORMANCE.md) has every number quoted here.

## 2026-10-26: particles, speed, and a green CI on both systems
- **Particle systems.** `ParticleSystem` (Niagara-style effects as JSON files): emitters, curves over a particle's life, forces, curl noise, collision with the world's colliders, events between emitters, ribbons, solid cubes, soft particles, lights, level of detail and a budget; emitters can run on the graphics card (`"simulate": "gpu"`); a voxel look (square, nearest-filtered); an Effects tab in the editor; a library in `examples/fx`. See [docs/FX.md](docs/FX.md).
- **A faster CPU renderer.** A 960 x 540 picture of 1,600 meshes: 44 to 40 ms on one thread and 27 to 15 ms on four, the picture bit for bit the same. On a machine with only a software graphics adapter the render tool and the Windows player now choose it over the adapter, which is five times slower.
- **Measuring the card.** `lunaris bench --gpu` (a picture and a frame as a game draws it), `perf_gate.py --gpu` (baselines per machine and adapter), profile entries for what the machine did while a picture was drawn, a `upload_cost` example, a sampling profiler for any command (`LUNARIS_SAMPLE`).
- **A server that holds a crowd.** Path searches done a few cells a step and kept between steps, a cache for the cells' solids, a grid over the mesh levels of the ray world, flying agents' patrol text read once, procedural grids found in a flat array: a city server with 210 players steps in 5 ms, 600 players tick at 24 ms on one eight-core machine (see [docs/NETWORK_SCALE.md](docs/NETWORK_SCALE.md)). `profile` and `--profile-sim` list the slowest steps with what each was made of; `check` warns about crowds of path finders and streamed worlds with no limits.
- **Editor.** A History tab (and a `history` tool), Copy and Paste values of a component, Play > Stop that keeps the selected entities' changes, the card's time pass by pass in the Profiler, draw calls and triangles in the Stats.
- **CI.** Tests, the picture hashes of 61 example scenes, the editor build and the classic-quality graphics comparisons pass on Windows and Linux; failures are written as annotations; the heavy graphics tests run on machines with a card.

## 2026-10-25: a performance program, and an editor that looks like Unreal's
- **Speed.** A performance gate (`tools/perf_gate.py`), a faster ECS (bundle spawn, a dense uid table, a state hash worked out once per archetype), lazy path-search grids (10 to 44 times faster), characters walked on several threads (5.3 to 1.7 ms), a static world gathered once, spawns of a procedural grid in one batch, `despawn_trees` 6.6 times cheaper.
- **Editor.** The look of the Unreal Editor 5, a Content Browser with thumbnails and drag into the scene, a Place tab, Paint mode, multi-select, Project Settings, a Profiler tab, the Stats overlay, a Sequencer over `Timeline`, live values and breakpoints in the Blueprint tab, an Output Log laid out like Unreal's.
- **Light and sound.** `Lighting` (fill, sun bounce, lamp falloff, up to four lamps with shadows), `MacroVary`, sound files with several variants, acoustic ray tracing against oriented boxes.
- **The city** got a round plan, ruined districts and baked blocks.

## 2026-10-24
- Gameplay toolkits for survival games (combat, senses, survival needs, building, raiding, vehicles, loot) and a game server for 100+ players (scale netcode, secure play, an admin console, lag compensation, parties).
- GPU skinning for crowds of puppets; ECS bundle and batch spawn; the first performance baseline and a comparison with the Rust ECS crates.

## 2026-10-19 and 2026-10-20: creatures, movement, motion matching
- Evolved creatures with neural brains (`lunaris-creature`), inventories and recipes, motion matching and inertialization for animation, ledge traversal and foot IK, hands-off play and a live API for the player (`--api`).

## 2026-10-18: physics speed
- A fast path that keeps still colliders in a grid, a thread pool, warm-started contacts, sleeping bodies, continuous collision for fast bodies, hinge motors and springs: bit-identical to the reference step.

## 2026-10-17: the procedural city
- An endless procedural city (streets, buildings with rooms, stairs and doors, skyscrapers, parks, night and weather), streamed by zones, with a game on top of it (doors, containers, base building, hunger and thirst, saves, sounds).
- A renderer with an early depth pass, software occlusion culling and per-face point-shadow culling; streaming without stalls (preparation on worker threads); a Tokyo pipeline from open 3D city data.

## 2026-10-16: the graphics card
- The GPU renderer grows up: bounced light, an atmosphere and volumetric clouds, water with depth, a sea from an ocean spectrum, grass made on the card, painted terrain with macro variation, frustum culling, automatic levels of detail, block-compressed textures with a disk cache, material lobes (clear coat, sheen, subsurface), a pass profiler.
- Real-world map tools (open elevation, lidar, orthophotos, buildings) and streamed worlds; the first CI.

## 2026-10-15: breadth
- Physics v2 to v4 (rotation, capsules, joints, mesh and convex colliders, collision layers, raycast vehicles); rollback netcode with spectators, rejoin, a lobby and a latency simulator; UI kit v3 (scroll views, drop-downs, tabs, drag and drop); terrain layers and water bodies; auto exposure, screen-space reflections, lens flare, reflection probes; particles clouds and modules; JPEG, Ogg and MP3 files; mixer buses and music; inverse kinematics; dialogue, steering, saved scenes and scene switching.
- Four complete example games built only through the tool API, with the engine gaps each one found (`docs/DOGFOOD.md`), and the public release tooling.

## 2026-10-14: the first day
- The first commit: an AI-first engine core (a tool API over MCP and JSON-RPC, every error saying how to fix it), a deterministic simulation with snapshots and replays, a CPU reference renderer that draws the same picture on any thread count, graph systems with a batch VM, and a model loop (`lunaris mind`).
- Later the same day: a GPU renderer on wgpu, glTF models with skeletons, sandboxed WebAssembly systems, a character controller, lockstep multiplayer, prefabs, navigation, an on-screen interface, acoustic ray tracing, a native editor in the looks of Unity, Unreal and Visual Studio Code, Lunaris Script, terrain, joints, the first gameplay blocks, and golden state and picture hashes that are the same on Windows and Linux.
