# The editor

**Lunaris Editor is a Windows application**, like the editors of the other engines: a real window with a menu bar, a toolbar, a hierarchy, a scene view drawn by the engine's own GPU renderer, an inspector, a project browser and a console. It is not a web page.

```
lunaris-editor.exe                              (or double-click "Lunaris Editor.bat" in the project folder)
lunaris-editor.exe examples/echo_tour.lunaris.json
lunaris edit examples/echo_tour.lunaris.json    (the same, started from the command line)
```

Build it with `cargo build --release -p lunaris-studio` (the program is `target/release/lunaris-editor.exe`). It is made with egui (the window and the panels) over the same wgpu the engine draws with.

## The layout (Unity's)

| Panel | What it does |
|---|---|
| **Menu bar** | File (new, open, save, save as: the system's own file dialogs), Edit (undo, redo, duplicate, delete), Entity (create anything), View, Play, Help |
| **Toolbar** | The tools of the scene view: Hand (Q), Move (W), Rotate (E), Scale (R); in the middle Play, Pause, Step; Undo and Redo |
| **Hierarchy** (left) | The scene's name and every entity below it, children under the entity they are attached to. Click selects, double-click renames, drag one onto another to attach it (it keeps its place), right-click for duplicate, delete, detach, create. The search box filters |
| **Scene** / **Game** (middle) | Scene: a free camera (right button drag looks, right button + W A S D Q E flies, middle drag pans, wheel zooms, Alt + left drag orbits, F frames the selection) drawn by the GPU renderer; click selects (also cameras and lights, which have no picture); the handles of the tool drag the selection along an axis (Ctrl snaps; Shift scales all three axes). The toolbar above the view picks the render quality (Classic ... Ultra, or the CPU reference renderer), 2D mode, grid and gizmos. Game: what the scene's own camera sees, with input going to the game while playing |
| **Inspector** (right) | The selected entity's components, one bar each (click to fold, right-click to remove): every field as the right kind of widget (drag boxes, colour pickers, check boxes, drop-downs, asset pickers), rotation as angles. Add Component at the bottom. The fields come from the engine's `schema`, so a component defined by a game shows up with no editor code |
| **Project** (bottom) | The workspace as folders on the left and tiles on the right: double-click a scene to open it; right-click a model, sound or picture to use it |
| **Console** | Everything the editor asked the engine (every click is a tool call; hover for the arguments and the answer), errors in red, search, "Check scene" (the `check` tool), and a box where you can type any tool call |
| **Systems** | The systems that run every tick. A graph system is drawn as **nodes** (a red event node on top, white execution wires down through the statements, green, red and magenta data wires from the expressions into the pins that use them; wheel zooms, drag pans, Fit shows it all), next to its JSON; click a constant in the picture to change its value, edit the text, then **Apply** (`define_system`; the engine's error comes back with its path). Systems can be switched on and off with their check box |

**Play** saves the scene in a snapshot and runs the simulation at 60 ticks a second inside the editor, with sound and with the keyboard and mouse going to the game when the pointer is over the Game view. **Stop** (or Esc) puts the scene back exactly as it was (the state hash is the same). Edits made while playing are thrown away with the run.

## The UI tab (Unreal's UMG look)

The **UI** tab is a designer for the on-screen interface (`docs/UI.md`), in the manner of Unreal's widget designer. The left panel becomes the **Palette** (Button, Text Block, Progress Bar, Border) over the **Hierarchy** of the widgets (front to back, the eye hides one, double-click renames, right-click deletes), the right panel **Details**, and the middle the **Designer**: the screen as a frame (*Screen Size* picks 1920 x 1080, a phone in portrait, ...) with the widgets drawn as the game draws them.

* Drag a palette entry onto the screen to add a widget there (or click it to add one in the middle, or right click the screen). Click a widget to select it; drag it to move it; drag a white handle to resize it. Positions snap to an 8 pixel grid (`Alt` turns the snap off; `Snap` and `Grid` are toolbar buttons). Arrow keys nudge by one pixel (`Shift`: eight), `Delete`, `Ctrl+D`, `Ctrl+Z` / `Ctrl+Y` work as in the scene view; the wheel zooms, the right button drags the view; *Zoom to Fit* resets it.
* **Details** has the *Slot* (position, size and ZOrder in pixels of the chosen screen), *Appearance* (colour and opacity, background colour, fill percent for a bar, border), *Content* (the text with `{resource}` placeholders, its size and colour) and *Behavior* (visible, is a button).
* The widgets are the `Ui` entities of the scene: every change is a `set`, `spawn` or `despawn` tool call, so undo, recordings and an agent working in the same world all see it; one drag is one undo step. **Graph** (top right) switches to the Blueprint tab, where the systems that react to `Ui.clicked` are nodes.

## The Blueprint tab (Unreal's look)

The **Blueprint** tab edits a graph system (`docs/GRAPH.md`) as nodes, the way Unreal Engine's Blueprint editor does. The left panel becomes **My Blueprint** (the graph systems of the world, and its resources: click one to add a *Get Resource* node), the right panel **Details** (the properties and default values of the selected node, the system's own settings when nothing or the red event node is selected). In the graph:

* The red **event node** starts the system (*Every Tick* or *On Contact*). White **execution** wires run from node to node and say what happens after what; **Branch** has *True*, *False* and *Completed* (what comes after both).
* **Data pins** are round: green numbers, red true/false, pink text. A wire carries a value from the output of one node into an input of another. An input with no wire holds a value of its own: click the box next to its name and type it (a check box for true/false).
* Properties such as the field of *Set Field* are rows at the foot of the node; click one to type. *Spawn Entity*, *Add Component* and the maths nodes with any number of inputs have **+ Add pin**; double-click the name of a spawn pin to rename it (`Transform.pos`).

| | |
|---|---|
| Right button drag, wheel | Pan, zoom around the pointer |
| Right click | The **All Actions** menu with a search box (`Emit`, `Add`, `Branch`...); on a node, a pin or a comment, the menu of that. Dropping a wire on an empty place opens it with only what fits the pin. |
| Left drag | A node (header or body) moves, an empty place draws a selection box, from a pin pulls a wire (onto another pin to connect; an input that already has a wire gives it up and you can move it); `Alt` + click on a pin breaks its wires |
| `Delete`, `Ctrl+D`, `Ctrl+C` / `Ctrl+V`, `Ctrl+A` | Delete, duplicate, copy and paste, select all; `Ctrl+Z` / `Ctrl+Y` undo and redo (of the graph) |
| `C`, `F` | A comment box around the selection (drag its title to move it with what it holds, the corner to resize it, double-click the title to rename it); frame the selection |
| `Ctrl+S`, **Compile** | Checks the graph (a missing field name, a circle of wires: shown on the node, with a list in *Compiler Results*) and sends it to the world with `define_system`. What the *world* refuses (an unknown component) comes back with its place in the graph when it can be found. |
| **Arrange**, **Frame All**, **Edit as Script** | Lay all nodes out again, show all of them, open the same system as text in the Script tab |

Nodes that are not wired to the event do nothing and are dimmed (*not connected*). Where nodes are, and the comment boxes, are kept in `blueprints/<system>.bp.json` of the project; the file is used only while it says the same thing as the system in the world. Anything the nodes have no words for stays as a **Raw** node holding its JSON, so no system is ever too odd to open. The nodes, the JSON and the script (`docs/SCRIPT.md`) are three views of one system, and each can open any system of any scene (`cargo test -p lunaris-studio blueprint` runs every example scene through the nodes and back).

## The Script tab (Visual Studio Code's look)

The **Script** tab next to Scene and Game is a code editor for **Lunaris Script** (`docs/SCRIPT.md`), drawn the way Visual Studio Code draws: an activity bar, an Explorer with the scripts of the project (`scripts/*.lun`) and the graph systems of the scene, tabs for the open files, breadcrumbs, line numbers, indent guides, a minimap, squiggles under mistakes, a Problems and an Output panel, a status bar. The colours are not imitated: the tab reads the theme files VS Code itself reads (`colors` and `tokenColors`, with `include` chains). **Dark 2026**, **Dark Modern**, **Dark+** and **Visual Studio Dark** are built in (`crates/lunaris-studio/assets/vscode`, MIT licence), and `Ctrl+Shift+P`, *Preferences: Color Theme* lists every dark theme of an installed VS Code and of its extensions, previews each as you move through the list, and remembers the choice in `.lunaris/editor.json`.

| | |
|---|---|
| `Ctrl+S` (or the triangle) | Compiles, defines the systems with `define_system` and saves the file. A mistake of the text is a red squiggle with a line and column; one the world finds (an unknown field) is shown at the same place. A system that is no longer in the script is removed from the world. |
| `Ctrl+Shift+P`, `F1` | The command palette: apply, new script, find, go to line, format document, toggle minimap and Problems, colour theme |
| `Ctrl+P` | Quick open: a script file or a system of the scene |
| `Ctrl+F`, `Ctrl+G`, `Ctrl+W`, `Ctrl+B`, `Ctrl+Shift+M` | Find (with match case), go to line (`:12` in the palette too), close the tab, side bar, Problems |
| Typing | Completion for keywords, functions, components, the fields of a component after its dot, local variables, `$resources`; `Ctrl+Space` asks for it. Brackets and quotes close and are stepped over, Enter keeps the indentation and opens a block, Tab and Shift+Tab indent, `Ctrl+/` comments lines, hovering a squiggle shows its message |

The Systems panel has **Edit as script** for the system it shows. The text, the node view and the JSON are three views of one system (`docs/SCRIPT.md`).

## Building a game

**File > Build Windows Player...** asks for a name and a folder and makes the game as a program there: `<Name>.exe` (a copy of the Lunaris player, which has no console window), `game.json` (which scene to open, the quality, the window size), the scene as it is in the editor right now, and every file the scene mentions (pictures, models, sounds, WebAssembly modules) at the same relative paths. The folder runs on a machine that has nothing else of Lunaris on it: double-click `<Name>.exe`. **Build and Run...** starts it at once. The player is `lunaris-player.exe` (next to the editor; `lunaris-player scene.json` plays a scene from the command line).

## Everything is a tool call

The editor never touches the world by itself: it calls `spawn`, `set`, `despawn`, `snapshot`, `restore`, `undo` ... exactly as an AI agent or `lunaris call` does. So undo and redo cover whatever you or an agent did, a recording made with `record_start` contains the editor's edits, and an agent connected to the same project through MCP sees the same engine. A drag of a handle is many `set` calls with one `undo_group`, so it is one undo step.

## Keyboard

Q W E R tools - F frame the selection - Ctrl+Z / Ctrl+Y undo, redo - Ctrl+D duplicate - Delete - Ctrl+S save - Ctrl+O open - Ctrl+N new - Ctrl+P play and stop - Esc stop.

## Command line

`lunaris-editor [scene] [--root DIR] [--seed N] [--size WxH] [--quality classic|low|medium|high|ultra|reference] [--select NAME] [--play] [--mute] [--tab project|console|systems] [--view scene|2d|game] [--shot FILE [--shot-delay SECONDS]]`. `--shot` saves a picture of the window and quits (the documentation and the tests use it). Windows only for now; the engine, the tools and the browser editor below run elsewhere.

---

# The browser editor (`lunaris edit --web`)

The older editor is a web page served by `lunaris edit --web`; it stays for what a window cannot do: another machine (a server, Linux), or a person and an AI agent sharing one live world (`--session-file`). It does the same things through the same tool calls.


```bash
lunaris edit --web examples/bouncing.lunaris.json      # then open http://127.0.0.1:7777/
lunaris edit --web --port 0                            # any free port; the address is printed
```

The editor is a web page, so it needs no graphics libraries, runs on any machine with a browser, and can be driven by a person or by a test script. `lunaris edit` starts a tiny server (standard library only) that serves the page and forwards its requests to the engine's tool API. **Nothing the page does is special: every click becomes the same tool call an AI agent or the CLI would make.** The "Tool calls" tab lists them as they happen, and you can type any tool call yourself in the box below the list.

## What is in it

| Area | What it does | Tools it uses |
|---|---|---|
| Entity list (left) | All entities by name, filter box. Attached entities are listed under their parent, indented. `+` opens a menu of kinds (cube, sphere, cylinder, floor, point light, sun, camera, sprite, text, empty) and creates the one you pick at the view centre | `query`, `spawn` |
| Viewport (centre) | The engine's own renderer (the GPU renderer in high quality when the machine has a GPU, so it shows the sky, shadows and materials; `lunaris edit --no-gpu` or `--quality classic` change that). 3D orbit, 2D, or the scene's camera. Drag the **selected** entity to move it (across the floor in 3D, in the picture plane in 2D; the whole drag is one undo step); drag empty space to orbit (2D: pan), right-drag or Shift-drag to pan, wheel to zoom, click to select | `render`, `pick` |
| Inspector (right) | Every component of the selection as editable fields, add and remove components, duplicate, delete, **Attach…** (ask for the parent's uid; the entity gets `Parent` and a `LocalTransform` worked out so it does not move; 0 detaches; one undo step) | `get`, `set`, `add_component`, `remove_component`, `spawn`, `despawn` |
| Toolbar | Play and pause (fixed 60 Hz steps), single step, undo and redo, snapshot and restore, load and save scene | `step`, `undo`, `redo`, `snapshot`, `restore`, `load_scene`, `save_scene` |
| Systems (bottom) | Lists native and graph systems, switch graph systems on and off, and shows the selected graph system as **nodes** | `systems`, `get_system`, `define_system`, `enable_system`, `remove_system` |
| Assets (bottom) | The workspace's files by kind (`files`): click a texture to put it on the selected sprite or tile map (or make a sprite), a model to put it on the selected mesh (or make a new entity), a scene to load it | `files`, `set`, `spawn`, `load_scene` |
| Tool calls (bottom) | A live log of every call the page made, with arguments, results and timing, plus a box to run any tool | any |

Sound: events emitted by graph systems (`{"emit": "coin"}`) arrive with each `step` and play as small synthesised effects (the **Sound** button mutes). A `Sound` entity defines how a name sounds; common names have built-in voices.

Keyboard: Space plays and pauses, Delete removes the selection, F centres the view on it, Ctrl+Z / Ctrl+Y undo and redo.

## Node graphs

A graph system (`docs/GRAPH.md`) is JSON, and the editor draws it Unreal-style:

- a red **event** node at the top says which entities the system visits every tick;
- white **execution** wires run down through the statements (`Set`, `Let`, `If`, `Spawn`, ...);
- coloured **data** wires (green numbers, red booleans, magenta strings) run from expression nodes (`Get Transform.pos[1]`, `mul`, `lt`, constants) into the pins of the statement that uses them;
- an `If` node has `then` and `else` pins that lead to its nested blocks.

The JSON beside the graph and the picture are the same document: the text is the source of truth, and the graph is redrawn from it after every change. **Apply** sends it to `define_system`; if the engine refuses, you get its error with the exact `path` into the JSON and a hint, the same message an AI agent would get. Applied changes are undoable like any edit. Wheel zooms the graph, drag pans it, **Fit** shows all of it, **JSON** hides or shows the text.

Editing the graph (every edit rewrites the text; **Undo edit** steps back):

| Do this | To get this |
|---|---|
| Click a constant, a `Get`/`Var` name, or the name on a `Set`/`Let`/`Add`/`Remove` node | edit that value or name |
| Right-click a value node | wrap it in any operation (`add`, `mul`, `clamp`, `lt`, `select`, ...; the old value becomes the first input), replace it with a constant, field, variable, `dt`, `tick`, `uid` or `rand`, delete it, add or remove inputs of `add`/`mul`/`min`/`max`/`and`/`or` |
| Right-click a statement | add a statement before or after it, move it up or down, delete it; on an `If`, add statements to `then` or `else` or remove the `else` block |
| Right-click the red event node | rename the system, change the components it visits or skips, change the run order, add a statement at the start or end |
| Right-click empty space | add a statement at the end |

Because the language is a tree (statements in lists, expressions as trees), this covers everything a free-form wiring tool would: a value feeding a pin *is* the expression sitting in that slot. Free dragging of wires between arbitrary pins is not offered because the language has no such thing as a shared sub-expression.

## One world for a person and an AI agent

`lunaris edit scene.json --session-file session.json` starts the editor and writes the connection details (`rpc` URL and token) to `session.json`. An agent or script joins the same live world:

```python
from lunaris import connect          # clients/python/lunaris.py (standard library only)
w = connect("session.json")          # the same methods as Engine: w.spawn(...), w.query(...), w.render(...)
w.spawn(components={"Name": {"value": "Visitor"}, "Transform": {}})
```

```js
import { Engine } from './clients/js/lunaris.mjs';
const w = await Engine.connect({ session: 'session.json' });   // Node 18+
```

or `POST <rpc>` with the header `X-Lunaris-Token` from the file, from any language. What the agent does shows up on the person's page by itself (the page polls the cheap read-only `revision` tool about twice a second and refreshes its entity list, systems, inspector and picture when the number moved because of somebody else); what the person does is there in the agent's next `query`. The agent can ask the same question with `revision`: a counter that goes up for every tool call that is not read-only, from any client. Calls are served one at a time, so nothing is lost or interleaved halfway; `undo` is shared too (it undoes the last edit, whoever made it). The session file holds the secret token: keep it in your own folder, it is only valid while that editor process runs.

## Working on the page

The page is one file, `crates/lunaris-editor/src/index.html`, built into the binary. Set `LUNARIS_EDITOR_PAGE=crates/lunaris-editor/src/index.html` when starting `lunaris edit` to serve it from disk instead, then reload the browser after each edit.

## Safety

The server is a door into the engine, so it is locked:

- it listens on `127.0.0.1` only;
- requests must carry the right `Host` header (stops DNS rebinding) and, if they carry an `Origin`, it must be the editor's own;
- `/rpc` needs a random per-run token that is built into the page (another site cannot read the page, because no CORS headers are ever sent) and a JSON content type, which makes browsers send a preflight request that is never answered;
- file tools only touch paths inside the workspace folder (`--root`, default the current folder);
- request headers and bodies are size limited and connections time out.

This is a development tool for your own machine. Do not put it behind a public proxy.

## Testing it without a person

`node tools/editor_smoke.mjs` starts the server, opens the page in headless Chrome (or Edge) over the DevTools protocol (Node 22+, no packages) and checks what a person would do: select by clicking the view, edit a field, play, a bad graph edit (error with path and hint), a constant edited in the graph and applied then undone, a system built entirely from the menus, an edit made by another client appearing on the page by itself, the `+` menu, attaching and detaching (no jump, the parent drags the child, the list nests it), and the tool-call log. The server's own tests (`cargo test -p lunaris-editor`) cover the token, host and origin rules and the size limits.

The page talks to `POST /rpc` with JSON-RPC 2.0 (`{"method": "lunaris.<tool>", "params": {...}}`) and the token from the page, so a script can do anything the page can.
