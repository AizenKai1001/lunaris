# Physics

Deterministic "physics-lite" in `crates/lunaris-sim/src/physics.rs` and `physics_rot.rs`: enough for the world to have consequences (things fall,
collide, bounce, rest, roll, tip over, swing) with no dependencies. It is built to give the same bits on every machine and thread count, so a replay
and a networked game agree.

## Bodies

| you have | it is |
|---|---|
| `Transform` + `Collider` | a **static collider** |
| `Transform` + `Collider` + `RigidBody` (`mass > 0`) + `Velocity` | a **dynamic body**: gravity, drag, contacts. It slides and falls but never turns |
| the same + `AngularVelocity` | a body that **turns**: contacts push at a point, so balls roll, boxes tip and topple, things tumble (below) |
| `Collider.trigger = true` | an overlap that is reported (`contacts`, `on_contact` systems) but not pushed apart |
| `CharacterBody` | a walking capsule moved by the character controller instead (see `docs/GAMES.md`, `examples/walker.lunaris.json`) |
| `Terrain` | static ground from a heightmap (`docs/TERRAIN.md`) |
| `Transform` + `MeshCollider` | static ground, walls and ramps made of the triangles of a model file: a whole level (below) |
| `Transform` + `ConvexCollider` (+ `RigidBody` + `Velocity` + `AngularVelocity`) | a solid with the shape of a model's convex hull: a rock, a crate, a wedge (below). Use it instead of a `Collider` |
| `Joint` | a rope, rod or spring between a dynamic body and another body or a point; or a ball-and-socket or a hinge (below) |
| `Vehicle` (+ `Collider` + `RigidBody` + `Velocity` + `AngularVelocity`) | a raycast car: a chassis body held up and driven by wheel rays with springs, tyres, an engine and brakes (below) |
| `CollisionLayer` | which things collide with which: a `layer` (0 to 31) and a `mask` of the layers it collides with, for every shape, trigger, character, agent, ray, sensor and wheel (below) |

`Collider.shape` 0 is a sphere (radius `size[0] / 2` times the largest `Transform.scale`), 1 a box (`size` times `scale`), 2 a capsule: a cylinder with a
half-sphere on each end, along the entity's own y axis, `size[0]` across and `size[1]` long in all (so `size` [0.5, 2, 0.5] is 0.25 m in radius and 2 m
from end to end; `size[2]` is not used by physics, set it to `size[0]`). A box is **axis-aligned** until its entity's `Transform.rot` is turned or it
has an `AngularVelocity`: then it is an **oriented box** that turns with the entity. A sphere has no orientation, so its `rot` means nothing to physics.

`RigidBody` has `mass`, `restitution`, `friction`, `gravity_scale` and `drag`; contacts take the larger restitution of the two bodies and the geometric
mean of the two frictions. Gravity is `PhysicsSettings.gravity` on one entity (the lowest uid wins), default (0, -9.8, 0).

Each tick: gravity and drag, spring forces, then bodies that overlap (found by sort-and-sweep along the axis where they are most spread out) are
separated and their velocities adjusted by sequential impulses (restitution, Coulomb friction, rods and ropes); contacts are processed in
uid order, so the outcome depends on nothing but the world.

## Rotation

Give a dynamic body an `AngularVelocity` (`w`: radians per second about the world x, y and z axes, `damping`: the fraction of the spin it loses per second,
0.05 by default) and it turns. Every tick the engine turns its `Transform.rot` by `w` (and keeps it a unit quaternion), the contacts change `w` the way
they change `Velocity.lin`, and you can read both back or set them from a graph system (a kick: set `w`). Do not use `Velocity.ang` on such a body: that
is the spin of things that have no physics, applied by another system.

```
{"Transform": {"pos": [0, 0.5, 0]}, "Velocity": {}, "AngularVelocity": {},
 "Collider": {"shape": 1, "size": [0.2, 1.0, 0.6]}, "RigidBody": {"mass": 0.3, "friction": 0.5}}
```

What it does:

* **Contacts have points.** Two boxes touching face to face make up to four contact points, a ball on the floor one, a capsule lying on the floor two, a
  box on a terrain up to four. Each point pushes along the contact normal at its own place, so an impulse that is not through the centre of mass makes the
  body turn, and a ball that is pushed along the floor gets friction at its contact point and starts to roll (a solid ball sliding at 7 m/s ends up
  rolling at 5 m/s, spinning at speed / radius; a ball let go on a slope runs down it at 5/7 of the acceleration a sliding block would have).
* **Mass is spread out.** The moment of inertia comes from the shape: a solid sphere, a solid box, a capsule (a cylinder and two half-spheres sharing the
  mass by volume). A long thin box is easy to spin about its length and hard to spin end over end. Free tumbling keeps its angular momentum (the engine
  integrates the gyroscopic term, implicitly, so it stays stable).
* **Friction holds.** A box on a slope stays put while `friction` is above the tangent of the slope, and slides when it is not; a domino stands on its
  base until it is pushed past its tipping angle, and a row of them falls one onto the next (`examples/dominoes.lunaris.json`).
* **Fast balls do not tunnel.** A sphere with an `AngularVelocity` that would move farther than its own radius in the next step is tried against
  every solid static body on the way (walls, floors, ramps, terrain), however thin: if it would hit one, its `Velocity.lin` is cut to the share of the
  step that ends exactly at the surface and its real velocity is kept in `AngularVelocity.stash` for that one tick, so the bounce is the bounce of
  the real speed at the real place. Only spinning spheres get this, and only against static things; without an `AngularVelocity` a ball is the old kind
  of ball (it can still fly through a wall thinner than its step).
* **No rolling resistance.** A ball on a floor rolls on until something stops it; `AngularVelocity.damping` is the knob (a ball that should come to
  rest after a while wants 0.3 or more).
* **Limits.** The spin is at most 60 radians per second. A body whose `Transform.rot` is zero or not a number counts as unturned.

A body without an `AngularVelocity` and with an unturned box (or a sphere) collider takes exactly the old code path, so a scene that does not use
rotation behaves bit for bit as before. A body that does not turn but rests on one that does (or on a rotated box, or a capsule) is treated as a body
whose moment of inertia is infinite: it is pushed at its centre.

## Joints

`Joint` goes on the body that is pulled. The other end is the entity `target` (a body, or anything with a `Transform`: a hook, a pole) or, with `target`
0, the world point `anchor`.

| `kind` | what it does |
|---|---|
| 0, rope | the ends are never farther apart than `length`; slack when closer |
| 1, rod | the ends stay exactly `length` apart |
| 2, spring | pulls toward `length` with `stiffness` (newtons per metre) and `damping` (newton-seconds per metre); integrated implicitly, so it stays stable however stiff |
| 3, ball-and-socket | one point (`anchor`) belongs to both bodies and they may turn freely about it |
| 4, hinge | as 3, and they may only turn about one `axis`; optionally with limits on the angle |

Kinds 0 to 2 join the centres of the two bodies (a point to point link: a rod is a stiff straight link). A `length` of 0 or less means "as far apart as they
are now": the engine writes the distance into `length` on the first tick. With `width` above 0 both renderers draw the joint as a straight cord of that
thickness in `color`. A chain is one joint per link, each targeting the previous ball. `examples/ropes.lunaris.json` has a wrecking ball on a rod that
scatters a tower, a chain of rods, and a ball on a spring that settles where 60 N/m balance its weight (`tools/build_ropes.py` writes it,
`examples/tests/ropes.test.json` plays it).

```
{"Joint": {"kind": 1, "target": 12, "length": 2.0, "width": 0.07}}
```

**Ball-and-socket and hinge** are for bodies that turn (give both an `AngularVelocity`; a body without one cannot turn, so the joint then acts as a
point that it is pinned at). Say where the joint is with `anchor`, a point in **world coordinates as the scene starts**; the engine turns it into a point
on each of the two bodies the first time it looks at the joint (it writes `local_a`, `local_b`, `local_axis` and `rest`), and from then on the joint
follows the bodies. Between a body and the world use `target` 0, or a body or an entity with a `Transform` as the other end (a post: the entity's own
position and orientation then say where the hinge is). The two bodies do not collide with each other.

* A hinge has an `axis` (world direction at the start, default up (0, 1, 0)) the two bodies turn about, and `limit` [low, high]: the angle of the body
  relative to its target, in radians, 0 as the scene starts, positive the right-handed way about `axis`. It may not leave that range (when `limit[0]` is
  below `limit[1]`; the default [0, 0] means no limit). The limit closes in on its end and stops there, with no bounce.
* A hinged door: a thin tall box that is a body, `{"kind": 4, "target": <the post>, "anchor": <a point on the edge>, "axis": [0, 1, 0], "limit": [0, 1.5]}`.
  Joints in a chain of capsules: one ball-and-socket per link, each `target`ing the one before it, `anchor` the point where the two meet.
* `examples/dominoes.lunaris.json` (`tools/build_dominoes.py` writes it, `examples/tests/dominoes.test.json` plays it) has a ball that rolls down a ramp
  (a box turned 15 degrees) into a winding row of dominoes, the last of which pushes a hinged door open to its limit, and a chain of four capsules on
  ball-and-socket joints swinging from a gantry.
* **Ragdolls**: `examples/ragdolls.lunaris.json` (`tools/build_ragdolls.py`, playtest `examples/tests/ragdolls.test.json`) drops three crash-test dummies, each eleven bodies (pelvis, torso and head as boxes and a ball, arms and legs as capsules) tied by ten ball-and-socket joints, onto a floor with a step and a wall. Nothing drives them: they tumble and go limp, and a test checks that none of them explodes or sinks. With no cone limit on a ball-and-socket the limbs can bend further than a body would, and a joint that is far from where the pieces are bound pulls them together hard: give each joint its `anchor` exactly where the two pieces meet. Give the parts an `AngularVelocity` `damping` of about 0.3 so they come to rest. A character that gets up again swaps the ragdoll for an animated model (despawn one, spawn the other at the pelvis).
* To tie the joint down again from `anchor` (the bodies were moved), set `rest` to [0, 0, 0, 0].

`check` warns about a joint whose owner is not a dynamic body or whose target does not exist.

A long chain of heavy things on six solver rounds stretches a little (the solver pulls the ends back every tick); lighten the end, or add links.

## Mesh colliders: a level made of triangles

A cave, a town or a racetrack made in a modelling program is a nuisance to rebuild from boxes. Put a `MeshCollider` on the entity that shows the
model, and the model's own triangles become static ground, walls and ramps:

```
{"Transform": {"pos": [0, 0, 0]}, "MeshRenderer": {"mesh": "models/cave.glb"}, "MeshCollider": {"mesh": "models/cave.glb"}}
```

| field | what it is |
|---|---|
| `mesh` | the `.glb` or `.gltf` of the workspace whose triangles are the level (`files` lists them): normally the very file the `MeshRenderer` draws |
| `friction` (0.6), `restitution` (0.1) | the surface's own material, combined with the body's like a `Terrain`'s (the larger restitution, the geometric mean of the frictions) |
| `solid` (true) | false turns the collision off; the model may still be drawn |

**What it does.** Things that have a `RigidBody` (balls, boxes, capsules, hulls, with or without an `AngularVelocity`) land on it, rest on it, roll down
it and slide along its walls, and a stack of boxes on a mesh floor stays a stack. A `CharacterBody` walks on it, climbs low ledges and stairs, and
slides along its walls (a face steeper than `max_slope` is a wall, not ground). A `NavAgent` plans routes over it (docs/NAVIGATION.md). `raycast`, `pick`
and a `Sensor` with `los` hit the very triangles that are drawn (a test compares the physics reader with the renderer's importer bit for bit).

**How it is done, in plain words.**

* **The triangles are read once and put into a tree.** The model is read from the file (the node tree of the default scene, every node's matrix, strips and
  fans unrolled, a mirroring node flipped back: the same rules as the renderer, so physics and picture agree), the triangles are sorted into a *bounding
  volume hierarchy* (a tree of boxes, four triangles in a leaf), and a question such as "what touches this ball?" walks only the boxes it overlaps. The
  tree is built by always cutting a box at the median along its longest side, with ties broken by the triangle's number, so it is the same tree on every
  machine and every run. The model is read again when the file changes (it is looked at every 60 ticks); a file that is broken keeps the last good copy.
* **The `Transform` places it.** Position, rotation and scale of the entity put the model in the world; the placed copy is kept between ticks, so a level
  that stands still is placed once. A level that is moved from a system is placed again every tick it moves (4.6 ms for 100,000 triangles, a few
  microseconds for a lift of a hundred), so keep moving meshes small. A mesh has no velocity of its own: like every static body it never carries
  what stands on it.
* **Every triangle is solid on both sides.** A triangle has no inside, so a thin floor works from above and below. A ball that crossed a surface during the
  last tick is pushed back to the side it came from, and a body that moved farther than its own size in a tick is first checked along its path
  (where it was a tick ago to where it is), so things dropped from 80 m, and a 150 m/s shot, do not pass through a floor or wall of triangles. The
  path test follows the body's centre only: a long box that flies sideways through a thin wall may still catch an edge of it.
* **A flat floor is many triangles, and the seams do not catch.** A ball that rolls over the join of two triangles is, for the one it is not above,
  nearest to an *edge*, and a push from that edge would be slightly tilted and slow the ball down. A contact on an edge or a corner is therefore dropped
  when a triangle that shares that very point is nearer (that triangle's own push is the true one). Where a body really touches an edge (the ridge of a
  roof, the lip of a step) nothing is nearer and the contact stays. Boxes get the same treatment and use only the faces' own directions (no
  edge-against-edge axes), which is also what keeps them from catching on seams.
* **Contact points.** A ball touches at one point, a capsule is a ball at each end plus the nearest point of its axis, a box comes with the clipped face that
  lies against the triangle (up to four points: it lies flat), so bodies roll, tip and settle as they do on a box ramp. A test lets a ball roll down a
  mesh ramp and down a box turned to the same angle: after 1.5 s they are within 10 cm of each other and 4 percent in speed, and the speed is that of a
  solid ball on that slope (5/7 of the pull a sliding block would feel), rolling without slipping.
* **Determinism.** The tree, the contact order (by uid, then by triangle number) and every sum are fixed, so a scene gives the same bits on any machine,
  any thread count, and from a snapshot taken in the middle. The state hash covers the model's name and a hash of its file, so a changed model is a
  changed world. A scene without a `MeshCollider` never runs any of this code and keeps its old hashes bit for bit.

**`check`** warns about a `MeshCollider` with no `mesh` or no `Transform`, a model that is missing or cannot be read (with the reason), a model without a
single triangle that has an area, triangles with no area (they are left out), a model with more than 200,000 triangles (it works but reads slowly),
one with more than **1,000,000** triangles (refused: make a simpler copy of the level for collision; the model file may be 128 MB and 4 million vertices
at most), and a `MeshCollider` on an entity that also has a `RigidBody` (the mesh stays where it is; put the body on its own entity).

**Not covered.** The sound tracer does not see meshes (give walls a plain `Collider` for sound); a skinned model keeps the positions it is stored
with, not the skeleton's pose (a character is not a level); every triangle is solid from both sides, so there is no floor that can be jumped up
through; friction and restitution are one number for the whole mesh (no surface type per triangle); and a mesh never moves things by itself.

### What it costs

Measured on a desktop CPU at 3.6 GHz with the 100,000-triangle level of the tests (a 224 by 224 grid of hills, 301,056 vertices), in the test build
(optimisation level 2; a release build has not been timed, and the numbers on another machine will differ). `cargo test -p lunaris-sim --lib --
--nocapture hundred_thousand` prints them:

| | cost |
|---|---|
| read the `.glb` and build the tree (once per file) | 41 ms (the tree alone 37 ms) |
| place it with a `Transform` (once, and again when it moves) | 4.6 ms |
| the first tick with a mesh collider (read, build, place) | 48 ms |
| a ball against the level, one question | 2.9 microseconds |
| a capsule | 7.7 microseconds |
| a box | 38 microseconds |
| a ray | 0.7 microseconds |
| **a whole tick with three bodies rolling on it** | **0.045 ms** (physics 0.033 ms) |

The cost grows with the number of bodies that touch the level and not with the size of the level: each body costs one such question per tick on top
of the solver's usual work (the numbers are averages over spots on and just above the surface of the level). A scene with no `MeshCollider` pays nothing: a test checks
that it never reads, builds or places anything, and the golden hashes of the examples that existed before were not changed by this feature.

## Convex hulls: `ConvexCollider`

A rock, a crate, a wedge, a gem, a barrel: a shape that is not a ball, box or capsule but has no dents. `ConvexCollider {mesh, trigger}` gives an entity the
**convex hull** of the vertices of a model (the smallest solid without dents that contains them all), stretched by the entity's `Transform.scale`. Use it
**instead of** a `Collider` (an entity with both has two shapes that do not collide with each other: `check` warns).

```
{"Transform": {"pos": [0, 3, 0]}, "MeshRenderer": {"mesh": "models/rock.glb"}, "ConvexCollider": {"mesh": "models/rock.glb"},
 "RigidBody": {"mass": 4, "friction": 0.6, "restitution": 0.15}, "Velocity": {}, "AngularVelocity": {"damping": 0.3}}
```

**What it does.**

* **A static hull** (no `RigidBody`) is a solid in the world: bodies land on it and bump into it, rays and sensors see it (the hull's own faces: a ray
  hits the hull and not the model's dents), and `trigger: true` reports overlaps without pushing, like a `Collider`'s.
* **A dynamic hull** (`RigidBody` with `mass` above 0, a `Velocity`, an `AngularVelocity`) tumbles, rests, stacks and rolls on boxes, balls, capsules, other
  hulls, terrain and mesh levels. Give it an `AngularVelocity` like any body that turns; without one it slides and never turns.
* **The mass is spread evenly over the volume.** The volume, the centre of mass and the inertia come from cutting the hull into tetrahedra (exact for a
  solid with flat faces). The centre of mass is the middle of the solid, which is not necessarily the model's origin: the entity's `Transform` is the
  model's origin and the engine turns the body about its centre of mass (it keeps the hull in its principal axes and converts back when it writes the
  `Transform`). A lopsided rock therefore rights itself and wobbles like a real one.
* **At most 64 corners.** The hull is built by *quickhull* in `f64` from the points sorted by x, y, z (exact duplicates dropped), always adding the
  point that sticks out most; it stops at 64 corners, which is the simplification: the points left out are the ones nearest the hull, and `check` says
  how far the farthest of them is. Triangles that lie in one plane are joined into one polygon (a cube has six faces, not twelve). A model that is flat
  (all points in a plane) or a single point has no hull and collides with nothing; `check` says so. The hull is built once for each model and scale.
* **Contacts.** Hull against hull and hull against box use the *separating axis test*: the faces of each against the other, plus the pairs of edges
  that can make a face of the difference (a test of the directions on a sphere keeps it to the few pairs that matter). The shallowest way out
  decides: a **face contact** (up to four points, clipped to the outline of the face: this is what keeps a crate flat on a floor) or, when two edges
  cross and that is clearly shallowest (by 2 mm), a one-point **edge contact**. A ball or a capsule against a hull takes the nearest point of the hull
  (a centre inside leaves through the nearest face). A hull against the triangles of a mesh level goes triangle by triangle like a box does.
  The boxes of the engine are now handled by this same code when they meet a mesh.
* **Characters, navigation and the sound tracer** see a static hull as the axis-aligned box around it (for characters and agents: a rock is a block to
  walk round or jump onto); the tracer does not see hulls at all.

**What it does not do (honestly).**

* **A hull has no dents.** A bowl, a ring or an L-shaped crate is *one* hull that fills the dent. Build such a thing from several entities (parts of a
  level: a `MeshCollider`). There is no compound shape.
* **A body pushed deeper than its own thickness in one tick may be pushed out the wrong way.** Fast bodies get the path test described above, but only
  against mesh levels (a hull dropped from 80 m onto a mesh floor lands on it), and it follows the body's centre only. A fast hull flying at a box, another
  hull or a thin wall of boxes can still pass through, as any body can (see Limits): keep thin things slower or thicker, or shorten the step.
* **A face that is nearly parallel to another and a little bent** can give a contact patch that is a bit small (the body is held on fewer than four
  points for a moment).
* **Nothing is remembered between ticks**, as for every body here: a tall pile of hulls creeps like the piles of boxes do (see Limits).
* **Hull against hull is the costly one** (table below).

### What it costs

| | cost |
|---|---|
| a hull of 3,000 points simplified to 64 corners (once per model and scale) | 2.1 ms |
| two hulls of 12 corners against each other | 4.0 microseconds a pair |
| two hulls of 64 corners against each other | 105 microseconds a pair |
| a hull of 12 corners against the 100,000-triangle level | 19 microseconds |

A pair of heavy 64-corner hulls is by far the most expensive question there is: 100 of them in contact would take about 10 ms a tick. Ten to twenty
corners is plenty for a rock or a crate (use a simpler model for collision, as with a `MeshCollider`).

### The example

`examples/mesh_level.lunaris.json` (`tools/build_mesh_level.py` writes it and the models it uses, `examples/tests/mesh_level.test.json` plays it): a 24 by
16 m courtyard of triangles with a pit, a platform and a ramp, a staircase and a low wall. A ball rolls down the ramp, another falls into the pit, a stack
of crates stands on the platform, a box slides off the ledge and tumbles down the stairs, a capsule falls over, three rocks and a wedge (hulls) settle on
the floor and the ramp, a guard walks by itself up the ramp, and the hero walks where WASD says over all of it. The playtest makes 28 checks of that.

## Collision layers: who collides with whom

Unity's layer collision matrix and Unreal's collision channels, as one small component. Put a `CollisionLayer` next to the shape of an entity:

```
{"Transform": {"pos": [0, 1, 0]}, "Collider": {"shape": 0}, "RigidBody": {}, "Velocity": {},
 "CollisionLayer": {"layer": 2, "mask": 3}}
```

| field | what it is |
|---|---|
| `layer` (0) | the group this entity is in, 0 to 31 (a bigger number counts as 31; `check` warns) |
| `mask` (4294967295) | the groups it collides with: bit `n` set means layer `n`, so layers 0 and 1 are `1 + 2 = 3`. The default has every bit |

**The rule: both must agree.** Two entities touch only if `a.mask` has the bit of `b.layer` **and** `b.mask` has the bit of `a.layer`. One side leaving the other out is enough for them to pass through each other, whichever of the two is dynamic, a trigger or the ground. An entity **without** the component is layer 0 with a full mask, so a scene that never uses layers behaves bit for bit as before (the state hash of every example is unchanged: a test compares ten of them with `tools/golden_hashes.txt`, and the component is hashed only in scenes that have it).

**What it decides** (one rule in all of these, nothing else to switch on):

* **Rigid-body contacts of every shape**: spheres, boxes (turned or not), capsules, `ConvexCollider` hulls, `MeshCollider` levels and `Terrain`, against each other. A pair that does not agree is never solved, so a ball rolls through a wall on another layer, a crate lands on the floor of its own layer and falls through the other floor. The continuous collision (a fast spinning ball against thin walls, a fast body against a mesh level) skips what the body does not collide with, so a bullet is not stopped by a wall it is meant to cross.
* **Triggers**: a trigger overlaps only what it collides with, so `contacts` and `on_contact` systems see only those pairs (a pickup on its own layer is touched by the player layer and by nothing else).
* **`CharacterBody`**: walks through what it does not collide with, falls through a floor it does not collide with, and the ground it stands on must agree like anything else.
* **`NavAgent` and `path`**: an agent plans over, round and through only the colliders it collides with (so ghosts walk through doors, and a `NavObstacle` blocks only the agents that collide with it). The `path` tool plans as the entity named by `from` when that is a uid (its `CollisionLayer`), and counts every layer for a bare point.
* **`raycast`**: takes `mask` (a number: bit `n` is layer `n`) or `layers` (a list of layer numbers, 0 to 31) and hits only entities on those layers; without them it hits everything, as before. The mask applies to what is drawn (`include: visuals`) as well as to colliders. A ray is a question, not a collider, so only the mask you give counts: the masks of the things it hits are not asked.
* **`Sensor` with `los`**: the sensor's line of sight is blocked only by blockers whose layer is in the **mask of the sensor's own entity** (a `CollisionLayer.mask`; a full mask when the entity has none, which is the old behaviour). One way, like a ray: the blockers' masks are not asked. That is the default because "what does this entity collide with" is also "what can it not see through": a guard whose mask leaves out the pickups layer is not blinded by a coin. Targets (the entities `filter` names) are never filtered.
* **`Vehicle` wheels** stand on what the chassis collides with (the full rule, both masks), so a car on another layer than the ground falls through it (see Raycast vehicles).

Not covered: the sound tracer (`acoustics`, `record_audio`) does not look at layers (walls muffle sound whatever their layer); `pick` and `render` show everything; `Joint`s are explicit and tie their bodies whatever the layers say; the particle `collide` modules have their own planes and spheres; `tilemap_colliders` makes colliders on layer 0 (pass `components: {"CollisionLayer": {"layer": 1}}` to give them a layer).

**Masks without arithmetic.** A mask is a sum of powers of two. For a small game name the layers once and keep the numbers in the scene's notes:

| layer | what | its mask (collides with) | the number |
|---|---|---|---|
| 0 | default (no component) | everything | 4294967295 |
| 1 | world: floors, walls | players, enemies, crates | 2^2 + 2^3 + 2^5 = 44 (plus 2^0 if default things stand on it: 45) |
| 2 | player | world, enemies, pickups, crates | 2^1 + 2^3 + 2^4 + 2^5 = 58 |
| 3 | enemies | world, player, crates | 2^1 + 2^2 + 2^5 = 38 |
| 4 | pickups (triggers) | player only | 2^2 = 4 |
| 5 | crates | world, player, enemies, crates | 2^1 + 2^2 + 2^3 + 2^5 = 46 |

"Everything except layer 3" is `4294967295 - 8 = 4294967287`; "only layer 4" is `16`; a layer `n` is `1 << n` in a graph script or in Python. Remember the other half of the rule: the pickups of that table (`mask` 4) are touched by the player only if the **player's** mask has the pickup layer too (bit 4, which it has in the table: 58).

**`check`** warns about a `layer` above 31, a `mask` of 0 (collides with nothing: bodies pass through it, its triggers never fire, its rays see through everything), a `CollisionLayer` on an entity that has nothing it applies to, two kinds of entities where only one side wants the other (listed by layer and mask, at most eight: this is the usual mistake, a coin that can never be picked up because the player's mask left the coin layer out), and a dynamic body that collides with no static solid of the world at all (it falls through everything).

### What it costs

Nothing for a scene that does not use it: the physics step asks the world for the list of layered entities (an empty list is one query that finds no archetype) and each pair of bodies then compares two bit masks. `cargo test -p lunaris-sim --lib what_layers_cost -- --nocapture` prints the cost of 2,000 balls piling up on a floor, once without layers and once with a `CollisionLayer` on every ball: 0.81 ms a tick without layers and 0.85 ms with them (one thread, a desktop CPU at 3.6 GHz in the test build; the difference is the per-body lookup of the layer list and is within the noise of a run).

## Raycast vehicles: `Vehicle`

A car, a truck, a buggy: Unity's `WheelCollider` and the arcade end of Unreal's Chaos vehicles. The vehicle is **one rigid body** (the chassis) and a handful of rays. Put a `Vehicle` on an entity that has a `Collider` (a box about the size of the body: it is the chassis's shape and its inertia), a `RigidBody` (the mass), a `Velocity` and an `AngularVelocity`:

```
{"Transform": {"pos": [0, 0.8, 0]}, "Velocity": {}, "AngularVelocity": {},
 "Collider": {"shape": 1, "size": [1.8, 0.7, 4.2]}, "RigidBody": {"mass": 1200},
 "Vehicle": {"wheels": "defaults: radius=0.35 rest=0.3 travel=0.2 spring=35000 damper=3200 grip=1.2\nfl: -0.8 -0.15 -1.3 steer=1\nfr: 0.8 -0.15 -1.3 steer=1\nrl: -0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1\nrr: 0.8 -0.15 1.3 drive=1 brake=0.7 handbrake=1"}}
```

The chassis's own **-z is forward** (like a camera and a sensor), +x is right and +y up. Each wheel is a ray from its **mount point** (the top of the strut: `x y z` in the chassis's own coordinates, relative to its `Transform`) down the chassis's -y. Where the ray meets the ground the wheel stands, and four things push the chassis there:

* **The suspension.** A spring and a damper along the ground's normal. `rest` is the strut's length when the wheel hangs in the air, which is also its zero-force length; the wheel is pressed in by up to `travel` before the **stop** holds it (the stop is firm: it cancels the speed of the approach at once and pushes the overlap out over a few ticks, so a car that lands hard presses its wheels in to the stop and is pushed back out; past that the chassis's own box meets the ground like any body). The spring pushes `spring * compression`; the damper `damper` times the speed of the compression, limited to 80 percent of what would stop that speed in one tick, so it can never reverse the motion it damps. A car at rest sits exactly where `spring * compression` equals its wheel's share of the weight (in the test the four springs are pressed in 0.083999 m against the 0.084000 m the formula gives). `spring` and `damper` can be `auto`: the car's weight per wheel over a quarter of the travel, and a damping ratio of 0.6. An **anti-roll bar** (`anti_roll`, N per metre) joins the two wheels of an axle (the ones at the same distance along the car, on opposite sides).
* **Tyre friction**, in the plane of the ground and at the contact points. Along the wheel: the engine, the brakes and a little rolling resistance. Across it: the grip that makes the car follow its wheels. The impulses are found together (eight rounds of sequential impulses over the wheels, nothing kept from the tick before) and each wheel's pair is held inside the **friction circle**: its length cannot exceed `grip` times the load pressing the wheel down. Ask for more and the wheel slides. That one rule makes the rest: a wheel that drives hard has less left to hold the car sideways (power oversteer), locked brakes do not steer, a handbraked rear wheel lets the tail swing out, a car in the air has no grip at all, and a car on ice (`grip` 0.2) goes where it was going.
* **The engine.** `throttle` -1 to 1: positive pushes the driven wheels (`drive` shares) with `engine_force`, less once the power runs out (`power / speed`, 0 for no limit) and fading to nothing over the last tenth of `top_speed`; negative reverses at 60 percent of the force up to `reverse_speed`, or, while the car still rolls forward faster than 1 m/s, brakes it (and the same the other way round). `brake` 0 to 1 and `handbrake` 0 to 1 spend `brake_force` and `handbrake_force` over the wheels' `brake` and `handbrake` shares; a handbraked wheel also keeps only `handbrake_grip` of its grip (and its model stops turning). With no foot on anything a light brake holds the car below 2 m/s, so it stays put on a slope and a coasting car comes to rest.
* **The air.** `air_drag` (N per (m/s) squared) and `downforce` (the same, pushing down on the chassis).

**Steering.** `steer` -1 to 1 (right is positive) asks for that share of `steer_max` degrees, less the faster the car goes (`steer_fade` is the share left at `steer_fade_speed` and above), and the wheels turn there at `steer_rate` degrees a second, so a key that is down or up gives a smooth wheel. A wheel takes `steer` times that angle (1 for the front, 0 for the rest, a small negative number for rear-wheel steering).

**Where the force is applied** matters for flipping. Contact forces push at the contact point, but the friction pushes at a point whose height above the ground is scaled by `roll_influence` (1: the contact point; 0.5, the default: half way up to the chassis's centre; 0: at its centre height), which is the cheap and old way to make a raycast car hard to roll over. Lower it for a taller car, raise it for a simulation.

**The ground** is what the wheel rays hit: static `Collider`s (boxes, turned or not, spheres, capsules), static `ConvexCollider`s, `MeshCollider` levels and `Terrain`, with the same shapes and the same rules as every other query, **and the collision layers** (the wheels stand on what the chassis collides with: a car on another layer than the ground falls through it). A ray that is nearly flat against a wall (a normal more than 75 degrees from the chassis's up) is not ground. Bodies that move (a `RigidBody` with a mass, a `CharacterBody`) are not ground for a wheel: it does not push on them.

**The wheel models.** Give a wheel `entity=UID` and the engine sets that entity's pose every tick: where the wheel's centre is (on the strut, `rest - compression` below the mount point), turned by the steering and by the angle the wheel has rolled. A child of the vehicle's entity (`Parent` and `LocalTransform`) gets its `LocalTransform` set; an entity with no `Parent` gets its `Transform` set in the world. The vehicle's own `Transform.scale` should be 1 then (a child is scaled by its parent); put the body's mesh on a child, not on the chassis. A wheel is a **pivot**: give it children for the tyre (a cylinder turned onto its side) and a spoke, so that the turning shows (`examples/driving.lunaris.json` does).

**Outputs**, written every tick (read them from a graph system or `get`): `speed` (forward, m/s), `steering` (degrees), `on_ground` (how many wheels touch), `contact` (a bit per wheel), and per wheel `compression` (metres), `spin` (the angle it has rolled, radians, wrapped) and `slip` (the sideways speed of the tyre over the ground, for skid marks and sound). At most **8 wheels**; the output arrays hold eight.

**Controls from a game.** `throttle`, `steer`, `brake` and `handbrake` are plain fields: a graph system sets them from the actions of an `InputMap` (`throttle = w, arrowup, gamepad.rt`, `steer = d, arrowright, a:-1, arrowleft:-1, gamepad.lx`):

```
system drive each Vehicle order 10 {
    Vehicle.throttle = $action.throttle - $action.reverse;
    Vehicle.steer = $action.steer;
    Vehicle.handbrake = $action.handbrake;
}
```

**Seeing it.** `render {debug: ["wheels"]}` draws the ray of every wheel (dark), the strut to the wheel's centre (yellow) and the wheel itself with a stroke across it: green when it touches the ground, red in the air.

**`check`** says what is wrong: a `Vehicle` with no `RigidBody` (or no mass), `Collider`, `Velocity` or `AngularVelocity`; a wheel table that does not parse (with its line and what is wrong); no wheels; fewer than three; a wheel whose radius, rest, travel, spring, damper or grip is not a car's; springs **too stiff** for the mass at 60 ticks a second (the explicit spring shakes apart when `spring * dt^2 / (mass per wheel)` is above 0.5), dampers too strong, springs **too soft** (the weight presses a wheel in more than 90 percent of its travel, so the car sits on the stops); no wheel with `drive`, `steer` or `brake`; an `engine_force`, `brake_force` or `top_speed` that is 0; and wheel models that do not exist or cannot be placed.

**Tuning.** The spring for a wanted sag: `spring = mass / wheels * 9.8 / sag`; the damper for a damping ratio `z`: `damper = 2 * z * sqrt(spring * mass / wheels)` (0.3 is bouncy, 0.6 is a car, 1 is a truck on a bad road). `engine_force` of 0.75 times the weight in newtons is brisk at a standstill and `power` sets the top end (90 kW and 1200 kg reach 100 km/h in 6.3 s); `brake_force` about 1.3 times the weight; `grip` 1.0 to 1.5 on tarmac, 0.6 on gravel, 0.2 on ice; `steer_max` 30 to 40. A heavy truck wants a bigger `rest` and `travel`; a go-kart almost none.

**The example.** `examples/driving.lunaris.json` (`tools/build_driving.py` writes it and its hills, `examples/tests/driving.test.json` plays it): a car on a proving ground. A flat meadow in a ring of hills (a `Terrain` of 256 m), a road with two gates (translucent triggers on their own collision layer, so that the cones and crates, which are on another, never set them off), rumble strips, a slalom of light cones, a wall of crates and a jump (a box turned 14 degrees). W and the arrow keys drive, S brakes and then reverses, A and D steer, Space is the handbrake, R puts the car back; a pad's triggers and left stick do the same; the camera swings round behind the car. The playtest drives it straight through a gate, brakes, and turns, and replays the run.

### What it measured

Numbers from the tests (`cargo test -p lunaris-sim --lib vehicle -- --nocapture` prints them), a car of 1,200 kg with four springs of 35,000 N/m, 90 kW and a rear-wheel drive, on a desktop CPU at 3.6 GHz in the test build:

| | measured |
|---|---|
| ride height of the chassis's centre on flat ground, dropped from 0.3 m above | 0.7160 m, which is what the springs say (0.35 radius + 0.3 rest - 0.0840 sag + 0.15 mount); the same on a box, a mesh level, a terrain and a static hull; 0.7119 on a capsule of 200 m radius (its curve) |
| a 12 degree slope of a turned box: height over the surface | 0.733 m for 0.732 predicted, the nose up 12.7 degrees |
| 0 to 100 km/h | 6.3 s; 46 m/s after 20 s (power and drag: the top speed is 55) |
| braking from 20 m/s | 17.3 m, 1.18 g (the tyres' 1.2); the front springs press in 0.13 m more than the rear |
| circle at full lock, 6.2 m/s | 5.6 m radius for 4.9 m from the wheelbase and the angle (it understeers a little: no Ackermann) |
| flipping | 1,500 ticks of full-lock steering, braking and handbrake below 14 m/s: the chassis never rolled more than 3 degrees |
| the jump (a 12 degree ramp of 14 m at full throttle) | airborne for about half a second, lands on four wheels and drives on |
| any number of threads (1, 2, 5, 8), a copy taken in the middle, a rewound checkpoint, a recording | the same state hash |
| cost of a vehicle's tick | the wheels (four rays, the suspension, eight rounds of friction) take **1.5 to 2 microseconds** a car; with the chassis's own rigid-body physics a car is **3 microseconds** of a tick (50 cars: 0.14 ms a tick). The wheel rays scan the static colliders each tick, so the first car of a world with 5,000 static boxes adds 0.17 ms (about 33 ns a box) |

### Limits

* **A wheel is a ray.** It sees the ground below it and nothing in front: a kerb is climbed as if the wheel jumped onto it (the spring takes the shock), a wall is met by the chassis's box and not by the wheel, and a wheel whose mount point is inside a solid sees nothing. There is no sweep and no wheel shape.
* **The tyre is a friction circle, not a tyre curve.** The sideways force is whatever cancels the sideways speed, up to `grip` times the load: grip comes on at once and saturates sharply (a real tyre eases into its limit over a slip angle), and the load sensitivity of a real tyre (more load, less grip per newton) is not modelled. The two front wheels turn by the same angle (no Ackermann), so a tight circle is a little wider than the geometry says (16 percent in the test) and the front tyres scrub.
* **Not modelled:** a gearbox, a differential (the drive is shared by the `drive` shares, not by who has grip), the inertia of the wheels (they do not spin up: a driven wheel on ice does not spin faster than the car), clutch, engine speed, the ground's own friction (a `Terrain` or `RigidBody` friction is not read: the tyre's `grip` is the whole coefficient), and suspension geometry (the strut is straight along the chassis's -y and does not change the camber or the track).
* **The chassis's inertia is its box.** A box 0.7 high rolls and pitches more easily than a car whose mass sits higher: make the box as tall as the body with its cabin, or lower `roll_influence`. Only a `Collider` (a box, a ball or a capsule) can be a chassis: a `ConvexCollider` cannot.
* **Wheels see the static world.** They do not stand on other cars, crates or characters (the chassis boxes collide as any bodies do). `Joint`s can tie a trailer on, but a trailer is a second vehicle that pulls on the first.
* The forces are applied right after the rigid bodies of the tick, so a control set by a graph system takes effect one tick later (the usual one-tick latency of everything a graph system writes). A vehicle's `Transform.scale` is not used for the wheel positions.
* The friction solver is order dependent when the wheels fight (the two front wheels at different angles): the answer is deterministic and the same on every machine, but a wheel earlier in the table gets its way a little more.

## Limits (what a full engine would add)

* Balls, boxes, capsules, convex hulls and triangle meshes (a `MeshCollider` is static; there is no compound shape). Rays hit oriented boxes and capsules as
  they are, but characters and navigation (`CharacterBody`, `NavAgent`) see every box as axis-aligned, a capsule as the box `size` and a hull as the box around
  it; they see the triangles of a mesh as they are. The sound tracer sees boxes and spheres only.
* **Nothing is remembered from tick to tick**, so that a snapshot or a replay from the middle of a run gives exactly the same future. A stack of
  turning boxes therefore does not settle as firmly as in an engine that remembers its contact forces: a tower of six boxes creeps sideways by a
  few centimetres in ten seconds. Four boxes or fewer stay put; give tall piles a bit more friction.
* No sleeping (resting bodies are still solved every tick), no joint motors or springs on the hinge, no cone limit on a ball-and-socket.
* Continuous collision only for spinning spheres against static things (above) and the path test of fast bodies against mesh levels (their centre's
  path only). Anything else that moves faster than its size in a tick can pass through a thin wall: make the wall thicker or the step smaller.
