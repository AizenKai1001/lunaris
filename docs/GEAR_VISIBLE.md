# Worn gear the other players see (2026-10-24)

`Equipment` (docs/EQUIPMENT.md) makes a character's worn pieces protect it. This is the other half: **what a character wears is seen on its puppet by the other players**, with nothing added to the wire. Code: `crates/lunaris-sim/src/gear.rs` (two components and one step), a few lines in `SimGame` (`crates/lunaris-net/src/scale/simgame.rs`, the loop that already writes each avatar's health and held gun) and one line in `render.rs`. Tests: `gear_tests.rs` (5, sim), `simgame_gear_tests.rs` (2, in memory over a bad network).

## The server: a mask in `anim`

```json
"WornTell": {}
```

On the avatar (next to its `Equipment` and `Inventory`). Every step the engine works out `WornTell.mask`: **bit `k` is set while the `k`-th worn place of the character's `Equipment` (the order of its text) has an item in it** (an `id` and a `count`; the first 7 places count). `SimGame` puts the mask into the low 7 bits of the `anim` the other players are told about this character, **in place of whatever the player itself reported in `anim`** (the player says what it holds, the server says what it wears: a client that claims boots it does not have shows nothing). A piece that breaks (`armor::absorb` empties its slot), is taken off, or leaves with a dead player's bag drops out of the mask at the next step.

* **No bytes added.** `anim` is a varint and a record already carries it: 0 to 127 is one byte, so a record with a mask of four pieces is exactly as long as one with none (`a_mask_adds_nothing_to_a_record...` writes both, near and far, for 4 and for 7 bits). The mask is only in the full record (a player within `Rates.mid_m`, 80 m by default); the basic record of a farther player has no `anim`, so a far puppet reads 0 and shows nothing.
* **A game that also uses `anim` for its players' clips** has to put them above bit 7 (or not use `WornTell`): a character with a `WornTell` is told `anim` = the mask.
* **Cost on the server:** one component read of each avatar a step (`gear::update` works the mask out, a write only when it changed) and one more in the loop that writes the avatar's health and gun. Measured numbers are in the game's own notes (`32-gear-visible`).

## The client: `WornPart`

```json
"WornPart": { "place": 1 }
```

On a child of a puppet's mannequin (the entity with the skinned model), together with a `MeshRenderer` (the piece's model, `visible: false` to begin with, `cast_shadow` false) and a `BoneAttach` with no `target` (the model it is a child of) that names the joint it hangs on. Every step, after the game's systems (`gameplay::update`, so after any level-of-detail rule has set the mannequin's `visible`), the engine sets the part's `MeshRenderer.visible` to: **the mannequin is drawn AND bit `place` of the mask in the mannequin's `NetPuppet.anim` is set**. (The mannequin has a `NetPuppet` of its own, which the net layer gives the puppet's every tick: docs/GAME_SERVER.md, "Players".) So a far puppet, a puppet whose mannequin the game hides (a box stands in) and a puppet behind the view cone wear nothing and cost nothing, and a game's own rules over the mannequin stay in charge. A part whose mannequin is gone is hidden.

* **Cost:** a step looks at each part once (the parts of one mannequin come one after the other, so the mannequin is looked up once for the run) and writes only when a part changes. `the_cost_of_the_gear_of_a_crowd_is_small_and_is_measured` has 100 puppets with 24 parts each (3 mannequins x 8, a third drawn): **about 26 us a step, 11 ns a part** on a Linux test machine (release build).
* **The picture.** `render.rs` collects the `BoneAttach` things of a picture; it now leaves out those whose `MeshRenderer` is not visible. Before, a mannequin with a hidden attachment was posed one more time (`joint_worlds`) for a thing that is not drawn, which would have made every unarmored player pay for the gear they do not wear. A visible part is drawn after the mannequin at its joint in the pose of the picture (docs/GAMEPLAY.md, "Bone attachments").
* A piece is a rigid model on one joint (a vest on the spine, a helmet on the head, a pair of pants as a part on each thigh and each calf, boots on the feet): it follows the animation but does not bend with the skin.

## Not here
The player's own view of its gear (first person), corpses and falling bodies (the worn pieces are in the corpse's bag; a game can tell a corpse's mask the way it tells its variant), more than 7 places, a second look for a piece (tiers, colours by item: another model name per item would need the item's number on the wire, the mask is a bit a place), and anything that is not a piece in a worn place (a held gun is `NetPuppet.extra`).
