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

On Windows, a machine whose only graphics adapter is a software one (WARP) plays with the CPU renderer instead, and says so: a software adapter is a graphics API run on the processor, and on Mesa's llvmpipe a 960 x 540 picture of 1,600 meshes took 210 ms against 29 to 48 ms for the engine's own CPU renderer (PERFORMANCE.md, "The card"); WARP itself has not been measured. `LUNARIS_GPU=<part of the adapter's name>` keeps the adapter.

The `render` tool takes `quality`: `reference` (the CPU renderer), `classic`, `low`, `medium`, `high`, `ultra` or `auto` (the default: `high` when the host has a GPU, otherwise `reference`; a software adapter such as llvmpipe or WARP does not count as a GPU for this, because the reference renderer is several times faster than it, and the reply says so: name a quality to use the adapter anyway). Its reply says which renderer drew the picture (`renderer`, `quality`) and whether the picture is `deterministic`: only `reference` pictures are bit-identical on every machine; a GPU picture is the same to the eye but not to the last bit (graphics cards round differently), so hash and golden-image tests use `reference`. `visible`, `coverage` and the entity ids work for every quality (the GPU draws an entity-id picture next to the colour one).

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

**`Weather`** (the first one by uid counts; `rain` 0..1 draws up to 36,000 falling drops, stretched along their fall, in a box round the camera, placed by the vertex shader alone and depth tested (parallax, hidden behind buildings), for a fraction of a millisecond; `wet` 0..1 darkens and glosses flat ground in puddles, walls a little; a game sets them from its own weather and puts the rain out indoors; the reference renderer and `classic` draw neither). **`Environment`** (the first one by uid counts; colours are 0..1 display colours): `sky` (a procedural sky: `sky_top` at the zenith, `sky_horizon`, `ground` below the horizon, a sun disc where the first directional light shines from, `clouds` cover 0..1), `ambient` (how strongly the sky lights the scene and shows in reflections), `exposure`, `fog` (density) with `fog_color`, `shadows` and `shadow_distance` (how far from the camera the sun's shadows reach), `ssao` (ambient occlusion strength 0..2), `bloom` (0..2) with `bloom_threshold`, `vignette`, `saturation`, `dof` (0..1) with `focus` and `focus_range` (depth of field: sharp around `focus` metres from the camera and `focus_range` metres either side of it, blurrier the further off; `high` and `ultra` only; `examples/depth_of_field.lunaris.json`), and `tonemap` (0 automatic, 1 filmic ACES, 2 neutral, 3 none). Without an `Environment` a plain look is worked out from the camera's `clear` colour; a flat 2D scene (no 3D meshes) gets no effects at all, so its colours come out exactly as authored.

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

A sheet of water at the entity's height, `size` metres wide and deep. In the high qualities it is see-through (`color`'s alpha) and reflects the sky and the sun's glints in a sea of waves. The reference renderer and `classic` draw a flat sheet of its colour.
The clock `phase` runs in the simulation, so a replay shows the same water and the water stops when the world is paused with `time.scale`; `speed` is the time scale of the sea (1 = real time: `phase` counts seconds and every wave goes at the
speed of deep water, omega = sqrt(g k)). Water does not push anything: a trigger `Collider` of the same size tells which things are in it, and `Floater` bodies ride the very surface that is drawn (the same numbers, in `floaters.rs`).
For rivers and lakes use several sheets; a very large sheet gets a coarser mesh (up to 400 x 400 cells).

### What the sea is made of

`wave_height` is the **significant wave height** Hs (the mean of the highest third of the waves; a sea of Hs 1 m has crests about 0.6 m above its level now and then, and never more than 1.3 m: `Floater` uses that as the top of the surface) and `wave_length`
is the length of the **peak** of the spectrum (the waves that carry most of the energy). The surface is a sum of waves sampled from a real ocean spectrum, not hand-picked sines (`tools/gen_ocean.py` makes the table, and writes it into the shader and the simulation, which must agree):

- **JONSWAP** (Hasselmann et al. 1973: a Pierson-Moskowitz shape, `x^-5 exp(-1.25 x^-4)`, with the peak raised by 3.3 and sigma 0.07 below the peak and 0.09 above) says how much energy each frequency has. The amplitudes are found by integrating it over a bin of
  frequency, so the surface has exactly the variance the spectrum says (a test checks that Hs, measured as four times the standard deviation of the surface, comes out within 10% of `wave_height` for seas of 0.5, 2 and 4 m).
- **Directional spreading** after Hasselmann et al. 1980 / Mitsuyasu: `cos^(2s)` of half the angle off the wind, with `s = 11.5 x^5` below the peak and `11.5 x^-2.5` above it (x = frequency over the peak's), so the long waves run in a narrow band of directions round the
  wind and the short ones go every way. One wave per bin, its direction drawn by a low-discrepancy sequence and its phase at random from a fixed seed.
- Twenty-five waves (from 2.6 peak lengths down to a seventh of the peak's, crowded round the peak itself) move the mesh and the floating bodies; twenty-four finer ones, down to millimetres, with the surface tension in their speed
  (omega^2 = g k (1 + k^2/km^2), km = 363 rad/m), only tilt the normal. Their slopes follow the equilibrium range of the spectrum (the same slope per octave), so the mean square slope of the whole sea is about 0.8 (Hs kp)^2, the size the Cox-Munk
  measurements of the real sea give (0.003 + 0.005 U, about 0.04 at 8 m/s). The amplitudes are Rayleigh-distributed (as the components of the real sea, and of the FFT oceans, are) and then scaled back to exactly the energy of the spectrum; the seed of the draw is
  the first one in which no wave owns more than a sixth of the energy and the strongest five do not all run the same way. A handful of plane waves of equal height crossing makes an egg-carton lattice that the eye finds at once.
- **A domain warp** hides what is left: every wave is evaluated at a point bent by two slow sines (up to a fifth of the peak length, over 4 to 5 peak lengths), so that crest lines curve gently and the local wave length varies by a quarter, and the pattern of the crossings never repeats
  (the chain rule takes the normal and the Jacobian back to the unbent point; the simulation does the same bend, so bodies still float on the very surface that is drawn).
- Every wave is a **Gerstner wave** (Tessendorf 2001, "Simulating Ocean Water"): the points of the surface also move sideways towards the crests (the "choppy" displacement). It is the second order of the real nonlinearity: crests sharpen, troughs flatten. The shader puts the
  vertices of the mesh at `u + D(u)` and works the normal out at the point that rests at `u` (the mesh carries it in `uv`), as the cross product of the tangents of the displaced surface. The simulation undoes the pull with one step before it adds the heights;
  a test checks the copy against the same sea solved to convergence in double precision (within 5% of Hs, and it is nearer to the choppy sea than to the straight one, bend included).
- The **Jacobian** of the sideways displacement falls towards zero where the crests fold over: that is where waves break, and `lit_water` makes foam of it (the `foam` of `WaterLook` scales it), in tufts that a noise breaks up and that drift with the water. A calm sea has none; a steep one has whitecaps on its steepest crests, the way the real one does
  (Tessendorf's own suggestion in the notes above, and the practice of the FFT oceans in games).
- **Roughness from the waves too small to see.** A wave finer than a pixel is faded out of the normal (it would only shimmer), and the mean square of its slope is added to the surface's microfacet roughness (GGX alpha^2 = alpha0^2 + slope variance): the sea is a mirror close to the eye and a soft broad glitter
  far away, with no moire (Bruneton, Neyret and Holzschuch 2010, "Real-time Realistic Ocean Lighting using Seamless Transitions from Geometry to BRDF"). The reflection of a facet that tips away from the eye is folded above the horizon (it would be hidden by the waves in front of it, and what it shows is the sky).
- **Caustics** on the bottom (`lit_splat`) gather the sun's light into lines, they do not add any (the pattern is divided by its mean, 0.2), lose their edges with depth, and are their own average when the net of 4 m cells is smaller than a pixel. (Fixed a bug here: the pattern's divisions need the phase taken from -250 as in Dave Hoskins' original shader or they
  blow up to infinity, which showed as a white disc over the shallows.)

- **The shore follows the waves.** The terrain's shader (`lit_splat`) works out where the waves put the surface (`frame.p29` carries the sea's Hs and peak length) and wets and darkens the sand against that, not against the mean level: the swash runs up and down the beach with the waves, and a trough
  that is lower than the sand leaves it wet instead of drawing a patch of 'underwater' ground.

- **Light through the crests** (subsurface scattering, the practice of Atlas and Crest): `lit_water` adds the colour of the shallows times the sun, strongest when the eye looks towards the sun, on the sides of the waves that face away from it and near the crests (`g_crest`): the green-gold glow of a wave against a low sun.
- **Shoaling and breaking** where the water gets shallow. The renderer makes a window of the sea's depth round the eye from the terrains (128 x 128 points four metres apart: the level of the sea minus the ground, as 16-bit floats in the frame uniform; a texture would be one too many for the painted terrain's pipeline, which already
  uses all sixteen a portable device has; it is made again when the eye moves by a cell), and the waves read it (`ocean_shoal`): the height of a wave grows as the depth to the power minus one quarter once the depth is less than half the wave length (Green's law: the energy flux is kept while the wave slows),
  and the sea cannot keep a significant height of more than about half the depth (the surf zone saturates at Hs of 0.4 to 0.6 of the depth; individual waves break at 0.78): so the waves swell as they near the beach, are cut at half the depth, and fall to nothing at the waterline, where the terrain's swash takes over. Where the unlimited
  height would exceed about a third of the depth the crests carry surf foam (`g_break`, `g_crest`), in tufts. The mesh, the normal, the roughness, the terrain's wet sand and the foam all use the same depth. Only water at the level of the sea whose depth the window holds shoals (a lake higher up, or a river, keeps its waves), and `Floater` bodies on that sea ride the same shoaling waves (the simulation reads the terrains' heights itself, with the same formula; a test puts a plank at 5 m of water and at 0.75 m: the waves under it are a third as high). What is not done: the wave length does not shorten and the crests do not bend towards the shore (refraction), and the waves do not reflect.

Still to come for the water, in order: absorption of each colour of light by the depth (Pope and Fry) with a refraction pass, refraction and shorter waves in the shallows (a travel-time field over the depth), ripples left by bodies (the iWave height-field of Tessendorf 2004, "Interactive Water Surfaces"), the view from under the surface.

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

## Real surfaces: `PbrMaps`

A mesh with a `PbrMaps` beside its `MeshRenderer` gets the detail of a photographed material (high quality only; the reference picture and classic mode ignore it). `normal` is a tangent-space normal map (a PNG or JPEG of the workspace, **DirectX** convention: green points down the picture, what Poly Haven's `nor_dx` and Unreal's maps are) and `arm` a map with ambient occlusion in red, roughness in green and metallic in blue. Both hold numbers, not colours: they are never converted from sRGB, and they repeat `MeshRenderer.uv_scale` times with the colour texture. `normal_strength` scales the bumps (1 as authored), `ao_strength` (0 to 1) how much the occlusion map darkens the ambient light, `roughness_scale` multiplies the map's roughness; a name left empty is not used. No tangents are needed in the model: the frame comes from the screen-space change of position and picture coordinates. `examples/hollowlight/` uses it with the CC0 sets `tools/fetch_polyhaven.py` downloads.

## Water that knows its depth: `WaterLook`

Beside a `Water` sheet or a `WaterBody`, a `WaterLook` makes the water read the depth prepass: for every piece of the surface the picture knows how far it is from the opaque thing behind it, and so how thick the water is there. The water's own `color` is what it is in the deep; where it is thin the colour fades to the `shallow` one and the water becomes see-through (about two thirds of the bottom is hidden by `clarity` metres of water), so a beach, a rock or a post stands in clear water that darkens as it deepens. `foam` (0 to 1) and `foam_width` put a band of white on the water's edge wherever it touches something, broken up by a noise that drifts with the waves. Reflections are the job of `Ssr` and the probes as before, and the sun's glitter that of the surface's roughness. It works in every high-mode quality (the prepass is made for it) and a flat sheet (`wave_height` 0) takes the same path. `tools/build_gfx_water.py` makes a pond (`examples/gfx_lab/water_pond.png`).

## Temporal anti-aliasing

The `high` and `ultra` qualities (`Quality::taa`) draw every picture with the pixel grid shifted by a fraction of a pixel (a point of the Halton sequence, a different one each time) and blend it with the pictures before: a pixel's colour becomes the average of many samples, which smooths edges beyond what 4x multisampling can, calms the shimmer of thin things (grass, wires, parallax-mapped surfaces) and averages away the noise of the soft shadows and the bounced light. The history is looked up where the surface was in the last picture (the matrices without the shift, so a still camera finds the same pixel and a moving one follows the surface), and kept inside the range of colours the pixel's neighbours span (so what changed is not smeared), and a history that had to be pulled in counts for less. The film grain and the dither are added after the blend (they would be averaged away before it). A still made by `render` is drawn eight times with the same eight shifts and averaged, and eye adaptation does not advance between them, so a still made twice is the same picture. The first picture after a resize is not blended.

## A sky made by the air: `Atmosphere`

`Atmosphere` replaces the painted sky gradient with the physics of one: the air is a layer of molecules that scatters blue light (Rayleigh, 8 km deep) and a layer of haze and dust (Mie, 1.2 km), and the light of the sun is scattered towards the eye along every line of sight (Nishita's single scattering, summed in 16 steps along the view ray with 6 towards the sun at each, for every pixel of the sky). The sky is blue at noon with a pale horizon, the sun reddens and dims as it gets low, the sky glows round it, and after sunset it goes dark and the stars come out: all from the direction of the first directional light, the sun. The ambient light (the sky's colour at the zenith and round the horizon), the fog colour (brighter and golden looking towards the sun: `haze_glow`) and the colour of the sun's light (1 at noon as authored on its `Light`, redder and dimmer towards the horizon, none when it is down) follow the same numbers (`lunaris_render::atmosphere` does the sums for those few directions). `turbidity` is how hazy the air is (1 very clear, 2 clear, 6 hazy: haze whitens the sky and brightens it round the sun), `rayleigh` scales the molecules' density, `mie_g` how far forward the haze throws light, `intensity` how bright the sky is. `tools/build_gfx_sky.py` makes four pictures of one plain at noon, in the afternoon, at sunset and at dusk (`examples/gfx_lab/sky_*.png`); with `AutoExposure` the eye follows the light down into the evening. High quality only; the reference picture and classic keep the `Environment`'s colours.

## Bounced light: `GlobalIllumination`

**Ambient occlusion** (`Environment.ssao`, `high` and above) is ground-truth ambient occlusion, a horizon search: for every pixel three slices through the point are taken, each along its own direction on the screen (turned a little differently every picture, so that the temporal anti-aliasing averages the noise away), and along each, eight steps on either side (spread over a radius of about half a metre plus 2.5% of the distance, at most four metres) find the highest horizon the depth of the picture makes. How much of the surface's cosine-weighted half circle lies above both horizons is added up exactly (not by counting samples), so open ground sees its whole half circle and stays unshaded, and the foot of a wall or the inside of a crease loses the part below the horizon, smoothly. A surface far in front of the point stops counting at the edge of the radius (a falloff, so a post in front of a wall does not leave a halo), and the result goes through the depth-aware blur that the bounced light uses too. `ssao` 1 is as drawn by the real horizon search, 2 half again as strong.

**Clouds with depth.** The `clouds` cover of the `Environment` is drawn in the sky pass as a slab of cloud, not a flat picture: six steps through it along the view ray, the noise that says where the clouds are passing a threshold that rises towards the base and the top (so a cloud is flat underneath and domed on top), and at each step a look up the slab towards the sun for how much cloud lies between there and the light. What is thick and shaded is grey, what faces the sun is bright, and the thin edges next to the sun glow (the silver lining); the steps are jittered a little differently at every pixel, which the temporal anti-aliasing averages. Reflections of the sky in glossy surfaces, the sky's light on meshes and the fog use the cheap flat layer (the slab is the sky pass's own, so a thousand shiny things do not each march a cloud). `examples/gfx_lab/sky_clouds_noon.png` and `sky_clouds_sunset.png` are made by `CLOUDS=0.65 python3 tools/build_gfx_sky.py`.

**More surfaces: clear coat, sheen, subsurface.** A **`MaterialExt`** beside a `MeshRenderer` (high quality and above; the reference picture and classic mode ignore it) adds the three looks that metallic, roughness and maps cannot make. `clearcoat` (0 to 1) puts a second, smooth lobe over the surface, as on car paint, lacquered wood or wet paint: a sharp sun highlight and a mirror of the sky (`clearcoat_roughness`, 0.02 to 1, 0.05 glass-smooth) over a duller base, which it dims by the coat's Fresnel (more at grazing angles). `sheen` (0 to 1) is the soft glow at the rim of velvet, felt, satin and fuzzy leaves (the Charlie distribution, tinted a little by the base colour; face-on it counts a fifth, so velvet stays dark in front and glows at the edge). `subsurface` (0 to 1) wraps the light a little way round the terminator and adds the colour the body gives it there (`subsurface_color`: reddish for skin and wax, yellow-green for leaves), so the shaded side of a lit cheek is warm and not dead. They cost a few operations a light and nothing for a surface without the component; the numbers ride in two spare words of the object data, a byte each. `examples/gfx_lab/materials.png` (made by `tools/build_gfx_materials.py`) shows plain, coated, velvet, wax and leaf.

**Grass made of blades.** A `GrassField` with no `mesh` is made of single blades, the way shipping games do it (what the first version did, crossed cards of a texture, looked like a doormat). Researched 2026-10-16: the talk
"Procedural Grass in Ghost of Tsushima" (Sucker Punch, GDC 2021) and the write-ups and re-implementations of it (<https://csarko.sh/research/grass-and-trail-realism>, <https://jairsantana.com/posts/grass/>), AMD GPUOpen's procedural grass
(<https://gpuopen.com/learn/mesh_shaders/mesh_shaders-procedural_grass_rendering/>, after Jahrmann and Wimmer, "Responsive Real-Time Grass Rendering for General 3D Scenes", i3D 2017) and Horizon Zero Dawn's and The Witcher 3's grass as
described there. What was taken from them:
* **A blade is a curve, not a card.** A cubic Bezier from the foot to the tip (nearly upright at the foot, leaning over towards the tip), a ribbon that tapers to a point: 15 vertices near the camera, 9 and 5 further out (Tsushima: 15 and 7). No
  texture: the shape and the colour are the whole blade. A bundle of four blades is one instance of a ring of the lattice that follows the camera; nothing is stored, every vertex works out its blade from a hash of its cell.
* **Clumps, not noise.** The blades of a Voronoi clump (the nearest of a jittered lattice of points, `clump_size` metres across) share their height, lean, heading and tint; a blade is `(1 - clump) x its own randomness + clump x its clump's` (Tsushima,
  GodotGrass: 0.8). That is what makes a field look like a field: tall and short patches, a lean in a common direction, pale and dark tufts, where a per-blade random value alone gives "the same tuft everywhere". A few tall stems stand out among them.
* **Round, two-sided, soft.** The normal is the ribbon's, bent outward across its width (so a flat strip shades like a round blade), tilted a quarter of the way to the sky (GPUOpen's 25%), and turned to face the eye (a blade is lit from both sides, as
  in Horizon, which takes `abs()` of the view-space normal). A blade seen edge-on is widened a little in the screen's direction so that it does not vanish into a sliver (Tsushima does the same).
* **Colour and occlusion.** A gradient from `base_color` at the foot to `tip_color`, with the foot darkened by the ambient occlusion of a dense field (GPUOpen: `((y - root) / height)^1.5`; GodotGrass: `mix(1 - 0.75 d, 1, h^2)`), the field's patches of
  lush and dry (`color_a`, `color_b`), and a share of the clump's own tint. The ground under the grass should be dark: the shade between blades is the field's own.
* **The sun through the blades** is added where the sun reaches them (the shadow of a rock falls on blades), in the blade's colour, growing towards the tip and when you look towards the sun (Horizon: light from behind x view/light angle x thickness).
  Their highlights are dull (a blade is a diffuse thing).
* **Wind.** One field: a gust that travels across it (a long wave along the wind with a slow wobble across it) and a flutter of the blade's own, phased by its place (the sway Horizon and GPUOpen use, a sine of the position along the wind plus time); the foot
  stays, the top leans by the square of the height along the blade.
* **Level of detail.** Three rings (near, middle, far); the further ones hold fewer bundles per square metre and their blades are wider by the same factor, so a distant field is as full as a near one (GPUOpen scales the width by `maxBlades / blades`;
  Tsushima drops three of four blades per doubling of the tile). Only the width grows with the distance, not the height (scaling all three axes makes a far field look dotty). At the rim of a ring a blade sinks into the ground and the next ring's blades
  take over, so nothing pops.
* **Shadows.** Grass is not put into the shadow maps (every game that ships it says the same: it costs far too much); the ground's shadow on it works, and the blades' own darkness at the foot stands for the rest.
Cost: a few million vertices a picture for the default field (`radius` 150 m, `density` 20: 80 blades a square metre near the camera), on the card, with no entities and no CPU work.

**No two places alike.** A texture repeated many times shows its pattern: a lawn of 1,200 repeats of one photograph has a grid you can count. Researched the same day: Mikkelsen, "Practical Real-Time Hex-Tiling", JCGT 11(3), 2022 (after Heitz and Neyret,
"High-Performance By-Example Noise using a Histogram-Preserving Blending Operator") and Quilez's three techniques (<https://iquilezles.org/articles/texturerepetition/>); Unreal's landscape macro-variation as the cheap alternative. The renderer uses
**hex-tiling**: the plane is cut into the triangles of a skewed grid, each of the three corners of a triangle gives the texture a random offset (and a random turn), and a pixel is the blend of three copies, weighted by the closeness to each corner (raised to
the seventh power, so the blend is narrow and the photograph's contrast survives), by the brightness of each copy (so a dark patch does not wash a bright one out) and a contrast curve. Every fetch uses the turned derivatives (`textureSampleGrad`), so
mip-mapping and anisotropic filtering are right and there are no seams. Three fetches a layer instead of one.
* **Painted terrain** (`TerrainLayers`): every layer is hex-tiled (colour and relief, with a turn), always, in the high qualities. The reference picture and `classic` are unchanged.
* **Macro variation** on the same ground: three scales of noise (170 m, 31 m, 6.5 m) darken and lighten it by up to 16 per cent, which breaks up the wide stretches of one shade that no tiling trick can (the macro-variation overlay of Unreal's landscape materials, reached by the same search; <https://www.versluis.com/2023/05/how-to-use-macro-variations-for-unreal-engine-landscape-materials/>).
* **Meshes**: `PbrMaps.untile` (it also works with no maps, beside a `texture`) hex-tiles the texture, the normal map, the occlusion-roughness-metallic map and the height map with the same weights, without turning them (a turned normal map would need its
  normals turned too). Leave it off for a pattern that should repeat, such as bricks laid in rows.
* **`MacroVary`** (a component beside a `MeshRenderer`; high quality and above; default off, so nothing existing changes): the far-away half of the problem. A facade or a street that repeats one small texture shows one grey over a whole block whatever hex-tiling does to the small picture. The surface shader multiplies the colour by smooth value noise made from the **world position** at three scales (`scale` metres, a fifth, a twenty-fifth), so it needs no UV layout, the next piece of the same material carries on the same patches, and a wall and the road at its foot do not disagree about where a patch ends. `strength` (0 to 1) is the brightness variation (0.25: about a tenth either way in most places, a third at the extremes), `tint` leans the colour warm in some patches and cool in others, `rough` makes darker patches rougher and lighter ones smoother. It is the macro-variation overlay of Unreal's landscape materials (the technique above, which the painted terrain already has built in) for ordinary meshes, and it costs a few noise lookups a pixel on the surfaces that have it. The noise is still, so TAA and stills are steady. Test: `macro_vary_breaks_up_a_flat_colour_in_big_patches_and_leaves_a_surface_without_it_alone` (a flat grey ground: block-to-block spread under 2% without it and several times that with it; all three numbers at 0 give the same picture bit for bit; the same picture twice; the colour lean differs from block to block).
The test `untile_breaks_up_the_repetition_of_a_texture_and_leaves_it_alone_otherwise` makes a noise texture, repeats it three times over a plane and measures how much one period of a row resembles the next: above 0.85 plain, at least 0.3 less untiled.

Light that bounces: a lit wall tints the floor beside it in its own colour, a bright lamp lights the room it hangs in, a shaded corner is not black. `GlobalIllumination` (the first enabled one counts; high quality and above, where `ssao` is on) works it out on the screen from what the **last picture** showed: for each pixel `directions` rays, cosine-weighted about the surface, are marched through the picture's depth for up to `radius` metres, and where one meets a surface (`thickness` metres of slack behind its face) the light that surface sent out last time comes back along it (clamped to 8 times white, found where the surface was in that picture, so a moving camera is right). The result goes through a depth-aware blur and is blended with the last pictures' (a running average of at most the last eight), then the surface shader adds it as diffuse light in the colour of the surface. Every picture's bounce is in the next one's, so the second bounce comes by itself and the light settles in a few pictures; a still made by `render` is drawn seven times first (the same seven every time, so a still made twice is the same picture). `intensity` scales it.

What it cannot do: it knows only what is on the screen (a lit wall behind you does not light the floor in front of you; what the picture does not show is lit by the sky and the probes as before), and it gives diffuse light only (the metal and glossy reflections stay the job of `Ssr` and the probes). A room with no `Ambient` and one lamp is the test: `tools/build_gfx_lab.py` makes the Cornell box (`examples/gfx_lab.lunaris.json`) and `crates/lunaris-gpu/tests/high.rs` checks that the floor beside a red wall turns red and the shaded side of a block is lighter with it.

## Soft shadows: `Shadows`

`Shadows.softness` (the sun's apparent size as a share of the distance: 0.0093 is the real sun, 0.03 a look that shows well; 0 keeps the plain filter) turns the sun's shadow filter into contact-hardening shadows (PCSS): sixteen taps look for what blocks the light, the distance to the blockers gives the width of the penumbra there, and sixteen more taps filter as widely as that (turned by noise at every pixel). A shadow is sharp where it starts and softer the further it falls from what casts it.

## Fill, bounce and lamps: `Lighting`

Found by rendering a game's city on a Linux test machine at noon, dusk and midnight (`tools/` of that game; the pictures differ by the hour only): in a canyon of tall buildings the sun or the moon never reaches the street, the sky is dark after sunset, and the engine's light from the sky is only the sky's own colour, so a street at night was **pure black** (the whole lower half of the picture), the shaded side of everything was flat, and a lamp lit a broad flat pool. `Lighting` (the first enabled one counts; high qualities; a world without it is lit exactly as before, and a disabled one changes nothing, to the bit) is what a game adds for that:

* **`fill`** (a colour, linear, before the exposure; default `[0.02, 0.025, 0.04]`): the lowest light a surface gets from the sky whatever the hour, a night sky's glow and the glow of a city, multiplied by the surface colour, the ambient occlusion and `Environment.ambient`. It is added to the sky's irradiance, so it also shows in what a rough surface reflects of the sky.
* **`bounce`** (default 0.18): the share of the sun's (or the moon's) light that comes back as ambient light from the ground and the walls: most on faces that look down or sideways, little on the ground itself (0.6 - 0.4 n.y, clamped to 0.2..1), more the higher the light is, none when it is under the horizon. It lifts the shade where no direct light reaches, in the colour of the light (a warm sun gives a warm shade, the blue moon a blue one). It is a cheap stand-in for light that bounces; the screen-space `GlobalIllumination` does the real thing for what is on the screen and costs 1.6 ms on a GTX 1070 at 1600 x 900.
* **`lamp_falloff`** (0 to 1, default 1) blends the lamps' old falloff (a window `(1 - (d/range)^2)^2`: a broad, flat pool that fades to nothing at the range) into **inverse-square light** from a soft core of `lamp_core` (default 0.12) of the range, with Karis' window `(1 - (d/range)^4)^2` so that it still reaches zero at the range, scaled so that the two curves are equal at 60% of the range (at the pool's edge the picture is as bright as before; the middle is brighter and the tail longer: a bright core and a long tail, as a lamp has). 0 is the old curve to the bit (a test).
* **`shadow_distance`** (default 200 m, 0 leaves it): the sun's three cascades reach at least that far (the `Environment`'s 80 m is kept when it is larger). The far cascade's texel is then about 20 cm on `high` (2048 texels across a slice that is about 400 m wide). Measured in the city (GTX 1070, 1600 x 900): the shadow pass is 2.6 to 2.8 ms with the reach at 80 m, 200 m and 400 m (what a cascade's box cannot see is culled), so the reach costs nothing worth saying; what a bigger reach costs is texel size.
* **`softness`** (default 0.03): the sun's apparent size for soft shadows (`Shadows`) when the world has no `Shadows` component of its own (a `Shadows` component wins; 0 keeps the plain filter).
* **`point_shadows`** (0 to 4, default 4; the renderer's own, with no `Lighting`, is 2): how many point or spot lights cast shadows, the most important ones (see below) first. Each costs six faces of the point-light shadow map (4 MB a face at `high`'s 1024; the map grows to six faces for each light that can cast, and keeps what it has): in the city on the GTX 1070 the point-shadow pass was 0.65 ms with two lamps and 1.4 ms with four. A test lets three lamps each throw the shadow of a cube: with two the third lamp's floor is lit where the cube should shadow it, with four all three have the shadow.

a game's city at midnight, 1600 x 900 on the GTX 1070, `high`: no `Lighting` and the crosswalk and road are black; with the default `Lighting` they are visible (dark blue-grey) and the picture keeps its night; the street lamp in `lamp` and `lampdusk` views is a pool with a hot core and a tail instead of a flat disc. `LUNARIS_GPU=1070 lunaris ...` chooses the card by name on a machine with two (the picture above was made while the other card ran something else).

**Also changed for every picture** (found by the same renders): the sun's **shadow bias is a distance now** (2 cm plus a texel and a fifth, divided by the cascade's depth range in metres): it was a constant 0.0004 of the cascade's depth range, and that range grows with the biggest caster in the scene (a ground a kilometre across makes it more than a kilometre), which put the bias at half a metre and floated the shadows of poles and feet off the ground ("peter-panning"); **soft shadows use 8 + 12 taps** (it was 16 + 16, 1.7 ms of 4.6 saved on the card above) and turn their pattern a different amount every picture when temporal anti-aliasing is on, so that the taps add up over the pictures; **which sixteen lights** is now by how much of the picture a light lights (below).

## Light in the air: `Volumetrics`

A `Volumetrics` makes every point and spot light visible in the air: from the camera to the first surface it sees (at most `max_distance` metres, in `steps` steps) the picture adds up the light of each lamp that reaches the air there, scattered towards the viewer. `density` is how thick the air is per metre (0.01 a faint haze, 0.05 a fog), `anisotropy` (0 to 0.95) how much the air throws light on towards the viewer (0.6 makes a lamp glow when you look at it and a flashlight show its beam), `intensity` and `color` scale and tint it. It is not shadowed (the light stops at the first surface seen, but a lamp behind a thin wall still glows in the air next to it); the sun is not in it (`PostFx.shafts` is its own). High quality only.

## Which sixteen lights

The GPU renderer lights each picture with at most sixteen lights. It takes the sun and the other directional lights first, then the point and spot lights **that light most of the picture**: a light's brightness times the square of the share of the view its reach covers (`range / max(distance - range / 2, 1)`). A lamp by the camera is the most important, a lamp across the street less, and a big bright light over a far tower **more than many small lamps nearer you** (it was "nearest the camera first", which dropped a 600 m light over the Core of a game's city behind fifty street lamps, so the tower it was meant to light stayed black). The choice changes smoothly as you walk (a lamp's light is zero at its range). The reference picture and the classic mode keep drawing the first sixteen by entity number. A test (`a_big_bright_light_over_a_far_wall_is_kept_among_many_small_lamps_nearer_the_camera`) puts forty small lamps by the camera and a big light 340 m away that lights a wall: the wall is as bright as with ten lamps.

## Depth in a flat picture: `PbrMaps.height`

Give `PbrMaps` a `height` map (white = high; Poly Haven's `disp`) and `height_scale` and the surface gets parallax occlusion mapping: a ray march through the height map that shifts what you see by how high it is, so carpet, tile and brick have relief that changes as you move.

## Grass made on the card: `GrassField`

A `GrassField` puts grass on a `Terrain` without a single entity for it: round the camera, up to `radius` metres, the GPU lays tufts of the model `mesh` (a `.glb` of a few crossed cards of grass, standing on y = 0 and about 0.6 m tall, with its picture as a material so it is cut out) on the ground at about `density` tufts per square metre near the camera. The ground is read from the terrain's heightmap on the card, so the tufts follow it. Three rings (fine tufts near, bigger ones further out) overlap and swap by scale, so nothing pops. Every tuft is turned, sized (`size`, the smallest and biggest scale) and tinted (between `color_a` and `color_b`, multipliers of the picture) by hashes of where it stands, and is darker at its foot (the grass at the bottom of a field is in shade); its normal leans with the wind. The wind is a gust that travels across the field: `wind_amount` metres of lean at the top, `wind_wavelength` metres between crests, `wind_speed` crests a second, going `wind_dir` (radians about up); the foot stays and the top leans by the square of the height, and every tuft flutters a little by itself. `phase` is the engine's clock, so replays show the same wind. Only the GPU renderer's high qualities draw it (the reference picture ignores it). Hundreds of thousands of tufts cost vertices and no CPU, and the scene file stays a few lines (`tools/build_hollowlight_hills.py` makes a 4 km field of smooth hills and its grass in 19 entities).

**On Direct3D 12 (checked 2026-10-26).** The high-quality tests (`cargo test -p lunaris-gpu --test high`, 49 of them) were run on Windows 11's software Direct3D 12 adapter (WARP, a four-core virtual machine): 48 pass in 377 seconds, among them the card-simulated particle emitters, soft particles, the particle lights and the example effects. One fails: `a_field_of_single_blades_is_full_of_detail_and_a_bare_one_is_not`; a `GrassField` of single blades draws the same picture as a bare field there (the blades are not drawn; the same test passes on Vulkan on two NVIDIA cards and on Mesa's llvmpipe). Not investigated yet, and not run in the public CI (the runner's software adapter takes minutes for a single high-quality picture, so only the classic-quality comparisons run there). A game that uses grass on Direct3D 12 should look at it before relying on it.

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

## Models with several materials

A glTF model keeps what its materials say. The importer (`crates/lunaris-render/src/gltf.rs`) makes one mesh of the whole model, as before, with the base colour factors on the vertices and the first base colour picture (this whole is what physics and the rays read, and what the reference renderer and `classic` draw); and when the materials differ in more than a colour (two base colour pictures; a normal map or a metallic-roughness map on any of them; an alpha cut-out) it also keeps one mesh per distinct material (`Mesh::parts`, not for models with a skeleton), each with its own picture, its normal map and its metallic-roughness map (glTF's roughness in green, metallic in blue, occlusion in red: the renderer's `arm`). The scene gatherer draws the parts instead of the whole (unless the entity names a `texture` of its own, which replaces them all), the GPU renderer's high qualities light them with the maps (as a `PbrMaps`, unless the entity has one). `alphaMode` `MASK` cuts the surface where the base picture's alpha is under `alphaCutoff` (and `BLEND` is shown as a cut-out at 0.5, with a warning): in the main picture, in the depth prepass and in the shadow maps, through their own pipelines (`DepthPreCut`, `DepthShadowCut`: a fragment stage that reads the picture and discards), so the shadow of a leaf is the leaf's and not its card's; a two-sided cut-out is drawn from both sides by doubling its triangles. A test draws a flat square whose picture is clear and checks the picture is that of a scene without it, shadow and ambient occlusion included. `tools/build_gfx_trees.py` shows a Poly Haven sapling both ways (one texture for everything, as `prepare_polyhaven_models.py` makes it, and the original file with its six photographs).

## Culling: what is out of the view is not drawn

Every pass leaves out the meshes whose bounding sphere is wholly outside its view volume: the main picture, the depth prepass, the reflection and id passes use the camera's six planes (`Frustum` in `util.rs`, made from the view-projection matrix, with a margin of a pixel or so for the temporal anti-aliasing's shift); each of the three shadow cascades uses the four sides of its box (a caster between the light and the box still throws its shadow in, so the near and far planes do not count). Each item gets a mask of the passes that see it (bit 0 the camera, bits 1 to 3 the cascades; casters only) once a frame, before the batches are made: an item that no pass sees is not drawn or even uploaded, and the batches are made by mesh, texture, level of detail and mask, so that a pass draws whole batches (a batch is one instanced draw): a hundred thousand scattered objects are a few draws and not thousands (an earlier version split each batch into runs of the instances a pass could see, which does that badly). See-through meshes keep their far-to-near order. A sphere that only touches the volume is drawn, and the spheres of swaying and rolling things are grown by the most they move. The picture is the very same with and without it (a test draws 100 cubes both ways and compares every byte, and checks that most of them were left out; `GpuRenderer.culling` is the switch, `LUNARIS_NO_CULL=1` sets it from outside, `last_cull` says how many of the object list the camera did not see). Measured on a Linux test machine's RTX 2060 SUPER with `tools/bench_cull.py` (1280 x 720, high, the camera on the edge of a field looking along it): 1600 Poly Haven rocks 174 ms without culling and 61 ms with it (2.84 times; with the levels of detail below, 39 ms and 34 ms); 2500 cubes, which cost nothing to draw, 39 ms either way (the cost is in the passes that always run).

## Skinned models: skinned on the card

A skinned model with an `Animator` used to be posed on the CPU for every picture (the pose maths, then every vertex through its joints: about 0.3 ms of one core for an armed mannequin of 15,000 vertices), and the GPU renderer laid its vertices out for the card (about 0.2 ms) and uploaded them: 1.1 MB a model a picture. Now the card does the vertices. `gather_scene_with(.., skin_on_card = true)` (what the GPU renderer calls; `gather_scene` and the reference renderer are as before) hands each animated model over as `MeshRef::Skinned { key, base, matrices }`: the model in its rest pose and the joint matrices of the pose (`Rig::skin_matrices` and its kin, about 6 KB for a mannequin of 90 joints), worked out on all cores (`pose_models`, `par_map`; nothing is spawned for fewer than three models). `crates/lunaris-gpu/src/skin.rs` keeps what a model needs on the card once for every model whoever uses it (its rest vertices, its joints and weights), stages the pose's matrices into one buffer, and a compute pass (`shaders/skin.wgsl`, one thread a vertex, the same arithmetic as `Rig::skin`) writes the skinned vertices into the entity's own vertex buffer before the first pass of the picture. The main, shadow and depth passes then draw that buffer as they draw any other mesh, so nothing else in the renderer knows the difference. `LUNARIS_GPU_SKIN=0` or `GpuRenderer::set_gpu_skinning(false)` goes back to skinning on the processor.

The box round a posed model (the culling of the view and of the shadows, the level of detail) cannot come from vertices that are not on the processor any more: `SkinBounds` works it out from the joints. A vertex lies within `radius[j]` (the farthest vertex the joint has weight on, in the rest pose and in the joint's own space) of the joint it is moved by, so the box round balls of that size round the posed joints holds every skinned vertex, and the box of the vertices that no joint moves is added. The joint's space can be scaled (the game's mannequin was made in centimetres: its joints say 50 for half a metre), so the radius is multiplied by the scale of the joint in the pose (`matrix x inverse bind pose`). It is at most 1.6 times the longest side of the real vertices over every clip of the mannequins (`skin::tests::the_box_round_a_pose_holds_every_skinned_vertex`: every vertex of 981 poses is inside it).

The picture is the same whatever the number of threads and whoever skins (`posing_the_models_on_several_threads_draws_the_same_picture`, `a_renderer_that_skins_on_the_card_is_handed_the_pose_and_not_the_vertices`, the GPU renderer against the reference in `a_crowd_of_posed_models_is_drawn_the_same_in_both_renderers`, and `skinning_on_the_card_draws_the_same_as_skinning_on_the_processor`: classic and high, mean difference 0.0000 of 255). Measured (`PUPPET_ROOT=<game folder> cargo run --release -p lunaris-gpu --example puppets -- 60 960 540 classic`, 60 armed mannequins walking at different moments, an Intel UHD 630 on the 8-core Linux test machine with other work running): a picture with the read-back **55 ms with the processor skinning, 26 ms with the card**; of that the gather stage (pose and skinning) went from 7 to 12 ms to 0.5 to 1 ms and the preparing of the draws (laying out and uploading 66 MB) from 18 to 28 ms to 0.7 ms; the card's own time did not change (10 ms, the compute pass is not noticed). At 1280 x 720 in high quality the iGPU is the limit and the gain is the 87 ms of 508. The two pictures are bit for bit the same. Before that (posing on every core, the vertex layout on every core): 60 armed mannequins, CPU reference picture at 64 x 36, 105.7 ms on one pose thread, 87.2 on eight (`pose_threads_cost`).

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

At 100,000 meshes the picture costs the same at 320 x 180 as at 1080p: the time is the CPU's per-mesh work, not the GPU and not the draw calls. The renderer's own share of it (the `draw` call; what each phase takes is in the profile's `cpu_ms.*`), measured on a Linux test machine (i7-9700K, 8 cores) for one draw of the 100,000-sphere scene:

| phase | before | after |
|---|---|---|
| objects: look up the material, the level of detail, the sphere (`items`) | 18 ms | 2.8 ms |
| culling against the camera's and the cascades' planes (`cull`) | 1.8 | 0.6 |
| group the solid ones: one integer sort (`sort`) and the batches with the object numbers (`visibility+batches`) | 15 | 4.2 |
| uniforms and upload | 3 | 3.1 |
| **the whole `draw` call** | **36 ms** | **11.7 ms** |

What made it: the numbers a mesh and its material give every object that shares them (the mesh on the card, its texture bind group, the levels of detail) are worked out once for each (mesh, texture) pair in a picture, not once for each object; an object's own numbers (the 208 bytes the shaders read) are made only for the ones that are drawn, after the culling, not for all; the sort of the solid objects is a sort of one integer each (the class of the (mesh, texture, terrain), the flags, the level and the mask above the object's own place: the same order every time) instead of a tuple of eleven fields compared twice a step; and the per-object phases (`items`, `cull`, the objects' numbers) run on up to eight threads (`par_chunks_mut`; each object is a function of its own index, so the result does not depend on how many). Gathering the scene from the world (`gather`, 13.6 ms for 100,000) is the next biggest part; the step after that is to keep the object buffer between frames and update only what moved.


### Where the time goes: the pass profiler

`LUNARIS_GPU_PROFILE=1` (or a card without timestamp queries: nothing) makes the renderer write a timestamp at the start and the end of every pass (and between the draws of the main pass: sky, meshes, terrain, grass, blend), and reports the milliseconds of the last picture in the `measured` numbers of the `render` tool as `gpu_ms.<pass>` (`gpu_ms.total`, `gpu_ms.shadow`, `gpu_ms.main.terrain`, ...) and in `GpuRenderer::last_gpu_ms`. `tools/bench_scenes.py` prints the pass times of the example scenes; `tools/build_gfx_coast.py` with `REPEAT=6` keeps the best of six pictures (the card's clock changes from one picture to the next) and `tools/gpu_profile_line.py` makes a table of it. What this found and what was done about it, on the aerial coast picture (1920 x 1080, ultra, RTX 2060 SUPER, ms of GPU time):

| | before | after |
|---|---|---|
| the whole picture | 25.0 | 15.3 |
| painted terrain (`main.terrain`) | 7.5 | 3.4 |
| grass (`main.grass`) | 2.8 | 0.2 (what the camera sees is drawn; with grass in view it is ~2.4 for two fields) |
| ambient occlusion + its blur | 1.8 | 0.65 |
| bounced light | 1.6 | 0.43 |
| sky | 1.9 | 1.1 |

- **Terrain**: a painted ground looked up four hex-tiled layers (six anisotropic taps each) at every pixel, though a pixel has one or two layers painted. Layers under 1.2% of the weight are not looked at (the rest are renormalised), the height tap is skipped for a layer with no relief, and the layers' averages (for the macro detail ratio) come from a uniform made when the picture was uploaded instead of four more taps. The pixels differ by 0.04 of 255 on average from before.
- **Ambient occlusion and bounced light** are made at half the size of the picture: each half-size pixel reads the depth of one of the four pixels it covers, a different one in each picture, and the blur (which already was a depth-aware filter) is also the upsampler, weighing the half-size pixels about each full-size one by how near they are and by how near their distance from the camera is to its own.
- **Sky**: drawn after the solid things, where nothing is (the depth test `Equal` the cleared depth), instead of first, over the whole picture, only to be covered; and the atmosphere is no longer integrated at every pixel (16 view steps of 6 light steps each, for the sky and again for every glossy surface's reflection): a compute pass (`skylut.wgsl`) integrates the in-scattered light once a picture for a 64 x 64 grid of view directions (the angle to the sun's heading, and the elevation, squared so that the horizon has most of the rows: the kind of table Hillaire's 2020 sky-view LUT is, here for single scattering) into a storage buffer, and `atmosphere_sky` looks it up with a bilinear read and applies the Rayleigh and Mie phase functions per pixel (the Mie peak near the sun stays sharp). The picture differs by 1 of 255 in the sky. The clouds are now most of what is left (0.55 ms of 0.64).
- **Grass** was bound by its vertex shader (skipping every fragment changed nothing): the tufts of a ring are numbered in tiles of 16 x 16 cells, and the CPU draws only the tiles that the camera's view volume (and the ring, not its hole) can see, as runs of neighbouring tiles.

### Block-compressed textures (every picture of the high qualities)

A picture sent to the card is block-compressed when the card can read BC formats (every desktop card; asked for as `Features::TEXTURE_COMPRESSION_BC`), the picture is sampled smoothly (the anisotropic sampler: models, terrain layers, the terrain's colour picture; nearest-sampled pixel art and sprites are never touched), at least 64 texels on each side and a multiple of 4 on both. `crates/lunaris-gpu/src/compress.rs` makes the blocks on the CPU when the picture is first used, for every mip, split over the cores:

| picture | format | bytes a texel | how |
|---|---|---|---|
| colour, opaque | BC1 (sRGB) | 0.5 | endpoints from the block's principal axis, two rounds of least squares |
| colour with alpha (cut-outs) | BC3 (sRGB) | 1 | the same colour, and BC4-style alpha endpoints and 3-bit indices |
| normal map | BC5 | 1 | x and y, each with its own endpoints; the shader makes z (`normal_xyz`: a unit vector pointing out of the surface) |
| occlusion-roughness-metallic | BC7 (mode 6) | 1 | four numbers, two 7-bit endpoints with a shared low bit, 4-bit indices |
| displacement | not compressed | 4 | |

The encoders are small and fast rather than exhaustive, integer arithmetic and fixed-order float sums, so the same picture gives the same bytes on any machine and any number of threads (the tests decode the blocks and measure: BC1 and BC3 above 30 dB, a normal map's x and y within 4 levels of 255). The memory of the textures is in the `measured` numbers of the `render` tool as `texture_mb`. Poly Haven's two saplings (1k colour, normal and metallic-roughness maps for bark and twigs, two copies of the model): **32 MB to 6.7 MB** and 64 MB to 13.3 MB; the pictures differ by 0.02 levels of 255 on average, and 385 and 1,079 of 4.3 million values by more than 3 levels. `LUNARIS_NO_COMPRESS=1` turns it off.

The blocks are made once: they are saved under `<workspace>/.lunaris/cache/textures` (or the folder `LUNARIS_TEXTURE_CACHE` names; `off` for none), in a file named by the format, the size and a 64-bit fingerprint of the picture's bytes and the encoder's version, written whole under another name and then renamed. A picture seen again (the next run, another scene that uses it) is read from there: no mip chain, no encoding. A folder over 4 GB is trimmed, oldest files first, the first time it is used. The compression itself costs about as much as loading the picture (0.8 s for the six 1k maps of one sapling on a Linux test machine's 8 cores), and only once.

### A forest from the ground (`tools/bench_forest.py`)

3,000 Poly Haven saplings (431,000 and 793,000 triangles each, several materials, normal maps, cut-out leaves), 400 m on a side, the camera on the ground, 1920 x 1080 high, RTX 2060 SUPER: **174 ms (6 fps) to 27 ms (37 fps)**, the triangles in the main view 97 M to 20 M, the shadow pass 72 ms to 3.9 ms. What changed: the levels of detail go from 4 to 6 (grids of 384, 192, 96, 48, 24 and 12 cells along the mesh's biggest side, so a 430,000-triangle tree has 100,000, 33,000, 9,200, 2,500, 520 and 130 triangle versions between whole and a speck, where it had a drop from 430,000 to 8,600), a vertex may be 1.5 pixels from where it was (it was 1), the levels are made on a thread each (`lod_level`), and **a shadow cascade draws each batch at the coarsest level its shadow texels allow** (the vertex error at most 0.7 of a texel of that cascade, never finer than the picture's own level): the far cascades, whose texels are decimetres, draw trees in a few thousand triangles. Without the levels the same picture is 1.9 seconds. `LUNARIS_LOD_REPORT=1` prints each mesh's levels; the `render` tool's `measured` has `triangles` and `lod0`..`lod7` (how many objects are drawn at each level).

### Levels of detail and clusters (every mesh)

A mesh of at least 3,000 triangles gets coarser versions of itself when it is uploaded (`make_lods`: vertex clustering on a grid of 96, 48, 24 and 12 cells along the biggest side of its box; the vertices of a cell are replaced by the one nearest to their mean; the triangles that fall in on themselves or turn over are dropped; a level that has not shed a fifth of the one before is not kept). The coarser levels are only lists of indices into the mesh's own vertices, kept after the whole mesh in the same index buffer, so they cost no vertex memory. Each frame (high qualities, perspective views) the renderer takes for every object the coarsest level whose vertices (moved by at most half a cell) stay within a pixel of where they were, from the distance of the nearest point of its bounding sphere, the object's scale and the picture's focal length (`choose_lod`); water and swaying things keep all of their triangles, `LUNARIS_NO_LOD=1` (or `GpuRenderer::lod = false`) turns it off, `last_triangles` says how many triangles the picture's main view drew. The level is part of the batch key, so objects of one mesh at one level are one instanced draw, and the shadow maps, the depth prepass, the reflection and id passes draw the same level.

A mesh of at least 24,576 triangles (a terrain) is also cut into clusters of 4,096 triangles (the triangles ordered along a Morton curve over the mesh's box, so that each run lies in one place) with a bounding sphere each. An object of such a mesh that is drawn once to four times is culled cluster by cluster (the camera's six planes, the sides of each shadow cascade), as runs of neighbouring clusters; with more copies the instanced draw is kept (the cost is then the copies'). Only the finest level is clustered (a coarse level is a thin list, drawn whole).

On the aerial coast picture (120 Poly Haven rocks of tens of thousands of triangles each, a 2-million-triangle terrain; 1920 x 1080 ultra; RTX 2060 SUPER): the whole picture 13.9 ms without levels of detail, 10.0 ms with (shadow maps 2.06 to 0.70, reflection surfaces 1.36 to 0.64); the pictures differ by 0.03 of 255 on average. Tests: the coarser levels have fewer triangles from the same vertices and none turned over; the level follows the distance and the scale; the clusters hold every triangle once and each has a sphere that holds it, in the same order on every run; a hill of 45,000 triangles drawn near is identical with and without levels, and drawn 420 m away has under half the triangles and a picture within one level of 255.

## Limits

* GPU pictures are not bit-identical between machines: use `quality: reference` where a hash must match (the default for tests and the Python/JS/C clients that embed the engine without the `lunaris` binary).
* Shadows: the sun and the two most important point or spot lights (up to four with `Lighting.point_shadows`); the other lights (up to 16) light without shadows. See-through meshes cast no shadow and are sorted by their centre (two overlapping panes can swap order). Skinned models are posed on the CPU every frame (fine for dozens of characters, not thousands). No depth of field or motion blur yet.
* Up to 16 lights, like the reference.
* The native window runs on Windows (winit); `lunaris edit` works everywhere, and the renderer itself (`GpuRenderer::headless`) works on any system wgpu supports.
