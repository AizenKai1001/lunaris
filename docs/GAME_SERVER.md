# The game server: a scene's own simulation, hosted for a hundred players and more

`lunaris server --scene SCENE` runs a game's scene as the server of an online game. The server is the real simulation (`Sim`, headless): the engine's own systems and the game's scripts (weapons, building, loot,
the machines, respawn) run **once, here**, and the players' computers show what it says. Under it is the big-match netcode of `docs/NETWORK_SCALE.md` (UDP, interest management, rates by distance and rank, a hundred
and more players per server); this layer, `SimGame` (`crates/lunaris-net/src/scale/simgame.rs`), is the bridge between the two. The same scene is what a player runs:

```
lunaris server --scene scenes/game.lunaris.json --players 100 --bind 0.0.0.0 --port 7777
lunaris play   scenes/game.lunaris.json --connect HOST:7777 --name Ada
```

```
 player's computer                          server
 ┌────────────────────┐   UDP (30 Hz)    ┌──────────────────────────────────────────────┐
 │ the scene, the     │ ──────────────►  │ NetServer (interest, rates, budget)          │
 │ player's own       │   place, look,   │   ▲ replicated state     │ what players report│
 │ movement code      │   buttons,       │ SimGame ◄────────────────┘                    │
 │ NetPuppets of the  │   commands       │   │ avatars ──► Sim: Weapon, BuildTool, loot, │
 │ rest of the world  │ ◄──────────────  │   ▼               machines, respawn, ...      │
 └────────────────────┘   what is near,  └──────────────────────────────────────────────┘
                          events
```

## What the scene says

One entity of the scene has a **`NetHost`** (the lowest uid counts):

```json
{ "uid": 4, "components": { "NetHost": {
    "player": "Avatar",
    "forward": "shot 150\nbuild_placed 100\ncorpse 100",
    "static_every": 15,
    "save": "saves/world.json", "save_every": 60,
    "persist": "Transform Inventory Vitals HitPoints" } } }
```

| Field | Meaning |
|---|---|
| `player` | the prefab made for every player that connects: their **avatar** |
| `forward` | the events of the simulation the players are told about, one line a kind, `NAME RADIUS`: every player within RADIUS metres of where it happened gets it, as an event of **its own** simulation (what the sound and the effects of the game listen to); its `uid` there is the server's number of the entity it is about ("What an event is about", below) |
| `sync`, `sync_every` | names of **resources of the server's world that every client is told** (`dn.t dn.rate dn.force`, spaces, commas or lines; at most 32): see "Resources every client is told" below. `sync_every` is the most often a changed one is sent, in seconds (default 1) |
| `skip` | component names the **server leaves out** (`Ui Text`): when it starts, every entity of its world that has one of them, and everything attached to it with a `Parent`, is removed (also from a world loaded from a save). A server has no screen: a game's HUD, menus and crafting pages are hundreds of `Ui` entities that cost it memory and the time of every system that visits them. The clients run the same scene and keep their interface. An entity that itself has the `NetHost`, a `NetEntity` or a `SpawnPoint` is not removed because of a listed component. Measured on a survival scene (1,140 entities, 1,038 of them interface; a Linux test machine, a server with no player): the tick went from **2.1 to 0.9 ms** and the memory from **27.3 to 20.3 MB** (stripping only the `Ui` and `Text` components and keeping the entities gains 0.4 ms and 3 MB: the entities are what costs). Check that no server system needs an entity of the interface before you list `Ui`. |
| `static_every` | every how many ticks the still things are looked at for changes (they are copied to the clients slowly anyway) |
| `save`, `save_every` | a file the world is written to every N seconds (and when the server stops) and loaded from when it starts |
| `persist` | the components of an avatar kept **for the player, by name**, and given back to the avatar they get when they connect again (their place, their bag, their hunger). One name is one player: a second connection with the name of a player who is in is refused (the client is told "That name is already in the game": nobody can push a player out by typing their name), and the saved character is given to one avatar only. The same player coming back with a connect token before the server noticed the old connection takes over from it. On a plain server a name is all the identity there is (anyone can arrive under a name that is free, and take what that name kept): run `--secure` (docs/SECURE.md) so that the name is the one in a token. At most 20,000 names are kept |
| `occlusion`, `occlude_machines` | **wallhack defence** (docs/NETWORK_SCALE.md, "Wallhacks"): `occlusion` is the safe radius in metres, 0 (the default) is off. With a number (16 to 25 is good) a player that has a wall of the scene (any `Collider` that does not move, mesh level or terrain) between it and another, farther away than that, is not told where the other is (nor for 0.6 s after it was last in sight, nor when it is about to come into sight); `occlude_machines` hides the moving `NetEntity`s of class 4 and up the same way. Loot and the things of the world are always told. The server gathers the static world once and again about every half second (`SimStats::sight_us`) |
| `party` | the most members of a **party** (docs/PARTIES.md; 4 by default, 0 or 1 switches parties off): players invite each other by typing `/invite NAME` in the chat box, a party's members never hurt each other, talk to each other by `/p`, see each other's health and distance in a panel, share places to get up at and land, and are told about each other past walls |
| `character_days` | how many days a stored character (`persist`) is kept after its player last left: 30 by default, 0 keeps it for ever. It is let go at the next save (and when a save is loaded), so that the world's save, and the memory a save needs, do not grow with every name that ever played (about 40 to 65 KB a name: docs/SERVER_ROBUSTNESS.md) |
| `world_version` | the **layout** of the world the scene has (a number, 0 if you never use it): a save made with another `world_version` is not loaded as a world (it would lack the scene's new things and keep what the scene removed) and the world starts fresh from the scene, while the players' characters (`persist`) are still given back. The scene's graph systems and prefabs are code and always come from the scene over a loaded save, so a new version of the game's logic reaches a world saved before it without a reset: bump `world_version` only when the scene's world entities change. The scene's systems are compiled again against the loaded world; a save that does not fit the scene (its custom components differ, or it cannot be read) is not loaded: the world starts fresh from the scene (it says so on the console) and the characters are kept |

The scene is the game, so everything else is the scene's: the ground, the building rules, loot containers, the machines and their director, spawn points (`SpawnPoint`: where players start), the corpse prefab.
The entity of the scene with a `NetPlayer` (the local player's stub for the client) is removed on the server: it is the client's own.

## The avatar

The server makes an avatar for every player from the `player` prefab, puts what the player reports on it before every step, and the simulation does the rest. A prefab for it:

```json
{ "components": {
    "Name": { "value": "Avatar" }, "Transform": {},
    "CharacterBody": { "radius": 0.35, "height": 1.8, "gravity_scale": 0 },
    "HitPoints": { "value": 100, "max": 100 },
    "Respawn": { "delay": 5, "corpse": "Corpse" },
    "Inventory": { "size": 24 }, "Vitals": {}, "Team": {},
    "NetAvatar": { "fire_mask": 1, "reload_mask": 2, "sprint_mask": 4 } },
  "children": [ { "components": {
    "Name": { "value": "Head" }, "LocalTransform": { "pos": [0, 1.6, 0] },
    "Weapon": { "damage": 25, "range": 150 }, "BuildTool": { "reach": 6 }, "Interactor": { "reach": 3, "look": false } } } ] }
```

- The avatar's `Transform` is the player's **feet** and its yaw; `CharacterBody` (with `gravity_scale` 0: the player's own computer moves the character, the server does not) is what a shot is tested against.
- The **head** is the child that carries the tools (`Weapon`, `BuildTool`): the engine finds it when the avatar is made, puts the player's pitch on its `LocalTransform` and so the tools aim where the player looks, from the
  eyes. Without a head the tools sit on the avatar and aim from its feet.
- The buttons are the bits of the `flags` the player reports (`NetPlayer.flags`): `fire_mask` pulls the trigger of the avatar's `Weapon`, `reload_mask` reloads, `sprint_mask` sets `Vitals.sprint`, `crouch_mask` sets `Stealth.crouch` (docs/STEALTH.md), `aim_mask` (bit 64) sets `Loadout.aiming`, which narrows the held gun's scatter and kick by the gun's `aim` and `aim_recoil` (docs/LOADOUT.md), `use_mask` presses
  the use button of the avatar's `Interactor` (docs/INTERACT.md: doors swing, containers open, items are picked up, all decided here; put the `Interactor` on the head with `"look": false`).
  What else a bit means (crouch, use, lean) the game's scripts read from `NetAvatar.buttons`.
- `NetAvatar.held` is the number the player says it holds (`NetPlayer.extra`), `pitch` its look: for the game's scripts too. With a `Loadout` (docs/LOADOUT.md) the low 6 bits of that number are the slot of the bag in the hand and the server writes the item of the gun there above them for the other players.
- Health is the avatar's: `HitPoints` goes out as the player's health (a byte, 255 is full, 0 dead); a client's claim of health is not believed. A shot is the avatar's `Weapon` firing in the
  simulation, so it is decided here: from where the avatar is now.
- **What a player is told about itself** (the `STATUS` message, 12 bytes): the server's vitals, heat and gun are not on the client, and an interface needs them. When anything changes (at most five times a second, at once when the gun or the magazine does) the
  player is told its health, food, water and stamina, the director's heat, whether it bleeds, is exhausted or reloads, the item of the gun in its hand and the rounds in the magazine and in its bag; the client sets `NetPlayer.health`, `food`, `water`, `stamina` (0 to 1: the `fill` of a `Ui` bar,
  docs/UI.md), `heat` (0 to 10), `tier` (the Core's alert tier for this player, 0 to 7: bits 3 to 5 of the `STATUS` flags, `docs/WALKER.md`), `bleeding`, `exhausted`, `reloading`, `gun`, `mag`, `reserve` (65535: never runs out). A game's HUD is graph systems that copy them into its `Ui` entities (the outpost's is one: bars, the ammunition counter and the heat meter). `Vitals`, `Heat`, a `Weapon` (with a `Loadout`, docs/LOADOUT.md) are what it reads: an avatar without them reads 0.
- **Respawn** is the engine's `Respawn` (`docs/RESPAWN.md`): the corpse is a `NetEntity`, and the player who gets up somewhere else is told to go there (the client's `NetPlayer.corrections` counts it).
- A player who joins starts at a `SpawnPoint`, chosen like a respawn (far from the others), or at the origin without any.

## What the players see: `NetEntity`

A thing of the simulation that the players must see carries a **`NetEntity`**: `class` (1 an item on the ground, 3 a thing in the world: both **still**, sent slowly; 2 something in flight; 4 and up things that move:
machines, vehicles, sent like players), `kind` (what it is, a number) and optionally `flags`, `anim`, `anim_t`. Every tick (still things every `static_every` ticks) its `Transform`, its `HitPoints` as a
byte, those fields and its `kind` are copied into the network's world, and the clients near it are sent them by distance and rank, as for players. The engine adds one to every **piece** the building system makes
(class 3, kind = the piece's number in the rules from 1) and a prefab with its own keeps it; the corpse prefab, the machines, the loot bags, a helicopter get theirs from the game.

On the client the entity with the `NetPlayer` says how to show them, in `kinds` (a line per kind, `CLASS KIND PREFAB`, `KIND` `*` for any):

```json
"NetPlayer": { "puppet": "Mate", "kinds": "3 1 FoundationModel\n3 9 CorpseModel\n4 * SpiderModel\n1 * LootBag" }
```

Each thing becomes an entity made from that prefab with a **`NetPuppet`** (`id`, `class`, `kind`, `health`, `flags`, `anim`, `anim_t`, `speed`), moved smoothly between the server's records and removed when it goes (a still thing
that nothing has said for five seconds, a moving one for two). A thing with no line is not shown. A `NetEntity` that the *client's* copy of the scene has is removed when the client connects: the server's copy counts.

**A picture for each item on the ground.** A pickup's item is text inside its `Interactable` (`rifle 1`), which the clients are never told, and one `Pickup` prefab (the dropped item's: `NetHost.drop`) has one `kind`. `NetHost.item_kinds` is the way to
show a rifle as a rifle: a text with a line `ITEM KIND` for each item key (`pistol 33`, `rifle 34`; `#` starts a comment). A pickup (an `Interactable` of kind 3 with a `NetEntity` of a still class) that holds such an item is told to the clients
with that `kind` instead of its own, whatever made it (the scene's loot, `inv_drop`, a script), and the client's `kinds` has a line for it (`1 33 PistolOnTheGround`). It is worked out when the still things are looked at, so it follows the item: a stack that is partly
picked up keeps its picture, a pickup whose item a script changes is shown by the new item's prefab (the client makes the puppet again when the `kind` it was made for is no longer the server's), and a saved world shows the pictures of the text it is loaded with. An item with no line keeps the
pickup's own `kind`. The server's `NetEntity.kind` is not changed. A dropped item (`inv_drop`) lies turned the way the player was looking, so that a model of a gun points where it was dropped. `check` names a line that is not `KEY KIND`,
an item that is not in the `ItemTable`, and a kind that no `NetPlayer.kinds` line of class 1 or 3 shows.

**Parts of a puppet.** A prefab of a puppet (the player's `puppet`, or a thing's in `kinds`) may have children, and a child that has a `NetPuppet` of its own is a *part*: every tick the engine gives it the puppet's `NetPuppet` (all the fields: `id`, `flags`,
`pitch`, `health`, `extra`, `speed` ...). A graph system cannot read its parent's fields, so this is how a piece of the body is dressed by what the player is told: a gun model in the hand that is shown while `NetPuppet.extra >> 6` is its item
(docs/LOADOUT.md, "What the others see"), a name tag, a light that is lit while a flag is on. The parts go with the puppet.

## What the player says

- **Place, look, buttons** go up thirty times a second as for any big-match client (`NetPlayer.flags`, `pitch`, the `Transform`'s yaw and place). The server refuses a move the character could not make and puts the player back.
- **Commands** are said by setting `NetPlayer.cmd` (a name) and `cmd_args` (four numbers) for one tick; the engine sends them reliably and clears them. Two are the engine's:
  `build:KIND` chooses what the avatar's `BuildTool` builds (a piece of the rules) and `place` builds it (`docs/BUILDING.md`: the tool aims where the player looks); any other name is put on the avatar as
  `NetAvatar.cmd` and `args` for **one tick**, for the game's scripts (use, drop, craft, take from a container).
- A graph system cannot set a string, so a command can also be said by **number**: `NetPlayer.acts` is a text with a line `NUMBER NAME` for each (`1 build:foundation`, `3 place`; `#` starts a comment) and
  setting `NetPlayer.act` to a number says the command of that line, with the `cmd_args` as they are. A name set in `cmd` wins over a number; a number no line has says nothing. Either way the command is said once and
  `cmd`, `act` and `cmd_args` are cleared. `examples/outpost.lunaris.json` binds the keys 1, 2, F and G this way (below).
- The player's own computer can run the `BuildTool` itself against the replicated pieces (give the pieces' prefabs a `BuildPiece`) to show where the piece would go; the server decides.

## Chat (2026-10-23)

Text chat between the players of a server, told by distance. `NetHost.chat` is how far, in metres, a message is told: to every player within that distance of the speaker (the speaker included); **0, the default, switches chat off** (the server drops what is said), a very large number is a chat for everybody. Nothing else is needed on the server.

On the player's computer (the client's scene):

* `NetPlayer.say_input` is the uid of a `UiInput` (the chat box). When the player presses Enter in it, the engine sends its text to the server and empties it. The game opens and focuses the box (a graph system sets `UiInput.focused`) and closes it when `UiInput.submitted` is true or the focus is gone; while the box has the focus the game's actions read 0 (docs/UI.md, "Typing"), so the letters typed do not walk, reload or drop things.
* A `Text` (or `Ui`) entity with a **`NetChat`** is the log: the engine writes the last `lines` messages that are not older than `life` seconds into its text, one to a line, `name: text`. `NetChat.count` goes up by one for every message that comes in (a game plays a sound by watching it).

What the server does with a message (`simgame.rs`, `SimGame::say`): the text is cleaned (UTF-8, printable characters only, runs of white space made one space, trimmed, at most 120 characters), an empty result is dropped, and each player may say three things at once and then one more every 1.2 seconds (a token bucket, so a loop of `say` cannot flood the others); the speaker's name is the one the player joined with (a secure server: the name in its token), cleaned like the text (a line break in a name cannot make a second line that looks like another player's), so nobody can speak as another; the message goes to the players near the speaker's avatar as a reliable `CHAT` message (kind `0x49`; the player's `SAY` is `0x05`). Tested in `simgame_tests.rs`: the cleaning, the distance (the speaker and those within it hear, a player farther away does not), the field emptied, empty and control-only messages dropped, the 120-character cut, the burst and the credit coming back, chat off by default, a line leaving the log after its life.

Not here: whisper, mute and block, a list of banned words, a log kept on the server, the speaker's place on a map. They are a game's rules on top. **Parties (2026-10-23, docs/PARTIES.md)** add a channel for a player's party (`/p text`, or `NetPlayer.say_party_input`: a channel byte after the kind byte, `[0x05][0x01][text]`; it reaches the members wherever they are as `[PARTY] Name: text`), invitations by name, a panel of the members (`NetParty`), `Team`s that never hurt each other, shared places to get up at and shared land, and members that walls do not hide from each other; `NetHost.party` is the most members (4; 0 switches it off).

## Bags and containers

What a player carries and what is in the container it has open are told to **that player only**, whenever they change (a reliable message of at most a few hundred bytes), and nobody else hears of them:

- the player's own `Inventory` on the client (the entity with the `NetPlayer`) is kept equal to the avatar's, so the interface kit's `InvSlot`s bound to it show the bag as in a game for one player;
- an entity of the client's scene with a **`NetConnect`** lets the game connect from its own main menu (an address field, a Connect button, `status` and `message` for the screen; docs/UI.md, "Going online from a menu") instead of `--connect HOST:PORT` at start;
- an entity of the client's scene with a **`NetContainer`** and an `Inventory` is the container the player has open: when the server says one is open its contents are written into that inventory and `open` goes on
  (`source` is the server's number for it); when it shuts (the player uses it again, or walks off, or it is gone) `open` goes off and the inventory is emptied. Bind the window's `InvSlot`s to that entity.

Things are moved by commands (`NetPlayer.cmd` and `cmd_args`), decided here:

| Command | Arguments | Does |
|---|---|---|
| `inv_move` | `from`, `to`, `count` | a place is 100 times the bag (0 the player's own, 1 the container it has open) plus the slot; `count` 0 moves the whole stack. Into an empty slot, onto a stack of the same item up to its stack size, or a whole stack swaps with another item |
| `inv_drop` | `slot`, `count` | puts some of the bag on the ground in front of the player as the `NetHost.drop` prefab (a pickup: an `Interactable` of kind 3 and a `NetEntity`; the engine gives it the item) |
| `inv_craft` | the recipe number | asks the avatar's `Inventory` to craft that recipe (docs/GAMEPLAY.md, "Inventories") |
| `inv_craft_start` | the recipe number, how many (1 to 99) | starts a **timed craft** (a recipe with `~ SECONDS`): the server takes the ingredients, counts the seconds and makes the item, and tells the owner the craft in hand with the bag; a recipe without a time is made at once, that many times |
| `inv_craft_cancel` | none | ends the craft in hand and gives its ingredients back |
| `inv_use` | `slot` | uses the item in that slot of the player's bag as `NetHost.uses` says (below): a food, a drink, a bandage, a medical kit, a tool. Nothing happens for an empty slot, an item with no rule, or a dead player |

**Using items.** `NetHost.uses` is a text with a line for each item that can be used, `use KEY eat=30 drink=0 heal=0 bandage=1 keep=0` (`KEY` the item's key in the `ItemTable`; `#` starts a comment): the numbers are added to the avatar's `Vitals` requests
(`eat`, `drink`, `heal`, `bandage`, docs/SURVIVAL.md: the survival system does them in the next tick) and one of the item is used up unless `keep` is 1 (a tool). The player is told its bag again either way.
The outpost has `use can eat=35`, `use water drink=40`, `use bandage heal=5 bandage=1` and `use medkit heal=60 bandage=1`; the key U uses what is in the hand.

Picking things up, searching a container and opening a door are the use button (`NetAvatar.use_mask`, docs/INTERACT.md).

**Dragging in the interface needs no code.** When the client is connected (`NetPlayer.connected`) the engine's own slot and card dragging (`InvSlot`, `InvCard`, `UiDrag`, docs/GAMEPLAY.md, "Inventories") also sends the move: a card dropped on another slot of the player's bag or
of the open container sends `inv_move` (from, to, whole stack) and a card of the player's own bag let go over a `UiDrop` that is not a slot (the ground) sends `inv_drop`. The client's copy shows the move at once; the server decides, and **always tells the player the
bag and the open container afterwards, even when it refused**, so a refused move jumps back. Slots of any other inventory are the client's own business and send nothing; offline nothing is sent. Bind the window's `InvSlot`s to the entity with the `NetPlayer` (the bag) and to the one with the
`NetContainer` (the container) and give the client's scene the game's `ItemTable` (it moves stacks by its stack sizes). Two moves in the same tick send only the last.

## Events

The events of the simulation that `forward` lists reach the players near them (a reliable message with the name and the place); `NetDriver` makes each an **event of the client's own simulation** (`scripts.events`),
so the sounds, the particles and the camera shake of the game need no network code: `shot`, `hit`, `kill`, `build_placed`, `build_collapse`, `corpse`, `respawned`, `heli_crash` are events the engine's systems
emit already.

**What an event is about.** The `uid` of a forwarded event on the client is the **server's number** (the one `NetPlayer.id` and `NetPuppet.id` hold) of the entity the event is about, 0 when that is not a replicated thing: for `hit` and `kill` the victim, for `shot` the shooter (the avatar, not its gun), for the events of a machine or a vehicle that thing. So a client reads `event.hit.uid` (or `.0.uid` to `.3.uid` of the several of a tick, docs/GAMEPLAY.md) and compares it with its own `NetPlayer.id`: equal is "I was hit", different is "someone near me was hit" (and `event.shot.uid` equal to `NetPlayer.id` is its own shot). Before this the uid on the client was always 0. The number is written after the place as a variable-length integer (one to five bytes, nothing at all for 0), at the end of the message: an old client stops reading before it and a new client reads an old server's message as 0, so the protocol version did not change (`simgame::event_about`, `parse_event_about`).

**Raiding** (`docs/RAIDING.md`) is told this way: the blasts (`blast_TAG`, with a radius that is how far the bang is heard), `build_destroyed`, `build_collapse` and `died` (so that a client can take away at once the puppet of what the server broke: a client forgets what the server stopped telling of only after seconds, and a destroyed entity has net id 0 by then, so the event's **place** is what matches), and the player's own answers (`lock_prompt`, `lock_bad`, `build_upgrade_ok`, `throw_refused_WHY`, ...: events about the player's head, which the server maps to the avatar's number: a radius of a few metres is enough). A grenade's fuse is `NetEntity.anim_t`, a lock is bit 2 of `NetEntity.flags`, a tier is another `NetEntity.kind` (the client makes the puppet again from the prefab `NetPlayer.kinds` names for it).

## Resources every client is told

`NetHost.sync = "dn.t dn.rate dn.force"` lists resources of the **server's** world that only the server decides and every client only shows: the world clock, the weather, the zone's radius. The server looks at them every tick and sends

* the ones that **changed** since the last message, to every client, at most once every `sync_every` seconds (default 1; 0 is every tick that changed something): a clock that changes every tick is one small message a second, a rate that never changes is never sent again;
* **all** of them again every 5 seconds whether they changed or not (a refresh: a client whose copy drifted, or whose scene was reloaded, is set right within a few seconds);
* **all** of them at once to a client that has just joined (it does not wait for the next change).

The client's `NetDriver` writes each into the client's own simulation as a resource of the same name (`$dn.t` in a graph system, `{dn.t}` in a text); the client's scene needs nothing, not even the `NetHost`. A resource that does not exist on the server yet is skipped until it does, and the names of the engine's own resources (`input.`, `action.`, `game.`, `scene.`, `time.`) are never sent or accepted. The message is reliable and lazy (it never makes a quiet client's packet come sooner): `0x48`, the number of resources, then for each a name (one length byte and the bytes), a tag and the value, written as one byte for a whole number from 0 to 255, four for a value an `f32` holds exactly (7.25) and eight otherwise (an `f64`: 13.2371): a clock and its rate and its forced weather are about 30 bytes the first time and about 14 once a second after. The value is **not timestamped**: a client is behind the server by the one-way latency (a game that cares extrapolates with its rate); this replaces the use of an event's place as a number channel. Code: `crates/lunaris-net/src/scale/netsync.rs`; tests: `netsync_tests.rs` and `simgame_tests.rs`.

## Saving

With `NetHost.save` the world is written to that file (a workspace path or an absolute one) every `save_every` seconds, when the server stops, and at `SimGame::save_now`; the avatars are not in it. What `persist`
names is kept by the player's **name** (the name the client connects with; with `--secure` it is the name in the connect token, docs/SECURE.md) in the same file, and an avatar made for a player of that name gets those components back (a
`Transform` brings them back where they stood). At the start the server loads the file if it is there. A periodic save **copies the world on the server's thread and writes it on a thread of its own** (`SimGame::save_in_background`;
one at a time): for 100,000 entities the copy takes 2.5 ms and the writing (building the text, 78 MB, and the file) a third of a second, which no tick feels. The file is written beside the save and renamed over it, so that a crash never leaves half a
save. The save when the server stops (`save_now`) waits for any save that is being written and then writes its own.

## What it costs

`lunaris server --scene` prints, every five seconds, the server's tick and, from the game, the microseconds a tick spent putting the players' input on the avatars, **stepping the simulation** (two steps a tick: 60 steps a
second at 30 ticks) and replicating. The simulation is the part that grows with the game; the input and the replication are small (the replication scans the entities with a `NetEntity` each tick, the still ones
every `static_every` ticks).

Measured on 2026-10-21, real UDP on one machine, release build, the scene of the tests (a ground, the building rules, a crate, two spawn points, the avatar with a weapon and a build tool) with
**200 swarm bots** that all start at the two spawn points, walk about and (through their `flags`) shoot each other with the avatars' weapons, so the simulation has a gunfight, deaths, corpses and respawns going on:
the server's tick is 4.5 ms on average and 6 ms at worst of the 33 ms there is, of which **the game is about 0.75 ms** (input on the avatars 40 us, the simulation's two steps 600 us, replication 90 us) and the rest is the
network layer (ranking and writing 200 packets 1 ms, handing them to the operating system 2.6 ms), 5.3 MB/s out for 200 clients (27 KB/s each), 0.2 MB/s in. The things replicated grew from 100 to about 900
over the 40 seconds (corpses: they stay a minute) without the tick growing noticeably.

**What the engine paths a server runs every tick cost, measured again** (2026-10-22, the survival scene with 200 players that walk about, `cargo test --release -p lunaris-net --lib game_phase_bench -- --ignored --nocapture` with `SURVIVAL_SCENE=...`: the game's part of a tick, in memory, without the network; `BENCH_SIMPROFILE=1` adds the stages of the simulation): the game's part of a tick
took 2.72 ms and takes 1.92 ms (-30 percent; the simulation's two steps 2.50 ms and 1.70 ms; the slowest of 450 ticks 3.8 ms and 2.7 ms), and the state of the simulation after 450 ticks is bit for bit the same (`c684ae5d947eb166` with 200 avatars, before and after, in every run; so is the state hash of each of the 50 example scenes after 400 steps and of the survival scene for three seeds, with the old binary and the new one). `lunaris server --scene S --profile-sim`
prints the stages of the simulation (ms a step) when the server stops, with the blocks of `gameplay` listed one by one (`gameplay loadout::before`, `gameplay perception::update`, ...), and found five things that were not the work of the game at all:

- **The text a table is parsed from was found again by hashing it** a byte at a time, on every call, by every system that touches a bag, for every character (the item table, the guns of a `Loadout`, the loot tables, the building rules): 200 characters' `loadout::before` took 0.14 ms a step, a third of it hashing the guns' text 200 times. They are found again by comparing the text now (a read of memory a quarter of the cost, and no collision to hope for): 0.06 ms.
- **A query looked at every archetype** (about 60 in this scene, of which most have nothing to do with it) for the components it needs: each archetype now has a 64-bit mask with a bit for each of its components, and `World::matching_archetypes`, `for_each` and `World::chunks` skip the ones that lack a bit the query needs before they look closer (ids share bits, so it can only make a query look closer, never miss an archetype: `a_query_finds_the_same_archetypes_as_looking_at_every_one_even_with_more_components_than_a_mask_has_bits`).
- **A graph system that no entity matches was made ready to do nothing**: most of a scene's 200 systems are for things that are not there at the moment (on a server, the whole interface it has stripped with `NetHost.skip`), and each was given a list of its archetypes twice, a list of chunks and of resources and a copy of its name before running over nothing. It now stops after the first look (a system that is a plugin does not: it may report an error for its own sake), and the list of archetypes is made once for the chunks.
- The set of the uids of what moves, made for each gathering of the world for rays (perception, weapons, interaction, characters: each every step), is a hash set of its own kind and not a `BTreeSet` (25 microseconds for 200 characters, now 3).
- The periodic save (`NetHost.save_every`, 60 s) copies the whole simulation on the thread of the server and held that tick up for 3.7 ms with 200 players (4 to 11 ms in a live run on a busy machine): 0.3 ms was the world, 2.4 the 200 systems' definitions, deep JSON trees copied one by one. `ScriptSystem::def` is an `Arc<Json>` now, a copy shares it, and the hold-up is 1.2 ms (it still grows with the world: 2.5 ms for 100 000 entities). The writing of the file is on a thread of its own.

Left, by the same profile (ms a step with 200 players): `characters` 0.16 (it builds the list of the world's obstacles every step), `gameplay perception::update` 0.09 (four drones look at 200 targets, and gather the static world each step), `gameplay inventory::update` 0.08 (every bag is copied and its weight and what can be made worked out again every step: 0.4 microseconds a bag; skipping the ones with no request that have not changed needs the bag's contents kept somewhere to know it, and costs about what it saves), `gameplay loadout::before` 0.06, `gameplay weapons::update` 0.04, `physics` 0.05; the 200 graph systems together are about 0.15.

## Try it: the outpost

`examples/outpost.lunaris.json` is a small greybox online game on this layer, made by `tools/build_outpost.py` (read it to see every line of the scene): an 80 m yard with a ring of eight spawn points near the walls, a hut with a door that
swings, six crates with loot (`LootTable`), four things on the ground, a helicopter and a family car (docs/SEATS.md), a lighthouse in the middle (a `Searchlight` 35 m up that sweeps a ring 9 to 23 m round the tower: a player it holds for 0.8 s is detected, the `Director` raises their `Heat` and sends hunters, flying machines made from the `Hunter` prefab; the hut's roof is cover), two crawlers on the ground (low machines: a `NavAgent` walks them round the obstacles to a player they notice within 25 m, a shut door is a wall to them, and a weapon on a child entity at the height of their mouth bites), two armed drones that patrol (`FlyAgent` driven by `Perception`: they see a player in the open from 18 m, circle and shoot, and kill one in about five
seconds; the middle of the yard is theirs, the spawn ring is out of their sight), the avatar with a bag that starts with a pistol and 24 rounds (its `Weapon` is the gun in the hand: `Loadout`, docs/LOADOUT.md; five guns and four kinds of ammunition are in the crates), a `BuildTool` and a use reach, and the first-person controller of `examples/first_person.lunaris.json` with the network added. Everything is the engine's own
cubes and spheres; the prefabs named in `NetPlayer.kinds` are what a real game replaces with its own models.

```
lunaris server --scene examples/outpost.lunaris.json --players 16
lunaris play   examples/outpost.lunaris.json --connect 127.0.0.1:7777 --name Ada
```

| Key | Does |
|---|---|
| W A S D, mouse, space | walk, look, jump (the client's own character controller) |
| click, R | fire the gun in the hand (`fire` is bit 1 of the `flags`), reload it (bit 2: the rounds it takes leave the bag) |
| Q | take the next gun of the bag in the hand (`NetPlayer.extra`, the slot: the others see the gun, `NetPuppet.extra >> 6`) |
| shift, ctrl | sprint (bit 4), crouch (bit 16: quieter and harder to see, docs/STEALTH.md) |
| E | use: open the door, search a crate (its contents appear in the `NetContainer` inventory), take a thing from the ground (bit 8) |
| 1, 2 | choose the foundation or the wall, then F builds it where the player looks (`build:KIND` and `place` as numbered acts) |
| E at the car | sit in the driver's seat. W throttle, S brake then reverse, A D steer, Ctrl brake, Space handbrake, E again once it is below 4 m/s gets out (the handbrake is put on). The greybox car's body is low: the seated player stands up above it, so the drones can shoot the driver |
| E at the helicopter | sit in the pilot's seat (the rotors spool up for four seconds). Then W S pitch, A D the pedals, Space Ctrl collective up and down, Z C roll, mouse looks; E again on the ground gets out (not in the air). The rotors are loud: the drones come to it and wear it down |
| G | drop one of what is in the hand (the slot in `NetPlayer.extra`) on the ground (`inv_drop`) |
| U | use what is in the hand (`inv_use`: a can is eaten, a bottle drunk, a bandage stops the bleeding; `NetHost.uses`) |
| H held on a drone (within 25 m, in the cross hairs, in the line of sight) | hack it (3 s at the goggles' speed 2: 1.5 s); then it is yours for up to 40 s: the camera is at the drone, W A S D Space Ctrl fly it in the way you look, click fires its gun, R lets it go. Your own body stands where it was, defenceless; hacking leaves a trace (heat) |

`lunaris play` loads scenes only from under its working directory, so run both commands from the repository root, or from the folder that holds the scene.

**What was checked on the real binaries** (2026-10-21, `tools/outpost_smoke.py`: a server and two hidden clients, each driven through its `--api` port): both players connect and see each other and the 13 replicated things
(door, crates, things on the ground, drones); a use at the door opens it for both; a use at a crate fills the client's container inventory, `inv_move` carries the stacks into the bag and a second use shuts it; `inv_drop`
puts a thing on the ground that the other player sees; a shot lowers the health the other player's client shows; a `build:foundation` and `place` puts a piece where it is seen by the other; the goggles take a drone the player aims at, the camera goes to the drone and comes home when it is given back (a third run of the scene with one drone); the drones take a player standing in
the open from full health to dead in about five seconds, a corpse stays and the player gets up at a spawn point far from the other (a refused move and a respawn are both a correction: `NetPlayer.corrections`);
two crawlers notice a player and walk up to it and bite, and cannot reach a player in the hut behind its shut door (a run with only the crawlers); the dead player's bag is on the corpse, which the player searches and takes it back from; the player sits in the car, drives it off, is refused an exit at speed, brakes and gets out beside it; the player sits in the helicopter, flies it up 14 m (the other player sees both climb), is refused an exit in the air, brings it down on the keys and gets out; and, in a second run of the scene without the drones, the lighthouse's
beam sweeps (its puppet turns on the clients), a player under the hut's roof and one out of the beam's reach are left alone through a whole turn of the beam, and a player who stands in the beam is detected and a hunter appears.
The scene is a test of the mechanisms, not a game: the drones are deliberately deadly, hide in the hut.

**Machines** (2026-10-21, the same measuring with **machines** added to the scene: flying drones with a `FlyAgent` driven by `Perception` and crawlers with a `NavAgent`, 200 players on top): with 300 drones and 100 crawlers the game's simulation took
3.6 ms a tick (the whole tick 9.3 ms of the 33 there is); with 1000 drones and 300 crawlers 9.4 ms (the tick 17.8 ms, still 30 ticks a second). Before the three changes below it was 4.9 ms and **35 ms** (the server could not keep up: 22 ticks a second). What
was done, each by a profile of the gameplay blocks first and each bit for bit the same result (the state hash of 1300 machines and 200 players after 400 steps did not change): the `FlyAgent`'s ray casts and every other
ray of the engine now go through a **grid over the solids** (`RayWorld`: a ray looks only at the solids in the cells it passes through, in the same order as before; with fewer than 48 solids nothing changes), a `FlyAgent` finds its neighbours to keep apart from
in a grid of its own instead of looking at all the others, and a `Perception` looks only at the targets of the other teams instead of at all of them and skipping its own side.

## What to supply

The scene (the world, the rules, the avatar and corpse prefabs, the spawn points), the client prefabs for what is replicated, the game's scripts for its own commands, and what the players see and hear
(the events). The engine makes no picture or sound here.

## Limits (so far)

- **Wallhacks**: with `NetHost.occlusion` off a modified client is told where every player within 150 m is, through any wall. With it on (docs/NETWORK_SCALE.md, "Wallhacks") it is told only what its character could see, or is within the safe radius,
  or is about to see; loot, machines (unless `occlude_machines`) and the things of the world are still told at their rates, and nothing here hides **sound** (a game that wants footsteps through walls to be fair sends events by distance and must not send ones the listener cannot hear).
- **Shots are lag compensated** (docs/LAG_COMPENSATION.md): a hitscan shot is tested against the characters and moving things where the shooter saw them (half its round trip, one tick and `NetHost.interp` ago, at most
  `NetHost.lag_comp`). Flying rounds, doors and building pieces are not rewound, and the muzzle and aim are what the client reports when the server handles the shot.
- **Names are the identity** on an open server: whoever connects with a name gets what that name kept. Run it **secure** (`--secure KEYFILE`, docs/SECURE.md) and only a player with a connect token gets in, as the name in the token, with everything sealed.
- The server runs one simulation on one thread: the replication is cheap, the simulation is the budget. A thousand machines with `Perception` cost what they cost on a single player's computer.
- **Goggles** (docs/AI.md, hacking): a player aims at a machine, holds the hack key (`NetAvatar.hack_mask` 32), takes it, sees from it (`NetPlayer.viewing`, `camera`, `eye`) and flies it with the keys (`NetPlayer.axes`); the `VIEW` message tells the player.
- **Vehicles** (docs/SEATS.md): a player can sit in a helicopter of the server's and fly it (`Seat`, `Controls`, `NetPlayer.axes`, the `CONTROL` and `RIDE` messages); other vehicle types, gunner seats and the cabin's tilt on the other screens are not done.
- A client's own drag in the inventory window moves its copy at once and is also sent to the server (above); the server's next message makes it true. Equipment slots and hotbar are slots of the same inventory (the engine treats them like any other).
