# Loot tables: what is in the chests

Weighted loot tables, as text, that fill the engine's own `Inventory`: the same chest holds the same things in every world, a rare find can hide inside a common table, and a looted chest fills again after a while.
The items themselves are the game's `ItemTable` (`docs/GAMEPLAY.md`, "Inventories"); this decides only which ones, and how many, are where. It makes no item and no picture. Code:
`crates/lunaris-sim/src/loot.rs` (tests in `loot_tests.rs`).

## The table text

One entity of the world has a `LootTable` (the lowest uid counts):

```json
{ "uid": 2, "components": { "LootTable": { "text":
"# a clinic mostly has bandages, sometimes pills, rarely a gun\ntable clinic rolls=2-3 empty=0.1\n  bandage w=9 n=1-3\n  pills w=3\n  @guns w=1\ntable guns rolls=1\n  pistol w=3\n  rifle w=1\n" } } }
```

- `table NAME rolls=A-B empty=P`: how many times it is rolled (a number or a range), and the chance P (0 to 1) that a roll gives nothing.
- An entry is one line: `ITEM w=WEIGHT n=A-B` (an item of the `ItemTable` by key; its weight among the entries, 1 when left out; how many of it, 1 when left out) or `@OTHER w=WEIGHT n=A-B` (roll the table `OTHER`, `n` times).
  Each roll picks one entry by weight. Tables may name each other up to six deep (a loop ends instead of running for ever).
- `#` starts a comment. Lines the engine does not understand are skipped and reported by `check`.

## A chest

```json
{ "uid": 10, "components": {
    "Inventory": { "size": 12 },
    "LootContainer": { "table": "clinic", "key": 40213, "refill": 600 } } }
```

The first tick it fills the `Inventory` from the table (through the inventory's own `fill` request, so stacks, weights and published totals work as for any starting loot). **`key` is what makes it the same
every time**: give each container its own number (a building's, a room's, the position hashed), and it holds the same things in every world and every run; 0 uses the entity's uid. Each filling rolls differently
(the number of fills is part of the roll). With `refill` above 0 the container fills again that many seconds after the last filling, **once its inventory is empty** (it has been looted); a full one is left alone.
Set `filled` to false to fill it again at once. `fills` counts the fillings; the event `loot_filled` is emitted each time.

## Drops: what a destroyed thing leaves

A `LootDrop` beside `HitPoints` (and a `Transform`) drops loot where the thing is destroyed: the first tick its `HitPoints` are `dead` (the engine sets that when `value` reaches 0) it rolls the table `table` of the world's `LootTable` and makes, for every stack that comes out, a copy of the prefab `prefab` with its `Interactable.item` set to `KEY N`, in a ring of `spread` metres round the thing and `lift` metres up **from the ground below it** (a ray down from each place finds the floor, so a drone that dies 7 m up leaves its loot where it can be taken; `ground` false drops from where the thing was).

```json
{ "Name": {"value": "Drone"}, "Transform": {"pos": [0, 7, 0]}, "HitPoints": {"value": 40, "max": 40},
  "LootDrop": { "table": "machine", "prefab": "Pickup", "spread": 0.9, "lift": 0.3 } }
```

The prefab is the game's pickup (the same one a game server makes when a player drops an item: `NetHost.drop`): an `Interactable` of kind 3 that takes the item into the bag when used, and, on a game server, a `NetEntity` so that the players are told about it. The roll is the same every time for the same `key` (0 uses the thing's uid), like a container's, and it is made once (`done`; set it false to drop again). Nothing is made, and nothing is tried again, when the table, the item table or the prefab is missing (`check` says which). Event `loot_dropped` (the thing's uid and where). On a game server only the server drops: a client's own copy of a machine is replaced by the server's, which has no `LootDrop`.

A pickup of the ring never lies in a wall: the ring point is pulled in to 0.3 m in front of whatever stands between the thing and the point (a ray at the height of the thing). A single filling is limited to 20,000 table rolls (a table that rolls itself many times ends there, not minutes later), and counts and rolls may be any range up to the largest number without overflow.

## What to supply

The items (`ItemTable`: names, stacks, weights, icons) and the containers' bodies, models, opening animations and sounds are the game's; the engine makes none. A game's loot design is the table text: put the
rare and powerful things in nested tables with low weights, and give military, medical or domestic places their own tables.

## Limits (so far)

- A drop is not shared out or owned: whoever uses a pickup first takes it, and a pickup stays until taken (or, on a game server, for as long as the server keeps it).
- A chest is filled with whole stacks the inventory can hold: what does not fit (a full inventory) is lost.
- Entries cannot depend on conditions (the time of day, how many have been looted, the player's level): a game that wants that sets `LootContainer.table` to a different table by a graph system before the fill.
- No quality or condition roll for an item (a worn rifle): the items are the `ItemTable`'s own.
