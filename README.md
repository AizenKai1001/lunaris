# Lunaris Engine

A from-scratch game engine in Rust, built so that **AI agents and people use the same front door**.

- **2D and 3D in one engine.** One transform, one world, one renderer path (2D is an orthographic camera and sprites).
- **AI-first.** Everything the editor can do is a tool call (MCP / JSON-RPC), every error says how to fix it, and the whole simulation is deterministic, headless, snapshot-able and replayable.
- **Fast.** Data-oriented archetype ECS, plain-data components stored as columns, no per-frame allocation in the hot path. See the numbers below.
- **Logic as data, any language, visual nodes.** Gameplay can be written as **graph systems**: typed expression/statement graphs in JSON that an AI or a node editor defines at runtime with no compiler (`docs/GRAPH.md`; working today). Any language can drive the engine today: a child process speaking JSON lines (also MCP), a shared live world next to the editor, or an in-process C library (`lunaris_ffi`: C, C++, C#, Go, Python ctypes ...), see `docs/EMBEDDING.md`. Your own compiled code can run *inside* the tick as a **sandboxed WebAssembly module** (`docs/WASM.md`; any language that compiles to WebAssembly, fuel-limited, deterministic, no permission needed) or as a native plugin (`docs/PLUGINS.md`; full speed, opt-in). A node editor will be a *view over the graph text*, not a separate language.

**Status: early (version 0.0.x).** Lunaris is a working engine for small 2D and 3D games, made by one developer working with an AI, and it is **not** a replacement for Unity or Unreal: there is no mature editor, no macOS, mobile, web or console support, a small asset pipeline (glTF models, PNG/JPEG pictures, WAV/Ogg/MP3 sounds, TrueType fonts, Tiled maps) and no large game has been shipped with it yet. What is checked, and where: more than 700 Rust tests plus end-to-end scripts (`python -I tools/check_all.py` runs everything); every example scene's state and reference-picture hashes are identical on Windows 11 and Ubuntu 24.04 (`tools/golden_hashes.txt`, checked by `tools/xplat_hashes.py --check` and by the CI of this repository); the GPU renderer is tried on an RTX 4060 (DirectX 12 on Windows) and an RTX 2060 SUPER (Vulkan on Linux) and compared pixel by pixel with the CPU reference renderer; the native Windows editor `lunaris-studio` is the least tested part. What a version contains:

- **Core**: archetype ECS with runtime schemas, deterministic fixed-step simulation (**the same state and reference-picture hashes on Windows and on Linux** for every example: `tools/golden_hashes.txt`, checked by `tools/xplat_hashes.py --check`), undo, snapshots, recording and bit-exact replay, hierarchy (`Parent` + `LocalTransform`), physics-lite and a character controller (walk, jump, stairs, walls), parallel systems.
- **One tool API** (52 tools) over MCP / JSON-RPC, an in-process C library, a shared live world next to the editor, and Python / JavaScript clients; `verify` runs playtests written as JSON.
- **Logic**: graph systems (JSON, a typed batch vm, a math library, contact systems), sandboxed WebAssembly modules and native plugins (your own compiled code inside the tick).
- **Rendering**: a deterministic CPU reference renderer (2D sprites, tile maps, 3D lit meshes, glTF models with skeletons and animation clips, HUD text) and a GPU renderer on wgpu (Vulkan, DirectX 12, Metal) with two modes: *classic* matches the reference pixel for pixel in tests, *high* looks good (physically based materials, a procedural sky with clouds, cascaded sun shadows and shadows for the two main point lights, ambient occlusion, bloom, fog, depth of field, 4x MSAA, filmic tone mapping; `docs/GPU.md`, `examples/vista.lunaris.json`).
- **Making games**: **prefabs** (entity templates with children, inheritance, spawned from tools, scenes or graph systems; `docs/PREFABS.md`), **navigation** (`NavAgent`, `path`; `docs/NAVIGATION.md`), an **on-screen interface** (`Ui`: panels, bars, buttons that react to the mouse; `docs/UI.md`), animation crossfades, and **multiplayer** over TCP with desync detection: lockstep, or opt-in **rollback** (guess the inputs that have not arrived and go back when a guess was wrong), a lobby, late spectators and players who come back by replay (`lunaris host`, `lunaris play --join`, `lunaris watch`; `docs/NETWORK.md`).
- **Sound that knows the room**: WAV files and looping sources placed in the world, a stereo mixer, and **acoustic ray tracing**: sound is traced through the colliders so walls muffle it, rooms ring (reverberation time measured within about 10% of the textbook formula), and the first echoes come from the side of the wall they bounce off. An agent can check all of it without ears: the `acoustics` tool describes a room and what reaches a listener, `record_audio` mixes the game to a WAV and reports loudness per ear and a timeline (`docs/AUDIO.md`, `examples/echo_tour.lunaris.json`).
- **People**: a browser editor (node view of graph systems, hierarchy, entity menu), a native window with sound (`lunaris play`), a tutorial, example games (Orb Dash, Moon Hopper, a two-player duel, a maze, a menu, an orchard).
- **AI**: `lunaris mind` lets a language model play a body, with permissions, replay without the model, and goals as data (`--success`); against a local 4B model served by ForgeInfer it collects all 12 orbs of Orb Dash in 12 decisions through the `go_to` verb (`docs/MIND.md`).
- **Next**: audio files and 3D mixing, rollback netcode, reflections and temporal anti-aliasing.

Part of the wider **Lunaris** project (an AI model that lives in a game world, and the tools to build both).

## Quick start

```bash
cargo build --release -p lunaris
target/release/lunaris tools                       # the tool list an AI sees
target/release/lunaris call describe               # one tool, printed as JSON
target/release/lunaris run examples/orbits.lunaris.json --steps 600 --twice   # load, simulate, prove determinism
target/release/lunaris bench --entities 1000000    # measure this machine
target/release/lunaris mind examples/arena.lunaris.json --body 1 --goal "reach the Goal" --scripted examples/mind_walk.scripted.json --think-ticks 30   # a model plays a body (offline stand-in)
node tools/mcp_smoke.mjs                           # end-to-end check over MCP
node tools/editor_smoke.mjs                        # drives the editor page in headless Chrome
python -I tools/check_all.py                       # every check in one go (build, tests, smokes, playtests, bots, tutorial, soak)
target/release/lunaris play examples/orb_dash.lunaris.json    # a native window: WASD / arrows, Esc quits (Windows; or double-click play_orb_dash.bat)
target/release/lunaris verify examples/tests/orb_dash.test.json   # a playtest written as data: steps, inputs, expectations, replay check
target/release/lunaris-editor.exe examples/orb_dash.lunaris.json   # the Lunaris Editor, a Windows application (also: `lunaris edit <scene>`): press Play
```

Give an AI agent the engine (Claude Code example):

```bash
claude mcp add lunaris -- target/release/lunaris serve --root path/to/workspace
```

Then it can `describe`, `schema`, `define_component`, `spawn`, `query`, `step`, `snapshot`/`restore`, `undo`, `record_start`/`replay` and more. `lunaris tools` lists them all with descriptions.

## What makes it AI-first

| Property | How |
|---|---|
| One front door | The editor, an AI agent (MCP), the CLI and CI all call the same `Engine::call(tool, args)`. |
| Self-describing | `schema` lists every component with field names, types and sizes; `tools/list` returns JSON Schemas. |
| Behaviour at runtime | **Lunaris Script** (`docs/SCRIPT.md`, a text language that is the same thing as the graph, edited in a Visual Studio Code style tab of the editor) and `define_system` takes a JSON graph (set / if / let / spawn / despawn, arithmetic, comparisons, per-entity randomness) and runs it deterministically every tick: no Rust, no compile, undoable, saved with the scene. See `docs/GRAPH.md`. |
| Components at runtime | `define_component` lets an agent invent a new component type without writing Rust. |
| Errors that teach | Every error has a stable `code`, a `path` into the input, and a `hint` ("did you mean `Transform`?", "fields of Pos: x: f32, y: f32"). |
| Safe experiments | `snapshot` is a cheap copy of the whole simulation; `restore` returns to it; `undo`/`redo` per edit; `batch` is atomic. |
| Deterministic | Fixed time step, seeded RNG, per-entity randomness keyed by uid (not storage order), state hash after every `step`. Two runs with the same scene, seed and commands give the same hash. |
| Provable | `record_start` ... `record_stop` ... `replay` re-simulates from the recorded start and checks the final hash. |
| It can feel and touch | `raycast` (line of sight, aiming), `nearby` (who is close), `contacts` (who is touching whom), plus physics-lite (spheres, boxes and capsules with gravity, bounce and friction: `Collider`, `RigidBody`, `PhysicsSettings` components; with an `AngularVelocity` bodies turn, so balls roll and dominoes topple: `examples/dominoes.lunaris.json`; **levels made of triangles** from a model file with `MeshCollider`, and convex hulls with `ConvexCollider`: `examples/mesh_level.lunaris.json`) a **character controller** (`CharacterBody`: a capsule that walks, jumps, climbs stairs, slides along walls and falls; `examples/walker.lunaris.json`) and **navigation** (`NavAgent`: pathfinding around colliders, up stairs and down drops, one `set` and the entity walks there; `examples/maze.lunaris.json`). All deterministic. |
| It can see | `render` returns the frame as an image (MCP image content) plus which entities are visible with their pixel areas and boxes; `pick` and `project` map pixels to entities and world points to pixels. Same world, same pixels, so frames can be hashed and compared. |
| Humans use the same door | `lunaris edit` serves a browser editor: viewport (the engine's own renderer), inspector, node graph of graph systems, undo, play and snapshot. Every action is a tool call you can watch in its log. An AI agent can join the same live world (`--session-file`, `connect()`), and its edits appear on the person's page by themselves. See `docs/EDITOR.md`. |
| Games are data | Components, graph systems, world **resources** (score, timers, input state) and HUD `Text` entities make a complete game with no compiler. `examples/orb_dash.lunaris.json` is playable from the keyboard and by a bot, and its recording replays without either. See `docs/GAMES.md`. |
| 2D art | `Sprite` has sprite sheets (`sheet`, `frame`: animate by changing `frame` from a graph system); `Tilemap` draws a whole level from a tileset PNG in one layer (`data` is the tile numbers). `examples/moon_hopper.lunaris.json` is a pixel-art platformer built from them, with physics colliders made from the same map. |
| Animation layers | `AnimLayers` blends extra clips over the Animator's (walk to run blend trees, a wave on one arm by joint mask). `docs/GAMEPLAY.md`. |
| UI widgets | `UiLayout` (column, row, grid, nested), `UiSlider`, `UiToggle`, `UiInput` text fields (`type_text` tool), `UiNav` keyboard / gamepad focus on top of `Ui` rectangles. `docs/UI.md`. |
| Behaviour trees | `BehaviorTree` runs selector/sequence/parallel trees with conditions, actions, waits, repeat, invert and cooldown, written in indented text. `docs/GAMEPLAY.md`. |
| Hit points | `HitPoints` keeps hit points in range, regenerates, grants grace time, tracks time since a hit and emits `died` once. `docs/GAMEPLAY.md`. |
| Streaming | `StreamZone` loads and unloads a prefab by distance to the camera: big worlds as many zones. `docs/GAMEPLAY.md`. |
| Level of detail | `Lod` swaps a model for simpler ones by camera distance and stops drawing far things. `docs/GPU.md`. |
| Wind | `Sway` bends a model like leaves in the wind in the GPU renderer's high qualities. `docs/GPU.md`. |
| Skirmish | `examples/skirmish.lunaris.json`: an arena game built only from data and Lunaris Script (hero, hunting grunts, bolts, HUD, sunset, slow-motion game over), with a playtest. |
| Water | `Water`: a see-through sheet with moving waves and sky reflections in the GPU renderer. `docs/GPU.md`. |
| Village example | `examples/village.lunaris.json`: a quest with dialogue, sensor, minimap, particles, music, two languages and a playtest, made only of data and Lunaris Script. `docs/GAMES.md`. |
| Events into the game | `EventWatch`: names of events become resources `event.NAME` (count last tick, `.x .y .z .uid` of the first), so graph systems can react to `emit`s, animation events and dialogue ends. `docs/GAMEPLAY.md`. |
| Debug overlays | `render {debug: [...]}` draws collider shapes, velocities, navigation targets, light reach and sensor cones over the picture, for agents and people finding out why something collides or does not move. `docs/AI_GUIDE.md`. |
| Decals | `Decal`: a picture stuck on a surface by its normal (bullet holes, blood, scorches, footprints), fading out after `life` seconds. `docs/GAMEPLAY.md`. |
| Controllers | `lunaris play` reads Xbox-style game pads (XInput on Windows, the joystick interface on Linux) into `input.gamepad.*` for an `InputMap`, notices plug-in and unplug, and shakes them from `rumble.low` / `rumble.high` resources (Windows). `docs/GAMEPLAY.md`. |
| Dungeon example | `examples/dungeon.lunaris.json`: a top-down crawl on a level made by `generate_map` + `autotile` + `tilemap_colliders`, with pad support, sight-line guards, coins and an exit, and two playtests (one walks the shortest way through the 67-cell level). `docs/GAMES.md`. |
| Level generator | `generate_map`: dungeons (rooms and corridors), caves, mazes and winding caverns from a seed into a `Tilemap`, always connected, with a start, a far exit and spread-out spawn points; integer-only, so a seed is the same level everywhere. `docs/GAMES.md`. |
| Height fog | `HeightFog`: a mist thickest at a chosen height that thins upwards, summed exactly along every ray; valleys fill, hilltops stay clear (GPU high qualities). `docs/GPU.md`. |
| Colour grading | `ColorGrade`: lift / gamma / gain, temperature and tint, and a `fade` colour over the whole picture (fade to black, hit flashes, underwater) in the GPU renderer's high qualities. `docs/GPU.md`. |
| Scene switching | `SceneSwitch` + `Persist`: a door, a menu button or a restart replaces the world with another scene file at the end of the tick, keeping the persistent entities, the resources and (optionally) the systems; errors change nothing. `docs/GAMEPLAY.md`. |
| Steering and flocking | `Steer`: seek with arrive, flee, wander, flocking (separation, alignment, cohesion) by group, fences, facing; 1,000 agents in about 4.5 ms; ninety sheep running from a dog in `examples/flock.lunaris.json`. `docs/GAMEPLAY.md`. |
| Mouse look | `MouseLook` (first-person camera on a `CharacterBody`, or a free camera): `lunaris play` writes `input.mouse_dx/dy/wheel`, holds the pointer when `cursor.lock` is set, and a pad's right stick looks too; the body takes the yaw, the camera the pitch (with limits), `forward`/`right` steer the walk. `docs/GAMEPLAY.md`, `examples/first_person.lunaris.json`. |
| Particle modules | `ParticleModules` beside a `Particles` cloud (Niagara-style): colour and size gradients with any number of stops, wind with gusts, attractors, vortices, noise fields, floor and ball colliders (bounce, stick, kill), ribbon trails and tumbling solids; stepped from birth with a fixed step so a cloud is still a pure function of its clock; 60,000 particles with a floor in 16 ms on 4 threads. `docs/GAMEPLAY.md`, `examples/particle_fx.lunaris.json`. |
| Sprite flipbooks | `SpriteAnim` beside an `Animator` and a `Sprite`: clips as text (`run: 4-9 fps=12 loop step@1`), switched by a state machine or `Animator.clip`, with event markers and a `done` flag. `docs/GAMEPLAY.md`. |
| Tiled import | `import_tiled`: a map from the Tiled editor (JSON) becomes `Tilemap` layers with their tilesets, merged colliders for the solid layers, and a list of the objects (spawn points, triggers) in world coordinates. `docs/GAMES.md`. |
| Tilemap colliders | `tilemap_colliders`: the solid cells of a `Tilemap` become merged static boxes (few, even for a big level) that things stand on, navigate round and hit with rays; run again after the map changes. `docs/GAMES.md`. |
| Autotiling | The `autotile` tool turns a map of ground and nothing into edge and corner tiles (16-tile and 47-tile sets). `docs/GAMES.md`. |
| Moving obstacles | `NavObstacle` on a dynamic body makes navigation plan round it. `docs/NAVIGATION.md`. |
| Ragdolls | A dummy of eleven bodies on ten ball-and-socket joints that goes limp (`examples/ragdolls.lunaris.json`), made from the physics v2 pieces. `docs/PHYSICS.md`. |
| Parallax | `Parallax` on a `Sprite` or `Tilemap` draws it shifted against the camera by a factor: depth for 2D levels without a system. `docs/GAMES.md`. |
| Conversations | `Dialogue`: branching talk written like a screenplay (lines, choices with conditions, jumps, actions, events), shown on `Text`/`Ui` entities, driven by clicks, actions or a counter, optional typewriter. `docs/GAMEPLAY.md`. |
| Camera views | `CameraView`: a second camera draws into a picture named `view:UID` (minimap, security monitor, mirror, portrait, scope), used as a mesh texture or a `Ui` picture; no interface in it, no loops, redrawn every N pictures. `docs/GAMEPLAY.md`, `examples/views.lunaris.json`. |
| Animation events and root motion | `AnimEvents` (footsteps and hit frames fire named events at moments of a clip, once per loop) and `RootMotion` (the animation moves the entity so feet do not slide; `model_info` writes the curves). `docs/GAMEPLAY.md`. |
| Particle clouds | `Particles`: up to 60,000 billboards in one entity (fire, smoke, sparks, rain, snow, bursts), each a closed-form function of the cloud's clock: deterministic, scrubbable, identical in both renderers, additive or sorted-alpha, sprite sheets, velocity stretch. `docs/GAMEPLAY.md`, `examples/particle_clouds.lunaris.json`. |
| Inverse kinematics | `Ik` goals (one empty entity each): a hand or foot reaches a point with a two-bone chain and an elbow/knee pole, a head or turret looks at a point within an angle limit; blended by weight, deterministic, the same in both renderers. `docs/GAMEPLAY.md`. |
| Profiler | The `profile` tool runs ticks and reports the time of every stage (each built-in system, every graph system, navigation, characters, gameplay blocks, interface) with shares and advice, and can time a picture. |
| Hot reload | A texture or model file saved over while the game runs is read again within half a second (a half-written one keeps the old picture and warns); a missing one shows up when it appears. |
| Packaging | `lunaris export scene.json --out dist/game` (or the `export_game` tool): the scene and every file it names in a folder that plays by itself, with the player and launchers. `docs/EXPORT.md`. |
| Interface looks | `UiStyle` (rounded corners, gradients, pictures with fit modes, drop shadows), `TextFont` (TrueType and OpenType text) and `UiAnchor` (names and bars pinned over characters) beside `Ui`. `docs/UI.md`, `examples/ui_showcase.lunaris.json`. |
| Music | `MusicPlayer`: a playlist (files or built-in voices) with equal-power crossfades, shuffle, repeat, pause and skip from graph systems. `docs/AUDIO.md`. |
| Sound buses | `AudioMixer` text: buses with volumes, routes by file pattern, ducking (voices turn music down), settings through `bus.NAME.volume` / `.mute` resources. `docs/AUDIO.md`. |
| Picture effects | `PostFx`: light shafts, motion blur, film grain, contrast and tint in the GPU renderer's high qualities. `docs/GPU.md`. |
| Particle effects | `EmitterFx` beside an `Emitter`: sphere/box/disc starts, drag, turbulence, spin, a bouncing or sticking floor, a middle colour and size. `docs/GAMEPLAY.md`. |
| Paths | `Spline` (smooth or straight, closed or open) and `PathFollower` (platforms, patrols, rail cameras; once, loop or ping-pong). `docs/GAMEPLAY.md`. |
| Timelines | `Timeline` runs a text script of timed steps and ramps (cutscenes, tutorials, traps). `docs/GAMEPLAY.md`. |
| Gameplay blocks | `save_game` / `load_game` and `game.save` / `game.load` resources, `Localization` (`{#key}` texts from `lang/xx.json`), `Lifetime`, `PrefabSpawner`, `TimeOfDay` (sun and sky over a day), the `time.scale` resource (slow motion, pause), `InputMap` (named actions with edges, rebinding), `StateMachine` (animation and AI states in text, driving the `Animator`), `Tween` (14 easings, loop, ping-pong), `CameraRig` (follow, orbit, shake), `Sensor` (distance, view cone, line of sight). `docs/GAMEPLAY.md`. |
| Joints | `Joint` ties a body to another body or a point by a rope, a rod or a spring (swings, chains, bouncing platforms); drawn as a cord when it has a `width`. For bodies that turn it is a ball-and-socket or a hinge with angle limits (doors, chains of capsules). `docs/PHYSICS.md`, `examples/ropes.lunaris.json`, `examples/dominoes.lunaris.json`. |
| Navigation extras | `NavLink` (jumps, ladders, teleporters as extra edges the planner uses), `NavAvoid` (agents keep out of each other's way). `docs/NAVIGATION.md`. |
| Terrain | `Terrain` makes ground from a 16-bit heightmap PNG (Gaea, World Machine and the like export one): drawn by both renderers, stood on by spheres, boxes and characters, hit by rays, with `terrain_height` to put things on it. `docs/TERRAIN.md`, `examples/terrain.lunaris.json`. |
| Mesh colliders | `MeshCollider` turns a model file (`.glb` / `.gltf`: a cave, a town, a racetrack) into static ground, walls and ramps made of its own triangles, kept in a deterministic tree: balls roll down it, boxes and capsules come to rest on it, characters walk and climb on it, `NavAgent`s find routes over it, rays and sensors hit the very triangles that are drawn. 100,000 triangles cost 0.04 ms a tick for a few rolling bodies. `docs/PHYSICS.md`, `docs/NAVIGATION.md`, `examples/mesh_level.lunaris.json` (`tools/build_mesh_level.py`). |
| Collision layers | `CollisionLayer {layer, mask}` is Unity's layer collision matrix and Unreal's collision channels: two entities touch only if each one's mask has the other's layer. It decides rigid-body contacts of every shape (balls, boxes, capsules, hulls, mesh levels, terrain), which triggers fire and which `contacts` / `on_contact` events there are, what a `CharacterBody` walks through, what a `NavAgent` plans round, and what a `Sensor`'s sight is blocked by; `raycast` takes a `mask` or a list of `layers`. A scene without the component is bit for bit what it was (the golden hashes did not change), and `check` names the usual mistakes (a coin the player's mask leaves out). `docs/PHYSICS.md`. |
| Raycast vehicles | `Vehicle` on a chassis (`Collider` + `RigidBody` + `Velocity` + `AngularVelocity`) and a wheel table as text, one wheel a line (`fl: -0.8 -0.15 -1.3 steer=1 brake=1`): each wheel is a ray down the strut, a spring and damper along the ground's normal hold the car up (it rests exactly where its springs say), tyres push along and across the ground inside a friction circle (power oversteer, locked brakes and handbrake drifts come out of that one rule), with an engine (force, power, top speed), brakes, steering that fades with speed, an anti-roll bar, downforce and wheel models that follow the strut. Wheels stand on boxes, hulls, mesh levels and terrain and honour the collision layers; controls are plain fields (`throttle`, `steer`, `brake`, `handbrake`) a graph system sets from an `InputMap`; `render debug: ["wheels"]` draws the rays; `check` catches springs too stiff or too soft for the mass. A 1,200 kg car goes 0 to 100 km/h in 6.3 s, stops from 20 m/s in 17 m and costs about 3 microseconds a tick. `docs/PHYSICS.md`, `examples/driving.lunaris.json` (`tools/build_driving.py`). |
| Convex hulls | `ConvexCollider` gives a dynamic body the shape of a model's convex hull (at most 64 corners) with the mass spread out as a solid of that volume: rocks, crates, wedges and gems that tumble, stack and roll on boxes, balls, capsules, other hulls and mesh levels. `docs/PHYSICS.md`, `examples/mesh_level.lunaris.json`. |
| 3D models | `MeshRenderer.mesh` can be a `.glb` / `.gltf` path in the workspace. The importer is in-tree (`crates/lunaris-render/src/gltf.rs`): node hierarchies, per-material colours, one PNG texture, flat normals when missing; rays hit the model's real triangles. `tools/make_models.py` writes two test models; `examples/models.lunaris.json` shows them. |
| Rollback netcode | `lunaris host --rollback`: every player's copy plays at once with guessed inputs for the ones that have not arrived (a player is assumed to change nothing) and goes back to a saved checkpoint, then plays the last few ticks again, when a guess was wrong. A window of 8 ticks, input delay 2, state hashes compared only on settled ticks; a rollback of 8 ticks costs 0.2 to 1.2 ms in the example scenes. Exact in the end: tests through delaying, reordering proxies match an offline replay of the input log. `docs/NETWORK.md`. |
| Spectators and rejoin | The rollback host keeps the input log (key changes only, 64 MiB at most): `lunaris watch HOST:PORT <scene>` replays a running game from tick 0 at full speed and follows it live, and a player who dropped comes back to its seat with its token by replaying the same log. An empty seat freezes after a grace period (its last keys stay held) so the others never wait for ever. `docs/NETWORK.md`. |
| Lobby | `lunaris host --lobby`: players join with a name, mark themselves ready, the game starts when all are, or when the first seat says `start`; the player list goes to everybody. Lockstep or rollback. `docs/NETWORK.md`. |
| Latency simulator | `--sim-latency MS --sim-jitter MS --sim-loss PCT` on `join`, `watch` and `play --join` (and `LatencyProxy` in Rust): a proxy that delays, reorders and duplicates messages (TCP never loses data, so loss is simulated as lateness) to exercise rollback realistically, in tests or with two windows. `docs/NETWORK.md`. |
| Text everything | Scenes are readable JSON (`save_scene`/`load_scene`), diff-able and editable with ordinary tools. |
| Sandboxed files | File tools only touch paths inside the workspace root. |

## Layout

| Crate | What it is |
|---|---|
| `lunaris-json` | Strict RFC 8259 JSON (no dependencies): parser with line/column errors, ordered objects, writer. |
| `lunaris-ecs` | Archetype ECS: type-erased plain-data columns, generational entity handles + stable uids, runtime component schemas, typed queries, text scenes, state hash. |
| `lunaris-sim` | Fixed-step simulation, native and graph systems (graph systems run as a typed batch vm, checked against the reference interpreter on thousands of random programs; parallel chunks, deterministic for any thread count), physics-lite, RNG, deterministic math, commands with inverses, batches, recording and replay. |
| `lunaris-render` | Reference renderer: a deterministic CPU rasteriser for 3D meshes (lit), 2D sprites, tile maps and HUD text, with an entity-id and a depth buffer, PNG output. Bit-identical images on every machine. `gather_scene` collects what to draw for both this and the GPU renderer. |
| `lunaris-api` | The tool set (`Engine`) and its JSON-RPC 2.0 / MCP transport. |
| `lunaris-mind` | Lets a language model drive one body through the same tool API: observation, permissions, an OpenAI-compatible HTTP client, recorded sessions. See `docs/MIND.md`. |
| `lunaris-wasm` | The sandboxed WebAssembly host (the `wasmi` interpreter in its deterministic profile): modules in any language run inside the tick with a fuel budget, a memory cap and no imports but `rand`; bit-identical to the graph and the native plugin in tests. See `docs/WASM.md`. |
| `lunaris-studio` | **The Lunaris Editor**, a native desktop application (egui, the Unity layout): hierarchy, a scene view drawn by the engine's GPU renderer with move/rotate/scale handles, inspector built from the engine's schema, project browser, console, systems, play mode with sound; and three editors in tabs: **Blueprint** (graph systems as nodes, Unreal's look), **Script** (Lunaris Script, `docs/SCRIPT.md`, in Visual Studio Code's look with its real themes) and **UI** (an on-screen interface designer in Unreal UMG's look). Binary `lunaris-editor`; every click is a tool call. See `docs/EDITOR.md`. |
| `lunaris-audio` | Sound, standard library only: a WAV reader and writer, the built-in synthesiser, a stereo mixer with voices placed in the world, a feedback-delay reverb, a bounding-volume hierarchy and the acoustic ray tracer (room reverberation, occlusion, echoes), and the sound card on Windows. See `docs/AUDIO.md`. |
| `lunaris-pad` | Game controllers, standard library only: XInput on Windows (dlopen, with rumble), the joystick interface on Linux; changes become `gamepad.*` names. |
| `lunaris-net` | Deterministic multiplayer, standard library only: a relay host that does not run the game, and clients that play the same simulation from the same inputs, with state-hash desync detection. Lockstep (the default) or rollback netcode (`--rollback`: checkpoints, guesses, re-simulation), a lobby, spectators and rejoin by replay of the input log, and a latency simulator. `lunaris host`, `lunaris play --join`, `lunaris join`, `lunaris watch`; a two-player example (`examples/duel.lunaris.json`). See `docs/NETWORK.md`. |
| `lunaris-gpu` | The GPU renderer on wgpu (Vulkan, DirectX 12, Metal): classic mode (the reference renderer's picture, compared pixel by pixel in tests) and high-quality mode (PBR, sky, shadows, AO, bloom, fog, MSAA); into images or a window; also draws the entity-id picture. 11 to 18 times faster than the reference even with the read-back. See `docs/GPU.md`. |
| `lunaris-ffi` | The engine as a C library (`lunaris_ffi.dll` / `.so`): five functions taking and returning JSON, header in `include/lunaris.h`; a C# example and a Python `Embedded` class use it. See `docs/EMBEDDING.md`. |
| `lunaris-editor` | The browser editor's server (standard library only): entity list, viewport, inspector, node-graph view of graph systems, a live log of every tool call. Every click is an ordinary tool call. See `docs/EDITOR.md`. |
| `lunaris` | The command line (`serve`, `call`, `run`, `tools`, `schema`, `bench`, `mind`, `edit`, `verify`, `play`, and for multiplayer `host`, `join`, `watch`). `play` is a native window drawn by the GPU (winit, Windows for now; `--quality`): keyboard and mouse become `input` resources, sound effects play. |

The core (`json`, `ecs`, `sim`), the mind crate, the editor server and the C library have **zero third-party dependencies**; the reference renderer uses only the `png` crate for image files; the GPU renderer adds `wgpu` (with its Vulkan and DirectX 12 / Metal backends), `pollster` and `bytemuck`; the native window adds `winit`.

## Measured (release build, one thread, 1,000,000 entities, run at low priority while the PC was in use)

| What | Result |
|---|---|
| Spawn 1M entities (Transform + Velocity, a quarter with Wander) | 177 ms (5.7 M entities/s) |
| One full tick (wander + movement over all 1M), one thread | 7.9 ms = 7.9 ns per entity (127 M entity-updates/s) |
| The same on 8 threads (memory-bound; same state hash) | 4.3 ms (1.8x) |
| Read one component of every entity | 1.9 ms |
| State hash of the whole world | 44 ms |
| Snapshot (clone ~64 MB of component data) | 20 ms |
| Scene export / import, 100k entities (13.4 MB of JSON) | 185 ms / 170 ms |
| A 2-statement graph system over 1M entities | reference interpreter 39 ms; **batch vm 11.4 ms** on one thread (3.4x), 3.7 ms on 8 threads, identical results |
| A native plugin doing a comparable job over 1M entities | 19 ns per entity on one thread, **3.9 ns on 8 threads** (the vm: 7 ns on 8), bit-identical to the graph |
| Physics: 5,000 spheres piling up (5,000 contacts), one thread | 2.6 ms per tick; a 5,000-sphere column: 0.65 ms |
| Render 1,600 lit spheres and cubes at 960x540 (CPU reference renderer) | 33 ms on one thread, 14 ms on 8 (same picture bit for bit) |
| The same scene on the **GPU** at 1920x1080 (RTX 4060), drawn and read back with entity ids | **8.7 ms classic, 10.5 ms high quality** (the CPU reference needs 113 ms on one thread) |

Systems that touch every entity run in deterministic parallel chunks: the result is bit-identical for any thread count (default: half the cores, at most 8; `--threads N` or `LUNARIS_THREADS`).

## Determinism rules for systems

A system is a pure function of `(world, ctx)`. No wall-clock time, no thread-dependent order, no order-dependent floating-point sums across entities, and per-entity randomness through `ctx.entity_rand(uid, salt)`. Floating point is IEEE-exact for `+ - * / sqrt`; avoid libm functions (`sin`, `cos`, `exp`) in simulation code and use `lunaris_sim::detmath` instead (`sin cos tan atan atan2 exp ln`), because the platform's results can differ in the last bits and break replays.

## Docs

- `docs/DESIGN.md` - architecture, decisions, roadmap.
- `docs/GRAPH.md` - graph systems: the logic language.
- `docs/AI_GUIDE.md` - how an AI agent should build with Lunaris (also the `guide` tool).
- `docs/MCP.md` - connecting an agent, and driving the engine from Python or JavaScript (`clients/`).
- `docs/WASM.md` - sandboxed WebAssembly systems: the contract, the guarantees, the numbers.
- `docs/UI.md` - the on-screen interface: `Ui` rectangles, bars, buttons that react to the mouse.
- `docs/NAVIGATION.md` - pathfinding and `NavAgent`: say where, the engine walks there (the `path` tool, the `go_to` mind verb).
- `docs/PREFABS.md` - entity templates: define once, spawn from tools or graph systems, nest and inherit.
- `docs/AUDIO.md` - sound: files, sources placed in the world, acoustic ray tracing, and the tools that let an agent hear without ears.
- `docs/NETWORK.md` - multiplayer: lockstep or rollback over TCP, a lobby, spectators, coming back to a seat, a latency simulator; how to host and join, what it guarantees, what it costs (measured) and what it does not do.
- `docs/GPU.md` - the GPU renderer: the qualities, how to make a scene look good (`Environment`, materials), how classic mode stays identical to the reference, the numbers, the limits.
- `docs/PLUGINS.md` - native plugins: your own compiled code inside the tick (`include/lunaris_plugin.h`, the Rust sample).
- `docs/EMBEDDING.md` - every way into the engine, the C interface, and the C# and Python in-process examples.
- `docs/MIND.md` - a language model playing a body, with permissions and replay.
- `docs/EDITOR.md` - the browser editor and the node graph view.
- `docs/TUTORIAL.md` - your first game in fifteen minutes (Coin Chase), as a runnable script.
- `docs/GAMES.md` - making a game: input, resources, HUD text; the Orb Dash walk-through; **playtests as data** (`lunaris verify`, the `verify` tool).
- `examples/` - sample scenes: `orbits` (runtime component), `showcase` and `sprites` (rendering), `bouncing` (graph systems), `pile` (physics), `dominoes` (rotating bodies: a ball rolls down a ramp into a winding row of dominoes that pushes a hinged door open, and a chain of capsules swings on ball-and-socket joints; `tools/build_dominoes.py`), `driving` (a raycast car on a proving ground of hills, gates, rumble strips, cones, crates and a jump, with collision layers; `tools/build_driving.py`), `arena` (a body, a goal and a rock for the mind demo), `orb_dash` (a complete small game: play it in the editor or let the bot play it), `models` (imported glTF models), `moon_hopper` (2D platformer: tile map, sprite sheet animation, jumping, coins; a bot plays it with `tools/play_moon_hopper.py`), `particles` (a fountain, a campfire and fireworks from `Emitter` entities; `tools/build_particles.py`), `clockwork` (parent/child hierarchy: planets, moons and a moon of a moon, no orbit code; built by `tools/build_clockwork.py` through tool calls).

## Platforms

- **Windows**: everything, including the native window (`lunaris play`, `play_*.bat`) with sound.
- **Linux**: everything, including the native window (`lunaris play`: an X11 window drawn by the GPU renderer, which also works on Wayland desktops through XWayland; tried on Ubuntu 24.04 with KDE Plasma on X11 and an RTX 2060 SUPER over Vulkan) and the sound (PulseAudio's client library, found when the sound is opened: PipeWire provides it too): the engine, the tool API and MCP server, the browser editor (`lunaris edit --web`; the Lunaris Editor itself is a Windows application for now), the CPU and GPU renderers (wgpu on Vulkan), minds, multiplayer host and bots, WebAssembly. Built and tested on Ubuntu 24.04 (rustc 1.99): the whole test suite passes and every example reproduces the same state and picture hashes as on Windows (`tools/golden_hashes.txt`). The Linux window has no CPU fallback (`--cpu` says so): it needs a GPU with Vulkan. macOS and the phones are not done: nothing there could be tried.
- **macOS**: not tried; nothing in the engine is Windows-specific outside the player.

## License

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and what keeps the engine deterministic.

Open source, dual licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option. Run `python -I tools/export_public.py` to make a clean copy of the tracked files for publishing (it scans them for personal paths, e-mail addresses and keys first; it never pushes anything).
