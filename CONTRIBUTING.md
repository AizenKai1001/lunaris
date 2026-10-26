# Contributing to Lunaris

Thank you for looking. Lunaris is a game engine in Rust whose first rule is **determinism**: the same scene, seed and commands give the same state, bit for bit, on every machine, operating system and thread count, and that is checked, not hoped for. The second rule is that **everything is a tool call**: whatever a person can do in the editor, an AI agent can do through MCP, and the other way round.

## Build and check

```bash
cargo build --release                         # the engine and the `lunaris` command
cargo test --workspace                        # every crate's tests, the fuzzers, the integration tests
python -I tools/check_all.py                  # everything: build, tests, MCP and client smoke tests, the editor window,
                                              # playtests, bots, the golden hashes, GPU-vs-CPU checks (about three minutes)
python -I tools/check_all.py --quick          # without the editor script and the long soak
```

Node 22+ is needed for the MCP and JavaScript client tests only. Without a GPU the GPU tests skip themselves. On a machine with a card, run the whole graphics suite before you change the GPU renderer: `cargo test -p lunaris-gpu --test compare --test high` (`LUNARIS_GPU=name` picks the card on a machine with two). The public CI runs only the classic-quality comparisons on a software adapter, where a single high-quality picture takes minutes (`docs/PERFORMANCE.md`, "The card"); `lunaris bench --gpu` and `cargo run --release -p lunaris-gpu --example upload_cost` measure a card.

## The rules that keep it deterministic

- **Simulation code uses only IEEE `+ - * / sqrt`** and the `detmath` functions (sin, exp, ... written in the crate so that every platform gives the same bits). No `f32::sin`, no `powf`, no `HashMap` iteration order, no wall clock, no thread-dependent reductions. Randomness comes from `hash_rand(seed, tick, uid, salt)`, never from a global generator.
- Anything that decides the future is in the state hash (`Sim::state_hash`); anything that does not (events, caches, last timings) must not change the future.
- The GPU renderer may differ between graphics cards; nothing that hashes or replays may depend on it. The CPU reference renderer is exact.
- A change that alters behaviour on purpose also changes `tools/golden_hashes.txt` (`python -I tools/xplat_hashes.py --write`). Say why in the commit message. The same file is checked on Windows and Linux.
- The core crates (`lunaris-json`, `lunaris-ecs`, `lunaris-sim`) have **no third-party dependencies** and `lunaris-net` only the standard library. A new dependency in another crate needs a reason.
- Every tool is advertised in `tool_defs()` and dispatched in `Engine::call`; a test ties the two lists together. A new component goes in `register_standard_components` and has a doc comment (that text is what the `schema` tool tells an agent; a test checks that every component has one). Errors carry a stable `code`, a `path` and a `hint`.
- Paths a tool accepts go through `lunaris_json::safe_relative` (the same answer on every OS).

## Style

- Plain data, small functions, comments that say **why** (the lessons are in the build log: a test that failed and what it taught). Match the code around you.
- Write the test first when you can; a bug found by a fuzzer or another platform gets a regression test.
- Docs live in `docs/`; `python -I tools/check_docs.py` fails if a document mentions a file that does not exist. A feature without a page in `docs/` and an example under `examples/` is not finished.

## Before you publish a copy

`python -I tools/export_public.py` writes a clean copy of the tracked files after scanning them for personal paths, e-mail addresses and key-like strings. It never pushes anything.

## Licence

Contributions are accepted under the same terms as the project: MIT or Apache-2.0, at the user's option (see `LICENSE-MIT` and `LICENSE-APACHE`).
