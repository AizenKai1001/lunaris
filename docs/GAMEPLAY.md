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
Rebinding is editing the text. The `input` tool (or a window's key handler) sets the raw `input.*` resources. A settings screen that lets the player choose the keys uses `KeyBind` buttons and `{key:ACTION}` in texts: `docs/KEYBINDS.md`.

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
| `Speaker: words` | a line of speech (the name is up to 24 letters, digits, spaces, dots or apostrophes, or a translation key `{#npc.guard}`, and a space follows the colon). A line without a speaker is narration. The words may hold `{resource}` placeholders and `{#key}` translations, like any `Text`. |
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

Every tick the engine then writes, for each name, the resource `event.NAME`: how many times that event was emitted **during the tick before** (0 when none). While it is above 0 the resources `event.NAME.x`, `.y`, `.z` and `.uid` say where and by whom the first one was emitted. **Several in one tick**: the first four are also told one by one, `event.NAME.0.x`, `.0.y`, `.0.z`, `.0.uid` to `event.NAME.3.x` ... (slot 0 is the same event as `event.NAME.x`, the rest follow in the order the events were emitted, which is uid order for the events of one system), and `event.NAME.n` says how many slots are filled (0 to 4; `event.NAME` still counts all of them). A slot is only meaningful while `n` is above its number; `event.NAME.n` does not exist until the first event of that name and reads 0 in every tick without one, so a scene that never has the event is unchanged. A graph system reads them like any resource:

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
| `kind` 2 | **pin**: like `reach`, but the joint is moved only along `axis` (and `axis2`, if it is not zero) so that it is level with the goal in those directions, and keeps the place the animation gives it in all others. A hand put on the line of a ledge: `axis` up and `axis2` out of the wall, the goal anywhere on the ledge; a foot onto the plane of a wall: `axis` alone, along the wall's normal. `one_sided` moves the joint only when it is behind the plane (as seen from the goal, along `axis`): a foot or hand that must not sink into the ground has `axis` up, `one_sided` on and the goal at the height of the ground plus the joint's own height above the sole or palm. |
| `kind` 3 | **turn**: `joint` is turned by `angle` degrees about `axis` (in the **model's own space**, right-hand rule), times `weight`, on top of whatever the clip plays; the goal's position is not used. A spine that leans back as a player looks up (give each spine joint a share of the angle: `{"kind": 3, "joint": "spine_03", "axis": [1,0,0], "angle": -12}` on a child of the model, and a graph system that sets `Ik.angle` from the pitch), a head that tilts, a turret that follows a gauge. The axis is the model's, not the joint's, so a lean about the model's side axis stays a lean whatever the clip does. |
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

* every entity goes except those with a `Persist` (the player, a score board, the music: "do not destroy on load"); **an entity of the new scene that has the number of a persistent entity is that entity**: the persistent one wins, keeps the state it has and is not made again, and the duplicate is skipped. So the scene that holds the music, the title say, can be switched back to as often as you like with the music entity still in its file (and a scene can be played on its own, with its own copy of the player). The resource `scene.skipped` says how many were skipped by the last switch (it is written from the first switch that skips one, so a game that never meets this has no such resource); an old game that put its persistent entities in a first scene that nothing returns to (`examples/depths/boot.lunaris.json` holds the music and switches to the title at once) works as before;
* the graph systems and prefabs of the old scene go too, and the new scene brings its own (put shared systems in each scene file, or set `keep_systems` so the old ones stay); `keep_resources` (on by default) keeps every resource, the score and the time of day, and they win over the new scene's defaults of the same names, so a counter carries from level to level. To reset some of them on entry put a **`SceneReset {resources}`** beside the `SceneSwitch` (`"resources": "paused leave time.scale"`, names separated by spaces or commas, a name ending in `*` is a prefix: `"fx.*"`): each resource named is **what the new scene says it is**, and gone (reading 0, or the default of an engine resource such as `time.scale`) when the new scene does not say, so a level no longer needs a first-tick system to clear `paused` and `time.scale`. The `SceneReset` of the switch that fires counts, and `check` reports one without a `SceneSwitch` or with a name that is not a resource;
* the tick and the random numbers go on, and terrains are read again; the resource `scene.loaded` is the tick of the last switch and the event `scene_loaded` is emitted (a few lines of a graph system can fade in with `ColorGrade`); a scene that cannot be read, parsed or merged changes nothing and sets `scene.error` to 1; `check` reports a path that is not a file: a **warning** when the name is a scene's (`*.lunaris.json`), because scenes that name each other in a circle (game over, level, game over) cannot all exist the first time they are built, and an **error** for any other name (a typo or a wrong ending); either way a switch to a missing file fails and sets `scene.error`, and `export_game` refuses a game that switches to a scene file that does not exist (also one a later scene names), unless `force`;
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

## Draw order: `DrawLayer`

A 2D picture is drawn far to near by each thing's depth along the camera's view, which is its `Transform.pos[2]`. But `z` is also a position in 3D, and solid bodies push each other along it: a hero given a bigger `z` than the guards, only to be drawn on top, made every contact push them out of the plane (the dungeon game lost its guards that way, `docs/DOGFOOD.md`). A **`DrawLayer {layer}`** is the drawing order and nothing else:

```json
{ "Name": {"value": "Hero"}, "Transform": {"pos": [3, 2, 0]}, "Sprite": {"size": [1, 1]}, "DrawLayer": {"layer": 11} }
{ "Name": {"value": "Guard"}, "Transform": {"pos": [4, 2, 0]}, "Sprite": {"size": [1, 1]}, "DrawLayer": {"layer": 10} }
{ "Name": {"value": "Floor"}, "Transform": {"pos": [0, 0, 0]}, "Tilemap": {"size": [16, 9]}, "DrawLayer": {"layer": -1} }
```

* Things are drawn from the **lowest `layer` to the highest**, the highest on top, whatever their `z` and their uid. Equal layers go by depth and then by uid, exactly as before.
* An entity **without** the component is layer 0. A scene that never uses it is drawn as it always was (the picture hash of every example is unchanged: a test compares them), and a thing in layer 0 mixes with the unlayered ones by depth.
* It orders a **`Sprite`**, a **`Tilemap`**, a **`Decal`** and a **`Particles`** cloud against one another, in the reference renderer and in the GPU renderer (both draw the list `gather_scene` makes: `crates/lunaris-render/src/render.rs`). A cloud **without** a `DrawLayer` is drawn after everything, as ever (sparks over the whole picture); with one it takes its place among the sprites: smoke behind the hero in layer 4, a spark burst in front of it in layer 12. Clouds and sprites with the same layer go by depth and uid as well.
* A `Ui` and a `Text` are the screen's overlay: they are composed over the finished picture in the order of `Ui.layer` (then uid) in both renderers, after the tone mapping and the glow in the GPU renderer's high qualities, so a `DrawLayer` does not move them between sprites and `check` says so. A name or a damage number that must stand between layers of the world is a `Sprite` (a picture of the text) or a `UiAnchor`ed `Text` in front of everything.
* 3D meshes are not sorted at all: they are depth tested, a nearer one hides a farther one.
* For a top-down game where what is lower on the screen is in front, a graph system sets the layer from the position: `system ysort each Transform, DrawLayer { DrawLayer.layer = 0 - Transform.pos[1]; }` (a layer is any number, not a whole one). For a platformer: a layer each for the background, the ground, the actors, the effects.

`check` warns about a `DrawLayer` on an entity it cannot order (a `Ui`, a `Text`, a mesh, or nothing) and about a layer that is not a number (it then counts as 0), and its warning about solid bodies at different z now points here.

## Hit points: `HitPoints`

`HitPoints {value, max, regen, regen_delay, invulnerable, remove_after}`: lower `value` to damage (a graph system on a contact, a state machine action: `set HitPoints.value += -10`), and the engine keeps
it between 0 and `max`, heals `regen` per second after `regen_delay` quiet seconds, tracks `hurt` (seconds since the last hit, for flashes and shakes), undoes damage during `invulnerable` seconds of grace (a contact system can take health off and set `invulnerable` in the same tick: that hit counts and the next ones are refused until the grace is over, which is the usual "bite, then a moment of safety"), and the first
time `value` hits 0 sets `dead`, emits the event `died` and, with `remove_after`, removes the entity after that many seconds. A state machine can react: `* -> dead when HitPoints.dead != 0`.

## Loot tables: `LootTable`, `LootContainer`

What is in a chest, rolled from text tables, the same for the same container and refilled after a looting: see `docs/LOOT.md`.

## Stealth: `Stealth`

Noise by how you move, visibility by crouch, stillness and light: see `docs/STEALTH.md`.

## Interaction: `Interactor`, `Interactable`, `Door`

The use button: doors that swing, containers that open, items picked up, locks and keys: see `docs/INTERACT.md`.

## Drones: `FlyAgent`

A flying machine's mind (height, patrol, follow, circle, avoid, keep apart, driven by `Perception`): see `docs/DRONES.md`.

## Death and respawn: `Respawn`, `SpawnPoint`

Corpses with the dropped bag, spawn points, a fresh life: see `docs/RESPAWN.md`.

## Helicopters: `Helicopter`

A flight model for a game (lift, attitude from the stick, noise the AI hears, heat on the pilot, crashes): see `docs/HELICOPTER.md`.

## Building: `BuildRules`, `BuildTool`, `BuildPiece`

Base building with snapping pieces, stability, collapse and decay: see `docs/BUILDING.md`.

## Survival needs: `Vitals`

Hunger, thirst, stamina and bleeding, what an empty meter does, healing, and the speed a character may go: see `docs/SURVIVAL.md`.

## AI senses and searchlights: `Perception`, `Noisy`, `Searchlight`

What a machine has noticed (sight with line of sight, hearing, awareness, memory, squad alerts, an autopilot that chases and shoots), a beam that exposes and detects, and a director that sends waves of hunters when a player's heat rises: see `docs/AI.md`.

## Weapons, bullets and teams: `Weapon`, `Bullet`, `Team`

Guns that fire hitscan or travelling rounds at anything with `HitPoints`, with falloff, headshots, bloom, recoil, ammo and no friendly fire between `Team`s: see `docs/COMBAT.md` (it draws and plays nothing; the game answers the events with its own assets).

## Streaming big worlds: `StreamZone`

```
{"Transform": {"pos": [400, 0, 0]}, "StreamZone": {"prefab": "Village", "radius": 150, "hysteresis": 30}}
```

The prefab `Village` (a whole village as a prefab with children) is created at the zone's position while the active camera (or the entity named in `target`) is within `radius` metres on the ground, and removed again
beyond `radius + hysteresis`. A world of many zones keeps only what is near in memory and in the simulation. `min_radius` makes a zone a ring (loaded only while the target is at least that far away): several zones on the same place with
rings that meet give a coarse prefab in the distance and a detailed one up close. A prefab cannot name one of its own entities, so a `GrassField` inside one leaves `terrain` at 0 and grows on the `Terrain` of its own entity.
`requires` (an entity with a `StreamZone`) makes a zone load only while that one is loaded, which is how a tree of tiles refined with distance is built (a city's levels of detail: a tile's coarse prefab is a ring zone, a "refined" zone of the same place with the
ring's inner radius stands for the finer level, and the tiles under it require it, so exactly one level of every branch is loaded; a tick of delay a level).
**Many players, no camera: `StreamAnchor`** (a server). The zones that name no `target`, and the cells of a `ProcGrid`, work from the active camera; put a `StreamAnchor` on every entity that should keep the world loaded round it (a player's avatar) and they work from the camera and every anchor: a zone loads while ANY of them is within its radius (the nearest orders the loads) and is let go only when ALL are beyond radius plus hysteresis, a `ProcGrid` makes the cells round each anchor once, however many overlap. `StreamAnchor.on` false leaves one out. Test `stream_zones_follow_every_anchor_and_a_zone_goes_only_when_all_have_left` and the end of `a_proc_grid_keeps_the_zones...`. A zone with a `target` still follows only its target. With hundreds of anchors the work is made to scale: `procgrid` walks the cells round each anchor and keeps the nearest anchor's distance (not every cell against every anchor), a cell is kept by the first anchor that is near enough, and `streaming` keeps the anchors in squares of 256 m so that a zone that is not loaded looks only at the anchors near enough to want it and a loaded one stops at the first anchor that keeps it. The resource **`procgrid.every`** (a whole number, 1 or missing: every step) makes a `ProcGrid` look at its cells only every that many steps (a city on a server with 200 players: 1.4 ms a step with 3 grids, 0.4 ms with 4). Measured on 200 bots over a 1.5 km city: docs/CITY_GAME.md. Test `a_crowd_of_anchors_loads_and_drops_exactly_the_zones_and_cells_that_measuring_every_anchor_would` (60 anchors, 200 zones and a grid, equal to measuring everything at every step) and `a_proc_grid_can_look_at_its_cells_only_every_few_steps`.
The resource `stream.loads_per_tick` (a whole number; missing or 0: no limit) caps how many zones are loaded in one tick, the nearest first: a prefab of three thousand entities takes about twenty milliseconds to create, and a world that
brings in several at once (a streamed city) would spend sixty of them in one tick; with the cap the cost is spread over the ticks and the pictures stay even.
The cache of pictures, models, heightmaps and splat maps lets go of what no picture has asked for in the last 600 (`LUNARIS_TRIM_IDLE`), so a cell that has left is freed. What happens to loaded entities is forgotten when they unload; things that must remember (a chest's contents)
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

**Previewing: `timeline_seek`.** The tool `timeline_seek {uid, time}` puts the timeline of an entity at a second without playing it: every number a ramp or a `set` of the script moves gets the value it would have at that time (a ramp by its progress and easing, a `set` as of the last one that has happened, a ramp that has not begun at its `from`), and `Timeline.time` reads that second. Events, resources and `+=` steps are not run (they cannot be undone by going back), a `set` that is still ahead keeps the value it had. It answers with the clamped `time`, the script's `length` and how many numbers it wrote. It changes the world and `undo` does not take it back, so use it on a snapshot (the editor's Sequencer tab scrubs inside a preview that Stop undoes: `docs/EDITOR.md`).

## Camera rigs: `CameraRig`

On the camera entity (for a flat 2D picture set `look_at` to false: the default turns the camera toward `target` plus `look_offset` [0, 1, 0], which tilts it a few degrees, and `check` says so): `target` (a uid), `mode` 0 follow (stay at `offset`, eased at `follow_speed` per second, 0 snaps) or 1 orbit (`distance`, `yaw`, `pitch` in radians: a graph system
can add the mouse movement to them), `look_at` with `look_offset` to face the target, and `shake` (0 to 1: set it to 1 when something explodes) with `shake_decay` per second and `shake_amount` metres.

`roll` (radians, positive leans the top of the picture to the right) turns the picture about the way the camera looks, for a game that banks the view in a wall run or a slide (set it from a graph system and ease it: the engine applies what it finds). An orbit camera (`mode` 1) places itself at `target + look_offset + distance * (sin(yaw) cos(pitch), sin(pitch), cos(yaw) cos(pitch))`: at yaw 0 it hangs behind the target on the +z side, so the way the camera looks along the ground is (-sin yaw, -cos yaw) and its right is (cos yaw, -sin yaw): a walking system turns the stick or the keys by the camera's yaw, a mouse turn to the right lowers the yaw (`CameraRig.yaw -= $input.mouse_dx * 0.004`), and pushing the mouse down raises the pitch.

**A camera that trails a fast character without lagging the mouse: `lag`, `lag_up`, `lead`, `lag_max`, `lag_max_up`.** `follow_speed` eases the camera itself, so in orbit mode it eases the mouse's turn too (the camera lags the look by a few degrees: a bad feel for aiming and for flicks). With `lag` above 0 the rig instead moves a *pivot*: a critically damped spring (the closed form of Ryan Juckett's "Damped Springs": exact for any step, never overshoots) on the target, `lag` its natural frequency in rad/s sideways (12 settles in about a quarter of a second; at a steady speed `v` the pivot trails by `2 v / lag`) and `lag_up` up (0 = `lag`; softer up than sideways, so a jump, a fall or a stair does not shake the picture). The camera then orbits the pivot at `distance` with exactly the `yaw` and `pitch` it is given, and looks at the pivot plus `look_offset`: **the mouse stays one to one** and only the position trails, so the character leaves the middle of the picture a little when it runs, which is what shows speed. `lead` aims the spring that many seconds of the target's smoothed velocity ahead, sideways (not up: a falling camera that runs ahead of the fall shows the ground, but the picture rushes); `lead = 2 / lag` keeps up exactly at a steady speed, more shows what is ahead, and the velocity is smoothed over a tenth of a second so one stray tick (a step up) does not kick the spring. `lag_max` and `lag_max_up` (metres, 0 = no limit, `lag_max_up` 0 = `lag_max`) are the farthest the pivot may fall behind or run ahead: the character never leaves a window of the picture, however fast the fall. A target that moves faster than 90 m/s in one tick was put somewhere else, and takes the pivot with it. `pivot`, `pivot_vel`, `pivot_from`, `pivot_lead` and `pivot_on` are the spring's state (written by the engine; they are in the world's hash). The camera collision below starts its line at the target itself, not at the pivot, so a pivot that leads into a corner never puts the line inside a wall. Tests: `gameplay_tests.rs` (`a_rigs_*`).

**Keeping the camera out of the walls: `CameraCollide {radius, min_distance, recover}`** beside a `CameraRig`. After the rig has placed the camera each tick, a line is looked along from the point it looks at (target plus `look_offset`) to the camera; if a solid `Collider` (not a trigger), a terrain, a `MeshCollider` or a convex hull is in the way, the camera is put `radius` metres in front of it, never nearer to the target than `min_distance`. The rig eases it out again when the way is free (give the rig a `follow_speed` of 10 or so; with 0 it snaps). With `recover` above 0 the camera is pulled in at once (you must never see through a wall) but let out at that rate per second, exponentially (4: about three quarters of a second): a camera that slides along a wall or passes a pillar does not pop in and out, and the rig's own `follow_speed` or `lag` does not matter for it (`held` is the distance it is held at while it comes back, 0 when free; written by the engine). The target's own collider does not count. It was added by the 3D game Caldera (`docs/DOGFOOD_3D.md`): without it an orbit camera sees the inside of every tree and hill. `check` warns about a `CameraCollide` beside no `CameraRig`.

## Character motion: `CharacterMotion`, `Climbable`

A `CharacterBody` walks, jumps and steps up kerbs. Put a **`CharacterMotion`** beside it and it also wades, swims, rolls, climbs ladders and pulls itself up ledges. The engine does the moving (after the graph systems, like the walk); a graph system only sets the input and reads the state back, like `CharacterBody.walk` and `grounded`. A world without the component is bit for bit what it was.

```json
{"Transform": {"pos": [0, 0, 0]}, "CharacterBody": {}, "CharacterMotion": {"swim_speed": 2.4, "roll_speed": 7.0}}
```

**Water** (a `WaterBody`, docs/TERRAIN.md): while the surface is less than `swim_depth` (1.1 m) above the feet the character **wades**: its `walk` is slowed in proportion to the depth, down to `wade_speed` (0.7) times at `swim_depth`. From there on it **swims**: no gravity, the feet rest `float_depth` (1.35 m) under the surface (the shoulders at it), the horizontal speed is at most `swim_speed` (2.4 m/s), and `vertical` (-1 to 1) dives or rises at `dive_speed` (2 m/s); rising stops at the float level. `depth` is how far the surface is above the feet, `under` says the head is under it (a game spends breath), `wading` and `swimming` say which state it is in. The state has a little hysteresis, so the edge does not flicker. (`InWater`, written by the engine on every character in water, is still there and means the same.)

**Roll**: set `roll` (a request, cleared every tick) on the ground and the character rolls for `roll_time` (0.6 s) along its `walk`, or along `roll_dir` when it is not walking (a game sets `roll_dir` to where the model faces; with neither there is no roll). The roll goes `roll_speed` (7 m/s) and slows to a quarter by its end, ignores `walk` and `jump` until it is over, and needs `roll_cooldown` (0.25 s) before the next one. The engine says `rolling` and `roll_left`; **a game makes the roll untouchable by reading them** (the engine does not touch `HitPoints`): `{"if": {"get": "CharacterMotion.rolling"}, ...}`. No rolling in the air or in deep water.

**Ladders**: a **`Climbable {height, width, reach, speed}`** entity is a ladder, a rope or a climbable wall: its `Transform.pos` is its foot (bottom, in the middle) and its rotation turns it; the side a climber stands on is its local +z. A character that **walks into** one, or pushes `vertical` at it, grabs it (`climbing` is true and `ladder` is its uid), holds on to it and climbs with `vertical` at `climb_speed` (2 m/s) times the ladder's `speed`. No input, no sliding. At the top it steps onto whatever is behind the ladder; `jump` lets go (with a small hop back, and it cannot grab again for 0.4 s), and going down to the ground lets go. The ladder has no shape of its own: put a model and, if it should be solid, a `Collider` beside it.

**Ledges**: `jump` while walking against a ledge that is higher than `step_height` and at most `mantle_height` (1.6 m) above the feet pulls the character up onto it in `mantle_time` (0.55 s): up first, then over, landing a little way in from the edge (`mantling`, `mantle_left`, `mantle_from` and `mantle_to` say what it does; nothing else moves it meanwhile). A swimmer that pushes against a dock or a rock up to about half of `mantle_height` above the surface does the same, so a lake can be left over a dock. A jump with no wall in reach is still only a jump, and a wall that is too high is not climbed. **Corner correction**: a rising character whose head would stop dead against the corner of a ceiling (it overlaps the edge by 0.22 m or less) is nudged sideways round it, towards where it walks first, and the jump goes on up beside the ceiling; a ceiling over the whole head still stops it.

The state fields (`wading`, `swimming`, `under`, `depth`, `rolling`, `roll_left`, `roll_wait`, `climbing`, `ladder`, `climb_lock`, `mantling`, `mantle_left`, `mantle_from`, `mantle_to`) are rewritten by the engine every tick: read them, leave them alone. Everything is deterministic (f32, `+ - * / sqrt`, fixed order); the tests are `crates/lunaris-sim/src/motion_tests.rs`.

**A `Steer` on a character**: an entity with a `CharacterBody`, a `CharacterMotion`, a `Velocity` and a `Steer` is walked by the steering: the velocity the steering works out (on the ground plane) is its `walk`, the movement system leaves it alone (the controller moves it, so it is not pushed twice), and the `Velocity` it keeps afterwards is what the character really did, so a wall stops the agent and the steering sees it. Flocks, seekers and fleers can therefore walk over terrain, up stairs and along walls like the hero.

## Looking at the ground and ahead: `Probe`

A graph system cannot cast a ray or ask the terrain for its height; it can read a **`Probe`**. Every tick, after the physics step and before the graph systems, the engine looks down from `up` (2 m) above the entity and writes `over_ground`, `ground` (the world y of the first solid it meets: terrain, a mesh level, a box, a hull; anything the entity collides with by `CollisionLayer`), `ground_normal` and `ground_uid`, and `wet` and `surface` when the place is over a `WaterBody`. With `ray_max` above 0 it also casts a ray from `ray_from` (a point relative to the entity, turned by its rotation when `ray_local` is true) along `ray_dir` and writes `hit`, `hit_dist`, `hit_point`, `hit_normal` and `hit_uid`. It sees what a wheel sees: static solids, not characters or moving bodies (use a `Sensor` for those), and not the entity itself. To know the ground at another place, move an entity with a `Probe` there. A world without a `Probe` pays nothing, and neither does a step in which no probe casts: **`up` + `depth` of 0 means no ground cast** (`over_ground` is false, `ground` is the entity's own y) and **`ray_max` of 0 means no ray**, and the static world (the expensive part, one gather of every collider) is gathered when the first probe that casts needs it, once for all of them, and not at all in a step where none does. A game that checks the ground or a roof on every avatar every few steps can set `up`, `depth` and `ray_max` to 0 in between (`lunaris bench`: 150 probes cost 0.005 ms a step that way and 0.41 ms in a step where all of them cast, in a city of 36 mesh shells and 3 300 boxes). Code: `crates/lunaris-sim/src/probe.rs`.

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

## Motion matching: `MotionMatcher`

Instead of a state machine of clips (idle, walk, run and the transitions between them) a character can be animated by a **search**: a database holds the poses of many clips, each described by what the character will do in the next 0.7 s and what its feet and hips are doing; several times a second the best frame for what the character is doing and wants to do is found and the `Animator` plays that clip from there (crossfaded). A start, a stop, a pivot and a loop are all just clips that compete. It is the algorithm of Unreal's Pose Search and of Simon Clavet's GDC 2016 talk (Ubisoft), with the schema of Epic's Game Animation Sample (`PSS_Default`): the character's path at -0.05 s (position), 0 s (velocity, facing), +0.35 s (position, facing) and +0.7 s (position, velocity, facing), the position of one foot relative to the other, both feet's velocities and the heading of the hips, in the character's own frame and normalised by their deviation over the database; the cost is the weighted mean of the squared differences.

* **The database** is a file made from a model's clips: `lunaris mmbuild MODEL.glb OUT.mmdb [--prefix NAME] [--rate 30]` samples every clip (names that start with the prefix) every 1/30 s: the pose by forward kinematics (the joints `foot_l`, `foot_r`, `thigh_l`, `thigh_r`) and the path from the clip's **root motion**, which is what the clip does to the node that holds the skeleton in the glTF file (the translation and rotation tracks of the armature node: the importer keeps them as `Clip.root`; the pose itself never uses them). A clip whose name has `Loop` loops; the others hold their ends. A clip starts at time 0 whatever time its first key has (exporters that begin one frame in made loops stutter: the importer rebases every clip), and a root track that stops one frame short of the poses' (the last step repeats the one before) is carried on at the pace before it. The words in a name put it in the kinds listed by `TAGS` in `crates/lunaris-render/src/motion_db.rs` (idle, turn, walk_loop, run_loop, walk_start, run_start, walk_stop, run_stop, walk_pivot, run_pivot, crouch, jump_start, fall_start, jump_fall, land_light, land_heavy, land_roll, land_stumble, sprint_loop, sprint_start, sprint_stop, sprint_pivot, other): a roll and a stumble are kinds of their own, so a script says which of the two a landing may be, and the sprint clips (`Sprint_Loop`, `Sprint_Start`, `Sprint_Stop`, `Sprint_Turn` and `Sprint_Diamond` are its pivots) have kinds of their own like the walk's and the run's.
* **`MotionMatcher`** goes on the entity with the model's `MeshRenderer` and `Animator`; its `Transform` is the character's place and facing (an entity that is a child of the character, turned to face where it goes, works). `db` names the file. A graph system sets `vel` (the ground velocity the player asks for), `face` (the direction to face; 0 keeps it) and `tags` (the **chooser**: a bit mask of the kinds of clip that may be picked now); `accel`, `brake` (m/s2) and `turn` (1/s) say how the character will speed up, slow down and turn, which the predicted path is made of. `decel` (m/s2: slowing down or turning round while a wish remains; 0 = `brake`, which is for no wish at all) and `turn_rate` (radians a second: a steady turn instead of the smooth `turn`) let the prediction match a movement code that does the same; these numbers decide how well a captured clip fits (a run start in the Game Animation Sample reaches 5 m/s in 0.8 s: a character that does it in 0.3 s matches no start clip well and slides). `react` (m/s: a change of the wish of more than this, or of `tags`, searches at once instead of at the next `search_interval`) and `hysteresis` (the playing clip is left only for one that costs this fraction less) keep the choice quick and steady. Every `search_interval` seconds (and when a clip has ended) the lowest-cost frame is found; the clip that is playing is kept unless a frame costs less than it plus `continuing_bias` (negative keeps it longer); `blend` is the crossfade; a jump inside the playing clip must be at least `jump_threshold` seconds. `cost`, `keep_cost`, `clip` (the database's clip number + 1), `switches`, `searches` and `speed` are the answers. **Stride warping**: a loop is played up to `warp` (0.2) faster or slower so that its own speed (the velocity feature of its frame) is the character's measured `speed`: no sliding feet between the captured speeds. `force` makes the next update search at once: a script that changes `tags` for a jump or a landing sets it (the jump clips are a start, the fall loop and landings by their tags: `jump_start` or `fall_start` when leaving the ground, `jump_fall` while in the air (the start clip keeps playing until it ends because the playing clip is always a candidate), `land_light` or `land_heavy` on touching down).
* The character should be moved by its own movement code (a `CharacterBody` whose velocity speeds up and slows down at the same rates as `accel` and `brake`), not by the clips: motion matching only chooses what it looks like. Everything is deterministic (`+ - * / sqrt` on `f32`). Code: `crates/lunaris-sim/src/motionmatch.rs`, `crates/lunaris-render/src/motion_db.rs`.

## Inertialization: `Inertia`

Switching clips by a crossfade shows two moving poses at once for a moment (limbs ghost, feet slide). Unreal's animation system (and its Game Animation Sample) instead uses **inertialization**: only the new clip is shown, offset by the difference between the pose the old clip had at the switch and the new clip's first pose (and between their velocities), and the offset dies away smoothly. Put **`Inertia`** (`span`: seconds the offset takes, 0.25 by default) on an entity with an `Animator` and every change of `Animator.clip`, and every jump of `Animator.time` (what motion matching does inside a clip), is inertialised instead of crossfaded (`Animator.fade` is ignored). The offset follows a quintic Hermite curve: it starts at the difference and its velocity and ends at rest (zero velocity and acceleration), so there is no jump and no jerk, and a new switch while an offset is still dying starts from the pose that was showing. The world holds what the pose is made of (the pose is a pure function of it): `Inertia` keeps the chain of the last six clips the animator has played (the time each started at, how long each played, its rate, its span); `animate` updates it (`crates/lunaris-sim/src/inertia.rs`) and `Rig::chain_local` works the pose out (`crates/lunaris-render/src/rig.rs`; rotation offsets are quaternions scaled along their own axis, translations and scales vectors; no trigonometry). An entity without `Inertia` crossfades as before.

`lunaris posetrace SCENE.json --uid UID --joints pelvis,foot_l,... --script "40:;80:w;12:w,d;12:w,a"` plays a scene with keys held in time segments (`TICKS:KEY,KEY`) and prints a CSV line per tick: the hero's place and yaw, the clip playing, the length of the inertialization chain and the model-space position of every named joint exactly as the renderer poses it. It is for looking at animation frame by frame (`pose_jitter.py` in the Lunaris game turns it into accelerations: a pop shows as a spike of thousands of m/s2 where steady running has 250).

## Traversal: `Traversable`, `Traversal`

Vaulting, hurdling, mantling and climbing over obstacles with captured clips and **motion warping**, as in Epic's Game Animation Sample (`crates/lunaris-sim/src/traversal.rs`). Obstacles are boxes (a `Collider` of shape 1, axis-aligned or turned about the vertical axis) with a **`Traversable`** (`margin`: how far in from the ends of an edge the hands go). The character's model entity (the one with `Animator` and `MotionMatcher`, a `Parent` that is the character with its `CharacterBody`) gets a **`Traversal`** whose `data` is a traversal file (`make_traversal.py` of the Lunaris game writes one from the sample's montages: the root path of every clip, the ledge the clip passes, its warp windows, its branch-in window and its blend-out moment).

A script asks by setting `Traversal.try_for` (seconds the check keeps trying; 0.5 when the player presses Space). Every tick that it counts down the engine **senses**: a sweep forward (`reach_min`, 0.75 m standing, up to `reach_max`, 3.5 m, at 5 m/s: a game may want less, so that Space a good way from an obstacle is a jump and not a climb) for a `Traversable` box; its nearest top edge is the *front ledge*, the opposite edge the *back ledge*, and it measures the height above the feet, the depth, the floor behind (the highest solid top below the ledge, within the obstacle's height) and the room above. The kind of move is Epic's rule (metres): **vault** (a back ledge, no floor behind, height 0.5 to 1.25, depth under 0.59), **hurdle** (floor behind, drop more than 0.5, same height and depth), **mantle** (deeper than 0.59, height 0.5 to 2.75; above 1.5 m it is the 2.5 m *climb* clip). The answers are in the component (`action`, `height`, `depth`, `drop`, `front`, `back`, `floor` ...). It **chooses** the clip of that kind for the gait (stand, walk, run) by how well its pose at the moment it would be entered matches the character's own (with the motion matching database; this picks the left or right foot and the 1.0 or V2 style), the moment being where the clip is as far from the ledge as the character is (the clip's branch-in window); too far from the ledge for the clip to begin, it waits (the check goes on). Then it **warps**: the clip's root path is laid on the real ledge (the clip's own ledge, the animated `attach` bone, onto the real one, facing it) and the difference from where the character is is removed inside the clip's front-ledge window; the back-ledge window carries the root over the real back edge and the back-floor window lands it at the real height of the floor (offsets that grow smoothly over each window). The root never goes into the ground. While `active` the engine writes the character's `Transform` (and gravity is off), plays the clip on the `Animator` and holds motion matching off; at the clip's blend-out moment (a mantle or climb: when the root is over the top, a climb also waits for `move_input`) it ends: `exit` is 1 for a tick with the velocity (`exit_vel`) and facing (`exit_yaw`) of the clip, motion matching resumes with a fresh choice and the switch is inertialised.  `failed` is 1 for the tick the answer is that there is nothing to traverse: a game script uses it to jump instead.

**Room beyond the ledge.** The sense also looks along the way in for a wall: onto a block, as far as its top reaches and a solid on it allows (`ahead` in the answer); beyond a thin obstacle, as far as the landing is free. The root of the clip is held to that room (`u_max`), so the character is not carried into a wall that the clip had no wall for, and with less than 0.3 m of room to stand the obstacle is not traversed at all.

**Only tall ledges can be hung on.** `hang_min` (m, 1.8) is how high a ledge must be for the pause, the letting go (`cancel`), the shimmy and the push off the wall below: a lower ledge (a mantle of a metre, a climb of up to 1.8 m) is always climbed out in full whatever is held (`hang_ok` says which it is for the move that is playing).

**Hold and let go.** A mantle or climb that has its hands on the ledge (the end of the clip's front-ledge window) *waits* there, the clip held at its pose (`hanging` is 1), until the script sets `hold` (the game: forward pushed towards the obstacle); letting go of `hold` before the root is over the top stops it again wherever it is, so the player has to keep holding forward to climb all the way. `cancel` lets go: the move ends at once (`dropped` is 1 with `exit`), the body is put out of the wall and drifts back, gravity is on again, the matcher's tags are set to `drop_tags` (the game: the fall loop, so that its first choice is a fall and not a walking clip of the tags it had on the ground) and the game's own fall takes over (the game: S or C). With `move_input` (any direction) a move ends a little early, as soon as the character is over the top (mantle) or has landed (hurdle, vault), instead of at the clip's blend-out moment, so that the player can move on sooner.

**Finishing, pushing off, shimmying.** Once the root of a mantle or climb has got over the height of the ledge (`above` is on) the move simply finishes: no more waiting for `hold` or `move_input`, the clip plays `fin_speed` times as fast and ends as soon as the root is over the edge, so the player is free quickly. A hold is eased too: the clip's speed goes to 0 and back over a few ticks, it never freezes from one picture to the next. `jump` (the game: Space) while climbing, 0.12 s after the move began and not yet above, **pushes off the wall**: a move of its own of `action` 4 (the file's `Wall_Jump`, entered at its `branch_end`, laid where the character is, its path stretched away from the wall by `push_scale`), it ends as soon as the character is off the wall (the move's `blend_out.at`) with `exit` and `pushed`, leaving with momentum: at least `push_vel` away from the wall and `push_up` up (`exit_vel`, `exit_vy`, the vertical one is given to the `CharacterBody`), straight into the falling clip: `push_tags` (the matcher's tags of the fall start) are set and the switch is nearly a cut (`push_exit_blend`, 0.05 s). `side` (the game: A is -1, D is 1) does nothing during a move except while the character hangs on the ledge: if the file has a `shimmy` (the hanging pose: `hands_up`, `hands_fwd`, `feet_up`, a clip each way and `speed`) and the ledge is wide enough and the feet would clear the ground, the character goes over to the hanging pose (`pose` 1: the capsule where the hands are on the ledge, the model lowered by `vis` where the ground would not let the capsule down), shifts along the ledge (`shim`, between `shim_lo` and `shim_hi`, the ends of the edge less the margin) while the key is held and holds the pose when it is let go; `hold` takes it back to the climb clip, from where it waited and from the new place.

**Hands and feet.** `ik_hand_l`, `ik_hand_r`, `ik_foot_l`, `ik_foot_r` are the uids of four `Ik` goal entities of kind 2 (pins) of the model's hands and feet: while the character hangs (and a moment before and after) the engine writes their places and weights each tick, easing them in and out (`ik_w`, `ik_wf`): both hands are held on the line of the ledge (on top of it and a little way over its edge in the climb clip's pose; in the shimmy's hanging pose the wrists are `grip` (0.1 m, from the file's `shimmy`) under the top and at the wall's face, so that the fingers curl over the edge instead of floating above it; wherever along the ledge the clip puts them: the hands go along with a shimmy), the feet are kept out of the wall in the climb clip's hanging pose and braced on the wall's plane in the shimmy's. All four are off again at the end. The shimmy only starts where the character stopped on the ledge (the end of the clip's front-ledge window), not where forward was let go of in the middle of the pull-up. `push_speed` is how many times as fast the wall jump's clip plays; exit velocities follow the playback speed. `pose_blend` is the inertialization span of the switches between the climb clip and the hanging pose.

**Air catch.** A character that falls beside a ledge can grab it. A script sets `catch_try` (every tick it should be looked for: the game, while the character is in the air and falling); the engine, when the character is not on the ground and not rising (`CharacterBody.fall` under 1.5 m/s), looks along its velocity (its facing when it is slow) for a `Traversable` block within `catch_reach` (0.9 m) of the feet whose top is between a hand's reach under them (0.15 m) and `catch_top` (2.2 m) above them, at least as deep as a mantle and with room on it. The moves of the file with `"catch": true` (`rel`: how far the ledge is above the root at the catch moment, `catch_dist`: how far before it; the front warp window ends at the catch moment, `hang_at`) are the candidates; the one whose `rel` is nearest the real ledge's height above the feet (within `catch_tol`, 0.5 m) and whose gait is nearest the character's (stand, walk, run) is entered `catch_lead` (0.18 s) before its catch moment, laid on the real ledge, and the way into the grab is a cubic (a Hermite curve) from where the character is, at the velocity it has (`catch_v`, from its `walk` and `fall`), to where the clip has the root at the catch moment, at the clip's own velocity there: the clips fall from a great height at about 15 m/s, a game's fall is slower, and the clip's fall laid over it would be a jump of the speed at the entry and a dive under the ledge. `caught` is on while the move is a catch, and a catch of any height (also a low ledge, a hurdle, a vault) can be let go of with `cancel` from the grab to half a second after it. From the catch on it is a mantle: a ledge `catch_hang` (1.1 m) or more above the feet is hung on until `hold` (the pause, the shimmy, `cancel`, the push off the wall), a lower one is climbed out at once. A catch over a **thin wall** (shallower than a mantle) is the same with the other two kinds of moves: with a floor beyond the wall that is more than `MIN_DROP` (0.5 m) and under `CATCH_MAX_DROP` (2.5 m) below its top the character catches the top and **hurdles** it (the move's front window is the catch, its back window puts the root over the real back edge and its floor window lands it on the floor beyond: `Check.floor_any`), with nothing to land on that near it **vaults** it (the move ends, with the speed of the clip's path, when the root has dropped under the top, and the character falls on); a wall with the floor right under its top (0.5 m or less) is not caught. These moves have `"action"` 2 (hurdle) or 1 (vault) with `"catch": true`, and no hang: they go on at once. A catch move is never chosen by the check on the ground. `make_catch.py` of the Lunaris game makes the moves of the Game Animation Sample's `Traversal_Catch_*` clips; `crates/lunaris-sim/src/traversal_tests.rs` has the tests.

**Smoothness.** `blend_in` and `blend_out` (seconds, 0: leave it) set the inertialization span of the switch into the move and out of it, back to the usual after a few ticks.

## Feet on the ground: `FootIk`

A clip plays the same wherever the character stands. `FootIk` (on the model, the entity with the `Animator` and a `Parent` that is the character with the `CharacterBody`) makes it stand on what is there: on stairs, on a kerb, on a slope of terrain, on a rock under one foot.

* **The table.** `data` names a file of the workspace (`lunaris footpaths MODEL.glb OUT.json [--prefix NAME] [--rate 30]` writes one for the clips of a model: `{"rate":30,"clips":{name:{"dur","l":[[x,y,z]..],"r":[..],"cl":[0/1..],"cr":[..]}}}`): where the clip puts each foot joint (model space: x left, y up, z forward, metres) every 1/rate seconds, and whether that foot is planted (it is within 4 cm of the lowest it gets). A clip that is not a loop on the spot (a stair clip made with the climb taken out of the hips) brings a table of its own with `"stance":1` and the planted flags worked out by its tool (`ue_export/retarget_extras.py`, `stairs` mode); `"lift"` moves its origin. Clips without an entry get no foot placement.
* **Where a planted foot goes.** Four rays go down under each foot (the heel, the joint, the ball and the toe, the sole counted 2 cm shorter at each end), and the foot goes where the whole of it is on **one tread**: the ground is read outwards from the middle of the foot until it jumps (a riser, an edge; a slope has no jump), and a foot that would lie across an edge is moved along the way the model faces until it is on the tread (it may hang 3 cm over a nose, never into a riser or the step behind going down). The foot is then **held** (a lock: Unreal's foot placement calls it a plant) where it was put until the clip lifts it (a tenth of a second without the planted flag, so that a switch of clips by motion matching does not let go of a foot on its step); but only a foot that had to be **moved** to fit (more than 1.5 cm) is locked, and a lock is on a leash (it never holds a foot more than 0.35 m from where the clip has it): on level ground the clip's own foot is where it should be, and a lock there would pin it to a place while the clip's foot goes on at the clip's speed (not the character's), pulling the leg out straight. The hold begins to take the foot `lead` seconds (0.15) before the clip plants it and lets go over `release` seconds (0.2), so a foot comes down onto its step and leaves it smoothly. The goals are `Ik` kind 2 pins: `goal_l` and `goal_r` (the up direction, and the way the model faces too where the foot was fitted to a tread or the gait places it: the foot is put on the tread in height and along its length; on level ground only the height is pinned, so that the foot keeps the place the pose on the screen gives it), weighted by the hold. A foot in the air is asked for the height the clip gives it over the ground it is above (so a step ahead is cleared as the foot comes over it, and level ground asks nothing of a swinging foot); a planted foot is put on the ground, in proportion to how planted the clip says it is. **Switches of clips.** After a switch (motion matching does it several times a second) the picture is the new clip offset by the old pose (inertialization, `Inertia`), and a foot that the new clip has planted may still be in the air on the screen: the hold on the feet rises with the offset dying away (the same quintic curve), not before. **Landing.** The goals come on over a tenth of a second after the character lands (`wk`): a foot that the jump clip has in the ground at the moment of landing is brought out of it, not thrown out in one picture. The other two goals, `floor_l` and `floor_r`, pin up only and one-sided: a swinging foot is kept out of the ground, the toes out of the riser ahead. `ankle` is how high the foot joint is above the sole, `heel` and `toe` how far the sole reaches behind and in front of it.
* **The model follows the ground.** Its origin (`LocalTransform` y) goes to the average of the ground along the way the character goes (eleven places over two metres, at spacings that do not repeat the pitch of any stairs, smoothed: the steps of a flight make a ramp of it) and is kept within reach of the planted feet (and of a foot that is coming down, which loosens the limits gradually): a leg may reach `slack_up` (6 cm) further than the clip has it and bend `slack_down` (25 cm) more. A stair clip, whose body is a treadmill, learns slowly how far its planted feet say the origin is from the line (`bias`). A critically damped spring (`smooth`, seconds) carries it, so there is no jump of speed when a foot is planted: the capsule jumps up a stair at a time, the picture rises smoothly. A game that follows the character with a camera should follow the **model** (the `Transform` of this entity), not the capsule: the capsule rolls up each edge and the camera would shake the picture of the character.
* **Stairs ahead.** The same rays look ahead along the way the character goes (`look` metres, plus a fifth of the speed, then 0.5 and 1 m further, and one 0.5 m behind): two steps in a row in one direction, or being on a flight already, sets `ahead` to 1 (up) or -1 (down); a single kerb is not stairs and neither is a slope (the rays must hit flat treads). A game uses it to pick a stair clip and to hold the pace to a stair's. `stride_down` (1 = off) shortens a walking clip's stride going down a flight.
* **The gait (hybrid: the clip for the body, a planner for the legs).** While a game sets `drive` (it does on stairs) the clip no longer chooses where the feet go. A planner does, with the same rays: its phase `theta` (0..1, a cycle is two steps) goes on by the distance the character covers (the smoothed `pace`) over two `pitch`es (the distance between steps, learnt from the treads: 0.25 m on a flight), so the feet land on the treads whatever the speed, and stand still with the character (a step in the air is finished in a third of a second). The left foot is in the air for the first `swing` (0.42) of a cycle, the right for the same share from the middle: when a step begins the foot that is behind goes to the tread a step ahead of the other foot (found by the fit above) and is carried there in an arc (`arc` metres over the higher end, forward travel starting after the foot has left its tread, a smooth landing). A foot on its tread has its joint as far above it as the clip's pose says (`dl` and `dr` of the table: how far below the joint the lowest of the heel and toe is: the foot rolls over the tread as the clip's does). The clip is played at the planner's phase (the engine sets `Animator.time` and `speed` 0; the table's `t_l0` is when the clip's left foot leaves the ground and `cycle` the length of two steps of the clip: a loop of several steps works), so that its arms and body swing with the feet that are really in front; the goals (kind 2 pins along up and the way the model faces) bend the clip's legs to the planner's feet, knees where the clip has them. The origin is the line of the ground kept within reach of the planter's feet (the same limits as above, counting a foot that is coming down as it comes). Going down is the same: no clip of descending stairs is needed (the Mixamo ones do not loop). When `drive` goes off the feet are held where they are (a lock) for the clip that plays next. `lunaris footpaths` writes `dl` and `dr`; the planner needs nothing else from the table.
* **Knees, and the gait's edges.** Every leg's goals have a **pole**: `pole_l` and `pole_r` are two entities that the engine keeps a metre ahead of the character at the height of its hips, and the knees bend towards them. Without a pole the solver bends the knee towards where the clip has it relative to the new hip-to-foot line, which flips when a leg that the clip has nearly straight meets a goal that has moved past the knee (the knee then bends backwards: it did, on stairs going down). The gait's cycle is taken between two real lift-offs of the clip's left foot (the pair whose distance is nearest the average): with the average length the cycle of an uneven loop of mocap ends in the middle of a stride, and the clip jumps by half a stride at every wrap of the phase. A foot of the gait is never asked for less than the floor under it, is lowered over a twentieth of a second when the floor drops away (the heel clearing the nose of the tread it leaves), and a step down is lifted first, goes forward over the edge, and is lowered by how far it has come. When the gait comes on or goes off (`smx`), and whenever a goal of a flat-ground foot would jump (a foot fitted to another tread), the goal's target eases to its place (a third of a second after the gait, 3.5 m/s along the ground and 5 m/s in height otherwise), and the hold on the foot lets go over `release` after the gait: no foot is thrown from one place to another in one picture.
* **Edges.** The ground at a place that is more than `EDGE_DROP` (0.7 m) from the character's own is another surface, both under a foot (a foot over the edge of the platform the character stands on keeps the clip's pose: its goal is off) and in the line the ground makes under the character (the floor behind the edge of the platform does not pull the pelvis down while the character is within a metre of it).
* **When it is off.** In the air, while a `Traversal` plays, and with `weight` 0 (a game eases it out for a slide, which has its own goals): the goals are off and the model comes back to the capsule. The picture only: the simulation never sees the bent legs. Everything is f32 with `+ - * /`, `sqrt` and the engine's own `sin` and `cos`: the same on every machine. It costs about 10 microseconds a tick on a flight.
* **Looking at it.** `lunaris posetrace SCENE --uid MODEL --hero CHARACTER --joints pelvis,foot_l,foot_r,ball_l,ball_r --ik 1 --world 1 --place x,y,z --script "25:;780:w"` prints the pose as the renderer poses it, with the goals applied and in the world, plus the numbers of `FootIk` (`fk_*`): measure the sole against the steps instead of guessing.

## Pictures between ticks

The simulation runs in fixed steps (60 a second) and a played game (`lunaris play`, the player) draws one picture per refresh of the display (144, 280 ...). Drawn as the world is, every state would show for two, three or five pictures in a row: the motion is stepped at 60 Hz and on a 144 Hz display unevenly (two pictures, then three), which looks like stutter and stiff animation. So the player draws a moment *between* the last two ticks: every entity's `Transform`, the clock of every `Animator` that plays on and the offset an `Inertia` is decaying are blended from a tick ago to now by how far the next tick is away (`crates/lunaris/src/interp.rs`). Only the picture is blended: the simulation never sees it (the originals are put back after drawing), so the game stays deterministic and what is saved, replayed or networked is unchanged. A thing that moved more than 6 m in a tick (a teleport), a clip that was switched or a time that was set (motion matching) are shown as they are; a world of more than 20,000 entities, a networked game, a hidden window and a self-test are drawn as the world is; `LUNARIS_NO_INTERP=1` turns it off. The price is that the picture is a tick (1/60 s) behind the simulation.

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

`lunaris play` (and an exported game) closes its window on Esc. A game that wants Esc for itself, a pause menu, sets the resource `app.quit_on_escape` to 0: Esc is then the key `input.escape` (bind it in an `InputMap`: `pause = escape, gamepad.start`). A game closes itself, a Quit button, by setting the resource `app.quit` above 0 (also in a hidden self-test). A self-test presses keys at set frames with `--press enter@45:4,s@80` (KEY@FRAME[:FRAMES]; one tick a frame), prints the resources with `--report` and saves the last picture with `--save-frame`: `tools/check_depths_export.py` uses them to play an exported folder. **`--hidden` with `--frames` opens no window at all** (one player; with `--join`, or without `--frames`, the player needs a window's event loop and still opens one): the simulation steps one tick a frame as in a window, the keys arrive at the same frames, and the last picture is drawn offscreen by the GPU renderer's headless adapter (Vulkan or DirectX needs no display for that) or, with `--cpu` or where there is no GPU, by the reference renderer, so a self-test runs over ssh, in a container and in CI, on Linux and Windows. Both give the same state hash. A menu that draws its own pointer hides the system one with the resource `cursor.hide` above 0.5 (`cursor.lock` hides it too, and holds it in the window), and a settings screen switches full screen with `display.fullscreen` (1 borderless full screen, 0 a window) and the GPU quality with `display.quality` (0 classic, 1 low, 2 medium, 3 high, 4 ultra); the player applies each when it changes (no effect with `--hidden`). `display.volume` (the master volume, 0 to 4), `display.fps` (a frame cap) and `settings.save` (keep the `opt.*` and `display.*` resources in `settings.json`) are in docs/UI.md, "Settings menus and going online from a menu".

## Languages: `Localization`

Put `{#key}` in any `Text.value` or `Ui.text`; one `Localization {"language": "en"}` entity picks `lang/en.json` of the workspace: `{"hello": "Hello", "menu": {"play": "Play"}}` gives `{#hello}` and `{#menu.play}`
(a key that is not in the file shows as the key). Setting `language` to `"de"` switches at once. `{resource}` placeholders still work in the same text: `{#score_label}: {score}`, and a translated text may hold placeholders of its own (`"stats": "Level {level}   Score {score}"` in the language file). A `language` with slashes is the path of the file without `.json`, so that a game keeps its texts beside its other files (`"language": "mygame/lang/en"` is `mygame/lang/en.json`; `examples/depths` does it); an export copies that whole folder of languages.

**Switching the language inside the game.** A graph system cannot set a text, so the choice is a number: put a **`LanguageList {languages}`** on the entity that has the `Localization` (`"languages": "en, de, fr"`, comma separated, in the order of your menu; names as in `Localization.language`, so `"mygame/lang/en, mygame/lang/de"` works) and set the resource **`language.index`** (0 is the first). Every tick, after the graph systems, the engine makes `Localization.language` the name at that place, so the picture of that very tick is in the new language; an index out of range goes to the nearest end and a fraction to the nearest whole number, and the resource says what is really on. Until a game sets it, the language the `Localization` starts with counts (the resource then says its place in the list). The engine also writes **`language.count`**, so a button cycles with one line:

```
system next_language each LanguageList {
    if $action.language_next.pressed > 0.5 { $language.index = ($language.index + 1) % $language.count; }
}
```

A settings screen shows the language it is in with a key of its own in every file (`"language": {"name": "English"}` in `en.json`, `"Deutsch"` in `de.json`, and `{#language.name}` on the button). The resources are saved by `game.save` and carried to the next scene by `SceneSwitch` like any other, so the choice stays; a scene without a `LanguageList` is not touched at all (no resource, no change of state). `check` reports a list without a `Localization`, an empty one, a language without its file, a repeated name and a start language that is not in the list. An export copies the files of every listed language.

**Where `{#key}` works.** In every `Text.value` and `Ui.text`, which includes what a conversation writes to its screens: the line, the choices, and the **speaker** (`{#npc.guard}: {#npc.hello}` in a `Dialogue` script shows the guard's name in the language that is on; a speaker is a `{#key}` of letters, digits, dots, underscores and hyphens, or a plain name of up to 24 characters). A line that is typed out (`Dialogue.speed`) shows the key itself until it is complete, so give a translated line `speed` 0 or keep its typewriter short.

## Inventories: `ItemTable`, `Inventory`, `InvSlot`, `InvCard`

Bags of item stacks on any entity (a hero's backpack, a chest, a corpse), an item and recipe table, and the binding of the interface kit's slots to them (`crates/lunaris-sim/src/inventory.rs`).

**The table.** One entity has an `ItemTable {text, icons, icon_cols, icon_rows}` (the lowest uid counts). `text` has a line per item and per recipe, `#` starts a comment:

```text
item wood | Wood | stack=50 weight=0.5
item iron_axe | Iron axe | stack=1 weight=2.5 class=1 power=9 dur=120 icon=14
item helm | Iron helm | stack=1 weight=3 class=8 kind=1 icon=20
recipe iron_axe 1 = wood 4, stone 6 @1
```

Properties: `stack` (default 50), `weight`, `class`, `power`, `hp`, `sp`, `secs` (numbers a game gives meaning to), `dur` (durability of one item), `icon` (a cell of the `icons` picture, a grid of `icon_cols` by `icon_rows`, counted from the top left, row by row) and `kind` (0 to 31: the `UiDrag.kind` of the item's card, so that a helmet slot's `UiDrop.accept` takes only what fits). Items are numbered 1, 2, 3 ... in the order of their lines; 0 is "nothing". A recipe is `recipe RESULT COUNT = INGREDIENT N, INGREDIENT N @STATION ~ SECONDS`: `@n` (1 to 31) needs bit n of the inventory's `station` mask, none (or `@0`) means by hand; `~ SECONDS` makes it a **timed craft** (below), none (or `~ 0`) means at once. Lines that are not understood are skipped (`parse_table` lists them).

**The bag.** `Inventory {size, bag, id[48], count[48], dur[48], ...}`: up to 48 slots; `id` is an item number (0 = empty). Ask by setting a request field from a graph system; the engine does it at the end of the tick (in the order `fill`, `wear`, `take`, `give`, `craft`), writes the answer and clears the request:

| Request | Answer | |
|---|---|---|
| `give_id`, `give_n` | `gave`, `left` | puts items in: onto stacks of the same item first, then into empty slots among the first `bag` (so the last slots can be equipment); `left` is what did not fit (and the event `inv_full`) |
| `take_id`, `take_n` | `took` | takes from the first slots that hold the item |
| `craft` (recipe number + 1) | `crafted` (the same number, or 0) | needs the ingredients, the station bit in `station`, and room for the result; otherwise nothing changes. **A recipe with a time (`~ SECONDS`) is never made at once by this request: it is a start of one timed craft** |
| `craft_start` (recipe number + 1), `craft_qty` (1 to 99), `craft_cancel` | `crafting`, `craft_left`, `craft_total`, `craft_todo` | a **timed craft**: the ingredients are taken at once (nothing happens if they are not there), after `~ SECONDS` the result is put in the bag (a bag with room for it: otherwise it waits and `inv_full` is emitted) and the next of the quantity starts when its ingredients are there; `craft_cancel` gives the ingredients of the craft in hand back. A recipe without a time is made at once, `craft_qty` times. On a game server the owner is told the craft in hand with the bag (the seconds left once, then the client counts them down) and the engine never makes the item on a client: an inventory beside a `NetPlayer` only counts `craft_left` down for the display. `have.crafting`, `have.craft_left`, `have.craft_total` and `have.craft_todo` are published like the other totals |
| `wear` (slot + 1) | | one use of a tool: `dur` goes down; at 0 it breaks (the slot empties, event `item_break`) |
| `fill` (`"wood 5, stone 3"`) | | starting loot, put in once |

A recipe line is `recipe RESULT N = ING N, ... @STATION ~ SECONDS`: the station and the time come after the ingredients in either order, and a time that is no number drops the line (it never becomes an instant recipe). A timed craft is refused (event `inv_full`) when its result would not fit once the ingredients are out; the ingredients of a cancelled craft go back only if they fit (a full bag keeps the craft and says `inv_full`); a craft whose result no longer fits when its time is up waits with `craft_wait` set (`PREFIX.craft_wait`) and says `inv_full` once; a start and a cancel in one tick cancel the new craft. `weight` is kept up to date; with `strict_weight` and `max_weight` above 0, things that would take it over are refused (`left`). `can` is a bit mask of the recipes that can be made now (bit n of `can[n / 32]`), for colouring a recipe list. With `publish` set to a prefix (`"have"`) the resources `have.KEY` hold how many of each item are held (`have.wood`), `have.weight` the weight, `have.sel` the selected slot (`sel`: the game's hotbar choice) and `have.held.id`, `.class`, `.power`, `.dur`, `.kind`, `.hp`, `.sp`, `.secs`, `.n` what that slot holds (a graph system reads `$have.held.class` instead of looking at slots): a graph system tests `$have.wood >= 5` without looking at slots (`have.can0` and `have.can1` are the `can` words and `have.free` the empty slots of the bag; `check` knows all of these names, so a page that reads them is not told that nothing sets them). Events: `item_given`, `item_taken`, `craft`, `item_break`, `inv_full`, `inv_move`.

**Stations as entities: `Station`.** A workbench, a forge or an electronics bench is an entity with a `Transform` and `Station {bit, radius, height}`: it unlocks the recipes with `@bit` (1 to 31) for every `Inventory` whose own entity is within `radius` metres of it along the ground (x and z, the rim counts) and within `height` metres above or below it (default 2.5, so the room above or below does not count; 0 = no vertical limit). The engine works it out every tick in the inventory step, on the server and on every client from the same entities, and **adds** the bits to the ones a graph system sets in `Inventory.station` (that field stays the game's own: the sum is used for `craft`, `craft_start`, the next craft of a quantity and `can`, and never written back). So a bench is one entity that a player can place, move, build or destroy, the client's own `have.can0` / `have.can1` account for it with no system of the game, and a game that never made a `Station` behaves bit for bit as before. With `publish` set, the resource `PREFIX.near` is the bit mask of the stations in reach (bit n set: station n at hand) for a hint over the hotbar; it exists only in a world that has a `Station` (and goes back to 0 when the last one is gone). A `Station` with no `Transform`, a `bit` outside 1 to 31 or a `radius` of 0 or less reaches nobody; `check` says so, and also when no recipe asks for the bit. The measuring uses only `+ - *` and comparisons: it is deterministic. A craft already in hand finishes whatever happens to the station (the rule is checked when a craft starts and before each next one of a quantity), so walking away from a bench keeps the craft in hand and ends the rest of a quantity.

**Why a craft was refused: `craft_denied`.** A craft that was asked for (`craft`, `craft_start`) and not started emits the event `craft_denied` from the inventory's entity **and** one that says why: `craft_denied_station` (the recipe's station is not at hand), `craft_denied_ingredients`, `craft_denied_room` (the result would not fit once the ingredients are out; a timed craft also says `inv_full`, as before) or `craft_denied_busy` (a craft is already in hand). Station is named before ingredients when both are missing. The same events say why the rest of a quantity could not go on (`craft_denied_station` after walking away from the bench, `craft_denied_ingredients` when the materials ran out); a quantity of an instant recipe that makes at least one and then runs out of ingredients is not a refusal, and a recipe number that does not exist is not either. Read them with an `EventWatch` (`names: "craft_denied_station, craft_denied_room"`); on a game server the events are in the server's simulation, so a client hears of them through `NetHost.forward` (docs/GAME_SERVER.md).

**Slots and cards (the picture and the dragging).** A slot is a `Ui` with a `UiDrop` and an `InvSlot {inv, index, label}`: the rectangle shows slot `index` of the `Inventory` of entity `inv`; `label` is a `Ui` that gets the stack count (shown above 1). The thing in the slot is a **card**: a `Ui` button with a `UiDrag {slot: SLOT_UID}`, a `UiStyle` and an `InvCard {slot: SLOT_UID}`. The engine hides a card when its slot is empty, puts the item's picture on it (`UiStyle.image` and `image_uv` from the table), sets `UiDrag.item` and `UiDrag.kind`, and when the card has been dropped on another slot it moves the stack in the inventories (onto a stack of the same item up to its size, swapping places, or into an empty slot, also between two inventories: a backpack and a chest) and puts the card back in its own slot. Make slots with `swap` on (the default); give an equipment slot an `accept` mask that has only its kind's bit. A card dropped on a `UiDrop` that is not an inventory slot (the ground, a bin) goes back and the resources `inv.out.uid` (that slot), `inv.out.id` and `inv.out.n` say what was dropped for one tick; a graph system can `take` it and spawn it in the world. `Ui.visible` of a card and of a label is the engine's: hide a whole panel with a `UiGroup`.

## Limits

* Conditions look at the entity that owns the machine (and resources). To react to another entity, copy its value into a resource with a graph system, or read it with a `Sensor`.
* One state machine per entity; give a second entity (a child) another one.
* Sensors see by position, not by size: a big thing counts from its centre.
