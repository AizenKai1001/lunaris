//! Inertialization: the chain of clips an animator keeps, and when its offsets are over.
use super::*;
use crate::components::Animator;
use crate::inertia::Inertia;
use lunaris_json::{parse, Json};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn chain(s: &Sim) -> Inertia {
    let e = s.world.entity(1).unwrap();
    *s.world.get::<Inertia>(e).unwrap()
}

#[test]
fn a_switch_extends_the_chain_and_the_offset_dies_with_the_span() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Animator":{"clip":"walk","looping":true,"fade":0.5},"Inertia":{"span":0.2}}"#);
    for _ in 0..10 {
        s.step();
    }
    assert_eq!(chain(&s).n, 1);
    let e = s.world.entity(1).unwrap();
    let b = lunaris_ecs::Sym(s.world.strings_mut().intern("run"));
    {
        let a = s.world.get_mut::<Animator>(e).unwrap();
        a.clip = b;
        a.time = 3.0;
        a.fade = 0.5;
    }
    s.step();
    let c = chain(&s);
    assert_eq!(c.n, 2, "a changed clip is a switch");
    assert_eq!(c.clip[1], b.0);
    assert!((c.t0[1] - 3.0).abs() < 1e-6 && (c.len[1] - 0.2).abs() < 1e-6, "{c:?}");
    assert!(c.gap[0] > 0.14 && c.gap[0] < 0.2, "the first clip played about 10 steps: {}", c.gap[0]);
    let a = *s.world.get::<Animator>(e).unwrap();
    assert!(a.fade == 0.0 && a.blend == 1.0, "no crossfade: {a:?}");
    // the offset is over after its span: the chain is just the clip playing
    let steps = (0.2 / s.dt).ceil() as usize + 2;
    for _ in 0..steps {
        s.step();
    }
    let c = chain(&s);
    assert_eq!(c.n, 1);
    assert_eq!(c.clip[0], b.0);
    // a jump of the time inside the same clip is a switch too (what motion matching does)
    {
        let a = s.world.get_mut::<Animator>(e).unwrap();
        a.time += 1.5;
    }
    s.step();
    assert_eq!(chain(&s).n, 2);
    // a clip that plays on at its own speed is not
    for _ in 0..steps {
        s.step();
    }
    assert_eq!(chain(&s).n, 1);
    for _ in 0..5 {
        s.step();
    }
    assert_eq!(chain(&s).n, 1);
}

#[test]
fn many_switches_keep_the_newest_entries() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Animator":{"clip":"a"},"Inertia":{"span":5.0}}"#);
    s.step();
    let e = s.world.entity(1).unwrap();
    for k in 0..10 {
        {
            let a = s.world.get_mut::<Animator>(e).unwrap();
            a.time += 2.0 + k as f32;
        }
        s.step();
    }
    let c = chain(&s);
    assert_eq!(c.n as usize, crate::inertia::MAX, "{c:?}");
}

#[test]
fn a_switch_made_after_animate_is_noticed_in_the_same_tick() {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Animator":{"clip":"walk"},"Inertia":{"span":0.2}}"#);
    for _ in 0..5 {
        s.step();
    }
    let e = s.world.entity(1).unwrap();
    let b = lunaris_ecs::Sym(s.world.strings_mut().intern("run"));
    {
        let a = s.world.get_mut::<Animator>(e).unwrap();
        a.clip = b;
        a.time = 4.0;
    }
    // what motion matching does after `animate`: the chain is told at once
    crate::inertia::notice(&mut s.world);
    let c = chain(&s);
    assert_eq!(c.n, 2, "{c:?}");
    assert_eq!(c.clip[1], b.0);
    // and the next step does not see the same switch again
    s.step();
    let c = chain(&s);
    assert_eq!(c.n, 2, "{c:?}");
    assert!(c.gap[1] > 0.0 && c.gap[1] < 0.05, "{}", c.gap[1]);
}
