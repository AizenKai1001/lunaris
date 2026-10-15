# Making a game

A Lunaris game is data: components, graph systems, and a few entities. This page walks through `examples/orb_dash.lunaris.json`, a complete small game, and the pieces it uses.

```bash
lunaris play examples/orb_dash.lunaris.json      # a native window (Windows): WASD or the arrows, Esc quits, F11 full screen
lunaris edit examples/orb_dash.lunaris.json      # the Lunaris Editor (a Windows application): press Play; `lunaris edit --web ...` is the browser page
python -I tools/play_orb_dash.py --check          # a bot plays the same game through the tools, then the run is replayed without it
python -I tools/build_orb_dash.py                 # rebuilds the scene file by calling tools, as an agent would
```

Roll the blue ball with WASD or the arrow keys and collect the 12 gold orbs before the 45 second clock runs out.

## The pieces

| Piece | In Orb Dash |
|---|---|
| Components (plain data) | `Player {speed}`, `Orb {phase}`, `Timer {left}`, `WinBanner`, `LoseBanner`, defined at runtime with `define_component` |
| Standard components | `Transform`, `Velocity`, `MeshRenderer`, `Camera`, `Light`, `Collider`, `RigidBody`, `Text` |
| Graph systems (the rules) | `timer`, `player_move`, `orb_pickup` (a contact system), `orb_bob`, `camera_follow`, `win_banner`, `lose_banner` (`get_system` shows each; the editor draws them as nodes) |
| Input | WASD and arrows become `input.w`, `input.arrowleft`, ... resources; `player_move` turns them into velocity |
| Globals | `score`, `time_left`, and `player.x` / `player.z`, which the player system publishes for the other systems |
| Sound | `{"emit": "coin"}` in `orb_pickup`, `win` / `lose` in `timer`; `Sound` entities define how each name sounds (wave, pitch glide, length, volume), and the editor plays them with WebAudio; in the native player `SoundFile` and `AudioSource` play WAV files placed in the world and traced through the colliders (`docs/AUDIO.md`) |
| HUD | `Text` entities with `{score}` and `{time_left}` placeholders, drawn over the picture by the engine's own renderer |

## How the pieces talk

A graph system sees one entity at a time. Two entities that *touch* are handled by a **contact system** (`on_contact`, see `docs/GRAPH.md`), and entities that need to tell each other other things use **resources**:

```json
{"name": "player_move", "each": ["Player", "Transform", "Velocity"], "do": [
  {"set": "Velocity.lin[0]", "to": {"mul": [{"get": "Player.speed"}, {"sub": [{"res": "input.d"}, {"res": "input.a"}]}]}},
  {"res_set": "player.x", "to": {"get": "Transform.pos[0]"}}]}

{"name": "orb_pickup", "order": 110, "on_contact": {"a": ["Player"], "b": ["Orb"]}, "do": [
  {"res_add": "score", "by": 1},
  {"despawn": "b"}]}
```

The player has a sphere `Collider`, the orbs have trigger colliders, and `orb_pickup` runs once for each (player, orb) pair that overlaps. Resource writes are applied after the system runs, ordered by uid, so thousands of orbs adding to `score` give the same total on any machine, any thread count, any spawn order. (Before contact systems existed the orbs did this with a distance check against `player.x` / `player.z`; both ways work.) `order` decides which system runs first within a tick.

## Humans and AI use the same door

- A person plays in the editor: Play forwards key presses as `input` calls. Escape pauses.
- A program plays through `input` and `step`: `tools/play_orb_dash.py` reads orb positions with `query`, "presses" keys, steps five ticks, repeats, and wins in about 15 simulated seconds.
- Both are recorded as ordinary commands. `record_start` ... `record_stop` ... `replay` re-runs the whole game from the recording alone and checks the final state hash. The bot's run prints `replay without the bot: identical`.
- An AI *mind* (`lunaris mind`, `docs/MIND.md`) can drive a body in a game like this the same way.

## A second game: Moon Hopper (2D)

`examples/moon_hopper.lunaris.json` is a pixel-art platformer: run and jump (Space / W / Up), collect the coins, do not fall in the water. It shows the 2D parts:

- a `Tilemap` draws the whole level from `tileset.png` (`tools/make_tiles.py` draws the pixel art); the solid tiles are also turned into static `Collider` boxes by `tools/build_moon_hopper.py`, one per horizontal run;
- the hero is a `Sprite` with a 4-frame sheet; `hero_move` picks the frame from the tick while running and flips the sprite by the sign of its x scale;
- standing on the ground is a contact system: `hero_ground` sets `Hero.grounded` when the contact normal from the hero to a `Ground` box points down, and `hero_move` reads it next tick to allow the jump; letting go of the key early cuts the jump short;
- coins are trigger colliders, `coin_pickup` is another contact system; falling below the level puts the hero back at the start;
- the `autotile` tool draws the edges for you: write the level as ground (1) and nothing (-1) in the `Tilemap.data`, call `autotile {uid, base, mode}` and every ground cell becomes the tile that fits its neighbours. `mode` `"4"` is the 16-tile set (`base + N + 2 E + 4 S + 8 W`, each letter 1 when the cell above, right, below or left is ground), `"blob"` the 47-tile set (the eight neighbours, a corner counting only when both edges beside it are ground; the 47 results numbered in increasing order of `N + 2 NE + 4 E + 8 SE + 16 S + 32 SW + 64 W + 128 NW`: 0 is an island, 46 a cell with ground all round). Draw the tileset in that order; `solid` lists which numbers are ground, and the map's border counts as ground unless `edge` is `"empty"`. It is undoable, and cheap to call again after the level changes;
- a far hill or a cloud layer is a `Sprite` (or a `Tilemap`) with a `Parallax`: it is drawn shifted against the camera by its `factor` (1 moves with the camera like a fixed backdrop, 0.5 drifts at half speed, 0 is an ordinary sprite), so a camera that follows the hero gives the level depth without any system; `origin` is the camera position where the layer sits exactly at its own `Transform`, and the sprite's real position does not change;
- `python -I tools/play_moon_hopper.py --check` runs a bot that looks at the `Ground` boxes with `query`, jumps gaps and walls, and checks that the recording replays without it.

## A third game: the village (many blocks together)

`examples/village.lunaris.json` (built by `tools/build_village.py`, played by `examples/tests/village.test.json`) is a small quest in a village that uses the newer blocks together: the hero walks with `CharacterBody` and an `InputMap` (WASD), a `CameraRig` follows, a `Sensor` notices the elder (`$near`), E starts a `Dialogue` that is typed out in a TrueType font on rounded `Ui` panels, translated by `Localization` (`lang/village_en.json`, `village_de.json`: the lines are written as `{#elder.hello}` keys), with choices picked by the keys 1 to 3 or a click; accepting sets `$quest`, walking into the bucket by the well (an `on_contact` system) advances it, bringing it back pays 10 coins and emits `quest_done`, which an `EventWatch` turns into the resource `$event.quest_done` that restarts a burst of `Particles`; the elder's name is pinned over their head by a `UiAnchor`, a `CameraView` draws the minimap, a campfire is `Particles` and a `Light`, and a `MusicPlayer` loops a sound. The playtest plays the whole quest with key presses and checks every step, and the recording replays to the same hash. To see it in another language set `Localization.language` to `village_de`.

## A fourth game: the dungeon (a level the engine makes)

`examples/dungeon.lunaris.json` (built by `tools/build_dungeon.py` through the tool API, with the tileset from `tools/make_dungeon_tiles.py`) is a top-down crawl whose level nobody drew: `generate_map` makes 28 rooms and their corridors from a seed, `autotile` turns the walls into the edge tiles of `examples/dungeon_tiles.png`, `tilemap_colliders` turns them into 85 solid boxes, and the hero, six guards, five coins and the golden ring are placed at the `start_world`, `spawns_world` and `exit_world` the tools returned. WASD, the arrow keys or a game pad (`gamepad.lx`/`ly`) walk; a guard whose `Sensor` sees the hero through the open rooms (line of sight is blocked by the wall boxes) runs at it for three seconds after it loses sight and bites on contact (`HitPoints`, 15 damage then 0.8 s of grace in the same tick); coins score; the ring wins. `examples/tests/dungeon.test.json` is generated from the scene (`tools/make_dungeon_test.py` finds the shortest way with a breadth-first search over the tiles and turns it into key presses: 67 cells in six straight stretches) and walks it to the exit; `dungeon_guards.test.json` puts a guard in the start room and expects it to be alerted and to bite. To get another dungeon change `SEED` in the builder and run the three scripts again.

## Making levels: `generate_map`

A level does not have to be drawn by hand. `generate_map {uid, mode, seed, width, height}` makes one from a seed and writes it into the `Tilemap` of entity `uid` (size and `data`; undoable). Without a `uid` it only returns the grid as `data`. Floor cells are `floor` (default 0), rock cells `wall` (default 1, so `autotile` can draw the edges of the rock next; -1 leaves a number empty). The generators use their own seeded integer-only random numbers, not the simulation's, so a seed is the same level on every machine and calling the tool never changes the state of a game it does not write into.

| `mode` | What you get | Knobs |
|---|---|---|
| `rooms` (default) | the map is cut in two again and again, a room is carved in every piece and the two halves of every cut are joined by an L-shaped corridor: a dungeon | `min_room` (4), `max_room` (10), `loops` extra circuits between random rooms (default a quarter of the rooms) |
| `caves` | random rock smoothed by "rock when five of the nine cells round it are", only the biggest cave kept (rolled again when too little is open) | `fill` percent rock at the start (45), `smooth` passes (5) |
| `maze` | a perfect maze (recursive backtracker; the size is made odd) | `braid` percent of dead ends opened into loops (0) |
| `walk` | a drunkard's walk from the middle until `fill` percent of the inside is open: winding caverns | `fill` (40) |

Every open cell can be walked to from the `start` (`connected` is always true). The answer also has the `exit` (the cell farthest from the start by walking, with its `distance` in steps), `spawns` (`points` cells, default 8, each as far as it can be from the start, the exit and the ones before it, at least `clear` steps from the start: places for enemies, chests and pickups), the `rooms` (`x y w h` in cells, for `rooms` mode) and a `hash` that names the level. With a `uid` it also gives `start_world`, `exit_world` and `spawns_world`: the world position of each cell's centre for a map that is not turned or scaled (the map is centred on the entity, the top row first), so a graph system or a `spawn` can put the hero and the enemies there. The default size is the `Tilemap`'s own when it is at least 8 x 8, else 48 x 32; the most is 512 x 512. The sample levels, as text, come from `cargo test -p lunaris-api show_levels -- --ignored --nocapture`.

**Solid ground for a drawn map.** `tilemap_colliders {uid, solid}` turns the cells whose tile number is in `solid` (numbers and/or `[from, to]` ranges, default `[1]`; after `autotile` that is the whole range of the edge tiles, for instance `[[100, 146]]`) into static box colliders, merged into as few boxes as possible (a solid floor of 40 cells in a row is one box; equal runs on rows under each other join into one taller box). The boxes are plain entities named `tile collider <uid>` in the plane of the tilemap (its own rotation and scale are not used), so heroes, balls and crates stand on the level that is drawn, and navigation and `raycast` see it. `components` puts more components on every box (`{"Ground":{}}` for a marker your contact systems look for), `depth` is their thickness across the map. Run it again after the map changes: the boxes made before are removed first, and the whole call is one undo step. The whole chain for a level that is ready to play: `generate_map` (the level), `autotile` (the edges), `tilemap_colliders` (the ground), then spawn the hero at `start_world` and enemies at `spawns_world`.

**Maps from Tiled.** `import_tiled {path, tile, pos}` reads a map exported from [Tiled](https://www.mapeditor.org) as JSON (`.tmj`; choose CSV for the layer data in the map properties): each tile layer becomes an entity with a `Tilemap` (the tileset picture with its columns and rows as the sheet, the tile numbers as gid minus the tileset's first gid, empty cells -1), a layer that has the custom property `solid` set to true (or whose name starts with `solid` or `collision`) also gets the merged static boxes of `tilemap_colliders`, and the objects of the object layers come back in `objects` (name, type, custom properties, position and size in world units, y up, the map centred on `pos`) so that spawn points and doors can become entities. Embedded and external (`.tsj`) tilesets work when each is made of one picture; tile flips are ignored, compressed layer data, image layers and infinite maps are skipped or refused and the `warnings` say so. `tile` is how many world units a tile is (default 1). It is one undo step.

## Playtests as data: `verify`

A bot written in Python is one way to test a game; a **spec** is another that needs no programming language. A spec names a scene and lists what to do and what must be true afterwards. `lunaris verify spec.json` runs it (exit code 1 if anything fails, one PASS/FAIL line per expectation), and the `verify` tool runs the same thing for an agent on a *scratch copy* of the engine, so the live world is never touched. The simulation is deterministic, so a spec that passes once passes every time, and a failing one fails the same way for everyone.

```json
{ "name": "holding D moves the ball and picks up an orb",
  "scene": "examples/orb_dash.lunaris.json", "seed": 1, "replay_check": true,
  "steps": [
    { "expect": { "count": { "with": ["Orb"] }, "eq": 12, "message": "twelve orbs to collect" } },
    { "input": { "d": true } },
    { "step": 60 },
    { "expect": { "resource": "player.x", "between": [5, 6.5] } },
    { "expect": { "events": "coin", "ge": 1 } },
    { "until": { "resource": "time_left", "le": 40, "max_ticks": 400 } },
    { "call": { "tool": "spawn", "args": { "components": { "Transfrom": {} } }, "error": "unknown_component" } } ] }
```

| Step | Meaning |
|---|---|
| `{"step": 60}` | advance 60 ticks |
| `{"input": {"d": true, "space": false}}` | set keys (the `input` tool) |
| `{"call": {"tool": "set_resource", "args": {...}, "error": "code"?}}` | any tool; with `error` the call must fail with exactly that code |
| `{"expect": {target, comparator}}` | check now |
| `{"until": {target, comparator, "max_ticks": 600, "poll": 1}}` | step until it holds; fails if it does not within `max_ticks` |
| `{"note": "text"}` | for readers |

**Targets:** `resource` (name), `entity` (uid) or `name` (entity name) with `field` (`"Transform.pos[1]"`, `"Hero.grounded"`), `exists` (uid or name; true/false), `count` (`{"with": [...], "without": [...]}`: how many entities), `events` (name: how many times it was emitted), `tick`, `hash`, `check` (`"errors"` or `"warnings"`: how many the `check` tool reports). **Comparators:** `eq` `ne` `gt` `ge` `lt` `le`, `approx` `[value, tolerance]`, `between` `[low, high]`; several may be combined; `message` adds your own words to a failure. `"replay_check": true` records the whole run and requires that replaying the recording gives the same state hash. The report lists every expectation with the value seen and, for failures, what was wanted. Samples: `examples/tests/*.test.json` (run by `cargo test`).

## Building your own

1. `define_component` for each kind of thing that needs its own data.
2. `spawn` entities (with `Name`, `Transform`, `MeshRenderer` or `Sprite`).
3. `define_system` for each rule. Test it with `step` and `query`, look at it with `render`, fix it from the error's `path` and `hint`.
4. `Text` entities for the score, timers and messages; show or hide one with a system that sets `Text.visible`.
5. `check` to catch the usual mistakes (no camera, missing textures, a resource nothing sets), `render` to look, and the bot idea above to play it; `tools/soak.py` replays every example with random keys and checks that one thread and eight threads agree.
6. `save_scene` when it works. Everything above is in the scene file, as text.

A step-by-step version of this, with a runnable script, is `docs/TUTORIAL.md` (Coin Chase). What is not here yet: music files and mixing, animation blending.
