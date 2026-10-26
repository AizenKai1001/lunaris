//! Particle systems (docs/FX.md): the stepping is exact and deterministic, so the tests check the formulas, the counts, the events, the order and the picture.
use crate::fx_draw::{build, lights};
use crate::fx_sim::*;
use crate::fx_sim::curl as curl_at;
use crate::math::*;
use crate::particles::CameraBasis;
use crate::render::{render, RenderOptions};
use crate::texture::TextureCache;
use lunaris_json::{parse as json, Json};
use lunaris_sim::components::Transform;
use lunaris_sim::fx::{parse, Effect};
use lunaris_sim::{Command, Sim};
use std::sync::Arc;

fn effect(text: &str) -> Arc<Effect> {
    Arc::new(parse(text).unwrap_or_else(|e| panic!("{e}\n{text}")))
}

fn camera() -> CameraBasis {
    CameraBasis { pos: [0.0, 0.0, 10.0], forward: [0.0, 0.0, -1.0], right: [1.0, 0.0, 0.0], up: [0.0, 1.0, 0.0] }
}

fn job(e: &Arc<Effect>, uid: u64, time: f32) -> FxJob {
    FxJob { uid, effect: e.clone(), key: 1, time, seed: 5, scale: 1.0, intensity: 1.0, quality: 1.0, tint: [1.0; 4], tr: Transform::default(), camera: [0.0, 0.0, 10.0], frame: 1, gpu: false, world: None }
}

fn go(w: &mut FxWorld, j: &FxJob) {
    w.advance_many(std::slice::from_ref(j), 1);
}

/// Everything that is true of the particles of a system, as bits: two systems with the same list are the same.
fn fingerprint(w: &FxWorld, uid: u64) -> Vec<u32> {
    let sys = w.get(uid).expect("system");
    let mut out = vec![sys.step as u32];
    for em in &sys.em {
        out.push(em.parts.len() as u32);
        for p in &em.parts {
            out.extend(p.pos.iter().chain(p.vel.iter()).map(|v| v.to_bits()));
            out.extend([p.age.to_bits(), p.life.to_bits(), p.size.to_bits(), p.cs[0].to_bits(), p.id]);
        }
    }
    out
}

fn live(w: &FxWorld, uid: u64) -> usize {
    w.get(uid).map_or(0, |s| s.live())
}

const FIRE: &str = r#"{"emitters":[{"spawn":{"rate":90,"shape":{"type":"sphere","radius":0.3}},"init":{"life":[0.8,1.4],"speed":[1,2],"spread":30,"size":[0.1,0.3],"spin":[-90,90]},"update":{"gravity":[0,1,0],"drag":0.3,"noise":{"strength":2,"scale":0.7,"speed":0.5}}}]}"#;

#[test]
fn a_stream_fills_up_and_then_holds_its_number() {
    // 2 a step for a life of one second: 120 are alive, a few fewer at the moment of a step
    let e = effect(r#"{"emitters":[{"spawn":{"rate":120},"init":{"life":1,"size":0.1}}]}"#);
    let mut w = FxWorld::default();
    for t in [3.0, 3.5, 4.01] {
        go(&mut w, &job(&e, 1, t));
        let n = live(&w, 1);
        assert!((118..=122).contains(&n), "t = {t}: {n}");
    }
    // a stream that starts at time 0 is already full (the system is stepped from before time 0: prewarm)
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 0.0));
    assert!((118..=122).contains(&live(&w, 1)), "{}", live(&w, 1));
    // and the stream is smooth: every particle is born somewhere inside its step, so no two are at the same age
    let ages: Vec<f32> = w.get(1).unwrap().em[0].parts.iter().map(|p| p.age).collect();
    let mut distinct = ages.clone();
    distinct.sort_by(|a, b| a.total_cmp(b));
    distinct.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    assert!(distinct.len() > 100, "{} ages of {}", distinct.len(), ages.len());
}

#[test]
fn a_burst_makes_its_number_once_and_a_looping_burst_makes_it_every_round() {
    let one = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"time":0,"count":50}]},"init":{"life":2}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&one, 1, 0.5));
    assert_eq!(live(&w, 1), 50);
    go(&mut w, &job(&one, 1, 1.9));
    assert_eq!(live(&w, 1), 50);
    go(&mut w, &job(&one, 1, 2.6));
    assert_eq!(live(&w, 1), 0, "they have lived out their life and nothing makes more");
    // every round of a looping emitter with a duration: 20 every half second, living a second: 40 alive once it is going
    let rounds = effect(r#"{"emitters":[{"loop":true,"duration":0.5,"spawn":{"bursts":[{"time":0,"count":20}]},"init":{"life":1}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&rounds, 1, 3.2));
    assert_eq!(live(&w, 1), 40);
    // an emitter starts `delay` seconds late
    let late = effect(r#"{"emitters":[{"loop":false,"delay":1,"spawn":{"bursts":[{"time":0,"count":7}]},"init":{"life":5}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&late, 1, 0.9));
    assert_eq!(live(&w, 1), 0);
    go(&mut w, &job(&late, 1, 1.2));
    assert_eq!(live(&w, 1), 7);
}

#[test]
fn gravity_drag_and_the_speed_limit_do_what_the_steps_say() {
    let fall = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":10,"speed":0},"update":{"gravity":[0,-10,0]}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&fall, 1, 1.0));
    // born at step 0 and moved by 60 steps of a semi-implicit Euler: y = -g dt^2 n (n + 1) / 2
    let y = w.get(1).unwrap().em[0].parts[0].pos[1];
    let want = -10.0 * DT * DT * (60.0 * 61.0 / 2.0);
    assert!((y - want).abs() < 1e-3, "{y} vs {want}");
    // with drag the speed dies away: thrown at 10 m/s with k = 2, it has gone about 10 / 2 = 5 m in all
    let drag = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":10,"speed":10,"direction":[1,0,0]},"update":{"drag":2}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&drag, 1, 4.0));
    let x = w.get(1).unwrap().em[0].parts[0].pos[0];
    assert!((x - 5.0).abs() < 0.1, "{x}");
    // a speed limit
    let limit = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":10,"speed":0},"update":{"gravity":[0,-100,0],"speed_limit":3}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&limit, 1, 2.0));
    let v = length(w.get(1).unwrap().em[0].parts[0].vel);
    assert!((v - 3.0).abs() < 1e-3, "{v}");
}

#[test]
fn the_same_effect_seed_and_clock_give_the_same_particles_however_they_are_reached_and_on_how_many_threads() {
    let e = effect(FIRE);
    // one go, or a picture at a time
    let (mut a, mut b) = (FxWorld::default(), FxWorld::default());
    go(&mut a, &job(&e, 1, 0.0));
    go(&mut b, &job(&e, 1, 0.0));
    go(&mut a, &job(&e, 1, 1.0));
    let mut t = 0.0f32;
    for _ in 0..60 {
        t += 1.0 / 60.0;
        go(&mut b, &job(&e, 1, t));
    }
    assert_eq!(fingerprint(&a, 1), fingerprint(&b, 1));
    assert!(live(&a, 1) > 50);
    // another seed is another fire; the entity's number is the seed when there is none
    let mut c = FxWorld::default();
    go(&mut c, &FxJob { seed: 6, ..job(&e, 1, 1.0) });
    assert_ne!(fingerprint(&a, 1), fingerprint(&c, 1));
    let (mut d1, mut d2) = (FxWorld::default(), FxWorld::default());
    go(&mut d1, &FxJob { seed: 0, ..job(&e, 1, 1.0) });
    go(&mut d2, &FxJob { seed: 0, ..job(&e, 2, 1.0) });
    assert_ne!(fingerprint(&d1, 1), fingerprint(&d2, 2), "two fires of the same effect are not twins");
    // many systems on one thread or four
    let jobs: Vec<FxJob> = (1..=9).map(|u| FxJob { tr: Transform { pos: [u as f32, 0.0, 0.0], ..Transform::default() }, ..job(&e, u, 1.5) }).collect();
    let (mut one, mut four) = (FxWorld::default(), FxWorld::default());
    one.advance_many(&jobs, 1);
    four.advance_many(&jobs, 4);
    for u in 1..=9 {
        assert_eq!(fingerprint(&one, u), fingerprint(&four, u), "system {u}");
    }
    assert_eq!(one.stats.systems, 9);
}

#[test]
fn a_surface_bounces_sticks_or_kills_and_a_hit_is_an_event() {
    let make = |mode: &str| {
        effect(&format!(
            r#"{{"emitters":[
              {{"name":"ball","loop":false,"spawn":{{"bursts":[{{"count":1}}]}},"init":{{"life":10,"speed":5,"direction":[0,-1,0]}},"update":{{"gravity":[0,-9,0],"collide":{{"mode":"{mode}","bounce":0.5,"friction":0,"planes":[{{"normal":[0,1,0],"d":-1}}]}}}},
               "events":[{{"on":"collide","spawn":"sparks","count":3}}]}},
              {{"name":"sparks","child":true,"init":{{"life":5,"speed":2,"spread":180}}}}]}}"#
        ))
    };
    // bounce: it never goes through the floor, it goes up again, and the first hit made three sparks (a little hop later is under the limit)
    let bounce = make("bounce");
    let mut w = FxWorld::default();
    let (mut low, mut went_up) = (0.0f32, false);
    let mut t = 0.0f32;
    for _ in 0..90 {
        t += 1.0 / 60.0;
        go(&mut w, &job(&bounce, 1, t));
        if let Some(p) = w.get(1).unwrap().em[0].parts.first() {
            low = low.min(p.pos[1]);
            went_up |= p.vel[1] > 0.5;
        }
    }
    assert!(low >= -1.0001 && low < -0.99, "{low}");
    assert!(went_up);
    assert!(w.get(1).unwrap().em[1].parts.len() >= 3, "sparks: {}", w.get(1).unwrap().em[1].parts.len());
    // kill: gone at the floor, with sparks
    let kill = make("kill");
    let mut w = FxWorld::default();
    go(&mut w, &job(&kill, 1, 1.5));
    assert_eq!(w.get(1).unwrap().em[0].parts.len(), 0);
    assert_eq!(w.get(1).unwrap().em[1].parts.len(), 3);
    // stick: it rests on the floor
    let stick = make("stick");
    let mut w = FxWorld::default();
    go(&mut w, &job(&stick, 1, 2.0));
    let p = w.get(1).unwrap().em[0].parts[0];
    assert!(p.stuck && p.vel == [0.0; 3] && (p.pos[1] + 1.0).abs() < 1e-4, "{p:?}");
}

#[test]
fn balls_and_boxes_hold_particles_in_the_entitys_space_even_when_they_are_in_the_worlds() {
    // a box of half 1 round the entity that catches what falls on its lid, for a particle of the world's space on an entity that has moved
    let e = effect(r#"{"emitters":[{"space":"world","loop":false,"spawn":{"bursts":[{"count":1}],"shape":{"type":"point"}},"init":{"life":10,"speed":2,"direction":[0,-1,0]},"update":{"collide":{"mode":"stick","boxes":[{"pos":[0,0,0],"half":[1,1,1]}]}}}]}"#);
    let mut w = FxWorld::default();
    // the entity is 5 m up: the particle is born at its middle, inside the box, and is pushed out along the shortest way
    go(&mut w, &FxJob { tr: Transform { pos: [0.0, 5.0, 0.0], ..Transform::default() }, ..job(&e, 1, 1.0) });
    let p = w.get(1).unwrap().em[0].parts[0];
    assert!(p.stuck, "{p:?}");
    assert!((p.pos[1] - 5.0 - 1.0).abs() < 0.15 || (p.pos[1] - 5.0 + 1.0).abs() < 0.15, "on a face of the box: {p:?}");
}

#[test]
fn a_death_makes_the_particles_of_the_emitter_it_names_where_it_happened() {
    let e = effect(
        r#"{"emitters":[
          {"name":"shell","loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":0.5,"speed":4,"direction":[1,0,0]},"events":[{"on":"death","spawn":"burst","count":5}]},
          {"name":"burst","child":true,"init":{"life":2,"speed":[1,1],"spread":180}}]}"#,
    );
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 0.45));
    assert_eq!((w.get(1).unwrap().em[0].parts.len(), w.get(1).unwrap().em[1].parts.len()), (1, 0));
    go(&mut w, &job(&e, 1, 0.6));
    let sys = w.get(1).unwrap();
    assert_eq!(sys.em[0].parts.len(), 0);
    assert_eq!(sys.em[1].parts.len(), 5);
    let mean_x: f32 = sys.em[1].parts.iter().map(|p| p.pos[0]).sum::<f32>() / 5.0;
    // the shell was at about x = 4 * 0.5 = 2 when it died; five particles flying off at 1 m/s for a tenth of a second are within 0.1 m of that
    assert!((mean_x - 2.0).abs() < 0.2, "{mean_x}");
    assert!(sys.em[1].parts.iter().all(|p| (p.pos[0] - 2.0).abs() < 0.25));
    // and it takes along as much of the shell's speed as the event says
    let heir = effect(
        r#"{"emitters":[
          {"name":"shell","loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":0.5,"speed":4,"direction":[1,0,0]},"events":[{"on":"death","spawn":"burst","count":1,"inherit_velocity":0.5}]},
          {"name":"burst","child":true,"init":{"life":2,"speed":0}}]}"#,
    );
    let mut w = FxWorld::default();
    go(&mut w, &job(&heir, 1, 0.65));
    assert!((w.get(1).unwrap().em[1].parts[0].vel[0] - 2.0).abs() < 1e-3);
}

#[test]
fn particles_of_the_entitys_space_follow_it_and_those_of_the_worlds_stay_behind() {
    let e = effect(
        r#"{"emitters":[
          {"name":"in","space":"local","loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":10,"speed":0}},
          {"name":"out","space":"world","loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":10,"speed":0}}]}"#,
    );
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 0.2));
    go(&mut w, &FxJob { tr: Transform { pos: [5.0, 0.0, 0.0], ..Transform::default() }, ..job(&e, 1, 0.3) });
    let sys = w.get(1).unwrap();
    let world_of = |ei: usize| {
        let p = sys.em[ei].parts[0];
        if ei == 0 { sys.model.transform_point(p.pos) } else { p.pos }
    };
    assert!((world_of(0)[0] - 5.0).abs() < 1e-4, "{:?}", world_of(0));
    assert!(world_of(1)[0].abs() < 1e-4, "{:?}", world_of(1));
    // scaling the entity scales the picture of the one and the birth of the other: a world particle is born at the entity's size
    let sized = effect(r#"{"emitters":[{"space":"world","loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":10,"size":0.5,"speed":2,"direction":[0,1,0]}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &FxJob { scale: 3.0, ..job(&sized, 1, 0.1) });
    let p = w.get(1).unwrap().em[0].parts[0];
    assert!((p.size - 1.5).abs() < 1e-5 && (p.vel[1] - 6.0).abs() < 1e-4, "{p:?}");
}

#[test]
fn a_world_emitter_takes_along_the_speed_of_the_entity_it_comes_from() {
    let e = effect(r#"{"emitters":[{"space":"world","spawn":{"rate":60},"init":{"life":1,"speed":0,"inherit_velocity":1}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 1.0));
    // the entity flies at 6 m/s along x for the next second
    let mut t = 1.0f32;
    for k in 1..=60 {
        t += 1.0 / 60.0;
        go(&mut w, &FxJob { tr: Transform { pos: [6.0 * k as f32 / 60.0, 0.0, 0.0], ..Transform::default() }, ..job(&e, 1, t) });
    }
    let newest = w.get(1).unwrap().em[0].parts.iter().min_by(|a, b| a.age.total_cmp(&b.age)).copied().unwrap();
    assert!((newest.vel[0] - 6.0).abs() < 0.2, "{newest:?}");
}

#[test]
fn curves_set_the_size_the_colour_and_the_opacity_over_the_life() {
    let e = effect(
        r##"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":1,"speed":0,"size":1,"color":"#ff8000"},
            "curves":{"size":[[0,2],[1,0.5]],"alpha":[[0,1],[1,0]],"color":[[0,"#ffffff"],[1,[0,0,1,1]]]},"render":{"blend":"add"}}]}"##,
    );
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 0.5));
    let sys = w.get(1).unwrap();
    let p = sys.em[0].parts[0];
    let u = p.age / p.life;
    let layers = build(sys, &camera(), 1);
    assert_eq!(layers.len(), 1);
    let m = &layers[0].mesh;
    assert_eq!(m.pos.len(), 4);
    let width = length(sub(m.pos[1], m.pos[0]));
    let want = 2.0 + (0.5 - 2.0) * u;
    assert!((width - want).abs() < 1e-4, "{width} vs {want}");
    // opacity 1 - u; colour: orange times the gradient from white to blue, as a linear colour (squared)
    assert!((m.texw[0] - (1.0 - u)).abs() < 1e-4);
    let (r, g, b) = (1.0 * (1.0 + (0.0 - 1.0) * u), 0.5019608 * (1.0 + (0.0 - 1.0) * u), 0.0 * (1.0 + (1.0 - 1.0) * u));
    let c = m.col[0];
    assert!((c[0] - r * r).abs() < 1e-3 && (c[1] - g * g).abs() < 1e-3 && (c[2] - b * b).abs() < 1e-3, "{c:?} vs {:?}", [r * r, g * g, b * b]);
    // added pictures are drawn as they come; the layer says so
    assert!(layers[0].additive);
    // the colour of the system multiplies
    let tinted = FxJob { tint: [0.5, 1.0, 1.0, 0.5], ..job(&e, 1, 0.5) };
    go(&mut w, &tinted);
    let m2 = &build(w.get(1).unwrap(), &camera(), 1)[0].mesh;
    assert!((m2.texw[0] - (1.0 - u) * 0.5).abs() < 1e-4);
}

#[test]
fn pictures_play_a_flipbook_over_the_life_by_chance_or_at_a_rate() {
    let sheet = |mode: &str| {
        effect(&format!(
            r#"{{"emitters":[{{"loop":false,"spawn":{{"bursts":[{{"count":40}}]}},"init":{{"life":1,"speed":0,"size":1}},"render":{{"sheet":[4,2],"frame_mode":"{mode}","fps":4}}}}]}}"#
        ))
    };
    let frame_of = |m: &crate::mesh::Mesh, k: usize| -> (u32, u32) {
        let (u, v) = (m.uv[k * 4][0], m.uv[k * 4][1]);
        // (the first corner of a quad is the frame's bottom left)
        ((u * 4.0).round() as u32, ((1.0 - v) * 2.0 - 0.5) as u32)
    };
    // over the life: half way through, frame 4 of 8 (the first column of the second row)
    let life = sheet("life");
    let mut w = FxWorld::default();
    go(&mut w, &job(&life, 1, 0.5));
    let m = &build(w.get(1).unwrap(), &camera(), 1)[0].mesh;
    let f = frame_of(m, 0);
    assert_eq!(f, (0, 1), "{f:?}");
    // at random: they spread over all eight
    let random = sheet("random");
    let mut w = FxWorld::default();
    go(&mut w, &job(&random, 1, 0.5));
    let m = &build(w.get(1).unwrap(), &camera(), 1)[0].mesh;
    let mut seen: Vec<(u32, u32)> = (0..40).map(|k| frame_of(m, k)).collect();
    seen.sort();
    seen.dedup();
    assert!(seen.len() >= 6, "{seen:?}");
}

#[test]
fn sprites_can_lie_in_a_plane_or_stretch_along_the_speed() {
    let flat = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":5,"speed":0,"size":2},"render":{"facing":"axis","axis":[0,1,0]}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&flat, 1, 0.5));
    let m = &build(w.get(1).unwrap(), &CameraBasis { pos: [0.0, 10.0, 0.0], forward: [0.0, -1.0, 0.0], right: [1.0, 0.0, 0.0], up: [0.0, 0.0, -1.0] }, 1)[0].mesh;
    assert!(m.pos.iter().all(|p| p[1].abs() < 1e-5), "lies in the plane y = 0");
    assert!((length(sub(m.pos[1], m.pos[0])) - 2.0).abs() < 1e-4);
    assert!(m.nrm[0][1] > 0.99, "shows its face to the camera above it");
    // stretched: as long as its size plus the speed times the stretch, and along the way it flies
    let fast = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":5,"speed":10,"direction":[0,1,0],"size":0.2},"render":{"facing":"velocity","stretch":0.1}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&fast, 1, 0.1));
    let m = &build(w.get(1).unwrap(), &camera(), 1)[0].mesh;
    let len = length(sub(m.pos[3], m.pos[0]));
    assert!((len - (0.2 + 10.0 * 0.1)).abs() < 1e-3, "{len}");
    let along = normalize(sub(m.pos[3], m.pos[0]));
    assert!(along[1].abs() > 0.999, "{along:?}");
}

#[test]
fn a_ribbon_runs_through_the_places_a_particle_has_been() {
    let e = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":9,"speed":3,"direction":[1,0,0],"size":0.4},"render":{"type":"ribbon","ribbon":{"points":6,"interval":0.05}}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 1.0));
    let layer = &build(w.get(1).unwrap(), &camera(), 1)[0];
    // the head and six places, two vertices each
    assert_eq!(layer.mesh.pos.len(), 14);
    assert_eq!(layer.mesh.idx.len(), 6 * 6);
    // the middles of the pairs go back along the way it came, a place every 0.05 s of a 3 m/s flight apart
    let mid = |k: usize| scale(add(layer.mesh.pos[k * 2], layer.mesh.pos[k * 2 + 1]), 0.5);
    // (the newest is as far from the head as the particle has flown since the last place was kept)
    assert!((0.0..0.16).contains(&(mid(0)[0] - mid(1)[0])));
    for k in 1..6 {
        let step = mid(k)[0] - mid(k + 1)[0];
        assert!((step - 0.15).abs() < 0.01, "segment {k}: {step}");
    }
    // it thins and fades towards the tail
    assert!(layer.mesh.texw[0] > layer.mesh.texw[12] && layer.mesh.texw[12] < 0.05);
    assert!(length(sub(layer.mesh.pos[1], layer.mesh.pos[0])) > length(sub(layer.mesh.pos[13], layer.mesh.pos[12])));
}

#[test]
fn solids_are_small_shapes_one_for_each_particle() {
    let e = effect(r#"{"emitters":[{"spawn":{"rate":30},"init":{"life":1,"size":0.1,"spin":[10,90]},"render":{"type":"solid","solid":"cube"}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 2.0));
    let n = live(&w, 1);
    let m = &build(w.get(1).unwrap(), &camera(), 1)[0].mesh;
    assert_eq!(m.pos.len(), n * 36);
    assert_eq!(m.idx.len(), n * 36);
}

#[test]
fn the_brightest_particles_give_the_light() {
    let e = effect(
        r##"{"emitters":[{"spawn":{"rate":60,"shape":{"type":"sphere","radius":1}},"init":{"life":1,"speed":0,"color":"#ff6020"},
            "curves":{"alpha":[[0,1],[1,0]]},"render":{"blend":"add","light":{"intensity":4,"range":7,"max":3}}}]}"##,
    );
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 2.0));
    let l = lights(w.get(1).unwrap());
    assert_eq!(l.len(), 3);
    assert!(l.iter().all(|x| x.point && x.range == 7.0 && x.color[0] > x.color[2]));
    // brightest first: the youngest (opacity falls with age)
    assert!(l[0].color[0] >= l[1].color[0] && l[1].color[0] >= l[2].color[0]);
    assert!(l[0].color[0] <= 4.0 + 1e-4);
    // no `light`, no light
    let dark = effect(r#"{"emitters":[{"spawn":{"rate":60},"init":{"life":1}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&dark, 1, 2.0));
    assert!(lights(w.get(1).unwrap()).is_empty());
}

#[test]
fn the_level_of_detail_the_budget_and_the_quality_thin_a_system_out() {
    let lod = effect(r#"{"lod":{"near":10,"far":50},"emitters":[{"spawn":{"rate":120},"init":{"life":1}}]}"#);
    let at = |d: f32| {
        let mut w = FxWorld::default();
        go(&mut w, &FxJob { camera: [0.0, 0.0, d], ..job(&lod, 1, 3.0) });
        live(&w, 1)
    };
    let (near, mid, far, beyond) = (at(5.0), at(30.0), at(49.0), at(80.0));
    assert!(near >= 118 && (mid as i32 - 60).abs() <= 6 && far < 15 && beyond == 0, "{near} {mid} {far} {beyond}");
    assert_eq!(lod_factor(&lod, 30.0), 0.5);
    // the budget holds however much is asked for, and so does an emitter's own number
    let rich = effect(r#"{"budget":50,"emitters":[{"max":20,"spawn":{"rate":2000},"init":{"life":5}},{"spawn":{"rate":2000},"init":{"life":5}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&rich, 1, 3.0));
    let sys = w.get(1).unwrap();
    assert!(sys.em[0].parts.len() <= 20 && sys.live() <= 50, "{} {}", sys.em[0].parts.len(), sys.live());
    assert!(sys.live() >= 45, "it does fill the budget: {}", sys.live());
    // intensity and quality scale the rate
    let plain = effect(r#"{"emitters":[{"spawn":{"rate":100},"init":{"life":1}}]}"#);
    let count = |i: f32, q: f32| {
        let mut w = FxWorld::default();
        go(&mut w, &FxJob { intensity: i, quality: q, ..job(&plain, 1, 3.0) });
        live(&w, 1) as i32
    };
    assert!((count(1.0, 1.0) - 100).abs() <= 3 && (count(0.5, 1.0) - 50).abs() <= 3 && (count(1.0, 0.25) - 25).abs() <= 3 && count(0.0, 1.0) == 0);
}

#[test]
fn a_system_that_was_out_of_view_for_long_starts_again_full_and_one_whose_clock_went_back_starts_over() {
    let e = effect(r#"{"emitters":[{"spawn":{"rate":120},"init":{"life":1}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 1.0));
    go(&mut w, &job(&e, 1, 200.0));
    let s = w.get(1).unwrap();
    assert!((118..=122).contains(&s.live()), "{}", s.live());
    assert!(s.steps_taken as i64 <= 62, "a loop starts again from its prewarm, not from 200 s ago: {}", s.steps_taken);
    // a gap of a second is stepped through
    go(&mut w, &job(&e, 1, 201.0));
    assert_eq!(w.get(1).unwrap().steps_taken, 60);
    // the clock went back: from the start
    go(&mut w, &job(&e, 1, 0.5));
    assert_eq!(w.get(1).unwrap().step, 31);
    assert!((118..=122).contains(&w.get(1).unwrap().live()));
    // another effect on the same entity: from the start
    let other = effect(r#"{"emitters":[{"spawn":{"rate":30},"init":{"life":1}}]}"#);
    go(&mut w, &FxJob { key: 2, ..job(&other, 1, 0.6) });
    assert!((28..=32).contains(&w.get(1).unwrap().live()), "{}", w.get(1).unwrap().live());
    // an effect that ends is stepped from its beginning when it is looked at late
    let burst = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"time":2,"count":9}]},"init":{"life":3}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&burst, 1, 3.0));
    assert_eq!(live(&w, 1), 9, "the burst at 2 s is there at 3 s though nobody looked before");
    // and a system nobody asks for is let go
    w.trim(1000, 600);
    assert!(w.is_empty());
}

#[test]
fn curl_noise_moves_particles_without_a_preferred_way_and_the_rest_of_the_forces_push_the_right_way() {
    let drift = |update: &str| -> V3 {
        let e = effect(&format!(
            r#"{{"emitters":[{{"spawn":{{"rate":200,"shape":{{"type":"sphere","radius":0.5}}}},"init":{{"life":2,"speed":0,"size":0.1}},"update":{update}}}]}}"#
        ));
        let mut w = FxWorld::default();
        go(&mut w, &job(&e, 1, 3.0));
        let parts = &w.get(1).unwrap().em[0].parts;
        let n = parts.len() as f32;
        parts.iter().fold([0.0; 3], |a, p| add(a, p.pos)).map(|v| v / n)
    };
    // noise: the particles spread out
    let e = effect(r#"{"emitters":[{"spawn":{"rate":200},"init":{"life":2,"speed":0,"size":0.1},"update":{"noise":{"strength":6,"scale":1,"speed":0.3}}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 3.0));
    let parts = &w.get(1).unwrap().em[0].parts;
    let spread = parts.iter().map(|p| length(p.pos)).sum::<f32>() / parts.len() as f32;
    assert!(spread > 0.3, "{spread}");
    // (the swirl is divergence free, which its own test checks: it carries particles along the field without piling them up; the field has no reason to average to nothing over a few cells)
    // wind pushes along x; an attractor pulls towards its point; a vortex turns them about its axis
    assert!(drift(r#"{"wind":[4,0,0]}"#)[0] > 1.0);
    let toward = drift(r#"{"attractors":[{"pos":[0,5,0],"strength":8,"radius":20,"falloff":0}]}"#);
    assert!(toward[1] > 1.0 && toward[0].abs() < 0.3, "{toward:?}");
    let swirl = effect(r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}],"shape":{"type":"point"}},"init":{"life":9,"speed":1,"direction":[1,0,0]},"update":{"vortices":[{"pos":[0,0,0],"axis":[0,1,0],"strength":20,"radius":10}]}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&swirl, 1, 0.1));
    let p = w.get(1).unwrap().em[0].parts[0];
    // a particle that starts along +x and is swirled about +y is pushed along cross(y, x) = -z
    assert!(p.vel[2] < -1.0, "{p:?}");
    // an attractor that kills what comes close
    let sink = effect(r#"{"emitters":[{"spawn":{"rate":100,"shape":{"type":"sphere","radius":2}},"init":{"life":9,"speed":0},"update":{"attractors":[{"pos":[0,0,0],"strength":30,"radius":10,"falloff":0,"kill":0.3}]}}]}"#);
    let mut w = FxWorld::default();
    go(&mut w, &job(&sink, 1, 3.0));
    // (it is looked for at the start of each step, and a particle falling in at 20 m/s gets 0.3 m further in a step: a few are inside, none stays)
    let inside = w.get(1).unwrap().em[0].parts.iter().filter(|p| length(p.pos) < 0.3).count();
    assert!(inside <= 4, "{inside}");
    assert!(live(&w, 1) < 100, "{}", live(&w, 1));
}

fn scene_with(effect_text: &str) -> Sim {
    let mut s = Sim::new(1);
    let system = Json::obj([("effect", Json::str(effect_text))]);
    for (uid, comps) in [
        (1u64, Json::obj([("Transform", json(r#"{"pos":[0,0,8]}"#).unwrap()), ("Camera", json(r#"{"clear":[0,0,0,1]}"#).unwrap())])),
        (2, Json::obj([("Transform", json(r#"{"pos":[0,0,0]}"#).unwrap()), ("ParticleSystem", system)])),
    ] {
        let Json::Obj(components) = comps else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    s
}

fn picture(s: &mut Sim, tex: &mut TextureCache) -> crate::frame::Frame {
    render(&mut s.world, tex, &RenderOptions { width: 96, height: 64, ..RenderOptions::default() }).unwrap()
}

fn lit(f: &crate::frame::Frame) -> usize {
    f.rgba.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30).count()
}

#[test]
fn a_system_shows_in_the_picture_runs_with_the_clock_and_the_same_clock_gives_the_same_picture() {
    let mut s = scene_with(r#"{"emitters":[{"spawn":{"rate":150,"shape":{"type":"sphere","radius":0.8}},"init":{"life":1.5,"speed":[0.2,0.6],"spread":180,"size":0.5,"color":[1,0.6,0.2,1]},"render":{"blend":"add"}}]}"#);
    let mut tex = TextureCache::new(None);
    let first = picture(&mut s, &mut tex);
    assert!(lit(&first) > 200, "{} lit pixels", lit(&first));
    s.step_n(45);
    let t = s.world.get::<lunaris_sim::components::ParticleSystem>(s.world.entity(2).unwrap()).unwrap().time;
    assert!((t - 0.75).abs() < 1e-3, "{t}");
    // the particles went on from where they were: the same picture cache, a clock 45 steps on
    let later = picture(&mut s, &mut tex);
    assert_ne!(later.hash(), first.hash(), "it moved");
    assert_eq!(tex.fx.stats.steps, 45, "only the 45 steps that were missing");
    // the same clock, the same picture: asking again steps nothing
    assert_eq!(picture(&mut s, &mut tex).hash(), later.hash());
    assert_eq!(tex.fx.stats.steps, 0);
    // the same history gives the same picture: a second run of the same two pictures
    let mut s2 = scene_with(r#"{"emitters":[{"spawn":{"rate":150,"shape":{"type":"sphere","radius":0.8}},"init":{"life":1.5,"speed":[0.2,0.6],"spread":180,"size":0.5,"color":[1,0.6,0.2,1]},"render":{"blend":"add"}}]}"#);
    let mut tex2 = TextureCache::new(None);
    assert_eq!(picture(&mut s2, &mut tex2).hash(), first.hash());
    s2.step_n(45);
    assert_eq!(picture(&mut s2, &mut tex2).hash(), later.hash());
    // a hidden system draws nothing
    s.apply(&Command::Set { uid: 2, component: "ParticleSystem".into(), value: json(r#"{"visible":false}"#).unwrap() }).unwrap();
    assert_eq!(lit(&picture(&mut s, &mut tex)), 0);
    // one that is behind the camera is not worked out
    s.apply(&Command::Set { uid: 2, component: "ParticleSystem".into(), value: json(r#"{"visible":true}"#).unwrap() }).unwrap();
    s.apply(&Command::Set { uid: 2, component: "Transform".into(), value: json(r#"{"pos":[0,0,40]}"#).unwrap() }).unwrap();
    let mut tex3 = TextureCache::new(None);
    picture(&mut s, &mut tex3);
    assert!(tex3.fx.is_empty(), "behind the camera, nothing was stepped");
}

/// A scene with a camera, a static box of the world with its top at y = 0 (so a particle falling on it has something to hit), and a system above it with the given effect.
fn box_scene(effect_text: &str) -> Sim {
    let mut s = Sim::new(1);
    let system = Json::obj([("effect", Json::str(effect_text))]);
    for (uid, comps) in [
        (1u64, Json::obj([("Transform", json(r#"{"pos":[0,3,10]}"#).unwrap()), ("Camera", json(r#"{"clear":[0,0,0,1]}"#).unwrap())])),
        (2, Json::obj([("Transform", json(r#"{"pos":[0,-0.5,0]}"#).unwrap()), ("Collider", json(r#"{"shape":1,"size":[20,1,20]}"#).unwrap())])),
        (3, Json::obj([("Transform", json(r#"{"pos":[0,2,0]}"#).unwrap()), ("ParticleSystem", system)])),
    ] {
        let Json::Obj(components) = comps else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }
    s
}

#[test]
fn particles_that_collide_with_the_world_bounce_off_its_colliders_and_those_that_do_not_fall_through() {
    let spray = |world: bool| {
        format!(
            r#"{{"emitters":[{{"space":"world","spawn":{{"rate":120}},"init":{{"life":3,"speed":[1,3],"direction":[0,-1,0],"spread":30,"size":0.05}},
              "update":{{"gravity":[0,-9.8,0],"collide":{{"mode":"bounce","bounce":0.5,"friction":0.2,"world":{world}}}}},
              "events":[{{"on":"collide","spawn":"dust","count":1}}]}},
             {{"name":"dust","child":true,"space":"world","init":{{"life":1,"size":0.1}}}}]}}"#
        )
    };
    let lowest = |world: bool| {
        let mut s = box_scene(&spray(world));
        let mut tex = TextureCache::new(None);
        let (mut low, mut dust) = (f32::MAX, 0usize);
        for _ in 0..60 {
            s.step_n(3);
            render(&mut s.world, &mut tex, &RenderOptions { width: 64, height: 36, ..RenderOptions::default() }).unwrap();
            let sys = tex.fx.get(3).expect("system");
            low = sys.em[0].parts.iter().map(|p| p.pos[1]).fold(low, f32::min);
            dust = dust.max(sys.em[1].parts.len());
        }
        (low, dust)
    };
    let (kept, dust) = lowest(true);
    assert!(kept > -0.05, "nothing goes through the top of the box: {kept}");
    assert!(dust > 0, "a hit is an event: {dust} dust");
    let (fell, none) = lowest(false);
    assert!(fell < -3.0 && none == 0, "without `world` they fall through and nothing hits: {fell}, {none}");
}

#[test]
fn a_square_picture_is_a_hard_edged_square_and_nearest_keeps_the_texels_hard() {
    let lit_by = |texture: &str, filter: &str| {
        let effect = format!(r#"{{"emitters":[{{"loop":false,"spawn":{{"bursts":[{{"count":1}}]}},"init":{{"life":100,"speed":0,"size":2,"color":[1,1,1,1]}},"render":{{"texture":"{texture}","filter":"{filter}","blend":"add"}}}}]}}"#);
        let mut s = scene_with(&effect);
        s.step_n(3);
        let f = picture(&mut s, &mut TextureCache::new(None));
        // (the picture is cleared to black: every pixel that is not black is the particle's)
        f.rgba.chunks(4).filter(|p| p[0] > 200).count()
    };
    let (square, circle) = (lit_by("square", "nearest"), lit_by("circle", "nearest"));
    // a square of 2 m and a disc of the same width: the square holds 4 / pi times as much
    let ratio = square as f32 / circle as f32;
    assert!(circle > 100 && (1.15..1.4).contains(&ratio), "{square} against {circle}: {ratio}");
    // the edge of the picture is all or nothing: no pixel half lit between the square and the black
    let effect = r#"{"emitters":[{"loop":false,"spawn":{"bursts":[{"count":1}]},"init":{"life":100,"speed":0,"size":2,"color":[1,1,1,1]},"render":{"texture":"square","filter":"nearest","blend":"add"}}]}"#;
    let mut s = scene_with(effect);
    s.step_n(3);
    let f = picture(&mut s, &mut TextureCache::new(None));
    let lit = f.rgba.chunks(4).filter(|p| p[0] > 200).count();
    let dim = f.rgba.chunks(4).filter(|p| p[0] > 20 && p[0] <= 200).count();
    assert!(dim * 20 < lit, "{dim} half-lit against {lit}");
}

#[test]
fn effects_come_from_files_that_are_read_again_when_they_change_and_mistakes_are_warned_of_once() {
    let dir = std::env::temp_dir().join(format!("lunaris_fx_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("fx")).unwrap();
    std::fs::write(dir.join("fx/a.fx.json"), r#"{"emitters":[{"spawn":{"rate":10}}]}"#).unwrap();
    let mut tex = TextureCache::new(Some(dir.clone()));
    tex.reload_interval = std::time::Duration::from_millis(0);
    let (a, k1) = tex.fx_effect("fx/a.fx.json").unwrap();
    assert_eq!(a.emitters[0].spawn.rate, 10.0);
    assert_eq!(tex.fx_effect("fx/a.fx.json").unwrap().1, k1, "the same file is the same effect");
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(dir.join("fx/a.fx.json"), r#"{"emitters":[{"spawn":{"rate":99}}]}"#).unwrap();
    let (b, k2) = tex.fx_effect("fx/a.fx.json").unwrap();
    assert_eq!(b.emitters[0].spawn.rate, 99.0);
    assert_ne!(k1, k2);
    // a file that is not there, a path that leaves the workspace, a mistake: an error each, and the warning only once
    assert!(tex.fx_effect("fx/none.fx.json").unwrap_err().contains("cannot read"));
    assert!(tex.fx_effect("../secret.json").unwrap_err().contains("cannot use `..`"));
    std::fs::write(dir.join("fx/bad.fx.json"), r#"{"emitters":[{"update":{"noise":{"scale":0}}}]}"#).unwrap();
    let before = tex.warnings.len();
    assert!(tex.fx_effect("fx/bad.fx.json").unwrap_err().contains("emitters[0].update.noise.scale"));
    let once = tex.warnings.len();
    assert_eq!(once, before + 1);
    let _ = tex.fx_effect("fx/bad.fx.json");
    assert_eq!(tex.warnings.len(), once);
    // JSON written in the component works the same, and so does an empty field (an error, not a crash)
    assert!(tex.fx_effect(r#"  {"emitters":[{}]}"#).is_ok());
    assert!(tex.fx_effect("").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_radix_sort_is_the_stable_sort_of_the_library() {
    let mut rng = Rng::new(9);
    for n in [0usize, 1, 5, 1023, 1024, 5000, 40_000] {
        // many equal keys, so that the order of equals counts
        let keys: Vec<u32> = (0..n).map(|_| (rng.next() % 3000) << 11 | (rng.next() % 4) << 30).collect();
        let mut want: Vec<u32> = (0..n as u32).collect();
        want.sort_by_key(|&i| keys[i as usize]);
        assert_eq!(crate::fx_draw::order(&keys), want, "{n} keys");
    }
}

#[test]
fn a_layer_is_the_same_mesh_however_many_threads_make_it() {
    let e = effect(
        r##"{"budget":50000,"emitters":[
          {"max":20000,"spawn":{"rate":8000,"shape":{"type":"sphere","radius":2}},"init":{"life":3,"speed":[0.5,2],"spread":180,"size":[0.05,0.2],"spin":[-90,90],"color":"#ffcc88"},
           "update":{"noise":{"strength":1,"scale":1.5,"speed":0.2}},"curves":{"alpha":[[0,0],[0.2,1],[1,0]]},"render":{"blend":"alpha","sort":true,"sheet":[2,2],"frame_mode":"random"}},
          {"max":20000,"spawn":{"rate":6000},"init":{"life":2,"speed":3,"spread":60,"size":0.03},"render":{"blend":"add","facing":"velocity"}}]}"##,
    );
    let mut w = FxWorld::default();
    go(&mut w, &job(&e, 1, 5.0));
    let sys = w.get(1).unwrap();
    assert!(sys.em[0].parts.len() > 15_000 && sys.em[1].parts.len() > 10_000, "{} {}", sys.em[0].parts.len(), sys.em[1].parts.len());
    let one = build(sys, &camera(), 1);
    for threads in [2usize, 3, 8] {
        let many = build(sys, &camera(), threads);
        assert_eq!(one.len(), many.len());
        for (a, b) in one.iter().zip(&many) {
            assert_eq!((&a.mesh.pos, &a.mesh.nrm, &a.mesh.uv, &a.mesh.col, &a.mesh.texw, &a.mesh.idx), (&b.mesh.pos, &b.mesh.nrm, &b.mesh.uv, &b.mesh.col, &b.mesh.texw, &b.mesh.idx), "{threads} threads");
        }
    }
    // alpha-blended, so far to near: the depth of each quad's middle never goes up
    let m = &one[0].mesh;
    let depth = |k: usize| {
        let c = scale(add(add(m.pos[k * 4], m.pos[k * 4 + 1]), add(m.pos[k * 4 + 2], m.pos[k * 4 + 3])), 0.25);
        -(c[2] - 10.0)
    };
    let quads = m.pos.len() / 4;
    assert!((1..quads).all(|k| depth(k) <= depth(k - 1) + 1e-3), "sorted far to near");
}

#[test]
fn an_emitter_for_the_card_is_only_scheduled_here_with_the_births_of_every_step_and_the_ring_it_fills() {
    let e = effect(r#"{"emitters":[{"simulate":"gpu","max":500,"spawn":{"rate":90,"bursts":[{"time":0.5,"count":40}]},"init":{"life":2,"speed":1}},{"name":"on the processor","spawn":{"rate":30},"init":{"life":1}}]}"#);
    let gpu = |t: f32| FxJob { gpu: true, ..job(&e, 1, t) };
    let mut w = FxWorld::default();
    go(&mut w, &gpu(1.0));
    let sys = w.get(1).unwrap();
    // nothing of the card's emitter lives here; the other emitter is stepped as ever
    assert_eq!(sys.em[0].parts.len(), 0);
    assert!((28..=32).contains(&sys.em[1].parts.len()), "{}", sys.em[1].parts.len());
    // a step for every step the system took (the prewarm of 2 s up to the clock), the clock going up, the births of the rate (1.5 a step) and of the burst
    let steps = &sys.em[0].gpu.pending;
    assert_eq!(steps.len() as u32, sys.steps_taken);
    assert!(steps.windows(2).all(|p| p[1].t > p[0].t), "the clock goes up");
    assert!(steps.windows(2).all(|p| p[1].first == (p[0].first + p[0].spawn) % 500 && p[1].serial == p[0].serial.wrapping_add(p[0].spawn)), "the ring is filled in order");
    let total: u32 = steps.iter().map(|s| s.spawn).sum();
    // the 2 s of prewarm up to the clock (from -1 s to 1 s) at 90 a second, and the burst at 0.5 s
    assert!((total as i32 - (180 + 40)).abs() <= 3, "{total}");
    assert_eq!(steps.iter().filter(|s| s.spawn >= 40).count(), 1, "the burst is one step's");
    // what the picture takes to the card
    let jobs = sys.gpu_jobs(1);
    assert_eq!(jobs.len(), 1);
    let j = &jobs[0];
    assert_eq!((j.uid, j.emitter, j.capacity), (1, 0, 500));
    assert_eq!(j.steps.len(), steps.len());
    assert!(j.key >> 63 == 1, "its mesh key is kept apart from the entities'");
    // the card has taken the steps up to a clock: they are forgotten, the rest stay
    let half = steps[steps.len() / 2].t;
    let epoch = sys.epoch;
    w.gpu_applied(1, 0, epoch, half);
    assert!(w.get(1).unwrap().em[0].gpu.pending.iter().all(|s| s.t > half));
    assert!(!w.get(1).unwrap().em[0].gpu.pending.is_empty());
    w.gpu_applied(1, 0, epoch + 1, 1.0e9);
    assert!(!w.get(1).unwrap().em[0].gpu.pending.is_empty(), "another system's number does not forget them");
    // the next picture adds its step
    go(&mut w, &gpu(1.0 + 1.0 / 60.0));
    assert_eq!(w.get(1).unwrap().em[0].gpu.pending.last().unwrap().t, 61.0 * DT);
    // a renderer with no card steps the same emitter here, and a system whose renderer changes starts over
    go(&mut w, &job(&e, 1, 1.0 + 2.0 / 60.0));
    let sys = w.get(1).unwrap();
    assert!(sys.em[0].parts.len() > 100 && !sys.gpu_mode, "{} here", sys.em[0].parts.len());
    assert_ne!(sys.epoch, epoch);
    assert!(sys.gpu_jobs(1).is_empty());
    // the ball that holds the effect is known without the particles of the card
    go(&mut w, &gpu(5.0));
    assert!(w.get(1).unwrap().bounds.is_some_and(|(_, r)| r >= e.radius() * 0.99));
}

/// Effects that are strange but read (the sim's own test makes thousands of them by changing characters of the examples): stepped for two seconds and drawn, they never panic, make a vertex that is not a number, or hold more than the budget.
#[test]
fn strange_effects_that_read_are_stepped_and_drawn_without_a_panic_or_a_vertex_that_is_not_a_number() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fx");
    let names = ["campfire", "explosion", "mind_motes", "voxel/rain_streaks", "voxel/machine_death", "voxel/fire_barrel"];
    let texts: Vec<String> = names.iter().map(|n| std::fs::read_to_string(root.join(format!("{n}.fx.json"))).unwrap()).collect();
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let values = ["0", "-1", "1e9", "100000", "0.0001", "180", "[0,0,0]", "[1e6,1e6,1e6]", "999", "0.5"];
    let (mut ran, mut moved) = (0, 0u32);
    for round in 0..600 {
        let mut s = texts[round % texts.len()].clone();
        for _ in 0..1 + next() % 3 {
            // change a number somewhere: the digits of one number are replaced by a strange value
            let bytes = s.as_bytes();
            let starts: Vec<usize> = (1..bytes.len()).filter(|&i| bytes[i].is_ascii_digit() && !bytes[i - 1].is_ascii_digit() && bytes[i - 1] != b'.' && bytes[i - 1] != b'"' && bytes[i - 1] != b'#').collect();
            let at = starts[(next() as usize) % starts.len()];
            let end = (at..bytes.len()).find(|&i| !(bytes[i].is_ascii_digit() || bytes[i] == b'.')).unwrap_or(bytes.len());
            s.replace_range(at..end, values[(next() as usize) % values.len()]);
        }
        let Ok(e) = parse(&s) else { continue };
        let e = Arc::new(e);
        let mut w = FxWorld::default();
        let mut t = 0.0f32;
        for k in 0..120 {
            t += 1.0 / 60.0;
            // the entity moves and turns, and is scaled oddly now and then
            let tr = Transform { pos: [k as f32 * 0.1, 1.0, 0.0], rot: [0.0, (k as f32 * 0.05).sin(), 0.0, (k as f32 * 0.05).cos()], scale: if round % 7 == 0 { [0.0, 1.0, 1.0] } else { [1.0, 1.0, 1.0] } };
            go(&mut w, &FxJob { tr, gpu: round % 2 == 0, ..job(&e, 1, t) });
        }
        let sys = w.get(1).unwrap();
        assert!(sys.live() <= e.budget as usize + e.emitters.len() * 8192, "{} live of a budget of {}: {s}", sys.live(), e.budget);
        for l in build(sys, &camera(), 2) {
            assert!(l.mesh.pos.iter().all(|p| p.iter().all(|v| v.is_finite())), "a vertex that is not a number: {s}");
            assert!(l.mesh.texw.iter().all(|v| v.is_finite()) && l.mesh.col.iter().all(|c| c.iter().all(|v| v.is_finite())));
        }
        let _ = lights(sys);
        ran += 1;
        moved += sys.live() as u32;
    }
    assert!(ran > 100 && moved > 0, "{ran} ran");
}

#[test]
fn the_swirl_of_the_noise_has_no_divergence_and_is_not_still() {
    let mut rng = Rng::new(3);
    let (mut worst, mut mean_len) = (0.0f32, 0.0f32);
    for _ in 0..2000 {
        let p = [rng.f() * 40.0 - 20.0, rng.f() * 40.0 - 20.0, rng.f() * 40.0 - 20.0];
        let e = 0.01;
        let d = |a: usize| {
            let (mut hi, mut lo) = (p, p);
            hi[a] += e;
            lo[a] -= e;
            (curl_at(hi)[a] - curl_at(lo)[a]) / (2.0 * e)
        };
        let div = d(0) + d(1) + d(2);
        let len = length(curl_at(p));
        worst = worst.max(div.abs());
        mean_len += len / 2000.0;
    }
    // (a finite difference over a lattice whose second derivative jumps at the cell walls is not exact: the divergence stays a small share of the swirl)
    assert!(mean_len > 0.4, "the swirl moves: {mean_len}");
    assert!(worst < mean_len * 0.6, "the divergence is {worst} against a swirl of {mean_len}");
}

/// Not a test: prints what stepping and drawing particle systems costs (`cargo test --release -p lunaris-render how_fast_are_particle_systems -- --ignored --nocapture`).
#[test]
#[ignore]
fn how_fast_are_particle_systems() {
    let cases: [(&str, &str, u32); 5] = [
        ("plain sprites (no forces)", r#"{"budget":100000,"emitters":[{"max":100000,"spawn":{"rate":10000},"init":{"life":5,"speed":[1,2],"spread":180,"size":0.1}}]}"#, 50_000),
        ("gravity and drag", r#"{"budget":100000,"emitters":[{"max":100000,"spawn":{"rate":10000},"init":{"life":5,"speed":[1,2],"spread":180,"size":0.1},"update":{"gravity":[0,-3,0],"drag":0.3}}]}"#, 50_000),
        ("curl noise", r#"{"budget":100000,"emitters":[{"max":100000,"spawn":{"rate":10000},"init":{"life":5,"speed":[1,2],"spread":180,"size":0.1},"update":{"noise":{"strength":2,"scale":1,"speed":0.3}}}]}"#, 50_000),
        ("the mind (noise, vortex, attractor)", r#"{"budget":100000,"emitters":[{"max":100000,"spawn":{"rate":4000,"shape":{"type":"sphere","radius":7,"shell":true}},"init":{"life":12,"speed":[0,0.4],"spread":180,"size":0.1},"update":{"noise":{"strength":1,"scale":3,"speed":0.1},"vortices":[{"axis":[0,1,0],"strength":3,"pull":0.5,"radius":18}],"attractors":[{"strength":1,"radius":20,"falloff":0,"kill":0.8}]}}]}"#, 48_000),
        ("a bouncing spray with ribbons", r#"{"budget":100000,"emitters":[{"max":100000,"spawn":{"rate":8000},"init":{"life":4,"speed":[2,6],"spread":60,"size":0.05},"update":{"gravity":[0,-9.8,0],"collide":{"bounce":0.5,"planes":[{"normal":[0,1,0],"d":-1}]}},"render":{"type":"ribbon","ribbon":{"points":6,"interval":0.03}}}]}"#, 32_000),
    ];
    for (name, text, expect) in cases {
        let e = effect(text);
        let mut w = FxWorld::default();
        // fill it, then time 120 steps of a full system
        go(&mut w, &job(&e, 1, 8.0));
        let live = live(&w, 1);
        let t0 = std::time::Instant::now();
        let mut t = 8.0f32;
        for _ in 0..120 {
            t += 1.0 / 60.0;
            go(&mut w, &job(&e, 1, t));
        }
        let step_ms = t0.elapsed().as_secs_f64() * 1000.0 / 120.0;
        let mut drawn = Vec::new();
        for threads in [1usize, 4] {
            let t1 = std::time::Instant::now();
            let mut verts = 0;
            for _ in 0..20 {
                verts = build(w.get(1).unwrap(), &camera(), threads).iter().map(|l| l.mesh.pos.len()).sum::<usize>();
            }
            drawn.push(format!("{verts} vertices {:.2} ms on {threads}", t1.elapsed().as_secs_f64() * 1000.0 / 20.0));
        }
        println!("{name}: {live} live (about {expect}), {step_ms:.2} ms a step = {:.0} ns a particle, drawing {}", step_ms * 1e6 / live.max(1) as f64, drawn.join(", "));
    }
}

/// Not a test: prints what colliding with the world costs a picture, with many systems among many colliders (`cargo test --release -p lunaris-render how_fast_is_colliding_with_the_world -- --ignored --nocapture`).
#[test]
#[ignore]
fn how_fast_is_colliding_with_the_world() {
    let spray = |world: bool| {
        Json::str(&format!(
            r#"{{"emitters":[{{"space":"world","spawn":{{"rate":60}},"init":{{"life":2,"speed":[2,5],"spread":60,"size":0.03}},"update":{{"gravity":[0,-9.8,0],"collide":{{"mode":"bounce","bounce":0.4,"world":{world}}}}}}}]}}"#
        ))
        .to_string_compact()
        .unwrap()
    };
    for (colliders, systems) in [(5000usize, 40usize), (5000, 4)] {
        for world in [false, true] {
            let mut ents = vec![r#"{"uid":1,"components":{"Camera":{"fov_y":60},"Transform":{"pos":[0,6,40],"rot":[-0.1,0,0,0.995]}}}"#.to_string()];
            // a city of boxes, a ground, and the sprays on top of it
            for i in 0..colliders {
                let (x, z) = ((i % 100) as f32 * 8.0 - 400.0, (i / 100) as f32 * 8.0 - 200.0);
                ents.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{x},3,{z}]}},"Collider":{{"shape":1,"size":[5,6,5]}}}}}}"#, 100 + i));
            }
            ents.push(format!(r#"{{"uid":50,"components":{{"Transform":{{"pos":[0,-0.5,0]}},"Collider":{{"shape":1,"size":[2000,1,2000]}}}}}}"#));
            for i in 0..systems {
                ents.push(format!(r#"{{"uid":{},"components":{{"Transform":{{"pos":[{},2,{}]}},"ParticleSystem":{{"effect":{}}}}}}}"#, 10000 + i, (i % 10) as f32 * 6.0 - 30.0, (i / 10) as f32 * -6.0, spray(world)));
            }
            let mut sim = Sim::new(1);
            sim.load_scene(&json(&format!(r#"{{"lunaris":1,"entities":[{}]}}"#, ents.join(","))).unwrap()).unwrap();
            let mut tex = TextureCache::new(None);
            let opts = RenderOptions { width: 640, height: 360, threads: 4, ..RenderOptions::default() };
            crate::render::gather_scene(&mut sim.world, &mut tex, &opts).unwrap();
            let t0 = std::time::Instant::now();
            for _ in 0..60 {
                sim.step();
                crate::render::gather_scene(&mut sim.world, &mut tex, &opts).unwrap();
            }
            println!("{systems} systems ({} live) among {colliders} boxes, world collision {}: {:.2} ms a picture", tex.fx.stats.particles, if world { "on" } else { "off" }, t0.elapsed().as_secs_f64() * 1000.0 / 60.0);
        }
    }
}

/// Not a test: what the processor pays for the mind's twelve thousand motes when it steps them itself and when the card does (`cargo test --release -p lunaris-render how_much_does_the_card_save -- --ignored --nocapture`).
#[test]
#[ignore]
fn how_much_does_the_card_save() {
    let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fx/mind_motes.fx.json")).unwrap();
    let on_card = effect(&text);
    // the same effect with the card's emitter on the processor
    let on_cpu = effect(&text.replace("\"simulate\": \"gpu\",", ""));
    let cam = camera();
    for (name, e, gpu) in [("on the processor", on_cpu, false), ("on the card (the schedule only)", on_card, true)] {
        let mut w = FxWorld::default();
        let mut t = 14.0f32;
        go(&mut w, &FxJob { gpu, ..job(&e, 1, t) });
        let t0 = std::time::Instant::now();
        let mut verts = 0;
        for _ in 0..120 {
            t += 1.0 / 60.0;
            go(&mut w, &FxJob { gpu, ..job(&e, 1, t) });
            let sys = w.get(1).unwrap();
            verts = build(sys, &cam, 4).iter().map(|l| l.mesh.pos.len()).sum::<usize>();
            for j in sys.gpu_jobs(1) {
                let last = j.steps.last().map_or(0.0, |s| s.t);
                let (uid, emitter, epoch) = (j.uid, j.emitter, j.epoch);
                w.gpu_applied(uid, emitter, epoch, last);
            }
        }
        println!("the mind {name}: {:.2} ms a picture on the processor ({verts} vertices built here, {} particles live here)", t0.elapsed().as_secs_f64() * 1000.0 / 120.0, w.get(1).unwrap().live());
    }
}
