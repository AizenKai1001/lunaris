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

## The Canvas tab

A board for plans and maps, in the editor: notes, groups and arrows on an endless surface. The files are `*.canvas` in the JSON Canvas format (the one Obsidian reads and writes: `nodes` of type `text`, `file`, `link` and `group`, and `edges` with a label and a colour), so a canvas made there opens here and the other way round; fields the editor does not know are kept when it saves.

* **Open** one from the list in the tab's toolbar (every `.canvas` file of the workspace), or start the editor with `--canvas FILE` (or `--tab canvas`). The game plan of `examples/wyrdwood/` is one: `lunaris-editor --canvas examples/wyrdwood/wyrdwood.canvas`.
* **Move around**: drag the background (or the middle button) to pan, scroll to zoom around the pointer, `F` or the **Fit** button shows the whole board. Far away a note shows only its first line, so the shape of the board still says what is where.
* **Edit**: click a note, then change its text (a little Markdown: `#` headings, `-` bullets, `**bold**`, `` `code` ``, `[[links]]` shown as their text), colour (Obsidian's six presets), position and size in the **Details** panel; drag a note to move it (dragging a group takes what is inside it); **+ Note** and **+ Group** add one in the middle of the view; right-click the board for a note where the pointer is; `Delete` removes the selected note and its arrows. `Ctrl+S` or **Save** writes the file.
* **Notes** (the left panel) lists the notes in order with a search box: click one to select it and bring it to the middle.
* **It follows the file**: when another program saves the open canvas (an agent keeping a plan up to date) and the board has no unsaved changes, the editor reads it again and keeps the view and the selection. With unsaved changes of its own it never throws them away.

Arrows are drawn and shown in the Details of the notes they join; they are not edited in the editor yet (edit them in the file or in Obsidian). Code: `crates/lunaris-studio/src/canvas_ui.rs`.
