# Raiding: explosives, code locks and tiers, so that a base means something

Base building (`docs/BUILDING.md`) only matters if a base can be attacked and defended. This addon is the attacker's side and the defender's answer, the Rust and DayZ way, **decided by the server and cheap**:
**explosives** (a grenade thrown along the look, a charge put on a wall, a blast that hurts what is near it and breaks what people built), **code locks** (a door or a box that opens only for the players who know its
four digits, with three tries and a lockout), and **tiers** (a wood piece becomes a stone one for materials from the bag; bullets barely hurt a base). It makes no model, plays no sound and draws no page: the
grenade's picture, the keypad's screen and the explosion's sound are the game's. Code: `crates/lunaris-sim/src/blast.rs`, `lock.rs` and the tier part of `building.rs` (tests in `blast_tests.rs`, `lock_tests.rs`, `tier_tests.rs`,
which are also the examples below). A worked example is a survival game scene that keeps its own build scripts outside this repository. What a base needs next (more tiers, repair, melee on pieces, an alarm): `docs/DEPTH.md`.

| Component | On | What it is |
|---|---|---|
| `Explosive` | the grenade, the charge, a barrel | goes off by a fuse, by touch, by its owner's trigger or when it is destroyed |
| `Thrower` | the avatar (the entity with the bag) | the player's side: what can be thrown or put down, the caps, `want` and `trigger` |
| `CodeLock` | a door or a container | the code, who entered it, who was wrong lately |
| `Keypad` | the avatar | the player's side of locks: enter a code, set a code, fit a lock |
| `Upgrader` | the avatar | the player's side of tiers: make the piece in front of me the next tier |
| `Resist` | a piece, a door, anything with `HitPoints` | what share of a bullet's damage gets through |

Everything the player asks is a **request** the game sets for one tick (from the avatar's command: `NetAvatar.cmd`, `docs/GAME_SERVER.md`); the engine answers at once, clears it, and says what it did in **events**.
Nothing a client says is believed but the request itself: the item must be in the bag, the player alive, the cooldown over, the caps respected, the surface or the piece within reach and in sight.

## Explosives

```json
{ "uid": 7, "components": {
    "Transform": { "pos": [0, 0.9, 0] }, "Collider": { "shape": 0, "size": [0.12, 0.12, 0.12] }, "RigidBody": { "mass": 0.4 }, "Velocity": {},
    "Explosive": { "fuse": 3.5, "radius": 6.5, "damage": 150, "core": 0.6, "edge": 0.1, "cover": 0.2, "structure_mult": 0.8, "tag": "grenade" } } }
```

| Field | Meaning |
|---|---|
| `fuse` | seconds from its first tick to going off (3); below 0 it never does by time |
| `radius`, `damage` | the blast hurts every entity with `HitPoints` within `radius` metres: the distance is to the **nearest point of the thing** (a capsule for a character, the box of a collider or of a piece), not to its middle |
| `core`, `edge` | `damage` in full inside `core` metres, falling in a straight line to `edge` times that at `radius`, nothing beyond |
| `cover` | the share of the damage that gets through **solid cover**: the lines from the blast to a thing (two for a character, its middle and its head; one to the nearest point of a box) end a hand before the thing, and if something else is in the way (a wall, a door, a crate: whatever a bullet would stop at) only `cover` of it arrives. 0 stops it all, 1 ignores cover |
| `structure_mult` | a `BuildPiece` or a `Door` takes this many times as much: a grenade is a weapon against people (0.8) and a satchel charge a tool against walls (2.8) |
| `arm`, `contact`, `remote`, `sticky` | `arm` seconds before touch or the trigger count; `contact` goes off at the first hard impact of a body that moves (a change of velocity of more than 2 m/s in one step); `remote` goes off when the `Thrower` of its `owner` presses the trigger; `sticky` stops a body at its first impact (it stays where it hit) |
| `life` | above 0, it is taken out unexploded after that many seconds (a charge nobody set off) |
| `owner`, `tag` | `owner` is the avatar whose bag it came from; `tag` names the kind in the events: `blast_TAG` |
| `noise` | how far, in metres, the machines hear the blast (default 0: silent). At the blast the engine puts a `Noisy` of that radius on the owner's side for one step, which an AI's `Perception` hears as it hears a shot (`docs/AI.md`): the machines of the other side within `noise` times their `hearing` go to where it went off (`Perception.noise_pos`, awareness at least 0.55). Nothing is replicated; the thrower's own side hears nothing |
| `age`, `stuck`, `last_vel` | the engine's. With a `NetEntity` the seconds left of the fuse are written to `NetEntity.anim_t`, for a light that blinks faster on the clients |

An explosive also goes off when its own `HitPoints` are gone (a barrel, or a charge somebody shot). Explosion damage is **not a weapon hit**: worn armor does not lessen it, and teams do not matter (it hurts everyone, its owner
too). A piece brought to 0 is destroyed by the building system **in the same tick** (`build_destroyed`, and what it held up may collapse, `docs/BUILDING.md`). Events: `blast` and `blast_TAG` (where it went off, about the explosive),
`hit` for each thing that is not a structure and took damage (at most 24 a blast: the clients are told of the people, not of every wall), `kill` for each thing it killed.

**Bounded and deterministic.** Everything is visited in uid order and nothing is random. A blast hurts at most 64 things (the nearest) and casts at most two rays for each; at most 8 explosions go off in one tick
(the rest in the next, the age they have is past their fuse). The world is looked at through the same rays as a bullet.

## The thrower

```
# what a player throws and puts down
throw grenade prefab=Grenade speed=15 lift=0.18 cooldown=1.2
place satchel prefab=Satchel reach=3 cooldown=2 gap=0.05
cap player=4 server=400
trigger range=120
```

`throw KEY` takes the item `KEY` of the bag out of the player's hand along the way it looks (`speed` m/s plus a little of the player's own movement, at most 8 m/s of it, tilted up by `lift`) and makes the prefab there: it
should have a `RigidBody` and a `Collider` (it flies, bounces and rolls) and an `Explosive`; the engine adds the `Velocity` and the owner. `place KEY` puts it on the **surface in front of the player**, within `reach`, `gap`
off it and turned so that its up is the surface's normal. `cap` is the most live explosives one player and the whole server may have (the defaults are 4 and 64), `trigger` how far a trigger reaches. A line with an unknown word, a missing prefab or a
bad number is skipped and named by `check`.

The game sets `Thrower.want` (the number of a throw or place line, 1 for the first) or `Thrower.trigger` (true: the owner's `remote` explosives within range go off) for one tick. The answer is at once. A request that is
refused says why in `Thrower.denied` for a tick and in the events `throw_refused` and `throw_refused_WHY`: `unknown` (no such line), `dead`, `no_item` (not in the bag), `cap`, `cap_server`, `no_surface` (nothing in reach to put it on) and
`no_prefab`. An accepted one is the event `throw` or `charge_placed` (about the player's aim entity, at the thing) and a **cooldown** (`ready`: a request before it is not heard at all, no answer and no event); a refused one
pauses the player a quarter of a second, so that a client that asks in a loop makes a few refusals a second, not thousands. The trigger is not held by the cooldown.

## Code locks

A door or a container (an entity with an `Interactable`) with a `CodeLock` opens only for the players in its `auth`. The server enforces it: the use button (`docs/INTERACT.md`) refuses anyone else (`locked`) and the event
`lock_prompt` is about that player, so that its screen asks for the code.

| Field of `CodeLock` | Meaning |
|---|---|
| `code` | 0 to 9999; -1 while none is chosen (nobody but the keeper can use a lock with no code yet) |
| `auth` | the **names** of up to eight players who entered it (the oldest goes when a ninth comes). A player is known by `NetAvatar.name` (on a secure server the name in its token: `docs/SECURE.md`), so what a player entered survives a server restart (the lock is part of the world's save) and a player who leaves and comes back is still in |
| `tries`, `lockout` | after `tries` wrong codes (3) a player cannot try again at this lock for `lockout` seconds (30). The lockout is per player and per lock: nobody else is held by it. It is kept as tick stamps, so a lock costs nothing per tick |
| `owner` | the name of the player who fitted it, its keeper |

The avatar's `Keypad` carries the requests: `act` 1 **enter** `code` at the lock the player looks at (the right code puts the player in `auth` and opens the thing at once, as if the use button had been pressed), 2 **set** `code` as the
lock's new code (the keeper only; everybody else's authorisation goes, `auth` is only the keeper), 3 **fit** a lock (one `item` leaves the bag and a `CodeLock` with no code is put on the door or container the player
looks at: it must be one that can be destroyed, with `HitPoints`, and be a kind in `Keypad.kinds` (bit 1 doors, bit 2 containers); the fitter is the keeper and is asked to choose a code) or, at a lock the player is the keeper of, ask for the page that changes it
(`lock_setup`). The aim is the use button's (`reach` along the aim entity; a wall in front hides things), so a code can only be tried at a lock the player stands at and looks at; a request is not heard again for 0.4 s.

Events, about the player's aim entity: `lock_ok`, `lock_bad`, `lock_out`, `lock_new`, `lock_placed` (choose the code now), `lock_setup`, `lock_prompt`, and `lock_refused` with `lock_refused_WHY` (`no_target`, `no_lock`, `no_code`, `not_yours`, `bad_code`,
`not_lockable`, `no_item`, `dead`; a code that was not accepted is `lock_bad` or `lock_out` and `Keypad.denied` says `wrong_code` or `locked_out`). Bit 2 of `NetEntity.flags` of a thing with a lock is set (bits 0 and 1 are a door open and a container open), so that the clients can show a padlock. A text field cannot be read by a graph as a number, so
`UiInput.number` is the text as a number when it is one to nine digits (-1 when not): the game sends it as the argument of the command (`docs/UI.md`). A destroyed door takes its lock with it.

## Tiers, and what shrugs off bullets

The rules of the building (`docs/BUILDING.md`) have three keys on a `piece` line for this:

```
piece wall       size=4,3,0.2 hp=300  inset=0.12 bullet=0.05 upgrade=wall_stone upgrade_cost=scrap:20,tape:4
piece wall_stone size=4,3,0.2 hp=900  inset=0.12 bullet=0.02
```

`bullet=` is the share of a bullet's damage that gets through (a piece built from these rules gets a `Resist` with it), `upgrade=PIECE` the piece this one becomes (a tier: the same size and the same sockets, made of something
better) and `upgrade_cost=ITEM:COUNT,...` what it takes out of the bag. The avatar's `Upgrader` asks: `want` for one tick, and the engine looks along the aim (`reach` metres, 5; the avatar's head), at the **nearest piece** that no wall of the
world hides, and if the piece has an `upgrade` and the bag holds all the cost (otherwise **nothing** is taken) the items go and the piece **becomes** the other piece at once: its kind, its hit points (full), its `Resist`, and the
`NetEntity.kind` of a piece that is replicated (the number of the piece in the rules, from 1), so that the clients show the new model. It keeps its place, its owner and its links. Not on land that is another owner's cupboard
(`Upgrader.owner`, or `NetAvatar.slot` + 1, against `BuildUpkeep.owner`). Events: `build_upgraded` (the piece) and `build_upgrade_ok` (about the aim entity) or `build_upgrade_refused` with `build_upgrade_refused_WHY`: `no_aim`
(no piece in reach or in sight), `no_upgrade` (nothing better), `no_materials`, `not_yours`, `dead`; a pause of half a second follows any request.

`Resist.bullet` is asked by the weapons on every hit (`weapons.rs`), before armor: a wall that takes a twentieth of a rifle round takes a magazine to scratch. It is on pieces made from rules with `bullet=`, and a door or anything else
carries it itself. Explosions have their own number, `structure_mult`.

## Over the network

All of it runs on the server only. What a game does:

- **Commands.** `NetPlayer.acts` maps a numbered act to a command string (`60 rd_throw`); a graph system on the server, `each: ["NetAvatar", "Thrower", "Keypad"]`, turns `NetAvatar.cmd` into the request (`Thrower.want`, `Keypad.act` with `Keypad.code` from
  `NetAvatar.args[0]`, `Upgrader.want`). That is all the game's code on the server.
- **Events.** List what the players should be told in `NetHost.forward` (`blast_grenade 170`, `build_destroyed 120`, `died 120`, `lock_prompt 8`, `build_upgrade_ok 8`): the event a client gets is about the entity that the server's `uid` mapped to the avatar's net id,
  so a client compares `event.lock_bad.uid` with its own `NetPlayer.id` to know that it is its own.
- **What the clients show.** A grenade or a charge is a `NetEntity` (class 2 or 3, a kind) with a prefab in `NetPlayer.kinds`; `anim_t` is the seconds left of its fuse; a door's `NetEntity.flags` bit 2 is the padlock; a tier is a different `NetEntity.kind`,
  and a client **makes the puppet again** when the kind changes (`NetDriver`), so the stone model replaces the wood one.
- **A thing the server destroyed stays on a client's screen for seconds** (`NetClient::prune`: it forgets what the server no longer tells of after 5 seconds for a still thing and 2 for one that moves, measured), and the game cannot despawn it (the driver makes
  a puppet the game took away again while the server's table still has it). The events say where it happened (a destroyed entity has net id 0 by then: match by **place**): make the puppet there nothing (scale 0, no collider) and it is gone at once.
  a game's build script (`build_survival_x_raiding.py`) does this (`vb_rd_gone`).

## Checking a scene

`check` names: a `Thrower` with no `throw` or `place` line, an item that is not in the `ItemTable`, a prefab that does not exist, has no `Explosive`, or is thrown with no `RigidBody` and `Collider`; an `Explosive` with no `Transform`, no radius or
damage, nothing that sets it off, or a `cover` outside 0 to 1; a `CodeLock` with no `Interactable`, on a kind that is not a door or a container, or a code outside 0 to 9999; a `Keypad` with no `item` or with an item that does not exist; a rules line
with a bad `bullet`, `upgrade` or `upgrade_cost`, an `upgrade` to a piece that does not exist, a cost with no upgrade, a cost in an item that is not in the `ItemTable` (it could never be paid).

## Cost

Nothing when nobody asks and nothing is live: a system that looks for requests (`Thrower`, `Keypad`, `Upgrader`) visits the avatars that have one and returns. Measured by hand (`blast_costs_what_the_docs_say`, `lock_costs_what_the_docs_say`,
`tier_costs_what_the_docs_say`: `cargo test -p lunaris-sim --lib costs --release -- --ignored --nocapture`; a Linux test machine, release build, other jobs running):

| What | Cost |
|---|---|
| 201 throwers, nothing live: `blast::update` | 0.0007 ms a tick (0.002 at most) |
| the same with one live explosive | 0.0009 ms a tick |
| the same with 801 live explosives that wait (no fuse, a remote trigger): the most a big server would let live | 0.07 ms a tick (0.11 at most): about 90 ns for each |
| 200 keypads, nobody asks: `lock::update` | 0.0004 ms a tick |
| one code tried at a lock (the tick of the request) | 0.08 ms |
| 200 upgraders, nobody asks: `building::update` | 0.0003 ms with one, 0.0007 ms with 201 |
| one upgrade (the whole tick of the request) | 0.02 ms |
| one blast in a crowd of 200 people (64 hurt, the cap): the whole tick | 0.2 ms |

What is paid per player is a few nanoseconds (a graph system on the game's side adds about 25 ns an avatar to read the command, the game's own notes `26-raiding`); what is paid per explosive is about 90 ns a tick while it lives; a blast is bounded (64 things, two rays each, at most 8 blasts a tick).

## What to supply

The pictures and models (the grenade, the charge, the padlock, the stone pieces: the engine makes none), the explosion's effects (`blast_TAG` is the event to spawn them on), the sounds (throw, fuse beeps, explosion near and far,
keypad, wrong code, unlock), the keypad screen (a text field with `UiInput.number`), the items and their recipes, the costs of the tiers, and what a destroyed container does with what is in it.

## Limits (so far)

- A blast is a sphere with cover by two rays: it does not shape itself to a room, a door that is open does not let it through differently from a closed one, and a thin wall is as good cover as a thick one.
- Explosion damage ignores armor and teams on purpose (a game that wants otherwise lowers `damage` itself).
- A destroyed container takes what is in it with it (no loot spills); a game that wants a spill spawns it on `kill` or `build_destroyed`.
- Only the avatar's own explosives can be set off by its trigger (`owner`), and within `trigger range`; a charge somebody else put on your wall can be shot (it has no `HitPoints` unless its prefab gives it any) or broken with the wall.
- A lock knows players by name: two players with the same name on a server that is not `--secure` are one player to it.
- A tier is a different piece of the same shape, not an upgrade of its parts (a wall of two thicknesses): the sockets and the size stay.
- The upgrade heals the piece to full (the new piece's hit points): a game that does not want that keeps the old value itself.
- Bullets do not damage a base meaningfully when `bullet` is small; melee on walls is the game's (lower the `HitPoints`).
