# Hollowlight: a photoreal backrooms game about scale

First person, no weapons, no map. You slip out of the world into places that are too big, too empty and too quiet: office carpet that goes on to the horizon, a hall whose ceiling you cannot see, a pool of still water a kilometre across, a thing in the fog that is the size of a building and does not know you are there. The fear is **megalophobia**: scale, distance, being small, being looked past. Everything is built to look like a photograph of a real place, not a game.

Written 2026-10-15 from the user's idea ("a hyper realistic backrooms game, megalophobia based, use assets online"). **It is built on Lunaris**, the user's order. "Backrooms" is a shared internet idea (no one owns it); the names, layouts, creatures and text here are our own.

## What makes it scary (the design rules)

1. **Scale is the monster.** Every level has one moment where the space opens to something far bigger than the player expected, and one thing in it that shows the size (a doorway the height of a house, a column you cannot see the top of, a figure on the far side of the hall).
2. **Repetition, then a break.** Rooms repeat with small differences, then a long view opens. The player learns to read the pattern and then loses it.
3. **Light is the only comfort.** Fluorescent panels buzz, flicker and die; the flashlight is a short cone in a huge dark; a lit doorway a long way off is the only goal.
4. **Sound does half the work.** A fluorescent hum with a pitch that changes by room, wet-carpet footsteps, far-off water, a low pulse when something very large is moving.
5. **No fighting, little chasing.** The player hides, waits, walks away. The big things are not hunting: they are *there*. A smaller thing (the Watcher, tall and thin, only moves while not looked at) supplies the chase.
6. **The picture must be believable**: real PBR surfaces (normal, occlusion, roughness maps), soft shadows, ambient occlusion, fog and light scattering, film grain, a camera that breathes.

## The setting: extremely large, empty, public places

The user's second order (2026-10-15, mid-build): **"have it in an extremely large, like a water park and such."** So the heart of the game is not corridors but **colossal abandoned public spaces**: places made for thousands of people, lit and empty. The Lobby is only the way in (small and tight, so the first big opening hurts). The water park comes first among the big ones, then the others.

| # | Name | Look | Scale moment |
|---|---|---|---|
| 0 | **The Lobby** | damp yellow wallpaper, brown carpet, drop ceiling with fluorescent panels, 2.6 m high, rooms and halls that never end | a wall ends: the same carpet runs on, under a ceiling 40 m up, light panels shrinking to dots |
| 1 | **The Water Park** (centrepiece) | an indoor water park the size of a town under a glass-and-steel roof 80 m up: wave pool, lazy river that loops for kilometres, slide towers 60 m tall with tube slides that vanish into the haze, a lifeguard chair every 30 m, white tile, turquoise water that is perfectly still, warm humid light, the sound of water that is not moving | the entry hall opens onto the main basin: 600 m of flat water, a wave machine at the far end, towers you can only see the base of; a slide you can climb forever |
| 2 | **The Mall** | a shopping centre with four floors of shuttered shops around an atrium 50 m wide and 200 m long, escalators running, a fountain, a food court of 2,000 seats | the atrium floor drops away for six levels and the ceiling does not end |
| 3 | **The Terminal** | an airport hall with check-in desks to the horizon, a departures board that is blank, jet bridges to nothing, rain on a roof you cannot see | a runway outside the glass that is a kilometre wide and has no end |
| 4 | **The Bowl** | an empty stadium: 80,000 seats, floodlights, a pitch of dead grass, the tunnel you came in by is the only door | you stand on the centre spot and every seat is looking at you |
| 5 | **The Pillar Hall** | bare concrete, a grid of columns 2 m wide every 18 m, ceiling lost in haze, puddles, one lit exit sign | the floor is 2 km across; one column in the middle is 200 m wide |
| 6 | **The Atrium** | marble and glass offices stacked up out of sight, the Giant walking along the far side, one step every ten seconds | the Giant stops and turns its head |

Each level is **one huge connected space** (hundreds of metres to a few kilometres across) made by a generator from a small set of modular pieces (tile bays, pool basins, slide towers, shop fronts, columns, seating rows), so a 2 km hall is a few hundred meshes, not a million. The water is a still `WaterBody`/`Water` sheet with reflections (`Ssr`); the long sight lines are the point, so fog is thin and light carries far.

Between levels: a door that does not belong, a stairwell that is too long, a room where the "wrong" wall is lit.

### The Water Park in detail (build this first after the Lobby)

* **Layout** (seeded, generated): an entry hall with ticket gates and lockers; a changing-room maze (tight, a sample of the Lobby); a **concourse** ring 40 m wide around everything; the **main basin** (wave pool, shallow end sloping to 4 m deep, tile lane lines); a **lazy river** that loops the whole park with floating rings still on it; two **slide towers** 40 and 60 m with spiral tube slides (you can climb the stairs and look down the tube; no riding); a **splash zone** with giant bucket and fountains; an **indoor beach** with sand and umbrellas; a **deep end** with a diving platform 12 m up over water that is too dark to see the bottom.
* **Look**: white and pastel tile, blue and yellow plastic slides, wet concrete, ceiling girders with skylights (daylight shafts through haze, `PostFx` shafts), warm humid air, caustic ripples on the pool floor (an animated texture or a light cookie), reflections in the water and on the wet floor.
* **Sound**: a distant pump drone, drips that echo for seconds, water lapping on a tile edge, far-off a single slide still running with water.
* **Dread**: the long open sight lines and the deep water. Something large moves under the surface of the main basin far away (a wake that crosses 300 m and stops).

## What the player does

Walk, sprint (stamina), crouch, flashlight (battery), look, open doors, read notes, pick up almond water (sanity), find the way to the next level (a **seam**: a door, a wall, a stair). No inventory beyond the flashlight, batteries and water. A *dread* meter rises with open space and long sight lines and falls in small lit rooms; at the top the picture degrades (grain, tilt, tunnel vision, tinnitus).

## Engine work this needs (Lunaris)

| Need | State |
|---|---|
| Normal, occlusion, roughness, metallic maps on a mesh | **new this session**: `PbrMaps` component (GPU high quality) |
| Many lights (fluorescent rooms) | the GPU renderer takes the 16 nearest; enough for a hall, to be measured |
| First-person character, mouse look, flashlight | have (`CharacterBody`, `MouseLook`, spot `Light`) |
| Huge worlds without huge entity counts | chunk meshes made by a Python generator (one glb per chunk and material, `MeshCollider`), streamed with `StreamZone` |
| Fog, scattering, AO, bloom, exposure, SSR | have (`HeightFog`, `PostFx` shafts, `Ssao`, `Ssr`, `AutoExposure`) |
| Film grain, chromatic fringe, breathing camera | check `PostFx`; add what is missing |
| Audio: hum, footsteps, water, far thump | have (`AudioSource`, `docs/AUDIO.md`), sounds to be made by arithmetic or CC0 |
| Big slow creature | the CC0 mannequin and animation libraries from Wyrdwood, scaled 40x, walking in fog |

## Assets

Surfaces: Poly Haven textures (CC0), fetched by `tools/fetch_polyhaven.py` into `examples/hollowlight/textures/` (not committed): wallpaper, carpet, ceiling, plaster, garage and hangar concrete, painted concrete, rebar concrete, tile, linoleum, marble. Models: simple modular geometry made by the generator (walls, panels, columns); props (chairs, doors, signs) from CC0 sources when needed (Poly Haven models, Quaternius, Kenney). Credits in `CREDITS.md`.

## Milestones

* **H0 Surface check** (done 2026-10-15): a wall and a floor with the real maps look right; normal map direction checked with a dome (convex under a light from the upper left, as it should be).
* **H1 Lobby slice**: first-person walk through 40 generated rooms, fluorescent flicker, flashlight, hum. Looks like a photo.
* **H2 The Opening**: the wall of the Lobby ends and the **Water Park entry hall** opens: the first scale moment, fog, far lights, tile and still water with reflections.
* **H3 The Water Park**: the whole park (concourse, main basin, lazy river, towers, splash zone, beach, deep end), sound, the wake under the water.
* **H4 More giants**: the Mall, the Terminal, the Bowl; seams between levels; the Watcher; the Giant; dread meter; saves.
