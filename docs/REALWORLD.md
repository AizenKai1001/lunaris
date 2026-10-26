# The real world as a map (feasibility, 2026-10-16)

The idea: a DayZ/Rust-style game on the actual Earth. Rules from the user: only real, official data of the real place, nothing hand-modelled, nothing from Tellus, as detailed as the data allows. The full research (sources, licences, scale arithmetic, what the engine lacks) is in the vault note `Projects/Real-World Map Game/Real-world map game research 2026-10-16.md`. This page is what was built to prove it.

## The proof: Stowe, Vermont (a 3 km window, the village core measured by lidar)

`tools/realmap/` (Python, no Tellus, data kept outside the repo in `realmap-data\NAME\`):

| step | tool | data (all open) |
|---|---|---|
| fetch | `fetch.py NAME LAT LON [SIZE_M]` | USGS 3DEP bare-earth elevation, 1 m, public domain (nine 1 km tiles of 4 MB); USGS NAIP aerial photo, 0.5 m a pixel, public domain; OpenStreetMap through its official API (ODbL, credit required) |
| lidar | `lidar.py NAME PROJECT [HALF_M]` | USGS 3DEP point clouds from the Entwine copy on AWS (`s3://usgs-lidar-public`, no account): 24 million points for 1 km2 (23.9 per m2, 187 MB of LAZ) |
| ground, photo, water, roads | `build.py NAME` | heightmap (16 bit, a sample a metre, hydro-flattened water carved), the aerial photo as the ground's texture, water from OSM areas and streams, roads as ribbons of their tagged width draped on the ground |
| roofs, trees | `build_lidar.py NAME` | per OSM footprint: RANSAC roof planes fitted to the lidar returns (tree crowns over a roof belong to no plane and drop out), walls from the ground to the roof's edge, roof colour per vertex from the photo; tree tops from the canopy height model (5,980 trees in 1 km2, median 21.5 m) |
| scene, pictures | `scene.py`, `render.py` | a Lunaris scene (`Terrain`, `WaterBody`, meshes) and the GPU renderer on a Linux test machine |

What it showed:

* The engine takes a 3,001 x 3,001 real heightmap, a 4096 x 4096 aerial texture, a water map, 24 MB of lidar roofs and 1,155 measured trees and draws them at ultra quality (about 7-11 M triangles).
* **OpenStreetMap alone is not enough for buildings**: in this window 604 of 605 buildings have no height tag, none has a roof shape, none a wall colour. The lidar survey fixes the first two (height and real roof planes); wall colour and texture need street-level imagery or official textured 3D models.
* **This survey is classified only as ground (2) and everything else (1)**: no building or vegetation classes, so roofs and canopy are told apart with the OSM footprints (the lowest return 2.2 m above the ground is the roof, the highest above the ground outside a footprint is the canopy).
* The USGS and NAIP services work from a script, but only in pieces: 1 km tiles for elevation (a 3 km request fails with a server error) and 4000 px at most for imagery. The public Overpass servers refused us (expired certificates, HTTP 500), the official OSM API works for small boxes.

Open problems in the pictures: some roofs are still ragged where trees hide the roof from the laser; walls are a plain colour; the aerial photo carries its own sun and shadows (it has to be de-lit before it can be an albedo); the only trees are two borrowed photoscan saplings scaled to the measured height; no grass, no interiors.

## What the engine needs for a whole county or country

See the vault note, section 4: world streaming by cells, tiled terrain with level of detail and tiled imagery, an importer inside the engine (this Python is the prototype of it), GPU scatter for trees, interiors, a bake pipeline and cell format. f32 positions are fine up to about +-100 km from a centre.

## Free data other people already give away (checked live 2026-10-16)

The vault note section 8 has the full table. The short version: **buildings of the whole planet with heights** are one command away (`pip install overturemaps`; `overturemaps download --bbox=W,S,E,N -f geojson --type=building -o out.geojson`: 497 buildings of the Stowe window in 2 seconds, 450 with a height; the whole building theme is 277 GB of GeoParquet on `s3://overturemaps-us-west-2`, free, ODbL); **elevation of the whole planet** is tile-addressable at `https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png` (Mapzen terrain tiles, 3DEP-fine in the US) and Copernicus 30 m; the USA is covered by USGS 3DEP and NAIP, Switzerland by swisstopo's STAC API (0.5 m terrain, 10 cm photos, 3D buildings), New Zealand by `s3://nz-elevation` (1 m, CC BY 4.0), the Netherlands by PDOK (8 cm photos), France by IGN LiDAR HD (still being published). Nobody gives away a finished game world: Build The Earth's Terra++ (open source) shows the same pipeline for Minecraft, built from these datasets.

## Quality of the free datasets, measured from real samples (2026-10-16)

`tools/realmap/quality_nl.py` and `quality_sheet.py` (and the Swiss, New Zealand and Melbourne downloads in `realmap-data\quality`) compare them at native pixels: imagery of the same 64 m of ground from USA NAIP (0.5 m: blocky), Zurich swissIMAGE (10 cm), Utrecht PDOK (8 cm) and Wellington LINZ (7.5 cm): the last three show roof tiles, chimneys and people. Surface models: Dutch AHN and swissSURFACE3D at 0.5 m have crisp buildings and trees, US 3DEP at 1 m is soft. 3D BAG gives 866 buildings with real roof planes (LoD 2.2) and attributes for 500 m of Utrecht; the Melbourne photomesh tile is 1.19 M vertices and 2.29 M faces with 3,374 photo patches for 281 x 281 m (275 MB zipped). Best open quality: Switzerland, the Netherlands, New Zealand, Helsinki and Melbourne; US data needs our own lidar processing to approach it.

## Grindelwald (Switzerland): the best-quality open data in the engine (2026-10-16 evening)

`tools/realmap/swiss_fetch.py` + `swiss_build.py` (+ `scene.py`, `render.py`): a 2 x 2 km window of the Bernese Oberland from swisstopo's open data, 338 MB downloaded (swissALTI3D 0.5 m, swisssurface3d 0.5 m, swissIMAGE 10 cm, swissBUILDINGS3D 3.0, OpenStreetMap): the ground as four 1 km tiles of 2001 x 2001 samples (0.5 m, drawn at 1 m), each with its own 4096 x 4096 aerial photograph (about 24 cm a pixel), **1,522 buildings from the official LoD2 model with their real roof and wall polygons** (24.6 K roof and 48.9 K wall triangles; the roofs carry the aerial photo projected from above, so the real roof tiles, solar panels and skylights show), 14,063 tree tops measured from the surface model, the Lutschine gorge and stream, 121 km of OSM paths (not drawn: the photo shows the roads better than flat ribbons do). The scene draws at ultra quality in about 0.1-0.2 s a picture on a Linux test machine's RTX 2060 SUPER (8.7 M triangles).

Findings: (1) **the engine refuses textures larger than 4096 x 4096** ("unsupported size 8192x8192 (max 4096)", magenta placeholder), and a 10 cm photo of 2 km is 20,000 px: the ground has to be tiles with one texture each (what a streaming world needs anyway) or the texture loader has to take tiled/virtual textures; (2) the OSM road ribbons are not needed where the photo is sharp; (3) at street level the 24 cm ground texture is blurry and the survey has no facade data: fixed in the same evening with ground-detail layers (real CC0 PBR ground photographs chosen per place by a splat map made from the photograph's green and brightness and the slope), the engine's blade grass grown only on the grass layer, and walls of weathered timber or render (Poly Haven CC0 photographs, UVs in wall metres, a tint per building; 62 % timber), no window or door can be had from the survey; the aerial and 100 m views are convincing; (4) CityGML triangulation is a few seconds per 1,500 buildings with an ear-clipper in Python; the orientation of the roof/wall polygons needed a per-triangle check against the building's centre.

**The bug that hung grass in the sky (fixed).** With several terrain tiles every tuft of a `GrassField` ring beyond its own heightmap read the (clamped) edge height and was drawn there, so a sheet of grass hung over the neighbouring tile. Both grass vertex paths (cards and blades) now skip places off their own terrain (`grass_on_terrain` in `scene.wgsl`); a GPU test with two terrains of different height checks both paths (4,070 green pixels in the sky without the check, none with it).

## Photogrammetry: Melbourne CBD (City of Melbourne open data, CC BY, 2026-10-16 night)

`tools/realmap/obj_to_gltf.py` + `photomesh_scene.py`: one 281 x 281 m tile (Flinders Street Station, St Paul's Cathedral, Federation Square) of the city's 2020 photomesh: 1.19 M vertices, 2.29 M faces, **3,374 JPEG patches, 2.08 billion texels (1.6 cm each)**, 275 MB zipped. It is the real thing: window tracery, posters, tram tracks, the dome. Results on a Linux test machine's RTX 2060 SUPER:

| texture set | decoded | on the card (BC1 + mips) | peak RAM | first picture | next pictures |
|---|---|---|---|---|---|
| half size (TEXTURE_SCALE=0.5, 520 M texels, 3.2 cm) | 2 GB as bytes | 330 MB | 10.6 GB | 27.6 s (BC encoding), then 11 s with the disk cache | 0.1 s |
| full size (2.08 G texels) | 8.3 GB as bytes | 1,320 MB | 12.7 GB | 68 s | 0.1 s |

Four things this showed:
1. **The loader refuses models over 128 MB and 4 million vertices** (`obj_to_gltf.py` now writes the tile as chunks of at most 1.5 M face corners, `mesh_0.gltf` ...; a scene lists one MeshRenderer each).
2. **The converter's winding flip was wrong**: (x, y, z) to (x, z, -y) is a rotation; with the flip every roof and the whole ground were culled and only the far walls of each building showed. No flip, and the tile is complete.
3. **Textures were held as 16-byte floats a texel**: 3,374 patches needed 33 GB and the process died at the 27 GB cap. `Texture.texels` is now `Texels::{F32, Srgb8}`: a decoded PNG or JPEG is 4 bytes a texel (the very floats as before through a 256-entry table, the card's bytes are the file's own pixels, no round trip), made pictures stay floats. Every picture test (reference against GPU) is unchanged.
4. **Aerial photogrammetry is for the mid and far view, not for the street**: at eye height the facades hold up, but the ground floor is melted (trees, trams, vehicles, awnings are blobs pulled over the pavement, the ground has holes under overhangs) because the capture is from the air. A playable street needs the mesh for the buildings above the first floor, real library assets (trees, cars, street furniture) and a clean ground near the camera.

What a city of these needs: 2 cm texels are far more than a 1080p screen can show beyond a few metres; the world build must write a texture level of detail (`TEXTURE_SCALE` is the first step), the engine must stream patches by distance (a tile is 1.3 GB on the card at full size) and drop the CPU copy after upload; textures larger than 4096 and a megatexture for the ground are the same need.

## A streamed world: Grindelwald as cells (2026-10-16 night)

The engine already had the mechanism (`StreamZone`: a prefab is created while the camera is within `radius` of the zone and removed beyond `radius + hysteresis`); `tools/realmap/world.py` makes the scene for it from the files `swiss_build.py` wrote: **no ground in the scene at all**, only 4 zones for the 1 km ground tiles (prefab `Cell_i_j`: `Terrain`, `TerrainLayers`, `GrassField`, and a child with the tile's own buildings, `buildings_i_j.gltf`, roofs carrying that tile's photograph) and 16 zones for the measured trees (prefab `Trees_a_b` of up to 4,000 tree entities for each 500 m). `tools/realmap/stream_test.py` flies a camera over it, setting the camera entity's position and stepping:

| stop (camera x, z) | ground cells | entities | step | RSS of the server |
|---|---|---|---|---|
| (-900, -900) | 1 | 3,163 | 304 ms (spawn) | 1.6 GB after the first picture |
| (-500, -500) | 3 | 6,288 | 241 ms | 1.7 GB |
| (-100, -100) | 4 | 4,192 | 160 ms | 3.6 GB |
| (330, 300) | 4 | 4,170 | 16 ms | 3.6 GB |
| (700, 700) | 3 | 3,040 | 17 ms | 3.6 GB |
| (900, 900), then 20 more pictures | 1 | 2,635 | 16 ms | 3.3 GB, then 2.8 GB |

What it needed in the engine (all done): (1) a `GrassField` on a prefab cannot name its terrain (a prefab's entities cannot refer to each other), so `terrain: 0` now means the `Terrain` on the same entity; (2) **the cache of pictures, models, heightmaps and splat maps never let go of anything**, and the card's copies kept a full CPU copy of every texture and mesh they held: now the card keeps weak references (the address stays unique, the pixels go), and `TextureCache::trim` drops what no picture asked for in the last 600 pictures (`LUNARIS_TRIM_IDLE`), counted from `gather_scene`; a cell that leaves is freed on the CPU and, 256 pictures later, on the card; (3) tile files and the tree prefabs are written per cell.

Not solved yet: the first picture of a cell decodes and encodes its 8192 x 8192 photograph on the main thread (about half a second with the disk cache of compressed textures, many seconds without), so a cell arriving while playing hitches; far cells vanish entirely (no distant level of detail: a mountainside beyond `radius` is bare); trees are one entity each (4,000 a cell is fine for the simulation, a forest of millions is the GPU scatter's job); the heightmaps the physics reads are kept by the simulation.

## Library houses on the real footprints (2026-10-17, the user's idea)

The survey's buildings have the right place, size and roof shape but no look beyond a plain wall. Existing textured house models (Sketchfab, CC Attribution, downloaded through the user's logged-in browser; credits in `realmap-data\models\chalets\CREDITS.md`) are fitted to them by `tools/realmap/fit_models.py`: a building whose footprint fills at least 82 % of its oriented bounding box (a plain block: 78 % of the 4,000 buildings of the 8 x 8 km Grindelwald region) is drawn as a model turned to the building's own direction and scaled to its own length, width and height (position, size and orientation are the survey's, only the look is the model's); a building of a complex shape keeps the survey geometry with the timber or render walls. The pick is by kind (long and low: barn, small: shed, else house) and a hash of the position, so it is the same every time. `swiss_region.py ... refit` remakes the buildings of built cells without touching the rest; `world.py` puts the fitted models into the cell prefab as ordinary entities (the engine instances the same model).

What it showed: of seven free models only three are Alpine or central-European in style (an Alpine chalet with a timber balcony, a half-timbered medieval house, a red barn; the rest were an American farmhouse, a cartoon cabin, a ruined shed, a row of Rothenburg townhouses), so the village repeats two houses; a model in centimetres needs its height scaled relative to its footprint scale, not by an absolute ratio (a shed came out 164 m tall); CC-BY needs the credit shown in the game. More variety needs more models with an Alpine look (Sketchfab has more; each download is a few clicks in the logged-in browser).

Later the same night the fitting learned two more things (`fit_all` in `fit_models.py`): a very long building is made of several models side by side (none stretched more than about one and a half times), and a building of a complex shape is cut into the largest rectangles that fit in its footprint along its own axes (a largest-rectangle search on a 0.5 m raster of the footprint) and each gets a model, its height from the roof over it; only a building that the rectangles cannot cover (72 % of the footprint) keeps the survey geometry. With nine models almost every building of the 8 x 8 km region is now textured (the lighting was also lowered: sun 2.6, ambient 1.4, exposure 0.26, because pale ground came out white).

## Streaming without stalls (2026-10-17, "why is it running at 60 and below fps")

Measured in `lunaris play --perf --novsync` (the window prints once a second the frame times, the passes of the card from a ring of timestamp readbacks that never waits for the card, the meshes with most triangles in view, and a line for every picture over 25 ms; `LUNARIS_GPU_PROFILE=1`, `LUNARIS_TIMING=sum`). What it found, and what was done:

| finding | change |
|---|---|
| a Linux test machine's screen is 60 Hz and `play` waits for the refresh: the cap was the screen | `--novsync` to measure what the machine can do |
| every cell that came in cost 0.9 s on the picture thread (1,000,000-vertex terrain mesh, its clusters and levels of detail, 8192 x 8192 textures, a splat map, grass heights) | everything that needs no card is prepared on worker threads (`lunaris_render::bg`), the card's buffers and textures are created there too, and the copy to the card is a slice a picture (`GpuRenderer::async_prep`, `upload_budget`, `TextureCache::async_jobs`); objects wait outside the picture until ready, with no warnings |
| trees: two photographed models of 793,000 and 433,000 triangles, 29 million triangles in view in the village, 20 ms of the card | coarser levels down to a grid of 3, levels of detail for meshes from 600 triangles, a level chosen also by triangle density (`LUNARIS_LOD_TRI_PIXELS`, 3 square pixels a triangle) and a base budget of 150,000 triangles (`LUNARIS_LOD_BASE_TRIS`): 3 to 7 million triangles, 6 to 16 ms |
| gather: a name string and two hashes for every object, a warning for every entity whose model was still loading | names resolved once a picture, models still being read keep their objects out silently, the mesh list sorted by index |
| a prefab of 3,000 entities takes 18 ms to create (measured) and several came in at once | `stream.loads_per_tick` (the nearest zone first) |

Result on a Linux test machine's RTX 2060 SUPER, 1080p, high quality, the valley tour: worst stalls from 0.9 s to under 50 ms, the village from 25 to 51+ fps, the open valley 100 to 150 fps. What is left there is the processor: in the village the gather (15 to 20 ms) and the draw (10 ms) are
single-threaded and per object; a retained, GPU-driven scene is the next step before anything much denser.

## Tokyo from Project PLATEAU (2026-10-17, the user: "i would like it to be in like a city, for example like tokyo")

Japan's Ministry of Land (MLIT) publishes the 3D city models of Tokyo's 23 wards as open data (Project PLATEAU: CityGML, 3D Tiles, textured LoD2 buildings, LoD3 roads, street furniture and trees, bridges, relief, orthophotographs), free for any use including commercial,
CC BY 4.0 compatible: credit "出典：国土交通省 PLATEAUウェブサイト" and say that the data was processed. Shibuya (2025 release, 13113) is the first piece: `geospatial.jp/ckan/dataset/plateau-13113-shibuya-ku-2025`; the 23-ward sets
(`plateau-tokyo23ku`, 2020 and 2022) cover the rest with LoD1 and partial LoD2.

Pipeline (`tools/realmap/`, run on a Linux test machine; the data is in `~/tokyo` and what the engine reads in `~/realmap/tokyo`):

1. `plateau_tiles.py TILESET OUT LAT0 LON0 [H0]`: every tile (b3dm: header, feature and batch tables, a glTF with `KHR_draco_mesh_compression`, `EXT_texture_webp` and `CESIUM_RTC`) becomes a glTF with JPEG pictures in a local frame (x east, y up, z south, the Scramble crossing at the origin). Cesium's
   y-up to z-up turn and the centre give Earth-centred metres; the east-north-up matrix of the origin gives the engine's frame. Surfaces without a picture get a colour of their own (roofs and walls differ, each building its own shade). `index.json` lists the tiles with their middle, radius, depth,
   geometric error, whether they are leaves and their parent. The glTF says `asset.extras.lunaris_lod = "whole"`: the engine's levels of detail merge vertices, which smears a picture atlas, and the data brings its own coarser tiles.
2. `plateau_dem.py`: the relief (a TIN of triangles, heights above the geoid) into cells of 1 km, 1001 x 1001 samples, 16 bits, with the geoid's 36.7 m added so that it matches the tiles.
3. `plateau_ortho.py`: the orthophotographs (JGD2011 longitude/latitude, 0.5 m a pixel, two sheets that meet in the middle of the area) cut and resampled for every cell at 2048, 1024 and 512 pixels.
4. `tokyo_world.py`: a streamed scene. Ground cells in three rings (as the valley's), and the tile tree of the buildings and roads as zones: a tile's coarse mesh is a ring zone, the same place's "refined" zone has the ring's inner radius, and the tiles under it `require` it (`StreamZone.requires`, new), so exactly one
   level of each branch is there. Street furniture, bridges, water and trees are leaf tiles within a radius. `assets` lists make the files read ahead, and the renderer prepares the meshes and pictures of every model the cache holds before anything draws them.

Measured: 730 building tiles (1.7 million triangles with the coarse ones, 1.5 million in the 513 leaves), 220,000 triangles of road, 207,000 of street furniture; the tour over Shibuya runs at 115 to 230 frames a second on the RTX 2060 SUPER at 1080p, 5 to 8 ms of the card, worst picture 30 ms. Not done: a walking player and collision with the buildings, the other 22 wards,
trees beyond the four tiles PLATEAU has in Shibuya, signs/vehicles/people, interiors.
