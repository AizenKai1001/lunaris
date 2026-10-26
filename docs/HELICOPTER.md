# Helicopters: a flight model for a game

A helicopter is a rigid body that the rotors lift along its own up axis, whose stick tips it to fly, whose pedals turn it, that is **loud** (the machines hear it from far, and it raises the pilot's `Heat`), and that
is hurt by a crash. It is a model for a game, not a simulator: the stick asks for an *attitude* (how far the nose is down, how far the right side is down), the collective for *lift*, the pedal for a *turn rate*,
and the engine steers the body's spin towards what was asked. Gravity, bumping into things and landing are the rigid-body physics. Nothing is drawn or played: the rotors are entities the game supplies and the
engine turns, the sound is the game's, and `Noisy` is what an AI hears. Code: `crates/lunaris-sim/src/helicopter.rs` (tests in `helicopter_tests.rs`, which are the examples below).

## A helicopter

```json
{ "uid": 1, "components": {
    "Transform": { "pos": [0, 30, 0] },
    "Collider": { "shape": 1, "size": [2, 2, 6] },
    "RigidBody": { "mass": 2000 },
    "Velocity": {},
    "AngularVelocity": { "damping": 0 },
    "HitPoints": { "value": 1000, "max": 1000 },
    "Helicopter": { "engine": true, "noise": 160, "heat": 2, "rotor": 7, "tail_rotor": 8 } } }
```

It needs the body components (`Collider`, `RigidBody` with a mass, `Velocity`, `AngularVelocity`); without them it is left alone. Forward is its local **-z** (like a camera and a `Weapon`), +x is right, +y is up.
`HitPoints` are optional (without them it cannot be hurt). The `Collider` is the game's: a box for the cabin is enough, and the model is a child or the same entity's `MeshRenderer`.

## The controls

The game (a graph system, the `InputMap`, an AI) sets these every tick:

| Field | Range | Does |
|---|---|---|
| `engine` | on/off | the rotors; off, they spool down |
| `collective` | -1 to 1 | lift: 1 climbs, -1 gives none, 0 **hovers** (when `lift` is twice gravity, as it is by default) |
| `pitch` | -1 to 1 | 1 tips the nose down: forward |
| `roll` | -1 to 1 | 1 tips the right side down: sideways to the right |
| `yaw` | -1 to 1 | the pedal: 1 turns to the right |
| `pilot` | uid | whose `Heat` rises while it flies (see below) |

Neutral controls hold height, attitude and heading (the test `with_the_stick_centred_it_hovers` hovers for five seconds and moves less than a metre). `pitch` 1 settles at `max_tilt` (30 degrees) in a second
and flies at the speed where `drag` balances the push; releasing the stick levels it and the drag stops it. The pedal turns it at `yaw_rate` (80 degrees a second) and a released pedal holds the heading.

## The machine (all optional)

`lift` (m/s^2 at full collective and full rotor speed, 19.6), `spool_up` and `spool_down` (seconds for the rotors to reach speed and to stop, 6 and 10: lift goes with the *square* of the rotor speed, and below 30
percent the body is not steered at all, so a dead helicopter tumbles), `max_tilt` (30), `tilt_rate` (degrees a second the attitude follows the stick, 70), `yaw_rate` (80), `assist` (0 to 1, how much of the lift that
tilting costs is given back, 1: a tipped helicopter does not sink), `drag` (1/s sideways, 0.15) and `climb_drag` (up and down, 0.8; it falls to a quarter of that with the rotors stopped, so a dead
helicopter does not float down), `ground_effect` (0.2: the extra share of lift near the ground, fading out over `ground_height`, 6 m).

## Noise and heat: flying draws attention

`noise` (160 m) is how far the rotors are heard at full power. The engine keeps a **`Noisy`** on the helicopter (it adds one if there is none) with `radius = noise * spool * (idle_noise + (1 - idle_noise) *
power)` where `power` is 0 to 1 by the collective: at full collective 160 m, hovering about 108 m, at idle 56 m, with the engine off 0. An AI's `Perception` (docs/AI.md) of another side hears it inside
that radius times its own `hearing`, goes alert and searches where it came from (`the_rotors_are_loud...`). `heat` (per second at full noise, 0 by default) is added to the pilot's `Heat` while the rotors are up, so the
director (`docs/AI.md`) sends hunters to someone who flies: the game decides how much a helicopter costs its pilot in trouble.

## Crashing

A sudden loss of speed above `crash_speed` (8 m/s; 0 switches crashes off) in one tick, the speed the contacts took away, costs `crash_damage` (12) hit points for every m/s over it and gives the event `heli_crash`
(on the helicopter, at its place). A gentle landing, a bump, a rotor strike on nothing: no damage. At 0 hit points the engine stops for good (the rotors spool down by themselves). What a wreck looks like, burning, a
`Lifetime`, is the game's: listen for the event `died`.

## The rotors

`rotor` and `tail_rotor` name entities whose rotation the engine sets: the main one turns about its own y axis, the tail one about its own x, at `rotor_rps` turns a second at full rotor speed (the tail 5 times
that). A part that has a `Parent` and a `LocalTransform` gets the turn as its local rotation, any other as its `Transform.rot`: the part's own rotation is replaced by the spin, so a rotor that must be tilted
goes on a parent. The model, its blur and its sound are the game's; `rotor_angle` (radians) and `spool` are outputs for whoever wants them.

## Outputs

`spool` (0 to 1), `speed` (m/s), `altitude` (metres from the floor of the collider to what is under it, -1 if nothing within 300 m), `tilt` (nose down, right side down: degrees), `heading` (degrees, a quarter
turn to the left is positive: the same as `Transform` yaw elsewhere), `rotor_angle`. `last_v` is the engine's (to see what a crash took away): leave it alone, and set it with the velocity if you move a helicopter
by hand and do not want a crash counted.

## What to supply

The model, its rotors, the sound (the rotor loop pitched by `spool`, a hum louder with `Noisy.radius`), the cockpit and the seats (a pilot is an entity whose `Parent` is the helicopter and whose input the game
reads: the game sets `pilot` and the controls), the wreck and the smoke, the fuel (set `engine` false when the tank is empty), the weapons on the skids (a `Weapon` with the helicopter as parent), the crash and rotor
damage on the crew. None of it is in the engine.

## Limits (so far)

- Attitude control, not rotor dynamics: no blade stall, no vortex ring, no torque from the main rotor, no autorotation (a helicopter with the engine off falls with a quarter of the air resistance).
- Altitude is one ray under the middle of the body, so it is the height over what is under the centre, not the lowest point.
- A helicopter that is moved by something else (a script that sets its `Velocity`) can count a crash: set `last_v` with it.
- Water landings, rotor wash on the ground and on the sea, and tail strikes are the game's.
- Over the network (docs/NETWORK_SCALE.md) the server runs this and the clients see the pose; the controls of a pilot go up as the player's input.

## Flying it: seats

A player gets into a helicopter and flies it with a `Seat` (`drive` 1) and the sitter's `Controls`, alone or on a game server: see `docs/SEATS.md`.
