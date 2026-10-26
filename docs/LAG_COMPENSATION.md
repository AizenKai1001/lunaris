# Lag compensation: a shot hits what the shooter saw

On a game server (`lunaris server --scene`, docs/GAME_SERVER.md) a player's screen is behind the server: what the player sees of the others is the server's picture of a moment ago. Without help a shot is decided at
the present, so a player who puts the cross on a running player and clicks, and sees it hit, misses: by the time the click has reached the server the target has run on. Lag compensation fixes that on the server, with
nothing to write in the game: a **hitscan** shot is tested against the targets where the *shooter* saw them when it pulled the trigger.

```
 the shooter's screen                   the server
 draws the others 100 ms behind   ─►    keeps the last 250 ms of where every character and moving thing was
 (interpolation), the snapshot          (a frame a tick); the shot arrives half a round trip late
 itself was half a round trip old       looks back  half RTT + 100 ms + a tick  and tests the shot against that picture
                                        (the walls, the shooter's own place and everything that is not a target stay as they are now)
```

## What the engine does

- **`SimGame`** (`crates/lunaris-net/src/scale/simgame.rs`) records, after every tick's steps, the place of every avatar and of every moving `NetEntity` (class 0 and 4 and up) that has `HitPoints`: a `Frame` of `(uid, place)` sorted by
  uid, kept for as long as `NetHost.lag_comp` says. The buffers are reused: after the first second nothing is allocated.
- Before the steps, for each avatar whose weapon's trigger is held this tick, it works out how far back the player was looking: `rtt / 2 + NetHost.interp` plus one tick, in milliseconds (`rtt` is the smoothed round trip the server measures for that client,
  `World::rtt_ms`; the tick is the average wait, half a tick for the client's next send and half for the server's next tick), at most `NetHost.lag_comp`; finds the two frames round that moment, interpolates the place of every target within the weapon's `range` (plus 5 m) of the shooter, and gives the list to the weapons as that
  shooter's **`weapons::Rewind`** (`Sim.scripts.rewind`, host policy: it is not saved, not hashed, and cleared after the steps).
- **`weapons::update`** (the engine, `crates/lunaris-sim/src/weapons.rs`) tests a hitscan round of that shooter against the targets named in its `Rewind` as if they stood where the list says (a character's capsule, a box, a
  sphere, and the head and legs zones measured from the rewound feet), and against everything else where it is. The damage goes to the target itself, wherever it is now; the `hit` event is where the shooter saw it.

Another game host can use the same hook: fill `sim.scripts.rewind.shooters` (`shooter uid -> [(target uid, place)]`, **sorted by target uid**) before `Sim::step`.

## Settings (`NetHost`)

| Field | Default | Meaning |
|---|---|---|
| `lag_comp` | 250 | The most milliseconds a shot is looked back by; 0 switches lag compensation off (shots are decided at the present). Also how long the history is kept. |
| `interp` | 100 | The milliseconds the clients show others behind the present: the interpolation delay of `NetClient` (its default `interp_ms` is 100). Keep the two equal. |

A shot is never looked back by more than `lag_comp`: a player with a 400 ms ping shoots at a world 250 ms old and has to lead the rest, which is the price that keeps a lagging player from shooting people who have
already walked behind a wall.

## What it does and does not cover

- Hitscan weapons (`Weapon.kind` 0) against characters and the moving things the server replicates. Flying rounds (`Weapon.kind` 1) are physical: they are not rewound. Doors, building pieces and the other still things
  are not rewound (they change rarely; a door that closed 100 ms ago can still stop a shot that the shooter saw go through).
- The shooter itself is not rewound: the server uses the place the player reports. Its own movement is the client's (the server only refuses what it could not have done).
- The shooter's muzzle and aim are what the client reports at the moment the server handles the shot, not what they were when the player pulled the trigger: with 80 ms each way a player who flicks the mouse in that
  time moves the shot with it. Nothing is guessed about that.
- Fairness: the one who is shot sees it a little after the fact (they may be behind a corner on their own screen): the usual price of "favour the shooter" in every shooter with a server-side hit decision.

## Cost

One shooter holding the trigger costs one pass over the targets in range (two merged sorted lists, a few multiplications each) and, in the weapons, a binary search per target in the ray test; a player who is not holding the
trigger costs nothing. The history is one `(uid, place)` per character and moving thing per tick (reused buffers).
Measured on 2026-10-21, real UDP on one machine, release build, the server with **200 swarm bots** walking about and firing (5,013 shots in 32 s, the scene of the tests): the game's share of a tick was **simulation 591 us,
input 47 us, replication 75 us** with lag compensation and 577 / 46 / 68 us with `lag_comp` 0: about 20 to 60 microseconds a tick more (the tick as a whole 4.71 ms against 4.65 ms, of the 33 ms there is).

## Test it

- `weapons_tests` (3 tests): a round meets a target where the shooter saw it, not where it is; one shooter's rewind does not touch another's; a rewound character keeps its zones and a box its faces.
- `simgame_tests::a_shot_hits_what_the_shooter_saw_on_its_screen_and_with_lag_compensation_off_it_is_decided_at_the_present`: a runner crosses the shooter's line at 12 m/s on a link with 80 ms each way; the shooter fires when the
  runner, as the shooter sees it, is in the sights. With lag compensation the runner is hit, wherever in a server tick the run starts (five phases, 7 ms apart); with `lag_comp` 0 or 20 the round goes behind it.
  Where the shooter sees a target is within a few centimetres of where the rewind puts it at 12 m/s only on average: the send and tick waits are really 0 to 33 ms each, so a target that runs fast across the sights can be
  missed by up to a body's width; walking and jogging speeds are well inside the 0.35 m radius.
