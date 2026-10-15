//! `autotile`: turn a map of "ground here, nothing there" into the right edge and corner tiles. A `Tilemap` whose `data` holds one number for every kind of
//! cell is rewritten so that each solid cell becomes the tile that fits its neighbours: `base` plus a number that says which of the neighbours are solid.
//!
//! * `mode` `"4"` (the default): 16 tiles, `base + N + 2 E + 4 S + 8 W` where each letter is 1 when the neighbour above, right, below or left is solid.
//! * `mode` `"blob"`: 47 tiles, the standard "blob" set: the eight neighbours, where a corner counts only when both edges beside it are solid; the 47 possible
//!   results are numbered in increasing order of `N + 2 NE + 4 E + 8 SE + 16 S + 32 SW + 64 W + 128 NW` (0 an island, 46 a cell surrounded on all sides).
use crate::engine::{Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_sim::components::Tilemap;
use lunaris_sim::Command;

/// The mask of the eight neighbours with the corners that have a missing side removed, in the blob numbering above.
fn reduce(mask: u32) -> u32 {
    let bit = |b: u32| mask >> b & 1;
    let (n, ne, e, se, s, sw, w, nw) = (bit(0), bit(1), bit(2), bit(3), bit(4), bit(5), bit(6), bit(7));
    n | (ne & n & e) << 1 | e << 2 | (se & e & s) << 3 | s << 4 | (sw & s & w) << 5 | w << 6 | (nw & w & n) << 7
}

/// The 47 masks a cell can end up with, in increasing order.
fn blob_masks() -> Vec<u32> {
    let mut v: Vec<u32> = (0..256).map(reduce).collect();
    v.sort_unstable();
    v.dedup();
    v
}

impl Engine {
    pub(crate) fn autotile_tool(&mut self, a: &Args) -> Result<Json> {
        let uid = a.u64("uid")?;
        let base = a.opt_u64("base")?.unwrap_or(0) as i64;
        let blob = match a.opt_str("mode")?.unwrap_or("4") {
            "4" => false,
            "blob" => true,
            other => return Err(EcsError::new("bad_mode", format!("unknown mode `{other}`")).hint("modes: \"4\" (16 tiles) and \"blob\" (47 tiles)").at("mode")),
        };
        let solid: Vec<i64> = match a.0.get("solid") {
            None | Some(Json::Null) => vec![1],
            Some(Json::Arr(v)) => v.iter().map(|x| x.as_i64().ok_or_else(|| EcsError::new("type_mismatch", "`solid` must be a list of whole numbers").at("solid"))).collect::<Result<_>>()?,
            Some(_) => return Err(EcsError::new("type_mismatch", "`solid` must be a list of tile numbers").at("solid")),
        };
        let edge_solid = !matches!(a.0.get("edge"), Some(Json::Str(s)) if s == "empty");
        let e = self.sim.world.entity(uid).ok_or_else(|| EcsError::new("no_such_entity", format!("no entity {uid}")).at("uid"))?;
        let map = *self.sim.world.get::<Tilemap>(e).ok_or_else(|| EcsError::new("no_tilemap", format!("entity {uid} has no Tilemap")).hint("add a Tilemap whose `data` marks the ground with a number (default 1)").at("uid"))?;
        let (w, h) = (map.size[0].min(1024) as usize, map.size[1].min(1024) as usize);
        if w == 0 || h == 0 || w * h > 262_144 {
            return Err(EcsError::new("bad_size", "the map is empty or bigger than 262144 cells").at("uid"));
        }
        let text = self.sim.world.strings().get(map.data.0).to_string();
        let cells: Vec<i64> = text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).map(|t| t.parse::<i64>().unwrap_or(-1)).chain(std::iter::repeat(-1)).take(w * h).collect();
        let is_solid = |x: i64, y: i64| -> bool {
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                return edge_solid;
            }
            solid.contains(&cells[y as usize * w + x as usize])
        };
        let ranks = blob_masks();
        let mut out: Vec<i64> = cells.clone();
        let mut changed = 0usize;
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                if !is_solid(x, y) {
                    continue;
                }
                // neighbours clockwise from the north: N NE E SE S SW W NW
                let around = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];
                let mask: u32 = around.iter().enumerate().map(|(b, (dx, dy))| (is_solid(x + dx, y + dy) as u32) << b).sum();
                let index = if blob {
                    let r = reduce(mask);
                    ranks.iter().position(|m| *m == r).unwrap_or(0) as i64
                } else {
                    ((mask & 1) | (mask >> 2 & 1) << 1 | (mask >> 4 & 1) << 2 | (mask >> 6 & 1) << 3) as i64
                };
                out[y as usize * w + x as usize] = base + index;
                changed += 1;
            }
        }
        let rows: Vec<String> = out.chunks(w).map(|r| r.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")).collect();
        let data = rows.join("\n");
        self.single(Command::Set { uid, component: "Tilemap".into(), value: Json::obj([("data", Json::str(data))]) })?;
        Ok(Json::obj([("tiles_set", Json::from(changed)), ("mode", Json::str(if blob { "blob" } else { "4" })), ("tiles_used", Json::from(if blob { 47 } else { 16 })), ("size", Json::arr([Json::from(w), Json::from(h)]))]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blob_set_has_47_masks_and_corners_need_both_edges() {
        let m = blob_masks();
        assert_eq!(m.len(), 47);
        assert_eq!((m[0], m[46]), (0, 255));
        // a lone corner without its edges is nothing; with both edges it counts
        assert_eq!(reduce(0b0000_0010), 0);
        assert_eq!(reduce(0b0000_0111), 0b0000_0111);
        assert!(m.windows(2).all(|w| w[0] < w[1]));
    }
}
