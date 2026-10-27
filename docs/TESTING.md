# Testing the engine

What has to be green before a change is shared, and the tests that run the engine against real data instead of the small scenes a test makes up. A claim in these pages counts when a command or a test reproduces it; this page says which.

## The gates

| Gate | Command | What it promises |
|---|---|---|
| Unit and integration tests | `cargo test --release -p <crate>` (every crate under `crates/`) | each crate does what its tests say; the editor's tests drive it with synthetic pointer and key events and need `--features eframe/x11` on Linux |
| Golden hashes | `python3 tools/xplat_hashes.py target/release/lunaris --check` | every example scene gives the same state hash after 600 ticks and the same reference picture on every machine and operating system (`tools/golden_hashes.txt`) |
| Performance | `python3 tools/perf_gate.py` (`--gpu`, `--studio`) | no benchmark row got slower than the stored baseline of the same machine, and an entity costs no more memory (`tools/perf_baseline.json`, [PERFORMANCE](PERFORMANCE.md)) |
| Documentation | `python -I tools/check_docs.py` | every path a page names exists |

## Tests that use every example

The scenes in `examples/` are the repository's demonstrations: scenes made by different tools at different times, with the values a real scene has (negative zeros, positions thousands of metres out, systems that use the rarer statements). A test that loops over all of them finds what a scene made up for the test does not.

| Test | Crate | It requires |
|---|---|---|
| `every_example_scene_saved_and_loaded_again_is_the_same_world` | `lunaris-api` | `save_scene`, the text, `load_scene`: the same state hash, and a second save is the same text |
| `every_system_of_every_example_survives_the_trip_through_script_text` | `lunaris-api` | each graph system written as Lunaris Script and read back is the same system (the order of the keys is not compared) |
| the Blueprint tests in `blueprint.rs` | `lunaris-studio` | each system becomes nodes and goes back to the same JSON |
| `rows_with_real_values_and_every_reset_arrow_showing_fit_in_the_inspector` | `lunaris-studio` | the Inspector's rows fit the panel with the numbers of real scenes |

What they found: a negative zero written as `-0` and read back as an integer (the sign was lost, a reloaded scene had another state hash); and drag boxes in the Inspector 3 to 28 points wider than the panel.

## Probes: the same on any workspace

These are `#[ignore]`d because they print what they find, or take a minute, or need a workspace of yours. Run them with `-- --ignored --nocapture`.

| Probe | Crate | Run it with | What it does |
|---|---|---|---|
| `every_example_scene_check_findings` | `lunaris-api` | `cargo test --release -p lunaris-api every_example_scene_check -- --ignored --nocapture` | runs `check` on every example and prints each finding: an error here is a mistake in an example or a rule that is wrong (it found three rules that were: ball and hinge joints called ropes, a terrain with no mesh called a flat picture, a `view:` texture called a missing file) |
| `every_example_scene_in_the_editor` | `lunaris-studio` | `cargo test --release -p lunaris-studio --features eframe/x11 every_example -- --ignored --nocapture` | opens every example in the editor, selects up to 25 entities of each, Duplicate and Delete and undoes them (the saved scene must come back, not counting the next free uid: uids are never used twice), Play and Stop (the state hash must come back), runs `check`; about a minute |
| `real_scene_in_the_editor` | `lunaris-studio` | `LUNARIS_BIG_ROOT=/your/workspace LUNARIS_BIG_SCENE=scenes/x.lunaris.json cargo test --release -p lunaris-studio --features eframe/x11 real_scene_in_the_editor -- --ignored --nocapture` | one big scene of yours in the editor: what opening it, the Hierarchy, every bottom tab, Play, Stop and Save cost, and the pictures it takes (`LUNARIS_SHOTS=dir`) |
| `one_example_entity` | `lunaris-studio` | `LUNARIS_ONE=examples/x.lunaris.json:5 cargo test ... one_example_entity -- --ignored --nocapture` | one entity of one scene in the Inspector, with how far its rows run past the panel |
| `every_example_scene_load_times`, `check_cost_on_a_big_scene` | `lunaris-api` | `cargo test --release -p lunaris-api every_example_scene_load_times -- --ignored --nocapture`; the second takes `LUNARIS_BIG_ROOT`, `LUNARIS_BIG_SCENE` and `LUNARIS_RUNS` | how long each example takes to load, check and save (all under 20 ms, a 2,000-entity world 18 ms to load), and what `check` costs on one scene run again and again (12 ms on that world; its first call, the cold one, took 166 ms) |
| `damaged_example_scenes_never_panic` | `lunaris-api` | `LUNARIS_FUZZ_ROUNDS=300 cargo test --release -p lunaris-api damaged_example_scenes -- --ignored --nocapture` (a short run, 4 rounds of every example, is in the regular suite) | the examples with numbers damaged (0, negative, 1e30, tiny, NaN, infinity) anywhere in them and, every third round, with values of another type or entries taken out, each loaded, stepped and checked: no panic and no runaway allocation (20,000 scenes in 90 s; it found a static collider 1e30 wide making a 4 GB hash table). Run the binary under `ulimit -v` and read `LUNARIS_FUZZ_TRACE=1` to see which damage kills the process |
| `damaged_model_picture_sound_and_effect_files_give_errors_and_not_panics` | `lunaris-api` | `LUNARIS_FUZZ_ROUNDS=400 LUNARIS_FUZZ_FILES=60 cargo test --release -p lunaris-api damaged_model_picture -- --ignored --nocapture` (a short run is in the regular suite) | the example glTF and GLB models, PNG and JPEG pictures, WAV sounds and effect JSON with bits flipped, cut short, runs of 0x00 or 0xFF, bytes inserted, stretches swapped and length fields set to 0, 1, 65535 or 4 billion, given to every loader of their kind (the model importer, the collision mesh, the PNG and JPEG decoders, the heightfield, the WAV, Ogg and MP3 reader, the JSON parser and the effect parser): 34,800 damaged files, no panic, no hang, no runaway allocation. A watchdog ends the run when one input does not come back for two minutes |
| `damaged_script_text_gives_errors_and_not_panics` | `lunaris-api` | `LUNARIS_FUZZ_ROUNDS=1500 cargo test --release -p lunaris-api damaged_script_text -- --ignored --nocapture` (a short run is in the regular suite) | the Lunaris Script text of each of the 360 graph systems of the examples with characters removed, repeated, swapped, replaced, cut off, a stretch repeated 30 times (nesting, long lines) or junk words, brackets and numbers put in, given to `compile_script` and `define_script`: 540,000 damaged texts in 84 s, no panic |
| `real_scene_scripts`, `real_scene_saves` | `lunaris-api` | `LUNARIS_BIG_ROOT=... LUNARIS_BIG_SCENE=... cargo test --release -p lunaris-api real_scene_ -- --ignored --nocapture` | the script round trip and the save and load of one scene of your workspace, with where the first difference is |
| `real_scene_systems` | `lunaris-studio` | the same variables, `... real_scene_systems ...` | the Blueprint round trip of one scene of your workspace |
| `big_scene_editor_costs`, `startup_cost` | `lunaris-studio` | `... big_scene ...`, `... startup_cost ...` | what the editor costs with 10,000 entities and at start; `tools/perf_gate.py --studio` gates the first |

A game server is checked the same way by running it on every example: `lunaris server --scene examples/x.lunaris.json --players 4 --seconds 2` starts and stops cleanly on all of them (see [GAME_SERVER](GAME_SERVER.md) and [SERVER_ROBUSTNESS](SERVER_ROBUSTNESS.md)).

## Writing a test that finds things

- Prefer real data to made-up data: loop over `examples/`, or take a scene of your own project through the probes above.
- Compare what a user would see (the saved scene, the state hash, the pixels, the width of a panel), not what the code did.
- A test that writes into the workspace must write into a folder of its own; the examples are read-only.
- Print the first difference, with some text round it, not only "they differ".
