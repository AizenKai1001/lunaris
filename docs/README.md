# Lunaris documentation

Lunaris is a deterministic game engine written in Rust, built to be operated by an AI agent as easily as by a person: everything it does is a **tool call** (a name and a JSON object in, a JSON object out), the editor is one more client of those tools, and a speed or realism claim in these pages only counts with a command or a test that reproduces it.

Start with the first four; the rest is by topic. Every path in these pages is checked (`python -I tools/check_docs.py`).

## Start here

| Page | What it is |
|---|---|
| [TUTORIAL](TUTORIAL.md) | Your first game in fifteen minutes (a ball, ten coins, a following camera) |
| [GAMES](GAMES.md) | Making a game: components, graph systems and a few entities, walked through a finished small game; playing a scene in a window with an API port |
| [AI_GUIDE](AI_GUIDE.md) | Building with Lunaris as an AI agent: the tools, the loop (build, `check`, `render`, look), the traps |
| [DESIGN](DESIGN.md) | Goals in priority order and the decisions behind the engine |
| [ROADMAP](ROADMAP.md) | Where the engine is going: performance, the engine-drawn dashboard, the editor, documentation |
| [PERFORMANCE](PERFORMANCE.md) | How fast it is, how that is measured, every number with its command; comparison with other Rust ECS crates; start-up, memory, binary size |
| [COMPARISON](COMPARISON.md) | Lunaris next to Bevy, Godot, Unity and Unreal: only what can be compared fairly, with numbers measured by us or quoted with their source |

## Using the engine from outside

| Page | What it is |
|---|---|
| [MCP](MCP.md) | Connecting an AI agent (`lunaris serve`, the Model Context Protocol over stdin/stdout) |
| [EMBEDDING](EMBEDDING.md) | Using it from any language: the tool API, the clients (Python, JavaScript), the C interface |
| [EXPORT](EXPORT.md) | Packaging a game as a folder that plays on its own |
| [PLUGINS](PLUGINS.md) | Native plugins: your own Rust code inside the tick |
| [WASM](WASM.md) | Sandboxed WebAssembly systems: your own code in any language, no trust required |

## Making things: data, scripts, interface, editor

| Page | What it is |
|---|---|
| [GRAPH](GRAPH.md) | Graph systems: gameplay logic as data, run for every entity that has certain components |
| [SCRIPT](SCRIPT.md) | Lunaris Script: a small text language that compiles to exactly a graph system |
| [PREFABS](PREFABS.md) | Entity templates |
| [UI](UI.md) | The on-screen interface (`Ui`, `Text`, `UiPlot`, scale and safe area) |
| [UI_AUDIT](UI_AUDIT.md) | `ui_audit`: what a picture of a given size would show, for quality checks |
| [EDITOR](EDITOR.md) | The editor: scene, game, inspector, hierarchy, project, console, systems, profiler, and the UI, Blueprint and Script tabs |
| [EDITOR_AUDIT](EDITOR_AUDIT.md) | The editor against Unreal's, part by part: what is there, what is missing, in what order to close the gaps |

## The world: simulation, rendering, sound

| Page | What it is |
|---|---|
| [PHYSICS](PHYSICS.md) | Deterministic physics-lite: bodies, colliders, joints, rays |
| [NAVIGATION](NAVIGATION.md) | Say where; the engine walks there |
| [TERRAIN](TERRAIN.md) | Ground from heightmaps, layers, water |
| [GPU](GPU.md) | The wgpu renderer: PBR, shadows, sky, water, global illumination, compute passes |
| [FX](FX.md) | `ParticleSystem`: Niagara-style particle effects (emitters, curves, forces, collision, events, ribbons, lights, soft particles) written as JSON; ready-made effects in `examples/fx` |
| [AUDIO](AUDIO.md) | Sound as data, places in the world, rays that find out what a room does to it |
| [CITY](CITY.md) | The endless procedural city: no edge, no files, every street a function of a seed |
| [REALWORLD](REALWORLD.md) | Feasibility of the real Earth as a map |

## Gameplay toolkits (plain data, deterministic, hashed, recorded)

| Page | What it is |
|---|---|
| [GAMEPLAY](GAMEPLAY.md) | The building blocks: input map, characters, animation, traversal |
| [COMBAT](COMBAT.md) | Weapons, bullets, teams |
| [AI](AI.md) | Perception, hearing, awareness, squads, searchlights, hacking |
| [STEALTH](STEALTH.md) | Noise by how you move, how hard you are to see |
| [SURVIVAL](SURVIVAL.md) | Hunger, thirst, stamina, bleeding |
| [INTERACT](INTERACT.md) | The use button: doors, containers, keys, items on the ground |
| [LOOT](LOOT.md) · [LOADOUT](LOADOUT.md) · [EQUIPMENT](EQUIPMENT.md) · [GEAR_VISIBLE](GEAR_VISIBLE.md) | Loot tables, guns as items, worn armor, what other players see you wear |
| [BUILDING](BUILDING.md) · [RAIDING](RAIDING.md) · [DEPTH](DEPTH.md) | Snap-together bases, explosives and locks, tiers and repair |
| [RESPAWN](RESPAWN.md) · [PARTIES](PARTIES.md) · [EVENTS](EVENTS.md) · [KEYBINDS](KEYBINDS.md) | Death and a new life, friends who group up, world events, rebinding keys |
| [SEATS](SEATS.md) · [DRONES](DRONES.md) · [HELICOPTER](HELICOPTER.md) · [WALKER](WALKER.md) · [SENTRY](SENTRY.md) | Vehicles and machines |
| [CREATURES](CREATURES.md) · [MIND](MIND.md) | Evolved animals; a language model playing one body |

## Multiplayer and servers

| Page | What it is |
|---|---|
| [NETWORK](NETWORK.md) | A few players: lockstep or rollback, standard library only |
| [NETWORK_SCALE](NETWORK_SCALE.md) | Big servers: 100 to 200 and more players, measured |
| [GAME_SERVER](GAME_SERVER.md) | A scene's own simulation hosted as the server of an online game |
| [LAG_COMPENSATION](LAG_COMPENSATION.md) · [SECURE](SECURE.md) · [ADMIN](ADMIN.md) · [SERVER_ROBUSTNESS](SERVER_ROBUSTNESS.md) | A shot hits what the shooter saw; who may join and encrypted packets; the admin console; what was thrown at the server and what holds |
| [CITY_GAME](CITY_GAME.md) | The procedural city as the world of a multiplayer game |

## Case studies: what building a whole game found in the engine

[DOGFOOD](DOGFOOD.md) (a 2D game), [DOGFOOD_3D](DOGFOOD_3D.md) (a 3D game), [DOGFOOD_WYRDWOOD](DOGFOOD_WYRDWOOD.md) (an open-world plan): each is the honest list of what was in the way.

## Conventions

* Examples are runnable: every scene named here is in `examples/`, built by a script in `tools/`, and the golden hashes in `tools/golden_hashes.txt` fail the test run when an engine change moves one by accident.
* Numbers have a date, a machine and a command. Where another engine's number is quoted it carries its source; nothing is estimated.
* The engine and any game made with it are separate things: nothing in these pages depends on a particular game.
