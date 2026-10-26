# Base depth: tiers above stone, repair, and an alarm for a base that is hit

Raiding (`docs/RAIDING.md`) lets a base be attacked and defended. This is what a base needs next, the Rust way, **decided by the server and cheap**: a third tier that costs the raider clearly more than it costs the builder, a **hammer** that mends a damaged piece for materials, a **cupboard that rings an alarm** when a blast hits the base, and a **blow** with a tool that needs no charge. Code: `crates/lunaris-sim/src/depth.rs` (tests in `depth_tests.rs`, which are also the examples below), the `repair_cost=` key and `aimed_piece` in `building.rs`, the event `build_damaged` in `blast.rs`, and the private event `NAME own` of `NetHost.forward` in `crates/lunaris-net/src/scale/simgame.rs` (test `a_private_event_reaches_the_player_it_is_about_and_nobody_else`). A worked example is a survival game scene that keeps its own build scripts outside this repository.

| Component | On | What it is |
|---|---|---|
| `Repairer` | the avatar (the entity with the bag) | the player's side of repair: `want` for one tick mends the piece in front of me |
| `Striker` | the avatar | the player's side of hitting a piece with a tool: `want` for one tick |
| `RaidAlarm` | a cupboard (an entity with a `BuildUpkeep` and a `Transform`) | rings when a piece in its range is damaged by a blast |

## More tiers: nothing new

A tier is a `piece` line with `upgrade=` (docs/RAIDING.md, "Tiers"): the stone piece says `upgrade=wall_metal upgrade_cost=scrap:40,tape:8` and the metal piece is a line of its own (same size and sockets, more `hp=`, a smaller `bullet=`). The `Upgrader` already follows the chain, one step a request. The numbers are the game's (the table is in the game's doc).

## Repair

```
piece wall_stone size=4,3,0.2 hp=900 inset=0.12 bullet=0.02 upgrade=wall_metal upgrade_cost=scrap:40,tape:8 repair_cost=scrap:10,tape:2
```

`repair_cost=ITEM:COUNT,...` is what repairing the piece **from nothing to full** costs; a piece without it cannot be repaired. The avatar's `Repairer` asks: `want` for one tick, and the engine looks along the aim (`reach` metres, 5; the avatar's head) at the **nearest piece** that no wall of the world hides (`building::aimed_piece`, the same look as an upgrade's). The rest is checked in this order, and the first that fails is the answer:

| Check | Refusal (`Repairer.denied`, event `build_repair_refused_WHY`) |
|---|---|
| the player is alive | `dead` |
| a piece in reach and in sight | `no_aim` |
| the piece's rules line has a `repair_cost` | `no_repair` |
| the piece is the player's: `BuildPiece.owner` is the player's number (`Repairer.owner`, or `NetAvatar.slot` + 1), or the owner's avatar is in the game and in the same party (`party::same_party`) | `not_yours` |
| the tool `item` is in the bag (a hammer: it is **not** used up; empty `item`: no tool) | `no_tool` |
| at least 1 hit point is missing | `full` |
| `HitPoints.hurt` is at least `delay` seconds (10): the piece took no damage for that long | `recently_hit` |
| the bag holds the cost of this repair | `no_materials` (nothing is taken) |

A repair heals `step` (0.25) of the piece's full hit points, or what is missing if that is less, and costs **that share of `repair_cost`, rounded up** (a repair of a few hit points costs one of each material; four repairs of a quarter cost a little more than the whole). Afterwards a pause of `pause` seconds (0.8) is not heard, and a refused request pauses the player a quarter of a second (a client that asks in a loop is not answered in a loop). Events: `build_repaired` (the piece, at its place) and `build_repair_ok` (the player's aim entity) or `build_repair_refused` and `build_repair_refused_WHY` (the aim entity, at the player).

**Decay is not a hit.** `HitPoints.hurt` is the time since the value last went down, and the building system's decay lowers a piece's value a little every second: it used to restart the clock every second, so no piece could ever be "left alone for ten seconds". `building::sweep` now keeps `HitPoints.last` level with the value after decay, so `hurt` means a real hit (a blast, a bullet, anything else that took hit points). A decayed piece can be repaired as soon as it was not hit (`decay_is_not_a_hit_but_a_blast_is`).

## Melee on structures

`melee=SHARE` on a `piece` line (0 to 1, default 1) is the share of a blow's damage that gets through: wood 1, stone 0.25, metal 0.1. The avatar's `Striker` asks like the `Repairer`: `want` for one tick, the engine looks at the nearest piece within `reach` (3 m) that no wall of the world hides, checks the tool `item` is in the bag (kept) and the piece's `melee` is above 0, and takes `damage` times the share off the piece's `HitPoints` (a piece taken to 0 is destroyed by the building system the next tick). Refusals `dead`, `no_aim`, `no_tool`, `immune` (event `build_strike_refused_WHY`); a pause of `pause` seconds (0.7). Events: `build_struck` (the piece), `build_strike_ok` (the player's aim entity) and **`build_damaged`** (the piece, at its place), the same event a blast tells, so the raid alarm answers a blow too. Only building pieces: a door or a crate with hit points is not hurt by it (they have their own rules, docs/RAIDING.md).

## The raid alarm

A blast (or a blow) that damages a building piece tells the world: the event **`build_damaged`**, about the piece, at the place on it that was hurt (`blast.rs`; at most 16 a blast, the nearest first). The `RaidAlarm` on a cupboard listens: if a damaged piece lies within the cupboard's range (`RaidAlarm.radius`, or the `BuildUpkeep`'s radius when that is 0) and the cupboard has not rung for `throttle` seconds (30), it rings: **`raid_alarm`**, one event for each player who owns the cupboard (`BuildUpkeep.owner`; while that is 0 the engine fills it from the cupboard's own `BuildPiece.owner`, so a cupboard built with the building tool is its builder's) and everybody in the owner's party, at the place of the damage. `RaidAlarm.last` (a tick stamp), `rung` and `told` say what it did. At most 16 players are told. An owner who is not in the game tells nobody (the party is found through the owner's avatar): the alarm is for the players who are here.

Abuse: the alarm costs the attacker a blast on a piece of the base (satchels, grenades), so it cannot be made by spamming a cheap action; one cupboard rings once in 30 seconds whatever is thrown at it; and **it does not tell the enemy where the cupboard is**: the event is private (below) and its place is where the damage was, which the owner learns and a stranger does not.

### A private event

`NetHost.forward` lines are `NAME RADIUS`: every player within RADIUS metres of the event's place gets it. The word **`own`** (`raid_alarm own`) makes the event private: only the player it is about gets it, wherever that player is, and the place in the event is the game's to say (for the alarm, where the damage was, so the player's screen can say how far and which way). The event is about an avatar (`NetAvatar` or its head); an event about something else is dropped. Cost: nothing for events that are not private; a private event is one `Outbox::to` (the avatars are looked through for the one).

## Over the network

All of it runs on the server only. What a game does: the command (`NetPlayer.acts`: `80 dp_repair`) and a graph system on the server, `each: ["NetAvatar", "Repairer"]`, that sets `Repairer.want` for the command; `NetHost.forward` lists `raid_alarm own` and the repair's own answers (`build_repair_ok 8`, `build_repair_refused_WHY 8`: the players within 8 m, as for the upgrade); the clients read `event.raid_alarm.0.x/y/z` (where the damage was) and work out the distance and the direction themselves. Everything a component keeps is in the world's save (`Repairer`, `RaidAlarm`, the metal tier is a piece kind): a restart keeps them.

## Checking a scene

`check` names a piece line with a bad `repair_cost` (not ITEM:COUNT) like the other keys; the same list of keys in `parse_rules` names the new one.

## Cost

Nothing when nobody asks: `depth::update` visits the avatars that have a `Repairer` with a request pending (none in a quiet tick), and the cupboards only in a tick in which a blast damaged a piece (`build_damaged`), plus one pass over the (few) cupboards to fill the owner of a new one. A repair is one aimed look (the same cost as an upgrade, 0.02 ms) and one bag. A blast tells at most 16 pieces. The numbers measured on a Linux test machine are in the game's doc.

## Limits (so far)

- Who owns a piece or a cupboard is the **number** of the player (`slot + 1`, `BuildPiece.owner`), as everywhere in building: after a server restart a different player can get the number that a base was built with. Parties are found only for players who are in the game.
- The alarm answers blasts and blows (`build_damaged`), not bullets: a game that lets another weapon damage pieces tells `build_damaged` itself (an event of the name, about the piece, at the place).
- A repair is a step, not a held button: the player presses again (the pause is the cooldown).
- A cupboard that is destroyed is silent, like the base it was in; whoever wants a second line of defence builds two.
