---
name: lunaris-game-builder
description: Use when building, changing or checking a game or scene with the Lunaris engine through its MCP tools (spawn, render, check, step, define_system, Environment, Animator). Gives the build loop, how to make a picture look good, how to verify behaviour, and the usual mistakes.
---

# Building with Lunaris

You drive a deterministic engine through the `lunaris` MCP tools. Work in small verified steps: every change is followed by a look or a check.

## 1. Orient (once per session)

- `guide` (whole or `topic`), `describe` (what is in the world), `schema` (every component and field), `files` (scenes, models, textures, sounds in the workspace).
- Load an example to learn from: `load_scene {path: "examples/vista.lunaris.json"}`. The workspace is the repository root unless the server was started elsewhere.

## 2. Build

- `spawn {components: {...}}`: always a `Transform`; 3D things add `MeshRenderer`, 2D things `Sprite`/`Tilemap`, hills and mountains `Terrain` from a heightmap PNG (`docs/TERRAIN.md`; `terrain_height` tells where the ground is); a level made in a modelling program (a cave, a town) is a `.glb` shown by `MeshRenderer` and collided with by `MeshCollider` on the same entity (balls roll on it, characters and agents walk on it, `check` says what is wrong with the model; `docs/PHYSICS.md`), and a rock or crate with a shape of its own is a `ConvexCollider` on a model's convex hull; things that must pass through each other (a ghost, a pickup, a bullet that crosses a team's own wall) get a `CollisionLayer` (`layer` and a `mask`; both masks must have the other's layer); a car is a `Vehicle` on a chassis body with a wheel table as text (`docs/PHYSICS.md`, `examples/driving.lunaris.json`); a `Camera` is needed to see anything (without one, `render` uses a default view and says so).
- Behaviour is data: `define_system` with a JSON graph (`docs/GRAPH.md`), or `define_script` with the same system as readable text (`docs/SCRIPT.md`; mistakes come back with a line and column), input through `input`, scores through resources. Do not invent fields: `schema` is the truth, errors carry a `hint`.
- Units are metres, y is up, the camera looks along its local -Z, colours are 0..1.

## 3. Make it look good (GPU renderer, `render` with `quality: "high"` which is the default on a GPU host)

- Add one **`Environment`** entity: `sky` true, `sky_top` / `sky_horizon`, `clouds`, `fog` + `fog_color` (match the horizon colour), `bloom`, `ssao`, `exposure`, and for a cinematic look `dof` (0..1) with `focus` (metres to the sharp point) and `focus_range`. The first directional `Light` is the sun and casts the big shadows (`rot` aims it: it shines along its local -Z). Give it a warm colour and intensity 1.5 to 3.
- Materials on `MeshRenderer`: `metallic` 0 or 1, `roughness` 0.05 (mirror) to 1 (chalk), `texture` (`grass`, `dirt`, `stone`, `bricks`, `wood`, `sand`, `concrete`, `tiles`, `metal`, or a PNG path) with `uv_scale` to repeat it (a 100-unit floor with `uv_scale` [40, 40] shows a tile every 2.5 units), colour alpha below 1 for glass.
- Lamps: a small `MeshRenderer` sphere with `emissive` 4 to 8 plus a point `Light` (`kind` 1, `range` 6 to 12) at the same position; bright things bloom. Spot lights are `kind` 2 with `angle` and `softness`.
- Models: `model_info {path}` for size and clips, then `MeshRenderer.mesh` = the path and, for a skinned model, `Animator {clip, speed, looping}`.
- A flat 2D game needs none of this and keeps its colours exactly as authored.
- `quality: "reference"` draws with the exact CPU renderer (use it when comparing hashes); `classic` is the reference picture on the GPU.

## 3b. Reuse, move, show, play together

- **Prefabs**: anything that appears twice is a prefab: `define_prefab {name, components, children}`, then `spawn_prefab {prefab, at, overrides, count}` or the `spawn_prefab` statement in a graph system (`docs/PREFABS.md`). Scenes can hold `prefabs` and `instances`.
- **Walking**: give a `CharacterBody` (or `Velocity`) entity a `NavAgent` and `set NavAgent {target: [x, y, z], active: true}`; read `status` (2 arrived, 3 no path); `path {from, to}` shows the route first (`docs/NAVIGATION.md`).
- **Speed**: `profile {ticks}` shows where a tick's time goes (stage by stage, graph system by graph system) with advice.
- **Shipping**: `export_game {out, name}` packages the world, every file it names, the player and a launcher into a folder (`check` runs first; `docs/EXPORT.md`).
- **Menus and HUD**: `Ui` entities (rectangles, bars, buttons with `Ui.clicked`); test them by sending `input {values: {mouse_x, mouse_y, mouse_left}}` (`docs/UI.md`). `UiStyle` (rounded corners, gradient, picture, shadow), `TextFont` (a .ttf/.otf for a `Text` or a `Ui` label, with `wrap`) and `UiAnchor` (a name or bar pinned over a character) make it look like a real game: `examples/ui_showcase.lunaris.json`.
- **Animation**: change `Animator.clip` and it crossfades over `fade` seconds.
- **Multiplayer**: lockstep over TCP (`lunaris host` / `lunaris play --join`) or rollback (`--rollback`: guessed inputs, checkpoints, small corrections; also a lobby with `--lobby`, spectators with `lunaris watch`, a seat token to come back, `--sim-latency` to try a bad network); player 1's keys are `input.<key>`, player 2's `input.p2.<key>`. In a rollback game keep all state in components and resources, use `{"rand": ...}` for randomness and do not define or switch systems while it runs; sound and other events of a corrected tick are not repeated (`docs/NETWORK.md`).
- **A language model as a player or NPC**: `lunaris mind` with the verbs `go_to` / `wait` is far stronger for small models than key presses (`docs/MIND.md`).

## 4. Check and look

- `check` after every build step: no camera, a 3D scene with no light and no environment (flat ambient colour), unknown meshes or textures, missing models, clips that do not exist, tile maps with the wrong number of tiles, graph systems that read resources nothing sets.
- `render` at 640x360 to look; the reply lists `visible` entities with pixel counts and boxes, `renderer` says what drew it. `minimap` is a text map; `pick`, `raycast`, `nearby` answer "what is there". To check sound without ears: `acoustics {listener, source}` (room reverberation, what gets through walls, echoes) and `record_audio {ticks, path}` (levels per ear, a timeline, `silent`); sounds are `SoundFile` (event name to WAV), `AudioSource` (loops on an entity) and `AcousticMaterial` on colliders (`docs/AUDIO.md`).
- A black or flat picture usually means: no light, the camera inside something, a mesh scale of 0, or a camera pointing away (the camera looks down -Z).

## 5. Run and prove

- `step {ticks}` and compare `hash`; `snapshot` / `restore` to experiment; `undo` for edits; `record_start` / `record_stop` / `replay` to prove a session replays to the same state.
- `verify` runs a playtest written as JSON (`examples/tests/*.test.json`); write one for any behaviour that matters.
- `save_scene {path}` when it works; scenes are plain JSON in the workspace.

## Mistakes seen before

- Editing the `Transform` of an attached entity (one with `Parent`): edit its `LocalTransform` instead.
- Forgetting `Collider` for contact systems, or leaving physics bodies out of one z plane in 2D.
- Putting a `RigidBody` on a `MeshCollider` and expecting the level to fall (it is static: a thing that moves is its own entity with a `Collider`), or giving a `MeshCollider` a model with a million triangles (the limit; `check` says so): make a simpler copy of the level for collision. Faces that point down are ceilings, not floors, for navigation.
- Putting a light inside its own glowing bulb and expecting the bulb to cast no shadow: bulbs with `emissive` are skipped automatically, other meshes around a light shadow it (set `cast_shadow` false on them).
- Expecting a GPU picture to hash the same on another machine: it will not; use `quality: "reference"`.
