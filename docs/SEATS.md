# Seats: sitting in a vehicle, flying it, getting out

A **`Seat`** is a place in a vehicle that a character sits in. A character whose use button (`Interactor`, docs/INTERACT.md) uses it sits down, is carried with the vehicle, and gets out when it uses it again. The pilot's seat
(`drive` 1) flies the vehicle's `Helicopter` (docs/HELICOPTER.md) from the sitter's **`Controls`**. It works in a single-player game and, with no extra code, on a game server (docs/GAME_SERVER.md), where the pilot is a
player somewhere else and the helicopter is the server's.

```json
{ "uid": 60, "components": {
    "Transform": { "pos": [0, 1, -3], "scale": [2, 2, 2] }, "Collider": { "shape": 1, "size": [1, 1, 1] },
    "RigidBody": { "mass": 2000 }, "Velocity": {}, "AngularVelocity": { "damping": 0 }, "HitPoints": { "value": 1000, "max": 1000 },
    "Helicopter": { "engine": false },
    "Interactable": { "kind": 0, "prompt": "Fly" },
    "Seat": { "offset": [0, 0.5, 0.2], "exit": [2.5, -0.5, 0], "drive": 1 } } }
```

The way in is an `Interactable` of kind 0 on the same entity (it only reports the use, which the seat answers; it needs a `Collider` to be aimed at). `lunaris call check` names a seat with no way in and a pilot's seat with no helicopter.

## `Seat` (on the vehicle, or on a child entity of it for a second seat)

| Field | Default | Meaning |
|---|---|---|
| `offset` | 0,0,0 | where the sitter sits, in the seat entity's own axes (rotated with the vehicle) |
| `exit` | 2,0,0 | where it stands when it gets out, in the same axes |
| `drive` | 0 | 0 a passenger's seat; 1 the pilot's: the entity's `Helicopter` is flown from the sitter's `Controls`; 2 the driver's: the entity's `Vehicle` (the raycast car of docs/GAMEPLAY.md) is driven from them: `axes[2]` the throttle (forward is positive, back reverses), `axes[3]` the steering (right is positive), `axes[1]` below 0 the brake, above 0.5 the handbrake |
| `max_exit_altitude` | 3 | the highest (metres above the ground, the helicopter's `altitude`) a pilot may get out from; higher up the use button does nothing but `seat_denied` |
| `max_exit_speed` | 4 | the fastest (m/s) a car's driver may get out at; faster the use button does nothing but `seat_denied`. A driver who gets out leaves the car with the handbrake on |
| `occupant`, `held` | | the engine's: who sits, and whether the use button was down last tick |

- **Sitting down:** the seat must be free and the character alive. The press that sat it down is still down: it must be let go and pressed again to get out. While somebody sits the seat's `Interactable.enabled` is off (no prompt, nobody else can use it).
- **While seated** the character gets a **`Seated { seat }`** component, its place is the seat's every step (and its `Velocity` the vehicle's), so it goes where the vehicle goes. Its look and its buttons are its own. A game stops the walking of a
  seated character with a system that skips `Seated`, or ignores its controls.
- **Getting out:** at `exit`. A sitter who **dies** is let go where it is (the respawn puts it somewhere else), one that is **removed** leaves the seat free. Leaving a pilot's seat puts the stick back to rest, shuts the engine (`Helicopter.engine` false:
  the rotors spool down) and clears `Helicopter.pilot`; sitting down in it starts the engine and sets the pilot (whose `Heat` the rotors' noise then raises).
- Events: `seat_enter`, `seat_exit`, `seat_denied` (on the seat's entity).

## `Controls` (on the character): what the hands say

`axes` (four numbers, -1 to 1) and `buttons` (bits the game defines). For a helicopter: `axes[0]` roll (right is positive), `axes[1]` collective (up is positive), `axes[2]` pitch (forward is positive), `axes[3]` yaw (right is positive).
A single-player game writes them from the keys with a graph system (`{"set": "Controls.axes[1]", "to": {"sub": [{"res": "action.up"}, {"res": "action.down"}]}}`); a game server writes them from the player's `NetPlayer.axes`. Values beyond
the stick's travel are held to it and a number that is not a number is 0.

## On a game server

- The pilot's computer sets `NetPlayer.axes` (a graph system from the keys); the driver sends them (**`CONTROL`**, 5 bytes: four signed bytes) when they change (at most 20 a second) and again every 200 ms while the hands are away from rest;
  the server forgets them after two thirds of a second of silence (a pilot whose connection dies lets go of the stick). They become the avatar's `Controls`.
- When the avatar sits down the server tells that player (**`RIDE`**: the server's number of the vehicle and the seat's offset) and the client sets `NetPlayer.riding` and `NetPlayer.seat`. From then on the engine carries the player's `Transform`
  to the seat on the vehicle's puppet every tick (so the camera, a child of the player, rides too) and the server does not believe the place the player reports (its net entity is `locked`: the look, buttons and the rest are the player's,
  the place is the seat's). When the player gets out the server says `RIDE 0` and puts it on its feet at the exit (a correction). Other players see the pilot at the seat.
- The vehicle must be a `NetEntity` (class 4 or above: it moves) with a line in the clients' `NetPlayer.kinds`. A seat on a child entity finds its vehicle through its `Parent`.
- The pilot sees the vehicle as the other players do, a tenth of a second behind (`NetClient.interp_ms`): the helicopter answers the stick a tenth of a second plus the round trip late. Flying is as forgiving of that as the flight model is slow.

## What to supply

The vehicle's look (the puppet prefab, the rotor that spins on the client), the key bindings to the axes, the sound, and what the cockpit shows. Nothing is drawn or played here.

## Limits (so far)

The helicopter and the raycast car are the driven vehicles (a boat or a tank would be another `drive` number mapping the same axes). A rider is carried to the puppet's place with its yaw only (the helicopter's tilt is not replicated: the cabin does not
tilt on the other screens). Weapons on a vehicle and a gunner's seat are not done. One seat per entity (a second seat is a child entity); a seat child's own place is that of the last hierarchy pass (one step late).

## Tests

`seats_tests` (10: a car driven from the controls, no exit at speed and a handbrake after; sitting, being carried, an occupied seat, the press that sat it down not getting it out, the seat and exit turning with the vehicle, the pilot's controls and the stick going back, no exit from the air, a dead or removed sitter,
determinism), `simgame_tests` (a player sits in the helicopter, flies it up while the other player sees both, is refused an exit in the air, brings it down and gets out; the messages round-trip), two `check` tests.
