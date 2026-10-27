//! The `check` tool: a static look at the whole world for mistakes that would otherwise show up as a blank picture or a silent
//! zero. It is for agents (and people) that have just built something: run it, fix what it lists, run it again.
use crate::engine::{uid_json, Args, Engine};
use lunaris_ecs::Result;
use lunaris_json::Json;
use lunaris_render::mesh::builtin;
use lunaris_sim::components::{Animator, Camera, Collider, LocalTransform, MeshRenderer, Parent, Sound, Sprite, Text, Tilemap, Transform};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

mod draw_order;
mod language;
mod layers;
mod menus;
mod render_fx;
mod terrain_layers;
mod ui_kit3;
mod motion;
mod physics_joints;
mod vehicle;
mod water;

struct Problems(Vec<Json>);

impl Problems {
    fn add(&mut self, severity: &str, uid: Option<u64>, message: String, hint: &str) {
        let mut o = Json::obj([("severity", Json::str(severity)), ("message", Json::Str(message))]);
        if let Some(u) = uid {
            o.set("entity", uid_json(u));
        }
        if !hint.is_empty() {
            o.set("hint", Json::str(hint));
        }
        self.0.push(o);
    }
}

/// Every `{"res_set"|"res_add": "name"}` in a system definition.
fn written_resources(def: &Json, out: &mut BTreeSet<String>) {
    match def {
        Json::Obj(pairs) => {
            for (k, v) in pairs {
                if (k == "res_set" || k == "res_add") && v.as_str().is_some() {
                    out.insert(v.as_str().unwrap_or("").to_string());
                }
                written_resources(v, out);
            }
        }
        Json::Arr(items) => items.iter().for_each(|i| written_resources(i, out)),
        _ => {}
    }
}

/// The `{name}` placeholders of a text template.
fn placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if lunaris_ecs::valid_resource_name(&after[..close]) => {
                out.push(after[..close].to_string());
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// How much bigger, seen from above, is the axis-aligned box round a turned box than the box itself (1 = not at all)? `rot` is a quaternion (x, y, z, w), `half` the half sizes.
/// A box turned by a multiple of 90 degrees is its own axis-aligned box (1); a square post turned by 45 degrees is twice as big (2); a long thin wall turned a little is much more.
fn turned_growth(rot: [f32; 4], half: [f32; 3]) -> f32 {
    let n = (rot[0] * rot[0] + rot[1] * rot[1] + rot[2] * rot[2] + rot[3] * rot[3]).sqrt();
    if !n.is_finite() || n < 1.0e-6 {
        return 1.0;
    }
    let (x, y, z, w) = (rot[0] / n, rot[1] / n, rot[2] / n, rot[3] / n);
    let m = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    if m.iter().all(|row| row.iter().any(|v| v.abs() > 0.998)) {
        return 1.0;
    }
    let (hx, hz) = ((m[0][0].abs() * half[0] + m[0][1].abs() * half[1] + m[0][2].abs() * half[2]), (m[2][0].abs() * half[0] + m[2][1].abs() * half[1] + m[2][2].abs() * half[2]));
    let own = (half[0] * half[2]).max(1.0e-6);
    hx * hz / own
}

fn file_exists(root: &Path, rel: &str) -> bool {
    lunaris_json::safe_relative(rel).is_ok_and(|norm| root.join(norm).is_file())
}

/// How many cells round one anchor a `ProcGrid` may look at before `check` says so.
const MAX_GRID_WINDOW: f32 = 2500.0;

/// How many `StreamZone`s a scene may have, with no `ProcGrid`, before `check` asks for `stream.loads_per_tick`.
const STREAM_ZONES_UNLIMITED: usize = 24;

/// How many `NavAgent`s make a crowd that the checks on navigation limits are about.
/// About how many triangles the CPU reference renderer holds at 30 pictures a second on one core (960 x 540, `lunaris bench` on an i7-9700K: 359,040 triangles in 40 ms).
const CPU_TRIANGLES_ADVICE: u64 = 300_000;
const CROWD_AGENTS: usize = 20;

impl Engine {
    pub(crate) fn check_tool(&mut self, _a: &Args) -> Result<Json> {
        let mut problems = Problems(Vec::new());
        let root = self.root.clone();
        // terrain: the heightmap must be readable (the simulation reads it on its first step; this reads it now)
        self.sim.load_terrains();
        {
            use lunaris_sim::components::{Terrain, Transform};
            let mut raw: Vec<(u64, u32)> = Vec::new();
            self.sim.world.for_each_uid::<(&Terrain,), _>(|_, uid, (g,)| raw.push((uid, g.heightmap.0)))?;
            let mut grounds: Vec<(u64, String, bool)> = Vec::new();
            for (uid, sym) in raw {
                let name = self.sim.world.strings().get(sym).to_string();
                let placed = self.sim.world.entity(uid).is_some_and(|e| self.sim.world.get::<Transform>(e).is_some());
                grounds.push((uid, name, placed));
            }
            grounds.sort_by_key(|g| g.0);
            for (uid, name, has_transform) in grounds {
                if !has_transform {
                    problems.add("error", Some(uid), "has a Terrain but no Transform, so it has no place".into(), "add a Transform: its position is the middle of the ground, its y the height of black");
                }
                if name.is_empty() {
                    problems.add("error", Some(uid), "Terrain.heightmap is empty".into(), "name a greyscale PNG of the workspace (16 bits is best); `files` lists the pictures");
                } else if !self.sim.heightfields.contains_key(&name) {
                    let why = self.sim.terrain_errors.get(&name).cloned().unwrap_or_else(|| "it has not been read".to_string());
                    problems.add("error", Some(uid), format!("the heightmap `{name}` could not be used: {why}"), "a greyscale PNG (8 or 16 bits, not interlaced) inside the workspace folder");
                }
            }
        }
        // mesh colliders (level geometry from a model file): the model must be there, have triangles, be of a size that can be used, and the
        // entity must not pretend to be a moving body
        {
            use lunaris_sim::components::{MeshCollider, RigidBody, Transform, Velocity};
            use lunaris_sim::meshcol::{BIG_TRIANGLES, MAX_TRIANGLES};
            self.sim.load_meshes();
            let mut raw: Vec<(u64, u32)> = Vec::new();
            self.sim.world.for_each_uid::<(&MeshCollider,), _>(|_, uid, (m,)| raw.push((uid, m.mesh.0)))?;
            raw.sort_by_key(|r| r.0);
            for (uid, sym) in raw {
                let name = self.sim.world.strings().get(sym).to_string();
                let e = self.sim.world.entity(uid);
                let (has_transform, has_body, moves) = match e {
                    Some(e) => (self.sim.world.get::<Transform>(e).is_some(), self.sim.world.get::<RigidBody>(e).is_some(), self.sim.world.get::<Velocity>(e).is_some() && self.sim.world.get::<RigidBody>(e).is_some_and(|r| r.mass > 0.0)),
                    None => (true, false, false),
                };
                if !has_transform {
                    problems.add("warning", Some(uid), "has a MeshCollider but no Transform, so the level has no place".into(), "add a Transform: the model is placed by its position, rotation and scale");
                }
                if name.is_empty() {
                    problems.add("warning", Some(uid), "MeshCollider.mesh is empty, so there is no level to collide with".into(), "name a .glb or .gltf of the workspace (`files` lists them): the same file as the MeshRenderer that shows it");
                } else if let Some(mesh) = self.sim.meshes.by_name.get(&name) {
                    if mesh.tris.len() > BIG_TRIANGLES {
                        problems.add("warning", Some(uid), format!("the model `{name}` has {} triangles: it works, but reading it takes a moment (the limit is {MAX_TRIANGLES})", mesh.tris.len()), "a simpler copy of the level for collision (no small props, no decoration) reads faster and is easier to stand on");
                    }
                    if mesh.degenerate > 0 {
                        problems.add("warning", Some(uid), format!("{} triangles of the model `{name}` have no area and are left out", mesh.degenerate), "they cannot be stood on or hit: remove them in the modelling program (merge by distance)");
                    }
                    for w in &mesh.warnings {
                        problems.add("warning", Some(uid), format!("the model `{name}`: {w}"), "");
                    }
                } else {
                    let why = self.sim.mesh_errors.get(&name).cloned().unwrap_or_else(|| "it has not been read".to_string());
                    let hint = if why.contains("at most") {
                        format!("the limit is {MAX_TRIANGLES} triangles: make a simpler copy of the level for collision")
                    } else if why.contains("no triangles") || why.contains("no usable triangles") {
                        "a MeshCollider needs a model with at least one triangle that has an area".to_string()
                    } else {
                        "a .glb or .gltf inside the workspace folder; `files` lists the models and `model_info` shows what one holds".to_string()
                    };
                    problems.add("warning", Some(uid), format!("the model `{name}` could not be used for collision: {why}"), &hint);
                }
                if has_body {
                    let what = if moves { "a dynamic body" } else { "a RigidBody" };
                    problems.add("warning", Some(uid), format!("a MeshCollider is static level geometry, but this entity also has {what}: the RigidBody is not used for the mesh (put the body on its own entity with a Collider)"), "a MeshCollider never moves by itself; move its Transform from a system or a Tween if it must (a platform)");
                }
            }
        }
        // convex hulls (a solid with the shape of a model's hull): the model must be there and make a solid
        {
            use lunaris_sim::components::{Collider, ConvexCollider, Transform};
            use lunaris_sim::hull::MAX_HULL_VERTICES;
            self.sim.load_meshes();
            let mut raw: Vec<(u64, u32)> = Vec::new();
            self.sim.world.for_each_uid::<(&ConvexCollider,), _>(|_, uid, (c,)| raw.push((uid, c.mesh.0)))?;
            raw.sort_by_key(|r| r.0);
            for (uid, sym) in raw {
                let name = self.sim.world.strings().get(sym).to_string();
                let (transform, has_collider) = match self.sim.world.entity(uid) {
                    Some(e) => (self.sim.world.get::<Transform>(e).copied(), self.sim.world.get::<Collider>(e).is_some()),
                    None => (None, false),
                };
                if transform.is_none() {
                    problems.add("warning", Some(uid), "has a ConvexCollider but no Transform, so the solid has no place".into(), "add a Transform: the model's origin is put there");
                }
                if has_collider {
                    problems.add("warning", Some(uid), "has both a Collider and a ConvexCollider: they are two shapes of one body that do not collide with each other".into(), "use one of them: the ConvexCollider for a shape of your own, the Collider for a ball, box or capsule");
                }
                if name.is_empty() {
                    problems.add("warning", Some(uid), "ConvexCollider.mesh is empty, so there is no shape".into(), "name a .glb or .gltf of the workspace (`files` lists them): its vertices make the hull");
                } else if self.sim.meshes.by_name.contains_key(&name) {
                    let scale = transform.map_or([1.0; 3], |t| [t.scale[0].abs(), t.scale[1].abs(), t.scale[2].abs()]);
                    match self.sim.meshes.hull(&name, scale) {
                        Ok(h) => {
                            if h.dropped > 0.0 {
                                problems.add("warning", Some(uid), format!("the model `{name}` has {} different corners; the hull keeps at most {MAX_HULL_VERTICES} (the corners that stick out least are left out: the farthest of them is {:.3} m from the hull)", h.points, h.dropped), "a simpler model (a low-poly copy) gives a hull that is exactly its shape");
                            }
                        }
                        Err(why) => problems.add("warning", Some(uid), format!("the model `{name}` makes no convex solid: {why}"), "the vertices of the model must enclose a volume (not lie in one plane or on a line)"),
                    }
                } else {
                    let why = self.sim.mesh_errors.get(&name).cloned().unwrap_or_else(|| "it has not been read".to_string());
                    problems.add("warning", Some(uid), format!("the model `{name}` could not be used for a hull: {why}"), "a .glb or .gltf inside the workspace folder; `files` lists the models and `model_info` shows what one holds");
                }
            }
        }
        // conversations: the script must parse, and the entities that show it must be a Text or a Ui
        {
            use lunaris_sim::components::{Dialogue, Text, Ui};
            let w = &mut self.sim.world;
            let mut talks: Vec<(u64, u32, [u64; 6])> = Vec::new();
            w.for_each_uid::<(&Dialogue,), _>(|_, uid, (d,)| talks.push((uid, d.script.0, [d.text_ui.0, d.speaker_ui.0, d.choice_1.0, d.choice_2.0, d.choice_3.0, d.choice_4.0])))?;
            for (uid, sym, shows) in talks {
                let text = w.strings().get(sym).to_string();
                if let Err(e) = lunaris_sim::dialogue::parse_dialogue(&text) {
                    problems.add("error", Some(uid), format!("Dialogue.script: {e}"), "== start, then lines (`Guard: Halt!`), choices (`* [Yes] -> next`), jumps (`-> end`); see docs/GAMEPLAY.md");
                }
                let names = ["text_ui", "speaker_ui", "choice_1", "choice_2", "choice_3", "choice_4"];
                for (k, id) in shows.iter().enumerate().filter(|(_, id)| **id != 0) {
                    let ok = w.entity(*id).is_some_and(|e| w.get::<Text>(e).is_some() || w.get::<Ui>(e).is_some());
                    if !ok {
                        problems.add("error", Some(uid), format!("Dialogue.{} is {id}, which is not an entity with a Text or a Ui", names[k]), "the uid of the Text or Ui entity that shows it");
                    }
                }
                if shows[0] == 0 {
                    problems.add("warning", Some(uid), "Dialogue.text_ui is not set, so nothing shows the lines".into(), "the uid of a Text or Ui entity");
                }
            }
        }
        // camera views: the camera they look through must be a camera, and not the main one
        {
            use lunaris_sim::components::{Camera, CameraView};
            let w = &mut self.sim.world;
            let mut views: Vec<(u64, u64)> = Vec::new();
            w.for_each_uid::<(&CameraView,), _>(|_, uid, (v,)| views.push((uid, v.camera.0)))?;
            for (uid, cam) in views {
                match w.entity(cam).and_then(|e| w.get::<Camera>(e).map(|c| c.active)) {
                    None => problems.add("error", Some(uid), format!("CameraView.camera {cam} is not an entity with a Camera"), "the uid of an entity with a `Camera` component"),
                    Some(true) => problems.add("warning", Some(uid), format!("the camera {cam} of this CameraView is `active`, so it can also become the main camera (the active one with the lowest uid is)"), "set that Camera's `active` to false: the view finds it by uid"),
                    Some(false) => {}
                }
            }
        }
        // animation events and root motion written as text
        {
            use lunaris_sim::components::{AnimEvents, Animator, RootMotion};
            let w = &mut self.sim.world;
            let mut texts: Vec<(u64, u32, bool)> = Vec::new();
            w.for_each_uid::<(&AnimEvents,), _>(|_, uid, (v,)| texts.push((uid, v.events.0, true)))?;
            w.for_each_uid::<(&RootMotion,), _>(|_, uid, (v,)| texts.push((uid, v.curves.0, false)))?;
            for (uid, sym, events) in texts {
                let text = w.strings().get(sym).to_string();
                let (what, parsed) = if events { ("AnimEvents.events", lunaris_sim::gameplay::parse_anim_events(&text).map(|_| ())) } else { ("RootMotion.curves", lunaris_sim::gameplay::parse_root_motion(&text).map(|_| ())) };
                if let Err(e) = parsed {
                    problems.add("error", Some(uid), format!("{what}: {e}"), if events { "walk length=0.8: 0.1 step_left, 0.5 step_right" } else { "walk length=0.8: 0 0 0 0, 0.4 0 0 0.5, 0.8 0 0 1.0 (`model_info` writes this for a model's clips)" });
                }
                if !w.entity(uid).is_some_and(|e| w.get::<Animator>(e).is_some()) {
                    problems.add("warning", Some(uid), format!("{what} needs an Animator on the same entity to know which clip plays"), "add an Animator");
                }
            }
        }
        // particle clouds: a picture that exists, and a count that fits
        {
            use lunaris_sim::components::Particles;
            let w = &mut self.sim.world;
            let mut clouds: Vec<(u64, u32, u32, f32)> = Vec::new();
            w.for_each_uid::<(&Particles,), _>(|_, uid, (p,)| clouds.push((uid, p.texture.0, p.count, p.lifetime)))?;
            for (uid, sym, count, lifetime) in clouds {
                let name = w.strings().get(sym).to_string();
                if !name.is_empty() && !lunaris_render::is_procedural_texture(&name) && !root.join(&name).is_file() {
                    problems.add("error", Some(uid), format!("Particles.texture `{name}` is not a built-in texture or a file of the workspace (a magenta square is drawn)"), "soft, circle, ring, or a PNG/JPEG path relative to the workspace folder");
                }
                if count > lunaris_render::particles::MAX_PARTICLES {
                    problems.add("warning", Some(uid), format!("Particles.count {count} is more than a cloud can have; only {} are drawn", lunaris_render::particles::MAX_PARTICLES), "split it into several clouds");
                }
                if !(lifetime > 0.0) {
                    problems.add("warning", Some(uid), "Particles.lifetime must be above 0 seconds".into(), "1 to 4 seconds suits most effects");
                }
            }
        }
        // flipbooks: the table parses, the entity has what it needs, the clip is in the table and the pictures are on the sheet
        {
            use lunaris_sim::components::{Animator, Sprite, SpriteAnim};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, u32)> = Vec::new();
            w.for_each_uid::<(&SpriteAnim,), _>(|e, uid, (s,)| found.push((uid, e, s.clips.0)))?;
            for (uid, e, sym) in found {
                let text = w.strings().get(sym).to_string();
                let (anim, sprite) = (w.get::<Animator>(e).copied(), w.get::<Sprite>(e).copied());
                if anim.is_none() || sprite.is_none() {
                    problems.add("warning", Some(uid), "SpriteAnim needs an Animator and a Sprite on the same entity (the Animator's clip and clock choose the picture)".into(), "add an Animator (clip = the name of a line of the table) and a Sprite with a `sheet`");
                }
                match lunaris_sim::spriteanim::parse(&text) {
                    Err(e) => problems.add("error", Some(uid), format!("SpriteAnim.clips: {e}"), "one clip a line: run: 4-9 fps=12 loop step@1"),
                    Ok(clips) => {
                        if let Some(a) = anim {
                            let name = w.strings().get(a.clip.0).to_string();
                            if !name.is_empty() && !clips.iter().any(|c| c.name == name) {
                                problems.add("warning", Some(uid), format!("Animator.clip `{name}` is not a clip of SpriteAnim.clips ({}), so the picture does not change", clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")), "write a line for it, or switch to one of the clips");
                            }
                        }
                        if let Some(s) = sprite {
                            let pictures = (s.sheet[0].max(1) as u64) * (s.sheet[1].max(1) as u64);
                            if let Some(c) = clips.iter().find(|c| c.frames.iter().any(|f| *f as u64 >= pictures)) {
                                problems.add("warning", Some(uid), format!("the clip `{}` uses a picture past the end of the Sprite's sheet ({} x {} = {pictures} pictures)", c.name, s.sheet[0], s.sheet[1]), "raise Sprite.sheet or use lower picture numbers");
                            }
                        }
                    }
                }
            }
        }
        // scene switches: the file has to be there to be switched to
        {
            use lunaris_sim::components::SceneSwitch;
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&SceneSwitch,), _>(|_, uid, (s,)| found.push((uid, s.path.0)))?;
            for (uid, sym) in found {
                let name = w.strings().get(sym).to_string();
                if name.is_empty() {
                    problems.add("warning", Some(uid), "SceneSwitch.path is empty, so there is no scene to go to".into(), "the path of a scene file of the workspace, like levels/two.lunaris.json");
                } else if !lunaris_json::safe_relative(&name).is_ok_and(|n| root.join(n).is_file()) {
                    if name.ends_with(".lunaris.json") {
                        // scenes that name each other in a circle (game over -> level -> game over) cannot all exist the first time they are built: a warning, and `export_game` still refuses the game
                        problems.add(
                            "warning",
                            Some(uid),
                            format!("SceneSwitch.path `{name}` is not a file of the workspace yet: until it is saved a switch to it fails and sets `scene.error`, and `export_game` refuses the game"),
                            "build every scene file, then run `check` again (scenes that go to each other cannot all exist at the first build)",
                        );
                    } else {
                        problems.add("error", Some(uid), format!("SceneSwitch.path `{name}` is not a file of the workspace, and its name is not a scene's (`*.lunaris.json`), so it is probably a typo (a switch to it would fail and set `scene.error`)"), "a path relative to the workspace, like levels/two.lunaris.json");
                    }
                }
            }
        }
        // steering: it needs a velocity to turn, and the entities it names must exist
        {
            use lunaris_sim::components::{Steer, Velocity};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Steer)> = Vec::new();
            w.for_each_uid::<(&Steer,), _>(|e, uid, (s,)| found.push((uid, e, *s)))?;
            for (uid, e, s) in found {
                if w.get::<Velocity>(e).is_none() {
                    problems.add("warning", Some(uid), "Steer needs a Velocity on the same entity (it steers by turning it)".into(), "add a Velocity component");
                }
                for (what, target) in [("seek", s.seek.0), ("flee", s.flee.0)] {
                    if target != 0 && w.entity(target).is_none() {
                        problems.add("warning", Some(uid), format!("Steer.{what} names entity {target}, which does not exist"), "the uid of the entity to run at or from (0 = nobody)");
                    }
                }
                if s.plane > 1 {
                    problems.add("warning", Some(uid), format!("Steer.plane {} is not 0 (ground: x and z) or 1 (picture: x and y)", s.plane), "use 0 or 1");
                }
            }
        }
        // weapons: the numbers make a gun that can fire, and its holder is named (so that its own body is never hit)
        {
            use lunaris_sim::components::{Parent, Weapon};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Weapon)> = Vec::new();
            w.for_each_uid::<(&Weapon,), _>(|e, uid, (g,)| found.push((uid, e, *g)))?;
            for (uid, e, g) in found {
                if g.kind > 1 {
                    problems.add("warning", Some(uid), format!("Weapon.kind {} is not 0 (a hitscan: an instant ray) or 1 (a projectile: a bullet that flies and falls)", g.kind), "use 0 or 1");
                }
                if !(g.rate > 0.0) || g.mag_size == 0 || !(g.range > 0.0) {
                    problems.add("warning", Some(uid), "Weapon needs a rate above 0 (rounds a minute), a magazine size above 0 and a range above 0 to fire".into(), "set rate, mag_size and range");
                }
                if g.kind == 1 && !(g.muzzle_speed > 0.0) {
                    problems.add("warning", Some(uid), "a projectile Weapon (kind 1) needs a muzzle_speed above 0".into(), "set muzzle_speed (metres a second)");
                }
                if g.mag > g.mag_size {
                    problems.add("warning", Some(uid), format!("Weapon.mag {} is more than mag_size {}", g.mag, g.mag_size), "a magazine holds at most mag_size rounds");
                }
                if g.owner.0 != 0 && w.entity(g.owner.0).is_none() {
                    problems.add("warning", Some(uid), format!("Weapon.owner names entity {}, which does not exist", g.owner.0), "the uid of the character that holds the gun (its own body is never hit)");
                } else if g.owner.0 == 0 && w.get::<Parent>(e).is_none_or(|p| p.target.0 == 0) {
                    problems.add("warning", Some(uid), "this Weapon has no owner and no Parent: a round fired from inside its holder's body can hit it".into(), "set Weapon.owner to the holder's uid, or attach the gun to the holder with a Parent");
                }
            }
        }
        // online play: one local player, and a prefab for the others that exists
        {
            use lunaris_sim::components::NetPlayer;
            let names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, NetPlayer)> = Vec::new();
            w.for_each_uid::<(&NetPlayer,), _>(|_, uid, (p,)| found.push((uid, *p)))?;
            found.sort_by_key(|f| f.0);
            for (i, (uid, p)) in found.iter().enumerate() {
                if i > 0 {
                    problems.add("warning", Some(*uid), "more than one NetPlayer: only the one with the lowest uid is the local player".into(), "give NetPlayer to the local player's entity only");
                }
                let name = w.strings().get(p.puppet.0).to_string();
                if !name.is_empty() && !names.contains(&name) {
                    problems.add("warning", Some(*uid), format!("NetPlayer.puppet `{name}` is not a prefab of this scene: the other players would be bare entities"), "define the prefab first (define_prefab) or fix the name");
                }
            }
        }
        // loot: the text reads, the items exist, the containers name tables that exist and have a bag to fill
        {
            use lunaris_sim::components::LootContainer;
            use lunaris_sim::inventory::Inventory;
            let loot = lunaris_sim::loot::loot_of(&mut self.sim.world);
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, LootContainer)> = Vec::new();
            w.for_each_uid::<(&LootContainer,), _>(|e, uid, (c,)| found.push((uid, e, *c)))?;
            if let Some(l) = &loot {
                for p in &l.problems {
                    problems.add("warning", None, format!("LootTable: {p}"), "see docs/LOOT.md for the text of a loot table");
                }
            } else if !found.is_empty() {
                problems.add("warning", None, "there are LootContainers but no LootTable".into(), "add an entity with a LootTable component");
            }
            for (uid, e, c) in found {
                let name = w.strings().get(c.table.0).to_string();
                if w.get::<Inventory>(e).is_none() {
                    problems.add("warning", Some(uid), "a LootContainer needs an Inventory on the same entity to fill".into(), "add an Inventory component");
                }
                if let Some(l) = &loot {
                    if !l.tables.contains_key(&name) {
                        problems.add("warning", Some(uid), format!("LootContainer.table `{name}` is not a table of the LootTable"), "use the name after `table` in the text");
                    }
                }
            }
        }
        // loot drops: what a destroyed thing drops needs a table that exists, a prefab that exists, hit points to run out and a place
        {
            use lunaris_sim::components::{HitPoints, LootDrop, Transform};
            let loot = lunaris_sim::loot::loot_of(&mut self.sim.world);
            let prefab_names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, LootDrop)> = Vec::new();
            w.for_each_uid::<(&LootDrop,), _>(|e, uid, (d,)| found.push((uid, e, *d)))?;
            for (uid, e, d) in found {
                let (table, prefab) = (w.strings().get(d.table.0).to_string(), w.strings().get(d.prefab.0).to_string());
                if w.get::<HitPoints>(e).is_none() {
                    problems.add("warning", Some(uid), "a LootDrop needs HitPoints on the same entity: it drops when they run out".into(), "add a HitPoints component");
                }
                if w.get::<Transform>(e).is_none() {
                    problems.add("warning", Some(uid), "a LootDrop needs a Transform: that is where the loot falls".into(), "add a Transform");
                }
                match &loot {
                    Some(l) if !l.tables.contains_key(&table) => problems.add("warning", Some(uid), format!("LootDrop.table `{table}` is not a table of the LootTable"), "use the name after `table` in the text"),
                    None => problems.add("warning", Some(uid), "a LootDrop needs a LootTable in the world".into(), "add an entity with a LootTable component"),
                    _ => {}
                }
                if !prefab_names.contains(&prefab) {
                    problems.add("warning", Some(uid), format!("LootDrop.prefab `{prefab}` is not a prefab of this scene: nothing would be dropped"), "define the pickup prefab first (define_prefab)");
                }
            }
        }
        // building: the rules read, the prefabs exist, the pieces and tools name kinds the rules have, sockets have something to plug into
        {
            use lunaris_sim::components::{BuildPiece, BuildRules, BuildSocket, BuildTool};
            let prefab_names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let first = {
                let mut best: Option<(u64, u32)> = None;
                w.for_each_uid::<(&BuildRules,), _>(|_, uid, (r,)| {
                    if best.is_none_or(|b| uid < b.0) {
                        best = Some((uid, r.text.0));
                    }
                })?;
                best
            };
            let rules = first.map(|(uid, sym)| (uid, lunaris_sim::building::parse_rules(w.strings().get(sym), Some(&prefab_names))));
            if let Some((uid, r)) = &rules {
                for p in &r.problems {
                    problems.add("warning", Some(*uid), format!("BuildRules: {p}"), "see docs/BUILDING.md for the text of the building rules");
                }
            }
            let mut pieces: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&BuildPiece,), _>(|_, uid, (p,)| pieces.push((uid, p.kind.0)))?;
            let mut tools: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&BuildTool,), _>(|_, uid, (t,)| tools.push((uid, t.kind.0)))?;
            let mut sockets: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&BuildSocket,), _>(|_, uid, (s,)| sockets.push((uid, s.kind.0)))?;
            match &rules {
                None => {
                    if !pieces.is_empty() || !tools.is_empty() {
                        problems.add("warning", None, "there are BuildPieces or BuildTools but no BuildRules".into(), "add an entity with a BuildRules component");
                    }
                }
                Some((_, r)) => {
                    let mut reported: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
                    for (uid, sym) in pieces {
                        let name = w.strings().get(sym).to_string();
                        if r.kind(&name).is_none() && reported.insert(format!("p{name}")) {
                            problems.add("warning", Some(uid), format!("BuildPiece.kind `{name}` is not a piece of the BuildRules: the engine leaves it alone"), "use a name after `piece` in the rules");
                        }
                    }
                    for (uid, sym) in tools {
                        let name = w.strings().get(sym).to_string();
                        if !name.is_empty() && r.kind(&name).is_none() {
                            problems.add("warning", Some(uid), format!("BuildTool.kind `{name}` is not a piece of the BuildRules"), "use a name after `piece` in the rules");
                        }
                    }
                    let types: std::collections::BTreeSet<&str> = r.kinds.iter().flat_map(|k| k.sockets.iter().flat_map(|s| s.mates.iter().map(String::as_str))).collect();
                    for (uid, sym) in sockets {
                        let name = w.strings().get(sym).to_string();
                        if !types.contains(name.as_str()) && reported.insert(format!("s{name}")) {
                            problems.add("warning", Some(uid), format!("BuildSocket.kind `{name}` is in no socket's `mates`: no piece can be built onto it"), "list the type in the `mates` of the sockets that should plug into it");
                        }
                    }
                }
            }
        }
        // crafting stations: a `Station` reaches somebody only with a Transform, a bit from 1 to 31 and a radius above 0, and a recipe has to ask for its number
        {
            use lunaris_sim::components::Transform;
            use lunaris_sim::inventory::{table_of, Station};
            let table = table_of(&mut self.sim.world).map(|(t, _)| t);
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Station)> = Vec::new();
            w.for_each_uid::<(&Station,), _>(|e, uid, (s,)| found.push((uid, e, *s)))?;
            for (uid, e, s) in found {
                if w.get::<Transform>(e).is_none() {
                    problems.add("warning", Some(uid), "a Station needs a Transform: that is where its reach is measured from".into(), "add a Transform");
                }
                if !(1..=31).contains(&s.bit) {
                    problems.add("warning", Some(uid), format!("Station.bit {} is not a station number from 1 to 31 (recipe `@N`): it unlocks nothing", s.bit), "use the number of the recipe lines, 1 to 31");
                } else if let Some(t) = &table {
                    if !t.recipes.iter().any(|r| r.station == s.bit) {
                        problems.add("warning", Some(uid), format!("Station.bit {}: no recipe of the ItemTable asks for station {} (`@{}`)", s.bit, s.bit, s.bit), "write `@N` after the ingredients of the recipes that need it");
                    }
                }
                if !(s.radius > 0.0) || !s.radius.is_finite() {
                    problems.add("warning", Some(uid), "Station.radius must be above 0 metres: with 0 it reaches nobody".into(), "set radius (metres), 2 to 3 is a bench");
                }
            }
        }
        // raiding (docs/RAIDING.md): the thrower's rules read and name items and prefabs that exist (the prefab has an Explosive, and a thrown one a body that flies), an Explosive that can hurt and that something sets off, a code lock on a thing that is used, a keypad with the item of the lock
        {
            use lunaris_sim::blast::{parse_rules, Explosive, Thrower};
            use lunaris_sim::components::{HitPoints, Interactable, Transform};
            use lunaris_sim::inventory::table_of;
            use lunaris_sim::lock::{CodeLock, Keypad, MAX_CODE};
            let table = table_of(&mut self.sim.world).map(|(t, _)| t);
            let prefabs = self.sim.scripts.prefabs.clone();
            let has = |name: &str, comp: &str| lunaris_sim::prefab::plan(&prefabs, name, &[]).is_ok_and(|p| p[0].components.iter().any(|(n, _)| n == comp));
            let w = &mut self.sim.world;
            let mut throwers: Vec<(u64, Thrower)> = Vec::new();
            w.for_each_uid::<(&Thrower,), _>(|_, uid, (t,)| throwers.push((uid, *t)))?;
            let mut said: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for (uid, t) in throwers {
                let text = w.strings().get(t.rules.0).to_string();
                if !said.insert(text.clone()) {
                    continue; // (every avatar of a game server has the same text: said once)
                }
                let r = parse_rules(&text);
                for p in &r.problems {
                    problems.add("warning", Some(uid), format!("Thrower.rules: {p}"), "see docs/RAIDING.md for the text of a thrower's rules");
                }
                if r.acts.is_empty() && r.problems.is_empty() {
                    problems.add("warning", Some(uid), "a Thrower has no `throw` or `place` line in its rules: nothing can be thrown or put down".into(), "write `throw KEY prefab=NAME` or `place KEY prefab=NAME` (docs/RAIDING.md)");
                }
                for a in &r.acts {
                    if let Some(tb) = &table {
                        if tb.id_of(&a.key) == 0 {
                            problems.add("warning", Some(uid), format!("Thrower.rules: `{}` is not an item of the ItemTable: nothing could be taken out of a bag to {} it", a.key, if a.place { "put down" } else { "throw" }), "use an item key of the ItemTable");
                        }
                    }
                    if !a.prefab.is_empty() {
                        if !prefabs.contains_key(&a.prefab) {
                            problems.add("warning", Some(uid), format!("Thrower.rules: prefab={} is not a prefab of this scene: nothing would be made", a.prefab), "define the prefab first (define_prefab)");
                        } else {
                            if !has(&a.prefab, "Explosive") {
                                problems.add("warning", Some(uid), format!("Thrower.rules: the prefab `{}` has no Explosive: what is made would never go off", a.prefab), "give the prefab an Explosive");
                            }
                            if !a.place && !(has(&a.prefab, "RigidBody") && has(&a.prefab, "Collider")) {
                                problems.add("warning", Some(uid), format!("Thrower.rules: the prefab `{}` is thrown and has no RigidBody and Collider: it would not fly or fall", a.prefab), "give it a RigidBody (mass above 0) and a Collider, the thrower adds the Velocity");
                            }
                        }
                    }
                }
            }
            let mut bombs: Vec<(u64, lunaris_ecs::Entity, Explosive)> = Vec::new();
            w.for_each_uid::<(&Explosive,), _>(|e, uid, (x,)| bombs.push((uid, e, *x)))?;
            for (uid, e, x) in bombs {
                if w.get::<Transform>(e).is_none() {
                    problems.add("warning", Some(uid), "an Explosive needs a Transform: it goes off where it is".into(), "add a Transform");
                }
                if !(x.radius > 0.0) || !(x.damage > 0.0) {
                    problems.add("warning", Some(uid), "an Explosive with no radius or no damage hurts nothing".into(), "set radius (metres) and damage above 0");
                }
                if x.fuse < 0.0 && !x.contact && !x.remote && w.get::<HitPoints>(e).is_none() {
                    problems.add("warning", Some(uid), "an Explosive with no fuse (fuse below 0), no contact, no remote trigger and no HitPoints is never set off".into(), "give it a fuse in seconds, or contact, or remote, or HitPoints");
                }
                if !(0.0..=1.0).contains(&x.cover) {
                    problems.add("warning", Some(uid), format!("Explosive.cover {} is not a share from 0 to 1 of the damage that gets through cover", x.cover), "use 0 (cover stops it all) to 1 (cover does nothing)");
                }
            }
            let mut locks: Vec<(u64, lunaris_ecs::Entity, CodeLock)> = Vec::new();
            w.for_each_uid::<(&CodeLock,), _>(|e, uid, (l,)| locks.push((uid, e, *l)))?;
            for (uid, e, l) in locks {
                if !(-1..=MAX_CODE).contains(&l.code) {
                    problems.add("warning", Some(uid), format!("CodeLock.code {} is not a code from 0 to {MAX_CODE} (or -1 for none yet)", l.code), "use four digits at most");
                }
                match w.get::<Interactable>(e) {
                    None => problems.add("warning", Some(uid), "a CodeLock needs an Interactable beside it (a door or a container): it locks what is used".into(), "add an Interactable"),
                    Some(i) if i.kind != 1 && i.kind != 2 => problems.add("warning", Some(uid), format!("a CodeLock on an Interactable of kind {}: only doors (1) and containers (2) are opened by code", i.kind), "use a door or a container"),
                    _ => {}
                }
            }
            let mut pads: Vec<(u64, Keypad)> = Vec::new();
            w.for_each_uid::<(&Keypad,), _>(|_, uid, (k,)| pads.push((uid, *k)))?;
            let mut said: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for (uid, k) in pads {
                let item = w.strings().get(k.item.0).to_string();
                if !said.insert(item.clone()) {
                    continue;
                }
                match &table {
                    Some(tb) if !item.is_empty() && tb.id_of(&item) == 0 => problems.add("warning", Some(uid), format!("Keypad.item `{item}` is not an item of the ItemTable: no lock could be fitted"), "use an item key of the ItemTable"),
                    _ if item.is_empty() => problems.add("warning", Some(uid), "a Keypad with no `item` cannot fit a lock (it can still enter and set codes)".into(), "set item to the key of the lock item"),
                    _ => {}
                }
            }
            // tiers: what an upgrade costs is items of the table (a name that is not one could never be paid)
            if let (Some((rules_e, rules)), Some(tb)) = (lunaris_sim::building::rules_of(w), &table) {
                let at = w.uid_of(rules_e);
                for k in &rules.kinds {
                    for (item, _) in &k.upgrade_cost {
                        if tb.id_of(item) == 0 {
                            problems.add("warning", at, format!("BuildRules: the piece `{}` has upgrade_cost `{item}`, which is not an item of the ItemTable: the upgrade could never be paid", k.name), "use an item key of the ItemTable");
                        }
                    }
                    for (item, _) in &k.repair_cost {
                        if tb.id_of(item) == 0 {
                            problems.add("warning", at, format!("BuildRules: the piece `{}` has repair_cost `{item}`, which is not an item of the ItemTable: the piece could never be repaired", k.name), "use an item key of the ItemTable");
                        }
                    }
                }
            }
        }
        // interaction: doors with a Door, containers with an Inventory, pickups with an item that exists, keys that exist
        {
            use lunaris_sim::components::{Collider, Door, Interactable, Transform};
            use lunaris_sim::inventory::{table_of, Inventory};
            let table = table_of(&mut self.sim.world).map(|(t, _)| t);
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Interactable)> = Vec::new();
            w.for_each_uid::<(&Interactable,), _>(|e, uid, (i,)| found.push((uid, e, *i)))?;
            for (uid, e, i) in found {
                if w.get::<Transform>(e).is_none() {
                    problems.add("warning", Some(uid), "an Interactable needs a Transform: that is where it is aimed at".into(), "add a Transform");
                }
                match i.kind {
                    1 if w.get::<Door>(e).is_none() => problems.add("warning", Some(uid), "an Interactable of kind 1 (a door) has no Door beside it: using it would do nothing".into(), "add a Door component (docs/INTERACT.md)"),
                    2 if w.get::<Inventory>(e).is_none() => problems.add("warning", Some(uid), "an Interactable of kind 2 (a container) has no Inventory beside it: there is nothing to open".into(), "add an Inventory component"),
                    k if k > 3 => problems.add("warning", Some(uid), format!("Interactable.kind {k} is not 0 use, 1 door, 2 container or 3 pickup"), "use a kind from 0 to 3"),
                    _ => {}
                }
                if i.kind != 1 && i.kind != 3 && w.get::<Collider>(e).is_none() && !(i.radius > 0.0) {
                    problems.add("warning", Some(uid), "an Interactable with no Collider needs a radius above 0 to be aimed at".into(), "set radius (metres)");
                }
                if let Some(t) = &table {
                    let key = w.strings().get(i.key.0).to_string();
                    if i.locked && !key.is_empty() && t.id_of(&key) == 0 {
                        problems.add("warning", Some(uid), format!("Interactable.key `{key}` is not an item of the ItemTable: nobody could unlock it"), "use an item key of the ItemTable");
                    }
                    if i.kind == 3 {
                        let item = w.strings().get(i.item.0).to_string();
                        let k = item.split_whitespace().next().unwrap_or("").to_string();
                        if k.is_empty() || t.id_of(&k) == 0 {
                            problems.add("warning", Some(uid), format!("Interactable.item `{item}` is not `ITEM N` with an item of the ItemTable"), "write the item's key and how many: `bandage 3`");
                        }
                    }
                } else if i.kind == 3 || (i.locked && i.key.0 != 0) {
                    problems.add("warning", Some(uid), "an Interactable that picks up items or wants a key needs an ItemTable in the scene".into(), "add an entity with an ItemTable (docs/GAMEPLAY.md, Inventories)");
                }
            }
            let mut doors: Vec<(u64, lunaris_ecs::Entity, Door)> = Vec::new();
            w.for_each_uid::<(&Door,), _>(|e, uid, (d,)| doors.push((uid, e, *d)))?;
            for (uid, e, d) in doors {
                if w.get::<Transform>(e).is_none() {
                    problems.add("warning", Some(uid), "a Door needs a Transform: it is that pose that swings".into(), "add a Transform");
                }
                if d.axis.iter().all(|a| *a == 0.0) {
                    problems.add("warning", Some(uid), "Door.axis is the zero vector: it cannot swing".into(), "the hinge axis, up by default: [0, 1, 0]");
                }
                if !(d.speed > 0.0) {
                    problems.add("warning", Some(uid), "Door.speed is 0: it would never move".into(), "degrees a second, 160 by default");
                }
            }
        }
        // flying machines: a body to move, a target and patrol that make sense, a perception to follow
        {
            use lunaris_sim::components::{FlyAgent, Perception, Transform, Velocity};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, FlyAgent)> = Vec::new();
            w.for_each_uid::<(&FlyAgent,), _>(|e, uid, (a,)| found.push((uid, e, *a)))?;
            for (uid, e, a) in found {
                if w.get::<Transform>(e).is_none() || w.get::<Velocity>(e).is_none() {
                    problems.add("warning", Some(uid), "a FlyAgent moves its entity by its Velocity and needs a Transform and a Velocity on it".into(), "add the components (docs/DRONES.md has an example)");
                }
                if a.mode > 4 {
                    problems.add("warning", Some(uid), format!("FlyAgent.mode {} is not 0 hold, 1 goto, 2 follow, 3 circle or 4 patrol", a.mode), "use a mode from 0 to 4");
                }
                if (a.mode == 2 || a.mode == 3) && !a.perception && (a.target.0 == 0 || w.entity(a.target.0).is_none()) {
                    problems.add("warning", Some(uid), format!("FlyAgent.target {} does not exist: in this mode it has nothing to follow", a.target.0), "set the uid of the entity to follow or circle");
                }
                let patrol = w.strings().get(a.patrol.0).to_string();
                if (a.mode == 4 || (a.perception && !patrol.is_empty())) && patrol.lines().filter(|l| l.split('#').next().unwrap_or("").split_whitespace().filter(|x| x.parse::<f32>().is_ok()).count() >= 3).count() == 0 {
                    problems.add("warning", Some(uid), "FlyAgent.patrol has no point: one `x y z` a line".into(), "write the points as lines of three numbers");
                }
                if a.perception && w.get::<Perception>(e).is_none() {
                    problems.add("warning", Some(uid), "FlyAgent.perception is on but the entity has no Perception to follow".into(), "add a Perception (docs/AI.md) or switch perception off");
                }
                if !(a.speed > 0.0) || !(a.accel > 0.0) {
                    problems.add("warning", Some(uid), "a FlyAgent needs a speed and an accel above 0".into(), "set speed (m/s) and accel (m/s^2)");
                }
            }
        }
        // a game server: the avatar prefab, the replicated things and the way the client shows them
        {
            use lunaris_sim::components::{NetEntity, NetHost, NetPlayer};
            let prefab_names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut hosts: Vec<(u64, NetHost)> = Vec::new();
            w.for_each_uid::<(&NetHost,), _>(|_, uid, (h,)| hosts.push((uid, *h)))?;
            hosts.sort_by_key(|h| h.0);
            // (the kinds that `NetHost.item_kinds` gives to pickups: the clients need a `NetPlayer.kinds` line to show each)
            let mut item_kinds: Vec<(u64, u32)> = Vec::new();
            if let Some((uid, h)) = hosts.first() {
                let player = w.strings().get(h.player.0).to_string();
                if player.is_empty() {
                    problems.add("warning", Some(*uid), "NetHost.player is empty: the players would get a bare capsule with no weapon, no tools and no respawn".into(), "name the prefab made for every player (docs/GAME_SERVER.md has one)");
                } else if !prefab_names.contains(&player) {
                    problems.add("warning", Some(*uid), format!("NetHost.player `{player}` is not a prefab of this scene"), "define the prefab first (define_prefab) or fix the name");
                } else if let Ok(plan) = lunaris_sim::prefab::plan(&self.sim.scripts.prefabs, &player, &[]) {
                    let has = |c: &str| plan.iter().any(|p| p.components.iter().any(|(n, _)| n == c));
                    for need in ["HitPoints", "NetAvatar"] {
                        if !has(need) {
                            problems.add("warning", Some(*uid), format!("the player prefab `{player}` has no {need}"), "an avatar needs hit points to be shot and a NetAvatar for its buttons (docs/GAME_SERVER.md)");
                        }
                    }
                    if !has("CharacterBody") && !has("Collider") {
                        problems.add("warning", Some(*uid), format!("the player prefab `{player}` has no CharacterBody or Collider: a shot could not hit it"), "add a CharacterBody (gravity_scale 0) to the avatar");
                    }
                }
                for line in w.strings().get(h.forward.0).lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()) {
                    let mut words = line.split_whitespace();
                    let _ = words.next();
                    // (`NAME own`: a private event, told only to the player it is about: docs/DEPTH.md)
                    if words.next().is_some_and(|r| r != "own" && r.parse::<f32>().is_err()) {
                        problems.add("warning", Some(*uid), format!("NetHost.forward line `{line}`: the radius is not a number"), "one line a kind, NAME RADIUS");
                    }
                }
                // the resources every client is told (`sync`), and the components the server leaves out (`skip`)
                let sync = w.strings().get(h.sync.0).to_string();
                let listed: Vec<&str> = sync.split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()).collect();
                for (i, n) in listed.iter().enumerate() {
                    // (the rule of `lunaris_net::scale::netsync::parse_names`)
                    let ok = lunaris_ecs::valid_resource_name(n) && n.len() <= 60 && i < 32 && !["input.", "action.", "game.", "scene.", "time."].iter().any(|p| n.starts_with(p)) && !listed[..i].contains(n);
                    if !ok {
                        problems.add("warning", Some(*uid), format!("NetHost.sync: `{n}` is not sent (a resource name of letters, digits, underscores and dots, not one of the engine's own: input. action. game. scene. time.; 32 at most)"), "name the resources of the server's world the clients should show, such as dn.t");
                    }
                }
                if h.sync_every < 0.0 || !h.sync_every.is_finite() {
                    problems.add("warning", Some(*uid), "NetHost.sync_every must be 0 or more seconds".into(), "1 is a good number for a clock");
                }
                let skip = w.strings().get(h.skip.0).to_string();
                for n in skip.split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()) {
                    if w.component_id(n).is_none() {
                        problems.add("warning", Some(*uid), format!("NetHost.skip: `{n}` is not a component"), "use component names such as Ui Text");
                    }
                }
                // what using an item does (`inv_use`): lines `use KEY eat=N ...` whose keys are items of the table
                let uses = w.strings().get(h.uses.0).to_string();
                if !uses.trim().is_empty() {
                    let keys: Option<Vec<String>> = lunaris_sim::inventory::table_of(w).map(|(t, _)| t.items.iter().map(|i| i.key.clone()).collect());
                    for line in uses.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()) {
                        let mut words = line.split_whitespace();
                        if words.next() != Some("use") {
                            problems.add("warning", Some(*uid), format!("NetHost.uses line `{line}` does not start with `use`"), "one line an item: use KEY eat=30 drink=0 heal=0 bandage=1 keep=0");
                            continue;
                        }
                        match (words.next(), &keys) {
                            (None, _) => problems.add("warning", Some(*uid), format!("NetHost.uses line `{line}` names no item"), "use KEY eat=30 ..."),
                            (Some(k), Some(keys)) if !keys.iter().any(|x| x == k) => problems.add("warning", Some(*uid), format!("NetHost.uses: `{k}` is not an item of the ItemTable"), "use the key of an item (docs/GAMEPLAY.md, \"Inventories\")"),
                            (Some(_), None) => problems.add("warning", Some(*uid), "NetHost.uses needs an ItemTable in the world".into(), "add an entity with an ItemTable"),
                            _ => {}
                        }
                        for word in words {
                            let ok = word.split_once('=').is_some_and(|(k, v)| matches!(k, "eat" | "drink" | "heal" | "bandage" | "keep") && v.parse::<f32>().is_ok());
                            if !ok {
                                problems.add("warning", Some(*uid), format!("NetHost.uses line `{line}`: `{word}` is not eat, drink, heal, bandage or keep = a number"), "use KEY eat=30 drink=0 heal=0 bandage=1 keep=0");
                            }
                        }
                    }
                }
                // the picture of each item on the ground (`item_kinds`): lines `KEY KIND` whose keys are items of the table
                let texts = w.strings().get(h.item_kinds.0).to_string();
                if !texts.trim().is_empty() {
                    let keys: Option<Vec<String>> = lunaris_sim::inventory::table_of(w).map(|(t, _)| t.items.iter().map(|i| i.key.clone()).collect());
                    for line in texts.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()) {
                        let words: Vec<&str> = line.split_whitespace().collect();
                        let kind = words.get(1).and_then(|k| k.parse::<u32>().ok());
                        match (words.len(), kind) {
                            (2, Some(kind)) => {
                                item_kinds.push((*uid, kind));
                                match &keys {
                                    Some(keys) if !keys.iter().any(|x| x == words[0]) => problems.add("warning", Some(*uid), format!("NetHost.item_kinds: `{}` is not an item of the ItemTable", words[0]), "use the key of an item (docs/GAMEPLAY.md, \"Inventories\")"),
                                    None => problems.add("warning", Some(*uid), "NetHost.item_kinds needs an ItemTable in the world".into(), "add an entity with an ItemTable"),
                                    _ => {}
                                }
                            }
                            _ => problems.add("warning", Some(*uid), format!("NetHost.item_kinds line `{line}` is not KEY KIND (KIND a number)"), "one line an item: rifle 34"),
                        }
                    }
                }
                if hosts.len() > 1 {
                    problems.add("warning", Some(hosts[1].0), "more than one NetHost: only the one with the lowest uid counts".into(), "keep one");
                }
            }
            // the container window of a client: an Inventory beside the NetContainer
            let mut containers: Vec<(u64, lunaris_ecs::Entity)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::NetContainer,), _>(|e, uid, _| containers.push((uid, e)))?;
            for (uid, e) in containers {
                if w.get::<lunaris_sim::inventory::Inventory>(e).is_none() {
                    problems.add("warning", Some(uid), "a NetContainer needs an Inventory on the same entity: the open container's contents are written there".into(), "add an Inventory component");
                }
            }
            // the way into a game from a menu: its address and name are the text of `UiInput`s
            let mut connects: Vec<(u64, lunaris_sim::components::NetConnect)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::NetConnect,), _>(|_, uid, (c,)| connects.push((uid, *c)))?;
            for (uid, c) in &connects {
                for (field, input) in [("address_input", c.address_input), ("name_input", c.name_input), ("token_input", c.token_input)] {
                    if input != 0 && !w.entity(input as u64).is_some_and(|e| w.get::<lunaris_sim::components::UiInput>(e).is_some()) {
                        problems.add("warning", Some(*uid), format!("NetConnect.{field} {input} is not an entity with a UiInput: the typed text cannot be read"), "give it the uid of the text field (0 uses NetConnect.address / name)");
                    }
                }
                if c.address_input == 0 && w.strings().get(c.address.0).trim().is_empty() {
                    problems.add("warning", Some(*uid), "NetConnect has neither `address_input` nor an `address`: Connect would have nowhere to go".into(), "set address_input to the uid of the UiInput that holds host:port");
                }
            }
            if connects.len() > 1 {
                problems.add("warning", Some(connects[1].0), "more than one NetConnect: only the one with the lowest uid counts".into(), "keep one");
            }
            let mut things: Vec<(u64, NetEntity)> = Vec::new();
            w.for_each_uid::<(&NetEntity,), _>(|_, uid, (n,)| things.push((uid, *n)))?;
            let mut kinds: Vec<(u64, u32, u32)> = Vec::new();
            w.for_each_uid::<(&NetPlayer,), _>(|_, uid, (p,)| kinds.push((uid, p.kinds.0, p.puppet.0)))?;
            for (uid, n) in things {
                if n.class > 127 {
                    problems.add("warning", Some(uid), format!("NetEntity.class {} is above 127: the network has room for 128 classes", n.class), "use a class from 1 to 127");
                }
            }
            // (what the clients' `kinds` lines show of class 1 and 3: a kind number, or `None` for any)
            let mut shown: Vec<Option<u32>> = Vec::new();
            let players = kinds.len();
            for (uid, kinds_sym, puppet_sym) in kinds {
                let text = w.strings().get(kinds_sym).to_string();
                let puppet = w.strings().get(puppet_sym).to_string();
                if !puppet.is_empty() && !prefab_names.contains(&puppet) {
                    problems.add("warning", Some(uid), format!("NetPlayer.puppet `{puppet}` is not a prefab of this scene: the other players would be bare entities"), "define the prefab first (define_prefab) or fix the name");
                }
                for line in text.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty()) {
                    let words: Vec<&str> = line.split_whitespace().collect();
                    let ok = words.len() == 3 && words[0].parse::<u32>().is_ok() && (words[1] == "*" || words[1].parse::<u32>().is_ok());
                    if !ok {
                        problems.add("warning", Some(uid), format!("NetPlayer.kinds line `{line}` is not CLASS KIND PREFAB (KIND a number or `*`)"), "one line a thing: `3 12 WallModel` or `4 * MachineModel`");
                    } else {
                        if !prefab_names.contains(&words[2].to_string()) {
                            problems.add("warning", Some(uid), format!("NetPlayer.kinds names the prefab `{}`, which is not a prefab of this scene", words[2]), "define the prefab first (define_prefab) or fix the name");
                        }
                        if matches!(words[0], "1" | "3") {
                            shown.push(words[1].parse::<u32>().ok());
                        }
                    }
                }
            }
            if players > 0 {
                for (uid, kind) in item_kinds {
                    if !shown.iter().any(|k| k.is_none_or(|k| k == kind)) {
                        problems.add("warning", Some(uid), format!("NetHost.item_kinds gives the kind {kind}, but no NetPlayer.kinds line shows a thing of class 1 or 3 with that kind: the item would not be shown on the ground"), "add a line `1 KIND PREFAB` to NetPlayer.kinds");
                    }
                }
            }
        }
        // death and respawn: hit points to die from, a corpse prefab that exists, somewhere to get up
        {
            use lunaris_sim::components::{HitPoints, Respawn, SpawnPoint};
            let prefab_names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Respawn)> = Vec::new();
            w.for_each_uid::<(&Respawn,), _>(|e, uid, (r,)| found.push((uid, e, *r)))?;
            let mut points: Vec<SpawnPoint> = Vec::new();
            w.for_each::<(&SpawnPoint,), _>(|_, (p,)| points.push(*p))?;
            for (uid, e, r) in found {
                if w.get::<HitPoints>(e).is_none() {
                    problems.add("warning", Some(uid), "a Respawn needs HitPoints on the same entity: it is dead when they reach 0, and without them the engine leaves it alone".into(), "add a HitPoints component");
                }
                let corpse = w.strings().get(r.corpse.0).to_string();
                if !corpse.is_empty() && !prefab_names.contains(&corpse) {
                    problems.add("warning", Some(uid), format!("Respawn.corpse `{corpse}` is not a prefab of this scene: no corpse would be made"), "define the prefab first (define_prefab) or fix the name");
                }
                if points.iter().filter(|p| p.active && (p.group == 0 || (r.group != 0 && p.group == r.group))).count() == 0 {
                    problems.add("warning", Some(uid), "there is no active SpawnPoint for this Respawn: the player would get up where they fell".into(), "add an entity with a Transform and a SpawnPoint");
                }
                if !(r.delay >= 0.0) || !(r.avoid >= 0.0) {
                    problems.add("warning", Some(uid), "Respawn.delay and avoid are negative or not a number".into(), "seconds and metres, 0 or more");
                }
            }
        }
        // helicopters: a body to fly, sane numbers, parts that exist
        {
            use lunaris_sim::components::{AngularVelocity, Collider, Helicopter, RigidBody, Velocity};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Helicopter)> = Vec::new();
            w.for_each_uid::<(&Helicopter,), _>(|e, uid, (h,)| found.push((uid, e, *h)))?;
            for (uid, e, h) in found {
                let mut missing: Vec<&str> = Vec::new();
                if w.get::<Collider>(e).is_none() {
                    missing.push("Collider");
                }
                if w.get::<RigidBody>(e).is_none_or(|r| !(r.mass > 0.0)) {
                    missing.push("RigidBody with a mass");
                }
                if w.get::<Velocity>(e).is_none() {
                    missing.push("Velocity");
                }
                if w.get::<AngularVelocity>(e).is_none() {
                    missing.push("AngularVelocity");
                }
                if !missing.is_empty() {
                    problems.add("warning", Some(uid), format!("a Helicopter is a rigid body and needs {} on the same entity: without it the engine leaves it alone", missing.join(", ")), "add the components (docs/HELICOPTER.md has an example)");
                }
                for (name, part) in [("rotor", h.rotor.0), ("tail_rotor", h.tail_rotor.0)] {
                    if part != 0 && w.entity(part).is_none() {
                        problems.add("warning", Some(uid), format!("Helicopter.{name} names entity {part}, which does not exist"), "the uid of the rotor's model entity (0: none)");
                    }
                }
                if h.pilot.0 != 0 && w.entity(h.pilot.0).is_none() {
                    problems.add("warning", Some(uid), format!("Helicopter.pilot names entity {}, which does not exist", h.pilot.0), "the uid of whoever flies it (0: nobody)");
                }
                if !(h.lift > 0.0) || !(h.spool_up > 0.0) || !(h.spool_down > 0.0) {
                    problems.add("warning", Some(uid), "a Helicopter needs a lift and spool times above 0".into(), "set lift (m/s^2: twice gravity hovers at a neutral collective), spool_up and spool_down (seconds)");
                } else if h.lift < 9.8 {
                    problems.add("warning", Some(uid), format!("Helicopter.lift is {}: below gravity it can never leave the ground", h.lift), "lift is an acceleration in m/s^2: 19.6 hovers at a neutral collective");
                }
            }
        }
        // loadouts: guns that are items: the text, the items it names, a weapon to configure and a bag to count the ammunition in
        {
            use lunaris_sim::components::{Loadout, Weapon};
            use lunaris_sim::inventory::{table_of, Inventory};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Loadout)> = Vec::new();
            w.for_each_uid::<(&Loadout,), _>(|e, uid, (l,)| found.push((uid, e, *l)))?;
            let table = table_of(w).map(|(t, _)| t);
            for (uid, e, l) in found {
                let text = w.strings().get(l.guns.0).to_string();
                let (guns, bad) = lunaris_sim::loadout::parse_guns(&text);
                for b in bad {
                    problems.add("warning", Some(uid), format!("Loadout.guns: {b}"), "a line is `gun ITEM key=value ...` (docs/LOADOUT.md)");
                }
                if guns.is_empty() {
                    problems.add("warning", Some(uid), "this Loadout names no gun: its Weapon will always be switched off".into(), "add a line `gun ITEM ammo=AMMO damage=... rate=... mag=...`");
                }
                match &table {
                    None => problems.add("warning", Some(uid), "a Loadout needs an ItemTable in the world: the guns and the ammunition are items".into(), "add an entity with an ItemTable"),
                    Some(t) => {
                        for g in &guns {
                            if t.id_of(&g.item) == 0 {
                                problems.add("warning", Some(uid), format!("Loadout gun `{}` is not an item of the ItemTable", g.item), "add the item to the ItemTable (docs/GAMEPLAY.md, \"Inventories\")");
                            }
                            if !g.ammo.is_empty() && t.id_of(&g.ammo) == 0 {
                                problems.add("warning", Some(uid), format!("Loadout gun `{}` takes ammunition `{}`, which is not an item of the ItemTable", g.item, g.ammo), "add the item to the ItemTable, or drop `ammo=` for a gun that never runs out");
                            }
                        }
                    }
                }
                let we = if l.weapon.0 != 0 { w.entity(l.weapon.0) } else { Some(e) };
                if !we.is_some_and(|we| w.get::<Weapon>(we).is_some()) {
                    problems.add("warning", Some(uid), "this Loadout has no Weapon to configure (on this entity, or the one Loadout.weapon names)".into(), "put the Weapon on the same entity, or set Loadout.weapon to the entity that has it");
                }
                let bag = if l.inventory.0 != 0 { w.entity(l.inventory.0) } else if w.get::<Inventory>(e).is_some() { Some(e) } else { w.get::<lunaris_sim::components::Parent>(e).and_then(|p| w.entity(p.target.0)) };
                if !bag.is_some_and(|b| w.get::<Inventory>(b).is_some()) {
                    problems.add("warning", Some(uid), "this Loadout has no Inventory to hold the guns and the ammunition (on this entity, its parent, or the one Loadout.inventory names)".into(), "give the character an Inventory");
                }
            }
        }
        // seats: a way in, and a helicopter to fly for the pilot's seat
        {
            use lunaris_sim::components::{Helicopter, Interactable, Seat};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Seat)> = Vec::new();
            w.for_each_uid::<(&Seat,), _>(|e, uid, (s,)| found.push((uid, e, *s)))?;
            for (uid, e, s) in found {
                match w.get::<Interactable>(e) {
                    None => problems.add("warning", Some(uid), "a Seat has no way in: nothing can use it without an Interactable on the same entity".into(), "add an Interactable of kind 0 with a prompt (\"Enter\") and a Collider to aim at (docs/SEATS.md)"),
                    Some(i) if i.kind != 0 => problems.add("warning", Some(uid), format!("a Seat's Interactable is of kind {}: using it would also be the use of a door, a container or an item", i.kind), "kind 0 only reports the use, which the Seat answers"),
                    _ => {}
                }
                if s.drive == 1 && w.get::<Helicopter>(e).is_none() {
                    problems.add("warning", Some(uid), "Seat.drive 1 flies a Helicopter and there is none on the same entity".into(), "put the Helicopter on the entity of the seat, or use drive 0 for a passenger's seat");
                }
                if s.drive == 2 && w.get::<lunaris_sim::components::Vehicle>(e).is_none() {
                    problems.add("warning", Some(uid), "Seat.drive 2 drives a Vehicle and there is none on the same entity".into(), "put the Vehicle on the entity of the seat, or use drive 0 for a passenger's seat");
                }
                if s.drive > 2 {
                    problems.add("warning", Some(uid), format!("Seat.drive is {}: 0 is a passenger's seat, 1 flies a Helicopter, 2 drives a Vehicle", s.drive), "0, 1 or 2");
                }
            }
        }
        // survival needs: they hurt the HitPoints beside them
        {
            use lunaris_sim::components::{HitPoints, Vitals};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Vitals)> = Vec::new();
            w.for_each_uid::<(&Vitals,), _>(|e, uid, (v,)| found.push((uid, e, *v)))?;
            for (uid, e, v) in found {
                if w.get::<HitPoints>(e).is_none() {
                    problems.add("warning", Some(uid), "Vitals needs HitPoints on the same entity: starving, thirst and bleeding hurt them, and a fed body heals them".into(), "add a HitPoints component");
                }
                if [v.hunger_rate, v.thirst_rate, v.sprint_cost, v.stamina_regen, v.starve_damage, v.dehydrate_damage].iter().any(|r| *r < 0.0 || !r.is_finite()) {
                    problems.add("warning", Some(uid), "a rate of Vitals is negative or not a number".into(), "rates are points (or hit points) a second and are not negative");
                }
            }
        }
        // hacking: a camera that can follow, a machine that can be taken
        {
            use lunaris_sim::components::{CameraRig, Hackable, Hacker};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, Hacker)> = Vec::new();
            w.for_each_uid::<(&Hacker,), _>(|_, uid, (h,)| found.push((uid, *h)))?;
            for (uid, h) in found {
                if h.camera.0 != 0 && w.entity(h.camera.0).and_then(|ce| w.get::<CameraRig>(ce)).is_none() {
                    problems.add("warning", Some(uid), format!("Hacker.camera names entity {}, which does not exist or has no CameraRig", h.camera.0), "the uid of the entity whose CameraRig follows the machine while controlling (0: the view does not change)");
                }
                if !(h.range > 0.0) || !(h.speed > 0.0) {
                    problems.add("warning", Some(uid), "a Hacker needs a range and a speed above 0".into(), "set range (metres) and speed (1 is the machine's difficulty in seconds)");
                }
            }
            let mut found: Vec<(u64, Hackable)> = Vec::new();
            w.for_each_uid::<(&Hackable,), _>(|_, uid, (k,)| found.push((uid, *k)))?;
            for (uid, k) in found {
                if !(k.difficulty > 0.0) || !(k.range > 0.0) {
                    problems.add("warning", Some(uid), "a Hackable needs a difficulty (seconds) and a range (metres) above 0".into(), "set difficulty and range");
                }
            }
        }
        // the AI director: a prefab to send, and players to watch
        {
            use lunaris_sim::components::{Director, Heat};
            let names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, Director)> = Vec::new();
            w.for_each_uid::<(&Director,), _>(|_, uid, (d,)| found.push((uid, *d)))?;
            let mut watched = 0usize;
            w.for_each::<(&Heat,), _>(|_, _| watched += 1)?;
            for (uid, d) in found {
                let name = w.strings().get(d.prefab.0).to_string();
                if name.is_empty() {
                    problems.add("warning", Some(uid), "this Director has no prefab to send".into(), "set Director.prefab to the name of the hunter prefab (the game's own: body, Team, Perception with engage, NavAgent, Weapon)");
                } else if !names.contains(&name) {
                    problems.add("warning", Some(uid), format!("Director.prefab `{name}` is not a prefab of this scene"), "define the prefab first (define_prefab) or fix the name");
                }
                if watched == 0 {
                    problems.add("warning", Some(uid), "no entity has a Heat: the director has nobody to watch".into(), "add a Heat component to each player");
                }
                if d.min_dist > d.max_dist {
                    problems.add("warning", Some(uid), "Director.min_dist is greater than max_dist".into(), "the ring is min_dist to max_dist metres round the player");
                }
            }
        }
        // AI senses: something to sense with, a side, and what the autopilot needs to act
        {
            use lunaris_sim::components::{CharacterBody, NavAgent, Parent, Perception, Searchlight, Team, Weapon};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, Perception)> = Vec::new();
            w.for_each_uid::<(&Perception,), _>(|e, uid, (p,)| found.push((uid, e, *p)))?;
            for (uid, e, p) in found {
                if !(p.sight_range > 0.0) && !(p.hearing > 0.0) {
                    problems.add("warning", Some(uid), "this Perception neither sees (sight_range) nor hears (hearing): it can never notice anything".into(), "set sight_range above 0 or hearing above 0");
                }
                if w.get::<Team>(e).is_none_or(|t| t.id == 0) {
                    problems.add("warning", Some(uid), "this Perception has no Team (or team 0): it treats every entity with HitPoints as an enemy, other machines included".into(), "give the machines a Team of one id and the players another");
                }
                if p.engage && p.chase && w.get::<NavAgent>(e).is_none() {
                    problems.add("warning", Some(uid), "Perception.engage with chase needs a NavAgent on the same entity to walk to the target".into(), "add a NavAgent, or set chase to false and move it yourself");
                }
                if p.weapon.0 != 0 && w.entity(p.weapon.0).and_then(|we| w.get::<Weapon>(we)).is_none() {
                    problems.add("warning", Some(uid), format!("Perception.weapon names entity {}, which does not exist or has no Weapon", p.weapon.0), "the uid of the entity with the Weapon to fire (0: this entity's own)");
                } else if p.engage && p.weapon.0 == 0 && w.get::<Weapon>(e).is_none() {
                    problems.add("warning", Some(uid), "Perception.engage has no weapon to hold the trigger of (this entity has no Weapon and Perception.weapon is 0): it will chase and turn but never shoot".into(), "add a Weapon here, or set Perception.weapon");
                }
                if p.engage && p.weapon.0 == 0 && w.get::<Weapon>(e).is_some() && w.get::<CharacterBody>(e).is_some() {
                    problems.add("warning", Some(uid), "this character fires its own Weapon from its feet (the Weapon sits on the CharacterBody's Transform)".into(), "set the Weapon's muzzle to chest height, for example [0, 1.4, 0]");
                }
            }
            let mut found: Vec<(u64, lunaris_ecs::Entity, Searchlight)> = Vec::new();
            w.for_each_uid::<(&Searchlight,), _>(|e, uid, (s,)| found.push((uid, e, *s)))?;
            for (uid, e, s) in found {
                if !(s.range > 0.0) || !(s.angle > 0.0) {
                    problems.add("warning", Some(uid), "a Searchlight needs a range and an angle above 0 to light anything".into(), "set range (metres) and angle (degrees either side of the axis)");
                }
                if s.sweep != 0.0 && w.get::<Parent>(e).is_some_and(|p| p.target.0 != 0) {
                    problems.add("warning", Some(uid), "a sweeping Searchlight with a Parent: the parent's movement overwrites the turn the engine gives it".into(), "put the Searchlight on an entity without a Parent, or turn the parent");
                }
            }
        }
        // heavy machines (docs/WALKER.md): legs that need an agent, a gun that needs a shell, a blow that needs a side, a weak point that needs to be a ghost, and the Core's learning
        {
            use lunaris_sim::components::{CollisionLayer, Collider, Director, HitPoints, NavAgent, Parent, Perception, Team, Weapon};
            use lunaris_sim::escalation::{parse_noises, parse_tiers, Escalation};
            use lunaris_sim::stomp::Stomp;
            use lunaris_sim::walker::{Cannon, Hitbox, Pursuit};
            let names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut legs: Vec<(u64, lunaris_ecs::Entity)> = Vec::new();
            w.for_each_uid::<(&Pursuit,), _>(|e, uid, _| legs.push((uid, e)))?;
            for (uid, e) in legs {
                if w.get::<NavAgent>(e).is_none() {
                    problems.add("warning", Some(uid), "Pursuit needs a NavAgent on the same entity: it walks by asking the agent for paths, and does nothing without one".into(), "add a NavAgent with the machine's size (radius, height, step_height)");
                }
                match w.get::<Perception>(e) {
                    None => problems.add("warning", Some(uid), "Pursuit with no Perception: there is nothing to tell it what to chase or search, it can only patrol".into(), "add a Perception (docs/AI.md)"),
                    Some(p) if p.engage => problems.add("warning", Some(uid), "Pursuit and Perception.engage both walk and turn the machine: the autopilot snaps round and stops at 85 percent of its attack range".into(), "set Perception.engage to false: Pursuit, Cannon and Stomp do what it would"),
                    Some(_) => {}
                }
            }
            let mut guns: Vec<(u64, lunaris_ecs::Entity, Cannon)> = Vec::new();
            w.for_each_uid::<(&Cannon,), _>(|e, uid, (c,)| guns.push((uid, e, *c)))?;
            for (uid, e, c) in guns {
                let mut gun: Option<(u64, Weapon)> = if c.weapon.0 != 0 { w.entity(c.weapon.0).and_then(|we| w.get::<Weapon>(we).copied()).map(|x| (c.weapon.0, x)) } else { None };
                if gun.is_none() && c.weapon.0 == 0 {
                    let mut best: Option<(u64, Weapon)> = None;
                    w.for_each_uid::<(&Parent, &Weapon), _>(|_, wuid, (p, wp)| {
                        if p.target.0 == uid && best.as_ref().is_none_or(|b| wuid < b.0) {
                            best = Some((wuid, *wp));
                        }
                    })?;
                    gun = best;
                }
                match gun {
                    None => problems.add("warning", Some(uid), "this Cannon has no gun: no child with a Weapon (and Cannon.weapon is not the uid of one)".into(), "make the gun a child of the machine: a Weapon of kind 1 with a muzzle at the end of its barrel"),
                    Some((_, wp)) if wp.kind != 1 => problems.add("warning", Some(uid), "the Cannon's weapon is not a flying round (kind 1): the barrel is aimed for a shell's flight, and a hitscan gun does not fall".into(), "set Weapon.kind to 1 with a muzzle_speed and a gravity"),
                    Some(_) => {}
                }
                if w.get::<Team>(e).is_none_or(|t| t.id == 0) {
                    problems.add("warning", Some(uid), "a Cannon on a machine with no Team (or team 0): its shells and their splash hurt everything, the machine's own parts included".into(), "give the machine a Team");
                }
            }
            let mut stomps: Vec<(u64, lunaris_ecs::Entity)> = Vec::new();
            w.for_each_uid::<(&Stomp,), _>(|e, uid, _| stomps.push((uid, e)))?;
            for (uid, e) in stomps {
                if w.get::<Team>(e).is_none_or(|t| t.id == 0) {
                    problems.add("warning", Some(uid), "a Stomp on a machine with no Team (or team 0) hurts everything near it, its own parts and friends too".into(), "give the machine a Team of its own side's id");
                }
                if w.get::<Perception>(e).is_none() {
                    problems.add("warning", Some(uid), "a Stomp with no Perception never starts: it waits for the target the Perception is alert to".into(), "add a Perception");
                }
            }
            let mut parts: Vec<(u64, lunaris_ecs::Entity, Hitbox)> = Vec::new();
            w.for_each_uid::<(&Hitbox,), _>(|e, uid, (h,)| parts.push((uid, e, *h)))?;
            for (uid, e, h) in parts {
                let owner = if h.owner.0 != 0 { h.owner.0 } else { w.get::<Parent>(e).map_or(0, |p| p.target.0) };
                if owner == 0 || w.entity(owner).and_then(|oe| w.get::<HitPoints>(oe)).is_none() {
                    problems.add("warning", Some(uid), "this Hitbox has no machine to pass the damage to (no Parent, and Hitbox.owner is not the uid of an entity with HitPoints)".into(), "put it on a child of the machine, or set Hitbox.owner");
                }
                if w.get::<HitPoints>(e).is_none() || w.get::<Collider>(e).is_none_or(|c| c.trigger) {
                    problems.add("warning", Some(uid), "a Hitbox is shot as a target of its own: it needs HitPoints (a max too big to be used up) and a Collider that is not a trigger".into(), "add HitPoints and a Collider (a small sphere on the weak side)");
                } else if w.get::<CollisionLayer>(e).is_none_or(|l| l.mask != 0) {
                    problems.add("warning", Some(uid), "a Hitbox without a CollisionLayer mask of 0 is a solid thing: the machine's own body walks into it and its paths plan round it".into(), "add CollisionLayer { layer: 30, mask: 0 } (a ghost that shots still meet)");
                }
                if owner != 0 && w.entity(owner).is_some_and(|oe| w.get::<Team>(oe).is_some_and(|t| t.id != 0)) && w.get::<Team>(e).is_none_or(|t| t.id == 0) {
                    problems.add("warning", Some(uid), "this Hitbox has no Team while its machine has one: the machine's own shells and blows hit it".into(), "give it the machine's Team");
                }
            }
            let mut cores: Vec<(u64, Escalation)> = Vec::new();
            w.for_each_uid::<(&Escalation,), _>(|_, uid, (c,)| cores.push((uid, *c)))?;
            let mut directors = 0usize;
            w.for_each::<(&Director,), _>(|_, (d,)| directors += d.active as usize)?;
            for (k, (uid, c)) in cores.iter().enumerate() {
                if k > 0 {
                    problems.add("warning", Some(*uid), "more than one Escalation: only the one with the lowest uid counts".into(), "keep one");
                }
                if directors == 0 {
                    problems.add("warning", Some(*uid), "an Escalation needs an active Director in the scene: the director is what keeps each player's alert and tier".into(), "add a Director");
                }
                let tiers = w.strings().get(c.tiers.0).to_string();
                if !tiers.trim().is_empty() {
                    for m in parse_tiers(&tiers).1 {
                        problems.add("warning", Some(*uid), format!("Escalation.tiers: {m}"), "a line is `tier N at=ALERT hunters=MULT cooldown=MULT alive=ADD per_heat=MULT`; the tiers are numbered from 1 and the thresholds rise");
                    }
                }
                for m in parse_noises(w.strings().get(c.noise.0)).1 {
                    problems.add("warning", Some(*uid), format!("Escalation.noise: {m}"), "a line is `EVENT GAIN`");
                }
                let name = w.strings().get(c.walker.0).to_string();
                if c.walker_tier > 0 && !name.is_empty() && !names.contains(&name) {
                    problems.add("warning", Some(*uid), format!("Escalation.walker `{name}` is not a prefab of this scene"), "define the prefab first (define_prefab) or fix the name");
                }
                if c.walker_min_dist > c.walker_max_dist {
                    problems.add("warning", Some(*uid), "Escalation.walker_min_dist is greater than walker_max_dist".into(), "the ring is walker_min_dist to walker_max_dist metres round the player");
                }
            }
        }
        // particle modules: the text parses, a cloud is there to use it, and what is dear is said so
        {
            use lunaris_sim::components::{ParticleModules, Particles};
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, lunaris_ecs::Entity, u32)> = Vec::new();
            w.for_each_uid::<(&ParticleModules,), _>(|e, uid, (m,)| found.push((uid, e, m.modules.0)))?;
            for (uid, e, sym) in found {
                let text = w.strings().get(sym).to_string();
                let cloud = w.get::<Particles>(e).copied();
                if cloud.is_none() {
                    problems.add("warning", Some(uid), "ParticleModules needs a Particles cloud on the same entity".into(), "add a Particles component");
                }
                match lunaris_sim::particle_modules::parse(&text) {
                    Err(err) => problems.add("error", Some(uid), format!("ParticleModules.modules: {err}"), "one module a line: color, size, wind, attract, vortex, noise, collide, trail, mesh (docs/GAMEPLAY.md)"),
                    Ok(m) => {
                        if let Some(c) = cloud {
                            let count = c.count.min(lunaris_render::particles::MAX_PARTICLES) as usize;
                            if m.solid.is_some() && count > lunaris_render::particles::MAX_SOLIDS {
                                problems.add("warning", Some(uid), format!("`mesh` draws at most {} solids; Particles.count {} is more", lunaris_render::particles::MAX_SOLIDS, c.count), "lower the count or split the cloud");
                            }
                            if let Some(t) = m.trail {
                                if count * (t.segments + 1) > lunaris_render::particles::MAX_TRAIL_POINTS {
                                    problems.add("warning", Some(uid), format!("{} particles with trails of {} points are more than the {} ribbon points a cloud has: the last ones get none", count, t.segments + 1, lunaris_render::particles::MAX_TRAIL_POINTS), "fewer particles or segments");
                                }
                            }
                            let noisy = m.forces.iter().any(|f| matches!(f, lunaris_sim::particle_modules::Force::Noise { .. }));
                            if m.dynamic() && count > 20_000 && noisy {
                                problems.add("warning", Some(uid), format!("a `noise` force on {count} particles costs a lot every frame (each particle is stepped from its birth)"), "use `turbulence` on the cloud, or fewer particles");
                            }
                        }
                    }
                }
            }
        }
        // particle systems (docs/FX.md): the effect reads (a file of the workspace or the JSON in the component), its pictures are there, and what it can cost is said
        {
            use lunaris_sim::components::ParticleSystem;
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, u32, f32, f32)> = Vec::new();
            w.for_each_uid::<(&ParticleSystem,), _>(|_, uid, (s,)| found.push((uid, s.effect.0, s.scale, s.quality)))?;
            for (uid, sym, scale, quality) in found {
                let text = w.strings().get(sym).to_string();
                let trimmed = text.trim();
                let source: std::result::Result<String, String> = if trimmed.starts_with('{') {
                    Ok(trimmed.to_string())
                } else if trimmed.is_empty() {
                    Err("ParticleSystem.effect is empty".to_string())
                } else if !file_exists(&root, trimmed) {
                    Err(format!("the effect file `{trimmed}` is not in the workspace"))
                } else {
                    std::fs::read_to_string(root.join(lunaris_json::safe_relative(trimmed).unwrap_or_default())).map_err(|e| format!("cannot read `{trimmed}`: {e}"))
                };
                let effect = match source.and_then(|s| lunaris_sim::fx::parse(&s)) {
                    Ok(e) => e,
                    Err(e) => {
                        problems.add("error", Some(uid), format!("ParticleSystem.effect: {e}"), "an effect is JSON with a list of `emitters` (docs/FX.md); the message says where the mistake is");
                        continue;
                    }
                };
                for m in &effect.warnings {
                    problems.add("warning", Some(uid), format!("ParticleSystem.effect: {m}"), "docs/FX.md lists every key");
                }
                for (i, e) in effect.emitters.iter().enumerate() {
                    let tex = &e.render.texture;
                    if !tex.is_empty() && !lunaris_render::texture::is_procedural_texture(tex) && !file_exists(&root, tex) {
                        problems.add("warning", Some(uid), format!("ParticleSystem.effect: emitters[{i}] (`{}`) uses the picture `{tex}`, which is not in the workspace", e.name), "`files` lists the pictures; the built-in ones are soft, circle, ring, gradient, checker");
                    }
                }
                // the most that can be alive: a stream holds rate times the longest life, a burst its count, an event what it asks for each particle it follows; the effect's budget cuts the rest
                let worst: f32 = effect
                    .emitters
                    .iter()
                    .map(|e| {
                        let life = e.init.life[1];
                        let bursts: f32 = e.spawn.bursts.iter().map(|b| b.count[1] as f32).sum::<f32>() * if e.looping && e.duration > 0.0 { (life / e.duration).ceil().max(1.0) } else { 1.0 };
                        (e.spawn.rate * life + bursts).min(e.max as f32)
                    })
                    .sum();
                let allowed = effect.budget as f32 * quality.clamp(0.1, 1.0);
                if worst > allowed * 1.5 && worst > 2000.0 {
                    problems.add("warning", Some(uid), format!("ParticleSystem.effect: its emitters can hold {worst:.0} particles at once and the budget is {allowed:.0}: the rest are never born"), "raise `budget`, or lower the rates and lives");
                }
                if scale <= 0.0 {
                    problems.add("warning", Some(uid), "ParticleSystem.scale is 0 or less: nothing is drawn".into(), "scale is a multiplier, 1 is the effect as written");
                }
            }
        }
        // inverse kinematics: the goal needs a model to bend
        {
            use lunaris_sim::components::{Animator, Ik, MeshRenderer};
            let w = &mut self.sim.world;
            let mut goals: Vec<(u64, u64, u32)> = Vec::new();
            let mut parents: Vec<(u64, u64, u32, lunaris_ecs::Entity)> = Vec::new();
            w.for_each_uid::<(&Ik,), _>(|e, uid, (g,)| parents.push((uid, g.skin.0, g.joint.0, e)))?;
            for (uid, skin, joint, e) in parents {
                // (skin 0: the model this goal is a child of)
                let skin = if skin != 0 { skin } else { w.get::<lunaris_sim::components::Parent>(e).map_or(0, |p| p.target.0) };
                goals.push((uid, skin, joint));
            }
            for (uid, skin, joint) in goals {
                let name = w.strings().get(joint).to_string();
                if name.is_empty() {
                    problems.add("warning", Some(uid), "Ik.joint is empty, so there is nothing to bend".into(), "the name of a joint of the model (`model_info` lists them)");
                }
                match w.entity(skin) {
                    None => problems.add("error", Some(uid), if skin == 0 { "Ik.skin is 0 and the goal is not a child of a model".to_string() } else { format!("Ik.skin {skin} is not an entity") }, "the uid of the entity with the animated model (or make the goal a child of it and leave skin 0)"),
                    Some(e) => {
                        if w.get::<MeshRenderer>(e).is_none() || w.get::<Animator>(e).is_none() {
                            problems.add("warning", Some(uid), format!("the entity {skin} that Ik.skin names has no MeshRenderer and Animator, so it is not posed and the goal does nothing"), "an animated model needs a MeshRenderer (a skinned .glb) and an Animator");
                        }
                    }
                }
            }
        }
        // music: every track of a playlist must be a file that exists (or a built-in voice)
        {
            use lunaris_sim::components::MusicPlayer;
            let w = &mut self.sim.world;
            let mut lists: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&MusicPlayer,), _>(|_, uid, (m,)| lists.push((uid, m.playlist.0)))?;
            for (uid, sym) in lists {
                let text = w.strings().get(sym).to_string();
                let tracks: Vec<&str> = text.split(['\n', ';']).map(|l| l.split('#').next().unwrap_or("").split_whitespace().next().unwrap_or("")).filter(|t| !t.is_empty()).collect();
                if tracks.is_empty() {
                    problems.add("warning", Some(uid), "MusicPlayer.playlist is empty, so there is no music".into(), "one track a line: a sound file of the workspace (music/theme.ogg) or voice:NAME");
                }
                for t in tracks {
                    if !t.starts_with("voice:") && !lunaris_json::safe_relative(t).is_ok_and(|n| root.join(n).is_file()) {
                        problems.add("error", Some(uid), format!("MusicPlayer track `{t}` is not a file of the workspace"), "a WAV, Ogg or MP3 path relative to the workspace folder; `files` with kind sound lists them");
                    }
                }
            }
        }
        // the interface beyond flat rectangles: styles, fonts and pins need what they style, a file that exists and an entity to follow
        {
            use lunaris_sim::components::{Text, TextFont, Ui, UiAnchor, UiStyle};
            let w = &mut self.sim.world;
            let mut styles: Vec<(u64, u32, f32)> = Vec::new();
            w.for_each_uid::<(&UiStyle,), _>(|_, uid, (s,)| styles.push((uid, s.image.0, s.radius)))?;
            let mut fonts: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&TextFont,), _>(|_, uid, (f,)| fonts.push((uid, f.file.0)))?;
            let mut pins: Vec<(u64, u64)> = Vec::new();
            w.for_each_uid::<(&UiAnchor,), _>(|_, uid, (a,)| pins.push((uid, a.entity.0)))?;
            let has = |w: &lunaris_ecs::World, uid: u64, ui: bool, text: bool| {
                w.entity(uid).is_some_and(|e| (ui && w.get::<Ui>(e).is_some()) || (text && w.get::<Text>(e).is_some()))
            };
            for (uid, sym, radius) in styles {
                if !has(w, uid, true, false) {
                    problems.add("warning", Some(uid), "has a UiStyle but no Ui, so nothing is drawn".into(), "add a `Ui` rectangle: the style says how it looks");
                }
                if !radius.is_finite() || radius < 0.0 {
                    problems.add("warning", Some(uid), "UiStyle.radius must be a number from 0 up (a fraction of the picture's height)".into(), "0.02 is a gentle rounding, 0.5 makes a pill");
                }
                let name = w.strings().get(sym).to_string();
                // a picture a `CameraView` draws (`view:UID`) is a texture too
                if !name.is_empty() && !lunaris_render::is_procedural_texture(&name) && !name.starts_with("view:") && !root.join(&name).is_file() {
                    problems.add("error", Some(uid), format!("UiStyle.image `{name}` is not a built-in texture or a file of the workspace (a magenta square is drawn)"), "a PNG or JPEG path relative to the workspace folder; `files` with kind texture lists them");
                }
            }
            let mut checker = lunaris_render::fonts::FontCache::new(Some(root.clone()));
            for (uid, sym) in fonts {
                let name = w.strings().get(sym).to_string();
                if !has(w, uid, true, true) {
                    problems.add("warning", Some(uid), "has a TextFont but no Text or Ui, so there is nothing to write".into(), "put the TextFont on the same entity as the Text, or the Ui whose `text` label should use it");
                }
                if name.is_empty() {
                    problems.add("warning", Some(uid), "TextFont.file is empty (the built-in font is used)".into(), "a .ttf or .otf path relative to the workspace folder; `files` with kind font lists them");
                } else if let Err(e) = checker.layout(&name, "Ag", 16) {
                    problems.add("error", Some(uid), format!("TextFont.file `{name}` cannot be used: {e} (the built-in font is used)"), "a .ttf or .otf file inside the workspace folder; `files` with kind font lists them");
                }
            }
            for (uid, target) in pins {
                if !has(w, uid, true, true) {
                    problems.add("warning", Some(uid), "has a UiAnchor but no Ui or Text, so there is nothing to pin".into(), "put the UiAnchor on the Ui or Text that should follow the world point");
                }
                if target != 0 && w.entity(target).is_none() {
                    problems.add("error", Some(uid), format!("UiAnchor.entity {target} does not exist"), "the uid of the entity to follow (the pin is hidden until it does), or 0 to pin a fixed point `world`");
                }
            }
        }
        // gameplay components written as text or by path: say what is wrong with them now, not after a silent no-op
        {
            use lunaris_sim::components::{CameraRig, InputMap, Sensor, StateMachine, Tween};
            let w = &mut self.sim.world;
            let mut maps: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&InputMap,), _>(|_, uid, (m,)| maps.push((uid, m.bindings.0)))?;
            let mut machines: Vec<(u64, lunaris_ecs::Entity, u32)> = Vec::new();
            w.for_each_uid::<(&StateMachine,), _>(|e, uid, (m,)| machines.push((uid, e, m.machine.0)))?;
            let mut tweens: Vec<(u64, u32, u64)> = Vec::new();
            w.for_each_uid::<(&Tween,), _>(|_, uid, (t,)| tweens.push((uid, t.path.0, t.entity.0)))?;
            let mut rigs: Vec<(u64, u64)> = Vec::new();
            w.for_each_uid::<(&CameraRig,), _>(|_, uid, (r,)| rigs.push((uid, r.target.0)))?;
            // a 2D picture with a rig that turns the camera toward its target (the default `look_at` with a `look_offset` of [0, 1, 0]) is tilted a few degrees: the layers are
            // sorted by their depth along the tilted view, so a big tilemap can come out in front of the sprites that stand on it
            let mut tilted: Vec<u64> = Vec::new();
            w.for_each_uid::<(&CameraRig, &Camera), _>(|_, uid, (r, c)| {
                if c.ortho && r.look_at && (r.look_offset[0] != 0.0 || r.look_offset[1] != 0.0) {
                    tilted.push(uid);
                }
            })?;
            // bodies of a 2D scene (an orthographic camera) stand in one plane: the contact normal of two solid bodies at different z has a part along z, which pushes
            // them out of the plane and apart, so they stop meeting (a hero drawn "above" a guard by a bigger z walks through it)
            // (the picture is the one of the active camera with the lowest uid: an orthographic camera that is switched off, a minimap's, does not make a 3D scene 2D)
            let mut main_camera: Option<(u64, bool)> = None;
            w.for_each_uid::<(&Camera,), _>(|_, uid, (c,)| {
                if c.active && main_camera.is_none_or(|b| uid < b.0) {
                    main_camera = Some((uid, c.ortho));
                }
            })?;
            let ortho = main_camera.is_some_and(|c| c.1);
            let mut collide: Vec<u64> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::CameraCollide,), _>(|_, uid, _| collide.push(uid))?;
            for uid in collide {
                if !rigs.iter().any(|r| r.0 == uid) {
                    problems.add("warning", Some(uid), "has a CameraCollide but no CameraRig, so nothing places the camera it would keep out of the walls".into(), "put it on the entity with the CameraRig");
                }
            }
            // a solid box that is turned (not by a multiple of 90 degrees): physics keeps its true shape, but characters and navigation agents see only the axis-aligned box round it,
            // which is bigger and can close a doorway (a ruin built from turned walls was sealed in Caldera)
            let mut movers = false;
            w.for_each_uid::<(&lunaris_sim::components::CharacterBody,), _>(|_, _, _| movers = true)?;
            w.for_each_uid::<(&lunaris_sim::components::NavAgent,), _>(|_, _, _| movers = true)?;
            if movers {
                let mut moving: BTreeSet<u64> = BTreeSet::new();
                w.for_each_uid::<(&lunaris_sim::components::Velocity,), _>(|_, uid, _| {
                    moving.insert(uid);
                })?;
                let mut turned: Vec<u64> = Vec::new();
                w.for_each_uid::<(&Collider, &Transform), _>(|_, uid, (c, t)| {
                    // (a square trunk or rock turned any way is at most twice as big from above: only long walls turned a little are a problem)
                    if c.shape == 1 && !c.trigger && !moving.contains(&uid) && turned_growth(t.rot, [0.5 * c.size[0].abs() * t.scale[0], 0.5 * c.size[1].abs() * t.scale[1], 0.5 * c.size[2].abs() * t.scale[2]]) > 2.3 {
                        turned.push(uid);
                    }
                })?;
                if let Some(first) = turned.first() {
                    problems.add(
                        "warning",
                        Some(*first),
                        format!("{} long solid box Colliders (walls) are turned by an angle that is not a multiple of 90 degrees (first: entity {}); characters and navigation agents see only the axis-aligned box round a turned box, which is much bigger and can close a gap or a doorway", turned.len(), first),
                        "keep walls that must be walked through or along square to the axes (docs/NAVIGATION.md), or build a turned wall from several small boxes",
                    );
                }
            }
            if ortho {
                let mut bodies: Vec<(u64, f32)> = Vec::new();
                w.for_each_uid::<(&lunaris_sim::components::RigidBody, &Collider, &Transform), _>(|_, uid, (_, c, t)| {
                    if !c.trigger {
                        bodies.push((uid, t.pos[2]));
                    }
                })?;
                let lo = bodies.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1));
                let hi = bodies.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1));
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    if hi.1 - lo.1 > 0.001 {
                        problems.add(
                            "warning",
                            Some(hi.0),
                            format!("solid bodies of a 2D scene are at different z (entity {} at {}, entity {} at {}): their contacts push along z and they drift apart", lo.0, lo.1, hi.0, hi.1),
                            "keep bodies in one plane (z 0) and put the drawing order in a `DrawLayer` (lower first, the highest on top) instead of in z",
                        );
                    }
                }
            }
            let mut has_tilemap = false;
            w.for_each_uid::<(&Tilemap,), _>(|_, _, _| has_tilemap = true)?;
            if has_tilemap {
                for uid in tilted {
                    problems.add("warning", Some(uid), "an orthographic camera with a CameraRig that looks at its target (`look_at`, `look_offset` [0, 1, 0] by default) tilts the 2D picture, and sprites can then be drawn behind the tilemap".into(), "for a top-down 2D game set the rig's `look_at` to false (and keep the camera's own rotation unturned)");
                }
            }
            let mut sensors: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&Sensor,), _>(|_, uid, (r,)| sensors.push((uid, r.filter.0)))?;
            for (uid, sym) in maps {
                let text = w.strings().get(sym).to_string();
                match lunaris_sim::gameplay::parse_actions(&text) {
                    Err(e) => problems.add("error", Some(uid), format!("InputMap.bindings: {e}"), "one action per line: jump = space, gamepad.a   or   move_x = d, a:-1"),
                    Ok((actions, _)) => {
                        // a misspelt controller input reads nothing, for ever: say so
                        for (action, terms) in actions {
                            for (resource, _) in terms {
                                let Some(name) = resource.strip_prefix("input.gamepad") else { continue };
                                let (pad, what) = name.split_once('.').unwrap_or((name, ""));
                                let known_pad = pad.is_empty() || matches!(pad, "2" | "3" | "4");
                                if !known_pad || !(what == "connected" || lunaris_pad::BUTTONS.contains(&what) || lunaris_pad::AXES.contains(&what)) {
                                    problems.add(
                                        "warning",
                                        Some(uid),
                                        format!("InputMap action `{action}` reads `{resource}`, which no controller sends"),
                                        &format!("controller inputs are gamepad.NAME (gamepad2 to gamepad4 for more pads) with NAME one of {}, {} or connected", lunaris_pad::BUTTONS.join(" "), lunaris_pad::AXES.join(" ")),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            for (uid, _e, sym) in machines {
                let text = w.strings().get(sym).to_string();
                match lunaris_sim::gameplay::parse_machine(&text) {
                    Err(e) => problems.add("error", Some(uid), format!("StateMachine.machine: {e}"), "see docs/GAMEPLAY.md for the language"),
                    Ok(m) => {
                        let mut paths: Vec<String> = Vec::new();
                        for t in &m.trans {
                            if let Some(c) = &t.when {
                                paths.extend(c.paths());
                            }
                        }
                        for h in &m.hooks {
                            for a in &h.actions {
                                if let lunaris_sim::gameplay::Action::Field { path, .. } = a {
                                    paths.push(path.clone());
                                }
                            }
                        }
                        paths.sort();
                        paths.dedup();
                        for p in paths {
                            if let Err(e) = lunaris_sim::fieldio::slot(w, &p) {
                                problems.add("error", Some(uid), format!("StateMachine.machine: {e}"), "write Component.field or Component.field[index]");
                            }
                        }
                    }
                }
            }
            for (uid, sym, other) in tweens {
                let path = w.strings().get(sym).to_string();
                if let Err(e) = lunaris_sim::fieldio::slot(w, &path) {
                    problems.add("error", Some(uid), format!("Tween.path: {e}"), "name a number field, e.g. Transform.pos[1]");
                }
                if other != 0 && w.entity(other).is_none() {
                    problems.add("warning", Some(uid), format!("the Tween's entity {other} does not exist"), "");
                }
            }
            for (uid, target) in rigs {
                if w.entity(target).is_none() {
                    problems.add("warning", Some(uid), format!("the CameraRig's target {target} does not exist, so the camera stays where it is"), "set `target` to the uid of what the camera should follow");
                }
            }
            let mut trees: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::BehaviorTree,), _>(|_, uid, (t,)| trees.push((uid, t.script.0)))?;
            for (uid, sym) in trees {
                let text = w.strings().get(sym).to_string();
                match lunaris_sim::gameplay::parse_tree(&text) {
                    Err(e) => problems.add("error", Some(uid), format!("BehaviorTree.script: {e}"), "nodes one per line, children indented two spaces: see docs/GAMEPLAY.md"),
                    Ok(nodes) => {
                        use lunaris_sim::gameplay::{Action, BtKind};
                        let mut paths: Vec<String> = Vec::new();
                        for n in &nodes {
                            match &n.kind {
                                BtKind::Cond(c) => paths.extend(c.paths()),
                                BtKind::Act(list) => paths.extend(list.iter().filter_map(|a| if let Action::Field { path, .. } = a { Some(path.clone()) } else { None })),
                                _ => {}
                            }
                        }
                        paths.sort();
                        paths.dedup();
                        for p in paths {
                            if let Err(e) = lunaris_sim::fieldio::slot(w, &p) {
                                problems.add("error", Some(uid), format!("BehaviorTree.script: {e}"), "write Component.field or Component.field[index]");
                            }
                        }
                    }
                }
            }
            let mut zones: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::StreamZone,), _>(|_, uid, (z,)| zones.push((uid, z.prefab.0)))?;
            for (uid, sym) in zones {
                let name = w.strings().get(sym).to_string();
                if !self.sim.scripts.prefabs.contains_key(&name) {
                    problems.add("warning", Some(uid), format!("StreamZone.prefab `{name}` is not a defined prefab, so nothing is ever loaded"), "define it with define_prefab (see `prefabs`)");
                }
            }
            // ---- streaming: a procedural grid that looks at a great many cells, zones that all load in the step they are wanted (docs/GAMEPLAY.md, "StreamAnchor", docs/REALWORLD.md)
            let mut grids: Vec<(u64, lunaris_sim::components::ProcGrid)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::ProcGrid,), _>(|_, uid, (g,)| grids.push((uid, *g)))?;
            for (uid, g) in &grids {
                let cell = g.cell.max(10.0);
                let side = (2.0 * g.radius.max(1.0) / cell).ceil() + 1.0;
                if side * side > MAX_GRID_WINDOW {
                    problems.add("warning", Some(*uid), format!("this ProcGrid looks at about {:.0} cells round every anchor (radius {} m, cell {} m): with a hundred anchors that is thousands of cells measured in each step it runs", side * side, g.radius, g.cell), "make the cells bigger or the radius smaller, and set the resource procgrid.every (a whole number: look at the cells every that many steps)");
                }
            }
            let mut zones_n = 0usize;
            w.for_each_uid::<(&lunaris_sim::components::StreamZone,), _>(|_, _, _| zones_n += 1)?;
            let loads = w.resource("stream.loads_per_tick").unwrap_or(0.0);
            if (zones_n >= STREAM_ZONES_UNLIMITED || !grids.is_empty()) && !(loads >= 1.0) {
                problems.add("warning", None, format!("{} with no limit on the zones loaded in a tick: every zone that comes into range loads in that tick, and a prefab of a few thousand entities takes tens of milliseconds to make", if grids.is_empty() { format!("{zones_n} StreamZones") } else { "a ProcGrid".to_string() }), "set the resource stream.loads_per_tick (a whole number, 2 to 4 for a city): the nearest zone loads first and the others the ticks after");
            }
            // ---- crowds of path finders: what a server with many of them has to ration (docs/NAVIGATION.md, "Crowds")
            let mut agents = 0usize;
            w.for_each_uid::<(&lunaris_sim::components::NavAgent,), _>(|_, _, _| agents += 1)?;
            let mut limits: Vec<(u64, lunaris_sim::components::NavLimits)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::NavLimits,), _>(|_, uid, (l,)| limits.push((uid, *l)))?;
            limits.sort_by_key(|l| l.0);
            if agents >= CROWD_AGENTS && limits.is_empty() {
                problems.add("warning", None, format!("{agents} NavAgents and no NavLimits: every request is searched in the tick it is made, and a target that cannot be reached is looked for in every direction, which can take a tick of tens of milliseconds"), "put one entity with NavLimits in the world (searches_per_tick, max_distance, max_searched: docs/NAVIGATION.md, \"Crowds\")");
            }
            if let Some((uid, l)) = limits.first() {
                if agents >= CROWD_AGENTS && l.max_searched == 0 {
                    problems.add("warning", Some(*uid), format!("NavLimits.max_searched is 0 with {agents} NavAgents: a search for a target that cannot be reached looks at every cell it can get to"), "set max_searched (a way that exists is found in a few thousand cells; 20000 caps a failure at about 50 ms in a city), and the resource nav.cells_per_step to spread what is left over several steps");
                }
            }
            let mut splines: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::Spline,), _>(|_, uid, (t,)| splines.push((uid, t.points.0)))?;
            for (uid, sym) in splines {
                if let Err(e) = lunaris_sim::gameplay::parse_points(w.strings().get(sym)) {
                    problems.add("error", Some(uid), format!("Spline.points: {e}"), "one point per `;` or line: 0 0 0; 5 0 0; 5 0 5");
                }
            }
            let mut walkers: Vec<(u64, u64)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::PathFollower,), _>(|_, uid, (t,)| walkers.push((uid, t.path.0)))?;
            for (uid, path) in walkers {
                let ok = w.entity(path).is_some_and(|pe| w.get::<lunaris_sim::components::Spline>(pe).is_some());
                if !ok {
                    problems.add("warning", Some(uid), format!("the PathFollower's path {path} is not an entity with a Spline, so it does not move"), "set `path` to the uid of a Spline entity");
                }
            }
            let mut timelines: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::Timeline,), _>(|_, uid, (t,)| timelines.push((uid, t.script.0)))?;
            for (uid, sym) in timelines {
                let text = w.strings().get(sym).to_string();
                match lunaris_sim::gameplay::parse_timeline(&text) {
                    Err(e) => problems.add("error", Some(uid), format!("Timeline.script: {e}"), "lines like: at 1.5 event boom   /   at 0 ramp Transform.pos[1] 0 -> 5 over 2"),
                    Ok(entries) => {
                        use lunaris_sim::gameplay::{Action, Step};
                        for x in entries {
                            let path = match &x.step {
                                Step::Ramp { path, .. } => Some(path.clone()),
                                Step::Do(Action::Field { path, .. }) => Some(path.clone()),
                                _ => None,
                            };
                            if let Some(Err(e)) = path.map(|p| lunaris_sim::fieldio::slot(w, &p)) {
                                problems.add("error", Some(uid), format!("Timeline.script: {e}"), "write Component.field or Component.field[index]");
                            }
                        }
                    }
                }
            }
            let mut spawners: Vec<(u64, u32)> = Vec::new();
            w.for_each_uid::<(&lunaris_sim::components::PrefabSpawner,), _>(|_, uid, (r,)| spawners.push((uid, r.prefab.0)))?;
            for (uid, sym) in spawners {
                let name = w.strings().get(sym).to_string();
                if !self.sim.scripts.prefabs.contains_key(&name) {
                    problems.add("warning", Some(uid), format!("PrefabSpawner.prefab `{name}` is not a defined prefab, so it spawns nothing"), "define it with define_prefab (see `prefabs`)");
                }
            }
            for (uid, sym) in sensors {
                let name = w.strings().get(sym).to_string();
                if name.is_empty() || w.component_id(&name).is_none() {
                    problems.add("error", Some(uid), format!("Sensor.filter `{name}` is not a component, so the sensor sees nothing"), "name the component that things to be seen have, e.g. Health");
                }
            }
        }
        // joints: the body that is pulled needs a collider, a rigid body with a mass and a velocity; the other end must exist
        {
            use lunaris_sim::components::{Collider, Joint, RigidBody, Velocity};
            let mut joints: Vec<(u64, Joint)> = Vec::new();
            self.sim.world.for_each_uid::<(&Joint,), _>(|_, uid, (j,)| joints.push((uid, *j)))?;
            joints.sort_by_key(|j| j.0);
            for (uid, j) in joints {
                let w = &self.sim.world;
                let e = w.entity(uid);
                let body = e.is_some_and(|e| w.get::<Collider>(e).is_some() && w.get::<Velocity>(e).is_some() && w.get::<RigidBody>(e).is_some_and(|r| r.mass > 0.0));
                if !body {
                    problems.add("warning", Some(uid), "has a Joint but is not a dynamic body, so the joint does nothing".into(), "add Collider, RigidBody (mass > 0) and Velocity: the joint pulls this body");
                }
                if j.target.0 != 0 && w.entity(j.target.0).and_then(|t| w.get::<Transform>(t)).is_none() {
                    problems.add("warning", Some(uid), format!("the Joint's target {} does not exist or has no Transform, so the joint does nothing", j.target.0), "name another end (`target`: the uid of a body, or of anything with a Transform) or use 0 and an `anchor` point");
                }
                if j.kind > 4 {
                    problems.add("warning", Some(uid), format!("Joint.kind {} is not 0 (rope), 1 (rod), 2 (spring), 3 (ball-and-socket) or 4 (hinge); it acts as a rope", j.kind), "");
                }
            }
        }
        layers::run(&mut self.sim.world, &mut problems)?;
        physics_joints::run(&mut self.sim.world, &mut problems)?;
        draw_order::run(&mut self.sim.world, &mut problems)?;
        language::run(&mut self.sim.world, &root, &mut problems)?;
        menus::run(&mut self.sim.world, &mut problems)?;
        ui_kit3::run(&mut self.sim.world, &mut problems)?;
        terrain_layers::run(&mut self.sim.world, &mut self.textures, &root, &mut problems)?;
        water::run(&mut self.sim, &mut problems)?;
        motion::run(&mut self.sim, &mut problems)?;
        render_fx::run(&mut self.sim.world, &mut problems)?;
        // world events (docs/EVENTS.md): a table that reads, prefabs that exist, a supply drop that has what the engine ages and places it by
        {
            use lunaris_sim::events::{parse_table, AirDrop, WorldEvent, WorldEvents};
            let names: Vec<String> = self.sim.scripts.prefabs.keys().cloned().collect();
            let w = &mut self.sim.world;
            let mut found: Vec<(u64, WorldEvents)> = Vec::new();
            w.for_each_uid::<(&WorldEvents,), _>(|_, uid, (c,)| found.push((uid, *c)))?;
            for (k, (uid, c)) in found.iter().enumerate() {
                if k > 0 {
                    problems.add("warning", Some(*uid), "more than one WorldEvents: only the active one with the lowest uid counts".into(), "keep one");
                }
                let text = w.strings().get(c.table.0).to_string();
                let (lines, bad) = parse_table(&text);
                for m in bad {
                    problems.add("warning", Some(*uid), format!("WorldEvents.table: {m}"), "a line is `event NAME kind=N prefab=PREFAB marker=PREFAB every=A..B first=S alive=N height=H life=S inside=M clear=R maxy=Y`");
                }
                for l in &lines {
                    for (what, p) in [("prefab", &l.prefab), ("marker", &l.marker)] {
                        if !p.is_empty() && !names.contains(p) {
                            problems.add("warning", Some(*uid), format!("WorldEvents.table: the {what} `{p}` of the event `{}` is not a prefab of this scene", l.name), "define the prefab first (define_prefab) or fix the name");
                        }
                    }
                }
            }
            let mut drops: Vec<(u64, lunaris_ecs::Entity)> = Vec::new();
            w.for_each_uid::<(&AirDrop,), _>(|e, uid, _| drops.push((uid, e)))?;
            for (uid, e) in drops {
                if w.get::<WorldEvent>(e).is_none() {
                    problems.add("warning", Some(uid), "an AirDrop needs a WorldEvent beside it: the engine ages it and takes the place and the height of its fall from that".into(), "add a WorldEvent (the scheduler adds one to what it spawns)");
                }
            }
        }
        // raycast vehicles: the chassis, the wheel table, the numbers; gravity (the lowest-uid PhysicsSettings, like the physics) tells how much weight a spring carries
        {
            let mut gravity = 9.8f32;
            let mut lowest = u64::MAX;
            self.sim.world.for_each_uid::<(&lunaris_sim::components::PhysicsSettings,), _>(|_, uid, (p,)| {
                if uid < lowest {
                    lowest = uid;
                    gravity = (p.gravity[0] * p.gravity[0] + p.gravity[1] * p.gravity[1] + p.gravity[2] * p.gravity[2]).sqrt();
                }
            })?;
            vehicle::run(&mut self.sim.world, &mut problems, self.sim.dt, if gravity > 0.0 { gravity } else { 9.8 })?;
        }
        let world = &mut self.sim.world;

        // ---- gather (sorted by uid so the report is stable)
        let mut cameras: Vec<(u64, bool)> = Vec::new(); // uid, active
        world.for_each_uid::<(&Camera,), _>(|_, uid, (c,)| cameras.push((uid, c.active)))?;
        let mut with_transform: BTreeSet<u64> = BTreeSet::new();
        world.for_each_uid::<(&Transform,), _>(|_, uid, _| {
            with_transform.insert(uid);
        })?;
        let meshes: Vec<(u64, String)>;
        let mesh_textures: Vec<(u64, String)>;
        let sprites: Vec<(u64, String)>;
        let tilemaps: Vec<(u64, Tilemap, String, String)>;
        let texts: Vec<(u64, String)>;
        let mut colliders: Vec<u64> = Vec::new();
        let sounds: Vec<(u64, String)>;
        {
            let mut raw_meshes: Vec<(u64, MeshRenderer)> = Vec::new();
            world.for_each_uid::<(&MeshRenderer,), _>(|_, uid, (m,)| raw_meshes.push((uid, *m)))?;
            let mut raw_sprites: Vec<(u64, Sprite)> = Vec::new();
            world.for_each_uid::<(&Sprite,), _>(|_, uid, (s,)| raw_sprites.push((uid, *s)))?;
            let mut raw_maps: Vec<(u64, Tilemap)> = Vec::new();
            world.for_each_uid::<(&Tilemap,), _>(|_, uid, (m,)| raw_maps.push((uid, *m)))?;
            let mut raw_texts: Vec<(u64, Text)> = Vec::new();
            world.for_each_uid::<(&Text,), _>(|_, uid, (t,)| raw_texts.push((uid, *t)))?;
            let mut raw_sounds: Vec<(u64, Sound)> = Vec::new();
            world.for_each_uid::<(&Sound,), _>(|_, uid, (s,)| raw_sounds.push((uid, *s)))?;
            world.for_each_uid::<(&Collider,), _>(|_, uid, _| colliders.push(uid))?;
            raw_meshes.sort_by_key(|x| x.0);
            raw_sprites.sort_by_key(|x| x.0);
            raw_maps.sort_by_key(|x| x.0);
            raw_texts.sort_by_key(|x| x.0);
            raw_sounds.sort_by_key(|x| x.0);
            colliders.sort_unstable();
            let st = world.strings();
            meshes = raw_meshes.iter().map(|(u, m)| (*u, st.get(m.mesh.0).to_string())).collect();
            mesh_textures = raw_meshes.iter().map(|(u, m)| (*u, st.get(m.texture.0).to_string())).filter(|(_, t)| !t.is_empty()).collect();
            sprites = raw_sprites.iter().map(|(u, s)| (*u, st.get(s.texture.0).to_string())).collect();
            tilemaps = raw_maps.iter().map(|(u, m)| (*u, *m, st.get(m.texture.0).to_string(), st.get(m.data.0).to_string())).collect();
            texts = raw_texts.iter().map(|(u, t)| (*u, st.get(t.value.0).to_string())).collect();
            sounds = raw_sounds.iter().map(|(u, s)| (*u, st.get(s.name.0).to_string())).collect();
        }
        cameras.sort_by_key(|c| c.0);
        let defined_resources: BTreeSet<String> = world.resources().keys().cloned().collect();

        // ---- the camera
        if !cameras.iter().any(|c| c.1 && with_transform.contains(&c.0)) {
            problems.add("warning", None, "no active Camera with a Transform: pictures use a default view".into(), "add an entity with Camera and Transform, or pass eye/target to `render`");
        }

        // ---- the CPU renderer: advice when the pictures would be drawn by the processor (no graphics card was offered) and the scene is heavy. The number is the engine's own measurement (`lunaris bench`, docs/PERFORMANCE.md):
        // the reference renderer draws 359,040 triangles at 960 x 540 in 40 ms on one thread of an i7-9700K, so about 300,000 hold 30 pictures a second on one core (the work follows the triangles, not the meshes).
        if self.default_quality == "reference" {
            let mut found: Vec<(bool, u32)> = Vec::new();
            world.for_each_uid::<(&MeshRenderer,), _>(|_, _, (m,)| found.push((m.visible, m.mesh.0)))?;
            let mut named: BTreeMap<u32, u64> = BTreeMap::new();
            for (visible, sym) in found {
                if visible {
                    *named.entry(sym).or_default() += 1;
                }
            }
            let mut triangles: u64 = 0;
            for (sym, n) in named {
                let name = world.strings().get(sym);
                // (a model's triangles are guessed from the size of its file, about 60 bytes each; a built-in's are counted)
                let each = match builtin(name) {
                    Some(m) => (m.idx.len() / 3) as u64,
                    None => std::fs::metadata(root.join(name)).map(|m| m.len() / 60).unwrap_or(0),
                };
                triangles += each * n;
            }
            if triangles > CPU_TRIANGLES_ADVICE {
                problems.add("info", None, format!("about {triangles} triangles are drawn: with no graphics card the processor draws the pictures, and it holds about {CPU_TRIANGLES_ADVICE} triangles of a 960 x 540 picture on one core at 30 a second (an i7-9700K; a slower machine holds less, a picture with more pixels too)"), "use a GPU quality, fewer or simpler meshes (models with levels of detail), or smaller pictures");
            }
        }

        // ---- a camera that looks away from every mesh draws an empty picture (it looks along its local -Z)
        if sprites.is_empty() && tilemaps.is_empty() {
            let w = &*world;
            let tf = |uid: u64| w.entity(uid).and_then(|e| w.get::<Transform>(e)).map(|t| (t.pos, t.rot));
            if let Some((cpos, rot)) = cameras.iter().find(|c| c.1).and_then(|c| tf(c.0)) {
                // (the quaternion's turn of (0, 0, -1))
                let (q, v) = ([rot[0], rot[1], rot[2]], [0.0f32, 0.0, -1.0]);
                let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
                let t = cross(q, v);
                let t = [t[0] + rot[3] * v[0], t[1] + rot[3] * v[1], t[2] + rot[3] * v[2]];
                let u = cross(q, t);
                let fwd = [v[0] + 2.0 * u[0], v[1] + 2.0 * u[1], v[2] + 2.0 * u[2]];
                let placed: Vec<[f32; 3]> = meshes.iter().filter_map(|(uid, _)| tf(*uid).map(|p| p.0)).collect();
                let finite = fwd.iter().chain(cpos.iter()).all(|x| x.is_finite());
                if finite && !placed.is_empty() && placed.iter().all(|p| (p[0] - cpos[0]) * fwd[0] + (p[1] - cpos[1]) * fwd[1] + (p[2] - cpos[2]) * fwd[2] <= 0.0) {
                    problems.add("warning", cameras.iter().find(|c| c.1).map(|c| c.0), "the active Camera looks along its local -Z and every mesh is behind it: pictures are empty".into(), "turn the camera (Transform.rot) or move it; `render` also takes eye and target to look at a point");
                }
            }
        }

        // ---- light: meshes with no Light and no Environment are drawn in one flat ambient colour, which looks like a picture with no shapes in it
        {
            use lunaris_sim::components::{Environment, Light};
            let (mut lights, mut environments) = (0usize, 0usize);
            world.for_each_uid::<(&Light,), _>(|_, _, _| lights += 1)?;
            world.for_each_uid::<(&Environment,), _>(|_, _, _| environments += 1)?;
            let shaded = meshes.iter().filter(|(u, _)| with_transform.contains(u)).count();
            // (a flat 2D game keeps its colours as authored and needs none: sprites and tile maps, or a lone mesh, are not asked to)
            if shaded >= 2 && sprites.is_empty() && tilemaps.is_empty() && lights == 0 && environments == 0 {
                problems.add("warning", None, format!("{shaded} meshes and no Light or Environment: surfaces are drawn in one flat ambient colour, with no shading"), "add a directional Light (its rot aims the sun) and an Environment for the sky and the glow");
            }
        }

        // ---- things that are drawn
        // (`view:UID` is the picture a `CameraView` draws, a texture too: the monitor of docs/GAMEPLAY.md)
        let texture_ok = |name: &str| name.is_empty() || lunaris_render::is_procedural_texture(name) || name.starts_with("view:") || file_exists(&root, name);
        for (uid, mesh) in &meshes {
            if !with_transform.contains(uid) {
                problems.add("error", Some(*uid), "has a MeshRenderer but no Transform, so it is never drawn".into(), "add a Transform");
            }
            if mesh.is_empty() {
                problems.add("error", Some(*uid), "MeshRenderer.mesh is empty".into(), "use cube, sphere, plane, cylinder, quad or a .glb / .gltf path");
            } else if builtin(mesh).is_none() {
                if !lunaris_render::is_model_name(mesh) {
                    problems.add("error", Some(*uid), format!("unknown mesh `{mesh}`"), "built-ins: cube, sphere, plane, cylinder, quad; models end in .glb or .gltf");
                } else if !file_exists(&root, mesh) {
                    problems.add("error", Some(*uid), format!("the model file `{mesh}` is not in the workspace"), "paths are relative to the workspace folder");
                }
            }
        }
        // a mesh with a 0 in its scale has no volume on that axis and draws nothing (a way of hiding it that `visible` says better)
        {
            let w = &self.sim.world;
            for (uid, _) in &meshes {
                let Some(t) = w.entity(*uid).and_then(|e| w.get::<lunaris_sim::components::Transform>(e)) else { continue };
                if t.scale.iter().any(|s| *s == 0.0) {
                    problems.add("warning", Some(*uid), format!("Transform.scale {:?} has a 0: the mesh has no volume and is not drawn", t.scale), "set the scale to 1 (or the size you want); to hide a mesh on purpose set MeshRenderer.visible to false");
                }
            }
        }
        for (uid, tex) in &mesh_textures {
            if !texture_ok(tex) {
                problems.add("error", Some(*uid), format!("the mesh texture `{tex}` was not found"), "use checker, gradient, one of the materials (grass, dirt, stone, bricks, wood, sand, concrete, tiles, metal) or a PNG path inside the workspace");
            }
        }
        // a `view:UID` that names nothing that draws a view is a picture that never comes
        {
            let w = &self.sim.world;
            for (uid, tex) in mesh_textures.iter().chain(sprites.iter()) {
                let Some(n) = tex.strip_prefix("view:") else { continue };
                let drawn = n.parse::<u64>().ok().and_then(|n| w.entity(n)).is_some_and(|e| w.get::<lunaris_sim::components::CameraView>(e).is_some());
                if !drawn {
                    problems.add("warning", Some(*uid), format!("the texture `{tex}` names no entity with a CameraView, so no picture is drawn into it"), "`view:` is followed by the uid of the entity that has the CameraView (docs/GAMEPLAY.md)");
                }
            }
        }
        for (uid, tex) in &sprites {
            if !with_transform.contains(uid) {
                problems.add("error", Some(*uid), "has a Sprite but no Transform, so it is never drawn".into(), "add a Transform");
            }
            if !texture_ok(tex) {
                problems.add("error", Some(*uid), format!("the texture `{tex}` was not found"), "use circle, ring, checker, gradient, one of the materials (grass, dirt, stone, bricks, wood, sand, concrete, tiles, metal) or a PNG path inside the workspace");
            }
        }
        for (uid, m, tex, data) in &tilemaps {
            if !with_transform.contains(uid) {
                problems.add("error", Some(*uid), "has a Tilemap but no Transform, so it is never drawn".into(), "add a Transform");
            }
            if !texture_ok(tex) {
                problems.add("error", Some(*uid), format!("the tileset `{tex}` was not found"), "use a PNG path inside the workspace");
            }
            if m.sheet[0] == 0 || m.sheet[1] == 0 || m.size[0] == 0 || m.size[1] == 0 {
                problems.add("error", Some(*uid), "a Tilemap's sheet and size need at least 1 in each direction".into(), "");
            }
            let given = data.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).count();
            let cells = m.size[0] as usize * m.size[1] as usize;
            if given != cells {
                problems.add("warning", Some(*uid), format!("the Tilemap has {given} tile numbers for {} x {} = {cells} cells", m.size[0], m.size[1]), if given < cells { "the missing cells stay empty" } else { "the extra numbers are ignored" });
            }
        }
        for uid in &colliders {
            if !with_transform.contains(uid) {
                problems.add("warning", Some(*uid), "has a Collider but no Transform, so physics ignores it".into(), "add a Transform");
            }
        }

        // ---- graph systems and the resources they use
        let mut written: BTreeSet<String> = BTreeSet::new();
        // an `EventWatch` makes `event.NAME` (and its position) resources the engine writes
        {
            let mut syms: Vec<u32> = Vec::new();
            self.sim.world.for_each::<(&lunaris_sim::components::EventWatch,), _>(|_, (w,)| syms.push(w.names.0))?;
            for sym in syms {
                let text = self.sim.world.strings().get(sym).to_string();
                for n in text.split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()) {
                    for suffix in ["", ".x", ".y", ".z", ".uid"] {
                        written.insert(format!("event.{n}{suffix}"));
                    }
                    written.insert(format!("event.{n}.n"));
                    for k in 0..lunaris_sim::gameplay::EVENT_SLOTS {
                        for f in ["x", "y", "z", "uid"] {
                            written.insert(format!("event.{n}.{k}.{f}"));
                        }
                    }
                }
            }
        }
        // an `Inventory` with `publish` makes `PREFIX.KEY` (the count of every item of the item table) and `PREFIX.weight`, `.can0`, `.can1`, `.free`, `.sel` and `.held.*` resources the engine writes (inventory.rs)
        {
            use lunaris_sim::inventory::{table_of, Inventory};
            let mut prefixes: Vec<u32> = Vec::new();
            self.sim.world.for_each::<(&Inventory,), _>(|_, (i,)| {
                if i.publish.0 != 0 && !prefixes.contains(&i.publish.0) {
                    prefixes.push(i.publish.0);
                }
            })?;
            if !prefixes.is_empty() {
                let keys: Vec<String> = table_of(&mut self.sim.world).map(|(t, _)| t.items.iter().map(|it| it.key.clone()).collect()).unwrap_or_default();
                for sym in prefixes {
                    let prefix = self.sim.world.strings().get(sym).to_string();
                    for k in &keys {
                        written.insert(format!("{prefix}.{k}"));
                    }
                    for n in ["weight", "can0", "can1", "free", "sel", "held.id", "held.class", "held.power", "held.dur", "held.kind", "held.hp", "held.sp", "held.secs", "held.n", "crafting", "craft_left", "craft_total", "craft_todo", "craft_wait", "near"] {
                        written.insert(format!("{prefix}.{n}"));
                    }
                }
            }
        }
        for s in &self.sim.scripts.systems {
            written_resources(&s.def, &mut written);
        }
        for s in &self.sim.scripts.systems {
            if let Some(e) = &s.last_error {
                problems.add("error", None, format!("graph system `{}` failed at run time: {e}", s.name), "see `get_system` and fix the statement named in the message");
            }
            for r in s.resources() {
                // (the engine itself sets these: raw input, the actions of an InputMap, the time scale, saved-game answers, the screen shape)
                let engine_made = r.starts_with("input.") || r.starts_with("action.") || r.starts_with("game.") || r.starts_with("scene.") || r == "language.count" || r == "language.index" || r == "time.scale" || r == "screen.aspect";
                if !engine_made && !written.contains(r) && !defined_resources.contains(r) {
                    problems.add("warning", None, format!("graph system `{}` reads the resource `{r}`, which nothing sets (it always reads 0)", s.name), "set it with `set_resource`, or write it with res_set / res_add");
                }
            }
        }
        for (uid, template) in &texts {
            for name in placeholders(template) {
                if !name.starts_with("input.") && !written.contains(&name) && !defined_resources.contains(&name) {
                    problems.add("warning", Some(*uid), format!("the text shows `{{{name}}}`, but no resource has that name (it will read 0)"), "set it with `set_resource`, or write it with res_set / res_add");
                }
            }
        }
        for (uid, name) in &sounds {
            if name.is_empty() {
                problems.add("warning", Some(*uid), "a Sound without a name is never played".into(), "set Sound.name to the event name, for example coin");
            }
        }
        // ---- attached entities (Parent / LocalTransform)
        {
            let world = &mut self.sim.world;
            let mut parents: Vec<(u64, u64)> = Vec::new(); // (child, parent uid)
            world.for_each_uid::<(&Parent,), _>(|_, uid, (p,)| parents.push((uid, p.target.0)))?;
            let mut with_local: BTreeSet<u64> = BTreeSet::new();
            world.for_each_uid::<(&LocalTransform,), _>(|_, uid, _| {
                with_local.insert(uid);
            })?;
            parents.sort_unstable();
            let attached: BTreeSet<u64> = parents.iter().filter(|p| p.1 != 0).map(|p| p.0).collect();
            for (child, parent) in &parents {
                if *parent == 0 {
                    continue;
                }
                // an interface element inside another one (a row of a `UiLayout` column, the content of a `UiScroll` view, a panel of a tab) is not attached in the world: it needs no Transform
                let ui_link = world.entity(*child).is_some_and(|c| world.get::<lunaris_sim::components::Ui>(c).is_some() || world.get::<lunaris_sim::components::Text>(c).is_some())
                    && world.entity(*parent).is_some_and(|p| world.get::<lunaris_sim::components::Ui>(p).is_some());
                if !ui_link && !with_local.contains(child) {
                    problems.add("error", Some(*child), "has a Parent but no LocalTransform, so it does not follow the parent".into(), "add a LocalTransform (its position, rotation and scale relative to the parent)");
                }
                if !ui_link && !with_transform.contains(child) {
                    problems.add("error", Some(*child), "has a Parent but no Transform, so there is nothing for the parent to move".into(), "add a Transform");
                }
                if world.entity(*parent).is_none() {
                    problems.add("error", Some(*child), format!("its Parent {parent} does not exist, so it stays where it is"), "set Parent.target to the uid of an existing entity (0 = no parent)");
                } else if !ui_link && !with_transform.contains(parent) {
                    problems.add("error", Some(*child), format!("its Parent {parent} has no Transform, so it cannot follow it"), "add a Transform to the parent");
                } else {
                    // walk up: a chain that never ends is a cycle
                    let (mut at, mut steps) = (*parent, 0usize);
                    let parent_of: std::collections::BTreeMap<u64, u64> = parents.iter().copied().collect();
                    while let Some(&next) = parent_of.get(&at) {
                        if next == 0 {
                            break;
                        }
                        steps += 1;
                        if next == *child || steps > parents.len() {
                            problems.add("error", Some(*child), "is part of a Parent cycle (it is its own ancestor), so it stays where it is".into(), "break the loop: set one Parent.target to 0");
                            break;
                        }
                        at = next;
                    }
                }
            }
            for uid in &with_local {
                if !attached.contains(uid) {
                    problems.add("warning", Some(*uid), "has a LocalTransform but no Parent (or Parent.target is 0), so it is ignored".into(), "set Parent.target to the uid of the entity to attach to");
                }
            }
        }
        // ---- animators: the entity's mesh must be a model with a skeleton, and the clip must exist
        {
            let world = &mut self.sim.world;
            let mut animated: Vec<(u64, String, String, bool)> = Vec::new(); // uid, clip, mesh, has a MeshRenderer
            let mut raw: Vec<(u64, Animator)> = Vec::new();
            world.for_each_uid::<(&Animator,), _>(|_, uid, (a,)| raw.push((uid, *a)))?;
            raw.sort_by_key(|x| x.0);
            for (uid, a) in raw {
                let clip = world.strings().get(a.clip.0).to_string();
                let mesh = world.entity(uid).and_then(|e| world.get::<MeshRenderer>(e).map(|m| world.strings().get(m.mesh.0).to_string()));
                // a 2D flipbook (`Sprite` + `SpriteAnim`) is animated by the Animator's clock too: nothing to say about it here (the SpriteAnim check above judges it)
                if mesh.is_none() && world.entity(uid).is_some_and(|e| world.get::<lunaris_sim::components::SpriteAnim>(e).is_some()) {
                    continue;
                }
                animated.push((uid, clip, mesh.clone().unwrap_or_default(), mesh.is_some()));
            }
            for (uid, clip, mesh, has_mesh) in animated {
                if !has_mesh {
                    problems.add("warning", Some(uid), "has an Animator but no MeshRenderer, so there is nothing to animate".into(), "add a MeshRenderer whose mesh is an animated .glb model");
                    continue;
                }
                if !lunaris_render::is_model_name(&mesh) {
                    problems.add("warning", Some(uid), format!("has an Animator but its mesh `{mesh}` is not a model file, so nothing moves"), "animations come from .glb / .gltf models with a skeleton; `model_info` lists a model's clips");
                    continue;
                }
                let before = self.textures.warnings.len();
                let model = self.textures.model(&mesh);
                self.textures.warnings.truncate(before);
                match model.as_ref().and_then(|m| m.rig.as_ref()) {
                    None if model.is_some() => problems.add("warning", Some(uid), format!("has an Animator but the model `{mesh}` has no skeleton, so nothing moves"), "use a skinned model; `model_info` shows whether a model has a rig"),
                    Some(rig) if !clip.is_empty() && rig.clip_index(&clip).is_none() => problems.add("error", Some(uid), format!("the model `{mesh}` has no animation called `{clip}`"), &format!("its clips: {}", rig.clips.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", "))),
                    _ => {}
                }
            }
        }
        let errors = problems.0.iter().filter(|p| p.get("severity").and_then(|s| s.as_str()) == Some("error")).count();
        let warnings = problems.0.len() - errors;
        Ok(Json::obj([
            ("ok", Json::Bool(errors == 0)),
            ("errors", Json::from(errors)),
            ("warnings", Json::from(warnings)),
            ("problems", Json::Arr(problems.0)),
            ("checked", Json::obj([("entities", Json::from(self.sim.world.len())), ("graph_systems", Json::from(self.sim.scripts.systems.len())), ("resources", Json::from(self.sim.world.resources().len()))])),
        ]))
    }
}

