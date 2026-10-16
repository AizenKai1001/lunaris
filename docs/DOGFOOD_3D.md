# Dogfooding Lunaris in 3D: building "Caldera", a whole small 3D game, with only the engine's tools

`docs/DOGFOOD.md` built a 2D game (Depths) this way and found what 2D was missing. This is the same exercise for 3D: one complete third-person island adventure of about ten minutes
(a heightmap island with four biomes, a day and a night, a skinned hero with a sword, a dodge roll and stamina, sleeping stone golems, a boss, night wraiths, six ember shards, a quest-giver,
saving at the fire, a title, pause and settings, game over, a win cutscene, two languages, music, an export), made **only through the tool API, text data, generated assets and Lunaris
Script**, with every file produced by a script in `tools/`. **Every time the engine got in the way it was written down here**: fixed (with a test) when it was small, documented when it
was not. This is the honest list, not a success story.

The game is `examples/caldera/` (scenes, the island's maps, models, sound, language files, `shots/`), made by `tools/build_caldera.py` (library `tools/caldera_lib.py`, scenes
`tools/caldera_scenes.py`, the rules as Lunaris Script `tools/caldera_scripts.py`; the island `tools/make_caldera_world.py`, the models `tools/make_caldera_models.py`, the sound
`tools/make_caldera_audio.py`), played by twelve playtests made by `tools/make_caldera_tests.py` (`examples/tests/caldera_*.test.json`), pictured by `tools/caldera_shots.py`, and proved
as an export by `tools/check_caldera_export.py`. `docs/GAMES.md` ("A sixth game: Caldera") says how it plays and which patterns in it are worth copying; `play_caldera.bat` starts it on Windows.

## The numbers

| | |
|---|---|
| The island | 449 x 449 heightmap over 448 m, 788 entities in the island scene (524 scattered trees and rocks, 6 golems with the boss, 6 wraiths, 16 crabs, 10 gulls, the camp, ruins, the HUD) |
| Playtests | 12 for the game: 204 expectations, **all pass and replay to the same hash** (`replay_check`); the 39 older playtests still pass (51 in all, on the Linux machine) |
| Golden lines | 5 added to `tools/golden_hashes.txt` (boot, title, island, gameover, win), no other line changed; all 51 lines reproduce on the Linux machine |
| Determinism soak | the five scenes: 3000 random-key ticks, 1 thread = 8 threads = replay |
| Export | `tools/check_caldera_export.py`: the exported folder, run hidden for 420 frames on the Linux machine, ends in the same state hash as the workspace copy (`98fdb02db202e024`); no saved game is exported |
| Engine tests added | 7 (`camera_collide_tests.rs` 2, `dogfood3d_tests.rs` 5); `lunaris-sim` 386 and `lunaris-api` 168 pass |
| Pictures | 15 in `examples/caldera/shots/` (1280 x 720, the GPU renderer at `high`, taken with the `render` tool); the first eight are the kept set |
| `check` | the island, the title, the game over, the win and the boot scene: 0 errors, 0 warnings |

## Frame time

Measured on the Linux test machine (RTX 2060 SUPER, Vulkan), release build. **Not measured: the windowed player** (the Linux machine has no display for it and the PC was not to be used for anything heavy).

| What | Time | How |
|---|---|---|
| One simulation tick of the island | **0.73 ms** average, 0.95 ms the worst of 300 | `profile` ({ticks 300}): 788 entities; the game's rules (graph systems) 0.18 ms, gameplay blocks 0.18 ms, characters 0.14 ms, physics 0.14 ms; the biggest single script is `hero_move` at 0.015 ms |
| One 1920 x 1080 picture, `high` quality, read-back included | **11.1 ms** (no post effect), **11.3 ms** with AutoExposure, Ssr, LensFlare and a probe | `crates/lunaris-gpu/examples/fx_cost.rs`, median of 30 pictures of the whole island |
| the same without the trees and rocks | 10.4 ms | they cost under a millisecond |
| the same without trees, terrain and water | 7.2 ms | the terrain, the lake and the sea cost about 3 ms together |
| Ssr, LensFlare, AutoExposure | +0.3 ms, about 0, about 0 | per effect, same tool |
| The `render` tool at 1080p | 56 to 76 ms a call | median of 22 calls at four places; 720p 27 to 38 ms. Most of a call is not drawing (the renderer needs 11 ms); not looked into |
| A `CameraView` (the minimap, a CPU reference picture) | about 35 ms each time it redraws | measured before it was made to redraw once (`every: 600`); it would cost a frame at 60 Hz otherwise |

So a game frame at 1080p is about 12 ms (80 frames a second) on this card, with room to spare for 60 Hz; the engine is not the limit for a game this size. The weak spot is the minimap (below).

## What it took

Wall clock by the machine's clock: **about 1 hour 30 minutes** (6:44 PM to about 8:15 PM), about 470 tool calls, one agent (a context summary in the middle), the build and every test on the Linux machine.
The minutes below are read off the timestamps of the tool calls, so they are rough; "to the first pictures" means a whole island that could be walked on and looked at.

| Part | Roughly | How it went |
|---|---|---|
| Reading `CLAUDE.md`, the docs, the Depths and terrain builders, the character and navigation code | 7 min | Fast: the Depths builder (`lunaris serve` driven from Python) is a good template, and the docs are in step with the code. |
| The island (heightmap, splat map, water map, lava, beaches, a lake, a flattened arena) | 6 min to a first island | The tools did the hard part: one `Terrain` with four `TerrainLayers` from a splat PNG gave a believable island at once. The painful part was making a procedural island *playable* (rows 6 to 9). |
| Models and clips (hero, golem, wraith, crab, gull, trees, rocks, tent, altar, beacon, skinned, with 5 to 8 clips) | 11 min | Pure Python (`write_glb`). The engine took the skinned `.glb` with vertex colours on the first try; the time went on my own mistakes (the winding of faces was wrong in three builders). |
| The rules as Lunaris Script, the scenes and the builder, to the first pictures | 9 min | One graph system per rule worked well; the compile errors were small and came with their position. |
| The orbit camera hitting trees (an engine gap) and the `check` false alarm | 6 min | The only gap that needed new engine code (`CameraCollide`); a build on the Linux machine is 2 minutes. |
| Playtests (a bot on the `path` tool's route, combat, boss, night, swim, death, save, win, talk, pause) and making every place reachable | 10 min | The `verify` format made every test short; the `path` tool made the walking bot easy. Unreachable shards and golems were the most surprising cost (rows 6 to 9). |
| Frame time, the minimap's cost, the camp's art | 5 min | `profile` and `fx_cost` answered quickly. |
| Tuning the look (exposure, fog, sky, camp light) from pictures | 8 min, and a second round at the end | Slow because there is no live view: change, rebuild (a minute), render, read the picture. |
| `check` rules, `profile` pictures, docs, commits | 13 min | |
| The last pass: a rebuild with square props, the camera playtest, the golden lines, the soak, the export proof, the pictures again, this file | 13 min | |

## Findings

`Fixed` says what was done; "(not fixed)" ones have a proposal or the way round that was used. Commits: A `96f8b28` (the world, the models, the rules, the builder), B `2daa9d2`
(`CameraCollide`, the `check` 2D rule), C `bedde2b` and D `aa8a27d` (scenes, tests, pictures), E `b84c8fc` (`check` and `profile`), F `dcaa288` (the final scenes, the camera playtest,
the golden lines), G `716759f` (this file, the games and README entries, the final pictures).

| # | Area | What happened | Fixed? | Commit |
|---|---|---|---|---|
| 1 | camera | The orbit camera (`CameraRig` mode 1) has no collision: behind the hero it ends up inside a tree, a ruin or the side of a hill, which for a third-person game is the first thing anyone sees. | **Fixed**: the component `CameraCollide {radius, min_distance}` beside the rig. After the rig has placed the camera a ray is looked along from the point it looks at to the camera; a solid collider, a terrain, a mesh collider or a hull in the way puts the camera in front of it, and the rig eases it out again where the way is free. A new component at the end of the list, so no existing scene changed. Tests in `camera_collide_tests.rs`, `check` warns about one without a rig, and the game has the playtest `caldera_camera` (which fails without the component: the camera sat 3 m inside the beacon tower). | B, F |
| 2 | camera / docs | The orbit convention (where yaw and pitch put the camera, which way is forward and right, which way the mouse turns it) was not written down anywhere I could find: it was worked out from the code to turn the walking keys by the camera's yaw. | **Fixed**: one paragraph in `GAMEPLAY.md` with the formulas. | B |
| 3 | check | A 3D scene with a minimap (a second `Camera` with `ortho` that is switched off) got a warning on **every** body of the world ("different z"): `check` took the scene for 2D because *some* camera is orthographic. | **Fixed**: the picture is the active camera with the lowest uid, as the renderer does it; test `an_orthographic_minimap_camera_does_not_make_a_3d_scene_2d`. | B |
| 4 | check | `$language.index`, `$language.count` and `$scene.loaded` (resources the engine writes) were reported as "nothing sets this resource" in every game with a language button or a scene switch. | **Fixed** (test `check_knows_the_resources_the_engine_writes...`). | E |
| 5 | profile | `profile` could only time a 640 x 360 picture, which says nothing about the 1080p picture a player sees, and the first picture (meshes and textures built) was mixed into the number. | **Fixed**: `width`, `height` and `pictures`; the first picture is reported alone (`render_first_ms`) and `render_ms` is the median of the rest (test `profile_times_pictures_of_the_size_asked_for`). | E |
| 6 | physics / check | Characters and the path planner see every box as the **axis-aligned box** round it (documented in `PHYSICS.md`). A ruin built from turned walls was sealed from every side, and nothing said so: the golems and shards inside could not be reached (about 8 calls to find). | **Fixed in `check` only**: a warning for long solid boxes turned by an angle that is not a multiple of 90 degrees in a world with a `CharacterBody` or `NavAgent` (not for a square trunk or rock, which is at most twice as big from above: the island's 555 scattered rocks and trees would have drowned it), with a test. **Not fixed**: the behaviour itself (turned walls are still axis-aligned to characters). The game's walls are square to the axes. | E |
| 7 | navigation | The path planner's cell is 0.5 m but "coarser when the area is very large" (documented); on long routes a 2.5 m doorway in a ruin was closed (the golems inside could not come out and the `path` tool found no way in). No `NavAgent` field asks for a finer grid. | **Not fixed**: the doorway was made 4 m. A per-agent `cell` (or a planner that refines near the target) would fix it. | C |
| 8 | procedural worlds | A scattered forest walls things in: a shard, a golem or the camp can end up inside trees and rocks, a slope too steep to climb, or a plateau with no way up. The engine can say whether two places are connected (`path`) but nothing says "everything the game needs is reachable from the start". | **Not fixed in the engine.** The builder does it: `tools/make_caldera_world.py` flood-fills the walkable ground and flattens the shard sites, the arena and two platforms; `caldera_lib.reachable` / `ensure_reachable` ask the `path` tool for every place of the game and thin out the trees round the ones that are shut. A `check` rule that takes the `Hero` start and the entities named in a list, or a `reachable` tool, would save every procedural game this. | C |
| 9 | scatter | `scatter` has no way to keep an area free (a camp, a shard, a start): the builder scatters first, then queries the trees by page and despawns the ones in the way (`clear_around`: 0 to 2 trees removed per place here). | **Not fixed**: an `avoid: [[x, z, r], ...]` argument is the obvious fix (a small change to the spot loop in `scatter.rs`). | C |
| 10 | contacts | `on_contact` does **not** report a `CharacterBody` touching a solid body: probed with a character walking into a wall, 0 reports in 120 ticks, against 27 for the trigger zone on the other side and 9 for a rigid ball dropped on the same wall. Every hit in the game is therefore a `Sensor` or a ray, not a contact. | **Not fixed**: changing it would change the state of any existing game with a contact system that happens to meet a character. Documented here; worth a sentence in `GRAPH.md`. | G |
| 11 | steering | `Steer` writes `Velocity`; a `CharacterBody` moves by `walk` and ignores `Velocity`. So the wraiths, crabs and gulls (free bodies, flocks and seeking) cannot walk on terrain the way the hero does, and the golems (a `CharacterBody` driven by a `NavAgent`) cannot flock. The game uses each where it fits: crabs are small rigid balls that `Steer` over the beach, golems walk with a `NavAgent`. | **Not fixed** (a `Steer` that writes `walk` when the entity has a `CharacterBody`). | A |
| 12 | scripts | A graph system cannot raycast or read the terrain's height, so the things that need the ground under a place are done by the *builder* (`terrain_height`, `path`) or by a `Sensor`: the wraith pool is moved next to the hero at the hero's height, a pick-up's height is baked in. | **Not fixed**: a `terrain_height(x, z)` function in Lunaris Script would remove most of it. | A |
| 13 | time of day | `TimeOfDay` rewrites the sky colours, the sun and the ambient light **after** the graph systems every tick, from a fixed palette: the game cannot tint dusk or make the crater sky redder with a resource, only add fog and exposure on top (`fog_mood`, `eye_mood`). | **Not fixed** (a palette on `TimeOfDay`, or a switch that leaves the sky to the game). | A |
| 14 | rendering | `AutoExposure` brightens a forest in shade towards its target as if it were a dark room: the day looked pale and flat. | **Not fixed** in the engine; the game caps the exposure's `max` by the time of day (`eye_mood`) and the numbers were tuned from pictures. A way to keep the day at a fixed key (or a `target` that follows the sun's height) belongs in the component. | D |
| 15 | cost | The minimap is a `CameraView`, which draws on the CPU with the reference renderer: about 35 ms every time it redraws. | **Not fixed**: the game redraws it once (`every: 600`) since the island does not change; the markers are `Ui` dots placed over it by a script (`map_blip`). A GPU `CameraView` is the real fix. | D |
| 16 | tool | `render` takes 55 to 76 ms a call at 1080p although the renderer needs 11 ms (row 5 numbers), so tuning the look by pictures is slow; there is no live view for an agent. | **Not fixed**, not investigated. | D |
| 17 | playtests | A walking bot needs a "walk to this point" step: the playtest format has `until` on a field, so the bot holds a stick towards each next point of the `path` tool's route until its x or z passes it, and the last metres use a trick (two `until`s in a row, one from each side, `converge`) or `until` on the resource the pick-up sets. It works, and the whole game could not have been tested otherwise, but it is clumsy. | **Not fixed**: a `walk_to` step (or a `bot` block with a route) would make it one line. | C, F |
| 18 | scripts | Small things that each cost a build cycle (2 to 3 minutes on the Linux machine): a resource written in a system reads the *old* value in the same system (documented in `GRAPH.md`; the NPC prompt never worked until the handling was nested in the right branch), `let` cannot be assigned twice, `on` is a reserved word, `hypot` takes two numbers (no 3D length). | **Not fixed**: all are by design or small. | A, C |
| 19 | hit-stop | A hit-stop by `time.scale` also scales `dt`, so a timer in a graph system stretches; the stop has to be counted in ticks, not seconds. | **Not fixed**, a design note (written in `GAMES.md`): the time scale is global. | A |

The first three rows are the ones a 3D game *needs* (a camera that does not go through the world, a docs page for it, a `check` that does not cry wolf); rows 6 to 9 are what makes a procedural
3D world cost the most time, and rows 10 and 11 are traps that cost no time here only because the game was built around them.

## What was easy

* **The ground and the water**: one `Terrain` from a 16-bit heightmap with a splat map of four `TerrainLayers`, one `WaterBody` for the lake and the sea, `terrain_height` and `terrain_layer`
  to ask the world where things go.
* **Light**: `Environment` + `TimeOfDay` + `HeightFog` + `PostFx` + `ColorGrade` + `AutoExposure` gave a day and a night, shafts of light and a believable sunrise without a shader of my own.
* **Characters**: `CharacterBody` (walk, jump, slopes, stairs, swimming through `InWater`), `HitPoints` with `invulnerable` (the roll's grace frames), `Sensor` with line of sight, `NavAgent` over the
  terrain, `StateMachine` for the clips, `BehaviorTree` for the boss. None of them needed a line of engine code.
* **The whole game structure**: `SceneSwitch` + `SceneReset` + `Persist` (boot, title, island, game over, win), `game.save` / `game.load` / `game.probe` (Continue from the title and Retry
  after a death bring the island back), `UiScale`, `UiNav`, `UiSlider` for the settings, `LanguageList` for a language button, `Dialogue` with a speaker as `{#key}`, `Timeline` for the win cutscene.
  Every one of these was a gap in Depths and a fixed one now: the second pass of `DOGFOOD.md` paid off.
* **Testing**: `verify`'s `step` / `input` / `call` / `expect` / `until` and `replay_check`, plus the `path` tool for the walking bot; the soak and the golden hashes for free.
* **Generated assets**: a skinned `.glb` with clips from plain Python, and music from arithmetic, were accepted as they are.

## What was painful

* **No live view.** Everything was built on the Linux machine and looked at as a picture: a round of "change, rebuild (a minute), render, read" for every tweak of exposure, fog or light.
  The look took longer than the rules.
* **The world had to be made playable by hand** (rows 6 to 9): sealed ruins, a doorway the planner closed, slopes the hero and the golems could not climb, shards with trees on top. Each looked like
  a bug in the game and was a property of the world.
* **Things that fail silently** (rows 6, 10, 11): a contact that never comes, a velocity that is ignored, a box that is bigger for a character than it looks. `check` is where these belong.
* **Tuning by number**: the camera, the fog, the exposure, the fire's light and the golem's reach are all numbers; there is no gizmo for a sensor's cone, a nav grid or a camera's distance
  in the engine's own picture.

## What is missing before this could ship

An honest list. Everything below is **not done**, not "done badly".

* **Nobody played it.** The twelve playtests cover each part (a walk to the first shard, a fight, the boss, a night, a swim, a death, saving, the win with the six shards put on the altar
  by setting resources), but **no bot or person has played the whole ten minutes** from the title to the beacon. Pacing, the difficulty of the golems and the boss, how long the walk
  between shards takes, and whether the camera is comfortable are untested.
* **Art.** Every model is procedural low poly (a few dozen boxes and prisms with vertex colours) and there are two textures; the clips are a few keyframes sampled linearly and there is no foot
  placement. A shippable game needs artists.
* **Sound.** Four music loops and three ambiences made from arithmetic at 16 kHz mono, and synthesized effects. I did not listen to them (no audio device), so **how they sound was not checked**; `record_audio` says the title (the boot scene) is not silent and does not clip
  (the title music peaks at -8 dB, 15 seconds) and the island's ambience (waves, fire, lava) peaks at -15 dB with no clipped sample, but the day, night and boss tunes and the effects were not recorded.
* **Hardware.** Gamepad and mouse input is tested only as playtest input: dead zones, a pad's right stick feel and the cursor lock on a real window are unchecked. `play_caldera.bat` and an
  export on **Windows** were never run (the PC was not to be used); the export was proved on Linux only. One GPU (an RTX 2060 SUPER) was measured; a weaker card is unknown.
* **The minimap** is drawn once and does not show golems, the hero's heading or fog of war; a GPU `CameraView` (row 15) would let it live.
* **Content**: one save slot, saving only at the fire; no difficulty options, no remapping, German is a first translation by me, no accessibility options (subtitles for the sound, a colour-blind
  mode), no controller prompts that follow the device.
* **Engine side**: the gaps of rows 7 to 13 would each make the next 3D game cheaper; rows 10 and 11 are traps.
