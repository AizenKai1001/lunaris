# Gameplay building blocks

Five components that are plain data, run every tick in entity-uid order inside the fixed step (so they are deterministic, hashed, recorded and replayed
like everything else), and that `check` reads for you: input actions, state machines, tweens, camera rigs and sensors. Code: `crates/lunaris-sim/src/gameplay.rs`,
tests: `gameplay_tests.rs`.

`examples/skirmish.lunaris.json` (written by `tools/build_skirmish.py`, played by `examples/tests/skirmish.test.json`) is a small game made only of these blocks and Lunaris Script: a hero that walks and fires,
grunts from three gates that hunt it over the arena, bites and bolts through contacts into `HitPoints`, a health bar, a score, a sunset and a slow-motion game over. Read it for how the blocks fit together.

## Input actions: `InputMap`

Names for what the player can do, so a game does not depend on which key does it. One entity (the lowest uid wins) holds the text:

```
jump = space, gamepad.a
move_x = d, a:-1, gamepad.lx
deadzone = 0.15
```

Each term is an `input.*` resource (`space` is `input.space`; `$score` reads the resource `score`) times an optional scale after a colon. Every tick, before the
graph systems run, the engine writes `action.NAME` (the terms added, a term under the dead zone ignored, clamped to -1..1), `action.NAME.pressed` and
`action.NAME.released` (1 on the one tick when the value went over or under a half). Graph systems, state machines and conditions then read `action.jump`.
Rebinding is editing the text. The `input` tool (or a window's key handler) sets the raw `input.*` resources.

**Controllers.** `lunaris play` (and an exported game's player) reads game controllers (Xbox-style pads: XInput on Windows, the joystick interface `/dev/input/js0` to `js3` on Linux; a visible window only, never a `--hidden` self-test) and writes `input.gamepad.NAME` for the first pad, `input.gamepad2.NAME` to `input.gamepad4.NAME` for the next ones:

| names | values |
|---|---|
| `a b x y lb rb back start ls rs dpad_up dpad_down dpad_left dpad_right` | 1 while held |
| `lx ly rx ry` (the sticks) | -1 to 1, **up and right positive** |
| `lt rt` (the triggers) | 0 to 1 |
| `connected` | 1 while the pad is plugged in; every name of a pad that goes away drops to 0 |

A stick inside a dead zone of 0.08 reads 0 and every value is rounded to 1/32, so a resting pad is silent (in a networked game every change is a message). Bind them like keys: `move_x = d, a:-1, gamepad.lx`, `move_y = w, s:-1, gamepad.ly`, `jump = space, gamepad.a`, `fire = mouse_left, gamepad.rt`. Plug-in and unplug are noticed while the game runs. `check` warns about a binding such as `gamepad.lxx` that no pad sends. **Rumble** (Windows): a graph system writes the resources `rumble.low` (the heavy motor) and `rumble.high` (the light one), each 0 to 1 (`rumble2.low` for the second pad, ...); they are output, like sound: not part of the state hash, and the motors stop when the game ends. The `lunaris-pad` crate (standard library only) is the whole reader.

## State machines: `StateMachine`

The Animator Controller of Unity and the state machine of Unreal, and a behaviour for AI, in text:

```
state idle clip=Idle
state walk clip=Walk speed=1.2
state jump clip=Jump once
idle -> walk when $action.move_x != 0 or $action.move_y != 0 fade 0.15
walk -> idle when $action.move_x == 0 and $action.move_y == 0
* -> jump when $action.jump.pressed >= 1
jump -> idle after 0.6
* -> dead when HitPoints.value <= 0
enter walk: res steps += 1, event footstep
exit dead: set HitPoints.value = 100
tick walk: set Velocity.lin[0] = 2
```

* `state NAME [clip=Clip] [speed=n] [loop|once]`: entering a state with a clip sets the entity's `Animator` (crossfade to the clip, speed, looping).
* `FROM -> TO [when CONDITION] [after SECONDS] [fade SECONDS]`: `FROM` can be `*` (any state). The first transition in the text that fits is taken, one per tick;
  `after` waits that long in the state; `fade` is the Animator crossfade for this move.
* Conditions use `==  !=  <  <=  >  >=`, `and`, `or` (`and` binds tighter), numbers, `true`/`false`, `$resource` (0 when missing) and `Component.field[i]` of this entity (0 when
  the entity lacks the component). A misspelt component or field is an error in `check`, not a silent "false".
* `enter STATE:`, `exit STATE:` and `tick STATE:` (every tick it is in the state) take actions separated by commas: `set Component.field = n` (or `+= n`), `res name = n` (or `+= n`), `event name`.
  Events are heard by the sound and graph systems like any other.
* `StateMachine.state`, `index` (the state's number in declaration order, handy in graph systems) and `time` are the engine's; `enabled` false pauses it.

For AI: a `Sensor` plus a machine gives patrol, chase and attack: `patrol -> chase when Sensor.seen != 0`, `enter chase: set NavAgent.active = 1` (a graph system can copy the seen entity's position into `NavAgent.target`).

## Blend trees and layers: `AnimLayers`

Beside an `Animator`, an `AnimLayers` lays up to two more clips over the base one. **A blend tree**: base clip `Walk`, `clip_b` `Run`, and `weight_b` going from 0 to 1 as the speed rises (a graph system, or a
state machine action such as `tick move: set AnimLayers.weight_b = 0.5`); with `sync_b` (the default) the run is kept in step with the walk so the feet agree. **A layer**: `clip_b` `Wave` with `root_b` the name of the
shoulder joint moves only that arm and what hangs from it while the legs go on walking; give it `sync_b` false and its own `speed_b`. `clip_c` / `weight_c` / `root_c` is a second extra clip. The renderer reports a clip or a joint name
the model does not have. A crossfade of the base clip (the `fade` of the Animator) still works underneath.

## Conversations: `Dialogue`

A `Dialogue` entity runs a branching conversation from text and writes the current line and the choices into the `Text` and `Ui` entities that show them. The script reads like a screenplay:

```
# the gate
== start
Guard: Halt! Who goes there?
Guard: State your business.
* [A friend] -> friend
* [Pay the toll] if $gold >= 10 -> paid
* [Nothing] -> end

== friend
res met_guard = 1
Guard: Then you may pass.
event gate_opens
-> end

== paid
res gold += -10
The guard pockets the coins.
if $met_guard == 0 -> friend
-> end
```

| Line | Meaning |
|---|---|
| `== name` | starts a node (one word; `end` is reserved). The first node is where a talk starts unless `Dialogue.start` names another. |
| `Speaker: words` | a line of speech (the name is up to 24 letters, digits, spaces, dots or apostrophes, and a space follows the colon). A line without a speaker is narration. The words may hold `{resource}` placeholders, like any `Text`. |
| `* [words] -> node` | a choice. Consecutive choices form a menu; `if CONDITION` after the `]` shows the choice only while the condition holds (`$gold >= 10`, `Health.hp < 3 or $key`: the conditions of `StateMachine`, resources start with `$`). |
| `-> node`, `-> end` | jump (`end` ends the talk). `if CONDITION -> node` jumps only when it holds, otherwise the script goes on. |
| `set Comp.field = n`, `res name = n`, `res name += n`, `event name` | actions, as in state machines, run the moment the script reaches them. |

```json
{ "Name": {"value": "Gate talk"}, "Dialogue": { "script": "== start
Guard: Halt!
* [Hi] -> end", "text_ui": 2, "speaker_ui": 3, "choice_1": 4, "choice_2": 5, "choice_3": 6, "choice_4": 7, "speed": 30 } }
```

Wiring: `text_ui` is the `Text` or `Ui` entity that shows the line (its `value` or `text`, made visible; hidden between talks), `speaker_ui` the name, `choice_1` to `choice_4` buttons for the choices on offer (a `Ui` with `button: true`; they are hidden when there are fewer choices). The question stays on screen while its choices show. **Controls**: set `active` to true to start (it goes back to false when the talk ends, and the events `dialogue_start` and `dialogue_end` are emitted from the entity); to continue add one to `advance`, click `text_ui` when it is a button, or press the action `dialogue_next`; to choose set `choose` to 1 to 4, click the button, or press `dialogue_choice_1` to `dialogue_choice_4` (bind them in an `InputMap`). `speed` above 0 types the line out that many characters a second, and the first press finishes the line. `Dialogue.speaker` and `Dialogue.text` hold what is being said (a graph system can play a sound for each speaker). The script is checked by `check` (jumps to nodes that do not exist, unreadable conditions) and a script that does not parse ends the talk at once.

## Marks on surfaces: `Decal`

Bullet holes, blood, scorch marks, footprints and cracks are `Decal` entities: a picture stuck on a flat surface, facing along `normal` (the way the surface faces), turned by `roll`, `size` metres across, in `color` with a `texture` (a soft dot by default). It sits a centimetre off the surface so it does not flicker, is drawn after the 3D meshes like a `Sprite` and is hidden by whatever is in front of it. With `life` above 0 it goes after that many seconds, fading over the last `fade`.

```json
{ "Name": {"value": "Bullet hole"}, "Transform": {"pos": [2.0, 1.2, 0.0]},
  "Decal": { "texture": "circle", "size": [0.12, 0.12], "color": [0.05, 0.05, 0.05, 0.9], "normal": [-1, 0, 0], "roll": 0.7, "life": 20, "fade": 3 } }
```

Spawn them from a graph system when something hits a surface: a `PrefabSpawner`-style `spawn_prefab` at the contact point, with the contact normal copied into `Decal.normal` (`{"contact": "nx"}` gives the normal of an `on_contact` system). On a curved surface the flat picture floats off its edges: keep decals small there. Many decals cost one picture each (they are layers, not a texture on the surface): fade them out with `life`.

## Cameras that draw into pictures: `CameraView`

A minimap, a security monitor, a mirror, a character portrait or a sniper scope is a second camera drawing into a picture. `examples/views.lunaris.json` has a wall monitor and a minimap (`tools/build_views.py`).

```json
{ "Name": {"value": "Map camera"}, "Transform": {"pos": [0, 40, 0], "rot": [-0.7071, 0, 0, 0.7071]},
  "Camera": {"active": false, "ortho": true, "ortho_height": 14} }
{ "Name": {"value": "Minimap"}, "Ui": {"pos": [0.76, 0.04], "size": [0.21, 0.37]},
  "CameraView": {"camera": 7, "width": 128, "height": 128, "every": 2} }
{ "Name": {"value": "Monitor view"}, "CameraView": {"camera": 8, "width": 160, "height": 120} }
{ "Name": {"value": "Monitor"}, "Transform": {"scale": [4, 3, 1]}, "MeshRenderer": {"mesh": "quad", "texture": "view:9", "emissive": 0.9} }
```

A `CameraView` says which camera to look through (`camera`, the uid of an entity with a `Camera`, which should be `active: false` so it is not mistaken for the main one) and how big the picture is (`width`, `height`, 16 to 1024). Before every main picture the world is drawn through that camera and kept as the texture **`view:UID`** (UID is the view entity's own uid), which goes anywhere a texture name does: `MeshRenderer.texture` (a monitor, a mirror), `UiStyle.image`, a sprite. A `CameraView` on a `Ui` makes that `Ui` show its own view, which is the simple way to make a minimap. `every` redraws it one main picture in `every` (a minimap needs no 60 a second); `active` false freezes it. The view has **no interface and no views of its own**, so a monitor that sees itself shows the picture from one picture ago and nothing loops. Views are drawn by the reference renderer (the GPU renderer shows them as textures, so both agree), which costs time in proportion to the size: keep monitors small and use `every`. `check` reports a camera that is missing or still `active`. A mirror is a view with the camera on the far side of the mirror plane, turned by the game each tick (a graph system); flipping the picture horizontally is `UiStyle.image_uv` of [1, 0, 0, 1] on a `Ui`.

## Reacting to events: `EventWatch`

Events (`emit`, animation events, `dialogue_end`, a state machine's `event died`) are output: a sound, a message to the outside. To let the **game** react to them, put an `EventWatch` on any entity and list the names:

```json
{ "Name": {"value": "Watch"}, "EventWatch": { "names": "quest_done, step_left, hit" } }
```

Every tick the engine then writes, for each name, the resource `event.NAME`: how many times that event was emitted **during the tick before** (0 when none). While it is above 0 the resources `event.NAME.x`, `.y`, `.z` and `.uid` say where and by whom the first one was emitted. A graph system reads them like any resource:

```
system confetti each Celebration, Particles {
    if $event.quest_done > 0 {
        Particles.time = 0;
        Particles.playing = true;
    }
}
```

An event lasts one tick as a resource, so a system sees it once. It reaches graph systems one tick after it was emitted (the systems of a tick run before the events of that tick exist). A scene without an `EventWatch` gets no such resources, so nothing about it changes. Names are letters, digits and underscores (and dots), at most 64 watched in all.

## Animation events and root motion: `AnimEvents`, `RootMotion`

Both sit beside an `Animator`, are plain text, and need the clips' lengths (`model_info` lists them, and writes the root motion for you).

```json
{ "Name": {"value": "Hero"}, "Animator": {"clip": "walk", "looping": true},
  "AnimEvents": { "events": "walk length=0.8: 0.1 step_left, 0.5 step_right
attack length=0.6: 0.25 hit" },
  "RootMotion": { "curves": "walk length=0.8: 0 0 0 0, 0.4 0 0 0.5, 0.8 0 0 1.0" } }
```

**`AnimEvents`**: one clip a line: its name (as in `Animator.clip`), `length=SECONDS`, then `time name` pairs. Every time the animation passes one of those moments (a looping clip passes it again every loop) the event `name` is emitted from the entity, at its position: a graph system answers by reading `$event.step_left` (see **Reacting to events** below), and a `SoundFile` with that name plays a footstep where the foot lands. Switching clip, restarting or scrubbing back fires nothing (it would be a lie); playing forward through a moment fires it, at most four times in one tick if time jumps. `check` reports a time outside the clip or a line it cannot read.

**`RootMotion`**: one clip a line: `clip length=SECONDS: TIME x y z, TIME x y z, ...` (times increasing, positions in metres in the entity's own axes, linear between keys). Each tick the entity moves by how far the root travelled in that tick, turned by the entity's rotation and times `scale`, so feet do not slide whatever the playback speed, and a looping clip carries on from where its last loop ended. `model_info` of a skinned model writes this text for every clip in `rig.root_motion` (the first joint of the skeleton, usually the hips, or `root` names another): paste it into `curves`, and zero the y of each key if the character should not bob. It moves the `Transform` directly; a `CharacterBody` that must collide is driven with `walk` instead.

## Inverse kinematics: `Ik`

An animated model plays the same clip whatever is around it; `Ik` bends the pose to the world. Each goal is **one empty entity**: its own `Transform` position is the point, so you move the goal and the joint follows (parent it to a door handle, a stair step, the player).

```json
{ "Name": {"value": "Right hand goal"}, "Transform": {"pos": [1.0, 1.4, 0.3]},
  "Ik": { "skin": 7, "joint": "hand_R", "kind": 0, "weight": 1, "pole": 12 } }
{ "Name": {"value": "Head goal"}, "Transform": {"pos": [0, 1.7, 3]},
  "Ik": { "skin": 7, "joint": "head", "kind": 1, "axis": [0, 0, 1], "max_angle": 70, "weight": 0.8 } }
```

| Field | Meaning |
|---|---|
| `skin` | the uid of the entity with the animated model (a `MeshRenderer` of a skinned `.glb` and an `Animator`). |
| `joint` | the name of a joint (`model_info` lists them). |
| `kind` 0 | **reach**: the two bones above `joint` (the forearm and upper arm of a hand, the shin and thigh of a foot) bend so that `joint` lands on the goal, as far as the bones allow; a goal beyond reach stretches the limb straight towards it. Bone lengths never change. `pole` is another entity the elbow or knee bends towards (without one it keeps the bend the animation has). |
| `kind` 1 | **look**: `joint` (a head, a neck, a turret) turns so that its own axis `axis` (in the joint's space; find which axis points forward in your model) faces the goal, at most `max_angle` degrees from where the animation points it. |
| `weight`, `active` | 0..1 blends the goal with the animation; `active` false switches it off. Several goals on one model apply in uid order (hands, feet, head). |

IK changes **the picture only**: the simulation never sees the bent pose (so it can never change a game's state), and both renderers show the same one. Feet that stand on stairs: a graph system casts a ray down from each foot (`raycast`) and sets the goal entity's position to the hit; the animator keeps walking. A hand that must really touch something: use the goal's own position, which the simulation knows, not the bent skeleton. `check` reports a goal whose model is missing or has no `Animator`, and a joint name that is not in the model is a render warning.

## Tweens: `Tween`

```
{"Tween": {"path": "Transform.pos[1]", "from": 0, "to": 3, "duration": 0.6, "ease": 10, "delay": 0.2}}
```

Moves one number: `path` is a field of this entity (or of `entity`). `ease` 0 linear, 1 in-quad, 2 out-quad, 3 in-out-quad, 4 in-cubic, 5 out-cubic, 6 in-out-cubic, 7 in-sine, 8 out-sine, 9 in-out-sine,
10 out-back, 11 out-bounce, 12 out-elastic, 13 smoothstep. `mode` 0 once (then `active` is false and `done` true), 1 loop, 2 ping-pong. To play again set `time` to 0 and `active` to true.
Doors, pickups bobbing, UI popping in, a light pulsing: a tween per number.

## Behaviour trees: `BehaviorTree`

For AI that picks between things by priority, with waiting and cooldowns, in text; nodes one per line, children indented two spaces:

```
selector
  sequence
    cond Sensor.seen != 0
    act set NavAgent.active = 1, event spotted
    wait 0.5
  sequence
    act set Velocity.lin[0] = 1
    wait 2
```

* `selector` tries its children in order and stops at the first that does not fail; `sequence` runs them in order and stops at the first that fails; `parallel` runs all of them every tick (it fails when one fails, succeeds when all do).
* `cond EXPRESSION` (the state machine's conditions), `act ACTION, ACTION` (`set Component.field = n` or `+= n`, `res name = n` or `+= n`, `event name`; always succeeds), `wait SECONDS` (running until the time is up).
* Decorators with one child: `repeat N` (0 = forever; the child's failure fails it), `invert`, `succeed`, `cooldown SECONDS` (after the child succeeds it fails for that long).
* The tree runs from the top every tick; a node that is still running (a `wait`, a sequence part-way) is resumed where it was, so a higher-priority branch gets its turn when the running one finishes. The root's last answer is
  `status` (1 running, 2 succeeded, 3 failed). `check` reports broken text and misspelt fields. For richer AI combine it with a `Sensor`, a `NavAgent` and a `StateMachine` (a tree can be the state machine's behaviour in one state).

## Changing scenes: `SceneSwitch` and `Persist`

`SceneManager.LoadScene` / `OpenLevel`: an entity with a `SceneSwitch` names a scene file in `path` (relative to the workspace, like `levels/two.lunaris.json`); when a graph system, a contact or a state machine sets its `go` to true, at the end of that tick the world is replaced by that scene. A main menu's Play button, the door at the end of a level, a game-over screen that restarts: each is a button or a contact that sets `SceneSwitch.go` on the entity that carries the path (a path is text, and graph systems only handle numbers, so the paths sit in the scene as data and the systems choose which switch to throw).

* every entity goes except those with a `Persist` (the player, a score board, the music: "do not destroy on load"); the entity numbers of the new scene must not clash with a persistent one, which also means that **a scene that holds a persistent entity cannot be switched back to while the entity lives**: put such entities in a first scene that nothing returns to (`examples/depths/boot.lunaris.json` holds the music and switches to the title at once);
* the graph systems and prefabs of the old scene go too, and the new scene brings its own (put shared systems in each scene file, or set `keep_systems` so the old ones stay); `keep_resources` (on by default) keeps every resource, the score and the time of day, and they win over the new scene's defaults of the same names, so a counter carries from level to level;
* the tick and the random numbers go on, and terrains are read again; the resource `scene.loaded` is the tick of the last switch and the event `scene_loaded` is emitted (a few lines of a graph system can fade in with `ColorGrade`); a scene that cannot be read, parsed or merged changes nothing and sets `scene.error` to 1; `check` reports a path that is not a file;
* replays and networked games switch at the same tick, because the switch is an outcome of the simulation itself: every machine needs the same scene files.

Code: `crates/lunaris-sim/src/scenes.rs`.

## Crowds, herds and swarms: `Steer`

Steering behaviours and flocking in one component, the part of Unreal's Mass crowds and Unity's DOTS steering that games use most. An entity with a `Steer` and a `Velocity` turns its velocity every tick (after the graph systems) by the behaviours that are on, each a weighted acceleration, together at most `max_accel` (m/s^2), with the speed at most `max_speed`; the usual movement carries it. All behaviours are optional and add up:

* `seek` (the uid of an entity) runs at it, slowing down inside `arrive` metres so it stops on the spot; `flee` (a uid) runs from it inside `flee_radius`, harder the nearer it is; `wander` (a weight) drifts in a smoothly changing direction;
* a `group` name makes the agent flock with the other `Steer` entities of that group within `radius` metres: `separation` keeps `personal` metres apart, `alignment` heads the same way, `cohesion` keeps together (the three weights). Flocks of different groups do not feel each other, so sheep and dogs, or two armies, can share a field;
* `plane` 0 moves on the ground (x and z, y is left alone), 1 in the picture plane (x and y); `face` turns the entity to face the way it moves (about y on the ground, about z in the picture plane); `bounds` `[min_a, min_b, max_a, max_b]` keeps the agent in a rectangle, pushing back in the last `edge` metres; `active` false switches it off. `speed` and `heading` are the engine's.

It is worked out from the positions and velocities of the start of the tick, in uid order, with neighbours found in a grid of cells the size of the radius: 1,000 flocking agents cost about 4.5 ms a tick (a release build on a Linux test machine), the same on every machine and every run. Combine it with the rest: a `Sensor` that sees the player can set `Steer.seek` from a graph system; the dungeon's guards could use `separation` to stop piling up in a doorway. `check` reports a `Steer` without a `Velocity` and a seek or flee target that does not exist. `examples/flock.lunaris.json` is ninety sheep that flock and run from a roaming dog, with no graph system at all (`tools/build_flock.py`). Code: `crates/lunaris-sim/src/steer.rs`.

## Looking around: `MouseLook`

First-person and free-look games need the mouse's motion, not its position. `lunaris play` writes, every tick that the mouse moved, `input.mouse_dx` and `input.mouse_dy` (pixels since the last tick; right and down are positive; zero again the tick after), `input.mouse_wheel` (notches) and the right stick of a pad as `input.gamepad.rx` / `.ry` (up and right positive). A game that sets the resource `cursor.lock` above 0.5 gets the pointer hidden and held in the window while the window has focus (Esc still quits). A `MouseLook` component turns by them after the graph systems of every tick:

* `sensitivity` (radians per pixel, default 0.0022), `stick_speed` (radians per second at full stick, 2.5), `pitch_min` / `pitch_max` (-1.5 and 1.5, a little short of straight down and up), `invert_y`, `enabled` (false freezes the look: a menu is open);
* on a **free** entity (a camera that is attached to nothing) the whole look goes into its `Transform.rot`: yaw about the vertical, then pitch;
* on an entity **attached to a body** (`Parent` and `LocalTransform`: the usual first-person camera as a child of a `CharacterBody`) only the pitch goes into the camera's `LocalTransform.rot` and the yaw turns the body (`body`, or the parent when 0), so the body faces where you look;
* `forward` and `right` (the engine's, on the ground plane, at the current yaw) say which way ahead and to the right are: `CharacterBody.walk[0] = ($look.rx * $action.move_x + $look.fx * $action.move_y) * 5` with a small graph system copying them to resources; `yaw` (turning left is positive) and `pitch` (looking up is positive) can be read and set (aim the player at a door by setting them).

A third-person orbit camera needs no `MouseLook`: `CameraRig.yaw -= $input.mouse_dx * 0.004` in a graph system (and `CameraRig.distance -= $input.mouse_wheel`). In a networked game the mouse's motion travels as ordinary input values, so every copy of the game turns the same way. `examples/first_person.lunaris.json` (`tools/build_first_person.py`) is a hall to walk round, and `examples/tests/first_person.test.json` plays it: a quarter turn from 714 pixels of motion, walking along the new direction, the pitch limit, jumping. Code: `crates/lunaris-sim/src/lookctl.rs`, the pointer in `crates/lunaris/src/play.rs`.

## Particle modules: `ParticleModules` (the Niagara-style part)

A `Particles` cloud (above) is one entity for tens of thousands of billboards, each a closed-form function of the cloud's clock. A `ParticleModules` on the same entity adds what Niagara calls modules, written as text, one a line:

```
color 0:1,0.95,0.7,0  0.12:1,0.8,0.3,0.95  0.45:1,0.45,0.1,0.8  1:0.2,0.05,0.02,0     # colour and opacity over the life, any number of stops
size 0:0.2  0.25:0.62  1:0.08                                                            # size in metres over the life
wind 0.5,0,0 gust=0.8 freq=0.6          # a steady push (m/s^2) that swells and fades like gusts
attract 0,5,0 strength=2 radius=7       # drawn to a point (negative strength pushes away), fading to nothing at `radius`
vortex 0,0,0 axis=0,1,0 strength=6 radius=3 pull=9   # swirled round an axis through the point, and pulled in by `pull`
noise strength=1 scale=0.7 speed=0.5    # a wandering force field
collide plane 0,1,0 d=0 bounce=0.4 friction=0.3       # a floor (dot(normal, p) = d); add `kill` to remove, `stick` to keep where it hit
collide sphere 0,1,0 r=1 bounce=0.3 inside            # a ball, `inside` keeps particles in it
trail segments=8 time=0.3 width=0.5     # a ribbon behind every particle: `segments` pieces over the last `time` seconds, thinning to the tail
mesh cube                               # solids (cube, diamond, tetra) tumbling about their own axis instead of pictures
```

Everything is in the cloud's own space (the entity's position, turning and scale move it all). `color` replaces `color`/`color_end` and `size` replaces `size` (the jitter, `fade_in`, `spin`, `stretch`, `texture` and `sheet` still apply); forces add to `gravity` and `drag`; a cloud with a `trail` keeps its head pictures. Colours are display colours as everywhere in `Particles`.

**What it costs.** A cloud with no force and no collider keeps its closed form (a gradient costs nothing). Forces and colliders make a particle depend on where it has been, so it is stepped from its birth with a fixed step (1/30 s, or 1/48 of the life when that is longer) and the trapezoid rule: still a pure function of the clock, the same on every machine and any number of threads, and a cloud can still be scrubbed by setting `time`. Measured on a Linux test machine (60,000 particles, 4 threads, a release build): plain 8 ms, gradients 10 ms, a gusting wind 29 ms, wind and a floor 16 ms, an attractor, a vortex and a floor 25 ms, a `noise` force 54 ms (the dear one: use `turbulence` on the cloud where you can), ribbons of 6 pieces on every particle 41 ms (a million vertices), solids 21 ms (capped at 20,000 solids, 720,000 vertices). `check` warns about the limits and the dear case. Ribbons and solids are meshes drawn by both renderers like the pictures. Code: `crates/lunaris-sim/src/particle_modules.rs` (the text and the forces), `crates/lunaris-render/src/particles.rs` (stepping, ribbons, solids); `examples/particle_fx.lunaris.json` shows a fountain that bounces, a campfire in a gusting wind, sparks with ribbons, a tornado and tumbling debris (`tools/build_particle_fx.py`).

## Sprite flipbooks: `SpriteAnim`

The Animator Controller for 2D. An entity with a `Sprite` (with a `sheet` of pictures), an `Animator` and a `SpriteAnim` shows the picture its animation says:

```
idle: 0-3 fps=6
run: 4-9 fps=12 step@1 step@4
jump: 10-12 fps=10 once
spin: 13-16 pingpong
```

One clip a line: the pictures of the sheet (numbers and `from-to` ranges, counted from 0 across and then down), `fps=N` (default 8), `loop`, `once` or `pingpong` (without one of them the animator's `looping` decides) and `NAME@K` markers that emit the event `NAME` from the entity each time the clip reaches its K-th picture (counted from 0, so `step@0` is the very start): footsteps for the sound director (`SoundFile`), hit frames for a graph system through `EventWatch`. The clip is `Animator.clip`, so everything that drives a 3D animation drives a sprite: a `StateMachine` state `state run clip=run speed=1.5` (and `once`) switches clips, a graph system can set `Animator.clip`, `Animator.speed` changes the pace and `Animator.playing` freezes it. A clip that changes starts again from its first picture. `SpriteAnim.done` turns on when a clip that plays once has shown its last picture for its whole time: `* -> idle when SpriteAnim.done != 0`. A clip name that the table does not have leaves the picture as it was; an empty `Animator.clip` plays the first line. Flipping to face the other way is a negative x scale on the `Transform`. `check` reports bad text, a missing `Animator` or `Sprite`, an `Animator.clip` that is not in the table and pictures past the end of the sheet. Code: `crates/lunaris-sim/src/spriteanim.rs`.

## Hit points: `HitPoints`

`HitPoints {value, max, regen, regen_delay, invulnerable, remove_after}`: lower `value` to damage (a graph system on a contact, a state machine action: `set HitPoints.value += -10`), and the engine keeps
it between 0 and `max`, heals `regen` per second after `regen_delay` quiet seconds, tracks `hurt` (seconds since the last hit, for flashes and shakes), undoes damage during `invulnerable` seconds of grace (a contact system can take health off and set `invulnerable` in the same tick: that hit counts and the next ones are refused until the grace is over, which is the usual "bite, then a moment of safety"), and the first
time `value` hits 0 sets `dead`, emits the event `died` and, with `remove_after`, removes the entity after that many seconds. A state machine can react: `* -> dead when HitPoints.dead != 0`.

## Streaming big worlds: `StreamZone`

```
{"Transform": {"pos": [400, 0, 0]}, "StreamZone": {"prefab": "Village", "radius": 150, "hysteresis": 30}}
```

The prefab `Village` (a whole village as a prefab with children) is created at the zone's position while the active camera (or the entity named in `target`) is within `radius` metres on the ground, and removed again
beyond `radius + hysteresis`. A world of many zones keeps only what is near in memory and in the simulation. What happens to loaded entities is forgotten when they unload; things that must remember (a chest's contents)
belong outside the prefab, in the zone's neighbourhood, or in resources and the saved game.

## Particle effects: `EmitterFx`

An `Emitter` throws particles from a point in a cone. Put an `EmitterFx` beside it for more (a plain `Emitter` is unchanged): `shape` 1 starts particles inside a sphere of radius `extent[0]`, 2 inside a box of half sizes
`extent`, 3 on a flat disc in the xz plane (rain over an area, embers from a pit); `drag` slows them (the share of speed lost per second), `turbulence` pushes them about, `spin` turns sprites (radians per second, either way),
`floor` with `floor_y` makes them bounce (`bounce` keeps that share of the speed) or, with `stick`, vanish when they land (sparks, rain, splashes); `mid` (0 = off, else the point of the particle's life) with `color_mid` and
`size_mid` adds a middle stop to the colour and size curves (a flame that is white at the core, orange, then dark smoke). Everything is decided from the seed, tick and emitter, so a replay throws the same particles.

## Particle clouds: `Particles`

`EmitterFx` (above) makes real entities, which can collide and be queried but cost one entity each. For fire, smoke, sparks, rain, snow, dust and magic you want **tens of thousands**, and only the picture needs to know about them: one `Particles` entity is a whole cloud. Every particle is a fixed function of the cloud's clock (`time`, game state that runs by itself), so there is nothing to store or integrate, the cloud is the same on every machine and in both renderers, and `time` can be set to scrub or restart it. `examples/particle_clouds.lunaris.json` has a fire, its smoke, sparks, snow, a burst and a ring of magic (`tools/build_particle_clouds.py`).

```json
{ "Name": {"value": "Campfire"}, "Transform": {"pos": [0, 0.2, 0]},
  "Particles": { "count": 700, "lifetime": 1.2, "shape": 3, "extent": [0.3, 0, 0], "cone": 14, "speed_range": [0.8, 1.8], "gravity": [0, 1.2, 0],
                 "turbulence": 0.18, "size": [0.55, 0.12], "color": [1, 0.65, 0.18, 0.9], "color_end": [0.9, 0.12, 0.02, 0], "additive": true } }
```

| Field | Meaning |
|---|---|
| `count`, `lifetime` | up to 60,000 particles, each living `lifetime` seconds. |
| `looping`, `prewarm`, `spread_time` | a looping cloud is a steady stream (`prewarm`: already full when it appears; false: it fills up over one lifetime). Not looping: one burst born over `spread_time` seconds from time 0; set `time` back to 0 to fire it again. |
| `playing`, `speed`, `time` | the clock runs while `playing`, `speed` times as fast (and follows `time.scale`). Set `time` to jump. |
| `shape`, `extent` | where particles start: 0 a point, 1 a ball (radius `extent.x`), 2 a box (half sizes `extent`), 3 a disc, 4 a ring (radius `extent.x`, in the horizontal plane). |
| `direction`, `cone`, `speed_range` | the way they fly off: inside a cone of half angle `cone` degrees round `direction` (180 = every direction), at a speed between the two numbers. |
| `gravity`, `drag`, `turbulence`, `turbulence_scale` | a constant pull (positive y for hot air), a slowing `drag` per second, and a wandering of `turbulence` metres (smooth noise; `turbulence_scale` is how fast it changes, larger = busier). |
| `size`, `size_jitter` | metres at birth and at death (+- `size_jitter` as a fraction, different for each particle). |
| `color`, `color_end`, `fade_in` | display colours at birth and death, blended over the life; an alpha of 0 at the end fades them out; `fade_in` is the share of the life spent fading in. |
| `spin`, `stretch` | `spin` turns the quads (radians a second, a random direction each); `stretch` above 0 stretches them along their velocity (sparks, rain streaks) by that much per unit of speed. |
| `texture`, `sheet` | the picture of one particle: the soft dot `soft` by default, `circle`, `ring`, or a PNG; `sheet` is (columns, rows) of frames played over each life (flames, puffs, sparkles). |
| `additive` | adds light (fire, magic, sparks: never needs sorting, and glows with bloom); false blends like smoke and dust, and the cloud is sorted far to near. |
| `seed`, `visible` | another seed is another cloud; `visible` false hides it. |

Everything is in the entity's local space: move, turn or scale the entity (parent it to a hand or a torch) and the whole cloud follows, so a trailing smoke needs the `Emitter` (real entities in world space). Particles are billboards (they face the camera), have no collisions, are not lit, and are not part of `pick` or the entity-id picture. The builder (`crates/lunaris-render/src/particles.rs`) is the single source: the reference renderer and the GPU draw the same mesh, which a test compares.

## Paths: `Spline` and `PathFollower`

```
{"Transform": {"pos": [0, 0, 0]}, "Spline": {"points": "0 1 0; 8 1 0; 8 1 8; 0 1 8", "smooth": true, "closed": true}}
{"Transform": {}, "PathFollower": {"path": 1, "speed": 3, "mode": 1}}
```

`Spline.points` is text (`x y z` separated by `;` or new lines, relative to the spline entity's position), `smooth` curves through the points (Catmull-Rom) instead of straight lines, `closed` joins the end to the start. A
`PathFollower` moves its entity along the path at `speed` metres per second, at a steady speed whatever the curve: `mode` 0 stops at the end (`done`), 1 goes round again, 2 goes back and forth; `align` turns it to face along the
path; `offset` is added to the position. Moving platforms, patrols, rail cameras (put a `CameraRig` on another entity, or follow with the camera itself), conveyors. `distance` is how far along it is; set it to jump.

## Timelines: `Timeline`

A scripted sequence in text, for cutscenes, tutorials and traps:

```
at 0 ramp Transform.pos[1] 0 -> 5 over 2 ease 2
at 0.5 set Light.intensity = 0
at 1 event thunder
at 1 res cutscene += 1
at 3 end
```

`at SECONDS` then an action: `set Component.field = n` (or `+=`), `res name = n` (or `+=`), `event name` (the state machine's actions), `ramp Component.field FROM -> TO over SECONDS [ease N]` (smooth, with the easings of
`Tween`), or `end` to fix the length. The timeline runs `time` forward at `speed` per second; steps fire once, in time order; a ramp holds its value while it runs and leaves its last value. At the end it stops (`done`) or, with
`looping`, begins again. To play it again set `time` 0 and `playing` true; set `time` to jump. Put a `Timeline` on the entity whose fields it moves (the paths are that entity's), a state machine's `enter` action can start one by
setting `Timeline.playing`.

## Camera rigs: `CameraRig`

On the camera entity (for a flat 2D picture set `look_at` to false: the default turns the camera toward `target` plus `look_offset` [0, 1, 0], which tilts it a few degrees, and `check` says so): `target` (a uid), `mode` 0 follow (stay at `offset`, eased at `follow_speed` per second, 0 snaps) or 1 orbit (`distance`, `yaw`, `pitch` in radians: a graph system
can add the mouse movement to them), `look_at` with `look_offset` to face the target, and `shake` (0 to 1: set it to 1 when something explodes) with `shake_decay` per second and `shake_amount` metres.

## Sensors: `Sensor`

Every tick the nearest entity that has the component named in `filter` (say `HitPoints`), within `radius` metres and `fov` degrees in front (the entity's local -z; 360 sees all around), is written to
`seen` (its uid, 0 when nobody), `distance`, and `count`. With `los` true, solid colliders, terrain, the triangles of `MeshCollider` levels and convex hulls (`ConvexCollider`) between the two (measured from `eye` metres above each) hide the target. Collision layers (`CollisionLayer`, `docs/PHYSICS.md`) decide which blockers count: only the ones whose layer is in the `mask` of the sensor's own entity (a full mask without the component, so nothing changes for a scene that has no layers); the targets are never filtered.

## Lifetimes and spawners: `Lifetime`, `PrefabSpawner`

`Lifetime {seconds}` removes the entity, and everything attached to it with `Parent`, when the time is up: bullets, sparks, a pickup that vanishes.

`PrefabSpawner` makes copies of a prefab at its entity:

```
{"Transform": {"pos": [0, 1, 0]}, "PrefabSpawner": {"prefab": "Bolt", "interval": 0.5, "count": 1, "max_alive": 3, "speed": 10}}
```

`count` copies every `interval` seconds while fewer than `max_alive` of its own are alive (0 = no limit) and fewer than `limit` have been made in all (0 = no limit); `radius` spreads them
over a disc around `offset`; copies are turned like the spawner and, with `speed`, thrown along its forward direction (local -z). The engine marks each copy with `SpawnedBy`. A turret, a
fountain, an enemy gate, falling leaves: one entity each. An unknown prefab spawns nothing.

## Day and night: `TimeOfDay`

One entity: `time` is the hour (12 = noon), `day_seconds` the length of a day (0 freezes it). Every tick the engine turns the first directional light along the sun's path (`azimuth` sets
where it rises), colours it warm at a low sun, sets its brightness between `night_intensity` and `sun_intensity`, and gives the first `Environment` the sky, horizon and fog colours and ambient light of
that hour. Jump to a time by setting `time`.

## Time scale

The resource `time.scale` (default 1, 0 to 8) multiplies the length of every step: 0.3 is slow motion, 0 is a pause (nothing moves, lifetimes and tweens wait, graph systems and the interface still run so a menu
can unpause), 2 is fast forward. `input` or a graph system sets it like any resource (`{"res_set": "time.scale", "to": 0}`).

## Saved games

`save_game {slot}` writes the whole world (entities, resources, graph systems, prefabs) with the tick and the random generator to `saves/<slot>.json` of the workspace; `load_game {slot}`
brings it back exactly (the next ticks are the ticks that would have come), `saves` lists the slots. A slot is a name (letters, digits, `_`, `-`) or a number (`3` is `slot3`). From inside a game,
set the resource `game.save` (or `game.load`) to a slot number from 1 to 99 in a graph system: at the end of the step the engine does it, sets the resource back to 0, and sets `game.saved` or
`game.loaded` to the tick, or `game.error` to 1 when it could not. Navigation paths in progress are not saved (agents plan again); `input.*` resources are not saved either.

A title screen asks whether to offer Continue with the resource `game.probe` (a slot number): at the end of the step `game.has.<slot>` is 1 when that slot has a saved game and 0 when not (`$"game.has.1"` in a script is fine too: a name may have a number between dots). A recording that is running goes on across an in-game load, and a replay is given the same answers the files gave the first time (what `game.probe` found, which save `game.load` read) and writes no files, so it matches whatever the files hold by then.

## Closing the player, and Esc

`lunaris play` (and an exported game) closes its window on Esc. A game that wants Esc for itself, a pause menu, sets the resource `app.quit_on_escape` to 0: Esc is then the key `input.escape` (bind it in an `InputMap`: `pause = escape, gamepad.start`). A game closes itself, a Quit button, by setting the resource `app.quit` above 0 (also in a hidden self-test). A self-test presses keys at set frames with `--press enter@45:4,s@80` (KEY@FRAME[:FRAMES]; one tick a frame), prints the resources with `--report` and saves the last picture with `--save-frame`: `tools/check_depths_export.py` uses them to play an exported folder.

## Languages: `Localization`

Put `{#key}` in any `Text.value` or `Ui.text`; one `Localization {"language": "en"}` entity picks `lang/en.json` of the workspace: `{"hello": "Hello", "menu": {"play": "Play"}}` gives `{#hello}` and `{#menu.play}`
(a key that is not in the file shows as the key). Setting `language` to `"de"` switches at once. `{resource}` placeholders still work in the same text: `{#score_label}: {score}`, and a translated text may hold placeholders of its own (`"stats": "Level {level}   Score {score}"` in the language file). A `language` with slashes is the path of the file without `.json`, so that a game keeps its texts beside its other files (`"language": "mygame/lang/en"` is `mygame/lang/en.json`; `examples/depths` does it); an export copies that whole folder of languages. A graph system cannot set a text, so a language switch in the game itself needs the `language` set from the outside.

## Limits

* Conditions look at the entity that owns the machine (and resources). To react to another entity, copy its value into a resource with a graph system, or read it with a `Sensor`.
* One state machine per entity; give a second entity (a child) another one.
* Sensors see by position, not by size: a big thing counts from its centre.
