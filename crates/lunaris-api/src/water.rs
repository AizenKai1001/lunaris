//! Water bodies for agents (`WaterBody`, `InWater`, `Buoyant`; docs/TERRAIN.md "Water bodies"): the `water_depth` tool measures the water at a point, and `check` finds what keeps a lake from
//! being drawn or from floating anything.
use crate::engine::{uid_json, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_sim::components::WaterBody;

impl Engine {
    /// `water_depth {x, z, y?}`: how high the water's surface is at a point, how deep the water is there over the ground, and (with `y`) how far it reaches above a height.
    pub(crate) fn water_depth_tool(&mut self, a: &Args) -> Result<Json> {
        let num = |k: &str| a.0.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).ok_or_else(|| a.missing(k, "number"));
        let (x, z) = (num("x")? as f32, num("z")? as f32);
        let y = match a.0.get("y") {
            None | Some(Json::Null) => None,
            Some(v) => Some(v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| EcsError::new("type_mismatch", "`y` must be a number").at("y"))? as f32),
        };
        self.sim.load_terrains();
        // the water bodies of the world, and the ones whose picture could not be read
        let mut all: Vec<(u64, u32)> = Vec::new();
        self.sim.world.for_each_uid::<(&WaterBody,), _>(|_, uid, (w,)| all.push((uid, w.heightmap.0)))?;
        if all.is_empty() {
            return Err(EcsError::new("no_water", "there is no water in the world").hint("spawn an entity with Transform and WaterBody (a 16-bit picture of the water's heights, 0 where it is dry; docs/TERRAIN.md, \"Water bodies\")"));
        }
        all.sort();
        let unread: Vec<String> = all.iter().map(|(_, s)| self.sim.world.strings().get(*s).to_string()).filter(|n| !self.sim.heightfields.contains_key(n)).collect();
        let list = lunaris_sim::water::bodies(&mut self.sim.world, &self.sim.heightfields);
        let found = lunaris_sim::water::surface_at(&list, &self.sim.heightfields, self.sim.world.strings(), x, z);
        let ground = lunaris_sim::water::ground_at(&mut self.sim.world, &self.sim.heightfields, x, z);
        let round = |v: f32| Json::Float((v as f64 * 10_000.0).round() / 10_000.0);
        let opt = |v: Option<f32>| v.map_or(Json::Null, round);
        // the water is here when there is a surface and the ground (when there is any under it) is below it
        let wet = found.is_some_and(|(s, _)| ground.is_none_or(|g| g < s));
        let depth = match (found, ground) {
            (Some((s, _)), Some(g)) => round((s - g).max(0.0)),
            (Some(_), None) => Json::Null,
            (None, _) => Json::Float(0.0),
        };
        let mut out = Json::obj([
            ("wet", Json::Bool(wet)),
            ("surface", opt(found.map(|f| f.0))),
            ("ground", opt(ground)),
            ("depth", depth),
            ("water", found.map_or(Json::Null, |f| uid_json(f.1))),
        ]);
        if let Some(y) = y {
            out.set("above", opt(found.map(|f| f.0 - y)));
        }
        if !unread.is_empty() {
            out.set("unread", Json::arr(unread.into_iter().map(Json::str)));
        }
        Ok(out)
    }
}
