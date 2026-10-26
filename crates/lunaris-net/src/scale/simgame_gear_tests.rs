//! Worn gear that the other players see, end to end in memory (docs/GEAR_VISIBLE.md): the server works out what a player wears from its bag and tells it in the low bits of `anim` (what the player itself says in
//! `anim` does not count), the other player's puppet carries it in `NetPuppet.anim`, and the engine draws the parts of the puppet's mannequin that the mask shows. No art: names and boxes.
use super::bytes::W;
use super::client::NetClient;
use super::play::NetDriver;
use super::server::{NetServer, ServerConfig};
use super::simgame::SimGame;
use super::state::{self, Detail, EntityState};
use super::transport::{Link, SimEndpoint, SimNet};
use lunaris_json::{parse, Json};
use lunaris_sim::components::{MeshRenderer, NetAvatar, NetPlayer, NetPuppet};
use lunaris_sim::gear::WornPart;
use lunaris_sim::inventory::Inventory;
use lunaris_sim::{Command, Sim};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn prefab(s: &mut Sim, name: &str, def: &str) {
    s.apply(&Command::DefinePrefab { name: name.into(), def: parse(def).unwrap() }).unwrap();
}

// the items: 1 helmet, 2 vest, 3 pants, 4 boots; the places of the game: slots 12 to 15 of a bag of 16
const ITEMS: &str = "item helmet | Helmet | stack=1 class=1 power=35 dur=100\\nitem vest | Vest | stack=1 class=2 power=40 dur=100\\nitem pants | Pants | stack=1 class=3 power=25 dur=100\\nitem boots | Boots | stack=1 class=4 power=15 dur=100\\n";
const PLACES: &str = "slot 12 accepts=1 zones=head\\nslot 13 accepts=2 zones=body\\nslot 14 accepts=3 zones=legs\\nslot 15 accepts=4 zones=legs";

/// A scene to host: a ground, two places to start and an avatar that wears gear (a bag of 16 with four worn places, a `WornTell`) and has a body to hit.
fn server_scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    spawn(&mut s, 4, r#"{"NetHost":{"player":"Avatar","forward":"hit 100","static_every":3}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[0,0,-6]},"SpawnPoint":{}}"#);
    spawn(&mut s, 6, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    prefab(
        &mut s,
        "Avatar",
        &format!(
            r#"{{"components":{{"Name":{{"value":"Avatar"}},"Transform":{{}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},
            "HitPoints":{{"value":1000,"max":1000,"last":1000}},"Inventory":{{"size":16,"bag":12}},"Equipment":{{"slots":"{PLACES}"}},"WornTell":{{}},"Respawn":{{"delay":1,"avoid":80,"protect":0}},"NetAvatar":{{}}}},
           "children":[{{"components":{{"Name":{{"value":"Head"}},"LocalTransform":{{"pos":[0,1.6,0]}},"Weapon":{{"active":false,"spread":0}}}}}}]}}"#
        ),
    );
    s
}

/// A player's computer: its own body, and the other players as the puppet `Mate`: a mannequin (drawn) with a part for each of the four places (hidden), as the game's prefab has.
fn client_scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{},"Inventory":{"size":16},"NetPlayer":{"puppet":"Mate","camera":9,"eye":[0,1.6,0]}}"#);
    spawn(&mut s, 6, &format!(r#"{{"ItemTable":{{"text":"{ITEMS}"}}}}"#));
    spawn(&mut s, 9, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[0,1.6,0]}}"#);
    let part = |name: &str, place: u32| format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"LocalTransform":{{}},"MeshRenderer":{{"mesh":"cube","visible":false}},"WornPart":{{"place":{place}}}}}}}"#);
    let parts = [part("helmet", 0), part("vest", 1), part("pants", 2), part("boots", 3)].join(",");
    prefab(
        &mut s,
        "Mate",
        &format!(r#"{{"components":{{"Name":{{"value":"Mate"}},"Transform":{{}},"NetPuppet":{{}}}},"children":[{{"components":{{"Name":{{"value":"Man"}},"LocalTransform":{{}},"MeshRenderer":{{"mesh":"cube","visible":true}},"NetPuppet":{{}}}},"children":[{parts}]}}]}}"#),
    );
    s
}

struct Rig {
    net: SimNet,
    server: NetServer<SimEndpoint, SimGame>,
    sims: Vec<Sim>,
    drivers: Vec<NetDriver<SimEndpoint>>,
    now: u64,
}

fn rig(n: usize) -> Rig {
    let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.02, duplicate: 0.01 }, 7);
    let end = net.endpoint();
    let addr = end.addr();
    let server = NetServer::new(ServerConfig::default(), end, SimGame::new(server_scene()));
    let sims = (0..n).map(|_| client_scene()).collect();
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
                // (a client's game runs its own simulation: the gear of the puppets is drawn by the engine's step)
                s.step();
            }
        }
    }

    /// Put item `id` (0: nothing) into slot `k` of the bag of the player in slot `slot` of the server (with 100 points of durability).
    fn bag(&mut self, slot: u32, k: usize, id: u32, dur: f32) {
        let _ = self.server.game.sim.world.for_each::<(&NetAvatar, &mut Inventory), _>(|_, (n, inv)| {
            if n.slot == slot {
                (inv.id[k], inv.count[k], inv.dur[k]) = (id, (id != 0) as u32, dur);
            }
        });
    }
}

/// What a client sees of the other player: the mask on its puppet and which parts are drawn, as (place, drawn) sorted by place.
fn seen(s: &mut Sim) -> (u32, Vec<(u32, bool)>) {
    let mut mask = u32::MAX;
    let _ = s.world.for_each::<(&NetPuppet,), _>(|_, (p,)| {
        if p.class == 0 && p.id != 0 {
            mask = p.anim;
        }
    });
    let mut parts: Vec<(u32, bool)> = Vec::new();
    let _ = s.world.for_each::<(&WornPart, &MeshRenderer), _>(|_, (w, m)| parts.push((w.place, m.visible)));
    parts.sort();
    (mask, parts)
}

fn drawn_places(s: &mut Sim) -> Vec<u32> {
    seen(s).1.into_iter().filter(|p| p.1).map(|p| p.0).collect()
}

#[test]
fn what_a_player_wears_is_seen_on_its_puppet_by_the_other_player_and_only_what_is_really_in_the_places() {
    let mut r = rig(2);
    r.run(3000, |_, _| {});
    let (mask, parts) = seen(&mut r.sims[1]);
    assert_eq!(mask, 0, "nothing worn, nothing told");
    assert_eq!(parts.len(), 4, "the puppet has its four parts: {parts:?}");
    assert!(drawn_places(&mut r.sims[1]).is_empty());
    // a helmet and a vest go into the worn places of the player in slot 0 (a bag slot that is not worn changes nothing)
    r.bag(0, 3, 2, 100.0);
    r.run(400, |_, _| {});
    assert_eq!(seen(&mut r.sims[1]).0, 0, "a vest in a carried slot is in the bag, not on the body");
    r.bag(0, 12, 1, 100.0);
    r.bag(0, 13, 2, 100.0);
    r.run(400, |_, _| {});
    assert_eq!(seen(&mut r.sims[1]).0, 0b0011, "a helmet and a vest");
    assert_eq!(drawn_places(&mut r.sims[1]), vec![0, 1], "the helmet and the vest are drawn on the mannequin");
    // the player's own word does not count: it says it wears boots and pants (bits 2 and 3 of anim) and has none
    r.run(400, |sims, _| {
        let e = sims[0].world.entity(1).unwrap();
        sims[0].world.get_mut::<NetPlayer>(e).unwrap().anim = 0b1111100;
    });
    assert_eq!(seen(&mut r.sims[1]).0, 0b0011, "the server's mask, not the player's");
    assert_eq!(drawn_places(&mut r.sims[1]), vec![0, 1]);
    // boots and pants on, the vest off
    r.bag(0, 15, 4, 100.0);
    r.bag(0, 14, 3, 100.0);
    r.bag(0, 13, 0, 0.0);
    r.run(400, |_, _| {});
    assert_eq!(seen(&mut r.sims[1]).0, 0b1101);
    assert_eq!(drawn_places(&mut r.sims[1]), vec![0, 2, 3], "the vest is gone from the view, the pants and boots are on");
    // a vest that wears out in a fight is gone from the view: two hits of 20 on the body, the vest has 12 points
    r.bag(0, 13, 2, 12.0);
    r.run(200, |_, _| {});
    assert_eq!(drawn_places(&mut r.sims[1]), vec![0, 1, 2, 3]);
    let victim = {
        let mut uid = 0;
        let _ = r.server.game.sim.world.for_each_uid::<(&NetAvatar,), _>(|_, u, (n,)| {
            if n.slot == 0 {
                uid = u;
            }
        });
        uid
    };
    let mut ev: Vec<(String, u64, Option<[f32; 3]>)> = Vec::new();
    lunaris_sim::armor::absorb(&mut r.server.game.sim.world, victim, 20.0, 1, &mut ev);
    r.run(300, |_, _| {});
    assert_eq!(drawn_places(&mut r.sims[1]), vec![0, 1, 2, 3], "a damaged vest is still worn");
    lunaris_sim::armor::absorb(&mut r.server.game.sim.world, victim, 20.0, 1, &mut ev);
    assert!(ev.iter().any(|e| e.0 == "item_break"));
    r.run(300, |_, _| {});
    assert_eq!(drawn_places(&mut r.sims[1]), vec![0, 2, 3], "the vest broke and is gone from the view");
    // everything off
    for k in [12, 14, 15] {
        r.bag(0, k, 0, 0.0);
    }
    r.run(300, |_, _| {});
    let (mask, parts) = seen(&mut r.sims[1]);
    assert_eq!((mask, parts.iter().filter(|p| p.1).count()), (0, 0));
}

#[test]
fn a_mask_adds_nothing_to_a_record_and_a_far_player_is_told_none() {
    let rec = |anim: u16, d: Detail| {
        let mut s = EntityState { id: 7, class: 0, health: 200, flags: 5, anim, extra: 3 << 6, ..Default::default() };
        s.set_pos_m([10.0, 0.0, -4.0]);
        s.vel = [100, 0, -50];
        let mut w = W::new();
        state::write(&mut w, &s, d);
        w.buf
    };
    for d in [Detail::Full, Detail::Basic] {
        assert_eq!(rec(0, d).len(), rec(0b1111, d).len(), "a mask of four pieces is as long as none ({d:?})");
        assert_eq!(rec(0, d).len(), rec(0b111_1111, d).len(), "and seven");
    }
    let full = rec(0b1010, Detail::Full);
    let (s, d) = state::read(&mut super::bytes::R::new(&full)).unwrap();
    assert_eq!((d, s.anim), (Detail::Full, 0b1010), "a near player's record carries the mask");
    let basic = rec(0b1010, Detail::Basic);
    let (s, d) = state::read(&mut super::bytes::R::new(&basic)).unwrap();
    assert_eq!((d, s.anim), (Detail::Basic, 0), "a far player's record has no `anim`: the puppet shows no gear");
}
