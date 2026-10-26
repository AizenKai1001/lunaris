//! What a game server owns in the city (milestone M4 of the game's `docs/41-the-map.md`): the doors of the buildings, as entities of the engine's standard components (`Door`, `Interactable`, `NetEntity`), so that the
//! server decides who opens what, a door can be broken or locked by the game's own systems, and every player is told the same state. A server draws nothing: the clients draw the leaf from the `NetEntity`'s numbers.
//!
//! The leaf of a door has a size of its own (the doorways of the city are 0.8 to 3 m wide), which the client's puppet cannot know from a prefab of fixed size. A still thing (`NetEntity` class 3) is told to the clients with its place, its
//! facing, its `flags` and its kind and nothing else, so the size is in the flags: bit 0 and 1 are the engine's (a door is open, a container is open), bit 2 says that the leaf swings the way of a positive angle, bits 3 to 8 the width and
//! bits 9 to 14 the height, each in steps of 7.5 cm (`pack_flags`, `unpack_flags`); the game's own client system sets the puppet's scale, hinge and sense from them (an example game's build script does).
//!
//! The game says what else its doors and containers are (hit points, a team, a lock) in `city/city.json`, `"server"`: `{"door_kind": 35, "door": {"HitPoints": {...}}, "container_kind": 31, "container": {...}, "tables": {"Kitchen": "city_kitchen"}}`;
//! every component named there is added to the entity the city makes.
//!
//! `"door_socket": "edge"` also puts a `BuildSocket` of that type in every street doorway, on the threshold, facing out into the street: a foundation of the game's building rules joins it (a porch that is held as if it stood
//! on the ground), a wall on that edge bars the doorway (docs/BUILDING.md, "Building onto generated buildings").

use crate::mesh::*;
use crate::runtime::DoorEnt;
use lunaris_json::Json;

/// The step of a size in the flags of a door, in metres.
pub const SIZE_STEP: f32 = 0.075;

/// The flags of a door's `NetEntity`: the way it swings and its size.
pub fn pack_flags(l: &Leaf) -> u32 {
    let q = |m: f32| ((m / SIZE_STEP).round() as i64).clamp(0, 63) as u32;
    (if l.dir > 0.0 { 4 } else { 0 }) | (q(l.w) << 3) | (q(l.h) << 9)
}

/// What `pack_flags` says: (swings the way of a positive angle, width, height) in metres.
pub fn unpack_flags(flags: u32) -> (bool, f32, f32) {
    (flags & 4 != 0, ((flags >> 3) & 63) as f32 * SIZE_STEP, ((flags >> 9) & 63) as f32 * SIZE_STEP)
}

/// How thick the leaf of a door is for the server: the doorway's wall is thicker, the leaf stands in the middle of it.
pub const LEAF_T: f32 = 0.09;
/// How far a door opens, in degrees.
pub const OPEN_DEG: f32 = 100.0;

/// What the game adds to the entities the city makes for a server.
#[derive(Clone, Debug, Default)]
pub struct Parts {
    /// The `NetEntity.kind` of a door (the clients pick the picture by it).
    pub door_kind: u32,
    /// Components added to every door (`HitPoints`, `Team`, ...).
    pub door: Vec<(String, Json)>,
    /// The `NetEntity.kind` of a container.
    pub container_kind: u32,
    /// Components added to every container.
    pub container: Vec<(String, Json)>,
    /// For each kind of room (`Kitchen`, `Bedroom`, ...) the loot table of the container that stands in it (`LootContainer.table`); a room not named has none.
    pub tables: Vec<(String, String)>,
    /// Seconds after which a looted container fills again (`LootContainer.refill`; 0 never).
    pub refill: f32,
    /// The `BuildSocket.kind` put on the threshold of every street door (empty: none), see `socket_entity`.
    pub door_socket: String,
    /// The sockets of the street doors are only places to snap to, not solid ground to what joins them (`"door_socket_anchor": false`): a piece that is held by one of them alone collapses when the socket goes away, and the sockets are
    /// entities of the building's zone, which is unloaded when nobody is near.
    pub door_socket_loose: bool,
}

impl Parts {
    /// The `"server"` object of `city/city.json`.
    pub fn from_json(doc: &Json) -> Parts {
        let comps = |v: Option<&Json>| -> Vec<(String, Json)> {
            match v {
                Some(Json::Obj(o)) => o.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
                _ => Vec::new(),
            }
        };
        let num = |k: &str| doc.get(k).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as u32;
        Parts {
            door_kind: num("door_kind"),
            door: comps(doc.get("door")),
            container_kind: num("container_kind"),
            container: comps(doc.get("container")),
            tables: comps(doc.get("tables")).into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect(),
            refill: doc.get("refill").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            door_socket: doc.get("door_socket").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            door_socket_loose: matches!(doc.get("door_socket_anchor"), Some(Json::Bool(false))),
        }
    }
}

/// Where the leaf of a door stands and how it turns (relative to `origin`, the centre of the block): the numbers the single-player city and the server's both make their entity from.
#[derive(Clone, Copy, Debug)]
pub struct Leaf {
    /// The hinge, relative to the origin.
    pub hx: f32,
    pub hz: f32,
    /// The middle of the leaf.
    pub cx: f32,
    pub cy: f32,
    pub cz: f32,
    /// The directions along the leaf and out of the building (unit, on the ground plane).
    pub ax: f32,
    pub az: f32,
    pub ox: f32,
    pub oz: f32,
    /// The yaw of the leaf (rotation about +y) with its x axis along the leaf, and the sign of the turn that swings it into the building.
    pub yaw0: f32,
    pub dir: f32,
    /// The size of the leaf and half its width.
    pub w: f32,
    pub h: f32,
    pub half: f32,
}

/// The leaf of the door `d` in a block whose centre is `origin`: a little narrower and lower than the doorway, hung on the hinge side.
pub fn leaf(d: &DoorEnt, origin: V3) -> Leaf {
    let hinge = add(d.hinge, scale(d.along, 0.045));
    let (w, h) = (d.width - 0.1, d.height - 0.06);
    let cy = hinge[1] + 0.03 + h * 0.5 - origin[1];
    let (hx, hz) = (hinge[0] - origin[0], hinge[2] - origin[2]);
    let unit = |v: V3| {
        let l = (v[0] * v[0] + v[2] * v[2]).sqrt().max(1e-6);
        (v[0] / l, v[2] / l)
    };
    let (ax, az) = unit(d.along);
    let (ox, oz) = unit(d.out);
    let yaw0 = (-az).atan2(ax);
    // the sense in which the yaw changes when the leaf swings into the building (towards minus `out`)
    let e = 0.2f32;
    let (bx, bz) = (ax * e.cos() - ox * e.sin(), az * e.cos() - oz * e.sin());
    let mut dy = (-bz).atan2(bx) - yaw0;
    while dy > std::f32::consts::PI {
        dy -= std::f32::consts::TAU;
    }
    while dy < -std::f32::consts::PI {
        dy += std::f32::consts::TAU;
    }
    let dir = if dy >= 0.0 { 1.0f32 } else { -1.0 };
    let half = w * 0.5;
    Leaf { hx, hz, cx: hx + ax * half, cy, cz: hz + az * half, ax, az, ox, oz, yaw0, dir, w, h, half }
}

/// A door for a game server: the box that blocks the way (and is what a shot or an eye meets), the standard `Door` that swings it about its hinge and the `Interactable` that opens it, and the `NetEntity` that tells the
/// clients about it (its size and the way it swings in the `flags`: see the top of this file); then what the game added.
pub fn door_entity(d: &DoorEnt, origin: V3, parts: &Parts) -> Json {
    let n = |x: f32| Json::Float(x as f64);
    let v3 = |a: [f32; 3]| Json::arr(a.map(n));
    let l = leaf(d, origin);
    let rot = [0.0, (l.yaw0 * 0.5).sin(), 0.0, (l.yaw0 * 0.5).cos()];
    let mut comps: Vec<(String, Json)> = vec![
        ("LocalTransform".into(), Json::obj([("pos", v3([l.cx, l.cy, l.cz])), ("rot", Json::arr(rot.map(n))), ("scale", v3([l.w, l.h, LEAF_T]))])),
        ("Collider".into(), Json::obj([("shape", Json::from(1)), ("size", v3([1.0, 1.0, 1.0])), ("trigger", Json::Bool(false))])),
        ("Interactable".into(), Json::obj([("kind", Json::from(1)), ("prompt", Json::str("Open"))])),
        ("Door".into(), Json::obj([("hinge", v3([-l.half, 0.0, 0.0])), ("angle_open", n(l.dir * OPEN_DEG)), ("speed", n(200.0))])),
        (
            "NetEntity".into(),
            Json::obj([("class", Json::from(3)), ("kind", Json::from(parts.door_kind as i64)), ("flags", Json::from(pack_flags(&l) as i64))]),
        ),
    ];
    for (k, v) in &parts.door {
        comps.retain(|(name, _)| name != k);
        comps.push((k.clone(), v.clone()));
    }
    Json::obj([("name", Json::str("Door")), ("components", Json::Obj(comps))])
}

/// How far in front of the wall plane the socket of a doorway stands, in metres: a piece that joins it starts at the face of the wall, not inside it.
pub const SOCKET_OUT: f32 = 0.12;

/// The `BuildSocket` of a street doorway (None for a door between rooms, or when the game names no `door_socket`): at the middle of the threshold, on the floor of the doorway, `SOCKET_OUT` in front of the wall plane, turned
/// to face out into the street (its local -z is the way out, which the pieces' `turn` is relative to). It is an anchor unless the game says `"door_socket_anchor": false`: the building is then solid ground to what joins it.
pub fn socket_entity(d: &DoorEnt, origin: V3, parts: &Parts) -> Option<Json> {
    if !d.street || parts.door_socket.is_empty() {
        return None;
    }
    let n = |x: f32| Json::Float(x as f64);
    let l = leaf(d, origin);
    // (the middle of the doorway: the hinge plus half the width along it, not the leaf's middle, which is narrower)
    let mid = [d.hinge[0] - origin[0] + l.ax * d.width * 0.5, d.hinge[1] - origin[1], d.hinge[2] - origin[2] + l.az * d.width * 0.5];
    let pos = [mid[0] + l.ox * SOCKET_OUT, mid[1], mid[2] + l.oz * SOCKET_OUT];
    let yaw = (-l.ox).atan2(-l.oz);
    Some(Json::obj([
        ("name", Json::str("Door socket")),
        (
            "components",
            Json::obj([
                ("LocalTransform", Json::obj([("pos", Json::arr(pos.map(n))), ("rot", Json::arr([0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()].map(n)))])),
                ("BuildSocket", Json::obj([("kind", Json::str(parts.door_socket.clone())), ("anchor", Json::Bool(!parts.door_socket_loose))])),
            ]),
        ),
    ]))
}

/// A container for a game server, where something to search stands: the box (`c`: the place on the floor, the turn in degrees, the size, and the number of the kind of room in `interior::SEARCH_ROOMS`) that is what a player looks at, the
/// engine's `Interactable` of kind 2, an `Inventory`, and the `LootContainer` that fills it from the table of its room's kind, rolled from `key` (the same container holds the same things in a world); and the `NetEntity` that tells the clients
/// about it. None when the game has no table for that kind of room.
pub fn container_entity(c: &[f32; 8], key: u32, parts: &Parts) -> Option<Json> {
    let n = |x: f32| Json::Float(x as f64);
    let v3 = |a: [f32; 3]| Json::arr(a.map(n));
    let room = crate::interior::SEARCH_ROOMS.get(c[7] as usize)?;
    let table = parts.tables.iter().find(|(k, _)| k == room).map(|(_, t)| t.clone())?;
    let half = c[3].to_radians() * 0.5;
    let size = [c[4].max(0.2), c[5].max(0.2), c[6].max(0.2)];
    let mut comps: Vec<(String, Json)> = vec![
        ("LocalTransform".into(), Json::obj([("pos", v3([c[0], c[1] + size[1] * 0.5, c[2]])), ("rot", Json::arr([0.0, half.sin(), 0.0, half.cos()].map(n)))])),
        ("Collider".into(), Json::obj([("shape", Json::from(1)), ("size", v3(size)), ("trigger", Json::Bool(false))])),
        ("Interactable".into(), Json::obj([("kind", Json::from(2)), ("prompt", Json::str("Search"))])),
        ("Inventory".into(), Json::obj([("size", Json::from(8))])),
        ("LootContainer".into(), Json::obj([("table", Json::str(table)), ("key", Json::from(key as i64)), ("refill", n(parts.refill))])),
        ("NetEntity".into(), Json::obj([("class", Json::from(3)), ("kind", Json::from(parts.container_kind as i64))])),
    ];
    for (k, v) in &parts.container {
        comps.retain(|(name, _)| name != k);
        comps.push((k.clone(), v.clone()));
    }
    Some(Json::obj([("name", Json::str("Container")), ("components", Json::Obj(comps))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door(along: V3, out: V3) -> DoorEnt {
        DoorEnt { hinge: [10.0, 0.15, -4.0], along, out, width: 1.2, height: 2.3, glass: false, open: 0.0, street: true }
    }

    /// Where the middle of the leaf is after it has turned `angle` degrees about its hinge (a rotation about +y, as the engine's `Door` does).
    fn swung(l: &Leaf, angle: f32) -> (f32, f32) {
        let (s, c) = angle.to_radians().sin_cos();
        let (rx, rz) = (l.cx - l.hx, l.cz - l.hz);
        // a rotation about +y by a: x' = x cos a + z sin a, z' = -x sin a + z cos a
        (l.hx + rx * c + rz * s, l.hz - rx * s + rz * c)
    }

    #[test]
    fn a_door_swings_into_the_building_whichever_way_its_wall_faces() {
        for (along, out) in [([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0]), ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0]), ([0.0, 0.0, 1.0], [-1.0, 0.0, 0.0])] {
            let l = leaf(&door(along, out), [0.0; 3]);
            // shut: the leaf lies along the doorway, from the hinge
            assert!(((l.cx - l.hx) * l.ax + (l.cz - l.hz) * l.az - l.half).abs() < 1e-4, "{along:?} {out:?}");
            let (x, z) = swung(&l, l.dir * OPEN_DEG);
            let into = -((x - l.hx) * l.ox + (z - l.hz) * l.oz);
            assert!(into > l.half * 0.9, "the leaf swung to {into:.2} m inside for {along:?} out {out:?} (half width {})", l.half);
        }
    }

    #[test]
    fn a_street_doorway_gets_a_socket_on_its_threshold_facing_out() {
        let parts = Parts { door_socket: "edge".into(), ..Parts::default() };
        for (along, out) in [([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0]), ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]), ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0])] {
            let d = door(along, out);
            let j = socket_entity(&d, [0.0; 3], &parts).expect("a street door has one");
            let c = j.get("components").unwrap();
            let sock = c.get("BuildSocket").unwrap();
            assert_eq!(sock.get("kind").and_then(|v| v.as_str()), Some("edge"));
            let t = c.get("LocalTransform").unwrap();
            let p: Vec<f32> = t.get("pos").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
            let q: Vec<f32> = t.get("rot").unwrap().as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
            // on the doorway's middle (1.2 m wide: 0.6 along from the hinge), the floor level of the hinge, a hand in front of the wall plane
            let want = [10.0 + along[0] * 0.6 + out[0] * SOCKET_OUT, 0.15, -4.0 + along[2] * 0.6 + out[2] * SOCKET_OUT];
            assert!((p[0] - want[0]).abs() < 1e-4 && (p[1] - want[1]).abs() < 1e-4 && (p[2] - want[2]).abs() < 1e-4, "{p:?} vs {want:?}");
            // its local -z, turned by the rotation (a rotation about +y by a: x' = x cos a + z sin a, z' = -x sin a + z cos a, with the quaternion's w = cos(a/2), y = sin(a/2)), is the way out
            let a = 2.0 * q[1].atan2(q[3]);
            let (fx, fz) = (-a.sin(), -a.cos());
            assert!(fx * out[0] + fz * out[2] > 0.999, "faces ({fx:.2}, {fz:.2}) for out {out:?}");
        }
    }

    #[test]
    fn no_socket_on_a_door_between_rooms_or_when_the_game_asks_for_none() {
        let parts = Parts { door_socket: "edge".into(), ..Parts::default() };
        let mut inner = door([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        inner.street = false;
        assert!(socket_entity(&inner, [0.0; 3], &parts).is_none());
        assert!(socket_entity(&door([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), [0.0; 3], &Parts::default()).is_none());
        let doc = lunaris_json::parse(r#"{"door_kind": 35, "door_socket": "edge"}"#).unwrap();
        assert_eq!(Parts::from_json(&doc).door_socket, "edge");
        assert!(!Parts::from_json(&doc).door_socket_loose, "an anchor unless the game says otherwise");
        let loose = Parts { door_socket: "edge".into(), door_socket_loose: true, ..Parts::default() };
        let j = socket_entity(&door([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), [0.0; 3], &loose).unwrap();
        assert_eq!(j.get("components").unwrap().get("BuildSocket").unwrap().get("anchor"), Some(&Json::Bool(false)));
        let doc = lunaris_json::parse(r#"{"door_socket": "edge", "door_socket_anchor": false}"#).unwrap();
        assert!(Parts::from_json(&doc).door_socket_loose);
    }

    #[test]
    fn the_door_entity_carries_the_standard_components_and_the_games_own() {
        let parts = Parts { door_kind: 35, door: vec![("HitPoints".into(), Json::obj([("value", Json::from(400))])), ("Interactable".into(), Json::obj([("kind", Json::from(1)), ("prompt", Json::str("Use"))]))], ..Parts::default() };
        let j = door_entity(&door([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), [0.0; 3], &parts);
        let c = j.get("components").unwrap();
        for name in ["LocalTransform", "Collider", "Interactable", "Door", "NetEntity", "HitPoints"] {
            assert!(c.get(name).is_some(), "{name}");
        }
        let net = c.get("NetEntity").unwrap();
        assert_eq!(net.get("kind").and_then(|v| v.as_i64()), Some(35));
        let (positive, w, h) = unpack_flags(net.get("flags").and_then(|v| v.as_i64()).unwrap() as u32);
        assert!((w - 1.1).abs() <= SIZE_STEP * 0.5 + 1e-4 && (h - 2.24).abs() <= SIZE_STEP * 0.5 + 1e-4, "the leaf is 1.1 m wide and 2.24 high (the doorway less a hand): {w} by {h}");
        let l = leaf(&door([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), [0.0; 3]);
        assert_eq!(positive, l.dir > 0.0);
        // what the game says replaces what the city made
        assert_eq!(c.get("Interactable").unwrap().get("prompt").and_then(|v| v.as_str()), Some("Use"));
    }
}
