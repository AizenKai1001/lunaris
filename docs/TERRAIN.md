# Terrain

Ground made from a **heightmap**: a greyscale PNG where black is the lowest ground and white the highest. Terrain programs (Gaea, World Machine,
WorldPainter, Blender's baked height textures and many more) export exactly this, so a landscape made there is one file and one entity here.
Spheres, boxes and characters stand on it, rays hit it, and both renderers draw it: the triangles that are drawn are the triangles that are
stood on.

```
{"Transform": {"pos": [0, 0, 0]},
 "Terrain":   {"heightmap": "examples/terrain/hills.png", "size": [256, 256], "height": 60}}
```

`examples/terrain.lunaris.json` is a whole scene with a meadow, balls, a hero you can walk across it and a sea, `examples/tests/terrain.test.json`
its playtest, and `tools/build_terrain.py` writes both (with the picture).

## The component

| field | default | meaning |
|---|---|---|
| `heightmap` | (empty) | path of a PNG in the workspace (`files` lists them). Greyscale, 8 or 16 bits, not interlaced. A colour picture counts by its first (red) channel. At most 64 million samples. |
| `size` | `[256, 256]` | metres covered by the picture: x (east-west) and z (north-south). The ground is centred on the entity. |
| `height` | `64` | metres from black to white. The entity's `Transform.pos[1]` is the height of black. |
| `step` | `1` | use every step-th sample: 2 makes a ground of a quarter of the triangles. At most 2048 cells across are used, whatever you ask (the step is raised for a bigger picture). |
| `color`, `texture`, `uv_scale` | green, none, `[64, 64]` | the looks: a tint, an optional texture repeated `uv_scale` times across the whole ground, and the alpha of `color`. |
| `friction`, `restitution` | `0.8`, `0.1` | the ground's contact material: contacts take the larger restitution of the two bodies and the geometric mean of their frictions. |
| `solid` | `true` | physics and physical rays see it. |
| `visible` | `true` | both renderers draw it and visual rays and `pick` see it. |

The entity's rotation and scale are not used: the ground lies flat, and you choose its size with `size` and `height`.

## Reading the picture

* **Row 0 of the picture is the north edge (-z), the left column is the west edge (-x)**: the picture looks like the map from above, as it does in
  Gaea's preview. Samples sit on the corners of the cells: a 257 x 257 picture over 256 m has one sample every metre, and a vertex at each.
* **16 bits are the right choice.** 8 bits give 256 levels: over 60 m of relief that is steps of 23 cm, which a rolling ball and a walking hero feel
  as terraces. A 16-bit picture has 65536 levels.
* Each cell is two triangles with the diagonal from its north-east corner to its south-west corner, always. Drawing and physics agree on it.
* The file is read **once**, when the simulation first needs it (the first `step` or `check`), and kept. A picture that cannot be read does not stop
  the simulation: that terrain has no ground, `check` explains why, and the simulation tries the file again every few seconds, so a file
  that is still being written appears when it is complete. After you have **changed the file**, call `reload_terrain`.
* The state hash includes a hash of every heightmap in use, so a replay or a networked game with a different picture is noticed instead of drifting.

## Tools

* `terrain_height {x, z}` answers "how high is the ground here, and which way does it lean?" (`height`, `normal`, the terrain's `uid`). Put things
  on the ground with it: spawn at `height` plus half the object's size. Outside every terrain it is an error with code `no_terrain`.
* `reload_terrain` reads every heightmap again after the files changed, and reports what was read and what failed.
* `minimap` shows the ground in its text map: letters `a` (lowest) to `j` (highest) over a terrain in the top view, `#` under the surface in the side view; the
  terrain entity itself is not a letter. This is how a language model "sees" hills.
* `scatter` places many copies of something on the ground in one undoable step (below).
* `check` reports a terrain whose picture is missing or unreadable, a terrain without a `Transform`, and an empty `heightmap`.
* `raycast`, `pick`, `render` and `step` need nothing special.

## Scatter: forests, rocks, grass

```
{"tool": "scatter", "args": {"prefab": "Pine", "count": 300, "slope": [0, 28], "height": [3, 40], "min_distance": 6, "scale": [0.8, 1.4], "seed": 7}}
```

puts up to 300 pines on ground that is gentle (0 to 28 degrees), above the beach (world height 3 to 40) and at least 6 m apart, each turned by a random
angle and scaled between 0.8 and 1.4, in one undoable step. Instead of `prefab` you can give `components` (what each copy has; its `Transform` is filled
in). More arguments: `terrain` (which entity, default the first), `area` ([x_min, z_min, x_max, z_max]), `align` (tilt to the slope), `yaw`, `sink`
(bury the foot a little), `attempts`. The same seed on the same ground gives the same forest; the result says how many fit (`placed`) and adds a `note`
when it was fewer than asked. What lands in the world, and in a recording, are ordinary spawn commands with their final positions, so replays do not
depend on the random choice. A copy is an ordinary entity: give the prefab a `Collider` if it must be solid.

## Physics

* Spheres, boxes and capsules (`Collider` shapes 0, 1 and 2) with a `RigidBody` come to rest on the ground, slide down slopes and bounce by the friction and
  restitution of both materials; with an `AngularVelocity` they roll and turn to lie on the slope (`docs/PHYSICS.md`). Characters (`CharacterBody`) walk on it, follow its slopes up and down, and cannot climb a slope steeper than their `max_slope` (degrees).
* The ground is static: it does not move and nothing pushes it. Two terrains can overlap (a body stands on the higher one), they do not collide with
  each other.
* Everything is computed with plain `+ - * /` and `sqrt` in a fixed order, so every machine finds the same contacts. `tools/golden_hashes.txt` pins the
  hashes of `examples/terrain.lunaris.json` on Windows and Linux.

## Limits

* Acoustics (`acoustics`, sound occlusion) does not know about terrain: it sees colliders, so a hill does not muffle a sound. Put `Collider` boxes
  where sound must be blocked.
* The GPU renderer and the CPU reference draw the same mesh; there is no level of detail, so a picture of 2049 x 2049 samples is eight million triangles.
  Use `step` for far-away or huge landscapes, or split them into several terrains.
* A picture of more than 64 million samples is refused, and so is one that is not a PNG, is interlaced, or has a palette.
* There is no sculpting in the editor yet: make the picture in a terrain program (or with `tools/make_heightmap.py`, which needs only Python), save it
  in the workspace, and point `heightmap` at it.

## Navigation

`NavAgent` and the `path` tool walk over terrain: every cell of the search grid stands on the height of the ground at its centre, ground that leans
more than 50 degrees (a cliff) is not walkable, and gentle slopes are walked because neighbouring cells differ by less than a step. Trees and boxes with
colliders stand on the ground and block as before. A wall of ground with a gap in it makes the path go through the gap (`crates/lunaris-sim/src/terrain_tests.rs`).

## Making a heightmap without a terrain program

```
python -I tools/make_heightmap.py ground.png --size 513 --seed 3
```

writes hills, mountains in the north, a flat meadow at the middle and the sea at the edges as a 16-bit picture, the same bytes for the same arguments.
`--meadow 0` leaves out the flat circle. Edit it in any paint program that keeps 16 bits (Krita, GIMP set to 16-bit, Photoshop), or use it as the base for a
terrain program.

Terrain programs have their own licences: for instance the free edition of Gaea is for non-commercial use. A picture you make with one is yours to use
as the licence of that program allows; the examples in this repository do not include any such picture, they are made by `tools/make_heightmap.py`.
