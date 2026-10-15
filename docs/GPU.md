# The GPU renderer

Lunaris draws on the graphics card with **wgpu**: Vulkan, DirectX 12 or Metal, whichever the machine has (`crates/lunaris-gpu`). It draws pictures into a window (`lunaris play`) and into images (the `render` tool, the editor's viewport, tests), and it has two modes:

* **classic** draws exactly what the reference CPU renderer draws, only faster. The tests compare the two pixel by pixel.
* **high** is the one made to look good: HDR rendering, physically based materials, a procedural sky that lights the scene, cascaded sun shadows, ambient occlusion, bloom, fog, 4x multisampling and filmic tone mapping.

```
lunaris play examples/vista.lunaris.json                    # a window, high quality (falls back to the CPU renderer without a GPU)
lunaris play examples/vista.lunaris.json --quality ultra    # classic | low | medium | high | ultra
lunaris play examples/vista.lunaris.json --cpu              # force the software renderer
lunaris call render '{"quality":"high","path":"vista.png"}' --scene examples/vista.lunaris.json
lunaris edit examples/vista.lunaris.json                    # the editor's viewport is drawn by the GPU too (its Quality menu picks the renderer)
lunaris serve --no-gpu                                      # an agent server that only ever uses the CPU reference renderer
```

The `render` tool takes `quality`: `reference` (the CPU renderer), `classic`, `low`, `medium`, `high`, `ultra` or `auto` (the default: `high` when the host has a GPU, otherwise `reference`). Its reply says which renderer drew the picture (`renderer`, `quality`) and whether the picture is `deterministic`: only `reference` pictures are bit-identical on every machine; a GPU picture is the same to the eye but not to the last bit (graphics cards round differently), so hash and golden-image tests use `reference`. `visible`, `coverage` and the entity ids work for every quality (the GPU draws an entity-id picture next to the colour one).

## The qualities

| quality | what it draws |
|---|---|
| `classic` | the reference renderer's picture: gamma-2.0 colours, Blinn-Phong lighting, nearest-neighbour textures, nothing else |
| `low` | high-quality shading and sky, sun shadows, no multisampling, no ambient occlusion, no bloom |
| `medium` | `low` + 4x multisampling + bloom + shadows for point lights (512 texel maps); 1024 sun shadow maps |
| `high` | `medium` + ambient occlusion; 2048 sun shadow maps, 1024 point-light maps |
| `ultra` | `high` with 4096 sun shadow maps and 2048 point-light maps |

## Making a scene look good

The look is data, like everything else. Add an **`Environment`** entity and give meshes **materials**:

```json
{ "uid": 1, "components": { "Name": { "value": "Environment" },
  "Environment": { "sky": true, "sky_top": [0.13, 0.27, 0.62], "sky_horizon": [0.98, 0.66, 0.42],
                   "clouds": 0.35, "fog": 0.016, "fog_color": [0.97, 0.66, 0.44], "bloom": 0.45, "ssao": 1.0, "vignette": 0.28 } } }
{ "uid": 2, "components": { "Name": { "value": "Gold" }, "Transform": { "pos": [0, 0.65, 0], "scale": [1.3, 1.3, 1.3] },
  "MeshRenderer": { "mesh": "sphere", "color": [1.0, 0.77, 0.33, 1], "metallic": 1.0, "roughness": 0.22 } } }
{ "uid": 3, "components": { "Transform": { "scale": [200, 1, 200] },
  "MeshRenderer": { "mesh": "plane", "texture": "grass", "uv_scale": [80, 80], "roughness": 0.95 } } }
```

`examples/vista.lunaris.json` (made by `tools/build_vista.py`) uses all of it: a procedural evening sky with clouds, sun shadows, a cobbled path, brick wall, wooden crates, metal and plastic spheres, glowing lanterns, trees and fog.

**`Environment`** (the first one by uid counts; colours are 0..1 display colours): `sky` (a procedural sky: `sky_top` at the zenith, `sky_horizon`, `ground` below the horizon, a sun disc where the first directional light shines from, `clouds` cover 0..1), `ambient` (how strongly the sky lights the scene and shows in reflections), `exposure`, `fog` (density) with `fog_color`, `shadows` and `shadow_distance` (how far from the camera the sun's shadows reach), `ssao` (ambient occlusion strength 0..2), `bloom` (0..2) with `bloom_threshold`, `vignette`, `saturation`, `dof` (0..1) with `focus` and `focus_range` (depth of field: sharp around `focus` metres from the camera and `focus_range` metres either side of it, blurrier the further off; `high` and `ultra` only; `examples/depth_of_field.lunaris.json`), and `tonemap` (0 automatic, 1 filmic ACES, 2 neutral, 3 none). Without an `Environment` a plain look is worked out from the camera's `clear` colour; a flat 2D scene (no 3D meshes) gets no effects at all, so its colours come out exactly as authored.

**`MeshRenderer`** adds `metallic` (0 = plastic, stone, wood; 1 = bare metal), `roughness` (0 = mirror, 1 = chalk; a negative value, the default, is worked out from `shininess`), `texture` and `uv_scale` (repeat a picture over the mesh), `bump` (relief depth in world units for the built-in materials) and `cast_shadow`.

**Built-in material textures** (no files needed; tileable; with a height map for the bumps): `grass`, `dirt`, `stone` (cobbles), `bricks`, `wood` (planks), `sand`, `concrete`, `tiles`, `metal` (brushed). A PNG or a JPEG (baseline or progressive, colour or grey) in the workspace works too (`"texture": "textures/floor.jpg"`), and so do JPEG pictures inside a `.glb` model, and so do the little pictures `checker`, `gradient`, `circle`, `ring`.

**Transparency**: a mesh whose colour has an alpha below 1 (`"color": [0.75, 0.92, 1.0, 0.18]`) is see-through in the high-quality modes: drawn after the opaque things, far to near, blended with a Fresnel boost at grazing angles so that glass keeps its reflections and highlights, picked by `pick` and listed by `visible` like anything else. See-through meshes cast no shadow. The reference renderer and `classic` draw every mesh opaque.

**Night**: a sky whose `sky_top` is dark shows stars (and the clouds still drift over them); the lamps' bloom does the rest.

**Lights**: the first directional light is the sun (it casts the big shadows, makes the sun disc and the clouds' bright edges). Spot lights (`Light.kind` 2, with `angle` and `softness`) are point lights restricted to a cone; the reference renderer and `classic` draw them as plain point lights. The two most important point or spot lights (bright, near the camera, long reach) cast shadows too (from `medium` up): a lantern between pillars throws their shadows across the floor; the others add light without shadows. Light colours above 1 are fine: the picture is HDR until the end. Set `"cast_shadow": false` on a mesh that should not throw shadows (the glowing bulb around a lamp).

## Level of detail: `Lod`

`{"Lod": {"mesh1": "models/tree_low.glb", "distance1": 40, "mesh2": "cube", "distance2": 120, "cull": 300}}` beside a `MeshRenderer`: from 40 m from the camera the entity draws `mesh1` instead (a model with fewer triangles), from 120 m `mesh2`,
and beyond 300 m it is not drawn at all. Both renderers use it, and `visible` / `pick` agree with what is drawn. An empty name keeps the model the entity has.

## Wind: `Sway`

`{"Sway": {"amount": 0.3, "extent": 5, "speed": 0.8, "offset": 1.7}}` beside a `MeshRenderer` makes the model bend in the wind in the high qualities: the part above the entity's position leans by `amount` metres at `extent` metres
height, back and forth `speed` times a second, more the higher it is (the foot stays put), differently at different places, and a different `offset` for each entity keeps a forest from nodding in step. Trees, grass, banners, lamps on chains.
The clock runs in the simulation like the water's. The reference renderer draws it still.

## Water: `Water`

```
{"Transform": {"pos": [0, 0.4, 0]}, "Water": {"size": [200, 200], "color": [0.04, 0.28, 0.38, 0.8], "wave_height": 0.4, "wave_length": 7}}
```

A sheet of water at the entity's height, `size` metres wide and deep. In the high qualities it is see-through (`color`'s alpha), reflects the sky and the sun's glints in rolling waves
(`wave_height`, `wave_length`, `speed`; the waves are moved by the vertices and the normals, and the clock `phase` runs in the simulation, so a replay shows the same water and the water stops when the world is paused with
`time.scale`). The reference renderer and `classic` draw a flat sheet of its colour. Water does not push anything: a trigger `Collider` of the same size tells which things are in it, and the height of the surface at a point is
`Transform.y` plus the waves (about `wave_height` at most) for floating things. For rivers and lakes use several sheets; a very large sheet gets a coarser mesh (up to 400 x 400 cells).

## Painted terrain: `TerrainLayers`

A `Terrain` with a `TerrainLayers` beside it is drawn with up to four materials weighed by an RGBA splat map (`docs/TERRAIN.md`, "Layers"). In `classic` the pixel is the reference renderer's: the layers' textures sampled nearest-neighbour and repeated across the ground at their own rates,
weighed by the splat map (blended bilinearly between texels that lie on the corners of the grid, then normalised), multiplied by `Terrain.color`. In the high qualities the textures are sRGB, mip-mapped and filtered 16 times at a slant, the blend is done in true linear light, and the built-in materials
add their relief: the slope of the weighed height maps (centred, so that a change of layer is no ridge) tilts the normal by each layer's `bump` metres. It is one more pipeline (`MeshSplat`, `fs_mesh_splat`) with a third bind group of nine textures (the splat map, four colours, four reliefs; the whole shader stage uses
14 of the 16 a portable device has) and a small uniform; the ground still takes part in shadows, the depth prepass and the entity-id picture as the plain mesh it is. A test (`a_painted_terrain_is_blended_the_same_way_in_both_renderers`) draws hills painted with four materials with both renderers
(mean error 0.11 of 255, fewer than 0.005% of the pixels off by more than 40), another puts four solid colours on the four quarters of the ground and checks each is in its place in every quality, with the first row of the map at the north. The example `examples/terrain_layers.lunaris.json` (painted valley, lake, river, floating crates, trees)
is in the list of scenes the classic test compares: mean error 0.25 of 255, fewer than 0.005% of pixels off by more than 40. `examples/terrain_layers/shot.png` is its `high` picture at 1280 x 720.

## Water that follows a picture: `WaterBody`

`WaterBody` (`docs/TERRAIN.md`, "Water bodies") draws the surface a 16-bit picture of water heights describes: rivers at their own slopes, lakes at their own levels, only where the picture has water. It is the `Water` sheet's material and waves on another mesh (the cells that have water, built once and kept):
the same see-through, sky-reflecting, sun-glinting surface in the high qualities (the ripples tilt the surface's own normal, so a river leans downstream), flat and solid in `classic` and in the reference renderer. A test (`a_water_body_follows_its_picture_in_both_renderers`) draws two lakes at different levels with both renderers (mean error 0.04 of 255), checks the id
pictures agree on where the water is and that the high quality shows both blue, steady between pictures and moving with the clock `phase`.

## Picture effects: `PostFx`

Beside the `Environment`, one `PostFx` entity adds effects to the high qualities (the reference renderer and `classic` ignore it): `shafts` (0 to 2) rays of light fanning out from the sun through whatever stands in front of it (it needs a directional
light and a visible sky, and the sun in or near the picture), `motion_blur` (0 to 1) smears what moves between one picture and the next (live play and the editor; a picture a tool asks for is blurred against the last one it drew, so leave it 0
for tools), `grain` (0 to 1) film grain that changes every picture, `contrast` (1 = as is) and `tint` (a colour that multiplies the picture: warm, cold, sepia). `tools/render_scene.py` renders a scene with extra entities for trying them:
`python3 tools/render_scene.py examples/vista.lunaris.json out.png --add '{"PostFx":{"shafts":1.5}}'`.

## Height fog: `HeightFog`

One `HeightFog` entity (the first counts; the reference renderer and `classic` ignore it) adds a mist that is thickest at `height` metres and thins out upwards by `falloff` per metre (0.1 halves the density about every 7 metres): valleys fill, hilltops stand clear, and a camera above the layer looks down into it. `density` is the thickness at `height` (0.05 a light haze, 0.5 thick) and `color` the fog's colour (it replaces the `Environment`'s fog colour while the entity exists); it adds to the `Environment`'s uniform `fog`. The density is summed exactly along each ray (no steps, no banding). It fogs meshes, terrain and water surfaces; particles and sprites are not fogged. A test on the real GPU (`height_fog_fills_low_ground_and_leaves_high_ground_clear`) checks that a far floor takes the colour, that a layer far below the view does almost nothing and that classic ignores it.

## Colour grading and fades: `ColorGrade`

One `ColorGrade` entity (the first counts) grades the finished picture in the high qualities (the reference renderer and `classic` ignore it): `lift` is added in the shadows per colour channel, `gamma` bends the midtones (1 = unchanged, above 1 brightens, below 1 darkens), `gain` multiplies the highlights, `temperature` warms (up to 1) or cools (down to -1) the whole picture and `tint_shift` trades green (-1) against magenta (1). `fade` is a colour laid over the whole picture, with the amount in its alpha: a fade to black between scenes (`fade [0,0,0,1]`), the red flash of a hit, the blue of being under water. The values are plain numbers a `Tween` or a graph system can animate (`Tween` a `ColorGrade.fade[3]` from 0 to 1 for a fade out). The defaults change nothing. It is the last step before the picture is encoded, after tone mapping, saturation and vignette. A test on the real GPU (`a_colour_grade_fades_warms_and_lifts_the_picture`) checks the fade, the flash, warm against cold, lift and gamma.

## Eye adaptation: `AutoExposure`

```
{"AutoExposure": {"min": 0.05, "max": 16, "speed": 1.5, "target": 0.18}}
```

One `AutoExposure` entity (the first counts; the reference renderer and `classic` ignore it; every high quality from `low` up draws it) makes the picture adapt to the light the way an eye does: the renderer measures how bright the finished HDR picture is (the geometric mean of the luminance over a 64 x 64 grid of points, the middle of the picture counting a little more than the corners, the flat background not at all),
and multiplies the light by the exposure that brings that average to `target` (0.18 is middle grey; the tone mapper puts it at about half way up). The exposure never goes below `min` or above `max` (they are multipliers: 0.05 turns the light down twentyfold, 16 up sixteenfold; a dark cave and a sunny day are about 30 times apart), and it multiplies with `Environment.exposure`, which stays the compensation you choose (2 = a stop brighter whatever the light does).
`speed` is how fast it adapts: every second covers `1 - e^-speed` of the remaining way in stops, so 1.5 settles in about a second and 4 is quick.

The measurement is one small compute pass (a single workgroup, 4,096 samples, one dispatch) after the main pass; the state lives on the card in a 16-byte buffer that the bloom prefilter and the composite read, so nothing waits for the CPU. The exposure is applied before bloom (a lamp glows in a dark room, the same lamp does not in the sun), light shafts and tone mapping, and the flat background colour is not exposed.
**Over time**: in `lunaris play` and in any window the pictures follow each other and the exposure glides, by the real time between them (a pause of two seconds or more starts again from the measured value). **One picture asked for by a tool** (`render`, tests, the editor's viewport) is measured and applied at once, with no memory of what was drawn before, so the same world always draws the same picture; the `render` reply says what the eye did in `measured` (`exposure`, `average_luminance`: information about the picture, never written into the simulation or its hash).
A program that films the world into images can ask for the gliding with `GpuRenderer::set_adaptation_step(Some(seconds))` (every picture then counts as that many seconds after the one before). A test on the real GPU (`eye_adaptation_brings_dark_and_bright_scenes_to_a_similar_brightness_and_takes_time_to_do_it`) draws a lit room 400 times darker than a bright one (mean picture brightness 13 and 247 without it, 130 and 131 with it),
checks that the same picture comes out twice and that classic ignores it, and films a bright room followed by a dark one a quarter of a second apart: the first picture in the dark is still black and it takes about seven pictures to get most of the way (1.6, 12, 32, 53, 73, 89, 102, 110, 116, 121 out of 131). `check` warns about a second `AutoExposure`, `min` above `max`, a `speed` that never moves it, a `target` outside 0 to 4 and a scene with no 3D meshes to measure.

## Screen-space reflections: `Ssr`

```
{"Ssr": {"strength": 1, "max_distance": 40, "thickness": 0.5}}
```

One `Ssr` entity (the first counts; `high` and `ultra` only: the reference renderer, `classic`, `low` and `medium` ignore it) lets smooth and metallic surfaces, and `Water`, mirror what is on the screen, not only the sky: a lamp above a polished floor shows in it, a hero wading through a lake has her reflection in the water.
The renderer always had the sky's reflection (a split-sum approximation weighed by the environment BRDF); SSR keeps that term and **swaps it for what the picture shows** where a ray from the surface meets something, so where a ray leaves the screen, finds nothing, is too far or hits a surface seen from behind the sky's reflection stays
as it was (where SSR found nothing the picture is the same as without it to within one level of the 8-bit colour). `strength` (0 to 2) scales the swap, `max_distance` (metres) is the longest a ray goes, `thickness` (metres of depth) how thick things are taken to be: a ray that passes behind something thinner than that is not stopped by it.
A surface fades out of it between roughness 0.3 and 0.7 (rougher surfaces see the blurred sky, which is right for them).

How it is done, three steps beside the main pass: **a surface pass** (after the depth prepass, one sample, full size) draws every mesh again, the see-through ones too, into two small targets: the world normal with the relief of its material and the ripples of water, and the roughness (`Rgba16Float`), and `F0` (0.04 for a plastic, the colour for a metal) and a "there is a surface" mark (`Rgba8Unorm`), with a depth buffer of its own that includes the water;
after the main pass **the reflection pass** (full screen) marches a ray from every glossy pixel in the mirror direction through the depth of the opaque picture, in steps of about two pixels on the screen (at most 96, perspective-correct, jittered per pixel), takes a hit where the ray goes from in front of a surface to behind it within `thickness`, halves in six times to find the place, reads the finished HDR picture there, fades the result by
how near the screen's edge the hit is, how far the ray went and whether the surface there faces the ray, weighs it with the environment BRDF and the fog as the mesh shader weighs the sky's reflection, and writes the **difference** (what was seen minus the sky's reflection) into a layer; **the composite** adds that layer to every multisample before exposure and tone mapping, so the reflections are exposed, tone mapped and anti-aliased with the rest.
The cost is one more draw of the meshes (about the depth prepass's) and the march; `AutoExposure`, shafts and the other picture effects are untouched. Only what is in the picture can be reflected: not what is behind the camera or hidden by a nearer object, not sprites, particles or text. Reflections do not reflect reflections (the ray reads the picture before the layer is added).
A test on the real GPU (`screen_space_reflections_show_a_bright_cube_in_a_glossy_floor_and_in_water`) puts a glowing orange cube above a polished floor and checks that it appears in the floor's picture (1,904 pixels of 129,600 brightened, none darkened, 98.5% of the picture unchanged), that a picture is the same twice, that a rough floor, `classic` and `low` do not change at all, and that a lake reflects it too. `check` warns about a second `Ssr`, a `strength` of 0, numbers that are not above 0, a scene with no 3D meshes and a scene with nothing smooth to reflect in.

## Lens flare and the sun: `LensFlare`

```
{"LensFlare": {"intensity": 1, "ghosts": 5, "halo": 0.5}}
```

One `LensFlare` entity (the first counts; `high` and `ultra` only) adds what a camera lens does with the sun, while the sun (the first directional light) is in front of the camera and in view: a **glare** around the disc the sky draws (the sun disc, `Environment.sky`, is 2.1 degrees across and about 14 times brighter than the light), `ghosts` (0 to 8) soft coloured discs, brighter at their rims, strung out along the line from the sun through the middle of the picture (on both sides of it,
the sun's mirror image too), and a `halo` ring around the middle of the picture that is brightest on the side away from the sun. `intensity` (0 to 4, 1 is a sunny day, scaled by the sun's own brightness and colour) scales everything. The flare is drawn analytically in the composite pass (no textures: a few discs and a ring per pixel, nothing at all for the pixels where they are zero) and added to the light **before exposure and tone mapping**, like the light shafts, so it blooms
into a dark picture and stays pale in a bright one.

**Occlusion**: the depth of the picture is read at 16 points on the sun's disc (its size on the screen follows from the camera: the sky shader's disc is 0.0375 radians across). The share of them where nothing is drawn (sky) is how much of the sun is in view; the flare is that share of itself, so a cube or a hill sliding over the sun makes it dim and then vanish, and a part of the disc outside the picture counts as open. The flare fades out as the sun leaves the picture (a little way outside it still lights the lens: it is gone when the sun is 1.4 half-pictures from the middle),
and there is none when the sun is behind the camera or the camera is orthographic. Only opaque meshes hide the sun (the depth prepass is what is read), not sprites, particles or see-through meshes.
A test on the real GPU (`a_lens_flare_appears_with_the_sun_in_view_and_vanishes_when_a_cube_covers_it`) puts the sun 30 degrees up ahead of the camera and checks that 14,303 pixels of 129,600 get brighter and none darker, that the ghosts and the halo are most of it, that a stronger `intensity` is brighter and `0` changes nothing, that the same picture comes out twice, that a cube between the camera and the sun leaves 0 pixels brighter, that the sun behind the camera draws no flare and that `classic`, `low` and `medium` ignore it.
`check` warns about a second `LensFlare`, an `intensity` of 0, more than 8 `ghosts`, a scene with no 3D meshes and a scene with no directional light.

## Reflection probes: `ReflectionProbe`

```
{"Transform": {"pos": [0, 2.5, 0]}, "ReflectionProbe": {"size": [13, 6, 13], "resolution": 128}}
```

The sky is a fine thing to reflect outdoors and a wrong one in a room. A `ReflectionProbe` stands in the middle of a room or a cave and **captures the world around its position**: six pictures (the faces of a cube, 94 degrees wide each so that filtering at an edge still reads real texels, drawn like the faces of a point light's shadow map) at `resolution` pixels a side (16 to 256), in true HDR light, with the main picture's
shadows and the sky wherever the sky shows. Every surface inside the probe's `size` box (metres, x y z, centred on the entity, not turned by its rotation) then **reflects and is lit by what the probe captured instead of the plain sky**: a metal ball in a red room shows the red room and a floor of a cave is lit by the cave's walls and not by the sky above the rock.

* **Box projection**: a reflection is bent to come from where the ray from the surface leaves the box (the walls of the room, not an infinite distance away), so the reflection of a wall is on the right part of a ball and a floor, whatever the surface's place in the room.
* **Blur**: every face has a full chain of mip levels (box filtered, 2 by 2 texels at a time) and the surface's roughness picks the level (0 sharp, 1 the smallest); the light for the ambient (diffuse) term is read at the 4 x 4 level around the normal.
* **Fading**: inside the outer third of the way to a wall the probe fades out to the sky (so that a doorway has no seam); where boxes overlap, the smaller box wins; a surface outside every box gets the sky as it always did. The weight is 0 outside, so a probe whose box holds no surface changes no picture (to within rounding).
* **Baked, not live**: the six pictures are drawn the first time a picture needs the probe, and again when the probe's position, `size` or the world's biggest `resolution` changes, and on demand with the **`bake_probe`** tool (`uid` for one probe; without it all of them): after you moved things, or loaded another scene. It shows the world as it was captured (moving things are not in it), and **a probe does not see other probes**. Capturing costs six small pictures and their mip levels, a few milliseconds, and is done once.
* **Limits**: at most four probes work at once (the four with the lowest uids; `check` says so), all captured at the same size (the biggest `resolution` asked for), stored as one array of 24 layers of `Rgba16Float` (4 MB at 128, 17 MB at 256); the box is axis aligned. The reference renderer, `classic` and the editor's classic mode ignore probes; a probe changes the picture of the `high` qualities from `low` up.
* **With the other effects**: the screen-space reflections swap the probe's reflection (not the sky's) for what the picture shows inside a box; `AutoExposure` does not apply to the probe's pictures (they hold plain light, the exposure comes after); a probe's pictures have no ambient occlusion, bloom or depth of field.

A test on the real GPU (`a_reflection_probe_gives_a_mirror_ball_the_room_around_it_and_is_baked_once`) puts a mirror ball in a closed room with a red wall on the left and a green one on the right: without a probe the ball shows the blue sky on both sides (93, 124, 182), with one its left shows the red wall (145, 0, 19) and its right the green (0, 155, 30);
the same picture comes out twice and classic ignores the probe; after the left wall turns yellow the ball still shows the red wall it was captured with until `bake_probes` captures it again and then shows yellow; and a probe whose box holds nothing changes no picture. `check` warns about a probe with no `Transform` (an error), a fifth probe, a `size` that is not three numbers above 0, a `resolution` outside 16 to 256 and a scene with no 3D meshes.

## The lighting showcase

`examples/lighting_showcase.lunaris.json` (made by `tools/build_lighting_showcase.py`, with the playtest `examples/tests/lighting_showcase.test.json`) puts the four effects in one world: a bright courtyard of polished dark stone with five metal spheres, columns, two lanterns and a puddle (`Water`), the sun low ahead of the camera, and next to it a dark cellar (a cave-like room 24 x 15.5 metres with a door 6 metres wide, a treasure on a plinth, a chest, a silver ball, four torches)
whose door stands open. One entity each carries `AutoExposure`, `Ssr` and `LensFlare`, and two `ReflectionProbe`s stand in the courtyard (a box of 80 x 14 x 80 metres: the spheres mirror the building and the columns, not only the sky) and in the cellar (24 x 5.5 x 15.5: the metal in there reflects the cellar). W A S D walk the camera (`lunaris play examples/lighting_showcase.lunaris.json`): through the door the picture goes dark and then brightens as the eye adapts,
and coming out it is dazzled for a moment. `examples/lighting_showcase/shot.png` is its `high` picture at 1280 x 720 (the sun's flare, the floor and the spheres mirroring the facade and each other; the exposure the renderer measured is 0.997 for an average luminance of 0.18), `examples/lighting_showcase/inside.png` one from inside the cellar (exposure 1.84 for an average luminance of 0.098), both made on the RTX 2060 SUPER with
`lunaris call render '{"quality":"high","width":1280,"height":720,"path":"examples/lighting_showcase/shot.png"}' --scene examples/lighting_showcase.lunaris.json`. The playtest walks the camera through the door and back and checks the resource `scene.indoors` that a graph system sets from the camera's place: what the eye does is the renderer's business and never enters the simulation (a hash must not depend on the graphics card), so the
`render` reply says it in `measured.exposure` and the GPU test checks it. There is no GPU-against-reference comparison for this scene (the two renderers cannot agree on these effects).

**What the effects cost** (`cargo run --release -p lunaris-gpu --example fx_cost -- examples/lighting_showcase.lunaris.json 1920 1080`, the `high` quality, 1920 x 1080, RTX 2060 SUPER, the median of 150 pictures per variant with the variants taking turns, the read-back of the picture included in all of them: 8.8 ms with none of the effects):

| effect | extra time per picture |
|---|---|
| `AutoExposure` | 0.0 to 0.1 ms (one compute dispatch of 4,096 samples) |
| `Ssr` | 2.5 ms (the surface pass and the ray march) |
| `LensFlare` | 0.3 ms (a few discs and a ring per pixel; the depth is read only where the flare is not zero; the scene's ambient occlusion already makes the depth prepass the flare needs) |
| `ReflectionProbe` (two in the scene) | 0.35 ms per picture, and 2.9 ms once for each probe captured (six faces at 128 pixels and their mip levels) |
| all four | 3.4 ms |

## How a picture is made (high quality)

```
shadow maps  ->  depth prepass  ->  surfaces (SSR)  ->  ambient occlusion  ->  main pass  ->  reflections (SSR)  ->  eye adaptation  ->  bloom  ->  composite (with depth of field)  ->  interface and HUD text
 (3 sun cascades,  (for the AO)      (normals,          (16 samples,          (sky, PBR       (full screen:        (compute: the     (13-tap     (per-sample   (display
  6 faces for                         roughness,         depth-aware blur)     meshes,         rays through the     exposure of the   down /      tone mapping,  colours)
  each of 2 lamps)                    F0)                                      sprites;        depth)               picture)          tent up)    grading,
                                                                               HDR, 4x MSAA)                                                        sRGB, dither)
```

* **Shading**: GGX specular with Smith visibility and Schlick Fresnel, Lambert diffuse, a split-sum approximation for the sky's reflections (a roughness-blurred procedural sky stands in for an environment map), specular occlusion from the AO.
* **Shadows**: the sun has three cascades fitted to the camera's view with the shadow map snapped to texels (no shimmer when the camera moves), 3x3 hardware-filtered lookups, normal-offset bias, blending between cascades. A point light draws the casters inside its range into six slightly-wider-than-90-degree faces (kept as layers of a 2D array, so no cube-map orientation conventions are involved) and the shader picks the face by the dominant axis.
* **Tone mapping happens per multisample**, before the samples are averaged, so bright edges (a lamp against dark sky) stay anti-aliased. The flat background keeps the exact colour you gave it.
* **Colours**: classic mode works in the reference renderer's gamma-2.0 space; high mode uses real sRGB (textures are sRGB textures with mipmaps and 16x anisotropic filtering, sprites stay nearest-neighbour).

## How it stays honest (classic mode)

The CPU renderer is the *reference*: bit-identical on every machine, used by replays and the golden-image tests. The GPU renderer must agree with it, so both draw from **one `Scene`** (`lunaris_render::gather_scene`: the camera, the lights, the lit meshes, the sprite and tile-map layers far to near, the HUD texts, the environment, all gathered and sorted once). `crates/lunaris-gpu/tests/compare.rs` draws nine example scenes in classic mode with both and compares them (pictures go to `target/gpu-compare/`: the CPU picture, the classic GPU picture, an amplified difference, and the high-quality picture for a look):

| scene | mean absolute error (of 255) | pixels differing by more than 40 |
|---|---|---|
| showcase (lit 3D) | 0.03 | 0.00% |
| models (glTF) | 0.07 | 0.00% |
| sprites | 0.52 | 0.02% |
| moon_hopper (tile map, sprite sheets, text) | 0.04 | 0.01% |
| orb_dash (3D game with HUD) | 0.03 | 0.00% |
| clockwork, coin_chase, pile, particles | 0.00 to 0.05 | 0.00% to 0.01% |

The differences are triangle-edge pixels (different rasterisation rules) and 16-bit float rounding. The test fails if any scene's mean error goes above 1 or more than 0.5% of its pixels differ a lot. Another test checks that the entity ids the GPU draws (for `visible` and `pick`) agree with the reference's, in both modes. `crates/lunaris-gpu/tests/high.rs` checks what the high-quality mode promises: flat 2D scenes keep their authored colours, the same world draws the same picture twice, the sun makes shadows, the sky replaces the background and lights the scene, metal reflects the sky and plastic does not, bright things glow, fog hides the distance, ambient occlusion darkens creases, textures repeat.

## Speed

1,600 lit meshes (spheres and cubes), one point of view, this PC (RTX 4060), each picture **drawn and read back to the CPU as an image with its entity ids** (the most expensive way: a window skips the read-back):

| size | CPU reference, 1 thread | GPU classic | GPU high |
|---|---|---|---|
| 960 x 540 | 49 ms | 2.8 ms (18x) | 3.7 ms (13x) |
| 1920 x 1080 | 113 ms | 8.7 ms (13x) | 10.5 ms (11x) |

(`cargo test --release -p lunaris-gpu --test compare how_fast -- --nocapture`.) A small scene at 1280 x 720 in high quality, with sky, shadows, ambient occlusion and bloom, takes about 2.4 ms per picture including the read-back. The native window draws at the display's refresh rate with vsync; the simulation still runs in its fixed 60 Hz steps.

**Many meshes** (`cargo run --release -p lunaris-gpu --example many -- 100000`; spheres and cubes drawn instanced in batches by mesh and texture, 1280 x 720, high quality, read-back included):

| meshes | gather the scene (CPU) | whole picture, high | whole picture, classic |
|---|---|---|---|
| 2,000 | 0.4 ms | 5.2 ms | |
| 20,000 | 3.9 ms | 20.5 ms (49 fps) | |
| 100,000 | 18.5 ms | 73 ms (14 fps) | 56 ms |

At 100,000 meshes the picture costs the same at 320 x 180 as at 1080p: the time is the CPU's per-mesh work (gathering, sorting into batches, filling the object buffer, culling the shadow casters), about 0.5 microseconds a mesh, not the GPU and not the draw calls. The next step if a game needs more is to keep the object buffer between frames and update only what moved.

## Limits

* GPU pictures are not bit-identical between machines: use `quality: reference` where a hash must match (the default for tests and the Python/JS/C clients that embed the engine without the `lunaris` binary).
* Shadows: the sun and the two most important point or spot lights; the other lights (up to 16) light without shadows. See-through meshes cast no shadow and are sorted by their centre (two overlapping panes can swap order). Skinned models are posed on the CPU every frame (fine for dozens of characters, not thousands). No depth of field or motion blur yet.
* Up to 16 lights, like the reference.
* The native window runs on Windows (winit); `lunaris edit` works everywhere, and the renderer itself (`GpuRenderer::headless`) works on any system wgpu supports.
