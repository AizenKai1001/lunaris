//! The decision about a pair of players in occlusion.rs against the one that was there before the entries were kept in integers and the table was looked at once: a long busy match, call by call.
use super::occlusion::{Counters, Occlusion, Params, Sightings, CALM_MS, CALM_Q, MOVED_M, MOVED_Q, STILL_M, STILL_Q};
use super::server::Game;
use super::state::{class, EntityState, POS_SCALE, VEL_SCALE};

type V3 = [f32; 3];

#[test]
fn the_limits_in_integers_are_the_limits_in_metres() {
    for k in -5000i32..=5000 {
        let (p, v) = (k as f32 / POS_SCALE, k as f32 / VEL_SCALE);
        assert_eq!(p.abs() < STILL_M, k.unsigned_abs() < STILL_Q, "still, {k}");
        assert_eq!(p.abs() < MOVED_M, k.unsigned_abs() < MOVED_Q, "moved, {k}");
        assert_eq!(v.abs() < CALM_MS, k.unsigned_abs() < CALM_Q, "calm, {k}");
    }
}

/// The decision about one pair as it was made before (floats, NaN for a place that is not known, the table looked at again for the old answer): the new one has to give its answers, spend the same rays and count
/// the same things, call after call.
mod old {
    use super::super::occlusion::{sees, Counters, Params, CALM_MS, MOVED_M, STILL_M};
    use super::super::server::Game;
    use super::super::state::{class, EntityState};
    use std::collections::HashMap;

    pub struct Seen {
        until: u32,
        due: u32,
        touched: u32,
        epoch: u32,
        last: bool,
        low: bool,
        at: [f32; 6],
        v: [f32; 6],
    }

    #[derive(Default)]
    pub struct Sightings {
        map: HashMap<u32, Seen>,
        calls: u32,
    }

    fn le(a: u32, b: u32) -> bool {
        (a.wrapping_sub(b) as i32) <= 0
    }

    fn lt(a: u32, b: u32) -> bool {
        (a.wrapping_sub(b) as i32) < 0
    }

    impl Sightings {
        pub fn prune(&mut self, tick: u32) {
            self.calls += 1;
            if self.calls % 32 == 0 {
                self.map.retain(|_, e| tick.wrapping_sub(e.touched) < 120);
            }
        }

        pub fn len(&self) -> usize {
            self.map.len()
        }

        #[allow(clippy::too_many_arguments)]
        pub fn visible<G: Game>(&mut self, game: &G, p: &Params, obs: &EntityState, tgt: &EntityState, dist: f32, budget: &mut u32, n: &mut Counters) -> bool {
            let tick = p.tick;
            let id = tgt.id;
            let (op, tp) = (obs.pos_m(), tgt.pos_m());
            let (ov, tv) = (obs.vel_ms(), tgt.vel_ms());
            let low = tgt.class == class::PLAYER && tgt.flags & p.low_flags != 0;
            let hold_until = tick.wrapping_add(p.hold);
            if dist <= p.safe {
                let seen = Seen { until: hold_until, due: tick, touched: tick, epoch: p.epoch, last: true, low: false, at: [f32::NAN; 6], v: [f32::NAN; 6] };
                self.map.insert(id, seen);
                return true;
            }
            if let Some(e) = self.map.get_mut(&id) {
                e.touched = tick;
                if e.epoch == p.epoch {
                    let calm = (0..3).all(|a| (ov[a] - e.v[a]).abs() < CALM_MS && (tv[a] - e.v[3 + a]).abs() < CALM_MS);
                    if calm && lt(tick, e.due) && (0..3).all(|a| (op[a] - e.at[a]).abs() < MOVED_M && (tp[a] - e.at[3 + a]).abs() < MOVED_M) {
                        return le(tick, e.until);
                    }
                    let still = (0..3).all(|a| (op[a] - e.at[a]).abs() < STILL_M && (tp[a] - e.at[3 + a]).abs() < STILL_M);
                    if calm && still && e.low == low {
                        e.due = tick.wrapping_add(p.still);
                        if e.last {
                            e.until = e.due.wrapping_add(p.hold);
                        }
                        n.still += 1;
                        return le(tick, e.until);
                    }
                }
            }
            let open = dist <= p.mid_m;
            let afford = |s: &mut Sightings, n: &mut Counters| -> bool {
                n.unaffordable += 1;
                match s.map.get_mut(&id) {
                    Some(e) => {
                        e.due = tick.wrapping_add(1);
                        le(tick, e.until)
                    }
                    None => open,
                }
            };
            if *budget == 0 {
                return afford(self, n);
            }
            n.pairs += 1;
            let Some(vis) = sees(game, p, obs, tgt, low, budget, &mut n.rays) else {
                return afford(self, n);
            };
            let tier = if dist <= p.near_m {
                0
            } else if dist <= p.mid_m {
                1
            } else {
                2
            };
            let period = p.retest[tier];
            let stagger = id.wrapping_mul(0x9E37_79B1) >> 31;
            let due = tick.wrapping_add(period + stagger);
            let at = [op[0], op[1], op[2], tp[0], tp[1], tp[2]];
            let v = [ov[0], ov[1], ov[2], tv[0], tv[1], tv[2]];
            match self.map.get_mut(&id) {
                Some(e) => {
                    if vis {
                        e.until = due.wrapping_add(p.hold);
                    }
                    e.last = vis;
                    e.low = low;
                    e.due = due;
                    e.touched = tick;
                    e.epoch = p.epoch;
                    e.at = at;
                    e.v = v;
                    le(tick, e.until)
                }
                None => {
                    let until = if vis { due.wrapping_add(p.hold) } else { tick.wrapping_sub(1) };
                    self.map.insert(id, Seen { until, due, touched: tick, epoch: p.epoch, last: vis, low, at, v });
                    vis
                }
            }
        }
    }
}

/// A level of a few boxes, for the tests: a line is blocked when it goes through one of them.
struct Level {
    boxes: Vec<(V3, V3)>,
}

fn crosses(a: V3, b: V3, lo: V3, hi: V3) -> bool {
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for k in 0..3 {
        let d = b[k] - a[k];
        if d.abs() < 1.0e-9 {
            if a[k] < lo[k] || a[k] > hi[k] {
                return false;
            }
        } else {
            let (mut u, mut v) = ((lo[k] - a[k]) / d, (hi[k] - a[k]) / d);
            if u > v {
                std::mem::swap(&mut u, &mut v);
            }
            t0 = t0.max(u);
            t1 = t1.min(v);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl Game for Level {
    fn on_join(&mut self, _w: &mut super::server::World, _slot: usize, _name: &str) -> EntityState {
        EntityState::default()
    }
    fn blocked(&self, from: V3, to: V3) -> bool {
        self.boxes.iter().any(|(lo, hi)| crosses(from, to, *lo, *hi))
    }
}

#[test]
fn the_decision_about_a_pair_is_what_it_was_before_the_table_was_looked_at_once_and_kept_in_integers() {
    // 36 players in a 130 m square with a wall across the middle and two shorter ones, walking at 0 to 8 m/s, some of them crouching now and then; a level that changes every 700 ticks; and rays that run out
    let game = Level { boxes: vec![([-1.0, 0.0, -30.0], [1.0, 8.0, 30.0]), ([40.0, 0.0, 10.0], [41.0, 5.0, 70.0]), ([-50.0, 0.0, -30.0], [-20.0, 4.0, -29.0])] };
    let n = 36usize;
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    };
    let mut pos: Vec<V3> = (0..n).map(|_| [(rnd() - 0.5) * 130.0, 0.0, (rnd() - 0.5) * 130.0]).collect();
    let mut head: Vec<f32> = (0..n).map(|_| rnd() * 6.28).collect();
    let mut speed = vec![0.0f32; n];
    let mut flags = vec![0u16; n];
    let cfg = Occlusion::default();
    let per = 40u32;
    let mut new_seen: Vec<Sightings> = (0..n).map(|_| Sightings::default()).collect();
    let mut old_seen: Vec<old::Sightings> = (0..n).map(|_| old::Sightings::default()).collect();
    let mut budget: Vec<(u32, u32)> = vec![(per * 4, 0); n];
    let (mut calls, mut stills, mut pairs, mut hidden, mut afforded) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for tick in 1..=2100u32 {
        let mut states: Vec<EntityState> = Vec::new();
        for i in 0..n {
            if tick % 37 == i as u32 % 37 {
                head[i] += (rnd() - 0.5) * 2.4;
                speed[i] = [0.0, 0.5, 2.0, 5.0, 8.0][(rnd() * 4.99) as usize];
                flags[i] = if rnd() < 0.2 { 1 } else { 0 };
            }
            let (s, c) = head[i].sin_cos();
            pos[i][0] = (pos[i][0] + s * speed[i] / 30.0).clamp(-65.0, 65.0);
            pos[i][2] = (pos[i][2] + c * speed[i] / 30.0).clamp(-65.0, 65.0);
            let mut st = EntityState { id: i as u32 + 1, class: class::PLAYER, health: 100, flags: flags[i], ..Default::default() };
            st.set_pos_m(pos[i]);
            st.set_vel_ms([s * speed[i], 0.0, c * speed[i]]);
            states.push(st);
        }
        let epoch = 1 + tick / 700;
        let p = Params::new(&cfg, tick, 30, 16.0, 35.0, 80.0, 16.0, epoch, 1);
        for i in 0..n {
            let (mut b_new, last) = budget[i];
            b_new = b_new.saturating_add(per * tick.wrapping_sub(last).min(8)).min(per * 4);
            let mut b_old = b_new;
            let (mut c_new, mut c_old) = (Counters::default(), Counters::default());
            let mut order: Vec<usize> = (0..n).filter(|&j| j != i).collect();
            let dist = |j: usize| {
                let (a, b) = (states[i].pos_m(), states[j].pos_m());
                ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
            };
            // (the near ones first, as the server does)
            order.sort_by_key(|&j| dist(j) > p.mid_m);
            for j in order {
                let d = dist(j);
                let a = new_seen[i].visible(&game, &p, &states[i], &states[j], d, &mut b_new, &mut c_new);
                let b = old_seen[i].visible(&game, &p, &states[i], &states[j], d, &mut b_old, &mut c_old);
                calls += 1;
                assert_eq!(a, b, "tick {tick}, observer {i}, target {j}, distance {d}");
                assert_eq!(b_new, b_old, "the rays left: tick {tick}, observer {i}, target {j}");
                assert_eq!((c_new.pairs, c_new.rays, c_new.still, c_new.unaffordable), (c_old.pairs, c_old.rays, c_old.still, c_old.unaffordable), "what was counted: tick {tick}, observer {i}, target {j}");
                if !a {
                    hidden += 1;
                }
            }
            new_seen[i].prune(tick);
            old_seen[i].prune(tick);
            assert_eq!(new_seen[i].len(), old_seen[i].len(), "the entries that are kept: tick {tick}, observer {i}");
            budget[i] = (b_new, tick);
            stills += c_new.still;
            pairs += c_new.pairs;
            afforded += c_new.unaffordable;
        }
    }
    // (the match must have used every way a decision can go, or this proves little)
    assert!(calls > 2_000_000 && hidden > 200_000 && stills > 5_000 && pairs > 100_000 && afforded > 5_000, "{calls} {hidden} {stills} {pairs} {afforded}");
}
