//! `tilemap_colliders`: solid ground for a `Tilemap`. The cells whose tile number is in `solid` are merged into as few boxes as possible (runs along a row, then
//! equal runs on rows beneath each other joined) and one static `Collider` box is spawned for each, so heroes, balls and crates stand on the level that is drawn, and
//! navigation and `raycast` see it. Run it again after the map changes: the boxes it made before are removed first. Undoable as one step.
//!
//! The boxes are plain entities named `tile collider <uid>`, beside the tilemap (not children), in the plane of the tilemap entity: the map's own rotation and
//! scale are not used.
use crate::engine::{uid_json, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_sim::components::{Name, Tilemap, Transform};
use lunaris_sim::Command;
use std::collections::BTreeMap;

/// A box of cells: columns `x0..=x1`, rows `y0..=y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub x0: usize,
    pub x1: usize,
    pub y0: usize,
    pub y1: usize,
}

/// The fewest-ish rectangles that cover exactly the cells for which `solid` is true: each row's runs are extended downward while the next row has the very same
/// run. Boxes come out in order of the row they end on, then by column, so the result is the same every time.
pub(crate) fn merge(w: usize, h: usize, solid: impl Fn(usize, usize) -> bool) -> Vec<Block> {
    let mut out = Vec::new();
    // the runs that continue from the row above: (x0, x1) -> the row they started on
    let mut open: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for y in 0..=h {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        if y < h {
            let mut x = 0;
            while x < w {
                if solid(x, y) {
                    let x0 = x;
                    while x < w && solid(x, y) {
                        x += 1;
                    }
                    runs.push((x0, x - 1));
                } else {
                    x += 1;
                }
            }
        }
        let ended: Vec<((usize, usize), usize)> = open.iter().filter(|(k, _)| !runs.contains(k)).map(|(k, v)| (*k, *v)).collect();
        for (k, start) in ended {
            open.remove(&k);
            out.push(Block { x0: k.0, x1: k.1, y0: start, y1: y - 1 });
        }
        for r in runs {
            open.entry(r).or_insert(y);
        }
    }
    out
}

/// The tile numbers that count as solid: a whole number, or a pair `[from, to]` for a range (both ends included).
fn solid_set(a: &Args) -> Result<Vec<(i64, i64)>> {
    let bad = || EcsError::new("type_mismatch", "`solid` must be a list of tile numbers or [from, to] pairs").at("solid");
    match a.0.get("solid") {
        None | Some(Json::Null) => Ok(vec![(1, 1)]),
        Some(Json::Arr(v)) => v
            .iter()
            .map(|x| match x {
                Json::Arr(p) if p.len() == 2 => Ok((p[0].as_i64().ok_or_else(bad)?, p[1].as_i64().ok_or_else(bad)?)),
                other => other.as_i64().map(|n| (n, n)).ok_or_else(bad),
            })
            .collect(),
        Some(_) => Err(bad()),
    }
}

impl Engine {
    pub(crate) fn tilemap_colliders_tool(&mut self, a: &Args) -> Result<Json> {
        let uid = a.u64("uid")?;
        let ranges = solid_set(a)?;
        let e = self.sim.world.entity(uid).ok_or_else(|| EcsError::new("no_such_entity", format!("no entity {uid}")).at("uid"))?;
        let map = *self.sim.world.get::<Tilemap>(e).ok_or_else(|| EcsError::new("no_tilemap", format!("entity {uid} has no Tilemap")).at("uid"))?;
        let pos = self.sim.world.get::<Transform>(e).map(|t| t.pos).unwrap_or([0.0; 3]);
        let (w, h) = (map.size[0].min(1024) as usize, map.size[1].min(1024) as usize);
        if w == 0 || h == 0 || w * h > 262_144 || !map.tile.is_finite() || map.tile <= 0.0 {
            return Err(EcsError::new("bad_size", "the map is empty, bigger than 262144 cells, or its `tile` is not a positive number").at("uid"));
        }
        let depth = match a.0.get("depth") {
            None | Some(Json::Null) => map.tile as f64,
            Some(v) => v.as_f64().filter(|d| d.is_finite() && *d > 0.0).ok_or_else(|| EcsError::new("bad_value", "`depth` must be a positive number").at("depth"))?,
        };
        let extra: Vec<(String, Json)> = match a.0.get("components") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Obj(o)) => o.clone(),
            Some(_) => return Err(EcsError::new("type_mismatch", "`components` must be an object of extra components for every box, like {\"Ground\":{}}").at("components")),
        };
        let clear = !matches!(a.0.get("clear"), Some(Json::Bool(false)));
        let text = self.sim.world.strings().get(map.data.0).to_string();
        let cells: Vec<i64> = text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).map(|t| t.parse::<i64>().unwrap_or(-1)).chain(std::iter::repeat(-1)).take(w * h).collect();
        let is_solid = |x: usize, y: usize| {
            let n = cells[y * w + x];
            ranges.iter().any(|(lo, hi)| n >= *lo && n <= *hi)
        };
        let solid_cells = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|(x, y)| is_solid(*x, *y)).count();
        let blocks = merge(w, h, is_solid);

        let name = format!("tile collider {uid}");
        let mut cmds: Vec<Command> = Vec::new();
        let mut removed = 0usize;
        if clear {
            let mut named: Vec<(u64, u32)> = Vec::new();
            self.sim.world.for_each_uid::<(&Name,), _>(|_, u, (n,)| named.push((u, n.value.0)))?;
            let strings = self.sim.world.strings();
            let mut old: Vec<u64> = named.into_iter().filter(|(_, sym)| strings.get(*sym) == name).map(|(u, _)| u).collect();
            old.sort_unstable();
            removed = old.len();
            cmds.extend(old.into_iter().map(|u| Command::Despawn { uid: u }));
        }
        let round = |v: f64| Json::Float((v * 10_000.0).round() / 10_000.0);
        let tile = map.tile as f64;
        for b in &blocks {
            let (cols, rows) = ((b.x1 - b.x0 + 1) as f64, (b.y1 - b.y0 + 1) as f64);
            let cx = pos[0] as f64 + (b.x0 as f64 + cols / 2.0 - w as f64 / 2.0) * tile;
            let cy = pos[1] as f64 + (h as f64 / 2.0 - b.y0 as f64 - rows / 2.0) * tile;
            let mut parts: Vec<(String, Json)> = vec![
                ("Name".to_string(), Json::obj([("value", Json::str(name.clone()))])),
                ("Transform".to_string(), Json::obj([("pos", Json::arr([round(cx), round(cy), round(pos[2] as f64)]))])),
                ("Collider".to_string(), Json::obj([("shape", Json::from(1i64)), ("size", Json::arr([round(cols * tile), round(rows * tile), round(depth)]))])),
            ];
            for (k, v) in &extra {
                match parts.iter_mut().find(|(n, _)| n == k) {
                    Some(slot) => slot.1 = v.clone(),
                    None => parts.push((k.clone(), v.clone())),
                }
            }
            cmds.push(Command::Spawn { uid: None, components: parts });
        }
        let mut out = Json::obj([("tilemap", uid_json(uid)), ("solid_cells", Json::from(solid_cells)), ("boxes", Json::from(blocks.len())), ("removed", Json::from(removed))]);
        if cmds.is_empty() {
            out.set("undoable", Json::Bool(false));
            return Ok(out);
        }
        let (done, inverse) = self.sim.apply_batch(&cmds)?;
        let undoable = self.push_history(inverse);
        let made: Vec<u64> = done.iter().filter(|d| matches!(d.executed, Command::Spawn { .. })).filter_map(|d| d.uid).collect();
        if let (Some(first), Some(last)) = (made.first(), made.last()) {
            out.set("first_uid", uid_json(*first));
            out.set("last_uid", uid_json(*last));
        }
        out.set("undoable", Json::Bool(undoable));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> (usize, usize, Vec<bool>) {
        let h = rows.len();
        let w = rows[0].len();
        (w, h, rows.iter().flat_map(|r| r.chars().map(|c| c == '#')).collect())
    }

    #[test]
    fn solid_cells_merge_into_boxes_that_cover_exactly_those_cells() {
        let (w, h, g) = grid(&["##...", "#....", "#####", "..#..", "..#.#"]);
        let blocks = merge(w, h, |x, y| g[y * w + x]);
        // every solid cell is in exactly one box, and no box holds an empty cell
        let mut covered = vec![0u8; w * h];
        for b in &blocks {
            for y in b.y0..=b.y1 {
                for x in b.x0..=b.x1 {
                    assert!(g[y * w + x], "{b:?} covers an empty cell");
                    covered[y * w + x] += 1;
                }
            }
        }
        for i in 0..w * h {
            assert_eq!(covered[i], g[i] as u8, "cell {i}");
        }
        assert!(blocks.len() <= 5, "{blocks:?}");
        // a solid slab is one box, and nothing is none
        let (w, h, all) = (12, 7, vec![true; 84]);
        assert_eq!(merge(w, h, |x, y| all[y * w + x]), vec![Block { x0: 0, x1: 11, y0: 0, y1: 6 }]);
        assert!(merge(4, 4, |_, _| false).is_empty());
    }

    fn j(s: &str) -> Json {
        lunaris_json::parse(s).unwrap()
    }

    fn y_of(e: &mut crate::Engine, uid: u64) -> f64 {
        let r = e.call("get", &j(&format!(r#"{{"uid":{uid},"components":["Transform"]}}"#))).unwrap();
        r.get("components").unwrap().get("Transform").unwrap().get("pos").unwrap().as_array().unwrap()[1].as_f64().unwrap()
    }

    fn count(e: &mut crate::Engine, with: &str) -> u64 {
        e.call("query", &j(&format!(r#"{{"with":{with},"count_only":true}}"#))).unwrap().get("total").unwrap().as_u64().unwrap()
    }

    #[test]
    fn a_ball_stands_on_the_boxes_made_from_the_map_and_they_are_made_again_in_place() {
        let mut e = crate::tests::engine();
        // 6 x 4 tiles of 2 units centred on (10, 5): the map's top is y 9; the bottom two rows are ground (1), with a hole in the top one
        let data = "0 0 0 0 0 0\\n0 0 0 0 0 0\\n1 1 -1 1 1 1\\n1 1 1 1 1 1";
        e.call("spawn", &j(&format!(r#"{{"uid":1,"components":{{"Transform":{{"pos":[10,5,0]}},"Tilemap":{{"size":[6,4],"tile":2.0,"data":"{data}"}}}}}}"#))).unwrap();
        // an extra component that does not exist stops the whole call: nothing is made
        assert!(e.call("tilemap_colliders", &j(r#"{"uid":1,"components":{"Ground":{}}}"#)).is_err());
        assert_eq!(count(&mut e, r#"["Collider"]"#), 0);
        e.call("define_component", &j(r#"{"name":"Ground","fields":[{"name":"on","type":"f32","default":0}]}"#)).unwrap();
        let r = e.call("tilemap_colliders", &j(r#"{"uid":1,"components":{"Ground":{}}}"#)).unwrap();
        assert_eq!(r.get("solid_cells").unwrap().as_u64(), Some(11));
        // the top row's two runs [0,1] and [3,5] end where the bottom row's run of six begins: two blocks and a slab
        assert_eq!(r.get("boxes").unwrap().as_u64(), Some(3), "{}", r.to_string_lossy());
        assert_eq!(r.get("removed").unwrap().as_u64(), Some(0));
        assert_eq!(count(&mut e, r#"["Collider","Ground"]"#), 3);
        // the left block: columns 0 and 1 of row 2: 4 x 2 units, centred at x = 10 + (0 + 1 - 3) * 2 = 6 and y = 5 + (2 - 2 - 0.5) * 2 = 4
        let first = r.get("first_uid").unwrap().as_u64().unwrap();
        let block = e.call("get", &j(&format!(r#"{{"uid":{first},"components":["Transform","Collider"]}}"#))).unwrap();
        let c = block.get("components").unwrap();
        let p: Vec<f64> = c.get("Transform").unwrap().get("pos").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let s: Vec<f64> = c.get("Collider").unwrap().get("size").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(s, vec![4.0, 2.0, 2.0]);
        assert_eq!((p[0], p[1]), (6.0, 4.0));
        // a ball dropped over the block lands on its top (y = 5); one dropped over the hole falls to the slab beneath (top y = 3)
        for (uid, x) in [(50, 7), (51, 9)] {
            e.call("spawn", &j(&format!(r#"{{"uid":{uid},"components":{{"Transform":{{"pos":[{x},9,0]}},"Velocity":{{}},"RigidBody":{{}},"Collider":{{"shape":0,"size":[1,1,1]}}}}}}"#))).unwrap();
        }
        e.call("step", &j(r#"{"ticks":240}"#)).unwrap();
        let (on, in_hole) = (y_of(&mut e, 50), y_of(&mut e, 51));
        assert!((on - 5.5).abs() < 0.1, "the ball rests on the block: y {on}");
        assert!((in_hole - 3.5).abs() < 0.1, "the ball in the hole rests on the slab: y {in_hole}");
        // again: the old boxes go and new ones come; the whole thing undoes in one step
        let again = e.call("tilemap_colliders", &j(r#"{"uid":1,"components":{"Ground":{}}}"#)).unwrap();
        assert_eq!((again.get("removed").unwrap().as_u64(), again.get("boxes").unwrap().as_u64()), (Some(3), Some(3)));
        assert_eq!(count(&mut e, r#"["Collider","Ground"]"#), 3);
        e.call("undo", &j("{}")).unwrap();
        assert_eq!(count(&mut e, r#"["Ground"]"#), 3, "undo brings back the old three");
        // errors, and ranges of tiles
        assert_eq!(e.call("tilemap_colliders", &j(r#"{"uid":99}"#)).unwrap_err().code, "no_such_entity");
        assert_eq!(e.call("tilemap_colliders", &j(r#"{"uid":50}"#)).unwrap_err().code, "no_tilemap");
        assert_eq!(e.call("tilemap_colliders", &j(r#"{"uid":1,"solid":"x"}"#)).unwrap_err().code, "type_mismatch");
        let none = e.call("tilemap_colliders", &j(r#"{"uid":1,"solid":[[5,9]]}"#)).unwrap();
        assert_eq!((none.get("boxes").unwrap().as_u64(), none.get("removed").unwrap().as_u64()), (Some(0), Some(3)));
    }
}
