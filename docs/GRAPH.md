# Graph systems

Gameplay logic as data. A graph system is a JSON document that runs once per tick for every entity that has certain
components. It needs no Rust, no compiler and no restart: an AI agent (or a node editor) defines it with the
`define_system` tool, the engine checks it, and it takes part in the deterministic simulation like any native system. It is
saved with the scene, recorded and replayed, snapshotted, and every change is undoable.

```json
{
  "name": "gravity",
  "doc": "Fall, and bounce with some energy loss.",
  "each": ["Transform", "Velocity"],
  "without": ["Frozen"],
  "order": 100,
  "do": [
    { "set": "Velocity.lin[1]", "to": { "sub": [ { "get": "Velocity.lin[1]" }, { "mul": [9.8, { "dt": true }] } ] } },
    { "if": { "lt": [ { "get": "Transform.pos[1]" }, 0 ] },
      "then": [ { "set": "Transform.pos[1]", "to": 0 },
                { "set": "Velocity.lin[1]", "to": { "mul": [ { "neg": { "get": "Velocity.lin[1]" } }, 0.8 ] } } ] }
  ]
}
```

## Anatomy

| Key | Meaning |
|---|---|
| `name` | Letters, digits, underscore. Defining a system with an existing name replaces it. |
| `each` | Components an entity must have. Their fields can be read and written as `Component.field` or `Component.field[index]`. |
| `without` | Components an entity must not have. |
| `order` | Integer, default 100. Graph systems run after the native systems, lowest `order` first, then in definition order. |
| `do` | List of statements run for each matching entity. |
| `doc`, `enabled` | Optional. |

## Statements

| Statement | Effect |
|---|---|
| `{"set": "Comp.field[i]", "to": expr}` | Write a field (converted to the field's type; non-finite numbers are refused). |
| `{"let": "name", "be": expr}` | A local variable, visible to the statements after it (and inside nested blocks). |
| `{"if": expr, "then": [...], "else": [...]}` | Branch. `else` is optional. |
| `{"despawn": true}` | Delete this entity. |
| `{"emit": "name"}` | Announce an event (a sound, a message for the outside). Collected in order (tick, then uid), returned by `step` and the `events` tool (with the emitter's `pos`), played by the editor and the native player (`docs/AUDIO.md`); events are output, not state: not in the state hash, and a replay makes them again. |
| `{"res_set": "name", "to": expr}` | Set a world resource (a named global number: score, timer, a key's state). Queued like the structural statements, applied in uid order. |
| `{"res_add": "name", "by": expr}` | Add to a world resource. Many entities can add to the same one: the sum is the same for any thread count or storage order. |
| `{"spawn": {"Comp": {"field": expr, ...}, ...}}` | Create an entity with these components (scalar fields; others take defaults). |
| `{"spawn_prefab": "Name", "at": [x, y, z], "velocity": [x, y, z], "with": {"Comp": {"field": expr}}}` | Spawn a prefab (an entity template, `docs/PREFABS.md`): the root and all its children, with consecutive uids. `at` is the root's `Transform.pos`, `velocity` its `Velocity.lin` (each three expressions), `with` replaces scalar fields of the root's components. Bullets, pickups, enemies, anything that appears over and over. A system with this statement runs on the interpreter, not the batch vm. |
| `{"add": "Comp", "values": {"field": expr}}` | Add a component to this entity. |
| `{"remove": "Comp"}` | Remove a component from this entity. |

`spawn`, `spawn_prefab`, `despawn`, `add` and `remove` are queued and applied after the system has visited every entity, **ordered by the uid
of the entity that issued them**. That keeps results independent of how entities happen to be stored.

## Expressions

A number, `true`/`false` or a string is a constant. Everything else is an object with one key:

| Group | Operations |
|---|---|
| Values | `{"get": "Comp.field[i]"}`, `{"var": "name"}`, `{"res": "name"}` (a world resource, 0 when never set), `{"dt": true}` (the fixed time step), `{"tick": true}`, `{"uid": true}`, `{"rand": salt}` (a number in [0,1) that depends only on the seed, tick, this entity and `salt`) |
| Arithmetic | `add` `mul` `min` `max` (one or more arguments), `sub` `div` `mod` (two), `neg` `abs` `sqrt` `floor` `ceil` `round` `sign` `trunc` `fract` (one), `clamp` `[x, lo, hi]`, `lerp` `[a, b, t]`, `smoothstep` `[lo, hi, x]` (0 below `lo`, 1 above `hi`, an S-curve between), `noise` `[x, y, seed]` (smooth value noise in [-1, 1] that depends only on the point and the whole-number seed: terrain, clouds, wobble, scattering; sum several with doubled `x`/`y` for rougher ground; the same on every machine) |
| Trigonometry, powers | `sin` `cos` `tan` `atan` (one, radians), `atan2` `[y, x]` (the angle of a direction, in (-pi, pi]; `atan2 [0, 0]` is 0), `exp` `ln` (one), `pow` `[base, exponent]`, `hypot` `[x, y]` (the length of a vector) |
| Comparison | `lt` `le` `gt` `ge` `eq` `ne` (two arguments; `eq`/`ne` also work on booleans and strings) |
| Logic | `and` `or` (short-circuiting), `not`, `select` `[condition, a, b]` |

Numbers are f64 inside the interpreter and are converted when written (f32 fields round to f32). The transcendental
functions (`sin` `cos` `tan` `atan` `atan2` `exp` `ln` `pow`) are the deterministic implementations from `detmath`, built from
`+ - * /` and `sqrt` only, so they give the same bits on every machine (accurate to about 1e-13; the standard library's would
differ between platforms and break replays). Runtime errors, with the same words in the interpreter and the batch vm: division
by zero, `sqrt` of a negative number, `ln` of a number that is not positive, `pow` of zero to a negative power or of a negative
number to a fractional one, results too large to be a number (`exp` above about 709, `pow`, `hypot`), `tan` at an odd multiple
of pi/2, `smoothstep` with `lo` equal to `hi`.

## Contact systems: when two things touch

A system can react to touching pairs instead of visiting single entities. It needs `Collider` components (and a body that moves, usually a
`Velocity` and `RigidBody`); the physics system finds the contacts each tick.

```json
{"name": "collect", "on_contact": {"a": ["Player"], "b": ["Orb"]}, "do": [
  {"res_add": "score", "by": {"get": "b.Orb.value"}},
  {"despawn": "b"}]}
```

- `on_contact` replaces `each`. `a` and `b` list the components each entity of the pair must have; the system runs once for every ordered
  pair that matches, in uid order. If both roles match both entities (two `Ball`s), it runs once for each orientation.
- Fields are read and written with the role in front: `a.Transform.pos[0]`, `b.Orb.value`.
- `{"despawn": "a"|"b"}`, `{"add": "Comp", "on": "a"|"b", "values": {...}}` and `{"remove": "Comp", "on": "a"|"b"}` choose which entity; changes are queued like any
  other structural change, and a change whose target is already gone (two players taking the same orb) is skipped quietly.
- `{"uid": "a"|"b"}` is the uid of either entity. `{"contact": "depth" | "nx" | "ny" | "nz" | "trigger"}` describes the contact; the normal points from `a` to `b`.
- `"triggers"` inside `on_contact` is `"all"` (default), `"only"` (react to trigger overlaps) or `"solid"` (real collisions).
- Pairs are handled one after another, so writes to either entity are safe and the result never depends on threads or storage order.
  Contact systems run on the interpreter (not the batch vm); there are few contacts compared with entities.
- It fires every tick the pair overlaps, so something that should happen once (a pickup) should despawn, or flip a flag, in the same system.

## Resources: globals and input

A *resource* is a named global number (`score`, `time_left`, `input.space`). They are part of the simulation state: hashed, snapshotted, saved in scenes (except the live `input.*` ones), undoable, recorded.

- Read one anywhere with `{"res": "score"}`. A system reads each resource once, before it visits any entity, so every entity sees the same value whatever the thread count; a resource that was never set reads as 0.
- Change them with `res_set` / `res_add`. The changes are applied after the system has run, ordered by the uid of the entity that issued them, so the result never depends on the order entities are stored in or on the number of threads. A later system (higher `order`) sees them in the same tick.
- The native window (`lunaris play`) also sets `input.mouse_x` and `input.mouse_y` (0 to 1 across the picture, y down) and `input.mouse_left` / `input.mouse_right`.
- Humans and agents set them with the `set_resource` tool, and press keys with the `input` tool, which sets `input.<name>` (`true` is 1, `false` is 0). The editor's Play mode forwards the keyboard to `input`; a bot calls it from code. Both end up in the recording, so a replay needs nobody at the keyboard.
- The idiom for "one entity tells the others": the player system does `res_set player.x`, an orb system does `{"res": "player.x"}`. See `docs/GAMES.md`.

## Native plugins

A system can name a compiled library instead of `do`: `{"name": "seek", "plugin": "plugins/swarm.dll", "each": [...], "fields": [...], "writes": [...], "res": [...]}`. The engine hands the library batches of the listed fields as arrays of doubles and writes the `writes` back. It is host opt-in (`--allow-plugins`) and has its own document: `docs/PLUGINS.md`.

## Errors

- **Definition errors** are returned immediately with a `path` into the document and a `hint`, and nothing is recorded:
  `{"code":"bad_graph","message":"unknown operation `subb`","path":"do[0].to.subb","hint":"expression operations: get var dt ..."}`.
- **Runtime errors** (division by zero, writing the wrong type, ...) skip the rest of that entity's statements for that tick,
  are stored as the system's `last_error` (see the `systems` tool) and never stop the simulation.

## Performance

Graph systems run on a **batch vm** (`crates/lunaris-sim/src/vm.rs`). At definition time the expression tree is type-checked and lowered
to a flat register program; each tick it runs over 128 entities at a time, one tight loop per operation, so the compiler can
vectorise it. `if` runs each branch only for the entities that take it and skips a branch nobody takes; an error (division by
zero, a value that does not fit a field) is raised only for the entities that actually ran the failing step.

| 1M entities, a two-statement system with a branch | Time per tick |
|---|---|
| reference interpreter, one thread | 36 ms (36 ns per entity) |
| batch vm, one thread | 10.9 ms (**3.4x faster**) |
| batch vm, 8 threads | 3.9 ms (faster than the native movement system on one thread) |

The interpreter is kept as the definition of what a graph means. The vm must match it exactly: the same bits in every field, the
same structural changes in the same order, the same error text for the same entity. `cargo test -p lunaris-sim vm_tests`
generates random programs and worlds and compares both after every tick (10,000 programs and 100,000 state-hash comparisons run
clean; deliberately breaking the vm makes the test fail within a few programs). A program that mixes types in a way only the
interpreter would reject at run time stays on the interpreter, and `systems` reports `"batch": true` or `false` for each system.
`LUNARIS_VM_SEED` and `LUNARIS_VM_PROGRAMS` run a longer campaign. Heavy per-entity math still belongs in a native system.

## Tools

`define_system`, `remove_system`, `enable_system`, `systems`, `get_system`; and for writing them as text `compile_script`, `define_script`, `get_script` (`docs/SCRIPT.md`). A node editor will be a *view* of this same text:
statements and operations are nodes, `get`/`set` paths are pins, and saving the graph writes exactly this JSON.
