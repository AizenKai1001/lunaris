# Stealth: noise by how you move, and how hard you are to see

A character that sneaks past the machines, or runs and wakes them: `Stealth` measures how fast the character moves and keeps up to date the two numbers the AI's senses (docs/AI.md) use. It draws and plays
nothing. Code: `crates/lunaris-sim/src/stealth.rs` (tests in `stealth_tests.rs`, which are the examples below), and one line in `perception.rs` that uses the visibility.

```json
{ "uid": 1, "components": {
    "Transform": { "pos": [0, 0, 0] }, "Team": { "id": 1 }, "HitPoints": { "value": 100, "max": 100 },
    "Stealth": { "crouch": false, "light": 1 } } }
```

## Noise

The engine measures the character's speed on the ground (from where it was a tick ago, smoothed over a fifth of a second; a jump of 40 m/s or more between two ticks is a respawn, not a speed) and keeps a
`Noisy` on the entity (it adds one if there is none) with the radius an AI's hearing reacts to:

| Speed | Noise (metres) |
|---|---|
| under 0.3 m/s | 0: standing still is silent |
| `walk_speed` 2 m/s | `walk_noise` 5 |
| `run_speed` 5 m/s | `run_noise` 14 |
| `sprint_speed` 8 m/s and over | `sprint_noise` 22 |

In between, the noise rises in straight lines. **Crouched** it is `crouch_mult` (0.3) times that. A `Perception` of another side hears the character inside that radius times its own `hearing`, goes alert and searches where the
noise was (docs/AI.md); a walking player can pass at 6 m and a sprinting one is heard at 22 (`a_running_player_is_heard_further_than_a_walking_one`).

## Visibility

`visibility` is 0 to 1: **how much of its sight range an AI has for this character**. A `Perception` that looks for a character with a `Stealth` sees it only within `sight_range * visibility` (what is within 1.5 m is
always noticed, and a character with no `Stealth` is seen from the whole range). It is the product of three things:

- **crouching**: `vis_crouch` (0.6) when `crouch` is on;
- **moving**: standing still is `still_mult` (0.7) of that, a run (and beyond) the whole of it, in between in a straight line;
- **light**: `min_light` (0.4) of it in the dark (`light` 0), all of it in the light (`light` 1). **A game sets `light`** from what it knows: the time of day, a torch, a lit room, a searchlight on the player.

So a standing player is seen from 28 m by a machine that sees 40 m, a crouching one standing still from 16.8 m, and in the dark from 6.7 m (`what_is_hard_to_see_is_seen_from_nearer`). Both numbers are
outputs: `speed`, `noise` and `visibility`.

## The inputs

`crouch` (a button of the game; on a game server the avatar's `crouch_mask`, bit 4 by default: `NetAvatar`, docs/GAME_SERVER.md) and `light`. Everything else is a number to tune: `walk_noise`, `run_noise`, `sprint_noise`,
`crouch_mult`, `walk_speed`, `run_speed`, `sprint_speed`, `vis_crouch`, `still_mult`, `min_light`.

## What to supply

The crouch button and the animation, the value of `light` (from `TimeOfDay`, a flashlight, a lit room), the sounds of footsteps (play them by `Stealth.noise`), and what the machines do when they hear
(their `Perception`, a behaviour tree: docs/AI.md).

## Limits (so far)

- Speed is measured on the ground plane (x and z): a fall or a climb is silent. A character on a vehicle is as loud as the vehicle moves unless the game removes its `Stealth` or sets `crouch_mult` to 0 and `walk_noise` to the vehicle's noise by script.
- Surfaces (a metal floor is louder than grass) and carried weight are not modelled: a game that wants them sets `walk_noise` and `run_noise` from the ground under the player by a graph system.
- Visibility scales the range only: it does not change how fast awareness fills (that is by distance to the scaled range, as it was).
- The `Searchlight`'s detection (docs/AI.md) is the light's own and does not use `visibility`.
