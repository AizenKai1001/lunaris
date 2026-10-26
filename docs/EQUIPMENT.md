# Worn gear: armor and clothing (2026-10-23)

A character's bag can have **worn places**: slots that take one kind of item and protect the character where it is hit. It is data: an `Equipment` component on the character, the items' `class`, `power` and `dur` in the `ItemTable`. Code: `crates/lunaris-sim/src/armor.rs` (the damage), `simgame.rs` `inv_move` (the server's slot rule). Tests: `armor_tests.rs` (6), `simgame_tests.rs` (`the_worn_places_of_a_players_bag_take_only_their_class...`).

```json
"Equipment": { "slots": "slot 12 accepts=1 zones=head\nslot 13 accepts=2 zones=body\nslot 14 accepts=3 zones=legs\nslot 15 accepts=4 zones=legs", "wear": 1.0 }
```

| Part | Meaning |
|---|---|
| `Equipment.slots` | one worn place a line: `slot INDEX accepts=CLASS zones=body,head,legs` (`#` starts a comment; mistakes are skipped and listed by `armor::parse_places`). `accepts=0` takes any item. `zones` are the weapons system's hit zones: `body` (1), `head` (2), `legs` (3, the legs and arms: what `Weapon.limb_mult` is for). |
| `Equipment.wear` | durability lost for each point of damage a piece stops (default 1; 0 switches wear off). |
| `Equipment.cover` | the engine's: the share of a hit that is stopped now in each zone (index 1 body, 2 head, 3 legs), kept every tick, **on a client as well** (from the bag the server told it), so that an interface can show it. |
| `Equipment.absorbed` | the engine's: all the damage the gear has stopped. |
| item `class` | which place takes it (the game numbers the classes: an example game uses 1 head, 2 chest, 3 legs, 4 feet). |
| item `power` | the percent of a hit it stops (0 to 95; 100 counts as 95). |
| item `dur` | how much damage it stops before it is used up (0: it never wears). |
| item `kind` | the `UiDrag.kind` of its card: the interface's `UiDrop.accept` can filter what is dragged to a place (the server does not rely on it). |

## What a hit does
Every hit of the weapons system (`weapons.rs`: a ray, or a flying round) goes through `armor::absorb(world, victim, damage, zone, events)` before the victim's `HitPoints` are lowered. The worn pieces that cover the zone that was hit each stop `power` percent; several pieces on one zone multiply (a 40 and a 50 stop 70 percent together). What was stopped wears the pieces by their share of it (`wear` points of durability for each point stopped); a piece whose durability reaches 0 breaks: its slot is emptied and the event `item_break` comes (on the victim). A hit on a zone nothing covers, a hit with no zone, and every other kind of damage (hunger, falls, a game's own `HitPoints.value` writes) are not changed. The damage the shot reports (`Weapon.hit_damage`) is what got through. **Piercing:** `Weapon.pierce` (also `Bullet.pierce` and the gun table's `pierce=`, 0 to 1) is the share of each piece's stopping that the hit ignores: with 0.5 a 40 percent vest stops 20 and wears by that much; with 1 the round goes through and wears nothing. `armor::absorb_piercing` is the function (`absorb` is the same with 0).

## The server's slot rule
A player's `inv_move` (the engine's own, docs/GAME_SERVER.md) is refused when it would put an item into a worn place that does not take its class, or when the swap it makes would (the item that was in the target slot goes to the slot it came from). Taking a piece off, into any carried slot, is always allowed. The places must be outside the bag's carried part, `Inventory.bag` (a bag of 16 with `bag` 12 has slots 12 to 15 for worn places): items that are picked up or crafted only go into the first `bag` slots, so they never land in a worn place by themselves. The worn pieces are part of the bag: they are saved with the character, copied into the corpse when it dies (and so can be looted), and told to the player with the rest of the bag.

## Not here
Armor piercing (a weapon that ignores part of the armor), a different protection against fire or explosions, partial protection as a piece wears down (it protects fully until it breaks), what other players SEE a character wearing is in docs/GEAR_VISIBLE.md (`WornTell`, `WornPart`), clothing that keeps warm (the game has no temperature), a weight penalty (items have `weight` already: a game can count it).
