//! Effects (docs/FX.md): what the reader accepts, what it makes of it, and how it says what is wrong.
use super::fx::*;

fn ok(text: &str) -> Effect {
    parse(text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn err(text: &str) -> String {
    parse(text).expect_err("should not parse")
}

const CAMPFIRE: &str = r##"{
  "name": "campfire", "bounds": 6, "budget": 3000, "lod": {"near": 30, "far": 80}, "prewarm": 2,
  "emitters": [
    { "name": "flames", "space": "local", "max": 300,
      "spawn": { "rate": 80, "shape": {"type": "disc", "radius": 0.25} },
      "init": { "life": [0.6, 1.1], "speed": [0.8, 1.6], "direction": [0, 1, 0], "spread": 12, "size": [0.2, 0.4], "rotation": [0, 360], "spin": [-30, 30], "color": "#ffd070" },
      "update": { "gravity": [0, 1.4, 0], "drag": 0.5, "noise": {"strength": 1.2, "scale": 0.8, "speed": 0.6} },
      "curves": { "size": [[0, 0.5], [0.3, 1], [1, 0.1]], "alpha": [[0, 0], [0.1, 1], [1, 0]], "color": [[0, "#ffffff"], [1, [1, 0.3, 0.1, 1]]] },
      "render": { "texture": "fx/flame.png", "sheet": [4, 4], "blend": "add", "soft": 0.3, "light": {"intensity": 3, "range": 8, "max": 2} },
      "events": [ {"on": "death", "spawn": "smoke", "count": [0, 1], "chance": 0.3} ] },
    { "name": "smoke", "child": true, "space": "world", "max": 200,
      "init": { "life": [2, 4], "size": 0.6, "inherit_velocity": 0.5, "color": [0.3, 0.3, 0.3, 0.5] },
      "update": { "gravity": [0, 0.6, 0], "wind": [0.4, 0, 0] },
      "render": { "blend": "alpha", "sort": true } }
  ]
}"##;

#[test]
fn a_campfire_reads_with_everything_it_says() {
    let e = ok(CAMPFIRE);
    assert_eq!((e.name.as_str(), e.bounds, e.budget, e.lod_near, e.lod_far, e.prewarm), ("campfire", 6.0, 3000, 30.0, 80.0, 2.0));
    assert_eq!(e.emitters.len(), 2);
    let f = &e.emitters[0];
    assert_eq!((f.name.as_str(), f.space, f.max, f.looping, f.child), ("flames", Space::Local, 300, true, false));
    assert_eq!(f.spawn.rate, 80.0);
    assert_eq!(f.spawn.shape, Shape::Disc { radius: 0.25, ring: false });
    assert_eq!((f.init.life, f.init.speed, f.init.spread, f.init.size), ([0.6, 1.1], [0.8, 1.6], 12.0, [0.2, 0.4]));
    assert!((f.init.color[0] - 1.0).abs() < 1e-6 && (f.init.color[1] - 0xd0 as f32 / 255.0).abs() < 1e-6 && (f.init.color[2] - 0x70 as f32 / 255.0).abs() < 1e-6 && f.init.color[3] == 1.0);
    assert_eq!(f.update.gravity, [0.0, 1.4, 0.0]);
    assert_eq!(f.update.noise, Some(Noise { strength: 1.2, scale: 0.8, speed: 0.6 }));
    let size = f.curves.size.as_ref().unwrap();
    assert_eq!(size.keys.len(), 3);
    assert!((size.at(0.3)[0] - 1.0).abs() < 1e-6 && (size.at(0.65)[0] - 0.55).abs() < 1e-5);
    assert_eq!(f.curves.color.as_ref().unwrap().keys[1].1, [1.0, 0.3, 0.1, 1.0]);
    assert_eq!((f.render.sheet, f.render.frames, f.render.additive, f.render.soft), ([4, 4], 16, true, 0.3));
    assert_eq!(f.render.light, Some(Light { color: None, intensity: 3.0, range: 8.0, max: 2 }));
    assert!(!f.render.sort, "added pictures are not sorted unless asked");
    // the event names the other emitter, and is a number by now
    assert_eq!(f.events, vec![Event { on: On::Death, spawn: 1, count: [0, 1], chance: 0.3, inherit_velocity: 0.0, inherit_color: 0.0 }]);
    let s = &e.emitters[1];
    assert!(s.child && s.space == Space::World && s.render.sort && !s.render.additive);
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
}

#[test]
fn an_effect_can_be_as_small_as_one_emitter_and_has_sane_defaults() {
    let e = ok(r#"{"emitters":[{"spawn":{"rate":10}}]}"#);
    let m = &e.emitters[0];
    assert_eq!(m.name, "emitter0");
    assert_eq!((m.space, m.max, m.looping, m.duration, m.delay), (Space::Local, 1000, true, 0.0, 0.0));
    assert_eq!(m.init.life, [1.0, 1.0]);
    assert_eq!(m.init.size, [0.2, 0.2]);
    assert_eq!(m.render.texture, "");
    assert_eq!(m.render.kind, Kind::Sprite);
    assert!(!m.render.additive && m.render.sort && !m.render.nearest);
    assert_eq!(e.budget, 4096);
    assert!(ok(r#"{"emitters":[{"render":{"filter":"nearest","texture":"square"}}]}"#).emitters[0].render.nearest);
    assert_eq!(err(r#"{"emitters":[{"render":{"filter":"cubic"}}]}"#), "emitters[0].render.filter: unknown filter `cubic` (linear, nearest)");
    assert_eq!((e.lod_near, e.lod_far), (0.0, 0.0));
    // a bare number is a range of one value, a pair is least and most (either way round)
    let e = ok(r#"{"emitters":[{"init":{"life":3,"speed":[5,2]}}]}"#);
    assert_eq!((e.emitters[0].init.life, e.emitters[0].init.speed), ([3.0, 3.0], [2.0, 5.0]));
}

#[test]
fn an_emitter_can_ask_for_the_card_and_only_for_what_the_card_does() {
    let e = ok(r#"{"emitters":[{"simulate":"gpu","spawn":{"rate":500},"update":{"gravity":[0,1,0],"noise":{},"vortices":[{},{}],"attractors":[{"kill":1}],"collide":{"planes":[{"normal":[0,1,0]}]}}},{"spawn":{"rate":5}}]}"#);
    assert!(e.emitters[0].gpu && !e.emitters[1].gpu);
    assert!(parse(r#"{"emitters":[{"simulate":"cpu"}]}"#).is_ok());
    let no = |emitter: &str| err(&format!(r#"{{"emitters":[{{"simulate":"gpu",{emitter}}}]}}"#));
    assert!(no(r#""render":{"type":"ribbon"}"#).starts_with("emitters[0].simulate: `gpu` emitters cannot have ribbons or solids"), "{}", no(r#""render":{"type":"ribbon"}"#));
    assert!(no(r#""render":{"type":"solid"}"#).contains("ribbons or solids"));
    assert!(no(r#""render":{"light":{}}"#).contains("a light for each particle"));
    assert!(no(r#""update":{"collide":{"world":true}}"#).contains("colliders other than up to two planes"));
    assert!(no(r#""update":{"collide":{"boxes":[{}]}}"#).contains("colliders other than up to two planes"));
    assert!(no(r#""update":{"collide":{"planes":[{},{},{}]}}"#).contains("more than two planes"));
    assert!(no(r#""update":{"vortices":[{},{},{}]}"#).contains("more than two vortices"));
    assert!(no(r#""child":true"#).contains("events or be children"));
    assert!(no(r#""events":[{"on":"death","spawn":"x"}]"#).contains("events or be children") || no(r#""events":[{"on":"death","spawn":"x"}]"#).contains("no emitter called"));
    assert_eq!(err(r#"{"emitters":[{"simulate":"tpu"}]}"#), "emitters[0].simulate: unknown way to simulate `tpu` (cpu, gpu)");
    // an event cannot spawn into the card
    let e = err(r#"{"emitters":[{"name":"a","events":[{"on":"death","spawn":"b"}]},{"name":"b","simulate":"gpu"}]}"#);
    assert_eq!(e, "emitters[0].events[0].spawn: `b` is simulated on the card (`simulate: gpu`), which events cannot spawn into");
}

/// Every ready-made effect of `examples/fx` (and its `voxel` folder) reads, without a warning, and says what it is.
#[test]
fn every_example_effect_reads_without_a_warning() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fx");
    let mut seen = 0;
    for dir in [root.clone(), root.join("voxel")] {
        for entry in std::fs::read_dir(&dir).expect("the example effects") {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".fx.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let e = parse(&text).unwrap_or_else(|m| panic!("{}: {m}", path.display()));
            assert!(e.warnings.is_empty(), "{}: {:?}", path.display(), e.warnings);
            assert!(!e.name.is_empty(), "{} has no name", path.display());
            seen += 1;
        }
    }
    assert!(seen >= 20, "{seen} effects");
}

/// Whatever is made of an example effect by changing a few of its characters, the reader says yes or no: it never panics, and what it says yes to has numbers in range.
#[test]
fn a_mutated_effect_is_read_or_refused_never_a_panic() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fx");
    let mut texts: Vec<String> = Vec::new();
    for name in ["campfire", "explosion", "mind_motes", "tracer", "voxel/fire_barrel", "voxel/rain_streaks", "voxel/machine_death"] {
        texts.push(std::fs::read_to_string(root.join(format!("{name}.fx.json"))).unwrap());
    }
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let bits = ["0", "-1", "1e9", "999999999999", "null", "\"x\"", "[]", "{}", "[1,2,3]", "true", "-0.0", "1e-30", "3.5", "\"#ff00\"", ",,", "]"];
    let (mut ok, mut refused) = (0, 0);
    for round in 0..4000 {
        let mut s = texts[round % texts.len()].clone().into_bytes();
        for _ in 0..1 + next() % 4 {
            let at = (next() as usize) % s.len();
            match next() % 3 {
                0 => {
                    let chars = b"0123456789.-[]{},:tfn\" abcxyz";
                    s[at] = chars[(next() as usize) % chars.len()];
                }
                1 => {
                    // replace a run with a strange value
                    let end = (at + 1 + (next() as usize) % 6).min(s.len());
                    let v = bits[(next() as usize) % bits.len()].as_bytes();
                    s.splice(at..end, v.iter().copied());
                }
                _ => {
                    let end = (at + (next() as usize) % 40).min(s.len());
                    s.drain(at..end);
                }
            }
            if s.is_empty() {
                s.push(b'{');
            }
        }
        let Ok(text) = String::from_utf8(s) else { continue };
        match parse(&text) {
            Ok(e) => {
                ok += 1;
                assert!(e.budget >= 1 && e.budget <= MAX_BUDGET && !e.emitters.is_empty() && e.emitters.len() <= MAX_EMITTERS);
                for m in &e.emitters {
                    assert!(m.max >= 1 && m.max <= MAX_EMITTER);
                    assert!(m.init.life[0] > 0.0 && m.init.life[1] <= MAX_LIFE && m.init.life[0] <= m.init.life[1]);
                    assert!(m.spawn.rate.is_finite() && m.spawn.rate >= 0.0);
                    assert!(m.init.size[0] >= 0.0 && m.init.size[1] <= 1000.0);
                }
                let r = e.radius();
                assert!(r.is_finite() && r > 0.0, "{r}");
            }
            Err(_) => refused += 1,
        }
    }
    assert!(ok > 200 && refused > 200, "{ok} read, {refused} refused: the mutations should do both");
}

#[test]
fn colours_come_in_every_form() {
    let c = |s: &str| ok(&format!(r##"{{"emitters":[{{"init":{{"color":{s}}}}}]}}"##)).emitters[0].init.color;
    assert_eq!(c(r##""#f00""##), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(c(r##""#f008""##), [1.0, 0.0, 0.0, 0x88 as f32 / 255.0]);
    assert_eq!(c(r##""#00ff00""##), [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(c(r##""#0000ff80""##), [0.0, 0.0, 1.0, 0x80 as f32 / 255.0]);
    assert_eq!(c("[0.5, 0.25, 2.5]"), [0.5, 0.25, 2.5, 1.0]);
    assert_eq!(c("[1, 1, 1, 0.5]"), [1.0, 1.0, 1.0, 0.5]);
}

#[test]
fn mistakes_say_where_they_are() {
    let e = err("{ not json");
    assert!(e.starts_with("not JSON") && e.contains("line 1"), "{e}");
    assert_eq!(err("{}"), "emitters: an effect needs a list of emitters");
    assert!(err(r#"{"emitters":[]}"#).starts_with("emitters: needs 1 to 32"));
    assert_eq!(err(r#"{"emitters":[{"update":{"noise":{"scale":0}}}]}"#), "emitters[0].update.noise.scale: 0 is outside 0.001 to 10000");
    assert_eq!(err(r#"{"emitters":[{},{"spawn":{"shape":"cube"}}]}"#), "emitters[1].spawn.shape: unknown shape `cube` (point, sphere, box, disc, line)");
    assert_eq!(err(r#"{"emitters":[{"init":{"life":"long"}}]}"#), "emitters[0].init.life: must be a number, not string");
    assert_eq!(err(r#"{"emitters":[{"init":{"life":[1,2,3]}}]}"#), "emitters[0].init.life: a range is a number or a pair [least, most]");
    assert!(err(r##"{"emitters":[{"init":{"color":"#12"}}]}"##).contains("is not a colour"));
    assert_eq!(err(r#"{"emitters":[{"curves":{"size":[[0.5,1],[0.2,1]]}}]}"#), "emitters[0].curves.size[1]: keys must go up in time");
    assert_eq!(err(r#"{"emitters":[{"spawn":{"bursts":[{"count":[1,2],"time":-1}]}}]}"#), "emitters[0].spawn.bursts[0].time: -1 is outside 0 to 3600");
    assert_eq!(err(r#"{"emitters":[{"render":{"sheet":[0,2]}}]}"#), "emitters[0].render.sheet: must be [columns, rows], each a whole number from 1 to 64");
    assert_eq!(err(r#"{"lod":{"near":50,"far":10},"emitters":[{}]}"#), "lod: `far` must be more than `near`");
    assert_eq!(err(r#"{"emitters":[{"update":{"vortices":[{"axis":[0,0,0]}]}}]}"#), "emitters[0].update.vortices[0]: axis must not be zero");
    // an event that names nothing that exists
    let e = err(r#"{"emitters":[{"name":"a","events":[{"on":"death","spawn":"smok"}]},{"name":"smoke"}]}"#);
    assert_eq!(e, "emitters[0].events[0].spawn: no emitter called `smok` (did you mean `smoke`?)");
    assert!(err(r#"{"emitters":[{"events":[{"on":"hit","spawn":"x"}]}]}"#).contains("unknown event `hit`"));
    // two with one name are told apart by nothing
    assert_eq!(err(r#"{"emitters":[{"name":"x"},{"name":"x"}]}"#), "emitters[1].name: `x` is also the name of emitters[0]");
}

#[test]
fn what_is_only_odd_is_a_warning_with_the_nearest_key() {
    let e = ok(r#"{"emitters":[{"spwan":{"rate":5},"spawn":{"rate":5}}]}"#);
    assert_eq!(e.warnings, vec!["emitters[0].spwan: unknown key, ignored (did you mean `spawn`?)".to_string()]);
    let e = ok(r#"{"colour":1,"emitters":[{"spawn":{"rate":1}}]}"#);
    assert_eq!(e.warnings, vec!["colour: unknown key, ignored".to_string()]);
    // an emitter that makes nothing, and a child nothing asks for
    let e = ok(r#"{"emitters":[{"name":"idle"},{"name":"orphan","child":true,"spawn":{"rate":3}}]}"#);
    assert!(e.warnings.iter().any(|w| w.contains("`idle`") && w.contains("makes nothing")), "{:?}", e.warnings);
    assert!(e.warnings.iter().any(|w| w.contains("`orphan`") && w.contains("no event spawns it")), "{:?}", e.warnings);
}

#[test]
fn the_ball_that_holds_an_effect_is_the_one_it_says_or_one_its_numbers_allow() {
    let said = ok(r#"{"bounds":12,"emitters":[{"init":{"speed":100,"life":10}}]}"#);
    assert_eq!(said.radius(), 12.0);
    // a particle that starts 1 m out, flies at 3 m/s for 2 s with a gravity of 2 and is 0.5 m wide goes about 1 + 6 + 4 + 0.5 from the middle
    let worked = ok(r#"{"emitters":[{"spawn":{"shape":{"type":"sphere","radius":1}},"init":{"life":2,"speed":3,"size":0.5},"update":{"gravity":[0,2,0]}}]}"#);
    let r = worked.radius();
    assert!(r >= 11.5 && r < 14.0, "{r}");
    assert_eq!(worked.max_life(), 2.0);
}

#[test]
fn the_component_is_registered_and_its_clock_runs_with_the_simulation() {
    use crate::components::ParticleSystem;
    use crate::{Command, Sim};
    use lunaris_json::{parse, Json};
    let mut s = Sim::new(1);
    let Json::Obj(components) = parse(r#"{"Transform":{"pos":[0,0,0]},"ParticleSystem":{"effect":"{\"emitters\":[{\"spawn\":{\"rate\":5}}]}","speed":2}}"#).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(7), components }).unwrap();
    s.step_n(60);
    let t = s.world.get::<ParticleSystem>(s.world.entity(7).unwrap()).unwrap().time;
    assert!((t - 2.0).abs() < 1e-3, "speed 2 for one second: {t}");
    s.apply(&Command::Set { uid: 7, component: "ParticleSystem".into(), value: parse(r#"{"playing":false}"#).unwrap() }).unwrap();
    s.step_n(30);
    assert_eq!(s.world.get::<ParticleSystem>(s.world.entity(7).unwrap()).unwrap().time, t);
    // the text the component carries is an effect
    let text = s.world.strings().get(s.world.get::<ParticleSystem>(s.world.entity(7).unwrap()).unwrap().effect.0).to_string();
    assert_eq!(parse_effect_len(&text), 1);
}

fn parse_effect_len(text: &str) -> usize {
    ok(text).emitters.len()
}
