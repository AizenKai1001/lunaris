# Turning a turret instead of taking it (`Hackable.mode` 1)

Hacking (docs/AI.md, "Hacking") was written for machines that a player drives: a drone, a crawler. The hacker holds the machine's controls, a camera follows it, the machine's own AI is off. A turret has no use for hands: it stands still and shoots. What a player wants of a hacked turret is that it **shoots for them without being steered**, for a while, while they carry on. That is `mode` 1.

## What it does

`Hackable.mode` (default 0: the old behaviour, unchanged). With `mode` 1:

* The hack fills as for any machine (`difficulty` seconds at the hacker's speed 1, range, line of sight, the hacker's trace in its `Heat`).
* At 100 percent the machine is **turned**, not taken: it joins the hacker's `Team` (event `hacked`, the machine) for `duration` seconds (`Hackable.time_left` counts down; `duration` 0 never runs out) and its own `Perception`, `NavAgent`, `FlyAgent` and `Weapon` go on working, now against the old side: whatever it was after is forgotten at the moment it turns, so that it does not go on shooting at the hacker's side.
* **The hacker is free.** `Hacker.state` stays 0, `controlled` stays 0, the `CameraRig` is not touched, `move_dir` and `fire` drive nothing. The hack key that is still held does not start another hack (`Hacker.blocked`, as after any control ends): let go first.
* **It never targets its hacker.** A hacker who is in a party has the party's `Team` (docs/PARTIES.md) and the machine gets that team, so the team rule already spares the whole party. A hacker with no party has team 0, which is hostile to everybody (`hostile(0, x)` is true for any `x`), so `Perception` skips the one uid in `Hackable.controller` of a turned machine (`perception.rs`, `Ai.friend`): as a target and as a noise (the hacker's own shots).
* It ends (event `link_lost`, the machine) when `time_left` reaches 0 or the machine is destroyed: its `Team` goes back to what it was (`keep` true leaves it on the hacker's side), `hacked`, `controller`, `progress` and `time_left` are cleared, it forgets its target and lets go of the trigger. **A hacker who dies or disconnects does not end it** (a base defence goes on without its owner until the time is over).
* A machine that is turned cannot be hacked again until it goes back (`hacked` is true); the engine's `progress` of every machine nobody is hacking falls back as before.

The countdown is in `Hackable.time_left` on the server; a game that wants its clients to see it puts it in a field it replicates (docs/GAME_SERVER.md, `NetEntity.anim_t`: the example game does).

## Where it lives

`hacking.rs`: `take` has the `mode` 1 branch, `run_down_turned` (called at the end of `update`, over every `Hackable` that is `hacked` with `mode` 1, in uid order) and `end_turn`; `perception.rs`: `Ai.friend`; `components.rs`: `Hackable.mode`. Cost: one pass over the `Hackable`s (machines, not players) a tick, and one `Hackable` lookup for each `Perception` entity.

## Tests (`sentry_tests.rs`, 5)

A turret is turned and not taken (the hacker is free, the camera stays, its AI keeps working, the team is the hacker's, the held key starts nothing); a turned turret shoots the old side (a machine of team 2) and spares a hacker of team 0; it goes back when its time runs out (events, team, shoots the player again, can be hacked again); a destroyed turret goes back and a dead hacker does not end it; `mode` 0 is still taken and driven. The existing 10 `hacking_tests` pass unchanged.

## Not done / limits

* A free player (team 0) who has turned a turret can still be hit by its **bullets** if the player stands in the line of fire to a target (the weapon rule spares only `team != 0 && same team`); the turret does not aim at them. A party member cannot be hit.
* A free player's own weapons can hurt the turret they turned (the same rule): nothing stops an owner from shooting their own turret.
* One hacker per machine, one turn at a time; a second hacker has to wait for it to go back.
