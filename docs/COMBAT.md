# Combat: weapons, bullets and teams

The shooting half of a game, as data. A `Weapon` fires along its entity's forward direction when you tell it to; the engine finds what the round hits (walls stop it, a character takes it), takes the
damage off that entity's `HitPoints`, keeps the ammo, the reload, the recoil and the spread, and tells you what happened. It **draws and plays nothing**: no muzzle flash, tracer, spark, hit marker or
gunshot sound is made here. Those are yours (see [What to supply](#what-to-supply)): the engine announces the events, and you answer them with your own effects, sounds and models.

Code: `crates/lunaris-sim/src/weapons.rs` (tests in `weapons_tests.rs`). Everything is deterministic: the spread is a hash of the tick, the gun and the round number (a replay fires the same
rounds), and guns and targets are visited in uid order.

## A rifle and a target in a minute

```json
{ "uid": 1, "components": {
    "Transform": { "pos": [0, 1.5, 0] },
    "Weapon": { "damage": 30, "rate": 600, "spread": 0.3, "mag_size": 30, "mag": 30, "reserve": 120, "owner": 3 } } }
{ "uid": 2, "components": {
    "Transform": { "pos": [0, 0, -20] }, "CharacterBody": {}, "HitPoints": { "value": 100, "max": 100 }, "Team": { "id": 2 } } }
```

The gun fires along its entity's local -z (the way a `Camera` looks), so put it on the camera, or on a `Parent`ed child of the character at the shoulder, or give it the holder's uid in `owner`.
To fire, set `Weapon.fire` to true while the trigger is held, from a graph system, a script or an AI (a graph system as `define_system` text):

```json
{ "name": "trigger", "each": ["Weapon"], "do": [
    { "set": "Weapon.fire",   "to": { "gt": [ { "res": "input.mouse_left" }, 0.5 ] } },
    { "set": "Weapon.reload", "to": { "gt": [ { "res": "input.r" }, 0.5 ] } } ] }
```

(A test proves this exact system: `a_graph_system_pulls_the_trigger_from_the_mouse_and_reloads_from_a_key`.) `Weapon.reload` is a one-tick request; `fire` is the held trigger.

## What a shot hits

1. The round starts at the muzzle (`muzzle`, an offset in the gun's own axes) and goes along the gun's forward direction, turned by the cone of fire (below). Hitscan (`kind` 0) is one instant ray of `range` metres;
   a projectile (`kind` 1) is a `Bullet` entity that flies at `muzzle_speed` metres a second and falls by `gravity`, and each tick the stretch it flew is tested the same way.
2. The nearest of these is hit: **the static world** (`Collider` boxes, spheres and capsules that are not targets, convex hulls, mesh levels and terrain: cover and walls stop the round, and a
   `CollisionLayer` mask decides which of them count for this gun's holder), or **a target**: any entity with `HitPoints` that is not dead and has a shape: a `CharacterBody` (a capsule from the feet, `radius`
   and `height`) or a `Collider` (a crate, a drone, a turret). A target is never also a wall.
3. Never hit: the `owner` (0 means the entity's `Parent`), the gun's own entity, and any target whose `Team` is the owner's non-zero team. A round goes through a teammate and hits the enemy behind them.
4. The hit takes `damage` off `HitPoints.value` (so `died`, `regen` and `invulnerable` of `HitPoints` work as for any damage: a target in its grace period takes nothing and is not counted as hit).
   Damage is **full up to `falloff_start` metres and falls in a straight line to `falloff_min` times at `range`**, nothing beyond. On a character the **head** (the top fifth) takes `head_mult` times and the **legs**
   (the lower 45 percent) `limb_mult` times; a `Collider` target has one zone. A shotgun is `pellets` above 1 (each with its own aim inside the cone and its own damage).

## The cone of fire: spread, bloom, recoil

| field | meaning |
|---|---|
| `spread` | degrees, the half angle of the cone at rest |
| `bloom`, `bloom_per_shot`, `bloom_max`, `bloom_recover` | extra degrees of cone: each round adds `bloom_per_shot` up to `bloom_max`, and it falls back by `bloom_recover` degrees a second |
| `recoil` | degrees per round: `[up, sideways]` (the sideways kick is random either way) |
| `kick` | (the engine's) the recoil piled up now, `[pitch up, yaw]` degrees; it settles by `recoil_recover` degrees a second. The rounds already lean by `kick`; a camera or an aim adds it to where it looks to make the screen jump: copy it into a resource (`{"res_set":"aim.kick_pitch","to":{"get":"Weapon.kick[0]"}}`) and let the camera's system add it |
| `muzzle` | where the round starts, an offset in the gun's own axes (x right, y up, z back) |

## Rate, ammo, reload

`rate` is rounds a minute (`auto` false: one round for each pull of the trigger, however fast), `mag` of `mag_size` in the magazine and `reserve` behind it (`infinite` for no limit). A reload takes `reload_time` seconds
and takes rounds from the reserve; holding the trigger on an empty magazine starts it by itself; `reload` true asks for one. A trigger pulled with nothing to fire sets `dry` and emits `dry`.

## What the engine tells you

On the `Weapon` itself, for the tick: `fired` (a round left), `dry`, `hit` (the uid hit, 0 for none or a wall), `hit_point`, `hit_normal`, `hit_zone` (0 nothing, 1 body, 2 head, 3 legs), `hit_dist`, `hit_damage` (all the
pellets of the tick), and the running `shots` and `kills`. For a projectile the hit is written on the tick it lands, not the one it was fired. As events (read them with an `EventWatch`, `docs/GAMEPLAY.md`):

| event | uid | position |
|---|---|---|
| `shot` | the gun | the muzzle |
| `hit` | the victim | where it was hit |
| `impact` | the gun | where a wall was hit |
| `kill` | the victim | where it was hit |
| `reload_done` | the gun | none |
| `dry` | the gun | none |

```json
{ "uid": 9, "components": { "EventWatch": { "names": "shot, hit, impact, kill" } } }
```

then `event.shot`, `event.hit`, ... count the events of the tick before and `event.hit.x/.y/.z/.uid` say where and who: a graph system answers them with `spawn_prefab` (your muzzle flash, spark, blood or decal prefab at that point),
`Particles`, a `Decal`, a sound, a hit marker on the screen.

## Teams

`Team {id}` says which side an entity is on. Weapons never hurt their own team. `id` 0 is no team: hurt by everyone, hurts everyone (a free-for-all player). A squad shares an id; a game's machines share another. The
same component is what an AI's perception will use to tell friend from foe. **A party** (docs/PARTIES.md) of a game server is a team too: its members' `Team.id` is the party's number, from 100 up, so friendly fire is off inside a party and on for everyone else; `lunaris_sim::party::same_party(world, a_uid, b_uid)` is the question for explosives and the like.

## Checking a scene

`check` warns about a `Weapon` that cannot fire (no rate, magazine or range, a projectile with no `muzzle_speed`, more rounds in the magazine than it holds), about an `owner` that does not exist, and about a gun
with no owner and no `Parent` (a round fired from inside its holder's body could hit it).

## What to supply

The engine ships no art. To make a gun feel like one, a game brings (all optional, none made by the engine):

- a **model** for each weapon and, if you want bullets to be seen, a **tracer or projectile model** (spawned by a graph system on `event.shot`, or read a `Bullet`'s `Transform` every tick to draw it);
- **animations**: the holder's aim, fire, reload and hit-react clips (play them from the `fired` / `reload_left` / `hit` fields or the events);
- **effects**: a muzzle flash, shell ejection, impact sparks and dust per surface, blood, a bullet-hole decal;
- **sounds**: a shot, a dry click, a reload, impacts per material, a hit marker tick, flesh hits;
- **interface**: crosshair and bloom ring, hit markers, an ammo counter and weapon icons (`Weapon.mag`, `Weapon.reserve` are ready to show).

## Limits (so far)

- Rounds hit the first thing in the way: no penetration through thin walls, no ricochets.
- A target is one shape (a capsule or a collider): no per-bone hit boxes yet (head, body and legs are by height).
- Hitscan sees the world as it is on the tick of the shot; lag compensation for a server is the netcode's (`docs/NETWORK_SCALE.md`: `World::hitscan` keeps a second of positions).
- Fire modes with bursts and charge-up weapons are not built in (a graph system can set `fire` in bursts). Thrown and placed explosives are `docs/RAIDING.md` (`Explosive`, `Thrower`); a thing that shrugs off bullets has a `Resist` (`bullet` is the share of a bullet's damage that gets through; asked on every hit, before armor).
