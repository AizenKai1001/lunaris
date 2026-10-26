# The endless city

`lunaris-citygen` makes a city that has no edge and no files: every street, building, room and tree is a function of a seed and of the numbers in its name, and exists only where somebody is near. Every building can be walked
into (doors, stairs, rooms, furniture on every floor); nothing is a texture painted on a box. It is built from real CC0 photographs (Poly Haven PBR sets) and real CC0 models (furniture, street lamps, trees), laid at their
true size, and runs through the engine's ordinary streaming (`StreamZone`), with a procedural provider instead of files.

## Run it

```
python tools/citygen/fetch_textures.py citygen-data/city          # 63 photographed material sets (diff / nor / arm, 1k)
python tools/citygen/fetch_models.py   citygen-data/city          # about 100 furniture, street and tree models
python tools/citygen/fetch_alpha.py    citygen-data/city/props    # the leaves' opacity (Poly Haven's glTFs leave it out)
python tools/citygen/shrink_props.py   citygen-data/city/props    # 512-pixel pictures for the models (memory)
python tools/citygen/make_clumps.py    citygen-data/city/props    # a picture of a clump of leaves for every tree
python tools/citygen/texture_stats.py  citygen-data/city          # the average colour of every set (far-off buildings)
citygen city citygen-data 1 60000 9 1200                         # city.json, city.lunaris.json (a walker), city_view.lunaris.json (a fixed camera); 60 km, start at 9 AM, a day is 1200 s
lunaris play city.lunaris.json --root citygen-data               # walk: WASD, mouse, space
```

`play_city.bat` (high quality, synced), `play_city_fast.bat` (medium, no vsync), `play_city_max.bat` (low, no vsync) and `play_city_perf.bat` (numbers every second) are in the workspace.

**Controls**: WASD walk, mouse look, Space jump, **E** opens and shuts the door you look at (about one street door in five is locked: with a hatchet or a crowbar in the backpack E breaks the lock, without one the prompt says it is locked; and E searches a cabinet, shelf, bedside table, crate or desk), **Tab** the backpack (drag things between it and a container),
**B** building (see below), **F6** steps the weather (clear, cloudy, rain, fog; it also changes by itself every five minutes), a thing dragged from the backpack onto the red box under it is put down on the ground and **E** at a thing on the ground picks it up (a full backpack leaves it), **E** at a tree with a hatchet in the backpack chops three planks' worth of wood from it (four times) and **E** at a fire hydrant drinks from it (35 water, three times), a compass at the top says which way you face (north is -z), **E** at a parked car with a crowbar strips four nails' worth of scrap (three times; without the tool the prompt says what is needed, and a tree or car streamed in again has its material back), **C** puts a storage crate down in front of you for 8 wood (E opens it beside the backpack; what is put in it stays, it is part of the saved world), **F** turns the flashlight on and off (with one in the backpack: a spot light that follows the view, a BATTERY bar that runs down in about four minutes of light, dims at the end and goes out; a click on batteries in the backpack puts 60 back), **F5** saves the game and **F9** loads it (slot 1), Esc closes the panels and, with nothing open, saves the auto slot and leaves; the next start continues from it
(`play_city_new.bat` moves it aside for a new game). The engine saves the whole world, systems included: what is built, what is carried, the bars and where the player stands; the streamed city itself is made again from its seed, so
loot in the furniture is rolled afresh and doors stand shut. The two slots are numbers that `citygen` makes from the scene's own text (quick 10 to 49, auto 50 to 98; `quick_slot` and `auto_slot` are resources), so a save of
an older scene is never loaded over a newer one (it would bring its older systems with it); a changed scene starts a new game.

The `lunaris` program installs the city of any workspace that has `city/city.json` (`{"seed": N}`). `citygen map ROOT` prints the land use round the origin; `citygen cityinfo ROOT` times the generation;
`citygen dump ROOT city/gen_in_1_1_1_0.glb` writes one of the generated files to disk; `citygen trees ROOT` makes a showroom of the outdoor models.

## How it is laid out

- **Plan** (`plan.rs`): blocks of 84 m between streets of 18.2 m (11 m of carriageway, 3.6 m of pavement each side), every sixth street an avenue. Land use (downtown, mid-rise, residential, outskirts, industrial, park)
  comes from smooth noise and jittered downtown centres; lots line the four edges of a block; the kind of building on a lot (tower, masonry block, apartment block, shophouse, house, warehouse) follows the land use.
- **Buildings** (`building.rs`, `facade.rs`, `interior.rs`): `spec_of` decides a building from its key without making geometry. The outside has real walls with window reveals, frames, sills, lintels, shopfronts, signs,
  awnings, roller doors, curtain walls, pitched and flat roofs; the inside has a stair hall (U-shaped stairs) and a lift shaft, corridors, rooms (shop, lobby, office, meeting room, living room, kitchen, bedroom, bathroom,
  storage, warehouse hall), partitions with doors standing ajar, furniture from real models, a lamp in every room. A floor is the same whatever else is made, so floors are made one pair at a time as somebody comes near.
- **Skyscrapers** (`Kind::Skyscraper`): a podium, one to three tiers each stepping in from the one below (32 to 78 floors in all), and a crown: equipment on the roof, a smaller stepped block, or a spire; glass in blue, green,
  teal, bronze, silver, black or grey, now and then a punched skin of stone or concrete. They make most of a downtown, whose heart has blocks with deeper lots (`plan.rs`).
- **Streets and parks** (`street.rs`, `streetscape.rs`): asphalt, kerbs, paving, lane paint, zebra crossings; trees along the kerb, street lamps, benches, hydrants, cabinets. Nine blocks in a hundred are parks of six kinds: green
  (a cross of paths), meadow (diagonal paths, thick with trees), plaza (paving, a fountain, rows of trees), sports (a football pitch with lines and goals, a basketball court), pond, playground (swings and a slide). `citygen parks` lists them.
- **Levels of detail** (`lod.rs`): level 0 everything (within 125 m), level 1 textured walls with windows as bands and trees as crossed cards (95 m to 380 m), level 2 boxes wearing a picture of a bay of windows
  (340 m to 3.8 km; spires are thin shafts). A coarser level is drawn a hand's width inside the finer one, and is loaded only where nobody can stand inside a building, so a room never shows a wall that is not there.
  The things of the streets (trees, lamps, cars, signs) have a zone of their own, 95 m.
- **Day and night**: the scene's `TimeOfDay` turns the sun, the sky, the stars and, after sunset, a moon; the street lamps' lights (`StreetLamp`, the system `lamps_on`) come on at night, the ground beyond the city darkens
  and the clock is in the corner. `citygen city ... HOUR DAY_SECONDS` sets the start and the length of the day (0 freezes it).
- **Doors** (`Door`): every street door is an entity in its doorway (a leaf model scaled to the opening, a box collider), looked at through the eyes' `Probe` ray (3.2 m), opened and shut with E; the leaf swings about its hinge
  into the building (`door_swing`). An open door can be shut from the doorway. Room doors inside are still standing ajar in the model.
- **Containers and the backpack** (`survival.rs`): the furniture that can hold something (cabinets, shelves, bedside tables, crates, desks) is recorded when an inside is made and carries a `Container` entity with a box;
  E opens it beside the backpack and, the first time, rolls what is in it by its kind (`LOOT`); things are dragged with the pointer between the two panels (the engine's `UiDrag` / `UiDrop`) and belong to the panel they lie in.
- **Building** (`survival.rs`, `build_parts`): B starts, 1 to 6 choose a foundation, wall, doorway, floor, roof or door, a see-through ghost shows where the eyes' ray (7 m) points, snapped to 3 m cells (walls and doors on
  the cell's nearest edge, at the height of what is aimed at), a click puts the piece down, X takes a box away. Pieces are real boxes with colliders; a door piece is a `Door` like the city's.
- **Provider** (`runtime.rs`): `City` implements `lunaris_sim::procgen::Provider`. Files are `city/gen_b0_<i>_<j>.glb` (a block), `gen_b1_...` (coarse), `gen_c2_<ci>_<cj>.glb` (a cell of 4 x 4 blocks), `gen_in_<i>_<j>_<lot>_<g>.glb`
  (floors 2g and 2g+1 of one building), each in the frame of its own place so that numbers stay small far from the origin. `gen:` prefabs put a file in the world with its collider (and the lamps of an inside); four
  `ProcGrid` entities (`b0`, `b1`, `c2`, `in`) make the `StreamZone`s of the cells near the camera. Worker threads make the files ahead of need; a bounded cache keeps them (LUNARIS_CITY_CACHE_MB, default 256).

## Speed

The renderer is made to draw this at 120 frames a second and more (measured on an RTX 2060 Super and an i7-9700K at 1920 x 1080: medium 140, high 133, with the slowest second's p95 at 6 ms). What does it, in the order of what it saved:

- an **early depth pass** into the main depth image: the opaque meshes' depth is drawn first and the main pass shades each pixel once (the main pass fell from 5.9 to 2.1 ms); `LUNARIS_EARLY_Z=0` turns it off;
- a model drawn in pieces (a block has one piece for each material) gets **one mesh of its solid pieces** (`Mesh::depth_whole`) for everything that needs only depth: the shadows, the early pass, the ids (draws 3100 to 1300);
- the city's **materials**: the lettering of the shops is one picture for all names (an atlas), the plain materials fall into five classes of roughness and metal, so a block's walls are 26 pieces and not 73;
- point-light shadow cubes cull by face and, inside a big model, by piece; the sun's cascades cull by their own frustums;
- **software occlusion culling** (`occlusion.rs`) from solid prisms that the buildings carry in their glTF (`asset.extras.lunaris_occluders`): drawn into a 256-wide depth picture on the CPU, and what lies wholly behind
  them is left out of the camera's passes. It pays from the air or among towers and not in a street, so it switches itself on only while it hides a quarter of what it tests (`LUNARIS_OCCLUSION=1` keeps it on,
  `LUNARIS_NO_OCCLUSION=1` off). The prisms nearer than 18 m and the rooms inside a building are never used or hidden by it;
- the city's own worker threads are few (a quarter of the cores) and below normal priority, and a graph system that sets resources no longer places every child of the world each time (`command.rs`).

`tools/citygen/hl_perf.sh SCENE QUALITY FRAMES [ENV=VALUE ...]` runs the scene on a Linux test machine and prints the frames per second, the slowest pictures and (with `GPU=1`) what every pass cost; `play --perf` with
`LUNARIS_TIMING=sum` and `LUNARIS_SLOW_MS=N` lists what a slow picture was busy with. The tests of the game on top are `tools/citygen/doors_test.py`, `inventory_test.py` and `build_test.py`.

## Numbers

A block of buildings and trees is made in 30 to 45 ms (10 to 13 MB), the inside of two floors in 15 to 50 ms (4 to 14 MB), a cell at level 2 in 1 ms. The engine takes about 1 ms a tick with 3500 zones and entities.
Memory is dominated by what the renderer keeps of every model and picture: furniture pictures are 512 pixels (`shrink_props.py`) and the normal and roughness maps of the materials too (`shrink_textures.py`); a flight at 80 m/s over 20 km settles near 4.6 GB, and a windowed run at 1920 x 1080 on an RTX 2060 Super draws 1.8 million triangles at 55 fps.

**Weather** (`weather.rs`, scene data like the rest): a graph system on the day's entity draws a state, eases how overcast (`wx`), rainy (`rain`) and foggy (`fogw`) it is towards it in about ten seconds, dims the sun with the cloud
and, in a storm, throws lightning (a flash on the ambient light, thunder one to four seconds later); another dulls and darkens the sky and haze and sets the fog's thickness (0.0006 clear, to 0.006 in thick fog); the rain itself is
`Weather.rain`: up to 36,000 drops in a 40 x 22 x 40 m box round the camera, stretched along their fall and placed by the card's vertex shader alone (`vs_rain`; the drops keep their place in the world, so they have parallax, and
the depth test hides them behind buildings), medium 145 fps and high 133 in the rain on a Linux test machine, as clear (a screen-space streak layer looked flat; 7000 `Particles` drops looked right but cost 6 ms of CPU and half the frame rate), and `Weather.wet` darkens and glosses the flat ground in puddles (the shader's `lit_tail`; walls a little); an entity that follows the player's head puts the rain out, and muffles its sound, while its eye ray straight up (13 m) meets a roof. The loops (rain, wind, the day's birds, the night's crickets, each fading with the weather and the light) and the thunder are made from noise by `tools/citygen/make_weather_sounds.py`
(`sounds/` of the workspace; no recordings; the player's footsteps are two synthesised voices in turn, one every 1.7 m on the ground, `footsteps` in survival.rs). Tests: `tools/citygen/weather_test.py`; `views.py` takes `RES="wx_target=2,wx=0.95,rain=1" HERO=1` to draw a weather.

**Checks that cost the reader nothing** (Linux test machine, detached): `bash tools/citygen/hl_all.sh` builds, makes the scene and runs every `tools/citygen/*_test.py` (95 checks, `hl_all.sh result` prints only the failures); `tools/citygen/soak.sh [MINUTES]` carries the player 9 m a second
along +x in the real play loop (`SRC=city_rain.lunaris.json` for a rain) and samples the memory every minute: 45 minutes (21 km, two day and night cycles) ended at 3.17 GB with no growth to speak of, and 36 minutes in forced rain at 3.2 GB, no errors;
`tools/citygen/hl_perf.sh` gives the frame rates (all of the above on, 1080p, RTX 2060 Super: medium 139 fps with 131 as the lowest, high 129 and 122; at night 145 and 139; in rain 145 and 133).

## Limits

- The map has an `extent` (60 km each way from the origin by default, 120 km across): `f32` positions lose their millimetres further out. A floating origin would remove the limit.
- The room doors inside a building are still baked ajar into the model (only the street doors swing). Cars stand parked under covers; there are no people or moving traffic, and none is planned.
- A piece of building does not check that it is supported. The unlit part of the far city is black at night (no moon).
- Containers roll their things once and forget them with the room when it streams out (what lay in one is swept away once the player is 160 m from it); items are one to a slot (no stacks) and cannot be dropped yet.
- Foliage is lit like any surface: the shaded side of a crown is dark (no translucency).
