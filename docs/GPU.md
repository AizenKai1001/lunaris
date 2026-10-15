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

## How a picture is made (high quality)

```
shadow maps  ->  depth prepass  ->  ambient occlusion  ->  main pass  ->  bloom  ->  composite (with depth of field)  ->  interface and HUD text
 (3 sun cascades,  (for the AO)       (16 samples,          (sky, PBR       (13-tap     (per-sample   (display
  6 faces for                          depth-aware blur)     meshes,         down /      tone mapping,  colours)
  each of 2 lamps)                                           sprites;        tent up)    grading,
                                                             HDR, 4x MSAA)               sRGB, dither)
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
