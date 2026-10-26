//! Effects: the data of a `ParticleSystem` (docs/FX.md). An effect is a JSON document of **emitters**; each emitter says how particles are born (a rate, bursts, a shape), what they start with, what happens to them
//! while they live (forces, noise, colliders), how their size, colour and speed follow their age (curves), how they are drawn (pictures with a flipbook, stretched along their speed, ribbons, small solids,
//! a light each) and what they cause (an **event** on a death, a hit or a birth spawns particles of another emitter of the same effect: sparks off a bullet, a smoke that comes from a dying flame).
//!
//! This module is only the data and its checking: `parse` reads and validates the text and says where a mistake is (`emitters[1].update.noise.scale: must be above 0`), so that the scene builder, the `check` tool
//! and the game all give the same answers. The simulation and the drawing are in `lunaris-render` (`fx_sim`, `fx_draw`): they only ever see a picture, never the state hash.
use crate::particle_modules::Gradient;
use lunaris_json::Json;

type V3 = [f32; 3];

/// The longest a particle may live, in seconds, the most particles one emitter may hold and the most one effect may.
pub const MAX_LIFE: f32 = 3600.0;
pub const MAX_EMITTER: u32 = 100_000;
pub const MAX_BUDGET: u32 = 200_000;
/// The most emitters in one effect and the most keys in one curve.
pub const MAX_EMITTERS: usize = 32;
pub const MAX_KEYS: usize = 64;

/// The space a particle lives in: `Local` particles move with the entity (a flame on a torch), `World` ones stay where they were born (smoke behind a car).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Local,
    World,
}

/// Where a particle is born, in the emitter's own space.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Point,
    /// A ball (or just its skin when `shell`).
    Sphere { radius: f32, shell: bool },
    Box { half: V3 },
    /// A disc in the horizontal plane (or just its rim when `ring`).
    Disc { radius: f32, ring: bool },
    /// A straight line from the origin to `to`.
    Line { to: V3 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    /// Seconds after the emitter starts (and, when the emitter has a `duration` and loops, after the start of each round).
    pub time: f32,
    pub count: [u32; 2],
    /// Chance (0..1) that the burst happens at all.
    pub chance: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    /// Particles a second while the emitter is active (times the system's `intensity`, the level of detail and `rate_curve`).
    pub rate: f32,
    /// A multiplier of the rate along the emitter's round (0 at its start, 1 at its end; a round is `duration` seconds, or 1 when it has none).
    pub rate_curve: Option<Gradient<1>>,
    pub bursts: Vec<Burst>,
    pub shape: Shape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Init {
    pub life: [f32; 2],
    pub speed: [f32; 2],
    /// The way particles fly (in the emitter's space), spread by `spread` degrees about it (180: every direction).
    pub direction: V3,
    pub spread: f32,
    /// 0..1: how much of the direction is "away from the middle of the shape" instead of `direction`.
    pub radial: f32,
    /// Size in metres (a picture's width) and its ratio of height to width.
    pub size: [f32; 2],
    pub aspect: f32,
    /// Degrees, and degrees a second.
    pub rotation: [f32; 2],
    pub spin: [f32; 2],
    /// Colour (display colours; above 1 glows when added) and how far a particle's brightness may stray from it (0..1).
    pub color: [f32; 4],
    pub color_jitter: f32,
    /// How much of the speed of the entity (a car's, a hand's) a particle born in the world's space takes along (0..1).
    pub inherit_velocity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    /// A wandering force (curl noise, so it swirls and never piles particles up), in m/s^2.
    pub strength: f32,
    /// The size of a swirl in metres.
    pub scale: f32,
    /// How fast the field changes, in swirls a second.
    pub speed: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vortex {
    pub pos: V3,
    pub axis: V3,
    pub strength: f32,
    pub pull: f32,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attractor {
    pub pos: V3,
    pub strength: f32,
    pub radius: f32,
    pub falloff: f32,
    /// A particle this close to the point is gone (0: it never is).
    pub kill: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// `dot(normal, p) = d`; the particle must be on the side the normal points to. `relative`: `p` is measured from the entity (the ground under it) and not from the emitter's space's origin (for particles in the world's space).
    pub normal: V3,
    pub d: f32,
    pub relative: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub pos: V3,
    pub radius: f32,
    pub inside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cuboid {
    pub pos: V3,
    pub half: V3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Bounce,
    Kill,
    Stick,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Collide {
    pub hit: Hit,
    pub bounce: f32,
    pub friction: f32,
    /// How much of its life a particle loses at each hit (0..1).
    pub life_loss: f32,
    pub planes: Vec<Plane>,
    pub balls: Vec<Ball>,
    pub boxes: Vec<Cuboid>,
    /// Also the colliders of the world near the entity (the boxes and balls of its `Collider`s that do not move).
    pub world: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub gravity: V3,
    pub drag: f32,
    pub wind: V3,
    pub noise: Option<Noise>,
    pub vortices: Vec<Vortex>,
    pub attractors: Vec<Attractor>,
    /// The most speed a particle may have (0: no limit).
    pub speed_limit: f32,
    pub collide: Option<Collide>,
}

/// What follows a particle through its life, as keys from 0 (born) to 1 (gone) joined by straight lines.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Curves {
    /// Times the size the particle was born with.
    pub size: Option<Gradient<1>>,
    /// Times the speed (the particle flies at its own speed times this).
    pub speed: Option<Gradient<1>>,
    /// Times the opacity.
    pub alpha: Option<Gradient<1>>,
    /// Times the colour the particle was born with.
    pub color: Option<Gradient<4>>,
    /// Times the light the particle gives (when the emitter has a `light`).
    pub light: Option<Gradient<1>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Facing {
    /// A flat picture that always faces the camera.
    Camera,
    /// A picture along the particle's speed, stretched by `stretch` seconds of travel (sparks, rain).
    Velocity,
    /// A flat picture fixed in a direction (a ripple on the ground: `[0, 1, 0]`).
    Axis(V3),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FrameMode {
    /// The flipbook plays once over a particle's life.
    OverLife,
    /// Every particle keeps one frame, chosen at birth.
    Random,
    /// The flipbook plays at this many frames a second, round and round.
    Fps(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// `None`: the particle's own colour.
    pub color: Option<V3>,
    /// In the engine's light units, times the particle's opacity and the `light` curve.
    pub intensity: f32,
    pub range: f32,
    /// At most this many of the particles (the brightest) give light at once.
    pub max: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Sprite,
    /// A ribbon through the places a particle has been.
    Ribbon,
    /// A small solid, one for every particle.
    Solid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solid {
    Cube,
    Diamond,
    Tetra,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Render {
    pub kind: Kind,
    /// A picture of the workspace or `soft`, `circle` (the built-in ones); empty: `soft`.
    pub texture: String,
    /// Hard-edged texels, no smoothing between them (`"filter": "nearest"`: pixel art; the default `linear` smooths).
    pub nearest: bool,
    pub sheet: [u32; 2],
    pub frames: u32,
    pub frame_mode: FrameMode,
    pub additive: bool,
    /// Pictures fade out where they come within this many metres of a surface behind them (0: they do not). The GPU renderer's high qualities do it; the reference renderer draws them without.
    pub soft: f32,
    pub facing: Facing,
    /// Seconds of travel a `Velocity` picture is long (its length is its size plus speed times this).
    pub stretch: f32,
    pub light: Option<Light>,
    /// For a `Ribbon`: the places kept (2..32), the seconds between them and the width as a multiple of the size.
    pub ribbon_points: u32,
    pub ribbon_interval: f32,
    pub ribbon_width: f32,
    pub solid: Solid,
    /// Alpha-blended pictures are drawn far to near; off, they are drawn in the order of birth (cheaper, and right for a spread of small things).
    pub sort: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On {
    Birth,
    Death,
    Collide,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub on: On,
    /// The number of the emitter (in `Effect::emitters`) whose particles are born at the place of the event.
    pub spawn: usize,
    pub count: [u32; 2],
    pub chance: f32,
    /// 0..1: how much of the speed of the particle that caused it the new ones take along.
    pub inherit_velocity: f32,
    /// 0..1: how much of its colour.
    pub inherit_color: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    pub name: String,
    pub space: Space,
    /// The most particles it holds at once.
    pub max: u32,
    /// A looping emitter starts over after `duration` seconds; one that does not loop spawns for `duration` seconds and is done.
    pub looping: bool,
    pub duration: f32,
    pub delay: f32,
    /// A child makes no particles by itself, only when an event of another emitter asks for them.
    pub child: bool,
    /// `"simulate": "gpu"`: the particles of this emitter are stepped and turned into pictures by a compute pass on the card (the GPU renderer's high qualities), which makes tens of thousands as cheap as a hundred; a renderer with no card
    /// (the reference picture) steps them on the processor as any other emitter, so the effect is the same either way, particle for particle not. The card cannot do everything: see `check_gpu` (sprites only, no events, no ribbons, solids or lights, planes as the only colliders).
    pub gpu: bool,
    pub spawn: Spawn,
    pub init: Init,
    pub update: Update,
    pub curves: Curves,
    pub render: Render,
    pub events: Vec<Event>,
}

/// A particle system's whole description.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub name: String,
    /// The radius of a ball round the entity that holds every particle of the effect (metres): outside the picture, nothing is worked out or drawn. 0: worked out from the emitters.
    pub bounds: f32,
    /// The most particles all the emitters hold together.
    pub budget: u32,
    /// Level of detail by the distance from the camera: full up to `lod_near`, thinning out to nothing at `lod_far` (0: always full).
    pub lod_near: f32,
    pub lod_far: f32,
    /// Seconds a system that has just appeared has already run (the looping emitters are full from the first picture); 0: from the longest life.
    pub prewarm: f32,
    pub emitters: Vec<Emitter>,
    /// What was odd but harmless (an unknown key, a number that was brought into range).
    pub warnings: Vec<String>,
}

impl Effect {
    /// The longest a particle of any emitter lives.
    pub fn max_life(&self) -> f32 {
        self.emitters.iter().map(|e| e.init.life[0].max(e.init.life[1])).fold(0.0, f32::max)
    }

    /// The radius of the ball that holds every particle (`bounds`, or what the emitters' numbers allow), in metres.
    pub fn radius(&self) -> f32 {
        if self.bounds > 0.0 {
            return self.bounds;
        }
        let mut r = 0.0f32;
        for e in &self.emitters {
            let start = match &e.spawn.shape {
                Shape::Point => 0.0,
                Shape::Sphere { radius, .. } | Shape::Disc { radius, .. } => *radius,
                Shape::Box { half } => length(*half),
                Shape::Line { to } => length(*to),
            };
            let life = e.init.life[0].max(e.init.life[1]);
            let speed = e.init.speed[0].abs().max(e.init.speed[1].abs());
            let g = length(e.update.gravity) + length(e.update.wind) + e.update.noise.map_or(0.0, |n| n.strength * 1.7321);
            let size = e.init.size[0].abs().max(e.init.size[1].abs()) * e.init.aspect.abs().max(1.0) * e.curves.size.as_ref().map_or(1.0, |c| c.keys.iter().fold(1.0f32, |m, k| m.max(k.1[0])));
            let stretch = if let Facing::Velocity = e.render.facing { (speed + g * life) * e.render.stretch } else { 0.0 };
            let travel = speed * life * e.curves.speed.as_ref().map_or(1.0, |c| c.keys.iter().fold(1.0f32, |m, k| m.max(k.1[0]))) + 0.5 * g * life * life;
            r = r.max(start + travel + size + stretch);
        }
        // (the attractors and vortices only ever pull particles in, a collider only keeps them near)
        (r * 1.1 + 0.05).clamp(0.1, 100_000.0)
    }
}

fn length(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

// ---- reading the text

/// The place in the document, for the messages: `emitters[1].update`.
struct At<'a> {
    j: &'a Json,
    path: String,
}

struct Cx {
    warnings: Vec<String>,
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (prev + usize::from(a[i - 1] != b[j - 1])).min(row[j] + 1).min(row[j - 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

impl<'a> At<'a> {
    fn root(j: &'a Json) -> At<'a> {
        At { j, path: String::new() }
    }

    fn child(&self, key: &str, j: &'a Json) -> At<'a> {
        At { j, path: if self.path.is_empty() { key.to_string() } else { format!("{}.{key}", self.path) } }
    }

    fn indexed(&self, key: &str, i: usize, j: &'a Json) -> At<'a> {
        let path = match (self.path.is_empty(), key.is_empty()) {
            (true, _) => format!("{key}[{i}]"),
            (false, true) => format!("{}[{i}]", self.path),
            (false, false) => format!("{}.{key}[{i}]", self.path),
        };
        At { j, path }
    }

    fn err<T>(&self, msg: impl std::fmt::Display) -> Result<T, String> {
        Err(if self.path.is_empty() { msg.to_string() } else { format!("{}: {msg}", self.path) })
    }

    fn obj(&self) -> Result<&'a [(String, Json)], String> {
        match self.j.as_object() {
            Some(o) => Ok(o),
            None => self.err(format!("must be an object, not {}", self.j.type_name())),
        }
    }

    /// The members of an object, with a warning for every key that is not in `allowed` (and the nearest one that is, when it is close).
    fn keys(&self, allowed: &[&str], cx: &mut Cx) -> Result<&'a [(String, Json)], String> {
        let o = self.obj()?;
        for (k, _) in o {
            if !allowed.contains(&k.as_str()) {
                let near = allowed.iter().filter(|a| edit_distance(a, k) <= 2).min_by_key(|a| edit_distance(a, k));
                let at = if self.path.is_empty() { String::new() } else { format!("{}.", self.path) };
                cx.warnings.push(match near {
                    Some(n) => format!("{at}{k}: unknown key, ignored (did you mean `{n}`?)"),
                    None => format!("{at}{k}: unknown key, ignored"),
                });
            }
        }
        Ok(o)
    }
}

fn member<'a>(o: &'a [(String, Json)], k: &str) -> Option<&'a Json> {
    o.iter().find(|(n, _)| n == k).map(|(_, v)| v)
}

/// What a numeric key takes: the least and most (a value outside is an error: a number that makes no sense is not guessed at).
fn number(at: &At, lo: f32, hi: f32) -> Result<f32, String> {
    let v = match at.j.as_f64() {
        Some(v) => v as f32,
        None => return at.err(format!("must be a number, not {}", at.j.type_name())),
    };
    if !v.is_finite() || v < lo || v > hi {
        return at.err(format!("{v} is outside {lo} to {hi}"));
    }
    Ok(v)
}

fn get_f(o: &At, items: &[(String, Json)], k: &str, d: f32, lo: f32, hi: f32) -> Result<f32, String> {
    match member(items, k) {
        Some(j) => number(&o.child(k, j), lo, hi),
        None => Ok(d),
    }
}

fn get_u(o: &At, items: &[(String, Json)], k: &str, d: u32, lo: u32, hi: u32) -> Result<u32, String> {
    match member(items, k) {
        Some(j) => {
            let a = o.child(k, j);
            match j.as_f64() {
                Some(v) if v.fract() == 0.0 && v >= lo as f64 && v <= hi as f64 => Ok(v as u32),
                Some(v) => a.err(format!("{v} is not a whole number from {lo} to {hi}")),
                None => a.err(format!("must be a number, not {}", j.type_name())),
            }
        }
        None => Ok(d),
    }
}

fn get_b(o: &At, items: &[(String, Json)], k: &str, d: bool) -> Result<bool, String> {
    match member(items, k) {
        Some(j) => match j.as_bool() {
            Some(b) => Ok(b),
            None => o.child(k, j).err(format!("must be true or false, not {}", j.type_name())),
        },
        None => Ok(d),
    }
}

fn get_s(o: &At, items: &[(String, Json)], k: &str, d: &str) -> Result<String, String> {
    match member(items, k) {
        Some(j) => match j.as_str() {
            Some(s) => Ok(s.to_string()),
            None => o.child(k, j).err(format!("must be text, not {}", j.type_name())),
        },
        None => Ok(d.to_string()),
    }
}

/// A number, or a pair `[least, most]`.
fn get_range(o: &At, items: &[(String, Json)], k: &str, d: [f32; 2], lo: f32, hi: f32) -> Result<[f32; 2], String> {
    let Some(j) = member(items, k) else { return Ok(d) };
    let a = o.child(k, j);
    if let Some(arr) = j.as_array() {
        if arr.len() != 2 {
            return a.err("a range is a number or a pair [least, most]");
        }
        let x = number(&a.indexed("", 0, &arr[0]), lo, hi)?;
        let y = number(&a.indexed("", 1, &arr[1]), lo, hi)?;
        Ok([x.min(y), x.max(y)])
    } else {
        let v = number(&a, lo, hi)?;
        Ok([v, v])
    }
}

fn get_urange(o: &At, items: &[(String, Json)], k: &str, d: [u32; 2], hi: u32) -> Result<[u32; 2], String> {
    let r = get_range(o, items, k, [d[0] as f32, d[1] as f32], 0.0, hi as f32)?;
    Ok([r[0].round() as u32, r[1].round() as u32])
}

fn vec3(at: &At, lo: f32, hi: f32) -> Result<V3, String> {
    match at.j.as_array() {
        Some(a) if a.len() == 3 => Ok([number(&at.indexed("", 0, &a[0]), lo, hi)?, number(&at.indexed("", 1, &a[1]), lo, hi)?, number(&at.indexed("", 2, &a[2]), lo, hi)?]),
        _ => at.err("must be three numbers [x, y, z]"),
    }
}

fn get_v3(o: &At, items: &[(String, Json)], k: &str, d: V3) -> Result<V3, String> {
    match member(items, k) {
        Some(j) => vec3(&o.child(k, j), -1.0e6, 1.0e6),
        None => Ok(d),
    }
}

/// A display colour: `"#rgb"`, `"#rrggbb"`, `"#rrggbbaa"`, or `[r, g, b]` / `[r, g, b, a]` (0..1; above 1 glows).
fn colour(at: &At) -> Result<[f32; 4], String> {
    if let Some(s) = at.j.as_str() {
        let h = s.strip_prefix('#').unwrap_or(s);
        let nib = |c: char| c.to_digit(16).map(|d| d as f32);
        let ch: Vec<char> = h.chars().collect();
        let v: Option<Vec<f32>> = match ch.len() {
            3 | 4 => ch.iter().map(|c| nib(*c).map(|d| (d * 17.0) / 255.0)).collect(),
            6 | 8 => ch.chunks(2).map(|p| nib(p[0]).zip(nib(p[1])).map(|(a, b)| (a * 16.0 + b) / 255.0)).collect(),
            _ => None,
        };
        return match v {
            Some(v) => Ok([v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0)]),
            None => at.err(format!("`{s}` is not a colour like #ff8800 or #ff880080")),
        };
    }
    match at.j.as_array() {
        Some(a) if a.len() == 3 || a.len() == 4 => {
            let mut c = [0.0, 0.0, 0.0, 1.0];
            for (i, v) in a.iter().enumerate() {
                c[i] = number(&at.indexed("", i, v), 0.0, 1000.0)?;
            }
            Ok(c)
        }
        _ => at.err("must be a colour: \"#rrggbb\" or [r, g, b, a]"),
    }
}

/// Keys `[[t, value], ...]` with `t` from 0 to 1 going up, or one number for a value that never changes.
fn curve(at: &At, lo: f32, hi: f32) -> Result<Gradient<1>, String> {
    if at.j.as_f64().is_some() {
        return Ok(Gradient { keys: vec![(0.0, [number(at, lo, hi)?])] });
    }
    let Some(a) = at.j.as_array() else { return at.err("must be a number or keys [[t, value], ...]") };
    if a.is_empty() || a.len() > MAX_KEYS {
        return at.err(format!("needs 1 to {MAX_KEYS} keys"));
    }
    let mut keys: Vec<(f32, [f32; 1])> = Vec::new();
    for (i, k) in a.iter().enumerate() {
        let ka = at.indexed("", i, k);
        match k.as_array() {
            Some(p) if p.len() == 2 => {
                let t = number(&ka.indexed("", 0, &p[0]), 0.0, 1.0)?;
                let v = number(&ka.indexed("", 1, &p[1]), lo, hi)?;
                if keys.last().is_some_and(|l| t < l.0) {
                    return ka.err("keys must go up in time");
                }
                keys.push((t, [v]));
            }
            _ => return ka.err("a key is a pair [t, value]"),
        }
    }
    Ok(Gradient { keys })
}

/// Keys `[[t, colour], ...]` (see `colour`), or one colour.
fn gradient(at: &At) -> Result<Gradient<4>, String> {
    let single = at.j.as_str().is_some() || at.j.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|v| v.as_f64().is_some()));
    if single {
        return Ok(Gradient { keys: vec![(0.0, colour(at)?)] });
    }
    let Some(a) = at.j.as_array() else { return at.err("must be a colour or keys [[t, colour], ...]") };
    if a.is_empty() || a.len() > MAX_KEYS {
        return at.err(format!("needs 1 to {MAX_KEYS} keys"));
    }
    let mut keys: Vec<(f32, [f32; 4])> = Vec::new();
    for (i, k) in a.iter().enumerate() {
        let ka = at.indexed("", i, k);
        match k.as_array() {
            Some(p) if p.len() == 2 => {
                let t = number(&ka.indexed("", 0, &p[0]), 0.0, 1.0)?;
                let c = colour(&ka.indexed("", 1, &p[1]))?;
                if keys.last().is_some_and(|l| t < l.0) {
                    return ka.err("keys must go up in time");
                }
                keys.push((t, c));
            }
            _ => return ka.err("a key is a pair [t, colour]"),
        }
    }
    Ok(Gradient { keys })
}

fn get_curve(o: &At, items: &[(String, Json)], k: &str, lo: f32, hi: f32) -> Result<Option<Gradient<1>>, String> {
    match member(items, k) {
        Some(j) => curve(&o.child(k, j), lo, hi).map(Some),
        None => Ok(None),
    }
}


/// The members of a list of objects under `k` (none when there is no such key), at most `max` of them.
fn items<'a>(at: &At<'a>, o: &'a [(String, Json)], k: &str, max: usize) -> Result<Vec<At<'a>>, String> {
    match member(o, k) {
        None => Ok(Vec::new()),
        Some(j) => match j.as_array() {
            Some(arr) if arr.len() <= max => Ok(arr.iter().enumerate().map(|(i, v)| at.indexed(k, i, v)).collect()),
            _ => at.child(k, j).err(format!("must be a list of at most {max}")),
        },
    }
}

static EMPTY: Json = Json::Obj(Vec::new());

fn parse_shape(at: &At, cx: &mut Cx) -> Result<Shape, String> {
    if let Some(s) = at.j.as_str() {
        return match s {
            "point" => Ok(Shape::Point),
            "sphere" => Ok(Shape::Sphere { radius: 0.5, shell: false }),
            "box" => Ok(Shape::Box { half: [0.5; 3] }),
            "disc" => Ok(Shape::Disc { radius: 0.5, ring: false }),
            _ => at.err(format!("unknown shape `{s}` (point, sphere, box, disc, line)")),
        };
    }
    let o = at.keys(&["type", "radius", "shell", "half", "ring", "to"], cx)?;
    let ty = get_s(at, o, "type", "point")?;
    match ty.as_str() {
        "point" => Ok(Shape::Point),
        "sphere" => Ok(Shape::Sphere { radius: get_f(at, o, "radius", 0.5, 0.0, 10_000.0)?, shell: get_b(at, o, "shell", false)? }),
        "box" => {
            let h = get_v3(at, o, "half", [0.5; 3])?;
            Ok(Shape::Box { half: [h[0].abs(), h[1].abs(), h[2].abs()] })
        }
        "disc" => Ok(Shape::Disc { radius: get_f(at, o, "radius", 0.5, 0.0, 10_000.0)?, ring: get_b(at, o, "ring", false)? }),
        "line" => Ok(Shape::Line { to: get_v3(at, o, "to", [0.0, 1.0, 0.0])? }),
        _ => at.child("type", &Json::Null).err(format!("unknown shape `{ty}` (point, sphere, box, disc, line)")),
    }
}

fn parse_spawn(at: &At, cx: &mut Cx) -> Result<Spawn, String> {
    let o = at.keys(&["rate", "rate_curve", "bursts", "shape"], cx)?;
    let mut bursts = Vec::new();
    if let Some(j) = member(o, "bursts") {
        let ba = at.child("bursts", j);
        let Some(arr) = j.as_array() else { return ba.err("must be a list of bursts") };
        if arr.len() > 64 {
            return ba.err("at most 64 bursts");
        }
        for (i, b) in arr.iter().enumerate() {
            let a = at.indexed("bursts", i, b);
            let bo = a.keys(&["time", "count", "chance"], cx)?;
            bursts.push(Burst { time: get_f(&a, bo, "time", 0.0, 0.0, MAX_LIFE)?, count: get_urange(&a, bo, "count", [10, 10], MAX_EMITTER)?, chance: get_f(&a, bo, "chance", 1.0, 0.0, 1.0)? });
        }
        bursts.sort_by(|x, y| x.time.total_cmp(&y.time));
    }
    Ok(Spawn {
        rate: get_f(at, o, "rate", 0.0, 0.0, 1.0e6)?,
        rate_curve: get_curve(at, o, "rate_curve", 0.0, 1000.0)?,
        bursts,
        shape: match member(o, "shape") {
            Some(j) => parse_shape(&at.child("shape", j), cx)?,
            None => Shape::Point,
        },
    })
}

fn parse_init(at: &At, cx: &mut Cx) -> Result<Init, String> {
    let o = at.keys(&["life", "speed", "direction", "spread", "radial", "size", "aspect", "rotation", "spin", "color", "color_jitter", "inherit_velocity"], cx)?;
    let life = get_range(at, o, "life", [1.0, 1.0], 0.01, MAX_LIFE)?;
    let direction = get_v3(at, o, "direction", [0.0, 1.0, 0.0])?;
    Ok(Init {
        life,
        speed: get_range(at, o, "speed", [0.0, 0.0], -10_000.0, 10_000.0)?,
        direction,
        spread: get_f(at, o, "spread", 0.0, 0.0, 180.0)?,
        radial: get_f(at, o, "radial", 0.0, 0.0, 1.0)?,
        size: get_range(at, o, "size", [0.2, 0.2], 0.0, 1000.0)?,
        aspect: get_f(at, o, "aspect", 1.0, 0.01, 100.0)?,
        rotation: get_range(at, o, "rotation", [0.0, 0.0], -3600.0, 3600.0)?,
        spin: get_range(at, o, "spin", [0.0, 0.0], -100_000.0, 100_000.0)?,
        color: match member(o, "color") {
            Some(j) => colour(&at.child("color", j))?,
            None => [1.0; 4],
        },
        color_jitter: get_f(at, o, "color_jitter", 0.0, 0.0, 1.0)?,
        inherit_velocity: get_f(at, o, "inherit_velocity", 0.0, 0.0, 1.0)?,
    })
}

fn parse_collide(at: &At, cx: &mut Cx) -> Result<Collide, String> {
    let o = at.keys(&["mode", "bounce", "friction", "life_loss", "planes", "balls", "boxes", "world"], cx)?;
    let hit = match get_s(at, o, "mode", "bounce")?.as_str() {
        "bounce" => Hit::Bounce,
        "kill" => Hit::Kill,
        "stick" => Hit::Stick,
        m => return at.child("mode", &Json::Null).err(format!("unknown mode `{m}` (bounce, kill, stick)")),
    };
    let mut planes = Vec::new();
    for a in items(at, o, "planes", 64)? {
        let po = a.keys(&["normal", "d", "relative"], cx)?;
        let n = get_v3(&a, po, "normal", [0.0, 1.0, 0.0])?;
        let len = length(n);
        if len < 1e-6 {
            return a.err("normal must not be zero");
        }
        planes.push(Plane { normal: [n[0] / len, n[1] / len, n[2] / len], d: get_f(&a, po, "d", 0.0, -1.0e6, 1.0e6)? / len, relative: get_b(&a, po, "relative", false)? });
    }
    let mut balls = Vec::new();
    for a in items(at, o, "balls", 64)? {
        let bo = a.keys(&["pos", "radius", "inside"], cx)?;
        balls.push(Ball { pos: get_v3(&a, bo, "pos", [0.0; 3])?, radius: get_f(&a, bo, "radius", 1.0, 0.001, 1.0e5)?, inside: get_b(&a, bo, "inside", false)? });
    }
    let mut boxes = Vec::new();
    for a in items(at, o, "boxes", 64)? {
        let bo = a.keys(&["pos", "half"], cx)?;
        let h = get_v3(&a, bo, "half", [0.5; 3])?;
        boxes.push(Cuboid { pos: get_v3(&a, bo, "pos", [0.0; 3])?, half: [h[0].abs().max(1e-4), h[1].abs().max(1e-4), h[2].abs().max(1e-4)] });
    }
    Ok(Collide {
        hit,
        bounce: get_f(at, o, "bounce", 0.4, 0.0, 1.0)?,
        friction: get_f(at, o, "friction", 0.2, 0.0, 1.0)?,
        life_loss: get_f(at, o, "life_loss", 0.0, 0.0, 1.0)?,
        planes,
        balls,
        boxes,
        world: get_b(at, o, "world", false)?,
    })
}

fn parse_update(at: &At, cx: &mut Cx) -> Result<Update, String> {
    let o = at.keys(&["gravity", "drag", "wind", "noise", "vortices", "attractors", "speed_limit", "collide"], cx)?;
    let noise = match member(o, "noise") {
        Some(j) => {
            let a = at.child("noise", j);
            let no = a.keys(&["strength", "scale", "speed"], cx)?;
            Some(Noise { strength: get_f(&a, no, "strength", 1.0, 0.0, 10_000.0)?, scale: get_f(&a, no, "scale", 1.0, 0.001, 10_000.0)?, speed: get_f(&a, no, "speed", 0.5, 0.0, 1000.0)? })
        }
        None => None,
    };
    let mut vortices = Vec::new();
    for a in items(at, o, "vortices", 16)? {
        let vo = a.keys(&["pos", "axis", "strength", "pull", "radius"], cx)?;
        let axis = get_v3(&a, vo, "axis", [0.0, 1.0, 0.0])?;
        let len = length(axis);
        if len < 1e-6 {
            return a.err("axis must not be zero");
        }
        vortices.push(Vortex { pos: get_v3(&a, vo, "pos", [0.0; 3])?, axis: [axis[0] / len, axis[1] / len, axis[2] / len], strength: get_f(&a, vo, "strength", 4.0, -10_000.0, 10_000.0)?, pull: get_f(&a, vo, "pull", 0.0, -10_000.0, 10_000.0)?, radius: get_f(&a, vo, "radius", 5.0, 0.01, 1.0e5)? });
    }
    let mut attractors = Vec::new();
    for a in items(at, o, "attractors", 16)? {
        let ao = a.keys(&["pos", "strength", "radius", "falloff", "kill"], cx)?;
        attractors.push(Attractor { pos: get_v3(&a, ao, "pos", [0.0; 3])?, strength: get_f(&a, ao, "strength", 5.0, -10_000.0, 10_000.0)?, radius: get_f(&a, ao, "radius", 5.0, 0.01, 1.0e5)?, falloff: get_f(&a, ao, "falloff", 1.0, 0.0, 8.0)?, kill: get_f(&a, ao, "kill", 0.0, 0.0, 1.0e5)? });
    }
    Ok(Update {
        gravity: get_v3(at, o, "gravity", [0.0; 3])?,
        drag: get_f(at, o, "drag", 0.0, 0.0, 1000.0)?,
        wind: get_v3(at, o, "wind", [0.0; 3])?,
        noise,
        vortices,
        attractors,
        speed_limit: get_f(at, o, "speed_limit", 0.0, 0.0, 1.0e5)?,
        collide: match member(o, "collide") {
            Some(j) => Some(parse_collide(&at.child("collide", j), cx)?),
            None => None,
        },
    })
}

fn parse_curves(at: &At, cx: &mut Cx) -> Result<Curves, String> {
    let o = at.keys(&["size", "speed", "alpha", "color", "light"], cx)?;
    Ok(Curves {
        size: get_curve(at, o, "size", 0.0, 1000.0)?,
        speed: get_curve(at, o, "speed", -1000.0, 1000.0)?,
        alpha: get_curve(at, o, "alpha", 0.0, 1000.0)?,
        color: match member(o, "color") {
            Some(j) => Some(gradient(&at.child("color", j))?),
            None => None,
        },
        light: get_curve(at, o, "light", 0.0, 1000.0)?,
    })
}

fn parse_render(at: &At, cx: &mut Cx) -> Result<Render, String> {
    let o = at.keys(&["type", "texture", "filter", "sheet", "frames", "frame_mode", "fps", "blend", "soft", "facing", "axis", "stretch", "light", "ribbon", "solid", "sort"], cx)?;
    let kind = match get_s(at, o, "type", "sprite")?.as_str() {
        "sprite" => Kind::Sprite,
        "ribbon" => Kind::Ribbon,
        "solid" => Kind::Solid,
        t => return at.child("type", &Json::Null).err(format!("unknown type `{t}` (sprite, ribbon, solid)")),
    };
    let sheet = match member(o, "sheet") {
        Some(j) => {
            let a = at.child("sheet", j);
            match j.as_array() {
                Some(p) if p.len() == 2 && p.iter().all(|v| v.as_f64().is_some_and(|x| x >= 1.0 && x <= 64.0 && x.fract() == 0.0)) => [p[0].as_f64().unwrap_or(1.0) as u32, p[1].as_f64().unwrap_or(1.0) as u32],
                _ => return a.err("must be [columns, rows], each a whole number from 1 to 64"),
            }
        }
        None => [1, 1],
    };
    let frames = get_u(at, o, "frames", sheet[0] * sheet[1], 1, sheet[0] * sheet[1])?;
    let frame_mode = match get_s(at, o, "frame_mode", if member(o, "fps").is_some() { "fps" } else { "life" })?.as_str() {
        "life" => FrameMode::OverLife,
        "random" => FrameMode::Random,
        "fps" => FrameMode::Fps(get_f(at, o, "fps", 12.0, 0.01, 1000.0)?),
        m => return at.child("frame_mode", &Json::Null).err(format!("unknown frame_mode `{m}` (life, random, fps)")),
    };
    let additive = match get_s(at, o, "blend", "alpha")?.as_str() {
        "add" | "additive" => true,
        "alpha" => false,
        b => return at.child("blend", &Json::Null).err(format!("unknown blend `{b}` (alpha, add)")),
    };
    let facing = match get_s(at, o, "facing", "camera")?.as_str() {
        "camera" => Facing::Camera,
        "velocity" => Facing::Velocity,
        "axis" => {
            let a = get_v3(at, o, "axis", [0.0, 1.0, 0.0])?;
            let len = length(a);
            if len < 1e-6 {
                return at.child("axis", &Json::Null).err("must not be zero");
            }
            Facing::Axis([a[0] / len, a[1] / len, a[2] / len])
        }
        f => return at.child("facing", &Json::Null).err(format!("unknown facing `{f}` (camera, velocity, axis)")),
    };
    let light = match member(o, "light") {
        Some(j) => {
            let a = at.child("light", j);
            let lo = a.keys(&["color", "intensity", "range", "max"], cx)?;
            Some(Light {
                color: match member(lo, "color") {
                    Some(c) => {
                        let c = colour(&a.child("color", c))?;
                        Some([c[0] * c[0], c[1] * c[1], c[2] * c[2]])
                    }
                    None => None,
                },
                intensity: get_f(&a, lo, "intensity", 2.0, 0.0, 1.0e5)?,
                range: get_f(&a, lo, "range", 6.0, 0.01, 1.0e5)?,
                max: get_u(&a, lo, "max", 2, 1, 16)?,
            })
        }
        None => None,
    };
    let (mut ribbon_points, mut ribbon_interval, mut ribbon_width) = (8, 0.04, 1.0);
    if let Some(j) = member(o, "ribbon") {
        let a = at.child("ribbon", j);
        let ro = a.keys(&["points", "interval", "width"], cx)?;
        ribbon_points = get_u(&a, ro, "points", 8, 2, 32)?;
        ribbon_interval = get_f(&a, ro, "interval", 0.04, 0.005, 10.0)?;
        ribbon_width = get_f(&a, ro, "width", 1.0, 0.0, 100.0)?;
    }
    let solid = match get_s(at, o, "solid", "cube")?.as_str() {
        "cube" => Solid::Cube,
        "diamond" => Solid::Diamond,
        "tetra" => Solid::Tetra,
        s => return at.child("solid", &Json::Null).err(format!("unknown solid `{s}` (cube, diamond, tetra)")),
    };
    Ok(Render {
        kind,
        texture: get_s(at, o, "texture", "")?,
        nearest: match get_s(at, o, "filter", "linear")?.as_str() {
            "linear" => false,
            "nearest" => true,
            f => return at.child("filter", &Json::Null).err(format!("unknown filter `{f}` (linear, nearest)")),
        },
        sheet,
        frames,
        frame_mode,
        additive,
        soft: get_f(at, o, "soft", 0.0, 0.0, 1000.0)?,
        facing,
        stretch: get_f(at, o, "stretch", 0.05, 0.0, 100.0)?,
        light,
        ribbon_points,
        ribbon_interval,
        ribbon_width,
        solid,
        sort: get_b(at, o, "sort", !additive)?,
    })
}

/// An emitter before the names of its events are turned into numbers.
struct Raw {
    emitter: Emitter,
    events: Vec<(On, String, [u32; 2], f32, f32, f32, String)>,
}

fn parse_emitter(at: &At, cx: &mut Cx) -> Result<Raw, String> {
    let o = at.keys(&["name", "space", "max", "loop", "duration", "delay", "child", "simulate", "spawn", "init", "update", "curves", "render", "events"], cx)?;
    let gpu = match get_s(at, o, "simulate", "cpu")?.as_str() {
        "cpu" => false,
        "gpu" => true,
        s => return at.child("simulate", &Json::Null).err(format!("unknown way to simulate `{s}` (cpu, gpu)")),
    };
    let sub = |k: &str| member(o, k).map(|j| at.child(k, j));
    let space = match get_s(at, o, "space", "local")?.as_str() {
        "local" => Space::Local,
        "world" => Space::World,
        s => return at.child("space", &Json::Null).err(format!("unknown space `{s}` (local, world)")),
    };
    let mut events = Vec::new();
    if let Some(j) = member(o, "events") {
        let ea = at.child("events", j);
        let Some(arr) = j.as_array() else { return ea.err("must be a list of events") };
        if arr.len() > 16 {
            return ea.err("at most 16 events");
        }
        for (i, e) in arr.iter().enumerate() {
            let a = at.indexed("events", i, e);
            let eo = a.keys(&["on", "spawn", "count", "chance", "inherit_velocity", "inherit_color"], cx)?;
            let on = match get_s(&a, eo, "on", "death")?.as_str() {
                "birth" => On::Birth,
                "death" => On::Death,
                "collide" => On::Collide,
                s => return a.child("on", &Json::Null).err(format!("unknown event `{s}` (birth, death, collide)")),
            };
            let target = get_s(&a, eo, "spawn", "")?;
            if target.is_empty() {
                return a.err("`spawn` names the emitter whose particles are born");
            }
            events.push((on, target, get_urange(&a, eo, "count", [1, 1], 1000)?, get_f(&a, eo, "chance", 1.0, 0.0, 1.0)?, get_f(&a, eo, "inherit_velocity", 0.0, 0.0, 1.0)?, get_f(&a, eo, "inherit_color", 0.0, 0.0, 1.0)?, a.path.clone()));
        }
    }
    let child = get_b(at, o, "child", false)?;
    let emitter = Emitter {
        name: get_s(at, o, "name", "")?,
        space,
        max: get_u(at, o, "max", 1000, 1, MAX_EMITTER)?,
        looping: get_b(at, o, "loop", true)?,
        duration: get_f(at, o, "duration", 0.0, 0.0, MAX_LIFE)?,
        delay: get_f(at, o, "delay", 0.0, 0.0, MAX_LIFE)?,
        child,
        gpu,
        spawn: match sub("spawn") {
            Some(a) => parse_spawn(&a, cx)?,
            None => Spawn { rate: 0.0, rate_curve: None, bursts: Vec::new(), shape: Shape::Point },
        },
        init: match sub("init") {
            Some(a) => parse_init(&a, cx)?,
            None => parse_init(&At { j: &EMPTY, path: at.child("init", &EMPTY).path }, cx)?,
        },
        update: match sub("update") {
            Some(a) => parse_update(&a, cx)?,
            None => Update { gravity: [0.0; 3], drag: 0.0, wind: [0.0; 3], noise: None, vortices: Vec::new(), attractors: Vec::new(), speed_limit: 0.0, collide: None },
        },
        curves: match sub("curves") {
            Some(a) => parse_curves(&a, cx)?,
            None => Curves::default(),
        },
        render: match sub("render") {
            Some(a) => parse_render(&a, cx)?,
            None => parse_render(&At { j: &EMPTY, path: at.child("render", &EMPTY).path }, cx)?,
        },
        events: Vec::new(),
    };
    if gpu {
        check_gpu(&emitter, !events.is_empty(), at)?;
    }
    Ok(Raw { emitter, events })
}

/// What an emitter must be to be simulated on the card: the compute pass knows sprites that face the camera, fly along their speed or lie in a plane; the forces of a step (gravity, drag, wind, noise, up to two vortices and two attractors, a speed limit); planes as colliders; curves, flipbooks and the emitter's shapes.
/// It does not know events, ribbons, solids, lights, children, balls, boxes or the world's colliders, or sorting.
fn check_gpu(e: &Emitter, has_events: bool, at: &At) -> Result<(), String> {
    let no = |what: &str| at.child("simulate", &EMPTY).err(format!("`gpu` emitters cannot have {what} (they are stepped on the card): use `\"simulate\": \"cpu\"` for this emitter"));
    if e.render.kind != Kind::Sprite {
        return no("ribbons or solids (they draw sprites only)");
    }
    if has_events || e.child {
        return no("events or be children");
    }
    if e.render.light.is_some() {
        return no("a light for each particle");
    }
    if let Some(c) = &e.update.collide {
        if c.world || !c.balls.is_empty() || !c.boxes.is_empty() {
            return no("colliders other than up to two planes (not balls, boxes or the world)");
        }
        if c.planes.len() > 2 {
            return no("more than two planes");
        }
    }
    if e.update.vortices.len() > 2 || e.update.attractors.len() > 2 {
        return no("more than two vortices or two attractors");
    }
    Ok(())
}

/// Reads and checks an effect. Everything wrong comes back as one message that says where (`emitters[1].update.noise.scale: 0 is outside 0.001 to 10000`).
pub fn parse(text: &str) -> Result<Effect, String> {
    let json = lunaris_json::parse(text).map_err(|e| format!("not JSON: {} (line {}, column {})", e.message, e.line, e.column))?;
    let root = At::root(&json);
    let mut cx = Cx { warnings: Vec::new() };
    let o = root.keys(&["name", "bounds", "budget", "lod", "prewarm", "emitters"], &mut cx)?;
    let (lod_near, lod_far) = match member(o, "lod") {
        Some(j) => {
            let a = root.child("lod", j);
            let lo = a.keys(&["near", "far"], &mut cx)?;
            let (n, f) = (get_f(&a, lo, "near", 0.0, 0.0, 1.0e6)?, get_f(&a, lo, "far", 0.0, 0.0, 1.0e6)?);
            if f > 0.0 && f <= n {
                return a.err("`far` must be more than `near`");
            }
            (n, f)
        }
        None => (0.0, 0.0),
    };
    let Some(ej) = member(o, "emitters") else { return Err("emitters: an effect needs a list of emitters".to_string()) };
    let ea = root.child("emitters", ej);
    let Some(list) = ej.as_array() else { return ea.err("must be a list") };
    if list.is_empty() || list.len() > MAX_EMITTERS {
        return ea.err(format!("needs 1 to {MAX_EMITTERS} emitters"));
    }
    let mut raws = Vec::new();
    for (i, e) in list.iter().enumerate() {
        raws.push(parse_emitter(&root.indexed("emitters", i, e), &mut cx)?);
    }
    // names: an emitter without one is called `emitter<n>`; two with the same name are an error (an event could not tell them apart)
    for (i, r) in raws.iter_mut().enumerate() {
        if r.emitter.name.is_empty() {
            r.emitter.name = format!("emitter{i}");
        }
    }
    for i in 0..raws.len() {
        if let Some(j) = (0..i).find(|&j| raws[j].emitter.name == raws[i].emitter.name) {
            return Err(format!("emitters[{i}].name: `{}` is also the name of emitters[{j}]", raws[i].emitter.name));
        }
    }
    let names: Vec<String> = raws.iter().map(|r| r.emitter.name.clone()).collect();
    let mut emitters = Vec::new();
    for r in raws {
        let mut e = r.emitter;
        for (on, target, count, chance, iv, ic, path) in r.events {
            let Some(spawn) = names.iter().position(|n| *n == target) else {
                let near = names.iter().filter(|n| edit_distance(n, &target) <= 2).min_by_key(|n| edit_distance(n, &target));
                return Err(format!("{path}.spawn: no emitter called `{target}`{}", near.map_or(String::new(), |n| format!(" (did you mean `{n}`?)"))));
            };
            e.events.push(Event { on, spawn, count: [count[0].min(count[1]), count[0].max(count[1])], chance, inherit_velocity: iv, inherit_color: ic });
        }
        emitters.push(e);
    }
    // an event cannot make particles on the card
    for (i, e) in emitters.iter().enumerate() {
        for (k, ev) in e.events.iter().enumerate() {
            if emitters[ev.spawn].gpu {
                return Err(format!("emitters[{i}].events[{k}].spawn: `{}` is simulated on the card (`simulate: gpu`), which events cannot spawn into", emitters[ev.spawn].name));
            }
        }
    }
    // a child with nothing that asks for it never makes a particle: say so
    for (i, e) in emitters.iter().enumerate() {
        if e.child && !emitters.iter().any(|o| o.events.iter().any(|ev| ev.spawn == i)) {
            cx.warnings.push(format!("emitters[{i}] (`{}`) is a child but no event spawns it", e.name));
        }
        if !e.child && e.spawn.rate == 0.0 && e.spawn.bursts.is_empty() && !emitters.iter().any(|o| o.events.iter().any(|ev| ev.spawn == i)) {
            cx.warnings.push(format!("emitters[{i}] (`{}`) has no rate, no bursts and no event that spawns it: it makes nothing", e.name));
        }
    }
    let budget = get_u(&root, o, "budget", 4096, 1, MAX_BUDGET)?;
    Ok(Effect { name: get_s(&root, o, "name", "")?, bounds: get_f(&root, o, "bounds", 0.0, 0.0, 1.0e6)?, budget, lod_near, lod_far, prewarm: get_f(&root, o, "prewarm", 0.0, 0.0, MAX_LIFE)?, emitters, warnings: cx.warnings })
}
