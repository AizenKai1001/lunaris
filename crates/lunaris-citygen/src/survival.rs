//! The game on top of the city, as data of a scene: a backpack (a panel of 24 slots), furniture to search (the `Container`s that the buildings' insides carry: cabinets, shelves, bedside tables, crates, desks), what
//! is found in them, and the way things move between the two. It is all components, user-interface entities and graph systems of the engine, nothing native: the panels are `Ui` rectangles, an item is a `Ui` button with a
//! `UiDrag` (a number for what it is, the slot it sits in), a slot is a `Ui` with a `UiDrop`, and the systems below keep who owns what.
//!
//! How it works. `Tab` opens and shuts the backpack; looking at a container (the eyes' probe ray, 3.2 m) and pressing `E` opens it beside the backpack (`inv_open` is its uid) and, the first time, rolls what is in it: a few
//! items dropped in its slots by its kind. An `Item` has an `owner` (0 for the backpack, else the container's uid): an item put in a backpack slot belongs to the backpack, in a container slot to the container that is
//! open; those of a container that is not open are hidden and out of the slots (they remember `held`, their slot, for when it is opened again).
//!
//! Building (`build_parts`): `B` starts and ends building; `1` to `6` choose what to put down (a foundation, a wall, a doorway, a floor, a roof, a door); a ghost of it stands where the eyes' ray points, snapped to a grid of
//! 3 m cells (walls on the cell's nearest edge, at the height of what is aimed at); a click puts it down (a piece's `key` stops a second one in the same place) and `X` takes away the part that is looked at. A piece is boxes,
//! moved to its place by a system the tick after it is made (a `spawn_prefab` can set numbers but not a position); the door is the city's own `Door` and swings like theirs.
//!
//! Living (`vitals`): health, food and water (bars at the bottom right) run down with time and, at nothing, hurt; a click on a thing in the backpack uses it (beans, water, a bandage ...: it is used up) and planks and nails
//! become stock (`mat_wood`, `mat_nails`) that building spends (each piece has a price in wood and nails); at no health the player wakes up at the start with some food and water.
use lunaris_json::Json;

/// What can be found: (short name for the slot, full name, what it is for, category). A category colours the slot: 0 food, 1 medicine, 2 tools, 3 materials, 4 odds and ends.
pub const ITEMS: &[(&str, &str, &str, u32)] = &[
    ("Beans", "Canned beans", "Food. Eat it to stop being hungry.", 0),
    ("Water", "Bottle of water", "Drink it to stop being thirsty.", 0),
    ("Apple", "Apple", "Food. A little sweet and a little hunger.", 0),
    ("Bar", "Energy bar", "Food that keeps. Light to carry.", 0),
    ("Bandage", "Bandage", "Stops bleeding.", 1),
    ("Pills", "Painkillers", "Takes the edge off a hurt.", 1),
    ("Hatchet", "Hatchet", "Fells trees, breaks doors, and makes a weapon in a pinch.", 2),
    ("Crowbar", "Crowbar", "Opens what is locked.", 2),
    ("Torch", "Flashlight", "A light for the night. Needs cells.", 2),
    ("Matches", "Matches", "Light a fire.", 2),
    ("Rope", "Rope", "Ties things, lashes things.", 2),
    ("Nails", "Nails", "For building: walls and doors are nailed.", 3),
    ("Planks", "Planks", "For building: floors, walls, doors.", 3),
    ("Scrap", "Scrap metal", "For building and for mending.", 3),
    ("Cells", "Batteries", "Power for a flashlight.", 4),
    ("Tape", "Duct tape", "Mends nearly anything.", 4),
    ("Map", "Folded map", "Shows where you have been.", 4),
    ("Soda", "Soda can", "Sweet and fizzy: some water and a little food.", 0),
    ("Chips", "Bag of chips", "Salty. Takes the edge off hunger.", 0),
    ("Medkit", "First aid kit", "Closes wounds. Restores a lot of health.", 1),
    ("Jerky", "Dried jerky", "Food that keeps for a long time.", 0),
];

/// The colours of the categories of items.
const CATEGORY_COLOURS: [[f32; 3]; 5] = [[0.26, 0.50, 0.24], [0.62, 0.20, 0.22], [0.33, 0.40, 0.52], [0.52, 0.38, 0.24], [0.58, 0.52, 0.20]];

/// What the kinds of container hold, as (item number counted from 0, weight): 1 cabinet, 2 shelves, 3 bedside table, 4 crate or box, 5 desk or cart, 6 stove or microwave, 7 barrel. How many things are in one is rolled from the container's own number.
const LOOT: [&[(usize, f32)]; 7] = [
    &[(0, 3.0), (1, 3.0), (5, 2.0), (4, 2.0), (9, 2.0), (14, 2.0), (15, 2.0), (2, 1.0), (17, 2.0), (18, 1.5), (19, 1.0)],
    &[(0, 4.0), (1, 3.0), (3, 3.0), (2, 2.0), (11, 2.0), (15, 2.0), (10, 1.0), (14, 1.0), (17, 2.0), (18, 2.0), (20, 1.5)],
    &[(5, 3.0), (4, 2.0), (9, 3.0), (8, 2.0), (16, 1.0), (14, 2.0), (19, 1.5)],
    &[(12, 3.0), (11, 3.0), (13, 3.0), (10, 2.0), (7, 1.0), (6, 1.0), (15, 1.0)],
    &[(15, 3.0), (14, 2.0), (16, 2.0), (11, 1.0), (13, 1.0), (5, 1.0), (7, 1.0)],
    &[(0, 4.0), (1, 3.0), (2, 2.0), (3, 2.0), (9, 3.0), (15, 1.0), (17, 2.0), (18, 2.0), (20, 1.0)],
    &[(13, 4.0), (11, 3.0), (12, 2.0), (10, 2.0), (7, 1.0), (6, 1.0), (14, 1.0)],
];

const PLAYER_COLS: usize = 6;
const PLAYER_ROWS: usize = 4;
const BOX_COLS: usize = 4;
const BOX_ROWS: usize = 3;
const SLOT: [f64; 2] = [0.0475, 0.0845];
const GAP: [f64; 2] = [0.0045, 0.0075];

/// Everything the scene needs for it. `first_uid` is the uid that the first of `entities` will have (the scene numbers its entities in order).
pub struct Survival {
    pub components: Vec<Json>,
    pub prefabs: Vec<(String, Json)>,
    pub entities: Vec<Json>,
    pub systems: Vec<Json>,
    pub resources: Vec<(String, Json)>,
    /// The bindings to add to the player's `InputMap` (one line each).
    pub bindings: &'static str,
}

fn n(x: f64) -> Json {
    Json::Float(x)
}

fn arr(a: &[f64]) -> Json {
    Json::arr(a.iter().map(|x| Json::Float(*x)))
}

pub fn build(first_uid: u64, spawn: [f32; 3], slots: [f64; 2], eyes_uid: u64) -> Survival {
    let (quick_slot, auto_slot) = (slots[0], slots[1]);
    let get = |k: &str| Json::obj([("get", Json::str(k))]);
    let res = |k: &str| Json::obj([("res", Json::str(k))]);
    let var = |k: &str| Json::obj([("var", Json::str(k))]);
    let op2 = |op: &str, a: Json, b: Json| Json::obj([(op, Json::arr([a, b]))]);
    let sel = |c: Json, a: Json, b: Json| Json::obj([("select", Json::arr([c, a, b]))]);
    let opn = |op: &str, a: Vec<Json>| Json::obj([(op, Json::Arr(a))]);
    let uid_now = Json::obj([("uid", Json::Bool(true))]);
    let field = |name: &str, ty: &str, default: f64| Json::obj([("name", Json::str(name)), ("type", Json::str(ty)), ("default", n(default))]);

    let components = vec![
        Json::obj([("name", Json::str("Container")), ("doc", Json::str("A piece of furniture that can be searched: kind 1 cabinet, 2 shelves, 3 bedside table, 4 crate, 5 desk, 6 stove, 7 barrel; seed its own number; state 0 until it has been opened once (then its things have been rolled).")), ("fields", Json::arr([field("kind", "f32", 1.0), field("seed", "f32", 0.0), field("state", "f32", 0.0)]))]),
        Json::obj([("name", Json::str("Item")), ("doc", Json::str("A thing in the backpack or in a container: owner is 0 for the backpack and else the container's uid; held is the slot it had in its container when that was shut; x and z are where that container stands (a thing whose container is far behind is swept away).")), ("fields", Json::arr([field("owner", "f64", 0.0), field("held", "f64", 0.0), field("x", "f32", 0.0), field("z", "f32", 0.0)]))]),
        Json::obj([("name", Json::str("Panel")), ("doc", Json::str("A panel of the interface: kind 1 the backpack, 2 the container that is open, 3 the drop box under the backpack.")), ("fields", Json::arr([field("kind", "f32", 1.0)]))]),
        Json::obj([("name", Json::str("Flashlight")), ("doc", Json::str("The player's flashlight: a spot light on the eyes, lit while `torch_on` is 1 (needs a flashlight in the backpack and charge in `battery`).")), ("fields", Json::arr([]))]),
        Json::obj([("name", Json::str("Harvest")), ("doc", Json::str("Something that gives material when it is worked: kind 1 a tree (wood, with a hatchet), kind 2 a car (scrap, with a crowbar), kind 3 a hydrant (water to drink); left is how many more times.")), ("fields", Json::arr([field("kind", "f32", 1.0), field("left", "f32", 4.0)]))]),
        Json::obj([("name", Json::str("Loot")), ("doc", Json::str("A thing lying on the ground (`item` is its number, counted from 1): look at it and press E to put it in the backpack.")), ("fields", Json::arr([field("item", "f32", 1.0)]))]),
    ];

    // ---- the entities: both panels with their slots, the tooltip, the starting items, the prompt and the hints
    let mut entities: Vec<Json> = Vec::new();
    let uid_of = |index: usize| first_uid + index as u64;
    let panel = |entities: &mut Vec<Json>, name: &str, kind: f64, title: &str, x: f64, y: f64, cols: usize, rows: usize| -> (u64, u64) {
        let (w, h) = (cols as f64 * SLOT[0] + (cols - 1) as f64 * GAP[0], rows as f64 * SLOT[1] + (rows - 1) as f64 * GAP[1]);
        let root = uid_of(entities.len());
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(name))])),
            ("Ui", Json::obj([("pos", arr(&[x - 0.012, y - 0.052])), ("size", arr(&[w + 0.024, h + 0.052 + 0.014])), ("color", arr(&[0.03, 0.035, 0.05, 0.9])), ("border", n(0.002)), ("border_color", arr(&[0.42, 0.48, 0.58, 0.85])), ("layer", Json::from(1))])),
            ("UiStyle", Json::obj([("radius", n(0.008))])),
            ("UiGroup", Json::obj([("shown", Json::Bool(false))])),
            ("Panel", Json::obj([("kind", n(kind))])),
        ]));
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("{name} title")))])),
            ("Ui", Json::obj([("pos", arr(&[x, y - 0.046])), ("size", arr(&[w, 0.04])), ("color", arr(&[0.0, 0.0, 0.0, 0.0])), ("text", Json::str(title)), ("text_size", n(0.022)), ("layer", Json::from(2))])),
            ("Parent", Json::obj([("target", Json::Float(root as f64))])),
        ]));
        let first_slot = uid_of(entities.len());
        for r in 0..rows {
            for c in 0..cols {
                entities.push(Json::obj([
                    ("Name", Json::obj([("value", Json::str(format!("{name} slot {}", r * cols + c)))])),
                    ("Ui", Json::obj([("pos", arr(&[x + c as f64 * (SLOT[0] + GAP[0]), y + r as f64 * (SLOT[1] + GAP[1])])), ("size", arr(&SLOT)), ("color", arr(&[0.13, 0.15, 0.2, 0.95])), ("border", n(0.0012)), ("border_color", arr(&[0.3, 0.34, 0.42, 0.9])), ("layer", Json::from(2))])),
                    ("UiStyle", Json::obj([("radius", n(0.005))])),
                    ("UiDrop", Json::obj([("name", Json::str(format!("{}{}", if kind < 1.5 { "p" } else { "c" }, r * cols + c))), ("swap", Json::Bool(true))])),
                    ("Parent", Json::obj([("target", Json::Float(root as f64))])),
                ]));
            }
        }
        (first_slot, first_slot + (cols * rows) as u64 - 1)
    };
    let (p0, p1) = panel(&mut entities, "Backpack panel", 1.0, "BACKPACK   (Tab)", 0.06, 0.16, PLAYER_COLS, PLAYER_ROWS);
    let (c0, c1) = panel(&mut entities, "Container panel", 2.0, "SEARCH", 0.62, 0.16, BOX_COLS, BOX_ROWS);
    // the drop box under the backpack: a thing dragged onto it is put down on the ground in front of the player (`item_flow` sees `drop.bin`)
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Drop box"))])),
        ("Ui", Json::obj([("pos", arr(&[0.06, 0.575])), ("size", arr(&[PLAYER_COLS as f64 * SLOT[0] + (PLAYER_COLS - 1) as f64 * GAP[0], 0.06])), ("color", arr(&[0.2, 0.06, 0.06, 0.85])), ("border", n(0.0015)), ("border_color", arr(&[0.7, 0.3, 0.3, 0.9])), ("text", Json::str("DROP HERE")), ("text_size", n(0.02)), ("layer", Json::from(2))])),
        ("UiStyle", Json::obj([("radius", n(0.006))])),
        ("UiDrop", Json::obj([("name", Json::str("bin")), ("swap", Json::Bool(false))])),
        ("UiGroup", Json::obj([("shown", Json::Bool(false))])),
        ("Panel", Json::obj([("kind", n(3.0))])),
    ]));
    // the tooltip box (the engine fills and places it)
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Tooltip"))])),
        ("Ui", Json::obj([("size", arr(&[0.2, 0.08])), ("layer", Json::from(40)), ("visible", Json::Bool(false)), ("text_size", n(0.02)), ("color", arr(&[0.03, 0.04, 0.09, 0.96]))])),
        ("UiStyle", Json::obj([("radius", n(0.008))])),
        ("UiTooltipBox", Json::Obj(Vec::new())),
    ]));

    // ---- the items: a prefab for each, so that a system can make one with a call
    let mut prefabs: Vec<(String, Json)> = Vec::new();
    for (k, (short, long, about, cat)) in ITEMS.iter().enumerate() {
        let col = CATEGORY_COLOURS[*cat as usize];
        prefabs.push((
            format!("Item_{k}"),
            Json::obj([("components", Json::obj([
                ("Name", Json::obj([("value", Json::str(format!("Item {long}")))])),
                ("Ui", Json::obj([("pos", arr(&[0.5, 0.5])), ("size", arr(&SLOT)), ("color", arr(&[col[0] as f64, col[1] as f64, col[2] as f64, 1.0])), ("button", Json::Bool(true)), ("text", Json::str(*short)), ("text_size", n(0.0138)), ("layer", Json::from(4)), ("visible", Json::Bool(false)), ("border", n(0.0015)), ("border_color", arr(&[1.0, 1.0, 1.0, 0.45]))])),
                ("UiStyle", Json::obj([("radius", n(0.006)), ("color2", arr(&[col[0] as f64 * 0.55, col[1] as f64 * 0.55, col[2] as f64 * 0.55, 1.0]))])),
                ("UiDrag", Json::obj([("item", Json::from(k as i64 + 1)), ("kind", Json::from(*cat as i64)), ("slot", Json::from(0))])),
                ("Item", Json::obj([("owner", n(0.0)), ("held", n(0.0))])),
                ("UiTooltip", Json::obj([("text", Json::str(format!("{long}\n{about}"))), ("delay", n(0.3))])),
            ]))]),
        ));
    }
    // what lies on the ground: a small box in the colour of the thing's kind, that can be looked at and picked up (E)
    for (k, (_, long, _, cat)) in ITEMS.iter().enumerate() {
        let col = CATEGORY_COLOURS[*cat as usize];
        prefabs.push((
            format!("Loot_{k}"),
            Json::obj([("components", Json::obj([
                ("Name", Json::obj([("value", Json::str(format!("Loot {long}")))])),
                ("Transform", Json::obj([("pos", arr(&[0.0, -50.0, 0.0])), ("scale", arr(&[0.3, 0.2, 0.3]))])),
                ("MeshRenderer", Json::obj([("mesh", Json::str("cube")), ("color", arr(&[col[0] as f64, col[1] as f64, col[2] as f64, 1.0])), ("roughness", n(0.7)), ("emissive", n(0.15))])),
                ("Collider", Json::obj([("shape", Json::from(1)), ("size", arr(&[1.0, 1.0, 1.0])), ("trigger", Json::Bool(false))])),
                ("Loot", Json::obj([("item", n(k as f64 + 1.0))])),
            ]))]),
        ));
    }
    // a storage crate (C, 8 wood): a box in front of the player that holds what is put in it, for good (it is part of the saved world; its things are not swept away like the loot of the furniture): a `Container` of kind 8
    // whose things are already rolled (state 1: nothing to find)
    prefabs.push((
        "StorageCrate".to_string(),
        Json::obj([("components", Json::obj([
            ("Name", Json::obj([("value", Json::str("Storage crate"))])),
            ("Transform", Json::obj([("pos", arr(&[0.0, -50.0, 0.0])), ("scale", arr(&[0.9, 0.7, 0.6]))])),
            ("MeshRenderer", Json::obj([("mesh", Json::str("cube")), ("color", arr(&[0.78, 0.6, 0.4, 1.0])), ("texture", Json::str("city/textures/wood_planks_grey/diff.jpg")), ("roughness", n(0.8))])),
            ("Collider", Json::obj([("shape", Json::from(1)), ("size", arr(&[1.0, 1.0, 1.0])), ("trigger", Json::Bool(false))])),
            ("Container", Json::obj([("kind", n(8.0)), ("seed", n(0.0)), ("state", n(1.0))])),
        ]))]),
    ));
    // the backpack starts with a hatchet, a bottle of water and a bandage
    for (slot, k) in [(0u64, 6usize), (1, 1), (2, 4)] {
        let mut item = prefabs[k].1.get("components").cloned().unwrap_or(Json::Obj(Vec::new()));
        if let Json::Obj(comps) = &mut item {
            for (name, value) in comps.iter_mut() {
                if name == "UiDrag" {
                    *value = Json::obj([("item", Json::from(k as i64 + 1)), ("kind", Json::from(ITEMS[k].3 as i64)), ("slot", Json::Float((p0 + slot) as f64))]);
                }
            }
        }
        entities.push(item);
    }
    // the flashlight: a spot light child of the eyes (it shines along their -z, as the camera looks), dark until the scene's `flashlight` system lights it
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Flashlight"))])),
        ("Parent", Json::obj([("target", Json::Float(eyes_uid as f64))])),
        ("LocalTransform", Json::obj([("pos", arr(&[0.12, -0.12, -0.15]))])),
        ("Light", Json::obj([("kind", Json::from(2)), ("color", arr(&[1.0, 0.95, 0.85])), ("intensity", n(0.0)), ("range", n(32.0)), ("angle", n(26.0)), ("softness", n(0.45))])),
        ("Flashlight", Json::Obj(Vec::new())),
    ]));
    // what the player sees: the line under the crosshair at a container, and the hint for the backpack
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Prompt container"))])),
        ("Text", Json::obj([("value", Json::str("E   search")), ("pos", arr(&[0.44, 0.58])), ("size", n(0.032)), ("color", arr(&[1.0, 1.0, 1.0, 0.0]))])),
        ("Prompt", Json::obj([("kind", n(2.0))])),
    ]));
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Prompt loot"))])),
        ("Text", Json::obj([("value", Json::str("E   pick up")), ("pos", arr(&[0.44, 0.58])), ("size", n(0.032)), ("color", arr(&[1.0, 1.0, 1.0, 0.0]))])),
        ("Prompt", Json::obj([("kind", n(3.0))])),
    ]));
    for (kind, line) in [(4.0, "E   chop for wood"), (5.0, "E   strip for scrap"), (6.0, "needs a hatchet"), (7.0, "needs a crowbar"), (8.0, "locked   (a hatchet or a crowbar breaks it)"), (9.0, "E   break the lock"), (10.0, "E   drink")] {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Prompt harvest {kind}")))])),
            ("Text", Json::obj([("value", Json::str(line)), ("pos", arr(&[0.44, 0.58])), ("size", n(0.032)), ("color", arr(&[1.0, 1.0, 1.0, 0.0]))])),
            ("Prompt", Json::obj([("kind", n(kind))])),
        ]));
    }
    entities.push(Json::obj([("Name", Json::obj([("value", Json::str("HUD backpack hint"))])), ("Text", Json::obj([("value", Json::str("TAB backpack   E use   F light   C crate   B build   F5 save   F9 load")), ("pos", arr(&[0.015, 0.05])), ("size", n(0.024)), ("color", arr(&[1.0, 1.0, 1.0, 0.7]))]))]));

    // ---- the systems
    let mut systems: Vec<Json> = Vec::new();
    // the keys: Tab shuts and opens the backpack, Esc shuts what is open (or leaves the game when nothing is); a panel open frees the pointer and holds the player still
    systems.push(Json::obj([
        ("name", Json::str("inventory_keys")),
        ("doc", Json::str("Tab opens and shuts the backpack, Esc shuts the panels (or quits when none is open); with a panel open the pointer is free and the player stands still.")),
        ("each", Json::arr([Json::str("Hero"), Json::str("CharacterBody")])),
        ("order", Json::from(12)),
        ("do", Json::arr([
            Json::obj([("if", op2("gt", res("action.inventory.pressed"), n(0.5))), ("then", Json::arr([
                Json::obj([("let", Json::str("np")), ("be", op2("sub", n(1.0), res("inv_player")))]),
                Json::obj([("res_set", Json::str("inv_player")), ("to", var("np"))]),
                Json::obj([("if", op2("lt", var("np"), n(0.5))), ("then", Json::arr([Json::obj([("res_set", Json::str("inv_open")), ("to", n(0.0))])]))]),
            ]))]),
            Json::obj([("if", op2("gt", res("action.close.pressed"), n(0.5))), ("then", Json::arr([
                Json::obj([("if", op2("or", op2("gt", res("inv_player"), n(0.5)), op2("gt", res("inv_open"), n(0.5)))), ("then", Json::arr([
                    Json::obj([("res_set", Json::str("inv_player")), ("to", n(0.0))]),
                    Json::obj([("res_set", Json::str("inv_open")), ("to", n(0.0))]),
                ])), ("else", Json::arr([Json::obj([("res_set", Json::str("game.save")), ("to", n(auto_slot))]), Json::obj([("res_set", Json::str("quit_pending")), ("to", n(1.0))])]))]),
            ]))]),
            Json::obj([("res_set", Json::str("ui_open")), ("to", sel(op2("or", op2("gt", res("inv_player"), n(0.5)), op2("gt", res("inv_open"), n(0.5))), n(1.0), n(0.0)))]),
            Json::obj([("res_set", Json::str("cursor.lock")), ("to", op2("sub", n(1.0), res("ui_open")))]),
            Json::obj([("if", op2("gt", res("ui_open"), n(0.5))), ("then", Json::arr([
                Json::obj([("set", Json::str("CharacterBody.walk[0]")), ("to", n(0.0))]),
                Json::obj([("set", Json::str("CharacterBody.walk[1]")), ("to", n(0.0))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // with a panel open the pointer is the player's, and the view does not turn with it (the player writes the mouse's motion whether the pointer is held or not)
    systems.push(Json::obj([
        ("name", Json::str("look_gate")),
        ("doc", Json::str("The view does not turn while a panel is open.")),
        ("each", Json::arr([Json::str("MouseLook")])),
        ("order", Json::from(12)),
        ("do", Json::arr([Json::obj([("set", Json::str("MouseLook.enabled")), ("to", op2("lt", res("ui_open"), n(0.5)))])])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("panels_show")),
        ("doc", Json::str("The backpack panel shows while the backpack is open, the container panel while a container is.")),
        ("each", Json::arr([Json::str("Panel"), Json::str("UiGroup")])),
        ("order", Json::from(13)),
        ("do", Json::arr([Json::obj([("set", Json::str("UiGroup.shown")), ("to", sel(op2("eq", get("Panel.kind"), n(2.0)), op2("gt", res("inv_open"), n(0.5)), op2("gt", res("inv_player"), n(0.5))))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // a container looked at: it is the target (prompt kind 2), and E opens it (or shuts it again); the first time what is in it is rolled
    let loot_if = |rolls: usize, kind: usize, c0: u64| -> Json {
        // for each of `rolls` places: a chance that something is there, then which item by weight
        let table = LOOT[kind];
        let total: f32 = table.iter().map(|t| t.1).sum();
        let mut stmts: Vec<Json> = Vec::new();
        for k in 0..rolls {
            let present = Json::obj([("rand", n(10.0 + k as f64))]);
            let mut chain: Json = Json::arr([]);
            let mut acc = 0.0f32;
            // built from the last item back to the first, so that each `else` holds the rest
            let cut: Vec<(usize, f32)> = table.iter().map(|t| { acc += t.1 / total; (t.0, acc) }).collect();
            for (idx, (item, upto)) in cut.iter().enumerate().rev() {
                let spawn = Json::obj([("spawn_prefab", Json::str(format!("Item_{item}"))), ("with", Json::obj([("UiDrag", Json::obj([("slot", Json::Float((c0 + k as u64) as f64))])), ("Item", Json::obj([("held", Json::Float((c0 + k as u64) as f64)), ("x", get("Transform.pos[0]")), ("z", get("Transform.pos[2]"))]))]))]);
                chain = if idx + 1 == cut.len() {
                    Json::arr([spawn])
                } else {
                    Json::arr([Json::obj([("if", op2("lt", var("pick"), n(*upto as f64))), ("then", Json::arr([spawn])), ("else", chain)])])
                };
            }
            let mut body = vec![Json::obj([("let", Json::str("pick")), ("be", Json::obj([("rand", n(40.0 + k as f64))]))])];
            if let Json::Arr(a) = chain {
                body.extend(a);
            }
            stmts.push(Json::obj([("if", op2("lt", present, n(0.62))), ("then", Json::Arr(body))]));
        }
        Json::Arr(stmts)
    };
    let mut by_kind: Json = Json::arr([]);
    for kind in (0..LOOT.len()).rev() {
        let rolls = if kind == 1 || kind == 3 { 5 } else { 4 };
        let stmts = loot_if(rolls, kind, c0);
        by_kind = if kind + 1 == LOOT.len() {
            stmts
        } else {
            Json::arr([Json::obj([("if", op2("eq", get("Container.kind"), n(kind as f64 + 1.0))), ("then", stmts), ("else", by_kind)])])
        };
    }
    systems.push(Json::obj([
        ("name", Json::str("container_use")),
        ("doc", Json::str("The container the player looks at is the target (prompt kind 2); E opens it beside the backpack, or shuts it; the first time its things are rolled into the container's slots.")),
        ("each", Json::arr([Json::str("Container"), Json::str("Transform")])),
        ("order", Json::from(9)),
        ("do", Json::arr([Json::obj([
            ("if", op2("eq", uid_now.clone(), res("interact_uid"))),
            ("then", Json::arr([
                Json::obj([("res_set", Json::str("interact_kind")), ("to", n(2.0))]),
                Json::obj([("if", op2("gt", res("interact_press"), n(0.5))), ("then", Json::arr([
                    Json::obj([("if", op2("eq", res("inv_open"), uid_now.clone())), ("then", Json::arr([Json::obj([("res_set", Json::str("inv_open")), ("to", n(0.0))])])), ("else", Json::arr([
                        Json::obj([("res_set", Json::str("inv_open")), ("to", uid_now.clone())]),
                        Json::obj([("res_set", Json::str("inv_x")), ("to", sel(op2("gt", get("Container.kind"), n(7.5)), n(0.0), get("Transform.pos[0]")))]),
                        Json::obj([("res_set", Json::str("inv_z")), ("to", sel(op2("gt", get("Container.kind"), n(7.5)), n(0.0), get("Transform.pos[2]")))]),
                        Json::obj([("res_set", Json::str("inv_player")), ("to", n(1.0))]),
                        Json::obj([("emit", Json::str("search"))]),
                        Json::obj([("if", op2("lt", get("Container.state"), n(0.5))), ("then", {
                            let mut v = vec![Json::obj([("set", Json::str("Container.state")), ("to", n(1.0))])];
                            if let Json::Arr(a) = &by_kind {
                                v.extend(a.clone());
                            }
                            Json::Arr(v)
                        })]),
                    ]))]),
                ]))]),
            ])),
        ])])),
        ("enabled", Json::Bool(true)),
    ]));
    // the things: who owns each by the slot it lies in, which are shown, and which sit in a slot
    let slot = get("UiDrag.slot");
    let in_p = op2("and", op2("ge", slot.clone(), n(p0 as f64)), op2("le", slot.clone(), n(p1 as f64)));
    let in_c = op2("and", op2("ge", slot.clone(), n(c0 as f64)), op2("le", slot.clone(), n(c1 as f64)));
    systems.push(Json::obj([
        ("name", Json::str("item_flow")),
        ("doc", Json::str("A thing put in a backpack slot belongs to the backpack, in a slot of the open container to that container; what belongs to a container that is not open is hidden and leaves its slot (it remembers it in `held`).")),
        ("each", Json::arr([Json::str("Item"), Json::str("UiDrag"), Json::str("Ui")])),
        ("order", Json::from(14)),
        ("do", Json::arr([
            Json::obj([("if", in_p), ("then", Json::arr([Json::obj([("set", Json::str("Item.owner")), ("to", n(0.0))])]))]),
            Json::obj([("if", op2("and", in_c, op2("eq", get("Item.owner"), n(0.0)))), ("then", Json::arr([
                Json::obj([("set", Json::str("Item.owner")), ("to", res("inv_open"))]),
                Json::obj([("set", Json::str("Item.x")), ("to", res("inv_x"))]),
                Json::obj([("set", Json::str("Item.z")), ("to", res("inv_z"))]),
            ]))]),
            // what lies in a container that has streamed out of reach is gone with it
            Json::obj([("if", opn("and", vec![op2("gt", get("Item.owner"), n(0.5)), op2("ne", get("Item.owner"), res("inv_open")), op2("or", op2("ne", get("Item.x"), n(0.0)), op2("ne", get("Item.z"), n(0.0))), op2("gt", Json::obj([("hypot", Json::arr([op2("sub", get("Item.x"), res("eye_x")), op2("sub", get("Item.z"), res("eye_z"))]))]), n(160.0))])), ("then", Json::arr([Json::obj([("despawn", Json::Bool(true))])]))]),
            // a container's thing that is not in the open container: out of its slot, hidden
            Json::obj([("if", op2("and", op2("gt", get("Item.owner"), n(0.5)), op2("ne", get("Item.owner"), res("inv_open")))), ("then", Json::arr([
                Json::obj([("if", op2("gt", slot.clone(), n(0.5))), ("then", Json::arr([
                    Json::obj([("set", Json::str("Item.held")), ("to", slot.clone())]),
                    Json::obj([("set", Json::str("UiDrag.slot")), ("to", n(0.0))]),
                ]))]),
            ]))]),
            // a container's thing in the open container: back in its slot
            Json::obj([("if", op2("and", op2("gt", get("Item.owner"), n(0.5)), op2("eq", get("Item.owner"), res("inv_open")))), ("then", Json::arr([
                Json::obj([("if", op2("lt", slot.clone(), n(0.5))), ("then", Json::arr([Json::obj([("set", Json::str("UiDrag.slot")), ("to", get("Item.held"))])]))]),
            ]))]),
            // dragged onto the drop box: it lies on the ground a step in front of the player
            Json::obj([("if", op2("and", op2("gt", res("drop.bin"), n(0.5)), op2("eq", res("drop.bin"), uid_now.clone()))), ("then", {
                let at = Json::arr([
                    op2("add", res("eye_x"), op2("mul", res("look.fx"), n(1.3))),
                    op2("sub", res("eye_y"), n(1.5)),
                    op2("add", res("eye_z"), op2("mul", res("look.fz"), n(1.3))),
                ]);
                let mut v: Vec<Json> = (0..ITEMS.len()).map(|k| Json::obj([("if", op2("eq", get("UiDrag.item"), n(k as f64 + 1.0))), ("then", Json::arr([Json::obj([("spawn_prefab", Json::str(format!("Loot_{k}"))), ("at", at.clone())])]))])).collect();
                v.push(Json::obj([("emit", Json::str("use_item"))]));
                v.push(Json::obj([("despawn", Json::Bool(true))]));
                Json::Arr(v)
            })]),
            Json::obj([("set", Json::str("Ui.visible")), ("to", sel(op2("lt", get("Item.owner"), n(0.5)), op2("gt", res("inv_player"), n(0.5)), op2("eq", get("Item.owner"), res("inv_open"))))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));

    // ground loot: looked at, a thing is the target (prompt kind 3); E asks for it (`pickup_item`), the free backpack slot that `slot_scan` found takes it (`loot_fill` makes the item there), and then the thing on the ground
    // goes; with no free slot the request is dropped after a moment and the HUD says the backpack is full
    systems.push(Json::obj([
        ("name", Json::str("loot_use")),
        ("doc", Json::str("A thing on the ground that is looked at is the target (prompt kind 3); E asks to pick it up; it goes once the backpack has taken it, and stays (with a message) when the backpack is full.")),
        ("each", Json::arr([Json::str("Loot"), Json::str("Transform")])),
        ("order", Json::from(9)),
        ("do", Json::arr([
            Json::obj([("if", op2("eq", uid_now.clone(), res("interact_uid"))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("interact_kind")), ("to", n(3.0))]),
                Json::obj([("if", op2("and", op2("gt", res("interact_press"), n(0.5)), op2("lt", res("pickup_item"), n(0.5)))), ("then", Json::arr([
                    Json::obj([("res_set", Json::str("pickup_item")), ("to", get("Loot.item"))]),
                    Json::obj([("res_set", Json::str("pickup_uid")), ("to", uid_now.clone())]),
                    Json::obj([("res_set", Json::str("pickup_t")), ("to", Json::obj([("tick", Json::Bool(true))]))]),
                ]))]),
            ]))]),
            Json::obj([("if", op2("eq", uid_now.clone(), res("pickup_uid"))), ("then", Json::arr([
                Json::obj([("if", op2("gt", res("pickup_done"), n(0.5))), ("then", Json::arr([
                    Json::obj([("res_set", Json::str("pickup_done")), ("to", n(0.0))]),
                    Json::obj([("res_set", Json::str("pickup_uid")), ("to", n(0.0))]),
                    Json::obj([("despawn", Json::Bool(true))]),
                ])), ("else", Json::arr([
                    Json::obj([("if", op2("and", op2("gt", res("pickup_item"), n(0.5)), op2("gt", op2("sub", Json::obj([("tick", Json::Bool(true))]), res("pickup_t")), n(6.0)))), ("then", Json::arr([
                        Json::obj([("res_set", Json::str("pickup_item")), ("to", n(0.0))]),
                        Json::obj([("res_set", Json::str("pickup_uid")), ("to", n(0.0))]),
                        Json::obj([("res_set", Json::str("full_tick")), ("to", Json::obj([("tick", Json::Bool(true))]))]),
                    ]))]),
                ]))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    let free_here = op2("and", op2("and", op2("ge", uid_now.clone(), n(p0 as f64)), op2("le", uid_now.clone(), n(p1 as f64))), op2("lt", get("UiDrop.holds"), n(0.5)));
    systems.push(Json::obj([
        ("name", Json::str("slot_scan")),
        ("doc", Json::str("A backpack slot that holds nothing names itself the free slot (the last that does so in a tick is it).")),
        ("each", Json::arr([Json::str("UiDrop"), Json::str("Ui")])),
        ("order", Json::from(16)),
        ("do", Json::arr([Json::obj([("if", free_here.clone()), ("then", Json::arr([Json::obj([("res_set", Json::str("free_slot")), ("to", uid_now.clone())])]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("loot_fill")),
        ("doc", Json::str("The free slot makes the thing that was asked for (a thing picked up from the ground).")),
        ("each", Json::arr([Json::str("UiDrop"), Json::str("Ui")])),
        ("order", Json::from(17)),
        ("do", Json::arr([Json::obj([("if", op2("and", op2("and", op2("gt", res("pickup_item"), n(0.5)), op2("eq", res("free_slot"), uid_now.clone())), free_here)), ("then", {
            let mut v: Vec<Json> = (0..ITEMS.len()).map(|k| Json::obj([("if", op2("eq", res("pickup_item"), n(k as f64 + 1.0))), ("then", Json::arr([Json::obj([("spawn_prefab", Json::str(format!("Item_{k}"))), ("with", Json::obj([("UiDrag", Json::obj([("slot", uid_now.clone())]))]))])]))])).collect();
            v.push(Json::obj([("res_set", Json::str("pickup_item")), ("to", n(0.0))]));
            v.push(Json::obj([("res_set", Json::str("pickup_done")), ("to", n(1.0))]));
            v.push(Json::obj([("emit", Json::str("search"))]));
            Json::Arr(v)
        })])])),
        ("enabled", Json::Bool(true)),
    ]));
    // C makes a storage crate in front of the player for 8 wood (not with a panel open)
    systems.push(Json::obj([
        ("name", Json::str("crate_place")),
        ("doc", Json::str("C puts a storage crate down a step in front of the player for 8 wood.")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(12)),
        ("do", Json::arr([Json::obj([("if", opn("and", vec![op2("gt", res("action.craft.pressed"), n(0.5)), op2("lt", res("ui_open"), n(0.5)), op2("ge", res("mat_wood"), n(8.0))])), ("then", Json::arr([
            Json::obj([("res_add", Json::str("mat_wood")), ("by", n(-8.0))]),
            Json::obj([("emit", Json::str("build_place"))]),
            Json::obj([("spawn_prefab", Json::str("StorageCrate")), ("at", Json::arr([
                op2("add", res("eye_x"), op2("mul", res("look.fx"), n(1.4))),
                op2("sub", res("eye_y"), n(1.35)),
                op2("add", res("eye_z"), op2("mul", res("look.fz"), n(1.4))),
            ]))]),
        ]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // footsteps: every 1.7 m walked on the ground the engine's synthesiser plays a step, the two voices in turn (a teleport or a death does not count: a tick's walk is capped)
    systems.push(Json::obj([
        ("name", Json::str("footsteps")),
        ("doc", Json::str("A footstep sound every 1.7 m walked on the ground.")),
        ("each", Json::arr([Json::str("Hero"), Json::str("CharacterBody"), Json::str("Transform")])),
        ("order", Json::from(12)),
        ("do", Json::arr([
            Json::obj([("let", Json::str("moved")), ("be", Json::obj([("min", Json::arr([Json::obj([("hypot", Json::arr([op2("sub", get("Transform.pos[0]"), res("last_x")), op2("sub", get("Transform.pos[2]"), res("last_z"))]))]), n(1.0)]))]))]),
            Json::obj([("res_set", Json::str("last_x")), ("to", get("Transform.pos[0]"))]),
            Json::obj([("res_set", Json::str("last_z")), ("to", get("Transform.pos[2]"))]),
            Json::obj([("let", Json::str("acc")), ("be", op2("add", res("step_acc"), sel(get("CharacterBody.grounded"), var("moved"), n(0.0))))]),
            Json::obj([("if", op2("gt", var("acc"), n(1.7))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("step_acc")), ("to", op2("sub", var("acc"), n(1.7)))]),
                Json::obj([("res_add", Json::str("steps_taken")), ("by", n(1.0))]),
                Json::obj([("if", op2("gt", res("step_alt"), n(0.5))), ("then", Json::arr([Json::obj([("emit", Json::str("footstep_a"))]), Json::obj([("res_set", Json::str("step_alt")), ("to", n(0.0))])])), ("else", Json::arr([Json::obj([("emit", Json::str("footstep_b"))]), Json::obj([("res_set", Json::str("step_alt")), ("to", n(1.0))])]))]),
            ])), ("else", Json::arr([Json::obj([("res_set", Json::str("step_acc")), ("to", var("acc"))])]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the compass (top middle): the way the player faces, north being -z
    let compass_component = Json::obj([("name", Json::str("Compass")), ("doc", Json::str("A label of the compass: sector 0 north, 1 north-east, 2 east, 3 south-east, 4 south, 5 south-west, 6 west, 7 north-west.")), ("fields", Json::arr([field("sector", "f32", 0.0)]))]);
    for (k, name) in ["N", "NE", "E", "SE", "S", "SW", "W", "NW"].iter().enumerate() {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Compass {name}")))])),
            ("Text", Json::obj([("value", Json::str(*name)), ("pos", arr(&[0.485, 0.075])), ("size", n(0.034)), ("color", arr(&[0.95, 0.95, 0.85, 0.0]))])),
            ("Compass", Json::obj([("sector", n(k as f64))])),
        ]));
    }
    systems.push(Json::obj([
        ("name", Json::str("compass")),
        ("doc", Json::str("Shows the label of the way the player faces (`look.fx`, `look.fz`: north is -z, east +x).")),
        ("each", Json::arr([Json::str("Compass"), Json::str("Text")])),
        ("order", Json::from(16)),
        ("do", Json::arr([
            Json::obj([("let", Json::str("a")), ("be", Json::obj([("atan2", Json::arr([res("look.fx"), op2("sub", n(0.0), res("look.fz"))]))]))]),
            Json::obj([("let", Json::str("sector")), ("be", Json::obj([("mod", Json::arr([op2("add", Json::obj([("floor", op2("add", op2("div", var("a"), n(std::f64::consts::FRAC_PI_4)), n(0.5)))]), n(8.0)), n(8.0)]))]))]),
            Json::obj([("set", Json::str("Text.color[3]")), ("to", sel(op2("eq", var("sector"), get("Compass.sector")), n(0.9), n(0.0)))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // tools: the hatchet (item 7) and the crowbar (item 8) in the backpack, looked for every tick (a system clears the marks, one marks them again)
    systems.push(Json::obj([
        ("name", Json::str("tools_reset")),
        ("doc", Json::str("Clears the marks of the tools in the backpack (`tools_scan` sets them again).")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(5)),
        ("do", Json::arr([Json::obj([("res_set", Json::str("has_hatchet")), ("to", n(0.0))]), Json::obj([("res_set", Json::str("has_crowbar")), ("to", n(0.0))]), Json::obj([("res_set", Json::str("has_torch")), ("to", n(0.0))])])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("tools_scan")),
        ("doc", Json::str("A hatchet or a crowbar in the backpack (not in a container) is marked in `has_hatchet` / `has_crowbar`.")),
        ("each", Json::arr([Json::str("Item"), Json::str("UiDrag")])),
        ("order", Json::from(6)),
        ("do", Json::arr([
            Json::obj([("if", op2("lt", get("Item.owner"), n(0.5))), ("then", Json::arr([
                Json::obj([("if", op2("eq", get("UiDrag.item"), n(7.0))), ("then", Json::arr([Json::obj([("res_set", Json::str("has_hatchet")), ("to", n(1.0))])]))]),
                Json::obj([("if", op2("eq", get("UiDrag.item"), n(8.0))), ("then", Json::arr([Json::obj([("res_set", Json::str("has_crowbar")), ("to", n(1.0))])]))]),
                Json::obj([("if", op2("eq", get("UiDrag.item"), n(9.0))), ("then", Json::arr([Json::obj([("res_set", Json::str("has_torch")), ("to", n(1.0))])]))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the flashlight (F): lit while it is on, there is one in the backpack and the battery has charge; the charge runs down (about four minutes of light) and a thing marked cells recharges it when it is clicked
    systems.push(Json::obj([
        ("name", Json::str("flashlight")),
        ("doc", Json::str("F turns the flashlight on and off (with one in the backpack and charge in the battery); the battery runs down while it shines and dims at the end.")),
        ("each", Json::arr([Json::str("Flashlight"), Json::str("Light")])),
        ("order", Json::from(12)),
        ("do", Json::arr([
            Json::obj([("if", op2("and", op2("gt", res("action.flashlight.pressed"), n(0.5)), op2("gt", res("has_torch"), n(0.5)))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("torch_on")), ("to", op2("sub", n(1.0), res("torch_on")))]),
            ]))]),
            Json::obj([("if", op2("or", op2("lt", res("has_torch"), n(0.5)), op2("le", res("battery"), n(0.0)))), ("then", Json::arr([Json::obj([("res_set", Json::str("torch_on")), ("to", n(0.0))])]))]),
            Json::obj([("res_set", Json::str("battery")), ("to", Json::obj([("clamp", Json::arr([op2("sub", res("battery"), op2("mul", op2("mul", Json::obj([("dt", Json::Bool(true))]), res("torch_on")), n(0.42))), n(0.0), n(100.0)]))]))]),
            Json::obj([("set", Json::str("Light.intensity")), ("to", op2("mul", op2("mul", res("torch_on"), sel(op2("gt", res("has_torch"), n(0.5)), n(1.0), n(0.0))), op2("mul", n(11.0), op2("add", n(0.55), op2("mul", n(0.45), Json::obj([("clamp", Json::arr([op2("div", res("battery"), n(15.0)), n(0.0), n(1.0)]))]))))))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // what is looked at that can be worked: the prompt says what it takes (and what is missing); E gives three planks' worth of wood from a tree, or four nails' worth from a car, a few times
    systems.push(Json::obj([
        ("name", Json::str("harvest_use")),
        ("doc", Json::str("A tree looked at (a hatchet in the backpack) gives wood when E is pressed, a car (a crowbar) gives scrap, until `left` runs out; without the tool the prompt says what is needed.")),
        ("each", Json::arr([Json::str("Harvest")])),
        ("order", Json::from(9)),
        ("do", Json::arr([
            Json::obj([("if", op2("and", op2("eq", uid_now.clone(), res("interact_uid")), op2("gt", get("Harvest.left"), n(0.5)))), ("then", {
                let tree = op2("lt", get("Harvest.kind"), n(1.5));
                let car = op2("and", op2("ge", get("Harvest.kind"), n(1.5)), op2("lt", get("Harvest.kind"), n(2.5)));
                let has_tool = sel(tree.clone(), res("has_hatchet"), sel(car.clone(), res("has_crowbar"), n(1.0)));
                Json::arr([
                    Json::obj([("res_set", Json::str("interact_kind")), ("to", sel(op2("gt", has_tool.clone(), n(0.5)), sel(tree.clone(), n(4.0), sel(car.clone(), n(5.0), n(10.0))), sel(tree.clone(), n(6.0), n(7.0))))]),
                    Json::obj([("if", op2("and", op2("gt", res("interact_press"), n(0.5)), op2("gt", has_tool, n(0.5)))), ("then", Json::arr([
                        Json::obj([("set", Json::str("Harvest.left")), ("to", op2("sub", get("Harvest.left"), n(1.0)))]),
                        Json::obj([("if", tree.clone()), ("then", Json::arr([Json::obj([("res_add", Json::str("mat_wood")), ("by", n(3.0))]), Json::obj([("emit", Json::str("chop"))])])), ("else", Json::arr([
                            Json::obj([("if", car.clone()), ("then", Json::arr([Json::obj([("res_add", Json::str("mat_nails")), ("by", n(4.0))]), Json::obj([("emit", Json::str("strip"))])])), ("else", Json::arr([Json::obj([("res_add", Json::str("water")), ("by", n(35.0))]), Json::obj([("emit", Json::str("use_item"))])]))]),
                        ]))]),
                    ]))]),
                ])
            })]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the sounds (the engine's small synthesiser plays the event of that name): a creak, a thud, a click, a crunch, a knock
    for (name, wave, freq, freq_end, duration, volume) in [("door_open", 2.0, 210.0, 120.0, 0.32, 0.16), ("door_close", 4.0, 300.0, 0.0, 0.11, 0.30), ("search", 3.0, 520.0, 380.0, 0.12, 0.2), ("use_item", 4.0, 800.0, 0.0, 0.16, 0.16), ("build_place", 1.0, 95.0, 55.0, 0.14, 0.3), ("build_remove", 4.0, 400.0, 0.0, 0.2, 0.3), ("chop", 4.0, 900.0, 150.0, 0.14, 0.34), ("strip", 4.0, 3200.0, 700.0, 0.16, 0.26), ("door_locked", 4.0, 220.0, 0.0, 0.1, 0.3), ("footstep_a", 4.0, 700.0, 180.0, 0.07, 0.2), ("footstep_b", 4.0, 560.0, 150.0, 0.08, 0.18)] {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Sound {name}")))])),
            ("Sound", Json::obj([("name", Json::str(name)), ("wave", n(wave)), ("freq", n(freq)), ("freq_end", n(freq_end)), ("duration", n(duration)), ("volume", n(volume))])),
        ]));
    }
    // ---- living: the bars of health, food and water, what a click on a thing does, how the numbers run down
    let mut components = components;
    components.push(compass_component);
    components.push(Json::obj([("name", Json::str("Vital")), ("doc", Json::str("A bar of the HUD: kind 1 health, 2 food, 3 water, 4 the flashlight's battery (shown with a flashlight in the backpack).")), ("fields", Json::arr([field("kind", "f32", 1.0)]))]));
    for (k, (label, colour)) in [("HEALTH", [0.78, 0.2, 0.2]), ("FOOD", [0.8, 0.6, 0.2]), ("WATER", [0.25, 0.55, 0.85]), ("BATTERY", [0.85, 0.82, 0.35])].into_iter().enumerate() {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Bar {label}")))])),
            ("Ui", Json::obj([("pos", arr(&[0.80, if k == 3 { 0.76 } else { 0.80 + k as f64 * 0.04 }])), ("size", arr(&[0.17, 0.028])), ("color", arr(&[colour[0], colour[1], colour[2], 0.92])), ("back_color", arr(&[0.05, 0.05, 0.07, 0.75])), ("fill", n(1.0)), ("border", n(0.0012)), ("border_color", arr(&[1.0, 1.0, 1.0, 0.5])), ("text", Json::str(label)), ("text_size", n(0.018)), ("layer", Json::from(1))])),
            ("UiStyle", Json::obj([("radius", n(0.004))])),
            ("Vital", Json::obj([("kind", n(k as f64 + 1.0))])),
        ]));
    }
    // what a click on a thing in the backpack does: (item number, food, water, health, wood, nails); what has an effect is used up
    let uses: [(i64, f64, f64, f64, f64, f64); 13] = [(1, 35.0, 0.0, 0.0, 0.0, 0.0), (2, 0.0, 40.0, 0.0, 0.0, 0.0), (3, 12.0, 8.0, 0.0, 0.0, 0.0), (4, 25.0, 0.0, 0.0, 0.0, 0.0), (5, 0.0, 0.0, 25.0, 0.0, 0.0), (6, 0.0, 0.0, 10.0, 0.0, 0.0), (12, 0.0, 0.0, 0.0, 0.0, 8.0), (13, 0.0, 0.0, 0.0, 4.0, 0.0), (14, 0.0, 0.0, 0.0, 0.0, 3.0), (18, 5.0, 20.0, 0.0, 0.0, 0.0), (19, 15.0, 0.0, 0.0, 0.0, 0.0), (20, 0.0, 0.0, 60.0, 0.0, 0.0), (21, 30.0, 0.0, 0.0, 0.0, 0.0)];
    let mut use_chain: Json = Json::arr([]);
    for (idx, (item, food, water, hp, wood, nails)) in uses.iter().enumerate().rev() {
        let mut then: Vec<Json> = Vec::new();
        for (name, v) in [("food", food), ("water", water), ("hp", hp), ("mat_wood", wood), ("mat_nails", nails)] {
            if *v != 0.0 {
                then.push(Json::obj([("res_add", Json::str(name)), ("by", n(*v))]));
            }
        }
        then.push(Json::obj([("emit", Json::str("use_item"))]));
        then.push(Json::obj([("despawn", Json::Bool(true))]));
        use_chain = if idx + 1 == uses.len() {
            Json::arr([Json::obj([("if", op2("eq", get("UiDrag.item"), n(*item as f64))), ("then", Json::Arr(then))])])
        } else {
            Json::arr([Json::obj([("if", op2("eq", get("UiDrag.item"), n(*item as f64))), ("then", Json::Arr(then)), ("else", use_chain)])])
        };
    }
    systems.push(Json::obj([
        ("name", Json::str("item_use")),
        ("doc", Json::str("A click on a thing in the open backpack uses it: food and water and medicine change the bars, planks and nails become stock for building; the thing is used up.")),
        ("each", Json::arr([Json::str("Item"), Json::str("UiDrag"), Json::str("Ui")])),
        ("order", Json::from(15)),
        ("do", Json::arr([
            Json::obj([("if", opn("and", vec![get("Ui.clicked"), op2("lt", get("Item.owner"), n(0.5)), op2("gt", res("inv_player"), n(0.5))])), ("then", use_chain)]),
            // batteries: a click puts charge in the flashlight's battery
            Json::obj([("if", opn("and", vec![get("Ui.clicked"), op2("lt", get("Item.owner"), n(0.5)), op2("gt", res("inv_player"), n(0.5)), op2("eq", get("UiDrag.item"), n(15.0))])), ("then", Json::arr([
                Json::obj([("res_add", Json::str("battery")), ("by", n(60.0))]),
                Json::obj([("emit", Json::str("use_item"))]),
                Json::obj([("despawn", Json::Bool(true))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // the numbers run down; at nothing a bar hurts; with both full enough health comes back; at no health the player wakes at the start
    let starving = op2("or", op2("le", res("food"), n(0.01)), op2("le", res("water"), n(0.01)));
    let healing = op2("and", op2("gt", res("food"), n(40.0)), op2("gt", res("water"), n(40.0)));
    let dt = Json::obj([("dt", Json::Bool(true))]);
    let clamp = |x: Json| Json::obj([("clamp", Json::arr([x, n(0.0), n(100.0)]))]);
    systems.push(Json::obj([
        ("name", Json::str("vitals")),
        ("doc", Json::str("Food and water run down (about half an hour from full), health falls at nothing and mends when both are above 40; at no health the player wakes up at the start.")),
        ("each", Json::arr([Json::str("Hero"), Json::str("Transform")])),
        ("order", Json::from(11)),
        ("do", Json::arr([
            Json::obj([("res_set", Json::str("food")), ("to", clamp(op2("sub", res("food"), op2("mul", dt.clone(), n(0.055)))))]),
            Json::obj([("res_set", Json::str("water")), ("to", clamp(op2("sub", res("water"), op2("mul", dt.clone(), n(0.085)))))]),
            Json::obj([("res_set", Json::str("hp")), ("to", clamp(op2("add", res("hp"), op2("mul", dt.clone(), sel(starving, n(-1.4), sel(healing, n(0.35), n(0.0)))))))]),
            Json::obj([("if", op2("le", res("hp"), n(0.01))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("hp")), ("to", n(100.0))]),
                Json::obj([("res_set", Json::str("food")), ("to", n(80.0))]),
                Json::obj([("res_set", Json::str("water")), ("to", n(80.0))]),
                Json::obj([("set", Json::str("Transform.pos[0]")), ("to", n(spawn[0] as f64))]),
                Json::obj([("set", Json::str("Transform.pos[1]")), ("to", n(spawn[1] as f64 + 0.3))]),
                Json::obj([("set", Json::str("Transform.pos[2]")), ("to", n(spawn[2] as f64))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("vitals_hud")),
        ("doc", Json::str("The bars show health, food, water and the flashlight's battery (that one only while a flashlight is carried).")),
        ("each", Json::arr([Json::str("Vital"), Json::str("Ui")])),
        ("order", Json::from(16)),
        ("do", Json::arr([
            Json::obj([("set", Json::str("Ui.fill")), ("to", op2("div", sel(op2("eq", get("Vital.kind"), n(1.0)), res("hp"), sel(op2("eq", get("Vital.kind"), n(2.0)), res("food"), sel(op2("eq", get("Vital.kind"), n(3.0)), res("water"), res("battery")))), n(100.0)))]),
            Json::obj([("set", Json::str("Ui.visible")), ("to", op2("or", op2("lt", get("Vital.kind"), n(3.5)), op2("gt", res("has_torch"), n(0.5))))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // ---- saved games: F5 saves to the quick slot, F9 loads it; leaving by Esc saves to the auto slot and a new start continues from there (the slots are numbers made from the scene's own text by `citygen`, so that a save of an older
    // game, with its older systems and entities, is never loaded over a newer scene) (the engine saves the whole world: what is built, what is carried, the bars, where the
    // player stands; the streamed city is made again, so loot in the furniture is rolled afresh); a line of the HUD says what happened
    components.push(Json::obj([("name", Json::str("Toast")), ("doc", Json::str("A line of the HUD that shows for a moment: kind 1 saved, 2 loaded, 3 nothing to load.")), ("fields", Json::arr([field("kind", "f32", 1.0)]))]));
    for (k, line) in ["GAME SAVED   (F5)", "GAME LOADED   (F9)", "NO SAVED GAME YET   (F5 saves)", "THE BACKPACK IS FULL"].iter().enumerate() {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Toast {}", k + 1)))])),
            ("Text", Json::obj([("value", Json::str(*line)), ("pos", arr(&[0.36, 0.1])), ("size", n(0.034)), ("color", arr(&[1.0, 0.95, 0.7, 0.0]))])),
            ("Toast", Json::obj([("kind", n(k as f64 + 1.0))])),
        ]));
    }
    systems.push(Json::obj([
        ("name", Json::str("save_keys")),
        ("doc", Json::str("F5 saves the game (the quick slot), F9 loads it.")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(12)),
        ("do", Json::arr([
            Json::obj([("if", op2("gt", res("action.save.pressed"), n(0.5))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("game.error")), ("to", n(0.0))]),
                Json::obj([("res_set", Json::str("game.save")), ("to", n(quick_slot))]),
                Json::obj([("res_set", Json::str("key_tick")), ("to", Json::obj([("tick", Json::Bool(true))]))]),
            ]))]),
            Json::obj([("if", op2("gt", res("action.load.pressed"), n(0.5))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("game.error")), ("to", n(0.0))]),
                Json::obj([("res_set", Json::str("game.load")), ("to", n(quick_slot))]),
                Json::obj([("res_set", Json::str("key_tick")), ("to", Json::obj([("tick", Json::Bool(true))]))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // leaving: Esc with nothing open saves the auto slot and asks to quit; the quit itself comes a tick later, once the save has been made, so that the saved game does not hold a request to quit (a stale request, as when
    // a game saved a moment before is loaded, is dropped)
    systems.push(Json::obj([
        ("name", Json::str("quit_after_save")),
        ("doc", Json::str("A request to quit is carried out the tick after the save that goes with it; otherwise it is dropped.")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(3)),
        ("do", Json::arr([Json::obj([("if", op2("gt", res("quit_pending"), n(0.5))), ("then", Json::arr([
            Json::obj([("res_set", Json::str("quit_pending")), ("to", n(0.0))]),
            Json::obj([("if", op2("and", op2("ge", op2("sub", Json::obj([("tick", Json::Bool(true))]), res("game.saved")), n(-1.0)), op2("lt", op2("sub", Json::obj([("tick", Json::Bool(true))]), res("game.saved")), n(3.0)))), ("then", Json::arr([Json::obj([("res_set", Json::str("app.quit")), ("to", n(1.0))])]))]),
        ]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // a new start continues from the slot that leaving wrote (the first ticks look for it, then the looking stops)
    systems.push(Json::obj([
        ("name", Json::str("continue_game")),
        ("doc", Json::str("In the first ticks of a start: if the auto slot holds a game, load it (unless `continue_enabled` is 0).")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(2)),
        ("do", Json::arr([
            Json::obj([("if", op2("and", op2("ge", Json::obj([("tick", Json::Bool(true))]), n(2.0)), op2("lt", Json::obj([("tick", Json::Bool(true))]), n(3.0)))), ("then", Json::arr([
                Json::obj([("res_set", Json::str("game.probe")), ("to", n(0.0))]),
                Json::obj([("if", op2("and", op2("gt", res("continue_enabled"), n(0.5)), op2("gt", res(&format!("game.has.{}", auto_slot as u64)), n(0.5)))), ("then", Json::arr([Json::obj([("res_set", Json::str("game.load")), ("to", n(auto_slot))])]))]),
            ]))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    let tick = Json::obj([("tick", Json::Bool(true))]);
    let within = |since: Json| op2("and", op2("ge", since.clone(), n(0.0)), op2("lt", since, n(180.0)));
    systems.push(Json::obj([
        ("name", Json::str("toast_show")),
        ("doc", Json::str("The line of the HUD for a save or a load shows for three seconds.")),
        ("each", Json::arr([Json::str("Toast"), Json::str("Text")])),
        ("order", Json::from(16)),
        ("do", Json::arr([Json::obj([("set", Json::str("Text.color[3]")), ("to", sel(
            op2("eq", get("Toast.kind"), n(1.0)),
            sel(within(op2("sub", tick.clone(), res("game.saved"))), n(0.95), n(0.0)),
            sel(
                op2("eq", get("Toast.kind"), n(2.0)),
                sel(within(op2("sub", tick.clone(), res("game.loaded"))), n(0.95), n(0.0)),
                sel(
                    op2("eq", get("Toast.kind"), n(4.0)),
                    sel(within(op2("sub", tick.clone(), res("full_tick"))), n(0.95), n(0.0)),
                    sel(op2("and", op2("gt", res("game.error"), n(0.5)), within(op2("sub", tick.clone(), res("key_tick")))), n(0.95), n(0.0)),
                ),
            ),
        ))])])),
        ("enabled", Json::Bool(true)),
    ]));
    let mut resources = vec![("game.saved".to_string(), n(-10000.0)), ("game.loaded".to_string(), n(-10000.0)), ("game.probe".to_string(), n(auto_slot)), ("quick_slot".to_string(), n(quick_slot)), ("auto_slot".to_string(), n(auto_slot)), ("continue_enabled".to_string(), n(1.0)), ("quit_pending".to_string(), n(0.0)), ("step_acc".to_string(), n(0.0)), ("step_alt".to_string(), n(0.0)), ("steps_taken".to_string(), n(0.0)), ("last_x".to_string(), n(spawn[0] as f64)), ("last_z".to_string(), n(spawn[2] as f64)), ("has_hatchet".to_string(), n(0.0)), ("has_crowbar".to_string(), n(0.0)), ("has_torch".to_string(), n(0.0)), ("torch_on".to_string(), n(0.0)), ("battery".to_string(), n(100.0)), ("pickup_item".to_string(), n(0.0)), ("pickup_uid".to_string(), n(0.0)), ("pickup_t".to_string(), n(0.0)), ("pickup_done".to_string(), n(0.0)), ("free_slot".to_string(), n(0.0)), ("full_tick".to_string(), n(-10000.0)), ("key_tick".to_string(), n(-10000.0)), ("inv_x".to_string(), n(0.0)), ("inv_z".to_string(), n(0.0)), ("inv_player".to_string(), n(0.0)), ("inv_open".to_string(), n(0.0)), ("ui_open".to_string(), n(0.0)), ("app.quit_on_escape".to_string(), n(0.0)), ("hp".to_string(), n(100.0)), ("food".to_string(), n(80.0)), ("water".to_string(), n(80.0)), ("mat_wood".to_string(), n(12.0)), ("mat_nails".to_string(), n(12.0))];
    // building (its first entity, the anchor, takes the next uid)
    let mut prefabs = prefabs;
    let b = build_parts(first_uid + entities.len() as u64);
    components.extend(b.components);
    prefabs.extend(b.prefabs);
    entities.extend(b.entities);
    systems.extend(b.systems);
    resources.extend(b.resources);
    Survival { components, prefabs, entities, systems, resources, bindings: "inventory = tab\nclose = escape\nsave = f5\nload = f9\ncraft = c\nbuild = b\npiece1 = 1\npiece2 = 2\npiece3 = 3\npiece4 = 4\npiece5 = 5\npiece6 = 6\ndemolish = x" }
}

/// What building needs: the pieces as prefabs, the ghost and the labels, the systems that aim, place and remove, the resources and the bindings. The first entity of `entities` is an anchor at the world's origin that a door
/// piece hangs on (the city's door system moves a leaf by its `LocalTransform`, and the leaf must have a parent); its uid is `first_uid`.
struct Building {
    components: Vec<Json>,
    prefabs: Vec<(String, Json)>,
    entities: Vec<Json>,
    systems: Vec<Json>,
    resources: Vec<(String, Json)>,
}

const PIECES: [&str; 6] = ["Foundation", "Wall", "Doorway", "Floor", "Roof", "Door"];

fn build_parts(first_uid: u64) -> Building {
    let get = |k: &str| Json::obj([("get", Json::str(k))]);
    let res = |k: &str| Json::obj([("res", Json::str(k))]);
    let var = |k: &str| Json::obj([("var", Json::str(k))]);
    let op1 = |op: &str, a: Json| Json::obj([(op, a)]);
    let op2 = |op: &str, a: Json, b: Json| Json::obj([(op, Json::arr([a, b]))]);
    let opn = |op: &str, a: Vec<Json>| Json::obj([(op, Json::Arr(a))]);
    let sel = |c: Json, a: Json, b: Json| Json::obj([("select", Json::arr([c, a, b]))]);
    let field = |name: &str, default: f64| Json::obj([("name", Json::str(name)), ("type", Json::str("f32")), ("default", n(default))]);
    let uid_now = Json::obj([("uid", Json::Bool(true))]);
    let anchor = first_uid;
    // what a piece costs in wood and nails
    let price = |k: u32| -> (f64, f64) {
        match k {
            1 => (6.0, 0.0),
            2 => (4.0, 2.0),
            3 => (4.0, 2.0),
            4 => (4.0, 0.0),
            5 => (4.0, 2.0),
            _ => (3.0, 2.0),
        }
    };
    let components = vec![
        Json::obj([("name", Json::str("Placing")), ("doc", Json::str("A piece that has just been put down and is waiting to be moved to (x, y, z) turned yaw radians; side is the side a door's outside is on; kind says how the numbers are used.")), ("fields", Json::arr([field("x", 0.0), field("y", 0.0), field("z", 0.0), field("yaw", 0.0), field("side", 1.0), field("kind", 1.0)]))]),
        Json::obj([("name", Json::str("Piece")), ("doc", Json::str("A piece of a player's building: the key of where it stands (so that a second is not put in the same place).")), ("fields", Json::arr([Json::obj([("name", Json::str("key")), ("type", Json::str("f64")), ("default", n(0.0))])]))]),
        Json::obj([("name", Json::str("PieceBox")), ("doc", Json::str("One box of a piece (what can be taken away).")), ("fields", Json::arr([]))]),
        Json::obj([("name", Json::str("BuildGhost")), ("doc", Json::str("The see-through piece that shows where the next one goes.")), ("fields", Json::arr([]))]),
        Json::obj([("name", Json::str("BuildLabel")), ("doc", Json::str("A line of the HUD shown while building with piece `kind`.")), ("fields", Json::arr([field("kind", 1.0)]))]),
    ];
    let texture = |name: &str| format!("city/textures/{name}/diff.jpg");
    let boxed = |name: &str, pos: [f64; 3], scale: [f64; 3], tex: &str, col: [f64; 3]| {
        Json::obj([
            ("name", Json::str(name)),
            ("components", Json::obj([
                ("LocalTransform", Json::obj([("pos", arr(&pos)), ("scale", arr(&scale))])),
                ("MeshRenderer", Json::obj([("mesh", Json::str("cube")), ("color", arr(&[col[0], col[1], col[2], 1.0])), ("texture", Json::str(texture(tex))), ("uv_scale", arr(&[1.0, 1.0])), ("roughness", n(0.85))])),
                ("Collider", Json::obj([("shape", Json::from(1)), ("size", arr(&[1.0, 1.0, 1.0])), ("trigger", Json::Bool(false))])),
                ("PieceBox", Json::Obj(Vec::new())),
            ])),
        ])
    };
    let root = |name: &str, kind: f64, children: Vec<Json>| {
        Json::obj([
            ("components", Json::obj([
                ("Name", Json::obj([("value", Json::str(name))])),
                ("Transform", Json::obj([("pos", arr(&[0.0, -50.0, 0.0]))])),
                ("Placing", Json::obj([("kind", n(kind))])),
                ("Piece", Json::obj([("key", n(0.0))])),
            ])),
            ("children", Json::Arr(children)),
        ])
    };
    let wood = [0.85, 0.8, 0.72];
    let mut prefabs: Vec<(String, Json)> = Vec::new();
    prefabs.push(("Build_Foundation".to_string(), root("Foundation", 1.0, vec![boxed("Slab", [0.0, 0.15, 0.0], [3.0, 0.3, 3.0], "concrete_slab_wall_02", [0.8, 0.8, 0.78])])));
    prefabs.push(("Build_Wall".to_string(), root("Wall", 2.0, vec![boxed("Planks", [0.0, 1.5, 0.0], [3.0, 3.0, 0.2], "wood_planks_grey", wood)])));
    prefabs.push(("Build_Doorway".to_string(), root("Doorway", 3.0, vec![
        boxed("Left post", [-1.05, 1.5, 0.0], [0.9, 3.0, 0.2], "wood_planks_grey", wood),
        boxed("Right post", [1.05, 1.5, 0.0], [0.9, 3.0, 0.2], "wood_planks_grey", wood),
        boxed("Lintel", [0.0, 2.65, 0.0], [1.2, 0.7, 0.2], "wood_planks_grey", wood),
    ])));
    prefabs.push(("Build_Floor".to_string(), root("Floor", 4.0, vec![boxed("Boards", [0.0, 0.1, 0.0], [3.0, 0.2, 3.0], "wood_planks_grey", [0.8, 0.72, 0.62])])));
    prefabs.push(("Build_Roof".to_string(), root("Roof", 5.0, vec![boxed("Sheet", [0.0, 0.1, 0.0], [3.2, 0.2, 3.2], "roof_slates_03", [0.7, 0.7, 0.72])])));
    // a door: the city's own `Door`, hanging on the anchor so that its leaf is placed by `door_swing`
    prefabs.push(("Build_Door".to_string(), Json::obj([("components", Json::obj([
        ("Name", Json::obj([("value", Json::str("Door piece"))])),
        ("Parent", Json::obj([("target", n(anchor as f64))])),
        ("LocalTransform", Json::obj([("pos", arr(&[0.0, -50.0, 0.0])), ("scale", arr(&[1.15, 2.2, 0.06]))])),
        ("MeshRenderer", Json::obj([("mesh", Json::str("cube")), ("color", arr(&[0.72, 0.5, 0.3, 1.0])), ("texture", Json::str(texture("wood_planks_grey"))), ("roughness", n(0.8))])),
        ("Collider", Json::obj([("shape", Json::from(1)), ("size", arr(&[1.0, 1.0, 1.0])), ("trigger", Json::Bool(false))])),
        ("Door", Json::obj([("half", n(0.575)), ("maxang", n(1.75))])),
        ("Placing", Json::obj([("kind", n(6.0))])),
        ("Piece", Json::obj([("key", n(0.0))])),
    ]))])));

    // ---- entities: the anchor (first), the ghost, the labels
    let mut entities: Vec<Json> = Vec::new();
    entities.push(Json::obj([("Name", Json::obj([("value", Json::str("World anchor"))])), ("Transform", Json::obj([("pos", arr(&[0.0, 0.0, 0.0]))]))]));
    // the stock and the price of the piece, while building
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Build stock"))])),
        ("Text", Json::obj([("value", Json::str("WOOD {mat_wood}   NAILS {mat_nails}      this piece: wood {build_cost_w}  nails {build_cost_n}")), ("pos", arr(&[0.30, 0.86])), ("size", n(0.024)), ("color", arr(&[1.0, 0.95, 0.7, 0.0]))])),
        ("BuildLabel", Json::obj([("kind", n(0.0))])),
    ]));
    entities.push(Json::obj([
        ("Name", Json::obj([("value", Json::str("Build ghost"))])),
        ("Transform", Json::obj([("pos", arr(&[0.0, -50.0, 0.0])), ("scale", arr(&[3.0, 0.3, 3.0]))])),
        ("MeshRenderer", Json::obj([("mesh", Json::str("cube")), ("color", arr(&[0.3, 0.9, 0.4, 0.38])), ("cast_shadow", Json::Bool(false)), ("visible", Json::Bool(false)), ("roughness", n(0.9))])),
        ("BuildGhost", Json::Obj(Vec::new())),
    ]));
    for (k, name) in PIECES.iter().enumerate() {
        entities.push(Json::obj([
            ("Name", Json::obj([("value", Json::str(format!("Build label {name}")))])),
            ("Text", Json::obj([("value", Json::str(format!("BUILD   {name}     1-6 choose   click place   X remove   B stop"))), ("pos", arr(&[0.30, 0.90])), ("size", n(0.026)), ("color", arr(&[1.0, 0.95, 0.7, 0.0]))])),
            ("BuildLabel", Json::obj([("kind", n(k as f64 + 1.0))])),
        ]));
    }

    // ---- systems
    let mut systems: Vec<Json> = Vec::new();
    // B starts and ends building, 1 to 6 choose the piece (with a panel open nothing happens and building ends)
    let mut keys: Vec<Json> = vec![Json::obj([("if", opn("and", vec![op2("gt", res("action.build.pressed"), n(0.5)), op2("lt", res("ui_open"), n(0.5))])), ("then", Json::arr([Json::obj([("res_set", Json::str("build_on")), ("to", op2("sub", n(1.0), res("build_on")))])]))])];
    for k in 1..=6 {
        keys.push(Json::obj([("if", op2("gt", res(&format!("action.piece{k}.pressed")), n(0.5))), ("then", Json::arr([Json::obj([("res_set", Json::str("build_piece")), ("to", n(k as f64))])]))]));
    }
    keys.push(Json::obj([("if", op2("gt", res("ui_open"), n(0.5))), ("then", Json::arr([Json::obj([("res_set", Json::str("build_on")), ("to", n(0.0))])]))]));
    systems.push(Json::obj([("name", Json::str("build_keys")), ("doc", Json::str("B starts and ends building, the number keys choose the piece.")), ("each", Json::arr([Json::str("Hero")])), ("order", Json::from(12)), ("do", Json::Arr(keys)), ("enabled", Json::Bool(true))]));
    // where the next piece goes: the ray's hit point snapped to a 3 m grid; a wall, doorway or door on the nearest edge of the cell, a foundation, floor or roof in the middle of it; the height is that of what is aimed at
    let (hx, hy, hz) = (get("Probe.hit_point[0]"), get("Probe.hit_point[1]"), get("Probe.hit_point[2]"));
    let cell = |h: Json| op2("add", op2("mul", op1("floor", op2("div", h, n(3.0))), n(3.0)), n(1.5));
    let aim = vec![
        Json::obj([("let", Json::str("cx")), ("be", cell(hx.clone()))]),
        Json::obj([("let", Json::str("cz")), ("be", cell(hz.clone()))]),
        Json::obj([("let", Json::str("dx")), ("be", op2("sub", hx.clone(), var("cx")))]),
        Json::obj([("let", Json::str("dz")), ("be", op2("sub", hz.clone(), var("cz")))]),
        // along x (the edge is a line of constant z) when the hit lies nearer that edge than the other
        Json::obj([("let", Json::str("alongx")), ("be", op2("ge", op1("abs", var("dz")), op1("abs", var("dx"))))]),
        Json::obj([("let", Json::str("wallkind")), ("be", opn("or", vec![op2("eq", res("build_piece"), n(2.0)), op2("eq", res("build_piece"), n(3.0)), op2("eq", res("build_piece"), n(6.0))]))]),
        Json::obj([("let", Json::str("yaw")), ("be", sel(opn("and", vec![var("wallkind"), op1("not", var("alongx"))]), n(std::f64::consts::FRAC_PI_2), n(0.0)))]),
        Json::obj([("res_set", Json::str("build_yaw")), ("to", var("yaw"))]),
        Json::obj([("res_set", Json::str("build_x")), ("to", sel(var("wallkind"), sel(var("alongx"), var("cx"), op2("add", var("cx"), op2("mul", op1("sign", var("dx")), n(1.5)))), var("cx")))]),
        Json::obj([("res_set", Json::str("build_z")), ("to", sel(var("wallkind"), sel(var("alongx"), op2("add", var("cz"), op2("mul", op1("sign", var("dz")), n(1.5))), var("cz")), var("cz")))]),
        // the height: a horizontal surface aimed at (its normal up) is the base; a wall's side: the floor under the player
        Json::obj([("res_set", Json::str("build_y")), ("to", op2("mul", op1("round", op2("mul", sel(op2("gt", get("Probe.hit_normal[1]"), n(0.7)), hy.clone(), get("Probe.ground")), n(20.0))), n(0.05)))]),
        Json::obj([("res_set", Json::str("build_side")), ("to", sel(op2("ge", op2("add", op2("mul", op2("sub", res("eye_x"), res("build_x")), op1("sin", var("yaw"))), op2("mul", op2("sub", res("eye_z"), res("build_z")), op1("cos", var("yaw")))), n(0.0)), n(1.0), n(-1.0)))]),
        Json::obj([("res_set", Json::str("build_key")), ("to", opn("add", vec![
            op2("mul", res("build_piece"), n(1_000_003.0)),
            op2("mul", op1("round", op2("mul", res("build_x"), n(2.0))), n(7919.0)),
            op2("mul", op1("round", op2("mul", res("build_z"), n(2.0))), n(104729.0)),
            op2("mul", op1("round", op2("mul", res("build_y"), n(10.0))), n(31.0)),
        ]))]),
    ];
    let aiming = opn("and", vec![op2("gt", res("build_on"), n(0.5)), get("Probe.hit")]);
    let aim_do = vec![
        Json::obj([("res_set", Json::str("build_dup")), ("to", n(0.0))]),
        Json::obj([("res_set", Json::str("build_aimed")), ("to", sel(aiming.clone(), n(1.0), n(0.0)))]),
        Json::obj([("if", aiming), ("then", Json::Arr(aim))]),
    ];
    systems.push(Json::obj([("name", Json::str("build_aim")), ("doc", Json::str("Where the next piece goes: the eye ray's hit point snapped to the grid.")), ("each", Json::arr([Json::str("Probe"), Json::str("Transform")])), ("without", Json::arr([Json::str("RainCloud")])), ("order", Json::from(8)), ("do", Json::Arr(aim_do)), ("enabled", Json::Bool(true))]));
    // a piece already where the next goes
    systems.push(Json::obj([
        ("name", Json::str("build_check")),
        ("doc", Json::str("Tells the aim when a piece of the same kind already stands where it points.")),
        ("each", Json::arr([Json::str("Piece")])),
        ("order", Json::from(9)),
        ("do", Json::arr([Json::obj([("if", op2("eq", get("Piece.key"), res("build_key"))), ("then", Json::arr([Json::obj([("res_set", Json::str("build_dup")), ("to", n(1.0))])]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // the ghost: shown while building and aiming, the size and height of the piece, green where it can go and red where one stands
    let dims = |k: u32| -> ([f64; 3], f64) {
        match k {
            1 => ([3.0, 0.3, 3.0], 0.15),
            2 | 3 => ([3.0, 3.0, 0.2], 1.5),
            4 => ([3.0, 0.2, 3.0], 0.1),
            5 => ([3.2, 0.2, 3.2], 0.1),
            _ => ([1.15, 2.2, 0.06], 1.1),
        }
    };
    let by_kind = |f: &dyn Fn(u32) -> f64| -> Json {
        let mut chain = n(f(6));
        for k in (1..6u32).rev() {
            chain = sel(op2("eq", res("build_piece"), n(k as f64)), n(f(k)), chain);
        }
        chain
    };
    // the price of the piece chosen, and whether the stock pays it
    systems.push(Json::obj([
        ("name", Json::str("build_cost")),
        ("doc", Json::str("The price of the chosen piece and whether the wood and nails in stock pay for it.")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(8)),
        ("do", Json::arr([
            Json::obj([("res_set", Json::str("build_cost_w")), ("to", by_kind(&|k| price(k).0))]),
            Json::obj([("res_set", Json::str("build_cost_n")), ("to", by_kind(&|k| price(k).1))]),
            Json::obj([("res_set", Json::str("build_afford")), ("to", sel(op2("and", op2("ge", res("mat_wood"), by_kind(&|k| price(k).0)), op2("ge", res("mat_nails"), by_kind(&|k| price(k).1))), n(1.0), n(0.0)))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    let mut ghost: Vec<Json> = Vec::new();
    for i in 0..3 {
        ghost.push(Json::obj([("set", Json::str(format!("Transform.scale[{i}]"))), ("to", by_kind(&|k| dims(k).0[i]))]));
    }
    ghost.push(Json::obj([("set", Json::str("Transform.pos[0]")), ("to", res("build_x"))]));
    ghost.push(Json::obj([("set", Json::str("Transform.pos[1]")), ("to", op2("add", res("build_y"), by_kind(&|k| dims(k).1)))]));
    ghost.push(Json::obj([("set", Json::str("Transform.pos[2]")), ("to", res("build_z"))]));
    ghost.push(Json::obj([("set", Json::str("Transform.rot[0]")), ("to", n(0.0))]));
    ghost.push(Json::obj([("set", Json::str("Transform.rot[1]")), ("to", op1("sin", op2("mul", res("build_yaw"), n(0.5))))]));
    ghost.push(Json::obj([("set", Json::str("Transform.rot[2]")), ("to", n(0.0))]));
    ghost.push(Json::obj([("set", Json::str("Transform.rot[3]")), ("to", op1("cos", op2("mul", res("build_yaw"), n(0.5))))]));
    ghost.push(Json::obj([("set", Json::str("MeshRenderer.visible")), ("to", op2("gt", res("build_aimed"), n(0.5)))]));
    let blocked = op2("or", op2("gt", res("build_dup"), n(0.5)), op2("lt", res("build_afford"), n(0.5)));
    ghost.push(Json::obj([("set", Json::str("MeshRenderer.color[0]")), ("to", sel(blocked.clone(), n(0.95), n(0.3)))]));
    ghost.push(Json::obj([("set", Json::str("MeshRenderer.color[1]")), ("to", sel(blocked, n(0.25), n(0.9)))]));
    systems.push(Json::obj([("name", Json::str("build_ghost")), ("doc", Json::str("The ghost of the next piece.")), ("each", Json::arr([Json::str("BuildGhost"), Json::str("Transform"), Json::str("MeshRenderer")])), ("order", Json::from(15)), ("do", Json::Arr(ghost)), ("enabled", Json::Bool(true))]));
    systems.push(Json::obj([
        ("name", Json::str("build_labels")),
        ("doc", Json::str("The line of the piece chosen shows while building.")),
        ("each", Json::arr([Json::str("BuildLabel"), Json::str("Text")])),
        ("order", Json::from(15)),
        ("do", Json::arr([Json::obj([("set", Json::str("Text.color[3]")), ("to", sel(opn("and", vec![op2("gt", res("build_on"), n(0.5)), op2("or", op2("eq", get("BuildLabel.kind"), n(0.0)), op2("eq", res("build_piece"), get("BuildLabel.kind")))]), n(0.95), n(0.0)))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // putting down: a click (the left button came down this tick) while aiming at a free place
    let mut place_chain: Json = Json::arr([]);
    for k in (1..=6u32).rev() {
        let spawn = Json::obj([("spawn_prefab", Json::str(format!("Build_{}", PIECES[k as usize - 1]))), ("with", Json::obj([
            ("Placing", Json::obj([("x", res("build_x")), ("y", res("build_y")), ("z", res("build_z")), ("yaw", res("build_yaw")), ("side", res("build_side")), ("kind", n(k as f64))])),
            ("Piece", Json::obj([("key", res("build_key"))])),
        ]))]);
        place_chain = if k == 6 { Json::arr([spawn]) } else { Json::arr([Json::obj([("if", op2("eq", res("build_piece"), n(k as f64))), ("then", Json::arr([spawn])), ("else", place_chain)])]) };
    }
    systems.push(Json::obj([
        ("name", Json::str("build_place")),
        ("doc", Json::str("A click puts the chosen piece where the ghost is.")),
        ("each", Json::arr([Json::str("Hero")])),
        ("order", Json::from(16)),
        ("do", Json::arr([
            Json::obj([("if", opn("and", vec![op2("gt", res("input.mouse_left"), n(0.5)), op2("lt", res("build_mouse_prev"), n(0.5)), op2("gt", res("build_aimed"), n(0.5)), op2("lt", res("build_dup"), n(0.5)), op2("gt", res("build_afford"), n(0.5)), op2("lt", res("ui_open"), n(0.5))])), ("then", {
                let mut v = vec![Json::obj([("emit", Json::str("build_place"))]), Json::obj([("res_add", Json::str("mat_wood")), ("by", op1("neg", res("build_cost_w")))]), Json::obj([("res_add", Json::str("mat_nails")), ("by", op1("neg", res("build_cost_n")))])];
                if let Json::Arr(a) = &place_chain {
                    v.extend(a.clone());
                }
                Json::Arr(v)
            })]),
            Json::obj([("res_set", Json::str("build_mouse_prev")), ("to", res("input.mouse_left"))]),
        ])),
        ("enabled", Json::Bool(true)),
    ]));
    // a new piece is moved to where it was put (and a door's leaf is hung there)
    let yaw = get("Placing.yaw");
    let half_yaw = op2("mul", yaw.clone(), n(0.5));
    let plain = vec![
        Json::obj([("set", Json::str("Transform.pos[0]")), ("to", get("Placing.x"))]),
        Json::obj([("set", Json::str("Transform.pos[1]")), ("to", get("Placing.y"))]),
        Json::obj([("set", Json::str("Transform.pos[2]")), ("to", get("Placing.z"))]),
        Json::obj([("set", Json::str("Transform.rot[1]")), ("to", op1("sin", half_yaw.clone()))]),
        Json::obj([("set", Json::str("Transform.rot[3]")), ("to", op1("cos", half_yaw.clone()))]),
        Json::obj([("remove", Json::str("Placing"))]),
    ];
    let door = {
        // the leaf stands in the opening, hinged on the left jamb, opening away from the player's side
        let half = 0.575;
        let (ax, az) = (op1("cos", yaw.clone()), op1("neg", op1("sin", yaw.clone())));
        let (ox, oz) = (op2("mul", get("Placing.side"), op1("sin", yaw.clone())), op2("mul", get("Placing.side"), op1("cos", yaw.clone())));
        vec![
            Json::obj([("set", Json::str("Door.hx")), ("to", op2("sub", get("Placing.x"), op2("mul", ax.clone(), n(half))))]),
            Json::obj([("set", Json::str("Door.hy")), ("to", op2("add", get("Placing.y"), n(1.1)))]),
            Json::obj([("set", Json::str("Door.hz")), ("to", op2("sub", get("Placing.z"), op2("mul", az.clone(), n(half))))]),
            Json::obj([("set", Json::str("Door.ax")), ("to", ax)]),
            Json::obj([("set", Json::str("Door.az")), ("to", az)]),
            Json::obj([("set", Json::str("Door.ox")), ("to", ox)]),
            Json::obj([("set", Json::str("Door.oz")), ("to", oz)]),
            Json::obj([("set", Json::str("Door.yaw0")), ("to", yaw.clone())]),
            Json::obj([("set", Json::str("Door.dir")), ("to", get("Placing.side"))]),
            Json::obj([("set", Json::str("LocalTransform.pos[0]")), ("to", get("Placing.x"))]),
            Json::obj([("set", Json::str("LocalTransform.pos[1]")), ("to", op2("add", get("Placing.y"), n(1.1)))]),
            Json::obj([("set", Json::str("LocalTransform.pos[2]")), ("to", get("Placing.z"))]),
            Json::obj([("remove", Json::str("Placing"))]),
        ]
    };
    systems.push(Json::obj([
        ("name", Json::str("piece_place")),
        ("doc", Json::str("A new piece moves to where it was put.")),
        ("each", Json::arr([Json::str("Placing"), Json::str("Transform")])),
        ("order", Json::from(17)),
        ("do", Json::arr([Json::obj([("if", op2("ne", get("Placing.kind"), n(6.0))), ("then", Json::Arr(plain))])])),
        ("enabled", Json::Bool(true)),
    ]));
    systems.push(Json::obj([
        ("name", Json::str("door_place")),
        ("doc", Json::str("A new door piece hangs its leaf where it was put.")),
        ("each", Json::arr([Json::str("Placing"), Json::str("Door"), Json::str("LocalTransform")])),
        ("order", Json::from(17)),
        ("do", Json::arr([Json::obj([("if", op2("eq", get("Placing.kind"), n(6.0))), ("then", Json::Arr(door))])])),
        ("enabled", Json::Bool(true)),
    ]));
    // taking away: X while looking at a box of a piece
    systems.push(Json::obj([
        ("name", Json::str("build_remove")),
        ("doc", Json::str("X takes away the box of a piece that is looked at (a wall is one box; a doorway comes down post by post).")),
        ("each", Json::arr([Json::str("PieceBox")])),
        ("order", Json::from(18)),
        ("do", Json::arr([Json::obj([("if", opn("and", vec![op2("eq", uid_now, res("interact_uid")), op2("gt", res("action.demolish.pressed"), n(0.5)), op2("gt", res("build_on"), n(0.5))])), ("then", Json::arr([Json::obj([("emit", Json::str("build_remove"))]), Json::obj([("despawn", Json::Bool(true))])]))])])),
        ("enabled", Json::Bool(true)),
    ]));
    let resources = vec![("build_cost_w", 0.0), ("build_cost_n", 0.0), ("build_afford", 1.0), ("build_on", 0.0), ("build_piece", 1.0), ("build_x", 0.0), ("build_y", 0.0), ("build_z", 0.0), ("build_yaw", 0.0), ("build_side", 1.0), ("build_key", 0.0), ("build_dup", 0.0), ("build_aimed", 0.0), ("build_mouse_prev", 0.0)].into_iter().map(|(k, v)| (k.to_string(), n(v))).collect();
    Building { components, prefabs, entities, systems, resources }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panels_slots_and_systems_fit_together() {
        let s = build(100, [0.0; 3], [1.0, 9.0], 2);
        // two panels with their titles and slots, the tooltip box, three starting items, the prompt and the hint
        // (and, for building, the anchor, the ghost and six labels)
        // (and the three bars, and the stock line of building)
        assert_eq!(s.entities.len(), 2 + PLAYER_COLS * PLAYER_ROWS + 2 + BOX_COLS * BOX_ROWS + 1 + 1 + 3 + 2 + 1 + 4 + 8 + 4 + 1 + 1 + 6 + 2 + 1 + 2 + 1 + 2 + 8 + 4);
        assert_eq!(s.prefabs.len(), 2 * ITEMS.len() + 6 + 1);
        assert_eq!(s.systems.len(), 4 + 9 + 4 + 1 + 4 + 3 + 3 + 1 + 1 + 1 + 1);
        for t in LOOT {
            assert!(t.iter().all(|(i, w)| *i < ITEMS.len() && *w > 0.0));
        }
        assert!(s.components.len() == 3 + 1 + 1 + 1 + 1 + 5 + 1 + 1);
    }
}
