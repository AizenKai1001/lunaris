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
    /// `AcousticMaterial`). `volume` is 0..8 and `pitch` multiplies the playback speed. `file` may name several files (one on each line, or
    /// separated by `|`): each time the event comes, one of them is played, never the same twice in a row, so a footstep or a gunshot does
    /// not repeat itself. `pitch_var` and `volume_var` (0 to 1) make every play a little different: the pitch is multiplied by a random number
    /// in `1 +- pitch_var` and the volume by one in `1 +- volume_var`. (What is played is output: it is never part of the state.)
    pub struct SoundFile {
        pub name: Sym = Sym(0),
        pub file: Sym = Sym(0),
        pub volume: f32 = 1.0,
        pub pitch: f32 = 1.0,
        pub spatial: bool = true,
        pub range: f32 = 40.0,
        pub near: f32 = 2.0,
        pub pitch_var: f32 = 0.0,
        pub volume_var: f32 = 0.0,
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
    /// `friction` and `restitution` are the ground's own contact material. `skirt` (metres, 0 = none) hangs a wall of that depth below every edge of the drawn mesh, to hide the cracks between neighbouring
    /// tiles drawn at different `step`s (a streamed world draws its far tiles coarser: `docs/REALWORLD.md`); the physics ground is unchanged.
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
        pub skirt: f32 = 0.0,
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
    /// `event.NAME.x`, `.y`, `.z` and `.uid` hold where and by whom the first one was emitted, and `event.NAME.0.x` ... `event.NAME.3.uid` the first four one by one (`event.NAME.n`: how many of the four slots are filled). So `{"if": {"gt": [{"res": "event.coin"}, 0]}, ...}` (Script: `if $event.coin > 0`) runs once
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
    /// `skin` is the entity with the animated model (a `MeshRenderer` of a skinned `.glb` and an `Animator`; 0: the model this entity is a child of, so that a prefab can carry its goals), `joint` the name of the joint (`model_info` lists them).
    /// `kind` 0 reaches: the two bones above `joint` bend, like an arm or a leg, so that `joint` (the hand, the foot) lands on the point, as far as the bones allow; `pole`
    /// is another entity the elbow or knee bends towards (without one it keeps the bend the animation has). `kind` 1 looks: `joint` (a head, a neck, a turret) turns so that its
    /// own axis `axis` faces the point, at most `max_angle` degrees away from where the animation points it. `weight` (0..1) blends the goal with the animation, `active` false
    /// switches it off. `kind` 2 **pins**: like 0 it reaches, but the joint is moved only along `axis` (and `axis2`, if it is not zero) so that it is level with the goal in those directions, and keeps the place the
    /// animation gives it in all others (a hand put on the line of a ledge with `axis` up and `axis2` out of the wall, a foot onto the plane of a wall with `axis` alone); `one_sided` moves it only when it is on the
    /// wrong side (behind `axis`, as seen from the goal: a foot or hand that must not sink into the ground). `kind` 3 **turns**: `joint` is turned by `angle` degrees about `axis` (in the model's own space, right-hand rule; the goal's
    /// position is not used), scaled by `weight`: a spine that leans back as the player looks up, a head that tilts. Several goals on one model are applied in uid order. Only the picture changes (the simulation never sees the bent pose); a game that needs the hand's
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
        pub axis2: [f32; 3] = [0.0, 0.0, 0.0],
        pub one_sided: bool = false,
        pub angle: f32 = 0.0,
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
    /// Limits on navigation for a crowd (docs/NAVIGATION.md, "Crowds"): put it on one entity (the lowest uid counts) and the agents' path searches are rationed. Each search
    /// builds a grid of the ground round its two points, which is expensive in a world of many meshes, and an agent that asks again every time its target moves can ask a dozen
    /// times a second. `searches_per_tick` is how many searches the engine does in one tick (0: no limit): the agents that asked and did not get one wait their turn, the one
    /// that asked first goes first, so nobody starves. `min_replan` is the least time in seconds between two searches of one agent (0: none): asking sooner is answered with the
    /// old path until the time has passed. `max_distance` is the longest search in metres (0: none): a target farther than that is walked towards in parts, each search for the
    /// point that far along the way, and the agent asks again when it gets there. `blocked_wait` is the time in seconds an agent waits after a search that found no way before it may search again (0: none; a failed
    /// search leaves no path to count `min_replan` from, and has looked at every cell it could reach). `max_searched` is the most cells one search may expand (0: none): a search that has looked at that many gives up as
    /// "no path", which caps the cost of a failure. `keep_cells` (default off) keeps the data of the grid's cells between the agents' searches, which makes the second search over the same streets many times cheaper: the
    /// cells are then on a lattice anchored at the world's origin and not at the corner of each search's box, so the paths are not the ones of the same world without it (a waypoint may differ by up to half a cell), and the
    /// cells are dropped whenever the world's solids, grounds or mesh levels change (a recorded game that turns it on must be recorded again). Without this component nothing is limited and nothing changes.
    pub struct NavLimits {
        pub searches_per_tick: u32 = 2,
        pub min_replan: f32 = 0.5,
        pub max_distance: f32 = 0.0,
        pub blocked_wait: f32 = 3.0,
        pub max_searched: u32 = 0,
        pub keep_cells: bool = false,
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
    /// A motor for a hinge (`Joint` with `kind` 4; put this on the same entity): it turns the body about the hinge axis at `speed` radians per
    /// second (positive is counter-clockwise about the axis, seen from the side the axis points to, relative to the joint's target) with a torque of
    /// at most `max_torque` newton-metres, so a strong motor holds the speed against a load and a weak one is slowed or stalled by it. A `speed` of 0
    /// with a large `max_torque` is a brake that holds an arm where it is. No `max_torque` (0), no motor. The hinge limits still hold.
    pub struct JointMotor { pub speed: f32 = 0.0, pub max_torque: f32 = 0.0 }
}

component! {
    /// A swing limit for a ball-and-socket (`Joint` with `kind` 3; put this on the same entity): the joint's `axis` (a world direction as the scene
    /// starts, default up) is a line fixed in the body and a line fixed in the target; the angle between the two lines may not grow past `angle`
    /// radians (a shoulder or a hip that bends only so far). An `angle` of 0 or of pi and more means no limit.
    pub struct JointCone { pub angle: f32 = 0.0 }
}

component! {
    /// Lets resting rigid bodies fall asleep (put this on one entity, the lowest uid wins; without it nothing sleeps). A group of touching bodies in which
    /// every body has stayed within `linear * time` metres of one place (an average speed under `linear` metres per second) and has spun slower than
    /// `angular` radians per second for `time` seconds goes to sleep: the solver skips it and it stays exactly where it is, until something touches it,
    /// moves it, or changes the colliders around it. A body tied by a `Joint`, a convex hull or a trigger never sleeps. See `docs/PHYSICS.md`.
    pub struct PhysicsSleep { pub time: f32 = 0.5, pub linear: f32 = 0.02, pub angular: f32 = 0.04 }
}

component! {
    /// A spring on a hinge (`Joint` with `kind` 4; put this on the same entity): it turns the body towards `angle` radians (the hinge angle as `limit` has it, of the body
    /// relative to its target, 0 where the joint started), with a torque of `stiffness` newton-metres per radian of error and `damping` newton-metres per radian per second
    /// of the spin about the axis. A door that swings shut by itself, a suspension arm, a muscle. A spring is soft: it is solved with the rest of the joint, so a very stiff one
    /// is held by the joint's limits and motors if they are there.
    pub struct JointSpring { pub stiffness: f32 = 0.0, pub damping: f32 = 0.0, pub angle: f32 = 0.0 }
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
    /// A button that rebinds one key of an action of the `InputMap` (docs/KEYBINDS.md): put it on a `Ui` button. `action` is the action's name and `index` which of its keys (from 0, in the order the `InputMap` text
    /// has them; pad terms and `$resources` are not counted: `move_y = w, s:-1, gamepad.ly` has the keys `w` and `s`). The engine writes the button's text, the key now (`E`, `SPACE`, `LEFT CLICK`), and while it waits for
    /// a key `PRESS A KEY`. A click starts the wait, the next key or mouse button pressed becomes the action's key (Esc cancels; a key another button has trades places with this one), the `InputMap` text is rewritten
    /// and `settings.save` is set, so the native player keeps it (`bind.ACTION.INDEX` in `settings.json`). `listening` is true while the button waits (a game lights it). `default` is the key the scene had (the engine
    /// fills it in on the first tick when it is empty): the resource `rebind.reset` above 0 puts it back. The resources: `rebind.state` (0 nothing, 1 clicked, 2 waiting for the key, 3 taken, not let go yet: the actions
    /// all read 0 while it is above 0), `rebind.cancel` (set it when the screen closes), `rebind.count` (how many changes), `rebind.denied`. `{key:ACTION}` in any text is the first key of that action, `{key:ACTION:N}`
    /// the Nth. Not for a lockstep or rollback game: the bindings are part of the world there.
    pub struct KeyBind {
        pub action: Sym = Sym(0),
        pub index: u32 = 0,
        pub listening: bool = false,
        pub default: Sym = Sym(0),
    }
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
    /// camera jitters by `shake_amount` metres times its square and calms down at `shake_decay` per second. `roll` leans the picture (radians, positive to the right) about the way it looks.
    ///
    /// A follow spring on the point the rig orbits (the pivot), for a camera that trails a fast character without lagging the mouse: with `lag` above 0 the pivot is a critically damped spring on the target (its natural
    /// frequency in rad/s: 12 settles in about a quarter of a second, `lag_up` the same for the up axis, 0 = `lag`), the camera orbits the pivot with the yaw and pitch it is given (the mouse stays one to one: nothing
    /// eases the turn), and `follow_speed` is not used. `lead` is the seconds of the target's velocity the pivot aims ahead sideways (`2 / lag` keeps up with a target that goes at a steady speed, more shows what is ahead),
    /// `lag_max` and `lag_max_up` the farthest the pivot may fall behind or run ahead, in metres (0 = no limit, `lag_max_up` 0 = `lag_max`). A target that moves faster than 90 m/s in a tick (a teleport) takes the pivot with it.
    /// The last five fields are the spring's state, written by the engine.
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
        // the picture's roll about the way the camera looks, in radians (positive leans the top of the picture to the right): a game eases it in a wall run, a slide
        pub roll: f32 = 0.0,
        // the follow spring of the pivot (see above)
        pub lag: f32 = 0.0,
        pub lag_up: f32 = 0.0,
        pub lead: f32 = 0.0,
        pub lag_max: f32 = 0.0,
        pub lag_max_up: f32 = 0.0,
        pub pivot: [f32; 3] = [0.0, 0.0, 0.0],
        pub pivot_vel: [f32; 3] = [0.0, 0.0, 0.0],
        pub pivot_from: [f32; 3] = [0.0, 0.0, 0.0],
        pub pivot_lead: [f32; 3] = [0.0, 0.0, 0.0],
        pub pivot_on: bool = false,
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

/// Most samples a `UiPlot` keeps (and so most bars it can have).
pub const PLOT_MAX: usize = 128;

component! {
    /// A live graph of a resource, drawn as bars: put it on an entity with a `Ui` (the frame and background; the bars lie inside its rectangle). `source` is the
    /// name of the resource to plot (`cpu.percent`); every `every` ticks the plot takes its value as a new sample and the graph scrolls to the left, newest on the
    /// right; `samples` bars (1 to 128) show the last that many. The vertical scale is `min` to `max`, or, when `max` is not above `min`, from zero (the lowest sample
    /// when `log` is on) to the largest sample shown; `log` plots the base-10 logarithm of the value, for something that grows by factors (a population). `gap` is the
    /// space between bars as a share of a bar's slot, `bar_color` their colour. The engine makes the bars (`UiPlotBar` entities) the first tick, and keeps `data`, `head`, `count`,
    /// `timer`, `built`, `scale_lo` and `scale_hi` (the scale now in use: a label can show it). See `docs/UI.md`.
    pub struct UiPlot {
        pub source: Sym = Sym(0),
        pub every: u32 = 6,
        pub samples: u32 = 60,
        pub min: f32 = 0.0,
        pub max: f32 = 0.0,
        pub log: bool = false,
        pub gap: f32 = 0.15,
        pub bar_color: [f32; 4] = [0.3, 0.75, 1.0, 1.0],
        pub built: bool = false,
        pub timer: u32 = 0,
        pub head: u32 = 0,
        pub count: u32 = 0,
        pub scale_lo: f32 = 0.0,
        pub scale_hi: f32 = 1.0,
        pub data: [f32; 128] = [0.0; 128],
    }
}

component! {
    /// One bar of a `UiPlot` (made by the engine, with a `Ui` that the plot moves and sizes every tick): `plot` is the plot's uid and `index` the bar's place from the left.
    pub struct UiPlotBar { pub plot: Uid = Uid(0), pub index: u32 = 0 }
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
    /// empty and not focused, and a blinking bar while focused). `max_len` characters at most. In a window (`lunaris play`) the keyboard types into the focused field, Ctrl+V pastes the
    /// clipboard, holding Backspace repeats and Ctrl+Backspace clears it (docs/UI.md, "Typing"). `show_last` above 0 shows only the last that many characters after `...` (a long text
    /// such as a connect token does not run out of its box; the whole text is still `text`). `remember` is a name: the native player keeps the field's text in `settings.json` under `text.NAME` when it has been changed and the field has lost the focus, and puts it back before the first tick of the next start (an address, a player's name; never a secret). `number` (the engine's) is the text read as a whole number of up to nine digits (`4711`; `0042` is 42), -1 while the text is empty or is anything else: a graph system cannot read a text, and a code or an amount is read from here.
    pub struct UiInput {
        pub text: Sym = Sym(0),
        pub placeholder: Sym = Sym(0),
        pub focused: bool = false,
        pub max_len: u32 = 32,
        pub submitted: bool = false,
        pub show_last: u32 = 0,
        pub remember: Sym = Sym(0),
        pub number: i32 = -1,
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
    /// `min_radius` (0 = none) makes the zone a ring: it loads only while the target is at least that far away (and unloads when it comes nearer than `min_radius` minus `hysteresis`),
    /// so that the same piece of ground can be a coarse prefab in the distance and a detailed one up close (`docs/REALWORLD.md`, "Levels of detail of a streamed world").
    /// `prefetch` (metres, 0 = none) and `assets` make the load seamless: `assets` is text, one file a line (`texture NAME`, `model NAME`, `heights NAME`, `splat NAME`, names relative to the workspace), the files
    /// the prefab will need, and while the zone is not loaded and the camera is within `radius + prefetch` of it (and not nearer than `min_radius - prefetch`) the renderer's file cache reads them on other threads,
    /// so that when the zone loads they are already in memory instead of being read, and the game stopped, in the picture the prefab first appears in.
    /// `requires` (an entity with a `StreamZone`; 0 = none) makes the zone depend on another: it loads only while that zone is loaded (and unloads when it unloads), besides its own distances. A tree of tiles that is
    /// refined as the camera comes near (a city's levels of detail) is built so: a tile's coarse prefab is a ring zone, the same place's "refined" zone (a small prefab, nothing to see) has the ring's inner radius, and the
    /// tiles under it `require` the refined zone, so exactly one level of each branch is there at a time. A required zone that does not exist keeps the zone from loading.
    /// `vertical` (metres, 0 = ignore height) also bounds the height: the zone loads only while the target is within that many metres of the entity's height (and unloads beyond `vertical` plus `hysteresis`), so the
    /// floors of a tall building can each be a zone, and only the floors near the player are there (a city whose every building can be walked into: `docs/REALWORLD.md`).
    pub struct StreamZone {
        pub prefab: Sym = Sym(0),
        pub radius: f32 = 100.0,
        pub hysteresis: f32 = 20.0,
        pub target: Uid = Uid(0),
        pub loaded: bool = false,
        pub min_radius: f32 = 0.0,
        pub prefetch: f32 = 0.0,
        pub assets: Sym = Sym(0),
        pub requires: Uid = Uid(0),
        pub vertical: f32 = 0.0,
    }
}

component! {
    /// A place that streaming works from, besides the camera (a server has no camera and many players): put it on an entity that has a `Transform` (a player's avatar). The `StreamZone`s that name no target and the cells of a `ProcGrid`
    /// are loaded while ANY anchor (or the active camera) is within their radius and let go only when ALL of them are beyond radius plus hysteresis, so a server that gives every avatar a `StreamAnchor` has loaded the world round every player and
    /// nothing else. `on` false leaves the entity out. A client does not need it: its camera is its anchor.
    pub struct StreamAnchor {
        pub on: bool = true,
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
    /// Which side an entity is on (docs/COMBAT.md): weapons never hurt an entity of their own team, and the AI toolkit treats other teams as hostile. `id` 0 is no team: it is hurt
    /// by everybody and hurts everybody (a free-for-all player, a barrel). Players of a free-for-all server all have 0; a squad shares an id; the machines of a game share another.
    pub struct Team {
        pub id: u32 = 0,
    }
}

component! {
    /// A gun (docs/COMBAT.md): fires along its entity's local -z from `muzzle` (an offset in the entity's own axes), as an instant ray (`kind` 0, a hitscan: rifles, pistols, SMGs,
    /// shotguns with `pellets` above 1) or as a travelling round (`kind` 1: a `Bullet` entity that flies at `muzzle_speed` and falls by `gravity`, for snipers and anything where distance
    /// should take time). A script, the input or an AI holds `fire` true to pull the trigger and sets `reload` for one tick; `auto` false makes it one round for each pull. Hits hurt the
    /// `HitPoints` of the first thing in the way (cover and walls stop the round) except the `owner` (its holder, never hurt; 0 takes the entity's `Parent`) and anyone of the owner's
    /// `Team`. Damage is `damage` per pellet, full up to `falloff_start` metres and falling to `falloff_min` times that at `range` (nothing beyond); the head (the top fifth of a
    /// character) takes `head_mult` times, the legs (the lower 45 percent) `limb_mult`; `pierce` (0 to 1) is the share of worn armor's stopping that a hit ignores (0 none, 1 all: a sniper round goes through a vest), see `armor::absorb_piercing`. The cone of fire is `spread` plus the `bloom` that each shot adds (`bloom_per_shot` up to
    /// `bloom_max`, `bloom_recover` degrees a second back) and the recoil (`recoil` degrees up and at random sideways per shot) piles up in `kick` (pitch and yaw, degrees; a camera or an
    /// aim adds it to where it looks, and it settles by `recoil_recover` degrees a second); the shot itself already leans by `kick`. `noise` is how far (metres) a round can be heard by an AI's `Perception` (0 for a silenced gun). Ammo: `mag` of `mag_size`, `reserve` behind it
    /// (`infinite` never runs out), `reload_time` seconds to reload (an empty magazine starts it by itself when the trigger is held). Everything below `cooldown` is the engine's:
    /// `fired` is true for the tick a round left, `dry` for a trigger pulled on nothing, `hit` (a uid, 0 for none or a wall), `hit_point`, `hit_normal`, `hit_zone` (0 nothing, 1 body,
    /// 2 head, 3 legs), `hit_dist` and `hit_damage` (all pellets of the tick) describe what the first pellet of the tick hit, `shots` and `kills` count. Events: `shot` (the weapon,
    /// at the muzzle), `hit` (the victim, at the point), `impact` (a wall, at the point), `kill` (the victim, at the point), `reload_done`, `dry`.
    pub struct Weapon {
        pub active: bool = true,
        pub fire: bool = false,
        pub reload: bool = false,
        pub owner: Uid = Uid(0),
        pub kind: u32 = 0,
        pub damage: f32 = 25.0,
        pub pellets: u32 = 1,
        pub spread: f32 = 0.4,
        pub range: f32 = 200.0,
        pub falloff_start: f32 = 50.0,
        pub falloff_min: f32 = 0.35,
        pub head_mult: f32 = 2.0,
        pub limb_mult: f32 = 0.7,
        pub pierce: f32 = 0.0,
        pub rate: f32 = 600.0,
        pub auto: bool = true,
        pub mag_size: u32 = 30,
        pub mag: u32 = 30,
        pub reserve: u32 = 120,
        pub infinite: bool = false,
        pub reload_time: f32 = 2.2,
        pub bloom: f32 = 0.0,
        pub bloom_per_shot: f32 = 0.3,
        pub bloom_max: f32 = 3.0,
        pub bloom_recover: f32 = 5.0,
        pub recoil: [f32; 2] = [0.6, 0.25],
        pub recoil_recover: f32 = 8.0,
        pub kick: [f32; 2] = [0.0, 0.0],
        pub muzzle: [f32; 3] = [0.0, 0.0, 0.0],
        pub noise: f32 = 120.0,
        pub muzzle_speed: f32 = 400.0,
        pub gravity: f32 = 9.81,
        pub bullet_life: f32 = 4.0,
        pub cooldown: f32 = 0.0,
        pub reload_left: f32 = 0.0,
        pub held: bool = false,
        pub fired: bool = false,
        pub dry: bool = false,
        pub shots: u32 = 0,
        pub kills: u32 = 0,
        pub hit: Uid = Uid(0),
        pub hit_point: [f32; 3] = [0.0, 0.0, 0.0],
        pub hit_normal: [f32; 3] = [0.0, 0.0, 0.0],
        pub hit_zone: u32 = 0,
        pub hit_dist: f32 = 0.0,
        pub hit_damage: f32 = 0.0,
    }
}

component! {
    /// A round in flight, made by a `Weapon` of `kind` 1 (docs/COMBAT.md; a game does not add these by hand, but may read them to draw a tracer: `Transform.pos` is where it is,
    /// `vel` its velocity). Each tick it moves by `vel`, falls by `gravity`, and the stretch it flew is tested for what it hit (walls, targets with `HitPoints`, never `owner` or its
    /// team); a hit hurts with the weapon's own numbers (`damage` with the falloff over `travelled` metres) and removes it, as do `range` and `life`. `weapon` is the gun to credit.
    pub struct Bullet {
        pub owner: Uid = Uid(0),
        pub weapon: Uid = Uid(0),
        pub team: u32 = 0,
        pub damage: f32 = 25.0,
        pub range: f32 = 200.0,
        pub falloff_start: f32 = 50.0,
        pub falloff_min: f32 = 0.35,
        pub head_mult: f32 = 2.0,
        pub limb_mult: f32 = 0.7,
        pub pierce: f32 = 0.0,
        pub vel: [f32; 3] = [0.0, 0.0, 0.0],
        pub gravity: f32 = 9.81,
        pub life: f32 = 4.0,
        pub age: f32 = 0.0,
        pub travelled: f32 = 0.0,
    }
}

component! {
    /// Makes noise for an AI to hear (docs/AI.md): `radius` is how far, in metres, it can be heard from now. A game's script sets it every tick (footsteps by speed: a walk 5 m, a sprint 14 m,
    /// a crouch 1.5 m; a door slam 20 m); 0 is silent. A `Weapon` that fires is heard to its own `noise` without this. Only a `Perception` of another side hears it.
    pub struct Noisy {
        pub radius: f32 = 0.0,
    }
}

component! {
    /// The senses and the awareness of an AI (docs/AI.md), beside a `Transform` (and a `Team`: it perceives entities of other teams, or any when its own team is 0, that have `HitPoints`).
    /// **Sight**: within `sight_range` metres and `sight_fov` degrees in front (its local -z, from `sight_eye` metres above its `Transform`), with `los` true only if no wall is in the way
    /// (a thing within 1.5 m is always noticed). **Hearing**: a `Noisy` thing or a `Weapon` that fires is heard when it is nearer than its own `radius` times `hearing` (0 is deaf, 1 normal, 2 sharp).
    /// **Awareness** (0 to 1) fills while it sees a hostile (in `notice_time` seconds at the edge of its range, three times faster point blank) and drains in `lose_time` seconds when it does not;
    /// a noise lifts it to 0.55 and remembers the place. `state` is 0 idle, 1 suspicious (it sees something but is not sure), 2 alert (it sees the target and is sure), 3 searching (it knows where
    /// the target was: `seen_pos`). `target` is who, `visible` whether it sees it this tick, `distance`, `since_seen`, `seen_count` how many hostiles it sees, `heard` and `noise_pos`.
    /// When it becomes alert, same-team `Perception`s within `shout` metres start searching where the target is. Events: `spotted` (it became alert), `lost` (alert to searching), `forgot` (back to idle), `heard`.
    /// With `engage` true the toolkit also acts: while alert or searching it walks its `NavAgent` to `seen_pos` (`chase`, stopping at 85 percent of `attack_range` when it sees the target), turns to face
    /// a target it sees (`turn`), and holds `weapon`'s trigger (its own `Weapon` when `weapon` is 0) while it sees the target within `attack_range`. Without `engage` it only senses: a behaviour tree or a
    /// state machine reads `Perception.state` and decides.
    pub struct Perception {
        pub active: bool = true,
        pub sight_range: f32 = 40.0,
        pub sight_fov: f32 = 110.0,
        pub sight_eye: f32 = 1.6,
        pub los: bool = true,
        pub hearing: f32 = 1.0,
        pub notice_time: f32 = 1.0,
        pub lose_time: f32 = 6.0,
        pub shout: f32 = 0.0,
        pub engage: bool = false,
        pub chase: bool = true,
        pub turn: bool = true,
        pub attack_range: f32 = 25.0,
        pub weapon: Uid = Uid(0),
        pub state: u32 = 0,
        pub awareness: f32 = 0.0,
        pub target: Uid = Uid(0),
        pub visible: bool = false,
        pub seen_count: u32 = 0,
        pub distance: f32 = 0.0,
        pub seen_pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub since_seen: f32 = 0.0,
        pub heard: bool = false,
        pub noise_pos: [f32; 3] = [0.0, 0.0, 0.0],
    }
}

component! {
    /// A searching beam (docs/AI.md): a cone from this entity's `Transform` along its local -z, `angle` degrees either side of the axis and `range` metres long; with `sweep` degrees a second the
    /// engine turns the entity about the vertical axis (a lighthouse; the entity must have no `Parent`, or turn the parent). Hostile entities with `HitPoints` inside the beam (and, with `los`, not behind
    /// a wall) become exposed: their `Exposure.value` (added by the engine) fills in `hold` seconds and drains by its own `decay` when they are out of the light; at 1 they are detected. `lit` counts those in
    /// the beam now, `target` and `exposure` say who is the most exposed, `detected` whether any has been caught. Events: `lit` (a target entered the beam), `unlit` (left it), `detected` (an exposure reached 1).
    /// A game (or the AI director) answers `detected` by sending machines: set their `Perception` or give them a `NavAgent` target.
    pub struct Searchlight {
        pub active: bool = true,
        pub range: f32 = 150.0,
        pub angle: f32 = 6.0,
        pub sweep: f32 = 0.0,
        pub hold: f32 = 1.5,
        pub los: bool = true,
        pub lit: u32 = 0,
        pub target: Uid = Uid(0),
        pub exposure: f32 = 0.0,
        pub detected: bool = false,
    }
}

component! {
    /// How exposed something is to the `Searchlight`s of the other side (docs/AI.md): `value` 0 (hidden) to 1 (detected), filled by a beam and drained by `decay` per second out of it; `by` is the last light
    /// that held it, `lit` whether it is in a beam this tick, `detected` whether `value` is 1. The engine adds it to a target the first time a beam touches it; a game may add it beforehand to change `decay`
    /// (a player in cover or crouched drains faster).
    pub struct Exposure {
        pub value: f32 = 0.0,
        pub decay: f32 = 0.5,
        pub by: Uid = Uid(0),
        pub lit: bool = false,
        pub detected: bool = false,
    }
}

component! {
    /// The survival needs of a character (docs/SURVIVAL.md), beside its `HitPoints`: `hunger`, `thirst` and `stamina` (0 to 100; 100 is full) and `bleeding` (hit points lost a second until it is stopped).
    /// Hunger and thirst fall by `hunger_rate` and `thirst_rate` a second (`exertion` times faster while sprinting, half as fast while `resting`); at 0 they hurt (`starve_damage`, `dehydrate_damage`
    /// hit points a second). Stamina falls by `sprint_cost` a second while `sprint` is true and it can (`can_sprint`), and comes back by `stamina_regen` after `stamina_delay` seconds; at 0 the character
    /// is `exhausted` and cannot sprint until it is back to 25. A character that is fed and watered (both at least `heal_threshold`) and not bleeding heals `heal_rate` a second. A game's script sets the
    /// inputs (`sprint`, `resting`) and the one-tick requests when an item is used (`eat` and `drink` add to the meters, `heal` adds hit points at once, `bandage` stops the bleeding); the engine answers in
    /// `status` (bits: 1 hungry below 25, 2 thirsty below 25, 4 bleeding, 8 exhausted, 16 starving or dehydrated), `can_sprint` and `speed_scale` (multiply the walking speed by it: slower when
    /// exhausted, starving or bleeding). Events: `hungry`, `thirsty`, `bleeding`, `exhausted`, `starving` when the bit comes on. **Wounds:** a hit of a weapon (after the worn armor) of at least `wound_min` hit points starts a bleed with the chance `wound_chance` (0, the default, never: `survival::wound`, deterministic from a hash of the tick, the victim and the shot), adding `wound_rate` hit points a second to `bleeding` up to `bleed_max`; a bandage stops it.
    pub struct Vitals {
        pub active: bool = true,
        pub sprint: bool = false,
        pub resting: bool = false,
        pub hunger: f32 = 100.0,
        pub thirst: f32 = 100.0,
        pub stamina: f32 = 100.0,
        pub bleeding: f32 = 0.0,
        pub hunger_rate: f32 = 0.05,
        pub thirst_rate: f32 = 0.08,
        pub exertion: f32 = 2.0,
        pub sprint_cost: f32 = 18.0,
        pub stamina_regen: f32 = 12.0,
        pub stamina_delay: f32 = 1.0,
        pub starve_damage: f32 = 0.5,
        pub dehydrate_damage: f32 = 0.8,
        pub heal_threshold: f32 = 70.0,
        pub heal_rate: f32 = 0.3,
        pub eat: f32 = 0.0,
        pub drink: f32 = 0.0,
        pub heal: f32 = 0.0,
        pub bandage: bool = false,
        pub wound_chance: f32 = 0.0,
        pub wound_min: f32 = 6.0,
        pub wound_rate: f32 = 0.5,
        pub bleed_max: f32 = 2.0,
        pub exhausted: bool = false,
        pub can_sprint: bool = true,
        pub since_sprint: f32 = 10.0,
        pub status: u32 = 0,
        pub speed_scale: f32 = 1.0,
    }
}

component! {
    /// How much trouble a player is in with the machines (docs/AI.md, the director): a meter from 0 up (the engine caps it at 10) that the `Director` raises when the player is caught in a
    /// `Searchlight` (`Exposure.detected`), fires a `Weapon` that can be heard, or is seen by a machine, and that falls by `decay` a second. At the director's `threshold` hunters are sent. `cooldown`
    /// (seconds until another wave may be sent for this player) and `waves` (how many were sent) are the engine's. A game may also add to `value` itself (a loud alarm, a kill, a raid).
    /// With an `Escalation` in the scene (docs/WALKER.md) the director also keeps the player's `alert` (a slow meter that takes a share of all the heat gained and falls only slowly), its `tier` (0 to 7, from the alert), `walker_cd`
    /// (seconds until another Walker may be sent for this player) and `last` (what `value` was when the director left it: the heat that others added since is what it counts).
    pub struct Heat {
        pub value: f32 = 0.0,
        pub decay: f32 = 0.04,
        pub cooldown: f32 = 0.0,
        pub waves: u32 = 0,
        pub alert: f32 = 0.0,
        pub tier: u32 = 0,
        pub walker_cd: f32 = 0.0,
        pub last: f32 = 0.0,
    }
}

component! {
    /// The AI director (docs/AI.md): it watches every entity with a `Heat` and, when one reaches `threshold`, sends a wave of hunters made from the prefab `prefab` (the game's own: its body, its
    /// `Team`, a `Perception` with `engage`, a `NavAgent`, a `Weapon`) on a ring `min_dist` to `max_dist` metres round the player, `height` metres above its feet, facing it, already alerted to where it is
    /// (their `Perception` starts searching at the player's place). A wave is `1 + per_heat` times the heat above the threshold hunters (at most `max_alive` of this director's alive in all, and 16 in a
    /// wave), costs the player `wave_cost` of heat, and the next one for that player waits `cooldown` seconds. Heat comes from `light_heat` a second while a `Searchlight` has the player detected,
    /// `shot_heat` for each tick the player fires a `Weapon` that can be heard (the gun's `owner`, or when it has none the entity it is a child of: the gun on an avatar's head is the avatar's), and `seen_heat` a second for each machine (up to 3) that is alert and sees the player (the first active director's numbers
    /// are used for all). A hunter that has gone back to idle for `cleanup` seconds is removed (0 keeps it). `clear` is how many more places a hunter may be drawn for to find one with a clear line to the player's head (0: the first draw is taken, as before): hunters dropped behind a building in a city never saw the player. Events: `wave` (this director, at the player), `hunter` (the new hunter). `waves`, `sent` and
    /// `alive` are the engine's.
    pub struct Director {
        pub active: bool = true,
        pub prefab: Sym = Sym(0),
        pub threshold: f32 = 1.0,
        pub light_heat: f32 = 0.6,
        pub shot_heat: f32 = 0.15,
        pub seen_heat: f32 = 0.1,
        pub per_heat: f32 = 2.0,
        pub wave_cost: f32 = 0.6,
        pub cooldown: f32 = 25.0,
        pub max_alive: u32 = 6,
        pub min_dist: f32 = 40.0,
        pub max_dist: f32 = 70.0,
        pub height: f32 = 1.0,
        pub cleanup: f32 = 90.0,
        pub clear: u32 = 0,
        pub waves: u32 = 0,
        pub sent: u32 = 0,
        pub alive: u32 = 0,
    }
}

component! {
    /// A machine a `Hacker` can take over (docs/AI.md, hacking): within `range` metres of the hacker (and with `los`, in its line of sight) it takes `difficulty` seconds of continuous hacking at
    /// the hacker's speed 1, and `progress` falls back by half a second's worth a second when nobody hacks it. When it is taken (`hacked`), it joins the hacker's `Team`, its own `Perception` and
    /// autopilot stop (so its AI no longer shoots at the hacker's side), the hacker's `move_dir` drives it (`speed` metres a second at most, `accel` m/s^2: its `Velocity` if it has one, its `CharacterBody` walk
    /// if it has one) and the hacker's `fire` holds its `Weapon`'s trigger. Control lasts `duration` seconds (0: until released), or until the hacker lets go, the machine is destroyed or the hacker
    /// dies; then it goes back to its own team and AI (with `keep` true it stays on the hacker's team and its AI works again, now against the old side). **`mode` 1 turns the machine instead of taking it** (a turret that
    /// is stationary and has no use for hands, docs/SENTRY.md): at 100 percent it joins the hacker's `Team` for `duration` seconds and its own AI keeps working, now against the old side (it never targets its hacker or the
    /// hacker's party: `Hackable.controller`), the hacker stays free (no `state` 2, no camera, no driving; the hack key must be let go before the next hack) and nobody drives it; it goes back when the time runs out
    /// or it is destroyed (the hacker dying or leaving does not end it); events `hacked` (the machine) and `link_lost`. `time_left` is the seconds that remain. The last fields are the engine's.
    pub struct Hackable {
        pub active: bool = true,
        pub difficulty: f32 = 4.0,
        pub range: f32 = 25.0,
        pub los: bool = true,
        pub duration: f32 = 60.0,
        pub keep: bool = false,
        pub speed: f32 = 8.0,
        pub accel: f32 = 20.0,
        pub progress: f32 = 0.0,
        pub hacked: bool = false,
        pub controller: Uid = Uid(0),
        pub time_left: f32 = 0.0,
        pub saved_team: u32 = 0,
        pub saved_engage: bool = false,
        pub saved_perception: bool = true,
        pub saved_fly: bool = true,
        pub mode: u32 = 0,
    }
}

component! {
    /// The goggles (docs/AI.md, hacking), on a player beside its `Transform`: the game sets `target` (the machine the crosshair is on) and holds `hack` true; within `range` metres (and the machine's own
    /// `range`) and in line of sight the machine's `progress` fills at `speed` (1 is its `difficulty` in seconds) and the hacker's own `Heat` (if it has one) rises by `trace_heat` a second: the machines'
    /// network notices. At 1 the machine is taken: `state` 2, `controlled` its uid, and the `camera` (an entity with a `CameraRig`) follows it instead. While controlling the game sets `move_dir` (the
    /// direction in the world, up to length 1) and `fire` and the engine drives the machine; `release` true for a tick gives it back. `state` is 0 idle, 1 hacking, 2 controlling; `progress` is the hack's
    /// progress; `in_range` whether the target can be hacked now. Events: `hack_start` (the hacker), `hacked` (the machine), `released` (the machine), `link_lost` (the machine: time ran out, it was destroyed, or
    /// the hacker died). **Aiming**: with `aim` the engine chooses `target` itself while `hack` is held: the `Hackable` machine within `range` whose direction from the goggles (the hacker's place and 1.6 m) is
    /// nearest to `look` (the way the player looks, a unit vector in the world that the game sets: a game server sets it from the player's yaw and pitch) and within `aim_cone` degrees of it (4); nothing when none is.
    pub struct Hacker {
        pub active: bool = true,
        pub range: f32 = 30.0,
        pub speed: f32 = 1.0,
        pub trace_heat: f32 = 0.5,
        pub camera: Uid = Uid(0),
        pub target: Uid = Uid(0),
        pub hack: bool = false,
        pub release: bool = false,
        pub move_dir: [f32; 3] = [0.0, 0.0, 0.0],
        pub fire: bool = false,
        pub state: u32 = 0,
        pub progress: f32 = 0.0,
        pub controlled: Uid = Uid(0),
        pub in_range: bool = false,
        pub saved_view: Uid = Uid(0),
        pub blocked: bool = false,
        pub aim: bool = false,
        pub aim_cone: f32 = 4.0,
        pub look: [f32; 3] = [0.0, 0.0, -1.0],
    }
}

component! {
    /// The loot tables of a game, as text (docs/LOOT.md). One entity of the world has it (the lowest uid wins). A table is a line `table NAME rolls=A-B empty=P` (the number of rolls, and the chance
    /// P, 0 to 1, that a roll gives nothing) followed by its entries, one a line: `ITEM w=WEIGHT n=A-B` (an item of the `ItemTable` by key, its weight among the entries, how many of it) or `@OTHER w=WEIGHT`
    /// (roll another table: a rare find inside a common one). `#` starts a comment. Used by `LootContainer`.
    pub struct LootTable {
        pub text: Sym = Sym(0),
    }
}

component! {
    /// A chest, a cabinet, a corpse, a crate (docs/LOOT.md), beside an `Inventory`: the first time (or after a refill) it fills the inventory from the loot table named `table`, rolled from `key` (give each
    /// container its own number, a building's or a room's, so the same container always holds the same things in a world; 0 uses its uid) and how many times it has been filled. With `refill` above 0 the
    /// container fills again that many seconds after the last fill, once its inventory is empty (looted). `filled` and `fills` are the engine's; set `filled` false to fill it again at once. Event `loot_filled`.
    pub struct LootContainer {
        pub table: Sym = Sym(0),
        pub key: u32 = 0,
        pub refill: f32 = 0.0,
        pub filled: bool = false,
        pub fills: u32 = 0,
        pub timer: f32 = 0.0,
    }
}

component! {
    /// A thing that drops loot when it is destroyed (docs/LOOT.md, "Drops"), beside `HitPoints` and a `Transform`: the first tick its `HitPoints` are `dead` it makes, for every stack that the table `table` (of the world's
    /// `LootTable`) rolls, a copy of the prefab `prefab` (a pickup: a prefab of the game with an `Interactable`, like the thing a player's dropped item becomes) with `Interactable.item` set to `KEY N`, in a ring of `spread` metres round
    /// it and `lift` metres up (from the ground below it when `ground` is on, as it is by default: a drone that dies 7 m up leaves its loot on the floor, found by a ray down to the first thing the rays hit; with `ground` off, from where it was). The roll is the same every time for the same `key` (a number of your own: 0 uses the thing's uid), like a container's. Nothing is made without the table, the item table or the prefab. `done` is the engine's
    /// (set it false to drop again). Event `loot_dropped`. On a game server the pickups are told to the players like any pickup the prefab makes; a client's own copy of the machine is replaced by the server's, so only the server drops.
    pub struct LootDrop {
        pub table: Sym = Sym(0),
        pub prefab: Sym = Sym(0),
        pub key: u32 = 0,
        pub spread: f32 = 0.8,
        pub lift: f32 = 0.3,
        pub done: bool = false,
        pub ground: bool = true,
    }
}

component! {
    /// The local player of an online game (docs/NETWORK_SCALE.md, "In a game"): when the game is played with `lunaris play SCENE --connect HOST:PORT`, the entity with this component (the lowest uid) is the one the
    /// server hears about: its `Transform` (the feet) and `Velocity` go up thirty times a second with `pitch` (radians, looking up is positive), `flags` (bits the game gives meaning to: crouched, sliding, wall
    /// running ...), `anim` and `anim_t` (a clip number and its time, if the others should see it) and `extra` (a number: the weapon in the hands); the body's yaw is the rotation's. `puppet` names the prefab
    /// that is made for every other player (the game's own body, with its `NetPuppet` filled in by the engine; with none, a bare entity with a `Transform` and a `NetPuppet`). The server chooses where a
    /// player starts: on connecting the entity is put there, and a move the server refuses puts it back (`corrections` counts them). The engine writes `connected`, `id` (the server's number for this player),
    /// `ping` (round trip, milliseconds) and `players` (how many others it can see). Against a **game server** (`NetHost`, docs/GAME_SERVER.md) the same entity also: shows the things the server replicates
    /// (`NetEntity`: a wall, a machine, a loot bag) as puppets made from the prefab that the text `kinds` names, one line a thing `CLASS KIND PREFAB` (`KIND` `*` is any: `3 12 wall_model`,
    /// `4 * machine_model`; things with no line are not shown); and sends a command to the server's game by setting `cmd` (a name: `build:wall` chooses what the build tool builds, `place` builds it, any
    /// other is for the game's scripts, with `cmd_args` as numbers) for one tick (the engine clears both). A graph system cannot set a name, so a command can also be said by a number: `act` (the engine clears it) names a line of the text `acts`, one a line `NUMBER COMMAND` (`1 build:wall`, `2 place`), and is sent as that command with `cmd_args`. The buttons are `flags` bits (the server maps them to the avatar's weapon with `NetAvatar`). `say_input` is the uid of a `UiInput` whose text is sent to the server as a chat message when the player submits it (Enter), and then emptied (`NetHost.chat`, `NetChat`).
    /// **Vehicles** (docs/SEATS.md): while the player sits in a seat of the server's (`Seat`) the server tells it so and the engine sets `riding` (the server's number of the vehicle, 0 when not) and `seat` (where the player
    /// sits, in the vehicle's axes): the player's `Transform` is carried to that place on the vehicle's puppet every tick (the walking is overridden, the camera follows), and the server ignores the place it reports.
    /// What the hands say goes up in `axes` (four numbers from -1 to 1, a helicopter's roll, collective, pitch and yaw; a graph system sets them from the keys) whenever they change.
    /// **Goggles** (docs/AI.md, hacking): while the player controls a machine of the server's (`Hacker`) the server says so and the engine sets `viewing` (the server's number of the machine, 0 when not) and moves the entity
    /// `camera` (the player's camera, a child of the player: its `LocalTransform` position is set so that it is at the machine's puppet, its direction stays the player's) until it is given back to `eye`, its place when
    /// the player sees for itself. Then `axes` drive the machine: 1 up, 2 forward and 3 right (in the way the player looks), the fire button fires its weapon, the reload button gives it back.
    /// **Itself** (docs/GAME_SERVER.md, "What a player is told about itself"): the server tells the player what its interface shows of its own character, which only the server knows, and the engine sets `health`,
    /// `food`, `water` and `stamina` (0 to 1: `Ui.fill` of a bar), `heat` (the director's meter, 0 to 10), `tier` (the Core's alert tier for this player, 0 to 7: docs/WALKER.md), `bleeding`, `exhausted` (cannot sprint), `reloading`, and for the gun in its hand `gun` (the item number, 0
    /// for none), `mag` (rounds in the magazine) and `reserve` (rounds in the bag for it; 65535 for a gun that never runs out).
    /// **Parties** (docs/PARTIES.md): `say_party_input` is the uid of a second `UiInput` (the party chat box): what is typed in it goes to the player's party only (see `NetParty`).
    pub struct NetPlayer {
        pub puppet: Sym = Sym(0),
        pub kinds: Sym = Sym(0),
        pub cmd: Sym = Sym(0),
        pub cmd_args: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub acts: Sym = Sym(0),
        pub act: u32 = 0,
        pub flags: u32 = 0,
        pub anim: u32 = 0,
        pub anim_t: f32 = 0.0,
        pub pitch: f32 = 0.0,
        pub extra: u32 = 0,
        pub id: u32 = 0,
        pub connected: bool = false,
        pub ping: f32 = 0.0,
        pub players: u32 = 0,
        pub corrections: u32 = 0,
        pub axes: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub riding: u32 = 0,
        pub seat: [f32; 3] = [0.0, 0.0, 0.0],
        pub viewing: u32 = 0,
        pub camera: Uid = Uid(0),
        pub eye: [f32; 3] = [0.0, 1.6, 0.0],
        pub health: f32 = 1.0,
        pub food: f32 = 1.0,
        pub water: f32 = 1.0,
        pub stamina: f32 = 1.0,
        pub heat: f32 = 0.0,
        pub tier: u32 = 0,
        pub bleeding: bool = false,
        pub exhausted: bool = false,
        pub reloading: bool = false,
        pub gun: u32 = 0,
        pub mag: u32 = 0,
        pub reserve: u32 = 0,
        pub say_input: Uid = Uid(0),
        pub say_party_input: Uid = Uid(0),
    }
}

component! {
    /// Another player in an online game (docs/NETWORK_SCALE.md, "In a game"), made, moved and removed by the engine from what the server says: its `Transform` is where that player is (shown a tenth of a
    /// second in the past, between two records, so that it moves smoothly), its `Velocity` (if it has one) the speed, and these fields what the player reports: `id` (the server's number), `flags`, `anim` and
    /// `anim_t` (the clip and its time), `pitch` (looking up is positive, radians), `health` (0 to 255), `extra`, and `speed` (the ground speed, for a locomotion blend). A game's own systems dress it: they
    /// pick the clip from `speed` and `flags`, show a name, turn the head by `pitch`. Against a game server the things it replicates (`NetEntity`) are puppets too: `class` and `kind` say what, `extra`
    /// carries the server's `kind` again for a thing that is not a player.
    pub struct NetPuppet {
        pub id: u32 = 0,
        pub class: u32 = 0,
        pub kind: u32 = 0,
        pub flags: u32 = 0,
        pub anim: u32 = 0,
        pub anim_t: f32 = 0.0,
        pub pitch: f32 = 0.0,
        pub health: f32 = 0.0,
        pub extra: u32 = 0,
        pub speed: f32 = 0.0,
    }
}

component! {
    /// A thing the game server tells the players about (docs/GAME_SERVER.md), on the server's entity (a wall, a machine, a helicopter, a loot bag, a corpse): every tick (every `NetHost.static_every` ticks for
    /// the still `class`es 1 and 3) its `Transform`, its `HitPoints` (as a byte), `flags`, `anim`, `anim_t` and `kind` (the picture of it: the clients pick the prefab by `class` and `kind`) are copied into
    /// the server's world, and the clients near it are sent them at a rate that falls with distance. `class`: 1 an item on the ground and 3 a thing in the world are still (sent slowly), 2 a thing in flight is
    /// sent fast, 4 and up are things that move (machines, vehicles: sent like players). `net` is the server's number for it (the engine's). An entity with a `NetEntity` that is in a scene a client loads is
    /// removed from the client when it connects: the server's copy is the one that counts.
    pub struct NetEntity {
        pub class: u32 = 3,
        pub kind: u32 = 0,
        pub flags: u32 = 0,
        pub anim: u32 = 0,
        pub anim_t: f32 = 0.0,
        pub net: u32 = 0,
    }
}

component! {
    /// The server's body of a connected player (docs/GAME_SERVER.md), on the entity the server makes for each player from `NetHost.player`. The engine puts the player's reported place and look on it before
    /// every simulation step (`Transform` the feet and the yaw, the pitch on the `head` child, or on the entity itself if it has none) and turns the reported button `flags` into the actions of the avatar:
    /// bits of `fire_mask` pull the trigger of its `Weapon`, `reload_mask` reload it, `sprint_mask` set `Vitals.sprint`, `use_mask` press the use button of its `Interactor` (docs/INTERACT.md), `crouch_mask` set `Stealth.crouch` (docs/STEALTH.md), `aim_mask` set `Loadout.aiming` (aim down sights: docs/LOADOUT.md). `head` is the child that carries the `Weapon`, the `BuildTool` and the `Interactor` (an entity of the
    /// avatar's prefab with a `Parent`, a `LocalTransform` at eye height and one of those): the engine finds it when the avatar is made and turns it by the pitch; without one the tools sit on the avatar itself
    /// and aim from its feet. `buttons`, `pitch` and `held` (the number the player says it holds) are what the player reported, `cmd` and `args` the last command the player's game sent that is not the
    /// engine's own (`build:KIND` and `place` are), for one tick, for the game's scripts. `slot` and `id` are the server's numbers for the player.
    pub struct NetAvatar {
        pub slot: u32 = 0,
        pub id: u32 = 0,
        pub fire_mask: u32 = 1,
        pub reload_mask: u32 = 2,
        pub sprint_mask: u32 = 4,
        pub use_mask: u32 = 8,
        pub crouch_mask: u32 = 16,
        pub hack_mask: u32 = 32,
        pub aim_mask: u32 = 64,
        pub head: Uid = Uid(0),
        pub buttons: u32 = 0,
        pub pitch: f32 = 0.0,
        pub held: u32 = 0,
        pub cmd: Sym = Sym(0),
        pub args: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub name: Sym = Sym(0),
    }
}

component! {
    /// How a scene is hosted as a game server (docs/GAME_SERVER.md): one entity of the scene has it (the lowest uid wins) and `lunaris server --scene SCENE` reads it. `player` is the prefab made for every player
    /// that connects (it should have a `NetAvatar`, `HitPoints`, a `Respawn`, a collider and the player's `Weapon` and `BuildTool`; the server adds a `NetAvatar` if it has none). `forward` is the text of the
    /// events of the simulation that the players are told about, one line a kind, `NAME RADIUS` (`shot 150`: every client within 150 m of where the event happened gets it, as an event of its own simulation, which is
    /// what the sound and the effects of a game listen to; its `uid` there is the server's number, the one `NetPlayer.id` holds, of the entity it is about: the victim of a `hit` or a `kill`, the shooter of a `shot`, 0 for none). `static_every` is every how many ticks the still things (`NetEntity` class 1 and 3) are looked at for changes. `save` is a file of the workspace the
    /// world's state is written to every `save_every` seconds and loaded from when the server starts (empty: never saved); the players' avatars are not in it, but what `persist` names (the components of an avatar to
    /// keep for the player, by name, space separated: `Transform Inventory Vitals HitPoints`) is kept by the player's name and given back to the avatar they get when they connect again. `drop` is the prefab made on the ground when a player drops
    /// something from their bag (`inv_drop`): a pickup (an `Interactable` of kind 3, a `NetEntity`, whatever it looks like) that the engine gives the item.
    /// **Using items** (`inv_use`, docs/GAME_SERVER.md): `uses` is the text of what an item of the bag does when the player uses it, one line an item, `use KEY eat=30 drink=0 heal=0 bandage=1 keep=0` (`KEY` is the item's key in the
    /// `ItemTable`): the numbers are added to the avatar's `Vitals` requests (`eat`, `drink`, `heal` and `bandage`, docs/SURVIVAL.md) and one of the item is used up unless `keep` is 1 (a tool).
    /// **Lag compensation** (docs/LAG_COMPENSATION.md): a hitscan shot is tested against the characters and the moving `NetEntity`s with `HitPoints` where the shooter saw them on its screen, which is the shooter's half round trip plus
    /// `interp` (the milliseconds the clients show others behind the present: the client's default is 100) in the past; `lag_comp` is the most milliseconds that is allowed to be (250; 0 switches it off: shots are decided at the present).
    /// **A picture for each item on the ground**: `item_kinds` is a text with a line `ITEM KIND` for each item key whose pickup should look different from the others (`rifle 34`; `#` starts a comment). A pickup (an `Interactable` of kind 3 with a `NetEntity`)
    /// that holds such an item is told to the clients with that `kind` instead of its `NetEntity.kind`, whatever made it (a scene's loot, `inv_drop`, a game's script), so a client's `NetPlayer.kinds` can show a pistol on the ground as a pistol (`1 33 PickupPistol`).
    /// It follows the item: a stack that is partly picked up keeps its picture, and a world saved before the text changed shows the new one.
    /// **Updating a game with a saved world**: a save holds the world's state (entities, resources); the scene's graph systems and prefabs are CODE and always come from the scene the server is started with, so a new version of the
    /// game's logic reaches a world that was saved before it. `world_version` says which LAYOUT of the world the scene has (bump it when the scene's entities change: a bench is added, a crate moves): a save of another
    /// `world_version` is not loaded as a world (it would lack the new things and keep the old ones) and the world starts fresh from the scene, but the players' characters (`persist`) are still given back.
    /// **Resources every client is told** (docs/GAME_SERVER.md): `sync` lists names of resources of the server's world (`dn.t dn.rate dn.force`, separated by spaces, commas or lines; at most 32; the engine's own `input.`,
    /// `action.`, `game.`, `scene.` and `time.` names are ignored). The server sends the ones that changed to every client, at most once every `sync_every` seconds (a clock that changes every tick goes once a second), all of them again
    /// every 5 seconds, and all of them at once to a client that has just joined; a client writes them into its own simulation as resources of the same names. The client's scene needs nothing for it.
    /// **Leaving out what a server does not need**: `skip` lists component names (`Ui Text`, separated by spaces or commas); when the server starts, every entity of its world that has one of them, and everything attached to it with a `Parent`, is
    /// removed: a server has no screen, and the interface's entities (a game's HUD, menus and crafting pages are hundreds of `Ui` entities) cost it memory and the time of every system that visits them. The clients' own scene is not touched
    /// (they run the same scene file and keep their interface). An entity that itself has the `NetHost`, a `NetEntity` or a `SpawnPoint` is not removed because of a listed component (but whatever hangs on an entity that goes, goes with it). Measured on a game scene
    /// of 1,140 entities of which 1,038 were interface: the server's tick fell from 2.1 to 0.9 ms and its memory from 27.3 to 20.3 MB with no player in.
    /// **Chat** (docs/GAME_SERVER.md, "Chat"): `chat` is how far, in metres, a player's text message is told (to every player within that distance of the speaker, the speaker included); 0 switches chat off, a very large number is a chat for everybody.
    /// **Wallhacks** (docs/NETWORK_SCALE.md, "Wallhacks"): the server tells a client where the other players are, so a modified client could show them through walls. `occlusion` is the defence and the switch for it:
    /// 0 (the default) tells every client about every player within the interest radius, as before; a number above 0 is the SAFE RADIUS in metres: a player nearer than that to another is always told, and a
    /// player farther away is not told (the client forgets it after two seconds) while the static world (the `Collider`s that do not move, mesh levels, terrain: what `lunaris_sim::sight::Sight` sees) is in the way of every straight line
    /// from the other's eye to the player's head, chest, feet and shoulders, now and 0.45 s ahead at the speeds they are moving (so that nobody pops into view late round a corner), and for
    /// 0.6 s after it was last in sight (so that ducking behind a wall for a moment loses nothing). 16 to 25 is a good safe radius; loot and the things of the world are still always told. `occlude_machines` hides
    /// the things that move on their own and are told like players (`NetEntity.class` 4 and up: drones, vehicles) in the same way; off by default.
    /// **Parties** (docs/PARTIES.md): `party` is the most members a party has (4 by default; 0 or 1 switches parties off). Players invite each other by name, a party's members never hurt each other
    /// (weapons treat a party as a `Team` from 100 up), talk to each other by `/p` wherever they are, are told where the others are, share spawn points and land, and are always told about each other past walls.
    /// **Characters that are not coming back** (docs/SERVER_ROBUSTNESS.md): a stored character (`persist`) is kept for `character_days` days after its player last left (30 by default; 0 keeps it for ever), then it is let go
    /// at the next save: the world's save, and the memory a save needs, grow with the number of different players otherwise (about 40 to 65 KB for each new name).
    pub struct NetHost {
        pub player: Sym = Sym(0),
        pub forward: Sym = Sym(0),
        pub static_every: u32 = 15,
        pub save: Sym = Sym(0),
        pub save_every: f32 = 60.0,
        pub persist: Sym = Sym(0),
        pub drop: Sym = Sym(0),
        pub lag_comp: f32 = 250.0,
        pub interp: f32 = 100.0,
        pub uses: Sym = Sym(0),
        pub item_kinds: Sym = Sym(0),
        pub world_version: u32 = 0,
        pub sync: Sym = Sym(0),
        pub sync_every: f32 = 1.0,
        pub skip: Sym = Sym(0),
        pub occlusion: f32 = 0.0,
        pub occlude_machines: bool = false,
        pub chat: f32 = 0.0,
        pub party: u32 = 4,
        pub character_days: f32 = 30.0,
    }
}

component! {
    /// Worn gear (docs/EQUIPMENT.md), on a character that has an `Inventory`: `slots` is a text, one worn place of the bag a line, `slot INDEX accepts=CLASS zones=body,head,legs` (the slot takes only items whose `class`
    /// is CLASS, and what is in it protects those zones: the item's `power` is the percent of a hit it stops, its `dur` how much damage it stops before it is used up). A hit of the weapons system on this character
    /// is lessened by what it wears where it was hit (the zones are the weapons system's: body, head, legs and arms), the pieces that stopped something are worn by `wear` points of durability for each point stopped,
    /// and a piece that is used up is broken (the event `item_break`). `cover` (the engine's, kept on a client too) is the share of a hit that is stopped now in each zone (index 1 body, 2 head, 3 legs), `absorbed` all the damage the gear has stopped.
    pub struct Equipment {
        pub slots: Sym = Sym(0),
        pub wear: f32 = 1.0,
        pub cover: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub absorbed: f32 = 0.0,
    }
}

component! {
    /// The chat log of an online game (docs/GAME_SERVER.md, "Chat"), on a `Text` (or a `Ui`) entity of the client's scene: the engine writes the last `lines` messages the player has heard, one to a line as `name: text`, into its
    /// text, and takes a message off the log `life` seconds after it came. `count` is how many messages have come in all (a game that plays a sound for a new line watches it). Messages are said with `NetPlayer.say_input` and
    /// told by a server whose `NetHost.chat` is above 0. With `bottom` the text is filled out with empty lines above it up to `lines`, so that the newest line always stands at the bottom of a text box that grows downward (a log over a chat box).
    pub struct NetChat {
        pub lines: u32 = 6,
        pub life: f32 = 14.0,
        pub count: u32 = 0,
        pub bottom: bool = false,
    }
}

component! {
    /// The party panel of an online game (docs/PARTIES.md), on a `Text` (or a `Ui`) entity of the client's scene: the engine writes what the server says of the player's party into its text, `lines` lines at most:
    /// a header, then a line a member (a star for the leader, `you`, the health in percent, how far in metres and which way by the compass, or `away` for one that lost its connection and `dead`), or, while an
    /// invitation waits, who invites the player, the seconds left and `invite_hint` (a text of the game's, such as the keys that accept and decline; empty: `/accept  /decline`). The text is empty when the player is in no party and
    /// has no invitation. The engine also writes `count` (members, 0 for no party), `party` (the party's number), `leader` (the player leads it), `invited` and `invite_secs` (an invitation waits and for how long), and `heard`
    /// (how many panels have come in: a game that plays a sound for a change watches it). North is -z, east +x.
    /// `marker` names a prefab (a `Text` with a `UiAnchor`, `world` the offset above the feet, such as `[0, 2.2, 0]`): the engine makes one for each party mate whose puppet this client shows, pins it to the puppet (`UiAnchor.entity`),
    /// writes the mate's name into its `Text`, gives it the panel's `Parent` (so that it goes with the HUD) and takes it away when the mate goes, leaves the party or is no longer shown. It is drawn over the world, so it shows through walls.
    pub struct NetParty {
        pub lines: u32 = 8,
        pub invite_hint: Sym = Sym(0),
        pub count: u32 = 0,
        pub party: u32 = 0,
        pub leader: bool = false,
        pub invited: bool = false,
        pub invite_secs: f32 = 0.0,
        pub heard: u32 = 0,
        pub marker: Sym = Sym(0),
    }
}

component! {
    /// The container that the local player has open on a game server (docs/GAME_SERVER.md): an entity of the client's scene with this and an `Inventory` (the interface's `InvSlot`s are bound to that inventory, as to the
    /// player's own bag). When the player opens a container the server says what is in it and the engine writes it into this inventory and sets `open`; when it shuts, `open` goes off and the inventory is emptied.
    /// `source` is the server's number for the container. Things are moved by the commands `inv_move` (`cmd_args` `from`, `to`, `count`, 0: a place is 100 times the bag (0 is the player's own, 1 the open
    /// container) plus the slot; `count` 0 moves the whole stack), `inv_drop` (`slot`, `count`) and `inv_craft` (the recipe number): the server does them and tells the new contents.
    pub struct NetContainer {
        pub open: bool = false,
        pub source: u32 = 0,
    }
}

component! {
    /// The way into an online game from the game's own menu (docs/UI.md, "Connecting from a menu"): the native player (`lunaris play SCENE`, without `--connect`) watches the entity that has this (the lowest uid wins).
    /// When `go` is set (a Connect button's graph system sets it) it takes the server's address from the text of the `UiInput` entity `address_input` (or from `address` when there is no such input), the player's name
    /// from `name_input` (or `name`; "player" when empty), and starts a connection to that server of `docs/GAME_SERVER.md`; `go` goes back to 0. `status` is then 0 nothing going on, 1 connecting, 2 in the game,
    /// 3 the server said no (full, another version), 4 the connection was lost, 5 there was no answer or the address was bad; `message` says it in words. After 3, 4 and 5 the connection is spent: the
    /// menu stays and `go` tries again. A server that answers within `timeout` seconds is let in, otherwise status 5 ("No answer from the server"). `leave` (set by a Leave button) tells the server we go, takes
    /// the other players away and sets `status` 0. While `status` is 2 the player's `NetPlayer.connected` is on too.
    /// A server run with `--secure KEYFILE` (docs/SECURE.md) lets in only players with a connect token: `token_input` (a `UiInput` where the player pastes it) or `token` (text), the hex text or `@FILE` of `--connect-token`;
    /// when both are empty the token of `lunaris play --connect-token ...` given at start is used. With a token the player is whoever the token was made for (`name_input` and `name` are ignored); a text that is no token is
    /// `status` 5 ("that is not a connect token ..."). Without one the connection is a plain one.
    pub struct NetConnect {
        pub address_input: u32 = 0,
        pub name_input: u32 = 0,
        pub address: Sym = Sym(0),
        pub name: Sym = Sym(0),
        pub token_input: u32 = 0,
        pub token: Sym = Sym(0),
        pub go: bool = false,
        pub leave: bool = false,
        pub status: u32 = 0,
        pub message: Sym = Sym(0),
        pub timeout: f32 = 8.0,
    }
}

component! {
    /// The building rules of a game, as text (docs/BUILDING.md). One entity of the world has it (the lowest uid wins). `rules snap=1.2 min=0.1 gap=0.05 slope=0.7 step=15 ground=0.5 audit=1` sets how
    /// close to a socket the aim must be to snap, the stability under which a piece collapses, how much every piece's box is shrunk when testing for overlap, the steepest ground that can be built on
    /// (the ground normal's y), the turn step of a free piece in degrees, how far under a piece the ground may be and still hold it up, and every how many seconds the world is looked over for decay and for
    /// pieces that changed by other means. `piece NAME size=X,Y,Z hp=N free=1 anchor=1 loose=1 decay=D inset=G prefab=P debris=P` starts a kind of piece (a box of that size centred on its entity; free: it can
    /// stand on open ground; anchor: resting on the ground it holds itself up and holds up what is joined to it; loose: furniture, it needs no support but falls with what it was put on; decay: hit points lost a
    /// second without upkeep; prefab: what is spawned; debris: what is spawned when it falls) and each `socket NAME at=X,Y,Z yaw=D type=T mates=A,B carry=0.9,T:0.5 turn=180` under it a point where pieces join
    /// (`mates`: the socket types it plugs into; `carry`: the share of the neighbour's stability it takes, per type or for all). `pieces` and `stamp` are the engine's (what the world looked like at the last audit).
    pub struct BuildRules {
        pub text: Sym = Sym(0),
        pub pieces: u32 = 0,
        pub stamp: u32 = 0,
    }
}

component! {
    /// The building tool of a player (docs/BUILDING.md): on the entity that looks (a camera, a hammer in the hand), it aims along its local -z from its position, `reach` metres. The game sets `kind` (the piece to
    /// build, a `piece` of the `BuildRules`; empty switches the tool off), `owner` (a number for the player or the clan: building is refused inside another owner's `BuildUpkeep`), `turn` (extra degrees for a piece
    /// put on open ground), `flip` (face the other way when it snaps) and, for one tick, `place`. The engine writes where the piece would go (`pos`, `yaw` in degrees, a quarter turn to the left is positive), whether
    /// it can go there (`valid`; if not `why` says: unknown_piece, no_aim, no_socket, no_ground, blocked, not_yours, unstable), what it would be joined to (`snapped`, 0 for open ground) and how stable it
    /// would be (`stability`, 0 to 1). If `ghost` names an entity, that entity's `Transform` follows `pos` and `yaw` (the translucent model of the piece). `place` builds the piece when it is valid (event
    /// `build_placed`, the new piece's uid in `placed`) and is cleared either way (an invalid one gives the event `build_refused`).
    /// `party` (docs/PARTIES.md; the server's party system writes it, 0 for none) is the party the holder is in: a tool whose owner is in the same party as the owner of a `BuildUpkeep` is not refused (`not_yours`) on that land.
    pub struct BuildTool {
        pub kind: Sym = Sym(0),
        pub owner: u32 = 0,
        pub reach: f32 = 6.0,
        pub turn: f32 = 0.0,
        pub flip: bool = false,
        pub place: bool = false,
        pub ghost: Uid = Uid(0),
        pub valid: bool = false,
        pub why: Sym = Sym(0),
        pub pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub yaw: f32 = 0.0,
        pub snapped: Uid = Uid(0),
        pub stability: f32 = 0.0,
        pub placed: Uid = Uid(0),
        pub built: u32 = 0,
        pub party: u32 = 0,
    }
}

component! {
    /// A piece that has been built (docs/BUILDING.md), made by a `BuildTool` or put in a scene by hand (a base that is already there): `kind` is a `piece` of the `BuildRules`, `owner` who built it. The engine keeps
    /// `stability` (0 to 1: 1 is held by the ground, less the further it hangs from it through the sockets; under the rules' `min` it collapses) and `anchored` (it rests on the ground) up to date. `on` is what a
    /// `loose` piece stands on: it falls when that is gone. Its hit points are its `HitPoints`; when they are gone it is destroyed (event `build_destroyed`) and what it held up may collapse (`build_collapse`).
    /// A piece put in a scene by hand has `checked` false: the engine then finds out by itself whether it rests on the ground (the first audit) and sets `anchored` and `checked`; set both to say so yourself.
    pub struct BuildPiece {
        pub kind: Sym = Sym(0),
        pub owner: u32 = 0,
        pub stability: f32 = 1.0,
        pub anchored: bool = false,
        pub checked: bool = false,
        pub on: Uid = Uid(0),
    }
}

component! {
    /// A place on something that is not a piece, where pieces may be built (docs/BUILDING.md): the entity's `Transform` is the point and the way it faces (a point on the facade or the roof of a generated
    /// building). `kind` is a socket type, which a piece socket lists in its `mates` to join here. With `anchor` (the default) the thing is solid ground to the structure: a piece joined to it is held up
    /// through its `carry` as if it stood on the ground; without, it is only a place to snap to.
    pub struct BuildSocket {
        pub kind: Sym = Sym(0),
        pub anchor: bool = true,
    }
}

component! {
    /// A tool cupboard (docs/BUILDING.md): within `radius` metres of its `Transform` the pieces of an owner other than `owner` cannot be built (while `exclusive`), and the pieces decay no more while `protect`
    /// is above 0 (it runs down by a second each second; the game fills it from what the player puts in, so many seconds of upkeep for so much wood). `covered` is how many pieces it covers (the engine's: a
    /// game charges by it).
    pub struct BuildUpkeep {
        pub owner: u32 = 0,
        pub radius: f32 = 40.0,
        pub protect: f32 = 0.0,
        pub exclusive: bool = true,
        pub covered: u32 = 0,
    }
}

component! {
    /// A helicopter (docs/HELICOPTER.md), on a rigid body (a `Collider`, a `RigidBody`, a `Velocity` and an `AngularVelocity`): the rotors lift it along its own up axis, tilting it moves it, the pedals turn it, it is
    /// loud, and it is a crash risk. **Inputs** (a graph system or the game's controls set them): `engine` (rotors on), `collective` -1 to 1 (up is climb; 0 hovers when `lift` is twice gravity, -1 is no lift, 1 is
    /// full), `pitch` -1 to 1 (1 tips the nose down: forward), `roll` -1 to 1 (1 tips to the right) and `yaw` -1 to 1 (1 turns right); `pilot` is the uid of whoever flies it (their `Heat` is raised while the rotors are
    /// up). **The machine**: `lift` (m/s^2 at full collective and full rotor speed), `spool_up` and `spool_down` (seconds for the rotors to get to speed and to stop), `max_tilt` (degrees) and `tilt_rate`
    /// (degrees a second the attitude follows the stick at), `yaw_rate` (degrees a second at full pedal), `assist` 0 to 1 (how much of the lift lost to tilt is given back, so that a tipped helicopter does not
    /// sink), `drag` and `climb_drag` (1/s, sideways and up and down), `ground_effect` (the extra share of lift near the ground, fading out over `ground_height` metres). **Attention**: `noise` is how far (metres)
    /// the rotors are heard at full power by an AI's `Perception` (the engine keeps a `Noisy` on the helicopter at that times the rotor speed, between `idle_noise` and 1 by how hard the rotors work) and `heat` is how
    /// much heat a second the pilot's `Heat` gains at full noise: flying draws the machines. **Crash**: a sudden loss of speed above `crash_speed` (m/s) costs `crash_damage` hit points for every m/s over it, and
    /// the event `heli_crash` (the hit points reaching 0 stops the engine). **Parts**: `rotor` and `tail_rotor` name entities whose rotation the engine sets (the main one turns about its local y, the tail one about
    /// its local x, `rotor_rps` turns a second at full rotor speed, the tail 5 times that). **Outputs**: `spool` (0 to 1, rotor speed), `speed` (m/s), `altitude` (metres above what is under it, -1 if nothing within 300 m),
    /// `tilt` (nose down and right side down, degrees), `heading` (degrees, a quarter turn to the left is positive), `rotor_angle` (radians).
    pub struct Helicopter {
        pub engine: bool = true,
        pub collective: f32 = 0.0,
        pub pitch: f32 = 0.0,
        pub roll: f32 = 0.0,
        pub yaw: f32 = 0.0,
        pub pilot: Uid = Uid(0),
        pub lift: f32 = 19.6,
        pub spool_up: f32 = 6.0,
        pub spool_down: f32 = 10.0,
        pub max_tilt: f32 = 30.0,
        pub tilt_rate: f32 = 70.0,
        pub yaw_rate: f32 = 80.0,
        pub assist: f32 = 1.0,
        pub drag: f32 = 0.15,
        pub climb_drag: f32 = 0.8,
        pub ground_effect: f32 = 0.2,
        pub ground_height: f32 = 6.0,
        pub noise: f32 = 160.0,
        pub idle_noise: f32 = 0.35,
        pub heat: f32 = 0.0,
        pub crash_speed: f32 = 8.0,
        pub crash_damage: f32 = 12.0,
        pub rotor: Uid = Uid(0),
        pub tail_rotor: Uid = Uid(0),
        pub rotor_rps: f32 = 6.0,
        pub spool: f32 = 0.0,
        pub speed: f32 = 0.0,
        pub altitude: f32 = 0.0,
        pub tilt: [f32; 2] = [0.0, 0.0],
        pub heading: f32 = 0.0,
        pub rotor_angle: f32 = 0.0,
        pub last_v: [f32; 3] = [0.0, 0.0, 0.0],
    }
}

component! {
    /// What a character's hands say to the thing it drives (docs/SEATS.md): four `axes`, each -1 to 1 (for a helicopter: 0 roll, right is positive; 1 collective, up is positive; 2 pitch, forward is positive; 3 yaw, right is
    /// positive) and a set of `buttons` (bits the game defines). A game's graph system writes them from the keys (`{"set": "Controls.axes[1]", "to": ...}`); a game server writes them from what the player's computer says
    /// (`NetPlayer.axes`). The engine only reads them: a `Seat` with `drive` 1 flies its `Helicopter` from the sitter's.
    pub struct Controls {
        pub axes: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        pub buttons: u32 = 0,
    }
}

component! {
    /// A place in a vehicle (docs/SEATS.md), on the vehicle (or on a child of it, for a second seat) beside an `Interactable` of kind 0 (`prompt` "Enter"): a character whose `Interactor` uses it sits down while nobody does, and
    /// is carried with the vehicle at `offset` (in the seat entity's own axes); using again (the use button pressed once more: a press that sat it down does not count) gets it out at `exit` (own axes of the seat entity).
    /// `drive` 0 is a passenger's seat, 1 the pilot's: the entity's `Helicopter` is flown from the sitter's `Controls` (and its `pilot`, `engine` set; leaving puts the stick back, shuts the engine and clears the pilot), and
    /// the sitter may only get out of it within `max_exit_altitude` metres of the ground; 2 the driver's: the entity's `Vehicle` is driven from the sitter's `Controls` (`axes[2]` throttle, forward is positive; `axes[3]`
    /// steer, right is positive; `axes[1]` below 0 the brake, above 0.5 the handbrake) and the sitter may only get out of it below `max_exit_speed` m/s (the handbrake is put on when it does). `occupant` and `held` are the engine's. Events: `seat_enter`, `seat_exit` (on the seat's entity; also when the sitter dies or is gone) and
    /// `seat_denied` (the helicopter was too high to get out of). The sitter has a `Seated` component for as long as it sits.
    pub struct Seat {
        pub offset: [f32; 3] = [0.0, 0.0, 0.0],
        pub exit: [f32; 3] = [2.0, 0.0, 0.0],
        pub drive: u32 = 0,
        pub max_exit_altitude: f32 = 3.0,
        pub max_exit_speed: f32 = 4.0,
        pub occupant: Uid = Uid(0),
        pub held: bool = false,
    }
}

component! {
    /// Guns that are items (docs/LOADOUT.md), on a character or on the entity that carries its `Weapon` (a head): `guns` is a text, one gun a line, `gun ITEM key=value ...` (`ammo` the item of the ammunition, empty: it never
    /// runs out; `damage`, `rate`, `mag`, `reload`, `spread`, `range`, `auto`, `pellets`, `noise`, `head`, `limb`, `falloff`, `falloff_min`, `kind`, `speed`, `recoil`, `recoil_side`, `bloom`, `aim`, `aim_recoil`); `held` is the slot of the bag in hand (the game
    /// sets it: the hotbar, or a player's own number). The `Weapon` (`weapon`: its entity, 0 for this one) is the gun that slot holds, or switched off when it holds no gun; the bag is `inventory`'s (0: this entity's, or its parent's).
    /// The engine's: `holding` (the item), `slot`, `reserve` (the ammunition in the bag as the weapon saw it), `mags` (the magazine each slot's gun had when it was last held; the biggest number: never held), `took` (rounds taken out of the bag). `aiming` is the player's aim button (a game server sets it from the avatar's `NetAvatar.aim_mask`; a game may set it itself): while it is on, the gun's `aim` multiplies its `spread` and `bloom` and `aim_recoil` its kick. `aim_applied` is the engine's.
    pub struct Loadout {
        pub guns: Sym = Sym(0),
        pub weapon: Uid = Uid(0),
        pub inventory: Uid = Uid(0),
        pub held: u32 = 0,
        pub holding: u32 = 0,
        pub slot: u32 = 0,
        pub reserve: u32 = 0,
        pub took: u32 = 0,
        pub mags: [u32; 48] = [u32::MAX; 48],
        pub aiming: bool = false,
        pub aim_applied: bool = false,
    }
}

component! {
    /// The engine's mark on a character that sits in a seat (docs/SEATS.md): `seat` is the uid of the entity with the `Seat`. A game stops the walking of a seated character (a graph system `each: ["Seated"]`, or
    /// reads the component in its controls); a game server carries the player with the seat and ignores the place its computer reports.
    pub struct Seated {
        pub seat: Uid = Uid(0),
    }
}

component! {
    /// A player's life cycle (docs/RESPAWN.md), beside its `HitPoints`: when they reach 0 the player is **dead**: a corpse is made where they fell from the prefab `corpse` (the game's: a body, a bag) holding
    /// a copy of the player's `Inventory` (and removed after `corpse_time` seconds; 0 keeps it), the player's own inventory is emptied (unless `keep_items`), and after `delay` seconds the player **respawns**
    /// (if `hold`, only once the game has set `request`: a death screen with a button): back to full `HitPoints` with `protect` seconds of grace, fed to `food` percent, at one of the `SpawnPoint`s: the
    /// player's own (a `SpawnPoint` of the same non-zero `group`: a sleeping bag) if there is one, else a shared one (group 0) that is at least `avoid` metres from every other living player (or, if none
    /// is, the one farthest from them). Without any `SpawnPoint` the player gets up where it fell. `state` (0 alive, 1 dead), `timer` (seconds to go), `deaths`, `corpse_uid` and `died_at` are the engine's.
    /// Events: `corpse` (the corpse, at its place) and `respawned` (the player, at the spawn point).
    pub struct Respawn {
        pub delay: f32 = 5.0,
        pub hold: bool = false,
        pub request: bool = false,
        pub corpse: Sym = Sym(0),
        pub corpse_time: f32 = 300.0,
        pub keep_items: bool = false,
        pub group: u32 = 0,
        pub avoid: f32 = 80.0,
        pub food: f32 = 80.0,
        pub protect: f32 = 3.0,
        pub state: u32 = 0,
        pub timer: f32 = 0.0,
        pub deaths: u32 = 0,
        pub corpse_uid: Uid = Uid(0),
        pub died_at: [f32; 3] = [0.0, 0.0, 0.0],
    }
}

component! {
    /// A place where a player (with a `Respawn`) can start again (docs/RESPAWN.md): the entity's `Transform` is where the feet go and which way the player faces. `group` 0 is a shared point; a non-zero
    /// `group` belongs to the players whose `Respawn.group` is the same (a sleeping bag or a bed, which a game puts down and gives the player's number). `radius` scatters the player over a disc round it.
    /// `once` uses the point up (it goes inactive, a bag is used once). `active` false switches it off. `uses` counts, the engine's.
    pub struct SpawnPoint {
        pub group: u32 = 0,
        pub radius: f32 = 0.0,
        pub active: bool = true,
        pub once: bool = false,
        pub uses: u32 = 0,
    }
}

component! {
    /// A flying machine's mind (docs/DRONES.md), on an entity with a `Transform` and a `Velocity` (it is moved by setting its velocity, like a `Steer`; no physics body needed): it holds `hover` metres above
    /// whatever is under it, goes where it is told, steers round what is in its way and away from the other `FlyAgent`s, and follows or circles a target. **`mode`**: 0 hold (hover where it is), 1 go to `goal`,
    /// 2 follow `target` (an entity's uid) and stay `range` metres from it, 3 circle `target` at `range` metres with a speed of `orbit` m/s round it (negative the other way), 4 patrol the points of `patrol`
    /// (text, one `x y z` a line; only x and z are used, the height is `hover`), waiting `wait` seconds at each. With **`perception`** on, its own `Perception` chooses: alert (`state` 2) it circles the target
    /// it sees, searching (3) it goes to where it was last seen, otherwise it patrols (or holds without a patrol); the `Perception`'s own autopilot (`engage`) turns it to the target and pulls its `Weapon`'s trigger. **The body**:
    /// `speed` (m/s), `accel` (m/s^2), `climb` (m/s up or down), `turn_rate` (degrees a second it turns to face the way it goes while it is not engaged; `face` off leaves its rotation alone), `avoid` (the distance
    /// ahead it looks for obstacles, metres; 0 switches avoidance off), `separation` (the distance it keeps from other agents). **Outputs**: `arrived` (at the goal or the point it was patrolling to), `altitude` (metres
    /// above what is under it, -1 if nothing within 500 m), `blocked` (something is in its way), `patrol_at` and `wait_left` (the patrol's state), `speed_now` (m/s).
    pub struct FlyAgent {
        pub active: bool = true,
        pub mode: u32 = 0,
        pub goal: [f32; 3] = [0.0, 0.0, 0.0],
        pub target: Uid = Uid(0),
        pub range: f32 = 12.0,
        pub orbit: f32 = 0.0,
        pub perception: bool = false,
        pub patrol: Sym = Sym(0),
        pub wait: f32 = 1.0,
        pub speed: f32 = 12.0,
        pub accel: f32 = 10.0,
        pub climb: f32 = 6.0,
        pub turn_rate: f32 = 180.0,
        pub face: bool = true,
        pub hover: f32 = 6.0,
        pub avoid: f32 = 8.0,
        pub separation: f32 = 5.0,
        pub arrived: bool = false,
        pub altitude: f32 = 0.0,
        pub blocked: bool = false,
        pub patrol_at: u32 = 0,
        pub wait_left: f32 = 0.0,
        pub speed_now: f32 = 0.0,
    }
}

component! {
    /// Something a character can use (docs/INTERACT.md): a door, a chest, an item on the ground, a switch. An `Interactor` that looks at it from within `range` metres (the shape it is aimed at is its `Collider`, or a
    /// sphere of `radius` metres if it has none) and acts uses it. **`kind`**: 1 a door (the entity also has a `Door`: using it opens or shuts it), 2 a container (an `Inventory` beside it: using it opens it, `user` is
    /// the character who has it open until they use it again or walk off; the game's screen shows its inventory), 3 an item to pick up (`item` is the text `ITEM N`: it goes into the user's `Inventory` and the thing is
    /// removed, or stays with what did not fit), 0 anything else (only the event `used`: the game's scripts do the rest). `prompt` is the text for the screen (`Open`, `Search`). `locked` keeps it shut: a character
    /// with the item `key` in their inventory unlocks it by using it (and the key is used up with `key_consume`), others are told it is locked. `cooldown` seconds between two uses. `enabled` false hides it from
    /// everybody. `user`, `open` (a container is open), `uses` and `ready` (the tick it can be used again) are the engine's. Events: `interact` (always, on the thing), `door_open`, `door_close`, `container_open`, `container_close`, `picked`, `unlocked`, `locked`, `inventory_full`, `used`.
    pub struct Interactable {
        pub kind: u32 = 0,
        pub prompt: Sym = Sym(0),
        pub range: f32 = 3.0,
        pub radius: f32 = 0.6,
        pub enabled: bool = true,
        pub locked: bool = false,
        pub key: Sym = Sym(0),
        pub key_consume: bool = false,
        pub item: Sym = Sym(0),
        pub cooldown: f32 = 0.3,
        pub ready: u32 = 0,
        pub user: Uid = Uid(0),
        pub open: bool = false,
        pub uses: u32 = 0,
    }
}

component! {
    /// A door that swings (docs/INTERACT.md): its entity (a box with a `Collider`, a model, a leaf) turns about the hinge when `open` is set (by an `Interactable` of kind 1, or by a script) and back when it is cleared.
    /// `hinge` is the hinge's place in the door's own axes from its origin (a door one metre wide with its origin in the middle has its hinge at `[-0.5, 0, 0]` or `[0.5, 0, 0]`), `axis` the way the hinge runs in the same axes
    /// (up, `[0, 1, 0]`, by default), `angle_open` how far it swings in degrees (negative the other way) and `speed` how fast, degrees a second. The pose the door has when the scene starts is its shut pose: put it
    /// shut. With a `Parent` and a `LocalTransform` the swing is done on the local pose. `angle` (the engine's) is where it is now.
    pub struct Door {
        pub open: bool = false,
        pub hinge: [f32; 3] = [0.0, 0.0, 0.0],
        pub axis: [f32; 3] = [0.0, 1.0, 0.0],
        pub angle_open: f32 = 100.0,
        pub speed: f32 = 160.0,
        pub angle: f32 = 0.0,
        pub based: bool = false,
        pub base_pos: [f32; 3] = [0.0, 0.0, 0.0],
        pub base_rot: [f32; 4] = [0.0, 0.0, 0.0, 1.0],
    }
}

component! {
    /// The use button of a character (docs/INTERACT.md): it looks along its entity's local -z from its place (`eye` is an offset in the entity's own axes; put it on the camera or the avatar's head) `reach` metres,
    /// and `act` (held down by the game or the player's button: a press counts once) uses what it looks at. `inventory` is whose `Inventory` holds the keys and receives what is picked up (0: this entity's, else its
    /// parent's). With `look` (on by default) it finds what it is looking at every tick, for the prompt on the screen: `target` (a uid, 0 for nothing), `prompt` (its text), `kind`, `distance`; without it (a server's
    /// avatar) it looks only when it acts. `used` is the uid of what was used this tick, `denied` the reason one was refused (`locked`, `full`), both for one tick; `open` is the container it has open (a uid) and `held` the button's last state: the engine's.
    pub struct Interactor {
        pub reach: f32 = 3.0,
        pub eye: [f32; 3] = [0.0, 0.0, 0.0],
        pub act: bool = false,
        pub inventory: Uid = Uid(0),
        pub look: bool = true,
        pub target: Uid = Uid(0),
        pub prompt: Sym = Sym(0),
        pub kind: u32 = 0,
        pub distance: f32 = 0.0,
        pub used: Uid = Uid(0),
        pub denied: Sym = Sym(0),
        pub held: bool = false,
        pub open: Uid = Uid(0),
    }
}

component! {
    /// How hard a character is to notice (docs/STEALTH.md), beside a `Transform`: the engine measures how fast it moves and keeps a `Noisy` on it (the noise radius an AI hears: still 0, walking `walk_noise` metres, running
    /// `run_noise`, sprinting `sprint_noise`, between the speeds `walk_speed`, `run_speed` and `sprint_speed` in a straight line; crouched, `crouch_mult` times that) and a `visibility` (0 to 1: how much of its sight range an
    /// AI's eyes have for it: crouched `vis_crouch`, standing still `still_mult` of that, and less in the dark: `min_light` of it with `light` 0). **Inputs**: `crouch` and `light` (0 dark to 1 bright: a game sets it from
    /// the time of day or a flashlight). **Outputs**: `speed` (metres a second, smoothed), `noise` (metres), `visibility`.
    pub struct Stealth {
        pub crouch: bool = false,
        pub light: f32 = 1.0,
        pub walk_noise: f32 = 5.0,
        pub run_noise: f32 = 14.0,
        pub sprint_noise: f32 = 22.0,
        pub crouch_mult: f32 = 0.3,
        pub walk_speed: f32 = 2.0,
        pub run_speed: f32 = 5.0,
        pub sprint_speed: f32 = 8.0,
        pub vis_crouch: f32 = 0.6,
        pub still_mult: f32 = 0.7,
        pub min_light: f32 = 0.4,
        pub speed: f32 = 0.0,
        pub noise: f32 = 0.0,
        pub visibility: f32 = 1.0,
        pub last: [f32; 3] = [0.0, 0.0, 0.0],
        pub seen: bool = false,
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

component! {
    /// The weather of the picture, for the GPU renderer (the first `Weather` counts; the reference renderer ignores it, and so do the classic modes). `rain` (0..1) is how hard it rains: up to 36,000 drops, stretched along
    /// their fall (a little aslant), in a box of 40 x 22 x 40 m round the camera, drawn by the card alone (the vertex shader places each drop from its number and the clock; a `Particles` cloud of 7,000 drops cost
    /// 6 ms of CPU a picture). Drops keep their place in the world while the camera moves, so they have parallax, and the depth test hides them behind what stands in front. A game puts the rain out indoors by setting it to 0 there. `wet` (0..1) wets the world in the high qualities: flat surfaces darken and turn glossy, in
    /// patches (puddles), walls a little, so that lamps and sky shine in the street after rain; a game raises it while it rains and lets it fall slowly afterwards. Neither is read by the simulation.
    pub struct Weather {
        pub rain: f32 = 0.0,
        pub wet: f32 = 0.0,
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
    /// (see `check`) leaves the terrain drawn with its own texture. In the GPU renderer's high qualities a terrain that also has a `Terrain.texture` (a photograph of the ground) takes its colour from the photograph and
    /// only its detail from the layers: `photo` (0 to 1, default 0.85) is how much of the colour is the photograph's (1: all of it; the layers then only give the detail and the `cliff` layer's sideways rock).
    pub struct TerrainLayers {
        pub splat: Sym = Sym(0),
        pub layers: Sym = Sym(0),
        pub photo: f32 = 0.85,
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

component! {
    /// A body that rides the water properly (docs/TERRAIN.md, "Floaters and waves"; beside a `Collider`, a `RigidBody` and a `Velocity`, and an `AngularVelocity` if it should tip): a boat, a raft, a log, a crate, a ball.
    /// Where `Buoyant` pushes a body up from its middle on a flat surface, a `Floater` is lifted by `columns` x `columns` columns of its own volume, each by the water at its own place: it **rolls and pitches with the
    /// waves** of a `Water` sheet or a `WaterBody` (the same ripples that are drawn, `waves` 0 to 1 says how much they count), **rights itself** when it is tipped (the lift of the low side is bigger), is **carried by a
    /// `WaterFlow`** (`current` 0 to 1) and slowed by the water it moves through (`drag`, `angular_drag`: per second at full depth). `density` is in kilograms per cubic metre (500 wood, 920 ice, 1000 neutral, 2500 stone).
    /// The engine writes `submerged` (0 to 1, the share of the body under the surface) and `splash` (the speed in metres per second at which the body hit the water on this tick, otherwise 0) every tick, and the event
    /// `splash` (on the body, at the point on the surface above it) when that is faster than 1 m/s; with `splash_fx` (on by default) that also throws up spray and mist, two short-lived `Particles` entities that remove themselves after a second and a half. A body is looked at only when it is near the water: a thousand crates in a desert cost a rectangle test each.
    pub struct Floater {
        pub density: f32 = 500.0,
        pub columns: u32 = 3,
        pub drag: f32 = 1.2,
        pub angular_drag: f32 = 3.0,
        pub waves: f32 = 1.0,
        pub current: f32 = 1.0,
        pub splash_fx: bool = true,
        pub submerged: f32 = 0.0,
        pub splash: f32 = 0.0,
    }
}

component! {
    /// Lets characters swim in a flat `Water` sheet (beside the `Water` and its `Transform`): without it a sheet is only drawn (and floated on by `Floater`s) and characters walk through it, as they always did. With it a
    /// `CharacterBody` whose feet are under the sheet's level inside its rectangle gets an `InWater` (`body` is the sheet's entity) and a character with a `CharacterMotion` wades and swims in it exactly as in a
    /// `WaterBody` (docs/GAMEPLAY.md, "Character motion"). The level is the sheet's flat height: its waves are drawn, not part of the character's swimming. `enabled` false turns it off without removing it.
    pub struct Swimmable {
        pub enabled: bool = true,
    }
}

component! {
    /// More to a surface than a base colour, a metal/rough number and maps (the GPU renderer's high qualities; beside a `MeshRenderer`; the reference picture and classic mode ignore it). **`clearcoat`** (0 to 1) puts a second, smooth
    /// lobe over the surface, as the lacquer on a car, a lacquered table or wet paint has: a sharp highlight and a mirror of the sky over a duller base that it also dims a little (`clearcoat_roughness` 0.02 to 1, 0.05 glass-smooth).
    /// **`sheen`** (0 to 1) is the soft glow at grazing angles of velvet, felt, satin and fuzzy leaves, tinted a little by the base colour. **`subsurface`** (0 to 1) lets light wrap round the terminator and bleed through, in
    /// `subsurface_color` (reddish for skin, wax and ears held up to the sun; yellow-green for leaves): the shaded side of a lit cheek is warm, not dead. All three are cheap (a few operations a light) and cost nothing for a surface without
    /// the component.
    pub struct MaterialExt {
        pub clearcoat: f32 = 0.0,
        pub clearcoat_roughness: f32 = 0.05,
        pub sheen: f32 = 0.0,
        pub subsurface: f32 = 0.0,
        pub subsurface_color: [f32; 3] = [1.0, 0.35, 0.25],
    }
}

component! {
    /// A current in the water of this entity (beside a `Water` sheet or a `WaterBody`): the water moves at `velocity` (metres per second in x and z). `Floater` bodies on it are carried along (and a body held still feels
    /// the push as drag); nothing else is moved by it. A river that follows a heightmap flows one way over all of it: put one `WaterFlow` on each stretch you want to run its own way.
    pub struct WaterFlow {
        pub velocity: [f32; 2] = [0.0, 0.0],
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
    /// the camera out again when the way is free (`CameraRig.follow_speed`), so a camera that is pulled in does not snap back. With `recover` above 0 the camera is pulled in at once but goes back out at that rate
    /// (per second, exponentially: 4 takes about three quarters of a second), whatever the rig does: a camera that slides along a wall or passes a pillar does not pop in and out. `held` is the distance the camera is
    /// held at while it comes back (written by the engine, 0 when it is free). `pull_in` above 0 eases the pull-in as well (per second: 15 covers half of a step in three ticks), but never lets the camera
    /// come deeper into the `radius` margin than 0.7 of it, so it cannot go through the wall: a small step of the line's end (an edge crossing it) is rounded off, a big one is still taken at once.
    /// `whisker` above 0 (metres) adds four more lines, to the sides and above and below the camera's place at that distance (A Hat in Time's camera whiskers): an obstacle that is not on the middle line yet but is
    /// within a whisker of it pulls the camera in a little early (the distance it hits, `radius` less, plus the whisker: one metre of pull for each metre it is from the middle), so a wall that an edge of crosses the line
    /// is met in two smaller steps instead of one big one.
    pub struct CameraCollide {
        pub radius: f32 = 0.35,
        pub min_distance: f32 = 0.9,
        pub recover: f32 = 0.0,
        pub held: f32 = 0.0,
        pub pull_in: f32 = 0.0,
        pub whisker: f32 = 0.0,
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
    /// attachments"). `target` is the entity of the skinned model (a `MeshRenderer` of a rigged `.glb` with an `Animator`; 0: the model the entity is a child of, so that a prefab can carry its own attachments, like an `Ik` goal with no `skin`), `joint` the name of the joint in that model (`model_info` lists them), `pos`, `rot` (a quaternion
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
    /// good start). `untile` (also on its own, with no maps) breaks up the repetition of the texture and the maps: each point of the surface is a blend of three copies of them at random offsets over a hexagonal grid (Mikkelsen's
    /// hex-tiling, docs/GPU.md, "No two places alike"), so a floor or a wall of `uv_scale` 20 shows no pattern that comes round again; leave it off for a pattern that is meant to repeat (bricks laid in rows). New in the engine's standard set; a world without it is what it was.
    pub struct PbrMaps {
        pub normal: Sym = Sym(0),
        pub arm: Sym = Sym(0),
        pub height: Sym = Sym(0),
        pub height_scale: f32 = 0.03,
        pub normal_strength: f32 = 1.0,
        pub ao_strength: f32 = 1.0,
        pub roughness_scale: f32 = 1.0,
        pub untile: bool = false,
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
    /// **Blades.** With `mesh` empty (the default) the field is made of single blades, as in the shipped games' grass (docs/GPU.md, "Grass made of blades"): each blade is a curved ribbon (a cubic curve, a taper to a point, 15 vertices
    /// near the camera and 5 far) placed, sized, coloured and bent by hashes, with the shape and the colour shared by the blades of a clump (`clump` 0 to 1 how much, `clump_size` metres across) so that the field has tall and short, dark and pale
    /// patches and a lean in a common direction, not noise. `height` is the range of the blades' heights in metres, `blade_width` their width at the foot, `bend` how far they lean over (0 stiff, 1 drooping), `base_color` and `tip_color`
    /// their colours at the foot and the tip (linear light; the foot is darker than its colour by the ambient occlusion of a dense field), `density` how many clumps of four blades per square metre near the camera. Normals are bent
    /// across the width so the blades look round, blades seen edge-on are widened a little, the sun shines through them (and is shadowed where something stands in its way) and the far blades are wider, so that a distant
    /// field is as full as a near one. `mask_layer` (0 off; 1 to 4) makes the grass grow only where that layer of the terrain's `TerrainLayers` is painted (its density follows the splat map's weight, and the blades are
    /// shorter where it is thin), as the shipped games grow each kind of grass on its own kind of ground: none on the rock or the sand. Give `mesh` a model and the old way is used: that model is the tuft.
    /// `terrain` is the entity with the `Terrain` it grows on; 0 means the `Terrain` on this same entity (what a prefab needs: a streamed cell of the world is a prefab, and the entities of a prefab cannot name each other),
    /// or, when this entity has none, the first terrain in the scene.
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
        pub height: [f32; 2] = [0.25, 0.55],
        pub blade_width: f32 = 0.035,
        pub clump: f32 = 0.8,
        pub clump_size: f32 = 1.1,
        pub bend: f32 = 0.55,
        pub base_color: [f32; 3] = [0.03, 0.07, 0.012],
        pub tip_color: [f32; 3] = [0.2, 0.32, 0.07],
        pub mask_layer: u32 = 0,
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
    /// Fill, bounce and lamp falloff (the GPU renderer's high qualities; the first enabled `Lighting` counts; the reference picture and classic mode ignore it; a world without it is lit exactly as before). What the sun,
    /// the sky and the lamps leave dark gets a little light instead of black, and lamps fall off the way light does: `fill` is the lowest light a surface gets from the sky whatever the hour (a colour, linear, before the
    /// exposure: a night sky's glow and the glow of a city; a street at midnight is not pitch black), `bounce` is the share of the sun's (or the moon's) light that comes back as ambient light from the ground and the walls
    /// (it lifts the shadowed side of a wall and the floor of a street that no direct light reaches; 0 off), `lamp_falloff` blends the lamps' old falloff (0: a broad pool that fades smoothly to nothing at its range) into
    /// inverse-square light (1: a bright core and a long tail, as a real lamp), `lamp_core` is the size of the core as a share of the range (0.12), `shadow_distance` pushes the sun's cascades out to at least that many
    /// metres (the `Environment`'s own 80 is kept when it is the larger; 0 leaves it), and `softness` is the sun's apparent size for soft shadows when the world has no `Shadows` component of its own (see `Shadows`; 0
    /// the plain filter), and `point_shadows` is how many point or spot lights cast shadows (0 to 4; the renderer's own is the two that matter most: each costs six faces of a shadow map, 0.3 to 0.7 ms of a GTX 1070 in a street, and 4 MB a face at 1024).
    /// A new component.
    pub struct Lighting {
        pub fill: [f32; 3] = [0.02, 0.025, 0.04],
        pub bounce: f32 = 0.18,
        pub lamp_falloff: f32 = 1.0,
        pub lamp_core: f32 = 0.12,
        pub shadow_distance: f32 = 200.0,
        pub softness: f32 = 0.03,
        pub point_shadows: u32 = 4,
        pub enabled: bool = true,
    }
}

component! {
    /// Macro variation of a surface (the GPU renderer's high qualities; beside a `MeshRenderer`; the reference picture and classic mode ignore it). A wall, a floor or a road that repeats one small texture shows the repetition
    /// from afar whatever `PbrMaps.untile` does to the small picture: the same grey over a whole street. The engine multiplies the colour by smooth noise made from the **world position** (so it needs no UV layout, and the next
    /// piece of the same material carries on the same patches) at three scales: `scale` metres (the biggest patches), a fifth of that and a twenty-fifth. `strength` (0 to 1) is how much the brightness varies (0.25: about a tenth
    /// either way in most places, a third at the extremes), `tint` (0 to 1) leans the colour warm in some patches and cool in others, `rough` (0 to 1) makes the darker patches rougher and the lighter ones smoother. All three at 0
    /// draw the surface as it was. The noise is still (it does not move or flicker). It costs a few noise lookups a pixel on a surface that has it and nothing on the others (docs/GPU.md, "No two places alike").
    pub struct MacroVary {
        pub strength: f32 = 0.25,
        pub scale: f32 = 24.0,
        pub tint: f32 = 0.3,
        pub rough: f32 = 0.3,
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

component! {
    /// How deep water looks (put it beside a `Water` or a `WaterBody`; the GPU renderer's high qualities with the depth prepass: ambient occlusion, bounced light, anti-aliasing or this; the reference picture and classic mode draw the
    /// water as before): the water's `color` is what it is in the deep, and where the bottom or the shore is near it is `shallow`, because the picture knows how far the water is from what is behind it. `clarity` is how many
    /// metres of water it takes to hide what is under it (about two thirds hidden at that depth); the thinner the water the more of the bottom shows through, in the colour of the shallows. `foam` (0 to 1) is how much white
    /// the waves leave where the water meets the shore, a rock or a post, in a band `foam_width` metres wide, broken up by a noise that moves with the waves. A new component.
    pub struct WaterLook {
        pub clarity: f32 = 3.0,
        pub shallow: [f32; 3] = [0.22, 0.62, 0.58],
        pub foam: f32 = 0.7,
        pub foam_width: f32 = 0.7,
    }
}

component! {
    /// A world made on demand (`procgen`, docs/REALWORLD.md): a city whose every street, building and room is generated as the camera comes near, from a seed, so that its size is not a number of entities. The program's provider says which
    /// `StreamZone`s each cell of a grid has; this entity keeps the zones of the cells near the camera alive. A cell is a square of `cell` metres on the ground (cell (i, j) covers x from i `cell` to (i + 1) `cell`, z the same); the cells whose
    /// middle is within `radius` metres of the camera have their zones (at most `per_tick` cells a tick, nearest first), and a cell beyond `radius` plus `hysteresis` loses them again, and with them whatever they had loaded. `grid` is the name given
    /// to the provider; `extent` (0 = none) is the half width of the map in metres: there are no cells beyond it.
    pub struct ProcGrid {
        pub grid: Sym = Sym(0),
        pub cell: f32 = 400.0,
        pub radius: f32 = 3000.0,
        pub hysteresis: f32 = 400.0,
        pub extent: f32 = 0.0,
        pub per_tick: u32 = 2,
    }
}

component! {
    /// Marks a `StreamZone` that a `ProcGrid` made: the grid's entity, and the cell (i, j) it belongs to. The engine's.
    pub struct GridZone {
        pub grid: Uid = Uid(0),
        pub i: f32 = 0.0,
        pub j: f32 = 0.0,
    }
}

component! {
    /// An evolved creature (docs/CREATURES.md): a spider whose body and brain were trained together on the GPU and that walks to a target over any ground. `brain` is a file of the workspace (a `champions.json` of the trainer,
    /// or one genome) and `genome` says which of its programs; `model` is the file of another body (empty: the built-in spider); `body_seed` makes this animal's own body (its strength, weight, grip and stiffness differ
    /// a little from the next one's). It walks to the entity `target` (the nearest `Terrain` or the flat ground at its own height is the ground), or to the point `goal` (x, y, z) when `target` is 0. It is placed where its
    /// own `Transform` is, facing the way its x axis points; from then on the `Transform` follows its prosoma. The engine spawns 50 `CreaturePart` entities that draw it (turn `visible` off to draw it yourself, following
    /// `CreaturePart.index`) and writes, every tick, `distance` (metres to the target), `speed` (metres per second along the ground), `height` (of the prosoma above the ground), `upright` (1 standing, below 0.3 fallen over)
    /// and `feet` (how many touch the ground), `web` (a silk thread is held: the spider has shot it at a surface and pulls on it) and `tension` (newtons), `eaten` (how many foods it has eaten). `size` is how big it is against the animal it was trained as (1: a spider 1.5 m across; 0.25: a big tarantula, with every length and mass scaled and its clock sped up to match, so that it walks the same way, at half the speed). With `forage` true it chooses its own target, the nearest `Food` (and writes it to `target`). `enabled` false freezes it. A foraging creature that has not got closer to its food for `patience` seconds (20; 0 never) gives up on that food for three times as long and goes for the nearest other one; `stuck` (seconds without progress), `best` (the closest it has been), `given_up` and `give_up_left` are what it remembers for that, written by the engine. Like a real spider it moves in bursts: after it eats it **rests** for about `rest` seconds (0: never), while it walks it stops now and then to **scan** (every `scan_every` seconds on average for about `scan_for` seconds; 0: never), and while `alarm` is above 0 (a game sets it when something startles the creature; it counts down) it **freezes**; during all three it stands with all eight legs down and its brain waits (`pause_left` and `walked` are what the engine remembers for the first two).
    pub struct Creature {
        pub brain: Sym = Sym(0),
        pub model: Sym = Sym(0),
        pub genome: u32 = 0,
        pub body_seed: u32 = 1,
        pub target: Uid = Uid(0),
        pub goal: [f32; 3] = [0.0, 0.0, 0.0],
        pub enabled: bool = true,
        pub visible: bool = true,
        pub forage: bool = false,
        pub size: f32 = 1.0,
        pub distance: f32 = 0.0,
        pub speed: f32 = 0.0,
        pub height: f32 = 0.0,
        pub upright: f32 = 1.0,
        pub feet: u32 = 8,
        pub web: bool = false,
        pub tension: f32 = 0.0,
        pub eaten: u32 = 0,
        pub patience: f32 = 20.0,
        pub rest: f32 = 30.0,
        pub scan_every: f32 = 12.0,
        pub scan_for: f32 = 1.5,
        pub alarm: f32 = 0.0,
        pub pause_left: f32 = 0.0,
        pub walked: f32 = 0.0,
        pub stuck: f32 = 0.0,
        pub best: f32 = 0.0,
        pub given_up: Uid = Uid(0),
        pub give_up_left: f32 = 0.0,
    }
}

component! {
    /// Something a foraging `Creature` (`forage` true) goes to and eats: the creature walks to the nearest `Food`, and when it is there the food appears again at one of the `FoodSpot`s
    /// (never beside the creature), so that the animal has to travel, climb and web its way around the place it lives in. `eaten` counts how many times it was eaten.
    pub struct Food {
        pub value: f32 = 1.0,
        pub eaten: u32 = 0,
    }
}

component! {
    /// A place where `Food` can appear (the `Transform` is the place; `weight` is not used yet).
    pub struct FoodSpot {
        pub weight: f32 = 1.0,
    }
}

component! {
    /// One of the pieces that draw a `Creature` (made and moved by the engine): `owner` is the creature's entity, `index` the body (0 the prosoma, 1 the abdomen, then the six segments of each of the eight legs; 50 and up
    /// are the drawn-on details, 200 is the silk thread).
    pub struct CreaturePart {
        pub owner: Uid = Uid(0),
        pub index: u32 = 0,
    }
}

component! {
    /// A particle system (docs/FX.md): the engine's answer to Niagara. `effect` is the **description of the effect**, a JSON document of emitters (how particles are born, what they start with, forces, noise and colliders, size, colour and speed over their life, flipbook pictures, ribbons, solids, a light for each, and events that make
    /// one emitter's particles when another's die or hit something): either the name of a file of the workspace (`fx/campfire.fx.json`) or the JSON itself, when the text starts with `{`. One entity holds every emitter of an effect (a fire is flame, smoke and embers in one place), and
    /// `ParticleModules`/`Particles` go on working beside it. Unlike a `Particles` cloud the particles of a system **are stepped** (fixed 1/60 s steps from a clock), so they can collide, spawn each other and leave ribbons; the stepping happens in the picture, where the camera is, and only for what is in view (a server never does it, and
    /// the state hash never sees it). The same history (the effect, the seed and the clocks the pictures saw) gives the same particles on every machine, whatever the number of threads; a system seen for the first time at some clock starts from a steady state. `time` is the clock (it runs by itself while `playing`, `speed` times as fast; set it to 0 to play an effect that has ended again), `seed` 0 takes the entity's number (two campfires are not twins), `scale` scales its
    /// sizes and distances (the entity's own scale does too), `intensity` the rates of every emitter (0 stops it making more: the particles that live carry on), `tint` multiplies the colours, `quality` (0..1) thins out the rates and the budget for a slow machine, `repeat` (seconds, 0 never) takes the clock back to 0 whenever it gets there: an effect that is a burst (a blast, a shot) plays over and over, and when `repeat` is longer than the effect's longest life the seam is not seen. A new component.
    pub struct ParticleSystem {
        pub effect: Sym = Sym(0),
        pub playing: bool = true,
        pub time: f32 = 0.0,
        pub speed: f32 = 1.0,
        pub seed: u32 = 0,
        pub visible: bool = true,
        pub scale: f32 = 1.0,
        pub intensity: f32 = 1.0,
        pub tint: [f32; 4] = [1.0, 1.0, 1.0, 1.0],
        pub quality: f32 = 1.0,
        pub repeat: f32 = 0.0,
    }
}
