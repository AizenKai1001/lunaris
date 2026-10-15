//! Particle modules, the Niagara-style part of a `Particles` cloud: what happens to each particle over its life and in the world, written as text on a
//! `ParticleModules` component beside the `Particles` it belongs to. One module a line, `#` starts a comment:
//!
//! ```text
//! color 0:1,0.9,0.5,0  0.1:1,0.7,0.2,1  0.6:1,0.3,0.1,0.7  1:0.2,0.1,0.1,0   # colour and opacity over the life (0 = born, 1 = gone); display colours
//! size 0:0.1  0.3:0.5  1:0.05                                                   # size in metres over the life
//! wind 1,0,0 gust=0.5 freq=0.7                  # a steady push in m/s^2, swelling and fading by `gust` (0..1) about `freq` times a second
//! attract 0,3,0 strength=6 radius=8 falloff=2   # pulled to a point inside `radius` (strength below 0 pushes away); the pull fades with distance^falloff
//! vortex 0,0,0 axis=0,1,0 strength=4 radius=6 pull=1   # swirled round an axis through the point, and drawn in by `pull`
//! noise strength=2 scale=0.7 speed=0.5          # a wandering force field (3 noise lookups a step: dear)
//! collide plane 0,1,0 d=0 bounce=0.4 friction=0.3        # a floor (dot(normal, p) = d); `kill` removes the particle at the hit, `stick` leaves it there
//! collide sphere 0,1,0 r=1 bounce=0.3 inside             # a ball (`inside` keeps particles in it)
//! trail segments=8 time=0.4 width=0.6           # a ribbon behind each particle: `segments` pieces covering the last `time` seconds, `width` times the size
//! mesh cube                                     # a small solid (cube, diamond, tetra) instead of a flat picture
//! ```
//!
//! With no force or collider a cloud keeps its closed form (a function of its clock, as cheap as before). With them each particle is integrated from its birth with
//! a fixed step (1/30 s, or 1/48 of the life when that is longer), so it is still a pure function of the clock and the same on every machine, only dearer: a cloud
//! of 60,000 with forces costs a few tens of milliseconds a frame on one thread, which the threads divide.
use crate::noise::noise2;

type V3 = [f32; 3];

/// Keys `(position along the life 0..1, value)` joined by straight lines; before the first key and after the last one the value holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient<const N: usize> {
    pub keys: Vec<(f32, [f32; N])>,
}

impl<const N: usize> Gradient<N> {
    pub fn at(&self, u: f32) -> [f32; N] {
        let k = &self.keys;
        if u <= k[0].0 {
            return k[0].1;
        }
        for w in k.windows(2) {
            if u <= w[1].0 {
                let t = (u - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
                let mut out = [0.0; N];
                for (i, o) in out.iter_mut().enumerate() {
                    *o = w[0].1[i] + (w[1].1[i] - w[0].1[i]) * t;
                }
                return out;
            }
        }
        k[k.len() - 1].1
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Force {
    Wind { dir: V3, gust: f32, freq: f32 },
    Attract { pos: V3, strength: f32, radius: f32, falloff: f32 },
    Vortex { pos: V3, axis: V3, strength: f32, radius: f32, pull: f32 },
    Noise { strength: f32, scale: f32, speed: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Bounce,
    Kill,
    Stick,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Plane { normal: V3, d: f32 },
    Sphere { pos: V3, r: f32, inside: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Collide {
    pub shape: Shape,
    pub bounce: f32,
    pub friction: f32,
    pub hit: Hit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trail {
    pub segments: usize,
    pub time: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solid {
    Cube,
    Diamond,
    Tetra,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Modules {
    pub color: Option<Gradient<4>>,
    pub size: Option<Gradient<1>>,
    pub forces: Vec<Force>,
    pub colliders: Vec<Collide>,
    pub trail: Option<Trail>,
    pub solid: Option<Solid>,
}

impl Modules {
    /// Whether the particles have to be integrated step by step (a force or a collider) rather than worked out in closed form.
    pub fn dynamic(&self) -> bool {
        !self.forces.is_empty() || !self.colliders.is_empty()
    }

    /// The acceleration the forces give a particle at `pos` at the clock time `t`.
    pub fn acceleration(&self, pos: V3, t: f32, seed: u32) -> V3 {
        let mut a = [0.0f32; 3];
        for f in &self.forces {
            match f {
                Force::Wind { dir, gust, freq } => {
                    let k = if *gust > 0.0 { 1.0 + gust * noise2((t * freq) as f64, 0.37, seed as f64) as f32 } else { 1.0 };
                    for i in 0..3 {
                        a[i] += dir[i] * k;
                    }
                }
                Force::Attract { pos: c, strength, radius, falloff } => {
                    let d = [c[0] - pos[0], c[1] - pos[1], c[2] - pos[2]];
                    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if len < *radius && len > 1e-4 {
                        let w = strength * pow_fall(1.0 - len / radius, *falloff) / len;
                        for i in 0..3 {
                            a[i] += d[i] * w;
                        }
                    }
                }
                Force::Vortex { pos: c, axis, strength, radius, pull } => {
                    let d = [pos[0] - c[0], pos[1] - c[1], pos[2] - c[2]];
                    // the part of the offset across the axis
                    let along = d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2];
                    let across = [d[0] - axis[0] * along, d[1] - axis[1] * along, d[2] - axis[2] * along];
                    let len = (across[0] * across[0] + across[1] * across[1] + across[2] * across[2]).sqrt();
                    if len < *radius && len > 1e-4 {
                        let fade = 1.0 - len / radius;
                        // the way round: axis x across, as a unit vector
                        let tang = [axis[1] * across[2] - axis[2] * across[1], axis[2] * across[0] - axis[0] * across[2], axis[0] * across[1] - axis[1] * across[0]];
                        for i in 0..3 {
                            a[i] += (tang[i] * strength - across[i] * pull) * fade / len;
                        }
                    }
                }
                Force::Noise { strength, scale, speed } => {
                    let (x, y, z, w) = ((pos[0] * scale) as f64, (pos[1] * scale) as f64, (pos[2] * scale) as f64, (t * speed) as f64);
                    let s = seed as f64;
                    a[0] += strength * noise2(x + w, y, s + 11.0) as f32;
                    a[1] += strength * noise2(y + w, z, s + 12.0) as f32;
                    a[2] += strength * noise2(z + w, x, s + 13.0) as f32;
                }
            }
        }
        a
    }
}

fn pow_fall(x: f32, p: f32) -> f32 {
    // x in 0..1: a whole or half power is enough, cheap and the same everywhere
    let x = x.clamp(0.0, 1.0);
    if p <= 1.0 {
        x
    } else if p <= 2.0 {
        x * x
    } else if p <= 3.0 {
        x * x * x
    } else {
        x * x * x * x
    }
}

fn number(s: &str, line: usize, what: &str) -> Result<f32, String> {
    s.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| format!("line {line}: `{s}` is not a number for {what}"))
}

fn vec3(s: &str, line: usize, what: &str) -> Result<V3, String> {
    let p: Vec<&str> = s.split(',').collect();
    if p.len() != 3 {
        return Err(format!("line {line}: {what} is three numbers with commas, like `0,1,0`, not `{s}`"));
    }
    Ok([number(p[0], line, what)?, number(p[1], line, what)?, number(p[2], line, what)?])
}

fn unit(v: V3, line: usize, what: &str) -> Result<V3, String> {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 {
        return Err(format!("line {line}: {what} must not be all zeros"));
    }
    Ok([v[0] / l, v[1] / l, v[2] / l])
}

fn keys<const N: usize>(words: &[&str], line: usize, what: &str) -> Result<Gradient<N>, String> {
    let mut out: Vec<(f32, [f32; N])> = Vec::new();
    for w in words {
        let (u, v) = w.split_once(':').ok_or_else(|| format!("line {line}: `{w}` is not `POSITION:VALUE` (the position runs from 0 to 1 over the life)"))?;
        let u = number(u, line, what)?;
        if !(0.0..=1.0).contains(&u) || out.last().is_some_and(|k| u <= k.0) {
            return Err(format!("line {line}: positions must increase from 0 to 1 (at `{w}`)"));
        }
        let parts: Vec<&str> = v.split(',').collect();
        if parts.len() != N {
            return Err(format!("line {line}: `{w}` needs {N} number{} after the colon", if N == 1 { "" } else { "s, comma separated" }));
        }
        let mut val = [0.0; N];
        for (i, p) in parts.iter().enumerate() {
            val[i] = number(p, line, what)?;
        }
        out.push((u, val));
    }
    if out.len() < 2 {
        return Err(format!("line {line}: {what} needs at least two keys, like `0:... 1:...`"));
    }
    if out.len() > 16 {
        return Err(format!("line {line}: {what} has at most 16 keys"));
    }
    Ok(Gradient { keys: out })
}

/// `name=value` options after the first words of a line.
fn options<'a>(words: &[&'a str]) -> Vec<(&'a str, &'a str)> {
    words.iter().filter_map(|w| w.split_once('=')).collect()
}

fn opt(o: &[(&str, &str)], key: &str, default: f32, line: usize) -> Result<f32, String> {
    match o.iter().find(|(k, _)| *k == key) {
        Some((_, v)) => number(v, line, key),
        None => Ok(default),
    }
}

/// The text of a `ParticleModules` as modules; the error names the line.
pub fn parse(text: &str) -> Result<Modules, String> {
    let mut m = Modules::default();
    for (n, raw) in text.lines().enumerate() {
        let line = n + 1;
        let raw = raw.split('#').next().unwrap_or("").trim();
        if raw.is_empty() {
            continue;
        }
        let words: Vec<&str> = raw.split_whitespace().collect();
        let rest = &words[1..];
        let o = options(rest);
        // the words that are not `name=value`
        let plain: Vec<&str> = rest.iter().copied().filter(|w| !w.contains('=') || w.contains(':')).collect();
        match words[0] {
            "color" => m.color = Some(keys::<4>(&rest.to_vec(), line, "color")?),
            "size" => {
                let g = keys::<1>(&rest.to_vec(), line, "size")?;
                if g.keys.iter().any(|k| k.1[0] < 0.0) {
                    return Err(format!("line {line}: a size cannot be negative"));
                }
                m.size = Some(g);
            }
            "wind" => {
                let dir = vec3(plain.first().ok_or_else(|| format!("line {line}: wind needs a direction like `wind 1,0,0`"))?, line, "the wind")?;
                m.forces.push(Force::Wind { dir, gust: opt(&o, "gust", 0.0, line)?.clamp(0.0, 1.0), freq: opt(&o, "freq", 0.5, line)?.max(0.0) });
            }
            "attract" => {
                let pos = vec3(plain.first().ok_or_else(|| format!("line {line}: attract needs a point like `attract 0,3,0 strength=6`"))?, line, "the point")?;
                m.forces.push(Force::Attract { pos, strength: opt(&o, "strength", 5.0, line)?, radius: opt(&o, "radius", 5.0, line)?.max(0.01), falloff: opt(&o, "falloff", 1.0, line)?.max(0.0) });
            }
            "vortex" => {
                let pos = vec3(plain.first().ok_or_else(|| format!("line {line}: vortex needs a point like `vortex 0,0,0 axis=0,1,0 strength=4`"))?, line, "the point")?;
                let axis = match o.iter().find(|(k, _)| *k == "axis") {
                    Some((_, v)) => unit(vec3(v, line, "axis")?, line, "axis")?,
                    None => [0.0, 1.0, 0.0],
                };
                m.forces.push(Force::Vortex { pos, axis, strength: opt(&o, "strength", 4.0, line)?, radius: opt(&o, "radius", 5.0, line)?.max(0.01), pull: opt(&o, "pull", 0.0, line)? });
            }
            "noise" => m.forces.push(Force::Noise { strength: opt(&o, "strength", 2.0, line)?, scale: opt(&o, "scale", 0.7, line)?, speed: opt(&o, "speed", 0.5, line)? }),
            "collide" => {
                let kind = *plain.first().ok_or_else(|| format!("line {line}: collide needs `plane` or `sphere`"))?;
                let hit = if rest.contains(&"kill") {
                    Hit::Kill
                } else if rest.contains(&"stick") {
                    Hit::Stick
                } else {
                    Hit::Bounce
                };
                let (bounce, friction) = (opt(&o, "bounce", 0.4, line)?.clamp(0.0, 1.0), opt(&o, "friction", 0.2, line)?.clamp(0.0, 1.0));
                let shape = match kind {
                    "plane" => {
                        let normal = unit(vec3(plain.get(1).ok_or_else(|| format!("line {line}: a plane needs its normal, like `collide plane 0,1,0 d=0`"))?, line, "the plane's normal")?, line, "the plane's normal")?;
                        Shape::Plane { normal, d: opt(&o, "d", 0.0, line)? }
                    }
                    "sphere" => {
                        let pos = vec3(plain.get(1).ok_or_else(|| format!("line {line}: a sphere needs its centre, like `collide sphere 0,1,0 r=1`"))?, line, "the sphere's centre")?;
                        Shape::Sphere { pos, r: opt(&o, "r", 1.0, line)?.max(0.01), inside: rest.contains(&"inside") }
                    }
                    other => return Err(format!("line {line}: `{other}` is not a collider: use `plane` or `sphere`")),
                };
                if m.colliders.len() >= 8 {
                    return Err(format!("line {line}: at most 8 colliders"));
                }
                m.colliders.push(Collide { shape, bounce, friction, hit });
            }
            "trail" => {
                let segments = opt(&o, "segments", 8.0, line)?;
                if !(2.0..=16.0).contains(&segments) {
                    return Err(format!("line {line}: a trail has 2 to 16 segments"));
                }
                m.trail = Some(Trail { segments: segments as usize, time: opt(&o, "time", 0.4, line)?.clamp(0.02, 10.0), width: opt(&o, "width", 0.6, line)?.clamp(0.0, 10.0) });
            }
            "mesh" => {
                m.solid = Some(match plain.first().copied() {
                    Some("cube") => Solid::Cube,
                    Some("diamond") => Solid::Diamond,
                    Some("tetra") => Solid::Tetra,
                    other => return Err(format!("line {line}: mesh is `cube`, `diamond` or `tetra`, not `{}`", other.unwrap_or(""))),
                });
            }
            other => return Err(format!("line {line}: `{other}` is not a module: color, size, wind, attract, vortex, noise, collide, trail, mesh")),
        }
    }
    if m.forces.len() > 16 {
        return Err("at most 16 forces".to_string());
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &str = "color 0:1,0.9,0.5,0 0.1:1,0.7,0.2,1 1:0.2,0.1,0.1,0\nsize 0:0.1 0.3:0.5 1:0.05\nwind 1,0,0 gust=0.5 freq=2\nattract 0,3,0 strength=6 radius=8 falloff=2\nvortex 0,0,0 axis=0,2,0 strength=4 radius=6 pull=1\nnoise strength=2 scale=1 speed=0.1\ncollide plane 0,1,0 d=0 bounce=0.4 friction=0.3 stick\ncollide sphere 0,1,0 r=1 inside kill\ntrail segments=6 time=0.5 width=0.4\nmesh diamond # a comment";

    #[test]
    fn the_whole_language_reads() {
        let m = parse(ALL).unwrap();
        assert_eq!(m.color.as_ref().unwrap().keys.len(), 3);
        assert_eq!(m.size.as_ref().unwrap().at(0.3), [0.5]);
        assert_eq!(m.forces.len(), 4);
        assert!(matches!(m.forces[2], Force::Vortex { axis, .. } if axis == [0.0, 1.0, 0.0]), "the axis is made a unit vector");
        assert_eq!((m.colliders[0].hit, m.colliders[1].hit), (Hit::Stick, Hit::Kill));
        assert!(matches!(m.colliders[1].shape, Shape::Sphere { inside: true, .. }));
        assert_eq!(m.trail, Some(Trail { segments: 6, time: 0.5, width: 0.4 }));
        assert_eq!(m.solid, Some(Solid::Diamond));
        assert!(m.dynamic());
        assert!(!parse("color 0:1,1,1,1 1:0,0,0,0\nsize 0:1 1:2").unwrap().dynamic());
        assert_eq!(parse("").unwrap(), Modules::default());
    }

    #[test]
    fn mistakes_are_named_with_their_line() {
        for (bad, why) in [
            ("color 0:1,1,1", "number"),
            ("color 0:1,1,1,1", "at least two keys"),
            ("color 0.5:1,1,1,1 0.2:0,0,0,0", "increase"),
            ("size 0:-1 1:1", "negative"),
            ("wind", "direction"),
            ("wind 1,2", "three numbers"),
            ("vortex 0,0,0 axis=0,0,0", "zeros"),
            ("collide", "plane"),
            ("collide cone 0,0,0", "not a collider"),
            ("collide plane 0,0,0", "zeros"),
            ("trail segments=1", "2 to 16"),
            ("mesh ball", "cube"),
            ("sparkle 1", "not a module"),
            ("attract 0,0,0 strength=lots", "not a number"),
        ] {
            let e = parse(&format!("size 0:1 1:1\n{bad}")).unwrap_err();
            assert!(e.starts_with("line 2") && e.contains(why), "{bad}: {e}");
        }
    }

    #[test]
    fn gradients_join_their_keys_and_hold_beyond_them() {
        let g = Gradient::<4> { keys: vec![(0.2, [0.0, 0.0, 0.0, 0.0]), (0.6, [1.0, 0.5, 0.0, 1.0])] };
        assert_eq!(g.at(0.0), [0.0; 4]);
        let mid = g.at(0.4);
        assert!((mid[0] - 0.5).abs() < 1e-5 && (mid[1] - 0.25).abs() < 1e-5 && mid[2] == 0.0 && (mid[3] - 0.5).abs() < 1e-5, "{mid:?}");
        assert_eq!(g.at(0.9), [1.0, 0.5, 0.0, 1.0]);
    }

    #[test]
    fn forces_push_the_way_they_should() {
        let m = parse("wind 2,0,0\nattract 0,5,0 strength=4 radius=10").unwrap();
        // at the origin: the wind blows along x, the attractor pulls up (toward y 5) with 4 * (1 - 5/10) = 2
        let a = m.acceleration([0.0, 0.0, 0.0], 0.0, 1);
        assert!((a[0] - 2.0).abs() < 1e-6 && (a[1] - 2.0).abs() < 1e-6 && a[2].abs() < 1e-6, "{a:?}");
        // outside the radius nothing pulls, and a negative strength pushes away
        assert_eq!(m.acceleration([0.0, -20.0, 0.0], 0.0, 1)[1], 0.0);
        let away = parse("attract 0,5,0 strength=-4 radius=10").unwrap().acceleration([0.0, 0.0, 0.0], 0.0, 1);
        assert!(away[1] < 0.0);
        // a vortex round y at the point (1, 0, 0) pushes sideways (along z), and `pull` brings it in
        let v = parse("vortex 0,0,0 axis=0,1,0 strength=3 radius=4 pull=1").unwrap().acceleration([1.0, 0.0, 0.0], 0.0, 1);
        assert!(v[0] < 0.0 && v[2].abs() > 1.0 && v[1] == 0.0, "{v:?}");
        // gusts make the wind swell and fade, never reverse it
        let g = parse("wind 1,0,0 gust=0.8 freq=1").unwrap();
        let mut seen = (f32::MAX, f32::MIN);
        for k in 0..200 {
            let a = g.acceleration([0.0; 3], k as f32 * 0.07, 1)[0];
            seen = (seen.0.min(a), seen.1.max(a));
        }
        assert!(seen.0 >= 0.19 && seen.1 <= 1.81 && seen.1 - seen.0 > 0.3, "{seen:?}");
    }
}
