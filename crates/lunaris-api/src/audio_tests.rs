//! The sound director and the two audio tools, through the same calls an agent makes.
use super::*;
use crate::tests::engine;
use lunaris_audio::mixer::Mixer;
use lunaris_json::{parse, Json};
use std::sync::{Arc, Mutex};

fn j(s: &str) -> Json {
    parse(s).unwrap()
}

fn call(e: &mut Engine, tool: &str, args: &str) -> Json {
    e.call(tool, &j(args)).unwrap_or_else(|err| panic!("{tool} failed: {err}"))
}

fn num(v: &Json, path: &[&str]) -> f64 {
    let mut cur = v;
    for k in path {
        cur = cur.get(k).unwrap_or_else(|| panic!("no `{k}` in {}", v.to_string_lossy()));
    }
    cur.as_f64().unwrap_or_else(|| panic!("{path:?} is not a number in {}", v.to_string_lossy()))
}

fn wall(e: &mut Engine, pos: &str, size: &str) {
    call(e, "spawn", &format!(r#"{{"components":{{"Transform":{{"pos":{pos}}},"Collider":{{"shape":1,"size":{size}}}}}}}"#));
}

/// A closed room, 10 x 4 x 10 metres around the origin, and a camera (the listener) at its centre looking along -Z.
fn room(e: &mut Engine) {
    wall(e, "[0,-2.1,0]", "[10.4,0.2,10.4]");
    wall(e, "[0,2.1,0]", "[10.4,0.2,10.4]");
    wall(e, "[-5.1,0,0]", "[0.2,4,10]");
    wall(e, "[5.1,0,0]", "[0.2,4,10]");
    wall(e, "[0,0,-5.1]", "[10,4,0.2]");
    wall(e, "[0,0,5.1]", "[10,4,0.2]");
    call(e, "spawn", r#"{"components":{"Name":{"value":"Ears"},"Transform":{"pos":[0,0,0]},"Camera":{}}}"#);
}

/// A system that makes the entity at `x` emit `event`: every 30 ticks, or once at tick `at` when `at` is given.
fn emitter(e: &mut Engine, x: f64, event: &str, at: Option<u32>) {
    let when = match at {
        Some(t) => format!(r#"{{"eq":[{{"tick":true}},{t}]}}"#),
        None => r#"{"eq":[{"mod":[{"tick":true},30]},0]}"#.to_string(),
    };
    call(e, "define_system", &format!(r#"{{"name":"emit_{event}","each":["Transform"],"do":[{{"if":{{"and":[{when},{{"eq":[{{"get":"Transform.pos[0]"}},{x}]}}]}},"then":[{{"emit":"{event}"}}]}}]}}"#));
}

fn write_tone(e: &Engine, rel: &str, hz: f32, seconds: f32) {
    let samples: Vec<f32> = (0..(44_100.0 * seconds) as usize).map(|i| (i as f32 / 44_100.0 * hz * std::f32::consts::TAU).sin() * 0.5).collect();
    let path = e.root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lunaris_audio::wav::encode(44_100, 1, &samples)).unwrap();
}

#[test]
fn the_acoustics_tool_reports_a_room_a_wall_and_the_echoes() {
    let mut e = engine();
    room(&mut e);
    wall(&mut e, "[2,0,0]", "[0.2,4,10]"); // a partition across the room
    let behind = call(&mut e, "spawn", r#"{"components":{"Name":{"value":"Behind"},"Transform":{"pos":[4,0,0]}}}"#).get("uid").unwrap().as_u64().unwrap();
    let r = call(&mut e, "acoustics", &format!(r#"{{"source":{behind}}}"#));
    let src = r.get("source").unwrap();
    assert!(num(src, &["direct_transmittance"]) < 0.1, "{}", src.to_string_lossy());
    assert_eq!(src.get("clear_line_of_sight"), Some(&Json::Bool(false)));
    assert!(num(src, &["direct_lowpass_hz"]) < 9_000.0, "a wall takes the high pitches");
    let room = r.get("room").unwrap();
    assert!(num(room, &["closedness"]) > 0.99, "{}", room.to_string_lossy());
    assert!((0.3..3.0).contains(&num(room, &["rt60_seconds"])), "{}", room.to_string_lossy());
    assert!(room.get("verdict").unwrap().as_str().unwrap().len() > 5);
    assert_eq!(num(&r, &["occluders"]), 7.0);
    // a source in plain sight on the left, given as a point
    let clear = call(&mut e, "acoustics", r#"{"source":[-3,0,0]}"#);
    let s = clear.get("source").unwrap();
    assert!(num(s, &["direct_transmittance"]) > 0.999);
    assert_eq!(s.get("clear_line_of_sight"), Some(&Json::Bool(true)));
    assert!((num(s, &["distance_m"]) - 3.0).abs() < 1e-6);
    let echoes = s.get("echoes").unwrap().as_array().unwrap();
    assert!(echoes.len() >= 3, "{}", s.to_string_lossy());
    let delays: Vec<f64> = echoes.iter().map(|x| num(x, &["delay_ms"])).collect();
    assert!(delays.windows(2).all(|w| w[0] < w[1]) && delays[0] > 0.0 && *delays.last().unwrap() < 260.0, "{delays:?}");
    // errors say what is wrong
    assert_eq!(e.call("acoustics", &j(r#"{"source":123456}"#)).unwrap_err().code, "no_such_entity");
    assert_eq!(e.call("acoustics", &j(r#"{"source":[1,2]}"#)).unwrap_err().code, "type_mismatch");
    assert_eq!(e.call("acoustics", &j(r#"{"listener":"here"}"#)).unwrap_err().code, "type_mismatch");
    // a world with no colliders is open air
    let mut empty = engine();
    let r = call(&mut empty, "acoustics", "{}");
    assert_eq!(num(&r, &["occluders"]), 0.0);
    assert_eq!(num(r.get("room").unwrap(), &["rt60_seconds"]), 0.0);
}

#[test]
fn record_audio_hears_events_from_the_right_side_and_writes_a_wav() {
    let mut e = engine();
    room(&mut e);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[4,0,0]}}}"#);
    emitter(&mut e, 4.0, "coin", None);
    // with no SoundFile the event plays the built-in voice, flat: no panning, but it is heard
    let r = call(&mut e, "record_audio", r#"{"ticks":120,"path":"out/flat.wav"}"#);
    assert_eq!(num(&r, &["events"]), 4.0);
    assert_eq!(num(r.get("voices_started").unwrap(), &["coin"]), 4.0);
    assert!(r.get("silent") == Some(&Json::Bool(false)), "{}", r.to_string_lossy());
    let bytes = std::fs::read(e.root.join("out/flat.wav")).unwrap();
    let clip = lunaris_audio::wav::decode(&bytes).unwrap();
    assert_eq!((clip.channels, clip.rate, clip.frames()), (2, 44_100, 120 * 735));
    assert_eq!(num(&r, &["bytes"]) as usize, bytes.len());
    assert!((num(&r, &["left_rms_db"]) - num(&r, &["right_rms_db"])).abs() < 0.5, "flat voices are centred");
    // a SoundFile without a file makes the built-in voice spatial: it now comes from the right
    let mut s = engine();
    room(&mut s);
    call(&mut s, "spawn", r#"{"components":{"Transform":{"pos":[4,0,0]}}}"#);
    emitter(&mut s, 4.0, "coin", None);
    call(&mut s, "spawn", r#"{"components":{"SoundFile":{"name":"coin","file":""}}}"#);
    let r = call(&mut s, "record_audio", r#"{"ticks":90,"acoustics":false}"#);
    assert!(num(&r, &["right_rms_db"]) > num(&r, &["left_rms_db"]) + 3.0, "{}", r.to_string_lossy());
    assert!(r.get("path").is_none(), "without `path` nothing is written");
    assert_eq!(num(&r, &["room_rt60_seconds"]), 0.0, "acoustics off: no room");
    // with acoustics it is still on the right, and the room is measured
    let r = call(&mut s, "record_audio", r#"{"ticks":90}"#);
    assert!(num(&r, &["room_rt60_seconds"]) > 0.3 && num(&r, &["traces"]) > 0.0, "{}", r.to_string_lossy());
    // too many ticks, escaping paths
    assert_eq!(e.call("record_audio", &j(r#"{"ticks":99999}"#)).unwrap_err().code, "out_of_range");
    assert_eq!(e.call("record_audio", &j(r#"{"ticks":3,"path":"../x.wav"}"#)).unwrap_err().code, "bad_path");
}

#[test]
fn a_sound_file_plays_and_a_missing_one_is_a_warning_not_a_crash() {
    let mut e = engine();
    room(&mut e);
    write_tone(&e, "sounds/tone.wav", 440.0, 0.3);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[-3,0,0]}}}"#);
    emitter(&mut e, -3.0, "ding", Some(40));
    call(&mut e, "spawn", r#"{"components":{"SoundFile":{"name":"ding","file":"sounds/tone.wav","volume":1}}}"#);
    let r = call(&mut e, "record_audio", r#"{"ticks":60,"path":"out/ding.wav"}"#);
    assert!(num(&r, &["peak_db"]) > -20.0, "{}", r.to_string_lossy());
    assert!(num(&r, &["left_rms_db"]) > num(&r, &["right_rms_db"]) + 3.0, "the source is on the left: {}", r.to_string_lossy());
    assert!(r.get("warnings").is_none());
    // the timeline says when it sounded: nothing in the first quarter second, something after tick 40
    let tl = r.get("timeline").unwrap().as_array().unwrap();
    assert!(num(&tl[0], &["peak_db"]) < -100.0, "silence in the first quarter second");
    assert!(tl.iter().any(|x| num(x, &["peak_db"]) > -20.0));
    // a file that is not there
    let mut m = engine();
    room(&mut m);
    call(&mut m, "spawn", r#"{"components":{"Transform":{"pos":[-3,0,0]}}}"#);
    emitter(&mut m, -3.0, "ding", Some(10));
    call(&mut m, "spawn", r#"{"components":{"SoundFile":{"name":"ding","file":"sounds/nope.wav"}}}"#);
    let r = call(&mut m, "record_audio", r#"{"ticks":30}"#);
    assert_eq!(r.get("silent"), Some(&Json::Bool(true)));
    assert!(r.get("warnings").unwrap().to_string_lossy().contains("nope.wav"), "{}", r.to_string_lossy());
    // a file that is not a WAV
    std::fs::create_dir_all(m.root.join("sounds")).unwrap();
    std::fs::write(m.root.join("sounds").join("bad.wav"), b"not riff").unwrap();
    call(&mut m, "reset", "{}");
    call(&mut m, "spawn", r#"{"components":{"AudioSource":{"file":"sounds/bad.wav"}}}"#);
    let r = call(&mut m, "record_audio", r#"{"ticks":5}"#);
    assert!(r.get("warnings").unwrap().to_string_lossy().contains("not a WAV"), "{}", r.to_string_lossy());
}

#[test]
fn a_wall_between_a_looping_source_and_the_listener_makes_it_much_quieter() {
    let measure = |partition: bool| {
        let mut e = engine();
        room(&mut e);
        write_tone(&e, "sounds/hum.wav", 880.0, 0.5);
        if partition {
            wall(&mut e, "[2,0,0]", "[0.2,4,10]");
        }
        call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[4,0,0]},"AudioSource":{"file":"sounds/hum.wav","looped":true}}}"#);
        let r = call(&mut e, "record_audio", r#"{"ticks":90}"#);
        (num(&r, &["right_rms_db"]), num(&r, &["left_rms_db"]), num(&r, &["room_rt60_seconds"]))
    };
    let (open_r, _, rt_open) = measure(false);
    let (shut_r, _, rt_shut) = measure(true);
    assert!(shut_r < open_r - 8.0, "behind the partition: {shut_r} dB, in the open: {open_r} dB");
    assert!(rt_open > 0.3 && rt_shut > 0.3, "{rt_open} {rt_shut}");
}

#[test]
fn audio_sources_follow_start_stop_and_vanish_with_their_entity() {
    let mut e = engine();
    write_tone(&e, "sounds/hum.wav", 440.0, 0.05);
    let mixer = Arc::new(Mutex::new(Mixer::new()));
    let mut d = Director::new(mixer.clone());
    d.config.enabled = false;
    let uid = call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[0,0,-3]},"AudioSource":{"file":"sounds/hum.wav","looped":true}}}"#).get("uid").unwrap().as_u64().unwrap();
    let active = |m: &Arc<Mutex<Mixer>>| m.lock().unwrap().active();
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 1);
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 1, "it is not started twice");
    mixer.lock().unwrap().render_vec(4410); // the clip is 0.05 s: a loop is still there
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 1);
    call(&mut e, "set", &format!(r#"{{"uid":{uid},"component":"AudioSource","value":{{"playing":false}}}}"#));
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 0, "playing = false stops it");
    call(&mut e, "set", &format!(r#"{{"uid":{uid},"component":"AudioSource","value":{{"playing":true}}}}"#));
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 1);
    call(&mut e, "despawn", &format!(r#"{{"uid":{uid}}}"#));
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 0, "a source whose entity is gone is silent");
    // a one-shot plays once, however many updates follow
    let once = call(&mut e, "spawn", r#"{"components":{"AudioSource":{"file":"sounds/hum.wav","looped":false,"spatial":false}}}"#).get("uid").unwrap().as_u64().unwrap();
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 1);
    mixer.lock().unwrap().render_vec(4410);
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 0);
    d.update(&mut e, &[]);
    assert_eq!(active(&mixer), 0, "and does not start again");
    assert_eq!(d.stats.started.get(&format!("source:{once}")), Some(&1));
}

#[test]
fn the_reverb_and_the_filter_follow_from_the_measurements() {
    use lunaris_audio::trace::{Reflection, Room, SourceTrace};
    let cfg = AcousticsConfig::default();
    assert_eq!(room_settings(&Room::NONE, &cfg).wet, 0.0, "no room, no reverb");
    let live = Room { rt60: 2.0, closedness: 1.0, mean_free_path: 6.0, mean_absorption: 0.05, first_hit: 4.0 };
    let deadened = Room { rt60: 0.2, closedness: 1.0, mean_free_path: 2.0, mean_absorption: 0.5, first_hit: 1.5 };
    let outdoors = Room { rt60: 0.2, closedness: 0.45, mean_free_path: 6.0, mean_absorption: 0.1, first_hit: 6.0 };
    let (a, b, c) = (room_settings(&live, &cfg), room_settings(&deadened, &cfg), room_settings(&outdoors, &cfg));
    assert!(a.rt60 == 2.0 && a.damping < b.damping, "soft walls damp the high pitches more");
    assert!(c.wet < a.wet * 0.5, "open air hardly reverberates: {} vs {}", c.wet, a.wet);
    assert!(room_settings(&live, &AcousticsConfig { enabled: false, ..cfg }).wet == 0.0);
    // the direct filter: a wall closes it, distance closes it a little
    let clear = SourceTrace { transmittance: 1.0, visible: 1.0, distance: 2.0, reflections: vec![Reflection { delay: 0.02, gain: 0.1, dir: [1.0, 0.0, 0.0] }] };
    let blocked = SourceTrace { transmittance: 0.03, visible: 0.0, distance: 2.0, reflections: Vec::new() };
    let (ac, ab) = (acoustics_for(&clear, &cfg), acoustics_for(&blocked, &cfg));
    assert!(ac.direct_cutoff > 15_000.0 && ab.direct_cutoff < 6_000.0, "{} {}", ac.direct_cutoff, ab.direct_cutoff);
    assert_eq!((ac.direct_gain, ab.direct_gain), (1.0, 0.03));
    assert_eq!(ac.taps.len(), 1);
    assert!((ac.taps[0].gain - 0.1 * cfg.reflection_gain).abs() < 1e-6);
    let far = SourceTrace { distance: 400.0, ..clear };
    assert!(acoustics_for(&far, &cfg).direct_cutoff < ac.direct_cutoff / 3.0, "the air takes the high pitches over a distance");
}

/// The walking tour in `examples/echo_tour.lunaris.json`: four places that must sound like what they are.
#[test]
fn the_echo_tour_example_sounds_like_the_places_it_is_made_of() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut e = Engine::new(1, &root);
    call(&mut e, "load_scene", r#"{"path":"examples/echo_tour.lunaris.json"}"#);
    let at = |e: &mut Engine, x: f64| {
        let r = call(e, "acoustics", &format!(r#"{{"listener":[{x},1.6,0]}}"#));
        let room = r.get("room").unwrap().clone();
        (num(&room, &["rt60_seconds"]), num(&room, &["closedness"]), num(&room, &["reverb_level_db"]))
    };
    let (bath_rt, bath_closed, _) = at(&mut e, 2.0);
    let (living_rt, _, _) = at(&mut e, 8.0);
    let (hall_rt, hall_closed, hall_db) = at(&mut e, 24.0);
    let (_, yard_closed, yard_db) = at(&mut e, 46.0);
    eprintln!("bathroom {bath_rt} s, living room {living_rt} s, hall {hall_rt} s ({hall_db} dB), courtyard closedness {yard_closed} ({yard_db} dB)");
    assert!(bath_rt > 1.0 && bath_closed > 0.95, "tile rings: {bath_rt} s");
    assert!(living_rt < bath_rt * 0.7, "carpet and furniture dry it: {living_rt} s against {bath_rt} s");
    assert!(hall_rt > 1.5 * living_rt && hall_closed > 0.85, "the hall: {hall_rt} s");
    assert!(yard_closed < 0.7, "the courtyard is open: {yard_closed}");
    assert!(yard_db < hall_db - 10.0, "open air has far less reverberation than the hall: {yard_db} dB against {hall_db} dB");
    // the radio is in the store room: in line with the door it is much louder than from beside the door's wall
    let radio_pos = "[8,1.0,-5.4]";
    let aligned = call(&mut e, "acoustics", &format!(r#"{{"listener":[8,1.6,0],"source":{radio_pos}}}"#));
    let beside = call(&mut e, "acoustics", &format!(r#"{{"listener":[5,1.6,0],"source":{radio_pos}}}"#));
    let (t_door, t_wall) = (num(aligned.get("source").unwrap(), &["direct_transmittance"]), num(beside.get("source").unwrap(), &["direct_transmittance"]));
    assert!(t_door > 4.0 * t_wall, "through the door {t_door}, through the wall {t_wall}");
    assert!(num(aligned.get("source").unwrap(), &["direct_lowpass_hz"]) > num(beside.get("source").unwrap(), &["direct_lowpass_hz"]));
    // the whole tour, recorded: it makes sound, never clips, and starts every source
    let r = call(&mut e, "record_audio", r#"{"ticks":900}"#);
    assert_eq!(r.get("silent"), Some(&Json::Bool(false)), "{}", r.to_string_lossy());
    assert_eq!(num(&r, &["clipped_samples"]), 0.0, "{}", r.to_string_lossy());
    assert_eq!(r.get("voices_started").unwrap().as_object().map(|o| o.len()), Some(4), "{}", r.to_string_lossy());
    assert!(r.get("warnings").is_none(), "{}", r.to_string_lossy());
}

#[test]
fn the_background_tracer_ends_up_with_the_same_room_and_gives_the_first_blocks_their_acoustics_at_once() {
    let mut e = engine();
    room(&mut e);
    write_tone(&e, "sounds/hum.wav", 440.0, 0.4);
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[-3,0,0]},"AudioSource":{"file":"sounds/hum.wav","looped":true}}}"#);
    let inline = {
        let mut d = Director::new(Arc::new(Mutex::new(Mixer::new())));
        d.update(&mut e, &[]);
        d
    };
    // the first update already traced the new voice and measured the room, in the calling thread
    assert_eq!(inline.stats.traces, 1);
    assert_eq!(inline.stats.room_analyses, 1);
    assert!(inline.room.rt60 > 0.3 && inline.room.closedness > 0.99, "{:?}", inline.room);
    let mixer = Arc::new(Mutex::new(Mixer::new()));
    let mut threaded = Director::new(mixer.clone());
    threaded.spawn_tracer();
    threaded.update(&mut e, &[]);
    assert_eq!(threaded.stats.traces, 1, "the new voice is traced before it is heard even with a tracing thread");
    // later refreshes come from the thread: keep updating until one has arrived
    let t0 = std::time::Instant::now();
    while threaded.stats.traces < 3 && t0.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        threaded.update(&mut e, &[]);
    }
    assert!(threaded.stats.traces >= 3, "the tracing thread never answered");
    assert!((threaded.room.rt60 - inline.room.rt60).abs() < 0.2 * inline.room.rt60, "{} against {}", threaded.room.rt60, inline.room.rt60);
    assert_eq!(mixer.lock().unwrap().active(), 1);
}

#[test]
fn the_events_tool_says_where_each_event_happened() {
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[4,1.5,-2]}}}"#);
    emitter(&mut e, 4.0, "coin", Some(0));
    let r = call(&mut e, "step", r#"{"ticks":2}"#);
    let ev = r.get("events").unwrap().as_array().unwrap();
    assert_eq!(ev.len(), 1);
    let pos: Vec<f64> = ev[0].get("pos").unwrap().as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    assert_eq!(pos, vec![4.0, 1.5, -2.0]);
    let listed = call(&mut e, "events", "{}");
    assert!(listed.get("events").unwrap().as_array().unwrap()[0].get("pos").is_some());
}

#[test]
fn a_mixer_bus_turns_its_sounds_down_and_a_settings_resource_turns_it_down_more() {
    let run = |mixer: Option<&str>, setting: Option<f64>| {
        let mut e = engine();
        room(&mut e);
        write_tone(&e, "sounds/tone.wav", 440.0, 0.3);
        call(&mut e, "spawn", r#"{"components":{"Transform":{"pos":[-3,0,0]}}}"#);
        emitter(&mut e, -3.0, "ding", Some(40));
        call(&mut e, "spawn", r#"{"components":{"SoundFile":{"name":"ding","file":"sounds/tone.wav","volume":1}}}"#);
        if let Some(text) = mixer {
            let text = text.replace('\n', "\\n");
            call(&mut e, "spawn", &format!(r#"{{"components":{{"AudioMixer":{{"config":"{text}"}}}}}}"#));
        }
        if let Some(v) = setting {
            call(&mut e, "set_resource", &format!(r#"{{"name":"bus.sfx.volume","value":{v}}}"#));
        }
        let r = call(&mut e, "record_audio", r#"{"ticks":60,"acoustics":false}"#);
        num(&r, &["peak_db"])
    };
    let plain = run(None, None);
    let quiet = run(Some("bus master\nbus sfx parent=master volume=0.25\nroute sounds/* -> sfx"), None);
    assert!((plain - quiet - 12.04).abs() < 1.0, "a bus at 0.25 is 12 dB down: {plain} -> {quiet}");
    // the settings menu: a slider at 0.5 on top of it, and a mute
    let slid = run(Some("bus master\nbus sfx parent=master volume=0.25\nroute sounds/* -> sfx"), Some(0.5));
    assert!((quiet - slid - 6.02).abs() < 1.0, "{quiet} -> {slid}");
    // a route that matches nothing leaves the sound alone
    let other = run(Some("bus master volume=0.1\nbus music parent=master\nroute music/* -> music"), None);
    assert!((plain - other).abs() < 0.5, "{plain} vs {other}");
    // a broken mixer text is a warning, not a crash
    let broken = run(Some("bus a parent=zzz"), None);
    assert!((plain - broken).abs() < 0.5);
}

/// Levels of the quarter-second slices of a recording, in dB (the `peak_db` of each entry of the timeline).
fn peaks(r: &Json) -> Vec<f64> {
    r.get("timeline").unwrap().as_array().unwrap().iter().map(|t| t.get("peak_db").unwrap().as_f64().unwrap()).collect()
}

#[test]
fn a_music_player_crossfades_through_its_playlist_and_pauses_and_skips_on_command() {
    let mut e = engine();
    write_tone(&e, "music/a.wav", 440.0, 1.0);
    write_tone(&e, "music/b.wav", 660.0, 1.0);
    write_tone(&e, "music/c.wav", 880.0, 1.0);
    let player = r#"{"components":{"MusicPlayer":{"playlist":"music/a.wav\nmusic/b.wav # a comment\nmusic/c.wav volume=0.5","crossfade":0.5,"repeat":false}}}"#;
    let uid = call(&mut e, "spawn", player).get("uid").unwrap().as_u64().unwrap();
    let r = call(&mut e, "record_audio", r#"{"ticks":150,"acoustics":false}"#);
    // 3 tracks of 1 s with 0.5 s overlaps run 2 s; a list that does not repeat then falls silent
    let started = r.get("voices_started").unwrap();
    for t in ["music/a.wav", "music/b.wav", "music/c.wav"] {
        assert_eq!(started.get(&format!("music:{t}")).and_then(|n| n.as_u64()), Some(1), "{t} is played once: {started:?}");
    }
    let p = peaks(&r);
    assert!(p[0] < p[3] - 3.0, "the music fades in: {p:?}");
    // equal power: with different tones the level holds through the overlaps (within a few dB of the steady part)
    assert!(p[2..6].iter().all(|v| *v > p[3] - 5.0), "no dip while crossfading: {p:?}");
    assert!(p[9] < -60.0, "silent after the last track: {p:?}");
    assert!(r.get("warnings").is_none(), "{r:?}");

    // repeat: the list starts over
    call(&mut e, "set", &format!(r#"{{"uid":{uid},"component":"MusicPlayer","value":{{"repeat":true}}}}"#));
    let r = call(&mut e, "record_audio", r#"{"ticks":180,"acoustics":false}"#);
    assert!(r.get("voices_started").unwrap().get("music:music/a.wav").and_then(|n| n.as_u64()).unwrap_or(0) >= 1, "{r:?}");
    assert!(peaks(&r).iter().all(|v| *v > -40.0), "never silent while repeating: {:?}", peaks(&r));

    // `skip` moves on at once; pausing fades out and stops
    let mut e = engine();
    write_tone(&e, "music/a.wav", 440.0, 4.0);
    write_tone(&e, "music/b.wav", 660.0, 4.0);
    call(&mut e, "spawn", r#"{"components":{"MusicPlayer":{"playlist":"music/a.wav\nmusic/b.wav","crossfade":0.25}}}"#);
    call(&mut e, "define_system", r#"{"name":"dj","each":["MusicPlayer"],"do":[{"if":{"eq":[{"tick":true},60]},"then":[{"set":"MusicPlayer.skip","to":1}]},{"if":{"eq":[{"tick":true},120]},"then":[{"set":"MusicPlayer.playing","to":false}]}]}"#);
    let r = call(&mut e, "record_audio", r#"{"ticks":240,"acoustics":false}"#);
    let started = r.get("voices_started").unwrap();
    assert_eq!(started.get("music:music/b.wav").and_then(|n| n.as_u64()), Some(1), "b started by the skip, long before a ended: {started:?}");
    let p = peaks(&r);
    assert!(p[4] > -12.0 && p[5] > -12.0, "playing before the pause: {p:?}");
    assert!(p[10] < -60.0 && p[14] < -60.0, "paused after fading out: {p:?}");
    // a missing track is a warning, not a hang
    let mut e = engine();
    call(&mut e, "spawn", r#"{"components":{"MusicPlayer":{"playlist":"music/nope.wav\nmusic/nope2.wav"}}}"#);
    let r = call(&mut e, "record_audio", r#"{"ticks":30,"acoustics":false}"#);
    assert!(r.get("silent").unwrap().as_bool().unwrap());
    assert!(r.get("warnings").unwrap().as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("nope.wav")), "{r:?}");
}

