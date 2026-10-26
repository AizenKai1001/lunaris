# Interaction: the use button, doors, containers, keys and items on the ground

A character looks at a thing and uses it: a **door** swings about its hinge, a **container** opens for the one who used it, an **item** on the ground goes into their bag, a **locked** thing wants its key.
It makes no screen: the prompt text, the container's window and the sounds are the game's, driven by the outputs and the events. Code: `crates/lunaris-sim/src/interact.rs` (tests in `interact_tests.rs`, which are the
examples below). On a game server (docs/GAME_SERVER.md) the use button of a player's avatar is one of the bits of the buttons it reports (`NetAvatar.use_mask`).

## The character: `Interactor`

```json
{ "uid": 1, "components": {
    "Transform": { "pos": [0, 1.6, 0] }, "Inventory": { "size": 24 },
    "Interactor": { "reach": 3 } } }
```

It looks along its entity's local -z from its place (`eye` is an offset in the entity's own axes: put the `Interactor` on the camera, or on the avatar's head child) up to `reach` metres. `act` is the use button, held
down by the game or the player's button: **a press counts once** (a held button does nothing more until it is let go; the engine keeps `held`).

| Field | Meaning |
|---|---|
| `reach` | how far it looks (3) |
| `eye` | where from, in its own axes (0) |
| `act` | the button: set it true while the key is down |
| `inventory` | whose `Inventory` holds its keys and receives what it picks up: 0 is this entity's own, else its parent's (a head child uses the avatar's bag) |
| `look` | find what it looks at every tick, for the prompt on the screen (on); off (a server's avatar) it looks only when it acts |

The engine writes, every tick it looks: `target` (a uid, 0 for nothing), `prompt` (the thing's text), `kind`, `distance` (to the point it is aimed at); and for one tick when it uses something, `used` (the uid) or
`denied` (`locked` or `full`). `open` is the container it has open.

What it looks at is **the nearest thing that it can see**: the thing's shape (its `Collider`, a box, a sphere or a capsule, or a sphere of `radius` metres when it has none: a loot item needs no collider), within the
`Interactable.range` and the `reach`; a wall (any static collider) in front of it hides it; the thing's own collider is no wall. A closed door in front of a chest hides the chest and is what is looked at.

## The thing: `Interactable`

| Field | Meaning |
|---|---|
| `kind` | 1 a door (the entity also has a `Door`), 2 a container (an `Inventory` beside it), 3 an item to pick up (`item`), 0 anything else (the event `used`) |
| `prompt` | the text for the screen (`Open`, `Search`, `Take`) |
| `range`, `radius` | how near the user must be (3 m); the sphere that is aimed at when there is no collider (0.6 m) |
| `locked`, `key`, `key_consume` | a locked thing opens for a character with the item `key` in their bag (and is unlocked for everybody from then on; the key is used up only with `key_consume`); the others get `locked` |
| `item` | for a pickup: `ITEM N`, `bandage 3` |
| `cooldown`, `enabled` | seconds between two uses (0.3); false hides it from everybody |

Engine's: `user` (who used it last; for a container, who has it open), `open` (a container is open), `uses`, `ready` (the tick it can be used again). Events (the uid of the thing, at its place): `interact` (every use),
`door_open`, `door_close`, `container_open`, `container_close`, `picked`, `unlocked`, `locked`, `inventory_full`, `used`.

- **A container** (kind 2) opens for the one who uses it (`user` is their uid, the event `container_open`), shuts when they use it again, or **by itself when they walk more than `range` + 1 metres away**, or when
  the thing goes. Its window shows the container's `Inventory` (the UI kit's `InvSlot`s bound to it) and the player drags things between the two bags as in `docs/GAMEPLAY.md`, "Inventories".
- **A pickup** (kind 3) puts `ITEM N` into the user's bag and removes the thing; what does not fit stays on the ground (`item` says how much is left, the event `inventory_full`, `denied` `full`).
- **A locked door that is used with the key in the bag** unlocks and opens in the same press (`unlocked`, `door_open`).
- **A code lock** (`CodeLock`, `docs/RAIDING.md`) is the other way a thing refuses: only the players who entered its code (by name) can use it; the others get `locked` and the event `lock_prompt` about them, so that the game asks for the code. The lock is on a door or a container that has `HitPoints` and goes with it when it is broken.

## The door: `Door`

```json
{ "uid": 10, "components": {
    "Transform": { "pos": [0, 1.6, -2] }, "Collider": { "shape": 1, "size": [1, 2, 0.1] },
    "Interactable": { "kind": 1, "prompt": "Open" },
    "Door": { "hinge": [-0.5, 0, 0], "angle_open": 100, "speed": 160 } } }
```

When `open` is set (by the interaction or a script) the entity turns about the hinge to `angle_open` degrees at `speed` degrees a second, and back when it is cleared. `hinge` is the hinge's place in the door's own axes
from its origin (a door a metre wide with its origin in the middle: `[-0.5, 0, 0]` or `[0.5, 0, 0]`), `axis` the way the hinge runs in the same axes (up). **The pose the door has when the scene starts is its shut pose:
put it shut.** A negative `angle_open` swings it the other way. With a `Parent` and a `LocalTransform` the swing is done on the local pose. The door's collider turns with it, so an open door lets you through and a
shut one does not. The open leaf lies along the way: to shut it one aims at the leaf from the side (`a_door_swings_about_its_hinge_and_back_and_a_held_button_counts_once`).

## What to supply

The doors' and chests' models and colliders, the prompt text, the container window and the sounds (`door_open`, `container_open`, `picked` are the events to play them on), the items and the keys
(`ItemTable`), and what a switch or lever (`kind` 0) does when it is `used` (a graph system reacts to the event). The engine ships no art.

## Limits (so far)

- Looking for what is aimed at is one pass over the things that can be used, per tick that anybody looks (every tick for a character with `look`, only at a press for one without): fine for a player's computer and for a
  server's avatars, which do not look. A map of a hundred thousand things used by hundreds of players a second wants an index by place (not done).
- A shut door is a `Collider` that the **AI's** navigation does not know about opening: a machine does not open doors (they have no hands): the game's `NavAgent` handles doors by its own rule.
- Doors swing about a hinge only: sliding doors, shutters and lifts are a `Tween` of the entity's position.
- A pickup does not drop a stack in two slots when one is full; it goes where the bag's own `add` puts it (onto stacks first, then empty slots).

## Seats

An `Interactable` of kind 0 on an entity with a `Seat` is a way into a vehicle: the seat answers the use (`docs/SEATS.md`).
