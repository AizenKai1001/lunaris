# Navigation: say where, the engine walks there

An entity with a `NavAgent` component walks to a point on its own: it finds a path around the solid colliders (up stairs, onto platforms, down short drops, through the gaps of a maze) and follows it, every tick, deterministically. A person, a graph system or an AI mind only says **where**:

```json
{ "tool": "set", "args": { "uid": 17, "component": "NavAgent", "value": { "target": [-9, 0, 9], "active": true } } }
```

`active` is a request: the engine plans a path, turns it off again, and `status` tells what happened.

| `NavAgent.status` | meaning |
|---|---|
| 0 | idle (never asked) |
| 1 | walking; `remaining` is the path length left |
| 2 | arrived (within `arrive` metres of the target) |
| 3 | no path, or blocked for good (the agent replans three times when it makes no progress for a second) |

The fields: `target`, `active`, `speed` (4 m/s), `radius` (0.4), `height` (1.8), `step_height` (0.4, the highest step it climbs), `max_drop` (1.2, the longest fall it accepts), `arrive` (0.3), `feet` (how far the entity's `Transform` is above the ground; 0 for a `CharacterBody`), `status`, `remaining`. The agent is moved by setting a `CharacterBody`'s `walk`, or an entity's `Velocity.lin` x and z, or (with neither) its `Transform` directly. It runs after the graph systems (so a system may hand out targets) and before the character controller.

## The `path` tool

`path {from, to, radius?, height?, step_height?, max_drop?}` shows the route without walking it: `from` and `to` are `[x, y, z]` or the uid of an entity (an entity with a `NavAgent` brings its own size). It returns the waypoints (feet positions), `length`, `exact` (false when the target itself is not walkable and the route ends at the nearest ground that is) and `searched`. Failures say which end is the problem: `nav_no_start`, `nav_no_goal`, `nav_no_path` (both ends are fine but nothing connects them: walls, a step that is too high, a drop that is too long), `nav_too_big`. `examples/maze.lunaris.json` and its playtest use both.

## How it works

1. The ground plane between the two points (plus a margin) is cut into square cells, 0.5 m, coarser when the area is very large.
2. Every **solid, non-moving `Collider`** (not a trigger, not a `CharacterBody` or `NavAgent`, not a dynamic rigid body) adds to each cell it covers: if it is a **box**, its top is a **surface** to stand on; every solid is an **obstacle** for the heights it occupies. Walls are grown sideways by the agent's radius, except where they are lower than a step (so stairs do not push the agent away from them). A surface is walkable when no obstacle intersects the agent's height above it. Several levels in one cell (a bridge over a road) work. The triangles of a `MeshCollider` add surfaces and obstacles too (below).
3. Cells are connected in eight directions when the height difference is a step the agent climbs or a drop it accepts; diagonals never cut corners; an agent can step off a platform's edge past the cell that is too close to the platform's side.
4. A\* with the octile distance and integer costs; ties are broken by node number, so the answer depends only on the world. A line-of-sight pass over the grid removes the waypoints that only the cells needed.
5. A start that is not standing on any box (a top-down game whose floor is only a picture) means one flat open floor at the start's height: obstacles still block it, everything else is a straight line.

Everything is `f32` with only `+ - * / sqrt`: identical results on every machine, any thread count; the paths agents follow are part of the state hash, snapshots and recordings.

## Links: jumps, ladders, teleporters

A `NavLink` entity is a way across what the map cannot walk. `from` and `to` are offsets from the entity's position (move the entity to move the link), `two_way` lets agents use it both
ways, `speed` and `hop` say how an agent is carried over (metres per second along an arc `hop` metres high), `enabled` false closes it (a drawbridge). Planning treats it as one more edge
that costs its length, so a gap with a link becomes part of the route; the `path` tool lists the steps that are jumps in `jumps`. Both ends must be within about one and a half metres of ground an agent can stand on.

## Keeping out of each other's way: `NavAvoid`

Put a `NavAvoid` beside a `NavAgent` and it steers away from the other agents and characters it comes near (`range` times the sum of the radii, `strength` 1 is a firm push), and
turns to the right when it meets one head on, so two agents walking toward each other pass instead of stopping. Without it agents walk through one another, as characters do.

## Terrain

A solid `Terrain` (docs/TERRAIN.md) adds a surface to every cell it covers: the height of the ground at the cell's centre, unless the ground there leans
more than 50 degrees. Ground and boxes mix: stairs and platforms stand on the hills, and trees with colliders are obstacles on them.

## Meshes and hulls

A `MeshCollider` (a level made of triangles from a model file, docs/PHYSICS.md) is part of the map, and its slopes are walkable:

- Each cell looks at the triangles above it. A triangle that faces **up** within the agent's steepest slope (50 degrees, the character controller's default
  `max_slope`) is a **surface** to stand on, at the height of the triangle over the middle of the cell; a ramp of triangles therefore climbs smoothly and the
  agent walks up it (the step limit `step_height` applies between neighbouring cells, so a ramp that rises less than that per cell is walkable, a cliff
  is not). A pit is a drop, and the agent keeps out of it when it is deeper than `max_drop`.
- **Every** triangle is an obstacle for the heights it occupies: a wall, an overhang or ceiling above the agent's head, and a slope steeper than the limit
  (a floor that can be walked on is not an obstacle beside the cells it covers, like the ground of a terrain). Obstacles are grown sideways by the agent's
  radius, like a box. A face that points **down** is a ceiling and never a floor: an agent does not walk on the underside of a bridge.
- The agent is moved by the character controller or its velocity as ever, and the controller walks the same triangles, so the path and the walk agree.

A static `ConvexCollider` (a rock, docs/PHYSICS.md) is, for the planner, the axis-aligned box around its hull (a block to go round, or to step onto if it
is low): a dynamic one is not part of the map, like any dynamic body, unless it has a `NavObstacle`. `examples/mesh_level.lunaris.json` has a guard that
walks up a mesh ramp to a platform by itself, and the tests in `crates/lunaris-sim/src/mesh_physics_tests.rs` plan paths over a mesh floor, round a mesh
wall, up a gentle ramp and not up a steep one. The `path` tool takes the meshes into account too.

## Limits

- Boxes and spheres, no rotation (a rotated box, a capsule and a hull count as the axis-aligned box around them, as for characters); a ramp of boxes is stairs, but a ramp of mesh triangles is a slope (above).
- Dynamic bodies are not part of the map unless they carry a `NavObstacle` (a pushable crate, a sliding door, a parked car: agents then plan round where it is when they plan, and plan again when they get stuck against it); agents avoid each other only with `NavAvoid`, and then only by steering (no planning around crowds). Moving obstacles are noticed only when the agent stalls and replans.
- Collision layers (`CollisionLayer`, `docs/PHYSICS.md`) apply: an agent plans over, round and through only the colliders (boxes, spheres, hulls, terrain, mesh levels and `NavObstacle`s) that its own layer and mask collide with, so a ghost walks through doors and a guard is not stopped by a wall of another world. The `path` tool does the same for the entity it starts from (`from` as a uid) and counts every layer for a bare point.
- The grid is built for each planning request from the colliders, which is a few milliseconds for a few hundred colliders; ask for a path when a target changes, not every tick.

## With minds

`lunaris mind` has two verbs that use it (`docs/MIND.md`): `go_to` walks to a named thing and returns `arrived`, `blocked` (with the reason) or `still walking`; `wait` lets time pass. A 4B model that collected 2 of 12 orbs by pressing keys collected all 12 in 12 decisions with `go_to`.
