# Wyrdwood: what building it found in the engine

The game is planned in `examples/wyrdwood/PLAN.md`. This file is the honest list of what was in the way while building it, one line per finding, newest last, like `DOGFOOD.md` and `DOGFOOD_3D.md`. "Fixed" means it was fixed in the engine, with a test.

| # | Where | What was in the way | What was done |
|---|---|---|---|
| 1 | characters | A `CharacterBody` could not swim, roll, climb or pull itself up a ledge: the swim of Caldera was a game system pushing `walk` by hand with `InWater`. | **Fixed (2026-10-15)**: `CharacterMotion` (wade, swim, dive, roll with a cooldown, ladders with `Climbable`, ledge pull-up, a swimmer leaving over a dock) in `motion.rs`, 12 tests. |
| 2 | characters | The three traps Caldera found: characters reported no contacts, a `Steer` could not walk a `CharacterBody`, a system could not read the ground. | **Fixed for characters that opt in (2026-10-15)**: `CharacterMotion.report_contacts`, `Steer` on a character, and the `Probe` component (`DOGFOOD_3D.md` rows 10 to 12). |
| 3 | editor | A plan of a game lived in Obsidian, not in the editor the person works in. | **Fixed (2026-10-15)**: the editor has a **Canvas** tab (`canvas_ui.rs`) that reads and writes the JSON Canvas files Obsidian uses, follows the file when another program saves it, and opens with `--canvas FILE`. |
