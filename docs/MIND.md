# Minds: a language model playing one body

A *mind* drives one entity in a Lunaris world. It is not a special feature of the engine: the mind is a client of the same tool API (`Engine::call`) that the editor, the CLI and an MCP agent use. What the `lunaris-mind` crate adds is the loop around it.

```
observe  ->  ask the model  ->  check permissions  ->  run the tools  ->  step the world  ->  observe ...
```

## Try it offline

```bash
lunaris mind examples/arena.lunaris.json --body 1 --goal "reach the Goal" \
  --scripted examples/mind_walk.scripted.json --turns 16 --think-ticks 30 \
  --record target/renders/walk.recording.json --transcript target/renders/walk.transcript.json
```

`--scripted` plays back a fixed list of replies (a JSON array of strings or objects) instead of calling a model. It is how the loop is tested and demonstrated with no network and no GPU. The output ends with `replay without the model: identical`: the session was recorded as ordinary commands, so the same world state comes back without any model at all.

## Use a real model

```bash
lunaris mind examples/arena.lunaris.json --body 1 --goal "reach the Goal" \
  --endpoint http://127.0.0.1:8080/v1 --model qwen3-4b --turns 20
```

`--endpoint` is any OpenAI-compatible chat-completions server (ForgeInfer, llama.cpp, LM Studio, Ollama's `/v1`). There is no default endpoint: a model is called only when you pass one. `--key-env NAME` reads a bearer token from that environment variable. `https://` is not supported because Lunaris carries no TLS code; use a trusted network or a tunnel.

For a small model on a local server (ForgeInfer serving a 4B model, llama.cpp, LM Studio) these settings matter:

```bash
lunaris mind examples/arena.lunaris.json --body 1 --goal "reach the Goal" \
  --endpoint http://127.0.0.1:8080/v1 --model npc-mind --json-schema --no-thinking \
  --temperature 0.3 --max-tokens 200 --turns 20
```

| Option | What it does |
|---|---|
| `--json-schema` | sends the reply schema as `response_format` (`json_schema`, strict): a server with constrained decoding can only answer `{"thought": ..., "actions": [{"tool": <an allowed tool>, "args": {...}}]}`, so the reply always parses and never invents a tool. `lunaris_mind::reply_schema(&cfg)` builds it. |
| `--no-thinking` | asks the chat template not to think first (`chat_template_kwargs.enable_thinking = false`; servers that do not know it ignore it). |
| `--temperature` (0.3), `--max-tokens` (300), `--model-seed N` | sampling; a low temperature picks actions more reliably, a seed makes a test repeatable. |
| `--history N` (16) | messages kept. The history only grows by appending until it passes N; then the oldest half goes in one step. |
| `--context-tokens N` | the server's context window (`--ctx` of the server): the history is cut before system prompt + history would use three quarters of it, instead of the server refusing the call. |
| `--timeout S` (120) | seconds per call. A "server busy" answer (HTTP 503) is retried with growing waits; a "context too long" answer drops the older half of the history and asks again. |

**Why the history is cut in halves.** A model server keeps what it already read of the last prompt and reads only what is new, but only while the new prompt *starts with* the old one. Forgetting one turn at a time would change the start of every prompt; cutting half at once keeps most turns a pure append (a reply of about 60 tokens costs about a second on a 4B model; re-reading three thousand tokens of history on every turn would double that). The system prompt is built the same way: everything that is the same for every mind comes first and the body and the goal come last, so a server that caches prompt starts reads the shared part once.

**Debugging a slow or odd session.** `--transcript` records, for every turn, what the server said about the call (`model`: elapsed time, token counts, `cached_tokens`, timings), and the run prints one line per turn, for example `(prompt 812 tokens, 790 cached, reply 64, 1010 ms)`. A `cached` number far below the prompt size means the prompt start changed.

## What the model sees and may do

Each turn the model gets the system prompt (the goal, the reply format, and only the tools it is allowed to use, taken from the live tool list) and an observation:

| Field | Meaning |
|---|---|
| `tick`, `seconds` | simulation time |
| `you` | all components of the body |
| `nearby` | other entities within `radius` (default 12 m), nearest first, with names and positions |
| `touching` | entities in contact with the body, with the direction of the contact |
| `results_of_your_last_actions` | what each action did, including errors with their `hint` |
| `notes` | things the loop wants to tell it (an unusable reply, too many actions) |

It answers with one JSON object: `{"thought": "...", "actions": [{"tool": "set", "args": {...}}]}`. Free text or a code fence around the object is tolerated; a reply with no usable object is reported back on the next turn instead of stopping the run.

## Permissions

- `allowed_tools` is an allow-list (default: `get`, `query`, `nearby`, `raycast`, `contacts`, `set`, `add_component`, `remove_component`). A tool outside it is refused with a message listing what is allowed.
- The write tools (`set`, `add_component`, `remove_component`) work only on the mind's own body. The `uid` is filled in when left out, and any other `uid` is refused.
- At most `max_actions` (default 6) run per turn.

The world is never reached through any path the other clients do not have, so everything a mind does is also visible, undoable and replayable like any other edit.

## Settings

| Option | Default | Meaning |
|---|---|---|
| `--think-ticks` | 12 | simulation ticks between decisions (30 ticks = half a second at 60 Hz) |
| `--turns` | 20 | decisions to make |
| `--record FILE` | none | write the session as a recording |
| `--transcript FILE` | none | write every turn (observation, raw reply, actions and results) as JSON |

## Playing a game

A mind can play a game the way a person does: with the `input` tool. Allow it, widen what the mind can see, and give it the body that carries the game's components:

```bash
python -I tools/mock_model_server.py --port 8099 &        # a stand-in for a model; any OpenAI-compatible server works
lunaris mind examples/orb_dash.lunaris.json --body 4 --goal "collect all 12 orbs"   --endpoint http://127.0.0.1:8099/v1 --tools get,nearby,input --radius 80 --nearby-limit 60 --think-ticks 5 --turns 190   --record runs/orb.json
```

`--tools` replaces the allow-list; `--radius` and `--nearby-limit` set how far and how many entities the mind sees; `--max-actions` caps actions per turn. The observation also carries a `world` object with the game's resources (score, time left; the raw `input.*` state is left out), and when `input` is allowed the system prompt explains it. The run prints `replay without the model: identical`: every key press was recorded as an ordinary command.

**Hearing.** Events other things emit (`{"emit": "door"}`, a coin picked up elsewhere) arrive in the next observation as `heard`: for each, the `sound`, the `offset` from the body to where it happened, the `distance`, `ticks_ago`, and `muffled` when the colliders in between stop most of the direct sound (the same ray tracing as `docs/AUDIO.md`, so the mind is deafened by exactly what would deafen a player). Sounds from the mind's own body are not listed, nothing beyond 40 m is heard, and the 5 newest distinct sounds are kept (`--hearing N`, 0 = deaf).

**Seeing a layout.** `--minimap 21` adds a text map to every observation (the `minimap` tool, also available to any client): a grid of characters centred on the body, `@` is you, other things are marked with the first letter of their name, `.` is empty, up is -z and right is +x. Language models read a small grid like this far more easily than a list of coordinates.

`crates/lunaris-mind/tests/orb_dash.rs` does the same inside a test with a small rule-based server, including a reply wrapped in a code fence with prose around it, and checks that the model collects all 12 orbs before the clock runs out and that the allow-list refuses anything else.

## Goals as data: `--success`

A text goal ("reach the Goal") is for the model; to *measure* whether it worked, give the run a condition in a file: one condition object or an array of them, in the same language as an `expect` step of a playtest (`docs/GAMES.md`):

```json
[ { "name": "Walker", "field": "Transform.pos[0]", "ge": 8.0 },
  { "name": "Walker", "field": "Transform.pos[0]", "le": 9.0 } ]
```

```
lunaris mind examples/arena.lunaris.json --body 1 --goal "reach the Goal" --scripted examples/mind_walk.scripted.json --turns 12 --success examples/arena.success.json
success: the goal was reached after 6 turns (tick 180)
```

The condition is checked before the first turn and after every turn; when all of it holds the run stops. If it never holds you get `success: NOT reached in N turns` with what the last look saw, and the **exit code is 1**, so a script can score a model: turns used, whether it got there, from the same deterministic world every time (and the run is still recorded and replays without the model). Targets include resources, entity fields, counts of entities and events, so "collect all 12 orbs" is `{"resource": "score", "ge": 12}`.

## Why it is built this way

- **The session is a recording.** The model is the only non-deterministic part. Its choices enter the world as commands, so `replay` reproduces the run exactly without calling the model again. That makes a run something to hash, diff and keep as a regression test.
- **Small models can use it.** The prompt is short, the format is one object, and errors say what to change.
- **No hidden powers.** Permissions are checked before the tool runs, in the loop, in plain code.

## What a small model needs (measured on Qwen3.5-4B through ForgeInfer, 2026-10-14)

A 4B model decides in 0.2-0.3 s when most of its prompt is cached and always returns a usable reply with `--json-schema`. Its weak point is spatial reasoning:

- **Relative beats absolute.** Every entity in `nearby` carries `offset`, the vector from your body to it, and the system prompt says how to turn it into a velocity. With only absolute positions the model walked off along the wrong axis for twelve turns; with offsets it walked straight to the goal.
- **Put the controls in the goal** for key-driven games: `--goal "collect the orbs. Controls: d moves +x, a moves -x, s moves +z, w moves -z"`. Without the mapping it presses one key forever.
- **Keep the prompt small.** The body's visual components are left out of what it sees, a successful write is echoed as `{"tool": "set", "ok": true}` only, empty lists are dropped and numbers are rounded; cut `--radius`/`--nearby-limit` for scenes with many entities (a 12-orb observation is about 900 tokens).
- The transcript (`--transcript`) keeps the server's token counts, cached tokens and timing per turn; `python -I tools/mind_report.py <transcript>` prints them next to what the body did.

## Verbs: `go_to` and `wait`

Small models steer badly and choose well. Allow the two verbs the mind loop runs itself and let the engine do the walking (`--tools go_to,wait,nearby`):

| Verb | What it does | Answer |
|---|---|---|
| `go_to {target}` | `target` is the name of something (`"Orb 7"`, case-insensitive; the nearest with that name), its uid, or `[x, y, z]`. The body gets a `NavAgent` if it has none, the target is set, and game time passes (in steps of 10 ticks, at most `go_to_ticks`, 900 by default) until it arrives, is blocked or the time is up. See `docs/NAVIGATION.md`. | `{"result": "arrived"}`, `{"result": "blocked", "reason": "no walkable route ..."}` or `{"result": "still walking", "distance_left": 4.2}` |
| `wait {ticks}` | lets 1 to 600 ticks pass | `{"waited": 30}` |

A turn that runs a verb advances the world by the verb's own time, not by `--think-ticks` on top. Measured with Qwen3.5-4B on `examples/orb_dash.lunaris.json` (`--tools go_to,wait,nearby --max-actions 1 --json-schema`): all 12 orbs in 12 decisions (score 12 at tick 1840 of a 45 s clock), 0.36-0.48 s per decision, 86% of prompt tokens served from the server's cache; the same model pressing keys with the `input` tool got 2 of 12 in 80 decisions. The session replays without the model like any other (the `set` that gave the target is a recorded command, the walking is the simulation).

## Memory across the cuts: `--memory-endpoint`

The history is cut now and then (that is what keeps a small model's window useful), and what was in it is gone. A **memory** remembers across the cuts: every turn is written down as a short note ("the thought, then what it did and how it ended"), each note is turned into a vector by an embedding model, and each turn the older notes most like the present situation (the latest thought and the nearest things) come back in the observation as `memories`:

```bash
lunaris mind examples/maze.lunaris.json --body 17 --goal "visit every flag" --tools go_to,wait,nearby --endpoint http://127.0.0.1:8080/v1 --model npc-mind --json-schema \
  --memory-endpoint http://127.0.0.1:8081/v1 --memory-model nomic-embed-text-v1.5 --memory-k 3
```

Any OpenAI-style `/v1/embeddings` server works (ForgeInfer serves `nomic-embed-text-v1.5` that way; the task prefixes `search_document: ` and `search_query: ` that this model family wants are added for you). `--memory-k` is how many notes come back (default 3); a note must be at least 0.4 similar (cosine). The last three turns are never recalled (they are in the conversation). If the embedding server fails, the memory switches itself off and the model is told once; the run goes on. In code: `Mind::memory = Some(Memory::new(Box::new(OpenAiEmbedder::new(url, model))))`, or your own `Embedder`.

Measured with Qwen3.5-4B (chat) and nomic-embed-text-v1.5 (embeddings), both served by ForgeInfer on one RTX 4060: fourteen `go_to` decisions in `examples/maze.lunaris.json` (every one arrived, through the maze's gaps) with a note embedded per turn (one POST of a few milliseconds each) and memories recalled from the fifth turn on, 0.3 s per decision with the prompt 86% cached. Memory pays off when the history is cut (small context window, long session); in a short run the notes repeat what is still in the conversation.
