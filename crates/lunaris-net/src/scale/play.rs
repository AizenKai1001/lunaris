//! Going online with a game (docs/NETWORK_SCALE.md, "In a game"): [`NetDriver`] connects a running simulation to a big-match server. The entity with a `NetPlayer` (the local player) is what the server is
//! told about, 30 times a second; every other player the server shows becomes an entity made from the game's own prefab (or a bare one) with a `NetPuppet`, moved smoothly between the server's records and
//! removed when the player goes. The game's own rules run on the player's computer as always: this layer only says where everybody is.
//!
//! `lunaris play SCENE --connect HOST:PORT [--name NAME]` is this driver inside the player; an embedding program calls [`NetDriver::tick`] before each simulation tick, or [`NetDriver::tick_at`] with its
//! own clock (a test). It makes and shows nothing itself: the puppets' bodies, animations and names are the game's.
use super::client::{Event, NetClient};
use super::netinv;
use super::netsync;
use super::party::{self, Panel, Row};
use super::simgame;
use super::state::{class, EntityState};
use super::transport::Transport;
use lunaris_json::Json;
use lunaris_sim::components::{Door, NetChat, NetContainer, NetEntity, NetParty, NetPlayer, NetPuppet, Parent, Text, Transform, Ui, UiAnchor, UiInput, Velocity};
use lunaris_sim::inventory::Inventory;
use lunaris_sim::Sim;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// The yaw (radians, a turn to the left is positive) of a rotation: the way its local -z points on the ground.
pub fn yaw_of(q: [f32; 4]) -> f32 {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    (2.0 * (x * z + w * y)).atan2(1.0 - 2.0 * (x * x + y * y))
}

/// A rotation of `yaw` radians about the vertical axis.
pub fn yaw_rot(yaw: f32) -> [f32; 4] {
    let h = yaw * 0.5;
    [0.0, h.sin(), 0.0, h.cos()]
}

/// How the way into a game stands (`NetDriver::link`; a menu shows it through `NetConnect.status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// Asking the server in (every quarter of a second), not answered yet.
    Connecting,
    Connected,
    /// The server said no: 1 another protocol version, 2 full.
    Refused(u8),
    /// The server went quiet after letting us in.
    Lost,
    /// Nothing answered within `timeout_ms`.
    TimedOut,
}

impl LinkState {
    /// What to tell the player.
    pub fn message(&self) -> &'static str {
        match self {
            LinkState::Connecting => "Connecting...",
            LinkState::Connected => "Connected",
            LinkState::Refused(1) => "This server runs another version of the game",
            LinkState::Refused(2) => "The server is full",
            LinkState::Refused(3) => "That name is already in the game on this server",
            LinkState::Refused(4) => "You are banned from this server",
            LinkState::Refused(5) => "You were removed from the server",
            LinkState::Refused(_) => "The server refused the connection",
            LinkState::Lost => "Connection lost",
            LinkState::TimedOut => "No answer from the server",
        }
    }
}

pub struct NetDriver<T: Transport> {
    pub client: NetClient<T>,
    /// Where the way in stands, and how long (milliseconds on this driver's clock) the server has to answer before it is given up.
    pub link: LinkState,
    pub timeout_ms: u64,
    start: Instant,
    /// The puppets: the server's number of the player and the uid of the entity that shows it.
    puppets: BTreeMap<u32, u64>,
    /// The parts of each puppet: the entities of its prefab (children, and theirs) that have a `NetPuppet` of their own, which is given the root's every tick (a held gun, a name tag).
    parts: BTreeMap<u32, Vec<u64>>,
    /// Where the local player was at the last tick and when, to work out its speed when the game keeps none.
    last: Option<([f32; 3], u64)>,
    /// The messages the server's game sent (the game's own: a kill feed line, a hit): handed over for the game to use, newest last.
    pub messages: Vec<Vec<u8>>,
    /// The axes of the player's hands as the server last heard them, and when.
    last_axes: [f32; 4],
    last_axes_ms: u64,
    /// The camera has been put at a machine's puppet and has to go home when it is given back.
    view_moved: bool,
    /// The chat messages the server told this player (`NetHost.chat`): when each came (this driver's clock, milliseconds) and the line `name: text`; the newest last, at most 32. How many have come in all.
    chat: Vec<(u64, String)>,
    chat_heard: u32,
    /// What the server last said of this player's party and invitation (docs/PARTIES.md), and when it came (this driver's clock); how many panels have come in.
    party: Option<(Panel, u64)>,
    party_heard: u32,
    /// The markers over the party mates (`NetParty.marker`): the server's number of the mate and the uid of its marker.
    markers: BTreeMap<u32, u64>,
}

impl<T: Transport> NetDriver<T> {
    pub fn new(client: NetClient<T>) -> NetDriver<T> {
        NetDriver { client, link: LinkState::Connecting, timeout_ms: 8000, start: Instant::now(), puppets: BTreeMap::new(), parts: BTreeMap::new(), last: None, messages: Vec::new(), last_axes: [0.0; 4], last_axes_ms: 0, view_moved: false, chat: Vec::new(), chat_heard: 0, party: None, party_heard: 0, markers: BTreeMap::new() }
    }

    /// Leave the game: the server is told, the other players' puppets are taken away and the driver is spent (a new one makes a new connection).
    pub fn leave(&mut self, sim: &mut Sim) {
        self.client.disconnect();
        self.remove_puppets(sim, |_| true);
    }

    /// Before a simulation tick, by the wall clock.
    pub fn tick(&mut self, sim: &mut Sim) {
        let now = self.start.elapsed().as_millis() as u64 + 1;
        self.tick_at(sim, now);
    }

    /// Before a simulation tick, at time `now` (milliseconds on this driver's own clock).
    pub fn tick_at(&mut self, sim: &mut Sim, now: u64) {
        self.client.update(now);
        let local = {
            let mut best: Option<u64> = None;
            let _ = sim.world.for_each_uid::<(&NetPlayer,), _>(|_, uid, _| {
                if best.is_none_or(|b| uid < b) {
                    best = Some(uid);
                }
            });
            best
        };
        while let Some(ev) = self.client.poll_event() {
            match ev {
                Event::Connected { entity, pos, .. } => {
                    self.link = LinkState::Connected;
                    // (what a game server replicates comes from the server: the copies the scene has are not wanted)
                    let mut found: Vec<(lunaris_ecs::Entity, u64)> = Vec::new();
                    let _ = sim.world.for_each_uid::<(&NetEntity,), _>(|e, uid, _| found.push((e, uid)));
                    let own: Vec<u64> = found.into_iter().filter(|(e, _)| sim.world.get::<NetPuppet>(*e).is_none()).map(|(_, u)| u).collect();
                    if !own.is_empty() {
                        lunaris_sim::gameplay::despawn_trees(&mut sim.world, own);
                    }
                    if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
                        if let Some(t) = sim.world.get_mut::<Transform>(e) {
                            t.pos = pos;
                        }
                        if let Some(v) = sim.world.get_mut::<Velocity>(e) {
                            v.lin = [0.0; 3];
                        }
                        if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                            p.id = entity;
                        }
                    }
                }
                Event::Correct { pos } => {
                    if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
                        if let Some(t) = sim.world.get_mut::<Transform>(e) {
                            t.pos = pos;
                        }
                        if let Some(v) = sim.world.get_mut::<Velocity>(e) {
                            v.lin = [0.0; 3];
                        }
                        if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                            p.corrections = p.corrections.wrapping_add(1);
                        }
                    }
                }
                Event::Game(d) => match d.first() {
                    // what is in the player's bag, and in the container it has open: into the inventories the interface is bound to
                    Some(&netinv::msg::INV) => {
                        if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
                            if let Some(inv) = sim.world.get_mut::<Inventory>(e) {
                                let _ = netinv::apply_inv(&d, inv);
                            }
                        }
                    }
                    // the player has taken over a machine of the server's with its goggles, or given it back
                    Some(&simgame::msg::VIEW) => {
                        if let (Some(id), Some(e)) = (simgame::parse_view(&d), local.and_then(|u| sim.world.entity(u))) {
                            if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                                p.viewing = id;
                            }
                        }
                    }
                    // somebody near said something (`NetHost.chat`): a line of the chat log
                    Some(&simgame::msg::CHAT) => {
                        if let Some((name, text)) = simgame::parse_chat(&d) {
                            self.chat.push((now, format!("{name}: {text}")));
                            if self.chat.len() > 32 {
                                self.chat.remove(0);
                            }
                            self.chat_heard = self.chat_heard.wrapping_add(1);
                        }
                    }
                    // the player's party and its invitation (docs/PARTIES.md): the panel
                    Some(&party::msg::PANEL) => {
                        if let Some(panel) = Panel::parse(&d) {
                            self.party = Some((panel, now));
                            self.party_heard = self.party_heard.wrapping_add(1);
                        }
                    }
                    // what the player's interface shows of its own character: the vitals, the gun in the hand
                    Some(&simgame::msg::STATUS) => {
                        if let (Some(s), Some(e)) = (simgame::Status::parse(&d), local.and_then(|u| sim.world.entity(u))) {
                            if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                                let f = |b: u8| b as f32 / 255.0;
                                (p.health, p.food, p.water, p.stamina) = (f(s.health), f(s.food), f(s.water), f(s.stamina));
                                p.heat = s.heat as f32 / 25.5;
                                p.tier = ((s.flags >> simgame::Status::TIER_SHIFT) & 7) as u32;
                                p.bleeding = s.flags & simgame::Status::BLEEDING != 0;
                                p.exhausted = s.flags & simgame::Status::EXHAUSTED != 0;
                                p.reloading = s.flags & simgame::Status::RELOADING != 0;
                                (p.gun, p.mag, p.reserve) = (s.gun as u32, s.mag as u32, s.reserve as u32);
                            }
                        }
                    }
                    // resources of the server's world the scene lists in `NetHost.sync`: the world clock, the weather
                    Some(&netsync::msg::SYNC) => {
                        if let Some(entries) = netsync::parse(&d) {
                            netsync::apply(&mut sim.world, &entries);
                        }
                    }
                    // the player sits in a vehicle of the server's, or has got out of it
                    Some(&simgame::msg::RIDE) => {
                        if let (Some((id, seat)), Some(e)) = (simgame::parse_ride(&d), local.and_then(|u| sim.world.entity(u))) {
                            if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                                p.riding = id;
                                p.seat = seat;
                            }
                        }
                    }
                    Some(&netinv::msg::CONT) | Some(&netinv::msg::CONT_END) => {
                        let mut found: Vec<lunaris_ecs::Entity> = Vec::new();
                        let _ = sim.world.for_each_uid::<(&NetContainer,), _>(|e, _, _| found.push(e));
                        if let Some(&ce) = found.first() {
                            let opened = d[0] == netinv::msg::CONT;
                            let mut inv = sim.world.get::<Inventory>(ce).copied().unwrap_or_default();
                            let source = if opened { netinv::apply_cont(&d, &mut inv).unwrap_or(0) } else { 0 };
                            if !opened {
                                inv.id = [0; 48];
                                inv.count = [0; 48];
                                inv.dur = [0.0; 48];
                            }
                            if let Some(w) = sim.world.get_mut::<Inventory>(ce) {
                                *w = inv;
                            }
                            if let Some(c) = sim.world.get_mut::<NetContainer>(ce) {
                                c.open = opened;
                                c.source = source;
                            }
                        }
                    }
                    _ => {
                        // an event of the server's simulation: it happens in this one too, for the sounds and the effects of the game
                        if let Some((name, pos, about)) = simgame::parse_event_about(&d) {
                            // (the event's uid is the server's number of the entity it is about, `NetPlayer.id` for the player's own character: 0 when it is about no replicated thing)
                            sim.scripts.events.push((name, about as u64, Some(pos)));
                        } else if self.messages.len() < 256 {
                            self.messages.push(d);
                        }
                    }
                },
                Event::Disconnected => {
                    self.link = if self.link == LinkState::Connected { LinkState::Lost } else { LinkState::TimedOut };
                    // (a party panel that was told by a server that has gone is not true any more)
                    self.party = None;
                    self.remove_puppets(sim, |_| true)
                }
                Event::Refused(why) => {
                    self.link = LinkState::Refused(why);
                    self.party = None;
                    self.remove_puppets(sim, |_| true)
                }
            }
        }
        if self.link == LinkState::Connecting && now > self.timeout_ms {
            self.link = LinkState::TimedOut;
            self.client.disconnect();
        }
        let connected = self.client.connected();
        let mut prefab = String::new();
        let mut kinds: Vec<(u32, Option<u32>, String)> = Vec::new();
        if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
            if let Some(np) = sim.world.get::<NetPlayer>(e).copied() {
                prefab = sim.world.strings().get(np.puppet.0).to_string();
                kinds = parse_kinds(sim.world.strings().get(np.kinds.0));
                if connected && (np.cmd.0 != 0 || np.act != 0) {
                    // a command for the server's game, said once: by its name, or by the number of a line of `acts` (a graph system can set a number)
                    let mut name = sim.world.strings().get(np.cmd.0).to_string();
                    if name.is_empty() {
                        let acts = sim.world.strings().get(np.acts.0).to_string();
                        name = acts
                            .lines()
                            .filter_map(|l| {
                                let l = l.split('#').next().unwrap_or("").trim();
                                let (n, c) = l.split_once(char::is_whitespace)?;
                                (n.parse::<u32>().ok()? == np.act).then(|| c.trim().to_string())
                            })
                            .next()
                            .unwrap_or_default();
                    }
                    if !name.is_empty() {
                        self.client.command(simgame::action(&name, np.cmd_args));
                    }
                    if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                        p.cmd = lunaris_ecs::Sym(0);
                        p.act = 0;
                        p.cmd_args = [0.0; 4];
                    }
                }
                if connected && np.say_input.0 != 0 {
                    // the chat field: what was typed when the player pressed Enter is said to the server (it cleans it, limits it and tells the players near), and the field is emptied
                    if let Some(ie) = sim.world.entity(np.say_input.0) {
                        if let Some(inp) = sim.world.get::<UiInput>(ie).copied().filter(|i| i.submitted) {
                            let text = sim.world.strings().get(inp.text.0).to_string();
                            if !text.trim().is_empty() {
                                self.client.command(simgame::say(&text));
                                let empty = sim.world.strings_mut().intern("");
                                if let Some(w) = sim.world.get_mut::<UiInput>(ie) {
                                    w.text.0 = empty;
                                }
                            }
                        }
                    }
                }
                if connected && np.say_party_input.0 != 0 {
                    // the party's chat field: the same, said to the player's party only
                    if let Some(ie) = sim.world.entity(np.say_party_input.0) {
                        if let Some(inp) = sim.world.get::<UiInput>(ie).copied().filter(|i| i.submitted) {
                            let text = sim.world.strings().get(inp.text.0).to_string();
                            if !text.trim().is_empty() {
                                self.client.command(party::say(&text));
                                let empty = sim.world.strings_mut().intern("");
                                if let Some(w) = sim.world.get_mut::<UiInput>(ie) {
                                    w.text.0 = empty;
                                }
                            }
                        }
                    }
                }
                if connected {
                    // what the player's hands say (in a vehicle): when they change, at most twenty times a second, and again every 200 ms while they are away from rest, in case a message was lost
                    let changed = (0..4).any(|i| (np.axes[i] - self.last_axes[i]).abs() > 0.02);
                    let live = np.axes.iter().any(|a| a.abs() > 0.02);
                    let since = now.saturating_sub(self.last_axes_ms);
                    if (changed && since >= 50) || (live && since >= 200) {
                        self.client.command(simgame::control(np.axes));
                        self.last_axes = np.axes;
                        self.last_axes_ms = now;
                    }
                    // what the server is told about this player
                    let t = sim.world.get::<Transform>(e).copied().unwrap_or_default();
                    // (a game that keeps a Velocity says it; one that does not, a character controller, is measured from where it was)
                    let mut vel = sim.world.get::<Velocity>(e).map_or([0.0; 3], |v| v.lin);
                    if vel == [0.0; 3] {
                        if let Some((was, at)) = self.last {
                            let dt = now.saturating_sub(at) as f32 / 1000.0;
                            if dt > 0.0 {
                                vel = [(t.pos[0] - was[0]) / dt, (t.pos[1] - was[1]) / dt, (t.pos[2] - was[2]) / dt];
                            }
                        }
                    }
                    self.last = Some((t.pos, now));
                    let mut st = EntityState { class: class::PLAYER, ..Default::default() };
                    st.set_pos_m(t.pos);
                    st.set_yaw_rad(yaw_of(t.rot));
                    st.set_pitch_rad(np.pitch);
                    st.set_vel_ms(vel);
                    st.flags = np.flags.min(0xffff) as u16;
                    st.anim = np.anim.min(0xffff) as u16;
                    st.set_anim_time(np.anim_t.max(0.0));
                    st.extra = np.extra.min(0xffff) as u16;
                    self.client.set_state(st);
                }
            }
        }
        // the other players
        let me = self.client.entity;
        let mut seen: BTreeSet<u32> = BTreeSet::new();
        if connected {
            let ids: Vec<u32> = self.client.remotes().keys().copied().collect();
            for id in ids {
                if id == me {
                    continue;
                }
                let Some(s) = self.client.sample(id, now) else { continue };
                // (a player is made from the player prefab; anything else the server replicates from the prefab the `kinds` text names for it, and is not shown without one)
                let which: &str = if s.class == class::PLAYER {
                    &prefab
                } else {
                    match kinds.iter().find(|(c, k, _)| *c == s.class as u32 && k.is_none_or(|k| k == s.extra as u32)) {
                        Some((_, _, p)) => p.as_str(),
                        None => continue,
                    }
                };
                seen.insert(id);
                let mut made = self.puppets.get(&id).copied().filter(|u| sim.world.entity(*u).is_some());
                // (a thing that has another picture than the one it was made with, a pickup whose item was changed, is made again)
                if let Some(u) = made {
                    let changed = s.class != class::PLAYER && sim.world.entity(u).and_then(|e| sim.world.get::<NetPuppet>(e)).is_some_and(|p| p.id == id && p.kind != s.extra as u32);
                    if changed {
                        lunaris_sim::gameplay::despawn_trees(&mut sim.world, vec![u]);
                        self.puppets.remove(&id);
                        self.parts.remove(&id);
                        made = None;
                    }
                }
                let uid = match made {
                    Some(u) => u,
                    None => match spawn_puppet(sim, which, id, &s) {
                        Some((u, parts)) => {
                            self.puppets.insert(id, u);
                            if parts.is_empty() {
                                self.parts.remove(&id);
                            } else {
                                self.parts.insert(id, parts);
                            }
                            u
                        }
                        None => continue,
                    },
                };
                if let Some(e) = sim.world.entity(uid) {
                    let pos = s.pos_m();
                    let vel = s.vel_ms();
                    if let Some(t) = sim.world.get_mut::<Transform>(e) {
                        t.pos = pos;
                        t.rot = yaw_rot(s.yaw_rad());
                    }
                    if let Some(v) = sim.world.get_mut::<Velocity>(e) {
                        v.lin = vel;
                    }
                    // (a door that the server says is open swings open here: the puppet's own `Door` turns it)
                    if let Some(d) = sim.world.get_mut::<Door>(e) {
                        d.open = s.flags & 1 != 0;
                    }
                    if let Some(p) = sim.world.get_mut::<NetPuppet>(e) {
                        p.id = id;
                        p.class = s.class as u32;
                        p.kind = if s.class == class::PLAYER { 0 } else { s.extra as u32 };
                        p.flags = s.flags as u32;
                        p.anim = s.anim as u32;
                        p.anim_t = s.anim_time();
                        p.pitch = s.pitch_rad();
                        p.health = s.health as f32;
                        p.extra = s.extra as u32;
                        p.speed = (vel[0] * vel[0] + vel[2] * vel[2]).sqrt();
                    }
                    // (the parts of the puppet know what the puppet knows: a part is dressed by its own `NetPuppet`, a graph system cannot read its parent's)
                    if let Some(parts) = self.parts.get(&id) {
                        if let Some(root) = sim.world.get::<NetPuppet>(e).copied() {
                            for pu in parts {
                                if let Some(pe) = sim.world.entity(*pu) {
                                    if let Some(p) = sim.world.get_mut::<NetPuppet>(pe) {
                                        *p = root;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        self.remove_puppets(sim, |id| !seen.contains(&id));
        // a player in a vehicle of the server's is carried with the vehicle's puppet (the walking of the game is overridden; the camera, a child of the player, follows)
        if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
            if let Some(np) = sim.world.get::<NetPlayer>(e).copied().filter(|n| n.riding != 0) {
                let puppet = self.puppets.get(&np.riding).and_then(|u| sim.world.entity(*u)).and_then(|pe| sim.world.get::<Transform>(pe).copied());
                if let Some(pt) = puppet {
                    let (s, c) = yaw_of(pt.rot).sin_cos();
                    let o = np.seat;
                    if let Some(t) = sim.world.get_mut::<Transform>(e) {
                        t.pos = [pt.pos[0] + o[0] * c + o[2] * s, pt.pos[1] + o[1], pt.pos[2] - o[0] * s + o[2] * c];
                    }
                    if let Some(v) = sim.world.get_mut::<Velocity>(e) {
                        v.lin = [0.0; 3];
                    }
                }
            }
        }
        // the camera of a player who controls a machine is at the machine's puppet (its direction stays the player's); when it is given back the camera goes home
        if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
            if let Some(np) = sim.world.get::<NetPlayer>(e).copied().filter(|n| n.camera.0 != 0) {
                if let Some(ce) = sim.world.entity(np.camera.0) {
                    let puppet = (np.viewing != 0).then(|| self.puppets.get(&np.viewing).and_then(|u| sim.world.entity(*u)).and_then(|pe| sim.world.get::<Transform>(pe).copied())).flatten();
                    let body = sim.world.get::<Transform>(e).copied().unwrap_or_default();
                    match puppet {
                        Some(pt) => {
                            // (the camera is a child of the player's body, which turns about the vertical: the offset to the machine, in the body's axes)
                            let rel = [pt.pos[0] - body.pos[0], pt.pos[1] + 0.2 - body.pos[1], pt.pos[2] - body.pos[2]];
                            let (s, c) = yaw_of(body.rot).sin_cos();
                            if let Some(l) = sim.world.get_mut::<lunaris_sim::components::LocalTransform>(ce) {
                                l.pos = [rel[0] * c - rel[2] * s, rel[1], rel[0] * s + rel[2] * c];
                            }
                            self.view_moved = true;
                        }
                        None if self.view_moved => {
                            if let Some(l) = sim.world.get_mut::<lunaris_sim::components::LocalTransform>(ce) {
                                l.pos = np.eye;
                            }
                            self.view_moved = false;
                        }
                        None => {}
                    }
                }
            }
        }
        if let Some(e) = local.and_then(|u| sim.world.entity(u)) {
            let count = self.puppets.len() as u32;
            let ping = self.client.rtt_ms() as f32;
            if let Some(p) = sim.world.get_mut::<NetPlayer>(e) {
                p.connected = connected;
                p.ping = ping;
                p.players = count;
                if !connected {
                    p.id = 0;
                }
            }
        }
        self.show_chat(sim, now);
        self.show_party(sim, now);
    }

    /// Write the party panel into the entities that show it (`NetParty`): the lines (docs/PARTIES.md) into their `Text` (or `Ui`) text when that has changed, and the numbers beside them. The seconds an invitation has left are counted on
    /// this driver's clock from when the server said them.
    fn show_party(&mut self, sim: &mut Sim, now: u64) {
        let mut shows: Vec<(lunaris_ecs::Entity, NetParty)> = Vec::new();
        let _ = sim.world.for_each::<(&NetParty,), _>(|e, (p,)| shows.push((e, *p)));
        if shows.is_empty() {
            return;
        }
        let empty = Panel::default();
        let (panel, at) = match &self.party {
            Some((p, at)) => (p, *at),
            None => (&empty, now),
        };
        let secs = panel.invite.as_ref().map_or(0, |i| (i.secs as u64).saturating_sub(now.saturating_sub(at) / 1000) as u32);
        // (the first panel that names a marker prefab says what the markers are made from, and hangs them where it hangs)
        let shows_marker = shows
            .iter()
            .find(|(_, np)| np.marker.0 != 0)
            .map(|(e, np)| (sim.world.strings().get(np.marker.0).to_string(), sim.world.get::<Parent>(*e).map_or(0, |p| p.target.0)))
            .unwrap_or_default();
        for (e, np) in shows {
            let hint = sim.world.strings().get(np.invite_hint.0).to_string();
            let mut lines = panel.lines(secs, &hint);
            lines.truncate(np.lines.clamp(1, 16) as usize);
            let text = lines.join("\n");
            let sym = if let Some(t) = sim.world.get::<Text>(e) { Some(t.value.0) } else { sim.world.get::<Ui>(e).map(|u| u.text.0) };
            if let Some(cur) = sym {
                if sim.world.strings().get(cur) != text {
                    let new = sim.world.strings_mut().intern(&text);
                    if let Some(t) = sim.world.get_mut::<Text>(e) {
                        t.value.0 = new;
                    } else if let Some(u) = sim.world.get_mut::<Ui>(e) {
                        u.text.0 = new;
                    }
                }
            }
            if let Some(c) = sim.world.get_mut::<NetParty>(e) {
                c.count = panel.members.len() as u32;
                c.party = panel.party;
                c.leader = panel.leader;
                c.invited = panel.invite.is_some();
                c.invite_secs = secs as f32;
                c.heard = self.party_heard;
            }
        }
        self.mark_mates(sim, &shows_marker);
    }

    /// The markers over the party mates: one for each member of the panel (not the player itself, not one that is away) whose puppet this client shows, made from the prefab `NetParty.marker` names, pinned to the puppet, its text the
    /// mate's name; the ones whose mate is gone are taken away. `(prefab, parent)` is what the panel's entity says (empty: no markers).
    fn mark_mates(&mut self, sim: &mut Sim, rule: &(String, u64)) {
        let mut want: Vec<(u32, u64, String)> = Vec::new();
        if !rule.0.is_empty() {
            if let Some((panel, _)) = &self.party {
                for m in &panel.members {
                    if m.flags & (Row::ME | Row::AWAY) != 0 || m.id == 0 {
                        continue;
                    }
                    if let Some(&pu) = self.puppets.get(&m.id) {
                        want.push((m.id, pu, m.name.clone()));
                    }
                }
            }
        }
        let gone: Vec<u32> = self.markers.keys().filter(|id| !want.iter().any(|w| w.0 == **id)).copied().collect();
        if !gone.is_empty() {
            let doomed: Vec<u64> = gone.iter().filter_map(|id| self.markers.remove(id)).collect();
            lunaris_sim::gameplay::despawn_trees(&mut sim.world, doomed);
        }
        for (id, pu, name) in want {
            let mut made = self.markers.get(&id).copied().filter(|u| sim.world.entity(*u).is_some());
            if made.is_none() {
                let mut ov: Vec<(String, Json)> = Vec::new();
                if rule.1 != 0 {
                    lunaris_sim::prefab::merge_component(&mut ov, "Parent", &Json::obj([("target", Json::from(rule.1 as i64))]));
                }
                let base = sim.world.next_uid();
                if let Ok(plan) = lunaris_sim::prefab::plan(&sim.scripts.prefabs, &rule.0, &ov) {
                    let cmds = lunaris_sim::prefab::spawn_commands(&plan, base);
                    if lunaris_sim::apply_batch(&mut sim.world, &mut sim.scripts, &cmds).is_ok() {
                        self.markers.insert(id, base);
                        made = Some(base);
                    }
                }
            }
            let Some(me) = made.and_then(|u| sim.world.entity(u)) else { continue };
            if let Some(a) = sim.world.get_mut::<UiAnchor>(me) {
                if a.entity.0 != pu {
                    a.entity.0 = pu;
                }
            }
            let cur = sim.world.get::<Text>(me).map(|t| t.value.0);
            if cur.is_some_and(|c| sim.world.strings().get(c) != name) {
                let new = sim.world.strings_mut().intern(&name);
                if let Some(t) = sim.world.get_mut::<Text>(me) {
                    t.value.0 = new;
                }
            }
        }
    }

    /// Write the chat log into the entities that show it (`NetChat`): the last `lines` messages that are not older than `life`, one to a line, into their `Text` (or `Ui`) text, when that has changed.
    fn show_chat(&mut self, sim: &mut Sim, now: u64) {
        let mut logs: Vec<(lunaris_ecs::Entity, NetChat)> = Vec::new();
        let _ = sim.world.for_each::<(&NetChat,), _>(|e, (c,)| logs.push((e, *c)));
        for (e, nc) in logs {
            let life = (nc.life.max(0.5) * 1000.0) as u64;
            let lines = nc.lines.clamp(1, 16) as usize;
            let alive: Vec<&str> = self.chat.iter().filter(|(t, _)| now.saturating_sub(*t) < life).map(|(_, l)| l.as_str()).collect();
            let tail = &alive[alive.len().saturating_sub(lines)..];
            let text = if nc.bottom && !tail.is_empty() { format!("{}{}", "\n".repeat(lines - tail.len()), tail.join("\n")) } else { tail.join("\n") };
            let sym = if let Some(t) = sim.world.get::<Text>(e) {
                Some(t.value.0)
            } else {
                sim.world.get::<Ui>(e).map(|u| u.text.0)
            };
            if let Some(cur) = sym {
                if sim.world.strings().get(cur) != text {
                    let new = sim.world.strings_mut().intern(&text);
                    if let Some(t) = sim.world.get_mut::<Text>(e) {
                        t.value.0 = new;
                    } else if let Some(u) = sim.world.get_mut::<Ui>(e) {
                        u.text.0 = new;
                    }
                }
            }
            if nc.count != self.chat_heard {
                if let Some(c) = sim.world.get_mut::<NetChat>(e) {
                    c.count = self.chat_heard;
                }
            }
        }
    }

    /// Take away the puppets of the players `gone` says are gone.
    fn remove_puppets(&mut self, sim: &mut Sim, gone: impl Fn(u32) -> bool) {
        let doomed: Vec<(u32, u64)> = self.puppets.iter().filter(|(id, _)| gone(**id)).map(|(id, u)| (*id, *u)).collect();
        if doomed.is_empty() {
            return;
        }
        for (id, _) in &doomed {
            self.puppets.remove(id);
            self.parts.remove(id);
        }
        lunaris_sim::gameplay::despawn_trees(&mut sim.world, doomed.into_iter().map(|d| d.1).collect());
    }
}

/// The lines of `NetPlayer.kinds`: `CLASS KIND PREFAB`, `KIND` `*` for any (`#` starts a comment).
fn parse_kinds(text: &str) -> Vec<(u32, Option<u32>, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut w = line.split('#').next().unwrap_or("").split_whitespace();
        let (Some(c), Some(k), Some(p)) = (w.next(), w.next(), w.next()) else { continue };
        let Ok(c) = c.parse::<u32>() else { continue };
        // (a kind that is neither a number nor `*` is a mistake: the line is left out)
        let kind = if k == "*" {
            None
        } else {
            match k.parse::<u32>() {
                Ok(n) => Some(n),
                Err(_) => continue,
            }
        };
        out.push((c, kind, p.to_string()));
    }
    out
}

/// A new entity for the player `id`: the game's prefab if it has one, else a bare entity with a `Transform` and a `NetPuppet`. Returns its uid and the uids of the parts of it that have a `NetPuppet` of their own.
fn spawn_puppet(sim: &mut Sim, prefab: &str, id: u32, s: &EntityState) -> Option<(u64, Vec<u64>)> {
    let num = |v: f32| Json::Float(v as f64);
    let pos = s.pos_m();
    let base = sim.world.next_uid();
    if !prefab.is_empty() {
        let mut ov: Vec<(String, Json)> = Vec::new();
        lunaris_sim::prefab::merge_component(&mut ov, "Transform", &Json::obj([("pos", Json::arr(pos.map(num))), ("rot", Json::arr(yaw_rot(s.yaw_rad()).map(num)))]));
        lunaris_sim::prefab::merge_component(&mut ov, "NetPuppet", &Json::obj([("id", Json::from(id as i64))]));
        if let Ok(plan) = lunaris_sim::prefab::plan(&sim.scripts.prefabs, prefab, &ov) {
            let cmds = lunaris_sim::prefab::spawn_commands(&plan, base);
            if lunaris_sim::apply_batch(&mut sim.world, &mut sim.scripts, &cmds).is_ok() {
                let parts = plan.iter().enumerate().skip(1).filter(|(_, p)| p.components.iter().any(|(n, _)| n == "NetPuppet")).map(|(i, _)| base + i as u64).collect();
                return Some((base, parts));
            }
        }
    }
    let e = sim.world.spawn_with2(Transform { pos, rot: yaw_rot(s.yaw_rad()), ..Default::default() }, NetPuppet { id, ..Default::default() }).ok()?;
    let _ = e;
    Some((base, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::server::{Game, NetServer, ServerConfig, World as ServerWorld};
    use crate::scale::transport::{Link, SimEndpoint, SimNet};
    use lunaris_json::parse;
    use lunaris_sim::Command;

    /// A game that puts the players 6 m apart on a line, so that they can see each other.
    struct Line;
    impl Game for Line {
        fn on_join(&mut self, _w: &mut ServerWorld, slot: usize, _name: &str) -> EntityState {
            let mut s = EntityState { class: class::PLAYER, health: 100, ..Default::default() };
            s.set_pos_m([slot as f32 * 6.0, 0.0, 0.0]);
            s
        }
    }

    fn sim(prefab: &str) -> Sim {
        let mut s = Sim::new(1);
        let Json::Obj(components) = parse(&format!(r#"{{"Transform":{{}},"Velocity":{{}},"HitPoints":{{"value":80,"max":100}},"NetPlayer":{{"puppet":"{prefab}"}}}}"#)).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(1), components }).unwrap();
        s.apply(&Command::DefinePrefab { name: "Mate".into(), def: parse(r#"{"components":{"Name":{"value":"Mate"},"Transform":{},"Velocity":{},"NetPuppet":{}}}"#).unwrap() }).unwrap();
        s
    }

    fn puppets(s: &mut Sim) -> Vec<(u64, NetPuppet, Transform)> {
        let mut v = Vec::new();
        let _ = s.world.for_each_uid::<(&NetPuppet, &Transform), _>(|_, uid, (p, t)| v.push((uid, *p, *t)));
        v
    }

    fn local(s: &Sim) -> (NetPlayer, Transform) {
        let e = s.world.entity(1).unwrap();
        (*s.world.get::<NetPlayer>(e).unwrap(), *s.world.get::<Transform>(e).unwrap())
    }

    struct Rig {
        net: SimNet,
        server: NetServer<SimEndpoint, Line>,
        sims: Vec<Sim>,
        drivers: Vec<NetDriver<SimEndpoint>>,
        now: u64,
    }

    fn rig(n: usize, prefab: &str) -> Rig {
        let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.0, duplicate: 0.0 }, 3);
        let end = net.endpoint();
        let addr = end.addr();
        let server = NetServer::new(ServerConfig::default(), end, Line);
        let sims = (0..n).map(|_| sim(prefab)).collect();
        let drivers = (0..n).map(|i| NetDriver::new(NetClient::new(net.endpoint(), addr, &format!("p{i}"), 50 + i as u32))).collect();
        Rig { net, server, sims, drivers, now: 1000 }
    }

    impl Rig {
        fn run(&mut self, ms: u64, mut each: impl FnMut(&mut [Sim], u64)) {
            let end = self.now + ms;
            while self.now < end {
                self.now += 10;
                self.net.set_now(self.now);
                self.server.update(self.now);
                each(&mut self.sims, self.now);
                for (d, s) in self.drivers.iter_mut().zip(self.sims.iter_mut()) {
                    d.tick_at(s, self.now);
                }
            }
        }
    }

    #[test]
    fn two_players_see_each_other_as_puppets_that_move_with_them() {
        let mut r = rig(2, "Mate");
        // player 0 walks along z at 3 m/s once it is in (the server chose where it starts)
        r.run(3000, |sims, _| {
            let e = sims[0].world.entity(1).unwrap();
            let connected = sims[0].world.get::<NetPlayer>(e).unwrap().connected;
            if connected {
                let t = sims[0].world.get_mut::<Transform>(e).unwrap();
                t.pos[2] -= 0.03;
                t.rot = yaw_rot(0.5);
            }
        });
        let (a, ta) = local(&r.sims[0]);
        let (b, _) = local(&r.sims[1]);
        assert!(a.connected && b.connected && a.id != 0 && b.id != 0 && a.id != b.id, "{a:?} {b:?}");
        assert_eq!((a.players, b.players), (1, 1), "each sees the other");
        let p = puppets(&mut r.sims[1]);
        assert_eq!(p.len(), 1);
        let (_, np, t) = &p[0];
        assert_eq!(np.id, a.id, "the puppet is player 0");
        let d = ((t.pos[0] - ta.pos[0]).powi(2) + (t.pos[2] - ta.pos[2]).powi(2)).sqrt();
        assert!(d < 1.5, "the puppet is where the player is (a tenth of a second behind): {d} m");
        assert!(ta.pos[2] < -1.0, "and the player did walk: {:?}", ta.pos);
        assert!((yaw_of(t.rot) - 0.5).abs() < 0.05, "it faces the way the player faces: {}", yaw_of(t.rot));
        assert!(np.speed > 1.5, "its ground speed is known to the game: {}", np.speed);
        // the prefab was used: the puppet has the Name the prefab gave
        let mut named = 0;
        let _ = r.sims[1].world.for_each::<(&lunaris_sim::components::Name, &NetPuppet), _>(|_, _| named += 1);
        assert_eq!(named, 1, "made from the game's prefab");
    }

    #[test]
    fn what_the_player_reports_reaches_the_puppets_and_a_bare_puppet_needs_no_prefab() {
        let mut r = rig(2, "");
        r.run(2500, |sims, _| {
            let e = sims[0].world.entity(1).unwrap();
            let np = sims[0].world.get_mut::<NetPlayer>(e).unwrap();
            np.flags = 5;
            np.anim = 12;
            np.anim_t = 1.5;
            np.pitch = 0.4;
            np.extra = 77;
        });
        let p = puppets(&mut r.sims[1]);
        assert_eq!(p.len(), 1, "a puppet without a prefab is a bare entity");
        let np = p[0].1;
        assert_eq!((np.flags, np.anim, np.extra), (5, 12, 77), "{np:?}");
        assert!((np.anim_t - 1.5).abs() < 0.05 && (np.pitch - 0.4).abs() < 0.03, "{np:?}");
        assert_eq!(np.health, 100.0, "a player's health is the server's: the 80 of the sender's HitPoints is not believed (nor sent)");
    }

    /// A game that keeps every command its players send, whole.
    struct Hear(std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);
    impl Game for Hear {
        fn on_join(&mut self, w: &mut ServerWorld, slot: usize, name: &str) -> EntityState {
            Line.on_join(w, slot, name)
        }
        fn on_command(&mut self, _ctx: &mut crate::scale::server::Ctx, _slot: usize, data: &[u8]) {
            self.0.lock().unwrap().push(data.to_vec());
        }
    }

    #[test]
    fn a_numbered_act_is_said_to_the_server_as_the_command_of_its_line_and_a_name_still_works() {
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.0, duplicate: 0.0 }, 3);
        let end = net.endpoint();
        let addr = end.addr();
        let mut server = NetServer::new(ServerConfig::default(), end, Hear(heard.clone()));
        let mut s = sim("Mate");
        let acts = s.world.strings_mut().intern("# the keys of the game\n1 build:foundation\n3 place\n\n4 inv_drop  # a trailing note");
        let e = s.world.entity(1).unwrap();
        s.world.get_mut::<NetPlayer>(e).unwrap().acts = lunaris_ecs::Sym(acts);
        let mut d = NetDriver::new(NetClient::new(net.endpoint(), addr, "p0", 50));
        let mut now = 1000;
        let mut run = |ms: u64, s: &mut Sim, server: &mut NetServer<SimEndpoint, Hear>| {
            let end = now + ms;
            while now < end {
                now += 10;
                net.set_now(now);
                server.update(now);
                d.tick_at(s, now);
            }
        };
        run(2500, &mut s, &mut server);
        assert!(local(&s).0.connected);
        let say = |s: &mut Sim, act: u32, name: &str, args: [f32; 4]| {
            let sym = s.world.strings_mut().intern(name);
            let e = s.world.entity(1).unwrap();
            let np = s.world.get_mut::<NetPlayer>(e).unwrap();
            np.act = act;
            np.cmd = lunaris_ecs::Sym(if name.is_empty() { 0 } else { sym });
            np.cmd_args = args;
        };
        let commands = |heard: &std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>| heard.lock().unwrap().iter().filter(|m| m.first() == Some(&simgame::msg::ACTION)).cloned().collect::<Vec<_>>();
        let before = commands(&heard).len();
        say(&mut s, 3, "", [1.0, 2.0, 3.0, 4.0]);
        run(300, &mut s, &mut server);
        let c = commands(&heard);
        assert_eq!(c.len(), before + 1, "one command for one act");
        assert_eq!(c.last().unwrap(), &simgame::action("place", [1.0, 2.0, 3.0, 4.0]), "the line `3 place`, with the arguments");
        let np = local(&s).0;
        assert_eq!((np.act, np.cmd_args), (0, [0.0; 4]), "said once: the act and the arguments are cleared");
        say(&mut s, 4, "", [0.0, 1.0, 0.0, 0.0]);
        run(300, &mut s, &mut server);
        assert_eq!(commands(&heard).last().unwrap(), &simgame::action("inv_drop", [0.0, 1.0, 0.0, 0.0]), "a note after the name is not part of it");
        say(&mut s, 9, "", [0.0; 4]);
        run(300, &mut s, &mut server);
        assert_eq!(commands(&heard).len(), before + 2, "a number no line has says nothing");
        assert_eq!(local(&s).0.act, 0, "and is cleared");
        say(&mut s, 3, "hello", [5.0, 0.0, 0.0, 0.0]);
        run(300, &mut s, &mut server);
        assert_eq!(commands(&heard).last().unwrap(), &simgame::action("hello", [5.0, 0.0, 0.0, 0.0]), "a name wins over a number");
    }

    #[test]
    fn a_player_who_leaves_loses_their_puppet_and_a_refused_move_puts_the_player_back() {
        let mut r = rig(2, "Mate");
        r.run(2500, |_, _| {});
        assert_eq!(puppets(&mut r.sims[1]).len(), 1);
        // player 1 cheats: it jumps 200 m in one tick; the server refuses and tells it to go back
        let before = local(&r.sims[1]).1.pos;
        let mut jumped = false;
        r.run(1500, |sims, _| {
            if !jumped {
                jumped = true;
                let e = sims[1].world.entity(1).unwrap();
                sims[1].world.get_mut::<Transform>(e).unwrap().pos[0] += 200.0;
            }
        });
        let (np, t) = local(&r.sims[1]);
        assert!(np.corrections >= 1, "{np:?}");
        assert!((t.pos[0] - before[0]).abs() < 5.0, "put back near where it was: {:?} against {:?}", t.pos, before);
        // player 0 leaves
        r.drivers[0].client.disconnect();
        r.run(4500, |_, _| {});
        assert!(puppets(&mut r.sims[1]).is_empty(), "the puppet of a player who left is gone");
        assert_eq!(local(&r.sims[1]).0.players, 0);
    }

    /// The way into a game from a menu (`NetConnect`) needs to know how the attempt stands: asking, in, refused, lost, or no answer.
    #[test]
    fn a_driver_knows_whether_it_is_in_was_refused_or_got_no_answer() {
        // nobody listens at the address: asking goes on until the timeout, then it is given up
        let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.0, duplicate: 0.0 }, 3);
        let nobody = net.endpoint().addr();
        let mut d = NetDriver::new(NetClient::new(net.endpoint(), nobody, "p0", 50));
        d.timeout_ms = 1500;
        let mut s = sim("Mate");
        let mut now = 0;
        while now < 1200 {
            now += 10;
            net.set_now(now);
            d.tick_at(&mut s, now);
        }
        assert_eq!(d.link, LinkState::Connecting, "still asking before the timeout");
        while now < 1700 {
            now += 10;
            net.set_now(now);
            d.tick_at(&mut s, now);
        }
        assert_eq!(d.link, LinkState::TimedOut);
        assert_eq!(d.link.message(), "No answer from the server");
        // a server with room lets in, and one that is full refuses the second player
        let mut r = rig(2, "Mate");
        let end = r.net.endpoint();
        let addr = end.addr();
        r.server = NetServer::new(ServerConfig { max_clients: 1, ..ServerConfig::default() }, end, Line);
        r.drivers = (0..2).map(|i| NetDriver::new(NetClient::new(r.net.endpoint(), addr, &format!("p{i}"), 50 + i as u32))).collect();
        r.run(3000, |_, _| {});
        let links: Vec<LinkState> = r.drivers.iter().map(|d| d.link).collect();
        assert!(links.contains(&LinkState::Connected), "one is in: {links:?}");
        assert!(links.contains(&LinkState::Refused(2)), "the other is told the server is full: {links:?}");
        assert_eq!(LinkState::Refused(2).message(), "The server is full");
        // leaving: the driver is spent and the client no longer takes part
        let k = links.iter().position(|l| *l == LinkState::Connected).unwrap();
        let (mut sim_k, mut d_k) = (r.sims.remove(k), r.drivers.remove(k));
        d_k.leave(&mut sim_k);
        assert!(!d_k.client.connected());
    }
}
