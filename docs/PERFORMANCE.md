# Performance

How fast Lunaris is, how that is measured, and how to repeat every number. A speed claim in this repository only counts with a command that reproduces it.

Rules of the program (see `ROADMAP.md`): **measure first, change by the profile, keep the table honest** (a result where Lunaris is slower stays in the table), and every optimization must leave the simulation bit-identical (the state hash and the golden hashes do not change).

## The machine and the method

All numbers in this file come from one machine unless a section says otherwise:

| | |
|---|---|
| CPU | Intel Core i7-9700K, 8 cores / 8 threads, 3.6 GHz base, up to 4.9 GHz |
| Memory | 32 GB |
| OS / toolchain | Ubuntu 24.04, rustc 1.99.0, `--release` (`opt-level 3`, fat LTO, 1 codegen unit) |

Each timing is the **median of 15 samples**, where a sample runs the workload enough times to take about 20 ms; the minimum is printed next to it. One thread unless stated. No result is estimated; where a number comes from somewhere else it is quoted with its source.

## 1. The core simulation: `lunaris bench`

```
cargo run --release -p lunaris -- bench --entities 200000 --ticks 20
```

First baseline (commit `6a6aaa1`, 200 000 entities, 20 ticks): see the table. "4 threads" is what the default thread count picks on this machine.

| Row | Time | What it is |
|---|---:|---|
| spawn | 39 ms (5.1 M entities/s) | 200k entities straight into the final archetype, one at a time |
| step, 1 thread | 2.45 ms | wander on a quarter + movement on all: 12.2 ns/entity |
| step, 4 threads | 1.02 ms | 5.1 ns/entity, 2.4x, **bit-identical state hash for any thread count** |
| graph system, 1 thread | 2.56 ms | a 2-statement data-defined system through the batch VM (4.0x the reference interpreter) |
| query pass | 0.38 ms | read one component of every entity |
| state hash | 26.6 ms | canonical order, field by field |
| snapshot (clone) | 6.5 ms | ~13 MB of component data |
| scene export / import | 260 ms / 159 ms | 100k entities, 13.4 MB of JSON |
| physics | 1.29 ms / tick | 5 000 spheres piling up (sort-and-sweep, sequential impulses, one thread) |
| render 960x540 | 40 ms (CPU reference renderer, 1 thread; 48 before 2026-10-26) | 15.3 ms on 4 threads, 11.5 ms on 8 (27.8 ms on 4 before), picture identical |

### The "potato" run

Same command pinned to two cores and a 4 GB address-space limit (`taskset -c 2,3`, `ulimit -v 4194304`). It runs to the end and every picture and hash is identical. On this machine the clock is not lowered, so this measures the core count and the memory cap only, not a 2 GHz chip:

| Row | 8-core machine | 2 cores, 4 GB |
|---|---:|---:|
| step (1 thread) | 2.45 ms | 2.47 ms |
| physics | 1.29 ms | 1.88 ms |
| render 960x540 | 48 ms | 52 ms |

Again on 2026-10-25 (after the ECS changes of result 3; best of three runs each, the same Linux test machine i7-9700K, the same command with `taskset -c 2,3` and `ulimit -v 4194304`):

| Row | 8-core machine | 2 cores, 4 GB |
|---|---:|---:|
| spawn 200 000 entities | 17.5 ms (11.4 M/s) | 17.4 ms (11.5 M/s) |
| step (1 thread) | 2.32 ms | 2.32 ms |
| state hash | 15.2 ms | 15.4 ms |
| snapshot | 4.9 ms | 4.9 ms |
| physics | 1.27 ms | 1.85 ms |
| render 960x540 (CPU reference, one thread) | 47.4 ms | 47.3 ms |
| memory an entity costs | 102 bytes | 102 bytes |

Every hash and picture was identical in both configurations (the state hash `85b930a7463373a1` after 20 ticks is also the value it had before the hash was made faster). Only the physics row differs (1.85 ms against 1.27 ms), and it is a one-thread row, so the cores are not what it lacks: I did not find out why (the two pinned cores share the machine with whatever else runs on a Linux test machine, which had a load of about 4; not checked on an idle machine). One run in four starts slower (a 48 ms spawn in the first process): the machine, not the engine, and the reason the gate takes the best of several runs.

The definition of the target machine (2 cores at about 2 GHz, 4 GB, integrated GPU or the software rasteriser) and a run on real low-end hardware are still to do (`ROADMAP.md`, section 1).

## 2. The ECS against other Rust ECS crates

The workloads are those of the public [ecs_bench_suite](https://github.com/rust-gamedev/ecs_bench_suite) (rust-gamedev, now archived; it pinned 2021 versions of the crates): **simple_insert** (10 000 entities with four components of 64, 12, 12 and 12 bytes), **simple_iter** (add a velocity to a position over 10 000 entities), **frag_iter** (double one `f32` over 26 archetypes of 20 entities), **add_remove** (add and then remove one component on 10 000 entities). I added **simple_iter_1m** (the same loop over a million entities, which no longer fits the cache). Not yet ported: the scheduling, heavy-compute (multi-threaded) and serialization workloads of the suite.

The crates are current ones, resolved in `tools/bench/ecs_compare/Cargo.lock`: hecs 0.10.5, bevy_ecs 0.15.4, legion 0.4.0, shipyard 0.7.8, specs 0.20.0. One timer for everyone, one thread, no scheduler. The adapters are mine and written the idiomatic way for each crate (bulk spawn where the crate has one); if one can be written faster, a patch is welcome. The harness frees one 24 MiB block before it starts: glibc serves big allocations with fresh `mmap` pages (every first touch is a page fault) until the first free of a big block makes it use the heap instead, which is the state of any game that has been running for a second; without it every spawn benchmark of a process costs about twice as much for Lunaris (62 ns instead of 25 per entity; hecs and bevy_ecs do not change). **The block must be at most 32 MiB**: glibc raises its threshold only for a freed block up to `DEFAULT_MMAP_THRESHOLD_MAX` (32 MiB on 64-bit: `4*1024*1024*sizeof(long)`, see [mallopt(3)](https://manpages.ubuntu.com/manpages/noble/man3/mallopt.3.html)), and the first version of the harness used 33 MiB, which taught it nothing, so the tables of results 1 and 2 below that rest on the warm heap were measured on a heap that was not warm for some workloads (found on 2026-10-25; result 3 is re-measured with the fix). That is a finding in itself: creating a lot of entities right after start-up or a level load is bound by fresh memory, and the engine should reuse a world's storage across level loads (listed below). The "plain arrays" row is the same loop on two `Vec`s with no ECS at all: it is a reference, not a lower bound (a loop the compiler vectorizes better can beat it).

Reproduce (on any machine with Rust; the project is outside the engine workspace, so the other crates never enter the engine's `Cargo.lock`):

```
cargo run --release --manifest-path tools/bench/ecs_compare/Cargo.toml --features all
cargo run --release --manifest-path tools/bench/ecs_compare/Cargo.toml --features hecs -- --only hecs --only lunaris
```

### Result 1: before any optimization (commit `6a6aaa1`)

ns per entity, lower is better:

| | simple_insert | simple_iter | simple_iter_1m | frag_iter | add_remove |
|---|---:|---:|---:|---:|---:|
| plain arrays | | 0.99 | 1.27 | 0.17 | |
| hecs 0.10 | 28.5 | **0.98** | 1.40 | 0.61 | 44.8 |
| legion 0.4 | 59.9 | 1.11 | 1.42 | 0.81 | 215 |
| **Lunaris** | **131** | 1.16 | 1.61 | 0.82 | 72.4 |
| bevy_ecs 0.15 | 29.2 | 1.61 | 2.08 | 0.93 | 76.0 |
| shipyard 0.7 | 13.8 | 1.86 | 2.48 | 0.13 | 44.1 |
| specs 0.20 | 108 | 2.49 | 2.78 | 1.79 | 6.4 |

Lunaris was the slowest at creating entities (4.6x hecs): every entity was built through a spawn path that allocated twice and looked the archetype up in a tree on each call. Iteration was close to the front. The fragmented query lost to its setup cost (three allocations per query).

### Result 2: after the first round (this change)

* `World::spawn_bundle` and `World::spawn_batch`: a tuple of up to 8 components is spawned with no allocation, and a batch looks the archetype up once and reserves the storage and the uid table up front.
* `insert` and `remove` pair the columns of the two archetypes in one pass over sorted component lists (no search per column, no copy of the default bytes).
* The `TypeId` registry lookup (done on every typed insert, query and spawn) uses a cheap hasher instead of SipHash.
* Queries no longer allocate: the term list and the column pointers are fixed arrays, the pointers sit in a local tuple so the row loop keeps them in registers (and the compiler vectorizes it), and `for_each` no longer reads the entity's uid record (only `for_each_uid` does).

| | simple_insert | simple_iter | simple_iter_1m | frag_iter | add_remove |
|---|---:|---:|---:|---:|---:|
| plain arrays | | 1.00 | 1.40 | 0.18 | |
| **Lunaris** | **29.9** (batch) / 64.7 (loop of `spawn_bundle`) / 93 (old low-level path) | **0.71** | **1.18** | **0.30** | 53.9 |
| hecs 0.10 | 29.7 | 1.04 | 1.42 | 0.85 | **44.6** |
| legion 0.4 | 51.5 | 1.06 | 1.50 | 1.00 | 196 |
| bevy_ecs 0.15 | 28.6 | 1.71 | 2.02 | 0.67 | 77.1 |
| shipyard 0.7 | **13.9** | 1.82 | 2.54 | **0.13** | 56.0 |
| specs 0.20 | 110 | 2.51 | 2.71 | 1.90 | **6.4** |

What changed for Lunaris: spawn 131 to 29.9 ns (4.4x, now level with hecs and bevy_ecs), simple_iter 1.16 to 0.71 (the fastest of the group), the million-entity loop 1.61 to 1.18, frag_iter 0.82 to 0.30, add_remove 72 to 54. Still behind: shipyard (sparse sets: cheap to create and to fragment, slower to iterate pairs) on spawn and fragmented queries, hecs by 20% on add/remove, and specs on add/remove (a storage per component type makes adding one almost free there and costs it everywhere else).

What the numbers do not say: Lunaris keeps a stable uid and a generation for every entity (the uid map is part of the deterministic state, scenes and the network), and its components are plain data (`Copy`, a schema for tools and scenes), which makes moves a `memcpy` but excludes components that own heap data. The other crates pay neither cost and accept any Rust type. These are the trade-offs, not an excuse: they are counted in the Lunaris numbers above.

### Result 3: the uid table, and the harness fixed (2026-10-25)

Two changes. **The uid table**: `uid -> entity` was a hash map (about 30 bytes an entity, a hash and a probe per lookup, a rehash while it grows, a hash-map clone in every snapshot); uids are handed out one after another, so it is now a plain array indexed by uid (4 bytes an entity) for every uid that is not far past what the array holds, with a hash map only for the explicit far-away uids a loaded scene can carry (`UidTable` in `world.rs`; the array holds the slot index, the generation is read from the slot table, so an entry cannot go stale). **The harness** warm-up block is now 24 MiB (see above).

| `lunaris bench`, 200 000 entities | before | after |
|---|---:|---:|
| spawn | 8.7 M entities/s | 11.1 M entities/s |
| memory an entity costs (resident growth; the components themselves are 64 bytes) | 135 bytes | 102 bytes |
| snapshot (clone of the world) | 6.3 ms | 4.9 ms |

At a million entities the same change is 7.2 to 11.2 M entities/s and 144 to 101 bytes an entity. **The 10 000-entity `simple_insert` of the suite did not change** (24.8 ns before, 25.2 after, with the corrected warm-up): at that size the hash map is small and in cache, so the gain only shows where the map is large. (An earlier comparison on the same day showed 62 ns before and 52 ns after, because the cold allocator hid it; that comparison was wrong, the corrected one is in the table.)

ns per entity, lower is better, one thread, corrected harness, current code:

| | simple_insert | simple_iter | simple_iter_1m | frag_iter | add_remove |
|---|---:|---:|---:|---:|---:|
| plain arrays | | 1.00 | 1.47 | 0.17 | |
| **Lunaris** | **25.2** (batch) / 58.8 (loop of `spawn_bundle`) / 88.2 (old low-level path) | **0.72** | **1.26** | 0.29 | 53.6 |
| hecs 0.10 | 30.3 | 1.00 | 1.47 | 0.65 | 45.5 |
| legion 0.4 | 54.4 | 1.01 | 1.48 | 1.08 | 210.7 |
| bevy_ecs 0.15 | 29.6 | 1.73 | 2.16 | 1.14 | 81.1 |
| shipyard 0.7 | **13.9** | 2.23 | 2.81 | **0.13** | 49.1 |
| specs 0.20 | 111.4 | 2.63 | 2.90 | 1.84 | **5.9** |

(Result 2's Lunaris batch spawn of 29.9 ns was measured with the 33 MiB warm-up and is not comparable: with the fixed harness the old code gives 24.8.) Behind: shipyard on spawn and on fragmented queries (sparse sets), hecs on add/remove by 18%, specs on add/remove.

**The state hash** (`World::state_hash`) works out what to feed the hasher once for each archetype (a list of steps: constant words for the component names and counts, runs of fields read from one column) instead of walking each entity's schema field by field, and keeps the state in a register: 200 000 entities, 18.0 ms to 15.5 ms (90 to 77 ns an entity), **the same hash value** (a test compares it with the field-by-field way over every field type; the golden hashes of the examples did not move). Why not more: the hash is one long chain where every step waits for the one before it (`h = (rotl(h, 23) ^ x) * K + C`), 1.41 ns a word on this machine (measured with a 20-line probe: 6.5 cycles at 4.6 GHz), and an entity of the benchmark is about 24 words, so the floor of this definition is about 34 ns an entity; the rest is work the processor cannot fully hide behind the chain. Getting under it means changing the definition (each entity hashed on its own and the results combined, which could run on every core and skip the ordering by uid), and that changes every stored hash: a decision for a release, not a refactor. The editor's status bar no longer hashes the scene on every frame (after a change, and four times a second while the game runs).

**Memory**, `ecs_compare --memory N` (growth of the resident set for a world of N entities with the four components of `simple_insert`: 100 bytes of data; each crate in a process of its own; Linux only), 1 000 000 entities:

| | hecs | specs | legion | **Lunaris** | shipyard | bevy_ecs |
|---|---:|---:|---:|---:|---:|---:|
| bytes an entity | **116** | 118 | 143 | 148 | 175 | 176 |

Lunaris pays about 48 bytes an entity over the data: a slot record of 24 bytes (generation, archetype, row and the 8-byte stable uid), the archetype's entity list (8), the uid table (4) and, in this harness, the transient list of entities that `spawn_batch` returns. hecs pays about 16. That is the price of the stable uid and generation that scenes, the network and the deterministic state hash rely on; it is the next thing to shrink (below).

## Navigation: what a path search costs (2026-10-25)

`lunaris bench` has two rows for the cost of one path search (`find_path`): `nav open`, 164 m over open ground, and `nav city`, the same trip across 36 building shells of 4 000 triangles each (144 000 triangles), on a Linux test machine's i7-9700K. The search itself visits about a hundred nodes in both. **Before**: 6.8 ms and 41 ms, because the grid it runs on was built for the whole box between the two points, by cutting every triangle in the box into the cells it covers (about 235 ns a triangle, most of it the clipping of wall triangles against every cell they reach), for a search that looks at a few hundred of the box's thirty thousand cells. A city server asks for dozens of such searches a second when the players are loud: that is where its 1.3 s ticks came from (a game's soak test). **After** (the data of a cell is worked out the first time the search looks at it, from the triangles near that cell, and kept): **0.15 ms (44 times faster) and 4.1 ms (10 times faster)**, with every path bit for bit the same (`crates/lunaris-sim/src/nav_tests.rs`, `the_lazy_grid_gives_the_paths_of_a_grid_cut_whole`: 150 searches over a scene of boxes, a stair, a link, a sphere and 16 triangle shells, by agents of three sizes, hash of every path, jump, length and error, recorded from the old grid and equal to the new one). The 4 ms that is left in the city is mostly asking each mesh's tree for the triangles near each cell and gathering the colliders. `NavLimits` (`docs/NAVIGATION.md`, "Crowds") rations searches on top of this, so a crowd cannot ask for more than the budget. Still to do if it matters: a cell size per agent, a cache of the tree answers for a static shell. The two rows are in the performance gate.

**Cells kept between searches (the same day, opt-in).** Timing the `nav city` search from inside (a counter around the work of a cell): of its 4.1 ms, about 3 ms is working out the data of the cells (asking the mesh trees for the triangles near each cell and clipping them), and a server asks for the same streets again and again. `NavLimits.keep_cells` (or `NavParams.keep`) puts the window of a search on a lattice of the cell size anchored at the world's origin, so that a cell of one search is a cell of the next, and keeps the data of a cell (one set for each size of agent, at most 600 000 cells each, per thread) while a hash of the world's solids, grounds and mesh levels is the same; a different hash drops them. A search repeated over the same streets: `nav city` **4.16 -> 0.09 ms** (the `nav city kept` row of the bench, which repeats one search: the best case). **It is off by default, because a path is not bit for bit what it was**: the cells' centres are on the new lattice, so a waypoint can differ by up to half a cell and a tie between two equally near cells can fall the other way, and nav paths are part of the state hash, the snapshots, the recordings and the golden hashes of the examples. I first made it the default: the examples' golden hash of `maze` moved, which is how I found out. With it off every path is the one it was (the fingerprint test is unchanged, `0x14d88eab...`); with it on, what is tested is that kept cells give exactly the paths of cells worked out fresh on the same lattice (`cells_kept_between_searches_give_exactly_the_paths_of_cells_worked_out_fresh`: the 150 searches of the fingerprint, with keeping off, on and again) and that a wall built after the cells were kept is walked round (`a_wall_built_after_a_search_is_not_walked_through_by_the_next_one`). **Not shown in the city server:** a 50 s run with 150 swarm bots (`city_perf.py` of the a game game) has almost no searches (the stage is 0.03 to 0.05 ms a step); the gain is for what a game's author measured over a long run, hunters asking again and again for a player inside a building (two capped 20 000-cell searches in one tick, 140 to 209 ms): the first search pays, the rest find the cells made, as long as the world's solids have not changed in between. To be measured there, with `keep_cells` on in the game's `NavLimits`.

**Despawning a tree no longer maps every child in the world (2026-10-25).** `despawn_trees` (lifetimes that run out, explosions, the director's hunters, a building taken apart, the zones of a procedural cell that is left behind, interactions) built a `BTreeMap` of every `Parent` of the world on each call, to find the children of a few roots: 0.48 ms for 9 600 attached parts, and it grows with the world (the city server has far more) and with how often it is called (several times in a tick where things expire at once). It now collects the (child, parent) pairs of the world in one pass and finds the tree by passes over that list, taking the children of what is doomed and dropping those pairs, with a fast set of uids: **0.073 ms for the same 23 roots among 9 600 parents, 6.6 times less**, the same tree in the same order (test `the_tree_found_for_a_despawn_is_the_one_a_map_of_all_children_finds_and_costs_less`, which compares with the old method on 400 buildings of 24 parts, 30 000 loose entities, a nested root, a missing root and a repeated one). Suspected, not measured in a game's city: one of the sources of the 18 to 43 ms spikes of the `procgrid` stage (a cell that is left behind despawns the trees of all its zones in one call) and of part of the 2.7 to 3.5 ms of the gameplay blocks.

**The spikes of the procedural grid: a hundred hierarchy passes in one tick (2026-10-25).** The city server's `procgrid` stage averaged 0.3 to 0.4 ms but had a worst step of 15 to 20 ms in every run, and those steps set the worst step of the whole simulation (20 to 33 ms against 5 on average). Found by timing the parts of the stage inside the real city server (150 swarm bots, the a game scene on a Linux test machine, `city_perf.py`): while cells are made, the **spawning of their zones** took 2 to 16 ms for 1 to 20 cells (up to 133 zones in a cell), about 100 microseconds a zone. Each zone was spawned with its own `apply_batch`, and `apply_batch` ends with `hierarchy(world)`, a pass over every attached entity of the world (collect, sort, a hash map, the transforms), so a cell of 133 zones paid 133 passes. The zones of all the cells a grid makes in a tick are now spawned in one batch (`spawn_transients`): one pass, the same uids in the same order (a test checks the uids of cells with 3, 0, 2, 1 and 0 zones, the markers of the empty ones, that nothing is made twice and that a cell left behind goes with its zones). Same scene, same machine: `procgrid` worst step **15.7 -> 2.7 ms**, `gameplay blocks` worst step 17 to 19 -> 5 ms, the simulation's worst step **20 to 33 -> 14.5 ms**, average 4.8 to 5.2 -> 4.9 ms (it was never the average). What is left in the stage is the walk of the cells round the 150 anchors, about 1.3 ms in the steps that make cells; the same `apply_batch` + `hierarchy` cost applies to any code that spawns many entities one command at a time, so batch them. A measured side result of the same run: the older binary (before the probe, `despawn_trees` and static-solid changes of today) and the new one differ by less than the noise (5.7 against 4.8 to 5.2 ms a step) in this scene, which has no probes.

**The characters' grid is laid out once while the obstacles stay (2026-10-25).** Every step the characters stage gathered every solid collider of the world and laid them out in hash maps, a grid over x and z, so that a capsule looks only at what is near it: 0.57 ms of the city server's 4.9 ms a step (`characters: grid`, 150 bots) and 0.16 ms in the bench's city of 3 500 obstacles. The layout only depends on the middle and the half width and depth of each obstacle, so the maps are now kept (on the thread, handed back when the grid is dropped) and reused when those numbers are the same as in the last step, which is almost every step in a world whose solids stand still (a box that falls changes none of them: the grid looks at x and z). The compare is one pass over the list (about a tenth of the layout). `lunaris bench`, the `characters` row, 200 capsules among 3 500 obstacles, the old and the new engine run alternately four times on the same machine: **`characters: grid` 0.16 -> 0.015 ms, the row 1.33 to 1.53 -> 1.10 to 1.16 ms a tick**. Tests: the same crowd ends in the same state hash with and without reuse (the maps are only looked up, never walked), the layout is made once in 80 steps of a still world, and in every step in which something moved in x or z (an obstacle slid for 50 steps: 50 layouts; with reuse switched off, 90 of 90).

**The client's tick in the city: hidden widgets and the character grid (2026-10-25).** a game's `client_profile.py` (the offline client in the city, 4 700 to 6 000 entities, no GPU) put the simulation at 4.4 to 5.7 ms a tick, with two stages that were not the game's own systems. Timing the passes of the interface-widgets stage inside it: 0.75 to 0.87 ms, of which `hide_pointer` 0.38 and `drag` 0.33 and the rest nothing, and both asked for the set of what a switched-off `UiGroup` hides, which was built from scratch each time (a map of every `Parent` of the world, then every parented entity's ancestors). The set is a function of the groups that are off and of the `Parent` relation, so it is now kept (`hidden_set_shared`), with a key that is one cheap pass over the `Parent`s: the stage **0.75-0.87 -> 0.10-0.12 ms**. The same profile showed `characters: grid` at 0.8 to 1.5 ms: the layout of the obstacle grid, which my reuse of the same morning could not use because some obstacles move every step (cars, doors, drones). When the obstacles are the same in the same order and only a few have changed, only their cells are touched now (each list kept in the order of the obstacles, so the layout is exactly the one a fresh build makes; a test checks every patched layout against a fresh one while three scenes run, one with twenty boxes moving about); with many changes a fresh build is cheaper and is used. Same scene, same machine, before and after: the simulation **4.65 -> 2.78 ms** a tick (gate avenue, noon), 4.97 -> 2.72 (plaza), 5.70 -> 3.29 (the scar, dusk), the fourth place 3.0 (a street at midnight, in rain). Tests: `the_shared_hidden_set_is_always_the_one_made_from_scratch_whatever_changes` and `the_grid_of_obstacles_is_laid_out_once_patched_where_some_moved_and_the_result_is_the_same_without_reuse`.

**Where a frame of the city goes on the processor, and the particles (2026-10-26).** a game's client in the city, 1280 x 720 `high`, the GPU renderer on a Linux test machine's RTX 4060 (`LUNARIS_GPU_PROFILE=1`, a script that renders the city from three places and takes the median of the pictures; its `info` carries `cpu_ms.*` and `gpu_ms.*`): the card needs 3.2 to 3.9 ms (the main pass 1.7 to 2.0, the sun's shadows 0.6 to 0.8), the processor about 8 to 9 ms a picture before the read-back that a still needs: **`gather` 4.1 to 4.4 ms** (the scene as data), `draw_call` 2.1 to 3.0, `submit` 0.9 to 1.4, uniforms and upload 0.5 to 0.6. `LUNARIS_TIMING=1 LUNARIS_TIMING_MIN=0.02` now prints the gather's steps to two decimals (it printed whole milliseconds), and the gather has a guard for each step of the interface and the particles; they showed that the meshes, the lights, the camera, the interface (0.7 ms in all) and everything else together cost under 1 ms and that **the particle clouds took 3.0 to 3.7 ms**: the three effects of the AI tower (6 000, 2 400 and 1 400 particles with turbulence, additive) are built on the processor every picture, 300 ns a particle, in one thread (a cloud was cut into threads only above 4 096 particles per thread). Now every number that all the particles of a cloud share (the cone's axis and opening, the two directions across it) is worked out once, and the particles of all the clouds of a picture are worked out together in pieces of 1 024 taken by a few workers from one start of threads (`particles::build_many`), joined in order: **3.0-3.7 -> 1.0-1.3 ms**, every mesh the same bit for bit (`clouds_are_exactly_the_meshes_that_were_recorded`: hashes of the meshes of five clouds with all the shapes, drag, turbulence, sheets, stretch, sorting and two cameras, recorded from the code before the change, for 1, 2, 5 and 8 threads). **A cloud that is wholly out of the picture is not built at all.** `particles::bound` gives a sphere that holds every particle a cloud can have, whatever the clock says (how far a particle starts from the middle, how far it can travel in its life with its speed and gravity, how far turbulence can push it, how big a picture or ribbon can be; none for a cloud with forces, colliders or a size gradient, whose particles cannot be bounded without building them), and `particles::outside` tests the sphere against the four sides and the near plane of the picture. The gather skips such clouds (its timing line says how many). In the city at two of the three places every cloud was outside (the picture looked away from the tower): the interface-and-particles step of the gather went **3.5-3.7 -> 0.6 ms** there, and 4.1 -> 1.8 ms at the third (six clouds built, 1.2 ms). Test: 400 random clouds seen from random cameras, each cloud's whole mesh is inside its sphere and a cloud with any vertex in the picture is never called outside (and the test sees 40 or more clouds of each kind); and a scene with one cloud in front of the camera, one far to the side and one behind it builds only the first. What is left of the building itself is mostly the work and the start of the threads; a persistent pool is the next step.

**The physics step of a world with meshes: a grid for the statics (2026-10-26).** The fast physics path (statics kept in a grid between steps) is only for worlds without terrain, mesh colliders, convex hulls, joints and spinning bodies; the city, with mesh shells, takes the reference path, which sorted and swept **every** collider every step, though 22 of its 1 800 to 2 000 bodies could move. Timed inside the city server with the reference path's own stage counters (`physics::reference_profile`): `gather` 250-310 us and `broad phase` 370-560 us of a `system physics` stage of 0.9 ms, with the solver at 6 to 13 us. The broad phase now finds the pairs for the bodies that can move (or are triggers) only: they sweep among themselves, and each looks up the statics round it in a grid over x and z (`StaticGrid`, cells of 16 m, statics wider than 8 cells on a list every mover looks at, a mover whose box covers more than 256 cells looks at all of them, as the sweep would). Every candidate goes through the very tests the sweep applied, its own first, so the pairs are the same (`found` is sorted by uid afterwards); a world with a number that is not finite, or with many movers (one in eight or more), keeps the old sweep. The grid is kept between steps (the boxes of the statics are the same in almost every step) and patched for the few whose boxes changed, like the characters' grid; a first version that built the grid in every step spent 320 us on it and gained nothing. The gather also stops asking every static for a `RigidBody` and a `Velocity` (three lookups each, about the cost of the whole gather): the few entities that have one are listed first. City server, same scene: `system physics` 0.93-1.02 -> 0.60 ms and the simulation step 4.25 -> 3.84 ms in the run that was measured, on a machine that other jobs were using. Tests: `the_grid_broad_phase_finds_exactly_the_contacts_the_sweep_of_all_bodies_finds` (a scene with 260 statics of every size, a floor and a wall wider than the cells, a dozen movers of every shape and layers, a spinning body so that the reference path runs, two triggers, a dozen statics moved about, one removed and one added: the contacts of every step and the state of the world after 150 steps equal the sweep of all bodies, and every patched grid equals a fresh one) and the whole physics suite (102).

## Characters: what a walking capsule costs among triangles (2026-10-25)

`lunaris bench` has a `characters` row: 200 capsules (`CharacterBody`) walking in a city of 36 building shells of 4 000 triangles each and 3 280 boxes (3 517 entities), one thread, a Linux test machine's i7-9700K. In a short run (10 ticks a sample) it is **1.76 ms a tick**; in a long one, after the capsules have walked into the walls and slide along them (`LUNARIS_BENCH_ONLY=characters lunaris bench --ticks 1000`, which also prints the engine's own profile of the stage), **8.6 ms a tick, 43 microseconds a capsule**. This is the stage of the city server that came out at 3 ms a tick for 160 players in a game's soak test.

**Where it went.** `perf` cannot be used on a Linux test machine (`kernel.perf_event_paranoid` is 4; changing a kernel setting is not mine to do), so the controller was timed from inside, with counters in a scratch copy and, left in for good, three names in the profile: `characters: obstacles` (gathering every solid of the world: 0.07 to 0.10 ms), `characters: grid` (laying them out in the grid: 0.29 ms, 0.15 after the cheap hasher below) and `characters: walking` (moving each capsule: 7.6 to 8.3 ms, 95% of the stage). Per tick, in the long run: 1 690 grid queries (13 200 candidate obstacles, 0.19 ms in all), 8 200 box tests (0.26 ms) and **1 464 capsule-against-shell queries**, each of which is two ball queries at the ends (1.4 ms in all), the reduction of the contacts (0.16 ms) and **the search for the point of the axis that comes nearest to the mesh** (a pole lying across a ledge): **4.2 ms, 52% of the walking time, 2.9 microseconds a capsule**. That search measured every triangle in the capsule's box (the box grown by the radius) with the exact segment-to-triangle distance (a crossing test, two point distances and three segment-to-segment distances) and then two more point distances for the ends: for a character against a facade of a few dozen triangles, all of it to find out that every point of a vertical axis is as near as the ends.

**The change.** A triangle whose box is farther from the axis (or from either end) than the nearest triangle found so far cannot be the nearest and is not measured, with a margin of 2e-3 m² (growing with the distance from the origin) for the rounding of the two ways of measuring. The answer is the very same, bit for bit: `nearest_on_axis_reference` keeps the plain loop and a test compares the two on 100 000 capsules (vertical and slanted, radii 0.15 to 0.9, over a triangle soup, a floor, a squeezed soup and a soup 3 km from the origin), and the state hash and every test are unchanged (sim 893 tests). **Walking 8.34 to 5.05 ms (-39%), the stage 8.6 to 5.9 ms (-31%), the short row 1.86 to 1.49 ms (-20%).** Also: the grid's two maps use a cheap hash instead of SipHash (they are only looked up and filled, never walked, so it cannot change what a character does), and are given room for their entries up front.

**On several threads.** A capsule's walk reads the grid, the collision layers and the water and writes only its own result (characters collide with the world, never with each other), so the capsules can be walked at the same time: the grid is shared and each capsule looks at it through a `View` that carries its own layer and mask (that was a field of the grid, set capsule by capsule), a walk returns a `Walked` instead of writing into the world, and the results are put back in uid order. The worker threads are those of `pool.rs`, made the first time there are 24 capsules or more and `threads > 1` (a clone of a simulation starts without them), each task takes at least 8 capsules; fewer capsules are walked on the calling thread, which is the same code. The result does not depend on the number of threads: a test walks 90 capsules (some with `CharacterMotion`, jumps now and then, walls, boxes and a step in the way) for 160 ticks on 1, 2, 4 and 8 threads and compares the state hash and every position, and every golden hash and playtest is unchanged (sim 894 tests, the sample playtests, net, api, mind, script). **200 capsules in the long run, one thread: stage 5.3 ms, walking 5.0; on this 8-core machine (the default is four threads): stage 1.74 ms, walking 1.47 (3.4x).** A 2-core machine runs the default of one thread (the default is half the cores) and stays at 5.3 ms. The write-back of the results takes 4 microseconds.

**What is left** (long run, ms a tick, one thread): the ball queries 1.4 (five small allocations a ball: the triangle list, the candidates, the ghost flags, the hits, the contacts), the box tests 0.26, the grid queries 0.19, and the rest of the controller (the bisection of `lower_to_contact` runs `blocked` eleven times for a capsule that is not on the ground: 147 a tick).

## The static world and the wallhack defence: what a gather and a re-ask cost (2026-10-25)

Found by running a game's city (a game on the engine) as a server with 150 swarm bots on a Linux test machine (`city_perf.py` of that game; its `--profile-sim` table) and adding timers to `RayWorld::gather` by caller (a scratch build, not kept):

* **The wallhack defence asked every pair again every half second.** It keeps each answer while the signature of the static world (`Sight::signature`) is the same, and 111 of 111 gatherings found the world changed. The drones (`FlyAgent`: a `Collider` and a `Velocity`, no `RigidBody`) counted as static walls and move every tick. They are now left out of the line-of-sight world (`Sight::gather`), and when a change is real (a building streams in with fifty doors, a corpse, a sentry turning) `changed_since` tells it as boxes: mesh levels and hulls are boxes by uid, and more than sixteen boxes are merged by 64 m squares (everything only for terrain or more than sixteen squares). City, 150 bots, 50 s (before -> after): unaffordable pairs a tick 712 -> 0.1, pairs tested a tick 650 -> 500, `occlude` phase median 10.6 -> 8.6 ms, ticks over 33 ms 25/1353 -> 6/1350. What is left in `occlude` is the rays (about 4 500 a tick).
* **Five stages gather the whole static world again every step** (`RayWorld::gather`: scan every collider, sort, build the grid): the vehicles 0.14 ms, the helicopter 0.14, perception 0.29, the flying agents 0.31 and the game's probe 0.37 (it asks for the whole world), 1.2 ms of the 7 ms step, in a world of 1 495 colliders. Where the time of one whole-world gather goes (microseconds each, summed over a run): scanning the colliders 17, measuring and sorting them 91, building the grid 95, the mesh instances 30. A gather with a **reach** (a box) now leaves out the colliders that cannot touch it before anything is worked out for them: the `gather` row of `lunaris bench` (36 mesh shells and about 3 300 boxes, a 60 m box with 425 shapes in it) went from 0.127 to 0.067 ms; the whole world costs 0.347 ms. A gather of a **wide** box (150 x 150 m or more, or none: what the vehicles, the helicopters, the flying agents, perception and the probes ask for in a city) now keeps the static solids and their grid between calls (`StaticSolids`, one per thread): one pass over the colliders adds up a hash of each one's place, size and shape (and the layers); when the sum is the one of the last gathering the same solids are shared (an `Arc`), when it is not they are made again. What flies on its own (a collider and a velocity and no rigid body: a drone) is kept apart in a list of its own and gathered afresh, so that it does not make every call a miss. In the city 87% of the wide gatherings found the world as it was (the rest follow a door, a corpse or the sentry that turns, which are real changes). Whole-world gather in the bench: 0.347 -> 0.107 ms; the city server, 150 bots, the simulation step 7.0 -> 6.4 to 6.5 ms on average and the physics stage (the vehicles and the helicopter) 2.2 to 2.7 -> 1.85 to 1.9 ms (a shared Linux test machine: runs of the same binary differ by 0.2 ms, so these are medians of two to three runs); flying agents and perception did not get faster (their time is not the gather). Tests: the kept solids are the same `Arc` while nothing static changes, a drone moving or a static collider moving does the right thing, and a wide gather, a small one and the kept one answer the same for 400 rays.

## Inside the real city: the server tick, the client tick and the picture (2026-10-25 and 26)

Benchmarks on a built scene tell what a stage costs there; they do not tell what a real game's tick or picture is made of. The paragraphs below come from timing **inside** a game's city (a game on top of the engine), and the method is worth keeping:
* **The server tick**: `lunaris server --scene ... --profile --profile-sim` with `lunaris swarm` bots (the game's `city_perf.py EXE 150 40 700 --profile-sim`), whose report lists the simulation's stages (average and worst step) and the server's own parts. The worst step matters as much as the average (a 15 ms step among 5 ms ones is what a player feels), and it is a different stage from the dearest: put timers inside the stage that has the odd worst step (`std::time::Instant` and an `eprintln!` of what is over 3 ms) in a scratch copy of the tree and run the same scene.
* **The client tick**: the game's `client_profile.py` (the engine's `profile` tool, stage by stage and system by system, offline, no card).
* **The picture**: `LUNARIS_GPU_PROFILE=1` makes the `render` tool's `measured` carry `cpu_ms.*` (gather, items, cull, uniforms and upload, encode, submit, draw_call, read-back) and `gpu_ms.*` (each pass on the card) of the last picture; `LUNARIS_TIMING=1 LUNARIS_TIMING_MIN=0.02` prints every guarded step of the gather and the draw that takes 0.02 ms or more (`[timing] gather: particle clouds 3.2 ms`). A `render` call of a still draws the picture eight times (seven to settle the eye adaptation and the temporal effects, then the one it reports), so the sum of a timer's lines over a call is eight pictures: read the `cpu_ms.*` of the last one.
* Numbers taken while anything else runs on the machine are noise; compare two binaries by running them alternately, several times, in one go.

**The probe no longer gathers the world in a step where nothing casts (2026-10-25, from a game's measurement).** With a `Probe` on each of 150 avatars the physics stage went from 1.19 to 2.55 ms a step whatever the probes asked for, because `probe::step` gathered the whole static world (and the layers) as soon as one probe existed. Now the gather happens when the first probe of the step has something to cast (`up` + `depth` above 0 for the ground, `ray_max` above 0 with a finite direction for the ray) and not otherwise; an uncasting probe reports the same as a cast that found nothing (`over_ground` false, `ground` its own y), its water reading is unchanged. New rows in `lunaris bench` (150 probes in the gather row's city): `probe idle` 0.005 ms and `probe cast` 0.41 ms (one gather and 300 casts); a game's city's whole-world gather costs more than the bench city's (1.3 ms against 0.1), so a game that casts on 2 steps of 16 pays about an eighth of what it paid. Test: `a_step_in_which_no_probe_casts_does_not_gather_the_world_and_one_that_does_gathers_it_once`. Still to do if it matters: keep the `RayWorld` itself between steps when the hash of the static colliders has not changed.

**The tail of the step times in a crowd, and a profiler for it (2026-10-26).** Where the 10 to 100 ms steps of the city server come from, the 210, 300 and 400 bot numbers and the tools that found them (the slowest steps listed with what each was made of in `--profile-sim`, and the sampling profiler `LUNARIS_SAMPLE=file` with `tools/sample_report.py`, which says which function, not only which stage) are in [NETWORK_SCALE](NETWORK_SCALE.md), section "The tail of the step times in a crowd".

## Particle systems: what stepping and drawing cost (2026-10-26)

`ParticleSystem` (docs/FX.md) steps its particles in the picture, in fixed 1/60 s steps, only for the systems that are in view; a server never does it. Measured with `cargo test --release -p lunaris-render how_fast_are_particle_systems -- --ignored --nocapture` on a Linux test machine (one thread for the stepping; the drawing on one and on four; a shared machine, so the second decimal is noise):

| Case | Live particles | A step | Per particle | Drawing (vertices, 1 thread / 4) |
|---|---|---|---|---|
| plain sprites, no forces | 50,000 | 0.96 ms | 19 ns | 200,000: 6.5 ms / 5.1 ms |
| gravity and drag | 50,000 | 0.95 ms | 19 ns | 200,000: 6.3 ms / 6.1 ms |
| curl noise | 50,000 | 3.6 ms | 73 ns | 200,000: 6.4 ms / 5.6 ms |
| the mind (noise, vortex, attractor with kill) | 48,000 | 3.9 ms | 82 ns | 144,000: 4.5 ms / 4.3 ms |
| a bouncing spray with ribbons (6 points) | 32,000 | 2.4 ms | 76 ns | 389,000: 19 ms / 19 ms |

On the card (`"simulate": "gpu"`): the mind of the examples (12,000 motes with curl noise, a vortex and an attractor with `kill`) costs the processor **2.48 ms a picture** when it steps and builds the particles itself (11,035 alive, 36,944 vertices; the stepping on one thread, the mesh on four) and **0.00 ms** when the card does (the processor only schedules the births: 2 particles of the glow live there), `cargo test --release -p lunaris-render how_much_does_the_card_save -- --ignored --nocapture`. A whole picture of it on the RTX 4060 at 640 x 360, high quality, read back to the processor, takes 9.0 ms (the scene, the sky and the read-back are most of that; `cargo test -p lunaris-gpu --test high ten_thousand_motes -- --nocapture`).

Colliding with the world (`collide.world`, `cargo test --release -p lunaris-render how_fast_is_colliding_with_the_world -- --ignored --nocapture`): 40 systems of sparks, 4,800 live particles, among 5,000 box colliders and a ground, cost **0.66 ms a picture against 0.49 ms** without the world; 4 systems and 480 particles, 0.21 against 0.16 ms. The world's colliders are gathered once for all the systems of a picture that ask for them (the big box is the cached gathering of the static world), and each particle then casts the step it took as one ray.

In the bench (`LUNARIS_BENCH_ONLY=fx lunaris bench`, the row `fx campfires`): thirty of the shipped campfires, 2,358 live particles in all, all in view, each stepped one tick and drawn into meshes by `gather_scene`: **0.44 ms a picture** on four threads (the whole CPU side of the effects: stepping, looking, sorting, filling the meshes), measured three times at 0.443, 0.444 and 0.442 ms. The perf gate has the row.

Where the drawing goes (`LUNARIS_TIMING=1 LUNARIS_TIMING_MIN=0`, 50,000 sorted alpha sprites, one thread): looking at the particles (curves, colour, depth) 0.7 ms, the stable radix sort 1.6 ms, filling the mesh 2.4 ms, and the rest is the allocation of a fresh 10 MB mesh for every picture. Four threads help the first and the last part but not the sort. The honest reading: a few thousand particles are free (a campfire is about 80 live: about 0.002 ms a step); the tens of thousands of the AI's mind cost milliseconds and are what a GPU compute pass is for (`simulate: gpu`, below); ribbons are the dearest picture a particle can have (a vertex pair a point). The noise force is the curl of three smooth noise potentials, which share one hash for each of the eight corners of the lattice (it was three hashes: 114 ns became 73 ns a particle); a coarser field would be cheaper still and is the first thing to try if it ever matters.

## The card: what a picture costs (`lunaris bench --gpu`, 2026-10-26)

`lunaris bench --gpu` measures the graphics adapter instead of the core: it draws three scenes at 960 x 540 with the GPU renderer (`render_to_image`: the picture is drawn and read back to the processor) and prints the **wall time** of a picture, the median of 21 (`medium`) or 9 (`high`) after two pictures that pay for compiling the pipelines and uploading the meshes, next to the **time the card's own passes took** (timestamp queries, `gpu_ms.total`). With no adapter it says so and prints nothing. The rows are not in the perf gate (they depend on the card, and a machine without one would fail a missing row); `tools/perf_gate.py` still runs `bench` without `--gpu`.

On the Linux test machine, RTX 4060 (Vulkan, driver 595), three runs, no memory cap on the process (a `ulimit -v` makes a Vulkan process crawl, see below):

| Row | Scene | Wall time of a picture | The card's own passes |
|---|---|---|---|
| `gpu spheres medium` | 1,600 lit spheres and cubes, 359,040 triangles, one picture | 4.08 to 4.18 ms | about 0.6 ms |
| `gpu spheres high` | the same at `high` (a still: eight pictures, seven that settle the anti-aliasing and the bounced light, one kept) | 12.3 to 13.0 ms | about 0.8 ms (the kept one) |
| `gpu campfires high` | the 30 campfires of the `fx campfires` row, a `ParticleSystem` each, one tick between pictures | 8.76 to 8.79 ms | 0.07 ms |

**A frame as a game draws it** (`gpu frame ...` rows, `GpuRenderer::time_frame`): the same three scenes drawn one picture a frame, the anti-aliasing carried over from the frame before, nothing read back to the processor and no entity ids, the call waiting for the card; the median of 22 frames after 8 that settle (two runs, 4060, with another job using the machine's cores):

| Row | Frame, the two added (gather + encoding, wait for the card) | The processor | The card |
|---|---|---|---|
| `gpu frame spheres medium` | 1.87 to 1.91 ms | 0.82 to 0.86 ms | 1.02 to 1.09 ms |
| `gpu frame spheres high` | 2.07 to 2.08 ms | 0.82 to 0.85 ms | 1.23 ms |
| `gpu frame campfires high` | 1.85 to 1.87 ms | 1.28 ms (the particles' gather is 0.44 of it) | 0.56 to 0.58 ms |

A game overlaps the card's work with the next frame's processor work, so a frame costs between the larger of the two and their sum: 1.1 to 2.1 ms for 359,040 triangles at 960 x 540 on this card, which is hundreds of frames a second; a still (rows above) costs two to six times that because it reads the picture back and draws a `high` picture eight times. The window's own cost (presenting, waiting for the screen) is not in these rows: `lunaris play --perf` prints it.

How to read them, honestly:

* **The card is not what these pictures wait for.** 0.6 ms of the 4.1 ms of the medium picture is on the card. The processor's laps (the bench prints them): gathering the scene 0.15 ms, encoding the picture 0.70 ms, waiting for the card and reading the picture, the ids and the depth back 1.95 ms; the other 1.4 ms or so is not under a lap (building the `Frame`'s arrays is the likely part). In the `high` still the seven settling pictures cost the processor 4.9 ms to encode and the wait is 5.8 ms (eight pictures' worth of card work). The campfires' processor part is the particles' 0.44 ms gather. A picture of a game is not read back and the processor's part overlaps the card's, so these rows say what a *still image* costs (what the `render` tool and the tests pay), not what a frame costs in the window; for the window see the real-city numbers above.
* A `high` still is eight pictures; its row is not a frame time.
* The same rows on the RTX 2060 SUPER of the same box read 8.1 ms (medium), 38.8 ms (high) and 18.9 ms (campfires), but that card was training a model at 99% at the time, so the numbers are a card shared with another job, not a measurement of the card.
* **The software adapter** (`LUNARIS_GPU=llvmpipe lunaris bench --gpu`, Mesa's lavapipe, Vulkan on the processor's 8 threads, same box) is the floor for a machine with no graphics card: **210 ms** for the medium sphere picture (all of it in the card's passes: 207 ms), **1.92 s** for the high still, **105 ms** for the campfires. The engine's own CPU reference renderer draws a picture of the same grid in `bench`'s `render 960x540` row in 48 ms on one thread and 29 ms on 8 (the shading is simpler: it is not the same picture), so on a machine with no card the reference renderer, not Vulkan on the processor, is the way to draw (`--cpu`, `--no-gpu`). Not measured: an integrated GPU or any real low-end adapter.

**Never measure or run graphics under `ulimit -v`.** On 2026-10-26 a Linux test machine helper ran every command under a 12 GB virtual-memory cap and the engine looked slow: a 640 x 360 `high` picture took 1.15 s instead of 46 ms (uniform uploads 1 to 137 ms, the card's passes 16 times slower), only after the city had streamed a second place. A Vulkan/NVIDIA process maps far more address space than it uses; without the cap everything was normal. The profile now says what a slow picture was doing: with `LUNARIS_GPU_PROFILE=1` the numbers of a `render` carry `proc.*` (thread CPU time against wall time, major page faults, involuntary context switches), `sys.*` (load, free memory, swap), `cpu_ms.warm_draws` and the laps of the upload.

## The CPU renderer: where a picture goes, and what changed (2026-10-26)

The reference renderer is the floor for a machine with no usable graphics card (the software adapter is five times slower than it, see "The card"), so its time matters. `LUNARIS_BENCH_ONLY=render lunaris bench` runs its two rows (a 960 x 540 picture of 1,600 lit spheres and cubes, 359,040 triangles, median of five after one that warms up) and `LUNARIS_TIMING=1 LUNARIS_TIMING_MIN=0` prints the stages of a picture (`render: the draw list`, `the vertices`, `the bands`, `resolve`). The first version of the rows was one picture timed once, which paid for building the meshes: the 1.6x on four threads it showed was partly that.

Where one thread's 45 ms went (the rasteriser cut short stage by stage, Linux test machine i7-9700K): **17.1 ms** before any triangle reaches the rasteriser (moving the vertices, the per-triangle copies and clipping tests, clearing and resolving), **10.8 ms** setting triangles up (projecting each of the three vertices, again for every triangle that shares it, the area, the bounding box), **8.5 ms** testing pixels against the edges and the depth, **8.7 ms** shading what is visible.

What changed (the pictures are bit-identical: the hash `872dca645d6eccb2` of the grid, the GPU "classic" comparison tests, 216 render tests):

* a vertex is moved to the screen **once** (`Proj`: its position in pixels, depth and 1/w, worked out with the same f64 expressions as before) instead of once for every triangle that uses it, and when a picture has several bands every mesh's vertices are done once on all the threads instead of again in each band the mesh touches;
* a triangle that needs no clipping is no longer copied and run through the near-plane clipper, and the rows of the band are checked first (a triangle that does not reach them costs three loads and four comparisons);
* with that done the bands can be narrower: four per thread (two when a picture has too many vertices to keep them, 2,000,000). Measured on four threads: 8 bands 22.9 ms, 16 bands 15.0, 32 bands 14.9, 64 bands 15.9; on eight threads 32 bands 11.5 ms.

| Row (Linux test machine, median of five) | before | after |
|---|---:|---:|
| render 960x540, one thread | 44.1 ms | 40.0 ms |
| render, 4 threads (the default) | 26.9 ms (1.6x) | 15.3 ms (2.6x) |
| render, 8 threads | 17.7 ms (2.5x) | 11.5 to 14.2 ms (3.1 to 3.9x) |
| render, pinned to 2 cores and 4 GB (`taskset -c 2,3`, `ulimit -v`), one thread | 47.3 ms (2026-10-25 run) | 40.0 to 41.0 ms |
| the same, two threads | (not measured) | 24.2 to 25.9 ms (1.6 to 1.7x) |

Not done: the pixel stage (edge functions in f64 for every pixel of a triangle's box, 8.5 ms) and shading (8.7 ms) are as they were; making them faster without changing a bit of the picture needs an incremental or tiled rasteriser with exact arithmetic, which is its own piece of work. The scaling on more threads stops at about 3x because the bands are unequal (the sphere grid fills the middle rows) and the setup is not parallel.

## Putting bytes on the card: a card that copies slowly (2026-10-26)

While looking for what a city that streams in spends on the card, the timing guards (`LUNARIS_TIMING=1 LUNARIS_TIMING_MIN=0`) showed 2,690 mesh uploads costing 19 seconds on the RTX 4060 of the Linux test machine, 3 microseconds a vertex, while preparing the same meshes (vertex data, clusters, levels of detail) took a tenth of a millisecond each. `cargo run --release -p lunaris-gpu --example upload_cost` (`LUNARIS_GPU=name` picks the card) isolates it: a buffer is made and `queue.write_buffer` fills it, 300 times, no flush in between, the milliseconds a buffer:

| Buffer size | RTX 2060 SUPER | RTX 4060 |
|---|---:|---:|
| 4 KB | 0.004 ms | 0.012 ms |
| 16 KB | 0.007 ms | 0.87 ms |
| 64 KB | 0.017 ms | 3.5 ms |
| 330 KB (a building's part) | 0.065 ms | 17.8 ms |
| 1 MB | 0.30 ms | 24 ms |
| 6 MB | 2.8 ms | 3.5 ms |

The 2060 SUPER copies at 5 GB/s; the 4060 takes 18 ms for 330 KB (18 MB/s), and the same through a buffer mapped at creation (36 ms), and the same again when the writes go into slices of one big buffer (17.9 ms a slice, which rules out the number of buffers and an arena as the fix), so it is how this card's driver (595, Linux) or its slot (the card runs at PCIe 1.0 x8 when idle) takes copies of that size, not something the engine does. The engine's mesh uploads are as cheap as the card lets them be. What follows for measuring: **take streaming and upload numbers on the 2060 SUPER** (`LUNARIS_GPU=2060`), where the first picture at a new place of the city costs 0.9 to 2.6 s of which uploads are 2.7 s over the 18 pictures of the probe, and where the time is spent instead in reading the model files (2.5 s for 288), working out the levels of detail of each new mesh (1.8 s for 2,729) and the gather of names and models (3.0 s, which holds the reading): all processor work that the player does on other threads (`async_prep`). Whether the PC's RTX 3080 shows the slow-copy window is not measured (`upload_cost` is a few seconds on any card and prints the table by itself).

## Start-up, memory and binary size (2026-10-25, Linux test machine, Linux x86-64)

| | |
|---|---|
| `lunaris --version` | 0.00 s, 3.4 MB resident |
| `lunaris call describe` (an engine with its 219 components registered) | 0.00 s, 7.8 MB resident |
| `lunaris` release binary, stripped | **20.3 MB** (CLI, tool API and MCP server, CPU reference renderer, GPU renderer, physics, scripting, networking; no editor). The release profile keeps debug info (`debug = 1`), which is 140 MB: strip it for anything you ship |
| memory an entity costs | 102 bytes (`lunaris bench`: Transform and Velocity, 64 bytes of data) |

The potato budget (resident memory of a small scene, start-up to first frame, scene load) is still to be set on real low-end hardware; these are the numbers of the core on the development machine.

## Where the next speed is (from the profile, in order)

0. Reuse storage: a level load should keep the previous world's column capacity (a cold spawn costs 63 ns per entity against 30 on a warm heap).
1. The remaining add/remove cost: cache the column mapping of a transition (the edge) so a move is a list of copies, and move a row with one pass.
2. Shrink the per-entity record (done for the uid map in result 3; left: the 24-byte slot record and the 8-byte entity list, 48 bytes against hecs' 16).
3. The simulation step (12 ns/entity single thread: `wander` hashes a random number per axis per entity; SIMD and a cheaper counter-based generator are candidates) and the state hash (77 ns/entity after result 3; the floor of its definition is about 34, see result 3).
4. The potato run on a real 2 GHz class machine and the software-rasteriser floor; then memory (resident set at start-up and per 100 000 entities), start-up time and binary size.
5. ~~A performance gate~~ (done 2026-10-25: `tools/perf_gate.py`, see above); running it in CI needs a machine with a stable clock.

## The performance gate

`tools/perf_gate.py` keeps the numbers above from rotting. It runs `lunaris bench --json` three times (`--runs`), keeps the best time of each row, and **fails (exit 1) when a row is slower than the stored baseline of the same machine by more than its tolerance**, or when the memory an entity costs has grown by more than 3%:

```
python3 tools/perf_gate.py                 # compare with tools/perf_baseline.json
python3 tools/perf_gate.py --update        # measure and store the baseline for this machine (when a change is meant to move the numbers: say so in the commit)
python3 tools/perf_gate.py --selftest      # the gate's own comparison logic, no benchmark
```

The baseline (`tools/perf_baseline.json`) has one entry per machine, named by its processor and core count, because a time is only comparable on the machine it was taken on; a machine with no entry gets only the memory check (bytes an entity costs, which does not depend on the machine) and a line saying how to add an entry. Timings are noisy, so a failure is measured again (up to twice, judging on the best of everything) before it counts, and the tolerances are loose on purpose: 30% for the one-thread rows, 60% for the rows that use every core, 100% for rows that are mostly allocation or JSON text or are a fraction of a millisecond. That catches an accidental 2x, not a 5% drift; a 5% change is for the history table above, measured by hand. The current baseline is a Linux test machine's i7-9700K (200 000 entities, best of 5 runs, taken on 2026-10-25 while other jobs ran on the machine, so it is slightly pessimistic). It does not run in CI yet: shared runners have no stable clock, so the gate runs where the machine is known (a Linux test machine, with `python3 tools/perf_gate.py` after a release build).

## History

| Date | Change | Effect |
|---|---|---|
| 2026-10-24 | First baseline, `lunaris bench` and the ECS comparison | tables above |
| 2026-10-24 | ECS round 1 (bundle/batch spawn, add/remove pairing, allocation-free vectorizable queries) | spawn 4.4x, iteration up to 1.6x, fragmented query 2.7x |
| 2026-10-25 | performance gate (`tools/perf_gate.py`, baseline of a Linux test machine) | a row more than its tolerance slower than the stored baseline fails |
| 2026-10-25 | state hash by a plan per archetype | 18.0 to 15.5 ms at 200k entities, same values |
| 2026-10-25 | character controller: the search for the nearest point of a capsule's axis skips triangles that cannot matter (bit-identical), a cheap hash for the grid, the stage's three parts in the profile; new `characters` row | long run 8.6 to 5.9 ms a tick (-31%), short run 1.86 to 1.49 ms; every hash and test unchanged |
| 2026-10-25 | the character controller walks its capsules on several threads (a `View` of the grid for each, results put back in uid order, the pool of `pool.rs` from 24 capsules up) | 200 capsules in the long run: 5.3 -> 1.74 ms a tick on the 8-core Linux test machine (4 threads by default), identical state hash for 1, 2, 4 and 8 threads, every golden unchanged |
| 2026-10-25 | archetype add/remove edges as arrays indexed by component (they were trees) | add_remove 54.5 to 52.4 ns an entity (hecs 43.4) |
| 2026-10-25 | an `f32` field is written as its shortest text that reads back as the same `f32` (scene files and every tool answer) | `0.1` instead of `0.10000000149011612`; readable by a person or an AI, shorter, quicker to write and read; no speed claim (the benchmark scene holds whole numbers) |
| 2026-10-25 | uid table as a dense array; harness warm-up block fixed (33 to 24 MiB); `--memory` for every crate; start-up, memory and binary-size numbers | 135 to 102 bytes an entity, spawn at 200k +28%, snapshot -22%; the 10k suite spawn unchanged; Lunaris is 148 bytes an entity against hecs' 116 |
| 2026-10-26 | `lunaris bench --gpu`: picture times on the card; GPU profile gets `proc.*`/`sys.*` entries | spheres medium 4.1 ms (card 0.6), high still 12.3 to 13.0 ms, campfires 8.8 ms on the RTX 4060 |
| 2026-10-26 | CPU renderer: vertices projected once per vertex and moved once for all bands, rows of the band checked first, four bands per thread; the render rows measure the median of five | one thread 44.1 to 40.0 ms, four threads 26.9 to 15.3 ms, picture bit-identical |
