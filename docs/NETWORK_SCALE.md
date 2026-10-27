# Big servers: the server netcode for 100 to 200 and more players

[NETWORK.md](NETWORK.md) is for a few players: everybody runs the whole game and the host only passes inputs around. That cannot work for a hundred, because every
computer would run a hundred characters and the server would trust every one of them to run the same code honestly. A game with a big population (a DayZ- or Rust-style survival
server, a battle royale) needs the other design, and this is it (`lunaris-net`, module `scale`; `lunaris server` and `lunaris swarm` to run it). The match in `DemoGame` (loot, a shrinking zone, shots)
is a demo to test the network with: a game brings its own rules by implementing the `Game` trait.

```
lunaris server --players 200                       # a match for up to 200 players on UDP 127.0.0.1:7777 (--bind 0.0.0.0 to open it)
lunaris swarm 127.0.0.1:7777 --count 100 --seconds 30     # a hundred bots that join it, walk about, shoot, and report
```

## How it works

* **A server that owns the match.** The server keeps the other players, the loot, the zone and everyone's health, and it decides the shots. A client runs its own character
  with the game's own movement code (so it feels instant) and sends the state thirty times a second. The server refuses a move the character could not have made
  (`ServerConfig::max_speed`, `max_up`, `max_down`, with a short burst allowance) and tells that client to go back (`Event::Correct`); it never believes a position that
  breaks the budget. Nobody else's computer is trusted with anything but its own character.
* **UDP, with what a game needs of it.** Every datagram carries an acknowledgement of the last packets seen (a 32-bit field) in both directions. A lost position costs nothing
  (the next one replaces it); a message that must arrive (a shot, a hit, a kill, a zone change) is repeated until a packet that carries it is acknowledged and handed over
  once, in order (`proto::ReliableOut`/`ReliableIn`). The handshake is connect, challenge, response: an address that is not the sender's gets a challenge it never sees, so the
  server cannot be made to talk at someone else. A client that goes quiet for `timeout_ms` (5 s) is dropped and its slot is used again; a full server refuses politely.
* **Only what matters, as often as it matters** (`interest`). A grid of cells finds the entities within `interest` (150 m) of a client's character without looking at the rest
  of the match. Each is sent at a rate that falls with distance and depends on what it is: players 30 times a second within 35 m, 10 a second to 80 m, 3 a second beyond (and however
  near, only the 20 nearest are sent 30 times a second and the next 36 at 10, so what one client needs is bounded); things on the ground once a second, on a fixed schedule that needs no
  accounts. Every tick each candidate earns its rate in an account; the ones due go into the packet, the fullest account first (nearer ones count for more), until the
  packet's byte budget (`snapshot_budget`, 900) is used; loot never takes more than a sixth of it. So a crowd of a hundred players in one square makes every client's packet exactly as big as the budget and the nearest
  players are the freshest, and a hundred players spread over a map send each client the few that are near it.
* **Small, whole records.** An entity is sent as one record of about 20 bytes (`state`: position in 1/64 m, yaw, pitch, velocity, flags, health, animation and its time), far
  entities with only the basic fields. A record never depends on an earlier one, so nothing has to be agreed after a loss. The client keeps the last few records of every entity
  and shows it 100 ms in the past, between two of them, extrapolating up to 250 ms when the packets are late (`NetClient::sample`).
* **Shots are decided where the target was.** A shot carries the tick the shooter was looking at; the server keeps the last second of positions (`World::record`) and tests the ray
  against each player where it was then (`World::hitscan`, a cylinder), so a hit on a moving player lands where the shooter saw them, and a shooter cannot ask for a tick older
  than a second.
* **The game is yours.** `Game` is the trait a game implements: `on_join` (the character a new player starts as), `on_command` (what a reliable message from a client does), `on_tick`
  (the zone, the loot, the timer). `DemoGame` in `swarm.rs` is a complete small battle: loot, a shrinking zone that hurts, shots with damage, kills and respawns.
* **Tested without a network.** `SimNet` is a network in memory with latency, jitter that reorders, loss and doubled packets, all from a seed, so the same run happens every
  time. The tests play a hundred bots against a server over `Link::internet()` (40 ms, 20 ms jitter, 3% lost, 0.5% doubled) in a second.

## Numbers (this PC: i5-12600K, the server and 100 or 200 bots all on one machine)

Real UDP over loopback, release build, the server and 200 bots as separate processes at idle priority on a PC that was also running a game (65 to 80 percent CPU in use), 35 seconds each.
The bots walk, shoot at whoever is near (about 30 shots a second in all) and are killed and put back somewhere else, like a real match.

| | 100 bots, 3 km map | 200 bots, 3 km map | 200 bots, all in one 120 m square (1500 items among them) |
|---|---|---|---|
| connected | 100 of 100 | 200 of 200 | 200 of 200 |
| refused, dropped | 0 | 0 | 0 |
| ticks a second | 30.0 | 30.0 | 30.0 |
| server tick, average (of 33 ms) | 1.8 ms | 3.3 ms (10%) | 5.8 ms (18%) |
| server tick, longest in 5 s | 4 to 8 ms | 7 to 8 ms | 8 to 11 ms |
| server to all clients | 56 KB/s | 140 KB/s | 5.5 MB/s (44 Mbit/s) |
| server to one client | 0.6 KB/s | 0.7 KB/s | 27.6 KB/s |
| one client to server | 1.1 KB/s | 1.1 KB/s | 1.0 KB/s |
| longest gap between records of a player next to you | a tick | a tick (33 ms) | 3 ticks (100 ms) within 8 m |

The in-memory tests (`cargo test -p lunaris-net --lib scale`, 15 tests, under two seconds) play the same bots over `Link::internet()` and also check what a real network does to the protocol:
a hundred in one square, two hundred in one square, a heap of 800 loot items, 50 percent loss while connecting, doubled datagrams, a silent client, a full server, a move that could not be made, and a shot at a moving target.

The crowd case is the worst a match can make: everybody within 120 m of everybody, 1500 things on the ground in the same place. It still fits the tick with room to spare; what grows is the bandwidth, which is
bounded by the packet budget (`ServerConfig::snapshot_budget`, 900 bytes) and not by how many are near: 27.6 KB/s to a client, 44 Mbit/s for the server at 200 clients. A server that must send less turns the budget down;
the nearest players are sent first, so a smaller budget means fewer of the far ones, not a slower fight.

How the crowd stays cheap: things that do not move (loot) are cut into 30 grids and a client looks at one of them each tick, so they cost a thirtieth of a scan; all the entities of a tick are copied once into a flat list the grids point into
(no table lookup for each candidate for each client); accounts are updated in place with a hash made for integer keys. Before that the same crowd took 29 ms of the 33 ms tick.

The ones next to you stay fresh because what one client is sent a second is bounded by rank as well as by distance (`Rates::near_n`, `mid_n`): the 20 nearest players at 30 a second, the next 36 at 10, the rest at 3. With every player within 35 m of every other
a client would otherwise need about 6000 records a second and a packet holds about 1400, so each of 199 neighbours would get a seventh of what it needs; with the cap the nearest twenty get all of it.

The windows sleep granularity matters for the round trip: Windows sleeps in steps of 15.6 ms, so both commands ask for 1 ms (`timeBeginPeriod`); the 15 to 45 ms of round trip that `lunaris swarm` prints on loopback is this PC being busy with a game and
the bots running at idle priority, not the protocol (the in-memory tests have none of it).

## In a game: `lunaris play SCENE --connect HOST:PORT`

Any game can go online without netcode of its own: start a server (`lunaris server`, or a server of your own `Game`, see below) and play the scene with `--connect`.

```
lunaris server --players 100 --bind 0.0.0.0 --port 7777
lunaris play scenes/game.lunaris.json --connect 192.0.2.20:7777 --name Ada
```

Three components make it work (the engine's `NetDriver` runs inside the player and does the rest):

- **`NetPlayer`** on the local player's entity (the lowest uid with one): its `Transform` and `Velocity` go to the server 30 times a second (a character with no `Velocity`, such as a `CharacterBody`, has its speed measured from
  how it moved), with `pitch`, `flags` (bits your game gives meaning to: crouched, sliding, sprinting ...), `anim` and `anim_t` (a clip number and its time), `extra` (the weapon in the hands). `puppet` names the prefab made for every
  other player. The engine writes back `connected`, `id`, `ping` (ms) and `players` (how many others are in sight). The **server chooses where you start**: on connecting your entity is put there; a move the server refuses
  (faster than a character can go: 16 m/s on the ground by default) puts you back, and `corrections` counts them.
- **`NetPuppet`** on every other player: made from the `puppet` prefab (or a bare entity with a `Transform` and a `NetPuppet` if there is none), moved smoothly (a tenth of a second behind, between two of the server's records,
  facing the way the player faces), its `Velocity` set if it has one, and its `id`, `flags`, `anim`, `anim_t`, `pitch`, `health`, `extra` and ground `speed` filled in. It is removed when the player leaves or goes out of range.
  Your own systems dress it: pick the clip from `speed` and `flags`, show a name, turn the head by `pitch`. A child of the prefab that has a `NetPuppet` of its own is a part of the puppet and is given the same numbers every tick
  (a gun in the hand shown from `extra`; docs/GAME_SERVER.md, "Parts of a puppet").
- **Resources the server decides** (`NetHost.sync`, docs/GAME_SERVER.md, "Resources every client is told"): the world clock, the weather, anything the clients only show. The server copies the named resources of its own world to every client as one small reliable
  lazy message (`0x48`: the changed ones at most once per `sync_every` seconds, all of them every 5 seconds and all of them at once to a client that joins) and the `NetDriver` writes them into the client's simulation as resources of the same names.
- **Health is the server's**: a player's `HitPoints` are not sent and a claim of health would not be believed; `NetPuppet.health` is what the server says.

`NetDriver` is also a library type (`lunaris_net::scale::play`) for programs that embed the simulation: call `tick` before each simulation tick (or `tick_at` with your own clock), and read `messages` for what the server's game sent.
The tests run two simulations against a server in memory over a bad network: puppets appear, follow and carry the flags, a cheat is put back, a player who leaves loses their puppet.

What the server decides is its `Game`: `lunaris server` runs `DemoGame` (loot, a shrinking zone, shots with damage) to try the network; a game of your own implements the `Game` trait (`on_join`, `on_command`, `on_tick`) in Rust
and starts a `NetServer` itself. A server made from a scene's own rules (weapons, building, loot, the machines, respawn, saving) is `lunaris server --scene SCENE`: **`docs/GAME_SERVER.md`**. (A thing that
stays where it is, class `ITEM` or `WORLD`, is sent in its basic record plus its `extra`, one or two bytes, so that the client knows what it is.)

## Where the time goes, and what was done about it (`lunaris server --profile`)

`--profile` prints, every five seconds, the microseconds a tick spends in each step: `recv` (reading and acting on what arrived), `game` (the game's tick and its events), `grids` (copying the entities into the flat list and the grids),
then per client `gather` (finding its candidates), `rank` (ranks, accounts, the order of what is due), `write` (the packet) and `send` (handing it to the network). Measured on 400 bots over real UDP on loopback,
normal priority, release build (Windows loopback makes a send cost about 5 to 10 microseconds, because the receiving side's work is done in the sender's call; a real network card is cheaper):

| 400 bots | `recv` | `gather` | `rank` | `write` | `send` | tick |
|---|---|---|---|---|---|---|
| spread over 3 km, before | 0.55 ms | 0.43 ms | 0.07 ms | 0.17 ms | 4.0 ms | 4.8 ms |
| spread over 3 km, after | 0.57 ms | 0.32 ms | 0.02 ms | 0.10 ms | 1.95 ms | 2.5 ms |
| all in one 120 m square, before | 0.77 ms | 0.97 ms | 4.0 ms | 0.57 ms | 5.2 ms | 11.2 ms |
| all in one 120 m square, after | 0.74 ms | 0.93 ms | 2.1 ms | 0.60 ms | 5.1 ms | 9.0 ms |

With 200 bots spread over the map the tick went from 1.9 to 1.2 ms. The two changes:

- **A client with nobody near it gets a packet once in three ticks** (`ServerConfig::idle_every`, 3; ten a second): when nothing near it is sent faster than the mid rate (10 a second), a packet every tick only carries headers and records
  that are not due. Its accounts earn three ticks at a time and the loot offered on the three ticks since its last packet is offered in the one, so nothing is lost; a player that walks up to 35 m is noticed within three ticks (the packet rate is
  back to every tick as soon as one is near). Reliable messages that are urgent (a hit on you, a correction) still make a packet at once; broadcasts to everybody (a kill feed line, the zone) are lazy: they go with the client's next packet.
  200 players over a wide map: 30 packets a second to each client became 11.
- **The far tier is looked at once in a period** (the players beyond the first 56 or 80 m, sent 3 times a second): their account is only touched on their own tick of ten, so most of a crowd costs an addition and a comparison, not a table lookup.

What is left is mostly the network call itself (`send`) and, in a crowd, the 5.5 MB/s that the packet budget allows (27.6 KB/s to each of 200 clients, 44 Mbit/s): both are bounded by the machine's network, not by this code.
The next steps if more is needed: making the packets on several threads (the `gather`, `rank` and `write` of a client touch nothing shared), a batched send on Linux (`sendmmsg`), and smaller records (a position as a small offset from the packet's own origin, or a change since the last acknowledged record).

## Wallhacks: a client is told only about the players it could see (`NetHost.occlusion`)

Interest alone sends a client the place of everybody within 150 m, through any wall: a modified client only has to draw what it is told (an ESP overlay), and nothing but the protocol is needed to do it. The defence is
**occlusion**: the server leaves a player out of a client's packets while the level is in the way, and the client forgets it (after two seconds, like anybody who goes out of range). It is off by default (a game that
does not turn it on is byte for byte what it was: a test plays a fixed match and checks the packets, records and bytes), and it is one number: `NetHost.occlusion` in a scene that is hosted (docs/GAME_SERVER.md), the **safe radius** in
metres, 16 to 25 being good; a game with a `Game` of its own implements `Game::occlusion` (the radius), `Game::blocked` (is the level in the way of a straight line), and may add `Game::level_version`, `Game::low_flags` and `Game::occludes`.

A player is **told** about another player (every tick it would be, at its usual rate) if

* it is **within the safe radius**: close players are always told (round a corner, in the room above, on the stairs), so nothing near you is ever late and nothing there has to be exact;
* **a clear line** runs from the eye of the client's character (1.6 m) to any of the player's **head, chest, feet and left and right shoulders** (1.7, 1.1, 0.25 and 1.4 m, the shoulders 0.3 m to either side of the line of sight; a player whose flags
  say it is crouched, `Game::low_flags`, is 0.6 of that): a head over a wall or a shoulder past a corner counts. The lines are looked at where the two players are **now and 0.45 s ahead** at the speeds they are moving (the speed
  a client reports, never more than `max_speed`, so a lie buys no more than a few metres): a client shows others 100 ms in the past, hears about its own move after the trip of its packet, and the pair is looked at again only every few ticks, so
  a player must be sent before the line opens or it appears late;
* it was **in sight within 0.6 s**: ducking behind a wall for a moment, or a flickering line past a lamp post, loses nothing.
* it is **on the client's side** (`Game::allies`, docs/PARTIES.md): the members of a party are told to each other within the interest radius whatever is in the way (no ray is cast for them; a client with no party pays one `is_empty` for the packet).

The static world is what blocks a line: for a hosted scene the `Collider`s that do not move (boxes, balls and capsules), `ConvexCollider` hulls, `MeshCollider` levels and `Terrain`, the same rays that wheels and the sight of an AI use
(`lunaris_sim::sight::Sight`). A player, a vehicle or a pushed crate blocks nothing; a `CollisionLayer` that does not collide with layer 0 (a window's glass, a hedge to see through) is transparent. The world is gathered when the server starts and again
about every half second (a built wall, a shot-down wall and a swinging door are in it by then; a change is noticed by the signature of what was gathered and makes the server look again at the pairs whose lines of sight go through what changed: `Game::level_change` gives the boxes round it, and a pair keeps its answer when the tube its lines are in meets none of them; where the change is not a few boxes, or the game does not say, at every pair); a big level is gathered less often so that
it never costs more than a hundredth of the time. Loot, things of the world and, unless `NetHost.occlude_machines` asks, machines are always told.

**The cost** is rays, and at 200 players and 30 ticks a second they must not be cast for every pair on every tick, so: the answer for a pair is kept and asked again only after 3 to 6 ticks (the farther the pair, the longer: it cannot be sent
faster than its rate anyway; `Occlusion::retest_ms`), or sooner when either of the two has moved a metre or changed its velocity by 2 m/s since (a player who has stood still and starts to run is not forgotten for the length of a period); a pair of which
neither has moved a third of a metre keeps the answer for 0.6 s with no ray; pairs inside the safe radius cost a table lookup; in sight, the first ray of a pair is nearly always the last (the chest first, then the head, the feet and the shoulders);
and a ray is cheap because the rays of a pair almost all go through the same wall, so the shapes that blocked the last few rays are tried first (`RayWorld::occluded`, which also stops at the first hit: 0.1 microseconds a ray in the town below,
against 0.45 for the nearest-hit `cast`). Each client has **`Occlusion::rays_per_client`** (150) rays to spend a tick, up to four ticks' worth saved up: past that a pair that was found out keeps its old answer, one never tested that is within 80 m is told (a short leak,
never a near player who is not shown) and one farther away waits (a short delay; the near pairs are tested first), and the whole server pays at most 150 rays a tick for each client (200 clients: about 3 ms). It is a client's own rays: a crowd of
clients made to use up the rays of a server cannot take away the rays another client needs. `lunaris server --profile` prints the pairs, the rays, the answers kept without a ray, the players left out and the tests that had no rays, a tick, and the time
the decision takes (`occlude`, microseconds a tick); the static world is gathered about twice a second (0.5 ms for 4500 shapes: `SimStats::sight_us`).

**Measured** on 2026-10-22 on a Linux test machine (8 cores with other jobs running, load average 5 to 7), release build, real UDP on loopback, `python3 tools/occlusion_bench.py`: a server on a generated town (`tools/build_occlusion_town.py`:
closed buildings of four walls and a roof, 4501 static shapes for 900 buildings on 1.5 km, 200 spawn points) and 200 swarm bots walking about at 0 to 8 m/s (through the walls: they do not collide), 30 to 40 s each, the first run with the defence off and
the next with `NetHost.occlusion` 16 (24 is the same within the noise):

| 200 bots | tick off | tick with 16 m | the `occlude` step | pairs tested, rays a tick | players each bot knows |
|---|---|---|---|---|---|
| spread over the 1.5 km town (6 others within 150 m of each) | 0.82 ms | 1.03 ms (**+0.21 ms**) | 0.22 ms | 225, 2240 | 7.7 down to 3.1 |
| a town 600 m across (65 others within 150 m of each) | 1.57 ms | 3.10 ms (+1.5 ms) | 1.67 ms | 1980, 15600; 2260 tests a tick had no rays | 80 down to 41 |
| all 200 in one 120 m square of the town | 2.23 ms | 4.72 ms (+2.5 ms) | 2.62 ms | 4200, 19300; 5380 had no rays | 199 down to 152 |

The rows are three kinds of match: the first is what a big map with 200 players looks like (a few players near each one), the others are the worst a match can make (everybody in range of everybody, behind walls) and show the bound: the server pays
the same 150 rays a client whatever the crowd, so the tick never goes past about +3 ms, and where the rays run out the defence gets weaker (a near pair that could not be tested is told, a far one waits) instead of the tick getting longer. A first version that cast the nearest-hit `cast` for every ray, with no memory of the last blockers, took 0.85 ms for 1900 rays in the first row; the memory of the blockers made it four times quicker.

**What a wallhack client still learns** (the same town, 600 m across, 200 bots walking through its buildings for 22 s in memory over a network with 25 ms of latency; `cargo test --release -p lunaris-net --lib leak -- --ignored --nocapture`; each of the 65.8 other players within 150 m
of a bot 45.3 are behind a wall from it, and the figures are what the bot's client holds a fresh record of, sampled every half second):

| | holds | in sight | behind a wall, within the safe radius | behind a wall, beyond it |
|---|---|---|---|---|
| defence off | 64.2 | 20.0 | 0.0 | 44.1 (97 percent of those behind walls) |
| safe radius 16 m | 24.6 | 19.5 | 0.7 | 4.4 (9.8 percent) |
| safe radius 24 m | 26.1 | 19.6 | 2.2 | 4.3 (9.9 percent) |

So a cheater is shown nine tenths less of what is behind the walls beyond the safe radius, and what is left is the three things on purpose: players within the safe radius (through walls and floors: 0.7 to 2.2 of them), players who were in sight within the last 0.6 s (where they were,
and where they went: a few metres), and players who are about to be in sight within 0.45 s (they are told that far ahead so that nobody pops into view late; the cheater learns that somebody is about to come round the corner he is walking round, from where he is
walking, nothing more). In this town the bots cross walls as they like, so their lines of sight flip far more often than a real player's; and in the dense rows the rays of the 150 a client ran out for some pairs, which are told while they are within 80 m: both make the 10 percent bigger than it would be in a match with a spread of players.
Of the players in sight, 97.5 percent are known (19.5 of 20.0): the rest are the ones just round a corner that the client was about to be told about.

**What the defence does not hide.** It is about players (and, if the host asks, machines). Loot, containers, things of the world and what the game says with events (`NetHost.forward`: a shot is told to every client within its radius, with the
place it happened at) are still sent whatever is in the way; nothing here is about sound. A player within the safe radius is told through anything. The server sees the world through its own colliders: what the client draws and the server has
no collider for (foliage, props) does not block the server's rays, so the server is only as good at hiding as its scene is complete (put the walls, floors and the terrain on the server). A client that stops hearing about a player keeps the last place it was told for up to two seconds
(it forgets what it has not heard of for that long, as it does for a player that goes out of range): a player who ducks behind a wall beyond the safe radius is shown standing at the last place for a while and then removed; a message that takes a player away at once is a change of the protocol, not made here.
Crouching is the one thing about a player's body the server knows (`Game::low_flags`); a player that lies down or leans is tested as standing or crouched, so a player behind low cover that only a prone body hides is told.
A client that reports a false velocity gets a look-ahead of at most 0.45 s of `max_speed` (7 m): it could have been there. The no-late-pop-in figures (a record 180 to 465 ms before the line opens, tested at 8 m/s with the walker coming from behind the wall, the watcher
coming round it, and a runner who stood still first) are for players within 80 m: a player who comes into view beyond 80 m is a new one for the far tier, which is sent 3 times a second and takes up to a third of a second to send its first record, wall or no wall.

### What a crowd costs, where it goes, and what the long ticks are (the survival scene, 2026-10-22)

The real scene of the example game (`game/scenes/survival.lunaris.json`: 1386 entities of which the server keeps about 150 after `NetHost.skip`, 26 static boxes, `NetHost.occlusion` 16) with 200 swarm bots that spread out from the middle of the map, 40 s each, real UDP on loopback on
the shared Linux test machine, release build (`tools/srvperf_load.sh`; `tools/srvperf_ab.sh` runs two binaries in turns, three runs each, so that the noise of a shared machine falls on both alike; `tools/srvperf_report.py` makes the summary). The machine is noisy in a way that matters more than any
change here: the same binary averaged 7.4 ms a tick in one run and 13.5 ms in the next (other jobs on the machine), so the figures are medians of runs taken in turns, in a quiet period:

| 200 bots, the survival scene | before | after |
|---|---|---|
| tick, average of the 5 s windows (median of 3 runs) | 8.65 ms | 7.83 ms (-9.5 percent) |
| tick, 99th percentile (the old code with only the profile added, 2 runs; the new, 5 runs) | 11.6 ms | 9.9 ms |
| tick, longest of a window (median of the 8 windows) | 12.5 ms | 10.8 ms |
| `game` (the game's tick: input, two steps of the simulation, replication) | 3.35 ms | 2.45 ms (-27 percent) |
| `occlude` | 3.16 ms | 3.25 ms (the same within the noise) |
| `send`, `gather`, `rank`, `write`, `recv` | 0.88, 0.40, 0.49, 0.28, 0.28 ms | the same |

Without `--governor 0` the new binary cuts the rays when the ticks average more than 24 ms, as it should; the figures above are without, so that they are about the code. (In a busy period, with the other jobs of the machine in the way, the same two binaries gave 13.3 and 13.0 ms, `game` 5.4 and 4.4: the gain is the same, the tick
is mostly the machine.)

**Where the occlusion goes** (`cargo test --release -p lunaris-net --lib occlusion_bench -- --ignored --nocapture` with `SURVIVAL_SCENE=path/to/survival.lunaris.json`: the same filter the server runs, for 200 bots that walk the way the swarm's do, in memory, in 6 seconds, with the answers of a whole run hashed so that a change
that is meant not to change what is told to whom can be seen not to): 4000 pairs are tested and 27 000 rays cast a tick (the 150 rays a client may spend are all spent every tick, and 11 000 pairs a tick go untested for want of them: this is the crowd the budget is for), 93 percent of the rays are blocked (the pairs that
the walls hide are the ones that cost, and they are most), and 33 000 pairs a tick are looked at (a table look and some comparisons of whole numbers: 22 ns each, as much as the rays cost). What was done, each with the hash of the answers unchanged: **a ray is cheaper** (`RayWorld::occluded` tries a solid with
multiplications by the ray's reciprocal direction in place of six divisions and, for a box that is not turned, a test that needs no turning of the ray into the box's frame: 54 ns a ray down to 45; `the_quick_tests_of_occluded_give_the_answer_of_the_slow_ones_for_every_ray_that_is_not_within_a_hair_of_an_edge` gives the same answer as the old one for every
ray that does not pass within 4 mm of an edge, where the last bit of a division used to decide) and **a pair's answer is kept in the integers it is told in**, one look at the table deciding a pair (the entry is 56 bytes, not 68; `the_decision_about_a_pair_is_what_it_was_before_the_table_was_looked_at_once_and_kept_in_integers` runs the old decision beside the
new for two million pairs of a busy match and requires the same answer, the same rays and the same counts every time). In memory, the whole filter for 200 bots took 2.83 ms a tick and takes 2.64 (-6.5 percent: in the live runs that is inside the noise). Tried, and not in because it did not pay: a table of the observer's own for the answers in place of the
`HashMap` (a look is 22 ns either way: it is the comparisons that cost, not the table), and a "one wall stops all the lines of a pair" shortcut that was sound (a test of 60 000 pairs, some of them grazing an edge, required it) and cost as much as the rays it saved. What is left is the visits (every pair of every tick) and the set-up of
a pair's test; a retest interval by distance and relative speed would cut both, but the intervals are what the 0.45 s look-ahead is made of (a client shows others 100 ms in the past, the server hears of a move after the trip of the packet, finds out when the pair is next tested and sends the answer back along the same road): longer
ones would be late for a runner, and the guarantees of the tests are not changed here.

**What the long ticks are.** `--profile` prints the slowest tick of every 5 s window with its phases, the processor time of the server's thread, what the defence did in it, and two **canaries** (a fixed piece of arithmetic and a walk over a megabyte, timed at the start of every tick). Three kinds were found:

- *The machine.* In the busy periods the slowest ticks were 32 to 40 ms, with the time by the clock equal to the time on the processor (the thread was running, not waiting), every phase 2 to 4 times longer than in an ordinary tick at once (`game` 13 ms for 4, `send` 3.7 for 0.9, `rank` 1.8 for 0.5: a long job in one phase would stand out), and the canaries as slow (the core canary
  at 187 microseconds for a median of 57 in 24 of 26 windows: the core ran at about a third of its speed, the memory canary at 2.7 times). Nothing the server does: the shared machine took the core's speed away. It is why the figures above are of a quiet period, and it multiplies whatever the server's own cost is, so a shorter tick is a shorter spike.
- *A change of the level.* Every gathering of the static world (twice a second) found it changed in this scene (bots drop and pick up things that have colliders: 27 shapes grew to 70 in 40 s), and a changed world asked every answer the defence had kept again: in the tick after, 5 to 6 times the usual pairs (23 000 against 4 000) and 40 000 to 70 000 rays, `occlude` 6.5 to 7.7 ms for 3.0, the slowest tick of a quiet window. Now the game says where
  the level changed (`Game::level_change`: the boxes round what was added, taken away or moved, from `RayWorld::changed_since`, which compares the solids of the two gatherings), and a pair keeps its answer unless the tube its lines of sight are in (the line between the two, the width of the shoulders and the distance either is going to move, up and down the height of the head) meets one of the boxes
  (`lines_meet`; a test of 20 000 pairs requires that no line `sees` casts goes through a box the pair said it misses). In a tick with a change the pairs tested are 4 300 to 4 700, as in any other. (A game can also make this rarer: things on the ground that should not hide anybody can be triggers, or have a `CollisionLayer` that does not collide with layer 0: they are then not in the world the rays see.)
- *One-off work.* The first tick with 200 players in it tests every pair (22 000 pairs, 37 000 rays, 12 ms); a path search of a `NavAgent` took 6 to 11 ms (`navigation`) in single ticks of the simulation; the periodic save held the tick up for 3.7 ms (a copy of the simulation, of which the world is 0.3 ms and the scripts' 2.4: the 200 systems' definitions, deep trees, were copied each time; they are shared now, `ScriptSystem::def` being an `Arc`, and the hold-up is 1.2 ms; the writing is on a thread of its own, and the regathering of
  the static world for the rays is 0.01 ms). None of them is more than a third of a tick.

**The governor.** For a server that really is overloaded: `Occlusion::governor_ms` (`lunaris server --governor MS`, 24 there by default, 0 never) cuts the rays every client may spend, in proportion to how far over the average tick of the last half second is (never below a quarter, never more than a tenth a tick), and gives them back three in a hundred a tick once the ticks are quick again. A crowd that is too big for the tick is then paid
for with a short leak of near players and a short delay of far ones (what a client with no rays left gets already), not with a late tick for everybody. `Stats::occ_throttled`, `NetServer::occlusion_scale()`.

**How to look for the next one.** `lunaris server --scene S --profile` (the slowest tick, the percentiles by the clock and by the processor, the canaries, what the defence did in the slowest tick, the saves), `--profile-sim` (the stages of the simulation and each graph system, ms a step, printed when the server stops; the blocks of `gameplay` are listed one by one),
`cargo test --release -p lunaris-net --lib game_phase_bench -- --ignored --nocapture` (the game's part of a tick with 200 players, in memory; `BENCH_SIMPROFILE=1` for the stages; the state hash of the simulation afterwards; what a save holds the tick up for). `--spin` keeps the server's core busy between ticks, which a processor that clocks down while the server sleeps may like (in the quiet periods it made no difference: 7.4 ms with and without).

### The tail of the step times in a crowd, and what bounds it (2026-10-26)

On a city server with 210 bots the average tick is 19 to 20 ms of the 33 but 3 to 4 percent of the ticks are late, and the lateness is not in the average. What was done to find it, in the order that paid:

* **A sampling profiler for the whole process.** `LUNARIS_SAMPLE=file lunaris server ...` (Linux) writes the call stack of the running thread every millisecond of processor time (`ITIMER_PROF` and glibc's `backtrace`, no `perf`, which a locked-down kernel does not allow) and `tools/sample_report.py EXE file` turns it into functions with `addr2line` (`--stack A,B` the callers of a function, `--below A,B` what its time goes to, `--lines A,B` the lines). It said the profile of a step is flat: no function above 7 percent (the drones' patrol texts parsed every step, 6 percent, are the one that was fixed: they are read once now).
* **The slow steps by name.** `--profile-sim` keeps the twelve slowest steps over `LUNARIS_SLOW_STEP_MS` (30 by default) with their biggest stages and counts the steps over 10, 15, 20, 30, 50 and 100 ms (`steps over ...`), which an average and a maximum hide. In a hundred seconds of 210 bots all twelve slowest steps (27 to 118 ms) were **path searches** (a target that cannot be reached is looked for in every direction up to `NavLimits.max_searched`), and after them the steps in which the world streamed (`gameplay procgrid`, the characters' grid, the physics rebuild). A 300-bot run on 2026-10-26 said the same again (twelve steps of 31 to 58 ms, every one of them 24 to 53 ms of `navigation: searches`, the average step 5.6 ms): **`nav.cells_per_step` is the cure** (docs/NAVIGATION.md), and a server can switch it on without a change to the scene: `lunaris server --scene S --resources nav.cells_per_step=2000` (any resource, a list of `NAME=VALUE` separated by commas, set once the scene is loaded; also `stream.loads_per_tick=3`, `procgrid.every=4`). Measured on the same 300 bots, 100 s, three runs in turn (with, without, with): the slowest step **22.8 and 15.4 ms with it, 58.6 ms without**; steps over 30 ms **0 and 0 against 10**, over 20 ms 2 and 0 against 17; the average step 6.0 and 5.8 ms against 5.7 (the searches take more steps, the same cells in all). Ticks over 33 ms were 36 and 4 with it and 24 without: the box's noise is bigger than that difference, so the claim is about the steps, and the count of late ticks needs many more runs. Test `the_resources_flag_sets_each_named_resource_and_refuses_a_bad_list`. **The AI can look less often**: `gameplay.every.perception=2` and `gameplay.every.flyagent=2` (docs/AI.md) made the 300-bot step 4.6 and 4.9 ms instead of 5.6 and 5.8 (the tick 16.5 and 17.2 ms instead of 19.2 and 18.9): `--resources nav.cells_per_step=2000,gameplay.every.perception=2,gameplay.every.flyagent=2` is the server's cheap setting for a crowd.
* **Navigation: `nav.cells_per_step`** (docs/NAVIGATION.md) spreads a search over steps: the slowest navigation step went from 99 ms to 11.5 ms at 1,500 cells a step and the steps over 50 ms from 6 to 0, for the same total work. It does not lower the count of ticks over 33 ms (a long search becomes a few slightly slow ticks): it removes the stalls of 50 to 100 ms a player feels as a freeze. Lowering `max_searched` bounds the work itself.
* **Procedural grids.** The stage that decides which cells of a `ProcGrid` to make (the cells near each of 210 anchors, a hash map update each) had spikes of 10 to 12 ms; a flat array gives the same cells (tested) at a third of the cost: the stage's average 0.60 to 0.28 ms a step and its slowest 12-15 to 7-10 ms. `--profile-sim` now lists the parts of the stage (`present`, `drop`, `want`, `zones`, `spawn`) and the counts it worked with.
* **Rays against the mesh levels.** The flying agents were 1.3 ms of a 6 to 7 ms step for thirty machines, and timers inside the stage (`flyagent: rays`, and `ray casts: ...` for every system that casts: 212 casts a step on average, 433 at most) said that nearly all of it was the casts, and that three quarters of a cast, 1.07 of 1.42 ms a step, was the loop over the mesh levels: every ray asked every loaded building shell (a few hundred in the city), the boxes' grid of the solids not covering them. `RayWorld` now has a grid over the levels' boxes (kept on the thread by what it is made from, as the solids' is), a ray asks the levels in the cells it passes through, in the order of the list, so the answer is the same bit for bit (a test of 4,000 rays over 160 shells compares it with asking all, for `cast` and `occluded`). Same city, 210 bots: the levels part 1.07 to 0.17 ms a step, all casts 1.42 to 0.47, the flying agents 1.31 to 0.68, the step 5.9 to 5.3 ms, steps over 10 ms 137 to 56 and ticks over 33 ms 17 to 22 of 2,850 (the box was quiet in both; navigation stalls, which this run did not slice, are the steps over 50 ms that are left). `--profile-sim` lists the casts by part (`ray casts: solids / flying solids / terrain / mesh levels / hulls`, all casts, how many) and the flying agents by part (`flyagent: agents / world / decide / rays / agent loop`).
* **What a gathering of the static world costs, by part** (`--profile-sim` lists `ray casts: gathers ...`): 4.4 gatherings a step took 0.76 ms of a 4.9 ms step: the mesh levels 0.20 (the levels' boxes are worked out again at each gathering), the kept static solids 0.35 of which 0.21 is making them again: the static colliders of the city server change in **every other step** (0.5 rebuilds a step, 0.41 ms each for 3,900 solids: things dropped and picked up, doors, zones that load), the key of the solids (a hash of every static collider, to know it) 0.12, layers and sets 0.04. The solids that an `except` list leaves out (perception excepts its targets, and a door that can be shot has hit points, so it is one of the static solids) are kept as the static solids are, by the same key and a hash of the excepted: making them each time was a copy of the solids and a grid. What is left is the churn of the static world: an incremental update of the kept solids (a merge of what was added, taken away or moved) and of the levels' list is the next step, and a change count in the ECS (a version for each column, so that the key is not a hash of every collider) the one after.
* **The physics stage by part** (`physics: bodies / vehicles / probes`): 0.91 ms a step, of which the bodies' step 0.63 (about ten rigid bodies, but the reference step makes a `Body` of each of the 3,900 static colliders and sorts them every step), the nine raycast vehicles 0.20, the probes 0.08. Keeping the statics' bodies between steps by the static key (as `RayWorld` keeps its solids) is the way to the 0.4 ms of that which is not the ten bodies; the sleep and scenery logic reads every static, so it is a change to make with the golden hashes of the physics scenes beside it.
* **How many bots until the tick passes 33 ms** (the city server with the bots of `lunaris swarm` on the same 8-core machine, 100 s each, 2026-10-26, after the changes above): **400 bots** tick average 21.5 ms, p99 33 ms, 1.5% of ticks over 33 ms; **600 bots** 24.0 ms, p99 39 ms, 2.0% over; **800 bots** 28.4 ms average, 10% of ticks over 33 ms and only 455 of the 800 bots still connected at the end (the box, the server and the swarm together, is out of room: the swarm's clients are as heavy as the server). The simulation's step barely grows (5.9, 7.2, 7.3 ms a step: it is the world's cost, not the players'); what grows is the network's part of a tick: gather, rank, write and send added up 3.3 ms at 400 players, 5.6 at 600 and 9.2 at 800 (the packets are made on several threads; `occlude`, the wallhack defence, runs beside the tick). So a single machine that is only the server has room for well over 600 players of this city; the next costs to cut at that size are the network phases (about 11 microseconds a player a tick) and the world's own 12 to 16 ms a tick.
* **What the server's cost depends on.** With no player connected the same server still takes 14 ms a tick (the world's own machines and streamed zones): the players add about 30 percent at 210 and almost nothing for the next ones. 300 bots: tick average 22.0 ms, p99 43 ms, 2 percent of ticks over 33 ms; 400 bots: 21.8 ms, p99 51 ms, 3.3 percent over (Linux test machine, server and bots on the same machine, one run each; runs of the same build differ by a factor of two in the count of late ticks, so compare builds by running them alternately several times). The next gains are in the world's base cost (the drones' and perception's rays, physics, the characters), not in the players.

### The packets are made on several threads (2026-10-23)

At 400 players all within 120 m of each other (the dense worst case of the survival scene on a Linux test machine, `server_load.sh`, real UDP) the single-threaded tick was **19 ms of the 33**: the clients' packets were 13 of it (the wallhack filter alone 9.7: 160,000 pairs a tick, each a table look-up). Each client's packet depends only on that client's state and on the entities as they are this tick, so they are now made on several threads at once (`ServerConfig::threads`, `lunaris server --packet-threads N`; 0, the default, uses the machine's processors up to 8; one thread under 24 clients). How it is built: the entities, the grids, the settings and the wallhack parameters of the tick are one read-only `Frame`; each thread takes four clients at a time off a queue and fills its own scratch lists; the packets are handed to the network afterwards on the tick's thread, in the order of the slots, so what goes out is the same as before (a test runs the same town and bots with one thread and with four and compares what was sent: the same number of packets, bytes and rays to within half a percent, which is as close as two runs with one thread each, because the server lists its entities in the order of a hash table made anew every time). Two things had to be made shareable: the rays of the line-of-sight world (`RayWorld` kept a scratch list and a "last blockers" cache in `RefCell`s; they are per thread now), and the game's level of sight (`Game::blocker()` gives a `Blocker`, a `Sync` view of the same question as `Game::blocked`; a game that has none is made packets for on one thread, as before). With the wallhack defence on, the phase times printed by `--profile` for `gather`, `rank`, `write` and `occlude` are now the sum of the work of all the threads (processor time), not the time the tick took.

Measured: in memory with 400 dense bots and no swarm in the way (`cargo test --release -p lunaris-net --lib threads_bench -- --ignored --nocapture`, i7-9700K, 8 cores): the server's tick **11.8 ms with 1 thread, 6.9 with 2, 3.9 with 4, 2.6 with 8** (the work added up stays about 11 ms: it scales). On a Linux test machine with the real survival scene and 400 swarm bots on the same eight cores (the bots take several of them, and the simulation step slows when they all want the processors): the tick 19.0 ms with 1 thread, 20.9 with 2 (noise: the bots), 17.6 with 4, 15.6 with 6, **13.3 with 8**, the slowest ticks 44 ms down to 37 ms. A real server has its cores to itself, so the in-memory figures are the better guide; the simulation (`game`, 4 ms at 400 players) is still on one thread and is the next biggest part.

## What it does not do yet

* The game itself: this crate carries the state of characters, loot and a zone; a game's own logic (weapons, inventory, the drop) is a `Game` to write on top.
* Occlusion in shots: `hitscan` takes a `blocked` distance, so the game can hand it the nearest wall from the sim's raycast; the demo does not.
* Encryption and authentication are a mode of the server: `--secure KEYFILE` (connect tokens, sealed packets: docs/SECURE.md). Without it the handshake only proves an address.
* More than a thousand players on one server: the packets are on several threads (above) and the simulation is not; the next steps are the simulation's own parallel systems for the avatars, then regions.
