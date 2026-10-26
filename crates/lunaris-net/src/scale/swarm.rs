//! A match to try the netcode on: [`DemoGame`] (players spread over a map, loot on the ground, a safe zone that shrinks, shots resolved on the server from where the shooter saw the
//! others) and [`Bot`], a client that walks about, shoots at whoever is near, and picks things up, with nobody at the keyboard. The tests play a hundred of them against a server in
//! memory over a bad network; `lunaris swarm` plays them against a real server over UDP.

use super::bytes::{R, W};
use super::client::{Event, NetClient};
use super::server::{ev, Ctx, Game, World};
use super::state::{class, EntityState};
use super::transport::{Addr, Transport};

/// The game's commands (client to server) and events (server to client): the first byte of a reliable message.
pub mod cmd {
    /// A shot: origin (3 floats), direction (3 floats), the server tick the shooter was looking at.
    pub const FIRE: u8 = 1;
    /// Pick an item up: its id.
    pub const PICK: u8 = 2;
}

pub mod evt {
    /// Somebody was hit: victim id, damage, health left.
    pub const HIT: u8 = 1;
    /// Somebody died: killer id, victim id.
    pub const KILL: u8 = 2;
    /// The safe zone: centre x, centre z, radius (floats).
    pub const ZONE: u8 = 3;
    /// An item is gone: its id.
    pub const GONE: u8 = 4;
}

struct Rng(u64);

impl Rng {
    fn f(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }
}

/// The rules of the demo match.
pub struct DemoGame {
    rng: Rng,
    /// Half the width of the (square) map, metres.
    pub half: f32,
    pub items: usize,
    zone: [f32; 3],
    pub shots: u64,
    pub hits: u64,
    pub kills: u64,
}

impl DemoGame {
    pub fn new(half: f32, items: usize, seed: u64) -> DemoGame {
        DemoGame { rng: Rng(seed.max(1)), half, items, zone: [0.0, 0.0, half * 1.5], shots: 0, hits: 0, kills: 0 }
    }

    /// Puts the loot on the ground (before the first client).
    pub fn scatter(&mut self, world: &mut World) {
        for i in 0..self.items {
            let p = [(self.rng.f() * 2.0 - 1.0) * self.half, 0.0, (self.rng.f() * 2.0 - 1.0) * self.half];
            let id = world.spawn(class::ITEM, p);
            if let Some(e) = world.get_mut(id) {
                e.state.extra = (i % 7) as u16;
            }
        }
    }
}

impl Game for DemoGame {
    fn on_join(&mut self, _world: &mut World, _slot: usize, _name: &str) -> EntityState {
        let p = [(self.rng.f() * 2.0 - 1.0) * self.half, 0.0, (self.rng.f() * 2.0 - 1.0) * self.half];
        let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
        s.set_pos_m(p);
        s
    }

    fn on_command(&mut self, ctx: &mut Ctx, slot: usize, data: &[u8]) {
        let mut r = R::new(data);
        let Some(kind) = r.u8() else { return };
        let Some(shooter) = ctx.world.entities.iter().find(|(_, e)| e.owner == Some(slot)).map(|(&id, _)| id) else { return };
        match kind {
            cmd::FIRE => {
                let (Some(ox), Some(oy), Some(oz), Some(dx), Some(dy), Some(dz), Some(tick)) = (r.f32(), r.f32(), r.f32(), r.f32(), r.f32(), r.f32(), r.u32()) else { return };
                self.shots += 1;
                // the shooter must be where it says it is (a shot from across the map is not a shot), and may look back at most a second
                let Some(me) = ctx.world.get(shooter).map(|e| e.state.pos_m()) else { return };
                if (me[0] - ox).abs() > 3.0 || (me[2] - oz).abs() > 3.0 || (me[1] - oy).abs() > 4.0 {
                    return;
                }
                let len = (dx * dx + dy * dy + dz * dz).sqrt();
                if !(len > 0.5) {
                    return;
                }
                let dir = [dx / len, dy / len, dz / len];
                let tick = tick.clamp(ctx.tick.saturating_sub(ctx.tick_hz), ctx.tick);
                if let Some((victim, dist)) = ctx.world.hitscan(shooter, [ox, oy, oz], dir, tick, 200.0, None) {
                    self.hits += 1;
                    let (health, owner) = {
                        let e = ctx.world.get_mut(victim).expect("the victim is there");
                        e.state.health = e.state.health.saturating_sub(20);
                        (e.state.health, e.owner)
                    };
                    let mut w = W::new();
                    w.u8(evt::HIT);
                    w.varu(victim as u64);
                    w.u8(20);
                    w.u8(health);
                    w.f32(dist);
                    ctx.out.to(slot, w.buf.clone());
                    if let Some(o) = owner {
                        ctx.out.to(o, w.buf);
                    }
                    if health == 0 {
                        self.kills += 1;
                        let mut k = W::new();
                        k.u8(evt::KILL);
                        k.varu(shooter as u64);
                        k.varu(victim as u64);
                        ctx.out.all(k.buf);
                        // (the dead one is respawned somewhere else with its health back: a demo has no spectators)
                        let p = [(self.rng.f() * 2.0 - 1.0) * self.half, 0.0, (self.rng.f() * 2.0 - 1.0) * self.half];
                        if let Some(e) = ctx.world.get_mut(victim) {
                            e.state.health = 100;
                            e.state.set_pos_m(p);
                        }
                        if let Some(o) = owner {
                            let mut c = W::new();
                            c.u8(ev::CORRECT);
                            let pq = ctx.world.get(victim).map(|e| e.state.pos).unwrap_or_default();
                            for a in 0..3 {
                                c.vari(pq[a] as i64);
                            }
                            ctx.out.to(o, c.buf);
                        }
                    }
                }
            }
            cmd::PICK => {
                let Some(id) = r.varu() else { return };
                let id = id as u32;
                let Some(me) = ctx.world.get(shooter).map(|e| e.state.pos_m()) else { return };
                let Some(it) = ctx.world.get(id).filter(|e| e.state.class == class::ITEM).map(|e| e.state.pos_m()) else { return };
                if (me[0] - it[0]).abs() < 3.0 && (me[2] - it[2]).abs() < 3.0 {
                    ctx.world.remove(id);
                    let mut w = W::new();
                    w.u8(evt::GONE);
                    w.varu(id as u64);
                    ctx.out.near(it, 200.0, w.buf);
                }
            }
            _ => {}
        }
    }

    fn on_tick(&mut self, ctx: &mut Ctx) {
        // the zone shrinks to a fifth of the map over three minutes, and hurts what is outside it once a second
        let t = (ctx.tick as f32 / ctx.tick_hz as f32 / 180.0).min(1.0);
        let r = self.half * 1.5 * (1.0 - 0.8 * t);
        if (r - self.zone[2]).abs() > 1.0 {
            self.zone[2] = r;
            let mut w = W::new();
            w.u8(evt::ZONE);
            w.f32(self.zone[0]);
            w.f32(self.zone[1]);
            w.f32(r);
            ctx.out.all(w.buf);
        }
        if ctx.tick % ctx.tick_hz == 0 {
            let (cx, cz, rr) = (self.zone[0], self.zone[1], self.zone[2]);
            for e in ctx.world.entities.values_mut() {
                if e.owner.is_some() {
                    let p = e.state.pos_m();
                    if ((p[0] - cx).powi(2) + (p[2] - cz).powi(2)).sqrt() > rr {
                        e.state.health = e.state.health.saturating_sub(2).max(1);
                    }
                }
            }
        }
    }
}

/// A client that plays by itself.
pub struct Bot<T: Transport> {
    pub client: NetClient<T>,
    rng: Rng,
    pub pos: [f32; 3],
    heading: f32,
    speed: f32,
    half: f32,
    next_step: u64,
    next_turn: u64,
    next_fire: u64,
    last_step: u64,
    pub fired: u64,
    pub hits_taken: u64,
    pub hits_given: u64,
    pub kills_seen: u64,
    pub corrections: u64,
    pub zone: Option<[f32; 3]>,
    pub stalest_ms: f64,
    started: bool,
    fire_enabled: bool,
}

impl<T: Transport> Bot<T> {
    pub fn new(transport: T, server: Addr, name: &str, seed: u64, half: f32, fire: bool) -> Bot<T> {
        Bot::with_client(NetClient::new(transport, server, name, (seed as u32).wrapping_mul(2654435761) | 1), seed, half, fire)
    }

    /// A bot that joins a server that wants connect tokens (docs/SECURE.md) with this token.
    pub fn with_token(transport: T, server: Addr, token: super::secure::ConnectToken, seed: u64, half: f32, fire: bool) -> Bot<T> {
        Bot::with_client(NetClient::with_token(transport, server, token, (seed as u32).wrapping_mul(2654435761) | 1), seed, half, fire)
    }

    fn with_client(client: NetClient<T>, seed: u64, half: f32, fire: bool) -> Bot<T> {
        let mut rng = Rng(seed.max(1) * 7919 + 13);
        let heading = rng.f() * std::f32::consts::TAU;
        Bot {
            client,
            rng,
            pos: [0.0; 3],
            heading,
            speed: 0.0,
            half,
            next_step: 0,
            next_turn: 0,
            next_fire: 0,
            last_step: 0,
            fired: 0,
            hits_taken: 0,
            hits_given: 0,
            kills_seen: 0,
            corrections: 0,
            zone: None,
            stalest_ms: 0.0,
            started: false,
            fire_enabled: fire,
        }
    }

    /// Starts the staleness figure again (after the warm-up).
    pub fn reset_staleness(&mut self) {
        self.stalest_ms = 0.0;
        self.client.near_gap_ms = 0;
        self.client.near_gaps = [0; 32];
    }

    /// Whether the bot shoots.
    pub fn set_fire(&mut self, on: bool) {
        self.fire_enabled = on;
    }

    /// One step of the bot at time `now` (call it every few milliseconds).
    pub fn step(&mut self, now: u64) {
        self.client.update(now);
        while let Some(e) = self.client.poll_event() {
            match e {
                Event::Connected { pos, .. } => {
                    self.pos = pos;
                    self.started = true;
                    self.last_step = now;
                }
                Event::Correct { pos } => {
                    self.pos = pos;
                    self.corrections += 1;
                }
                Event::Game(d) => {
                    let mut r = R::new(&d);
                    match r.u8() {
                        Some(evt::HIT) => {
                            let victim = r.varu().unwrap_or(0) as u32;
                            if victim == self.client.entity {
                                self.hits_taken += 1;
                            } else {
                                self.hits_given += 1;
                            }
                        }
                        Some(evt::KILL) => self.kills_seen += 1,
                        Some(evt::ZONE) => {
                            if let (Some(x), Some(z), Some(rad)) = (r.f32(), r.f32(), r.f32()) {
                                self.zone = Some([x, z, rad]);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !self.started || !self.client.connected() || now < self.next_step {
            return;
        }
        let dt = (now - self.last_step) as f32 / 1000.0;
        self.last_step = now;
        self.next_step = now + 33;
        if now >= self.next_turn {
            self.heading += (self.rng.f() - 0.5) * 2.4;
            self.speed = [0.0, 2.0, 5.0, 8.0][(self.rng.f() * 3.99) as usize];
            self.next_turn = now + 800 + (self.rng.f() * 2500.0) as u64;
        }
        let (s, c) = self.heading.sin_cos();
        self.pos[0] += s * self.speed * dt;
        self.pos[2] += c * self.speed * dt;
        // (keep inside the map: turn round at the edge)
        if self.pos[0].abs() > self.half || self.pos[2].abs() > self.half {
            self.pos[0] = self.pos[0].clamp(-self.half, self.half);
            self.pos[2] = self.pos[2].clamp(-self.half, self.half);
            self.heading += std::f32::consts::PI;
        }
        let mut st = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
        st.set_pos_m(self.pos);
        st.set_yaw_rad(self.heading);
        st.set_vel_ms([s * self.speed, 0.0, c * self.speed]);
        st.flags = if self.speed > 6.0 { 1 } else { 0 };
        st.anim = if self.speed > 6.0 { 3 } else if self.speed > 0.5 { 2 } else { 1 };
        st.set_anim_time((now % 2000) as f32 / 1000.0);
        self.client.set_state(st);
        // shoot at the nearest player in sight, now and then
        if self.fire_enabled && now >= self.next_fire {
            self.next_fire = now + 500 + (self.rng.f() * 1500.0) as u64;
            let me = self.client.entity;
            let mut best: Option<(f32, [f32; 3])> = None;
            let ids: Vec<u32> = self.client.remotes().keys().copied().collect();
            for id in ids {
                if id == me {
                    continue;
                }
                if let Some(s) = self.client.sample(id, now) {
                    if s.class != class::PLAYER {
                        continue;
                    }
                    let p = s.pos_m();
                    let d = ((p[0] - self.pos[0]).powi(2) + (p[2] - self.pos[2]).powi(2)).sqrt();
                    if d < 80.0 && best.is_none_or(|b| d < b.0) {
                        best = Some((d, p));
                    }
                }
            }
            if let (Some((_, t)), Some(ms)) = (best, self.client.server_ms(now)) {
                let o = [self.pos[0], self.pos[1] + 1.5, self.pos[2]];
                let d = [t[0] - o[0], t[1] + 1.0 - o[1], t[2] - o[2]];
                // the tick the shooter was looking at: its picture is `interp_ms` old
                let tick = ((ms - self.client.interp_ms) * self.client.tick_hz as f64 / 1000.0).max(0.0) as u32;
                let mut w = W::new();
                w.u8(cmd::FIRE);
                for v in o.iter().chain(d.iter()) {
                    w.f32(*v);
                }
                w.u32(tick);
                self.client.command(w.buf);
                self.fired += 1;
            }
        }
        // a staleness figure: the longest a player within 30 m went between two records arriving
        self.stalest_ms = self.client.near_gap_ms as f64;
    }
}
