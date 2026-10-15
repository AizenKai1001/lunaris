//! The `check` tool: a static look at the whole world for mistakes that would otherwise show up as a blank picture or a silent
//! zero. It is for agents (and people) that have just built something: run it, fix what it lists, run it again.
use crate::engine::{uid_json, Args, Engine};
use lunaris_ecs::Result;
use lunaris_json::Json;
use lunaris_render::mesh::builtin;
use lunaris_sim::components::{Animator, Camera, Collider, LocalTransform, MeshRenderer, Parent, Sound, Sprite, Text, Tilemap, Transform};
use std::collections::BTreeSet;
use std::path::Path;

mod draw_order;
mod language;
mod layers;
mod menus;
mod vehicle;

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

fn file_exists(root: &Path, rel: &str) -> bool {
    lunaris_json::safe_relative(rel).is_ok_and(|norm| root.join(norm).is_file())
}

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
        // inverse kinematics: the goal needs a model to bend
        {
            use lunaris_sim::components::{Animator, Ik, MeshRenderer};
            let w = &mut self.sim.world;
            let mut goals: Vec<(u64, u64, u32)> = Vec::new();
            w.for_each_uid::<(&Ik,), _>(|_, uid, (g,)| goals.push((uid, g.skin.0, g.joint.0)))?;
            for (uid, skin, joint) in goals {
                let name = w.strings().get(joint).to_string();
                if name.is_empty() {
                    problems.add("warning", Some(uid), "Ik.joint is empty, so there is nothing to bend".into(), "the name of a joint of the model (`model_info` lists them)");
                }
                match w.entity(skin) {
                    None => problems.add("error", Some(uid), format!("Ik.skin {skin} is not an entity"), "the uid of the entity with the animated model"),
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
                if !name.is_empty() && !lunaris_render::is_procedural_texture(&name) && !root.join(&name).is_file() {
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
            let mut ortho = false;
            w.for_each_uid::<(&Camera,), _>(|_, _, (c,)| ortho |= c.ortho)?;
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
                if j.kind > 2 {
                    problems.add("warning", Some(uid), format!("Joint.kind {} is not 0 (rope), 1 (rod) or 2 (spring); it acts as a rope", j.kind), "");
                }
            }
        }
        layers::run(&mut self.sim.world, &mut problems)?;
        draw_order::run(&mut self.sim.world, &mut problems)?;
        language::run(&mut self.sim.world, &root, &mut problems)?;
        menus::run(&mut self.sim.world, &mut problems)?;
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

        // ---- things that are drawn
        let texture_ok = |name: &str| name.is_empty() || lunaris_render::is_procedural_texture(name) || file_exists(&root, name);
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
        for (uid, tex) in &mesh_textures {
            if !texture_ok(tex) {
                problems.add("error", Some(*uid), format!("the mesh texture `{tex}` was not found"), "use checker, gradient, one of the materials (grass, dirt, stone, bricks, wood, sand, concrete, tiles, metal) or a PNG path inside the workspace");
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
                let engine_made = r.starts_with("input.") || r.starts_with("action.") || r.starts_with("game.") || r == "time.scale" || r == "screen.aspect";
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
                if !with_local.contains(child) {
                    problems.add("error", Some(*child), "has a Parent but no LocalTransform, so it does not follow the parent".into(), "add a LocalTransform (its position, rotation and scale relative to the parent)");
                }
                if !with_transform.contains(child) {
                    problems.add("error", Some(*child), "has a Parent but no Transform, so there is nothing for the parent to move".into(), "add a Transform");
                }
                if world.entity(*parent).is_none() {
                    problems.add("error", Some(*child), format!("its Parent {parent} does not exist, so it stays where it is"), "set Parent.target to the uid of an existing entity (0 = no parent)");
                } else if !with_transform.contains(parent) {
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

