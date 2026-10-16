# Wyrdwood: the plan

An original 3D survival-and-building game for one to ten players, in the style of the Norse-flavoured survival sandboxes (a big seeded world, biomes that gate your tools, bases you build piece by piece, stamina combat, bosses, boats, a day that matters). It shares a genre with those games and nothing else: no names, models, sounds, maps or code of any existing game. It is built on Lunaris to find out what the engine still lacks, the way Caldera was (`docs/DOGFOOD_3D.md`); every gap found goes in `docs/DOGFOOD_WYRDWOOD.md` as it is met.

## The game in one page

You wash up on the shore of the **Drowned Marches**, an archipelago left by a sea that rose over an old kingdom. Five **Wardens**, one asleep in each wild region, hold the old beacons dark. Wake and beat a Warden and its beacon lights, and the land behind it opens: new ores, new animals, new dangers, new tools. Light all five and the road home appears.

**Pillars** (what every choice is tested against):

1. **Build anywhere, and it has to stand.** Pieces snap together; wood and stone carry weight; a wall with nothing under it falls. A base is a place you made, with a hearth, a bed and a roof.
2. **Stamina fights.** Swing, block, parry, roll. Every blow costs breath; a big animal teaches you to wait.
3. **The land is the progression.** No levels. A biome is a wall until you own the tool that opens it.
4. **A world that looks like weather.** Fog in the valleys, a low red sun, rain on the water, firelight on timber. Lunaris's GPU renderer (`HeightFog`, `AutoExposure`, `Ssr`, `LensFlare`, `TimeOfDay`) is the art direction.
5. **Together, loosely.** One to ten players in the same world, who may come and go; alone is a full game.

**The loop**: gather (wood, stone, fibre, berries) → craft at a station → build a base → cook, rest → explore a new region → meet its Warden → better tools → next region.

## The world

A seeded archipelago, 2 km by 2 km at full size (a 400 m by 400 m island for the first slices). Same seed, same world, on every machine (the generator is deterministic, like the engine).

| Region | Feel | Gives | Danger | Warden |
|---|---|---|---|---|
| **Meadow** | green hills, birches, berries, deer, tusked boars | wood, stone, flint, hide | boars, ill-tempered crows | the **Moss Colossus**, in a ruined ring |
| **Gloomwood** | dense old fir, blue light, mist | iron-bog ore, resin, strong timber | wolves, bark-skinned stalkers | the **Thornmother** |
| **Moor** | swamp, rain, rotten water, ruins | peat iron, reeds, bones | leeches, mire hounds, sickness | the **Drowned Jarl** |
| **Crag** | mountain, snow, cold that hurts | silver, ice stone | frost wolves, cold, rockfall | the **Rime Wyrm** |
| **Barrens** | ash plains, cinder wind | black iron, fire stone | raiders, ash storms | the **Cinder King** |
| **Sea** | open water between the islands | fish, wrecks, new islands | storms, sea beasts | none |

World building uses what exists: Tellus (our terrain tool) makes the height tiles, the splat pictures, the water and the forests as tile files and a scene; Lunaris loads them with `Terrain` + `TerrainLayers` + `WaterBody` and puts trees and rocks with `scatter`. The generator is run once per seed at world creation (a 2 km world is a few seconds of CPU), and the result is saved with the world.

## Systems (what has to exist, and where it stands today)

`have` = in the engine now; `part` = in the engine but not enough; `new` = to build.

| System | Plan | Lunaris today |
|---|---|---|
| Terrain, materials, water | tiles from Tellus; four blended materials; lakes, rivers, sea | **have** (`Terrain`, `TerrainLayers`, `WaterBody`, `Buoyant`) |
| Big world | stream tiles and their scatter in and out around the players | **part**: `StreamZone` streams prefabs; streaming of terrain tiles and forests is **new** |
| Forests with a view | thousands of trees at far distances | **part**: instanced batches exist (100,000 meshes in the benchmark); occlusion culling, LOD tuning and measured forest scenes are **new** |
| Character | walk, run, jump, roll, swim, climb a ledge, fall damage, carry weight | **part**: `CharacterBody` walks, jumps, climbs low ledges; swim, wade, climb, roll are **new** (queue item e) |
| Camera | third person with collision, first-person toggle, aim | **have** (`CameraRig`, `CameraCollide`) |
| Combat | weapon arcs, block, parry, stagger, damage types, knock-back | **part**: `HitPoints`, `Sensor`, contacts; the combat rules themselves are game content |
| Creatures | boars, deer, wolves, birds, fish; herds, packs, flee, aggro, sleep at night | **have** the parts (`Steer`, `Sensor`, `BehaviorTree`, `StateMachine`, `PrefabSpawner`, navigation); content to write |
| Bosses | five Wardens with phases, summons from an altar | **have** the parts; content to write |
| Items and inventory | grid, stacks, weight, hotbar, durability, drag and drop | **part**: UI kit has drag-and-drop slots; the item data and rules are **new** (game content as resources and a table file) |
| Crafting and stations | recipes, a workbench that must be near, stations that upgrade by what is built beside them | **new** (game content on top of a spatial query; `nearby` exists) |
| Gathering | fell a tree (it falls, the logs roll), mine a rock (it breaks into pieces) | **new**: destruction (queue item c) |
| **Building** | snap pieces, support values, integrity falls with distance from the ground, rain exposure, repair, a hearth gives comfort | **new**: the core of the game; first as game scripts, moved into the engine if it measures too slow |
| Survival | health, stamina, three foods at a time that buff, rest bonus, cold and wet | game content |
| Cooking and farming | a fire, a pot, a rack, planted crops | game content |
| Boats | sail and oar, wind, a hull that floats, waves | **part**: `Buoyant`, `WaterBody`, `Vehicle` is a car; a boat controller is **new** |
| Day, night, weather | sun, moon, rain, fog, storms, wind | **have** `TimeOfDay`, `HeightFog`; rain and storms are content |
| Light and look | firelight, torches, shadows, fog, water | **have** (GPU high/ultra) |
| Characters you see | animated people and animals, equipment on the body | **part**: skinned `.glb` with clips works; retargeting and morph targets are **new** (queue item d), needed for equipment and faces |
| Multiplayer | 1 to 10 players, drop in and out, a host or a dedicated server | **part**: `lunaris-net` has lockstep and rollback up to 16 players, but lockstep has no reconnection and a late joiner replays the input log; a **snapshot join** (state at tick N plus the log after it) is **new** |
| Saves | the world, its changes and every player | **part**: scenes and snapshots save; a world save that stores only the changes from the generated world is **new** |
| Audio | footsteps by material, wind, fire, music by region and hour | **have** the mixer and tools; content |
| Menus and HUD | health, stamina, food, hotbar, build menu, map | **have** (UI kit v3); the content |
| Tools for me | playtests, screenshots, golden hashes, `profile` | **have** |

## How it is built

* Like Caldera: models and animation clips as files made by Python builders (`tools/build_wyrdwood*.py`, `write_glb`), plus props from the local model generator (`tools/gen3d.sh`, `tools/glb_fit.py`) where a picture is a faster start than arithmetic; every model fitted to a height and standing on y = 0; sounds from arithmetic; terrain from Tellus.
* Every system comes with a playtest (`examples/tests/wyrdwood_*.test.json`) that plays it with the keyboard and checks the result, a golden scene line (new standard components only, never new fields on old ones), and pictures that I look at before calling anything done.
* The hard rules of the engine stay: the simulation is deterministic (the world generator, the creatures, the building rules and the combat all run inside it), and nothing in the game is allowed to depend on the graphics card.
* One helper builds one slice at a time; I merge it only after the whole check (crate tests, golden hashes on a Linux test machine, every playtest, the Windows type-check, then the public push).

## Milestones

Each one ends with something you can play, a playtest that proves it, and screenshots.

| | What you can do at the end | Engine work it needs |
|---|---|---|
| **M0** (this document) | read the plan | none |
| **M1 Shore** | spawn on a 400 m island, walk, run, swim, roll; day and night; gather wood and stone by felling and mining; hotbar and inventory; craft an axe and a hammer at a workbench; one boar | the character toolkit (swim, roll, wade), felling and breaking (destruction), item and recipe tables |
| **M2 Hearth** | build a house from snapping pieces that has to stand, with a door, a fire, a bed; rest and cook; save and load | the building system with support values; the world save |
| **M3 Blood** | stamina combat with sword, shield, block and parry; boars, wolves, deer; hurt, heal, eat to survive; the first Warden fight | combat rules, creature packs, boss phases; foot IK if the bodies look wrong |
| **M4 Wood** | the whole island with Meadow and Gloomwood from a seed, thousands of trees at a distance, streaming; the second Warden | forest performance (instancing, occlusion, LOD), tile streaming |
| **M5 Together** | two to four players on a host, one joining in the middle of the game | snapshot join, drop-out handling, a dedicated server run |
| **M6 Sail** | a boat you build and sail to a second island with a Moor | boat controller, waves, larger world |
| **M7 Mountain and ash** | Crag and Barrens, the last three Wardens, the beacon ending | content |
| **M8 Polish** | menus, settings, controller, sound, a map, the whole thing at 60 fps on the weakest machine we test | profile and tune |

Roughly: M1 to M3 are the game's first hour and are the first target; M4 to M5 are what make it a real engine test (scale and multiplayer); the rest is content on a finished foundation.

## The engine work, in order, and why

1. **Character toolkit** (swim, wade, roll, climb, foot IK, and the traps Caldera found: contact reporting on `CharacterBody`, `Steer` with `CharacterBody`, script raycast and terrain height). Needed by M1.
2. **Destruction** (felling a tree, breaking a rock, the pieces and the sound). Needed by M1. Cloth waits: sails and banners can be vertex-animated until cloth is wanted.
3. **3D performance** (instancing at forest scale, occlusion culling, LOD, measured with `profile`) and **terrain tile streaming**. Needed by M4; the earlier the better, because everything else is tested on it.
4. **Building** support graph, if the game-script version of it measures too slow with 2,000 pieces.
5. **Snapshot join** for the network. Needed by M5.
6. **Morph targets and retargeting**, when equipment on bodies is wanted (M3 to M7).
7. The `high.rs` parallel hang, whenever it blocks.

## What I will not claim

* That the five Wardens, the five regions or the two kilometres exist before they are played and photographed.
* That it feels good because it works: combat and building are judged by playing them, and the notes of every playtest go in `docs/DOGFOOD_WYRDWOOD.md`, including what is bad.
* That multiplayer works until two real windows on two machines have played a session to the end with the same state hash.
