//! Flipbook animation for 2D sprites: `SpriteAnim` beside an `Animator` and a `Sprite` turns the animator's clip name and clock into `Sprite.frame`.
//!
//! The `Animator` is the same one that plays 3D clips: `Animator.clip` names the clip, `speed` and `playing` run its clock, a `StateMachine` state with `clip=run`
//! switches it, and `looping` says whether it repeats. `SpriteAnim.clips` is the table of what each clip looks like, one clip a line:
//!
//! ```text
//! idle: 0-3 fps=6
//! run: 4-9 fps=12 step@1 step@4
//! jump: 10-12 fps=10 once
//! spin: 13-16 pingpong
//! ```
//!
//! After the name come the frames (numbers and `from-to` ranges, spaces or commas between them), `fps=N` (default 8), `loop`, `once` or `pingpong` (without one of
//! them the animator's `looping` decides: loop or once) and `NAME@K` markers: the event `NAME` is emitted from the entity each time the clip reaches its K-th picture
//! (counted from 0, so `step@0` fires at the very start), for the sound director or a graph system.
//!
//! A clip that changes starts again from its first picture. `done` is 1 once a `once` clip has shown its last picture for as long as the others.
use crate::components::{Animator, Sprite, SpriteAnim, Transform};
use lunaris_ecs::{Entity, World};

type V3 = [f32; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Loop,
    Once,
    PingPong,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpriteClip {
    pub name: String,
    pub frames: Vec<u32>,
    pub fps: f32,
    pub mode: Option<Mode>,
    /// (event name, position in the clip's list of pictures)
    pub marks: Vec<(String, usize)>,
}

/// The table of clips as text; the error says which line is wrong.
pub fn parse(text: &str) -> Result<Vec<SpriteClip>, String> {
    let mut out: Vec<SpriteClip> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let at = n + 1;
        let (name, rest) = line.split_once(':').ok_or_else(|| format!("line {at}: expected `name: 0-3 fps=8 loop`"))?;
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("line {at}: `{name}` is not a clip name (no spaces)"));
        }
        if out.iter().any(|c| c.name == name) {
            return Err(format!("line {at}: the clip `{name}` is written twice"));
        }
        let mut clip = SpriteClip { name: name.to_string(), frames: Vec::new(), fps: 8.0, mode: None, marks: Vec::new() };
        let mut marks: Vec<(String, usize, usize)> = Vec::new();
        for word in rest.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()) {
            if let Some(v) = word.strip_prefix("fps=") {
                clip.fps = v.parse::<f32>().ok().filter(|f| f.is_finite() && *f > 0.0 && *f <= 1000.0).ok_or_else(|| format!("line {at}: fps must be a number above 0, not `{v}`"))?;
            } else if let Some((event, k)) = word.split_once('@') {
                let k = k.parse::<usize>().map_err(|_| format!("line {at}: `{word}` is not `NAME@POSITION` (the position counts pictures from 0)"))?;
                if event.is_empty() || !event.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
                    return Err(format!("line {at}: `{event}` is not an event name"));
                }
                marks.push((event.to_string(), k, at));
            } else if word == "loop" || word == "once" || word == "pingpong" {
                clip.mode = Some(match word {
                    "loop" => Mode::Loop,
                    "once" => Mode::Once,
                    _ => Mode::PingPong,
                });
            } else if let Some((a, b)) = word.split_once('-') {
                let (a, b) = (a.parse::<u32>(), b.parse::<u32>());
                match (a, b) {
                    (Ok(a), Ok(b)) if a <= b && (b - a) < 4096 => clip.frames.extend(a..=b),
                    _ => return Err(format!("line {at}: `{word}` is not a range of pictures like `0-3`")),
                }
            } else if let Ok(f) = word.parse::<u32>() {
                clip.frames.push(f);
            } else {
                return Err(format!("line {at}: `{word}` is not a picture number, a range, `fps=N`, `loop`, `once`, `pingpong` or `NAME@POSITION`"));
            }
            if clip.frames.len() > 4096 {
                return Err(format!("line {at}: a clip has at most 4096 pictures"));
            }
        }
        if clip.frames.is_empty() {
            return Err(format!("line {at}: the clip `{name}` has no pictures: write them as `{name}: 0-3`"));
        }
        for (event, k, line) in marks {
            if k >= clip.frames.len() {
                return Err(format!("line {line}: `{event}@{k}`: the clip has only {} pictures (0 to {})", clip.frames.len(), clip.frames.len() - 1));
            }
            clip.marks.push((event, k));
        }
        out.push(clip);
    }
    Ok(out)
}

/// Which picture of a clip of `len` pictures is showing at step `n` (steps are counted from the start of the clip, one per `1/fps` seconds).
pub fn index(mode: Mode, len: usize, n: u64) -> usize {
    let len = len.max(1) as u64;
    match mode {
        Mode::Loop => (n % len) as usize,
        Mode::Once => n.min(len - 1) as usize,
        Mode::PingPong => {
            let period = (2 * len).saturating_sub(2).max(1);
            let m = n % period;
            (if m < len { m } else { period - m }) as usize
        }
    }
}

/// Run after the animators have advanced their clocks: write `Sprite.frame` and emit the markers that were reached.
pub fn update(world: &mut World, events: &mut Vec<(String, u64, Option<V3>)>) {
    let mut found: Vec<(u64, Entity, SpriteAnim, Animator)> = Vec::new();
    let _ = world.for_each_uid::<(&SpriteAnim, &Animator, &Sprite), _>(|e, uid, (s, a, _)| found.push((uid, e, *s, *a)));
    found.sort_by_key(|f| f.0);
    for (uid, e, mut s, a) in found {
        let text = world.strings().get(s.clips.0).to_string();
        let Ok(clips) = parse(&text) else { continue };
        let wanted = if a.clip.0 == 0 { None } else { Some(world.strings().get(a.clip.0).to_string()) };
        let Some(clip) = clips.iter().find(|c| wanted.as_deref().is_none_or(|w| c.name == w)) else { continue };
        // the clip's name as a string id, also when the animator names none (the first clip plays)
        let clip_sym = if a.clip.0 != 0 { a.clip.0 } else { world.strings_mut().intern(&clip.name) };
        if s.last_clip.0 != clip_sym {
            s.last_clip.0 = clip_sym;
            s.start = a.time;
            s.steps = 0;
            s.done = false;
        }
        let mut local = a.time - s.start;
        if !local.is_finite() || local < 0.0 {
            // the clock was set back: start the clip again from here
            s.start = a.time;
            s.steps = 0;
            local = 0.0;
        }
        let mode = clip.mode.unwrap_or(if a.looping { Mode::Loop } else { Mode::Once });
        let n = ((local * clip.fps).floor().max(0.0) as u64).min(u32::MAX as u64 - 1);
        let len = clip.frames.len();
        let frame = clip.frames[index(mode, len, n)];
        // the pictures entered since last time (at most a few: a long pause does not replay a second of footsteps)
        let first = (s.steps as u64).max(n.saturating_sub(7));
        if !clip.marks.is_empty() {
            let pos = world.get::<Transform>(e).map(|t| t.pos);
            for k in first..=n {
                if mode == Mode::Once && k >= len as u64 {
                    break;
                }
                let at = index(mode, len, k);
                for (name, mark) in &clip.marks {
                    if *mark == at {
                        events.push((name.clone(), uid, pos));
                    }
                }
            }
        }
        s.steps = (n + 1) as u32;
        s.done = mode == Mode::Once && n >= len as u64;
        if let Some(w) = world.get_mut::<SpriteAnim>(e) {
            *w = s;
        }
        if let Some(sp) = world.get_mut::<Sprite>(e) {
            sp.frame = frame;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::Sim;
    use lunaris_json::parse as json;

    #[test]
    fn the_table_is_read_and_mistakes_are_named() {
        let t = parse("idle: 0-3 fps=6\n# a comment\nrun: 4-9 fps=12 step@1, step@4\njump: 10 12 11 once\nspin: 13-16 pingpong").unwrap();
        assert_eq!(t.len(), 4);
        assert_eq!((t[0].name.as_str(), t[0].frames.clone(), t[0].fps, t[0].mode), ("idle", vec![0, 1, 2, 3], 6.0, None));
        assert_eq!(t[1].marks, vec![("step".to_string(), 1), ("step".to_string(), 4)]);
        assert_eq!((t[2].frames.clone(), t[2].mode, t[2].fps), (vec![10, 12, 11], Some(Mode::Once), 8.0));
        assert_eq!(t[3].mode, Some(Mode::PingPong));
        for (bad, why) in [("idle 0-3", "expected"), ("idle:", "no pictures"), ("a: 0-3\na: 1", "twice"), ("a: 3-1", "range"), ("a: 0-3 fps=0", "fps"), ("a: 0-3 x@9", "only 4 pictures"), ("a: 0-3 x@y", "NAME@POSITION"), ("a: 0-3 sprint", "not a picture")] {
            let e = parse(bad).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
    }

    #[test]
    fn the_picture_follows_the_mode() {
        let seq = |m, len, n: u64| (0..n).map(|k| index(m, len, k)).collect::<Vec<_>>();
        assert_eq!(seq(Mode::Loop, 3, 7), vec![0, 1, 2, 0, 1, 2, 0]);
        assert_eq!(seq(Mode::Once, 3, 6), vec![0, 1, 2, 2, 2, 2]);
        assert_eq!(seq(Mode::PingPong, 4, 10), vec![0, 1, 2, 3, 2, 1, 0, 1, 2, 3]);
        assert_eq!(seq(Mode::PingPong, 1, 3), vec![0, 0, 0]);
    }

    fn scene(clips: &str, animator: &str) -> Sim {
        let mut s = Sim::new(1);
        let c = json(&format!(r#"{{"Transform":{{"pos":[1,2,0]}},"Sprite":{{"sheet":[8,2]}},"Animator":{animator},"SpriteAnim":{{"clips":{}}}}}"#, lunaris_json::Json::str(clips).to_string_lossy())).unwrap();
        let comps = c.as_object().unwrap().to_vec();
        s.apply(&Command::Spawn { uid: Some(1), components: comps }).unwrap();
        s
    }

    fn frame(s: &Sim) -> u32 {
        s.world.get::<Sprite>(s.world.entity(1).unwrap()).unwrap().frame
    }

    #[test]
    fn a_looping_clip_shows_its_pictures_at_its_pace_and_a_new_clip_starts_again() {
        // 10 pictures a second at 60 ticks a second: six ticks a picture; look in the middle of each
        let mut s = scene("walk: 4-7 fps=10\nrun: 0 1 2 fps=10", r#"{"clip":"walk"}"#);
        let mut seen = Vec::new();
        for tick in 1..=30 {
            s.step();
            if tick % 6 == 3 {
                seen.push(frame(&s));
            }
        }
        assert_eq!(seen, vec![4, 5, 6, 7, 4], "{seen:?}");
        // another clip: from its first picture, whatever the animator's clock says
        s.apply(&Command::Set { uid: 1, component: "Animator".into(), value: json(r#"{"clip":"run"}"#).unwrap() }).unwrap();
        s.step();
        assert_eq!(frame(&s), 0);
        s.step_n(7);
        assert_eq!(frame(&s), 1);
        // a clip the table does not know leaves the picture as it was; the empty name plays the first clip
        s.apply(&Command::Set { uid: 1, component: "Animator".into(), value: json(r#"{"clip":"nope"}"#).unwrap() }).unwrap();
        s.step();
        assert_eq!(frame(&s), 1);
        s.apply(&Command::Set { uid: 1, component: "Animator".into(), value: json(r#"{"clip":""}"#).unwrap() }).unwrap();
        s.step();
        assert_eq!(frame(&s), 4);
    }

    #[test]
    fn a_clip_that_plays_once_holds_its_last_picture_and_says_when_it_is_done() {
        let mut s = scene("hit: 8 9 10 fps=10 once", r#"{"clip":"hit","looping":true}"#);
        let done = |s: &Sim| s.world.get::<SpriteAnim>(s.world.entity(1).unwrap()).unwrap().done;
        s.step_n(17);
        assert_eq!((frame(&s), done(&s)), (10, false), "on the last picture, which has not been shown for its whole time yet");
        s.step_n(15);
        assert_eq!((frame(&s), done(&s)), (10, true));
        s.step_n(200);
        assert_eq!((frame(&s), done(&s)), (10, true));
        // with the animator not looping, a clip with no mode of its own plays once too
        let mut t = scene("hit: 8 9 10 fps=10", r#"{"clip":"hit","looping":false}"#);
        t.step_n(100);
        assert_eq!(frame(&t), 10);
    }

    #[test]
    fn markers_fire_when_their_picture_comes_up_once_per_pass() {
        let mut s = scene("run: 0-3 fps=10 step@1 step@3 go@0", r#"{"clip":"run"}"#);
        s.step_n(60); // one second: ten pictures, two and a half passes
        let named = |n: &str| s.events.iter().filter(|e| e.name == n).count();
        assert_eq!((named("go"), named("step")), (3, 5), "{:?}", s.events.iter().map(|e| (e.tick, e.name.clone())).collect::<Vec<_>>());
        assert!(s.events.iter().all(|e| e.uid == 1 && e.pos == Some([1.0, 2.0, 0.0])));
        // a clip that plays once fires each marker once
        let mut t = scene("hit: 0-2 fps=10 hit@1 end@2 once", r#"{"clip":"hit"}"#);
        t.step_n(120);
        let count = |n: &str| t.events.iter().filter(|e| e.name == n).count();
        assert_eq!((count("hit"), count("end")), (1, 1));
    }

    #[test]
    fn a_state_machine_state_switches_the_sprite_clip() {
        let mut s = Sim::new(1);
        let machine = "state idle clip=idle\nstate run clip=run speed=2\nidle -> run when $go > 0.5\nrun -> idle when $go < 0.5";
        let comps = json(&format!(
            r#"{{"Transform":{{}},"Sprite":{{"sheet":[8,2]}},"Animator":{{}},"SpriteAnim":{{"clips":"idle: 0-1 fps=2\nrun: 8-11 fps=10"}},"StateMachine":{{"machine":{}}}}}"#,
            lunaris_json::Json::str(machine).to_string_lossy()
        ))
        .unwrap();
        s.apply(&Command::Spawn { uid: Some(1), components: comps.as_object().unwrap().to_vec() }).unwrap();
        s.step_n(5);
        assert!(frame(&s) <= 1, "idle shows pictures 0 and 1: {}", frame(&s));
        s.apply(&Command::SetResource { name: "go".into(), value: Some(1.0) }).unwrap();
        s.step_n(4);
        assert!((8..=11).contains(&frame(&s)), "run shows 8 to 11: {}", frame(&s));
        s.apply(&Command::SetResource { name: "go".into(), value: Some(0.0) }).unwrap();
        s.step_n(4);
        assert!(frame(&s) <= 1, "back to idle: {}", frame(&s));
    }

    #[test]
    fn animator_speed_and_playing_control_the_pace() {
        let mut s = scene("walk: 0-3 fps=10", r#"{"clip":"walk","speed":2}"#);
        s.step_n(9); // 0.15 s of the clock at twice the speed is 0.3 s: the third picture
        assert_eq!(frame(&s), 2);
        s.apply(&Command::Set { uid: 1, component: "Animator".into(), value: json(r#"{"playing":false}"#).unwrap() }).unwrap();
        let held = frame(&s);
        s.step_n(60);
        assert_eq!(frame(&s), held);
    }
}
