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
* `scatter` places many copies of something on the ground in one undoable step (below), and can keep to one layer of a painted ground.
* `terrain_layer {x, z}` says which layers of a painted ground (`TerrainLayers`, below) are under a point and how much of each.
* `water_depth {x, z}` measures the water of a `WaterBody` (below) at a point: its surface, the ground under it and how deep it is.
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

## Layers: paint the ground with materials

One texture over a whole landscape is blurry up close and cannot be grass here and rock there. Add a **`TerrainLayers`** to the entity that has the `Terrain` and the ground is painted with up to four
materials (Unity's terrain layers, Unreal's landscape layers) that a **splat map** blends: an RGBA picture over the whole ground whose red, green, blue and alpha say how much of layer 1, 2, 3 and 4 there is at each place.

```
{"Transform": {"pos": [0, 0, 0]},
 "Terrain": {"heightmap": "terrain/heightmap.png", "size": [5000, 5000], "height": 900, "color": [1, 1, 1, 1]},
 "TerrainLayers": {"splat": "terrain/splat.png",
                   "layers": "grass grass uv_scale=900\nrock dirt uv_scale=500 tint=0.8,0.8,0.85\nsand sand uv_scale=700\nsnow terrain/snow.png uv_scale=600"}}
```

`splat` is a PNG of the workspace (8 or 16 bits, any size; RGB works for three layers). `layers` is text, one layer a line, `NAME [TEXTURE] [uv_scale=N] [bump=D] [tint=R,G,B]`:

| word | meaning |
|---|---|
| `NAME` | letters, digits, `_`, `-`: what `terrain_layer` and `scatter` call the layer. Layer 1 is the first line and is weighed by the red channel, layer 2 by green, layer 3 by blue, layer 4 (the last possible) by alpha. |
| `TEXTURE` | a built-in material (`grass`, `dirt`, `stone`, `bricks`, `wood`, `sand`, `concrete`, `tiles`, `metal`) or a PNG or JPEG of the workspace. Left out, the name is used (`sand` alone is the sand material). |
| `uv_scale=N` | how many times the texture repeats across the **whole** ground (as for `Terrain.uv_scale`); `uv_scale=60,40` for different counts along x and z. Left out, the terrain's own `uv_scale`. A 5 km ground wants hundreds. |
| `bump=D` | the depth in metres of the built-in material's relief in the GPU renderer's high qualities (default 0.03, 0 for flat). A picture has no relief to show. |
| `tint=R,G,B` | a colour the texture is multiplied by, each 0 to 4: snow from `concrete tint=1.6,1.6,1.7`, a darker `dirt` for rock. |

Blank lines and lines that start with `#` are skipped. `Terrain.color` multiplies the blend the way it multiplies a texture, so set it to `[1, 1, 1, 1]` to see the layers as they are (the default is a green that would tint all of them);
`Terrain.texture` is not used while the layers are there, and the ground is drawn opaque.

**The splat map is laid on the ground like the heightmap**: row 0 is the north edge (-z), the left column the west edge (-x), and a texel sits on a corner of the grid that spans the ground, so a 513 x 513 splat map over a 513 x 513 heightmap has one
weight at every vertex (a map of another size is stretched over the same ground). Between texels the weights are blended smoothly, and where they do not add up to 1 they are made to (a texel that is black has none of its own: it shows layer 1). A layer
that the map has no channel for (an RGB map and four layers) is never shown. **Tellus's `splat.png` loads as it is** (R grass, G rock, B sand, A snow, the same size as its heightmap):
`"layers": "grass grass\nrock dirt\nsand sand\nsnow concrete tint=1.6,1.6,1.7"`.

Both renderers draw the blend: the reference renderer weighs the layers pixel by pixel (the same picture on every machine, and the number in `tools/golden_hashes.txt`), the GPU renderer does the same sum in its shader and in the high
qualities adds the relief of the built-in materials, filtered and sharp at a slant. A mistake in the layers text, a splat map that cannot be read or an empty `splat` leaves the terrain drawn with its own texture and a warning in the picture's `warnings`;
`check` says what, and also finds a layer that is nowhere on the ground (the wrong channel), a missing texture, the default green `Terrain.color` and a `Terrain.texture` that is not used. A splat map saved over while the game runs is read again.

* `terrain_layer {x, z}` answers "what is the ground made of here?": `weights` (the four numbers, adding up to 1), each layer's `name`, `texture` and `weight`, the `dominant` layer and its index. Use it for footstep sounds, decals and what a spawn may stand on.
  Errors: `no_terrain` outside every terrain, `no_layers` where the terrain has none, `splat_unreadable`, `bad_layers`. `terrain` names the entity when two terrains overlap (the highest painted one under the point is asked otherwise).
* `scatter` takes `layer` (a name or a number from 0) and `layer_min` (0 to 1, default 0.5): copies go only where that layer weighs at least that much, so trees stand on the grass and rocks on the rock.

**A whole example**: `examples/terrain_layers.lunaris.json` is a valley painted with all four layers (grass, rock, sand on the shores and the lake's bed, snow on the high ground), with a lake and a river that steps down to it (below), crates and a buoy
floating on the lake, pines on the grass, boulders on the rock and a hero you walk (WASD, Space) into the water and out again. `tools/build_terrain_layers.py` makes it from a seed (the 16-bit heightmap, the RGBA splat map painted from height and slope, the water picture, a snow
texture: `examples/terrain_layers/`), `examples/tests/terrain_layers.test.json` is its playtest, and `examples/terrain_layers/shot.png` a 1280 x 720 picture of it in the GPU renderer's `high` quality (made with
`python3 tools/render_scene.py examples/terrain_layers.lunaris.json examples/terrain_layers/shot.png --size 1280x720 --eye=-24,11,64 --target=-22,9,-10 --fov 62 --hold a --steps 130`).

## Water bodies: rivers and lakes that follow a picture

The flat `Water` sheet is one level. A **`WaterBody`** makes a surface from a 16-bit greyscale picture of the water's heights, so a river can step down its valley and lakes can stand at their own levels, all in one entity:

```
{"Transform": {"pos": [0, 0, 0]},
 "WaterBody": {"heightmap": "terrain/water.png", "size": [5000, 5000], "relief": 900, "color": [0.04, 0.28, 0.38, 0.82], "wave_height": 0.4, "wave_length": 12}}
```

| field | default | meaning |
|---|---|---|
| `heightmap` | (empty) | a greyscale PNG of the workspace, 16 bits best. **0 is dry land**; any other value is the height of the water's surface: `value / 65535 * relief` metres above the entity's `Transform.y` (0 for Tellus's `water.png`, whose heights are absolute metres: its `build.json` gives the `scale_m` to use as `relief`). |
| `size`, `step` | `[256, 256]`, `1` | the metres the picture covers (x, z), centred on the entity, and every step-th sample (at most 2048 cells across), laid on the ground exactly like a `Terrain`'s heightmap: row 0 north, samples on the corners of the cells. Give the water the same `size` as its terrain. |
| `relief` | `64` | the metres a value of 65535 stands for. |
| `color`, `wave_height`, `wave_length`, `speed`, `phase` | blue-green 0.82, `0.15`, `6`, `1`, 0 | the looks, as for `Water`: in the GPU renderer's high qualities the surface is see-through (the alpha), reflects the sky and the sun's glints and rolls with waves; the reference renderer and `classic` draw it flat and solid. `phase` is the engine's clock for the waves (it stops when the world is paused). |
| `visible` | `true` | the renderers draw it. |

The water is only where the picture says, and a cell is water when any of its four corners is: a dry corner next to water takes the average height of the wet samples round it, so the sheet runs to the middle of the shore cell and the ground, which stands higher
there, hides the rest. Only the cells with water are made (a river on a big map is a thin ribbon of triangles); a picture with no water draws nothing and `check` says so. It is not solid and rays pass through it. A water picture is read once like a heightmap, replaced by
`reload_terrain`, and part of the state hash (a scene without a `WaterBody` has exactly the hash it had).

**Is this in water?** The engine writes **`InWater`** on every entity that has a `CharacterBody` whose feet are under the surface and takes it off again when it climbs out: its presence is the answer, and a graph system with `"each": ["CharacterBody", "InWater"]` runs only for swimmers
and waders (slow them down, change the animation, play a splash). `InWater.depth` is how far the surface is above the feet in metres (compare it with `CharacterBody.height`: 0.5 is a wade, 1.5 a swim), `surface` the height of the surface and `body` the water's entity. A playtest asks
`{"expect": {"count": {"with": ["InWater"]}, "eq": 1}}` or `{"name": "Hero", "field": "InWater.depth", "between": [0.5, 2]}`. The character itself is not changed (it still walks on the bed of the lake): what a swimmer does is up to your systems.

**Floating.** `Buoyant {density, drag}` beside a `Collider` (sphere, box or capsule), a `RigidBody` and a `Velocity` makes a body float: every tick, before the physics step, the water pushes it up by gravity times how many times heavier water is than the body (`1000 / density`, the density in kg/m3:
500 wood, 920 ice, 1000 neutral, above 1000 it sinks slowly) times the share of the body under the surface (a ball's by its spherical cap, a box's by its height, tilted boxes by how far up their corners reach), and slows it (and its spin) by `drag` per second in proportion to that share. A ball of density 500 floats with its middle
at the surface, a cube of 250 with a quarter of it under. `Buoyant.submerged` (0 to 1) is the share the engine found: splash sounds, foam. The surface a body floats on is the flat one of the picture: the waves are drawn, not simulated.

* `water_depth {x, z, y?}` measures the water at a point: `wet`, the height of its `surface`, the `ground` under it (the highest solid terrain), the `depth` of water over the ground and the `water` entity; with `y`, `above` is how far the surface is above that height (what `InWater.depth` would be for feet there). Dry land answers `wet: false`,
  `depth: 0`. Error `no_water` when the world has no `WaterBody`; `unread` names water pictures that could not be read.
* `check` reports a water body without a place or a picture, a picture that cannot be read or has no water in it, a `relief` or `size` that means nothing, and a `Buoyant` body that lacks a `Collider`, `RigidBody` or `Velocity`, has a density of 0 or less, or has no water to float on.
* Limits: one surface level per place (the highest of overlapping bodies wins), no currents (a river is drawn flowing nowhere: push things along with your own system), and the waves do not move a floating body.
* The example above has a lake at 4 m and a river 110 m long whose surface falls from 17 m to the lake's level; its `wade` graph system (`"each": ["CharacterBody", "InWater"]`) slows the hero by the depth of the water, and its playtest follows `InWater` appearing as the hero wades in and going as it climbs out.

## Making a heightmap without a terrain program

```
python -I tools/make_heightmap.py ground.png --size 513 --seed 3
```

writes hills, mountains in the north, a flat meadow at the middle and the sea at the edges as a 16-bit picture, the same bytes for the same arguments.
`--meadow 0` leaves out the flat circle. Edit it in any paint program that keeps 16 bits (Krita, GIMP set to 16-bit, Photoshop), or use it as the base for a
terrain program.

Terrain programs have their own licences: for instance the free edition of Gaea is for non-commercial use. A picture you make with one is yours to use
as the licence of that program allows; the examples in this repository do not include any such picture, they are made by `tools/make_heightmap.py`.
