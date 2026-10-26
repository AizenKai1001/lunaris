//! Paint mode (Unreal's Foliage mode, small): pick a brush in the Place tab (a thing the Entity menu makes) or in the Project panel (a model), then drag over the scene view: the scene is
//! sprinkled with copies of it inside the brush's circle, each one standing on whatever is under it, with a turn of its own and a scale a little off. The stroke is shown as a ring while it
//! is made and everything of it is spawned when the button is let go, as **one** `batch` call, so that one Undo takes the whole stroke back.
use crate::place;
use crate::state::{Studio, Template};
use lunaris_json::Json;

type V3 = [f32; 3];

/// The most copies one stroke makes (a stroke over a whole field is spawned in one go, and one `batch` this big is still quick).
const MAX_PER_STROKE: usize = 2000;

/// What is painted.
#[derive(Clone, Debug, PartialEq)]
pub enum BrushItem {
    /// Something the Entity menu makes (a cube, a sphere, a light...).
    Template(Template),
    /// A model file of the workspace.
    Model(String),
}

/// The brush: what, how wide a circle and how closely.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub item: BrushItem,
    /// The radius of the circle the copies are scattered in, in metres.
    pub radius: f32,
    /// The least distance between two points of a stroke where copies are put, in metres (also how dense the copies are inside the circle).
    pub spacing: f32,
}

impl Brush {
    pub fn new(item: BrushItem) -> Brush {
        Brush { item, radius: 3.0, spacing: 1.2 }
    }

    /// The name of what is painted.
    pub fn label(&self) -> String {
        match &self.item {
            BrushItem::Template(t) => t.label().to_string(),
            BrushItem::Model(path) => {
                let file = path.rsplit('/').next().unwrap_or(path);
                file.rsplit_once('.').map_or(file, |(s, _)| s).to_string()
            }
        }
    }

    /// How many copies go round each point of a stroke: about the number of `spacing` squares in the circle, thinned a little.
    pub fn per_point(&self) -> usize {
        let n = (self.radius / self.spacing.max(0.05)).powi(2) * 0.8;
        (n.round() as usize).clamp(1, 40)
    }
}

/// A small xorshift (the brush's randomness is the editor's, not the simulation's).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
}

/// Spawn the stroke: copies round every point of `centres`, in one `batch` (so one Undo). Each copy stands on what is under it (a ray from above) and keeps the height of the point when
/// there is nothing. Returns how many were made.
pub fn stamp(studio: &mut Studio, brush: &Brush, centres: &[V3]) -> usize {
    if centres.is_empty() {
        return 0;
    }
    // how far above the ground a copy of scale 1 has its origin: a unit cube is centred (0.5), a light hangs (2), a model's lowest point is where the author put it
    let (stem, lift) = match &brush.item {
        BrushItem::Template(t) => (t.label().to_string(), place::lift(*t)),
        BrushItem::Model(path) => {
            let low = studio.try_call("model_info", Json::obj([("path", Json::str(path.clone()))])).and_then(|i| i.get("bounds")?.get("min")?.as_array()?.get(1)?.as_f64()).unwrap_or(0.0) as f32;
            (brush.label(), -low)
        }
    };
    // only the things that have a shape of their own are turned and scaled (a light or a sound keeps its size)
    let shaped = match &brush.item {
        BrushItem::Template(t) => matches!(t, Template::Cube | Template::Sphere | Template::Cylinder | Template::Empty),
        BrushItem::Model(_) => true,
    };
    // the numbers that go after the name continue those that are in the scene
    let mut next = studio.all.iter().filter_map(|r| r.name.strip_prefix(stem.as_str())?.trim().parse::<u64>().ok()).max().unwrap_or(0) + 1;
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (studio.total_entities as u64).wrapping_mul(0x2545_F491_4F6C_DD1D) ^ centres.len() as u64);
    let mut commands: Vec<Json> = Vec::new();
    'stroke: for c in centres {
        for _ in 0..brush.per_point() {
            if commands.len() >= MAX_PER_STROKE {
                break 'stroke;
            }
            let (a, d) = (rng.next() * std::f32::consts::TAU, brush.radius * rng.next().sqrt());
            let (x, z) = (c[0] + a.cos() * d, c[2] + a.sin() * d);
            // the ground under that place (a ray from a little above the point, so that a roof over the spot is not what is stood on)
            let y = ground_at(studio, x, c[1] + brush.radius + 1.0, z).unwrap_or(c[1]);
            let (yaw, s) = (rng.next() * std::f32::consts::TAU, 0.8 + rng.next() * 0.4);
            let name = format!("{stem} {next}");
            next += 1;
            let comps = match &brush.item {
                BrushItem::Template(t) => {
                    let mut comps = t.components(&name, [x, y + lift * if shaped { s } else { 1.0 }, z]);
                    if shaped {
                        comps.set("Transform", transform([x, y + lift * s, z], yaw, s));
                    }
                    comps
                }
                BrushItem::Model(path) => {
                    let mut comps = Json::obj([("Name", Json::obj([("value", Json::str(name))])), ("Transform", transform([x, y + lift * s, z], yaw, s))]);
                    comps.set("MeshRenderer", Json::obj([("mesh", Json::str(path.clone()))]));
                    comps
                }
            };
            commands.push(Json::obj([("op", Json::str("spawn")), ("components", comps)]));
        }
    }
    let n = commands.len();
    if studio.try_call("batch", Json::obj([("commands", Json::Arr(commands))])).is_some() {
        studio.rows_dirty = true;
        studio.refresh_rows();
        n
    } else {
        0
    }
}

/// A Transform at `pos`, turned by `yaw` about the vertical and scaled by `s` all round.
fn transform(pos: V3, yaw: f32, s: f32) -> Json {
    let r = |x: f32| Json::from(((x * 1000.0).round() / 1000.0) as f64);
    let arr = |v: &[f32]| Json::arr(v.iter().map(|x| r(*x)));
    Json::obj([("pos", arr(&pos)), ("rot", arr(&[0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()])), ("scale", arr(&[s, s, s]))])
}

/// The height of the first thing a ray comes to straight down over (x, z), starting at `top`.
fn ground_at(studio: &mut Studio, x: f32, top: f32, z: f32) -> Option<f32> {
    let arr = |v: [f32; 3]| Json::arr(v.iter().map(|c| Json::from(*c)));
    let r = studio.try_call("raycast", Json::obj([("origin", arr([x, top, z])), ("direction", arr([0.0, -1.0, 0.0])), ("max_distance", Json::from(400.0))]))?;
    if r.get("hit").and_then(|h| h.as_bool()) != Some(true) {
        return None;
    }
    r.get("point")?.as_array()?.get(1)?.as_f64().map(|v| v as f32)
}
