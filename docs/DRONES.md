# Flying machines: drones that hold a height, steer, follow, circle and patrol

A `FlyAgent` is the mind of a drone, a scout, or anything else that floats. It moves its entity by setting its `Velocity` (it is kinematic, like a `Steer`: no physics body needed), holds a height over whatever
is under it, goes to a place, follows a target or circles it at a distance, patrols a list of points, steers round what it sees ahead and away from the other agents, and, with `perception`, lets its own
`Perception` (docs/AI.md) decide among those. It makes no model and plays nothing: the body, the rotors' sound and the weapon are the game's. It is a classical controller: nothing is learned and nothing
is random. Code: `crates/lunaris-sim/src/flyagent.rs` (tests in `flyagent_tests.rs`, which are the examples below).

## A drone

```json
{ "uid": 7, "components": {
    "Transform": { "pos": [0, 6, 0] }, "Velocity": {},
    "Team": { "id": 2 }, "HitPoints": { "value": 40, "max": 40 },
    "Collider": { "shape": 0, "size": [0.8, 0.8, 0.8] },
    "Perception": { "sight_fov": 360, "sight_range": 60, "engage": true, "attack_range": 30 },
    "Weapon": { "damage": 8, "rate": 300, "range": 40, "spread": 1.5 },
    "FlyAgent": { "perception": true, "patrol": "0 0 -30\n40 0 -30\n", "speed": 14, "range": 14, "orbit": 6, "hover": 8 } } }
```

`Perception` with `engage` turns it to the target it sees and holds the `Weapon`'s trigger (docs/AI.md); the `FlyAgent` moves it. With no `Team`/`Perception`/`Weapon` it is a plain flier.

## What it does (`mode`)

| `mode` | Behaviour |
|---|---|
| 0 | hold: hover where it is (it still keeps its height and keeps apart from the others) |
| 1 | go to `goal` (x and z; the height is `hover` above the ground there), slowing so that it can stop on it; `arrived` when it is within two metres |
| 2 | follow `target` (an entity's uid), staying `range` metres from it on the ground plane |
| 3 | circle `target` at `range` metres at `orbit` m/s round it (negative the other way): the ring it flies is the one an attacking drone strafes round its prey |
| 4 | patrol `patrol` (text, one `x y z` a line, `#` starts a comment; at most 64 points; only x and z are used): `wait` seconds at each point, `patrol_at` the one it is going to, `wait_left` the wait that is left |

With **`perception`** on, the entity's own `Perception` chooses: alert (`state` 2) it circles the target it sees (mode 3); searching (3: it heard a noise or lost the target) it goes to `seen_pos`; calm it patrols
(mode 4 if there is a patrol, else it holds). A `Hackable` machine that the player takes over stops flying itself (`FlyAgent.active` is switched off, and back when it is released) and flies where the hacker steers it.

## The body

`speed` (m/s, 12), `accel` (m/s^2, 10: the velocity changes no faster), `climb` (m/s up or down, 6), `turn_rate` (degrees a second it turns to face the way it goes, 180; `face` off leaves its rotation
alone, and it never turns a machine that is engaged: the `Perception` autopilot turns that one to its target), `hover` (metres above what is under it, 6), `avoid` (how far ahead it looks for obstacles, plus
what its speed will carry it in 0.4 seconds; 0 switches avoidance off), `separation` (the distance it keeps from other agents, 5).

**Height**: a ray down finds the ground (or a roof, a platform, a car) and the agent climbs or sinks to `hover` above it at up to `climb`; over a platform 4 m high it flies 4 m higher
(`the_hover_follows_the_ground_up_and_down`). A ray up stops it climbing into a ceiling.

**Obstacles**: three rays ahead (straight and 35 degrees to each side). One that is blocked ahead turns it to the freer side and slows it in proportion to how near the obstacle is; where both sides are blocked too
it climbs over; a wall to one side only pushes it off a little. `blocked` says something is in its way. A pillar in a straight line to the goal is passed with room to spare and the goal is reached
(`it_steers_round_a_pillar_in_its_way_and_still_gets_there`). It is reactive: a wall that closes the whole way and is higher than the ray's reach can still hold it (it climbs while the rays ahead are
blocked); a game that has long walls gives its drones a patrol round them or a higher `hover`.

**Separation**: other agents closer than `separation` push it away (two on the same spot are pushed apart by their uids), in proportion to how close they are.

## Outputs

`arrived`, `altitude` (metres above what is under it, -1 if nothing within 500 m), `blocked`, `patrol_at`, `wait_left`, `speed_now`.

## What to supply

The drone's model and its parts (the rotors turn by `speed_now` or by a `Spin`), the sound, the weapon, the prefab and where the director spawns it, the patrol points (a game writes them from its map). The
engine ships no art.

## Limits (so far)

- Kinematic: no thrust, no tilting, no crash physics (a `Helicopter` is the body that does that, docs/HELICOPTER.md); a drone that is shot is a `HitPoints` that reaches 0 and the game's `died` handler.
- Avoidance is three rays in the plane of travel: thin wires, a window to fly through and obstacles that are only above or below the ray at the moment are not seen. It does not plan a path.
- The height is above what is straight under the middle of the agent.
- Patrol points are on the ground plane: the height of a patrol is `hover` above the ground at each point.
- Many agents: each costs four rays through the gathered static world and one pass over the others for separation (all agents are compared with all others: fine for tens, not for thousands in one place).
