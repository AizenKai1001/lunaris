# Building: pieces that snap, stand, fall and decay

Rust-style base building as an engine addon: a text of **rules** says what kinds of piece exist and where they join, the player's **tool** aims along its entity and says where a piece would go and whether it may,
`place` builds it, and the engine keeps every piece's **stability**: how well the ground holds it. A piece that nothing holds up collapses; a destroyed piece takes with it what hung on it; pieces decay unless a
cupboard pays for them; pieces can be built onto what a generator made. It makes no model, plays no sound and draws no ghost: the pieces' prefabs, their rubble and the translucent preview are the game's.
Code: `crates/lunaris-sim/src/building.rs` (tests in `building_tests.rs`, which are also the examples below).

## The five components

| Component | On | What it is |
|---|---|---|
| `BuildRules` | one entity (the lowest uid counts) | the rules, as text |
| `BuildTool` | the entity that looks: a camera, a hammer in the hand | what the player is building, where it would go, `place` |
| `BuildPiece` | every piece that has been built | its kind, owner, `stability`, `anchored` |
| `BuildSocket` | a point on something that is not a piece (a generated building) | a place pieces may be built onto |
| `BuildUpkeep` | a cupboard | land that is somebody's, and the upkeep that stops decay |

## The rules text

```
rules snap=1.5 min=0.2 gap=0.05 slope=0.7 step=90 ground=0.6 audit=1
piece foundation size=4,0.5,4 hp=500 free=1 anchor=1 decay=1
  socket n at=0,0.25,-2 yaw=0   type=edge mates=edge,wall_base carry=0.8,wall_base:0
  socket s at=0,0.25,2  yaw=180 type=edge mates=edge,wall_base carry=0.8,wall_base:0
  socket e at=2,0.25,0  yaw=-90 type=edge mates=edge,wall_base carry=0.8,wall_base:0
  socket w at=-2,0.25,0 yaw=90  type=edge mates=edge,wall_base carry=0.8,wall_base:0
piece wall size=4,3,0.2 hp=200 inset=0.12 decay=1
  socket base at=0,-1.5,0 yaw=0 type=wall_base mates=edge carry=0.9 turn=0
  socket top  at=0,1.5,0  yaw=0 type=wall_top
piece ceiling size=4,0.2,4 hp=200
  socket a at=0,-0.1,-2 yaw=0   type=ceil mates=wall_top carry=0.9
  socket b at=0,-0.1,2  yaw=180 type=ceil mates=wall_top carry=0.9
piece table size=1,0.8,1 hp=50 free=1 loose=1
```

`#` starts a comment. Indentation does not matter; a `socket` belongs to the `piece` above it. Everything is metres and degrees; a quarter turn to the left is +90 (a thing faces its local -z, forward, at yaw 0).

**`rules`** (all optional, the values above are not the defaults but the test set):

| Key | Default | Meaning |
|---|---|---|
| `snap` | 1.2 | how near (m) the aim must be to a socket for the piece to snap to it |
| `min` | 0.1 | the stability under which a piece collapses (0 to 1) |
| `gap` | 0.05 | metres every piece's box is shrunk by when two are tested for overlap, so that pieces that meet face to face do not count as overlapping |
| `slope` | 0.7 | the least `normal.y` of ground that can be built on (0.7 is about 45 degrees) |
| `step` | 15 | a free piece turns in steps of this many degrees (0: any angle) |
| `ground` | 0.5 | how far (m) under a piece the ground may be and still hold it up |
| `audit` | 1 | every how many seconds the world is looked over (decay, cupboards, pieces changed by other means) |

**`piece NAME`**:

| Key | Meaning |
|---|---|
| `size=X,Y,Z` | the piece is a box of this size centred on its entity (the same way a `Collider` box is), turned about the vertical axis only |
| `hp=N` | its hit points when built (`HitPoints`); 0 keeps the prefab's own, and a bare piece with none cannot be destroyed |
| `free=1` | can stand on open ground, with no socket (a foundation, a table); without it the piece needs a socket |
| `anchor=1` | a piece resting on the ground holds itself up and what is joined to it (a foundation) |
| `loose=1` | furniture: needs no support, is not part of the structure, and falls when what it was put on goes |
| `decay=D` | hit points lost a second while no cupboard pays for it (0: never) |
| `inset=G` | this piece's own overlap shrink (walls need more than their half thickness so that two meet at a corner) |
| `prefab=P` | the prefab spawned for it (with `Transform`, `BuildPiece` and `HitPoints` filled in); without one a bare solid box is made |
| `debris=P` | a prefab spawned where it fell or was destroyed |
| `bullet=S` | the share of a bullet's damage that gets through to it (0 to 1; the piece gets a `Resist`), `upgrade=PIECE` the piece it becomes when a player upgrades it and `upgrade_cost=ITEM:N,...` what that takes out of the bag: the tiers of `docs/RAIDING.md` |

**`socket NAME`** (a place on the piece where pieces join):

| Key | Meaning |
|---|---|
| `at=X,Y,Z` | its place in the piece's own axes, from the centre |
| `yaw=D` | which way it faces, in the piece's own turn |
| `type=T` | its type: a name |
| `mates=A,B` | the types it plugs into (two sockets join if either lists the other's type) |
| `carry=0.9` or `carry=0.8,T:0` | the share of a neighbour's stability this piece takes through the socket: one number for all, and/or per type of what it joins (a foundation takes 0.8 from another foundation and nothing from the wall on it) |
| `turn=D` | how the two sockets face when they join: 180 (the default) face each other, 0 face the same way (a wall on a foundation edge faces outward) |

`lunaris check` reads the text and says what is wrong (`BuildRules: ...`): a piece with no size, a socket with no type, a `prefab=` that does not exist, a piece that can never be placed.

## The tool

```json
{ "uid": 10, "components": {
    "Transform": { "pos": [0, 1.7, 0] },
    "BuildTool": { "kind": "wall", "owner": 7, "reach": 6 } } }
```

The entity aims along its local -z from its position, as a `Weapon` does, so it belongs on the camera or on a hammer held in the hand. The game sets `kind` (a piece of the rules; empty switches the tool off), `owner` (a number for the
player or the clan), `turn` (extra degrees for a piece put on open ground), `flip` (face the other way when it snaps: a wall's inside and outside) and, for one tick, `place`. Every tick the engine writes:

- `valid`: the piece can go there; if not, `why` says: `no_aim` (nothing in reach), `unknown_piece`, `no_socket` (the piece needs one and none is near), `no_ground` (open ground that is too steep, not ground, or not under it),
  `blocked` (it would go through another piece), `not_yours` (inside another owner's cupboard), `unstable` (it would collapse at once);
- `pos` and `yaw` (degrees): where it would go; `snapped`: the piece or socket entity it would be joined to (0 on open ground); `stability`: how well it would be held;
- if `ghost` names an entity, that entity's `Transform` follows `pos` and `yaw`: give it the translucent model and a colour by `valid`.

`place` builds the piece if it is valid: the event `build_placed` (the new piece's uid), `placed` and `built` on the tool. An invalid one gives `build_refused` (the tool's uid). Either way `place` is cleared. **Charging for the
piece is the game's**: look at `valid`, take the wood out of the inventory, then set `place`.

The aim is the first thing the ray meets: the world (ground, a building's wall) or another piece. The sockets within `snap` of that point are the options; the nearest that is allowed wins, the others are tried when it is not
(`blocked` is reported for the nearest). On a piece's own top surface a free piece is refused unless it is `loose` (a table on a floor); a piece that needs a socket and has none near says `no_socket`.

## Stability

A piece's **stability** is a number from 0 to 1 that the engine keeps in `BuildPiece.stability`:

- a piece with `anchor=1` that rests on the ground has 1 (five rays down from the piece's footprint: the middle and at least two of the four corners must find ground within `ground` metres below it and not steeper than `slope`);
- a piece joined to an anchoring `BuildSocket` takes 1 from it, times its own `carry`;
- every other piece has the **best** of what its neighbours have, times the `carry` of its socket for that neighbour's type.

Take the rules above. A foundation on the ground is 1. A wall on its edge takes 0.9: 0.9. A ceiling on that wall's top takes 0.9 of that: 0.81. A foundation joined to the edge of one that stands on the ground, but itself over
nothing, takes 0.8; one more beyond it 0.64, then 0.51, 0.41, 0.33, 0.26 and 0.21; the next would be 0.17, under the minimum 0.2, so the tool says `unstable` and it cannot be built (the tests build exactly this
chain). Build a second foundation on the ground at the other end and every piece of the chain takes the better value from it.

When the numbers change (a piece is built or destroyed) the engine works out again the whole structure the change belongs to (every piece joined to it through sockets): pieces that are then **under `min` collapse**
(the event `build_collapse`, their `debris` prefab made where they stood) and `loose` pieces whose base went fall (`build_fell`). A destroyed piece (its `HitPoints` reached 0, by a bullet, an explosion, decay, anything)
gives `build_destroyed`. A piece with nothing holding it can never be left standing: stability is the *best* path, so a piece that is above `min` always has a path to the ground through pieces that are above it.

## Building onto generated buildings

A `BuildSocket` is a point on something that is not a piece: put one (an entity with a `Transform`) on a facade, a roof or a doorway of a building the world generator made. Its `kind` is a socket type that the piece sockets
list in their `mates`. With `anchor` (the default) the building is solid ground to the structure: a piece joined to the socket is held as if it stood on the ground, through its own `carry`; a foundation hung on the
facade at a socket of type `edge` has stability 0.8 and needs no ground (`a_socket_on_a_generated_building_holds_a_piece_up_with_no_ground`). If the socket's entity goes away the piece collapses. Which way the
entity faces (its yaw: its local -z, which the pieces' `turn` is relative to) says which way a piece joined there points, so turn the socket to face out into the street.

The city generator makes one kind of socket, for a game server's city: `"door_socket": "edge"` in the `"server"` object of `city/city.json` puts a `BuildSocket` of that type on the threshold of every **street door** (not the doors between
rooms), on the floor of the doorway, 12 cm in front of the wall plane and facing out into the street (`serverside::socket_entity`; the entities are children of the building's ground-floor zone, so they exist while somebody is near the
building). A foundation (`free`, with `edge` sockets) joins it as a porch whose edge meets the wall; a wall (`wall_base` mates `edge`) placed there bars the doorway. `"door_socket_anchor": false` makes them only places to snap to
(`anchor` off): nothing is then held up by the building, and a piece that only such a socket would hold (a wall alone) is refused as `unstable`. An anchoring socket goes away with its zone when nobody is near, and a piece held by it alone collapses with it (the game's own notes (`55-door-sockets`) has what was measured).

## Land, upkeep and decay

```json
{ "uid": 5, "components": { "Transform": { "pos": [0, 0, 0] }, "BuildUpkeep": { "owner": 3, "radius": 40, "protect": 0 } } }
```

A cupboard is land: within `radius` metres of it the tool of any owner but `owner` is refused (`not_yours`) while `exclusive` (0 is nobody's and never refused), unless the two owners are in one party (`BuildTool.party`, docs/PARTIES.md: the party of whoever holds the tool; the owner of the land has to be in the game for its mates to be let in). `protect` is seconds of upkeep left: it runs down by a second a
second, and while it is above 0 the pieces within `radius` do not decay. **The game fills it** (so many seconds for so much wood put in the cupboard's inventory); the engine writes `covered`, how many pieces the cupboard
covers, so a game can charge by size. Every `audit` seconds a piece with `decay` above 0 and no protection loses `decay` times that many hit points; when they are gone it is destroyed like any other.

## Pieces made by hand

A scene can hold a base already (a ruined settlement, a starting hut): entities with `Transform`, `BuildPiece { "kind": "foundation" }` and, if they should be destroyable, `HitPoints`. A piece with `checked` false has its
ground looked up by the engine at the first audit (`anchored` and `checked` are then set); set both yourself to say it rests on the ground (or does not). The first audit also works out the stability of every piece. Pieces
that were removed, added or moved by anything but the building system (a script, a scene load) are noticed at the next audit, at most `audit` seconds later.

## Events

`build_placed` (the new piece), `build_refused` (the tool), `build_destroyed` (hit points gone), `build_collapse` (stability under `min`), `build_fell` (a loose piece whose base went). All carry the piece's position.

## Cost

Nothing when nobody builds and nothing changes. A tool costs one pass over the pieces near it per tick (the rays go through the world once for all the tools). A placement, a destruction and the audit rebuild the
index of all the pieces and settle the structures concerned; the cost grows with the size of the *connected* structure, not with the world. Measured by hand (`big_base_costs_what_the_docs_say`, release build, one big
field of connected foundations, so the whole field is one structure):

| Pieces | Audit tick | A tool, per tick | Destroying one piece |
|---|---|---|---|
| 900 | 1.4 ms | +0.01 ms | 1 ms |
| 4,900 | 5 ms | +0.05 ms | 5 ms |
| 19,881 | 34 ms | +0.1 ms | 33 ms |

A real base is a few hundred pieces in many separate structures; the second-long audit and a placement are far under a millisecond for it. (A world of 20,000 entities also costs the engine's own systems about 0.8 ms a tick.)

## What to supply

The pieces' models and prefabs (with their own colliders: the engine makes a plain box collider only for a piece with no prefab), the rubble prefabs, the ghost model and its colours, the build menu, the costs in items,
the sounds, and what a cupboard is (its inventory and its refill). Nothing here makes a picture.

## Limits (so far)

- Pieces turn about the vertical axis only: no ramps, roofs on a slope or tilted walls (a roof is a box that sits level; a pitched roof is a prefab with a flat socket).
- Overlap is tested between pieces only, not against the rest of the world: a piece can go through a tree or a car. Use the `Sensor` or the game's own check for that.
- Stability is the best single path to the ground, not a load calculation: a piece does not get weaker because it holds a lot.
- No terrain deformation: a piece that was grounded stays so if the ground is dug away until something else changes.
- Raiding has its own addon (`docs/RAIDING.md`): explosives that break pieces, code locks on doors and boxes, tiers (wood to stone) and bullets that barely scratch a base. Melee on walls is still the game's: lower the `HitPoints` of the piece.
- Over the network the server runs these rules and the pieces are replicated like any other static thing; building requests (`kind`, aim) are validated by the server running the same function. Wiring it into the scale
  server's `Game` is the next step of `docs/NETWORK_SCALE.md`.
