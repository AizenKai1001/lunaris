//! `import_tiled`: bring a map from Tiled (https://www.mapeditor.org, the usual 2D level editor; the JSON export `.tmj`/`.json`) into the world. Every tile layer becomes an
//! entity with a `Tilemap` (its tileset picture, sheet and numbers), a tile layer that is marked solid (a custom property `solid` set to true, or a name starting with
//! `solid` or `collision`) also gets merged static box colliders like `tilemap_colliders`, and the objects of the object layers (spawn points, triggers, doors: whatever
//! the level designer placed) come back as a list in world coordinates for the agent to turn into entities.
//!
//! Tiled counts pixels with y down from the map's top left corner; the world here is y up with the map centred on `pos`: a pixel `(px, py)` is at
//! `((px / tilewidth - width / 2) * tile, (height / 2 - py / tileheight) * tile)` from it, where `tile` is how many world units a tile is.
use crate::engine::{uid_json, Args, Engine};
use crate::tilecol::merge;
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_sim::Command;

fn prop<'a>(o: &'a Json, name: &str) -> Option<&'a Json> {
    o.get("properties")?.as_array()?.iter().find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name)).and_then(|p| p.get("value"))
}

fn num(o: &Json, key: &str, default: f64) -> f64 {
    o.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

/// The tileset a layer uses, as (first gid, picture path relative to the workspace, columns, rows, tile width, tile height).
struct Tileset {
    first: u64,
    image: String,
    columns: u64,
    rows: u64,
}

fn read_tileset(eng: &Engine, ts: &Json, map_dir: &str) -> Result<Tileset> {
    let first = ts.get("firstgid").and_then(|v| v.as_u64()).unwrap_or(1);
    // an external tileset (`source`) is another JSON file next to the map
    let (def, base): (Json, String) = match ts.get("source").and_then(|s| s.as_str()) {
        Some(src) => {
            let rel = join(map_dir, src);
            let text = std::fs::read_to_string(eng.root.join(&rel)).map_err(|e| EcsError::new("tileset_unreadable", format!("cannot read the tileset `{rel}`: {e}")).hint("export the tileset as JSON (.tsj) next to the map"))?;
            let loaded = lunaris_json::parse(&text).map_err(|e| EcsError::new("tileset_bad_json", format!("`{rel}` is not valid JSON: {e}")))?;
            (loaded, parent(&rel))
        }
        None => (ts.clone(), map_dir.to_string()),
    };
    let image = def.get("image").and_then(|i| i.as_str()).ok_or_else(|| EcsError::new("tileset_no_image", "a tileset of the map has no `image` (a collection of images is not supported)").hint("use a tileset made from one picture"))?;
    let (columns, count) = (def.get("columns").and_then(|v| v.as_u64()).unwrap_or(1).max(1), def.get("tilecount").and_then(|v| v.as_u64()).unwrap_or(1).max(1));
    Ok(Tileset { first, image: join(&base, image), columns, rows: count.div_ceil(columns) })
}

fn parent(path: &str) -> String {
    path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
}

/// `base/rel` with `..` folded away and no leading slash.
fn join(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    let rel = rel.replace('\\', "/");
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

impl Engine {
    pub(crate) fn import_tiled_tool(&mut self, a: &Args) -> Result<Json> {
        let path = a.str("path")?.to_string();
        let rel = lunaris_json::safe_relative(&path).map_err(|e| EcsError::new("bad_path", format!("{e}")).at("path"))?;
        let rel = rel.replace('\\', "/");
        let tile = match a.0.get("tile") {
            None | Some(Json::Null) => 1.0,
            Some(v) => v.as_f64().filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| EcsError::new("bad_value", "`tile` must be a positive number (world units per tile)").at("tile"))?,
        };
        let origin: [f64; 3] = match a.0.get("pos") {
            None | Some(Json::Null) => [0.0; 3],
            Some(Json::Arr(v)) if v.len() == 3 && v.iter().all(|x| x.as_f64().is_some()) => [v[0].as_f64().unwrap(), v[1].as_f64().unwrap(), v[2].as_f64().unwrap()],
            Some(_) => return Err(EcsError::new("bad_value", "`pos` must be [x, y, z]").at("pos")),
        };
        let text = std::fs::read_to_string(self.root.join(&rel)).map_err(|e| EcsError::new("map_unreadable", format!("cannot read `{rel}`: {e}")).at("path"))?;
        let map = lunaris_json::parse(&text).map_err(|e| EcsError::new("map_bad_json", format!("`{rel}` is not valid JSON: {e}")).hint("export the map from Tiled as JSON (File > Export As > .tmj)"))?;
        if map.get("infinite").and_then(|v| v.as_bool()) == Some(true) {
            return Err(EcsError::new("map_infinite", "an infinite map is not supported").hint("in Tiled: Map > Map Properties > turn off Infinite, then export again"));
        }
        let (w, h) = (map.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as usize, map.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as usize);
        let (tw, th) = (num(&map, "tilewidth", 0.0), num(&map, "tileheight", 0.0));
        if w == 0 || h == 0 || w * h > 262_144 || tw <= 0.0 || th <= 0.0 {
            return Err(EcsError::new("map_bad_size", "the map has no size, a tile size of 0, or more than 262144 cells"));
        }
        let dir = parent(&rel);
        let mut tilesets: Vec<Tileset> = Vec::new();
        for ts in map.get("tilesets").and_then(|t| t.as_array()).unwrap_or(&[]) {
            tilesets.push(read_tileset(self, ts, &dir)?);
        }
        tilesets.sort_by_key(|t| t.first);
        let mut warnings: Vec<String> = Vec::new();
        let mut cmds: Vec<Command> = Vec::new();
        let mut layers_out: Vec<Json> = Vec::new();
        let mut objects_out: Vec<Json> = Vec::new();
        let mut next = self.sim.world.next_uid();
        let world_x = |px: f64| (px / tw - w as f64 / 2.0) * tile + origin[0];
        let world_y = |py: f64| (h as f64 / 2.0 - py / th) * tile + origin[1];
        let round = |v: f64| Json::Float((v * 10_000.0).round() / 10_000.0);
        for layer in map.get("layers").and_then(|l| l.as_array()).unwrap_or(&[]) {
            let name = layer.get("name").and_then(|n| n.as_str()).unwrap_or("layer").to_string();
            match layer.get("type").and_then(|t| t.as_str()) {
                Some("tilelayer") => {
                    let Some(data) = layer.get("data").and_then(|d| d.as_array()) else {
                        warnings.push(format!("layer `{name}` has no data (compressed layer data is not supported: choose CSV in the map properties)"));
                        continue;
                    };
                    let gids: Vec<u64> = data.iter().map(|g| g.as_u64().unwrap_or(0) & 0x1FFF_FFFF).collect();
                    // the tileset that the first used gid belongs to
                    let used = gids.iter().copied().filter(|g| *g != 0).min();
                    let Some(first_gid) = used else {
                        warnings.push(format!("layer `{name}` is empty and was skipped"));
                        continue;
                    };
                    let Some(ts) = tilesets.iter().rev().find(|t| t.first <= first_gid) else {
                        warnings.push(format!("layer `{name}` uses tile {first_gid}, which no tileset has"));
                        continue;
                    };
                    let upper = tilesets.iter().find(|t| t.first > ts.first).map_or(u64::MAX, |t| t.first);
                    if gids.iter().any(|g| *g != 0 && (*g < ts.first || *g >= upper)) {
                        warnings.push(format!("layer `{name}` mixes tiles of more than one tileset: only those of `{}` are kept", ts.image));
                    }
                    let numbers: Vec<i64> = gids.iter().map(|g| if *g >= ts.first && *g < upper { (*g - ts.first) as i64 } else { -1 }).collect();
                    let rows: Vec<String> = numbers.chunks(w).map(|r| r.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ")).collect();
                    let uid = next;
                    next += 1;
                    let tilemap = Json::obj([
                        ("texture", Json::str(ts.image.clone())),
                        ("sheet", Json::arr([Json::Int(ts.columns as i64), Json::Int(ts.rows as i64)])),
                        ("size", Json::arr([Json::from(w), Json::from(h)])),
                        ("tile", round(tile)),
                        ("data", Json::str(rows.join("\n"))),
                        ("visible", Json::Bool(layer.get("visible").and_then(|v| v.as_bool()).unwrap_or(true))),
                    ]);
                    let z = origin[2] + (layers_out.len() as f64) * 0.01;
                    cmds.push(Command::Spawn {
                        uid: Some(uid),
                        components: vec![("Name".into(), Json::obj([("value", Json::str(name.clone()))])), ("Transform".into(), Json::obj([("pos", Json::arr([round(origin[0]), round(origin[1]), round(z)]))])), ("Tilemap".into(), tilemap)],
                    });
                    // a solid layer: merged boxes, in the layer's plane
                    let lower = name.to_lowercase();
                    let solid = prop(layer, "solid").and_then(|v| v.as_bool()).unwrap_or(lower.starts_with("solid") || lower.starts_with("collision"));
                    let mut boxes = 0usize;
                    if solid {
                        let blocks = merge(w, h, |x, y| gids[y * w + x] != 0);
                        boxes = blocks.len();
                        for b in &blocks {
                            let (cols, rws) = ((b.x1 - b.x0 + 1) as f64, (b.y1 - b.y0 + 1) as f64);
                            let (cx, cy) = (origin[0] + (b.x0 as f64 + cols / 2.0 - w as f64 / 2.0) * tile, origin[1] + (h as f64 / 2.0 - b.y0 as f64 - rws / 2.0) * tile);
                            cmds.push(Command::Spawn {
                                uid: None,
                                components: vec![
                                    ("Name".into(), Json::obj([("value", Json::str(format!("{name} collider")))])),
                                    ("Transform".into(), Json::obj([("pos", Json::arr([round(cx), round(cy), round(origin[2])]))])),
                                    ("Collider".into(), Json::obj([("shape", Json::from(1i64)), ("size", Json::arr([round(cols * tile), round(rws * tile), round(tile)]))])),
                                ],
                            });
                        }
                    }
                    layers_out.push(Json::obj([("uid", uid_json(uid)), ("name", Json::str(name)), ("tileset", Json::str(ts.image.clone())), ("tiles", Json::from(gids.iter().filter(|g| **g != 0).count())), ("solid_boxes", Json::from(boxes))]));
                }
                Some("objectgroup") => {
                    for o in layer.get("objects").and_then(|o| o.as_array()).unwrap_or(&[]) {
                        let (x, y, ow, oh) = (num(o, "x", 0.0), num(o, "y", 0.0), num(o, "width", 0.0), num(o, "height", 0.0));
                        let point = o.get("point").and_then(|p| p.as_bool()).unwrap_or(false);
                        // a rectangle is placed by its top left corner; a point is where it is
                        let (cx, cy) = if point { (world_x(x), world_y(y)) } else { (world_x(x + ow / 2.0), world_y(y + oh / 2.0)) };
                        let mut props: Vec<(String, Json)> = Vec::new();
                        for p in o.get("properties").and_then(|p| p.as_array()).unwrap_or(&[]) {
                            if let (Some(n), Some(v)) = (p.get("name").and_then(|n| n.as_str()), p.get("value")) {
                                props.push((n.to_string(), v.clone()));
                            }
                        }
                        let kind = o.get("type").or_else(|| o.get("class")).and_then(|t| t.as_str()).unwrap_or("");
                        objects_out.push(Json::obj([
                            ("layer", Json::str(name.clone())),
                            ("name", Json::str(o.get("name").and_then(|n| n.as_str()).unwrap_or(""))),
                            ("type", Json::str(kind)),
                            ("pos", Json::arr([round(cx), round(cy)])),
                            ("size", Json::arr([round(ow / tw * tile), round(oh / th * tile)])),
                            ("point", Json::Bool(point)),
                            ("properties", Json::Obj(props)),
                        ]));
                    }
                }
                other => warnings.push(format!("layer `{name}` is of kind `{}` and was skipped", other.unwrap_or("?"))),
            }
        }
        if cmds.is_empty() && objects_out.is_empty() {
            return Err(EcsError::new("map_empty", "the map has no tile layers with tiles and no objects").hint("paint a tile layer, or add objects"));
        }
        let made = cmds.len();
        if !cmds.is_empty() {
            let (_, inverse) = self.sim.apply_batch(&cmds)?;
            self.push_history(inverse);
        }
        Ok(Json::obj([
            ("map", Json::str(rel)),
            ("size", Json::arr([Json::from(w), Json::from(h)])),
            ("entities", Json::from(made)),
            ("layers", Json::Arr(layers_out)),
            ("objects", Json::Arr(objects_out)),
            ("warnings", Json::arr(warnings.into_iter().map(Json::str))),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use lunaris_json::{parse, Json};

    fn j(s: &str) -> Json {
        parse(s).unwrap()
    }

    const MAP: &str = r#"{
      "width": 4, "height": 3, "tilewidth": 16, "tileheight": 16, "infinite": false,
      "tilesets": [{"firstgid": 1, "image": "../art/tiles.png", "columns": 4, "tilecount": 8, "tilewidth": 16, "tileheight": 16}],
      "layers": [
        {"type": "tilelayer", "name": "ground", "width": 4, "height": 3, "data": [1,2,2,3, 5,0,0,6, 5,5,5,5]},
        {"type": "tilelayer", "name": "solid walls", "width": 4, "height": 3, "properties": [{"name": "solid", "type": "bool", "value": true}], "data": [0,0,0,0, 7,0,0,7, 7,7,7,7]},
        {"type": "tilelayer", "name": "empty", "width": 4, "height": 3, "data": [0,0,0,0, 0,0,0,0, 0,0,0,0]},
        {"type": "objectgroup", "name": "things", "objects": [
          {"name": "start", "type": "spawn", "x": 24, "y": 24, "width": 0, "height": 0, "point": true, "properties": [{"name": "team", "type": "string", "value": "blue"}]},
          {"name": "door", "type": "trigger", "x": 32, "y": 16, "width": 16, "height": 32, "properties": []}
        ]},
        {"type": "imagelayer", "name": "sky"}
      ]}"#;

    #[test]
    fn a_map_becomes_tilemaps_colliders_and_a_list_of_objects() {
        let mut e = crate::tests::engine();
        std::fs::create_dir_all(e.root.join("maps")).unwrap();
        std::fs::write(e.root.join("maps/level.tmj"), MAP).unwrap();
        let r = e.call("import_tiled", &j(r#"{"path":"maps/level.tmj","tile":2,"pos":[10,0,0]}"#)).unwrap();
        let layers = r.get("layers").unwrap().as_array().unwrap();
        assert_eq!(layers.len(), 2, "the empty layer and the picture layer are skipped: {}", r.to_string_lossy());
        assert_eq!(layers[0].get("tileset").unwrap().as_str(), Some("art/tiles.png"), "paths are made relative to the workspace");
        assert_eq!(layers[1].get("solid_boxes").unwrap().as_u64(), Some(3), "two single cells on the middle row and a slab of four under them");
        assert_eq!(layers[0].get("solid_boxes").unwrap().as_u64(), Some(0), "the ground is not solid");
        // the ground layer: tile numbers are the gid minus the first gid, empty cells -1, the sheet is 4 columns by 2 rows
        let first = layers[0].get("uid").unwrap().as_u64().unwrap();
        let g = e.call("get", &j(&format!(r#"{{"uid":{first},"components":["Tilemap","Transform"]}}"#))).unwrap();
        let t = g.get("components").unwrap().get("Tilemap").unwrap();
        assert_eq!(t.get("data").unwrap().as_str(), Some("0 1 1 2\n4 -1 -1 5\n4 4 4 4"));
        assert_eq!(t.get("sheet").unwrap().to_string_lossy(), "[4,2]");
        assert_eq!(t.get("size").unwrap().to_string_lossy(), "[4,3]");
        assert_eq!(t.get("tile").unwrap().as_f64(), Some(2.0));
        assert_eq!(g.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap()[0].as_f64(), Some(10.0));
        // objects in world coordinates: the point at pixel (24, 24) is cell (1.5, 1.5); the map is 4 x 3 tiles of 2 units centred on x = 10
        let objs = r.get("objects").unwrap().as_array().unwrap();
        let start = &objs[0];
        let pos: Vec<f64> = start.get("pos").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(pos, vec![10.0 + (1.5 - 2.0) * 2.0, (1.5 - 1.5) * 2.0]);
        assert_eq!(start.get("properties").unwrap().get("team").unwrap().as_str(), Some("blue"));
        assert_eq!(start.get("type").unwrap().as_str(), Some("spawn"));
        // the door is a 1 x 2 tile rectangle at cell (2, 1): its centre is at (2.5, 2.0) cells from the corner
        let door = &objs[1];
        assert_eq!(door.get("size").unwrap().to_string_lossy(), "[2,4]");
        assert_eq!(door.get("pos").unwrap().as_array().unwrap()[0].as_f64(), Some(10.0 + (2.5 - 2.0) * 2.0));
        // warnings say what was left out; the whole import is one undo step
        let w = r.get("warnings").unwrap().to_string_lossy();
        assert!(w.contains("empty") && w.contains("imagelayer"), "{w}");
        let n = r.get("entities").unwrap().as_u64().unwrap();
        assert!(n >= 4);
        e.call("undo", &j("{}")).unwrap();
        assert_eq!(e.call("query", &j(r#"{"with":["Tilemap"],"count_only":true}"#)).unwrap().get("total").unwrap().as_u64(), Some(0));
    }

    #[test]
    fn what_cannot_be_imported_is_said() {
        let mut e = crate::tests::engine();
        std::fs::write(e.root.join("bad.json"), "{ not json").unwrap();
        std::fs::write(e.root.join("inf.json"), r#"{"infinite": true, "width": 1, "height": 1}"#).unwrap();
        std::fs::write(e.root.join("none.json"), r#"{"width": 2, "height": 2, "tilewidth": 8, "tileheight": 8, "tilesets": [], "layers": []}"#).unwrap();
        assert_eq!(e.call("import_tiled", &j(r#"{"path":"missing.tmj"}"#)).unwrap_err().code, "map_unreadable");
        assert_eq!(e.call("import_tiled", &j(r#"{"path":"bad.json"}"#)).unwrap_err().code, "map_bad_json");
        assert_eq!(e.call("import_tiled", &j(r#"{"path":"inf.json"}"#)).unwrap_err().code, "map_infinite");
        assert_eq!(e.call("import_tiled", &j(r#"{"path":"none.json"}"#)).unwrap_err().code, "map_empty");
        assert_eq!(e.call("import_tiled", &j(r#"{"path":"../x.json"}"#)).unwrap_err().code, "bad_path");
        assert_eq!(e.call("import_tiled", &j(r#"{"path":"none.json","tile":-1}"#)).unwrap_err().code, "bad_value");
    }
}
