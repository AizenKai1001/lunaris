//! Tool catalogue: names, descriptions and JSON Schemas. The same list drives MCP `tools/list`, the CLI help and docs.
use lunaris_json::Json;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Json,
}

fn s(desc: &str) -> Json {
    Json::obj([("type", Json::str("string")), ("description", Json::str(desc))])
}

fn int(desc: &str) -> Json {
    Json::obj([("type", Json::str("integer")), ("minimum", Json::from(0)), ("description", Json::str(desc))])
}

fn names(desc: &str) -> Json {
    Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("string"))])), ("description", Json::str(desc))])
}

fn object(desc: &str) -> Json {
    Json::obj([("type", Json::str("object")), ("description", Json::str(desc))])
}

fn tool(name: &'static str, description: &'static str, props: Vec<(&str, Json)>, required: &[&str]) -> ToolDef {
    let input_schema = Json::obj([
        ("type", Json::str("object")),
        ("properties", Json::obj(props)),
        ("required", Json::arr(required.iter().map(|r| Json::str(*r)))),
        ("additionalProperties", Json::Bool(false)),
    ]);
    ToolDef { name, description, input_schema }
}

fn view_props(mut extra: Vec<(&'static str, Json)>) -> Vec<(&'static str, Json)> {
    let num3 = |d: &str| Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("number"))])), ("description", Json::str(d))]);
    let mut v = vec![
        ("width", int("Image width in pixels (default 640, maximum 4096).")),
        ("height", int("Image height in pixels (default 360, maximum 4096).")),
        ("camera", int("uid of a Camera entity to use (default: the active camera with the lowest uid).")),
        ("eye", num3("Explicit viewpoint [x, y, z]; with `eye` the world's cameras are ignored.")),
        ("target", num3("Point to look at (default [0,0,0]).")),
        ("up", num3("Up direction (default [0,1,0]).")),
        ("fov", Json::obj([("type", Json::str("number")), ("description", Json::str("Vertical field of view in degrees for `eye` views (default 60)."))])),
        ("ortho_height", Json::obj([("type", Json::str("number")), ("description", Json::str("Use an orthographic (2D) view this many world units tall."))])),
        ("supersample", int("1 to 3: render at that multiple and average down for smoother edges.")),
    ];
    v.append(&mut extra);
    v
}

pub fn tool_defs() -> Vec<ToolDef> {
    let uid = || int("Stable id of the entity (see `query` or the result of `spawn`).");
    let comp = || s("Component name, for example Transform. Use `schema` to list them.");
    vec![
        tool("describe", "Summary of the running engine: tick, entity count, components, systems, snapshots, workspace folder and the current state hash. Start here.", vec![], &[]),
        tool("schema", "Describe component types: their fields, types and sizes. Without `component`, lists all of them plus the available field types.", vec![("component", comp())], &[]),
        tool(
            "define_component",
            "Define a new component type at runtime. Fields have a type (f32 f64 i32 u32 i64 u64 bool str uid), an optional `count` (fixed array length) and an optional `default`. Components are plain data; a schema cannot be changed after it is defined.",
            vec![
                ("name", s("Component name: letters, digits, underscore; not starting with a digit.")),
                ("doc", s("What the component is for.")),
                (
                    "fields",
                    Json::obj([
                        ("type", Json::str("array")),
                        (
                            "items",
                            Json::obj([
                                ("type", Json::str("object")),
                                ("properties", Json::obj([("name", s("Field name.")), ("type", s("f32 f64 i32 u32 i64 u64 bool str uid")), ("count", int("Array length (default 1).")), ("default", Json::obj([("description", Json::str("Default value."))]))])),
                                ("required", Json::arr([Json::str("name"), Json::str("type")])),
                            ]),
                        ),
                    ]),
                ),
            ],
            &["name", "fields"],
        ),
        tool("spawn", "Create one or more entities with components. Missing fields take the component defaults. Undoable.", vec![("components", object("Object keyed by component name, e.g. {\"Transform\":{\"pos\":[0,1,0]}}.")), ("uid", int("Use this uid instead of the next free one (single entity only).")), ("count", int("Create this many identical entities (default 1)."))], &[]),
        tool("despawn", "Delete an entity. Undoable.", vec![("uid", uid())], &["uid"]),
        tool("get", "Read an entity's components.", vec![("uid", uid()), ("components", names("Only these components (default all)."))], &["uid"]),
        tool("set", "Change fields of a component the entity already has. Only the fields you give change. Undoable.", vec![("uid", uid()), ("component", comp()), ("value", object("Fields to change.")), ("undo_group", s("Optional. Consecutive `set` calls with the same group name become one undo step (a drag made of many small moves)."))], &["uid", "component", "value"]),
        tool("add_component", "Add a component to an entity (missing fields take defaults), or merge fields into it if present. Undoable.", vec![("uid", uid()), ("component", comp()), ("value", object("Initial fields."))], &["uid", "component"]),
        tool("remove_component", "Remove a component from an entity. Undoable.", vec![("uid", uid()), ("component", comp())], &["uid", "component"]),
        tool(
            "query",
            "Find entities by components and field values. Results are ordered by uid. `where` clauses compare a field with a value (== != < <= > >=); array fields need an `index`.",
            vec![
                ("with", names("Entity must have all of these components.")),
                ("without", names("Entity must have none of these components.")),
                ("select", names("Components to return for each entity (default: all of its components; [] returns only uids).")),
                ("where", Json::obj([("type", Json::str("array")), ("description", Json::str("[{component, field, op?, value, index?}]"))])),
                ("limit", int("Maximum entities returned (default 100, maximum 10000).")),
                ("offset", int("Skip this many matches.")),
                ("count_only", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Return only the total."))])),
            ],
            &[],
        ),
        tool("batch", "Apply several commands as one atomic step (all or nothing, one undo). Each command is {op: spawn|despawn|add|set|remove|define, ...} with the same fields as the matching tool.", vec![("commands", Json::obj([("type", Json::str("array")), ("description", Json::str("List of command objects.")), ("items", object("{op, uid?, component?, value?, components?}"))]))], &["commands"]),
        tool("step", "Advance the simulation by fixed time steps and return the new state hash. Deterministic: same scene, seed and commands always give the same hash.", vec![("ticks", int("Number of ticks (default 1).")), ("budget_ms", int("Wall-clock limit for this call in milliseconds (default 20000, at most 600000). If it runs out the call stops early, says so, and you call again to continue."))], &[]),
        tool("verify", "Run a playtest written as data on a scratch copy of the engine (the live world is untouched) and report which expectations hold. A spec is {\"scene\": path, \"seed\": 1, \"replay_check\": true, \"steps\": [...]}; steps are {\"step\": ticks}, {\"input\": {\"d\": true}}, {\"call\": {\"tool\": name, \"args\": {...}, \"error\": code?}}, {\"expect\": target + comparator}, {\"until\": target + comparator + max_ticks}. Targets: resource, entity or name with field (\"Transform.pos[1]\"), exists, count {with, without}, events, tick, hash, check (\"errors\"/\"warnings\"). Comparators: eq ne gt ge lt le approx [value, tolerance] between [lo, hi]. Because the simulation is deterministic, a spec that passes once always passes. See docs/GAMES.md.", vec![("spec", object("The spec, inline.")), ("path", s("Or the path of a spec file in the workspace."))], &[]),
        tool("hash", "The 64-bit state hash (world, tick, seed, RNG). Equal hashes mean identical simulations.", vec![], &[]),
        tool("revision", "A counter that goes up whenever something may have changed the world (any tool that is not read-only, from any client). Cheap: poll it while a person or another agent works in the same world (`lunaris edit --session-file`); when it differs from the number you saw, query again. Also returns the tick and the entity count.", vec![], &[]),
        tool("snapshot", "Save the whole simulation under a name (cheap copy). Use `restore` to return to it; this is how to try something and roll back.", vec![("name", s("Snapshot name."))], &["name"]),
        tool("restore", "Replace the simulation with a snapshot (clears undo history).", vec![("name", s("Snapshot name."))], &["name"]),
        tool("snapshots", "List saved snapshots.", vec![], &[]),
        tool("delete_snapshot", "Delete a snapshot.", vec![("name", s("Snapshot name."))], &["name"]),
        tool("undo", "Undo the last edit (spawn, despawn, set, add_component, remove_component, batch).", vec![], &[]),
        tool("redo", "Redo the last undone edit.", vec![], &[]),
        tool("record_start", "Start recording every command with the tick it ran at, from the current state.", vec![], &[]),
        tool("record_stop", "Stop recording. Optionally save the recording to a file in the workspace.", vec![("path", s("Relative file path, e.g. runs/a.recording.json."))], &[]),
        tool("replay", "Replay a recording (the last one, or from `path`) in a separate simulation and report whether the final hash matches. Proves determinism.", vec![("path", s("Relative path of a saved recording."))], &[]),
        tool("load_scene", "Replace the world with a scene given as a file in the workspace or inline JSON. Atomic: a bad scene leaves the current world untouched.", vec![("path", s("Relative path of a scene file.")), ("scene", object("Inline scene document."))], &[]),
        tool("files", "List the files in the workspace folder (or a folder of it) with what each one is: scene, model, texture, sound, font or other. This is how you find the textures, models and scenes you can use by name. Skips the build folder `target`, `.git` and hidden folders.", vec![("path", s("Folder to list, relative to the workspace (default: the workspace itself).")), ("recursive", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Also list inside sub-folders (default false)."))])), ("kind", s("Only this kind: scene, model, texture, sound, font, other.")), ("limit", int("At most this many entries (default 200, maximum 2000)."))], &[]),
        tool("model_info", "What is in a model file (`.glb` / `.gltf`) of the workspace: vertex and triangle counts, bounds (so you can place and scale it), whether it has a texture, and for a skinned model its skeleton and animation clips (name, duration). Put a clip's name in an entity's `Animator.clip` to play it. For a skinned model it also writes `rig.root_motion`: how far the root joint travels in each clip, as the text a `RootMotion` component takes (the clip lengths are what an `AnimEvents` text needs too).", vec![("path", s("The model's path relative to the workspace, for example models/swayer.glb.")), ("root", s("Optional: the joint whose travel is the root motion (default: the first joint of the skeleton, usually the hips)."))], &["path"]),
        tool("save_scene", "Write the world as a text scene file (or return it inline when no path is given).", vec![("path", s("Relative path, e.g. scenes/level1.lunaris.json."))], &[]),
        tool(
            "define_system",
            "Define (or replace) a graph system: gameplay logic as a JSON graph that runs every tick for each entity having the `each` components, or (with `on_contact`) for each pair of touching entities. No Rust, no compile, undoable, saved with the scene, deterministic. Statements in `do`: set, let, if, despawn, spawn, spawn_prefab, add, remove; expressions: get, var, dt, tick, uid, rand, arithmetic, comparison, logic. Errors carry a `path` and a `hint`. See docs/GRAPH.md.",
            vec![
                ("name", s("System name: letters, digits, underscore. An existing name is replaced.")),
                ("doc", s("What the system is for.")),
                ("each", names("Components an entity must have; their fields are read and written as `Component.field` or `Component.field[index]`. Give `each` or `on_contact`, not both.")),
                ("on_contact", object("Instead of `each`: {\"a\": [components of the first entity], \"b\": [components of the second], \"triggers\": \"all\"|\"only\"|\"solid\"}. Runs once per touching pair (needs Collider components); fields are a.Transform.pos[0], b.Orb.value; use {\"despawn\": \"b\"}, {\"add\": \"C\", \"on\": \"a\"}, {\"contact\": \"depth\"}.")),
                ("without", names("Components an entity must not have.")),
                ("order", Json::obj([("type", Json::str("integer")), ("description", Json::str("Run order among graph systems, lowest first (default 100)."))])),
                ("do", Json::obj([("type", Json::str("array")), ("description", Json::str("Statements run for each matching entity, for example {\"set\": \"Velocity.lin[1]\", \"to\": {\"sub\": [{\"get\": \"Velocity.lin[1]\"}, {\"mul\": [9.8, {\"dt\": true}]}]}}."))])),
                ("enabled", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Start enabled (default true)."))])),
            ],
            &["name", "do"],
        ),
        tool(
            "compile_script",
            "Check Lunaris Script text and turn it into graph systems without changing the world: `{ok: true, systems: [{name, def}]}`, or `{ok: false, errors: [{line, col, end_line, end_col, message, hint}]}`. The script is a readable way to write what `define_system` takes as JSON: `system gravity each Transform, Velocity { Velocity.lin[1] -= 9.8 * dt; if Transform.pos[1] < 0 { Transform.pos[1] = 0; } }`. See docs/SCRIPT.md.",
            vec![("source", s("The script: one or more `system name each A, B without C order N { statements }` blocks."))],
            &["source"],
        ),
        tool(
            "define_script",
            "Define every system of a Lunaris Script text (like `define_system` for each, all or nothing, one undo step). A mistake in the text or one the world finds (an unknown component or field) comes back with its line and column. See docs/SCRIPT.md.",
            vec![("source", s("The script text."))],
            &["source"],
        ),
        tool("get_script", "A graph system written as Lunaris Script text (`source`): the same system `get_system` shows as JSON. Edit it and send it back with `define_script`.", vec![("name", s("Graph system name (see `systems`)."))], &["name"]),
        tool(
            "terrain_height",
            "The height of the ground at a point: the highest `Terrain` under (x, z), with its slope. Use it to put things on the ground (spawn at `height` + half the object's size). An error (`no_terrain`) outside every terrain. See docs/TERRAIN.md.",
            vec![("x", Json::obj([("type", Json::str("number")), ("description", Json::str("World x."))])), ("z", Json::obj([("type", Json::str("number")), ("description", Json::str("World z."))]))],
            &["x", "z"],
        ),
        tool(
            "scatter",
            "Put many copies of something on a terrain in one undoable step: trees, rocks, tufts of grass. Give either `prefab` (a prefab name) or `components` (what each copy has; its Transform scale is multiplied by the random scale). Copies go where the ground suits: slope within `slope` degrees, height within `height`, at least `min_distance` apart, inside `area`; the same `seed` gives the same forest. Returns how many fit (`placed`) and a `note` when fewer than `count` did. See docs/TERRAIN.md.",
            vec![
                ("prefab", s("Name of a prefab to place (see `prefabs`). Use this or `components`.")),
                ("components", object("What each copy has, keyed by component name (a MeshRenderer, a Collider...). Its Transform is set for you (position, rotation, scale).")),
                ("count", int("How many copies to place (1 to 20000).")),
                ("terrain", int("uid of the Terrain entity (default: the first one).")),
                ("seed", int("Seed of the random choice (default 1).")),
                ("slope", Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("number"))])), ("description", Json::str("[min, max] steepness of the ground in degrees (default [0, 35])."))])),
                ("height", Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("number"))])), ("description", Json::str("[min, max] world height of the ground, for instance to keep trees off the beach (default: any)."))])),
                ("area", Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("number"))])), ("description", Json::str("[x_min, z_min, x_max, z_max] to place inside (default: the whole terrain)."))])),
                ("min_distance", Json::obj([("type", Json::str("number")), ("description", Json::str("Least distance in metres between two copies (default 0)."))])),
                ("scale", Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("number"))])), ("description", Json::str("[min, max] random uniform scale (default [1, 1])."))])),
                ("yaw", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Turn each copy by a random angle about the vertical (default true)."))])),
                ("align", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Tilt each copy to the slope of the ground (default false: upright)."))])),
                ("sink", Json::obj([("type", Json::str("number")), ("description", Json::str("Metres to bury each copy below the surface (default 0)."))])),
                ("attempts", int("Tries per copy before giving up (default 30).")),
            ],
            &["count"],
        ),
        tool(
            "autotile",
            "Rewrite the `data` of a `Tilemap` so that every solid cell becomes the tile that fits its neighbours (edges, corners, islands): `base` plus a number that says which neighbours are solid. `mode` \"4\" is the 16-tile set (base + N + 2 E + 4 S + 8 W), `\"blob\"` the 47-tile set (docs/GAMES.md). Cells whose number is in `solid` (default [1]) count as ground; other cells are left alone; the map's border counts as solid unless `edge` is \"empty\". Undoable.",
            vec![
                ("uid", s("The entity with the Tilemap (a uid).")),
                ("base", int("The tile number the set starts at (default 0).")),
                ("mode", s("\"4\" (default) or \"blob\".")),
                ("solid", Json::obj([("type", Json::str("array")), ("description", Json::str("Tile numbers that are ground (default [1])."))])),
                ("edge", s("\"solid\" (default) or \"empty\": what lies beyond the map's border.")),
            ],
            &["uid"],
        ),
        tool(
            "generate_map",
            "Make a level from a seed: `mode` \"rooms\" (default; rooms joined by corridors, with `loops` extra circuits), \"caves\" (smoothed random rock, the biggest cave kept), \"maze\" (a perfect maze, `braid` percent of dead ends opened) or \"walk\" (a drunkard's walk until `fill` percent is open). Every open cell can be reached from the start. Returns the `start`, the `exit` (farthest by walking, with its `distance`), spread-out `spawns` for enemies and items, and the `rooms`. With a `uid` the entity's Tilemap is rewritten (size and `data`, floor `floor` (0), rock `wall` (1); undoable) and the answer also gives the world positions of the cells; without it the grid comes back as `data`. The simulation's random numbers are not used: the same seed gives the same level on every machine. docs/GAMES.md.",
            vec![
                ("uid", s("The entity with the Tilemap to write into (a uid). Leave out to only get the grid back.")),
                ("mode", s("\"rooms\" (default), \"caves\", \"maze\" or \"walk\".")),
                ("seed", int("Which level (default 1).")),
                ("width", int("Cells across, 8 to 512 (default: the Tilemap's size when it is at least 8 x 8, else 48). A maze is made odd.")),
                ("height", int("Cells down, 8 to 512 (default: the Tilemap's size, else 32).")),
                ("floor", int("The tile number of open ground (default 0).")),
                ("wall", int("The tile number of rock (default 1; -1 leaves it empty). `autotile` then draws the edges of the rock.")),
                ("min_room", int("rooms: smallest room side (default 4, at least 3).")),
                ("max_room", int("rooms: largest room side (default 10).")),
                ("loops", int("rooms: extra corridors between random rooms (default a quarter of the rooms).")),
                ("fill", int("caves: percent of rock at the start (default 45); walk: percent of the map to open (default 40).")),
                ("smooth", int("caves: smoothing passes (default 5).")),
                ("braid", int("maze: percent of dead ends opened into loops (default 0).")),
                ("points", int("How many spawn points to pick (default 8, at most 256).")),
                ("clear", int("Spawn points are at least this many steps from the start (default 6).")),
            ],
            &[],
        ),
        tool(
            "tilemap_colliders",
            "Make a `Tilemap` solid: the cells whose tile number is in `solid` (a list of numbers and/or [from, to] ranges; default [1]) are merged into as few boxes as possible and a static `Collider` box (shape 1) is spawned for each, named `tile collider <uid>`, so heroes, balls and crates stand on the level that is drawn and navigation and `raycast` see it. Run it again after the map changes: the boxes made before are removed first (`clear` false keeps them). `components` adds more components to every box (for instance a marker your systems look for), `depth` is the boxes' size across the map (default the tile size). The map's own rotation and scale are not used. One undoable step. docs/GAMES.md.",
            vec![
                ("uid", s("The entity with the Tilemap (a uid).")),
                ("solid", Json::obj([("type", Json::str("array")), ("description", Json::str("Tile numbers that are solid: numbers and/or [from, to] pairs (default [1])."))])),
                ("components", Json::obj([("type", Json::str("object")), ("description", Json::str("Extra components for every box, like {\"Ground\":{}}."))])),
                ("depth", Json::obj([("type", Json::str("number")), ("description", Json::str("Size of the boxes across the map (default: the tile size)."))])),
                ("clear", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Remove the boxes this tool made for this map before (default true)."))])),
            ],
            &["uid"],
        ),
        tool(
            "import_tiled",
            "Bring a map from Tiled (the JSON export, .tmj or .json) into the world: every tile layer becomes an entity with a `Tilemap` (the tileset picture, its sheet, the tile numbers; paths made relative to the workspace), a tile layer with the custom property `solid` true (or a name starting with `solid` or `collision`) also gets merged static box colliders, and the objects of the object layers (spawn points, triggers, doors) come back as a list in world coordinates (y up, the map centred on `pos`) with their name, type and custom properties, for you to turn into entities. Layers with compressed data, image layers and infinite maps are not supported (the answer's `warnings` says what was left out). One undoable step. docs/GAMES.md.",
            vec![
                ("path", s("The .tmj / .json map, relative to the workspace.")),
                ("tile", Json::obj([("type", Json::str("number")), ("description", Json::str("World units per tile (default 1)."))])),
                ("pos", Json::obj([("type", Json::str("array")), ("items", Json::obj([("type", Json::str("number"))])), ("description", Json::str("[x, y, z] of the middle of the map (default [0, 0, 0])."))])),
            ],
            &["path"],
        ),
        tool(
            "profile",
            "Run some ticks and report where the time goes: the average and slowest tick, and every stage of a tick (each built-in system, the graph systems one by one, navigation, characters, gameplay blocks, the interface) with its average and worst time and share of the total, plus advice when the game cannot keep up with 60 ticks a second or spikes. The ticks really run (the game advances). With `render` true it also times one picture. Times are wall-clock measurements of this machine.",
            vec![("ticks", int("How many ticks to run (default 120, at most 3600).")), ("render", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Also time one 640x360 picture with the current render quality."))]))],
            &[],
        ),
        tool(
            "export_game",
            "Package the game: write the world as a start scene (`<name>.lunaris.json`) plus every file it uses (textures, models, sounds, fonts, heightmaps, language files: found from what the scene names) into a folder that plays by itself, with the player program (when this is `lunaris`), `play.bat`, `play.sh` and a README. The workspace is not changed. Runs `check` first and refuses when it reports errors (unless `force`). A folder that already has files in it is only written to with `overwrite`. See docs/EXPORT.md.",
            vec![
                ("out", s("Folder to write, inside the workspace (default dist/game).")),
                ("name", s("Name of the game: the scene is `<name>.lunaris.json` (letters, digits, `_`, `-`; default game).")),
                ("player", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Copy the player program next to the scene (default true; only done when this process is `lunaris`)."))])),
                ("overwrite", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Write into a folder that already has files in it."))])),
                ("force", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Export even though `check` reports errors."))])),
            ],
            &[],
        ),
        tool(
            "save_game",
            "Save the game: the whole world with its resources, the tick and the random generator, to `saves/<slot>.json` of the workspace (a slot is a name or a number, a number N is `slot<N>`). `load_game` brings it back exactly; a game can also ask for it from inside by setting the resource `game.save` (or `game.load`) to a slot number 1 to 99. See docs/GAMEPLAY.md.",
            vec![("slot", s("The slot: a name of letters, digits, `_` and `-` (up to 40), or a number."))],
            &["slot"],
        ),
        tool("load_game", "Replace the world with a saved game (see `save_game`, `saves`). Clears undo history; refuses while a recording is running.", vec![("slot", s("The slot to load."))], &["slot"]),
        tool(
            "type_text",
            "Type into the focused text field (a `UiInput`): one character per tick, then Enter when `enter` is true. Click the field first (set `input.mouse_x`, `input.mouse_y`, `input.mouse_left` with `input`, step, release, step). Advances the simulation by one tick per character. See docs/UI.md.",
            vec![("text", s("What to type (up to 200 characters).")), ("enter", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Press Enter after the text (submits the field)."))]))],
            &["text"],
        ),
        tool("saves", "List the saved games of the workspace: slot, size in bytes, tick and number of entities.", vec![], &[]),
        tool("reload_terrain", "Read every terrain heightmap again from the workspace (after a file changed, for instance a new export from Gaea or World Machine). Reports what was read and why anything could not be.", vec![], &[]),
        tool("remove_system", "Delete a graph system. Undoable.", vec![("name", s("Graph system name (see `systems`)."))], &["name"]),
        tool(
            "enable_system",
            "Turn a graph system on or off without deleting it. Undoable.",
            vec![("name", s("Graph system name (see `systems`).")), ("enabled", Json::obj([("type", Json::str("boolean")), ("description", Json::str("true to run it, false to pause it."))]))],
            &["name", "enabled"],
        ),
        tool(
            "define_prefab",
            "Define (or replace) a prefab: a named entity template. `components` are the root entity's components, `children` an array of nodes ({name?, components, children?, prefab?}) spawned attached to it (a child with a `LocalTransform` and no `Transform` is placed right away), `prefab` inherits another prefab. Spawn it with `spawn_prefab`, or from a graph system with the `spawn_prefab` statement. Undoable, saved with the scene, deterministic. See docs/PREFABS.md.",
            vec![
                ("name", s("Prefab name (1 to 64 characters). An existing name is replaced.")),
                ("components", object("The root entity's components, keyed by component name; partial values use the component defaults.")),
                ("children", Json::obj([("type", Json::str("array")), ("description", Json::str("Child nodes: {\"name\": \"Crown\", \"components\": {\"LocalTransform\": {\"pos\": [0, 2, 0]}}, \"children\": [...], \"prefab\": \"Other\"}."))])),
                ("prefab", s("Inherit this prefab's components and children (your own `components` replace its fields, your `children` come after its).")),
            ],
            &["name"],
        ),
        tool("remove_prefab", "Delete a prefab definition (entities already spawned from it stay). Undoable.", vec![("name", s("Prefab name (see `prefabs`)."))], &["name"]),
        tool("prefabs", "List the prefabs (name, how many entities one spawn makes), or with `name` read one definition and the entities it expands to.", vec![("name", s("Prefab name."))], &[]),
        tool(
            "spawn_prefab",
            "Spawn a prefab: the root and all its children, with the next free uids, in one undoable step (all or nothing). `at` sets the root's position, `overrides` replaces fields of the root's components, `count` makes several copies at the same place. Returns the root uids.",
            vec![
                ("prefab", s("Prefab name (see `prefabs`).")),
                ("at", Json::obj([("type", Json::str("array")), ("description", Json::str("[x, y, z] for the root's Transform.pos."))])),
                ("overrides", object("Replace fields of the root's components: {\"Transform\": {\"scale\": [2, 2, 2]}, \"Name\": {\"value\": \"Boss\"}}.")),
                ("count", int("How many copies (default 1).")),
            ],
            &["prefab"],
        ),
        tool("systems", "List the systems that run each tick: native ones, and graph systems with their components, order, enabled flag and the last runtime error.", vec![], &[]),
        tool("get_system", "Read one graph system, including its full JSON definition (`def`), to inspect or edit it.", vec![("name", s("Graph system name (see `systems`)."))], &["name"]),
        tool(
            "path",
            "Find a walkable path between two points (or entities) around the solid colliders: up stairs, onto platforms, down short drops, never through walls. Returns waypoints [x, y, z] (feet positions), the length, whether the target itself is walkable (`exact`) and how many cells were searched. To make an entity walk there, set its `NavAgent` component (target + active); this tool only shows the route. See docs/NAVIGATION.md.",
            vec![
                ("from", Json::obj([("description", Json::str("Start: [x, y, z], or the uid of an entity (its position; its NavAgent, if it has one, supplies the agent's size; its CollisionLayer, if it has one, says which colliders are in its way)."))])),
                ("to", Json::obj([("description", Json::str("Target: [x, y, z], or the uid of an entity."))])),
                ("radius", Json::obj([("type", Json::str("number")), ("description", Json::str("Agent radius in metres (default 0.4)."))])),
                ("height", Json::obj([("type", Json::str("number")), ("description", Json::str("Agent height (default 1.8)."))])),
                ("step_height", Json::obj([("type", Json::str("number")), ("description", Json::str("The highest step it climbs (default 0.4)."))])),
                ("max_drop", Json::obj([("type", Json::str("number")), ("description", Json::str("The longest drop it accepts (default 1.2)."))])),
            ],
            &["from", "to"],
        ),
        tool("contacts", "Overlaps found by the physics system in the last tick: pairs of entities with the contact normal (from a to b), penetration depth and whether either collider is a trigger. Set up physics with Collider and RigidBody components (see `schema`); gravity comes from a PhysicsSettings component on any entity.", vec![("limit", int("Maximum contacts (default 100)."))], &[]),
        tool(
            "raycast",
            "Cast a ray through the world and report the nearest thing it hits (meshes and sprites using their exact shapes, so it agrees with `render`, and physics colliders, including the triangles of a `MeshCollider`, which are the ones its model is drawn with): entity, distance, hit point and surface normal. Line of sight, aiming and picking.",
            vec![
                ("origin", Json::obj([("type", Json::str("array")), ("description", Json::str("[x, y, z] start of the ray."))])),
                ("direction", Json::obj([("type", Json::str("array")), ("description", Json::str("[x, y, z] direction (need not be normalised)."))])),
                ("max_distance", Json::obj([("type", Json::str("number")), ("description", Json::str("Ignore hits farther than this (default 1000)."))])),
                ("include", s("What the ray can hit: `visuals` (meshes and sprites), `colliders` (physics shapes) or `both` (default).")),
                ("mask", Json::obj([("type", Json::str("integer")), ("description", Json::str("Collision layers the ray can hit, as a bit mask (bit n = layer n of `CollisionLayer`; an entity without one is layer 0): 6 hits layers 1 and 2 and sees through everything else. Default: all layers."))])),
                ("layers", Json::obj([("type", Json::str("array")), ("description", Json::str("The same as `mask`, as a list of layer numbers (0 to 31): [1, 2]. Give `mask` or `layers`, not both."))])),
            ],
            &["origin", "direction"],
        ),
        tool(
            "acoustics",
            "Hear the scene without ears: trace sound through the physics colliders (walls, floors, objects; `AcousticMaterial` sets how much each absorbs and passes). Reports the room around the listener (reverberation time RT60 in seconds, how enclosed it is, an estimate of its size and a one-line verdict) and, when `source` is given, what reaches the listener from there: how much of the direct sound gets through walls and how muffled it is, whether there is a clear line of sight, and the first echoes with delay, level and the direction they seem to come from (azimuth 0 = ahead, 90 = right, 180 = behind). The listener is the active camera unless `listener` is given.",
            vec![
                ("listener", Json::obj([("type", Json::str("array")), ("description", Json::str("[x, y, z] where the ears are (default: the active camera)."))])),
                ("source", Json::obj([("description", Json::str("A sound source: an entity uid (its Transform position) or [x, y, z]. Without it only the room is reported."))])),
                ("rays", int("Rays used to measure the room (default 192, 16 to 4096): more rays, a steadier number.")),
                ("source_rays", int("Rays used for the echoes of the source (default 96).")),
            ],
            &[],
        ),
        tool(
            "record_audio",
            "Run the simulation for `ticks` ticks and mix everything the game would have sounded like (events with their `SoundFile` or built-in voice, `AudioSource` entities, placed in the world, muffled by walls, with echoes and the reverberation of the room) into a stereo 44.1 kHz WAV file. Returns levels (peak, left and right loudness in dB, clipping), a loudness timeline every quarter of a second, which sounds started and how often, and the measured RT60 of the room, so you can check that a sound happens, when, and from which side. This advances the simulation like `step`.",
            vec![
                ("ticks", int("Ticks to run (default 300, maximum 3600 = one minute).")),
                ("path", s("Where to write the WAV file (relative to the workspace). Without it nothing is written and only the report comes back.")),
                ("acoustics", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Trace sound through the scene (default true); false = distance and left/right only."))])),
            ],
            &[],
        ),
        tool(
            "nearby",
            "Entities within `radius` of a point (by their Transform position), nearest first. Optionally only those with certain components.",
            vec![
                ("point", Json::obj([("type", Json::str("array")), ("description", Json::str("[x, y, z] centre."))])),
                ("radius", Json::obj([("type", Json::str("number")), ("description", Json::str("Search radius in world units."))])),
                ("with", names("Only entities that also have all of these components.")),
                ("limit", int("Maximum results (default 50, maximum 1000).")),
            ],
            &["point", "radius"],
        ),
        tool(
            "render",
            "Render the world to an image and see it. Returns the picture (as an image for MCP clients), a hash, and which entities are visible with their pixel areas and bounding boxes. Uses the active Camera entity, or an `eye`/`target` view you give without touching the world. 2D sprites and 3D meshes are drawn together. `quality` picks the renderer: `reference` is the CPU renderer, slower but identical on every machine (deterministic hash); `classic`, `low`, `medium`, `high` and `ultra` use the GPU when the host has one (high and ultra add physically based materials, a sky, sun shadows, ambient occlusion, bloom, fog and filmic tone mapping, steered by an `Environment` entity and each mesh's `metallic`/`roughness`/`texture`). The reply says which renderer drew the picture.",
            view_props(vec![("debug", Json::obj([("type", Json::str("array")), ("description", Json::str("Draw debug lines over the finished picture, always in front: \"colliders\" (the shapes physics uses: gray static, green dynamic, blue characters, yellow triggers), \"velocity\" (red), \"targets\" (where navigation agents are going, magenta), \"lights\" (the reach of point and spot lights, orange), \"sensors\" (range and view cone, cyan), \"wheels\" (the rays, struts and wheels of `Vehicle`s: green on the ground, red in the air), or \"all\"."))])), ("quality", s("reference (CPU, deterministic), classic, low, medium, high or ultra (GPU), or auto: what this host uses by default (high on a GPU host).")), ("path", s("Also save the PNG to this relative path in the workspace.")), ("image", Json::obj([("type", Json::str("boolean")), ("description", Json::str("Return the image itself (default true)."))])), ("visible_limit", int("Maximum entries in `visible` (default 20)."))]),
            &[],
        ),
        tool("pick", "Which entity is at pixel (x, y) of a render with the same view parameters?", view_props(vec![("x", int("Pixel column, 0 at the left.")), ("y", int("Pixel row, 0 at the top."))]), &["x", "y"]),
        tool(
            "minimap",
            "A text map of the world around a point, one character per cell, for reading the layout at a glance (a language model reads this well). `@` is the entity in `around`, other things get the first letter of the first word of their name (see `legend`), `+` marks several things in one cell, `.` is empty. Over a `Terrain` the empty cells show the ground: letters `a` (lowest) to `j` (highest) in the top view (`relief` gives the heights), `#` under the ground in the side view. Top view: right is +x, up on the map is -z. Side view: up is +y. Things larger than the map (a floor) are left out.",
            vec![
                ("around", int("Centre the map on this entity (marked `@`).")),
                ("center", Json::obj([("type", Json::str("array")), ("description", Json::str("Centre point [x, y, z] when `around` is not given (default the origin)."))])),
                ("radius", Json::obj([("type", Json::str("number")), ("description", Json::str("Half the width of the map in world units (default 10)."))])),
                ("cells", int("Cells per side, 3 to 81 (default 21).")),
                ("view", s("\"top\" (default) or \"side\".")),
                ("with", names("Only entities that have all of these components.")),
            ],
            &[],
        ),
        tool("check", "Look over the whole world for mistakes that would otherwise show up as a blank picture or a value stuck at 0: no camera, a drawn thing without a Transform, unknown meshes, missing model or texture files, tile maps with the wrong number of tiles, graph systems that failed or read resources nothing sets, text showing unset resources. Returns `problems` (each with a severity, the entity and a hint). Run it after building something and fix what it lists.", vec![], &[]),
        tool("guide", "How to build things with Lunaris: the loop (describe, schema, build, check, render, step, record), the world model, writing behaviour as graph systems, input, contacts, events, seeing, checking your work, pitfalls. Without `topic` returns everything; with one returns that section.", vec![("topic", s("loop, world, behaviour, seeing, checking or pitfalls (a part of the name is enough)."))], &[]),
        tool("project", "Where does a world point appear in the image for this view? Returns pixel coordinates (y down) and whether it is on screen.", view_props(vec![("point", Json::obj([("type", Json::str("array")), ("description", Json::str("[x, y, z] in world units."))]))]), &["point"]),
        tool(
            "set_resource",
            "Set (or remove, with null) a world resource: a named global number such as `score`, a timer, or a key's state. Graph systems read them with {\"res\": \"score\"} and change them with res_set / res_add. Undoable, saved in scenes.",
            vec![("name", s("Resource name: letters, digits, underscores, dot-separated parts (for example `score` or `level.time_left`).")), ("value", Json::obj([("description", Json::str("A number, true/false (1/0), or null to remove it."))]))],
            &["name"],
        ),
        tool("events", "Events emitted by graph systems with {\"emit\": \"name\"} (sounds, messages for the outside), newest `limit`, oldest first. `step` also returns the events of that call. Each has the `pos` of the emitting entity at that moment when it had a Transform (sound is placed there). Events are output, not state: they are not in the state hash and a replay makes them again.", vec![("since_tick", int("Only events from this tick on.")), ("limit", int("Most events to return (default 200).")) ], &[]),
        tool("resources", "List every world resource and its value (including live input state under `input.`).", vec![], &[]),
        tool(
            "input",
            "Set controller state: keys and axes as resources `input.<name>` that graph systems read with {\"res\": \"input.w\"}. Booleans become 1/0. Humans (the editor) and AI agents use the same call, and it is recorded so a recording replays the same input. Not an edit: it is not undoable.",
            vec![("values", object("Map of input name to true/false or a number, for example {\"w\": true, \"space\": false, \"move_x\": -0.5}."))],
            &["values"],
        ),
        tool("reset", "Start over with an empty world at tick 0 (optionally with a new seed and a worker thread count; results never depend on the thread count).", vec![("seed", int("New seed.")), ("threads", int("Worker threads for parallel systems, 1 to 64."))], &[]),
    ]
}

pub fn tool_names() -> Vec<&'static str> {
    tool_defs().into_iter().map(|t| t.name).collect()
}
