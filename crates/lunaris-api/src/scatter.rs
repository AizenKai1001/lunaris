//! `scatter`: put many copies of something on a terrain (trees, rocks, grass tufts) in one undoable step, on ground that suits them: not too
//! steep, between two heights, not too close to each other. Everything is decided from the `seed`, so the same call on the same ground gives
//! the same forest; what is written into the world (and into a recording) is ordinary spawn commands with their final positions.
use crate::engine::{uid_json, Args, Engine, MAX_SPAWN_COUNT};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_sim::components::{Terrain, Transform};
use lunaris_sim::terrain::Placed;
use lunaris_sim::Command;
use std::collections::BTreeMap;

/// SplitMix64: a small generator of its own, so that placing things never touches the simulation's random numbers.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0 up to (not including) 1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / 16_777_216.0
    }

    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

fn pair(a: &Args, key: &str, default: [f32; 2]) -> Result<[f32; 2]> {
    match a.0.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => {
            let bad = || EcsError::new("type_mismatch", format!("`{key}` must be [min, max] (two numbers)")).at(key);
            let list = v.as_array().filter(|l| l.len() == 2).ok_or_else(bad)?;
            let (lo, hi) = (list[0].as_f64().ok_or_else(bad)?, list[1].as_f64().ok_or_else(bad)?);
            if !lo.is_finite() || !hi.is_finite() || lo > hi {
                return Err(EcsError::new("out_of_range", format!("`{key}` must be two finite numbers with min <= max")).at(key));
            }
            Ok([lo as f32, hi as f32])
        }
    }
}

fn flag(a: &Args, key: &str, default: bool) -> Result<bool> {
    match a.0.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be true or false")).at(key)),
    }
}

fn number(a: &Args, key: &str, default: f32) -> Result<f32> {
    match a.0.get(key) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(|x| x as f32).ok_or_else(|| EcsError::new("type_mismatch", format!("`{key}` must be a number")).at(key)),
    }
}

/// The quaternion (x, y, z, w) that turns up (0, 1, 0) into `n`, then spins by `yaw` about it.
fn orientation(n: [f32; 3], yaw: f32, align: bool) -> [f32; 4] {
    let (s, c) = ((yaw * 0.5).sin(), (yaw * 0.5).cos());
    let spin = [0.0, s, 0.0, c];
    if !align || n[1] > 0.9999 {
        return spin;
    }
    // shortest rotation from up to n: axis = up x n, w = 1 + up . n
    let (ax, az, w) = (n[2], -n[0], 1.0 + n[1]);
    let len = (ax * ax + az * az + w * w).sqrt();
    let q = [ax / len, 0.0, az / len, w / len];
    // q * spin (spin first, then the tilt)
    [q[3] * spin[0] + q[0] * spin[3] + q[1] * spin[2] - q[2] * spin[1], q[3] * spin[1] + q[1] * spin[3] + q[2] * spin[0] - q[0] * spin[2], q[3] * spin[2] + q[2] * spin[3] + q[0] * spin[1] - q[1] * spin[0], q[3] * spin[3] - q[0] * spin[0] - q[1] * spin[1] - q[2] * spin[2]]
}

impl Engine {
    pub(crate) fn scatter_tool(&mut self, a: &Args) -> Result<Json> {
        // what to place
        let prefab = a.opt_str("prefab")?;
        let components = match a.0.get("components") {
            None | Some(Json::Null) => None,
            Some(Json::Obj(p)) => Some(p.clone()),
            Some(_) => return Err(EcsError::new("type_mismatch", "`components` must be an object keyed by component name").at("components")),
        };
        if prefab.is_some() == components.is_some() {
            return Err(EcsError::new("bad_arguments", "give either `prefab` (the name of a prefab) or `components` (what each copy has), not both and not neither").hint("e.g. {\"prefab\": \"Pine\", \"terrain\": 3, \"count\": 200}"));
        }
        let count = a.u64("count")?;
        if count == 0 || count > MAX_SPAWN_COUNT.min(20_000) {
            return Err(EcsError::new("out_of_range", "`count` must be between 1 and 20000").at("count"));
        }
        let seed = a.opt_u64("seed")?.unwrap_or(1);
        let slope = pair(a, "slope", [0.0, 35.0])?;
        let height = pair(a, "height", [f32::NEG_INFINITY, f32::INFINITY])?;
        let scale = pair(a, "scale", [1.0, 1.0])?;
        let min_distance = number(a, "min_distance", 0.0)?.max(0.0);
        let sink = number(a, "sink", 0.0)?;
        let yaw = flag(a, "yaw", true)?;
        let align = flag(a, "align", false)?;
        let per_copy = a.opt_u64("attempts")?.unwrap_or(30).clamp(1, 1000);
        let layer_min = number(a, "layer_min", 0.5)?;
        if !(0.0..=1.0).contains(&layer_min) {
            return Err(EcsError::new("out_of_range", "`layer_min` must be between 0 and 1").at("layer_min"));
        }

        // where: a terrain of the world
        self.sim.load_terrains();
        let mut terrains: Vec<(u64, Transform, Terrain)> = Vec::new();
        self.sim.world.for_each_uid::<(&Transform, &Terrain), _>(|_, uid, (t, g)| terrains.push((uid, *t, *g)))?;
        terrains.sort_by_key(|t| t.0);
        let wanted = a.opt_u64("terrain")?;
        let (uid, t, g) = match wanted {
            Some(u) => terrains.iter().find(|t| t.0 == u).copied().ok_or_else(|| EcsError::new("no_terrain", format!("entity {u} has no Terrain (and a Transform)")).at("terrain"))?,
            None => terrains.first().copied().ok_or_else(|| EcsError::new("no_terrain", "there is no terrain in the world").hint("spawn an entity with Transform and Terrain first (docs/TERRAIN.md)"))?,
        };
        let name = self.sim.world.strings().get(g.heightmap.0).to_string();
        let field = self.sim.heightfields.get(&name).cloned().ok_or_else(|| {
            let why = self.sim.terrain_errors.get(&name).cloned().unwrap_or_else(|| "it has not been read".to_string());
            EcsError::new("terrain_unreadable", format!("the heightmap `{name}` of entity {uid} could not be used: {why}")).hint("see `check`; `reload_terrain` reads it again")
        })?;
        let placed = Placed::new(&field, t.pos, &g);
        // only where one layer of a painted terrain weighs enough: trees on the grass, rocks on the rock
        let layer_filter = match a.0.get("layer").filter(|v| !matches!(v, Json::Null)) {
            None => None,
            Some(which) => {
                let lookup = self.layer_lookup(uid)?;
                let k = lookup.find(which)?;
                Some((lookup, k))
            }
        };
        let half = [placed.size[0] * 0.5, placed.size[1] * 0.5];
        let mut area = [placed.centre[0] - half[0], placed.centre[1] - half[1], placed.centre[0] + half[0], placed.centre[1] + half[1]];
        if let Some(v) = a.0.get("area").filter(|v| !matches!(v, Json::Null)) {
            let bad = || EcsError::new("type_mismatch", "`area` must be [x_min, z_min, x_max, z_max] (four numbers)").at("area");
            let list = v.as_array().filter(|l| l.len() == 4).ok_or_else(bad)?;
            let mut r = [0.0f32; 4];
            for (slot, item) in r.iter_mut().zip(list) {
                *slot = item.as_f64().filter(|x| x.is_finite()).ok_or_else(bad)? as f32;
            }
            area = [area[0].max(r[0].min(r[2])), area[1].max(r[1].min(r[3])), area[2].min(r[0].max(r[2])), area[3].min(r[1].max(r[3]))];
            if area[0] >= area[2] || area[1] >= area[3] {
                return Err(EcsError::new("out_of_range", "`area` does not overlap the terrain").at("area"));
            }
        }

        // the copies: positions chosen from the seed
        let min_up = slope[1].to_radians().cos();
        let max_up = slope[0].to_radians().cos();
        let mut rng = Rng(seed);
        let cell = if min_distance > 0.0 { min_distance } else { 1.0 };
        let mut taken: BTreeMap<(i64, i64), Vec<[f32; 2]>> = BTreeMap::new();
        let mut spots: Vec<([f32; 3], [f32; 4], f32)> = Vec::new();
        let mut attempts = 0u64;
        let budget = count * per_copy;
        while (spots.len() as u64) < count && attempts < budget {
            attempts += 1;
            let (x, z) = (rng.between(area[0], area[2]), rng.between(area[1], area[3]));
            let (spin, s) = (rng.unit(), rng.between(scale[0], scale[1]));
            let Some(h) = placed.surface(x, z) else { continue };
            if let Some((lookup, k)) = &layer_filter {
                if !lookup.weights(x, z).is_some_and(|w| w[*k] >= layer_min - 1.0e-6) {
                    continue;
                }
            }
            let n = placed.normal(x, z);
            if h < height[0] || h > height[1] || n[1] > max_up + 1.0e-6 || n[1] < min_up - 1.0e-6 {
                continue;
            }
            if min_distance > 0.0 {
                let (cx, cz) = ((x / cell).floor() as i64, (z / cell).floor() as i64);
                let near = (cz - 1..=cz + 1).any(|j| (cx - 1..=cx + 1).any(|i| taken.get(&(i, j)).is_some_and(|l| l.iter().any(|p| (p[0] - x).powi(2) + (p[1] - z).powi(2) < min_distance * min_distance))));
                if near {
                    continue;
                }
                taken.entry((cx, cz)).or_default().push([x, z]);
            }
            let rot = orientation(n, if yaw { spin * std::f32::consts::TAU } else { 0.0 }, align);
            spots.push(([x, h - sink, z], rot, s));
        }
        if spots.is_empty() {
            let on_layer = layer_filter.as_ref().map_or(String::new(), |(l, k)| format!(", on the layer `{}` (weight {layer_min} or more)", l.specs[*k].name));
            return Err(EcsError::new("no_room", format!("none of {attempts} tries found ground that suits: slope {}..{} degrees, height {:?}..{:?}{on_layer}", slope[0], slope[1], height[0], height[1])).hint("allow steeper ground (`slope`), a wider `height` range, a lower `layer_min` or a smaller `min_distance`"));
        }

        // the commands
        let round = |v: f32| Json::Float((v as f64 * 10_000.0).round() / 10_000.0);
        let vec3 = |p: [f32; 3]| Json::arr([round(p[0]), round(p[1]), round(p[2])]);
        let mut cmds: Vec<Command> = Vec::new();
        let mut base = self.sim.world.next_uid();
        if let Some(name) = prefab {
            lunaris_sim::prefab::plan(&self.sim.scripts.prefabs, name, &[])?; // an unknown prefab is an error before anything is placed
        }
        for (pos, rot, s) in &spots {
            let rot_json = Json::arr([round(rot[0]), round(rot[1]), round(rot[2]), round(rot[3])]);
            match (prefab, &components) {
                (Some(name), _) => {
                    let mut overrides: Vec<(String, Json)> = Vec::new();
                    lunaris_sim::prefab::merge_component(&mut overrides, "Transform", &Json::obj([("pos", vec3(*pos)), ("rot", rot_json), ("scale", vec3([*s, *s, *s]))]));
                    let planned = lunaris_sim::prefab::plan(&self.sim.scripts.prefabs, name, &overrides)?;
                    cmds.extend(lunaris_sim::prefab::spawn_commands(&planned, base));
                    base += planned.len() as u64;
                }
                (None, Some(parts)) => {
                    let mut made = parts.clone();
                    let old = made.iter().position(|(k, _)| k == "Transform");
                    let mut tr = match old.map(|i| made[i].1.clone()) {
                        Some(Json::Obj(o)) => o,
                        _ => Vec::new(),
                    };
                    let given: [f32; 3] = tr.iter().find(|(k, _)| k == "scale").and_then(|(_, v)| v.as_array()).filter(|l| l.len() == 3).map(|l| [l[0].as_f64().unwrap_or(1.0) as f32, l[1].as_f64().unwrap_or(1.0) as f32, l[2].as_f64().unwrap_or(1.0) as f32]).unwrap_or([1.0; 3]);
                    tr.retain(|(k, _)| k != "pos" && k != "rot" && k != "scale");
                    tr.push(("pos".to_string(), vec3(*pos)));
                    tr.push(("rot".to_string(), rot_json));
                    tr.push(("scale".to_string(), vec3([given[0] * s, given[1] * s, given[2] * s])));
                    match old {
                        Some(i) => made[i].1 = Json::Obj(tr),
                        None => made.push(("Transform".to_string(), Json::Obj(tr))),
                    }
                    cmds.push(Command::Spawn { uid: None, components: made });
                }
                (None, None) => unreachable!(),
            }
        }
        let (done, inverse) = self.sim.apply_batch(&cmds)?;
        let undoable = self.push_history(inverse);
        let uids: Vec<u64> = done.iter().filter_map(|d| d.uid).collect();
        let mut out = Json::obj([
            ("ok", Json::Bool(true)),
            ("placed", Json::from(spots.len())),
            ("requested", Json::from(count as usize)),
            ("entities", Json::from(uids.len())),
            ("attempts", Json::from(attempts as usize)),
            ("terrain", uid_json(uid)),
            ("first_uid", uid_json(uids[0])),
            ("last_uid", uid_json(*uids.last().unwrap())),
            ("undoable", Json::Bool(undoable)),
        ]);
        if (spots.len() as u64) < count {
            out.set("note", Json::str(format!("only {} of {count} fit: allow steeper ground (`slope`), a wider `height` range or a smaller `min_distance`", spots.len())));
        }
        Ok(out)
    }
}
