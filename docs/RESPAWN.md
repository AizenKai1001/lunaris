# Death and respawn: corpses, spawn points, a new life

What happens to a player whose `HitPoints` reach 0 in a survival game, with nothing to draw: a **corpse** is made where they fell (the game's prefab) holding a copy of what they carried, their own bag is emptied,
and after a delay they get up again at a **spawn point**: their own (a sleeping bag) or a shared one that is far from the other living players. The corpse prefab, the death screen and the sounds are the game's.
Code: `crates/lunaris-sim/src/respawn.rs` (tests in `respawn_tests.rs`, which are the examples below).

## A player

```json
{ "uid": 10, "components": {
    "Transform": { "pos": [0, 0, 0] },
    "HitPoints": { "value": 100, "max": 100 },
    "Inventory": { "size": 24 },
    "Vitals": {},
    "Respawn": { "corpse": "corpse", "delay": 5, "group": 7 } } }
```

`Respawn` needs the `HitPoints` beside it (it is dead when `HitPoints.dead`: the engine sets it when `value` reaches 0). `Inventory`, `Vitals`, `Heat` and `Exposure` are optional: those that are there are
emptied or reset.

| Field | Default | Meaning |
|---|---|---|
| `corpse` | none | the prefab made where the player fell (a `Name`, a model, a collider, whatever a body is; the engine adds a copy of the bag as its `Inventory`, so a prefab may have one or not) |
| `corpse_time` | 300 | seconds until the corpse is removed (a `Lifetime`); 0 keeps it for ever |
| `keep_items` | off | the player keeps their bag as well as the corpse getting a copy (for a game with no item loss) |
| `delay` | 5 | seconds from dying to getting up |
| `hold` | off | wait for the game to set `request` first (a death screen with a Respawn button), after the delay |
| `group` | 0 | the player's number for `SpawnPoint.group`: a point with the same non-zero group is theirs |
| `avoid` | 80 | metres a shared spawn point must be from every other living player |
| `food` | 80 | hunger and thirst of the new life (percent) |
| `protect` | 3 | seconds of grace (`HitPoints.invulnerable`) after getting up: hits in it are undone |

The engine writes `state` (0 alive, 1 dead), `timer` (seconds to go), `deaths`, `corpse_uid` and `died_at`. Events: `corpse` (the corpse's uid, at its place) and `respawned` (the player, at the spawn point).

## Spawn points

(A player in a **party** (docs/PARTIES.md) also gets up at a place of a party mate's group when it has none of its own: the choice below is its own group's places, then its party mates', then the shared ones.)

```json
{ "uid": 20, "components": { "Transform": { "pos": [100, 0, 50] }, "SpawnPoint": { "radius": 8 } } }
```

The entity's `Transform` is where the feet go and which way the player faces. `radius` scatters the player over a disc round the point (by a hash of the tick, the player and the point: the same everywhere, not
the same for everyone). `active` false switches a point off; `once` uses it up when somebody respawns on it (a sleeping bag); `uses` counts. `group` 0 is shared; a point with the **group of the player** belongs
to them: a game that lets a player put down a sleeping bag spawns a `SpawnPoint` with `once` and the player's number as `group`, and `Respawn.group` is that number.

The choice, in order: a point of the player's own group (the lowest uid, however near anyone is); else, among the shared points, one that is at least `avoid` metres from every other living player (one of
them by a hash, so the places are all used over time); else the shared point **farthest** from the others (with equal distances the lowest uid); else, with no point at all, where they fell. Points of other
players' groups are never used. The tests build exactly these cases (`a_shared_spawn_point_far_from_the_living_is_chosen`, `a_players_own_place_is_used_whoever_is_near_and_a_used_one_is_gone`).

## The new life

Full `HitPoints` (and `dead` cleared, `protect` seconds of grace), `Velocity` stopped, `Vitals` at `food` percent hunger and thirst, full stamina, no bleeding, `Heat` and `Exposure` cleared, the player's own
bag empty unless `keep_items`. A player the game revives by itself (sets `HitPoints.dead` false) before the delay is alive again with no respawn.

## What to supply

The corpse prefab (body, a search prompt, a collider; the engine puts the loot in it), the death screen and its button (`hold`/`request`), the sleeping bag item and what it does (spawn a `SpawnPoint` with the
player's group), the sounds. A corpse is a container like any other: the game's looting UI shows its `Inventory`.

## A craft in progress when they die (2026-10-23)

A timed craft (`recipe ... ~ seconds`) takes its ingredients when it starts and gives the product when the time is up, so a player who died in between would lose both. The engine now gives the ingredients of the
recipe in hand (`Inventory.crafting`, the recipe's number plus one) back before the bag is copied: into the corpse's copy, or into the player's own bag with `keep_items`. `crafting` is reset to 0 either way, so no
product arrives after death. Needs the game's `ItemTable` resource (a game without one has no timed crafts to refund). Tests: `respawn_tests.rs` (`a_craft_in_hand_when_they_die_leaves_its_ingredients_in_the_corpse`,
`..._when_the_items_stay_goes_back_into_the_bag_that_stays`, `a_death_with_nothing_in_hand_and_nothing_carried_still_leaves_no_items`).

## Limits (so far)

- A copy of the *bag* is dropped, not equipped or worn items that the game keeps elsewhere: the game moves them into the inventory before they die (or `keep_items`).
- Places with other living players are judged on the ground plane (x and z), not in height.
- The corpse is made on the server's tick that the death was noticed: over the network (`docs/NETWORK_SCALE.md`) the server runs it, and the corpse and the respawn reach the players as ordinary entities and state.
- No wipe-the-map or bag limits: how many bags a player may put down is the game's.
