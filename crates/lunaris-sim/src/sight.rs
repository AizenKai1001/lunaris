//! Line of sight against the static world, for a game server that must not tell a player where somebody is that the player could not see (docs/NETWORK_SCALE.md, "Wallhacks").
//!
//! [`Sight`] is the world as a ray sees it, gathered once from a simulation and then asked any number of questions: the `Collider`s that do not move (boxes, balls and capsules, turned or not), the
//! `ConvexCollider` hulls, the `MeshCollider` levels and the `Terrain`, with the same shapes and the same rules as the wheels of a `Vehicle` and the sight of an AI (`vehicle_ray`). Bodies that
//! move (a `RigidBody` with mass, every `CharacterBody`) and triggers are not in it, so a crate that is pushed or a player who walks past blocks nothing. What `CollisionLayer` keeps from colliding
//! with layer 0 (a window's glass, a hedge a game wants to see through) is not there either. A ray that starts inside a solid does not see that solid.
pub use crate::vehicle_ray::Changed;
use crate::vehicle_ray::RayWorld;
use crate::components::{RigidBody, Velocity};
use crate::Sim;
use crate::layers::Filter;

type V3 = [f32; 3];

/// A hit that ends closer than this to the place a line is going to is the target's own floor or the wall it stands against, not something in the way.
pub const SLACK: f32 = 0.25;

/// The static world, gathered for line-of-sight tests.
pub struct Sight {
    rw: RayWorld,
    sig: u64,
}

impl Sight {
    /// Gathers the static things of the simulation's world (all of them: the world has no edge). What moves on its own is not in it: besides the bodies and the characters that `RayWorld` leaves out, a `Collider` with a
    /// `Velocity` and no `RigidBody` (a drone, which a `FlyAgent` flies about) is not a wall; left in, it made every gathering differ from the last, so a server that keeps its answers (`signature`) asked all of them again
    /// every half second.
    pub fn gather(sim: &mut Sim) -> Sight {
        let mut flying: Vec<u64> = Vec::new();
        let _ = sim.world.for_each_uid::<(&crate::components::Collider, &Velocity), _>(|_, uid, _| flying.push(uid));
        let except: std::collections::BTreeSet<u64> = flying.into_iter().filter(|uid| sim.world.entity(*uid).is_some_and(|e| sim.world.get::<RigidBody>(e).is_none())).collect();
        let rw = RayWorld::gather_except(&mut sim.world, &sim.heightfields, &sim.meshes, None, &except);
        let sig = rw.signature();
        Sight { rw, sig }
    }

    /// As [`Sight::gather`], but only what is within the box `lo`..`hi` (what a vehicle, a drone or a gun can touch near it): the rest of the world is not looked at more than it takes to see that it is far.
    pub fn gather_in(sim: &mut Sim, lo: V3, hi: V3) -> Sight {
        let rw = RayWorld::gather(&mut sim.world, &sim.heightfields, &sim.meshes, Some((lo, hi)));
        let sig = rw.signature();
        Sight { rw, sig }
    }

    /// Can a person at `from` see `to`? False when something solid is on the straight line between them (the last `SLACK` metres before `to` do not count).
    pub fn clear(&self, from: V3, to: V3) -> bool {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if !(dist > 1.0e-4) {
            return true;
        }
        let dir = [d[0] / dist, d[1] / dist, d[2] / dist];
        !self.rw.occluded(from, dir, dist - SLACK, Filter::DEFAULT)
    }

    /// What is different in this world from `old`, gathered earlier (see [`Changed`]): a game that keeps the answers of line-of-sight tests asks again only about the lines that go through it.
    pub fn changed_since(&self, old: &Sight) -> Changed {
        self.rw.changed_since(&old.rw)
    }

    /// A number that is the same for two gatherings of the same world and changes when a wall is built or taken down, a door swings or a thing is moved (see `RayWorld::signature`).
    pub fn signature(&self) -> u64 {
        self.sig
    }

    /// How many shapes there are.
    pub fn shapes(&self) -> usize {
        self.rw.shapes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    use lunaris_json::{parse, Json};

    fn spawn(s: &mut Sim, uid: u64, comps: &str) {
        let Json::Obj(components) = parse(comps).unwrap() else { panic!() };
        s.apply(&Command::Spawn { uid: Some(uid), components }).unwrap();
    }

    #[test]
    fn a_wall_blocks_the_line_and_a_mover_a_trigger_and_the_ground_do_not() {
        let mut s = Sim::new(1);
        // a floor, a wall at x = 10 (2 m thick, 4 m high, 20 m wide), a body that moves and a trigger, both at x = -10
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]}}"#);
        spawn(&mut s, 2, r#"{"Transform":{"pos":[10,2,0]},"Collider":{"shape":1,"size":[2,4,20]}}"#);
        spawn(&mut s, 3, r#"{"Transform":{"pos":[-10,2,0]},"Collider":{"shape":1,"size":[2,4,20]},"RigidBody":{"mass":5},"Velocity":{}}"#);
        spawn(&mut s, 4, r#"{"Transform":{"pos":[-20,2,0]},"Collider":{"shape":1,"size":[2,4,20],"trigger":true}}"#);
        let sight = Sight::gather(&mut s);
        assert!(!sight.clear([0.0, 1.6, 0.0], [20.0, 1.1, 0.0]), "through the wall");
        assert!(sight.clear([0.0, 1.6, 0.0], [5.0, 1.1, 0.0]), "short of the wall");
        assert!(sight.clear([0.0, 1.6, 0.0], [20.0, 6.0, 40.0]), "round the end of the wall");
        assert!(sight.clear([0.0, 6.0, 0.0], [20.0, 6.0, 0.0]), "over the wall");
        assert!(sight.clear([0.0, 1.6, 0.0], [-30.0, 1.1, 0.0]), "a body that moves and a trigger are not walls");
        assert!(sight.clear([0.0, 1.6, 0.0], [9.1, 1.1, 0.0]), "a thing that stands against the wall (0.1 m into it) is not hidden by it");
        assert!(!sight.clear([0.0, 1.6, 0.0], [9.6, 1.1, 0.0]), "half a metre into the wall is behind it");
        assert!(sight.shapes() >= 2);
        // the same world gives the same signature, a moved wall another
        let again = Sight::gather(&mut s);
        assert_eq!(sight.signature(), again.signature());
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<crate::components::Transform>(e).unwrap().pos[0] = 12.0;
        assert_ne!(sight.signature(), Sight::gather(&mut s).signature());
    }

    #[test]
    fn a_gather_with_a_reach_holds_exactly_the_colliders_that_reach_the_box() {
        let mut s = Sim::new(1);
        // a lattice of boxes of every size and scale, some of them tilted and some of them so big that they reach far
        for k in 0..400u64 {
            let (i, j) = ((k % 20) as f32, (k / 20) as f32);
            let size = 1.0 + (k % 7) as f32;
            let scale = 1.0 + (k % 3) as f32 * 0.5;
            spawn(&mut s, 1 + k, &format!(r#"{{"Transform":{{"pos":[{},1,{}],"scale":[{scale},{scale},{scale}],"rot":[0,0.2,0,0.98]}},"Collider":{{"shape":{},"size":[{size},{size},{size}]}}}}"#, i * 10.0, j * 10.0, k % 3));
        }
        spawn(&mut s, 5000, r#"{"Transform":{"pos":[0,0,95]},"Collider":{"shape":1,"size":[400,2,2]}}"#);
        let all = crate::vehicle_ray::RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, None);
        assert_eq!(all.shapes(), 401);
        for reach in [([35.0, -10.0, 35.0], [75.0, 10.0, 75.0]), ([-5.0, -10.0, -5.0], [5.0, 10.0, 5.0]), ([500.0, -10.0, 500.0], [510.0, 10.0, 510.0]), ([100.0, -10.0, 93.0], [101.0, 10.0, 94.0])] {
            let some = crate::vehicle_ray::RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, Some(reach));
            // (every ray that goes through the reach has the same answer in both: the exact rule is the same, only the work is less)
            for k in 0..200 {
                let f = k as f32 * 0.37;
                let o = [reach.0[0] + (reach.1[0] - reach.0[0]) * (f.sin() * 0.5 + 0.5), 5.0, reach.0[2] + (reach.1[2] - reach.0[2]) * (f.cos() * 0.5 + 0.5)];
                let d = [0.0, -1.0, 0.0];
                assert_eq!(all.cast(o, d, 10.0, crate::layers::Filter::DEFAULT, 0).map(|h| (h.uid, h.t.to_bits())), some.cast(o, d, 10.0, crate::layers::Filter::DEFAULT, 0).map(|h| (h.uid, h.t.to_bits())), "reach {reach:?} ray {k}");
            }
            assert!(some.shapes() <= all.shapes());
        }
        let far = crate::vehicle_ray::RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, Some(([500.0, -10.0, 500.0], [510.0, 10.0, 510.0])));
        assert_eq!(far.shapes(), 0, "nothing reaches a box far from everything");
        let tiny = crate::vehicle_ray::RayWorld::gather(&mut s.world, &s.heightfields, &s.meshes, Some(([100.0, -10.0, 93.0], [101.0, 10.0, 94.0])));
        assert!(tiny.shapes() >= 1 && tiny.shapes() < 20, "the long thin box reaches it: {}", tiny.shapes());
    }

    #[test]
    fn a_flying_body_with_no_rigid_body_is_no_wall_and_does_not_change_the_signature_as_it_moves() {
        let mut s = Sim::new(1);
        spawn(&mut s, 1, r#"{"Transform":{"pos":[0,-0.5,0]},"Collider":{"shape":1,"size":[200,1,200]}}"#);
        // a drone: a collider and a velocity, flown about by an agent, not pushed by the physics
        spawn(&mut s, 2, r#"{"Transform":{"pos":[10,2,0]},"Collider":{"shape":1,"size":[2,4,20]},"Velocity":{"lin":[1,0,0]}}"#);
        let a = Sight::gather(&mut s);
        assert!(a.clear([0.0, 1.6, 0.0], [20.0, 1.1, 0.0]), "a drone is not a wall");
        let e = s.world.entity(2).unwrap();
        s.world.get_mut::<crate::components::Transform>(e).unwrap().pos[0] = 15.0;
        let b = Sight::gather(&mut s);
        assert_eq!(a.signature(), b.signature(), "it moved, the world the rays see did not change");
        // a plain collider stays a wall
        spawn(&mut s, 3, r#"{"Transform":{"pos":[10,2,5]},"Collider":{"shape":1,"size":[2,4,2]}}"#);
        assert!(!Sight::gather(&mut s).clear([0.0, 1.6, 5.0], [20.0, 1.1, 5.0]), "a plain collider is a wall");
    }
}
