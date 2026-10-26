# Loadout: guns that are items in a bag

In a game where weapons are looted (a rifle in a chest, a box of rounds in a corpse's pockets) the gun is not a fixed part of the character: it is an **item** in its bag, and what the character shoots with is the gun it holds. A **`Loadout`**
makes that so. Its text says which items are guns and what each one is; its `held` says which slot of the bag is in the hand; every tick the character's `Weapon` (docs/COMBAT.md) is made the gun in hand, and the rounds it reloads
come out of the bag. It works in a single-player game and, with no extra code, on a game server (docs/GAME_SERVER.md), where the player says which slot is in its hand and the server's `Weapon` does the rest (with lag compensation,
docs/LAG_COMPENSATION.md).

```json
{ "uid": 2, "components": {
    "Parent": { "target": 1 },
    "Weapon": { "active": false, "spread": 0 },
    "Loadout": { "guns": "gun pistol ammo=ammo9 damage=25 rate=300 mag=12 reload=1.4 auto=0 spread=0.3\ngun rifle ammo=ammo556 damage=32 rate=640 mag=30 reload=2.4 auto=1" } } }
```

The character (uid 1) has an `Inventory` and the world has an `ItemTable` with `pistol`, `rifle`, `ammo9` and `ammo556` in it. The `Loadout` goes on the entity that carries the `Weapon` (a head, a hand) or on the character itself;
its bag is its own `Inventory`, or its parent's, or the entity `Loadout.inventory` names. `lunaris call check` names a gun or ammunition that is not an item, a line it cannot read, and a loadout with no `Weapon` or no bag.

## The text of the guns

One gun a line, `gun ITEM key=value key=value ...` (`#` starts a comment). `ITEM` is the key of an item of the `ItemTable`. Keys left out take the default of the second column.

| Key | Default | Meaning (the `Weapon` field it sets) |
|---|---|---|
| `ammo` | none | the key of the ammunition item it takes from the bag; none: it never runs out |
| `damage` | 25 | damage of one round (`damage`) |
| `rate` | 600 | rounds a minute (`rate`) |
| `mag` | 30 | the magazine (`mag_size`) |
| `reload` | 2.2 | seconds to reload (`reload_time`) |
| `auto` | 1 | 1 holds the trigger for a stream, 0 one shot a press |
| `pellets` | 1 | pellets a shot (a shotgun: 8) |
| `spread` | 0.4 | degrees of scatter at rest (`spread`) |
| `bloom` | 0.3 | degrees added by each shot while the trigger is held (`bloom_per_shot`) |
| `range` | 200 | metres (`range`) |
| `noise` | 120 | what the shot adds to the shooter's `Heat`, which the machines hear (`noise`, docs/STEALTH.md) |
| `head`, `limb` | 2, 0.7 | the multipliers for a head and for a limb |
| `falloff`, `falloff_min` | 50, 0.35 | the distance after which damage falls, and the least it falls to |
| `pierce` | 0 | the share (0 to 1) of worn armor's stopping that a hit of this gun ignores (`Weapon.pierce`, docs/EQUIPMENT.md): a sniper rifle's round goes through part of a vest |
| `kind`, `speed` | 0, 400 | 0 an instant ray, 1 a round that flies at `speed` m/s |
| `recoil`, `recoil_side` | 0.6, 0.25 | the kick of the look, up and sideways |
| `aim`, `aim_recoil` | 1, 1 | what the scatter (`spread` and `bloom`) and the kick are multiplied by while the player aims down the sights (`Loadout.aiming`); 1 changes nothing, 0.3 makes the gun three times as accurate and a sniper rifle 0.05 |

A key that is not understood, a value that is not a number and a gun defined twice are named by `check` and skipped; the other guns still work.

## `Loadout` (on the character, or its head)

| Field | Default | Meaning |
|---|---|---|
| `guns` | | the text above |
| `held` | 0 | the slot of the bag that is in the hand. A slot that is empty, holds an item that is no gun, or is beyond the bag: the hand holds nothing, and the `Weapon` is switched off |
| `weapon` | the same entity | the uid of the entity with the `Weapon`, when it is not this one |
| `inventory` | this entity, or its parent | the uid of the entity with the bag |
| `aiming` | off | the aim button: while on, the gun's `aim` and `aim_recoil` apply. A game server sets it every tick from the avatar's `NetAvatar.aim_mask` (bit 64 of the player's `flags` by default; not while a machine is controlled); a game that is not networked sets it itself |
| `holding`, `slot`, `reserve`, `took`, `mags` | | the engine's: the item in hand, the slot it is in, the rounds the bag held when the tick began, the rounds taken out of the bag so far, and the magazine of the gun of each slot |

## What happens each tick

Before the weapons (`gameplay::update`, docs/GAMEPLAY.md):

1. The slot in hand is looked up. If it holds a gun of the text and the gun is not the one held last tick, the `Weapon` becomes that gun (all the keys above), its magazine is the one **that slot** had when it was last held (a full magazine the first time),
   and its reload and its cooldown start over.
2. Leaving a gun saves its magazine for the slot, so a half empty rifle put away is half empty when it is taken out. Nothing is lost by switching, and a gun that is taken out of the bag (dropped, given, looted by someone else) is out of hand the next tick.
3. The `Weapon`'s reserve is the number of the gun's ammunition items in the bag (all stacks of it), so the display and the reload rules of the weapon are the ordinary ones. A gun with no `ammo` is `infinite`.

After the weapons: the rounds that went from the reserve into the magazine (a reload) are taken out of the bag, stack by stack from the first slot. Firing takes nothing from the bag: the magazine holds the rounds until a reload. A reload with no ammunition in the
bag does nothing.

Deterministic: characters are visited in uid order and nothing is random. The text is parsed once and cached, so a world with a hundred players costs a hash of the text and a lookup each. A world with no `Loadout` pays for one query.

## On a game server

The player's computer sends the number of the slot in its hand with its inputs, in the low 6 bits of `NetPlayer.extra` (a graph system sets it from the number keys: slots 0 to 47 fit); the server writes it to `Loadout.held` of that
avatar before the step. Everything else is the server's: the bag is the server's (docs/GAMEPLAY.md, "Inventories"), so a player cannot hold a gun it does not own. A player that says a slot beyond its bag holds nothing.

**What the others see.** Every tick the server writes the item of the gun that really is in the hand (`Loadout.holding`, 0 for none) above the slot, into the `extra` that the other players receive: on their screens the puppet's `NetPuppet.extra` is
`item << 6 | slot`. A client game shows the model of the gun with `extra >> 6` (the item numbers are the order of the `ItemTable`), and a player cannot show a gun it is not holding. Items up to 1023 fit.
The model is a child of the puppet's prefab with a `NetPuppet` of its own (the engine keeps a part's equal to the puppet's, docs/GAME_SERVER.md, "Parts of a puppet") and a graph system that sets its `MeshRenderer.visible` from `NetPuppet.extra`;
a gun lying on the ground is a pickup that `NetHost.item_kinds` gives a picture to (same page, "A picture for each item on the ground"). A survival example game does both with its guns' `world.glb`.

The example `examples/outpost.lunaris.json` (the Outpost note in the vault) uses it: the pistol in the starting bag, a rifle, a SMG, a shotgun and a sniper rifle in the crates with their rounds, number keys for the slots.

## What to supply

The guns' models (the one in the hand, the one on the ground, the one on another player's back), the sounds, the muzzle flash and the icons are the game's: the engine draws and plays none of them. A game with a held model changes it on the `Loadout.holding` of the
character (a graph system that reads the field), and on the client for the other players' puppets from `NetPuppet.extra >> 6`.

## Limits (so far)

One gun in hand at a time (no dual wielding). A gun has one fire mode and one kind of ammunition (an attachment that changes them is not done). Switching guns starts the new gun ready: a reload in progress on the old one is lost (the rounds were not
taken out of the bag yet, so nothing is lost with it).
