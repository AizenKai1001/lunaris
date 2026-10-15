# Lunaris Script

A small text language for gameplay. A script is a *view* of a graph system (`docs/GRAPH.md`): it compiles to exactly the JSON that
`define_system` takes, and a system's JSON turns back into script text. The text, the node graph and the JSON are three ways of looking at one
thing, so an AI agent, a person at the keyboard and a person with the node view all edit the same system.

```text
/// Fall, and bounce with some energy loss.
system gravity each Transform, Velocity without Frozen order 100 {
    Velocity.lin[1] -= 9.8 * dt;
    if Transform.pos[1] < 0 {
        Transform.pos[1] = 0;
        Velocity.lin[1] = -Velocity.lin[1] * 0.8;
    }
}
```

That is the `gravity` example of `docs/GRAPH.md`, and it compiles to that JSON, key for key.

## Where it is used

| | |
|---|---|
| Editor | The **Script** tab (`docs/EDITOR.md`): a code editor in the look of Visual Studio Code. `Ctrl+S` applies the script to the world and saves it as `scripts/<name>.lun` in the project. |
| Tools | `compile_script {source}` checks a script and returns the systems as JSON, or `{ok: false, errors: [{line, col, message, hint}]}`. `define_script {source}` defines every system in it (all or nothing, one undo step); a mistake, also one the *world* finds (an unknown component or field), comes back with its line and column. `get_script {name}` is a defined system as script text. |
| Library | `crates/lunaris-script`: `compile`, `decompile`, `highlight`, and `Compiled::locate` (a JSON path such as `do[2].to.add[1]` to the place in the text). It depends only on `lunaris-json`. |

## Systems

```text
/// optional doc comment lines (they become `doc`)
system NAME [each A, B] [without C] [order N] [disabled] { statements }
system NAME on_contact a(A, B) b(C) [triggers all|only|solid] [order N] { statements }
```

A file can hold several systems. `each` and `on_contact` are the two kinds of system of `docs/GRAPH.md`; `disabled` is `"enabled": false`.
Comments are `// ...` and `/* ... */`; `///` lines in front of `system` are its documentation.

## Statements

| Script | JSON statement |
|---|---|
| `Transform.pos[1] = expr;` | `{"set": "Transform.pos[1]", "to": expr}` |
| `Transform.pos[1] += expr;` (also `-=` `*=` `/=`) | `set` of the field and the operation: `{"add": [{"get": ...}, expr]}` |
| `let speed = expr;` | `{"let": "speed", "be": expr}` |
| `if cond { ... } else if cond { ... } else { ... }` | `{"if": cond, "then": [...], "else": [...]}` (`else if` is an `if` inside `else`) |
| `despawn;` `despawn a;` | `{"despawn": true}` / `{"despawn": "a"}` |
| `emit "thud";` | `{"emit": "thud"}` |
| `$score = expr;` `$score += expr;` `$score -= expr;` | `res_set`, `res_add` (and `res_add` of `neg`) |
| `spawn { Transform { pos: 0 }, Velocity {} };` | `{"spawn": {"Transform": {"pos": 0}, "Velocity": {}}}` |
| `spawn_prefab "Bullet" at (x, y, z) velocity (x, y, z) with { Tag { n: 4 } };` | `{"spawn_prefab": "Bullet", "at": [...], "velocity": [...], "with": {...}}` |
| `add Frozen;` `add Health { hp: 10 } on a;` | `{"add": "Health", "values": {"hp": 10}, "on": "a"}` |
| `remove Frozen;` `remove Frozen on b;` | `{"remove": "Frozen", "on": "b"}` |
| `json({"future": ...});` | the JSON as it is: an escape for what the language has no words for yet |

## Expressions

Numbers (`1`, `0.5`, `1e-3`; a number with a point or an exponent is a float), `true`, `false`, `"text"`.

| Script | JSON |
|---|---|
| `Transform.pos[1]`, `a.Orb.value` | `{"get": "Transform.pos[1]"}`, `{"get": "a.Orb.value"}` |
| `speed` (after `let speed`) | `{"var": "speed"}` |
| `$score`, `$input.space`, `$"odd name"` | `{"res": "score"}`, ... |
| `dt`, `tick`, `uid`, `uid(a)`, `contact.depth` | `{"dt": true}`, `{"tick": true}`, `{"uid": true}`, `{"uid": "a"}`, `{"contact": "depth"}` |
| `a + b`, `a - b`, `a * b`, `a / b`, `a % b` | `add`, `sub`, `mul`, `div`, `mod`; a chain `a + b + c` is one `{"add": [a, b, c]}` |
| `a < b`, `<=`, `>`, `>=`, `==`, `!=` | `lt le gt ge eq ne` |
| `a && b`, `a \|\| b`, `!a`, `-a` | `and`, `or`, `not`, `neg` |
| `sin(x)`, `clamp(x, 0, 1)`, `hypot(x, y)`, `rand(7)`, `select(c, a, b)` | every operation of `docs/GRAPH.md` by its name |

Precedence from the loosest: `||`, `&&`, `== !=`, `< <= > >=`, `+ -`, `* / %`, unary `- !`. Parentheses group; `(a + b) + c` is a different
JSON tree than `a + b + c` (the first has an `add` inside an `add`), and the text keeps that difference when it goes back and forth.

## What is checked where

* The **reader** (`compile`) finds mistakes of the text: a missing `;`, an unknown function, wrong number of arguments, an unclosed block. It
  reports every one it can find, each with a line and column, and it never stops at the first.
* The **world** (`define_system`) finds mistakes of meaning: a component that is not in `each`, a field that does not exist, a variable used
  before `let`. Its error has a JSON path; the editor and `define_script` turn that path back into a line and column.
* Native and WebAssembly systems (`plugin`, `wasm`) are compiled code and have no script form.

## Round trip

`decompile(compile(text))` is the same system again, and `compile(decompile(json))` is the same JSON (up to the order of keys and the
`"enabled": true` the engine adds). Every system of every example scene is tested that way (`cargo test -p lunaris-script`), and the
editor's Script tab is, because of it, always able to open any graph system of a scene as text.
