//! Parties on the game server, end to end in memory over a bad network (docs/PARTIES.md): players invite each other by typing in the chat box, the members are a team to the weapons, talk to each other wherever they are,
//! see each other's panel, get up at each other's places, build on each other's land and are told about each other behind a wall. No art: names and boxes. The rig is the one of `simgame_tests.rs`, with the interface a party needs.
use super::client::NetClient;
use super::play::{yaw_rot, NetDriver};
use super::party;
use super::server::{Game, NetServer, ServerConfig};
use super::simgame::SimGame;
use super::transport::{Link, SimEndpoint, SimNet};
use lunaris_json::{parse, Json};
use lunaris_sim::components::{BuildTool, HitPoints, NetAvatar, NetParty, NetPlayer, NetPuppet, Parent, Team, Text, Transform, UiAnchor, UiInput};
use lunaris_sim::{Command, Sim};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn prefab(s: &mut Sim, name: &str, def: &str) {
    s.apply(&Command::DefinePrefab { name: name.into(), def: parse(def).unwrap() }).unwrap();
}

const RULES: &str = "rules step=90\\npiece foundation size=4,0.5,4 hp=500 free=1 anchor=1\\n";

/// A scene to host: a ground, three places to start (not in a line), the building rules and an avatar with a gun that hurts (20 a round, two rounds a second) and a build tool on its head. `host` is more of `NetHost`.
fn server_scene(host: &str) -> Sim {
    let chat = if host.contains("\"chat\"") { "" } else { r#","chat":0.5"# };
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    spawn(&mut s, 3, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    spawn(&mut s, 4, &format!(r#"{{"NetHost":{{"player":"Avatar","forward":"hit 100\ncorpse 100","static_every":3{chat}{host}}}}}"#));
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[8,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 23, r#"{"Transform":{"pos":[0,0,8]},"SpawnPoint":{}}"#);
    prefab(
        &mut s,
        "Avatar",
        r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},
            "HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12},"Respawn":{"delay":1,"avoid":2,"protect":0,"corpse":"Corpse","corpse_time":30},"NetAvatar":{}},
           "children":[{"components":{"Name":{"value":"Head"},"LocalTransform":{"pos":[0,1.6,0]},
            "Weapon":{"damage":20,"spread":0,"recoil":[0,0],"rate":120,"auto":true,"infinite":true,"noise":0,"range":120},"BuildTool":{"reach":12},"Interactor":{"reach":3,"look":false}}}]}"#,
    );
    prefab(&mut s, "Corpse", r#"{"components":{"Name":{"value":"Corpse"},"NetEntity":{"class":3,"kind":9}}}"#);
    s
}

const CHAT_LOG: u64 = 901;
const CHAT_BOX: u64 = 902;
const PARTY_BOX: u64 = 903;
const PANEL: u64 = 904;

/// A player's computer: its own body, the chat log and box, the party box and the panel (a `Text` with a `NetParty`).
fn client_scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{},"NetPlayer":{"puppet":"Mate","camera":9,"eye":[0,1.6,0],"kinds":"3 9 Corpse","say_input":902,"say_party_input":903}}"#);
    spawn(&mut s, 9, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[0,1.6,0]}}"#);
    spawn(&mut s, CHAT_LOG, r#"{"Text":{"value":""},"NetChat":{"lines":8,"life":600}}"#);
    spawn(&mut s, CHAT_BOX, r#"{"Ui":{"button":true},"UiInput":{"max_len":600}}"#);
    spawn(&mut s, PARTY_BOX, r#"{"Ui":{"button":true},"UiInput":{"max_len":600}}"#);
    spawn(&mut s, 905, r#"{"Ui":{}}"#);
    spawn(&mut s, PANEL, r#"{"Text":{"value":""},"Parent":{"target":905},"NetParty":{"lines":8,"marker":"Tag"}}"#);
    prefab(&mut s, "Tag", r#"{"components":{"Name":{"value":"Tag"},"Text":{"value":""},"UiAnchor":{"world":[0,2.2,0]}}}"#);
    for name in ["Mate", "Corpse"] {
        prefab(&mut s, name, &format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"Transform":{{}},"NetPuppet":{{}}}}}}"#));
    }
    s
}

struct Rig {
    net: SimNet,
    addr: super::transport::Addr,
    server: NetServer<SimEndpoint, SimGame>,
    sims: Vec<Sim>,
    drivers: Vec<NetDriver<SimEndpoint>>,
    now: u64,
    comings: u32,
    /// How many of the players' computers are running: they start one after the other, so that the server's slot of player `i` is `i`.
    live: usize,
}

fn rig(server: Sim, n: usize) -> Rig {
    rig_with(server, n, ServerConfig::default())
}

fn rig_with(server: Sim, n: usize, cfg: ServerConfig) -> Rig {
    let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.02, duplicate: 0.01 }, 7);
    let end = net.endpoint();
    let addr = end.addr();
    let server = NetServer::new(cfg, end, SimGame::new(server));
    let sims = (0..n).map(|_| client_scene()).collect();
    // (the drivers' clock is the rig's, which has run for a while when the later ones start: a driver must not think it has been asking for a long time)
    let drivers = (0..n)
        .map(|i| {
            let mut d = NetDriver::new(NetClient::new(net.endpoint(), addr, &format!("p{i}"), 50 + i as u32));
            d.timeout_ms = 1 << 40;
            d
        })
        .collect();
    let mut r = Rig { net, addr, server, sims, drivers, now: 1000, comings: 0, live: 0 };
    for i in 0..n {
        r.live = i + 1;
        r.idle(900);
    }
    r
}

impl Rig {
    fn run(&mut self, ms: u64, mut each: impl FnMut(&mut [Sim], u64)) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 10;
            self.net.set_now(self.now);
            self.server.update(self.now);
            each(&mut self.sims, self.now);
            for (d, s) in self.drivers.iter_mut().zip(self.sims.iter_mut()).take(self.live) {
                d.tick_at(s, self.now);
            }
        }
    }

    fn idle(&mut self, ms: u64) {
        self.run(ms, |_, _| {});
    }

    /// Player `i` goes (the server is told), and comes back with the same name on a new connection and a new scene of its own.
    fn leave(&mut self, i: usize) {
        self.drivers[i].leave(&mut self.sims[i]);
        self.idle(100);
    }

    fn come_back(&mut self, i: usize) {
        self.comings += 1;
        self.sims[i] = client_scene();
        self.drivers[i] = NetDriver::new(NetClient::new(self.net.endpoint(), self.addr, &format!("p{i}"), 500 + self.comings));
        // (the driver's clock here is the rig's: a driver made late must not think it has been asking for a long time)
        self.drivers[i].timeout_ms = 1 << 40;
    }
}

fn type_into(sim: &mut Sim, uid: u64, text: &str) {
    let sym = sim.world.strings_mut().intern(text);
    let e = sim.world.entity(uid).unwrap();
    let f = sim.world.get_mut::<UiInput>(e).unwrap();
    f.text.0 = sym;
    f.submitted = true;
}

/// Type into the chat box and press Enter.
fn say(sim: &mut Sim, text: &str) {
    type_into(sim, CHAT_BOX, text);
}

/// Type into the party's box and press Enter.
fn say_party(sim: &mut Sim, text: &str) {
    type_into(sim, PARTY_BOX, text);
}

fn text_of(sim: &Sim, uid: u64) -> String {
    let e = sim.world.entity(uid).unwrap();
    sim.world.strings().get(sim.world.get::<Text>(e).unwrap().value.0).to_string()
}

fn log_of(sim: &Sim) -> Vec<String> {
    text_of(sim, CHAT_LOG).lines().map(str::to_string).collect()
}

fn panel_of(sim: &Sim) -> String {
    text_of(sim, PANEL)
}

fn panel_numbers(sim: &Sim) -> NetParty {
    *sim.world.get::<NetParty>(sim.world.entity(PANEL).unwrap()).unwrap()
}

/// The server's avatar of player `slot`: its entity.
fn avatar(r: &Rig, slot: u32) -> Option<lunaris_ecs::Entity> {
    let w = &r.server.game.sim.world;
    w.entities_by_uid().into_iter().find(|e| w.get::<NetAvatar>(*e).is_some_and(|n| n.slot == slot))
}

fn team_of(r: &Rig, slot: u32) -> u32 {
    let w = &r.server.game.sim.world;
    avatar(r, slot).and_then(|e| w.get::<Team>(e)).map_or(0, |t| t.id)
}

fn hp_of(r: &Rig, slot: u32) -> f32 {
    let w = &r.server.game.sim.world;
    avatar(r, slot).and_then(|e| w.get::<HitPoints>(e)).map_or(-1.0, |h| h.value)
}

/// How many times the avatar of `slot` has died.
fn deaths_of(r: &Rig, slot: u32) -> u32 {
    let w = &r.server.game.sim.world;
    avatar(r, slot).and_then(|e| w.get::<lunaris_sim::components::Respawn>(e)).map_or(0, |x| x.deaths)
}

fn pos_of(r: &Rig, slot: u32) -> [f32; 3] {
    let w = &r.server.game.sim.world;
    avatar(r, slot).and_then(|e| w.get::<Transform>(e)).map(|t| t.pos).unwrap()
}

/// The tool of player `slot` (owner `slot + 1` once it has said `build:`).
fn tool_of(r: &Rig, slot: u32) -> BuildTool {
    let w = &r.server.game.sim.world;
    w.entities_by_uid().into_iter().filter_map(|e| w.get::<BuildTool>(e).copied()).find(|t| t.owner == slot + 1).unwrap_or_default()
}

fn local(s: &Sim) -> (NetPlayer, Transform) {
    let e = s.world.entity(1).unwrap();
    (*s.world.get::<NetPlayer>(e).unwrap(), *s.world.get::<Transform>(e).unwrap())
}

fn set_local(s: &mut Sim, f: impl FnOnce(&mut NetPlayer, &mut Transform)) {
    let e = s.world.entity(1).unwrap();
    let mut np = *s.world.get::<NetPlayer>(e).unwrap();
    let mut t = *s.world.get::<Transform>(e).unwrap();
    f(&mut np, &mut t);
    *s.world.get_mut::<NetPlayer>(e).unwrap() = np;
    *s.world.get_mut::<Transform>(e).unwrap() = t;
}

/// Player `from` holds its trigger for `ms` aiming at the chest of player `at` (where each is on its own screen).
fn shoot(r: &mut Rig, from: usize, at: usize, ms: u64) {
    let (_, ta) = local(&r.sims[from]);
    let (_, tb) = local(&r.sims[at]);
    let d = [tb.pos[0] - ta.pos[0], tb.pos[1] + 1.0 - (ta.pos[1] + 1.6), tb.pos[2] - ta.pos[2]];
    let yaw = (-d[0]).atan2(-d[2]);
    let pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt());
    r.run(ms, |sims, _| {
        set_local(&mut sims[from], |np, t| {
            np.pitch = pitch;
            np.flags = 1;
            t.rot = yaw_rot(yaw);
        });
    });
    set_local(&mut r.sims[from], |np, _| np.flags = 0);
    r.idle(200);
}

/// Player `a` invites player `b` by typing, and `b` accepts by typing.
fn form(r: &mut Rig, a: usize, b: usize) {
    say(&mut r.sims[a], &format!("/invite p{b}"));
    r.idle(700);
    say(&mut r.sims[b], "/accept");
    r.idle(700);
}

fn players_seen(s: &mut Sim) -> usize {
    let mut n = 0;
    let _ = s.world.for_each::<(&NetPuppet,), _>(|_, (p,)| {
        if p.class == 0 {
            n += 1;
        }
    });
    n
}

#[test]
fn players_invite_and_accept_by_typing_the_panel_shows_the_party_and_the_members_are_a_team() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    assert!(r.sims.iter().all(|s| local(s).0.connected));
    assert_eq!((team_of(&r, 0), team_of(&r, 1), team_of(&r, 2)), (0, 0, 0), "nobody is in a party");
    assert_eq!(panel_of(&r.sims[0]), "", "no party, no invitation: the panel is empty");
    // p0 invites p1: p0 is told it went, p1 is asked, nobody is in a party yet
    say(&mut r.sims[0], "/invite p1");
    r.idle(700);
    assert_eq!(log_of(&r.sims[0]), ["[PARTY]: Invitation sent to p1."]);
    assert_eq!(log_of(&r.sims[1]), ["[PARTY]: p0 invites you to a party. Type /accept or /decline (30 s)."]);
    assert!(panel_of(&r.sims[1]).starts_with("p0 invites you to a party (") && panel_of(&r.sims[1]).ends_with(" s)\n/accept   /decline"), "{:?}", panel_of(&r.sims[1]));
    assert!(panel_numbers(&r.sims[1]).invited && panel_numbers(&r.sims[1]).invite_secs >= 29.0 && panel_numbers(&r.sims[1]).count == 0);
    assert_eq!(panel_of(&r.sims[0]), "", "the inviter has nothing to show yet");
    assert_eq!((team_of(&r, 0), team_of(&r, 1)), (0, 0));
    // p1 accepts: a party of two, led by p0, and a team that is its number
    say(&mut r.sims[1], "/accept");
    r.idle(700);
    let party = team_of(&r, 0);
    assert!(party >= 100 && party == team_of(&r, 1) && team_of(&r, 2) == 0, "the party's number is its members' team: {party}");
    let mut tools_in_party = 0;
    let _ = r.server.game.sim.world.for_each::<(&BuildTool,), _>(|_, (t,)| tools_in_party += (t.party == party) as u32);
    assert_eq!(tools_in_party, 2, "the tools of the two members say which party they are in (shared land, below)");
    // each member's panel: itself first marked `you`, the other with how far and which way (from the server's places, to the metre)
    let from = |a: u32, b: u32| {
        let (pa, pb) = (pos_of(&r, a), pos_of(&r, b));
        let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
        let dist = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() + 0.5) as u32;
        format!("{dist} m {}", party::compass(party::bearing_byte(d[0], d[2])))
    };
    assert_eq!(panel_of(&r.sims[0]), format!("PARTY (2)\n* p0   100%   you\n- p1   100%   {}", from(0, 1)));
    assert_eq!(panel_of(&r.sims[1]), format!("PARTY (2)\n* p0   100%   {}\n- p1   100%   you", from(1, 0)));
    assert_eq!(panel_of(&r.sims[2]), "", "the third player is told nothing");
    let (n0, n1) = (panel_numbers(&r.sims[0]), panel_numbers(&r.sims[1]));
    assert_eq!((n0.count, n0.party, n0.leader, n0.invited), (2, party, true, false));
    assert_eq!((n1.count, n1.party, n1.leader, n1.invited), (2, party, false, false));
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: p1 joined the party.");
    assert_eq!(log_of(&r.sims[1]).last().unwrap(), "[PARTY]: You joined p0's party.");
    // the same distance is what a player sees of the other on its own screen: the panel's number is the server's, the puppet's place is what the client shows (a tenth of a second behind)
    let (_, t0) = local(&r.sims[0]);
    let (_, t1) = local(&r.sims[1]);
    let metres = ((t0.pos[0] - t1.pos[0]).powi(2) + (t0.pos[1] - t1.pos[1]).powi(2) + (t0.pos[2] - t1.pos[2]).powi(2)).sqrt();
    assert!(metres > 5.0, "the start places are 8 m apart or so: {metres}");
}

#[test]
fn the_weapons_of_a_party_never_hurt_its_members_and_hurt_everybody_else() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    form(&mut r, 0, 1);
    let party = team_of(&r, 0);
    assert!(party >= 100 && party == team_of(&r, 1), "{party}");
    // p0 shoots p1 for two seconds (four rounds of 20): nothing happens to p1 (friendly fire is off inside a party)
    shoot(&mut r, 0, 1, 2000);
    assert_eq!(hp_of(&r, 1), 100.0, "a member's round does not hurt a member");
    // p2, who is in no party, shoots p1: it is hurt
    shoot(&mut r, 2, 1, 2000);
    let after = hp_of(&r, 1);
    assert!(after < 100.0 && after >= 0.0, "a round of a player who is not in the party hurts: {after}");
    // and p1 shoots p2 back (p1 is a member, p2 is not): hurt too: friendly fire is only off *inside* the party
    shoot(&mut r, 1, 2, 2000);
    assert!(hp_of(&r, 2) < 100.0, "a member's round hurts a stranger: {}", hp_of(&r, 2));
    // when p1 leaves the party it can be shot by its old mate: the team is its own again
    say(&mut r.sims[1], "/leave");
    r.idle(700);
    assert_eq!((team_of(&r, 0), team_of(&r, 1)), (0, 0), "nobody is in a party");
    let (before, deaths) = (hp_of(&r, 1), deaths_of(&r, 1));
    shoot(&mut r, 0, 1, 1500);
    assert!(hp_of(&r, 1) < before || deaths_of(&r, 1) > deaths, "no longer friends: {} against {before} (it had 20 left: it died and got up)", hp_of(&r, 1));
}

#[test]
fn party_chat_goes_to_the_party_wherever_it_is_and_to_nobody_else() {
    // the chat distance is half a metre: only the speaker hears the players near
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    form(&mut r, 0, 1);
    let skip = log_of(&r.sims[0]).len();
    // p0 says something in the party's box: p1 hears it with the tag, p2 does not
    say_party(&mut r.sims[0], "  hello   party  ");
    r.idle(700);
    assert_eq!(log_of(&r.sims[1]).last().unwrap(), "[PARTY] p0: hello party");
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY] p0: hello party", "the speaker hears itself");
    assert!(log_of(&r.sims[2]).is_empty(), "a player who is not in the party hears nothing: {:?}", log_of(&r.sims[2]));
    // `/p` in the ordinary box is the same
    say(&mut r.sims[1], "/p and back");
    r.idle(700);
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY] p1: and back");
    assert!(log_of(&r.sims[2]).is_empty());
    // what is said to the players near is not told to the party (they are further than half a metre away), and a party's word is not told to the near
    say(&mut r.sims[0], "hi all");
    r.idle(700);
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "p0: hi all");
    assert_eq!(log_of(&r.sims[1]).last().unwrap(), "[PARTY] p1: and back", "p1 is not within 0.5 m of p0");
    // a player who is in no party is told so, and nobody else hears it
    say_party(&mut r.sims[2], "anybody?");
    say(&mut r.sims[2], "/p anybody?");
    r.idle(700);
    assert_eq!(log_of(&r.sims[2]), ["[PARTY]: You are not in a party. Invite somebody: /invite NAME"; 2]);
    assert!(log_of(&r.sims[0]).iter().all(|l| !l.contains("anybody")));
    // the party's messages count against the same limit as the chat: three at once
    let before = log_of(&r.sims[1]).len();
    for k in 0..6 {
        say_party(&mut r.sims[0], &format!("burst {k}"));
        r.idle(40);
    }
    r.idle(700);
    let heard: Vec<String> = log_of(&r.sims[1]).into_iter().filter(|l| l.contains("burst")).collect();
    assert!(heard.len() <= 3 && !heard.is_empty(), "a burst of three: {heard:?}");
    let _ = (skip, before);
    // the commands that are not ours are not said to anybody
    say(&mut r.sims[0], "/shrug");
    r.idle(700);
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: No such command. Type /help.");
    assert!(log_of(&r.sims[2]).iter().all(|l| !l.contains("shrug")));
}

#[test]
fn leaving_kicking_and_disbanding_give_the_numbers_back_and_clear_the_panels() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    form(&mut r, 0, 1);
    say(&mut r.sims[0], "/invite p2");
    r.idle(700);
    say(&mut r.sims[2], "/accept p0");
    r.idle(800);
    let party = team_of(&r, 0);
    assert!(party >= 100 && team_of(&r, 1) == party && team_of(&r, 2) == party, "three in a party");
    assert!(panel_of(&r.sims[1]).starts_with("PARTY (3)\n"));
    // a member cannot kick
    say(&mut r.sims[1], "/kick p2");
    r.idle(700);
    assert_eq!(log_of(&r.sims[1]).last().unwrap(), "[PARTY]: Only the party leader can kick.");
    assert_eq!(team_of(&r, 2), party);
    // the leader kicks p2: its team is its own again, its panel is empty, the others' say two
    say(&mut r.sims[0], "/kick p2");
    r.idle(800);
    assert_eq!(team_of(&r, 2), 0);
    assert_eq!(panel_of(&r.sims[2]), "", "the panel of the one who was sent away is cleared");
    assert_eq!(log_of(&r.sims[2]).last().unwrap(), "[PARTY]: You were removed from the party.");
    assert!(panel_of(&r.sims[0]).starts_with("PARTY (2)\n") && panel_of(&r.sims[1]).starts_with("PARTY (2)\n"));
    assert_eq!(panel_numbers(&r.sims[2]).count, 0);
    // the leader leaves: a party of one is no party: p1 is told and its panel is cleared
    say(&mut r.sims[0], "/leave");
    r.idle(800);
    assert_eq!((team_of(&r, 0), team_of(&r, 1)), (0, 0));
    assert_eq!((panel_of(&r.sims[0]), panel_of(&r.sims[1])), (String::new(), String::new()));
    assert_eq!(log_of(&r.sims[1]).last().unwrap(), "[PARTY]: The party ended.");
    // a new party, and the leader disbands it
    form(&mut r, 1, 2);
    assert!(team_of(&r, 1) >= 100 && team_of(&r, 1) == team_of(&r, 2));
    assert_ne!(team_of(&r, 1), party, "the number of an ended party is not used again at once");
    say(&mut r.sims[2], "/disband");
    r.idle(700);
    assert_eq!(log_of(&r.sims[2]).last().unwrap(), "[PARTY]: Only the party leader can disband it.");
    say(&mut r.sims[1], "/disband");
    r.idle(800);
    assert_eq!((team_of(&r, 1), team_of(&r, 2)), (0, 0));
    assert_eq!((panel_of(&r.sims[1]), panel_of(&r.sims[2])), (String::new(), String::new()));
    assert_eq!(log_of(&r.sims[2]).last().unwrap(), "[PARTY]: The party was disbanded.");
    assert!(r.server.game.allies().1.is_empty(), "nobody is in a party: no allies");
}

#[test]
fn a_player_whose_connection_drops_keeps_its_seat_for_a_minute_and_the_panel_says_it_is_away() {
    let mut r = rig(server_scene(""), 2);
    r.idle(2500);
    form(&mut r, 0, 1);
    let party = team_of(&r, 0);
    assert!(party >= 100);
    r.leave(1);
    r.idle(2000);
    assert!(avatar(&r, 1).is_none(), "its avatar is gone");
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: p1 lost the connection (the seat is kept for 60 s).");
    assert_eq!(panel_of(&r.sims[0]), "PARTY (2)\n* p0   100%   you\n- p1   away", "the seat is kept, and the panel says so");
    // it comes back with the same name within the minute: the same party, the numbers on its avatar again
    r.idle(20_000);
    r.come_back(1);
    r.idle(3000);
    assert!(local(&r.sims[1]).0.connected);
    assert_eq!(team_of(&r, 1), party, "the same party");
    assert!(panel_of(&r.sims[1]).starts_with("PARTY (2)\n* p0   100%"), "{:?}", panel_of(&r.sims[1]));
    assert_eq!(log_of(&r.sims[1]).last().unwrap(), "[PARTY]: You are back in p0's party.");
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: p1 is back.");
    // it drops again and does not come back: after a minute the seat is given up and, with nobody else, the party ends
    r.leave(1);
    r.idle(61_000);
    assert_eq!(team_of(&r, 0), 0, "the party ended with its second member gone");
    assert_eq!(panel_of(&r.sims[0]), "");
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: The party ended.", "{:?}", log_of(&r.sims[0]));
}

#[test]
fn a_member_gets_up_at_a_party_mates_place_and_a_player_who_left_the_party_does_not() {
    // p0 (slot 0, group 1) has a place of its own, far away; p1 (slot 1, group 2) has none: in p0's party it gets up at p0's, out of it at a shared one
    let mut scene = server_scene("");
    spawn(&mut scene, 30, r#"{"Transform":{"pos":[0,0,60]},"SpawnPoint":{"group":1}}"#);
    let mut r = rig(scene, 3);
    r.idle(2500);
    form(&mut r, 0, 1);
    // p2 kills p1 (five rounds of 20)
    shoot(&mut r, 2, 1, 4000);
    r.idle(3000);
    let p = pos_of(&r, 1);
    assert!((p[2] - 60.0).abs() < 2.0 && p[0].abs() < 2.0, "p1 got up at p0's place: {p:?}");
    assert_eq!(hp_of(&r, 1), 100.0);
    assert_eq!(team_of(&r, 1), team_of(&r, 0), "and it is still in the party");
    // p1 leaves the party and is killed again: a shared place, not p0's
    say(&mut r.sims[1], "/leave");
    r.idle(700);
    shoot(&mut r, 2, 1, 4000);
    r.idle(3000);
    let p = pos_of(&r, 1);
    assert!((p[2] - 60.0).abs() > 20.0, "out of the party p0's place is not p1's: {p:?}");
    assert_eq!(hp_of(&r, 1), 100.0);
}

#[test]
fn land_that_a_party_mate_keeps_is_the_members_too() {
    // a cupboard of owner 1 (the owner number of the player in slot 0) that covers everything round the start places
    let mut scene = server_scene("");
    spawn(&mut scene, 31, r#"{"Transform":{"pos":[0,0,0]},"BuildUpkeep":{"owner":1,"radius":300}}"#);
    let mut r = rig(scene, 2);
    r.idle(2500);
    // p1 (owner 2) looks down at its feet with the foundation in its hands, and says so to the server
    let ask = |r: &mut Rig| {
        r.run(600, |sims, _| set_local(&mut sims[1], |np, _| np.pitch = -1.2));
    };
    // (the command is a name: set through the interface's way, `NetPlayer.cmd`)
    let command = |sim: &mut Sim, name: &str| {
        let sym = sim.world.strings_mut().intern(name);
        let e = sim.world.entity(1).unwrap();
        sim.world.get_mut::<NetPlayer>(e).unwrap().cmd = lunaris_ecs::Sym(sym);
    };
    command(&mut r.sims[1], "build:foundation");
    // (the owner of the land has said `build:` too, as it does to put a cupboard down: that is what gives its tool the owner number the land is kept under)
    command(&mut r.sims[0], "build:foundation");
    ask(&mut r);
    let t = tool_of(&r, 1);
    let why = r.server.game.sim.world.strings().get(t.why.0).to_string();
    assert!(!t.valid && why == "not_yours", "inside another owner's land: {why:?}");
    // p0 (the owner of the land) invites p1, who accepts: the land is p1's as well
    form(&mut r, 0, 1);
    ask(&mut r);
    let t = tool_of(&r, 1);
    let why = r.server.game.sim.world.strings().get(t.why.0).to_string();
    assert!(t.valid && why.is_empty() && t.party >= 100, "a party mate's land is the member's: valid {} why {why:?} party {}", t.valid, t.party);
    // p1 leaves: refused again
    say(&mut r.sims[1], "/leave");
    r.idle(700);
    ask(&mut r);
    let t = tool_of(&r, 1);
    let why = r.server.game.sim.world.strings().get(t.why.0).to_string();
    assert!(!t.valid && why == "not_yours" && t.party == 0, "out of the party: {why:?}");
}

#[test]
fn members_are_told_about_each_other_behind_a_wall_and_nobody_else_is() {
    // a wall down the middle (2 m thick, 8 m high, 60 m long), the safe radius 16 m, one place to start west of it and two east of it (10 m apart): with three players one is alone on its side
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,4,0]},"Collider":{"shape":1,"size":[2,8,60]}}"#);
    spawn(&mut s, 4, r#"{"NetHost":{"player":"Avatar","static_every":3,"chat":1000,"occlusion":16}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[-25,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[25,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 23, r#"{"Transform":{"pos":[25,0,10]},"SpawnPoint":{}}"#);
    prefab(&mut s, "Avatar", r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},"HitPoints":{"value":100,"max":100,"last":100},"NetAvatar":{}}}"#);
    let mut r = rig(s, 3);
    r.idle(4000);
    let x: Vec<f32> = (0..3).map(|i| local(&r.sims[i]).1.pos[0]).collect();
    let west: Vec<usize> = (0..3).filter(|i| x[*i] < 0.0).collect();
    assert_eq!(west.len(), 1, "one player is alone on the west side: {x:?}");
    let (w, e1, e2) = (west[0], (0..3).find(|i| x[*i] > 0.0).unwrap(), (0..3).rev().find(|i| x[*i] > 0.0).unwrap());
    assert_ne!(e1, e2);
    // before any party: nobody is told about a player behind the wall, the two on the east side see each other
    assert_eq!(players_seen(&mut r.sims[w]), 0, "the wall hides the others from the player on the west");
    assert_eq!((players_seen(&mut r.sims[e1]), players_seen(&mut r.sims[e2])), (1, 1), "the two on the east side are within the safe radius of each other");
    // w and e1 form a party: they are told about each other through the wall, and e2 still is not told about w
    form(&mut r, w, e1);
    r.idle(3000);
    assert_eq!(players_seen(&mut r.sims[w]), 1, "the west player sees its party mate through the wall (and not the stranger)");
    assert_eq!(players_seen(&mut r.sims[e1]), 2, "e1 sees w (a mate) and e2 (near)");
    assert_eq!(players_seen(&mut r.sims[e2]), 1, "the stranger is still hidden from w: e2 sees e1 only");
    let seen: Vec<[f32; 3]> = {
        let mut v = Vec::new();
        let _ = r.sims[w].world.for_each::<(&NetPuppet, &Transform), _>(|_, (p, t)| {
            if p.class == 0 {
                v.push(t.pos);
            }
        });
        v
    };
    assert!(seen.len() == 1 && seen[0][0] > 0.0, "what w sees is on the other side: {seen:?}");
    // the server's list for the wallhack defence says so
    let (version, list) = r.server.game.allies();
    assert!(version > 0 && list.len() == 2 && list[0].1 == list[1].1, "{version} {list:?}");
    // w leaves the party: the wall hides them from each other again (after the time a client keeps what it has not heard of)
    say(&mut r.sims[w], "/leave");
    r.idle(4000);
    assert_eq!(players_seen(&mut r.sims[w]), 0, "out of the party the wall hides e1 again");
    assert_eq!(r.server.game.allies().1.len(), 0);
}

#[test]
fn the_wall_exemption_holds_when_the_server_makes_the_packets_on_several_threads() {
    // the server makes the packets on several threads from 24 clients on (ServerConfig::threads): the same wall as above with 26 players, three of them beside it and the rest on places far past everybody's interest range
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    spawn(&mut s, 7, r#"{"Transform":{"pos":[0,4,0]},"Collider":{"shape":1,"size":[2,8,60]}}"#);
    spawn(&mut s, 4, r#"{"NetHost":{"player":"Avatar","static_every":3,"chat":1000,"occlusion":16}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[-25,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[25,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 23, r#"{"Transform":{"pos":[25,0,10]},"SpawnPoint":{}}"#);
    for k in 0..23u64 {
        spawn(&mut s, 100 + k, &format!(r#"{{"Transform":{{"pos":[{},0,190]}},"SpawnPoint":{{}}}}"#, -190.0 + 17.0 * k as f32));
    }
    prefab(&mut s, "Avatar", r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},"HitPoints":{"value":100,"max":100,"last":100},"NetAvatar":{}}}"#);
    let cfg = ServerConfig { threads: 4, ..ServerConfig::default() };
    assert_eq!(cfg.packet_threads(26), 4, "26 clients are made packets for on four threads");
    let mut r = rig_with(s, 26, cfg);
    assert!(r.server.game.blocker().is_some(), "the game's level of sight can be shared between threads");
    r.idle(6000);
    let connected: Vec<usize> = (0..26).filter(|i| local(&r.sims[*i]).0.connected).collect();
    assert_eq!(connected.len(), 26, "all 26 players are in the game: {connected:?}");
    let pos: Vec<[f32; 3]> = (0..26).map(|i| local(&r.sims[i]).1.pos).collect();
    let trio: Vec<usize> = (0..26).filter(|i| pos[*i][2].abs() < 50.0).collect();
    assert_eq!(trio.len(), 3, "three players start beside the wall: {pos:?}");
    let west: Vec<usize> = trio.iter().copied().filter(|i| pos[*i][0] < 0.0).collect();
    let east: Vec<usize> = trio.iter().copied().filter(|i| pos[*i][0] > 0.0).collect();
    assert!(west.len() == 1 && east.len() == 2, "one is alone on the west side: {pos:?}");
    let (w, e1, e2) = (west[0], east[0], east[1]);
    assert_eq!(players_seen(&mut r.sims[w]), 0, "the wall hides the others from the player on the west");
    assert_eq!((players_seen(&mut r.sims[e1]), players_seen(&mut r.sims[e2])), (1, 1));
    form(&mut r, w, e1);
    r.idle(3000);
    assert_eq!(players_seen(&mut r.sims[w]), 1, "the west player sees its party mate through the wall and not the stranger");
    assert_eq!(players_seen(&mut r.sims[e1]), 2, "e1 sees w (a mate) and e2 (near)");
    assert_eq!(players_seen(&mut r.sims[e2]), 1, "the stranger is still hidden from w");
    say(&mut r.sims[w], "/leave");
    r.idle(4000);
    assert_eq!(players_seen(&mut r.sims[w]), 0, "out of the party the wall hides e1 again");
}

#[test]
fn the_panel_is_told_when_something_changes_and_at_most_once_a_second_while_somebody_walks() {
    let mut r = rig(server_scene(""), 2);
    r.idle(2500);
    form(&mut r, 0, 1);
    // nothing moves: only the refresh every five seconds
    let heard0 = panel_numbers(&r.sims[1]).heard;
    r.idle(12_000);
    let still = panel_numbers(&r.sims[1]).heard - heard0;
    assert!((1..=3).contains(&still), "a refresh every five seconds when nothing changed: {still} panels in 12 s");
    // p0 walks about (a metre every tenth of a second: the distance changes every time): a panel a second, not one for every metre
    let heard0 = panel_numbers(&r.sims[1]).heard;
    let (_, start) = local(&r.sims[0]);
    r.run(10_000, |sims, now| {
        let back_and_forth = ((now / 4000) % 2) as f32;
        let k = (now % 4000) as f32 / 4000.0;
        set_local(&mut sims[0], |_, t| {
            t.pos = [start.pos[0] + 6.0 * if back_and_forth == 0.0 { k } else { 1.0 - k }, start.pos[1], start.pos[2]];
        });
    });
    let walking = panel_numbers(&r.sims[1]).heard - heard0;
    assert!((5..=13).contains(&walking), "about one panel a second while the distance changes: {walking} in 10 s");
}

#[test]
fn a_game_that_says_its_commands_by_number_can_invite_accept_kick_and_leave_without_typing() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    let command = |sim: &mut Sim, name: &str, arg: f32| {
        let sym = sim.world.strings_mut().intern(name);
        let e = sim.world.entity(1).unwrap();
        let np = sim.world.get_mut::<NetPlayer>(e).unwrap();
        np.cmd = lunaris_ecs::Sym(sym);
        np.cmd_args = [arg, 0.0, 0.0, 0.0];
    };
    let id1 = local(&r.sims[1]).0.id as f32;
    let id2 = local(&r.sims[2]).0.id as f32;
    assert!(id1 > 0.0 && id2 > 0.0);
    // p0 invites p1 by the number the server gave it (`NetPuppet.id`), p1 accepts with no argument
    command(&mut r.sims[0], "party_invite", id1);
    r.idle(700);
    assert!(panel_numbers(&r.sims[1]).invited);
    command(&mut r.sims[1], "party_accept", 0.0);
    r.idle(700);
    assert!(team_of(&r, 0) >= 100 && team_of(&r, 0) == team_of(&r, 1));
    // a number that is nobody's is said so
    command(&mut r.sims[0], "party_invite", 9999.0);
    r.idle(300);
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: No such player here.");
    // p0 invites p2, p2 declines by number, then is invited again after the quiet and accepts the one from p0 by its number
    command(&mut r.sims[0], "party_invite", id2);
    r.idle(300);
    command(&mut r.sims[2], "party_decline", 0.0);
    r.idle(300);
    assert!(!panel_numbers(&r.sims[2]).invited && team_of(&r, 2) == 0);
    r.idle(21_000);
    command(&mut r.sims[0], "party_invite", id2);
    r.idle(300);
    let id0 = local(&r.sims[0]).0.id as f32;
    command(&mut r.sims[2], "party_accept", id0);
    r.idle(700);
    assert_eq!(team_of(&r, 2), team_of(&r, 0));
    // the leader kicks p2 by its number, p1 leaves, the party ends
    command(&mut r.sims[0], "party_kick", id2);
    r.idle(700);
    assert_eq!(team_of(&r, 2), 0);
    command(&mut r.sims[1], "party_leave", 0.0);
    r.idle(700);
    assert_eq!((team_of(&r, 0), team_of(&r, 1)), (0, 0));
}

#[test]
fn with_no_party_size_the_chat_is_the_old_chat_and_nobody_is_ever_a_team() {
    let mut r = rig(server_scene(r#","party":0,"chat":1000"#), 2);
    r.idle(2500);
    // `/invite p1` is just something said to the players near
    say(&mut r.sims[0], "/invite p1");
    r.idle(700);
    assert_eq!(log_of(&r.sims[1]), ["p0: /invite p1"], "with parties off a text that starts with a slash is chat, as it was");
    assert_eq!(panel_of(&r.sims[1]), "");
    say(&mut r.sims[1], "/accept");
    r.idle(700);
    assert_eq!((team_of(&r, 0), team_of(&r, 1)), (0, 0));
    let group = |slot: u32| avatar(&r, slot).and_then(|e| r.server.game.sim.world.get::<lunaris_sim::components::Respawn>(e)).map_or(99, |x| x.group);
    assert_eq!((group(0), group(1)), (0, 0), "with parties off the places to get up at are left as the scene made them");
    // the party's box says nothing to anybody: it is not chat either
    say_party(&mut r.sims[0], "hello");
    r.idle(700);
    assert_eq!(log_of(&r.sims[1]).len(), 2, "the party channel is not a way round: {:?}", log_of(&r.sims[1]));
    assert!(r.server.game.allies().1.is_empty());
}

#[test]
fn parties_work_with_the_proximity_chat_off() {
    // NetHost.chat 0 (nobody can talk to the players near): the commands and the party's own channel still work, and what is said to the players near still goes nowhere
    let mut r = rig(server_scene(r#","chat":0"#), 3);
    r.idle(2500);
    form(&mut r, 0, 1);
    assert!(team_of(&r, 0) != 0 && team_of(&r, 0) == team_of(&r, 1), "p0 and p1 are a party with the chat off");
    say_party(&mut r.sims[0], "only for the party");
    r.idle(700);
    say(&mut r.sims[0], "hello neighbours");
    r.idle(700);
    let l1 = log_of(&r.sims[1]);
    assert!(l1.iter().any(|l| l == "[PARTY] p0: only for the party"), "the party hears its word: {l1:?}");
    assert!(l1.iter().all(|l| !l.contains("hello neighbours")), "what is said to the players near is nobody's with the chat off: {l1:?}");
    let l2 = log_of(&r.sims[2]);
    assert!(l2.iter().all(|l| !l.contains("only for the party") && !l.contains("hello neighbours")), "a stranger hears neither: {l2:?}");
}

#[test]
fn a_message_that_is_not_one_is_ignored_and_the_party_box_cannot_be_used_to_flood() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    form(&mut r, 0, 1);
    // the wire, as a modified client could send it: the channel byte with nothing after it, nothing but control characters, an enormous text, bytes that are no text
    for raw in [vec![0x05u8, party::CH_PARTY], vec![0x05, party::CH_PARTY, 0x07, 0x01], vec![0x05, party::CH_PARTY, 0xff, 0xfe], [vec![0x05u8, party::CH_PARTY], vec![b'x'; 400]].concat(), vec![0x05]] {
        r.drivers[0].client.command(raw);
        r.idle(50);
    }
    r.idle(700);
    let heard = log_of(&r.sims[1]);
    assert!(heard.iter().all(|l| l.starts_with("[PARTY]")) && heard.len() <= 4, "{heard:?}");
    assert!(heard.iter().filter(|l| l.contains("xxxx")).all(|l| l.len() <= "[PARTY] p0: ".len() + 120), "an enormous text is cut to the chat's 120 characters");
    // a hundred invitations in a second are answered a few times: eight commands at once and two a second after that
    let heard = |r: &Rig| r.sims[0].world.get::<lunaris_sim::components::NetChat>(r.sims[0].world.entity(CHAT_LOG).unwrap()).unwrap().count;
    let before = heard(&r);
    for k in 0..100 {
        say(&mut r.sims[0], &format!("/invite nobody{k}"));
        r.idle(10);
    }
    r.idle(800);
    let answered = heard(&r) - before;
    assert!((3..=12).contains(&answered), "a hundred commands in a second, {answered} answers");
    // the answers that need no table are limited the same way: `/help`, a word that is not ours and the party's box, said a hundred times to a player in no party
    let heard2 = |r: &Rig| r.sims[2].world.get::<lunaris_sim::components::NetChat>(r.sims[2].world.entity(CHAT_LOG).unwrap()).unwrap().count;
    let before = heard2(&r);
    for k in 0..99 {
        match k % 3 {
            0 => say(&mut r.sims[2], "/help"),
            1 => say(&mut r.sims[2], "/shrug"),
            _ => say_party(&mut r.sims[2], "anybody"),
        }
        r.idle(10);
    }
    r.idle(800);
    let answered = heard2(&r) - before;
    assert!(answered <= 4 * 12, "99 commands in a second: {answered} lines of answers (a `/help` is four)");
}

#[test]
fn two_parties_are_two_sides_and_each_panel_shows_only_its_own() {
    let mut r = rig(server_scene(""), 4);
    r.idle(2500);
    form(&mut r, 0, 1);
    form(&mut r, 2, 3);
    let (x, y) = (team_of(&r, 0), team_of(&r, 2));
    assert!(x >= 100 && y >= 100 && x == team_of(&r, 1) && y == team_of(&r, 3) && x != y, "two parties, two numbers: {x} {y}");
    assert!(panel_of(&r.sims[0]).starts_with("PARTY (2)\n* p0") && panel_of(&r.sims[2]).starts_with("PARTY (2)\n* p2"));
    assert!(!panel_of(&r.sims[0]).contains("p2") && !panel_of(&r.sims[2]).contains("p0"), "nobody is told of the other party's members");
    // a member of one party hurts a member of the other: friendly fire is only off inside a party
    let before = hp_of(&r, 2);
    shoot(&mut r, 0, 2, 1000);
    assert!(hp_of(&r, 2) < before, "party {x} against party {y}: {} against {before}", hp_of(&r, 2));
    // a player cannot invite a member of another party, and a member of one cannot be asked into the other
    say(&mut r.sims[0], "/invite p2");
    r.idle(700);
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: p2 is in a party already.");
}

/// The game's part of a server tick with 200 players and parties of various sizes, in memory: `SURVIVAL_SCENE=path/to/survival.lunaris.json cargo test --release -p lunaris-net party_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn party_cost_with_200_players() {
    use super::server::{Ctx, Game, Outbox, World};
    use lunaris_api::Engine;
    use std::time::Instant;
    let Ok(scene) = std::env::var("SURVIVAL_SCENE") else { return };
    let bots: usize = std::env::var("BENCH_BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    struct Rng(u64);
    impl Rng {
        fn f(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }
    }
    // (each configuration three times, the best of them: the machine is shared)
    for (label, size, off) in [("parties off (party 0)", 0usize, true), ("on, nobody in a party", 0, false), ("100 parties of two", 2, false), ("50 parties of four", 4, false)].iter().flat_map(|c| std::iter::repeat(*c).take(3)) {
        let path = std::path::Path::new(&scene);
        let root = path.parent().and_then(|p| p.parent()).unwrap().to_path_buf();
        let _ = std::fs::remove_file(root.join("saves/survival_world.json"));
        let mut engine = Engine::new(1, root);
        engine.call("load_scene", &Json::obj([("path", Json::str(format!("scenes/{}", path.file_name().unwrap().to_string_lossy())))])).expect("the scene loads");
        if off {
            for e in engine.sim.world.entities_by_uid() {
                if let Some(h) = engine.sim.world.get_mut::<lunaris_sim::components::NetHost>(e) {
                    h.party = 0;
                }
            }
        }
        let mut game = SimGame::new(engine.sim);
        let mut world = World::default();
        let mut out = Outbox::default();
        let mut ids = Vec::new();
        for slot in 0..bots {
            let st = game.on_join(&mut world, slot, &format!("bot{slot}"));
            let id = world.spawn_state(st);
            world.entities.get_mut(&id).unwrap().owner = Some(slot);
            ids.push(id);
        }
        let mut tick = 1u32;
        let step = |game: &mut SimGame, world: &mut World, out: &mut Outbox, tick: &mut u32| {
            world.tick = *tick;
            let mut ctx = Ctx { world, out, tick: *tick, tick_hz: 30 };
            game.on_tick(&mut ctx);
            *tick += 1;
        };
        for _ in 0..5 {
            step(&mut game, &mut world, &mut out, &mut tick);
            out = Outbox::default();
        }
        // the parties: the first of each group asks the others in, by name, and they say yes (one command each, five seconds apart for the bucket)
        if size > 0 {
            for first in (0..bots).step_by(size) {
                for k in first + 1..(first + size).min(bots) {
                    let mut ctx = Ctx { world: &mut world, out: &mut out, tick, tick_hz: 30 };
                    game.on_command(&mut ctx, first, &super::simgame::say(&format!("/invite bot{k}")));
                    game.on_command(&mut ctx, k, &super::simgame::say("/accept"));
                }
            }
            for _ in 0..5 {
                step(&mut game, &mut world, &mut out, &mut tick);
                out = Outbox::default();
            }
        }
        let mut rng = Rng(0x5EED_1234_ABCD_EF01);
        let mut walkers: Vec<(lunaris_sim::components::Transform, f32, f32, f32)> = Vec::new();
        for id in &ids {
            let p = world.get(*id).unwrap().state.pos_m();
            walkers.push((lunaris_sim::components::Transform { pos: p, ..Default::default() }, rng.f() * 6.28, 0.0, 0.0));
        }
        let (warm, ticks) = (150u32, 450u32);
        let (mut total, mut worst, mut panels, mut bytes) = (0u128, 0u128, 0usize, 0usize);
        let party0 = (game.stats.party_us, game.stats.party_ticks);
        for k in 1..=ticks {
            let now = k as f32 / 30.0;
            for (w, id) in walkers.iter_mut().zip(&ids) {
                if now >= w.3 {
                    w.1 += (rng.f() - 0.5) * 2.4;
                    w.2 = [0.0, 2.0, 5.0, 8.0][(rng.f() * 3.99) as usize];
                    w.3 = now + 0.8 + rng.f() * 2.5;
                }
                let (sn, cs) = w.1.sin_cos();
                w.0.pos[0] += sn * w.2 / 30.0;
                w.0.pos[2] += cs * w.2 / 30.0;
                if let Some(e) = world.entities.get_mut(id) {
                    e.state.set_pos_m(w.0.pos);
                    e.state.set_vel_ms([sn * w.2, 0.0, cs * w.2]);
                }
            }
            let t = Instant::now();
            step(&mut game, &mut world, &mut out, &mut tick);
            let us = t.elapsed().as_nanos();
            if k > warm {
                total += us;
                worst = worst.max(us);
                for (_, m) in &out.to {
                    if m.first() == Some(&party::msg::PANEL) {
                        panels += 1;
                        bytes += m.len();
                    }
                }
            }
            out = Outbox::default();
        }
        let n = (ticks - warm) as f64;
        let (pus, pt) = (game.stats.party_us - party0.0, game.stats.party_ticks - party0.1);
        println!(
            "{label:20}: the game's tick {:.0} us (the slowest {:.2} ms); the party part {:.1} us a tick over the {} ticks it ran; {:.1} panels a second ({:.0} bytes a second) for {} players",
            total as f64 / n / 1000.0,
            worst as f64 / 1.0e6,
            pus as f64 / pt.max(1) as f64,
            pt,
            panels as f64 / (n / 30.0),
            bytes as f64 / (n / 30.0),
            bots
        );
    }
}

/// The markers a client shows over its party mates: (the name in it, the uid it is pinned to, its parent), by name.
fn markers_of(sim: &mut Sim) -> Vec<(String, u64, u64)> {
    let mut raw: Vec<(u32, u64, u64)> = Vec::new();
    let _ = sim.world.for_each::<(&UiAnchor, &Text, &Parent), _>(|_, (a, t, p)| raw.push((t.value.0, a.entity.0, p.target.0)));
    let mut out: Vec<(String, u64, u64)> = raw.into_iter().map(|(t, e, p)| (sim.world.strings().get(t).to_string(), e, p)).collect();
    out.sort();
    out
}

/// The uid of the puppet a client shows for the player whose server number is `id`.
fn puppet_uid(sim: &mut Sim, id: u32) -> Option<u64> {
    let mut found = None;
    let _ = sim.world.for_each_uid::<(&NetPuppet,), _>(|_, uid, (p,)| {
        if p.id == id && p.class == 0 {
            found = Some(uid);
        }
    });
    found
}

#[test]
fn a_party_mate_you_can_see_gets_a_marker_pinned_to_its_puppet_and_a_stranger_does_not() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    assert!((0..3).all(|i| markers_of(&mut r.sims[i]).is_empty()), "no party, no markers");
    form(&mut r, 0, 1);
    r.idle(2000);
    let (id0, id1) = (local(&r.sims[0]).0.id, local(&r.sims[1]).0.id);
    let (pu1, pu0) = (puppet_uid(&mut r.sims[0], id1).unwrap(), puppet_uid(&mut r.sims[1], id0).unwrap());
    assert_eq!(markers_of(&mut r.sims[0]), [("p1".to_string(), pu1, 905)], "one marker over p1's puppet, its name, hung where the panel hangs (the HUD)");
    assert_eq!(markers_of(&mut r.sims[1]), [("p0".to_string(), pu0, 905)]);
    assert!(markers_of(&mut r.sims[2]).is_empty(), "a player in no party has none, and is not marked for the party");
    // a third member: each of the others has two
    say(&mut r.sims[0], "/invite p2");
    r.idle(700);
    say(&mut r.sims[2], "/accept");
    r.idle(2000);
    let names: Vec<String> = markers_of(&mut r.sims[0]).into_iter().map(|m| m.0).collect();
    assert_eq!(names, ["p1", "p2"], "{names:?}");
    assert_eq!(markers_of(&mut r.sims[2]).len(), 2);
    // the leader sends p2 away: its marker goes from the others' screens and it has none
    say(&mut r.sims[0], "/kick p2");
    r.idle(1500);
    assert_eq!(markers_of(&mut r.sims[0]).len(), 1);
    assert!(markers_of(&mut r.sims[2]).is_empty());
    // the party ends: no markers
    say(&mut r.sims[1], "/leave");
    r.idle(1500);
    assert!((0..3).all(|i| markers_of(&mut r.sims[i]).is_empty()));
    // (and no marker entity is left behind in the client's world: a marker is a Text with a UiAnchor, the panel is a Text without)
    let mut texts_with_anchor = 0;
    let _ = r.sims[0].world.for_each::<(&UiAnchor,), _>(|_, _| texts_with_anchor += 1);
    assert_eq!(texts_with_anchor, 0);
}

#[test]
fn invite_with_no_name_asks_the_nearest_player_that_is_in_no_party() {
    let mut r = rig(server_scene(""), 3);
    r.idle(2500);
    // the nearest of the others to player 0 (the start places are 8 m apart: when two are as near, the lower slot)
    let d = |a: u32, b: u32| {
        let (pa, pb) = (pos_of(&r, a), pos_of(&r, b));
        ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt()
    };
    let nearest = if d(0, 2) < d(0, 1) { 2 } else { 1 };
    let other = 3 - nearest;
    say(&mut r.sims[0], "/invite");
    r.idle(700);
    assert!(panel_of(&r.sims[nearest as usize]).starts_with("p0 invites you to a party ("), "{:?}", panel_of(&r.sims[nearest as usize]));
    assert_eq!(panel_of(&r.sims[other as usize]), "", "only the nearest was asked");
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), &format!("[PARTY]: Invitation sent to p{nearest}."));
    // a player who is in a party is not the nearest free one: with the nearest taken, the next is asked (p0 is a leader once p{nearest} accepts, and p{other} is free)
    say(&mut r.sims[nearest as usize], "/accept");
    r.idle(700);
    say(&mut r.sims[0], "/invite");
    r.idle(700);
    assert!(panel_of(&r.sims[other as usize]).starts_with("p0 invites you to a party ("), "{:?}", panel_of(&r.sims[other as usize]));
    // nobody free left: told so
    say(&mut r.sims[other as usize], "/accept");
    r.idle(700);
    say(&mut r.sims[0], "/invite");
    r.idle(700);
    assert_eq!(log_of(&r.sims[0]).last().unwrap(), "[PARTY]: Nobody is near you to invite. Say whom: /invite NAME");
}

#[test]
fn an_invitation_is_told_once_not_every_second_and_the_client_counts_it_down() {
    let mut r = rig(server_scene(""), 2);
    r.idle(2500);
    say(&mut r.sims[0], "/invite p1");
    r.idle(800);
    let (heard0, secs0) = (panel_numbers(&r.sims[1]).heard, panel_numbers(&r.sims[1]).invite_secs);
    assert!(secs0 >= 29.0, "the invitation has about 30 s: {secs0}");
    r.idle(8000);
    let n = panel_numbers(&r.sims[1]);
    assert!(n.heard - heard0 <= 2, "the seconds are not news: {} panels in 8 s (the refresh every five seconds is one)", n.heard - heard0);
    assert!((7.0..=9.5).contains(&(secs0 - n.invite_secs)), "the client counts it down on its own clock: {secs0} -> {}", n.invite_secs);
    assert!(panel_of(&r.sims[1]).starts_with("p0 invites you to a party ("), "{:?}", panel_of(&r.sims[1]));
    // it runs out at 30 s on the server: the panel is cleared
    r.idle(24_000);
    assert_eq!(panel_of(&r.sims[1]), "");
    assert!(!panel_numbers(&r.sims[1]).invited);
}
