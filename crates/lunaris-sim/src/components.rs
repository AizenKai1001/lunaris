//! Standard components every Lunaris world knows. 2D games use the same `Transform` (x, y, a layer in z, rotation about z).
use lunaris_ecs::{component, Sym, Uid};

component! {
    /// Human-readable name of an entity (not unique).
    pub struct Name { pub value: Sym = Sym(0) }
}

component! {
    /// Position, rotation (unit quaternion x, y, z, w) and scale.
    pub struct Transform {
        pub pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub rot: [f32; 4] = [0.0, 0.0, 0.0, 1.0],
        pub scale: [f32; 3] = [1.0, 1.0, 1.0],
    }
}

component! {
    /// Throws short-lived particles. Every tick the engine adds `rate * dt` particles (plus `burst` once, then `burst` goes back to 0), up
    /// to `max` alive at a time, each a small entity with a `Sprite` (or, when `mesh` is set, a `MeshRenderer` of that built-in shape: for
    /// 3D scenes) that moves, falls by `gravity`, shrinks from `size0` to `size1` and fades from `color0` to `color1` over its `life`, then
    /// disappears. Directions are random inside a cone around `dir` (`spread` 0 = a straight line, 1 = all directions); every random
    /// number depends only on the seed, the tick and the emitter, so a replay throws exactly the same particles. Each particle lives `life`
    /// times a random factor between 1 - `life_var` and 1 + `life_var`; `carry` is the fraction of a particle owed from earlier ticks (the
    /// engine keeps it up to date).
    pub struct Emitter {
        pub active: bool = true,
        pub rate: f32 = 20.0,
        pub burst: u32 = 0,
        pub life: f32 = 1.0,
        pub life_var: f32 = 0.2,
        pub speed: f32 = 2.0,
        pub speed_var: f32 = 0.3,
        pub dir: [f32; 3] = [0.0, 1.0, 0.0],
        pub spread: f32 = 0.15,
        pub gravity: [f32; 3] = [0.0, -3.0, 0.0],
        pub size0: f32 = 0.25,
        pub size1: f32 = 0.0,
        pub color0: [f32; 4] = [1.0, 0.8, 0.3, 1.0],
        pub color1: [f32; 4] = [1.0, 0.2, 0.05, 0.0],
        pub texture: Sym = Sym(0),
        pub mesh: Sym = Sym(0),
        pub max: u32 = 400,
        pub carry: f32 = 0.0,
    }
}

component! {
    /// Marks a particle made by an `Emitter`. The engine ages it, moves its colour and size along, and deletes it at the end of its life.
    pub struct Particle {
        pub age: f32 = 0.0,
        pub life: f32 = 1.0,
        pub size0: f32 = 0.25,
        pub size1: f32 = 0.0,
        pub color0: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub color1: [f32; 4] = [1.0, 1.0, 1.0, 0.0],
        pub gravity: [f32; 3] = [0.0, -3.0, 0.0],
        pub emitter: Uid = Uid(0),
    }
}

component! {
    /// More for an `Emitter` (put it beside it; a plain `Emitter` is unchanged): where particles start (`shape` 0 the emitter's point, 1 inside a sphere of
    /// radius `extent[0]`, 2 inside a box of half sizes `extent`, 3 on a flat disc of radius `extent[0]` in the xz plane), `drag` (the share of speed lost per
    /// second), `turbulence` (a random push in metres per second squared), `spin` (radians per second, random either way, for sprites), a flat `floor` at height
    /// `floor_y` that particles bounce off (`bounce` keeps that share of the speed; `stick` removes them instead) and a middle stop in the colour and the size
    /// (`color_mid` and `size_mid` reached at `mid` of the particle's life; `mid` 0 turns both off).
    pub struct EmitterFx {
        pub shape: u32 = 0,
        pub extent: [f32; 3] = [1.0, 1.0, 1.0],
        pub drag: f32 = 0.0,
        pub turbulence: f32 = 0.0,
        pub spin: f32 = 0.0,
        pub floor: bool = false,
        pub floor_y: f32 = 0.0,
        pub bounce: f32 = 0.4,
        pub stick: bool = false,
        pub mid: f32 = 0.0,
        pub color_mid: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub size_mid: f32 = 0.25,
    }
}

component! {
    /// The `EmitterFx` of a particle's emitter, copied to the particle when it is thrown (the engine adds it; it does not change afterwards).
    pub struct ParticleFx {
        pub drag: f32 = 0.0,
        pub turbulence: f32 = 0.0,
        pub floor: bool = false,
        pub floor_y: f32 = 0.0,
        pub bounce: f32 = 0.4,
        pub stick: bool = false,
        pub mid: f32 = 0.0,
        pub color_mid: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub size_mid: f32 = 0.25,
    }
}

component! {
    /// Attaches this entity to another one (by uid; 0 = nobody). With a `LocalTransform` and a `Transform`, the entity's
    /// `Transform` is worked out from the parent's `Transform` after every tick and every edit, so it follows its parent.
    pub struct Parent { pub target: Uid = Uid(0) }
}

component! {
    /// Position, rotation and scale relative to the `Parent` (edit this, not the `Transform`, of an attached entity).
    pub struct LocalTransform {
        pub pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub rot: [f32; 4] = [0.0, 0.0, 0.0, 1.0],
        pub scale: [f32; 3] = [1.0, 1.0, 1.0],
    }
}

component! {
    /// Linear velocity (units per second) and angular velocity (radians per second about each axis).
    pub struct Velocity { pub lin: [f32; 3] = [0.0, 0.0, 0.0], pub ang: [f32; 3] = [0.0, 0.0, 0.0] }
}

component! {
    /// Random walk: each tick the velocity is nudged by up to `strength` units per second (deterministic, per entity).
    pub struct Wander { pub strength: f32 = 1.0 }
}

component! {
    /// A camera. Looks down the entity's local -Z axis. Perspective by default; `ortho` makes it an orthographic (2D) camera
    /// that shows `ortho_height` world units from top to bottom. The active camera with the lowest uid is used.
    pub struct Camera {
        pub fov_y: f32 = 60.0,
        pub near: f32 = 0.1,
        pub far: f32 = 1000.0,
        pub ortho: bool = false,
        pub ortho_height: f32 = 10.0,
        pub active: bool = true,
        pub clear: [f32; 4] = [0.05, 0.05, 0.08, 1.0],
    }
}

component! {
    /// Draws a mesh at the entity's Transform: a built-in (`cube`, `sphere`, `plane`, `cylinder`, `quad`, each one unit in size and centred
    /// on the origin) or a model file in the workspace (`models/ship.glb`, `.gltf`). `color` multiplies the model's own colours. Colours are
    /// 0..1 display colours. `emissive` makes the surface glow: that fraction of its colour is added on top of the lighting
    /// (1 = shows its full colour even in the dark: a sun, a lamp, lava).
    ///
    /// Material: `texture` (a built-in picture such as `checker`, `bricks`, `grass`, `wood`, `stone`, or the path of a PNG) multiplies `color`
    /// and repeats `uv_scale` times over the mesh (a floor `plane` of 20 x 20 units with `uv_scale` [10, 10] shows a tile every 2 units).
    /// `metallic` (0 = plastic, stone, wood; 1 = bare metal) and `roughness` (0 = mirror-like, 1 = chalk) drive the physically based shading
    /// of the GPU renderer's high-quality mode; a negative `roughness` (the default) is worked out from `shininess`. `cast_shadow` lets the
    /// mesh throw shadows in that mode. `bump` is how deep (in world units) the built-in material textures' relief is: bricks and
    /// cobbles stand out of their mortar, wood grain catches the light (0 = flat). The reference CPU renderer ignores `metallic`,
    /// `roughness`, `cast_shadow` and `bump`.
    pub struct MeshRenderer {
        pub mesh: Sym = Sym(0),
        pub color: [f32; 4] = [0.8, 0.8, 0.8, 1.0],
        pub shininess: f32 = 32.0,
        pub visible: bool = true,
        pub emissive: f32 = 0.0,
        pub metallic: f32 = 0.0,
        pub roughness: f32 = -1.0,
        pub texture: Sym = Sym(0),
        pub uv_scale: [f32; 2] = [1.0, 1.0],
        pub cast_shadow: bool = true,
        pub bump: f32 = 0.03,
    }
}

component! {
    /// Plays an animation of an imported model (a `.glb` / `.gltf` with a skeleton and animation clips) on the entity's `MeshRenderer`.
    /// `clip` is the clip's name (empty = the model's first clip; the `model_info` tool lists them), `time` is how many seconds into it
    /// the picture is (it advances by `speed` times the fixed step while `playing`), and with `looping` the clip repeats, otherwise it
    /// holds its last pose. Without an `Animator` a skinned model stands in its rest pose. Set `time` to scrub, `speed` 0 to freeze.
    /// **Changing `clip` crossfades**: the picture blends from the old clip (which keeps playing) to the new one over `fade` seconds (0.25 by
    /// default, 0 cuts), so a graph system can switch idle, walk and run by just setting `Animator.clip`. `blend` (0..1, 1 = only the
    /// current clip) and the `prev_*` and `last_*` fields are the engine's bookkeeping for that; leave them alone.
    pub struct Animator {
        pub clip: Sym = Sym(0),
        pub time: f32 = 0.0,
        pub speed: f32 = 1.0,
        pub playing: bool = true,
        pub looping: bool = true,
        pub fade: f32 = 0.25,
        pub blend: f32 = 1.0,
        pub prev_clip: Sym = Sym(0),
        pub prev_time: f32 = 0.0,
        pub last_clip: Sym = Sym(0),
        pub last_time: f32 = 0.0,
        pub started: bool = false,
    }
}

component! {
    /// The look of the whole picture for the GPU renderer's high-quality mode (the first `Environment` entity, by uid, counts; without one a
    /// plain look is worked out from the camera's `clear` colour). All colours are 0..1 display colours.
    /// `sky` draws a procedural sky (`sky_top` at the zenith fading to `sky_horizon`, `ground` below the horizon, and a sun disc where the
    /// first directional light comes from) instead of the camera's flat `clear` colour; the sky also lights the scene from all sides
    /// (`ambient` scales that) and shows up as reflections on glossy and metallic surfaces. `fog` is the density of distance fog tinted
    /// `fog_color` (0 = none, 0.02 = light haze, 0.1 = thick). `shadows` turns the sun's shadows on, out to `shadow_distance` world units
    /// from the camera. `ssao` (0..2) darkens creases and contact points, `bloom` (0..2) makes bright things glow (only what is brighter
    /// than `bloom_threshold`), `vignette` darkens the corners, `exposure` brightens or darkens everything, `saturation` scales the colour.
    /// `clouds` (0..1) is how much of the sky they cover. `dof` (0..1) is depth of field: what is further from the camera than `focus` metres
    /// (plus `focus_range` metres that stay sharp) or nearer than it is blurred, the more the further off it is (0 = everything sharp).
    /// `tonemap`: 0 = automatic, 1 = filmic (ACES), 2 = neutral (keeps colours as authored), 3 = none (clamp).
    pub struct Environment {
        pub sky: bool = true,
        pub sky_top: [f32; 3] = [0.20, 0.42, 0.85],
        pub sky_horizon: [f32; 3] = [0.72, 0.82, 0.95],
        pub ground: [f32; 3] = [0.30, 0.27, 0.24],
        pub ambient: f32 = 1.0,
        pub exposure: f32 = 1.0,
        pub clouds: f32 = 0.35,
        pub fog: f32 = 0.0,
        pub fog_color: [f32; 3] = [0.72, 0.82, 0.95],
        pub shadows: bool = true,
        pub shadow_distance: f32 = 80.0,
        pub ssao: f32 = 0.8,
        pub bloom: f32 = 0.3,
        pub bloom_threshold: f32 = 1.0,
        pub vignette: f32 = 0.2,
        pub saturation: f32 = 1.0,
        pub tonemap: u32 = 0,
        pub dof: f32 = 0.0,
        pub focus: f32 = 10.0,
        pub focus_range: f32 = 4.0,
    }
}

component! {
    /// A flat 2D picture: a rectangle of `size` world units in the entity's XY plane, optionally textured
    /// (`circle`, `ring`, `checker`, `gradient`, or the path of a PNG in the workspace). Drawn after 3D meshes, back to front.
    /// For a sprite sheet the texture is a grid of `sheet[0]` columns by `sheet[1]` rows of frames and `frame` (counted row by row from the
    /// top left) picks the one to draw: change `frame` over time to animate.
    pub struct Sprite {
        pub size: [f32; 2] = [1.0, 1.0],
        pub color: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub texture: Sym = Sym(0),
        pub visible: bool = true,
        pub sheet: [u32; 2] = [1, 1],
        pub frame: u32 = 0,
    }
}

component! {
    /// A grid of tiles drawn as one sprite layer, centred on the entity: `size` tiles across and down, each `tile` world units square. `data`
    /// lists the tile numbers row by row, top row first, separated by spaces, commas or line breaks; a number picks a frame of the tileset
    /// `texture` (a grid of `sheet` columns by rows, counted from the top left) and -1 (or a missing number) leaves the cell empty.
    pub struct Tilemap {
        pub texture: Sym = Sym(0),
        pub sheet: [u32; 2] = [1, 1],
        pub size: [u32; 2] = [1, 1],
        pub tile: f32 = 1.0,
        pub data: Sym = Sym(0),
        pub color: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub visible: bool = true,
    }
}

component! {
    /// Text drawn over the picture (a HUD), in screen space, with the built-in pixel font. `pos` is the anchor as a fraction of the
    /// image (0,0 top left); `size` is the text height as a fraction of the image height; `align` 0 = anchor is the left edge,
    /// 1 = centre, 2 = right edge. `value` may hold `{resource}` placeholders, replaced by that world resource's current value.
    pub struct Text {
        pub value: Sym = Sym(0),
        pub pos: [f32; 2] = [0.02, 0.03],
        pub size: f32 = 0.05,
        pub color: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub align: u32 = 0,
        pub shadow: bool = true,
        pub visible: bool = true,
    }
}

component! {
    /// A sound effect, by name: when a graph system emits an event with this name (`{"emit": "coin"}`) the editor plays it with a small
    /// synthesiser. `wave` 0 sine, 1 square, 2 saw, 3 triangle, 4 noise; the pitch glides from `freq` to `freq_end` (0 = stay) over `duration`
    /// seconds. Events whose name has no Sound entity fall back to built-ins for common names (coin, hit, jump, win, lose, click, shoot, explosion).
    pub struct Sound {
        pub name: Sym = Sym(0),
        pub wave: u32 = 0,
        pub freq: f32 = 440.0,
        pub freq_end: f32 = 0.0,
        pub duration: f32 = 0.15,
        pub volume: f32 = 0.3,
    }
}

component! {
    /// A recorded sound for an event name: when a graph system emits an event called `name` (`{"emit": "door"}`) the native player plays the
    /// WAV file `file` (a path in the project folder; 8/16/24/32 bit PCM or float, mono or stereo, any sample rate) instead of a synthesised
    /// voice. With `spatial` the sound is placed where the emitting entity was: it gets quieter with distance (full loudness inside `near`
    /// metres, nothing beyond `range`), moves between the ears with its direction, is muffled by walls and rings with the room (see
    /// `AcousticMaterial`). `volume` is 0..8 and `pitch` multiplies the playback speed.
    pub struct SoundFile {
        pub name: Sym = Sym(0),
        pub file: Sym = Sym(0),
        pub volume: f32 = 1.0,
        pub pitch: f32 = 1.0,
        pub spatial: bool = true,
        pub range: f32 = 40.0,
        pub near: f32 = 2.0,
    }
}

component! {
    /// A sound that plays by itself from the entity's position (a campfire, a machine, music from a radio): the WAV file `file` (a path in
    /// the project folder; empty = a built-in voice named by `voice`, such as `hit` or `coin`), in a loop when `looped`, for as long as
    /// `playing` is true. It follows the entity, so it can be on a moving thing, and with `spatial` it is placed in the world like a
    /// `SoundFile` (distance, left and right, muffled by walls, reverberation of the room); without it it plays flat, like music.
    pub struct AudioSource {
        pub file: Sym = Sym(0),
        pub voice: Sym = Sym(0),
        pub volume: f32 = 1.0,
        pub pitch: f32 = 1.0,
        pub looped: bool = true,
        pub playing: bool = true,
        pub spatial: bool = true,
        pub range: f32 = 40.0,
        pub near: f32 = 2.0,
    }
}

component! {
    /// How a surface treats sound, for entities that have a `Collider` (they are what sound bounces off and is stopped by; without this
    /// component a collider counts as a plain wall). `absorption` 0..1 is the share of the energy a reflection loses: 0.02 bare concrete or
    /// tile, 0.12 a furnished room (the default), 0.5 thick carpet and curtains, 1 nothing comes back. `transmission` 0..1 is the amplitude
    /// that gets through a slab 0.2 m thick (thicker passes less): 0.03 a wall (the default), 0.3 a wooden door, 1 a curtain of air.
    pub struct AcousticMaterial {
        pub absorption: f32 = 0.12,
        pub transmission: f32 = 0.03,
    }
}

component! {
    /// A light. `kind` 0 = directional (shines along the entity's -Z axis), 1 = point (at the entity's position, fades out at `range`),
    /// 2 = spot (a point light that only shines inside a cone around the entity's -Z axis: `angle` degrees from the axis to the edge of the
    /// cone, the last `softness` fraction of it fading out). The reference renderer and the GPU renderer's classic mode draw a spot light as
    /// a point light; the high-quality mode draws the cone.
    pub struct Light {
        pub kind: u32 = 0,
        pub color: [f32; 3] = [1.0, 1.0, 1.0],
        pub intensity: f32 = 1.0,
        pub range: f32 = 20.0,
        pub angle: f32 = 35.0,
        pub softness: f32 = 0.3,
    }
}


component! {
    /// A collision shape for physics. `shape` 0 = sphere (diameter `size[0]`, scaled by the largest scale axis), 1 = box (full extents
    /// `size`, scaled per axis; it stays axis-aligned unless the entity's `Transform.rot` is turned or it has an `AngularVelocity`, then
    /// it is an oriented box), 2 = capsule (a cylinder with a half-sphere on each end, along the entity's local y axis: diameter `size[0]`
    /// scaled by the larger of the x and z scales, total height `size[1]` scaled by the y scale; `size[2]` is not used by physics:
    /// set it to `size[0]`, because characters and navigation see a capsule as the box `size`). A `trigger` reports overlaps but never
    /// pushes anything.
    pub struct Collider {
        pub shape: u32 = 0,
        pub size: [f32; 3] = [1.0, 1.0, 1.0],
        pub trigger: bool = false,
    }
}

component! {
    /// Ground made from a heightmap (`docs/TERRAIN.md`): a greyscale PNG of the workspace (16 bits is best) where black is the lowest
    /// ground and white is `height` metres higher; the picture covers `size` metres (x, z), centred on the entity, whose `Transform.y`
    /// is the height of black. The rotation and scale of the entity are not used. Spheres, boxes and characters stand on it and rays hit
    /// it (`solid`), and both renderers draw it (`visible`): `color` and an optional `texture` (repeated `uv_scale` times across the ground)
    /// give it its look. `step` uses every step-th sample of the picture, for a coarser (cheaper) ground; 1 uses them all.
    /// `friction` and `restitution` are the ground's own contact material.
    pub struct Terrain {
        pub heightmap: Sym = Sym(0),
        pub size: [f32; 2] = [256.0, 256.0],
        pub height: f32 = 64.0,
        pub step: u32 = 1,
        pub color: [f32; 4] = [0.34, 0.48, 0.27, 1.0],
        pub texture: Sym = Sym(0),
        pub uv_scale: [f32; 2] = [64.0, 64.0],
        pub friction: f32 = 0.8,
        pub restitution: f32 = 0.1,
        pub solid: bool = true,
        pub visible: bool = true,
    }
}

component! {
    /// Physical properties. A body that also has a `Velocity` and `mass > 0` is dynamic; anything else with a Collider is static.
    pub struct RigidBody {
        pub mass: f32 = 1.0,
        pub restitution: f32 = 0.3,
        pub friction: f32 = 0.5,
        pub gravity_scale: f32 = 1.0,
        pub drag: f32 = 0.0,
    }
}

component! {
    /// A character controller: a capsule that walks, jumps, climbs low steps, slides along walls and falls, moved by the engine every tick
    /// (after the graph systems, so this tick's input counts) instead of by rigid-body physics. The entity's `Transform` is its **feet**
    /// (put the visible model a child or an offset above it). Set `walk` (the horizontal speed it wants in x and z, metres per second)
    /// and `jump` (a request for that upward speed, honoured only on the ground and cleared every tick) from a graph system; read
    /// `grounded` and `fall` (the vertical speed, negative when falling) back. It collides with every solid (non-trigger) `Collider`,
    /// spheres and axis-aligned boxes, except other characters. `step_height` is the tallest ledge it climbs without jumping,
    /// `max_slope` (degrees) how steep a surface may be and still count as ground, `gravity_scale` 0 turns gravity off (flying), and the
    /// gravity is the `PhysicsSettings` one.
    pub struct CharacterBody {
        pub radius: f32 = 0.35,
        pub height: f32 = 1.8,
        pub walk: [f32; 2] = [0.0, 0.0],
        pub jump: f32 = 0.0,
        pub gravity_scale: f32 = 1.0,
        pub step_height: f32 = 0.4,
        pub max_slope: f32 = 50.0,
        pub fall: f32 = 0.0,
        pub grounded: bool = false,
    }
}

component! {
    /// A rectangle drawn over the picture, for panels, bars, buttons and menus. `pos` is its top-left corner and `size` its width and height,
    /// both as fractions of the picture (0..1; the same convention as `Text`, so `pos` [0.4, 0.45] and `size` [0.2, 0.1] is a box in the
    /// middle). It is drawn in `color`; `fill` below 1 draws only the left part of it in `color` and the rest in `back_color` (a health bar:
    /// set `fill` from a graph system), `border` (a fraction of the picture's height) draws an edge in `border_color`, and `text` is a label
    /// centred in it (with `{resource}` placeholders, like `Text`). `layer` orders overlapping rectangles (higher is on top; equal layers by
    /// uid). A `button` reacts to the mouse (`input.mouse_x`, `input.mouse_y`, `input.mouse_left`): the engine sets `hover` while the pointer is
    /// over it, `pressed` while the left button is down over it, and `clicked` for exactly one tick when the button is released over it after
    /// being pressed over it (a graph system turns that into an effect: `{"if": {"get": "Ui.clicked"}, "then": [...]}`). A button looks
    /// lighter under the pointer and darker while pressed. Not part of the 3D picture: it is drawn last, at the picture's own resolution.
    pub struct Ui {
        pub pos: [f32; 2] = [0.4, 0.45],
        pub size: [f32; 2] = [0.2, 0.1],
        pub color: [f32; 4] = [0.16, 0.2, 0.32, 0.92],
        pub back_color: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub fill: f32 = 1.0,
        pub border_color: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub border: f32 = 0.0,
        pub text: Sym = Sym(0),
        pub text_size: f32 = 0.04,
        pub text_color: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub layer: u32 = 0,
        pub visible: bool = true,
        pub button: bool = false,
        pub hover: bool = false,
        pub pressed: bool = false,
        pub clicked: bool = false,
    }
}

component! {
    /// Lets graph systems react to events (`emit`, animation events, `dialogue_end`...): `names` lists the events to watch, separated by spaces or commas. Every tick, for each
    /// name, the resource `event.NAME` holds how many times that event was emitted **during the tick before** (0 when it was not), and while it is above 0 the resources
    /// `event.NAME.x`, `.y`, `.z` and `.uid` hold where and by whom the first one was emitted. So `{"if": {"gt": [{"res": "event.coin"}, 0]}, ...}` (Script: `if $event.coin > 0`) runs once
    /// for each tick in which a coin was picked up. Names are letters, digits and underscores. Put it on any entity; the watched names of all `EventWatch`es count. A scene
    /// that has none is unchanged. See `docs/GAMEPLAY.md`.
    pub struct EventWatch { pub names: Sym = Sym(0) }
}

component! {
    /// A mark stuck on a surface: a bullet hole, a blood splat, a scorch, a footprint, a crack. It lies at the entity's `Transform.pos` (moved 1 cm out along the surface so it does
    /// not flicker against it), facing along `normal` (the direction the surface faces: the contact normal of what hit it, `[0, 1, 0]` for a floor), turned `roll` radians about
    /// it, `size` metres across, in `color` with the picture `texture` (default a soft dot). With `life` above 0 it disappears after that many seconds, fading over the last
    /// `fade`; `age` is the engine's clock for that. It is drawn like a `Sprite` (after the 3D meshes, seen through by nothing, hidden behind what is in front of it), so it sits
    /// on flat surfaces: on a curved one it floats off the edges. See `docs/GAMEPLAY.md`.
    pub struct Decal {
        pub texture: Sym = Sym(0),
        pub size: [f32; 2] = [0.5, 0.5],
        pub color: [f32; 4] = [0.1, 0.05, 0.05, 0.8],
        pub normal: [f32; 3] = [0.0, 1.0, 0.0],
        pub roll: f32 = 0.0,
        pub life: f32 = 0.0,
        pub fade: f32 = 0.5,
        pub age: f32 = 0.0,
        pub visible: bool = true,
    }
}

component! {
    /// Marks a moving thing as part of the navigation map: a pushable crate, a sliding door, a parked car. Agents plan their paths round the place where it is *when they
    /// plan* (and again when they get stuck against it), instead of ignoring it as they ignore every other dynamic body. It needs a `Collider`; `on` false stops it counting.
    /// See `docs/NAVIGATION.md`.
    pub struct NavObstacle { pub on: bool = true }
}

component! {
    /// Parallax for a `Sprite` or `Tilemap` on the same entity (a far hill, clouds, a forest layer of a 2D game): it is *drawn* shifted against the camera so that it seems to
    /// lie at another depth. `factor` is how much of the camera's movement the layer follows: 1 moves with the camera (a fixed backdrop), 0 stays where it is in the world
    /// (an ordinary sprite), 0.5 drifts at half speed. `origin` is the camera position (x, y) where the layer appears exactly where its `Transform` says. Only the picture
    /// changes; the entity's own position (and anything that collides with it) is not touched. See `docs/GAMES.md`.
    pub struct Parallax {
        pub factor: [f32; 2] = [0.5, 0.5],
        pub origin: [f32; 2] = [0.0, 0.0],
    }
}

component! {
    /// A conversation with branches, run from text and shown on `Text` and `Ui` entities. `script` is the text (see `docs/GAMEPLAY.md`: `== node`, `Speaker: words`,
    /// `* [choice] -> node`, `-> node`, `if CONDITION -> node`, `set`/`res`/`event` actions). Set `active` to true to start it at the node `start` (the first one when
    /// empty); it sets `active` back to false when the talk ends, and emits the events `dialogue_start` and `dialogue_end` (from this entity). The current line is
    /// written to the `Text` or `Ui` entity `text_ui` (its `value` or `text`, and shown; hidden between talks), the speaker's name to `speaker_ui` (hidden for
    /// narration), and the choices on offer, those whose condition holds, to `choice_1` to `choice_4` (buttons: a click picks that one). To continue a line
    /// add one to `advance`, click `text_ui` when it is a button, or press the action `dialogue_next`; to pick a choice set `choose` to 1 to 4 (cleared by the
    /// engine), click it, or press the action `dialogue_choice_N`. `speed` above 0 types the line out that many characters a second (a first press finishes the line).
    /// `node`, `item`, `speaker`, `text`, `elapsed` and `seen_advance` are the talk's state: read `speaker` and `text` to know what is being said.
    pub struct Dialogue {
        pub script: Sym = Sym(0),
        pub active: bool = false,
        pub start: Sym = Sym(0),
        pub text_ui: Uid = Uid(0),
        pub speaker_ui: Uid = Uid(0),
        pub choice_1: Uid = Uid(0),
        pub choice_2: Uid = Uid(0),
        pub choice_3: Uid = Uid(0),
        pub choice_4: Uid = Uid(0),
        pub speed: f32 = 0.0,
        pub advance: u32 = 0,
        pub choose: u32 = 0,
        pub node: Sym = Sym(0),
        pub item: u32 = 0,
        pub speaker: Sym = Sym(0),
        pub text: Sym = Sym(0),
        pub elapsed: f32 = 0.0,
        pub seen_advance: u32 = 0,
    }
}

component! {
    /// A picture drawn by a camera instead of the screen: a minimap, a security monitor, a mirror, a portrait, a scope. The entity of the view is the picture's name:
    /// before each main picture the world is drawn through `camera` (the uid of an entity with a `Camera`, which should have `active` false so it is not the main
    /// view) into a `width` x `height` picture (16 to 1024), kept as the texture `view:UID` (UID this entity's own uid). Use that name anywhere a texture goes: a
    /// `UiStyle.image` (or, simpler, put the `CameraView` on the `Ui` itself and it shows its own view), a `MeshRenderer.texture` of a monitor, a sprite. The view
    /// has no interface and no views of its own (a monitor that looks at itself shows the previous picture). `every` redraws it one picture in `every` (a minimap
    /// needs no 60 a second); `active` false freezes it. It is drawn by the reference renderer, so it costs time in proportion to its size: keep monitors small.
    /// See `docs/GAMEPLAY.md`.
    pub struct CameraView {
        pub camera: Uid = Uid(0),
        pub width: u32 = 256,
        pub height: u32 = 256,
        pub every: u32 = 1,
        pub active: bool = true,
    }
}

component! {
    /// Fires events at moments of an animation, beside an `Animator`: footsteps, the frame a sword hits, a door that opens. `events` is text, one clip a line:
    /// `walk length=0.8: 0.1 step_left, 0.5 step_right` (the clip's name as in the `Animator` and `model_info`, its length in seconds, then `time name` pairs). Every
    /// time the animation passes one of those moments (a looping clip passes it again every loop) the event `name` is emitted from this entity, at its position, for a
    /// graph system (`on event`) or the sound director (`SoundFile` of that name) to answer. Changing clip or scrubbing back does not fire anything. `prev_clip` and
    /// `prev_time` are the engine's bookkeeping. See `docs/GAMEPLAY.md`.
    pub struct AnimEvents {
        pub events: Sym = Sym(0),
        pub prev_clip: Sym = Sym(0),
        pub prev_time: f32 = 0.0,
    }
}

component! {
    /// Root motion: the animation moves the entity. Beside an `Animator`, `curves` says how far the character's root travels in each clip, one clip a line:
    /// `walk length=0.8: 0 0 0 0, 0.4 0 0 0.5, 0.8 0 0 1.0` (a time then x y z, in metres, in the entity's own axes; `model_info` writes this text for a model's clips). Each tick the
    /// entity moves by how far the root travelled in that tick, turned by the entity's rotation and times `scale`, so feet do not slide, and a looping clip carries on from
    /// where its last loop ended. The movement is made on the `Transform` directly (a `CharacterBody` is not asked: give a walking character `CharacterBody.walk` instead).
    /// `prev_clip` and `prev_time` are the engine's bookkeeping. See `docs/GAMEPLAY.md`.
    pub struct RootMotion {
        pub curves: Sym = Sym(0),
        pub scale: f32 = 1.0,
        pub prev_clip: Sym = Sym(0),
        pub prev_time: f32 = 0.0,
    }
}

component! {
    /// A cloud of thousands of billboards for fire, smoke, sparks, rain, snow, dust and magic. There is no entity for any particle: each one is a fixed function of the
    /// cloud's clock `time` (which runs by itself while `playing`, `speed` times as fast), so a cloud of 50,000 costs one entity, and the same clock gives the same
    /// cloud everywhere. `count` particles (up to 60,000) live `lifetime` seconds each; a `looping` cloud is a steady stream (`prewarm`: already full when it appears),
    /// otherwise it is one burst born over `spread_time` seconds from time 0 (set `time` to 0 to fire it again). They start at a point of the `shape` (0 a point, 1 a ball of
    /// radius `extent.x`, 2 a box of half sizes `extent`, 3 a disc, 4 a ring, both of radius `extent.x` in the horizontal plane), fly off in a cone of half angle `cone` degrees
    /// (180 = every direction) round `direction` at a speed between `speed_range`, are pulled by `gravity` and slowed by `drag`, wander by `turbulence` metres, and shrink from
    /// `size.x` to `size.y` (metres, +- `size_jitter`) while their colour runs from `color` to `color_end` (display colours; an alpha of 0 at the end fades them out) after
    /// fading in over the first `fade_in` of their life. `spin` turns them (radians a second), `stretch` above 0 stretches them along their velocity (sparks, rain),
    /// `texture` is a picture (default a soft dot; `soft`, `circle`, or a file) of which `sheet` (columns, rows) frames are played over each life. `additive` adds light
    /// (fire, magic: needs no sorting), otherwise the cloud is alpha-blended (smoke, dust), sorted far to near. Everything is in the entity's local space: moving or turning the
    /// entity moves the whole cloud. Only the picture sees it. See `docs/GAMEPLAY.md`.
    pub struct Particles {
        pub count: u32 = 200,
        pub lifetime: f32 = 2.0,
        pub looping: bool = true,
        pub prewarm: bool = true,
        pub playing: bool = true,
        pub time: f32 = 0.0,
        pub speed: f32 = 1.0,
        pub spread_time: f32 = 0.0,
        pub shape: u32 = 0,
        pub extent: [f32; 3] = [0.5, 0.5, 0.5],
        pub direction: [f32; 3] = [0.0, 1.0, 0.0],
        pub cone: f32 = 25.0,
        pub speed_range: [f32; 2] = [1.0, 3.0],
        pub gravity: [f32; 3] = [0.0, -2.0, 0.0],
        pub drag: f32 = 0.0,
        pub turbulence: f32 = 0.0,
        pub turbulence_scale: f32 = 1.0,
        pub size: [f32; 2] = [0.2, 0.05],
        pub size_jitter: f32 = 0.3,
        pub color: [f32; 4] = [1.0, 0.7, 0.2, 1.0],
        pub color_end: [f32; 4] = [1.0, 0.1, 0.0, 0.0],
        pub fade_in: f32 = 0.05,
        pub spin: f32 = 0.0,
        pub stretch: f32 = 0.0,
        pub texture: Sym = Sym(0),
        pub sheet: [u32; 2] = [1, 1],
        pub additive: bool = true,
        pub seed: u32 = 1,
        pub visible: bool = true,
    }
}

component! {
    /// Inverse kinematics: makes a joint of an animated model reach for, or look at, a point. **This entity's own position** (its `Transform`) is the point, so a hand
    /// that grips a door handle, a foot that rests on the stairs or a head that follows the player is one empty entity that you move (or parent to something).
    /// `skin` is the entity with the animated model (a `MeshRenderer` of a skinned `.glb` and an `Animator`), `joint` the name of the joint (`model_info` lists them).
    /// `kind` 0 reaches: the two bones above `joint` bend, like an arm or a leg, so that `joint` (the hand, the foot) lands on the point, as far as the bones allow; `pole`
    /// is another entity the elbow or knee bends towards (without one it keeps the bend the animation has). `kind` 1 looks: `joint` (a head, a neck, a turret) turns so that its
    /// own axis `axis` faces the point, at most `max_angle` degrees away from where the animation points it. `weight` (0..1) blends the goal with the animation, `active` false
    /// switches it off. Several goals on one model are applied in uid order. Only the picture changes (the simulation never sees the bent pose); a game that needs the hand's
    /// real place computes it from its own entities. See `docs/GAMEPLAY.md`.
    pub struct Ik {
        pub skin: Uid = Uid(0),
        pub joint: Sym = Sym(0),
        pub kind: u32 = 0,
        pub weight: f32 = 1.0,
        pub pole: Uid = Uid(0),
        pub axis: [f32; 3] = [0.0, 0.0, 1.0],
        pub max_angle: f32 = 80.0,
        pub active: bool = true,
    }
}

component! {
    /// Plays music from a list, one track after another, with the next one fading in as the last fades out. `playlist` is text, one track a line: a sound file of the
    /// workspace (WAV, Ogg, MP3) or `voice:NAME` (a built-in voice), optionally followed by `volume=0.8`; `#` starts a comment. `crossfade` is how many seconds the
    /// tracks overlap (0 cuts; capped at half a track), `volume` scales everything, `shuffle` plays the list in a random order (the same for the same entity), `repeat`
    /// starts over at the end (otherwise the music stops after the last track). Set `playing` to false to fade out and pause (true starts again) and increase `skip`
    /// by one to move to the next track at once. Nothing is written back to the entity (the sound never changes the game), so the current track is not readable.
    /// The mixer buses (`AudioMixer`) route tracks by file path like any other sound. See `docs/AUDIO.md`.
    pub struct MusicPlayer {
        pub playlist: Sym = Sym(0),
        pub crossfade: f32 = 2.0,
        pub volume: f32 = 1.0,
        pub shuffle: bool = false,
        pub repeat: bool = true,
        pub playing: bool = true,
        pub skip: u32 = 0,
    }
}

component! {
    /// How a `Ui` on the same entity looks beyond a flat rectangle: `radius` rounds the corners (a fraction of the picture's height), `color2` (with an alpha above 0) turns
    /// `Ui.color` into a gradient from top to bottom, `image` (a texture name: a PNG or JPEG of the workspace or a built-in one) draws a picture over the colour, tinted by `tint`,
    /// `image_uv` (x0, y0, x1, y1 as fractions of the picture, from the top left) picks the part of it shown, and `fit` decides how it sits in the rectangle (0 stretched, 1 whole
    /// with its proportions kept, 2 filling it with the overflow cropped). `smooth` filters a scaled image. `shadow_color` (alpha above 0) draws a drop shadow shifted by `shadow_offset`
    /// (fractions of the picture's height). The border follows the rounded shape. See `docs/UI.md`.
    pub struct UiStyle {
        pub radius: f32 = 0.0,
        pub color2: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub image: Sym = Sym(0),
        pub image_uv: [f32; 4] = [0.0, 0.0, 1.0, 1.0],
        pub tint: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub fit: u32 = 0,
        pub smooth: bool = true,
        pub shadow_color: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub shadow_offset: [f32; 2] = [0.004, 0.006],
    }
}

component! {
    /// A TrueType or OpenType font (a `.ttf` or `.otf` file of the workspace) for the text of a `Text` or the label of a `Ui` on the same entity, instead of the built-in 5x8 pixel
    /// font. The size is the `size` of the `Text` (or `text_size` of the `Ui`) as a fraction of the picture's height, taken as the font's em size, and letters are drawn on whole pixels.
    ///
    /// `wrap` (a fraction of the picture's width, 0 for none) breaks the text into lines at spaces so that no line is wider, and a newline (`\n`) in the text always starts a line;
    /// `line_spacing` is the extra gap between lines as a fraction of the font's size.
    pub struct TextFont {
        pub file: Sym = Sym(0),
        pub wrap: f32 = 0.0,
        pub line_spacing: f32 = 0.25,
    }
}

component! {
    /// Pins a `Ui` or a `Text` to a point of the world, so that a health bar or a name tag follows a character: the point is `world` (plus the position of `entity`, when one is
    /// named), seen through the camera of the picture; the `Ui` is centred on it and a `Text` starts at it (its alignment still applies), moved by `offset` (fractions of the picture).
    /// Hidden while the point is behind the camera. The sim does not move a `Ui` this way, so a button pinned to the world is drawn there but only answers the mouse where its own
    /// `pos` is.
    pub struct UiAnchor {
        pub entity: Uid = Uid(0),
        pub world: [f32; 3] = [0.0, 0.0, 0.0],
        pub offset: [f32; 2] = [0.0, 0.0],
    }
}

component! {
    /// A navigation agent: set `target` (a point in the world) and `active` to true and the engine finds a walkable path around the solid
    /// colliders (up stairs, onto platforms, down short drops) and walks the entity along it, every tick, with `speed` metres per second.
    /// `active` turns itself off once the path is found. Read `status` back: 0 idle, 1 walking, 2 arrived (within `arrive` metres of the
    /// target), 3 no path or blocked; `remaining` is the path length left. A `CharacterBody` is moved by setting its `walk`, an entity with a
    /// `Velocity` by setting `lin.x` and `lin.z`, anything else is moved directly. `radius`, `height`, `step_height` and `max_drop`
    /// describe the agent (what fits between obstacles, which steps it climbs, how far it will drop); `feet` is how far the entity's
    /// `Transform` is above the ground it stands on (0 for a `CharacterBody`, whose `Transform` is its feet). The `path` tool shows a path
    /// without walking it.
    pub struct NavAgent {
        pub target: [f32; 3] = [0.0, 0.0, 0.0],
        pub active: bool = false,
        pub speed: f32 = 4.0,
        pub radius: f32 = 0.4,
        pub height: f32 = 1.8,
        pub step_height: f32 = 0.4,
        pub max_drop: f32 = 1.2,
        pub arrive: f32 = 0.3,
        pub feet: f32 = 0.0,
        pub status: u32 = 0,
        pub remaining: f32 = 0.0,
    }
}

component! {
    /// Ties this body to another point by a rope, a rod or a spring (docs/PHYSICS.md): swings, chains, hanging lamps, springy platforms.
    /// The entity with the `Joint` needs a `Collider`, a `RigidBody` and a `Velocity` (it is the body that is pulled); the other end is the
    /// entity `target` (a body, or anything with a `Transform`: a fixed point) or, when `target` is 0, the world point `anchor`.
    /// `kind` 0 is a rope (never farther than `length`, slack when closer), 1 a rod (always `length`), 2 a spring (pulls back to `length`
    /// with `stiffness` newtons per metre and `damping` newton-seconds per metre). A `length` of 0 or less is set to the distance the two
    /// ends have when the simulation first looks at it. Kinds 0 to 2 join points (the bodies' centres). With a `width` above 0 both
    /// renderers draw them as a straight cord of that thickness (metres) and `color`.
    /// `kind` 3 is a ball-and-socket and 4 a hinge, between two bodies that may rotate (give them an `AngularVelocity`) or between a body and
    /// the world: both bodies share the pivot point `anchor`, given in world coordinates **as the scene starts** (the engine turns it into a
    /// point on each body the first time it looks, so the joint then follows the bodies). A ball-and-socket lets them turn freely about it;
    /// a hinge also keeps them turning about one `axis` only (world direction at the start, default up) and, when `limit[0]` is below
    /// `limit[1]`, keeps the angle between them (radians, 0 at the start) inside those limits. The two bodies do not collide with each
    /// other. `local_a`, `local_b`, `local_axis` and `rest` are the engine's record of that first look: leave them alone (set `rest` to
    /// [0, 0, 0, 0] to make the engine bind the joint again from `anchor`).
    pub struct Joint {
        pub target: Uid = Uid(0),
        pub kind: u32 = 0,
        pub length: f32 = 0.0,
        pub stiffness: f32 = 100.0,
        pub damping: f32 = 4.0,
        pub anchor: [f32; 3] = [0.0, 0.0, 0.0],
        pub width: f32 = 0.0,
        pub color: [f32; 4] = [0.45, 0.33, 0.2, 1.0],
        pub axis: [f32; 3] = [0.0, 1.0, 0.0],
        pub limit: [f32; 2] = [0.0, 0.0],
        pub local_a: [f32; 3] = [0.0, 0.0, 0.0],
        pub local_b: [f32; 3] = [0.0, 0.0, 0.0],
        pub local_axis: [f32; 3] = [0.0, 0.0, 0.0],
        pub rest: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
    }
}

component! {
    /// Input actions (put this on one entity, the lowest uid wins): names for what the player can do, so games do not depend on which key does it.
    /// `bindings` is text, one action per line: `jump = space, gamepad.a`, `move_x = d, a:-1, gamepad.lx` (each term is an `input.*`
    /// resource, or `$resource`, times an optional scale after the colon) and `deadzone = 0.15`. Every tick, before the graph systems run, the
    /// engine writes the resources `action.NAME` (-1 to 1: terms added and clamped), `action.NAME.pressed` and `action.NAME.released` (1 on the
    /// one tick when the action went above or below one half). Change `bindings` to rebind. See `docs/GAMEPLAY.md`.
    pub struct InputMap { pub bindings: Sym = Sym(0) }
}

component! {
    /// A state machine in text (`docs/GAMEPLAY.md`): `state walk clip=Walk speed=1.2`, `idle -> walk when $action.move_x != 0 fade 0.15`,
    /// `* -> dead when HitPoints.value <= 0`, `walk -> idle after 2`, `enter dead: event died, set HitPoints.value = 0`. It drives an `Animator`
    /// (each state may name a clip: entering the state crossfades to it) and anything else you can write as actions: set a field, set or add to
    /// a resource, emit an event (a sound, a graph system's trigger). Use it for animation, for AI (patrol, chase, attack) or for a door.
    /// `state` is the current state's name, `index` its number in the order they were declared, `time` how long it has lasted;
    /// all three are the engine's to write. `enabled` false pauses the machine.
    pub struct StateMachine {
        pub machine: Sym = Sym(0),
        pub state: Sym = Sym(0),
        pub index: u32 = 0,
        pub time: f32 = 0.0,
        pub enabled: bool = true,
    }
}

component! {
    /// Moves one number over time with an easing curve: `path` is a field such as `Transform.pos[1]` or `Light.intensity` of this entity (or of
    /// `entity`), from `from` to `to` over `duration` seconds after `delay`. `ease`: 0 linear, 1 in-quad, 2 out-quad, 3 in-out-quad, 4 in-cubic,
    /// 5 out-cubic, 6 in-out-cubic, 7 in-sine, 8 out-sine, 9 in-out-sine, 10 out-back, 11 out-bounce, 12 out-elastic, 13 smoothstep. `mode`: 0 once
    /// (then `active` turns false and `done` true), 1 loop, 2 ping-pong. Set `time` to 0 and `active` true to play it again.
    pub struct Tween {
        pub path: Sym = Sym(0),
        pub entity: Uid = Uid(0),
        pub from: f32 = 0.0,
        pub to: f32 = 1.0,
        pub duration: f32 = 1.0,
        pub delay: f32 = 0.0,
        pub ease: u32 = 0,
        pub mode: u32 = 0,
        pub time: f32 = 0.0,
        pub active: bool = true,
        pub done: bool = false,
    }
}

component! {
    /// A camera that works itself: on an entity with a `Transform` (and a `Camera`), it follows `target` (`mode` 0: stays at `offset` from it, eased
    /// at `follow_speed` per second, 0 snaps; `mode` 1 orbits it at `distance` with `yaw` and `pitch` in radians, which a graph system can steer from
    /// the mouse or a stick), and with `look_at` turns to face `target` plus `look_offset`. Set `shake` (0 to 1) when something big happens: the
    /// camera jitters by `shake_amount` metres times its square and calms down at `shake_decay` per second.
    pub struct CameraRig {
        pub target: Uid = Uid(0),
        pub mode: u32 = 0,
        pub offset: [f32; 3] = [0.0, 4.0, 8.0],
        pub look_at: bool = true,
        pub look_offset: [f32; 3] = [0.0, 1.0, 0.0],
        pub follow_speed: f32 = 5.0,
        pub yaw: f32 = 0.0,
        pub pitch: f32 = 0.4,
        pub distance: f32 = 8.0,
        pub shake: f32 = 0.0,
        pub shake_decay: f32 = 1.5,
        pub shake_amount: f32 = 0.4,
    }
}

component! {
    /// Eyes (or ears): every tick the engine looks for the nearest entity that has the component named `filter` within `radius` metres and the
    /// `fov` degrees in front of this entity (its local -z; 360 sees all around), optionally with nothing solid in between (`los`, measured from
    /// `eye` metres above each end). The answer: `seen` (its uid, 0 when nobody), `distance` and `count` (how many can be seen).
    pub struct Sensor {
        pub radius: f32 = 10.0,
        pub fov: f32 = 120.0,
        pub filter: Sym = Sym(0),
        pub los: bool = false,
        pub eye: f32 = 1.0,
        pub seen: Uid = Uid(0),
        pub distance: f32 = 0.0,
        pub count: u32 = 0,
    }
}

component! {
    /// The language of the interface (put it on one entity; the lowest uid wins): texts (`Text.value`, `Ui.text`) may contain `{#key}`, which is
    /// replaced by the entry `key` of the file `lang/<language>.json` of the workspace (`{"hello": "Hello", "menu": {"play": "Play"}}`: nested
    /// names are joined with dots, `{#menu.play}`). A key that is not in the file shows as the key itself. Change `language` (`"en"`, `"de"`...)
    /// to switch at once. See `docs/GAMEPLAY.md`.
    pub struct Localization { pub language: Sym = Sym(0) }
}

component! {
    /// Removes the entity (and everything attached to it by `Parent`) after `seconds` have passed: bullets, sparks, pickups that vanish,
    /// timed effects. The time counts down by the world's `time.scale` too.
    pub struct Lifetime { pub seconds: f32 = 1.0 }
}

component! {
    /// Marks an entity made by a `PrefabSpawner` (the engine adds it), so the spawner can count how many of its own are alive.
    pub struct SpawnedBy { pub spawner: Uid = Uid(0) }
}

component! {
    /// Makes copies of the prefab named `prefab` at this entity: `count` of them every `interval` seconds, while fewer than `max_alive` of its own
    /// exist (0 = no limit) and fewer than `limit` have been made in all (0 = no limit), spread over a disc of `radius` metres around the point
    /// `offset` from here. A copy is turned like the spawner, and with `speed` above 0 it is also given that speed along the spawner's forward
    /// direction (its local -z): a turret, a fountain, an enemy gate. `spawned` and `timer` are the engine's. Pair it with `Lifetime` in the prefab.
    pub struct PrefabSpawner {
        pub prefab: Sym = Sym(0),
        pub interval: f32 = 1.0,
        pub count: u32 = 1,
        pub max_alive: u32 = 0,
        pub limit: u32 = 0,
        pub radius: f32 = 0.0,
        pub offset: [f32; 3] = [0.0, 0.0, 0.0],
        pub speed: f32 = 0.0,
        pub active: bool = true,
        pub timer: f32 = 0.0,
        pub spawned: u32 = 0,
    }
}

component! {
    /// A day and night cycle (the lowest uid wins): `time` is the hour of the day (0 to 24, 12 = noon), advancing 24 hours every `day_seconds`
    /// of simulated time (0 freezes it). Every tick the engine turns the first directional light (the sun) along the sun's path (it rises in
    /// the direction `azimuth` radians from +z at 6, is highest at 12 and sets at 18), sets its colour (warm at low sun) and brightness
    /// (`sun_intensity` by day, `night_intensity` at night), and gives the first `Environment` the sky, horizon and fog colours and the ambient
    /// light of that hour. Set `time` yourself (a graph system, a tween) to jump to a time.
    pub struct TimeOfDay {
        pub time: f32 = 12.0,
        pub day_seconds: f32 = 240.0,
        pub azimuth: f32 = 0.6,
        pub sun_intensity: f32 = 3.0,
        pub night_intensity: f32 = 0.0,
        pub enabled: bool = true,
    }
}

component! {
    /// Makes a `NavAgent` keep out of the way of other agents and characters (put it beside the `NavAgent`): within `range` times the two radii
    /// it steers away from each of them, `strength` 1 being a firm push; two agents that meet head on pass each other on the right. Without it
    /// agents walk through one another, as characters do.
    pub struct NavAvoid {
        pub strength: f32 = 1.0,
        pub range: f32 = 2.0,
    }
}

component! {
    /// A way across what cannot be walked: a jump over a gap, a ladder, a teleporter, a door the map does not know. `from` and `to` are offsets
    /// from this entity's position (the entity can be moved in the editor); both ends must stand on ground an agent can reach. `two_way` lets
    /// agents use it in both directions. An agent that reaches one end is carried to the other at `speed` metres per second along an arc `hop`
    /// metres high (0 for a straight line), and plans with it like any other part of the way (it costs its length).
    /// `enabled` false closes it (a drawbridge).
    pub struct NavLink {
        pub from: [f32; 3] = [0.0, 0.0, 0.0],
        pub to: [f32; 3] = [0.0, 0.0, 4.0],
        pub two_way: bool = true,
        pub speed: f32 = 6.0,
        pub hop: f32 = 1.0,
        pub enabled: bool = true,
    }
}

component! {
    /// More animation on top of the `Animator`'s clip, for blend trees and layers (put it beside an `Animator`). Two extra clips, `b` and `c`, are
    /// laid over the pose by `weight` (0 to 1): **blending** (a walk that becomes a run as `weight_b` goes from 0 to 1, set from a graph system or
    /// a state machine action; with `sync` the extra clip is kept in step with the base clip, so the feet agree) or a **layer** (give `root` the name
    /// of a joint and the clip moves only that joint and what hangs from it: a wave on the arm while the legs walk). `time` and `speed` run the
    /// clip when it is not synced. Looping follows the `Animator`.
    pub struct AnimLayers {
        pub clip_b: Sym = Sym(0),
        pub weight_b: f32 = 0.0,
        pub root_b: Sym = Sym(0),
        pub speed_b: f32 = 1.0,
        pub time_b: f32 = 0.0,
        pub sync_b: bool = true,
        pub clip_c: Sym = Sym(0),
        pub weight_c: f32 = 0.0,
        pub root_c: Sym = Sym(0),
        pub speed_c: f32 = 1.0,
        pub time_c: f32 = 0.0,
        pub sync_c: bool = false,
    }
}

component! {
    /// A slider on a `Ui` button (`button` true): pressing it and moving the pointer sets `value` between `min` and `max` (in steps of `step` when above 0)
    /// from where the pointer is along the rectangle, even after leaving it, until the button is released; the arrow actions (`ui_left`, `ui_right`) change it
    /// while it has focus (`UiNav`). The engine draws it as the `Ui` bar: `Ui.fill` is set to the share of the way. `changed` is true on a tick when `value` moved.
    pub struct UiSlider {
        pub value: f32 = 0.5,
        pub min: f32 = 0.0,
        pub max: f32 = 1.0,
        pub step: f32 = 0.0,
        pub dragging: bool = false,
        pub changed: bool = false,
    }
}

component! {
    /// A switch on a `Ui` button: every click flips `on` (`changed` is true for that one tick) and the rectangle takes `on_color` or `off_color`.
    pub struct UiToggle {
        pub on: bool = false,
        pub changed: bool = false,
        pub on_color: [f32; 4] = [0.2, 0.7, 0.35, 1.0],
        pub off_color: [f32; 4] = [0.3, 0.3, 0.36, 1.0],
    }
}

component! {
    /// A text field on a `Ui` button: a click gives it focus (a click elsewhere takes it away), then typed characters are added to `text` (the host puts the
    /// code point of the key into the resource `input.char` for one tick at a time; `input.backspace`, `input.enter` and `input.escape` are the other keys,
    /// the `type_text` tool does it for agents). `submitted` is true for the tick after Enter. The rectangle shows the text (or `placeholder` while it is
    /// empty and not focused, and a blinking bar while focused). `max_len` characters at most.
    pub struct UiInput {
        pub text: Sym = Sym(0),
        pub placeholder: Sym = Sym(0),
        pub focused: bool = false,
        pub max_len: u32 = 32,
        pub submitted: bool = false,
    }
}

component! {
    /// Puts the `Ui` children of this entity (those whose `Parent` is it) in rows, in columns or in a grid inside its own rectangle, in uid order: `kind` 0 a
    /// column, 1 a row, 2 a grid of `columns` columns. `padding` and `spacing` are fractions of the picture's height (the resource `screen.aspect`, width over
    /// height, keeps them equal in pixels across). The children's own `pos` and `size` are overwritten; a hidden child takes no room. Containers may be nested.
    pub struct UiLayout {
        pub kind: u32 = 0,
        pub spacing: f32 = 0.01,
        pub padding: f32 = 0.01,
        pub columns: u32 = 2,
    }
}

component! {
    /// Focus for keyboard and gamepad menus (one entity; the lowest uid wins): `current` is the focused button. The actions `ui_up`, `ui_down`, `ui_left` and
    /// `ui_right` move it to the nearest button that way (the first press focuses the first button), `ui_accept` clicks it. Bind those actions in an `InputMap`.
    /// The focused button shows as hovered.
    pub struct UiNav { pub current: Uid = Uid(0) }
}

component! {
    /// A scripted sequence in text (a cutscene, a tutorial, a trap): lines `at SECONDS ACTION`, run as `time` advances. The actions are the state machine's
    /// (`set Component.field = n`, `res name += n`, `event name`), plus `ramp Component.field FROM -> TO over SECONDS [ease N]` that moves a number smoothly (the
    /// easings of `Tween`) and `end`, which fixes the length. When the end is reached the timeline stops (`done` true) or, with `looping`, starts again.
    /// `speed` scales its clock, `playing` false pauses it; set `time` to 0 and `playing` true to play it again. See `docs/GAMEPLAY.md`.
    pub struct Timeline {
        pub script: Sym = Sym(0),
        pub time: f32 = 0.0,
        pub speed: f32 = 1.0,
        pub playing: bool = true,
        pub looping: bool = false,
        pub done: bool = false,
    }
}

component! {
    /// A path through space: `points` is text, `x y z` for each point separated by `;` or new lines, relative to this entity's position (move the entity to
    /// move the path). `smooth` draws a gentle curve through the points (a Catmull-Rom spline), otherwise straight lines; `closed` joins the last point back to
    /// the first. A `PathFollower` makes something travel along it; a graph system can also read the points. See `docs/GAMEPLAY.md`.
    pub struct Spline {
        pub points: Sym = Sym(0),
        pub smooth: bool = true,
        pub closed: bool = false,
    }
}

component! {
    /// Moves this entity along the `Spline` entity `path` at `speed` metres per second (a moving platform, a patrolling guard, a rail camera, a conveyor).
    /// `mode` 0 stops at the end (`done` becomes true), 1 goes round again, 2 goes back and forth. `align` turns it to face along the path. `offset` is
    /// added to the position (a hover height). `distance` is how far along it is (set it to jump); `direction` is the engine's.
    pub struct PathFollower {
        pub path: Uid = Uid(0),
        pub speed: f32 = 3.0,
        pub mode: u32 = 1,
        pub align: bool = true,
        pub offset: [f32; 3] = [0.0, 0.0, 0.0],
        pub distance: f32 = 0.0,
        pub direction: f32 = 1.0,
        pub done: bool = false,
    }
}

component! {
    /// Loads a piece of the world only while somebody is near it, for worlds too big to keep all at once: when `target` (an entity; 0 = the active
    /// camera) comes within `radius` metres (measured on the ground plane) of this entity, the prefab `prefab` is created at this entity's position and
    /// turning, and when it goes farther than `radius` plus `hysteresis` the whole thing is removed again (the margin keeps it from flickering at the border).
    /// Whatever happened to the loaded entities is forgotten with them: put the things that must remember outside the prefab. `loaded` is the engine's.
    pub struct StreamZone {
        pub prefab: Sym = Sym(0),
        pub radius: f32 = 100.0,
        pub hysteresis: f32 = 20.0,
        pub target: Uid = Uid(0),
        pub loaded: bool = false,
    }
}

component! {
    /// Hit points, with what most games want around them. Take damage by lowering `value` (a graph system, a state machine action, a contact: `{"set": "HitPoints.value", "to": ...}`).
    /// Every tick the engine keeps `value` between 0 and `max`, heals `regen` points per second (only while alive and `regen_delay` seconds after the last damage), notes damage
    /// in `hurt` (the time since the last hit, for flashes and screen shake) and, the first time `value` reaches 0, sets `dead`, emits the event `died` (a sound, a score, a
    /// graph system's trigger) and, with `remove_after` above 0, gives the entity a `Lifetime` of that many seconds. `invulnerable` seconds left stop damage: a hit that would
    /// lower `value` while it is above 0 is undone, so a system that takes health off and sets `invulnerable` in the same tick keeps that one hit and the next ones are refused until the grace has run out. `last` and `guard` (on while grace was still running at the end of the last tick) are the engine's.
    pub struct HitPoints {
        pub value: f32 = 100.0,
        pub max: f32 = 100.0,
        pub regen: f32 = 0.0,
        pub regen_delay: f32 = 3.0,
        pub invulnerable: f32 = 0.0,
        pub remove_after: f32 = 0.0,
        pub hurt: f32 = 100.0,
        pub dead: bool = false,
        pub last: f32 = 100.0,
        pub guard: bool = false,
    }
}

component! {
    /// A behaviour tree in text (`docs/GAMEPLAY.md`): nodes one per line, children indented two spaces under their parent. Composites: `selector` (the first child that
    /// does not fail), `sequence` (children in order until one fails), `parallel` (all every tick); leaves: `cond EXPRESSION` (as in a state machine), `act ACTION, ACTION`
    /// (`set Component.field = n`, `res name += n`, `event name`) and `wait SECONDS`; decorators with one child: `repeat N` (0 = forever), `invert`, `succeed`, `cooldown SECONDS`.
    /// The tree runs from the top every tick and remembers where a running node was. `status` is what the root said last: 0 not run yet, 1 running, 2 succeeded, 3 failed.
    pub struct BehaviorTree {
        pub script: Sym = Sym(0),
        pub enabled: bool = true,
        pub status: u32 = 0,
    }
}

component! {
    /// Picture effects of the GPU renderer in its high qualities (the first `PostFx` counts; the reference renderer ignores them): `shafts` (0 to 2) rays of
    /// light fanning out from the sun through gaps in what is in front of it (needs a directional light and a sky), `motion_blur` (0 to 1) smears what moves across
    /// the picture between one picture and the next (for live play and the editor; a picture asked for by a tool is blurred against the last one),
    /// `grain` (0 to 1) film grain that changes every picture, `contrast` (1 = unchanged) and a `tint` that multiplies the colours (warm, cold, sepia).
    pub struct PostFx {
        pub shafts: f32 = 0.0,
        pub motion_blur: f32 = 0.0,
        pub grain: f32 = 0.0,
        pub contrast: f32 = 1.0,
        pub tint: [f32; 3] = [1.0, 1.0, 1.0],
    }
}

component! {
    /// A sheet of water at this entity's position: a flat rectangle of `size` metres (x, z) at the entity's height, drawn see-through in `color` (alpha is how
    /// clear it is), reflecting the sky. In the GPU renderer's high qualities it rolls with waves `wave_height` metres tall and `wave_length` metres long that
    /// move at `speed` (the surface is lit by the sun and reflects the sky in the ripples); the reference renderer and `classic` draw it flat. `phase` is the engine's
    /// clock for the waves (it runs in the simulation, so replays show the same water). It does not push anything: floats and swimmers are up to your systems
    /// (a `Collider` trigger of the same size tells who is in it).
    pub struct Water {
        pub size: [f32; 2] = [100.0, 100.0],
        pub color: [f32; 4] = [0.04, 0.28, 0.38, 0.82],
        pub wave_height: f32 = 0.3,
        pub wave_length: f32 = 8.0,
        pub speed: f32 = 1.0,
        pub phase: f32 = 0.0,
    }
}

component! {
    /// Wind in the leaves, for the GPU renderer's high qualities (put it beside a `MeshRenderer`; the reference renderer draws it still): the model bends back and forth by
    /// `amount` metres at the height `extent` metres above the entity's position (and by the square of the height below that: the foot stays put, the top sways), `speed` times a second.
    /// Trees, grass, banners, hanging lamps. `phase` is the engine's clock (it runs in the simulation, so replays are the same); give each entity its own `offset` so a forest does not nod in step.
    pub struct Sway {
        pub amount: f32 = 0.15,
        pub extent: f32 = 3.0,
        pub speed: f32 = 1.2,
        pub offset: f32 = 0.0,
        pub phase: f32 = 0.0,
    }
}

component! {
    /// Level of detail for a `MeshRenderer` (put it beside one; both renderers use it): from `distance1` metres from the camera the entity is drawn with the model
    /// `mesh1` instead (a simpler one), from `distance2` with `mesh2`, and beyond `cull` it is not drawn at all (0 = never). An empty name keeps the model it has.
    /// Small things far away cost nothing; a crowd of detailed models costs only for those that are near.
    pub struct Lod {
        pub mesh1: Sym = Sym(0),
        pub distance1: f32 = 30.0,
        pub mesh2: Sym = Sym(0),
        pub distance2: f32 = 80.0,
        pub cull: f32 = 0.0,
    }
}

component! {
    /// The sound mixer's buses (put it on one entity; the lowest uid wins), as text: `bus music parent=master volume=0.7`, `bus voice parent=master duck=music:0.6` (while a sound on `voice`
    /// is heard the `music` bus is turned down by 60 percent, smoothly), and `route music/* -> music` (a sound goes to the first route that matches its file path, or `voice:NAME` for a built-in
    /// voice; `*` at the start or end matches anything). A settings menu sets the resources `bus.NAME.volume` (a slider) and `bus.NAME.mute`. See `docs/AUDIO.md`.
    pub struct AudioMixer { pub config: Sym = Sym(0) }
}

component! {
    /// World physics settings: put this on one entity (the lowest uid wins). Without it gravity is (0, -9.8, 0).
    pub struct PhysicsSettings { pub gravity: [f32; 3] = [0.0, -9.8, 0.0] }
}

// ------------------------------------------------------------------------------------------------ physics v2: rotation
// New components for rotating rigid bodies (docs/PHYSICS.md). Kept together at the end of the file.

component! {
    /// Makes a rigid body turn (docs/PHYSICS.md). On a body that has a `Collider`, a `RigidBody` (mass > 0) and a `Velocity`: `w` is how fast
    /// it spins in radians per second about the world x, y and z axes, and the engine turns the entity's `Transform.rot` by it every tick
    /// (contacts then make spheres roll and boxes tip, topple and tumble). `damping` slows the spin by that fraction per second (the
    /// default 0.05 is a little air). Without this component a body never turns. Leave `Velocity.ang` at 0 on such a body: that is the
    /// spin of things without physics, applied by a different system. A spinning sphere also cannot pass through a thin wall or floor
    /// however fast it goes: when it would hit one on its next step the engine slows it to arrive exactly there and keeps its real
    /// velocity in `stash` (x, y, z, and the share of the step it was slowed to) for one tick: leave `stash` alone.
    pub struct AngularVelocity { pub w: [f32; 3] = [0.0, 0.0, 0.0], pub damping: f32 = 0.05, pub stash: [f32; 4] = [0.0, 0.0, 0.0, 0.0] }
}

component! {
    /// Flipbook animation for a 2D `Sprite` (docs/GAMEPLAY.md): beside an `Animator` and a `Sprite`, the animator's clip name and clock pick `Sprite.frame`. `clips` is
    /// text, one clip a line: `run: 4-9 fps=12 step@1 step@4` (the pictures of the sheet, `fps=N`, `loop` / `once` / `pingpong`, and `NAME@K` markers that emit the event
    /// NAME when the clip reaches its K-th picture). A `StateMachine` state with `clip=run` or a graph system setting `Animator.clip` switches clips, and the new clip starts
    /// from its first picture. `done` is on once a clip that plays once has shown its last picture for its whole time. `last_clip`, `start` and `steps` are the engine's.
    pub struct SpriteAnim {
        pub clips: Sym = Sym(0),
        pub last_clip: Sym = Sym(0),
        pub start: f32 = 0.0,
        pub steps: u32 = 0,
        pub done: bool = false,
    }
}

component! {
    /// Look around with the mouse or the right stick (docs/GAMEPLAY.md): every tick the entity turns by `input.mouse_dx` and `input.mouse_dy` (pixels since the last tick,
    /// which `lunaris play` writes: right and down are positive) times `sensitivity` radians a pixel, and by the right stick (`input.gamepad.rx`, `.ry`) at `stick_speed`
    /// radians a second; `invert_y` flips up and down; `pitch` stays between `pitch_min` and `pitch_max`; `enabled` false freezes the look. On a free entity (a camera) the whole look
    /// goes into its `Transform.rot`. On an entity attached to a body (`Parent` and `LocalTransform`, the first-person camera of a character) only the pitch is the camera's own:
    /// the yaw turns the body (`body`, or the parent when 0). `forward` and `right` are the engine's: the way ahead and to the right along the ground at the current yaw, for a
    /// graph system to steer with (`CharacterBody.walk = right * move_x + forward * move_y`). `yaw` (turning left is positive) and `pitch` (looking up is positive) are in radians.
    pub struct MouseLook {
        pub sensitivity: f32 = 0.0022,
        pub stick_speed: f32 = 2.5,
        pub pitch_min: f32 = -1.5,
        pub pitch_max: f32 = 1.5,
        pub invert_y: bool = false,
        pub enabled: bool = true,
        pub body: Uid = Uid(0),
        pub yaw: f32 = 0.0,
        pub pitch: f32 = 0.0,
        pub forward: [f32; 3] = [0.0, 0.0, -1.0],
        pub right: [f32; 3] = [1.0, 0.0, 0.0],
    }
}

component! {
    /// Steering behaviours and flocking (docs/GAMEPLAY.md), beside a `Velocity`: every tick (after the graph systems) the velocity is turned by the behaviours that are on, each
    /// a weighted acceleration, together at most `max_accel` (m/s^2), and the speed is at most `max_speed`. `seek` (an entity's uid) runs at it, slowing down inside `arrive`
    /// metres; `flee` runs from it inside `flee_radius`; `wander` (a weight) drifts in a smoothly changing direction. With a `group` name the agent flocks with the other
    /// `Steer` entities of that group within `radius`: `separation` keeps `personal` metres apart, `alignment` heads the same way, `cohesion` stays together (the weights).
    /// `plane` 0 moves on the ground (x and z), 1 in the picture plane (x and y); `face` turns the entity to face its way. `bounds` (min first axis, min second axis, max first
    /// axis, max second axis; all zero = none) keeps the agent in a rectangle: inside the `edge` metres next to a side it is pushed back in, harder the nearer the side.
    /// `speed` and `heading` (the wander angle) are the engine's. `active` false switches it off.
    pub struct Steer {
        pub max_speed: f32 = 4.0,
        pub max_accel: f32 = 10.0,
        pub plane: u32 = 0,
        pub seek: Uid = Uid(0),
        pub seek_weight: f32 = 1.0,
        pub arrive: f32 = 1.5,
        pub flee: Uid = Uid(0),
        pub flee_weight: f32 = 1.0,
        pub flee_radius: f32 = 6.0,
        pub wander: f32 = 0.0,
        pub group: Sym = Sym(0),
        pub radius: f32 = 4.0,
        pub personal: f32 = 0.8,
        pub separation: f32 = 1.5,
        pub alignment: f32 = 1.0,
        pub cohesion: f32 = 1.0,
        pub face: bool = true,
        pub active: bool = true,
        pub bounds: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub edge: f32 = 2.0,
        pub speed: f32 = 0.0,
        pub heading: f32 = 0.0,
    }
}

component! {
    /// Colour grading and screen fades for the GPU renderer's high qualities (the first `ColorGrade` counts; the reference renderer ignores it): `lift` is added in the
    /// shadows (per colour channel), `gamma` bends the midtones (1 = unchanged, above 1 brightens), `gain` multiplies the highlights, `temperature` warms (up to 1) or cools
    /// (down to -1) the whole picture and `tint_shift` trades green (-1) against magenta (1). `fade` is a colour laid over the whole picture: its alpha says how much (a fade to
    /// black, the red flash of a hit, the blue of being under water), and a graph system or a `Tween` can animate it. All defaults do nothing.
    pub struct ColorGrade {
        pub lift: [f32; 3] = [0.0, 0.0, 0.0],
        pub gamma: [f32; 3] = [1.0, 1.0, 1.0],
        pub gain: [f32; 3] = [1.0, 1.0, 1.0],
        pub temperature: f32 = 0.0,
        pub tint_shift: f32 = 0.0,
        pub fade: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
    }
}

component! {
    /// Changes the scene from inside the game (docs/GAMEPLAY.md): `path` names a scene file of the workspace; when a graph system or a state machine sets `go` to true, at the end of
    /// that tick the world is replaced by that scene (a door to the next level, a menu button, a restart). Everything goes except the entities with a `Persist`; the graph systems and
    /// prefabs go too unless `keep_systems`; the resources stay (and win over the new scene's own) unless `keep_resources` is off. A scene that cannot be loaded changes nothing
    /// and sets the resource `scene.error` to 1. `scene.loaded` is the tick of the last switch.
    pub struct SceneSwitch {
        pub path: Sym = Sym(0),
        pub go: bool = false,
        pub keep_resources: bool = true,
        pub keep_systems: bool = false,
    }
}

component! {
    /// "Do not destroy on load": an entity that stays when a `SceneSwitch` replaces the scene (the player, a score board, the music). `keep` false turns it off again.
    pub struct Persist {
        pub keep: bool = true,
    }
}

component! {
    /// Height fog for the GPU renderer's high qualities (the first `HeightFog` counts; the reference renderer and `classic` ignore it): a mist that is thickest at `height` (metres)
    /// and thins out upwards, `falloff` per metre (0.1 halves the density about every 7 metres), so valleys fill with fog and hilltops stand clear, and a camera above the layer looks
    /// down into it. `density` is the fog's thickness at `height` (0.05 is a light haze, 0.5 thick); it adds to the `Environment`'s uniform `fog`. `color` is the fog's colour (display
    /// colours; it also replaces the `Environment`'s fog colour while this exists).
    pub struct HeightFog {
        pub density: f32 = 0.05,
        pub falloff: f32 = 0.15,
        pub height: f32 = 0.0,
        pub color: [f32; 3] = [0.72, 0.82, 0.95],
    }
}

// ------------------------------------------------------------------------------------------------ physics v3: mesh colliders
// New components for static level geometry made of triangles (docs/PHYSICS.md). Kept together at the end of the file.

component! {
    /// Static ground, walls and ramps from a model file (docs/PHYSICS.md): `mesh` is the path of a `.glb` or `.gltf` of the workspace (the
    /// same file `MeshRenderer.mesh` shows: put both on one entity and what you see is what you stand on). The entity's `Transform`
    /// places it: position, rotation and scale (a stretched or mirrored level works). The triangles are read once and put in a tree, so
    /// even a level of a hundred thousand triangles costs next to nothing per tick; the file is read again when it changes on disk.
    /// Balls, boxes and capsules (`Collider` + `RigidBody`) roll, slide and come to rest on it, characters and navigation agents walk
    /// on it (the triangles that face up within the walker's slope are ground, the rest are walls and ceilings), and rays and sensors
    /// hit exactly the triangles you see. A triangle has no inside: a thin floor works. It is **static**: it moves only when the
    /// entity's `Transform` is moved by something else (a platform on a `Tween`), and it never gets a velocity of its own: do not put a
    /// `RigidBody` on it (`check` warns). `friction` and `restitution` are the surface's own contact material, combined with the
    /// body's like a `Terrain`'s; `solid` false turns the collision off (the model may still be drawn). A model with more than
    /// 1,000,000 triangles is refused (make a simpler copy for collision).
    pub struct MeshCollider {
        pub mesh: Sym = Sym(0),
        pub friction: f32 = 0.6,
        pub restitution: f32 = 0.1,
        pub solid: bool = true,
    }
}

component! {
    /// The Niagara-style modules of a `Particles` cloud on the same entity (docs/GAMEPLAY.md): `modules` is text, one module a line: `color 0:1,0.9,0.5,0 0.1:1,0.7,0.2,1 1:0.2,0.1,0.1,0`
    /// and `size 0:0.1 0.3:0.5 1:0.05` (gradients over the life), `wind`, `attract`, `vortex` and `noise` (forces), `collide plane` and `collide sphere` (bounce, stick or kill),
    /// `trail` (a ribbon behind each particle) and `mesh cube` (solids instead of pictures). Only the picture sees it.
    pub struct ParticleModules {
        pub modules: Sym = Sym(0),
    }
}

component! {
    /// A collision shape that is the **convex hull** of a model (docs/PHYSICS.md): the smallest solid without dents that contains all the
    /// vertices of the `.glb` / `.gltf` named by `mesh`, stretched by the entity's `Transform.scale`. It is the shape for rocks, crates,
    /// wedges, gems and barrels. Use it **instead of** a `Collider` (the two shapes of one entity would not collide with each other). With
    /// a `RigidBody` (mass above 0), a `Velocity` and an `AngularVelocity` it is a dynamic body that tumbles, rests, stacks and rolls
    /// on boxes, balls, capsules, other hulls, terrain and mesh colliders; the mass is spread evenly over the volume of the hull (its
    /// centre of mass is the middle of the solid, which need not be the model's origin: the `Transform` is the model's origin and the
    /// engine moves the body about its centre of mass) and `friction` and `restitution` are the `RigidBody`'s. Without them it is a static solid.
    /// A hull has at most 64 corners: a model with more is simplified by dropping the corners that stick out least (`check` says how much
    /// was lost). A model that is flat (no volume) has no hull. Characters and navigation see a hull as the box around it. `trigger`
    /// reports overlaps without pushing, like a `Collider`'s.
    pub struct ConvexCollider {
        pub mesh: Sym = Sym(0),
        pub trigger: bool = false,
    }
}

// ------------------------------------------------------------------------------------------------ physics v4: collision layers
// New components for collision layers (docs/PHYSICS.md). Kept together at the end of the file.

component! {
    /// Who collides with whom (docs/PHYSICS.md): the engine's version of Unity's layer collision matrix and Unreal's collision channels. `layer` (0 to 31) is the
    /// group this entity belongs to and `mask` the groups it collides with (bit `n` set = layer `n`, so `1 << n`; the default `4294967295` is all of them). Two
    /// entities touch only if **both** agree: `a.mask` has the bit of `b.layer` AND `b.mask` has the bit of `a.layer`, so a mask that leaves a layer out is a wall of
    /// air for that layer even when the other side would have liked to collide. It decides rigid-body contacts of every shape (spheres, boxes, capsules,
    /// `ConvexCollider`, `MeshCollider`, `Terrain`), which triggers fire and which `contacts` / `on_contact` events there are, what a `CharacterBody` walks through, what a
    /// `NavAgent` plans round (and a `NavObstacle` blocks), what a `Vehicle`'s wheels stand on, and what a `Sensor` with `los` is blocked by (only the blockers whose layer
    /// is in this entity's `mask`). The `raycast` tool takes its own `mask`. An entity without this component is layer 0 with a full mask, so a scene that never uses it
    /// behaves exactly as before. A `layer` above 31 counts as 31 (`check` warns).
    pub struct CollisionLayer {
        pub layer: u32 = 0,
        pub mask: u32 = 0xFFFF_FFFF,
    }
}

// ------------------------------------------------------------------------------------------------ physics v4: raycast vehicles
// A new component for raycast vehicles (docs/PHYSICS.md; the code is `vehicle.rs`). Kept at the end of the file, after the layers.

component! {
    /// A raycast vehicle (docs/PHYSICS.md; `examples/driving.lunaris.json`): put it on the chassis, an entity that has a `Collider` (a box about the size of the body), a
    /// `RigidBody` (the mass), a `Velocity` and an `AngularVelocity`. Each wheel is a ray from its mount point down the chassis's own -y; where it meets the ground (static
    /// colliders, `ConvexCollider`s, `MeshCollider`s, `Terrain`, honouring the `CollisionLayer`s) a spring and a damper hold the chassis up and the tyre pushes along and across the
    /// ground with the engine, the brakes and its grip (never more than `grip` times the load: ask for more and it slides). The chassis's local **-z is forward**, +x is right.
    /// `wheels` is text, one wheel a line: `fl: -0.8 -0.15 -1.3 steer=1 brake=1` is the mount point x y z and then `radius`, `rest` (the suspension's length when the wheel hangs in
    /// the air, which is also its zero-force length), `travel` (how far it can be pressed in), `spring` (N/m, `auto` for the car's weight over a quarter of the travel), `damper`
    /// (N s/m, `auto` for a damping ratio of 0.6), `steer` (the share of the steering angle: 1 front, -0.3 rear steering), `drive`, `brake` and `handbrake` (shares of the engine,
    /// the brakes and the handbrake), `grip` (the tyre's friction coefficient, about 1.2) and `entity` (the uid of a model whose pose the engine sets: its place on the strut,
    /// steering and spin); a line `defaults: radius=0.35 rest=0.3` sets the values of the lines after it. At most 8 wheels. **Inputs** (set them from a graph system or an
    /// `InputMap`): `throttle` -1 to 1 (negative reverses, or brakes while the car rolls forward), `steer` -1 to 1 (right is positive), `brake` and `handbrake` 0 to 1. **Engine**:
    /// `engine_force` (N for all driven wheels at a standstill), `power` (W: above where `power / speed` falls below the force the push falls with it; 0 for no limit),
    /// `top_speed` and `reverse_speed` (m/s, the push fades out in the last tenth), `brake_force` and `handbrake_force` (N). **Steering**: `steer_max` degrees, `steer_fade`
    /// (the share of it left at `steer_fade_speed` m/s and over) and `steer_rate` (degrees a second the wheels turn at). **Chassis**: `anti_roll` (N per metre of difference in
    /// compression between the two wheels of an axle), `downforce` and `air_drag` (N per (m/s) squared), `rolling` (rolling resistance, share of the load), `roll_influence` (1: the
    /// friction pushes at the contact point, below 1 it pushes nearer the chassis's centre height, which makes the car harder to flip) and `handbrake_grip` (the share of its grip
    /// a handbraked wheel keeps). **Outputs** (the engine writes them every tick): `speed` (forward, m/s), `steering` (degrees), `on_ground` (wheels touching), `contact` (a bit
    /// for each wheel touching), and for each wheel `compression` (metres), `spin` (the angle it has rolled, radians) and `slip` (sideways speed over the ground).
    pub struct Vehicle {
        pub wheels: Sym = Sym(0),
        pub throttle: f32 = 0.0,
        pub steer: f32 = 0.0,
        pub brake: f32 = 0.0,
        pub handbrake: f32 = 0.0,
        pub engine_force: f32 = 9000.0,
        pub power: f32 = 90000.0,
        pub top_speed: f32 = 55.0,
        pub reverse_speed: f32 = 10.0,
        pub brake_force: f32 = 16000.0,
        pub handbrake_force: f32 = 12000.0,
        pub steer_max: f32 = 32.0,
        pub steer_fade: f32 = 0.4,
        pub steer_fade_speed: f32 = 40.0,
        pub steer_rate: f32 = 180.0,
        pub anti_roll: f32 = 6000.0,
        pub downforce: f32 = 0.0,
        pub air_drag: f32 = 0.4,
        pub rolling: f32 = 0.015,
        pub roll_influence: f32 = 0.5,
        pub handbrake_grip: f32 = 0.45,
        pub speed: f32 = 0.0,
        pub steering: f32 = 0.0,
        pub on_ground: u32 = 0,
        pub contact: u32 = 0,
        pub compression: [f32; 8] = [0.0; 8],
        pub spin: [f32; 8] = [0.0; 8],
        pub slip: [f32; 8] = [0.0; 8],
    }
}

// ------------------------------------------------------------------------------------------------ 2D draw order apart from z
// A new component for the drawing order of sprites, tile maps, decals and particle clouds (docs/GAMEPLAY.md, "Draw order"). Kept at the end of the file.

component! {
    /// The drawing order of a `Sprite`, `Tilemap`, `Decal` or `Particles` cloud, **apart from its position** (docs/GAMEPLAY.md, "Draw order"). A 2D thing is drawn far to near by its
    /// depth along the camera's view, which is its `Transform.pos[2]`, and `z` is also a position in 3D that solid bodies are pushed along: a hero given a bigger `z` than the guards
    /// to be drawn on top makes every contact push them apart and out of the plane. `layer` is the order and nothing else: things are drawn from the **lowest `layer` to the highest** (the
    /// highest on top, whatever their `z`), and things with the same `layer` are drawn by depth, then by entity number, as ever. An entity without this component is layer 0, so a
    /// scene that never uses it is drawn exactly as before (the picture hash of every example is unchanged), and a thing with `layer` 0 is mixed with them by depth. A `Particles`
    /// cloud without it is drawn after everything else, as ever; with it the cloud takes its place among the sprites (smoke behind the hero, sparks in front). Both renderers
    /// draw by this order. A `Ui` and a `Text` are the screen's overlay, drawn over the whole picture in their own order (`Ui.layer`), and ignore it (`check` says so). For a top-down game
    /// set `layer` to minus the entity's y each tick in a graph system so that what stands lower on the screen is in front; for a platformer give the background, the
    /// ground, the actors and the effects a layer each.
    pub struct DrawLayer {
        pub layer: f32 = 0.0,
    }
}

// ------------------------------------------------------------------------------------------------ the language from inside a game
// New component for choosing the language with a number (docs/GAMEPLAY.md, "Languages"; the code is `language.rs`). Kept at the end of the file.

component! {
    /// The languages a game offers, so that the language can be switched **from a graph system** (docs/GAMEPLAY.md, "Languages"). Put it on the entity that has the `Localization`:
    /// `languages` is the list, separated by commas and in the order of your menu (`"en, de, fr"`; names as in `Localization.language`, so `"mygame/lang/en, mygame/lang/de"` works).
    /// The resource **`language.index`** says which one is on (0 is the first): a graph system or a button sets it (`$language.index = 1;`), and every tick, after the graph
    /// systems, the engine makes `Localization.language` the name at that place (a number out of range goes to the nearest end), so the picture of that very tick is in the new
    /// language. Until a game sets the resource the language the `Localization` starts with counts, and the resource then says its place in the list. The engine also keeps
    /// **`language.count`**, the number of languages, so that `language.index = (language.index + 1) mod language.count` cycles. The resources are saved with a saved game and
    /// carried to the next scene like every other, so the choice stays. A scene without this component is not touched.
    pub struct LanguageList {
        pub languages: Sym = Sym(0),
    }
}

// ------------------------------------------------------------------------------------------------ menus and scenes: smaller conveniences
// New components that make focus and scene changes easier without touching `UiNav` or `SceneSwitch` (a new field there would change the state hash of every scene that has them). Kept at the end of the file.

component! {
    /// Put it beside the `UiNav`: a menu that **takes the focus by itself** (docs/UI.md). Without it `UiNav.current` stays empty until a direction is pressed, so `ui_accept` on a freshly shown
    /// menu does nothing and a pause menu whose buttons appear loses the focus; with it, whenever no button is focused (at the start, or because the focused one was hidden) the engine focuses the
    /// visible button with the lowest uid. `enabled` false turns it off again.
    pub struct UiNavAuto {
        pub enabled: bool = true,
    }
}

component! {
    /// Beside a `SceneSwitch`: the resources that take **the new scene's value** when the switch happens (docs/GAMEPLAY.md, "Changing scenes"). A switch carries every resource over, and they win over
    /// the new scene's own defaults, so a scene could not reset anything by declaring it (`paused`, `leave`, `time.scale`, a timer). `resources` names them, separated by spaces or commas
    /// (`"paused leave time.scale"`; a name ending in `*` is a prefix: `"fx.*"`): each one is what the new scene says it is, and absent (reading 0, or the default of an engine resource such as
    /// `time.scale`) when the new scene does not say.
    pub struct SceneReset {
        pub resources: Sym = Sym(0),
    }
}

// ------------------------------------------------------------------------------------------------ the interface on other screens
// New components so that the interface keeps its shape on a picture that is not the shape it was made for (docs/UI.md, "Other screens"). Kept at the end of the file.

component! {
    /// Keep the interface in the shape it was made for (docs/UI.md, "Other screens"; one entity, the lowest uid wins). `Ui` and `Text` positions and sizes are fractions of the picture's width and
    /// height, so on a window that is not the shape the game was designed for a square button turns into a rectangle. With a `UiScale`, `aspect` (width over height, 1.7778 for 16:9, 1.3333
    /// for 4:3) is the shape the interface was made for, and every `Ui` and `Text` is laid out inside the biggest rectangle of that shape that fits the picture, in its middle (its **safe area**), as if that
    /// were the whole picture: boxes keep their proportions, the letters keep their size, and what is left over at the sides or at the top and bottom is the picture's own (the world is still drawn
    /// across all of it). The mouse follows: `input.mouse_x` and `input.mouse_y` are fractions of the safe area (outside 0..1 over the margins), which `lunaris play` does for you. A scene without it
    /// is drawn exactly as before; a picture of the same shape is drawn exactly as without it. `UiEdge` moves single elements to the edges of the picture.
    pub struct UiScale {
        pub aspect: f32 = 1.7778,
    }
}

component! {
    /// Beside a `Ui` or a `Text` when the world has a `UiScale`: where the element sits on a picture that is wider or taller than the safe area (docs/UI.md, "Other screens"). `x` and `y` say which edge of the
    /// picture it keeps its distance from: 0 the left (top) edge, 1 the right (bottom) edge, 0.5 the middle (the default, which is no `UiEdge` at all): a health bar at the left of the safe area with `x` 0 sits the same
    /// distance from the left edge of the **picture**, however wide the window. It keeps its size. With `screen` true the element is not in the safe area at all: its `pos` and `size` are fractions of the whole picture
    /// (a backdrop, a fade, a vignette that must cover the margins too). A pinned or screen element is drawn there but its mouse rectangle stays where the simulation keeps it, so use them for bars and
    /// backdrops, not buttons.
    pub struct UiEdge {
        pub x: f32 = 0.5,
        pub y: f32 = 0.5,
        pub screen: bool = false,
    }
}

// ------------------------------------------------------------------------------------------------ UI kit v3: scroll views
// New components for scrolling lists and panels (docs/UI.md, "Scroll views"; the code is `ui_kit3.rs`). Kept at the end of the file.

component! {
    /// A scrolling container on a `Ui` (docs/UI.md, "Scroll views"): the `Ui` rectangle is the **viewport**. Everything whose `Parent` chain reaches it (a `Ui` or a `Text`, at any depth, a
    /// `UiLayout` column of rows for instance) is **clipped** to that rectangle (whole pixels, in both renderers; a line of text is cut where the viewport ends) and moves with `offset`: the
    /// children's own `pos` is where they sit at offset 0 and the engine shifts them by minus the offset, so lay the whole list out as if it were all on screen and let the engine scroll it.
    /// `offset` (x, y) is how far the content has moved up and left, in fractions of the picture like `Ui.pos`, between 0 and `content` minus the viewport's size. `content_size` is the size
    /// of the content in the same fractions; an axis left at 0 is **measured** from the visible children (the lowest and rightmost edge), and the size used is written to `content`. The
    /// mouse wheel (`input.mouse_wheel`, in notches: `lunaris play` writes it, a test sets it with the `input` tool and back to 0) scrolls the innermost scrollable view under the pointer by `speed`
    /// (a fraction of the picture's height) a notch; dragging the content with the left button scrolls it too (`drag`; a press on a button only becomes a drag once the pointer has moved
    /// a little, and a drag is never a click); the right stick (`input.gamepad.ry`) scrolls the view that holds the focused button; and when `UiNav` moves the focus to a button inside
    /// the view the view scrolls just far enough to show it. A button scrolled out of the viewport does not hover or click from there. `changed` is true for a tick in which `offset` moved. A
    /// scroll bar is a `UiScrollBar`. Do not put a `UiLayout` on the viewport itself (put one on a child that is taller than the viewport). `applied`, `dragging`, `grab`, `grab_offset`,
    /// `grab_bar`, `down` and `focus` are the engine's bookkeeping.
    pub struct UiScroll {
        pub offset: [f32; 2] = [0.0, 0.0],
        pub content_size: [f32; 2] = [0.0, 0.0],
        pub speed: f32 = 0.06,
        pub drag: bool = true,
        pub content: [f32; 2] = [0.0, 0.0],
        pub changed: bool = false,
        pub applied: [f32; 2] = [0.0, 0.0],
        pub dragging: u32 = 0,
        pub grab: [f32; 2] = [0.0, 0.0],
        pub grab_offset: [f32; 2] = [0.0, 0.0],
        pub grab_bar: f32 = 0.0,
        pub down: bool = false,
        pub focus: Uid = Uid(0),
    }
}

component! {
    /// The scroll bar of a `UiScroll` (docs/UI.md, "Scroll views"), on the `Ui` that is its **thumb**: give the thumb `button` true and style it like any `Ui` (a `UiStyle` makes it a rounded,
    /// shaded handle). `view` is the entity with the `UiScroll`; `track` is optionally a second `Ui` (also a `button`) drawn behind it, which the engine places along the whole edge. Every tick the
    /// engine sets the position, size and visibility of the thumb and the track: along the right edge of the viewport (the bottom edge with `horizontal`), `thickness` wide (a fraction of the
    /// picture's height, so the resource `screen.aspect` or a `UiScale` makes it equally thick in pixels), `inset` from the edge, the thumb as long as the share of the content that is in view
    /// (never shorter than `min_length`) and as far along as the scroll `offset`. Pressing the thumb and moving the pointer scrolls; pressing the track puts the thumb under the pointer and
    /// carries on as a drag. With `hide` the bar and its track are invisible while there is nothing to scroll. Draw them above the content with a higher `Ui.layer`. Because the engine places them,
    /// they are not scrolled or clipped even when their `Parent` is the view.
    pub struct UiScrollBar {
        pub view: Uid = Uid(0),
        pub track: Uid = Uid(0),
        pub horizontal: bool = false,
        pub thickness: f32 = 0.014,
        pub inset: f32 = 0.004,
        pub min_length: f32 = 0.05,
        pub hide: bool = true,
    }
}

// ------------------------------------------------------------------------------------------------ UI kit v3: drop-downs, tabs and groups
// New components for choosing one of several things (docs/UI.md, "Drop-downs and tabs"; the code is `ui_kit3.rs` and `ui_select.rs`). Kept at the end of the file.

component! {
    /// Shows or hides a whole piece of the interface at once (docs/UI.md, "Drop-downs and tabs"): while `shown` is false the entity it is on and **everything below it** (every entity whose `Parent` chain reaches it:
    /// panels, rows, texts, buttons) is not drawn, does not react to the pointer, is not focused by `UiNav`, takes no room in a `UiLayout` and does not count as scroll content. The `visible` flags of the entities
    /// themselves are not touched, so a row a game hides stays hidden when its panel comes back. A pause menu is one `UiGroup` that a graph system sets. Tabs and drop-downs use it for their panels and lists.
    pub struct UiGroup {
        pub shown: bool = true,
    }
}

component! {
    /// A drop-down on a `Ui` button, the **header** (docs/UI.md, "Drop-downs and tabs"): a click opens a list of options and a click on one picks it. The options are ordinary `Ui` buttons marked with a `UiOption`
    /// that names this entity; `list` is the entity that holds them (a `Ui` with a `UiLayout` column, say), which is shown while `open` and hidden otherwise (give it a `UiGroup`, or a plain `Ui` whose `visible`
    /// is switched). `selected` is the index of the picked option, written to the resource named by `resource` (`"ui.quality"`; every tick, so a graph system reads it like any resource) and set from the outside
    /// to preselect; the header's text becomes the text of that option. `changed` is true for the tick in which a pick changed `selected`. While the list is open a press **outside** the header and the list closes it
    /// and goes no further (the press does not also click what is under it, until the button is let go), the buttons under the list do not react where the list covers them, the action `ui_cancel` closes it,
    /// and so does moving the `UiNav` focus to something else; opening it from the keyboard or a pad puts the focus on the picked option. Keep the list in a higher `Ui.layer` than what it covers, and use a
    /// `TextFont` for labels that must be covered by it (the built-in bitmap font is always drawn over every rectangle). `down`, `swallow` and `focus` are the engine's bookkeeping.
    pub struct UiDropdown {
        pub selected: u32 = 0,
        pub open: bool = false,
        pub list: Uid = Uid(0),
        pub resource: Sym = Sym(0),
        pub changed: bool = false,
        pub down: bool = false,
        pub swallow: bool = false,
        pub focus: Uid = Uid(0),
    }
}

component! {
    /// One option of a `UiDropdown`, on a `Ui` button: `dropdown` is the uid of the drop-down and `index` the number it gives to `selected` and to its resource when this option is picked. The text of the
    /// option's `Ui` is what the header shows once it is picked. Options are laid out and styled like any buttons (a `UiLayout` column in the list; a `UiStyle` for rounded corners).
    pub struct UiOption {
        pub dropdown: Uid = Uid(0),
        pub index: u32 = 0,
    }
}

component! {
    /// The state of a set of tabs (docs/UI.md, "Drop-downs and tabs"), on any entity: `selected` is the index of the tab that is shown, written to the resource named by `resource` every tick (when it is not empty).
    /// The tabs are `Ui` buttons with a `UiTab` that names this entity; a click on one makes its `index` the `selected` one (`changed` is true for that tick). Set `selected` from the outside to switch tabs from a
    /// graph system.
    pub struct UiTabs {
        pub selected: u32 = 0,
        pub resource: Sym = Sym(0),
        pub changed: bool = false,
    }
}

component! {
    /// A tab, on a `Ui` button (docs/UI.md, "Drop-downs and tabs"): `tabs` is the uid of the entity with the `UiTabs`, `index` this tab's number and `panel` the entity (a `Ui` panel with children) shown while it is the
    /// selected tab and hidden otherwise: the engine sets the `shown` of the panel's `UiGroup` (a panel without one has its `Ui.visible` set instead, which hides only the panel itself). The tab's rectangle takes
    /// `on_color` while it is selected and `off_color` otherwise.
    pub struct UiTab {
        pub tabs: Uid = Uid(0),
        pub index: u32 = 0,
        pub panel: Uid = Uid(0),
        pub on_color: [f32; 4] = [0.3, 0.5, 0.9, 1.0],
        pub off_color: [f32; 4] = [0.16, 0.2, 0.32, 0.92],
    }
}

// ------------------------------------------------------------------------------------------------ UI kit v3: drag and drop
// New components for inventories and equipment (docs/UI.md, "Drag and drop"; the code is `ui_drag.rs`). Kept at the end of the file.

component! {
    /// A thing that can be picked up with the mouse, on a `Ui` button (docs/UI.md, "Drag and drop"): while the left button is held on it the rectangle follows the pointer (keeping the grip it was taken
    /// with, drawn above everything: `lift` is added to its `Ui.layer` meanwhile), and when the button is let go it goes into the `UiDrop` under the pointer, or snaps back. `item` is a number of your
    /// own (an item id) that is reported with the drop, `kind` (0 to 31) is matched against the bits of each slot's `accept`. `slot` is the `UiDrop` the thing sits in (0 for a free thing): it is centred in
    /// that slot every tick, so it follows a slot that moves or scrolls, and a thing dropped on another slot sets `slot` to it; a free thing returns to its `home` (where it was at the start). Dropped on a
    /// slot that holds another thing, the two **trade places** when the other one fits where this one came from (`UiDrop.swap`), otherwise it snaps back. A press that does not move is a click
    /// (`Ui.clicked`) as for any button, a drag is not one. Only a press that starts on the thing picks it up, and a `UiScroll` under it does not scroll while it is held. `dragging` is true while it is
    /// held; `moved`, `grab`, `layer0`, `home_set`, `home` and `down` are the engine's bookkeeping.
    pub struct UiDrag {
        pub item: u32 = 0,
        pub kind: u32 = 0,
        pub slot: Uid = Uid(0),
        pub lift: u32 = 100,
        pub dragging: bool = false,
        pub moved: bool = false,
        pub home: [f32; 2] = [0.0, 0.0],
        pub home_set: bool = false,
        pub grab: [f32; 2] = [0.0, 0.0],
        pub layer0: u32 = 0,
        pub down: bool = false,
    }
}

component! {
    /// A place a `UiDrag` can be dropped, on a `Ui` (docs/UI.md, "Drag and drop"): an inventory slot, an equipment slot, a bin. `accept` is the set of kinds it takes (bit `n` for a `UiDrag.kind` of `n`; the
    /// default takes all 32), `swap` lets a thing dropped on it trade places with the one it holds. When a thing is dropped on it the engine reports it for one tick: `dropped` is true, `from` is the
    /// dragged entity's uid, `item` its `UiDrag.item`, `count` counts the drops so far, and the resources `drop.NAME` (the dragged entity's uid; back to 0 a tick later), `drop.NAME.item` and `drop.NAME.count`
    /// say the same to a graph system (`NAME` is `name`, or this entity's uid when `name` is empty). Putting a thing back in the slot it came from is not a drop. `holds` is the uid of the thing in the
    /// slot (0 when empty), worked out from the things' `slot` every tick; `over` is true while a thing that fits is held over the slot (to light it up). The topmost slot under the pointer (by `Ui.layer`) wins.
    pub struct UiDrop {
        pub name: Sym = Sym(0),
        pub accept: u32 = 0xFFFF_FFFF,
        pub swap: bool = true,
        pub holds: Uid = Uid(0),
        pub over: bool = false,
        pub dropped: bool = false,
        pub from: Uid = Uid(0),
        pub item: u32 = 0,
        pub count: u32 = 0,
    }
}

// ------------------------------------------------------------------------------------------------ UI kit v3: tooltips and radial fills
// New components for tooltips and cooldown rings (docs/UI.md, "Tooltips" and "Radial fill"; the code is `ui_tip.rs`, and the shape is drawn by `crates/lunaris-render/src/ui.rs`). Kept at the end of the file.

component! {
    /// A tooltip for the `Ui` it is on (docs/UI.md, "Tooltips"): when the pointer rests on that rectangle for `delay` seconds (the unscaled clock, so it works while the game is paused) the **tooltip box**,
    /// the `Ui` that has the `UiTooltipBox`, appears near the pointer with this `text` (it may hold `{resource}` placeholders and `{#key}` translations like any label). It goes away when the pointer leaves
    /// the rectangle or a button is pressed there (it comes back after the pointer has left and rested again). The triggering rectangle must be visible, not hidden by a `UiGroup` and, inside a
    /// `UiScroll`, under the pointer within the viewport; the topmost one under the pointer by `Ui.layer` wins.
    pub struct UiTooltip {
        pub text: Sym = Sym(0),
        pub delay: f32 = 0.5,
    }
}

component! {
    /// The box a `UiTooltip` shows its text in, on a `Ui` of your own style (a rounded dark rectangle with a `TextFont`; give it a high `Ui.layer`; docs/UI.md, "Tooltips"). One per scene (the lowest uid is
    /// used). The engine keeps it hidden (`Ui.visible` false) until a tooltip is due, then sets its `text`, its position (`offset` from the pointer, flipped to the other side of the pointer where it
    /// would run off the picture, never off the top or left) and, with `auto_size`, its size from the length of the text (an estimate: 0.75 of the text size a letter, `padding` all round, a fraction of the
    /// picture's height; switch `auto_size` off and give the box a `size` when the text changes a lot). `source` is the trigger it is shown for, `shown` whether it is up. `timer`, `anchor`, `suppress` and
    /// `down` are the engine's bookkeeping.
    pub struct UiTooltipBox {
        pub offset: [f32; 2] = [0.012, 0.025],
        pub padding: f32 = 0.012,
        pub auto_size: bool = true,
        pub source: Uid = Uid(0),
        pub shown: bool = false,
        pub timer: f32 = 0.0,
        pub anchor: [f32; 2] = [0.0, 0.0],
        pub suppress: bool = false,
        pub down: bool = false,
    }
}

component! {
    /// A radial fill for a `Ui` (docs/UI.md, "Radial fill"): instead of the rectangle's bar, the `Ui` draws a **sector** that sweeps `value` (0 to 1) of the way round: a cooldown wipe over an icon, a progress
    /// ring, a clock. The sector starts `start` degrees clockwise from twelve o'clock and runs clockwise (or counter-clockwise with `clockwise` false) for `value` of a full turn, in `Ui.color`
    /// (the display colour; a button's hover and press shading applies); `Ui.back_color`, when its alpha is above 0, is the full shape behind it. `shape` 0 is the circle (or ring) inscribed in the rectangle,
    /// `shape` 1 the whole rectangle (the classic clock wipe over a square icon); `inner` (0 to 1) cuts a hole of that share of the circle's radius out of shape 0, which makes a ring. The shape ignores the
    /// rectangle's border, shadow, gradient and picture; the label (`Ui.text`) is drawn as ever. Both renderers draw it from the same description: the reference renderer by testing each pixel (the circle's
    /// edge is anti-aliased, the straight edges of the sector are not), the GPU renderer as triangles. Drive `value` from a graph system (`{"set": "UiRadial.value", "to": {"res": "cooldown"}}`).
    pub struct UiRadial {
        pub value: f32 = 1.0,
        pub start: f32 = 0.0,
        pub clockwise: bool = true,
        pub shape: u32 = 0,
        pub inner: f32 = 0.0,
    }
}

// ------------------------------------------------------------------------------------------------ terrain layers
// A new component for painted terrain materials (docs/TERRAIN.md, "Layers"; the splat map is read and drawn by `crates/lunaris-render/src/terrain_layers.rs` and the GPU renderer). Kept at the end of the file.

component! {
    /// Painted materials for the `Terrain` on the same entity (docs/TERRAIN.md, "Layers"; Unity's terrain layers, Unreal's landscape layers): up to four materials blended by a **splat map**, in
    /// place of the terrain's one `texture`. `splat` is an RGBA PNG of the workspace that covers the whole ground like the heightmap does (row 0 is the north edge, the left column the west edge, samples on the
    /// corners of the cells; any size): the weight of layer 1 is its red channel, layer 2 the green, layer 3 the blue and layer 4 the alpha, normalised where they do not add up to 1 (a texel with no
    /// weight at all is the first layer). `layers` is text, one layer a line: `NAME [TEXTURE] [uv_scale=N] [bump=D] [tint=R,G,B]`: the name (letters, digits, `_`, `-`; it is what `terrain_layer` and
    /// `scatter` call the layer), the texture (a built-in material such as `grass`, `dirt`, `stone`, `sand`, or a PNG or JPEG of the workspace; the name itself when left out), how many times the
    /// texture repeats across the whole ground (`uv_scale=48`, or `uv_scale=48,32`; the `Terrain`'s own `uv_scale` when left out), the depth in metres of the relief of a built-in material's bumps in the GPU
    /// renderer's high qualities (`bump=0.03`, 0 for flat), and a colour the texture is multiplied by (`tint=1.2,1.2,1.3`, each 0 to 4). `Terrain.color` multiplies the blend as it multiplies a texture
    /// (set it to `[1, 1, 1, 1]` to see the layers as they are); `Terrain.texture` and its alpha are not used while this is here. Both renderers draw the blend; a mistake in the text
    /// (see `check`) leaves the terrain drawn with its own texture.
    pub struct TerrainLayers {
        pub splat: Sym = Sym(0),
        pub layers: Sym = Sym(0),
    }
}

// ------------------------------------------------------------------------------------------------ water bodies
// New components for water that follows a heightmap (docs/TERRAIN.md, "Water bodies"; the geometry and what the engine does with it is `water.rs`, the picture is drawn by both renderers). Kept at the end of the file.

component! {
    /// Water that follows a heightmap (docs/TERRAIN.md, "Water bodies"): rivers that step down a valley and lakes at their own levels, from one picture, where the flat `Water` sheet is one level. `heightmap` is a
    /// 16-bit greyscale PNG of the workspace (Tellus writes it as `water.png`) in which **0 is dry land** and any other value is the height of the water's surface: `value / 65535 * relief` metres above
    /// the entity's `Transform.y` (0 for Tellus's, whose heights are absolute). It covers `size` metres (x, z) centred on the entity, laid on the ground like a `Terrain`'s heightmap (row 0 north, samples on the
    /// corners of the cells, `step` uses every step-th sample). Both renderers draw the surface (in the GPU renderer's high qualities see-through in `color`, rolling with waves `wave_height` metres tall
    /// and `wave_length` long that move at `speed`, lit by the sun and reflecting the sky; the reference renderer and `classic` draw it flat and solid); the water is only where the picture says, so it hides
    /// under a bank that stands higher. `phase` is the engine's clock for the waves. The engine also uses it: characters that stand in it get an `InWater`, `Buoyant` bodies float on it, and the
    /// `water_depth` tool measures it. It is not solid and rays pass through it.
    pub struct WaterBody {
        pub heightmap: Sym = Sym(0),
        pub size: [f32; 2] = [256.0, 256.0],
        pub relief: f32 = 64.0,
        pub step: u32 = 1,
        pub color: [f32; 4] = [0.04, 0.28, 0.38, 0.82],
        pub wave_height: f32 = 0.15,
        pub wave_length: f32 = 6.0,
        pub speed: f32 = 1.0,
        pub phase: f32 = 0.0,
        pub visible: bool = true,
    }
}

component! {
    /// Written by the engine on every entity with a `CharacterBody` whose feet are under the surface of a `WaterBody`, and taken off again when it climbs out (docs/TERRAIN.md, "Water bodies"): its mere
    /// presence is the answer to "is it in water?", so a graph system that `each`es over `CharacterBody` and `InWater` runs only for swimmers and waders (slow them, change the animation, play a
    /// splash). `depth` is how far the surface is above the character's feet in metres (compare it with `CharacterBody.height`: 0.5 is a wade, 1.5 a swim), `surface` the height of the surface in
    /// world y and `body` the `WaterBody` entity. Leave the fields alone: they are rewritten every tick. A scene with no `WaterBody` never gets one.
    pub struct InWater {
        pub depth: f32 = 0.0,
        pub surface: f32 = 0.0,
        pub body: Uid = Uid(0),
    }
}

component! {
    /// Makes a rigid body float in a `WaterBody` (docs/TERRAIN.md, "Water bodies"; beside a `Collider`, a `RigidBody` and a `Velocity`: a ball, a box, a capsule). The water pushes the body up by gravity
    /// times how many times heavier the water is than the body (1000 / `density`, the density in kilograms per cubic metre: 500 is wood, 920 ice, 1000 floats awash, above 1000 it sinks slowly) times the
    /// share of the body that is under the surface (a ball's by its cap, a box's by its height), and slows it by `drag` per second in proportion to that share (and its spin too). A body
    /// of density 500 sinks until half of it is under. `submerged` (0 to 1) is the share the engine found this tick: read it for splash sounds and foam. Waves are not part of the simulation, so the surface
    /// is the flat one of the picture.
    pub struct Buoyant {
        pub density: f32 = 500.0,
        pub drag: f32 = 2.0,
        pub submerged: f32 = 0.0,
    }
}

// ------------------------------------------------------------------------------------------------ lighting effects of the GPU renderer
// Eye adaptation, screen-space reflections, lens flare and reflection probes (docs/GPU.md). Each is its own component beside the `Environment`, never a field of an
// existing one, and only the GPU renderer's high qualities draw them (the reference renderer and `classic` ignore them). Kept together at the end of the file.

component! {
    /// Eye adaptation (auto exposure) for the GPU renderer's high qualities (the first `AutoExposure` counts; the reference renderer and `classic` ignore it): the renderer measures how bright
    /// the picture is on average (the geometric mean of the luminance, the middle of the picture counting a little more) and brings it to `target` by multiplying the light by an exposure,
    /// which it multiplies with `Environment.exposure` (that one stays the compensation you choose: 2 = always a stop brighter). The exposure never goes below `min` or above `max`
    /// (multipliers: 0.05 to 16 by default, a cave next to open sunlight is about 30 times apart). `speed` is how fast the eye adapts: each second it covers `1 - e^-speed` of the remaining way in
    /// stops (1.5 takes about a second to settle); in `lunaris play` the pictures follow each other and the exposure glides, while one picture asked for by a tool is measured and
    /// applied at once (the same world always draws the same picture). The flat background colour is not exposed, bloom and light shafts are.
    pub struct AutoExposure {
        pub min: f32 = 0.05,
        pub max: f32 = 16.0,
        pub speed: f32 = 1.5,
        pub target: f32 = 0.18,
    }
}

component! {
    /// Screen-space reflections for the GPU renderer's high and ultra qualities (the first `Ssr` counts; the reference renderer and `classic` ignore it): smooth and metallic surfaces, and `Water`,
    /// mirror what is on the screen instead of only the sky. A ray from every glossy pixel is marched through the depth of the picture; where it meets something the surface shows that
    /// something's colour, where it leaves the screen, finds nothing or is too far the sky's reflection that was always there stays. `strength` (0 to 2, 1 = a full mirror image where the surface
    /// would reflect) scales it, `max_distance` is how far in metres a ray may travel, `thickness` how thick (in metres of depth) things are taken to be, so that a ray passes behind a thin
    /// object instead of hitting it. Surfaces rougher than about 0.6 do not use it (the blurred sky is right for them). Only what is in the picture can be reflected: not what is behind the
    /// camera, not what a nearer object hides, not sprites or particles.
    pub struct Ssr {
        pub strength: f32 = 1.0,
        pub max_distance: f32 = 40.0,
        pub thickness: f32 = 0.5,
    }
}

component! {
    /// Lens flare for the GPU renderer's high and ultra qualities (the first `LensFlare` counts; the reference renderer and `classic` ignore it): while the sun (the first directional light) is in
    /// front of the camera and in view, a glare around it, `ghosts` (0 to 8) soft coloured discs strung out along the line from the sun through the middle of the picture and a `halo` ring around
    /// the middle appear, as in a camera lens. `intensity` (0 to 4, 1 = a sunny day) scales all of it. The depth of the picture is looked at where the sun's disc is: the flare fades out as
    /// what is drawn (a cube, a hill, a wall) covers the sun, and as the sun leaves the picture. It is added to the light before exposure and tone mapping, so it glows with the rest.
    /// Only opaque meshes hide the sun (not sprites, particles or see-through meshes).
    pub struct LensFlare {
        pub intensity: f32 = 1.0,
        pub ghosts: u32 = 5,
        pub halo: f32 = 0.5,
    }
}

component! {
    /// A reflection probe for the GPU renderer's high qualities (the reference renderer and `classic` ignore it): at its entity's position it captures the world around it into a cube of pictures
    /// (six views, at `resolution` pixels a side, 16 to 256; all the probes of a world share the biggest one asked for) the first time it is drawn, and the surfaces inside its box
    /// use that instead of the plain sky for what they reflect and for the ambient light they get: a metal ball in a red room shows the red room, a cave's walls are lit by the cave and not by the sky
    /// outside. `size` is the box of the probe's influence in metres (x, y, z, centred on the entity; the box is not turned by the entity's rotation): inside it, it takes over from the sky
    /// (fading out over the outer third of the way to its walls), a reflection is bent to come from where the walls are (box projection, so a floor's reflection of a wall is in the right place),
    /// and where boxes overlap the smaller wins. It is **baked**: it shows the world as it was when it was captured. It is captured again when its position, `size` or `resolution` change, and on demand
    /// with the `bake_probe` tool (after you moved things, or loaded another scene). At most four probes work at once (the four with the lowest uids). A probe does not see other probes.
    pub struct ReflectionProbe {
        pub size: [f32; 3] = [20.0, 10.0, 20.0],
        pub resolution: u32 = 128,
    }
}

component! {
    /// Keeps a third-person camera out of the walls (put it beside a `CameraRig`): every tick, after the rig has placed the camera, a line is looked along from the point the camera looks at (the rig's
    /// target plus its `look_offset`) towards the camera; when something solid (a `Collider` that is not a trigger, terrain, the triangles of a `MeshCollider`, a convex hull) is in the way, the camera is
    /// pulled in to `radius` metres in front of it, but never nearer to the target than `min_distance`. Without this a camera orbiting a character goes through trees, ruins and the side of a hill. The rig eases
    /// the camera out again when the way is free (`CameraRig.follow_speed`), so a camera that is pulled in does not snap back.
    pub struct CameraCollide {
        pub radius: f32 = 0.35,
        pub min_distance: f32 = 0.9,
    }
}

// ------------------------------------------------------------------------------------------------ character motion
// Swimming, rolling, ladders and ledges for `CharacterBody` (docs/GAMEPLAY.md, "Character motion"; the work is `motion.rs`, called from `character.rs`). New components, never fields of `CharacterBody`: a world
// without them is bit for bit what it was. Kept at the end of the file.

component! {
    /// More ways to move for a `CharacterBody` (put it beside one; docs/GAMEPLAY.md, "Character motion"). **Water**: in a `WaterBody` the character wades while the surface is less than `swim_depth` metres above its
    /// feet (its `walk` is slowed down to `wade_speed` times at that depth, in proportion on the way) and swims from there on: no gravity, the feet rest `float_depth` metres under the surface (the shoulders at it),
    /// the horizontal speed is at most `swim_speed`, and `vertical` (-1 to 1, set by a graph system) dives (negative) or rises (positive) at `dive_speed`; it cannot rise above its floating height. **Roll**: set `roll`
    /// (a request, cleared every tick) on the ground and the character rolls for `roll_time` seconds along `walk` (or, with no `walk`, along `roll_dir`), at `roll_speed` metres per second slowing down to a quarter, deaf
    /// to `walk` and `jump` until it is over; then `roll_cooldown` seconds must pass before the next one. The engine says `rolling` and `roll_left`: a game makes the roll untouchable by reading them (the engine
    /// does not touch `HitPoints`). **Ladders** (a `Climbable`): a character that walks into one, or pushes `vertical` while at it, grabs it and climbs with `vertical` at `climb_speed` times the ladder's `speed`;
    /// `jump` lets go, going down to the ground lets go, and at the top it steps onto whatever is behind the ladder. **Ledges**: `jump` against a ledge that is higher than `step_height` and at most `mantle_height`
    /// above the feet pulls the character up onto it in `mantle_time` seconds (and a swimmer pushing against a bank does the same, so a lake can be left over a dock or a rock). Everything the engine writes
    /// (`wading`, `swimming`, `under`, `depth`, `rolling`, `roll_left`, `climbing`, `ladder`, `mantling`, and the rest) is rewritten every tick: read it, leave it alone. **Contacts**: with `report_contacts` (on by default) the solid things the character touches (a wall, the ground, a crate, a terrain) are reported like physics contacts (from the character to the thing, so `on_contact` sees them), one tick late. All of it is deterministic.
    pub struct CharacterMotion {
        pub swim_depth: f32 = 1.1,
        pub float_depth: f32 = 1.35,
        pub swim_speed: f32 = 2.4,
        pub dive_speed: f32 = 2.0,
        pub wade_speed: f32 = 0.7,
        pub roll_speed: f32 = 7.0,
        pub roll_time: f32 = 0.6,
        pub roll_cooldown: f32 = 0.25,
        pub climb_speed: f32 = 2.0,
        pub mantle_height: f32 = 1.6,
        pub mantle_time: f32 = 0.55,
        pub vertical: f32 = 0.0,
        pub roll: bool = false,
        pub roll_dir: [f32; 2] = [0.0, 0.0],
        pub depth: f32 = 0.0,
        pub wading: bool = false,
        pub swimming: bool = false,
        pub under: bool = false,
        pub rolling: bool = false,
        pub roll_left: f32 = 0.0,
        pub roll_wait: f32 = 0.0,
        pub climbing: bool = false,
        pub ladder: Uid = Uid(0),
        pub climb_lock: f32 = 0.0,
        pub mantling: bool = false,
        pub mantle_left: f32 = 0.0,
        pub mantle_from: [f32; 3] = [0.0, 0.0, 0.0],
        pub mantle_to: [f32; 3] = [0.0, 0.0, 0.0],
        pub report_contacts: bool = true,
    }
}

component! {
    /// A ladder, a rope or a climbable wall for a `CharacterBody` that has a `CharacterMotion` (docs/GAMEPLAY.md, "Character motion"). The entity's `Transform.pos` is the **foot** of the ladder (its bottom,
    /// in the middle) and its `Transform.rot` turns it: the side a climber stands on is the entity's local +z. The ladder is `width` metres wide and `height` metres tall; a character within `reach` metres in front of
    /// it (and over the width and the height) can grab it. `speed` multiplies the climber's `climb_speed`. It has no shape of its own: put a model and, if it should be solid, a `Collider` beside it.
    pub struct Climbable {
        pub height: f32 = 3.0,
        pub width: f32 = 0.8,
        pub reach: f32 = 0.7,
        pub speed: f32 = 1.0,
    }
}

component! {
    /// Eyes for graph systems and scripts (docs/GAMEPLAY.md, "Probe"): a system cannot cast a ray or ask the terrain, but it can read this. Every tick, after the physics step and before the graph systems, the engine
    /// looks **down** from `up` metres above the entity's `Transform` (at most `depth` metres) and writes `over_ground`, the height of the first solid it meets in `ground` (terrain, a mesh level, a box, anything the entity
    /// collides with by `CollisionLayer`; bodies that move are not ground), its unit `ground_normal` (so `ground_normal.y` is how flat it is) and its entity in `ground_uid`; and `wet` with the height of the water's
    /// `surface` when the entity's x and z are over a `WaterBody`. With `ray_max` above 0 it also casts a ray from `ray_from` (metres from the entity, turned by its rotation when `ray_local` is set) along `ray_dir`
    /// (a direction, any length) and writes `hit`, `hit_dist`, `hit_point`, `hit_normal` and `hit_uid`: the same answers `raycast` gives, but where a system can use them (put a pick-up on the ground, stop a
    /// projectile at a wall, see whether a ledge is ahead). It sees what a wheel sees: static solids, not characters or moving bodies (use a `Sensor` for those), and not the entity itself. Cheap: a world with no
    /// `Probe` pays nothing.
    pub struct Probe {
        pub up: f32 = 2.0,
        pub depth: f32 = 100.0,
        pub ray_from: [f32; 3] = [0.0, 1.0, 0.0],
        pub ray_dir: [f32; 3] = [0.0, 0.0, -1.0],
        pub ray_max: f32 = 0.0,
        pub ray_local: bool = false,
        pub over_ground: bool = false,
        pub ground: f32 = 0.0,
        pub ground_normal: [f32; 3] = [0.0, 1.0, 0.0],
        pub ground_uid: Uid = Uid(0),
        pub wet: bool = false,
        pub surface: f32 = 0.0,
        pub hit: bool = false,
        pub hit_dist: f32 = 0.0,
        pub hit_point: [f32; 3] = [0.0, 0.0, 0.0],
        pub hit_normal: [f32; 3] = [0.0, 1.0, 0.0],
        pub hit_uid: Uid = Uid(0),
    }
}

component! {
    /// Fixes an entity with a `MeshRenderer` to a **joint of an animated model**, so that a sword stays in a hand, a shield on an arm, a torch in a fist and a helmet on a head while the skeleton moves (docs/GAMEPLAY.md, "Bone
    /// attachments"). `target` is the entity of the skinned model (a `MeshRenderer` of a rigged `.glb` with an `Animator`), `joint` the name of the joint in that model (`model_info` lists them), `pos`, `rot` (a quaternion
    /// x y z w) and `scale` place the thing relative to the joint. Both renderers draw it at the joint's place in the pose of the picture, after the model itself: this is **presentation only**, like `Ik` (the entity's own
    /// `Transform` is left alone and stays what the simulation says; a game that needs the hand's place in the simulation uses a `Sensor` or the `Transform` of the character). If the target is not drawn animated (or has no
    /// such joint) the thing is drawn at its own `Transform`, and the picture says why.
    pub struct BoneAttach {
        pub target: Uid = Uid(0),
        pub joint: Sym = Sym(0),
        pub pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub rot: [f32; 4] = [0.0, 0.0, 0.0, 1.0],
        pub scale: [f32; 3] = [1.0, 1.0, 1.0],
    }
}

component! {
    /// Surface detail from pictures for a `MeshRenderer` (put it beside one; the GPU renderer's high-quality mode draws it, the reference picture and the classic mode ignore it). `normal` is a tangent-space normal map
    /// (a PNG or JPEG of the workspace, DirectX convention: green points down the picture, which is what Poly Haven's `nor_dx` and Unreal's maps are) and `arm` a map with ambient occlusion in red, roughness in green and
    /// metallic in blue (the usual packed form). Both hold numbers, not colours: they are read as they are, never turned from sRGB. They repeat `MeshRenderer.uv_scale` times like the colour texture. `normal_strength`
    /// scales the bumps (1 as authored, 0 flat), `ao_strength` how much the occlusion map darkens the ambient light (0 to 1), `roughness_scale` multiplies the map's roughness. A name left empty is not used: the mesh's own
    /// roughness and metallic count and the surface's own normal stays. `height` is a height map (white = high; Poly Haven's `disp`): the surface is given real depth by parallax occlusion mapping, a cheap ray march through the
    /// picture that shifts what you see by how high it is, so a carpet, a tile floor or a brick wall has relief that changes as you move. `height_scale` is how deep it goes, as a share of one repeat of the picture (0.03 is a
    /// good start). New in the engine's standard set; a world without it is what it was.
    pub struct PbrMaps {
        pub normal: Sym = Sym(0),
        pub arm: Sym = Sym(0),
        pub height: Sym = Sym(0),
        pub height_scale: f32 = 0.03,
        pub normal_strength: f32 = 1.0,
        pub ao_strength: f32 = 1.0,
        pub roughness_scale: f32 = 1.0,
    }
}

component! {
    /// A camera lens for the GPU renderer's high qualities (the first `Lens` counts; the reference picture and classic mode ignore it): what makes a picture look like it came out of a body camera or a phone. `distortion` bends the
    /// picture round its middle (above 0 barrel, a fisheye, below 0 pincushion), `zoom` is how much of the picture the lens frames (1 as it is; below 1 crops in, so a strong fisheye has no black corners), `chromatic` is the colour
    /// fringe that grows towards the edges (0.02 is a cheap lens), `soften` blurs like a small sensor (0 to 4 pixels), `block` is the size of the blocks of a compressed picture in pixels (1 = off, 2 or 3 = a low bit rate),
    /// `scan` (0 to 1) darkens every other line. A new component; a world without it is what it was.
    pub struct Lens {
        pub distortion: f32 = 0.0,
        pub zoom: f32 = 1.0,
        pub chromatic: f32 = 0.0,
        pub soften: f32 = 0.0,
        pub block: f32 = 1.0,
        pub scan: f32 = 0.0,
    }
}

component! {
    /// Grass the GPU draws by itself (the GPU renderer's high qualities; the reference picture and classic mode do not draw it, and a world without it is what it was). Nothing of it is an entity: round the camera, up to
    /// `radius` metres, the renderer lays tufts of the model `mesh` (a few crossed cards of grass, a `.glb` with its picture as a material, standing on y = 0 and about 0.6 m tall) on the ground of the `Terrain` that
    /// the entity `terrain` is (0 = the first one) at about `density` tufts per square metre near the camera, thinner and bigger further out in three rings that fade into each other, each tuft turned and sized by where it
    /// stands (`size` is the smallest and the biggest scale), tinted between `color_a` and `color_b` (multipliers of the picture) and darker at its foot. The wind is a gust travelling across the field:
    /// `wind_amount` metres of lean at the top, `wind_wavelength` metres between the crests, `wind_speed` crests a second, going the way `wind_dir` says (radians about up, 0 = +x). `phase` is the engine's clock
    /// (it runs in the simulation, so replays are the same). The field is patchy: over stretches of about `patch_size` metres it turns from lush to dry (towards `color_b`), the way the grass of a real plain does, and the sun shining
    /// through the blades lights them up when you look towards it. `texture` replaces the model's picture. One field costs a few hundred thousand tufts' vertices a picture and no entities, no scene file, no CPU.
    pub struct GrassField {
        pub terrain: Uid = Uid(0),
        pub mesh: Sym = Sym(0),
        pub texture: Sym = Sym(0),
        pub radius: f32 = 150.0,
        pub density: f32 = 8.0,
        pub size: [f32; 2] = [0.7, 1.3],
        pub wind_amount: f32 = 0.2,
        pub wind_wavelength: f32 = 26.0,
        pub wind_speed: f32 = 0.8,
        pub wind_dir: f32 = 0.5,
        pub patch_size: f32 = 180.0,
        pub color_a: [f32; 3] = [0.85, 1.0, 0.7],
        pub color_b: [f32; 3] = [1.15, 1.05, 0.85],
        pub seed: f32 = 1.0,
        pub phase: f32 = 0.0,
        pub enabled: bool = true,
    }
}

component! {
    /// Light scattered by the air, for the GPU renderer's high qualities (the first `Volumetrics` counts; the reference picture and classic mode ignore it): every point and spot light (a flashlight, a fluorescent tube) makes a visible glow and
    /// beam in dusty or humid air, the way it does on a body camera in a dark place. From the camera to the first thing it sees (at most `max_distance` metres, in `steps` steps) the picture adds up the light of every lamp that reaches the
    /// air there: `density` is how thick the air is per metre (0.01 a faint haze, 0.05 a fog), `anisotropy` (0 to 0.95) how much it throws the light on towards the viewer (0.6 makes lamps glow when you look at them),
    /// `intensity` scales the whole, `color` tints it. The sun is not in it (`PostFx.shafts` is its own). The light is not shadowed: it stops at the first surface seen, but a lamp behind a thin wall still glows in the air next to it.
    pub struct Volumetrics {
        pub density: f32 = 0.02,
        pub anisotropy: f32 = 0.6,
        pub intensity: f32 = 1.0,
        pub color: [f32; 3] = [1.0, 1.0, 1.0],
        pub steps: f32 = 24.0,
        pub max_distance: f32 = 120.0,
    }
}

component! {
    /// Light that bounces (the GPU renderer's high qualities; the first `GlobalIllumination` counts; the reference picture and classic mode ignore it): a lit wall lights the floor in its own colour, a bright lamp lights the room
    /// it hangs in, a shaded corner is not black. It is worked out on the screen from what the last picture showed: for every pixel, `directions` rays (cosine-weighted about the surface) are marched through the picture's depth
    /// for up to `radius` metres, and where one meets a surface the light that surface sent out last time comes back along it. Every picture's bounce is in the next one's, so the second bounce comes by itself and the
    /// light settles in a few pictures (a still picture made by `render` is drawn three times to let it settle). `intensity` scales the bounce (1 = as bright as the surfaces it comes from, which are never brighter than 8 times
    /// white), `thickness` is how far behind a surface's face a ray still counts as touching it. It only knows what is on the screen: a lit wall behind you does not light the floor in front of you, and what the picture
    /// does not show is lit by the sky and the probes as before.
    pub struct GlobalIllumination {
        pub intensity: f32 = 1.0,
        pub radius: f32 = 6.0,
        pub directions: f32 = 10.0,
        pub thickness: f32 = 0.6,
        pub enabled: bool = true,
    }
}

component! {
    /// How the sun's shadows are drawn (the GPU renderer's high qualities; the first `Shadows` counts; the reference picture and classic mode ignore it): `softness` is the sun's apparent size as a share of the distance
    /// (tan of its angular diameter: 0.0093 is the real sun, 0.03 a look that shows well), and it is what makes a shadow sharp where it starts and softer the further it falls from what casts it (contact-hardening
    /// shadows: a search for the casters finds how far away they are, and the filter is as wide as the penumbra there). 0 keeps the plain filter. A new component; a world without it keeps the plain filter.
    pub struct Shadows {
        pub softness: f32 = 0.03,
        pub enabled: bool = true,
    }
}

component! {
    /// A sky made by the physics of the air (the GPU renderer's high qualities; the first enabled `Atmosphere` counts, the reference picture and classic mode keep the `Environment`'s colours): the air is a layer of
    /// molecules that scatters blue light (Rayleigh) and a layer of haze and dust (Mie), and the sun's light is scattered towards the eye along every line of sight (Nishita's single scattering, worked out for every pixel
    /// of the sky). The sky is blue at noon with a pale horizon, the sun reddens and dims as it gets low, the sky glows round it, and after sunset it goes dark: all from the direction of the first directional light, which
    /// is the sun. The ambient light (the zenith and horizon colours), the fog colour (a haze towards the sun is brighter) and the colour of the sun's light follow the same numbers; the sun's colour and intensity on its `Light`
    /// are what it is at noon. `turbidity` is how hazy the air is (1 very clear, 2 clear, 6 hazy: haze whitens the sky and brightens it round the sun), `rayleigh` scales the density of the molecules (more makes the
    /// sky bluer and the sunset redder), `mie_g` is how far forward the haze throws the light (0.76), `intensity` how bright the sky is, `haze_glow` how much brighter fog is when looking at the sun. A new component.
    pub struct Atmosphere {
        pub turbidity: f32 = 2.0,
        pub rayleigh: f32 = 1.0,
        pub mie_g: f32 = 0.76,
        pub intensity: f32 = 1.0,
        pub haze_glow: f32 = 1.5,
        pub enabled: bool = true,
    }
}
