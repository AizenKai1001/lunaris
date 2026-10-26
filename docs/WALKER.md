# Heavy machines and the Core's learning: `Pursuit`, `Cannon`, `Stomp`, `Hitbox` and `Escalation`

The parts of a big armoured ground machine (a Walker, a mech, a war machine of the kind a rogue AI sends) and the director's memory that decides when to send one. They sit on top of what the AI toolkit already has
(`Perception`, `NavAgent`, `CharacterBody`, `Weapon`, `Equipment`, `Director`, `Heat`, `LootDrop`: `docs/AI.md`, `docs/NAVIGATION.md`, `docs/COMBAT.md`, `docs/EQUIPMENT.md`, `docs/LOOT.md`) and add what a machine that size needs: legs that
turn like a heavy thing turns, a gun that is aimed like artillery, a foot that comes down on whoever stays near, parts that are shot as things of their own, and a meter that makes the machines come in greater numbers the more a player does.
None of it draws, plays or makes a picture: the body, its animations and its sounds are the game's (a game's model and its slot belong to that game). Code: `crates/lunaris-sim/src/walker.rs` (`Pursuit`, `Cannon`, `Hitbox`),
`stomp.rs` (`Stomp` and [`area_damage`]), `escalation.rs` (`Escalation`, the tiers), the director's changes in `director.rs`; tests in `walker_tests.rs`, `stomp_tests.rs` and `escalation_tests.rs`. Everything is deterministic (no random
number: a Walker's place on its ring comes from a hash of the tick, the Core and the player).

## A Walker in three entities

```json
{ "components": {
    "Transform": {}, "Team": { "id": 2 },
    "HitPoints": { "value": 1500, "max": 1500, "remove_after": 30 },
    "CharacterBody": { "radius": 1.3, "height": 5.0, "step_height": 1.0 },
    "NavAgent": { "speed": 3.4, "radius": 1.3, "height": 5.0, "step_height": 1.0, "arrive": 2.4 },
    "Perception": { "sight_range": 70, "sight_fov": 140, "sight_eye": 4.4, "hearing": 1.4, "notice_time": 1.4, "lose_time": 15, "shout": 80 },
    "Pursuit": { "calm_speed": 1.8, "alert_speed": 3.4, "turn_rate": 55, "close": 2.6, "patrol": "0 0\n30 0\n" },
    "Stomp": { "radius": 4, "trigger": 3.2, "windup": 1.1, "recover": 1.4, "damage": 80 },
    "Cannon": { "range": 80, "min_range": 8, "net_kind": 81 },
    "Inventory": { "size": 4, "id": [22, 23, 24], "count": [1, 1, 1], "dur": [10000, 4000, 900] },
    "Equipment": { "slots": "slot 0 accepts=21 zones=body\nslot 1 accepts=22 zones=legs\nslot 2 accepts=23 zones=head" },
    "LootDrop": { "table": "walker", "prefab": "Pickup", "spread": 3.0 } },
  "children": [
    { "components": { "LocalTransform": { "pos": [1.38, 3.7, -2.05] },
                      "Weapon": { "kind": 1, "damage": 70, "rate": 24, "muzzle_speed": 55, "muzzle": [0, 0, -1], "infinite": true, "noise": 190 } } },
    { "components": { "LocalTransform": { "pos": [0, 3.4, 1.2] }, "Collider": { "shape": 0, "size": [1, 1, 1] },
                      "HitPoints": { "value": 1000000, "max": 1000000 }, "Team": { "id": 2 },
                      "CollisionLayer": { "layer": 30, "mask": 0 }, "Hitbox": { "mult": 3 } } } ] }
```

The root is the body: a capsule for the shots (the engine's zones by height: the top fifth is the head, zone 2, the lower 45 percent the legs, zone 3) with a `Perception` that only senses (leave its `engage` off: the autopilot snaps round and
walks to 85 percent of the attack range; `Pursuit` does what a heavy machine does instead). The first child is the gun, the second the weak point on the back. A prefab's children have the uids after the root's, which is how the engine
finds the gun (`Cannon.weapon` 0 = the first child that has a `Weapon`).

## `Pursuit`: the legs

| field | meaning |
|---|---|
| `patrol` | text, one `x z` a line (`#` comments, at most 32): the points it walks while calm; none: it holds |
| `wait`, `calm_speed` | seconds it stands at each point, metres a second while calm (1.8) |
| `alert_speed`, `close` | metres a second when it has something to go to (3.4), and how far from a target it sees it stops (2.6) |
| `turn_rate` | degrees a second the body turns (55); a machine that is facing more than 75 degrees away from the way it must go walks at a quarter of its speed until it has turned |
| `replan`, `moved`, `retry` | it plans again when the goal has moved `moved` metres (2) and `replan` seconds have passed (0.7); after a plan that found no way it waits `retry` seconds (2.5) |
| outputs | `mode` (0 hold, 1 patrol, 2 chase, 3 search), `patrol_at`, `arrived`, `wait_left`, `goal`, `plans` |

What it does follows the `Perception`: alert to a target it sees (state 2) it walks to it and faces it; searching (state 3: it lost the target, it heard a noise: the place is `seen_pos`) it walks there; suspicious (1) it stops and looks;
calm (0) it patrols or holds. It stands still while the machine's `Stomp` is winding up or recovering and when it is dead (a dead machine's `NavAgent.speed` is 0 and its body's walk is cleared). A hold gives up the `NavAgent`'s walk (status 2)
instead of leaving it to look like a walk that makes no progress, which the agent would answer with plan after plan. The body's walk is cleared too: the agent writes it only while it controls the body.
A big agent plans as a big agent: give the `NavAgent` the capsule's `radius` and `height` (walls are grown by the radius: a 1.3 m machine does not fit a 2 m doorway, and plans round).

## `Cannon`: the gun

A `Weapon` of `kind` 1 on a child (a flying round: `muzzle_speed`, `gravity`, `damage` with the falloff, the zones' factors: set `head_mult` and `limb_mult` to 1 for a shell that hurts the same anywhere) and a `Cannon` on the machine. Every tick, while the machine is
alert to a target it sees, the cannon turns the gun's `LocalTransform` (a turn about the vertical, then a pitch):

* **pitch**: the angle that makes a shell of the weapon's speed under its gravity land on the target (`launch_angle`: the flatter solution of the ballistic equation), led by `lead` (0 to 1) of the way the target has been moving
  (the change of its place, smoothed over a few ticks), at `aim_height` of the target's height, within `pitch_min` to `pitch_max` degrees;
* **turn**: the heading from the muzzle to that point, within `arc` degrees either side of the way the body faces (a body that turns at 55 degrees a second is followed by a barrel that turns at `slew`, 70).

It holds the trigger while the target is between `min_range` and `range` metres on the ground plane, the aim is within the barrel's reach (the arc and the pitch limits: a target too close to be looked down on is not fired at) and the barrel is within
0.6 degrees of it, and not while the machine's stomp is at work. **The splash**: a shell that lands this tick (the gun's `hit_dist` above 0: on a wall or on something) also hurts what is within `blast_radius` metres of the place (`blast` at the middle,
`blast_edge` of it at the edge, `structure` times as much for things that are not characters: the building system's pieces and vehicles), never a character that the shell hit (it took the shell's own damage; a wall or a vehicle that the shell bursts on takes the splash on top of it, so a wall piece takes the shell and two and a half times the splash) and never the machine's own team; the event
`cannon_blast` says where. **Shells shown**: a `Bullet` is not a `NetEntity` by itself; with `net_kind` above 0 the cannon makes each of its shells one of class 2 and that kind (a game server then tells the players near it, and the game's `kinds` line shows
a ball for it).
Outputs: `pitch`, `yaw` (degrees), `aimed`, `firing`, `shots`, `blasts`, `since_shot`, and what it tracks of the target.

## `Stomp`: an area blow with a wind-up

Calm (`phase` 0, `wait` run out), when the target the `Perception` is alert to stands within `trigger` metres (on the ground plane) the wind-up starts (`phase` 1, the event `stomp_windup`) and lasts `windup` seconds: that is the time a game plays the raised foot and a
ring on the ground in, and in which a player can run out of the radius (the blow lands where the machine's foot is, `reach` metres in front of it, whatever the player did). At the end it hurts, once, every hostile thing with `HitPoints` whose shape reaches into
`radius` metres of the spot and into the band from 1 m below it to `height` metres above: `damage` at the middle falling in a straight line to `edge` times that at the radius; a character takes it through its worn gear in `zone` (the same
`armor::absorb` a bullet goes through), anything else takes `structure` times as much. Events: `hit` and `kill` for each (as for a bullet: the game's hit markers and corpses work), then `stomp` (the machine, where it landed). `recover` seconds of standing still
follow (`phase` 2), then `cooldown` seconds. The machine's team must not be 0 (it would hurt itself and its friends: the blow spares what has the same non-zero `Team`, and the machine itself). `area_damage(world, events, centre, radius, damage, edge,
structure, zone, team, skip, below, above)` is the one function under it (and under the cannon's splash): everything hostile within a radius of a point takes a share. It is not an explosion system (no knock-back, no line of sight, no fire).

## `Hitbox`: a part that is shot as a thing of its own

The weapons system knows three zones of a character (head, body, legs) from the shot's height. A weak point on the BACK, or a rotor, or a power core is not a height: it is a place. A `Hitbox` is a small ghost on a child of the machine: a sphere `Collider`
(not a trigger) with `HitPoints` of a `max` too big to be used up, a `CollisionLayer` with `mask` 0 (nothing walks into it, plans round it or is stopped by it: both sides' masks have to agree, and a shot does not ask), the machine's `Team`, and a
`Hitbox { mult }`. A round meets it as it meets any target that can be shot, the nearest first: put the sphere where it sticks out of the machine's capsule on the side that is weak (0.4 m out of the capsule's back), and shots from behind meet it first while
shots from the front meet the body. After the weapons of the tick the engine takes what the part lost off the machine, times `mult`, with no worn gear in between, and puts the part's hit points back (`taken` counts what it has taken in all, `hits` the
ticks). A machine that dies of it says `kill` as for a bullet. The shot's own `hit` event is about the part (its uid): a game that shows hit markers by the victim's number should know that. The part has no `NetEntity`: the clients never see it.

## The plating: `Equipment` on a machine

The machine has an `Inventory` and an `Equipment` (`docs/EQUIPMENT.md`): the plates are items (`class` 21 to 23, `power` the percent of a hit they stop, `dur` how much damage they stop before they are used up) in the worn places, one place for each zone it
covers. The weapons system's hit goes through `armor::absorb` for any victim that has them, so nothing here is special: a plate stops its share, wears by what it stopped and breaks (the event `item_break`), and a machine whose body plate is gone takes whole hits
in the body. The numbers decide how a machine is fought, and a plate that wears out is what makes "shoot the same place" a plan: with 1500 hit points, a body plate of 92 percent and 10,000 points, whatever the weapon, the hit points a body shot takes are
8 percent of it until the plate is gone, which takes `10000 / (0.92 x damage)` rounds, and then all of it. Total damage to destroy through one zone = the machine's hit points + that zone's plate points (the plate only slows the hit points; it does not
change what they are): that is the number to design by (a game keeps its own table).

## `Escalation`: the Core learns

One entity of the scene has an `Escalation` (the lowest uid that has one). Every player with a `Heat` carries, beside the heat, a slow meter, `Heat.alert`, with its `tier` (0 to 7) and the wait for the next Walker (`walker_cd`):

* **it rises** by `heat_gain` times every bit of heat the player gains, from any source (the director's own: the beam, audible shots (a gun's `owner`, or when it has none the entity it is a child of, which is how a player's tool gun on its avatar's head counts), being seen; and everything else that adds to `Heat.value`: a helicopter's noise, a hacking trace, a game's script; the engine
  counts what was added since the director last left the heat), by `kill_gain` for each machine destroyed within `share` metres (a machine is an entity with a `Perception`; `heavy_gain` for one with a `Stomp`: a Walker), and by the gain of each event
  named in `noise` (text, `EVENT GAIN` a line: an explosion; the event needs a place, the engine's own events of the tick and what a graph system emitted both count), all of those the nearer the more (a straight line to 0 at `share`);
* **it falls** by `decay` a second (the scene's 0.008: ten minutes to lose a tier's worth), never below 0, never above `max`;
* **the tiers**: `tiers` is text, `tier N at=ALERT hunters=MULT cooldown=MULT alive=ADD per_heat=MULT` a line (N from 1, no gaps, thresholds rising; empty text is `DEFAULT_TIERS`). A player is in the highest tier whose `at` the alert has reached and leaves it
  when the alert is `hyst` below it, one tier at a time (no flicker). In a tier the director's waves for that player are `hunters` times as many (after `per_heat` times the growth with the heat), the wait after one `cooldown` times as long and up to
  `alive` more of that director's hunters may be alive. Tier 0 changes nothing. A scene without an `Escalation` is the old director exactly.

**The Walker**: at tier `walker_tier` (0: never) the Core sends the prefab `walker` to a player who has no Walker within `walker_near` metres and has not been sent one for `walker_cooldown` seconds: on the ground `walker_min_dist` to `walker_max_dist` metres away
(eight places on the ring, tried in an order from a hash: the ground is found by a ray down from 8 m above the player's own level and must be level, nothing may be over it — a roof, an overhang or the inside of a solid mass — and the `clearance` round it
must be free of walls up to 3.5 m; a flat roof lower than 8 m above the player counts as ground), `walker_height` above it, looking at the player and already searching the place where the player is (the engine's `Perception` state 3, the player as target).
When no place fits, it tries again in four seconds. At most `walker_max` Walkers are alive at a time over the whole server (the cap: a wreck, `HitPoints.dead`, is not counted); a Walker that is calm for `walker_cleanup` seconds is removed; every few ticks (6) the
count and the clean-up run. A new life forgets: a respawn clears `alert`, `tier` and `last`.

Told to the game: `Heat.tier` of each player and, on that player's client, `NetPlayer.tier` (the server sends it with the vitals: bits 3 to 5 of the `STATUS` flags, protocol-compatible), the resources `ai.tier`, `ai.alert` (the highest of any player: sync them with
`NetHost.sync`, a lighthouse that sweeps faster, a siren, a darker sky answer them in the game's own graph systems), `ai.walkers`; the events `tier` (the player, at its place: its tier changed) and `walker` (the new Walker, at its place; forward it with `NetHost.forward`
to warn the players near it). Outputs on the component: `tier`, `alert` (the highest), `walkers`, `sent`, `kills`.

## What it costs

(See the end of the game's doc 28 for the measured numbers: the cost of a Walker in a tick of the real survival scene and of a server with players.) By construction: `Pursuit`, `Cannon` and `Stomp` visit only the machines that carry them (an archetype
query: nothing for a world without one); the cannon asks for the target entity it is alert to, not for every target; a stomp asks for the targets once, when the foot comes down; the escalation does a few comparisons for each player that the director already
visited, and the Walkers are counted, cleaned up and sent every sixth tick. A plan of the `NavAgent` costs what it always costs (the grid is built for each plan: a few milliseconds for a few hundred colliders), which is why `Pursuit` plans at most every
0.7 s and only when the goal has moved 2 m.

## What to supply (not made by the engine)

The machine's body, its walk, stomp, fire and death clips, its sounds (the steps, the wind-up, the stomp, the cannon, the plates breaking: `item_break` says when), a picture for the plates, the shell, the stomp's ring and dust, the tier on the interface, the siren.
Read `Stomp.phase`/`timer`, `Cannon.since_shot` and `Equipment.cover` from a graph system on the machine's entity to drive them (the game's `vb_walker_net` writes `NetEntity.anim`, `anim_t` and `flags` that way).

## Limits (so far)

- It targets what the toolkit's `Perception` picks: the nearest hostile thing it sees with `HitPoints` (no team, or another team): a player, and also a building piece or a parked car when that is nearer. There is no threat ranking and no preference for players (a game that wants one gives the pieces the machines' team, or a `BehaviorTree` above).
- One gun. A turret of several barrels, a machine that swaps between guns, or one that fires on the move at a target it cannot see are for a behaviour tree on top.
- Facing is a heading about the vertical: no leaning, no foot placement. The stomp's centre is a point in front of the feet, not a foot's real place in a clip.
- The stomp and the splash push nobody (a client moves its own character: the server cannot knock a player back) and set nothing on fire.
- A machine does not climb, jump or break what is in its way: a wall that closes the way is a plan that fails (`NavAgent.status` 3; it waits `retry` seconds and asks again) and it fires at what it sees over it. Walls, doors and stairs a heavy machine cannot pass are not told to the
  planner except through its `radius` and `step_height`.
- The weak point on the back is a ghost sphere beside the capsule, not a part of the model: a model and its animation must keep the visible core where the sphere is.
- The Core's meter is per player (and by the kills and loud events near that player), not per area: a squad that shares a base has as many meters as it has players, and the Walker goes to the player whose turn it is. The "base" of a player is not known to the engine:
  the place a Walker goes to is the player's own place when it is sent.
- Placement of a Walker finds ground by one ray down from 8 m above the player's level: in a city with roofs a ring point on a low roof is accepted, and a player on a very high roof is out of reach of the ring (the ground there is more than 40 m below).
