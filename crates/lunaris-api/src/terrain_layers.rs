//! Painted terrain for agents (`TerrainLayers`, docs/TERRAIN.md "Layers"): the `terrain_layer` tool says which materials are under a point, the `scatter` tool can keep to a
//! layer, and `check` finds the mistakes that would leave a ground in one flat colour.
use crate::engine::{uid_json, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_render::terrain_layers::{parse_layers, LayerSpec, Splat};
use lunaris_sim::components::{Terrain, TerrainLayers, Transform};
use std::sync::Arc;

/// A painted terrain, ready to be asked what is where.
pub(crate) struct LayerLookup {
    pub uid: u64,
    pub splat_name: String,
    pub specs: Vec<LayerSpec>,
    pub splat: Arc<Splat>,
    /// The north-west corner of the ground (x, z) and its size in metres.
    pub corner: [f32; 2],
    pub size: [f32; 2],
}

impl LayerLookup {
    /// Where (x, z) is over the ground, 0 to 1 each way (`None` outside it).
    pub fn uv(&self, x: f32, z: f32) -> Option<(f32, f32)> {
        let (u, v) = ((x - self.corner[0]) / self.size[0], (z - self.corner[1]) / self.size[1]);
        (u >= 0.0 && v >= 0.0 && u <= 1.0 && v <= 1.0).then_some((u, v))
    }

    /// The weight of each layer at (x, z): the four numbers add up to 1 (layers that do not exist weigh 0).
    pub fn weights(&self, x: f32, z: f32) -> Option<[f32; 4]> {
        let (u, v) = self.uv(x, z)?;
        Some(self.splat.weights(u, v, self.specs.len()))
    }

    /// The layer with the most weight (the first on a tie).
    pub fn dominant(&self, w: [f32; 4]) -> usize {
        let mut best = 0;
        for k in 1..self.specs.len().min(4) {
            if w[k] > w[best] {
                best = k;
            }
        }
        best
    }

    /// A layer by name or by its number (0 for the first).
    pub fn find(&self, which: &Json) -> Result<usize> {
        let names = || self.specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ");
        match which {
            Json::Str(name) => self.specs.iter().position(|s| &s.name == name).ok_or_else(|| EcsError::new("no_such_layer", format!("the terrain {} has no layer called `{name}`", self.uid)).hint(format!("its layers: {}", names())).at("layer")),
            other => {
                let k = other.as_u64().ok_or_else(|| EcsError::new("type_mismatch", "`layer` must be a layer's name or its number (0 is the first)").at("layer"))?;
                if (k as usize) < self.specs.len() {
                    Ok(k as usize)
                } else {
                    Err(EcsError::new("no_such_layer", format!("the terrain {} has {} layers, so there is no layer number {k}", self.uid, self.specs.len())).hint(format!("its layers: {}", names())).at("layer"))
                }
            }
        }
    }
}

impl Engine {
    /// The painted terrains of the world (those with `Terrain`, `Transform` and `TerrainLayers`), by uid.
    fn painted_uids(&mut self) -> Result<Vec<u64>> {
        let mut uids = Vec::new();
        self.sim.world.for_each_uid::<(&Terrain, &Transform, &TerrainLayers), _>(|_, uid, _| uids.push(uid))?;
        uids.sort_unstable();
        Ok(uids)
    }

    /// Read the splat map and the layer text of the terrain entity `uid`.
    pub(crate) fn layer_lookup(&mut self, uid: u64) -> Result<LayerLookup> {
        let e = self.sim.world.require_entity(uid)?;
        let (Some(t), Some(g)) = (self.sim.world.get::<Transform>(e).copied(), self.sim.world.get::<Terrain>(e).copied()) else {
            return Err(EcsError::new("no_terrain", format!("entity {uid} has no Terrain (and a Transform)")));
        };
        let Some(l) = self.sim.world.get::<TerrainLayers>(e).copied() else {
            return Err(EcsError::new("no_layers", format!("the terrain {uid} has no TerrainLayers, so it has one texture and no layers to ask about")).hint("add a TerrainLayers {splat, layers} to the entity that has the Terrain (docs/TERRAIN.md, \"Layers\")"));
        };
        let (splat_name, text) = (self.sim.world.strings().get(l.splat.0).to_string(), self.sim.world.strings().get(l.layers.0).to_string());
        let specs = parse_layers(&text).map_err(|e| EcsError::new("bad_layers", format!("TerrainLayers.layers of entity {uid}: {e}")).hint(format!("a line is `{}`", lunaris_render::terrain_layers::LAYER_SYNTAX)))?;
        let before = self.textures.warnings.len();
        let splat = self.textures.splat(&splat_name);
        let why = self.textures.warnings.split_off(before).join("; ");
        let splat = splat.ok_or_else(|| EcsError::new("splat_unreadable", format!("the splat map `{splat_name}` of entity {uid} could not be read: {}", if why.is_empty() { "it has no name".to_string() } else { why })).hint("an RGBA PNG inside the workspace folder; `check` lists the problem, `reload_terrain` reads it again"))?;
        let size = [if g.size[0].is_finite() { g.size[0].abs().max(0.01) } else { 1.0 }, if g.size[1].is_finite() { g.size[1].abs().max(0.01) } else { 1.0 }];
        let corner = [t.pos[0] - size[0] * 0.5, t.pos[2] - size[1] * 0.5];
        Ok(LayerLookup { uid, splat_name, specs, splat, corner, size })
    }

    /// `terrain_layer {x, z}`: which materials are under a point, and how much of each.
    pub(crate) fn terrain_layer_tool(&mut self, a: &Args) -> Result<Json> {
        let num = |k: &str| a.0.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).ok_or_else(|| a.missing(k, "number"));
        let (x, z) = (num("x")? as f32, num("z")? as f32);
        self.sim.load_terrains();
        let candidates: Vec<u64> = match a.opt_u64("terrain")? {
            Some(u) => vec![u],
            None => self.painted_uids()?,
        };
        if candidates.is_empty() {
            let any = self.sim.world.entities_by_uid().iter().any(|e| self.sim.world.get::<Terrain>(*e).is_some());
            return Err(if any {
                EcsError::new("no_layers", "there is no TerrainLayers in the world, so no terrain has layers to ask about").hint("add a TerrainLayers {splat, layers} to the entity that has the Terrain (docs/TERRAIN.md, \"Layers\")")
            } else {
                EcsError::new("no_terrain", "there is no terrain in the world").hint("spawn an entity with Transform, Terrain and TerrainLayers first")
            });
        }
        // the painted terrains that cover the point: the highest ground wins (as for `terrain_height`), the lowest uid on a tie
        let mut found: Vec<(f32, LayerLookup)> = Vec::new();
        let mut first_error: Option<EcsError> = None;
        for uid in candidates {
            match self.layer_lookup(uid) {
                Ok(l) => {
                    if l.uv(x, z).is_some() {
                        let height = self.ground_height(uid, x, z).unwrap_or(f32::NEG_INFINITY);
                        found.push((height, l));
                    }
                }
                Err(e) => first_error = first_error.or(Some(e)),
            }
        }
        let Some((_, best)) = found.into_iter().min_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.uid.cmp(&b.1.uid))) else {
            return Err(first_error.unwrap_or_else(|| EcsError::new("no_terrain", format!("there is no painted terrain under x = {x}, z = {z}")).hint("a point inside the ground of a Terrain that has TerrainLayers")));
        };
        let w = best.weights(x, z).expect("inside the ground");
        let round = |v: f32| Json::Float((v as f64 * 10_000.0).round() / 10_000.0);
        let dominant = best.dominant(w);
        let layers = best.specs.iter().enumerate().map(|(k, s)| Json::obj([("name", Json::str(s.name.clone())), ("texture", Json::str(s.texture.clone())), ("weight", round(w[k]))]));
        Ok(Json::obj([
            ("terrain", uid_json(best.uid)),
            ("splat", Json::str(best.splat_name.clone())),
            ("weights", Json::arr(w.iter().map(|v| round(*v)))),
            ("layers", Json::arr(layers)),
            ("dominant", Json::str(best.specs[dominant].name.clone())),
            ("dominant_index", Json::from(dominant)),
        ]))
    }

    /// The height of the ground of terrain `uid` at (x, z), when its heightmap has been read and (x, z) is over it.
    fn ground_height(&mut self, uid: u64, x: f32, z: f32) -> Option<f32> {
        let e = self.sim.world.entity(uid)?;
        let (t, g) = (self.sim.world.get::<Transform>(e).copied()?, self.sim.world.get::<Terrain>(e).copied()?);
        let name = self.sim.world.strings().get(g.heightmap.0).to_string();
        let field = self.sim.heightfields.get(&name)?;
        lunaris_sim::terrain::Placed::new(field, t.pos, &g).surface(x, z)
    }
}
