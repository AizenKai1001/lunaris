//! The `render`, `pick` and `project` tools: let an agent see the world.
use crate::base64;
use crate::engine::{hex, uid_json, Args, Engine};
use lunaris_ecs::{EcsError, Result};
use lunaris_json::Json;
use lunaris_render::{raycast_masked, render, RayTargets, ExplicitCamera, Frame, RenderOptions, View};
use lunaris_sim::components::{Name, Transform};

fn vec3(v: &Json, key: &str) -> Result<[f32; 3]> {
    let bad = || EcsError::new("type_mismatch", format!("`{key}` must be an array of three numbers, e.g. [0, 2, 8]")).at(key);
    let a = v.as_array().ok_or_else(bad)?;
    if a.len() != 3 {
        return Err(bad());
    }
    let mut out = [0.0f32; 3];
    for (i, x) in a.iter().enumerate() {
        out[i] = x.as_f64().ok_or_else(bad)? as f32;
    }
    Ok(out)
}

/// Which collision layers a ray may hit: `mask` (a number, bit `n` = layer `n`) or `layers` (a list of layer numbers, 0 to 31); neither means all of them.
fn ray_mask(a: &Args) -> Result<u32> {
    let mask = match a.0.get("mask") {
        None | Some(Json::Null) => None,
        Some(v) => Some(v.as_u64().filter(|m| *m <= u32::MAX as u64).ok_or_else(|| EcsError::new("type_mismatch", "`mask` must be a whole number from 0 to 4294967295 (bit n = layer n)").at("mask").hint("e.g. 6 = layers 1 and 2; or pass `layers: [1, 2]`"))? as u32),
    };
    let layers = match a.0.get("layers") {
        None | Some(Json::Null) => None,
        Some(Json::Arr(list)) => {
            let mut bits = 0u32;
            for (i, v) in list.iter().enumerate() {
                let n = v.as_u64().filter(|n| *n <= 31).ok_or_else(|| EcsError::new("type_mismatch", format!("`layers[{i}]` must be a layer number from 0 to 31")).at("layers"))?;
                bits |= 1u32 << n;
            }
            Some(bits)
        }
        Some(_) => return Err(EcsError::new("type_mismatch", "`layers` must be an array of layer numbers (0 to 31)").at("layers")),
    };
    match (mask, layers) {
        (Some(_), Some(_)) => Err(EcsError::new("bad_arguments", "give `mask` or `layers`, not both").at("mask")),
        (Some(m), None) | (None, Some(m)) => Ok(m),
        (None, None) => Ok(u32::MAX),
    }
}

impl Engine {
    /// Offer a GPU renderer for the `render` tool: the factory starts it the first time a picture wants it, and `default_quality` (one
    /// of `classic`, `low`, `medium`, `high`, `ultra`) is what `render` draws with when the call names no `quality`. Without this the
    /// engine uses only the reference CPU renderer, which is slower but exactly the same on every machine.
    pub fn enable_gpu(&mut self, factory: crate::gpu::GpuFactory, default_quality: &str) {
        self.gpu = crate::gpu::GpuState::Offered(factory);
        self.default_quality = if crate::gpu::QUALITIES.contains(&default_quality) { default_quality.to_string() } else { "high".to_string() };
    }

    /// Start the GPU renderer if it was offered and not started. Returns whether one is ready.
    fn gpu_ready(&mut self) -> bool {
        if let crate::gpu::GpuState::Offered(_) = &self.gpu {
            let crate::gpu::GpuState::Offered(factory) = std::mem::replace(&mut self.gpu, crate::gpu::GpuState::Off) else { unreachable!() };
            self.gpu = match factory() {
                Ok(b) => crate::gpu::GpuState::Ready(b),
                Err(e) => crate::gpu::GpuState::Failed(e),
            };
        }
        matches!(self.gpu, crate::gpu::GpuState::Ready(_))
    }

    /// Build render options from the common view parameters: `width`, `height`, `supersample`, and either `camera` (uid) or
    /// `eye` + `target` (+ `up`, `fov`, `ortho_height`).
    fn view_options(&self, a: &Args, default_w: u32, default_h: u32) -> Result<RenderOptions> {
        let width = a.opt_u64("width")?.unwrap_or(default_w as u64);
        let height = a.opt_u64("height")?.unwrap_or(default_h as u64);
        if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
            return Err(EcsError::new("bad_size", "width and height must be between 1 and 4096").at("width"));
        }
        let supersample = a.opt_u64("supersample")?.unwrap_or(1).clamp(1, 3) as u32;
        let view = match (a.0.get("eye"), a.0.get("camera")) {
            (Some(eye), None) => {
                let mut c = ExplicitCamera { eye: vec3(eye, "eye")?, ..ExplicitCamera::default() };
                if let Some(t) = a.0.get("target") {
                    c.target = vec3(t, "target")?;
                }
                if let Some(u) = a.0.get("up") {
                    c.up = vec3(u, "up")?;
                }
                if let Some(f) = a.0.get("fov").and_then(|v| v.as_f64()) {
                    c.fov_y = f as f32;
                }
                if let Some(h) = a.0.get("ortho_height").and_then(|v| v.as_f64()) {
                    c.ortho_height = Some(h as f32);
                }
                // how far the view reaches (default 1000 m: a real landscape needs more) and how near it starts
                if let Some(f) = a.0.get("far").and_then(|v| v.as_f64()) {
                    c.far = f as f32;
                }
                if let Some(n) = a.0.get("near").and_then(|v| v.as_f64()) {
                    c.near = n as f32;
                }
                View::Explicit(c)
            }
            (Some(_), Some(_)) => return Err(EcsError::new("bad_arguments", "give either `camera` (an entity uid) or `eye`/`target`, not both")),
            (None, cam) => View::World(match cam {
                Some(v) => Some(v.as_u64().ok_or_else(|| EcsError::new("type_mismatch", "`camera` must be an entity uid").at("camera"))?),
                None => None,
            }),
        };
        Ok(RenderOptions { width: width as u32, height: height as u32, view, supersample, threads: self.sim.threads })
    }

    /// Draw the world through its own camera to raw pixels, with this engine's thread count (the native player calls this every frame).
    pub fn render_rgba(&mut self, width: u32, height: u32) -> Result<Frame> {
        let opts = RenderOptions { width, height, view: View::World(None), supersample: 1, threads: self.sim.threads };
        self.share_world();
        render(&mut self.sim.world, &mut self.textures, &opts)
    }

    /// Lets the pictures look at the world as the simulation does (its mesh colliders and terrain: particles that bounce off the world need them).
    pub fn share_world(&mut self) {
        self.textures.share_world(self.sim.meshes.clone(), self.sim.heightfields.clone());
    }

    fn frame_for(&mut self, a: &Args, w: u32, h: u32) -> Result<Frame> {
        let opts = self.view_options(a, w, h)?;
        self.share_world();
        render(&mut self.sim.world, &mut self.textures, &opts)
    }

    fn name_of(&self, uid: u64) -> Option<String> {
        let w = &self.sim.world;
        let e = w.entity(uid)?;
        let n = w.get::<Name>(e)?;
        let s = w.strings().get(n.value.0);
        (!s.is_empty()).then(|| s.to_string())
    }

    /// The picture the `render` tool returns, and what drew it: with the GPU renderer when the call (or the engine's default) asks for a
    /// quality other than `reference` and one is available, else with the CPU reference renderer.
    fn frame_for_tool(&mut self, a: &Args) -> Result<(Frame, String, String)> {
        let asked = match a.opt_str("quality")? {
            None | Some("auto") => self.default_quality.clone(),
            Some(q) if crate::gpu::QUALITIES.contains(&q) => q.to_string(),
            Some(other) => return Err(EcsError::new("bad_arguments", format!("unknown quality `{other}`")).hint("quality is reference (the CPU renderer: identical everywhere), classic, low, medium, high or ultra (the GPU renderer), or auto").at("quality")),
        };
        let opts = self.view_options(a, 640, 360)?;
        self.share_world();
        let mut note = String::new();
        if asked != "reference" {
            if self.gpu_ready() {
                if let crate::gpu::GpuState::Ready(b) = &mut self.gpu {
                    match b.render(&mut self.sim.world, &mut self.textures, &opts, &asked) {
                        Ok(mut frame) => {
                            let name = format!("gpu: {} ({asked})", b.name());
                            frame.warnings.retain(|w| !w.is_empty());
                            return Ok((frame, name, asked));
                        }
                        Err(e) => note = format!("the GPU renderer failed ({e}); drew with the CPU reference renderer"),
                    }
                }
            } else if let crate::gpu::GpuState::Failed(why) = &self.gpu {
                note = format!("no GPU renderer ({why}); drew with the CPU reference renderer");
            } else if a.opt_str("quality")?.is_some() {
                note = "this host has no GPU renderer; drew with the CPU reference renderer".to_string();
            }
        }
        let mut frame = render(&mut self.sim.world, &mut self.textures, &opts)?;
        if !note.is_empty() {
            frame.warnings.push(note);
        }
        Ok((frame, "cpu-reference".to_string(), "reference".to_string()))
    }

    pub(crate) fn render_tool(&mut self, a: &Args) -> Result<Json> {
        let t0 = std::time::Instant::now();
        let (mut frame, renderer, quality) = self.frame_for_tool(a)?;
        // debug overlays: lines over the finished picture
        if let Some(d) = a.0.get("debug") {
            let kinds = crate::debug_draw::parse_kinds(d)?;
            if !kinds.is_empty() {
                self.sim.load_meshes();
                let lines = crate::debug_draw::lines(&mut self.sim.world, &kinds, &self.sim.meshes);
                crate::debug_draw::draw(&mut frame, &lines);
            }
        }
        let t_render = t0.elapsed();
        let limit = a.opt_u64("visible_limit")?.unwrap_or(20).min(500) as usize;
        let t1 = std::time::Instant::now();
        let png = frame.png().map_err(|e| EcsError::new("encode_error", e))?;
        let t_png = t1.elapsed();
        let visible: Vec<Json> = frame
            .visible(limit)
            .into_iter()
            .map(|v| {
                let mut o = Json::obj([("uid", uid_json(v.uid)), ("pixels", Json::from(v.pixels)), ("bbox", Json::arr(v.bbox.iter().map(|&b| Json::from(b))))]);
                if let Some(n) = self.name_of(v.uid) {
                    o.set("name", Json::str(n));
                }
                o
            })
            .collect();
        let mut out = Json::obj([
            ("width", Json::from(frame.width)),
            ("height", Json::from(frame.height)),
            ("hash", hex(frame.hash())),
            ("renderer", Json::str(renderer.clone())),
            ("quality", Json::str(quality.clone())),
            ("deterministic", Json::Bool(quality == "reference")),
            ("camera", Json::str(frame.camera.clone())),
            ("coverage", Json::from((frame.coverage() as f64 * 1000.0).round() / 1000.0)),
            ("visible", Json::Arr(visible)),
            ("warnings", Json::arr(frame.warnings.iter().map(|w| Json::str(w.clone())))),
            ("png_bytes", Json::from(png.len())),
            ("timing_ms", Json::obj([("render", Json::from((t_render.as_secs_f64() * 1e5).round() / 100.0)), ("png", Json::from((t_png.as_secs_f64() * 1e5).round() / 100.0)), ("total", Json::from((t0.elapsed().as_secs_f64() * 1e5).round() / 100.0))])),
        ]);
        // what the GPU renderer measured for this picture (eye adaptation): information about the picture only, never written into the world
        if !frame.info.is_empty() {
            out.set("measured", Json::obj(frame.info.iter().map(|(k, v)| (k.as_str(), Json::from((v * 10000.0).round() / 10000.0)))));
        }
        if let Some(path) = a.opt_str("path")? {
            let p = self.write_bytes(path, &png)?;
            out.set("saved_to", Json::str(p.display().to_string()));
        }
        let want_image = !matches!(a.0.get("image"), Some(Json::Bool(false)));
        if want_image {
            out.set("image", Json::obj([("mime", Json::str("image/png")), ("data", Json::Str(base64::encode(&png)))]));
        }
        Ok(out)
    }

    /// `bake_probe`: capture the world's reflection probes now (the GPU renderer's, docs/GPU.md "Reflection probes"). A picture of the high qualities captures a probe by itself the first time it draws it and when
    /// its position or box changes; this is for after the world around it changed or another scene was loaded. It changes nothing in the world.
    pub(crate) fn bake_probe_tool(&mut self, a: &Args) -> Result<Json> {
        let uid = a.opt_u64("uid")?;
        let mut probes: Vec<u64> = Vec::new();
        self.sim.world.for_each_uid::<(&lunaris_sim::components::ReflectionProbe,), _>(|_, u, _| probes.push(u))?;
        probes.sort();
        if probes.is_empty() {
            return Err(EcsError::new("no_probe", "the world has no ReflectionProbe to capture").hint("add an entity with a Transform and `ReflectionProbe {size, resolution}` in the middle of a room or a cave"));
        }
        if let Some(u) = uid {
            if !probes.contains(&u) {
                return Err(EcsError::new("no_such_probe", format!("entity {u} has no ReflectionProbe")).at("uid").hint(format!("the probes are {probes:?}")));
            }
        }
        if !self.gpu_ready() {
            return Err(EcsError::new("no_gpu", "capturing a reflection probe needs the GPU renderer, and this host has none").hint("only the GPU renderer's high qualities draw probes; the reference renderer ignores them"));
        }
        let crate::gpu::GpuState::Ready(b) = &mut self.gpu else {
            return Err(EcsError::new("no_gpu", "the GPU renderer is not available"));
        };
        let name = b.name();
        match b.bake_probes(&mut self.sim.world, &mut self.textures, uid) {
            Ok(done) => {
                let mut out = Json::obj([("baked", Json::arr(done.iter().map(|u| uid_json(*u)))), ("probes", Json::from(probes.len())), ("renderer", Json::str(format!("gpu: {name}")))]);
                if probes.len() > 4 {
                    out.set("note", Json::str("only the four probes with the lowest uids work at once"));
                }
                Ok(out)
            }
            Err(e) => Err(EcsError::new("bake_failed", e)),
        }
    }

    /// A text picture of the world around a point: a grid of characters a language model can read at a glance.
    pub(crate) fn minimap_tool(&mut self, a: &Args) -> Result<Json> {
        let cells = a.opt_u64("cells")?.unwrap_or(21).clamp(3, 81) as usize;
        let radius = a.0.get("radius").and_then(|v| v.as_f64()).filter(|r| *r > 0.0 && r.is_finite()).unwrap_or(10.0) as f32;
        let side_view = match a.opt_str("view")? {
            None | Some("top") => false,
            Some("side") => true,
            Some(other) => return Err(EcsError::new("bad_arguments", format!("`view` is \"top\" (looking down, up on the map is -z) or \"side\" (up is +y), not `{other}`")).at("view")),
        };
        let around = a.opt_u64("around")?;
        let pos_of = |w: &lunaris_ecs::World, uid: u64| -> Option<[f32; 3]> { w.entity(uid).and_then(|e| w.get::<Transform>(e)).map(|t| t.pos) };
        let center = match (around, a.0.get("center")) {
            (Some(uid), _) => pos_of(&self.sim.world, uid).ok_or_else(|| EcsError::new("no_such_entity", format!("entity {uid} has no Transform")).at("around"))?,
            (None, Some(c)) => vec3(c, "center")?,
            (None, None) => [0.0; 3],
        };
        let with: Vec<_> = a.names("with")?.unwrap_or_default().iter().map(|n| self.sim.world.require_component(n)).collect::<Result<_>>()?;
        // horizontal axis is x; the other axis is z (top view) or y (side view)
        let other = if side_view { 1 } else { 2 };
        let cell = 2.0 * radius / cells as f32;
        let mut found: Vec<(u64, [f32; 3], f32)> = Vec::new();
        {
            let world = &mut self.sim.world;
            let mut raw: Vec<(u64, [f32; 3], f32, lunaris_ecs::Entity)> = Vec::new();
            world.for_each_uid::<(&Transform,), _>(|e, uid, (t,)| raw.push((uid, t.pos, t.scale[0].abs().max(t.scale[1].abs()).max(t.scale[2].abs()), e)))?;
            for (uid, pos, size, e) in raw {
                if with.iter().all(|&c| world.has(e, c)) {
                    found.push((uid, pos, size));
                }
            }
        }
        found.sort_by_key(|f| f.0);
        // symbols per kind of thing: the first word of the name, one letter each
        let mut symbols: Vec<(String, char)> = Vec::new();
        let mut grid = vec![vec!['.'; cells]; cells];
        let mut legend: Vec<(char, String, usize)> = Vec::new();
        let mut listed: Vec<Json> = Vec::new();
        for (uid, pos, size) in &found {
            // a terrain shows as its relief, not as a letter at its middle
            if self.sim.world.entity(*uid).is_some_and(|e| self.sim.world.get::<lunaris_sim::components::Terrain>(e).is_some()) {
                continue;
            }
            // background things (a floor 100 units wide) would cover the whole map
            if *size >= 2.0 * radius && around != Some(*uid) {
                continue;
            }
            let (dx, dy) = (pos[0] - center[0], pos[other] - center[other]);
            let col = ((dx + radius) / cell).floor();
            let row = if side_view { ((radius - dy) / cell).floor() } else { ((dy + radius) / cell).floor() };
            if !(0.0..cells as f32).contains(&col) || !(0.0..cells as f32).contains(&row) {
                continue;
            }
            let name = self.name_of(*uid).unwrap_or_else(|| format!("#{uid}"));
            let group: String = name.split(|c: char| !c.is_alphanumeric()).find(|w| !w.is_empty()).unwrap_or("?").to_string();
            let ch = if around == Some(*uid) {
                '@'
            } else if let Some((_, c)) = symbols.iter().find(|(g, _)| *g == group) {
                *c
            } else {
                let pick = group.chars().chain("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars()).map(|c| c.to_ascii_uppercase()).find(|c| c.is_ascii_alphanumeric() && !symbols.iter().any(|(_, s)| s == c) && *c != '@' && *c != '+').unwrap_or('?');
                symbols.push((group.clone(), pick));
                pick
            };
            let (r, c) = (row as usize, col as usize);
            grid[r][c] = if grid[r][c] == '.' || grid[r][c] == ch { ch } else if grid[r][c] == '@' || ch == '@' { '@' } else { '+' };
            match legend.iter_mut().find(|l| l.0 == ch) {
                Some(l) => l.2 += 1,
                None => legend.push((ch, if ch == '@' { name.clone() } else { group.clone() }, 1)),
            }
            if listed.len() < 40 {
                listed.push(Json::obj([("symbol", Json::str(ch.to_string())), ("uid", uid_json(*uid)), ("name", Json::str(name)), ("pos", Json::arr(pos.iter().map(|&x| Json::from(x))))]));
            }
        }
        // terrain: the empty cells show the ground, as a letter from `a` (lowest) to `j` (highest) in the top view, as `#` under the ground in
        // the side view (which is cut at the centre's z)
        self.sim.load_terrains();
        let ground = lunaris_sim::terrain::grounds(&mut self.sim.world, &self.sim.heightfields);
        let mut relief = Json::Null;
        if !ground.is_empty() {
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for g in &ground {
                let (a, b) = g.placed().y_range();
                lo = lo.min(a);
                hi = hi.max(b);
            }
            let span = (hi - lo).max(1.0e-3);
            let mut any = false;
            for (r, line) in grid.iter_mut().enumerate() {
                for (c, slot) in line.iter_mut().enumerate() {
                    if *slot != '.' {
                        continue;
                    }
                    let x = center[0] + (c as f32 + 0.5) * cell - radius;
                    if side_view {
                        let (z, y) = (center[2], center[1] + radius - (r as f32 + 0.5) * cell);
                        if ground.iter().filter_map(|g| g.placed().surface(x, z)).any(|h| h >= y) {
                            *slot = '#';
                            any = true;
                        }
                    } else {
                        let z = center[2] + (r as f32 + 0.5) * cell - radius;
                        if let Some(h) = ground.iter().filter_map(|g| g.placed().surface(x, z)).reduce(f32::max) {
                            *slot = (b'a' + (((h - lo) / span * 10.0).floor().clamp(0.0, 9.0)) as u8) as char;
                            any = true;
                        }
                    }
                }
            }
            if any {
                relief = if side_view {
                    Json::obj([("ground", Json::str("# is ground (terrain) under the line of sight at the centre's z"))])
                } else {
                    Json::obj([("letters", Json::str("a (lowest) to j (highest)")), ("low", Json::from(lo as f64)), ("high", Json::from(hi as f64)), ("step", Json::from((span / 10.0) as f64))])
                };
            }
        }
        let rows: Vec<Json> = grid.iter().map(|r| Json::str(r.iter().collect::<String>())).collect();
        Ok(Json::obj([
            ("relief", relief),
            ("rows", Json::Arr(rows)),
            ("cell_size", Json::from(cell as f64)),
            ("center", Json::arr(center.iter().map(|&x| Json::from(x)))),
            ("radius", Json::from(radius as f64)),
            ("right", Json::str("+x")),
            ("up", Json::str(if side_view { "+y" } else { "-z" })),
            ("legend", Json::Obj(legend.into_iter().map(|(c, g, n)| (c.to_string(), Json::str(format!("{g} x{n}")))).collect())),
            ("entities", Json::Arr(listed)),
        ]))
    }

    pub(crate) fn pick_tool(&mut self, a: &Args) -> Result<Json> {
        let x = a.u64("x")? as u32;
        let y = a.u64("y")? as u32;
        let frame = self.frame_for(a, 640, 360)?;
        if x >= frame.width || y >= frame.height {
            return Err(EcsError::new("out_of_range", format!("pixel ({x}, {y}) is outside the {}x{} image", frame.width, frame.height)).at("x"));
        }
        let mut out = Json::obj([("x", Json::from(x)), ("y", Json::from(y))]);
        match frame.pick(x, y) {
            Some(uid) => {
                out.set("uid", uid_json(uid));
                if let Some(n) = self.name_of(uid) {
                    out.set("name", Json::str(n));
                }
                out.set("depth_ndc", Json::from(frame.depth[(y * frame.width + x) as usize] as f64));
            }
            None => out.set("uid", Json::Null),
        }
        Ok(out)
    }

    pub(crate) fn project_tool(&mut self, a: &Args) -> Result<Json> {
        let point = vec3(a.value("point")?, "point")?;
        let frame = self.frame_for(a, 640, 360)?;
        match frame.world_to_pixel(point) {
            Some(p) => Ok(Json::obj([("x", Json::from(p[0] as f64)), ("y", Json::from(p[1] as f64)), ("on_screen", Json::Bool(p[0] >= 0.0 && p[1] >= 0.0 && p[0] < frame.width as f32 && p[1] < frame.height as f32))])),
            None => Ok(Json::obj([("x", Json::Null), ("y", Json::Null), ("on_screen", Json::Bool(false)), ("note", Json::str("the point is behind the camera"))])),
        }
    }

    pub(crate) fn contacts_tool(&mut self, a: &Args) -> Result<Json> {
        let limit = a.opt_u64("limit")?.unwrap_or(100).min(10_000) as usize;
        let total = self.sim.contacts.len();
        let list: Vec<Json> = self
            .sim
            .contacts
            .iter()
            .take(limit)
            .map(|c| {
                let mut o = Json::obj([
                    ("a", uid_json(c.a)),
                    ("b", uid_json(c.b)),
                    ("normal", Json::arr(c.normal.iter().map(|&x| Json::from(x)))),
                    ("depth", Json::from(c.depth as f64)),
                    ("trigger", Json::Bool(c.trigger)),
                ]);
                if let Some(n) = self.name_of(c.a) {
                    o.set("a_name", Json::str(n));
                }
                if let Some(n) = self.name_of(c.b) {
                    o.set("b_name", Json::str(n));
                }
                o
            })
            .collect();
        Ok(Json::obj([("tick", Json::from(self.sim.tick as i64)), ("total", Json::from(total)), ("contacts", Json::Arr(list))]))
    }

    pub(crate) fn raycast_tool(&mut self, a: &Args) -> Result<Json> {
        let origin = vec3(a.value("origin")?, "origin")?;
        let direction = vec3(a.value("direction")?, "direction")?;
        let max = a.0.get("max_distance").and_then(|v| v.as_f64()).unwrap_or(1000.0) as f32;
        let targets = match a.opt_str("include")? {
            None | Some("both") => RayTargets::Both,
            Some("visuals") => RayTargets::Visuals,
            Some("colliders") => RayTargets::Colliders,
            Some(other) => return Err(EcsError::new("bad_arguments", format!("`include` must be visuals, colliders or both, not `{other}`")).at("include")),
        };
        let mask = ray_mask(a)?;
        // (the level geometry of mesh colliders is the simulation's own copy of the model)
        self.sim.load_meshes();
        self.textures.share_collision_meshes(self.sim.meshes.clone());
        match raycast_masked(&mut self.sim.world, &mut self.textures, origin, direction, max, targets, mask)? {
            Some(h) => {
                let mut o = Json::obj([
                    ("hit", Json::Bool(true)),
                    ("uid", uid_json(h.uid)),
                    ("distance", Json::from(h.distance as f64)),
                    ("point", Json::arr(h.point.iter().map(|&x| Json::from(x)))),
                    ("normal", Json::arr(h.normal.iter().map(|&x| Json::from(x)))),
                ]);
                if let Some(n) = self.name_of(h.uid) {
                    o.set("name", Json::str(n));
                }
                Ok(o)
            }
            None => Ok(Json::obj([("hit", Json::Bool(false)), ("uid", Json::Null)])),
        }
    }

    pub(crate) fn nearby_tool(&mut self, a: &Args) -> Result<Json> {
        let c = vec3(a.value("point")?, "point")?;
        let radius = a.0.get("radius").and_then(|v| v.as_f64()).filter(|r| *r >= 0.0 && r.is_finite()).ok_or_else(|| EcsError::new("type_mismatch", "`radius` must be a non-negative number").at("radius"))? as f32;
        let limit = a.opt_u64("limit")?.unwrap_or(50).min(1000) as usize;
        let with: Vec<_> = a.names("with")?.unwrap_or_default().iter().map(|n| self.sim.world.require_component(n)).collect::<Result<_>>()?;
        let r2 = radius * radius;
        let mut found: Vec<(f32, u64, [f32; 3])> = Vec::new();
        {
            let world = &mut self.sim.world;
            let mut candidates: Vec<(u64, [f32; 3], lunaris_ecs::Entity)> = Vec::new();
            world.for_each_uid::<(&Transform,), _>(|e, uid, (t,)| {
                let d = [t.pos[0] - c[0], t.pos[1] - c[1], t.pos[2] - c[2]];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= r2 {
                    candidates.push((uid, t.pos, e));
                }
            })?;
            for (uid, pos, e) in candidates {
                if with.iter().all(|&cid| world.has(e, cid)) {
                    let d = [pos[0] - c[0], pos[1] - c[1], pos[2] - c[2]];
                    found.push(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt(), uid, pos));
                }
            }
        }
        found.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal).then(x.1.cmp(&y.1)));
        let total = found.len();
        let out: Vec<Json> = found
            .into_iter()
            .take(limit)
            .map(|(d, uid, pos)| {
                let mut o = Json::obj([("uid", uid_json(uid)), ("distance", Json::from(d as f64)), ("pos", Json::arr(pos.iter().map(|&x| Json::from(x))))]);
                if let Some(n) = self.name_of(uid) {
                    o.set("name", Json::str(n));
                }
                o
            })
            .collect();
        Ok(Json::obj([("total", Json::from(total)), ("entities", Json::Arr(out))]))
    }
}
