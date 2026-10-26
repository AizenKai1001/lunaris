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

1. The ground plane between the two points (plus a margin) is cut into square cells, 0.5 m, coarser when the area is very large (the window starts at the corner of the box between the two points; with `NavLimits.keep_cells` it starts on a lattice instead, see "Keeping the cells" below).
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

## How fast a search is

A search works out the data of a cell (the heights of the surfaces an agent can stand on) only when it first looks at the cell, from the solids, terrain and triangles over that cell, and keeps it: a search of a hundred nodes looks at a few hundred cells. A 164 m search is 0.15 ms over open ground and 4 ms across 36 shells of 4 000 triangles (it was 6.8 and 41 ms when the grid was cut whole over the box between the two points); the paths are the same bit for bit. `lunaris bench` has both numbers (`nav open`, `nav city`) and `docs/PERFORMANCE.md` has the story.

### Keeping the cells between searches (opt-in)

Most of what is left of a search over a city is working out the data of the cells (asking each mesh's tree for the triangles near the cell and clipping them: about 3 of the 4 ms), and a server asks again and again for the same streets. `NavLimits.keep_cells` (default **off**; or `NavParams.keep` for a call of `find_path_with`) keeps that data between searches: the window of a search then starts on a **lattice of the cell size anchored at the world's origin**, so the cell (i, j) of one search is the cell (i, j) of the next, and the data of a cell is kept, for each size of agent (radius, height, step), while a hash of the world's solids, grounds and mesh levels is the same (a different hash drops the cells; at most 600 000 per size, per thread, four sizes). A door that swings does not change the hash (its solid is a box that ignores turning); a wall built does, and then the cells are made again.

A repeated search over the same streets costs 0.09 ms across the city of the bench instead of 4.2 (`nav city kept`). The price is that **the paths are not the ones of the same world without it**: the cells' centres move to the lattice, so a waypoint can differ by up to half a cell and a tie between two equally near cells can fall the other way. Because nav paths are part of the state hash, snapshots and recordings, turning it on is a decision of the game, and a recording made without it does not replay with it. What is tested (`nav_tests.rs`): kept cells give exactly the paths of cells worked out fresh on the same lattice (150 searches by agents of three sizes, with keeping off, on and on again), a wall built after the cells were kept is walked round, and a search without `keep` leaves no cells behind and gives the paths it always gave (the fingerprint test, `0x14d88eab…`).

## Crowds: `NavLimits`

Every search builds a grid of the ground round its two points, and in a world of many meshes (a streamed city) one search can take tens of milliseconds and a bad one more than a second. A hundred agents that each ask again whenever their target moves a metre can ask dozens of times a second, and without a limit every request is answered in the tick it was made: a server with 150 roaming, shooting bots measured 40 to 60 ms ticks and single ticks of 1.0 to 1.4 s, with navigation as the cause. `NavLimits` (put it on one entity; the lowest uid counts) rations the searches:

| Field | Meaning |
|---|---|
| `searches_per_tick` (default 2, 0 = no limit) | At most this many searches in one tick (an agent's request, or the replan of a path that stalled). An agent that asked and did not get one keeps its request on (`active` stays true) and **waits its turn: first come, first served** (the tick it first asked, then uid), so nobody starves and the order does not depend on uids. It keeps following its old path meanwhile. |
| `min_replan` (default 0.5 s, 0 = none) | The least time between two searches of one agent: an agent that asks every tick gets a new path at most that often. |
| `max_distance` (default 0 = none) | The longest search in metres. A target farther than that is walked towards in parts: the search is for the point that far along the way (its height interpolated), the agent walks that path and, at its end, asks again for the rest (it never reports *arrived* until it reaches the real target). If that point has no ground to stand on the agent walks straight to it and asks again from there. |
| `blocked_wait` (default 3 s, 0 = none) | (2026-10-25) The time an agent waits after a search that found **no way** before it may search again; its request stays on meanwhile. A target that cannot be reached (a player in a building, a closed door) keeps a chaser's autopilot asking, a failed search leaves no path to count `min_replan` from, and a failed search has looked at every cell of the box it could get to: on the city server 26 to 42 of them in 110 s cost 200 to 800 ms each. |
| `max_searched` (default 0 = none) | The most cells one search may expand: a search that has looked at that many gives up as "no path" (`gave up after N cells`). A way that exists is found in a few thousand cells (a 95 m search across the city: 2,700 to 4,800 cells, a long detour 33,000), one that does not exist is looked for in all of them, so this caps the cost of a failure (5 microseconds a cell in a city: 20,000 is 100 ms). |

**Searches done a few cells at a time: the resource `nav.cells_per_step`** (a whole number; missing or 0: off, which is how every world behaves and hashes unless it sets it). A search that has to look at tens of thousands of cells (a target that cannot be reached is looked for in every direction up to `max_searched`; a long detour in a city is 17,000 cells) costs a tick tens of milliseconds, and two of them in the same tick, as many as `searches_per_tick` allows, twice that: on a 210-player city server the twelve slowest simulation steps of a hundred seconds were all that, 27 to 72 ms each against 7 ms for a step without one. With `nav.cells_per_step` set the searches of one step expand that many cells together (3,000 is about 7 ms in a city) and a search that is not over goes on in the next step, the oldest first: the agent keeps its old path and its request stays on meanwhile, and a hard search is a few steps of a little each instead of one step of a great deal. At most `searches_per_tick` searches are on at once. The grid is made again from the world at every step, so a search sees the world as it is then and keeps the costs of the cells it has already looked at; in a world that stays as it is, the search that stopped and went on finds exactly the path of the one that did not stop (`nav_tests.rs`). The searches that are on are part of the state (copied into snapshots and hashed, only while there are some). The replan of a path that stalled is still one search in its step.

**The second look.** When the first search (the box round the two points, with a margin of three quarters of their distance) finds no way, the search looks again over the whole world, because in a small level the way round may lead far outside the box. In a world wider than 300 m that second look is skipped (`WIDE_LIMIT`, or `LUNARIS_NAV_WIDE_LIMIT` in metres for comparing): on the city server it was a grid over the whole city for every unreachable target, 2.5 s each. `LUNARIS_NAV_TRACE=1` prints one line a search (microseconds, distance, result, cells) to stderr; `nav_trace_report.py` of the a game game folder summarises it.

Everything is counted in ticks, not in microseconds, so it is deterministic and replays. The waiting list is part of the state (hashed, copied into snapshots) **only while `NavLimits` exists**: a world without it behaves and hashes exactly as before. `crates/lunaris-sim/src/nav_tests.rs` checks the order of service, the least time, the far target arriving in parts and that a copy of the simulation carries the waiting list. Not done (ideas, in the order they would pay): a per-mesh cache of the rasterised cells, so that a search does not rasterise the same shell again; a cell size per agent (a big machine does not need 0.5 m); a corridor instead of a box.

## Limits

- Boxes and spheres, no rotation (a rotated box, a capsule and a hull count as the axis-aligned box around them, as for characters); a ramp of boxes is stairs, but a ramp of mesh triangles is a slope (above).
- Dynamic bodies are not part of the map unless they carry a `NavObstacle` (a pushable crate, a sliding door, a parked car: agents then plan round where it is when they plan, and plan again when they get stuck against it); agents avoid each other only with `NavAvoid`, and then only by steering (no planning around crowds). Moving obstacles are noticed only when the agent stalls and replans.
- Collision layers (`CollisionLayer`, `docs/PHYSICS.md`) apply: an agent plans over, round and through only the colliders (boxes, spheres, hulls, terrain, mesh levels and `NavObstacle`s) that its own layer and mask collide with, so a ghost walks through doors and a guard is not stopped by a wall of another world. The `path` tool does the same for the entity it starts from (`from` as a uid) and counts every layer for a bare point.
- The grid is built for each planning request from the colliders, which is a few milliseconds for a few hundred colliders; ask for a path when a target changes, not every tick (and, for a crowd, put `NavLimits` in the world: above).

## With minds

`lunaris mind` has two verbs that use it (`docs/MIND.md`): `go_to` walks to a named thing and returns `arrived`, `blocked` (with the reason) or `still walking`; `wait` lets time pass. A 4B model that collected 2 of 12 orbs by pressing keys collected all 12 in 12 decisions with `go_to`.
