# World events and the supply drop (2026-10-24)

A persistent world needs reasons for players to move and to meet: Rust has the airdrop and the cargo ship, DayZ the helicopter crash. This is a small **event system** for a server's world (`crates/lunaris-sim/src/events.rs`) and one finished event on it, the **supply drop**: a crate on a parachute that comes down over about twenty seconds, makes noise the machines hear, lands, can be searched for ten minutes and is taken away. The system is driven by a **text table**, so another event is a line of text, a prefab and (for the clients) a picture.

## How other games do it (what this is modelled on)

* **Rust**: an airdrop is a plane that crosses the map and drops a locked crate on a parachute; a **smoke grenade** goes off on the crate (a coloured column everybody can see from far away); the sound of the plane and of the crate's landing carries; the crate is opened with a hold of the use key and, for a while, **players fight over it**. The wiki and the community guides stress the two things that make it work: it is *visible from afar* (smoke) and *loud*.
* **DayZ**: helicopter crashes (and the contaminated zones) are fixed places picked from a list at server start (`events.xml`, one `<event>` element for each kind with `<nominal>` how many at once, `<lifetime>` how long they last, `<min>`/`<max>` how many in all, `<restock>`), loot is spawned by its own table, and the event is removed when its lifetime is over. The server's own central economy does the scheduling, not a script.
* **Escape from Tarkov** has no world event of this kind: the nearest are timed extractions and a cleanup of the loot at the end of a raid.

What this takes from them: an events *table* with a lifetime and a cap on how many of a kind are alive at once (DayZ's `lifetime`/`nominal`), an announcement and a visible marker (Rust), noise that draws the machines (the game's own twist: the only PvE is robots), and a loot table of its own (DayZ).

## The table

`WorldEvents.table` is text, one event a line (`#` starts a comment, at most eight lines):

```
event supply_drop kind=1 prefab=SupplyDrop marker=SupplyFlare every=480..900 first=240 alive=1 height=90 life=640 inside=34 clear=3.5 maxy=0.3
```

| Key | Meaning |
|---|---|
| `kind` | the number the clients are told (`ev.kind`); the line's number from 1 if left out |
| `prefab` | the prefab of the scene that is spawned (required) |
| `marker` | a second prefab spawned on the ground at the same place, with a `WorldEvent` whose `owner` is the event: it lives as long as the event does (the smoke, and the noise, of a drop) |
| `every` | seconds from one start to the next: a range `A..B` (drawn from a hash of the tick) or a number |
| `first` | seconds of the scheduler's clock before the first start |
| `alive` | how many of the line may be going at once (default 1: **two events of a kind never overlap**; a start that finds the line full is tried again 10 s later) |
| `height` | how high above the ground the prefab is spawned (the event's own behaviour, such as `AirDrop`, takes it from `WorldEvent.height`) |
| `life` | seconds from the start after which the engine removes the entity and everything hanging on it (0 never) |
| `near` | `near=MIN..MAX` (2026-10-25): instead of the square `inside` round the origin, a place MIN to MAX metres from a player (a random avatar, drawn again at every try of a start): for a world that is loaded only round its players, such as a streamed city whose server has the ground, the buildings and the sky of a far place nowhere (the supply drop of the game's dome city never found a place in the square round the Core). `clear` and `maxy` and the rest of the test below apply as before; with no avatar in, a `near` line falls back to the square |
| `inside`, `clear`, `maxy` | where it may be: the square of half size `inside` metres round the origin, a spot where a ray from the sky finds level ground that is not higher than `maxy` (a roof or the top of a building piece is), that is not a building piece, a door or a container, with open sky over it and nothing solid within `clear` metres round it at three heights (the hut, the walls of the arena, the tower, other pieces). Sixteen spots are tried; none free: the start is tried again in 10 s |

`check` names a line that is not understood, a prefab that is not in the scene, a second `WorldEvents`, and an `AirDrop` with no `WorldEvent`.

## The components

* **`WorldEvents`**: the scheduler, one entity of the scene (the active one with the lowest uid counts). `table`; `players`, how many players (avatars, `NetAvatar`) must be in for the clock to run (0 always; the example scene uses 1: an empty server does not run its events and does not start one the moment somebody joins); `force`, a line's number from 1, starts that line at once (cleared; `alive` still counts); and the engine's `clock`, `next` (when each line starts next, -1 not drawn yet), `fired` (how many of each line started), `n` (all started), `kind`, `x`, `z` (the newest event still going, else the last that started) and `alive` (how many are going). All of it is saved with the world.
* **`WorldEvent`**: on every entity an event made (the scheduler adds it to the prefab it spawns): `n` (the event's number), `kind`, `line`, `age`, `life`, `x`, `z`, `ground` (the height of the ground at the place), `height` and `owner`. When `age` reaches `life` (or the `owner` is gone) the entity and its children are removed; events `world_event_start` (at the place) and `world_event_end`.
* **`AirDrop`**: the supply drop's own behaviour, beside a `WorldEvent` and a `Transform`: it comes down from `height` above the ground to rest `rest` metres over it, over `fall` seconds on a straight line, with a sway of up to `sway` metres to each side that dies away at the landing (so it lands exactly at the announced place). While it falls its `Interactable` is switched off, at the touchdown it is switched on and its `NetEntity` becomes `land_class`/`land_kind` (a falling thing is a **moving** class that the clients see smoothly, a landed one a **still** class; the clients make the puppet again from the prefab of the new kind: the parachute is gone at that moment) and the event `airdrop_landed` is told. **Noise**: `noise` metres from the first moment until `noise_for` seconds after the touchdown, made by the `Noisy` of the markers of the event (they stand on the ground at the place, which is where the machines walk to; a `Noisy` on the crate itself works too, its radius lengthened by its height). **Heat**: with `heat` above 0 every `Heat` within `heat_range` metres of the place gains up to that much a second (the nearer the more) while it is loud; that is the only part that visits the players and only while a drop is loud. **Claim**: a player who has the container open makes it `claimed` for `claim_for` seconds (`claimed_by`, `opened`, and the event `airdrop_claimed` the moment it is first opened). **Clean-up**: a crate that is empty and shut for `empty_life` seconds is removed (0: it stays for its `life`). `NetEntity.flags` says what it is doing for the clients: 8 falling, 4 loud.

## What the clients are told

Nothing runs on a connected client (`NetPlayer.connected`): the events are the server's. The server publishes the resources `ev.n` (events started), `ev.kind`, `ev.x`, `ev.z` (the newest one still going, else the last) and `ev.on` (how many are going); the game lists them in `NetHost.sync` (docs/GAME_SERVER.md, "Resources every client is told") and the client works out the compass direction and the distance from the player's own place. The things themselves are replicated like anything else (`NetEntity`): the falling crate is class 4, kind 140 in the example scene, the landed crate class 3, kind 141, the smoke marker class 3, kind 142.

## A restart

The crate, its marker and the scheduler (its clock, its next times, its counts) are entities and components of the world, so they are in the world save (`NetHost.save`, saved every minute and when the server stops, `stop` in the admin console). A server that is started again from that save **goes on with the event where it was** (a drop that was falling goes on falling from the height it had, lands at the same place with the same loot, and the clock of the scheduler goes on: the next drop is the drawn time, not a new `first`). The simulation does not run while the server is down, so nothing is lost or made up. (A world that is wiped starts from the scene's own scheduler: a fresh clock.) Tested in `events_tests` (the saved and the loaded world have the same state hash and go on the same way) and live (`events_smoke.py` part C).

## Admin

`event` in the admin console (docs/ADMIN.md) lists the events (`supply_drop (kind 1): next in 523 s, 0 going, 2 started`); `event NAME` starts that event now.

## Cost

Nothing is done while no scheduler is in the world and no event is going (a query that finds nothing). With a scheduler the whole step is one scan of the `WorldEvents`, a comparison of the table text with the one last read, and the clock; a start is rare (a place is looked for with at most sixteen ray casts against the static world, and a spawn). While a drop is going: one entity's arithmetic a step, and the heat only when a drop is loud and has `heat` above 0 (one pass over the `Heat`s). No random numbers except `hash_rand`. Counting the players (`players`) is the sum of the lengths of the archetypes that have a `NetAvatar`, not a visit of every player. Measured (2026-10-24, Linux test machine, a survival server scene with 200 swarm bots, the server's own processor time over 40 s): 25.8 % of one core without any event system, 25.9 % with the idle scheduler, 26.1 % with a drop in progress (falling, loud, heating all 200 players' `Heat`): no difference beyond the noise of a shared machine.

## Adding a second event

1. A prefab of the scene (server side) that is the event: whatever the event is made of, with a `NetEntity` if the players must see it. The scheduler adds the `WorldEvent` (`n`, `kind`, `line`, `age`, `life`, `x`, `z`, `ground`, `height`) and places it at `[x, ground + height, z]`. Behaviour comes from graph systems or components that read `WorldEvent` (for example a system `each: ["MyEvent", "WorldEvent"]` that does something when `WorldEvent.age` passes 20), or from `AirDrop` if it is a thing that falls.
2. Optionally a `marker` prefab.
3. One line in the table, with its own `kind=`.
4. In the game: the picture the clients show it with (a prefab named in `NetPlayer.kinds` for its class and kind) and the words of the HUD (a game keeps its own `EV_KINDS` table).

## Tests

`cargo test --release -p lunaris-sim --lib events_tests` (14) and `-p lunaris-api --lib events_check` (1): the table and its mistakes; the first start after `first` and the next within the range, the marker, the resources, the end of life with the marker; two of a line never overlap (and `alive=2`); the clock waits for players, `force`; the place (inside the square, off the hut's walls, roof and a piece, the same every run: state hash equal); the descent in a straight line, the landing at the place, the switch of class and kind, the noise on the marker for the right seconds; machines hear it from 60 m and not from 100 m, for the whole of the noise and not after; the heat within range only while loud; the claim and the clean-up of an empty crate; a restart in the middle of a fall (the saved and the loaded world go on to the same hash); a connected client starts nothing; a missing prefab waits; a start that was asked for and refused leaves the drawn time alone; the admin command.

## Limits

One event is shown by the game's HUD at a time (the newest). The place is picked from a square round the origin (the survival arena); a game with a big map would want a list of places or a ring round a player. The crate collides with a player who stands where it lands (the character is pushed out like by any box).
