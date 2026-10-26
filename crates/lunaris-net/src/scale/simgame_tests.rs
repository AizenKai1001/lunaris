//! The game server end to end, in memory over a bad network: a server running a scene's own simulation, players that see the world it replicates, build in it, shoot each other, die, get up again and
//! keep what they carry. No art: names and boxes.
use super::client::NetClient;
use super::play::{yaw_rot, NetDriver};
use super::server::{NetServer, ServerConfig};
use super::simgame::{seen_of, SimGame, SEEN};
use super::transport::{Link, SimEndpoint, SimNet};
use lunaris_json::{parse, Json};
use lunaris_sim::components::{BuildPiece, HitPoints, NetPlayer, NetPuppet, Transform};
use lunaris_sim::{Command, Sim};

fn spawn(s: &mut Sim, uid: u64, comps: &str) {
    let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
    s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
}

fn prefab(s: &mut Sim, name: &str, def: &str) {
    s.apply(&Command::DefinePrefab { name: name.into(), def: parse(def).unwrap() }).unwrap();
}

const RULES: &str = "rules step=90\\npiece foundation size=4,0.5,4 hp=500 free=1 anchor=1\\n";

/// A scene to host: a ground, the building rules, a crate that is replicated, two places to start and an avatar prefab with a weapon and a build tool on its head.
fn server_scene(extra_host: &str) -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 2, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[400,1,400]}}"#);
    spawn(&mut s, 3, &format!(r#"{{"BuildRules":{{"text":"{RULES}"}}}}"#));
    spawn(&mut s, 4, &format!(r#"{{"NetHost":{{"player":"Avatar","forward":"build_placed 100\ncorpse 100\nshot 100\nhit 100\nkill 100","static_every":3,"drop":"Pickup"{extra_host}}}}}"#));
    spawn(&mut s, 5, r#"{"Transform":{"pos":[5,0,5]},"NetEntity":{"class":3,"kind":7}}"#);
    spawn(&mut s, 21, r#"{"Transform":{"pos":[0,0,0]},"SpawnPoint":{}}"#);
    spawn(&mut s, 22, r#"{"Transform":{"pos":[0,0,-6]},"SpawnPoint":{}}"#);
    spawn(&mut s, 6, r#"{"ItemTable":{"text":"item bandage | Bandage | stack=10\nitem axe | Axe | stack=1\n"}}"#);
    prefab(
        &mut s,
        "Avatar",
        r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},
            "HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":12},"Hacker":{"aim":true,"speed":2},"Respawn":{"delay":1,"avoid":80,"protect":0,"corpse":"Corpse","corpse_time":30},"NetAvatar":{}},
           "children":[{"components":{"Name":{"value":"Head"},"LocalTransform":{"pos":[0,1.6,0]},
            "Weapon":{"damage":60,"spread":0,"recoil":[0,0],"rate":120,"auto":true,"infinite":true,"noise":0,"range":80},"BuildTool":{"reach":12},"Interactor":{"reach":3,"look":false}}}]}"#,
    );
    prefab(&mut s, "Corpse", r#"{"components":{"Name":{"value":"Corpse"},"NetEntity":{"class":3,"kind":9}}}"#);
    prefab(&mut s, "Pickup", r#"{"components":{"Name":{"value":"Pickup"},"Transform":{},"Interactable":{"kind":3,"prompt":"Take"},"NetEntity":{"class":1,"kind":20}}}"#);
    s
}

const DOOR: &str = r#"{"Transform":{"pos":[X,1.0,Z]},"Collider":{"shape":1,"size":[1,2,0.1]},"Interactable":{"kind":1,"prompt":"Open"},"Door":{"hinge":[-0.5,0,0],"angle_open":100,"speed":300},"NetEntity":{"class":3,"kind":12}}"#;

fn client_scene() -> Sim {
    let mut s = Sim::new(1);
    spawn(&mut s, 1, r#"{"Transform":{},"Velocity":{},"Inventory":{"size":12},"NetPlayer":{"puppet":"Mate","camera":9,"eye":[0,1.6,0],"kinds":"3 7 Crate\n3 1 Foundation\n3 9 Corpse\n3 12 DoorModel\n3 13 ChestModel\n1 20 PickupModel\n4 60 HeliModel\n4 50 DroneModel"}}"#);
    spawn(&mut s, 3, r#"{"NetContainer":{},"Inventory":{"size":8}}"#);
    // (the client has the game's item table too: its interface moves stacks by it)
    spawn(&mut s, 6, r#"{"ItemTable":{"text":"item bandage | Bandage | stack=10\nitem axe | Axe | stack=1\n"}}"#);
    // the camera of the player: a child of it, at eye height
    spawn(&mut s, 9, r#"{"Transform":{},"Parent":{"target":1},"LocalTransform":{"pos":[0,1.6,0]}}"#);
    for name in ["Mate", "Crate", "Foundation", "Corpse", "ChestModel", "PickupModel", "HeliModel", "DroneModel"] {
        prefab(&mut s, name, &format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"Transform":{{}},"NetPuppet":{{}}}}}}"#));
    }
    prefab(&mut s, "DoorModel", r#"{"components":{"Name":{"value":"DoorModel"},"Transform":{},"NetPuppet":{},"Door":{}}}"#);
    s
}

struct Rig {
    net: SimNet,
    server: NetServer<SimEndpoint, SimGame>,
    sims: Vec<Sim>,
    drivers: Vec<NetDriver<SimEndpoint>>,
    now: u64,
}

fn rig(game: SimGame, n: usize) -> Rig {
    rig_on(game, n, Link { latency_ms: 25, jitter_ms: 5, loss: 0.02, duplicate: 0.01 })
}

fn rig_on(game: SimGame, n: usize, link: Link) -> Rig {
    let net = SimNet::new(link, 7);
    let end = net.endpoint();
    let addr = end.addr();
    let server = NetServer::new(ServerConfig::default(), end, game);
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
            }
        }
    }
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

/// The puppets a client has: (class, kind, name, position, health).
fn puppets(s: &mut Sim) -> Vec<(u32, u32, String, [f32; 3], f32)> {
    let mut raw: Vec<(NetPuppet, Transform, u32)> = Vec::new();
    let _ = s.world.for_each::<(&NetPuppet, &Transform, &lunaris_sim::components::Name), _>(|_, (p, t, n)| raw.push((*p, *t, n.value.0)));
    let mut out: Vec<(u32, u32, String, [f32; 3], f32)> = raw.into_iter().map(|(p, t, name)| (p.class, p.kind, s.world.strings().get(name).to_string(), t.pos, p.health)).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    out
}

/// The events of a name that have happened in a client's simulation (the test does not step the clients: the driver puts the server's events among the pending ones).
fn events(s: &Sim, name: &str) -> usize {
    s.events.iter().filter(|e| e.name == name).count() + s.scripts.events.iter().filter(|e| e.0 == name).count()
}

#[test]
fn the_players_see_the_world_the_server_runs_and_each_other() {
    let mut r = rig(SimGame::new(server_scene("")), 2);
    r.run(3000, |_, _| {});
    let (a, ta) = local(&r.sims[0]);
    let (b, tb) = local(&r.sims[1]);
    assert!(a.connected && b.connected && a.id != b.id, "{a:?} {b:?}");
    // the server chose where each starts: a spawn point (the scene's places), not the same one
    let spots = [[0.0, 0.0, 0.0], [0.0, 0.0, -6.0]];
    assert!(spots.contains(&ta.pos) && spots.contains(&tb.pos) && ta.pos != tb.pos, "{:?} {:?}", ta.pos, tb.pos);
    // what each client holds: the other player, and the crate of the scene, replicated, shown by the prefab the kinds text names
    for i in 0..2 {
        let p = puppets(&mut r.sims[i]);
        assert_eq!(p.len(), 2, "client {i}: {p:?}");
        assert_eq!((p[0].0, p[0].2.as_str()), (0, "Mate"), "the other player: {p:?}");
        assert_eq!((p[1].0, p[1].1, p[1].2.as_str()), (3, 7, "Crate"), "{p:?}");
        let d = ((p[1].3[0] - 5.0).powi(2) + (p[1].3[2] - 5.0).powi(2)).sqrt();
        assert!(d < 0.1, "the crate is where the server has it: {:?}", p[1].3);
    }
    // a thing that is gone on the server is gone on the clients (the still things are looked at every few ticks)
    lunaris_sim::gameplay::despawn_trees(&mut r.server.game.sim.world, vec![5]);
    r.run(7000, |_, _| {});
    assert_eq!(puppets(&mut r.sims[0]).len(), 1, "only the other player is left");
}

#[test]
fn a_player_builds_through_commands_and_everybody_sees_the_piece_and_hears_of_it() {
    let mut r = rig(SimGame::new(server_scene("")), 2);
    r.run(2500, |_, _| {});
    // player 0 looks down at the ground ahead and says what to build, then to build it
    set_local(&mut r.sims[0], |np, _| {
        np.pitch = -0.5;
        np.cmd = lunaris_ecs::Sym(0);
    });
    let say = |sim: &mut Sim, text: &str| {
        let sym = sim.world.strings_mut().intern(text);
        set_local(sim, |np, _| np.cmd = lunaris_ecs::Sym(sym));
    };
    say(&mut r.sims[0], "build:foundation");
    r.run(400, |_, _| {});
    say(&mut r.sims[0], "place");
    r.run(2500, |_, _| {});
    let mut pieces = 0;
    let _ = r.server.game.sim.world.for_each::<(&BuildPiece,), _>(|_, _| pieces += 1);
    assert_eq!(pieces, 1, "the server's avatar built it with its own tool");
    for i in 0..2 {
        let p = puppets(&mut r.sims[i]);
        let f: Vec<_> = p.iter().filter(|x| x.0 == 3 && x.1 == 1).collect();
        assert_eq!(f.len(), 1, "client {i} sees the foundation (class 3, kind 1 = the first piece of the rules): {p:?}");
        assert_eq!(f[0].2, "Foundation");
    }
    assert_eq!(events(&r.sims[0], "build_placed"), 1, "and the event of the server's simulation happened in the player's own");
    // a command that is nobody's reaches the avatar, for the game's scripts
    say(&mut r.sims[1], "wave");
    r.run(300, |_, _| {});
}

#[test]
fn a_player_uses_a_door_with_the_use_button_and_everybody_sees_it_open() {
    let mut r = rig(SimGame::new(server_scene("")), 2);
    r.run(2500, |_, _| {});
    // a door two metres ahead of player 0 (it looks down -z from its place)
    let (_, ta) = local(&r.sims[0]);
    let text = DOOR.replace('X', &format!("{}", ta.pos[0])).replace('Z', &format!("{}", ta.pos[2] - 2.0));
    spawn(&mut r.server.game.sim, 901, &text);
    r.run(1500, |_, _| {});
    for i in 0..2 {
        let p = puppets(&mut r.sims[i]);
        assert!(p.iter().any(|x| x.2 == "DoorModel"), "client {i} sees the door: {p:?}");
    }
    let open = |r: &mut Rig, i: usize| {
        let mut out = false;
        let _ = r.sims[i].world.for_each::<(&lunaris_sim::components::Door, &NetPuppet), _>(|_, (d, _)| out = d.open);
        out
    };
    assert!(!open(&mut r, 1), "shut at first");
    // the use button is bit 3 of the flags: press for a moment
    r.run(300, |sims, _| set_local(&mut sims[0], |np, _| np.flags = 8));
    set_local(&mut r.sims[0], |np, _| np.flags = 0);
    r.run(1500, |_, _| {});
    let d = *r.server.game.sim.world.get::<lunaris_sim::components::Door>(r.server.game.sim.world.entity(901).unwrap()).unwrap();
    assert!(d.open, "the avatar used it on the server: {d:?}");
    assert!(open(&mut r, 1) && open(&mut r, 0), "and both clients' copies of the door are open");
    // (shutting it again is the same press, but the open leaf has swung out of the line of sight of where the player stands: docs/INTERACT.md)
}

const CHEST: &str = r#"{"Transform":{"pos":[X,1.6,Z]},"Collider":{"shape":1,"size":[1,1,1]},"Interactable":{"kind":2,"prompt":"Search"},"Inventory":{"size":8,"fill":"bandage 5"},"NetEntity":{"class":3,"kind":13}}"#;

/// How many things are in an inventory of a client's entity.
fn total(s: &Sim, uid: u64) -> u32 {
    let inv = s.world.get::<lunaris_sim::inventory::Inventory>(s.world.entity(uid).unwrap()).unwrap();
    (0..48).map(|k| if inv.id[k] != 0 { inv.count[k] } else { 0 }).sum()
}

fn say(sim: &mut Sim, text: &str, args: [f32; 4]) {
    let sym = sim.world.strings_mut().intern(text);
    set_local(sim, |np, _| {
        np.cmd = lunaris_ecs::Sym(sym);
        np.cmd_args = args;
    });
}

#[test]
fn bags_and_containers_are_told_to_their_owner_and_items_move_by_command() {
    let mut r = rig(SimGame::new(server_scene("")), 2);
    r.run(2500, |_, _| {});
    let (_, ta) = local(&r.sims[0]);
    let text = CHEST.replace('X', &format!("{}", ta.pos[0])).replace('Z', &format!("{}", ta.pos[2] - 2.0));
    spawn(&mut r.server.game.sim, 902, &text);
    r.run(800, |_, _| {});
    assert_eq!(total(&r.sims[0], 1), 0, "the bag starts empty");
    // player 0 opens the chest with the use button: it is told what is in it, the other player is not
    r.run(300, |sims, _| set_local(&mut sims[0], |np, _| np.flags = 8));
    set_local(&mut r.sims[0], |np, _| np.flags = 0);
    r.run(1500, |_, _| {});
    let c = |r: &Rig, i: usize| {
        let e = r.sims[i].world.entity(3).unwrap();
        (r.sims[i].world.get::<lunaris_sim::components::NetContainer>(e).unwrap().open, total(&r.sims[i], 3))
    };
    assert_eq!(c(&r, 0), (true, 5), "the container's contents arrived in the NetContainer's inventory");
    assert_eq!(c(&r, 1), (false, 0), "the other player is not told");
    // move three of them into the bag (place 100 + slot 0 is the container's first slot, place 0 + slot 0 the bag's)
    say(&mut r.sims[0], "inv_move", [100.0, 0.0, 3.0, 0.0]);
    r.run(1500, |_, _| {});
    assert_eq!((total(&r.sims[0], 1), c(&r, 0).1), (3, 2), "three in the bag, two left in the chest");
    // drop two of them: a pickup lies in front of the player and the other player sees it
    say(&mut r.sims[0], "inv_drop", [0.0, 2.0, 0.0, 0.0]);
    r.run(2500, |_, _| {});
    assert_eq!(total(&r.sims[0], 1), 1);
    let p = puppets(&mut r.sims[1]);
    assert!(p.iter().any(|x| x.0 == 1 && x.1 == 20 && x.2 == "PickupModel"), "the dropped thing is on the ground for everybody: {p:?}");
    // walking away shuts the chest, and the client is told
    r.run(3000, |sims, _| set_local(&mut sims[0], |_, t| t.pos[0] += 0.05));
    assert_eq!(c(&r, 0), (false, 0), "the container shut by itself and was emptied on the client");
    assert_eq!(total(&r.sims[0], 1), 1, "the bag is still told");
}

/// What the server knows of the one avatar of a test: (hit points, bleeding, bandages in the bag, axes in the bag).
fn body(r: &mut Rig) -> (f32, f32, u32, u32) {
    let mut out = (0.0, 0.0, 0, 0);
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &HitPoints, &lunaris_sim::components::Vitals, &lunaris_sim::inventory::Inventory), _>(|_, (_, h, v, inv)| {
        out = (h.value, v.bleeding, (0..12).filter(|&k| inv.id[k] == 1).map(|k| inv.count[k]).sum(), (0..12).filter(|&k| inv.id[k] == 2).map(|k| inv.count[k]).sum());
    });
    out
}

#[test]
fn a_player_uses_an_item_of_its_bag_the_server_does_what_the_item_says_and_uses_one_up() {
    let mut sim = server_scene(r#","uses":"use bandage heal=25 bandage=1 # stops the bleeding\nuse axe keep=1""#);
    let mut ids = vec!["0"; 48];
    let mut counts = vec!["0"; 48];
    (ids[0], counts[0]) = ("1", "3");
    (ids[1], counts[1]) = ("2", "1");
    let (ids, counts) = (ids.join(","), counts.join(","));
    prefab(
        &mut sim,
        "Avatar",
        &format!(
            r#"{{"components":{{"Name":{{"value":"Avatar"}},"Transform":{{}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},
            "HitPoints":{{"value":40,"max":100,"last":100}},"Vitals":{{"bleeding":0.5,"heal_threshold":101}},"Inventory":{{"size":12,"id":[{ids}],"count":[{counts}]}},"NetAvatar":{{}}}}}}"#
        ),
    );
    let mut r = rig(SimGame::new(sim), 1);
    r.run(2500, |_, _| {});
    let (hp0, bleed0, bandages, axes) = body(&mut r);
    assert!(bleed0 > 0.0 && bandages == 3 && axes == 1, "it bleeds and carries what it should: {:?}", body(&mut r));
    // the bandage (slot 0): heals, stops the bleeding, one is used up
    say(&mut r.sims[0], "inv_use", [0.0, 0.0, 0.0, 0.0]);
    r.run(600, |_, _| {});
    let (hp1, bleed1, bandages, _) = body(&mut r);
    assert_eq!((bleed1, bandages), (0.0, 2), "the bleeding stopped and one bandage is gone");
    assert!(hp1 > hp0 + 15.0, "it healed: {hp0} -> {hp1}");
    assert_eq!(total(&r.sims[0], 1), 3, "the player is told its bag: two bandages and the axe");
    // a tool is not used up, an item with no rule and an empty slot do nothing
    say(&mut r.sims[0], "inv_use", [1.0, 0.0, 0.0, 0.0]);
    r.run(300, |_, _| {});
    say(&mut r.sims[0], "inv_use", [7.0, 0.0, 0.0, 0.0]);
    r.run(300, |_, _| {});
    assert_eq!((body(&mut r).2, body(&mut r).3), (2, 1));
}

#[test]
fn a_card_dragged_in_the_clients_interface_moves_the_stack_on_the_server_and_the_server_has_the_last_word() {
    let mut r = rig(SimGame::new(server_scene("")), 1);
    r.run(2500, |_, _| {});
    let (_, ta) = local(&r.sims[0]);
    let text = CHEST.replace('X', &format!("{}", ta.pos[0])).replace('Z', &format!("{}", ta.pos[2] - 2.0));
    spawn(&mut r.server.game.sim, 902, &text);
    r.run(800, |_, _| {});
    r.run(300, |sims, _| set_local(&mut sims[0], |np, _| np.flags = 8));
    set_local(&mut r.sims[0], |np, _| np.flags = 0);
    r.run(1500, |_, _| {});
    assert_eq!(total(&r.sims[0], 3), 5, "the chest is open on the client");
    // the interface: slot 0 of the chest (entity 3) and slot 4 of the bag (entity 1), each with its card
    let widgets = [
        (70, r#"{"Ui":{"pos":[0.6,0.1],"size":[0.08,0.08]},"UiDrop":{},"InvSlot":{"inv":3,"index":0}}"#),
        (71, r#"{"Ui":{"pos":[0,0],"size":[0.06,0.06],"button":true},"UiStyle":{},"UiDrag":{"slot":70},"InvCard":{"slot":70}}"#),
        (72, r#"{"Ui":{"pos":[0.2,0.1],"size":[0.08,0.08]},"UiDrop":{},"InvSlot":{"inv":1,"index":4}}"#),
        (73, r#"{"Ui":{"pos":[0,0],"size":[0.06,0.06],"button":true},"UiStyle":{},"UiDrag":{"slot":72},"InvCard":{"slot":72}}"#),
    ];
    for (uid, comps) in widgets {
        spawn(&mut r.sims[0], uid, comps);
    }
    // the player drags the stack of the chest onto the bag's slot 4
    let card = r.sims[0].world.entity(71).unwrap();
    r.sims[0].world.get_mut::<lunaris_sim::components::UiDrag>(card).unwrap().slot = lunaris_ecs::Uid(72);
    // (the rig does not step the clients' simulations: the interface runs when the client's game does)
    r.sims[0].step();
    r.run(1500, |_, _| {});
    let on_server = {
        let mut out = 0;
        let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &lunaris_sim::inventory::Inventory), _>(|_, (n, inv)| {
            if n.slot == 0 {
                out = (0..12).filter(|&k| inv.id[k] != 0).map(|k| inv.count[k]).sum::<u32>();
            }
        });
        out
    };
    assert_eq!(on_server, 5, "the server moved the stack into the player's bag");
    assert_eq!((total(&r.sims[0], 1), total(&r.sims[0], 3)), (5, 0), "and the client's screen agrees");
}

#[test]
fn a_shot_is_the_avatars_weapon_it_hurts_kills_leaves_a_corpse_and_the_dead_get_up_somewhere_else() {
    let mut r = rig(SimGame::new(server_scene("")), 2);
    r.run(2500, |_, _| {});
    // a third place, far off: the dead will be sent to the one farthest from the other player
    spawn(&mut r.server.game.sim, 900, r#"{"Transform":{"pos":[0,0,60]},"SpawnPoint":{}}"#);
    let (_, ta) = local(&r.sims[0]);
    let (_, tb) = local(&r.sims[1]);
    // player 0 aims at the chest of player 1 and holds the trigger
    let d = [tb.pos[0] - ta.pos[0], tb.pos[1] + 1.0 - (ta.pos[1] + 1.6), tb.pos[2] - ta.pos[2]];
    let yaw = (-d[0]).atan2(-d[2]);
    let pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt());
    r.run(2000, |sims, _| {
        set_local(&mut sims[0], |np, t| {
            np.pitch = pitch;
            np.flags = 1;
            t.rot = yaw_rot(yaw);
        });
    });
    set_local(&mut r.sims[0], |np, _| np.flags = 0);
    let victim_hp = |r: &mut Rig| {
        let mut out = None;
        let _ = r.server.game.sim.world.for_each::<(&HitPoints, &lunaris_sim::components::NetAvatar), _>(|_, (h, n)| {
            if n.slot == 1 {
                out = Some(*h);
            }
        });
        out.unwrap()
    };
    let hp = victim_hp(&mut r);
    assert!(hp.dead || hp.value < 100.0, "the shots landed on the server's avatar of player 1: {hp:?}");
    r.run(4000, |_, _| {});
    // the corpse was made where it fell, and told to the players
    assert!(events(&r.sims[0], "corpse") >= 1, "the corpse event reached the shooter");
    let seen = puppets(&mut r.sims[0]).iter().filter(|p| p.2 == "Corpse").count();
    assert_eq!(seen, 1, "the shooter sees the corpse");
    // the forwarded events say what they are about, by the server's number that `NetPlayer.id` and `NetPuppet.id` hold: a `shot` is about the shooter, a `hit` and a `kill` about the victim, so each
    // client can tell "I was hit" from "someone near me was hit"
    let (a, _) = local(&r.sims[0]);
    let (b, _) = local(&r.sims[1]);
    assert!(a.id != 0 && b.id != 0 && a.id != b.id);
    let uids = |r: &Rig, i: usize, name: &str| -> Vec<u64> { r.sims[i].scripts.events.iter().filter(|e| e.0 == name).map(|e| e.1).collect() };
    let shots = uids(&r, 0, "shot");
    assert!(!shots.is_empty() && shots.iter().all(|u| *u == a.id as u64), "the shooter's own client: shots are about itself: {shots:?} (it is {})", a.id);
    let hits_seen_by_victim = uids(&r, 1, "hit");
    assert!(!hits_seen_by_victim.is_empty() && hits_seen_by_victim.iter().all(|u| *u == b.id as u64), "the victim's client: hits are about itself: {hits_seen_by_victim:?} (it is {})", b.id);
    assert!(uids(&r, 0, "hit").iter().all(|u| *u == b.id as u64) && !uids(&r, 0, "hit").is_empty(), "the shooter's client: hits are about the other player");
    assert!(uids(&r, 1, "shot").iter().all(|u| *u == a.id as u64) && !uids(&r, 1, "shot").is_empty(), "the victim's client: shots are about the shooter, not itself");
    let kills = uids(&r, 1, "kill");
    assert!(kills.iter().all(|u| *u == b.id as u64) && !kills.is_empty(), "{kills:?}");
    // an event about nothing replicated (the world's own) is 0
    assert!(uids(&r, 0, "build_placed").iter().all(|u| *u == 0), "{:?}", uids(&r, 0, "build_placed"));
    // the dead player was put back on its feet at the farthest place and told to go there
    let (nb, tb2) = local(&r.sims[1]);
    assert!(nb.corrections >= 1, "{nb:?}");
    assert!((tb2.pos[2] - 60.0).abs() < 2.0, "player 1 got up at the far place: {:?}", tb2.pos);
    let hp = victim_hp(&mut r);
    assert!(!hp.dead && hp.value == 100.0, "{hp:?}");
}

#[test]
fn what_a_player_keeps_is_given_back_and_the_world_is_saved_and_loaded() {
    let dir = std::env::temp_dir().join(format!("lunaris_simgame_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("world.json");
    let host = format!(r#","save":"{}","persist":"Transform HitPoints""#, file.to_string_lossy().replace('\\', "/"));
    let mut sim = server_scene(&host);
    sim.asset_root = None;
    let mut r = rig(SimGame::new(sim), 1);
    r.run(2500, |_, _| {});
    // the player walks a few metres away and leaves
    let start = local(&r.sims[0]).1.pos;
    r.run(1200, |sims, _| {
        set_local(&mut sims[0], |_, t| {
            if t.pos[0] < start[0] + 5.0 {
                t.pos[0] += 0.05;
            }
        });
    });
    let there = local(&r.sims[0]).1.pos;
    assert!(there[0] > start[0] + 3.0, "{start:?} {there:?}");
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    assert!(r.server.game.save_now(), "the world is written");
    assert!(file.is_file());
    // a new server of the same scene loads it, and the player who comes back gets their place
    let mut sim2 = server_scene(&host);
    sim2.asset_root = None;
    let mut r2 = rig(SimGame::new(sim2), 1);
    r2.run(3000, |_, _| {});
    let back = local(&r2.sims[0]).1.pos;
    assert!((back[0] - there[0]).abs() < 1.0 && (back[2] - there[2]).abs() < 1.0, "the same name gets the same place: left at {there:?}, back at {back:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_character_that_nobody_comes_back_for_is_let_go_after_its_days() {
    // `NetHost.character_days`: a stored character is kept that long after its player left, then let go at the next save (the save and the memory it needs grow with the number of names otherwise); 0 keeps it for ever;
    // a character saved before characters had times starts to age when it is loaded
    let dir = std::env::temp_dir().join(format!("lunaris_chardays_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("world.json");
    let make = |days: f32| {
        let host = format!(r#","save":"{}","persist":"Transform HitPoints","character_days":{days}"#, file.to_string_lossy().replace('\\', "/"));
        let mut sim = server_scene(&host);
        sim.asset_root = None;
        sim
    };
    let mut r = rig(SimGame::new(make(2.0)), 1);
    r.run(2500, |_, _| {});
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    let now = super::server::unix_now();
    assert_eq!(r.server.game.stored.len(), 1, "the player who left has a stored character");
    let seen = seen_of(r.server.game.stored.values().next().unwrap()).expect("with the time it left");
    assert!(seen + 60 >= now && seen <= now + 1, "the time is now-ish: {seen} against {now}");
    assert_eq!(r.server.game.expire_characters_at(now + 86_400), 0, "after a day it is still kept (2 days)");
    assert_eq!(r.server.game.stored.len(), 1);
    assert_eq!(r.server.game.expire_characters_at(now + 3 * 86_400), 1, "after three days it is let go");
    assert!(r.server.game.stored.is_empty());
    // the save of a server whose character has gone has none of it, and a character with no time, in the file of an older server, is given one
    assert!(r.server.game.save_now());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains(SEEN), "the file holds no character time: {}", text.len());
    let mut r = rig(SimGame::new(make(2.0)), 1);
    r.run(2500, |_, _| {});
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    assert!(r.server.game.save_now());
    // (the file as an older server wrote it: no times on the characters, and no checksum, which the changed text would not match)
    let mut json = lunaris_json::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut kept_before = 0;
    if let Json::Obj(top) = &mut json {
        top.retain(|(k, _)| k != "checksum");
        for (k, v) in top.iter_mut() {
            if let (true, Json::Obj(players)) = (k == "players", v) {
                kept_before = players.len();
                for (_, c) in players.iter_mut() {
                    if let Json::Obj(members) = c {
                        members.retain(|(n, _)| n != SEEN);
                    }
                }
            }
        }
    }
    assert_eq!(kept_before, 1);
    std::fs::write(&file, json.to_string_pretty().unwrap()).unwrap();
    let mut old = SimGame::new(make(2.0));
    old.start();
    assert_eq!(old.stored.len(), 1, "an older save's character is kept (the checksum is not needed for a file that has none)");
    let stamped = seen_of(old.stored.values().next().unwrap()).expect("and given a time");
    assert!(stamped + 60 >= super::server::unix_now());
    // 0 days: for ever
    let mut forever = SimGame::new(make(0.0));
    forever.start();
    forever.stored.insert("somebody".to_string(), vec![(SEEN.to_string(), Json::Int(1))]);
    assert_eq!(forever.expire_characters_at(super::server::unix_now()), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// How many hits the server's avatar of the player in `slot` has taken (the `hit` events of the simulation: a victim that dies is back on its feet a second later, so health says little).
fn hits_on(r: &mut Rig, slot: u32) -> usize {
    let mut uid = None;
    let _ = r.server.game.sim.world.for_each_uid::<(&lunaris_sim::components::NetAvatar,), _>(|_, u, (n,)| {
        if n.slot == slot {
            uid = Some(u);
        }
    });
    let uid = uid.unwrap();
    r.server.game.sim.events.iter().filter(|e| e.name == "hit" && e.uid == uid).count()
}

/// Player 0 stands and shoots at the moment it sees player 1, who runs across its line of fire at 12 m/s eight metres ahead, cross it, on a link with 80 ms each way (so 100 ms of interpolation and 80 ms of half
/// a round trip: the player 0 sees where the runner was about 0.18 s ago, 2 m behind where it is). Returns how many times the runner was hit.
fn shot_at_a_runner(host: &str, lead_ms: u64) -> usize {
    let mut r = rig_on(SimGame::new(server_scene(host)), 2, Link { latency_ms: 80, jitter_ms: 0, loss: 0.0, duplicate: 0.0 });
    r.run(3000, |_, _| {});
    let a_pos = local(&r.sims[0]).1.pos;
    // the runner walks to where it starts, in hops that the server's speed limit lets through
    let b_start = [a_pos[0] - 6.0, 0.0, a_pos[2] - 8.0];
    for _ in 0..6 {
        let cur = local(&r.sims[1]).1.pos;
        let d = [b_start[0] - cur[0], b_start[2] - cur[2]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        let k = if len > 6.0 { 6.0 / len } else { 1.0 };
        set_local(&mut r.sims[1], |_, t| {
            t.pos[0] += d[0] * k;
            t.pos[2] += d[1] * k;
        });
        r.run(700, |_, _| {});
    }
    let at = local(&r.sims[1]).1.pos;
    assert!((at[0] - b_start[0]).abs() < 0.5 && (at[2] - b_start[2]).abs() < 0.5, "the runner is at the start of its run: {at:?} for {b_start:?}");
    // (the start of the run falls at another moment of the server's tick each time: the shot should land wherever)
    r.run(lead_ms, |_, _| {});
    let (mut fired, mut hold) = (false, 0u32);
    r.run(1600, |sims, _| {
        set_local(&mut sims[1], |_, t| t.pos[0] += 0.12);
        // player 0 shoots when the runner, as it sees it, is in front of it
        let x = puppets(&mut sims[0]).iter().find(|p| p.0 == 0).map(|p| p.3[0]);
        if !fired && x.is_some_and(|x| x >= a_pos[0]) {
            fired = true;
            hold = 4;
        }
        set_local(&mut sims[0], |np, t| {
            np.pitch = 0.0;
            t.rot = yaw_rot(0.0);
            np.flags = u32::from(hold > 0);
        });
        hold = hold.saturating_sub(1);
    });
    assert!(fired, "player 0 saw the runner cross its line");
    hits_on(&mut r, 1)
}

#[test]
fn a_shot_hits_what_the_shooter_saw_on_its_screen_and_with_lag_compensation_off_it_is_decided_at_the_present() {
    for lead in [0, 7, 14, 21, 28] {
        let on = shot_at_a_runner("", lead);
        assert_eq!(on, 1, "the runner was in the sights when the trigger was pulled: one hit (the run started {lead} ms into a tick)");
    }
    let off = shot_at_a_runner(r#","lag_comp":0"#, 0);
    assert_eq!(off, 0, "decided at the present the round goes behind the runner");
    // a limit of 20 ms is almost no compensation: the shot is behind the runner again
    let short = shot_at_a_runner(r#","lag_comp":20"#, 0);
    assert_eq!(short, 0, "the compensation is at most `lag_comp`");
}

/// A helicopter on the ground three metres in front of the first place: a two metre box with the pilot's seat, an engine that is off and rotors at speed.
const HELI: &str = r#"{"Transform":{"pos":[0,1.0,-3],"scale":[2,2,2]},"Collider":{"shape":1,"size":[1,1,1]},"RigidBody":{"mass":2000},"Velocity":{},"AngularVelocity":{"damping":0},
    "HitPoints":{"value":1000,"max":1000,"last":1000},"Helicopter":{"engine":false,"spool":1},"Interactable":{"kind":0,"prompt":"Fly"},
    "Seat":{"offset":[0,0.5,0.2],"exit":[2.5,-0.5,0],"drive":1},"NetEntity":{"class":4,"kind":60}}"#;

/// The first player stands at the origin looking down -z (where the helicopter is) unless it is riding.
fn stand(sims: &mut [Sim], flags: u32) {
    set_local(&mut sims[0], |np, t| {
        if np.riding == 0 {
            t.pos = [0.0, 0.0, 0.0];
            t.rot = yaw_rot(0.0);
        }
        np.pitch = 0.0;
        np.flags = flags;
    });
}

fn seated_players(r: &mut Rig) -> usize {
    let mut n = 0;
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::Seated,), _>(|_, _| n += 1);
    n
}

fn heli_puppet(s: &mut Sim) -> [f32; 3] {
    puppets(s).iter().find(|p| p.0 == 4).map(|p| p.3).expect("the helicopter is shown")
}

#[test]
fn a_player_sits_in_the_helicopter_flies_it_is_not_let_out_in_the_air_and_gets_out_on_the_ground() {
    let mut sim = server_scene("");
    spawn(&mut sim, 60, HELI);
    let mut r = rig(SimGame::new(sim), 2);
    r.run(3000, |_, _| {});
    // walk up to it (the server's speed limit is an eye on the hops) and use it
    r.run(800, |sims, _| stand(sims, 0));
    r.run(300, |sims, _| stand(sims, 8));
    r.run(800, |sims, _| stand(sims, 0));
    assert_eq!(seated_players(&mut r), 1, "the server has the player in the seat");
    let (np, _) = local(&r.sims[0]);
    assert!(np.riding != 0, "and told it so: {np:?}");
    // the helicopter's own place
    let on_ground = heli_puppet(&mut r.sims[1]);
    assert!(on_ground[1] < 2.0, "{on_ground:?}");
    // collective up: it climbs and takes the player with it (the other player sees both)
    set_local(&mut r.sims[0], |np, _| np.axes = [0.0, 0.3, 0.0, 0.0]);
    r.run(2500, |_, _| {});
    set_local(&mut r.sims[0], |np, _| np.axes = [0.0; 4]);
    r.run(700, |_, _| {});
    let up = heli_puppet(&mut r.sims[1]);
    let (np, t) = local(&r.sims[0]);
    assert!(up[1] > 4.0, "the helicopter climbed under the player's hands: {up:?}");
    assert!((t.pos[1] - up[1]).abs() < 2.5 && np.corrections <= 1, "the player rides with it: player {:?} helicopter {up:?} ({np:?})", t.pos);
    let mates: Vec<[f32; 3]> = puppets(&mut r.sims[1]).iter().filter(|p| p.0 == 0).map(|p| p.3).collect();
    assert_eq!(mates.len(), 1);
    assert!((mates[0][1] - up[1]).abs() < 2.5, "the other player sees the pilot up in the air too: {:?} {up:?}", mates[0]);
    // in the air the use button does not get it out
    r.run(300, |sims, _| stand(sims, 8));
    r.run(600, |sims, _| stand(sims, 0));
    assert!(local(&r.sims[0]).0.riding != 0 && seated_players(&mut r) == 1, "40 m is no place to get out of: here {:?}", heli_puppet(&mut r.sims[1]));
    // down, gently
    set_local(&mut r.sims[0], |np, _| np.axes = [0.0, -0.3, 0.0, 0.0]);
    let mut waited = 0;
    while heli_puppet(&mut r.sims[1])[1] > 2.2 && waited < 12000 {
        r.run(200, |_, _| {});
        waited += 200;
    }
    set_local(&mut r.sims[0], |np, _| np.axes = [0.0; 4]);
    r.run(800, |_, _| {});
    let down = heli_puppet(&mut r.sims[1]);
    assert!(down[1] < 3.0, "it came down: {down:?} after {waited} ms");
    // out: back on its feet beside the helicopter
    r.run(300, |sims, _| stand(sims, 8));
    r.run(1000, |sims, _| stand(sims, 0));
    let (np, t) = local(&r.sims[0]);
    assert_eq!(np.riding, 0, "got out: {np:?}");
    assert_eq!(seated_players(&mut r), 0);
    assert!(t.pos[1] < 2.0 && (t.pos[0] - down[0]).abs() < 4.0, "on the ground beside the helicopter: {:?} {down:?}", t.pos);
}

#[test]
fn the_messages_of_a_vehicle_round_trip() {
    use super::simgame::{control, parse_control, parse_ride, ride};
    let c = parse_control(&control([1.0, -1.0, 0.5, 0.0])).unwrap();
    assert!((c[0] - 1.0).abs() < 0.01 && (c[1] + 1.0).abs() < 0.01 && (c[2] - 0.5).abs() < 0.01 && c[3] == 0.0, "{c:?}");
    assert_eq!(parse_control(&control([f32::NAN, 9.0, -9.0, 0.0])).map(|c| c.map(|v| v.round())), Some([0.0, 1.0, -1.0, 0.0]), "a bad number is no hand movement and the rest is held to the stick's travel");
    assert_eq!(parse_ride(&ride(7, [0.0, 0.5, 0.25])), Some((7, [0.0, 0.5, 0.25])));
    assert_eq!(parse_ride(&ride(0, [0.0; 3])), Some((0, [0.0; 3])));
    assert!(parse_ride(&[0x45, 1, 2]).is_none() && parse_control(&[0x04, 1]).is_none() && parse_control(&[0x03, 0, 0, 0, 0]).is_none());
}

/// A machine floating ten metres ahead of the first place, one second to hack, that the goggles can take.
const DRONE: &str = r#"{"Transform":{"pos":[0,1.5,-9]},"Collider":{"shape":0,"size":[0.9,0.9,0.9]},"HitPoints":{"value":40,"max":40},"Team":{"id":2},"Velocity":{},
    "Hackable":{"difficulty":1,"range":25,"duration":30,"speed":6,"accel":30},"NetEntity":{"class":4,"kind":50}}"#;

fn camera_offset(s: &Sim) -> [f32; 3] {
    s.world.get::<lunaris_sim::components::LocalTransform>(s.world.entity(9).unwrap()).unwrap().pos
}

fn machine_puppet(s: &mut Sim) -> [f32; 3] {
    puppets(s).iter().find(|p| p.0 == 4 && p.1 == 50).map(|p| p.3).expect("the machine is shown")
}

#[test]
fn a_player_hacks_a_machine_it_looks_at_sees_through_it_drives_it_and_gives_it_back() {
    let mut sim = server_scene("");
    spawn(&mut sim, 70, DRONE);
    let mut r = rig(SimGame::new(sim), 2);
    r.run(3000, |_, _| {});
    // player 0 stands at the origin looking down -z, at the machine; the goggles' key (32) is held until it is taken
    r.run(800, |sims, _| stand(sims, 0));
    let start = machine_puppet(&mut r.sims[1]);
    r.run(1500, |sims, _| stand(sims, 32));
    r.run(600, |sims, _| stand(sims, 0));
    let (np, _) = local(&r.sims[0]);
    assert!(np.viewing != 0, "the server said that the machine is the player's: {np:?}");
    let machine_hacked = |r: &mut Rig| {
        let mut out = None;
        let _ = r.server.game.sim.world.for_each_uid::<(&lunaris_sim::components::Hackable,), _>(|_, uid, (k,)| {
            if uid == 70 {
                out = Some(k.hacked);
            }
        });
        out.unwrap()
    };
    assert!(machine_hacked(&mut r), "and it is taken on the server");
    // the camera is at the machine (ten metres ahead of the player, about at the player's eye height)
    let cam = camera_offset(&r.sims[0]);
    assert!(cam[2] < -8.0 && cam[1] < 2.0, "the camera is at the machine's puppet, in the player's own axes: {cam:?}");
    // forward: it drives off down -z, and everybody sees that
    set_local(&mut r.sims[0], |np, _| np.axes = [0.0, 0.0, 1.0, 0.0]);
    r.run(1200, |sims, _| stand(sims, 0));
    set_local(&mut r.sims[0], |np, _| np.axes = [0.0; 4]);
    r.run(500, |sims, _| stand(sims, 0));
    let moved = machine_puppet(&mut r.sims[1]);
    assert!(moved[2] < start[2] - 3.0, "the machine went forward under the player's hands: {start:?} -> {moved:?}");
    assert!(camera_offset(&r.sims[0])[2] < cam[2] - 3.0, "and the camera went with it: {:?}", camera_offset(&r.sims[0]));
    // the reload button (2) gives it back: the camera goes home
    r.run(500, |sims, _| stand(sims, 2));
    r.run(600, |sims, _| stand(sims, 0));
    let (np, _) = local(&r.sims[0]);
    assert_eq!(np.viewing, 0, "{np:?}");
    assert!(!machine_hacked(&mut r), "the machine is its own again");
    assert_eq!(camera_offset(&r.sims[0]), [0.0, 1.6, 0.0], "the camera is where the player's eyes are");
}

#[test]
fn the_message_of_a_view_round_trips() {
    use super::simgame::{parse_view, view};
    assert_eq!(parse_view(&view(41)), Some(41));
    assert_eq!(parse_view(&view(0)), Some(0));
    assert!(parse_view(&[0x46, 1]).is_none() && parse_view(&[0x45, 1, 0, 0, 0]).is_none());
}

#[test]
fn the_status_message_round_trips_and_refuses_what_is_not_one() {
    use super::simgame::Status;
    let s = Status { health: 200, food: 100, water: 50, stamina: 255, heat: 76, flags: Status::BLEEDING | Status::RELOADING, gun: 7, mag: 30, reserve: 65535 };
    assert_eq!(Status::parse(&s.message()), Some(s));
    assert_eq!(s.message().len(), 12);
    assert!(Status::parse(&s.message()[..11]).is_none() && Status::parse(&[0x46; 12]).is_none() && Status::parse(&[]).is_none());
}

#[test]
fn a_save_in_the_background_costs_the_servers_thread_only_the_copy_and_writes_the_same_file() {
    let dir = std::env::temp_dir().join(format!("lunaris_bgsave_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("world.json");
    let host = format!(r#","save":"{}","save_every":3600"#, file.to_string_lossy().replace('\\', "/"));
    let mut sim = server_scene(&host);
    sim.asset_root = None;
    let mut r = rig(SimGame::new(sim), 1);
    r.run(2500, |_, _| {});
    // a big world to save
    for i in 0..30_000u64 {
        spawn(&mut r.server.game.sim, 10_000 + i, r#"{"Transform":{"pos":[1,2,3]},"HitPoints":{"value":5,"max":9}}"#);
    }
    let t = std::time::Instant::now();
    assert!(r.server.game.save_now());
    let sync = t.elapsed();
    let first = std::fs::read_to_string(&file).unwrap();
    assert!(first.len() > 1_000_000, "{} bytes", first.len());
    // the same save, in the background: the call returns after the copy, the file arrives, and it is the same
    let _ = std::fs::remove_file(&file);
    let t = std::time::Instant::now();
    assert!(r.server.game.save_in_background());
    let call = t.elapsed();
    assert!(r.server.game.saving(), "it is being written");
    assert!(!r.server.game.save_in_background(), "one at a time: a second one waits for the first");
    let mut waited = 0;
    while r.server.game.saving() && waited < 20_000 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        r.server.game.poll_save();
        waited += 10;
    }
    assert!(!r.server.game.saving(), "it finished");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), first, "the same file");
    assert!(call * 3 < sync, "the server's thread was held for {call:?} where writing the save takes {sync:?}");
    assert_eq!(r.server.game.stats.saves, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The scene of the tests with the avatar made a character that holds guns as items: a pistol in slot 0 with twelve rounds behind its magazine, a bandage in slot 1, nothing else, and a `Weapon` that is off until a gun is in hand.
fn loadout_scene() -> Sim {
    loadout_scene_with("gun pistol ammo=ammo9 damage=10 rate=600 mag=4 reload=0.5 auto=0 spread=0 range=80")
}

/// `loadout_scene` with other guns (the text of the avatar's `Loadout`, one line a gun, no quotes or line breaks in it).
fn loadout_scene_with(guns: &str) -> Sim {
    let mut sim = server_scene("");
    // (the items: 1 pistol, 2 ammunition, 3 bandage)
    let e = sim.world.entity(6).unwrap();
    let text = sim.world.strings_mut().intern("item pistol | Pistol | stack=1\nitem ammo9 | 9 mm | stack=60\nitem bandage | Bandage | stack=5\n");
    sim.world.get_mut::<lunaris_sim::inventory::ItemTable>(e).unwrap().text = lunaris_ecs::Sym(text);
    let mut ids = vec!["0"; 48];
    let mut counts = vec!["0"; 48];
    (ids[0], counts[0]) = ("1", "1");
    (ids[1], counts[1]) = ("3", "2");
    (ids[2], counts[2]) = ("2", "12");
    let (ids, counts) = (ids.join(","), counts.join(","));
    prefab(
        &mut sim,
        "Avatar",
        &format!(
            r#"{{"components":{{"Name":{{"value":"Avatar"}},"Transform":{{}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},
            "HitPoints":{{"value":100,"max":100,"last":100}},"Inventory":{{"size":12,"id":[{ids}],"count":[{counts}]}},"Respawn":{{"delay":1,"avoid":80,"protect":0,"corpse":"Corpse","corpse_time":30}},"NetAvatar":{{}}}},
           "children":[{{"components":{{"Name":{{"value":"Head"}},"LocalTransform":{{"pos":[0,1.6,0]}},
            "Weapon":{{"active":false,"spread":0,"recoil":[0,0],"noise":0}},"Loadout":{{"guns":"{guns}"}},"BuildTool":{{"reach":12}},"Interactor":{{"reach":3,"look":false}}}}}}]}}"#
        ),
    );
    sim
}

#[test]
fn a_player_shoots_the_gun_it_holds_from_its_bag_reloads_from_the_ammunition_in_it_and_shoots_nothing_with_a_bandage_in_hand() {
    let mut r = rig(SimGame::new(loadout_scene()), 2);
    r.run(2500, |_, _| {});
    let (_, ta) = local(&r.sims[0]);
    let (_, tb) = local(&r.sims[1]);
    let d = [tb.pos[0] - ta.pos[0], tb.pos[1] + 1.0 - (ta.pos[1] + 1.6), tb.pos[2] - ta.pos[2]];
    let yaw = (-d[0]).atan2(-d[2]);
    let pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt());
    let aim = |sims: &mut [Sim], flags: u32, held: u32| {
        set_local(&mut sims[0], |np, t| {
            np.pitch = pitch;
            np.flags = flags;
            np.extra = held;
            t.rot = yaw_rot(yaw);
        });
    };
    let hits = |r: &mut Rig| r.server.game.sim.events.iter().filter(|e| e.name == "hit").count();
    // the bandage in hand: the trigger does nothing
    r.run(600, |sims, _| aim(sims, 1, 1));
    let none = hits(&mut r);
    assert_eq!(none, 0, "no gun in hand, no shots");
    // what the other player sees in its hand: the item of the gun (the server's) above the slot the player said
    let in_hand = |r: &mut Rig| {
        let mut out = Vec::new();
        let _ = r.sims[1].world.for_each::<(&NetPuppet,), _>(|_, (p,)| out.push(p.extra));
        out
    };
    assert!(in_hand(&mut r).contains(&1), "a bandage is no gun: only the slot is said: {:?}", in_hand(&mut r));
    // the pistol in hand: single shots (the trigger is let go between them)
    for _ in 0..4 {
        r.run(150, |sims, _| aim(sims, 1, 0));
        r.run(150, |sims, _| aim(sims, 0, 0));
    }
    let after = hits(&mut r);
    assert!(after >= 3, "the pistol hits: {after}");
    r.run(100, |sims, _| aim(sims, 0, 0));
    assert!(in_hand(&mut r).contains(&(1 << 6)), "the pistol (item 1) is in the hand of the puppet: {:?}", in_hand(&mut r));
    let bag = |r: &mut Rig| {
        let mut out = Vec::new();
        let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &lunaris_sim::inventory::Inventory), _>(|_, (n, inv)| {
            if n.slot == 0 {
                out = (0..12).filter(|&k| inv.id[k] != 0).map(|k| (inv.id[k], inv.count[k])).collect();
            }
        });
        out
    };
    let before_reload = bag(&mut r);
    assert_eq!(before_reload, vec![(1, 1), (3, 2), (2, 12)], "the magazine is the gun's: the bag has not paid yet");
    // reload (the reload bit, 2): a magazine of four takes what it needs of the twelve rounds in the bag
    r.run(100, |sims, _| aim(sims, 2, 0));
    r.run(900, |sims, _| aim(sims, 0, 0));
    let paid = bag(&mut r);
    assert_eq!(paid[2].1, 12 - 4 + 0, "the four rounds that went into the empty magazine left the bag: {paid:?}");
    // the player is told what its interface shows: the gun in its hand (item 1), the magazine and what is left in the bag for it, and its vitals
    r.run(300, |sims, _| aim(sims, 0, 0));
    let (np, _) = local(&r.sims[0]);
    assert_eq!((np.gun, np.mag, np.reserve), (1, 4, 8), "{np:?}");
    assert!(np.health > 0.99 && !np.bleeding && !np.reloading, "{np:?}");
    // (the test's avatar has no `Vitals`: the three bars read empty, which a game that has them never sees)
    assert_eq!((np.food, np.water, np.stamina), (0.0, 0.0, 0.0));
}

#[test]
fn the_aim_button_of_a_player_narrows_the_gun_of_its_avatar_on_the_server_and_letting_go_widens_it_again() {
    let mut r = rig(SimGame::new(loadout_scene_with("gun pistol ammo=ammo9 damage=10 rate=600 mag=4 reload=0.5 auto=0 spread=2 bloom=0.5 recoil=1 range=80 aim=0.25 aim_recoil=0.5")), 1);
    r.run(600, |_, _| {});
    let weapon_of_avatar = |r: &mut Rig| {
        let mut out = None;
        let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::Weapon, &lunaris_sim::components::Loadout), _>(|_, (w, l)| out = Some((w.spread, w.bloom_per_shot, w.recoil, l.aiming)));
        out.expect("the avatar's head has a weapon and a loadout")
    };
    // the pistol in hand (slot 0), nothing pressed
    r.run(200, |sims, _| set_local(&mut sims[0], |np, _| np.extra = 0));
    assert_eq!(weapon_of_avatar(&mut r), (2.0, 0.5, [1.0, 0.25], false), "the gun as it is");
    // the aim bit of the player's buttons (64): the server's weapon of this avatar scatters a quarter as much and kicks half as much
    r.run(200, |sims, _| set_local(&mut sims[0], |np, _| {
        np.extra = 0;
        np.flags = 64;
    }));
    assert_eq!(weapon_of_avatar(&mut r), (0.5, 0.125, [0.5, 0.125], true), "aimed");
    r.run(200, |sims, _| set_local(&mut sims[0], |np, _| {
        np.extra = 0;
        np.flags = 0;
    }));
    assert_eq!(weapon_of_avatar(&mut r), (2.0, 0.5, [1.0, 0.25], false), "the button let go: the gun's own numbers again");
}

/// The scene of the tests with a bag that holds an axe (slot 0) and five bandages (slot 1), pickups on the ground (an axe, three bandages, something the host has no picture for) and `NetHost.item_kinds` for the axe, the bandage and an item nobody has.
fn pickup_scene() -> Sim {
    let mut sim = server_scene(r#","item_kinds":"axe 21 # the axe on the ground\nbandage 22\nghost 23\nbroken x\n""#);
    let mut ids = vec!["0"; 48];
    let mut counts = vec!["0"; 48];
    (ids[0], counts[0]) = ("2", "1");
    (ids[1], counts[1]) = ("1", "5");
    let (ids, counts) = (ids.join(","), counts.join(","));
    prefab(
        &mut sim,
        "Avatar",
        &format!(
            r#"{{"components":{{"Name":{{"value":"Avatar"}},"Transform":{{}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},
            "HitPoints":{{"value":100,"max":100,"last":100}},"Inventory":{{"size":12,"id":[{ids}],"count":[{counts}]}},"Respawn":{{"delay":1,"avoid":80,"protect":0,"corpse":"Corpse","corpse_time":30}},"NetAvatar":{{}}}},
           "children":[{{"components":{{"Name":{{"value":"Head"}},"LocalTransform":{{"pos":[0,1.6,0]}},"BuildTool":{{"reach":12}},"Interactor":{{"reach":3,"look":false}}}}}}]}}"#
        ),
    );
    for (uid, at, item) in [(910, [8.0, 0.3, 8.0], "axe 1"), (911, [9.0, 0.3, 8.0], "bandage 3"), (912, [10.0, 0.3, 8.0], "rock 2")] {
        spawn(&mut sim, uid, &format!(r#"{{"Transform":{{"pos":{at:?}}},"Interactable":{{"kind":3,"prompt":"Take","item":"{item}"}},"NetEntity":{{"class":1,"kind":20}}}}"#));
    }
    sim
}

/// What a client shows of the things on the ground: the (kind, prefab name) of each class 1 puppet, sorted.
fn on_the_ground(s: &mut Sim) -> Vec<(u32, String)> {
    puppets(s).into_iter().filter(|p| p.0 == 1).map(|p| (p.1, p.2)).collect()
}

#[test]
fn a_pickup_is_shown_by_the_item_it_holds_whatever_made_it_and_follows_the_item() {
    let mut r = rig(SimGame::new(pickup_scene()), 2);
    for s in r.sims.iter_mut() {
        let e = s.world.entity(1).unwrap();
        let k = s.world.strings_mut().intern("3 7 Crate\n1 20 PickupModel\n1 21 AxeModel\n1 22 BandageModel");
        s.world.get_mut::<NetPlayer>(e).unwrap().kinds = lunaris_ecs::Sym(k);
        for name in ["AxeModel", "BandageModel"] {
            prefab(s, name, &format!(r#"{{"components":{{"Name":{{"value":"{name}"}},"Transform":{{}},"NetPuppet":{{}}}}}}"#));
        }
    }
    r.run(3000, |_, _| {});
    let names = |v: Vec<(u32, String)>| v.into_iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>();
    // the scene's own loot: the axe and the bandages have their pictures, the item with no line keeps the picture of its entity
    assert_eq!(names(on_the_ground(&mut r.sims[1])), ["20 PickupModel", "21 AxeModel", "22 BandageModel"]);
    // (the server's entity is as the scene made it: the picture is what the clients are told)
    let kind_of = |r: &mut Rig, uid: u64| {
        let e = r.server.game.sim.world.entity(uid).unwrap();
        r.server.game.sim.world.get::<lunaris_sim::components::NetEntity>(e).unwrap().kind
    };
    assert_eq!(kind_of(&mut r, 910), 20);
    // a player drops the axe and two of the bandages: the pickups the engine makes from the host's prefab are shown by their items too (and lie turned the way the player looks)
    set_local(&mut r.sims[0], |_, t| t.rot = yaw_rot(0.8));
    r.run(300, |_, _| {});
    say(&mut r.sims[0], "inv_drop", [0.0, 0.0, 0.0, 0.0]);
    r.run(600, |_, _| {});
    say(&mut r.sims[0], "inv_drop", [1.0, 2.0, 0.0, 0.0]);
    r.run(2500, |_, _| {});
    for i in 0..2 {
        assert_eq!(names(on_the_ground(&mut r.sims[i])), ["20 PickupModel", "21 AxeModel", "21 AxeModel", "22 BandageModel", "22 BandageModel"], "client {i}");
    }
    let mut turns: Vec<f32> = Vec::new();
    let _ = r.sims[1].world.for_each::<(&NetPuppet, &Transform), _>(|_, (p, t)| {
        if p.class == 1 && p.kind == 21 {
            turns.push(super::play::yaw_of(t.rot));
        }
    });
    assert!(turns.iter().any(|y| (y - 0.8).abs() < 0.05) && turns.iter().any(|y| y.abs() < 0.05), "the dropped axe lies the way the player looked, the scene's axe as the scene put it: {turns:?}");
    // the pickup of the scene's axe comes to hold bandages (a game's script changes it): its picture is made again, not left as it was
    let e = r.server.game.sim.world.entity(910).unwrap();
    let text = r.server.game.sim.world.strings_mut().intern("bandage 1");
    r.server.game.sim.world.get_mut::<lunaris_sim::components::Interactable>(e).unwrap().item = lunaris_ecs::Sym(text);
    r.run(2000, |_, _| {});
    assert_eq!(names(on_the_ground(&mut r.sims[1])), ["20 PickupModel", "21 AxeModel", "22 BandageModel", "22 BandageModel", "22 BandageModel"]);
}

#[test]
fn the_parts_of_a_puppet_are_told_what_the_puppet_is_told_and_go_with_it() {
    let mut r = rig(SimGame::new(server_scene("")), 2);
    // a body with a hand (it has a `NetPuppet` of its own) and a hat (it has none)
    for s in r.sims.iter_mut() {
        prefab(
            s,
            "Mate",
            r#"{"components":{"Name":{"value":"Mate"},"Transform":{},"NetPuppet":{}},
               "children":[{"components":{"Name":{"value":"Hand"},"LocalTransform":{"pos":[0.3,1.2,-0.5]},"NetPuppet":{}}},
                           {"components":{"Name":{"value":"Hat"},"LocalTransform":{"pos":[0,1.8,0]}}}]}"#,
        );
    }
    r.run(3000, |sims, _| set_local(&mut sims[0], |np, _| {
        np.extra = 3 | (5 << 6);
        np.pitch = 0.4;
    }));
    let named = |s: &mut Sim, name: &str| {
        let mut out = Vec::new();
        let _ = s.world.for_each::<(&NetPuppet, &lunaris_sim::components::Name), _>(|_, (p, n)| out.push((*p, n.value.0)));
        out.into_iter().filter(|(_, n)| s.world.strings().get(*n) == name).map(|(p, _)| p).collect::<Vec<_>>()
    };
    let body = named(&mut r.sims[1], "Mate");
    let hand = named(&mut r.sims[1], "Hand");
    assert_eq!((body.len(), hand.len()), (1, 1), "one body and one hand");
    assert_eq!(body[0].extra, 3 | (5 << 6), "{:?}", body[0]);
    assert_eq!((hand[0].id, hand[0].extra), (body[0].id, body[0].extra), "the hand is told what the body is told: {:?} against {:?}", hand[0], body[0]);
    assert!((hand[0].pitch - 0.4).abs() < 0.03 && hand[0].health == body[0].health, "{:?}", hand[0]);
    assert_eq!(named(&mut r.sims[1], "Hat").len(), 0, "a part with no NetPuppet is not a puppet");
    // the hand moves with the body (a child of it) and goes when the player does
    r.drivers[0].client.disconnect();
    r.run(4500, |_, _| {});
    assert_eq!((named(&mut r.sims[1], "Mate").len(), named(&mut r.sims[1], "Hand").len()), (0, 0));
}

fn bandages_on_avatars(r: &mut Rig) -> Vec<u32> {
    let mut v = Vec::new();
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &lunaris_sim::inventory::Inventory), _>(|_, (_, inv)| {
        v.push((0..48).map(|k| if inv.id[k] == 1 { inv.count[k] } else { 0 }).sum());
    });
    v
}

/// A player is one player: what a name kept is given to ONE avatar. A second connection with the name of one that is in is refused (it used to get a copy of the bag as well).
#[test]
fn two_connections_with_one_name_do_not_both_get_the_saved_bag() {
    let host = r#","persist":"Inventory""#;
    let mut sim = server_scene(host);
    sim.asset_root = None;
    let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.0, duplicate: 0.0 }, 7);
    let end = net.endpoint();
    let addr = end.addr();
    let server = NetServer::new(ServerConfig::default(), end, SimGame::new(sim));
    let mut r = Rig { net, server, sims: vec![], drivers: vec![], now: 1000 };
    r.sims.push(client_scene());
    r.drivers.push(NetDriver::new(NetClient::new(r.net.endpoint(), addr, "alice", 50)));
    r.run(2500, |_, _| {});
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &mut lunaris_sim::inventory::Inventory), _>(|_, (_, inv)| {
        inv.id[0] = 1;
        inv.count[0] = 5;
    });
    r.run(200, |_, _| {});
    assert_eq!(bandages_on_avatars(&mut r), vec![5]);
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    assert!(bandages_on_avatars(&mut r).is_empty(), "alice left");
    for id in [51u32, 52u32] {
        r.sims.push(client_scene());
        let mut d = NetDriver::new(NetClient::new(r.net.endpoint(), addr, "alice", id));
        d.timeout_ms = 1_000_000; // (the rig's clock is far on: a late client would give up at once)
        r.drivers.push(d);
    }
    r.run(4000, |_, _| {});
    let on = bandages_on_avatars(&mut r);
    assert!(on.iter().sum::<u32>() <= 5, "the saved bandages exist once, not once for each connection: {on:?}");
    assert_eq!(on, vec![5], "one avatar for the one name, with the bag it kept: {on:?}");
}

fn save_host(file: &std::path::Path, version: u32) -> String {
    format!(r#","save":"{}","persist":"Transform HitPoints","world_version":{version}"#, file.to_string_lossy().replace('\\', "/"))
}

/// A save is the world's state, the scene is the game's code: the graph systems and prefabs of the scene the server starts with are kept over a loaded save, so that an update reaches a world saved before it; the characters are kept as ever.
#[test]
fn a_saved_world_is_loaded_with_the_scenes_own_code_and_a_new_world_layout_starts_fresh_but_keeps_the_characters() {
    let dir = std::env::temp_dir().join(format!("lunaris_simgame_ver_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("world.json");
    // version 1: the world has a marker entity and an OLD prefab; the player walks away and leaves
    let host1 = save_host(&file, 1);
    let mut sim = server_scene(&host1);
    sim.asset_root = None;
    spawn(&mut sim, 900, r#"{"Name":{"value":"Marker"},"Transform":{"pos":[3,0,3]}}"#);
    prefab(&mut sim, "OldPrefab", r#"{"components":{"Name":{"value":"Old"}}}"#);
    let mut r = rig(SimGame::new(sim), 1);
    r.run(2500, |_, _| {});
    let start = local(&r.sims[0]).1.pos;
    r.run(1200, |sims, _| {
        set_local(&mut sims[0], |_, t| {
            if t.pos[0] < start[0] + 5.0 {
                t.pos[0] += 0.05;
            }
        });
    });
    let there = local(&r.sims[0]).1.pos;
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    assert!(r.server.game.save_now());
    let has = |r: &mut Rig, uid: u64| r.server.game.sim.world.entity(uid).is_some();

    // the same layout, a NEW version of the code (another prefab): the world comes from the save, the code from the scene
    let mut sim2 = server_scene(&host1);
    sim2.asset_root = None;
    prefab(&mut sim2, "NewPrefab", r#"{"components":{"Name":{"value":"New"}}}"#);
    let mut r2 = rig(SimGame::new(sim2), 1);
    r2.run(3000, |_, _| {});
    assert!(has(&mut r2, 900), "the world (its marker) comes from the save");
    let prefabs: Vec<String> = r2.server.game.sim.scripts.prefabs.keys().cloned().collect();
    assert!(prefabs.contains(&"NewPrefab".to_string()) && !prefabs.contains(&"OldPrefab".to_string()), "the code comes from the scene: {prefabs:?}");

    // another layout: the world starts fresh from the scene (no marker), the player who comes back still gets their place
    let mut sim3 = server_scene(&save_host(&file, 2));
    sim3.asset_root = None;
    let mut r3 = rig(SimGame::new(sim3), 1);
    r3.run(3000, |_, _| {});
    assert!(!has(&mut r3, 900), "a save of another layout is not loaded as a world");
    let back = local(&r3.sims[0]).1.pos;
    assert!((back[0] - there[0]).abs() < 1.0 && (back[2] - there[2]).abs() < 1.0, "the character is kept: left at {there:?}, back at {back:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------------------------------------------ resources every client is told (NetHost.sync)

fn client_res(r: &Rig, i: usize, name: &str) -> Option<f64> {
    r.sims[i].world.resource(name)
}

#[test]
fn a_client_gets_the_resources_the_host_lists_at_once_when_it_joins_and_nothing_else() {
    // (a long `sync_every`: only the message made for a joining client can carry them)
    let mut scene = server_scene(r#","sync":"dn.t dn.rate","sync_every":60"#);
    scene.world.set_resource("dn.t", Some(7.25)).unwrap();
    scene.world.set_resource("dn.rate", Some(1.0)).unwrap();
    scene.world.set_resource("dn.secret", Some(5.0)).unwrap();
    let mut r = rig(SimGame::new(scene), 2);
    r.run(3000, |_, _| {});
    for i in 0..2 {
        assert_eq!((client_res(&r, i, "dn.t"), client_res(&r, i, "dn.rate")), (Some(7.25), Some(1.0)), "client {i} was told at once");
        assert_eq!(client_res(&r, i, "dn.secret"), None, "what the host does not list stays on the server");
    }
    // a change is held back by `sync_every` (60 s): the clients still have the old value
    r.server.game.sim.world.set_resource("dn.t", Some(9.5)).unwrap();
    r.run(3000, |_, _| {});
    assert_eq!(client_res(&r, 0, "dn.t"), Some(7.25));
}

#[test]
fn a_change_of_a_listed_resource_reaches_every_client_and_a_drifted_copy_is_set_right_by_the_refresh() {
    let mut scene = server_scene(r#","sync":"dn.t dn.force","sync_every":0.2"#);
    scene.world.set_resource("dn.t", Some(7.25)).unwrap();
    scene.world.set_resource("dn.force", Some(0.0)).unwrap();
    let mut r = rig(SimGame::new(scene), 2);
    r.run(2000, |_, _| {});
    assert_eq!((client_res(&r, 0, "dn.t"), client_res(&r, 1, "dn.t")), (Some(7.25), Some(7.25)));
    // the server's clock moves a little every step: the clients follow it
    r.server.game.sim.world.set_resource("dn.t", Some(13.2371)).unwrap();
    r.server.game.sim.world.set_resource("dn.force", Some(4.0)).unwrap();
    r.run(1000, |_, _| {});
    for i in 0..2 {
        assert_eq!((client_res(&r, i, "dn.t"), client_res(&r, i, "dn.force")), (Some(13.2371), Some(4.0)), "client {i}");
    }
    // a client whose copy was changed (a scene reload, a stray system) is put right by the refresh within a few seconds
    r.sims[0].world.set_resource("dn.t", Some(0.0)).unwrap();
    r.run(7000, |_, _| {});
    assert_eq!(client_res(&r, 0, "dn.t"), Some(13.2371));
}

// ------------------------------------------------------------------------------------------------ what the server leaves out (NetHost.skip)

fn interface_scene(extra_host: &str) -> Sim {
    let mut scene = server_scene(extra_host);
    spawn(&mut scene, 700, r#"{"Name":{"value":"HUD"},"Ui":{}}"#);
    spawn(&mut scene, 701, r#"{"Name":{"value":"HUD child"},"Parent":{"target":700},"Ui":{}}"#);
    spawn(&mut scene, 702, r#"{"Name":{"value":"Page logic"},"Parent":{"target":700}}"#);
    spawn(&mut scene, 703, r#"{"Name":{"value":"Label"},"Text":{}}"#);
    spawn(&mut scene, 704, r#"{"Name":{"value":"Gate"},"Transform":{"pos":[3,0,3]},"Ui":{},"NetEntity":{"class":3,"kind":7}}"#);
    spawn(&mut scene, 705, r#"{"Name":{"value":"Floor sign"},"Transform":{"pos":[4,0,4]}}"#);
    scene
}

#[test]
fn nethost_skip_removes_the_interface_from_the_servers_world_and_what_hangs_on_it_and_nothing_else() {
    let mut r = rig(SimGame::new(interface_scene(r#","skip":"Ui, Text, NoSuchComponent""#)), 1);
    r.run(500, |_, _| {});
    let world = &r.server.game.sim.world;
    for gone in [700, 701, 702, 703] {
        assert!(world.entity(gone).is_none(), "{gone} has a listed component or hangs on one: not kept");
    }
    assert!(world.entity(704).is_some(), "a replicated thing is kept whatever else it has");
    assert!(world.entity(705).is_some() && world.entity(21).is_some() && world.entity(4).is_some(), "the rest of the scene is as it was (the floor sign, a spawn point, the host)");
    // the same scene without `skip` keeps everything
    let mut r = rig(SimGame::new(interface_scene("")), 1);
    r.run(500, |_, _| {});
    let world = &r.server.game.sim.world;
    assert!([700, 701, 702, 703, 704, 705].iter().all(|u| world.entity(*u).is_some()));
    // and a client still plays: it has its own copy of the scene, and the server's world minus the interface hosts it
    let (np, _) = local(&r.sims[0]);
    assert!(np.connected);
}

#[test]
fn skip_also_applies_to_a_world_loaded_from_a_save_made_before_it_was_set() {
    let dir = std::env::temp_dir().join(format!("lunaris_skip_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("world.json").to_string_lossy().replace('\\', "/");
    let host = format!(r#","save":"{file}""#);
    // the first server has no `skip`: its save holds the interface
    let mut sim = interface_scene(&host);
    sim.asset_root = None;
    let mut r = rig(SimGame::new(sim), 1);
    r.run(500, |_, _| {});
    assert!(r.server.game.save_now());
    // the second one has it: the world it loads from that save has none
    let mut sim = interface_scene(&format!(r#"{host},"skip":"Ui""#));
    sim.asset_root = None;
    let mut r = rig(SimGame::new(sim), 1);
    r.run(500, |_, _| {});
    let world = &r.server.game.sim.world;
    assert!(world.entity(700).is_none() && world.entity(701).is_none() && world.entity(704).is_some(), "the saved interface is left out too");
    let _ = std::fs::remove_file(dir.join("world.json"));
}

#[test]
fn an_event_message_says_what_it_is_about_and_both_ways_of_mixing_old_and_new_still_read() {
    use super::simgame::{event, event_about, parse_event, parse_event_about};
    // new server, new client
    let m = event_about("hit", [1.0, 2.0, 3.0], 4711);
    assert_eq!(parse_event_about(&m), Some(("hit".to_string(), [1.0, 2.0, 3.0], 4711)));
    // new server, old client: the old reader stops after the place and never sees the number
    assert_eq!(parse_event(&m), Some(("hit".to_string(), [1.0, 2.0, 3.0])));
    // old server (no number at the end), new client: the entity is 0
    let old = event("kill", [0.0, 1.0, 0.0]);
    assert_eq!(parse_event_about(&old), Some(("kill".to_string(), [0.0, 1.0, 0.0], 0)));
    assert_eq!(old.len() + 2, event_about("kill", [0.0, 1.0, 0.0], 300).len(), "the number costs one to five bytes, and nothing when it is 0: {}", old.len());
    assert_eq!(event_about("kill", [0.0, 1.0, 0.0], 0), old, "no entity: the very message that was always sent");
    // a number cut in the middle or too big for a server number is read as none
    let mut cut = event_about("hit", [0.0; 3], 300);
    cut.pop();
    assert_eq!(parse_event_about(&cut).map(|e| e.2), Some(0));
}

/// A chat rig: `n` players, the log (a `Text` with a `NetChat`, uid 901) and the chat field (a `UiInput`, uid 902, the player's `say_input`) in every client's scene.
fn chat_rig(chat: &str, n: usize) -> Rig {
    let mut r = rig(SimGame::new(server_scene(chat)), n);
    for sim in r.sims.iter_mut() {
        spawn(sim, 901, r#"{"Text":{"value":""},"NetChat":{"lines":3,"life":6}}"#);
        spawn(sim, 902, r#"{"Ui":{"button":true},"UiInput":{"max_len":600}}"#);
        set_local(sim, |np, _| np.say_input = lunaris_ecs::Uid(902));
    }
    r
}

/// Put `text` in a player's chat field and press Enter (the field says `submitted` for a tick: the driver sends what is in it before the next tick).
fn chat_say(sim: &mut Sim, text: &str) {
    let sym = sim.world.strings_mut().intern(text);
    let e = sim.world.entity(902).unwrap();
    let f = sim.world.get_mut::<lunaris_sim::components::UiInput>(e).unwrap();
    f.text.0 = sym;
    f.submitted = true;
}

fn log_of(sim: &Sim) -> String {
    let e = sim.world.entity(901).unwrap();
    sim.world.strings().get(sim.world.get::<lunaris_sim::components::Text>(e).unwrap().value.0).to_string()
}

fn field_of(sim: &Sim) -> String {
    let e = sim.world.entity(902).unwrap();
    sim.world.strings().get(sim.world.get::<lunaris_sim::components::UiInput>(e).unwrap().text.0).to_string()
}

#[test]
fn what_a_player_says_is_cleaned_and_told_to_the_players_within_the_chat_distance_the_speaker_included() {
    // (a player cannot be moved far from where the server put it: the distance is the chat's own, 4 m, between the places the server gave the avatars)
    let mut r = chat_rig(r#","chat":4.0"#, 3);
    r.run(2500, |_, _| {});
    let mut at: Vec<(u32, [f32; 3])> = Vec::new();
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &Transform), _>(|_, (n, t)| at.push((n.slot, t.pos)));
    at.sort_by_key(|a| a.0);
    let d = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    assert_eq!(at.len(), 3, "three avatars: {at:?}");
    // the speaker: a player with a neighbour within 4 m and another player farther away
    let (sp, near) = (0..3)
        .map(|i| (i, at.iter().map(|(_, p)| d(*p, at[i].1) <= 4.0).collect::<Vec<bool>>()))
        .find(|(i, n)| n.iter().enumerate().any(|(j, v)| *v && j != *i) && n.iter().any(|v| !*v))
        .unwrap_or_else(|| panic!("the layout gives somebody a neighbour and a far player: {at:?}"));
    chat_say(&mut r.sims[sp], "  hello\u{7}   there \n friend  ");
    r.run(600, |_, _| {});
    let line = format!("p{sp}: hello there friend");
    for (i, n) in near.iter().enumerate() {
        let want = if *n { line.as_str() } else { "" };
        assert_eq!(log_of(&r.sims[i]), want, "player {i} at {:?} ({:.1} m from the speaker): the speaker and those within 4 m hear it, cleaned (no bell, one space between words, nothing round the ends)", at[i].1, d(at[i].1, at[sp].1));
    }
    assert_eq!(field_of(&r.sims[sp]), "", "the field is emptied once it is said");
    // an empty message, and one of nothing but control characters, say nothing
    chat_say(&mut r.sims[sp], "   ");
    r.run(300, |_, _| {});
    chat_say(&mut r.sims[sp], "\u{7}\u{1}");
    r.run(300, |_, _| {});
    let listener = near.iter().enumerate().position(|(j, v)| *v && j != sp).unwrap();
    assert_eq!(log_of(&r.sims[listener]), line, "nothing was added");
    // a long message is cut at 120 characters
    r.run(2000, |_, _| {});
    chat_say(&mut r.sims[listener], &"x".repeat(300));
    r.run(600, |_, _| {});
    let heard = log_of(&r.sims[sp]);
    let last = heard.lines().last().unwrap();
    assert_eq!(last, format!("p{listener}: {}", "x".repeat(120)), "cut at 120 characters: {last}");
    // the count of what has come in, for a game that plays a sound for each
    let e = r.sims[sp].world.entity(901).unwrap();
    assert_eq!(r.sims[sp].world.get::<lunaris_sim::components::NetChat>(e).unwrap().count, 2);
}

#[test]
fn a_player_can_say_three_things_at_once_and_then_one_every_second_and_a_fifth_and_a_chat_that_is_off_says_nothing() {
    let mut r = chat_rig(r#","chat":1000.0"#, 2);
    r.run(2500, |_, _| {});
    for i in 0..6 {
        chat_say(&mut r.sims[0], &format!("m{i}"));
        r.run(50, |_, _| {});
    }
    r.run(400, |_, _| {});
    // (the log shows the last 3 lines of those that are less than 6 s old)
    let heard = log_of(&r.sims[1]);
    let e = r.sims[1].world.entity(901).unwrap();
    let count = r.sims[1].world.get::<lunaris_sim::components::NetChat>(e).unwrap().count;
    assert_eq!(count, 3, "six in a burst: only the first three get through: {heard:?}");
    assert_eq!(heard, "p0: m0\np0: m1\np0: m2");
    // 1.3 s later one more may be said
    r.run(1300, |_, _| {});
    chat_say(&mut r.sims[0], "again");
    r.run(400, |_, _| {});
    let count = r.sims[1].world.get::<lunaris_sim::components::NetChat>(r.sims[1].world.entity(901).unwrap()).unwrap().count;
    assert_eq!(count, 4, "the credit came back");
    // a server with no chat distance: nothing is told, whatever is said
    let mut off = chat_rig("", 2);
    off.run(2500, |_, _| {});
    chat_say(&mut off.sims[0], "anybody there");
    off.run(600, |_, _| {});
    assert_eq!((log_of(&off.sims[0]), log_of(&off.sims[1])), (String::new(), String::new()), "chat is off by default");
}

#[test]
fn the_chat_log_forgets_a_line_after_its_life_and_keeps_only_the_last_lines() {
    let mut r = chat_rig(r#","chat":1000.0"#, 2);
    r.run(2500, |_, _| {});
    chat_say(&mut r.sims[0], "one");
    r.run(300, |_, _| {});
    assert_eq!(log_of(&r.sims[1]), "p0: one");
    r.run(7000, |_, _| {});
    assert_eq!(log_of(&r.sims[1]), "", "the line was told more than 6 s ago: gone");
}

#[test]
fn a_chat_log_with_bottom_is_filled_out_above_so_the_newest_line_stands_at_the_bottom() {
    let mut r = chat_rig(r#","chat":1000.0"#, 2);
    for sim in r.sims.iter_mut() {
        let e = sim.world.entity(901).unwrap();
        sim.world.get_mut::<lunaris_sim::components::NetChat>(e).unwrap().bottom = true;
    }
    r.run(2500, |_, _| {});
    assert_eq!(log_of(&r.sims[1]), "", "an empty log is empty, not three blank lines");
    chat_say(&mut r.sims[0], "one");
    r.run(300, |_, _| {});
    assert_eq!(log_of(&r.sims[1]), "\n\np0: one", "lines 3: two blank lines and the message");
}

#[test]
fn a_chat_text_and_a_players_name_are_cleaned_the_same_way() {
    use super::simgame::clean_chat;
    assert_eq!(clean_chat(b"Bo\n: hello\r\n\tfriend"), "Bo : hello friend", "a line break in a name or a text becomes a space: nobody can make a second line that looks like another player's");
    assert_eq!(clean_chat(b"\x07\x00\x1b[31m"), "[31m", "control characters are dropped (the rest is plain text)");
    assert_eq!(clean_chat("héllo wörld ✓".as_bytes()), "héllo wörld ✓", "other scripts and symbols are kept");
    assert_eq!(clean_chat(&[0xff, 0xfe, b'o', b'k']), "ok", "bytes that are no text are dropped");
    assert_eq!(clean_chat(&vec![b'a'; 1000]).chars().count(), 120);
}

/// The scene of the tests with an avatar that wears gear: a bag of 8 of which the first 4 are the carried bag (a helmet in slot 0, a pistol in slot 1, a vest in slot 2) and slots 4 to 6 are worn places (head takes class 1, body takes class 2, legs take class 3).
fn gear_scene() -> Sim {
    let mut sim = server_scene("");
    let e = sim.world.entity(6).unwrap();
    let text = sim.world.strings_mut().intern("item helmet | Helmet | stack=1 class=1 power=50 dur=100\nitem pistol | Pistol | stack=1\nitem vest | Vest | stack=1 class=2 power=40 dur=50\n");
    sim.world.get_mut::<lunaris_sim::inventory::ItemTable>(e).unwrap().text = lunaris_ecs::Sym(text);
    let mut ids = vec!["0"; 48];
    let mut counts = vec!["0"; 48];
    (ids[0], counts[0]) = ("1", "1");
    (ids[1], counts[1]) = ("2", "1");
    (ids[2], counts[2]) = ("3", "1");
    let (ids, counts) = (ids.join(","), counts.join(","));
    prefab(
        &mut sim,
        "Avatar",
        &format!(
            r#"{{"components":{{"Name":{{"value":"Avatar"}},"Transform":{{}},"CharacterBody":{{"radius":0.35,"height":1.8,"gravity_scale":0}},
            "HitPoints":{{"value":100,"max":100,"last":100}},"Inventory":{{"size":8,"bag":4,"id":[{ids}],"count":[{counts}]}},"Equipment":{{"slots":"slot 4 accepts=1 zones=head\nslot 5 accepts=2 zones=body\nslot 6 accepts=3 zones=legs"}},"Respawn":{{"delay":1,"avoid":80,"protect":0,"corpse":"Corpse","corpse_time":30}},"NetAvatar":{{}}}},
           "children":[{{"components":{{"Name":{{"value":"Head"}},"LocalTransform":{{"pos":[0,1.6,0]}}}}}}]}}"#
        ),
    );
    sim
}

#[test]
fn the_worn_places_of_a_players_bag_take_only_their_class_and_the_server_refuses_the_rest() {
    let mut r = rig(SimGame::new(gear_scene()), 1);
    r.run(2500, |_, _| {});
    let bag = |r: &mut Rig| {
        let mut out = vec![0u32; 7];
        let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &lunaris_sim::inventory::Inventory), _>(|_, (n, inv)| {
            if n.slot == 0 {
                for (k, o) in out.iter_mut().enumerate() {
                    *o = if inv.count[k] > 0 { inv.id[k] } else { 0 };
                }
            }
        });
        out
    };
    let mv = |r: &mut Rig, from: f32, to: f32| {
        r.drivers[0].client.command(super::simgame::action("inv_move", [from, to, 0.0, 0.0]));
        r.run(400, |_, _| {});
    };
    assert_eq!(bag(&mut r), vec![1, 2, 3, 0, 0, 0, 0], "helmet, pistol, vest carried");
    mv(&mut r, 1.0, 4.0);
    assert_eq!(bag(&mut r), vec![1, 2, 3, 0, 0, 0, 0], "a pistol is not a helmet: refused");
    mv(&mut r, 2.0, 4.0);
    assert_eq!(bag(&mut r), vec![1, 2, 3, 0, 0, 0, 0], "a vest is not a helmet either");
    mv(&mut r, 0.0, 4.0);
    assert_eq!(bag(&mut r), vec![0, 2, 3, 0, 1, 0, 0], "the helmet goes on");
    mv(&mut r, 2.0, 5.0);
    assert_eq!(bag(&mut r), vec![0, 2, 0, 0, 1, 3, 0], "the vest goes in the body place");
    mv(&mut r, 5.0, 6.0);
    assert_eq!(bag(&mut r), vec![0, 2, 0, 0, 1, 3, 0], "a vest in the legs place: refused");
    // a swap that would put the wrong thing into a worn place is refused: the pistol over the helmet
    mv(&mut r, 1.0, 4.0);
    assert_eq!(bag(&mut r), vec![0, 2, 0, 0, 1, 3, 0], "the pistol cannot swap with the helmet");
    // taking it off is always allowed, into an empty carried slot
    mv(&mut r, 4.0, 3.0);
    assert_eq!(bag(&mut r), vec![0, 2, 0, 1, 0, 3, 0], "the helmet comes off");
}

/// A fresh folder for an admin console.
fn admin_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("lunaris-admin-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn an_admin_can_list_announce_mute_and_kick_players_and_ban_a_name_that_then_cannot_join() {
    use super::admin::Admin;
    use super::play::LinkState;
    let mut r = chat_rig(r#","chat":1000.0"#, 2);
    r.run(2500, |_, _| {});
    let dir = admin_dir("cmds");
    let mut admin = Admin::open(&dir, &mut r.server).unwrap();
    let list = admin.run(&mut r.server, "players");
    assert!(list.contains("p0") && list.contains("p1"), "{list}");
    assert!(admin.run(&mut r.server, "status").starts_with("2 players in"), "status");
    assert!(admin.run(&mut r.server, "frobnicate now").contains("unknown command"));
    // an announcement goes to everybody as SERVER, cleaned like a chat line
    admin.run(&mut r.server, "say   the  server restarts \x07 soon ");
    r.run(400, |_, _| {});
    assert_eq!((log_of(&r.sims[0]), log_of(&r.sims[1])), ("SERVER: the server restarts soon".to_string(), "SERVER: the server restarts soon".to_string()));
    // a muted player's chat is dropped (it is told so), an unmuted one's goes through
    assert!(admin.run(&mut r.server, "mute P1 60").contains("muted for 60 s"), "mute is not case sensitive");
    chat_say(&mut r.sims[1], "can anybody hear me");
    r.run(400, |_, _| {});
    assert!(!log_of(&r.sims[0]).contains("p1:"), "the muted player is not heard: {}", log_of(&r.sims[0]));
    assert!(log_of(&r.sims[1]).contains("SERVER: You are muted"), "and is told: {}", log_of(&r.sims[1]));
    admin.run(&mut r.server, "unmute p1");
    chat_say(&mut r.sims[1], "now");
    r.run(400, |_, _| {});
    assert!(log_of(&r.sims[0]).contains("p1: now"), "{}", log_of(&r.sims[0]));
    // a kick removes the player at once and its client is told
    assert!(admin.run(&mut r.server, "kick p0 for testing").contains("removed"));
    r.run(300, |_, _| {});
    assert_eq!(r.drivers[0].link, LinkState::Refused(5), "{:?}", r.drivers[0].link);
    assert_eq!(r.server.client_count(), 1);
    // a ban removes the player and keeps its name out: the banned name is refused at the handshake with its own reason, another name gets in
    assert!(admin.run(&mut r.server, "ban name P1").contains("banned and was removed"));
    r.run(300, |_, _| {});
    assert_eq!(r.drivers[1].link, LinkState::Refused(4));
    assert_eq!(r.server.client_count(), 0);
    let server_addr = r.server.transport.addr();
    let join = |r: &mut Rig, name: &str, salt: u32| {
        let mut sim = client_scene();
        spawn(&mut sim, 901, r#"{"Text":{"value":""},"NetChat":{"lines":3,"life":6}}"#);
        r.sims.push(sim);
        r.drivers.push(NetDriver::new(NetClient::new(r.net.endpoint(), server_addr, name, salt)));
        let i = r.drivers.len() - 1;
        r.drivers[i].timeout_ms = u64::MAX; // (the driver judges the wait by the rig's clock, which has run on: a late joiner would time out at once)
        r.run(2500, |_, _| {});
        r.drivers[i].link.clone()
    };
    assert_eq!(join(&mut r, "p1", 90), LinkState::Refused(4), "the name is banned (case does not matter)");
    assert_eq!(join(&mut r, "P1", 91), LinkState::Refused(4));
    assert_eq!(join(&mut r, "somebody", 92), LinkState::Connected, "another name is let in");
    assert!(admin.run(&mut r.server, "bans").contains("name p1"));
    assert!(admin.run(&mut r.server, "unban P1").contains("not banned any more"));
    assert_eq!(join(&mut r, "p1", 93), LinkState::Connected, "unbanned");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_admin_file_is_read_while_the_server_runs_and_the_bans_are_kept_in_a_file_for_the_next_run() {
    use super::admin::Admin;
    use super::server::Bans;
    let mut r = chat_rig(r#","chat":1000.0"#, 1);
    r.run(2500, |_, _| {});
    let dir = admin_dir("file");
    let mut admin = Admin::open(&dir, &mut r.server).unwrap();
    let cmds = dir.join("commands.txt");
    // lines are run when they are whole, and each only once
    std::fs::write(&cmds, "players # who is in\nban ip 192.0.2.7\nban name Mallory\nsay hel").unwrap();
    admin.poll(&mut r.server, 1000);
    let out = std::fs::read_to_string(dir.join("output.log")).unwrap();
    assert!(out.contains("> players") && out.contains("p0") && out.contains("192.0.2.7 is banned (0 removed)") && out.contains("Mallory is banned"), "{out}");
    assert!(!out.contains("say"), "a line with no end yet is not run: {out}");
    assert!(r.server.bans.contains("x", "192.0.2.7".parse().unwrap()) && r.server.bans.contains("MALLORY", "192.0.2.8".parse().unwrap()) && !r.server.bans.contains("friend", "192.0.2.8".parse().unwrap()));
    let mut f = std::fs::OpenOptions::new().append(true).open(&cmds).unwrap();
    use std::io::Write;
    f.write_all(b"lo all\nstatus\n").unwrap();
    admin.poll(&mut r.server, 1100);
    assert_eq!(std::fs::read_to_string(dir.join("output.log")).unwrap(), out, "twice a second at most");
    admin.poll(&mut r.server, 1600);
    let out2 = std::fs::read_to_string(dir.join("output.log")).unwrap();
    assert!(out2.contains("> say hello all") && out2.contains("> status") && out2.matches("> players").count() == 1, "{out2}");
    // the bans are in a file, and a new server reads them
    let kept = std::fs::read_to_string(dir.join("bans.txt")).unwrap();
    assert!(kept.contains("ip 192.0.2.7") && kept.contains("name mallory"), "{kept}");
    assert_eq!(Bans::parse(&kept), r.server.bans);
    let mut next = rig(SimGame::new(server_scene("")), 1);
    let _again = Admin::open(&dir, &mut next.server).unwrap();
    assert_eq!(next.server.bans, r.server.bans, "read back when the server starts");
    // `stop` is a command too
    std::fs::OpenOptions::new().append(true).open(&cmds).unwrap().write_all(b"stop\n").unwrap();
    admin.poll(&mut r.server, 2200);
    assert!(admin.stop);
    // a damaged ban file is read as far as it can be
    let b = Bans::parse("name Ok\nnonsense\nip not-an-address\nip ::1\n# note\nname \n");
    assert_eq!((b.names.len(), b.ips.len()), (1, 1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a player keeps is theirs, the shape of the bag is the game's: a character kept before the game gave its bags more places (worn ones, for instance) gets the new shape on its next join, with the things it kept.
#[test]
fn a_character_kept_before_the_game_gave_its_bag_more_places_gets_them_on_its_next_join_with_its_old_things() {
    let mut sim = server_scene(r#","persist":"Inventory""#);
    sim.asset_root = None;
    let net = SimNet::new(Link { latency_ms: 25, jitter_ms: 5, loss: 0.0, duplicate: 0.0 }, 7);
    let end = net.endpoint();
    let addr = end.addr();
    let server = NetServer::new(ServerConfig::default(), end, SimGame::new(sim));
    let mut r = Rig { net, server, sims: vec![client_scene()], drivers: vec![], now: 1000 };
    r.drivers.push(NetDriver::new(NetClient::new(r.net.endpoint(), addr, "alice", 50)));
    r.run(2500, |_, _| {});
    // three bandages in the third place, and alice leaves
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &mut lunaris_sim::inventory::Inventory), _>(|_, (_, inv)| {
        inv.id[2] = 1;
        inv.count[2] = 3;
    });
    r.run(200, |_, _| {});
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    assert!(bandages_on_avatars(&mut r).is_empty(), "alice left");
    // the game now gives its bags 16 places, 12 of them carried (the other four are worn places), and alice comes back
    prefab(
        &mut r.server.game.sim,
        "Avatar",
        r#"{"components":{"Name":{"value":"Avatar"},"Transform":{},"CharacterBody":{"radius":0.35,"height":1.8,"gravity_scale":0},
            "HitPoints":{"value":100,"max":100,"last":100},"Inventory":{"size":16,"bag":12},"Hacker":{"aim":true,"speed":2},"Respawn":{"delay":1,"avoid":80,"protect":0,"corpse":"Corpse","corpse_time":30},"NetAvatar":{}},
           "children":[{"components":{"Name":{"value":"Head"},"LocalTransform":{"pos":[0,1.6,0]},
            "Weapon":{"damage":60,"spread":0,"recoil":[0,0],"rate":120,"auto":true,"infinite":true,"noise":0,"range":80},"BuildTool":{"reach":12},"Interactor":{"reach":3,"look":false}}}]}"#,
    );
    let mut d = NetDriver::new(NetClient::new(r.net.endpoint(), addr, "alice", 51));
    d.timeout_ms = 1_000_000; // (the rig's clock is far on: a late client would give up at once)
    r.sims.push(client_scene());
    r.drivers.push(d);
    r.run(4000, |_, _| {});
    let mut kept = Vec::new();
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar, &lunaris_sim::inventory::Inventory), _>(|_, (_, inv)| kept.push((inv.size, inv.bag, inv.id[2], inv.count[2])));
    assert_eq!(kept, vec![(16, 12, 1, 3)], "one avatar, with the game's bag of 16 places (12 carried) and the bandages it kept: {kept:?}");
}

/// A saved world of layout 1 with a marker in it and a character that walked away and left, as a wipe finds it.
fn wipe_setup(file: &std::path::Path) -> (Rig, [f32; 3]) {
    let mut sim = server_scene(&save_host(file, 1));
    sim.asset_root = None;
    spawn(&mut sim, 900, r#"{"Name":{"value":"Marker"},"Transform":{"pos":[3,0,3]}}"#);
    let mut r = rig(SimGame::new(sim), 1);
    r.run(2500, |_, _| {});
    let start = local(&r.sims[0]).1.pos;
    r.run(1200, |sims, _| {
        set_local(&mut sims[0], |_, t| {
            if t.pos[0] < start[0] + 5.0 {
                t.pos[0] += 0.05;
            }
        });
    });
    let there = local(&r.sims[0]).1.pos;
    r.drivers[0].client.disconnect();
    r.run(7000, |_, _| {});
    assert!(r.server.game.save_now());
    (r, there)
}

/// `wipe world yes` and `wipe all yes` (docs/ADMIN.md): what the next start finds, that nothing happens without the `yes`, and that the server is asked to stop.
#[test]
fn a_wipe_starts_the_world_fresh_and_keeps_the_characters_or_not_and_asks_the_server_to_stop() {
    use super::server::Game;
    let dir = std::env::temp_dir().join(format!("lunaris_simgame_wipe_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let has = |r: &mut Rig, uid: u64| r.server.game.sim.world.entity(uid).is_some();
    let next_start = |file: &std::path::Path| {
        let mut sim = server_scene(&save_host(file, 1));
        sim.asset_root = None;
        let mut r = rig(SimGame::new(sim), 1);
        r.run(3000, |_, _| {});
        r
    };
    // the world only: a fresh world, the character where it left
    let file = dir.join("world.json");
    let (mut r, there) = wipe_setup(&file);
    let kept = std::fs::read_to_string(&file).unwrap();
    assert_eq!(parse(&kept).unwrap().get("world_version").and_then(|v| v.as_u64()), Some(1), "an ordinary save is of the scene's layout");
    assert!(r.server.admin_game("wipe", "").unwrap().contains("wipe world yes"), "no word: it says how");
    assert!(r.server.admin_game("wipe", "world").unwrap().contains("say `wipe world yes`") && !r.server.game.wants_stop(), "without the yes nothing happens");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), kept, "and the file is as it was");
    assert!(r.server.admin_game("wipe", "world yes").unwrap().contains("wiped") && r.server.game.wants_stop(), "the server is asked to stop");
    assert_eq!(parse(&std::fs::read_to_string(&file).unwrap()).unwrap().get("world_version").and_then(|v| v.as_u64()), Some(u32::MAX as u64), "the save says it is of no layout a scene has");
    assert!(r.server.game.save_now());
    assert_eq!(parse(&std::fs::read_to_string(&file).unwrap()).unwrap().get("world_version").and_then(|v| v.as_u64()), Some(u32::MAX as u64), "and so does the save on the way out");
    let mut r2 = next_start(&file);
    assert!(!has(&mut r2, 900), "the world starts fresh from the scene (no marker)");
    let back = local(&r2.sims[0]).1.pos;
    assert!((back[0] - there[0]).abs() < 1.0 && (back[2] - there[2]).abs() < 1.0, "the character is kept: left at {there:?}, back at {back:?}");
    // everything: the characters go too
    let file = dir.join("all.json");
    let (mut r, there) = wipe_setup(&file);
    assert!(file.exists());
    assert!(r.server.admin_game("wipe", "all").unwrap().contains("say `wipe all yes`") && file.exists(), "without the yes nothing happens");
    assert!(r.server.admin_game("wipe", "all yes").unwrap().contains("wiped") && r.server.game.wants_stop());
    assert!(!file.exists(), "the save is gone");
    assert!(!r.server.game.save_now() && !file.exists(), "and the save on the way out writes nothing");
    let mut r2 = next_start(&file);
    assert!(!has(&mut r2, 900));
    let back = local(&r2.sims[0]).1.pos;
    assert!((back[0] - there[0]).abs() > 2.0, "the character is a new one, at a place of the scene: left at {there:?}, back at {back:?}");
    // the admin console ends in a stop when the game asks for one (the file is the way an owner says it)
    {
        use super::admin::Admin;
        use std::io::Write;
        let file = dir.join("console.json");
        let (mut r, _) = wipe_setup(&file);
        let adir = admin_dir("wipe");
        let mut admin = Admin::open(&adir, &mut r.server).unwrap();
        std::fs::write(adir.join("commands.txt"), "wipe all
").unwrap();
        admin.poll(&mut r.server, 1000);
        assert!(!admin.stop && file.exists(), "no yes: nothing happens");
        std::fs::OpenOptions::new().append(true).open(adir.join("commands.txt")).unwrap().write_all(b"wipe all yes
").unwrap();
        admin.poll(&mut r.server, 1600);
        assert!(admin.stop && !file.exists(), "the console stops the server and the save is gone");
        let said = std::fs::read_to_string(adir.join("output.log")).unwrap();
        assert!(said.contains("> wipe all yes") && said.contains("wiped"), "{said}");
    }
    // a game with no save file has nothing to wipe
    let mut plain = rig(SimGame::new(server_scene("")), 1);
    plain.run(500, |_, _| {});
    assert!(plain.server.admin_game("wipe", "world yes").unwrap().contains("nothing to wipe") && !plain.server.game.wants_stop());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A ban may run out (`ban NAME 30m`): the console says for how long, `bans` says what is left, the ban file keeps the end of it as a unix second and drops what has run out.
#[test]
fn a_ban_may_run_out_and_the_ban_file_keeps_the_time() {
    use super::admin::Admin;
    use super::server::{unix_now, Bans};
    let ip = |s: &str| -> std::net::IpAddr { s.parse().unwrap() };
    let b = Bans::parse("name Old until 1000\nname New until 99999999999\nip 192.0.2.1 until 5\nname Forever\nip 192.0.2.2\nname Broken until x\n");
    assert!(b.contains_at("old", ip("192.0.2.99"), 999) && !b.contains_at("old", ip("192.0.2.99"), 1000), "a timed ban holds while its end is later");
    assert!(b.contains_at("x", ip("192.0.2.1"), 4) && !b.contains_at("x", ip("192.0.2.1"), 5));
    assert!(b.contains_at("FOREVER", ip("1.1.1.1"), u64::MAX / 2) && b.contains_at("x", ip("192.0.2.2"), u64::MAX / 2), "a ban for good does not run out");
    assert!(!b.contains_at("broken", ip("1.1.1.1"), 0), "a line that does not parse is skipped");
    let text = b.to_text();
    assert!(!text.contains("name old") && !text.contains("192.0.2.1") && text.contains("name new until 99999999999") && text.contains("name forever") && text.contains("ip 192.0.2.2"), "what has run out is left out: {text}");
    assert_eq!(Bans::parse(&text).count(unix_now()), 3);
    // the console
    let mut r = chat_rig("", 1);
    r.run(2500, |_, _| {});
    let mut admin = Admin::open(&admin_dir("timed"), &mut r.server).unwrap();
    let said = admin.run(&mut r.server, "ban name Mallory 30m");
    assert!(said.contains("banned for 30 m"), "{said}");
    assert!(r.server.bans.contains("MALLORY", ip("1.2.3.4")) && !r.server.bans.names.contains("mallory"), "timed, not for good");
    assert!(admin.run(&mut r.server, "ban ip 192.0.2.9 2h").contains("for 2 h") && r.server.bans.contains("x", ip("192.0.2.9")));
    let listed = admin.run(&mut r.server, "bans");
    assert!(listed.contains("name mallory (29 m left)") || listed.contains("name mallory (30 m left)"), "{listed}");
    assert!(listed.contains("ip 192.0.2.9 (1 h left)") || listed.contains("ip 192.0.2.9 (2 h left)"), "{listed}");
    assert!(admin.run(&mut r.server, "ban name Bad 5x").contains("not a time") && !r.server.bans.contains("bad", ip("1.2.3.4")));
    assert!(admin.run(&mut r.server, "status").contains("2 bans"), "status counts the timed ones");
    assert!(admin.run(&mut r.server, "unban Mallory").contains("not banned any more") && !r.server.bans.contains("mallory", ip("1.2.3.4")));
    // for good replaces a timed ban, and a timed ban never shortens one for good
    admin.run(&mut r.server, "ban name Zed 10m");
    admin.run(&mut r.server, "ban name Zed");
    assert!(r.server.bans.names.contains("zed") && !r.server.bans.timed_names.contains_key("zed"));
    admin.run(&mut r.server, "ban name Zed 10m");
    assert!(r.server.bans.names.contains("zed") && !r.server.bans.timed_names.contains_key("zed"));
}

#[test]
fn a_private_event_reaches_the_player_it_is_about_and_nobody_else() {
    // `raid_alarm own` in the forward text: told to the player the event is about, wherever it is; `blast 100` as before: to everyone within 100 m
    let mut sim = server_scene("");
    let host = sim.world.entity(4).unwrap();
    let text = sim.world.strings_mut().intern("build_placed 100\nraid_alarm own\nblast 100");
    sim.world.get_mut::<lunaris_sim::components::NetHost>(host).unwrap().forward = lunaris_ecs::Sym(text);
    // a piece of a base far from both players and a cupboard beside it, whose owner is set below
    spawn(&mut sim, 70040, r#"{"Transform":{"pos":[30,0.25,30]},"Collider":{"shape":1,"size":[4,0.5,4]},"BuildPiece":{"kind":"foundation","anchored":true,"checked":true,"owner":1},"HitPoints":{"value":1000000,"max":1000000,"last":1000000}}"#);
    spawn(&mut sim, 70041, r#"{"Transform":{"pos":[34,0.6,30]},"BuildUpkeep":{"owner":0,"radius":20},"RaidAlarm":{"throttle":30}}"#);
    let mut r = rig(SimGame::new(sim), 2);
    r.run(3000, |_, _| {});
    let (a, _) = local(&r.sims[0]);
    let (b, _) = local(&r.sims[1]);
    assert!(a.id != 0 && b.id != 0 && a.id != b.id);
    // the cupboard is player 0's: the owner number is the slot of its avatar plus one
    let mut owner = 0;
    let _ = r.server.game.sim.world.for_each::<(&lunaris_sim::components::NetAvatar,), _>(|_, (n,)| {
        if n.id == a.id {
            owner = n.slot + 1;
        }
    });
    assert!(owner != 0);
    let cup = r.server.game.sim.world.entity(70041).unwrap();
    r.server.game.sim.world.get_mut::<lunaris_sim::components::BuildUpkeep>(cup).unwrap().owner = owner;
    spawn(&mut r.server.game.sim, 900_000, r#"{"Transform":{"pos":[30,0.6,30]},"Explosive":{"fuse":0,"radius":5,"damage":100,"core":2,"structure_mult":1,"cover":1}}"#);
    r.run(1500, |_, _| {});
    let uids = |r: &Rig, i: usize, name: &str| -> Vec<u64> { r.sims[i].scripts.events.iter().filter(|e| e.0 == name).map(|e| e.1).collect() };
    assert!(events(&r.sims[0], "blast") >= 1 && events(&r.sims[1], "blast") >= 1, "the blast itself is told to both, by radius");
    assert_eq!(uids(&r, 0, "raid_alarm"), vec![a.id as u64], "the owner is told, and the event is about itself");
    assert_eq!(events(&r.sims[1], "raid_alarm"), 0, "the other player is not told, though it is within 100 m of the blast");
}
