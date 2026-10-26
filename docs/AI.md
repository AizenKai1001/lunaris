# AI toolkit: perception, hearing, awareness, squads and searchlights

The senses and the awareness of a machine, as data. `Perception` gives an AI sight (a cone, with line of sight), hearing, an awareness meter that fills and drains, a memory of where its target
was, an alert that spreads to its squad and, if you want it, an autopilot that walks to the target, turns to face it and holds a `Weapon`'s trigger. `Searchlight` is a beam (a lighthouse, a
spotlight on a drone, a camera) that exposes whatever stands in it. None of it draws or plays anything, and none of it needs art: the machines' bodies, their animations and their sounds are
the game's.

It sits beside what the engine already has: `BehaviorTree` and `StateMachine` (decide), `NavAgent` (walk), `Steer` (flock, seek, flee), `Sensor` (the nearest thing in a cone), `HitPoints`, and
`Weapon` (`docs/COMBAT.md`). `Perception` is the part that knows what a machine has noticed. Code: `crates/lunaris-sim/src/perception.rs` (tests in `perception_tests.rs`).

## A guard that sees, chases and shoots, in one entity

```json
{ "uid": 10, "components": {
    "Transform": { "pos": [0, 0, 0] },
    "Team": { "id": 2 },
    "HitPoints": { "value": 80, "max": 80 },
    "CharacterBody": {},
    "NavAgent": { "speed": 4 },
    "Perception": { "sight_range": 45, "sight_fov": 110, "notice_time": 0.8, "lose_time": 8, "shout": 30,
                    "engage": true, "attack_range": 25 },
    "Weapon": { "damage": 12, "rate": 450, "muzzle": [0, 1.4, 0], "owner": 10 } } }
```

It looks along its local -z from `sight_eye` (1.6 m) above its `Transform`. A hostile in view fills its awareness; at 1 it is alert, walks (its `NavAgent`) to a spot at attack range from the target,
turns to face it and fires its own `Weapon` while the target is visible within `attack_range`. If it loses sight it searches the last place it saw it, then gives up after `lose_time`. Allies within `shout`
metres start searching where the target is. Without `engage` the toolkit only senses: you decide.

**Teams.** A `Perception` notices entities that have `HitPoints` and a different `Team` (or any, if its own team is 0: give the machines a team, one id for all of them, and the players another).
`check` warns about a `Perception` with no team.

## `Perception`

Inputs:

| field | meaning |
|---|---|
| `sight_range`, `sight_fov`, `sight_eye`, `los` | metres, degrees (the whole cone), the eye's height above the `Transform`, and whether walls block sight; a thing within 1.5 m is always noticed |
| `hearing` | 0 deaf, 1 normal, 2 sharp: a noise is heard when it is nearer than its own radius times this |
| `notice_time`, `lose_time` | seconds to fill the awareness at the edge of the range (three times faster point blank), and to drain it when nothing is seen |
| `shout` | metres: when this AI becomes alert, same-team AIs this near start searching where the target is (0: it keeps it to itself) |
| `engage`, `chase`, `turn`, `attack_range`, `weapon` | the autopilot: walk the `NavAgent` (`chase`), face a target it sees (`turn`), hold the trigger of `weapon` (its own `Weapon` when 0) while it sees the target within `attack_range`. A target it sees is approached to a ring round it that is 0.9 of `attack_range` minus the agent's `NavAgent.arrive` from it (never nearer than 0.45 of the range), so the machine ends inside its own reach: a melee machine bites a player who stands still (an `arrive` above about 0.6 of the range cannot be honoured) |

Outputs (the engine's, read them from a behaviour tree, a state machine or a graph system):

| field | meaning |
|---|---|
| `state` | 0 idle, 1 suspicious (it sees something, not sure yet), 2 alert (sees the target, sure), 3 searching (knows where the target was) |
| `awareness` | 0 to 1 |
| `target`, `visible`, `distance`, `seen_count` | who, whether it sees it this tick, how far, how many hostiles it sees |
| `seen_pos`, `since_seen` | the last place it saw (or heard) the target, and for how long ago |
| `heard`, `noise_pos` | a noise reached it this tick, and where from |

Events (read through an `EventWatch`, `docs/GAMEPLAY.md`): `spotted` (became alert; at the target), `lost` (alert to searching), `forgot` (back to idle), `heard` (at the noise).
A behaviour tree reads it directly: `cond Perception.state == 2`, `cond Perception.state == 3`.

```
selector
  sequence
    cond Perception.state == 2
    act event attack
  sequence
    cond Perception.state == 3
    act event investigate
  act event patrol
```

## Noise

An AI hears a `Noisy {radius}` (metres it carries now; a game's script sets it every tick from what the entity is doing: footsteps by speed, a door, a crash) and every `Weapon` that fires, to that
weapon's own `noise` (120 m; 0 for a silenced gun). Only a `Perception` of another side hears it, and only inside the noise's radius times its own `hearing`. A noise it cannot see lifts the awareness to 0.55 and
sends the AI to the place (state 3, `seen_pos` = the noise). A footstep script as a graph system:

```json
{ "name": "footsteps", "each": ["Noisy", "Velocity"], "do": [
    { "set": "Noisy.radius", "to": { "select": [ { "gt": [ { "hypot": [ { "get": "Velocity.lin[0]" }, { "get": "Velocity.lin[2]" } ] }, 5 ] }, 14, 4 ] } } ] }
```

## `Searchlight` and `Exposure`

```json
{ "uid": 1, "components": {
    "Transform": { "pos": [0, 300, 0], "rot": [0.0, 0.0, 0.0, 1.0] },
    "Team": { "id": 2 },
    "Searchlight": { "range": 400, "angle": 4, "sweep": 20, "hold": 2.0 } } }
```

A cone from the entity along its local -z. With `sweep` (degrees a second) the engine turns the entity round the vertical axis: a lighthouse (keep the entity free of a `Parent`; tilt it down with its own
rotation). Hostile entities with `HitPoints` inside the cone, with no wall in the way (`los`), are exposed: their `Exposure.value` fills in `hold` seconds and drains by their own `decay` (0.5 a second, set it on the
target to make cover or crouching hide them faster) when they leave the light; at 1 they are **detected**. The light reports `lit` (how many are in it), `target` and `exposure` (the most exposed), `detected`.
Events: `lit`, `unlit`, `detected` (at the target). A game answers `detected`: send machines (set a hunter's `Perception` to alert, or give its `NavAgent` a target), raise the alarm, play a siren.

## The director: when the machines come

`Heat` is a meter on a player (0 up to 10, falling by its `decay`, 0.04 a second). A `Director` watches every entity with a `Heat`: it raises the heat when the player is **detected by a `Searchlight`**
(`light_heat` a second), **fires a gun that can be heard** (`shot_heat` for each tick a `Weapon` the player owns fires with `noise` above 0), or **is seen by an alert machine** (`seen_heat` a second for each, up to
three), and when the heat reaches `threshold` it **sends a wave**: hunters made from the game's own prefab (`prefab`: its body, a `Team`, a `Perception` with `engage`, a `NavAgent`, a `Weapon`) on a ring
`min_dist` to `max_dist` metres round the player, `height` above its feet, facing it and already searching where it stands. A wave is `1 + per_heat` times the heat above the threshold hunters, at most
`max_alive` of that director's alive in all, costs the player `wave_cost` of heat, and the next one for that player waits `cooldown` seconds. A hunter that has gone back to idle for `cleanup` seconds is removed. `clear` (2026-10-25, default 0: off) is for a world of buildings: a hunter dropped on the far side of a block flies at its own height and steers only round what it sees ahead, so it can hover and circle for ever without the player in sight; with `clear` above 0 the director draws up to that many more places for each hunter until one has a clear line to the player's head (one gathering of the static world for the wave; the first draw is kept when it already has one, and when none does).

```json
{ "uid": 3, "components": { "Director": { "prefab": "Hunter", "threshold": 1.0, "light_heat": 0.6, "max_alive": 8,
                                           "min_dist": 50, "max_dist": 90, "cooldown": 30 } } }
{ "uid": 7, "components": { "Transform": { "pos": [0, 0, 0] }, "CharacterBody": {}, "HitPoints": {}, "Team": { "id": 1 }, "Heat": {} } }
```

Events: `wave` (the director, at the player) and `hunter` (each new one). The director's `waves`, `sent` and `alive` and the player's `Heat.waves` are the engine's. A game may add to `Heat.value` itself
(a loud alarm, a kill, a raid); a game with several regions uses several directors with different prefabs (the first active director's heat numbers are used for all). The whole of a hunter is the game's: the
engine only decides when, where and how many, and tells them where the player is.

## Hacking: the goggles and taking over a machine

A player with a `Hacker` (the goggles) can take over a machine that has a `Hackable`: aim at it, hold the hack, and after a while it is yours.

```json
{ "uid": 1, "components": { "Transform": {}, "Team": { "id": 1 }, "Heat": {}, "CharacterBody": {},
                            "Hacker": { "range": 30, "speed": 1, "trace_heat": 0.5, "camera": 5 } } }
{ "uid": 2, "components": { "Transform": { "pos": [0, 1, -10] }, "Team": { "id": 2 }, "Velocity": {}, "Weapon": {},
                            "Perception": { "engage": true },
                            "Hackable": { "difficulty": 4, "range": 25, "duration": 60, "speed": 8 } } }
```

The game sets `Hacker.target` (what the crosshair is on) and holds `Hacker.hack`. With **`Hacker.aim`** the engine chooses the target itself while `hack` is held: the `Hackable` within range whose direction
from the goggles (the hacker's place and 1.6 m) is nearest to `Hacker.look` (the way the player looks, a unit vector in the world that the game sets) and within `aim_cone` degrees of it (4); nothing when none is. Within the smaller of `Hacker.range` and `Hackable.range`, and (with `los`) in line of sight, the machine's `progress` fills in
`difficulty` seconds (`Hacker.speed` 2 makes it twice as fast) and the hacker's own `Heat` rises by `trace_heat` a second: the machines' network notices, and the director (above) answers. At 1 the machine is
**taken**: it joins the hacker's `Team`, its own `Perception` and autopilot stop (its AI no longer fights the hacker's side), `Hacker.state` is 2 and `Hacker.controlled` is its uid, and the `camera` (an entity with a
`CameraRig`) follows it instead of the player. While controlling, the game sets `Hacker.move_dir` (a direction in the world, up to length 1; a longer one is not faster) and `Hacker.fire`: the engine drives the machine
(its `Velocity` accelerates by `Hackable.accel` to `Hackable.speed`; a machine with a `CharacterBody` is told to walk) and holds its `Weapon`'s trigger. `Hacker.release` true for a tick gives it back. Control also ends
when `Hackable.duration` seconds are up (0 means no limit), the machine is destroyed, or the hacker dies; then the machine goes back to its team and its AI, the camera goes home, and a hack key that is still held
does not start another hack until it has been let go once. With `Hackable.keep` true a released machine stays on the hacker's side and its AI works again, now against the old side (an ally drone).
While nobody hacks a machine its progress falls back (half a second of hacking each second).

**A turret is turned, not taken** (`Hackable.mode` 1, docs/SENTRY.md): at 100 percent it joins the hacker's `Team` for `duration` seconds with its own AI running (against the old side, and never at its hacker), the hacker stays free (no `state` 2, no camera, nothing to drive) and it goes back when the time is up or it is destroyed. Events `hacked` and `link_lost` as above.

**Online** (docs/GAME_SERVER.md): put a `Hacker` with `aim` on the avatar prefab; the server sets `look` from the player's yaw and pitch, `hack` from the key (`NetAvatar.hack_mask`, bit 32), and while a machine is
taken drives it from the player's hands (`NetPlayer.axes`: 1 up, 2 forward, 3 right, in the way the player looks; the fire button fires the machine's weapon; the reload button gives it back; the player's own gun is off).
It tells the player (`VIEW`, the machine's server number: `NetPlayer.viewing`) and the client puts the player's camera (`NetPlayer.camera`, a child of the player) at the machine's puppet until it is given back (`NetPlayer.eye`).

`Hacker.state` is 0 idle, 1 hacking, 2 controlling; `Hacker.progress` and `in_range` drive the interface. Events (through an `EventWatch`): `hack_start`, `hacked`, `released`, `link_lost`. The player's body
stays where it is while it controls: the game decides what that looks like (a vulnerable pose, a fade, a picture-in-picture) and whether the player's own input is disabled meanwhile.

## What to supply

The engine ships no art. A game brings: the bodies and animations of its machines (walk, turn, attack, hit reactions, death), the beam's look (a volumetric cone, a light, a lens flare: the engine only does the
detection, not the picture), alert and search sounds, a siren, and the interface for exposure (a vignette, an eye icon). Read `Perception.state` and `Exposure.value` to drive them.

## Limits (so far)

- One primary target per AI (the nearest it sees); `seen_count` says how many there are. No threat ranking, no per-target memory, no hearing through walls attenuation (a noise carries its radius through anything).
- Sight is the middle of the target's body; a target half behind cover is seen if its middle is.
- No light or darkness effect on sight yet (a player in shadow at night is as visible as at noon); a `Stealth` multiplier is the natural next addition.
- The autopilot walks and shoots; flanking, taking cover, retreating and coordinated tactics are for a behaviour tree on top (and an AI director above that).
- Hacking has no counter-hack, no hack minigame and no per-machine security levels beyond `difficulty`, `range` and `los`; a game adds them (a graph system can raise `difficulty` as the alarm level climbs).

## Heavy machines and the Core's memory

A machine too big for the toolkit's autopilot (a Walker: 5 m tall, plated, with a slow cannon and a stomp) is `Pursuit` (legs that turn like a heavy thing), `Cannon` (a ballistic gun on a child), `Stomp` (an area blow with a wind-up) and `Hitbox` (a part that is shot as a thing of its own:
a power core on the back) on a body that has a `Perception`, a `NavAgent` and an `Equipment`. And the director gains a memory: with an `Escalation` in the scene every player's `Heat` has a slow `alert`, a `tier`, and at the top tier the Core sends a Walker, with a cooldown and a cap;
the tiers also scale the waves of hunters. All of it is in `docs/WALKER.md`.
