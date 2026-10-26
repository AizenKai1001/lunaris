# Particle systems (`ParticleSystem`)

Lunaris has two particle components. They go on working side by side.

| | `Particles` (docs/GAMEPLAY.md) | `ParticleSystem` (this page) |
|---|---|---|
| How a particle is found | a **closed form** of the clock: no state, nothing stepped | **stepped**: fixed steps of 1/60 s, state kept between pictures |
| Cost | the same for any clock; very cheap | steps the particles that exist, only for what is in view |
| Can it collide, spawn other particles, leave a ribbon? | forces and colliders by stepping each particle from its birth (dear) | yes, all of it, cheaply |
| Emitters per entity | one cloud | up to 32, one **effect** (a fire is flames, smoke and embers in one place) |
| Description | numbers on the component (+ `ParticleModules` text) | a **JSON document** (`examples/fx/*.fx.json`) |
| Good for | rain, snow, a huge ambient cloud of one kind of thing | fire, smoke, muzzle flashes, impacts, blasts, exhaust, magic, anything that reacts |

The design follows what Unreal's Niagara taught: an **emitter** is a stack of what happens (spawn, initialise, update, render), values are **curves over the particle's life**, particles can **cause events** that make particles of another emitter (a death spawns smoke, a hit spawns sparks), an effect holds several emitters, and the cost is kept in check by a **budget**, **level of detail** and **culling**.

What it is not: a node-graph editor. An effect is text (an AI agent writes it as easily as a person), the format is checked (`check`) with messages that say where a mistake is, and a running game picks up a saved file at once.

## Using it

```json
{ "uid": 12, "components": {
    "Transform": { "pos": [4, 0, 2] },
    "ParticleSystem": { "effect": "examples/fx/campfire.fx.json" } } }
```

`effect` is the name of a file of the workspace, or **the JSON itself** when the text starts with `{` (handy for a small effect or a test). A file is read again when it changes on disk (a running game shows your edit within half a second). What is wrong with an effect is a warning once (`render` returns it; `check` reports it as an error with the place in the document) and the system draws nothing until it is fixed.

| Field | Default | |
|---|---|---|
| `effect` | empty | the file, or the JSON |
| `playing` | true | the clock runs by itself while it is |
| `time` | 0 | the clock in seconds: the particles are stepped up to it. Set it to 0 to play an effect that has ended again |
| `speed` | 1 | how fast the clock runs |
| `repeat` | 0 | seconds; when the clock reaches it, it goes back to 0 (a blast or a shot that plays over and over; when this is longer than the effect's longest life nobody sees the seam) |
| `seed` | 0 | 0 takes the entity's number, so two campfires are not twins |
| `visible` | true | |
| `scale` | 1 | scales sizes and distances (the entity's own scale does too) |
| `intensity` | 1 | multiplies every rate and burst: 0 stops it making more (the particles that live go on). A burning car's smoke is `health` of it |
| `tint` | white | multiplies every colour and the opacity |
| `quality` | 1 | 0.1 to 1: thins out the rates and the budget for a slow machine (a graphics setting) |

Start the clock at 0 and **set it back to 0 to fire a one-shot effect again**. For a muzzle flash that happens at a shot, spawn an entity with the effect and a `Lifetime` (the same way the engine's own splashes are made), or keep one and set `time` to 0.

## The effect

```json
{
  "name": "campfire",
  "bounds": 5,
  "budget": 900,
  "lod": { "near": 25, "far": 70 },
  "prewarm": 0,
  "emitters": [ { ... }, { ... } ]
}
```

| Key | |
|---|---|
| `name` | for messages |
| `bounds` | the radius (metres) of a ball round the entity that holds every particle; outside the picture nothing is stepped or drawn. 0 (default): worked out from the emitters' numbers (generous). Say it when the effect is much smaller than that |
| `budget` | the most particles all the emitters hold together (default 4096, at most 200,000) |
| `lod` | `near` and `far` (metres from the camera): full up to `near`, thinning out to nothing at `far` (the rates and bursts are scaled), nothing beyond |
| `prewarm` | seconds a system that has just appeared has already run, so that a looping fire is full in the first picture (default: the longest life) |
| `emitters` | 1 to 32 |

### An emitter

```json
{ "name": "flames", "space": "local", "max": 220, "loop": true, "duration": 0, "delay": 0, "child": false,
  "spawn": { ... }, "init": { ... }, "update": { ... }, "curves": { ... }, "render": { ... }, "events": [ ... ] }
```

| Key | |
|---|---|
| `name` | what events call it (default `emitter0`, `emitter1`, ...) |
| `space` | `local` (default): the particles move with the entity, in its space (a flame on a torch). `world`: they stay where they were born (smoke behind a car). Everything in `init`/`update` is in the emitter's space; a `local` emitter is scaled and turned with the entity when it is drawn; a `world` emitter's particles are born at the entity's scale and then follow the world |
| `max` | the most it holds at once (default 1000) |
| `simulate` | `"cpu"` (default) or `"gpu"`: see "On the card" below |
| `loop` | true (default): it starts over after `duration` seconds (0: it never stops). false: it makes particles for `duration` seconds and is done (0: only its bursts) |
| `duration` `delay` | seconds; `delay` seconds after the system's clock starts |
| `child` | true: makes nothing by itself, only when an event asks (see Events) |

**Spawn**

| Key | |
|---|---|
| `rate` | particles a second while active, times the system's `intensity`, the level of detail and `rate_curve` |
| `rate_curve` | keys `[[0, 1], [1, 0]]` along the emitter's round (0 its start, 1 its end; a round is `duration` seconds, or 1 when it has none) |
| `bursts` | `[{ "time": 0, "count": 20, "chance": 1 }]`: `count` is a number or `[least, most]`; a looping emitter with a `duration` fires its bursts again in every round |
| `shape` | where they are born: `"point"`, `{"type":"sphere","radius":1,"shell":false}`, `{"type":"box","half":[1,1,1]}`, `{"type":"disc","radius":1,"ring":false}` (horizontal), `{"type":"line","to":[0,2,0]}` |

A stream's particles are born **inside** the step (each has already flown for the fraction of the step that is left), so a fast emitter leaves a smooth line and not a row of clumps; the entity's movement is interpolated through the steps of one picture.

**Init** (a number or a pair `[least, most]` is a range, and each particle picks its own)

| Key | |
|---|---|
| `life` | seconds (0.01 to 3600) |
| `speed` | m/s |
| `direction` `spread` | the way they fly, spread by `spread` degrees about it (180: every direction) |
| `radial` | 0..1: how much of the direction is "away from the middle of the shape" |
| `size` `aspect` | a picture's width in metres, and its height as a multiple of it |
| `rotation` `spin` | degrees; degrees a second |
| `color` `color_jitter` | `"#ff8800"`, `"#f808"`, `"#ff880080"` or `[r, g, b, a]` (display colours; above 1 glows when added); the brightness strays by up to `color_jitter` (0..1) |
| `inherit_velocity` | 0..1: how much of the entity's speed a `world` particle takes along |

**Update**

| Key | |
|---|---|
| `gravity` `wind` | m/s², in the emitter's space |
| `drag` | the speed decays by `e^(-drag t)` |
| `noise` | `{"strength": 2, "scale": 0.8, "speed": 0.5}`: a wandering force that **swirls** (the curl of a noise, so it never piles particles up); `scale` is the size of a swirl in metres, `speed` how fast the field changes |
| `vortices` | `[{"pos":[0,0,0],"axis":[0,1,0],"strength":4,"pull":0,"radius":5}]` turns them about an axis through `pos` and draws them in by `pull` |
| `attractors` | `[{"pos":[0,3,0],"strength":5,"radius":8,"falloff":1,"kill":0}]`; a particle within `kill` metres of the point is gone (motes drawn into a core) |
| `speed_limit` | the most speed (0: none) |
| `collide` | see below |

`collide`: `{"mode":"bounce"|"kill"|"stick","bounce":0.4,"friction":0.2,"life_loss":0,"planes":[...],"balls":[...],"boxes":[...]}`. A **plane** is `{"normal":[0,1,0],"d":0,"relative":false}` (`dot(normal, p) = d`; `relative: true` moves it with the entity: the ground under it, turned as it is turned: **an entity whose up is the surface's normal puts the sparks of an impact on that surface**). A **ball** `{"pos":[..],"radius":1,"inside":false}` and a **box** `{"pos":[..],"half":[..]}` are in the entity's space. A hit is an event (below) when it is harder than 1 m/s: a particle that rolls does not make sparks at every step.

`"world": true` makes the particles also collide with **the colliders of the world**: the same ones a bullet or a wheel meets (the boxes, balls and capsules of `Collider`s, mesh colliders, terrain), found by casting the step a particle just took as a ray. The world round the systems that ask (their `bounds` and 4 m) is gathered once for each picture, whatever the number of systems; measured, 40 systems with 4,800 live particles among 5,000 box colliders cost 0.17 ms a picture more than without (docs/PERFORMANCE.md). Keep it to the emitters that need it (sparks, debris, droplets), not a haze of thousands. The colliders as they were when the picture was made are used for all the steps of that picture.

**Curves** are keys `[[t, value], ...]` from `t` 0 (born) to 1 (gone) joined by straight lines, or one number:

| Key | multiplies |
|---|---|
| `size` | the size the particle was born with |
| `speed` | its speed |
| `alpha` | its opacity |
| `color` | its colour: keys `[[t, colour], ...]` (a colour as above) |
| `light` | the light it gives |

**Render**

| Key | |
|---|---|
| `type` | `sprite` (default), `ribbon` (a strip through the places the particle has been: `"ribbon": {"points": 8, "interval": 0.04, "width": 1}`, thinning and fading to the tail), `solid` (a small tumbling `cube`, `diamond` or `tetra`) |
| `texture` | a picture of the workspace, or `soft` (default), `circle`, `square` (all of it opaque white: a hard-edged block), `ring`, `gradient`, `checker` |
| `filter` | `linear` (default: smooth) or `nearest` (the texels stay hard squares: pixel art, voxel looks) |
| `sheet` `frames` | a flipbook of `[columns, rows]` frames (`frames` the number used) |
| `frame_mode` `fps` | `life` (plays once over the particle's life; the default), `random` (each particle keeps one), or `fps` (plays at that rate and loops) |
| `blend` | `alpha` (default; smoke, dust) or `add` (fire, sparks, magic: no sorting needed) |
| `sort` | alpha-blended pictures are drawn far to near; `false` draws them in the order of birth (cheaper, fine for many small things) |
| `facing` | `camera` (default), `velocity` (along the way it flies, as long as its size plus its speed times `stretch` seconds: sparks, rain), or `axis` with `"axis": [0,1,0]` (a flat picture fixed in a direction: a ripple on the ground) |
| `soft` | metres: the picture fades out where it comes within this distance of a surface behind it, so smoke does not cut a hard line where it meets the ground. The GPU renderer's high qualities do it (it needs the depth of everything opaque, which the renderer makes only when some system has `soft`); the reference picture draws them without |
| `light` | `{"intensity": 3, "range": 9, "max": 2, "color": [1,0.6,0.3]}`: the `max` brightest particles of the emitter each give a point light of their colour (or `color`), as bright as their opacity and `curves.light` say. Lights are added to the picture's lights (the GPU renderer picks the sixteen that matter) |

**Events**

```json
"events": [ { "on": "death", "spawn": "smoke", "count": [0, 1], "chance": 0.3, "inherit_velocity": 0.5, "inherit_color": 0 } ]
```

`on`: `birth`, `death` (it aged out, was killed by a collider or an attractor) or `collide`. `spawn` is the **name of another emitter of the same effect** (a mistake says what it could have been). The new particles are born at the place where it happened, a step later, with that emitter's `init` (plus `inherit_velocity` of the speed of the particle that caused it, and `inherit_color` of its colour). Mark the target `"child": true` so it makes none of its own. At most 8,192 births a step.

## On the card (`"simulate": "gpu"`)

An emitter that says `"simulate": "gpu"` has its particles stepped by a **compute pass on the graphics card** (the GPU renderer's high qualities) instead of the processor: tens of thousands of particles cost the processor nothing it can measure. The processor still decides every birth (rate, bursts, delay, `rate_curve`, `intensity`, level of detail) in the same steps of 1/60 s and hands the card, for each step, the slots of the emitter's ring the births go into; the card keeps each particle's position, speed, age, life and serial number (48 bytes), steps them (gravity, drag, wind, curl noise, up to two vortices and two attractors with `kill`, a speed limit, up to two planes), and draws them into the same vertex buffer layout the other particle clouds use, so blend, texture, `soft` and the order are the ones every cloud has. The size, colour jitter, spin and flipbook frame of a particle are hashes of its serial number (so they cost no memory); the curves are tables of 32 samples.

What the card does not do, and a message says so when an emitter that asks for it has any of it: events (in or out), children, ribbons and solids (sprites only), a light for each particle, balls, boxes and the world's colliders (planes only), and sorting (alpha-blended pictures are drawn in the order of the ring: use `add`, or small particles). Everything else in the format works the same. A renderer with no card (the reference picture, `render` with `quality: reference`) steps the same emitter on the processor, so an effect is always drawn; the particles are not the same ones (another random generator), the cloud is the same kind: tests compare their amount of light, middle and width. A system whose renderer changes (the player switches from the card to the reference picture) starts over.

The mind of `examples/fx/mind_motes.fx.json` is 12,000 motes with noise, a vortex and an attractor that eats them: stepped on the processor it is 2.5 ms a picture, on the card 0.00 ms on the processor (docs/PERFORMANCE.md); its glow (a light) is a second emitter, on the processor, because a light is one.

## How it runs

* **Stepping.** (For an emitter on the card only the births are decided here.) The picture steps every visible system from where it was to the component's clock, in steps of 1/60 s (the picture of a slow frame steps twice). It happens in the **picture**, where the camera is: the simulation never does it, so a **server pays nothing**, and the state hash never sees it (the clock `time` is game state; the particles are not).
* **Culling.** A system whose ball (`bounds`, or the smoke that is still hanging in the picture) is outside the picture is neither stepped nor drawn. When it comes back after more than half a second (a looping effect: more than it takes to fill up again) it starts from a steady state; one whose clock went back starts over. Systems nobody asks for in ten seconds are forgotten.
* **Determinism.** Every random number comes from a generator seeded by the system's `seed` and the emitter's number; every sine and exponential from `detmath`; the systems are independent and are stepped in parallel, each by one thread. The same history (effect, seed and the clocks the pictures saw) gives the same particles on every machine and for any number of threads. A system seen for the first time at some clock starts from a steady state (a loop is full, as if it had run since before time 0), which is the same whatever the clock was.
* **Cost.** Each live particle costs a few tens of nanoseconds a step (a noise force about 55 ns more), and each drawn one four vertices (a ribbon two a point, a solid 36 at most). `bounds`, `lod`, `budget` and `quality` are the levers; `check` warns of an effect whose emitters can hold far more than its budget.
* **Order.** An effect's emitters are drawn in the order they are written, except that, as for every other particle cloud, the alpha-blended pictures are drawn before the added light.

## Recipes

The folder `examples/fx` holds working effects; `examples/fx_showcase.lunaris.json` shows all of them (`lunaris check examples/fx_showcase.lunaris.json`, then `render`).

| File | What |
|---|---|
| `campfire` | flames (local space, added, a light), embers (world, stretched), smoke (world, soft) |
| `burning_wreck` | the same grown up for a car: a box of fire, a column of black smoke, sparks that bounce |
| `muzzle_flash` | a flash with a light, a spray of stretched sparks, a puff of smoke (a one-shot: `repeat` or a `Lifetime`) |
| `bullet_impact` | dust, sparks and chips that bounce off the world's colliders (and a plane in the entity's frame as a floor when there is no collider); **the entity's up is the surface's normal** |
| `explosion` | fireball with a light, a shockwave ring on the ground, sparks, debris, a smoke that rises and thins |
| `tracer` | a ribbon behind a fast round |
| `toxic_dust` | a drifting haze and glittering motes in a box round the entity |
| `voxel/` | the same set for a blocky game: cube solids for fire, smoke, debris, chips (per surface: concrete, metal, wood, dirt), a brass cube casing, hard-edged `square` sprites with `filter: nearest`, rain streaks on the card, machine sparks and a death burst |
| `rain_splash` | rings and droplets on the ground (the rain itself is the card's: `Environment`) |
| `mind_motes` | ten thousand motes spiralling into a core: vortex, attractor with `kill`, noise, colour over life |

Small tricks: a light that flickers is a `light` curve that goes up and down; a spark that fades before it lands is `alpha` ending at 0 before `life`; an effect that must look bigger at a distance has a larger `size` curve end; a burning car's smoke follows `intensity`.

## What is missing (and next)

* **Mesh particles** from a model file (small solids are there), and a card emitter that can do events, solids and ribbons (today `simulate: gpu` is for sprites).
* A preview inside the editor (today `render` at a few clocks shows an effect, and `time` can be set).
* Sub-emitter **trails by distance** (ribbons are by time).
