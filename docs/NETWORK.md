# Multiplayer: lockstep or rollback, with nothing but the standard library

Lunaris promises that the same scene, seed and commands give the same state, bit for bit, on any machine and any thread count. That makes multiplayer cheap and exact: **nobody sends the world, everybody sends inputs**. Each player runs their own copy of the game, a small relay (the **host**) passes the players' inputs around, and every copy plays the same inputs in the same order. A game of 49 entities costs a few hundred bytes a second; a game of a million entities costs the same.

There are two ways to play it, and you choose when you start the host:

| | **lockstep** (the default) | **rollback** (`--rollback`) |
|---|---|---|
| a tick is played when | everybody's input for it has arrived | at once, with a guess for the inputs that have not arrived |
| a slow connection | slows everybody down (the game waits) | costs the player with the slow connection a few corrected ticks; nobody waits |
| what you see | exactly what everybody sees, never a correction | the guess, then (rarely) a small correction when it was wrong |
| a player who drops | ends the game for everybody | the game goes on; the player can come back (see below) |
| spectators, lobby | lobby only | spectators from any time, lobby |
| good for | turn-like games, strategy, tight LANs | action games over the internet |

```
lunaris host --players 2                                   # a relay on 127.0.0.1:7000 (it does not run the game)
lunaris play examples/duel.lunaris.json --join 127.0.0.1:7000 --name Ada     # window of player 1
lunaris play examples/duel.lunaris.json --join 127.0.0.1:7000 --name Bo      # window of player 2 (another terminal or computer)

lunaris host --players 2 --rollback                        # the same game with rollback netcode
lunaris play examples/duel.lunaris.json --join 127.0.0.1:7000 --rollback --name Ada
lunaris play examples/duel.lunaris.json --join 127.0.0.1:7000 --rollback --name Bo
lunaris watch 127.0.0.1:7000 examples/duel.lunaris.json    # a spectator, at any time (rollback hosts only)
```

(On one Windows computer, `play_duel.bat` starts the host and both windows in lockstep; the keyboard drives whichever window has the focus. `play_duel_rollback.bat` does the same with rollback, makes player 2's network bad on purpose, and opens a spectator window a few seconds in; the window titles count the rollbacks.)

`examples/duel.lunaris.json` is a two-player game (built by `tools/build_duel.py`): two heroes in an arena, 24 glowing orbs, first to 12 wins. Both players use WASD and Space on their own keyboard; the scene reads the first player's keys as `input.w`, `input.a`, ... and the second player's as `input.p2.w`, `input.p2.a`, ... (a third player's are `input.p3.*`, up to 16 players). Join order decides who is player 1.

To let other computers join, start the host with `--bind 0.0.0.0` and give them your address (`--port 7000` by default). There is **no encryption and no password**: use a trusted network or a VPN, and remember that anyone who can reach the port can take a seat (or watch).

## Commands

| command | what it does |
|---|---|
| `lunaris host [--players N] [--port P] [--bind ADDR] [--delay D] [--ticks N] [--seed S]` | lockstep relay for N players (default 2). `--delay` is the input delay in ticks (default 3, 50 ms at 60 Hz), `--ticks` ends the game after N ticks, `--seed` is the seed every copy starts with. Prints the result when the game ends and exits with an error if any copy drifted. |
| `lunaris host ... --rollback [--window W] [--grace S] [--max-log-mb M] [--max-watchers N]` | the same relay for a rollback game. `--delay` (default 2 here) is the input delay clients use unless they say otherwise; `--window` (default 8) is how many ticks a client may run ahead of the oldest tick it is still waiting for somebody's input for, and so the deepest a rollback can go; `--grace` (default 3 s) is how long an empty seat holds the game up before it is frozen; `--max-log-mb` (default 64) limits the memory of the input log; `--max-watchers` (default 16). |
| `lunaris host ... --lobby` | lockstep or rollback: wait in a lobby until the players are ready (below). |
| `lunaris play <scene> --join HOST:PORT [--name NAME]` | a window (GPU renderer) that takes a seat. Keys and mouse go to the host; in lockstep the game steps only the ticks everybody's input has arrived for. The title shows the seat. |
| `lunaris play <scene> --join HOST:PORT --rollback [--input-delay D] [--token T]` | the same in a rollback game. `--input-delay` (default: what the host suggests, 2) is how many ticks your own keys are held back so that everybody has them in time. `--token` takes your old seat back (below). The title shows how far ahead of the settled game you are and how many rollbacks there have been. |
| `lunaris play <scene> --join HOST:PORT --watch` | a window that only watches (rollback hosts). |
| `lunaris join HOST:PORT <scene> [--name NAME] [--hold d,space] [--ticks N] [--rollback ...]` | a seat without a window: a bot, a test client, a server-side watcher. It holds the listed keys the whole game and plays flat out; it prints its final tick, state hash and resources (and, in a rollback game, how much it rolled back). |
| `lunaris watch HOST:PORT <scene> [--name NAME] [--ticks N]` | a spectator without a window: replays the game from tick 0 at full speed, then follows it live. Prints the final tick and state hash. |
| `--lobby [--ready]` on `join` / `play` | for a host started with `--lobby`: `--ready` marks you ready at once (a bot); without it you type `ready`, `unready` or (first seat only) `start` in the terminal. |
| `--sim-latency MS --sim-jitter MS --sim-loss PCT [--sim-dup PCT --sim-seed N]` on `join` / `watch` / `play` | make *your* network worse on purpose (below). |

## Lockstep: how it works

1. A client connects and says hello (protocol version). The host answers with its player number, the player count, the seed and the delay.
2. Each client builds its engine with that seed, loads the scene and reports the state hash of what it loaded. **If the hashes differ** (another scene file, another engine build) the host refuses to start and says so: it is far better than a game that silently diverges.
3. The host sends `start` and the first `delay` ticks (they have nobody's input). From then on a client sends, for the tick `delay` ticks ahead of its simulation, the keys that changed. When the host has every player's input for the next tick it sends that tick to everybody. A client steps tick `t` only when it has tick `t`; it applies the inputs of `t` (player order) and steps. The result: a key pressed at tick `t` takes effect at tick `t + delay` in **every** copy, the same for everybody.
4. Every 30 ticks each client sends its state hash. The host compares them; on a mismatch it reports a **desync** (tick and hashes) to everybody and in its own result. A desync is a bug (a system that is not deterministic, a different file) or a cheat; the tick number tells you where to look (`lunaris run --twice`, the `hash` tool, snapshots).
5. When anybody leaves, or the tick limit is reached, the host says goodbye and the game ends for everybody. Ticks that had already arrived are still stepped, so all copies finish on the same tick.

The wire is one JSON object per line over TCP (the full table is at the top of `crates/lunaris-net/src/lib.rs`), so a client in any language can take a seat: `{"hello": ...}`, then `{"input": {"tick": 3, "values": {"d": true}}}` per tick, and so on. The lockstep protocol version is 1, and a lockstep host and client behave exactly as they always did: protocol 2 (below) is only spoken when somebody asks for rollback, a lobby, watching or a seat back.

What lockstep does and does not give:

- **Exact.** There is no prediction and no correction: what you see is what everybody sees, bit for bit. The tests compare a lockstep game with an offline replay of the same keys and require the same final state hash (`crates/lunaris-net/tests/lockstep.rs`).
- **Cheap.** Bandwidth is the keys that changed, 60 small messages a second per player, regardless of the size of the world.
- **Everybody waits for the slowest.** The game runs at the pace of the slowest player's connection, and a player who stops sending halts everybody until the host's timeout (60 s) ends the game. A lockstep game has no reconnection and no spectators: somebody who connects while it runs is told so at once. Input delay hides latency: raise `--delay` for a slow network (6 ticks is 100 ms).

## Rollback: nobody waits

**The idea.** Every tick, your copy plays *now*. For every player whose input for this tick has arrived it uses the real input; for the others it uses a **guess**: "this player changes nothing", that is, their keys stay as they were (a held key stays held, a released key stays released). Before every tick your copy saves a **checkpoint** of the simulation. When a real input arrives for a tick that was already played with a guess, and the guess was wrong, your copy goes back to the checkpoint of that tick and plays the ticks again with the truth, in the same frame. The picture jumps a little when that happens, but the game never waited for the network, and every copy still ends in exactly the same state, because the simulation is deterministic and the real inputs are the same for everybody.

```
tick           10   11   12   13   14   15
you play       10   11   12   13   14   15      <- Bo's inputs for 11 to 15 are guesses ("no change")
                    |
Bo's input for tick 11 arrives: he pressed D. The guess was wrong.
                    v
you go back to the checkpoint of tick 11 and play 11, 12, 13, 14, 15 again with D pressed
```

**The pieces, in plain words.**

- **Input delay** (`--input-delay`, default 2 ticks = 33 ms at 60 Hz): your own keys take effect that many ticks after you press them, in every copy, so everybody has them a little before they are needed. More delay means fewer corrections for the others and a less snappy feel for you; it is local to each player.
- **Settled ticks and the frontier.** A tick is *settled* when every player's input for it is known (or the player's seat is frozen). The first tick that is not settled is the *frontier*. Nothing below it can change any more: checkpoints below it are thrown away, and **only settled states are compared** by the state-hash check (so a correction never looks like a desync).
- **The window** (`--window` on the host, default 8, announced to every client): your copy never plays more than `window` ticks beyond the frontier. If it would, it **stalls** until the missing inputs arrive. That bounds the work of a rollback (never deeper than the window) and the memory of the checkpoints (window + 1). Eight ticks is 133 ms at 60 Hz: a connection that is slower than that makes the game stutter instead of drift.
- **The host does not run the game.** It forwards each player's input to everybody else the moment it arrives (it does not wait for a complete tick), keeps the book of who has sent what, and keeps the *input log* (below). Inputs may arrive late, twice or in a different order than they were sent; the book works out what is settled by itself.
- **The end.** The host ends the game on a settled tick (`--ticks N`, or when somebody times out) and tells everybody which; a copy that had played past it goes back to it. All copies end on the same tick in the same state.

**What a rollback restores.** The world (entities, components, strings, resources: the `input.*` keys too), the tick, the random generator, the navigation paths and the behaviour-tree memory. That is everything the simulation changes while a game runs (the rest is derived again by the next tick: contacts, heightmaps read from files), and the tests check the result: after a rollback the state hash is exactly the hash of a game that never went back.

**What a game must not do in a rollback game.**

- Change systems or prefabs while it runs (`define_system`, `enable_system`, `load_game` replacing the scene): a checkpoint deliberately does not copy them, and going back across such a change is an error, not a silent drift.
- Depend on a native plugin that keeps state between calls (plugins must already be pure functions; WebAssembly systems start a fresh instance every tick, so they are fine: `docs/PLUGINS.md`, `docs/WASM.md`).
- Record while playing (`record_start`): the replayed ticks would be recorded twice.
- Expect sound (or any event-driven effect) to be taken back. Events are output: a rolled-back tick emits its events again, and `lunaris play` plays the sounds of the ticks it newly plays, so the corrected version of an old tick is silent and a sound of a wrong guess has already played. Effects that must be exact should be made of state (a resource, a component) and not of events.

**What it costs, measured** (`cargo test -p lunaris-net --release --test rollback_cost -- --nocapture`; Intel Core i7-9700K, Ubuntu 24.04, release build; microseconds; "step" is one tick of the scene on its own):

| scene | entities | `Sim::clone` | checkpoint | rewind | step | rollback of 8 ticks |
|---|---:|---:|---:|---:|---:|---:|
| `examples/skirmish.lunaris.json` | 34 | 34.9 | 8.5 | 8.7 | 48.5 | 1160 |
| `examples/duel.lunaris.json` | 49 | 18.6 | 8.2 | 8.6 | 25.9 | 314 |
| `examples/orb_dash.lunaris.json` | 25 | 25.0 | 6.2 | 6.2 | 11.4 | 173 |
| `examples/pile.lunaris.json` | 54 | 7.1 | 6.8 | 6.4 | 20.9 | 240 |

A rollback of 8 ticks is: go back to a checkpoint, then play 8 ticks, saving a checkpoint before each. At 60 Hz a frame is 16,667 microseconds, so the worst case here (the skirmish) is 7% of one frame, and the checkpoints themselves cost less than the ticks they protect. Two things made it that cheap: a checkpoint copies only what changes (not the systems and prefabs, which a whole-simulation clone also copies), and a world shares its list of components between copies instead of copying it (that alone made a checkpoint five times cheaper: 45 to 8 microseconds). The state hash is not computed per tick (it costs 10 to 25 microseconds): only for the ticks that are reported, every 30. A scene whose single tick takes 3 ms rolls back 8 ticks in 24 ms; there the cost is the scene, not the netcode. An example run of three players on links of 40, 80 and 120 ms (one way, with jitter) over 150 ticks rolled back 40 times and played 229 ticks again in total, and ended bit-identical.

## Spectators and coming back

In a rollback game the host keeps the **input log**: every key change of every settled tick, in order (only the changes: a game in which nobody touches a key costs nothing). The log is what makes two things possible, both by *replay*:

- **A spectator** (`lunaris watch`, `lunaris play --watch`, or `connect` with `watch: true`) may join at any time. It loads the scene, reports its state hash (a spectator whose world is not the game's is refused at once), and the host sends it the log from tick 0 in chunks. The spectator plays it at full speed (hundreds of ticks of a light scene in a few milliseconds), then follows the game live. It never sends input, so it cannot change anything; it reports its state hash like a player, so a copy that drifted is found. At most `--max-watchers` (16) at a time.
- **A player who dropped** can take its seat back. Every player is given a **seat token** when it joins (`seat token: ...` is printed; a program reads it from `seat.token()`). It is a nickname for the seat, not a secret: there is no encryption on this wire. Joining again with the token (`--rollback --token T`) loads the scene, replays the game from tick 0 like a spectator, asks for the seat when it has caught up, and is given it back: the host tells it the tick it takes over (`go`), everybody else is told the seat is live again, and the player plays on, its keys let go of if the old life of the seat left any held. If the old connection was not noticed to be dead yet, the new one replaces it.

**While a seat is empty.** The game must not wait for ever for a player who is gone, and it must not pretend nothing happened either. This is exactly what happens, in order:

1. *The seat is empty* (the connection ended, or the player said goodbye, or it has been silent while everybody else went on). Everybody is told. The others carry on until they are `window` ticks past the last input the seat sent, and then wait.
2. After `--grace` seconds (default 3) the host **freezes** the seat from tick F, the first tick it has no input for, and tells everybody (`frozen`, with F). From F on the seat's input is empty: **its last keys stay as they were**, so a hero that was walking keeps walking, which is what "repeat the last input" means for inputs that are changes. Nobody waits for the seat any more. The freeze is part of the settled history: the log, the spectators and the player who comes back all see the same game. While a seat is frozen the host also tells the players how far the game is settled (`frontier`), and they keep within the window of *that* tick: the frozen seat's input counts as known for ever, so a player left alone would otherwise run on without limit and settle ticks that the frozen seat's player, coming back, still changes. A rollback is never deeper than the window, frozen seats or not.
3. When the player comes back (any time before the game ends) the seat is **live** again from a tick the host names; everybody is told. Between the freeze and that tick the seat's input stays empty.
4. If nobody comes back, the game goes on to its end without the seat. If everybody is gone, the host ends the game when nobody has sent anything for its timeout (60 s).

**The log has a memory limit**: `--max-log-mb` (default 64 MiB; about a million key changes, many hours of a four-player game). A game that produces more keeps playing and the spectators already watching keep watching, but the oldest changes are thrown away, so **nobody can join late or come back any more** (they are told "the game is too old to join late") and the host's report says the log is incomplete. The host's memory never grows past the limit however long the game runs.

Lockstep hosts have no log and no spectators.

## A lobby

`lunaris host --players N --lobby` (lockstep or rollback) does not start as soon as N players have taken seats. Players join with a name, load the scene (their world must be the same as the first player's, or they are sent away), and see the list of players: `lobby (2 of 3): Ada [ready], Bo [not ready]`. The game starts when **all N seats are taken and everybody is ready**, or when **the first seat says `start`**, in which case the players that have loaded the scene play and the game has as many players as there are of them. Seat numbers follow the order of arrival and may shift while people come and go; the final numbers are sent when the game starts. In a terminal, `ready`, `unready` and `start` are typed; `--ready` makes a bot ready at once. From Rust: `JoinOptions { lobby: true, commands: Some(rx), on_roster: ... }`.

## Making the network worse on purpose

`--sim-latency MS --sim-jitter MS --sim-loss PCT` (and `--sim-dup PCT`) on `join`, `watch` and `play --join` put a small proxy between your client and the host, so that rollback (and the stutter of lockstep) can be tried on one computer, by a test or by a person with two windows. In Rust: `LatencyProxy::start(host_addr, LinkSim { latency_ms, jitter_ms, loss_pct, .. })`, then connect to `proxy.addr()`.

**What it simulates, honestly.** The delay is *one way*, added in each direction (50 ms of latency makes the round trip 100 ms longer), with a random jitter on top, which lets messages overtake each other. TCP never loses data, so "loss" is not messages disappearing: it is what loss looks like to a game, a message that arrives much later than the rest (`max(3 x latency, 60 ms)` late). `--sim-dup` delivers a message twice. Nothing is dropped for good, **so the simulator tests delay and reordering, not data loss** (the operating system retransmits on a real network, and a game over TCP sees the same thing: lateness). The receivers do not rely on order: inputs are collected by tick and chunks of the log are numbered. The tests below run through the proxy and require the same final hash as a game with no network at all.

## Protocol 2, and who refuses whom

Rollback, the lobby, watching and taking a seat back speak protocol version 2 (the table is at the top of `crates/lunaris-net/src/lib.rs`): the same JSON lines, a `hello` with `"version": 2, "mode": "rollback", "role": "player"` (or `"watch"`, with an optional `"token"`), and a few more messages (`roster`, `lobby`, `assign`, `seat`, `frontier`, `log`, `resume`, `go`; `bye` carries the tick the game ended on). Every mismatch gets a clear refusal, never a hang:

| client | host | answer |
|---|---|---|
| lockstep (protocol 1) | rollback | "this host runs a rollback game: join with --rollback" |
| lockstep (protocol 1) | lockstep with `--lobby` | "this host has a lobby: join with --lobby" |
| rollback, lobby or watching (protocol 2) | plain lockstep | "this host speaks lockstep version 1 and has no lobby: rollback, lobby and watching need a host started with --rollback or --lobby" |
| rollback | lockstep with `--lobby` (or the reverse) | "this host runs a lockstep game; you asked for rollback: join without --rollback" |
| watching | lockstep (any) | "this host runs lockstep, which has no spectators" |
| anybody | a running lockstep game | "the game has already started" |
| a new player (no token) | a running rollback game | "the game has already started: you can watch it, or take your old seat back with its token" |
| a wrong seat token | a rollback game | "unknown seat token" |
| a different scene, seed or engine build | any | "your world is not the same as the first player's": only that client is sent away in a rollback game or a lobby; plain lockstep cancels the game, as it always did |

## What this design gives and does not give

- **Exact in the end.** Whatever the network does, every copy ends in the state the confirmed inputs give. The tests compare rollback games (through delaying, reordering, duplicating proxies) with a replay of the host's confirmed log *and* with a game worked out offline from the keys the scripts chose, and require the same final hash.
- **Same machine, different picture.** The simulation is identical everywhere; the GPU picture may differ slightly between graphics cards. Tests and hashes use `quality: reference`.
- **Scenes** must keep to the rules every scene follows already: all state in components and resources, randomness only through the engine (`{"rand": ...}`), no wall clocks. A drifting copy is reported as a desync with the tick.
- **One camera.** The scene has one `Camera`, shared by every window: a party-game view, as in `duel`. Per-player cameras need a "which player am I" resource per window, which is the next step.
- **Limits.** Up to 16 players. No encryption, no passwords, no host migration (the host's death ends the game; the log lives in its memory). The host does not check that inputs are sensible: every copy is the judge of its own state, and a copy that lies about its state hash is found only if the others disagree with it. A rollback game needs a scene whose systems do not change while it runs (above).

## From Rust

`lunaris-net` depends only on the engine itself:

```rust
// the relay (lockstep as before; set mode, window, lobby, grace for the rest)
let host = lunaris_net::Host::bind("0.0.0.0:7000")?;
let report = host.run(&lunaris_net::HostConfig { players: 2, mode: lunaris_net::Mode::Rollback, ..Default::default() })?;   // blocks until the game ends
// report.log is the confirmed input log: report.log.replay(&mut fresh_engine, report.ticks) plays the game again

// a seat of any kind: lockstep, rollback, spectator
let mut options = lunaris_net::JoinOptions::new("Ada", lunaris_net::Mode::Rollback);
options.input_delay = Some(2);                         // watch: true for a spectator, token: Some(..) to take a seat back
let (mut seat, mut engine) = lunaris_net::connect("127.0.0.1:7000", options, |welcome| {
    let mut engine = lunaris_api::Engine::new(welcome.seed, ".");
    engine.call("load_scene", &Json::obj([("path", Json::str("examples/duel.lunaris.json"))]))?;   // any scene
    Ok(engine)
})?;
loop {
    seat.set_key("d", 1.0);                                  // your keys; only changes cost bandwidth
    let played = seat.advance(&mut engine, 1)?;              // sends what is due, plays what can be played, goes back if a guess was wrong
    if played == 0 && seat.finished().is_some() { break }
}
```

`advance` is meant to be called every frame (a window passes the number of ticks the clock allows, a bot passes a large number). `seat.take_desync()`, `seat.finished()`, `seat.take_events()` (somebody left, a seat froze, somebody came back) and `seat.rollback_stats()` report what happened. The plain `lunaris_net::Lockstep::join(addr, name, prepare)` of lockstep games still exists and is what `connect` uses when nothing asks for protocol 2.

## Tests

`crates/lunaris-net/tests/lockstep.rs` (lockstep, unchanged), `rollback.rs` (three players through delaying proxies end identical and equal to the offline replay; a game with no key changes never rolls back; rollback and lockstep play the same game; the window stalls a client exactly; a mid-game divergence is reported; a tampered scene is refused; inputs that overtake the `start` message or the news that a seat is back are kept), `watch_rejoin.rs` (a late spectator ends with the same hash; a killed player comes back with its token, replays and ends with the same hash; an empty seat freezes after the grace period; a player who comes back at once while the others lag; the log's memory limit), `lobby_refusals.rs` (the lobby in both modes; every refusal in the table above), `rollback_cost.rs` (the numbers above), and `crates/lunaris/tests/network.rs` (the real programs: a rollback host and two bots on a simulated bad network, a watcher, a lobby).
