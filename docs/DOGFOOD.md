# Dogfooding Lunaris: building "Depths", a whole small game, with only the engine's tools

The question was "how close is this to a fully usable game engine?". The honest answer before this exercise was: nobody has built a complete game with it, only small
examples. So one complete game was built end to end (a top-down dungeon crawler: three generated levels, menus, pause, game over and win screens, saved games, music,
a conversation, two languages, an export) using only the tool API, text data and Lunaris Script, and **every time the engine got in the way it was written down here**:
fixed when small (with a test), documented when big. This is the honest list, not a success story.

The game is `examples/depths/` (scenes, art, sound, language files, `shots/`), made by `tools/build_depths.py` (art `tools/make_depths_art.py`, sound
`tools/make_depths_audio.py`, playtests `tools/make_depths_tests.py` into `examples/tests/depths_*.test.json`, pictures `tools/depths_shots.py`, export proof
`tools/check_depths_export.py`). `docs/GAMES.md` ("A fifth game: Depths") says how it plays and which patterns in it are worth copying.

## What it took

Wall clock by the machine's clock: about 1 hour 10 minutes (04:15 AM to about 05:25 AM), roughly 230 tool calls, one agent, the build and every test on a Linux test machine (the PC was not
used for anything heavy). Of the calls, about a third were engine work (the gaps below), a third building and debugging the game, a third tests, docs and pictures.

| Part | Roughly | Calls | How it went |
|---|---|---|---|
| Reading `CLAUDE.md`, the docs, the dungeon and village builders | 6 min | 15 | Fast: the village and dungeon builders are a good template (a Python script that drives `lunaris serve`). |
| Engine gaps that stopped the design on paper (Esc, quit, export chains, language folder, saves probe) | 25 min | 35 | Each small; a Linux test machine build (about 2 minutes) was the slow part of every iteration. |
| Art and sound scripts (pixel sprites, three tilesets, a lantern mask, two music loops) | 10 min | 10 | Pure Python, no engine involved; the first look at the sheets was enough. |
| `build_depths.py`: levels, hero, guards, pickups, HUD, pause, scenes | 20 min | 30 | The tools did the hard part (a connected level with a start, exit and spawn points in one call). |
| First pictures and the three bugs that made them wrong (see 7, 8, 9 below) | 15 min | 25 | The slowest stretch: the hero was invisible, then guards never hurt it. |
| Playtests (bots on BFS paths, menus, pause, saves, dialogue, pad, a whole run) | 20 min | 40 | The `verify` format (`until`, `expect`, `replay_check`) made bots robust: one spec language for everything. |
| Docs, golden lines, soak, export proof, screenshots | 20 min | 45 | |

## Findings

`Fixed` says what was done; "(not fixed)" ones have a proposal. Commits: A is `d0128f4`, B is `42a8fa4`, C is the last commit of the branch.

| # | Area | What happened | Fixed? | Commit |
|---|---|---|---|---|
| 1 | player | `lunaris play` (and every exported game) closes the window on Esc, so a pause menu on Esc was impossible, and a game had no way to close itself (a Quit button). | **Fixed**: the resource `app.quit_on_escape` set to 0 turns Esc into the key `input.escape`; the resource `app.quit` above 0 closes the player (also in a hidden self-test). Old games keep quitting on Esc. Unit test. | A |
| 2 | export | `export_game` copied only the files named by the *start* scene. In a game made of scenes (`SceneSwitch`) the level files were copied but **not what they use** (their textures, fonts, sounds): the exported game would draw placeholders from level 2 on. | **Fixed**: scenes the game can switch to are read too, down the chain (`export_tests::a_game_of_several_scenes...`). | A |
| 3 | export | A `MusicPlayer` playlist (one track a line) is a multi-line string, which the file finder skipped: **music never reached an export**. | **Fixed**: the first word of every line of a text field is a candidate file. | A |
| 4 | localization | `Localization.language` was a bare name and always meant `lang/<name>.json` at the workspace root: every game of a workspace shares one folder (`village_en` is prefixed by hand) and an export copies every game's languages. | **Fixed**: a name with slashes is the path of the file beside the game (`mygame/lang/en` is `mygame/lang/en.json`); the export copies that folder. | A |
| 5 | saves | A title screen cannot know whether a saved game exists, so Continue cannot be lit or greyed; `game.load` of an empty slot only sets `game.error` after the fact. | **Fixed**: the resource `game.probe` (a slot) is answered with `game.has.<slot>`. | A, B |
| 6 | check | `has an Animator but no MeshRenderer` was reported for every 2D flipbook sprite (`Sprite` + `Animator` + `SpriteAnim`): 20 to 30 false warnings on every scene of the game, which hides real ones. | **Fixed** (and tested). | A |
| 7 | camera | `CameraRig` defaults to `look_at` with a `look_offset` of [0, 1, 0]: a 2D follow camera is tilted five degrees, and because the layers are sorted by their depth along the tilted view, **a big tilemap came out in front of the hero and every sprite: an invisible game** (about 8 calls to find). | **check now warns** (orthographic camera + rig that looks + a Tilemap); documented in `GAMEPLAY.md` and `AI_GUIDE.md`. The default is not changed (it would change the hashes of existing scenes). | A |
| 8 | recording | An in-game `game.load` replaced the whole `Sim`, **dropping the recording in progress**: any playtest of Continue ended with `not_recording`, and a replay stopped at a tick number the load had gone back past. | **Fixed**: the recording survives a load, and counts steps instead of ticks (`Recording.steps`); old recordings still read. Tests. | A |
| 9 | replay | `game.probe` and `game.load` read files, which are not part of the recorded history: a run that created `saves/slot1.json` could not be replayed (the replay found the file where the run had not, so its state differed), found because the bot that plays the whole game failed `replay_check` the first time `saves/` was empty. A replay also wrote the saves again. | **Fixed**: a recording keeps what the files answered (`reads`), a replay is given the same answers and writes no files. Test with a changed file. | B |
| 10 | script | `$game.has.1` (a number between dots) did not lex; the engine's own resources have such names. | **Fixed** (lexer + test). | B |
| 11 | l10n | A translated text could not hold placeholders (`"Level {level}   Score {score}"` in the language file showed the braces): the game over and win screens printed `{score}`. | **Fixed** (renderer + test). | B |
| 12 | physics / 2D | The z of a 2D sprite is both its drawing order and, for solid bodies, a position in 3D: giving the hero a bigger z than the guards (to draw it on top) made every contact push them apart along z, and **the guards drifted out of the plane and never bit** (5 calls: their z had gone to -0.7). There is no drawing-order key apart from z. | **check warns** about solid bodies at different z under an orthographic camera (tested). The design gap is **not fixed**: proposal `Sprite.order` (a sort key that is not a position); until then bodies share z 0 and the uid decides who is on top. | B |
| 13 | player | A hidden self-test can hold keys (`--hold`) but not press them at a time, so no exported game could be taken from its menu into a level by a script. | **Fixed**: `--press enter@45:4,s@80` (key, first frame, frames held). `tools/check_depths_export.py` uses it. | B |
| 14 | CLI | `lunaris schema | head` panicked on the closed pipe. | **Fixed** (also `tools`). | B |
| 15 | tools | `tilemap_colliders` gives its boxes the uids after the map's. A scene that then spawns its own entities with uids 20, 21, 30 gets `duplicate_uid ... already exists` with no word about the boxes. | Not fixed. Proposal: a `first_uid` argument, and the error's hint should say which tool took the uids. Documented in `AI_GUIDE.md`. | |
| 16 | scenes | `SceneSwitch` + `Persist`: the scene that holds a persistent entity cannot be switched back to (uid clash), so the title cannot hold the music. Needed an extra first scene (`boot`) that nothing returns to. | Not fixed, documented (`GAMEPLAY.md`). Proposal: a scene's entity whose uid is already persistent is skipped, not an error. | |
| 17 | scenes | Resources carry over and **win over a new scene's own defaults**, so a scene cannot reset anything by declaring it: every scene needed a first-tick system (`scene_init`) clearing `paused`, `leave`, `time.scale`... | Not fixed, a pattern in `GAMES.md`. Proposal: `SceneSwitch.reset` listing resources the new scene's defaults win for. | |
| 18 | check | `check` calls a `SceneSwitch.path` that is not a file an *error*: scenes that name each other in a circle (game over -> level -> game over) can never all pass the first time they are built. | Not fixed; the builder writes stub files first. Proposal: a warning while the scene being built is not saved yet, or a `check {allow_missing_scenes}`. | |
| 19 | UI | `UiNav` focuses nothing until a direction is pressed: `ui_accept` on a menu with no focus does nothing, and the buttons that appear (a pause menu) lose the focus. Every menu needs `UiNav.current` set by hand when it opens. | Not fixed, a pattern in the game (`pause_focus`). Proposal: `UiNav.auto` (focus the first visible button when none is). | |
| 20 | UI / l10n | A graph system cannot set a string, so **a language switch inside the game is impossible**; a `Dialogue` speaker name cannot be a `{#key}`. | Not fixed (documented). Proposal: `Localization.language_index` or a number-driven choice among files. | |
| 21 | UI | `Ui` sizes are fractions of both width and height: icons and buttons stretch off 16:9 (checked at 4:3 and 21:9; text stays right, boxes do not). | Not fixed. Proposal: an aspect lock and anchors (a corner plus pixels). The world picture keeps its height, so the dark lantern had to be made much larger than the screen to cover a wide window. | |
| 22 | verify | `call` steps in a playtest cannot read what a tool answered, `load_scene` is refused in the middle of a recorded run (so a bot cannot go back to the title with it: the bots use `SceneSwitch`), and a test that saves leaves `saves/` in the repository. | Not fixed; `/saves/` is now ignored, the golden and soak scripts start from no saves. | C |
| 23 | determinism | The thread soak (1 against 8 workers) of a game that reads files compared two *live* runs of which the first had written the second's past: a false alarm that looked like a thread bug (boot scene, different hashes). | The soak and the golden script now start every run without `saves/`; no real thread-count difference exists (all seven scenes pass). | C |
| 24 | tests | `tools/xplat_hashes.py` and `tools/soak.py` only looked at `examples/*.lunaris.json`, so a game with its scenes in a folder was in neither. | **Fixed**: both read `examples/depths/` too; seven golden lines added, **no other line changed**. | B, C |
| 25 | golden | `examples/village.lunaris.json` frame hash in `tools/golden_hashes.txt` (`ec0d3ec33cdb55df`) is not what a Linux build gives (`822a6083c3ce9c15`): three other builds on a Linux test machine, including ones from before this branch, agree with each other. | Not caused here and **left alone** (only lines were to be added). Someone should regenerate it on Windows and Linux. | |
| 26 | player | A hidden window still needs a display on Linux (`DISPLAY=:0` over ssh): there is no truly headless player, only a headless `serve`. | Not fixed; the export check tries `DISPLAY=:0`. | |
| 27 | lighting | Sprites are not lit by `Light`: I did not try, and the darkness round the hero is one large sprite with a transparent middle (`lantern.png`), plus soft additive clouds as torch glow. It works and costs nothing, but a real 2D light would be better. | Not fixed. | |
| 28 | time | With `time.scale` 0 a graph system's `dt` is 0, so anything timed by `dt` (a fade) freezes: the pause menu must set the scale back before a fade-out starts. | Documented behaviour; handled in the game. | |

## What was easy

- **A level in one call.** `generate_map` + `autotile` + `tilemap_colliders` gave a connected, solid, drawn level with a start, an exit and spread-out spawn points; three different generators (rooms, caves, a maze) needed only a different `mode`. The only level logic written by hand is the door (a chokepoint cell found by trying each cell of the way) and the key (far from the start and the door).
- **Guards.** `Sensor` with line of sight and `Steer.seek` worked the first time: a guard sees the hero across a room, not through a wall, runs at it, loses interest after a few seconds. Bats (no line of sight, faster) were a parameter set.
- **Menus.** `Ui` + `UiStyle` + `TextFont` + `UiNav` + `InputMap` gave a title screen that works with keys, a pad and the mouse and looks good with no art. The Dialogue copied from the village worked unchanged.
- **Scene switching, saving, fading.** `SceneSwitch` + `ColorGrade.fade` + `game.save` / `game.load` did what they say.
- **Effects.** `ParticleModules` torches, the sparkle and death clouds restarted by `Particles.time = 0`, bloom and a vignette from `Environment`: a few lines each.
- **Tests as data.** `verify` with `until` made the bots independent of exact timing (hold a key until the hero's x reaches a cell); `replay_check` caught two real engine faults (8 and 9). A bot that plays the whole game, title to win screen, is one 400-step JSON file made from the scenes.
- **Export.** One command, 22 files, and the exported folder, started with a scripted Enter press, plays the same game tick for tick as the workspace.
- **Building and running on a separate Linux test machine** (copy the tree, build, run) was reliable; `serve` over stdin made a build script and a debugging probe the same kind of program.

## What was painful

- **Not seeing the game.** Everything is a still picture or a number: to know whether a guard hunts or a door blocks you have to build a probe. About a quarter of the calls were probes (`get`, `render` with `visible`, `contacts`).
- **The two bugs that looked like "nothing happens"** (7 and 12): an invisible hero and guards that never bite had causes (a camera tilt, a z drift) that no tool pointed at; both are `check` rules now.
- **Fields and defaults.** The schema lists fields but not what each default does to a 2D game (the CameraRig's tilt, `HitPoints.hurt` starting at 100, `UiNav` focusing nothing); I read the source several times (scene switching, the export, the player, `UiNav`) to be sure.
- **Scene switching has sharp edges** (16, 17): the first design (persistent hero and music) did not survive them.
- **Every engine fix costs a two minute release build** on a Linux test machine, so the cheap loop is the Python one; engine fixes were batched.

## What is still missing for a game like this to ship

- A settings screen (volume sliders on `bus.NAME.volume` are easy, key rebinding is not there), several save slots in a menu, and a language switch (20).
- Draw order apart from z (12), 2D lighting (27), animated tiles, UI that keeps its shape on other screens (21), nine-slice panels.
- A windows export that was run on Windows: only the Linux export was proved here (the PC was left alone); installers, icons, a window title.
- Sound made by hand: the music is arithmetic (`make_depths_audio.py`) and good for testing, not for release. No voice, no mix by ear.
- Content: the game is ten minutes long as designed (three levels), the guards are simple, and a bot that only walks the shortest way survives all three levels with the guards alive, so the balance needs a person to play it.
- A real headless player (26), and a way to look at the running game from outside (a streamed picture), which would have saved a quarter of the work.
