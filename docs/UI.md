# On-screen interface: the `Ui` component

Panels, bars, buttons and menus are entities with a `Ui` component. They are drawn over the picture (after the 3D scene, under the `Text` HUD) by both renderers, at the picture's own resolution, and buttons react to the mouse. No separate UI language: it is components, graph systems and the same tool calls as everything else, so an AI agent can build a menu, test it by sending mouse input, and change it while the game runs.

```json
{ "Name": {"value": "Health bar"},
  "Ui": { "pos": [0.1, 0.3], "size": [0.8, 0.09], "color": [0.2, 0.8, 0.3, 1], "back_color": [0.05, 0.05, 0.08, 0.8],
          "fill": 0.5, "border": 0.004, "border_color": [1, 1, 1, 0.9], "text": "{hp} / 100", "text_size": 0.045 } }
```

| Field | Meaning |
|---|---|
| `pos`, `size` | the rectangle: top-left corner, width and height, as **fractions of the picture** (the same convention as `Text`: `pos` [0.4, 0.45] with `size` [0.2, 0.1] is a box in the middle, whatever the window's size). |
| `color` | the fill colour (display colour, RGBA; alpha blends over what is behind). |
| `fill`, `back_color` | `fill` below 1 draws only the left part of the rectangle in `color` and the rest in `back_color`: a bar. Set `fill` from a graph system (`{"set": "Ui.fill", "to": {"div": [{"res": "hp"}, 100]}}`). |
| `border`, `border_color` | an edge, `border` as a fraction of the picture's height (at least one pixel when above 0). |
| `text`, `text_size`, `text_color` | a label centred in the rectangle, with `{resource}` placeholders like `Text` (and `{key:ACTION}`, the key the `InputMap` has for an action now, `docs/KEYBINDS.md`). |
| `layer` | drawing order: higher on top, equal layers by uid. |
| `visible` | false hides it (and a hidden button does not react). |
| `button`, `hover`, `pressed`, `clicked` | a button reacts to `input.mouse_x`, `input.mouse_y` (0..1 across the picture, y down) and `input.mouse_left`. The engine sets `hover` while the pointer is over it, `pressed` while the left button is down over it, and `clicked` for exactly **one tick** when the button is released over it after being pressed over it (press, drag away and release does nothing). It is drawn lighter under the pointer and darker while pressed. |

A click becomes an effect in a graph system, one per button:

```json
{ "name": "heal", "each": ["HealButton", "Ui"],
  "do": [ { "if": { "get": "Ui.clicked" }, "then": [ { "res_set": "hp", "to": { "min": [100, { "add": [{ "res": "hp" }, 10] }] } } ] } ] }
```

(`HealButton` is a tag component that says which button this is; `emit` an event instead to play a sound or tell the outside.)

The editor has a designer for all of this (`docs/EDITOR.md`, the UI tab): drag widgets from a palette onto the screen, move and resize them, edit colours and text in a Details panel.

## Layout, sliders, switches, text fields and menus with a keyboard or gamepad

These are components on top of `Ui` rectangles (`crates/lunaris-sim/src/ui_kit.rs`), run before the graph systems, so a graph system reacts in the tick it happens.

| Component | Beside a `Ui` | What it does |
|---|---|---|
| `UiLayout` | a container | `kind` 0 column, 1 row, 2 grid (`columns`); puts the `Ui` entities whose `Parent` is the container in uid order inside its rectangle, `padding` and `spacing` as fractions of the picture's height (set the resource `screen.aspect` = width / height so they are equal in pixels). Containers nest. A hidden child takes no room. |
| `UiSlider` | a `button` | `value` between `min` and `max` (`step`) follows the pointer while the button is held, even outside the rectangle; the arrow actions change it while focused; `Ui.fill` shows it; `changed` is true on a tick it moved. |
| `UiToggle` | a `button` | each click flips `on` (`changed` for a tick) and switches the colour between `on_color` and `off_color`. |
| `UiInput` | a `button` | a click focuses it; characters typed (the resource `input.char` holds the code point for one tick, plus `input.backspace`, `input.enter`, `input.escape`; the `type_text` tool does it for an agent) go into `text`; `submitted` is true the tick after Enter; `placeholder` shows while empty; `max_len` characters; `number` is the text as a number when it is one to nine digits and -1 when it is not (a graph system cannot read text: a code typed into a keypad is sent as `number`, `docs/RAIDING.md`); `show_last` above 0 shows only the last that many characters after `...`; `remember` NAME keeps the text between runs (below, "Settings are kept between runs"). In a window the keyboard, Ctrl+V and Backspace work ("Typing", below). |
| `UiNav` | (one entity) | focus for keyboard and gamepad menus: the actions `ui_up`, `ui_down`, `ui_left`, `ui_right` move `current` to the nearest button that way and `ui_accept` clicks it (bind them in an `InputMap`: `ui_accept = enter, gamepad.a`). The focused button shows as hovered. Alone it focuses nothing until a direction is pressed (the first press focuses the first button); add a **`UiNavAuto`** beside it for a menu that takes the focus by itself: whenever no button is focused (at the start, or because the focused one was hidden) the visible button with the lowest uid is, so `ui_accept` works on a pause menu the moment it appears. |

## Rounded corners, gradients, pictures, shadows: `UiStyle`

`UiStyle` on the same entity as a `Ui` says how it looks beyond a flat rectangle. Without it nothing changes.

```json
{ "Name": {"value": "Play button"},
  "Ui": { "pos": [0.3, 0.8], "size": [0.2, 0.1], "color": [0.3, 0.8, 0.4, 1], "button": true, "text": "PLAY", "text_size": 0.05 },
  "UiStyle": { "radius": 0.05, "color2": [0.1, 0.45, 0.2, 1], "shadow_color": [0, 0, 0, 0.5], "shadow_offset": [0.004, 0.008] },
  "TextFont": { "file": "fonts/ui.ttf" } }
```

| Field | Meaning |
|---|---|
| `radius` | corner radius as a fraction of the picture's height (0.02 is gentle; half the box's smaller side makes a pill or a disc). The border and a bar's fill follow the shape. |
| `color2` | with an alpha above 0, `Ui.color` becomes a gradient from the top edge to `color2` at the bottom edge. A button's hover and pressed shading applies to both. |
| `image`, `image_uv`, `tint` | a texture name (a PNG or JPEG of the workspace, or a built-in one such as `circle`, `checker`) drawn over the colour, multiplied by `tint`. `image_uv` (x0, y0, x1, y1 from the top left, as fractions) picks the part shown, so one sheet can hold many icons. A bar that is part full (`fill`) shows that part of the picture. |
| `fit` | how the picture sits in the box: 0 stretched to fill it, 1 whole with its proportions kept (centred, so the box shows around it), 2 filling the box with the overflow cropped evenly. |
| `smooth` | filter a scaled picture (default true); false keeps hard texels, for pixel art. |
| `shadow_color`, `shadow_offset` | a drop shadow of the same shape, shifted by the offset (fractions of the picture's height); alpha 0 means none. |

Edges are anti-aliased in the reference picture and by the graphics card's own rasteriser on the GPU; the two agree to within a pixel's edge (the comparison test draws `examples/ui_showcase.lunaris.json` with both).

### TrueType fonts: `TextFont`

The built-in font is a 5x8 pixel font. For anything else put a `TextFont` with the path of a `.ttf` or `.otf` file of the workspace on the same entity as a `Text` (the HUD text) or a `Ui` (its `text` label):

```json
{ "Text": { "value": "Score {score}", "pos": [0.05, 0.05], "size": 0.06, "shadow": true }, "TextFont": { "file": "fonts/ui.ttf" } }
```

`size` is the font's em size as a fraction of the picture's height (0.06 of 1080 pixels is a 65 pixel em). Letters are drawn on whole pixels from a fixed origin, so a letter always looks the same wherever it is, and kerning is applied. `align` and `shadow` of a `Text` work as for the built-in font; a `Ui` label is centred in its box. Several lines: a newline (`
`) in the text starts a line, and `wrap` on the `TextFont` (a fraction of the picture's width, 0 for none) breaks lines at spaces so that none is wider; `line_spacing` is the extra gap between lines as a fraction of the size (default 0.25). A `Ui` label made of several lines is centred as a block. A file that is missing or is not a font is a warning (and `check` an error) and the built-in font is drawn instead, so no text goes missing. `files` with `kind: "font"` lists the fonts. Fonts have licences of their own: ship ones you may redistribute. `tools/make_test_font.py` makes the pixel font used by the tests, drawn from the engine's own glyphs.

### Names above heads: `UiAnchor`

`UiAnchor` pins a `Ui` or a `Text` to a point of the world, seen through the active camera. The point is `world` plus the position of `entity` (when one is named); the `Ui` is centred on it and a `Text` starts at it (so use `align` 1 to centre a name), moved by `offset` (fractions of the picture). It is hidden while the point is behind the camera. A health bar over each enemy is one `Ui` and one `UiAnchor` per enemy, made by a prefab. Pinned rectangles are drawn there but their mouse rectangle (`pos`) stays where the sim keeps it, so use pins for tags and bars, not buttons.

## Other screens: `UiScale` and `UiEdge`

`pos` and `size` are fractions of the picture's width **and** height, so a button made square on a 16:9 screen is a tall rectangle on a 4:3 one and a wide one on 21:9: the letters (their `size` is a fraction of the height) stay right and the boxes do not. Add one entity with a **`UiScale {aspect}`** (width over height of the picture the interface was made for: `1.7778` for 16:9, `1.3333` for 4:3, the lowest uid counts) and every `Ui` and `Text` is laid out inside the biggest rectangle of that shape that fits the picture, in its middle: its **safe area**. All the fractions are then fractions of the safe area, as if it were the whole picture: boxes keep their proportions, letters keep their size, borders and rounded corners keep theirs, and what is left at the sides (a wide window: 21:9 beside a 16:9 interface) or above and below (a tall one) is the picture's own: **the world is still drawn across all of it**, only the interface is kept to the safe area.

```json
{ "Name": {"value": "Interface shape"}, "UiScale": {"aspect": 1.7778} }
{ "Name": {"value": "Health bar"}, "Ui": {"pos": [0.02, 0.03], "size": [0.25, 0.04], "fill": 0.7}, "UiEdge": {"x": 0, "y": 0} }
{ "Name": {"value": "Fade to black"}, "Ui": {"pos": [0, 0], "size": [1, 1], "color": [0, 0, 0, 0.6]}, "UiEdge": {"screen": true} }
```

* **Exactly as before without it.** A scene with no `UiScale` is drawn as it always was, and a picture of the very shape it names is drawn pixel for pixel as without it. The safe area is whole pixels: on a 640 x 480 window it is the 640 x 360 picture at 60 pixels from the top, and the interface inside is that 640 x 360 picture exactly (the tests compare it pixel for pixel, in the reference renderer, and with the GPU renderer at 4:3 and 21:9).
* **`UiEdge`** on a `Ui` or a `Text` moves single elements to the **edges of the picture** instead of the safe area: `x` and `y` are 0 (the left or top edge), 1 (the right or bottom edge) or 0.5 (the middle, the default, which is no pin). A health bar with `x` 0 sits 0.02 of the made-for width from the left edge of the *picture* however wide the window, a score with `x` 1 stays in the right corner; the element keeps its size. With **`screen` true** the element is not in the safe area at all: its `pos` and `size` are fractions of the whole picture (a backdrop, a fade or a vignette that must cover the margins too).
* **The mouse follows.** `input.mouse_x` and `input.mouse_y` are fractions of the safe area when the world has a `UiScale` (below 0 or above 1 over the margins, so that nothing at the edge of the interface is clicked from there): `lunaris play` does this for you (`lunaris_sim::ui_kit::pointer_to_ui` is the whole conversion); a test or an agent that sends the mouse with the `input` tool gives the fractions of the interface directly, whatever size the picture is. `UiLayout` pads in the shape of the scale. A pinned (`UiEdge`) or `screen` element is drawn at its place, but its mouse rectangle stays where the safe area puts it, so use them for bars, backdrops and fades, and keep buttons in the safe area; `UiAnchor` pins (names over characters) follow their world point on the real picture and take the interface's size.
* `check` reports a shape that is not a number a picture can have (it is then ignored), a second `UiScale`, a `UiEdge` without a `UiScale`, without a `Ui` or `Text`, out of 0..1, and a button with one. The editor's play view does not map the pointer yet (it sends the fractions of its own view): use a view of the made-for shape there.

## UI kit v3: lists, drop-downs, tabs, drag and drop, tooltips, cooldown rings

The next sections are the widgets real menus and inventories need. They are all **new components** on top of `Ui` rectangles (`crates/lunaris-sim/src/ui_kit3.rs`; nothing was added to an existing component, so no scene's state hash changed), they run in the same place as the kit above (every tick, before the graph systems, so a graph system reacts in the tick it happens), and everything is testable with the `input` tool: `examples/ui_kit3.lunaris.json` (built by `tools/build_ui_kit3.py`) has one of each and `examples/tests/ui_kit3.test.json` plays it with `mouse_x`, `mouse_y`, `mouse_left` and `mouse_wheel`. The pictures of the reference renderer and the GPU renderer agree on all of it (`crates/lunaris-gpu/tests/compare.rs` draws the scene with both).

### Scroll views: `UiScroll` and `UiScrollBar`

A `UiScroll` on a `Ui` makes that rectangle a **viewport**. Everything whose `Parent` chain reaches it (a `Ui` or a `Text`, at any depth) is **cut at the viewport** and moves with the scroll `offset`. Lay the whole list out as if it were all on the screen (the children's `pos` is where they sit at offset 0; a `UiLayout` column on one child that is taller than the viewport does it for you) and the engine scrolls it:

```json
{ "Name": {"value": "Items view"}, "Ui": {"pos": [0.03, 0.14], "size": [0.27, 0.5], "color": [0.11, 0.13, 0.2, 1]}, "UiScroll": {} }
{ "Name": {"value": "Items content"}, "Ui": {"pos": [0.03, 0.14], "size": [0.25, 1.2], "color": [0, 0, 0, 0]}, "UiLayout": {"kind": 0}, "Parent": {"target": 4} }
{ "Name": {"value": "Item 1"}, "Ui": {"button": true, "text": "Item 1"}, "Parent": {"target": 5} }
{ "Name": {"value": "Items bar"}, "Ui": {"button": true, "color": [0.55, 0.65, 0.95, 1]}, "UiStyle": {"radius": 0.01}, "UiScrollBar": {"view": 4} }
```

| Field | Meaning |
|---|---|
| `UiScroll.offset` | `[x, y]`, how far the content has moved up and to the left, in fractions of the picture like `Ui.pos`; kept between 0 and `content` minus the viewport's size. Set it to scroll from a graph system or to start part way down. |
| `content_size` | the content's size in the same fractions. An axis left at 0 is **measured** from the visible children (their lowest and rightmost edge); the size in use is written to `content`. |
| `speed` | how far one notch of the mouse wheel moves the content, a fraction of the picture's height (default 0.06). |
| `drag` | dragging the content with the left button scrolls it (default true). A press on a button only turns into a drag once the pointer has moved a little, and a drag is never a click: a still press on a row still clicks it. A press on a `UiSlider` never starts one. |
| `changed` | true for the tick in which `offset` moved. |

* **Wheel**: `input.mouse_wheel` is in notches (`lunaris play` writes it for one tick; a test sets `{"mouse_wheel": -1}` with the `input` tool, steps, and sets it back to 0; it applies in every tick it is non-zero). It scrolls the **innermost view under the pointer that has somewhere to scroll** (sideways when there is no vertical range). Negative (towards you) scrolls down.
* **Pad and keyboard**: when `UiNav` moves the focus to a button inside a view the view scrolls just far enough to show it (so `ui_up` and `ui_down` bound to a stick or the arrow keys walk a list of any length), and the **right stick** (`input.gamepad.ry`) scrolls the view that holds the focus.
* **Buttons that are scrolled away do not react**: a row's hover, press and click only count while the pointer is also inside the viewport (a row half out of the window is clickable in its visible half).
* **Scroll bar**: a `UiScrollBar` on a `Ui` with `button` true is the thumb of the bar of the view named by `view`; an optional second `Ui` named by `track` is drawn behind it. The engine places both every tick along the right edge of the viewport (the bottom edge with `horizontal`), `thickness` wide (a fraction of the picture's height, so set `screen.aspect` or add a `UiScale` to make it equally thick in pixels), the thumb as long as the part of the content in view and as far along as the offset. Press the thumb and move to scroll; press the track to put the thumb under the pointer and carry on dragging. With `hide` (the default) both are invisible while there is nothing to scroll. Because the bar is an ordinary `Ui`, give it a `UiStyle` for a rounded, shaded handle, and a higher `layer` than the rows so it is drawn over them.
* Do not put a `UiLayout` on the viewport itself (it would put the children back in the viewport every tick); put it on a child. Scroll views nest; each clips to its own viewport and every one above.

**How the clip is drawn.** Both renderers get the clip from the code they share (`gather` in `crates/lunaris-render/src/ui.rs`): every rectangle and every text of an element inside a view carries the viewport (`SceneRect.clip`, `SceneText.clip`, whole pixels, rounded exactly like a rectangle's own edges). The reference renderer draws nothing outside it, a line of text is drawn in part where the viewport ends, and a rounded shape keeps the pixels it would have had; the GPU renderer sets a scissor rectangle for each run of clipped shapes and cuts the letter quads of the built-in font (texture coordinates and all). A scene without a scroll view is drawn exactly as before (the golden hashes prove it), and shapes wholly outside the viewport are not even sent to the card.

### Showing and hiding a whole panel: `UiGroup`

`UiGroup {shown}` on any entity hides it **and everything below it** (every entity whose `Parent` chain reaches it) while `shown` is false: nothing is drawn, nothing is under the pointer, `UiNav` does not focus it, a `UiLayout` gives it no room and a `UiScroll` does not count it as content. The `visible` flag of each element is not touched, so a button a game hid on purpose stays hidden when the panel comes back (Unity's `CanvasGroup`, Unreal's collapsed parent widget). A pause menu is one group that a graph system sets: `{"set": "UiGroup.shown", "to": {"res": "paused"}}`.

### Drop-downs and tabs

Both are small components on buttons you lay out and style yourself; the engine does the choosing.

```json
{ "Name": {"value": "Quality list"}, "Ui": {"pos": [0.36, 0.225], "size": [0.26, 0.19], "layer": 20}, "UiLayout": {"kind": 0}, "UiGroup": {"shown": false} }
{ "Name": {"value": "Quality"}, "Ui": {"pos": [0.36, 0.15], "size": [0.26, 0.07], "button": true},
  "UiDropdown": {"list": 7, "selected": 1, "resource": "ui.quality"} }
{ "Name": {"value": "High"}, "Ui": {"button": true, "text": "High", "layer": 21}, "UiOption": {"dropdown": 8, "index": 2}, "Parent": {"target": 7} }
```

| | |
|---|---|
| `UiDropdown` (on the header button) | `list` is the entity that holds the options (give it a `UiGroup`: the engine sets its `shown` to `open`, and falls back to its `Ui.visible` without one). A click on the header toggles `open`; a click on an option makes its `UiOption.index` the `selected` one (`changed` is true for that tick) and closes the list. `selected` is written to the resource named by `resource` every tick, and the header's text becomes the picked option's text, so a localized label is the option's own `{#key}`. Set `selected` from a graph system to preselect (a saved setting). |
| `UiOption` (on each option button) | `dropdown` is the header's uid, `index` the number it picks. Any button can be an option: a `UiLayout` column in the list, a `UiStyle` for rounded rows. |
| While it is open | a press **outside** the header and the list closes it, and **goes no further**: the button under the pointer neither hovers, presses nor clicks until the button is let go (the click that closed it is not also a click on whatever was there). Buttons the open list covers do not react where it covers them. The action `ui_cancel` closes it without picking, and so does moving the `UiNav` focus to something else (a focus that was elsewhere before it opened does not count). Opened from the keyboard or a pad (`ui_accept` on the focused header) the focus goes to the picked option, so `ui_up` and `ui_down` walk the list; picking or cancelling puts it back on the header. |
| `UiTabs` (on any entity) | `selected` is the index of the shown tab, written to the resource named by `resource` every tick; `changed` is true for the tick a click changed it. Set `selected` from a graph system to switch tabs. |
| `UiTab` (on each tab button) | `tabs` is the `UiTabs` entity's uid, `index` the tab's number, `panel` the entity shown while this tab is selected: the engine sets the `shown` of its `UiGroup` (without one, only the panel's own `Ui.visible`, which hides the panel's rectangle but not what is in it: use a group), and the tab takes `on_color` while selected and `off_color` otherwise. Everything in the hidden panels is out of the pointer's reach, so buttons in different panels may sit in the same place. |

**Layering of a popup.** An open list must be drawn over what it covers: give it a higher `Ui.layer` than the widgets under it. The built-in 5x8 bitmap font is drawn **over every rectangle** (that is how it has always worked, and changing it would change old pictures), so the text of a widget under an open list shows through the list. Give the labels that a list or a tooltip can cover a `TextFont` (the letters of the TrueType label of a **`Ui`** are shapes in the same order as the rectangles, so a list covers them properly; a `Text` with a `TextFont` is drawn after all the rectangles, like the bitmap one, so put text that can be covered in a `Ui`'s `text`): `examples/ui_kit3.lunaris.json` does, with the pixel font of the tests.

### Drag and drop: `UiDrag` and `UiDrop`

Inventory slots, equipment, a trash bin: a thing you pick up with the mouse and put down somewhere.

```json
{ "Name": {"value": "Slot A"}, "Ui": {"pos": [0.70, 0.15], "size": [0.1, 0.178], "layer": 1}, "UiDrop": {"name": "A"} }
{ "Name": {"value": "Slot B"}, "Ui": {"pos": [0.84, 0.15], "size": [0.1, 0.178], "layer": 1}, "UiDrop": {"name": "B"} }
{ "Name": {"value": "Sword"}, "Ui": {"size": [0.07, 0.124], "layer": 2, "button": true}, "UiDrag": {"item": 7, "slot": 3} }
```

| | |
|---|---|
| `UiDrag` (on a `Ui` button) | While the left button is held on it the rectangle follows the pointer, keeping the grip it was taken with, and is drawn above everything (`lift` is added to its `layer` meanwhile). `item` is a number of yours (an item id) that is reported with the drop; `kind` (0 to 31) is matched against the slots' `accept`. `slot` is the uid of the `UiDrop` it sits in (0 for a free thing). It is **centred in its slot every tick**, so it follows a slot that moves, scrolls or is hidden with its panel; a free thing returns to its `home`, its place at the start. |
| `UiDrop` (on a `Ui`) | A place to drop on. `accept` is the set of kinds it takes (bit `n` for kind `n`; all by default), `swap` lets a thing dropped on it trade places with the one it holds. The topmost slot under the pointer by `Ui.layer` wins. `holds` is the uid of what is in it (0 when empty), worked out from the things' `slot` every tick; `over` is true while a thing that fits is held over it (so a graph system can light it up). |
| Dropped on a slot | The thing's `slot` becomes it. A slot that holds something else **trades places** with it when the other thing fits where this one came from (and `swap` is on); otherwise, and when the slot does not take this `kind`, or the thing is let go over nothing, it **snaps back** to where it was. Putting a thing back in its own slot is not a drop. |
| What is reported | For one tick: `UiDrop.dropped` is true, `from` is the dragged entity's uid, `item` its `UiDrag.item`, `count` the number of drops so far; and the resources `drop.NAME` (the dragged entity's uid, back to 0 a tick later), `drop.NAME.item` and `drop.NAME.count` (`NAME` is the slot's `name`, or its uid when it has none). A graph system moves the real inventory with them: `{"if": {"get": "UiDrop.dropped"}, "then": [{"res_set": "inv.last_item", "to": {"get": "UiDrop.item"}}]}`. |
| Clicks | A press that does not move is a click as for any button (`Ui.clicked`); a drag, even one that ends on its own slot, is not. Only a press that starts on the thing picks it up (a held button that slides over it does not), and a `UiScroll` under it does not scroll while it is held. |

The thing is only a `Ui`, so it can be a picture (`UiStyle.image`), a rounded card, or anything with a label. Slots inside a scroll view work (they move, the things follow); a thing that is itself inside a scroll view is clipped at its viewport while it is held, so keep the things outside the view and the slots in it. `examples/tests/ui_kit3.test.json` carries a sword from slot A to slot B and back to nothing.

### Tooltips: `UiTooltip` and `UiTooltipBox`

A text box that comes up near the pointer after it has rested on something for a moment.

```json
{ "Name": {"value": "Sword"}, "Ui": {"button": true}, "UiTooltip": {"text": "Iron sword\n+5 attack", "delay": 0.4} }
{ "Name": {"value": "Tooltip"}, "Ui": {"size": [0.2, 0.08], "layer": 40, "visible": false, "text_size": 0.034, "color": [0.03, 0.04, 0.09, 0.96]},
  "UiStyle": {"radius": 0.012}, "TextFont": {"file": "fonts/ui.ttf"}, "UiTooltipBox": {} }
```

* **The trigger** is any `Ui` with a `UiTooltip`: its `text` (with `{resource}` placeholders and `{#key}` translations like any label, and `\n` for lines) and `delay` in seconds (the unscaled clock: a pause menu, where `time.scale` is 0, has tooltips). It has to be visible, not hidden by a `UiGroup`, and inside its scroll view's viewport under the pointer. Among overlapping triggers the topmost by `Ui.layer` wins.
* **The box** is one `Ui` of your own style with a `UiTooltipBox` (the lowest uid is used). The engine keeps it hidden until a tooltip is due, then sets its text, its size (`auto_size`: an estimate from the length of the text, 0.75 of the text size a letter and `padding` all round; turn it off and give the box a size when you know better) and its position: `offset` from where the pointer is, on the other side of the pointer where it would run off the right or bottom of the picture, and never off the top or the left.
* **When**: the timer starts when the pointer comes onto a trigger and starts again if the pointer moves more than a hair (0.006 of the height) before the delay is up; once the box is up it stays while the pointer wanders over the trigger. It goes when the pointer leaves, and a **press** takes it away until the pointer has left and rested again (you do not want a tooltip over the thing you are dragging: picking an item up is a press, so a carried item has none).
* Give the box a high `layer`, and give any label it may cover a `TextFont` (see "Layering of a popup" above).

### Radial fill: `UiRadial`

A cooldown wipe over an icon, a progress ring, a clock: a `UiRadial` on a `Ui` makes the rectangle draw a **sector** instead of a bar.

```json
{ "Name": {"value": "Cooldown ring"}, "Ui": {"pos": [0.7, 0.55], "size": [0.07, 0.124], "color": [0.95, 0.75, 0.25, 1], "back_color": [0.15, 0.2, 0.35, 0.9]},
  "UiRadial": {"value": 0.4, "start": 0, "shape": 0, "inner": 0.72} }
{ "Name": {"value": "Use wipe"}, "Ui": {"pos": [0.8, 0.55], "size": [0.07, 0.124], "color": [0, 0, 0, 0.62]}, "UiRadial": {"value": 0.4, "shape": 1} }
```

| Field | Meaning |
|---|---|
| `value` | how much of a turn is filled, 0 to 1 (more is all of it, less is nothing). Set it from a graph system: `{"set": "UiRadial.value", "to": {"res": "cooldown"}}`. |
| `start`, `clockwise` | where the sector begins, in degrees clockwise from twelve o'clock (any number, negative or over 360 too), and which way it runs. |
| `shape` | 0 the circle (or ring) inscribed in the rectangle (the smaller side is the diameter, centred), 1 the whole rectangle: the classic clock wipe over a square icon, corners and all. |
| `inner` | 0 to 1, with shape 0: the share of the radius that is a hole, which makes a ring (0.72 is a thin ring). |

The sector is drawn in `Ui.color` (a button's hover and press shading applies) and `Ui.back_color`, when its alpha is above 0, is the whole shape behind it (the track of a ring). The rectangle's border, shadow, gradient, picture and `fill` are not drawn for a dial (`check` says so): put a second `Ui` behind it for a frame or an icon. A `Ui.text` label is drawn as ever. Put a dial over a button (not itself a button) and the button still gets the clicks.

**How it is drawn.** `gather` gives the shape to both renderers as one `SceneRect` with a `Sweep` (`start`, `amount`, direction, shape, hole). The reference renderer decides every pixel from its direction from the middle with products and comparisons only (`Sweep::contains`, plus the deterministic sine and cosine for the two edges, so every machine draws the same bits), and anti-aliases the circle's edge like a rounded corner; the GPU renderer draws the same sector as triangles (`sweep_triangles` in `crates/lunaris-gpu/src/util.rs`: a fan round the arc, quads for a ring, a fan to the points where the edges and the corners meet the rectangle). The two agree except along the arc (a polygon against coverage) and on a pixel whose middle lies exactly on a straight edge, which can go either way (an odd-sized box with a sector that starts at twelve has such a column).

### What the editor's UI tab will need (not built yet)

`lunaris-studio` is a Windows application that is not part of these changes; this is the list for whoever builds the designer side of the widgets above. The play view and the picture already get everything (the editor draws with the same `gather_scene`, so clips, groups and sectors come for free); what is missing is authoring:

* **`UiScroll`**: draw the viewport's outline and the content's extent (`content`, measured or given) in the designer; a scrubber for `offset` that shows the clipped result; children are authored at offset 0, so the designer must show and drag them **at offset 0** (not where the current offset has shifted them) or it will write shifted positions back. A "make scroll view" action that wraps the selection in a viewport, a `UiLayout` column taller than it and a bar. Picking in the designer should honour the clip (a row scrolled out of the viewport is not under the mouse) with `lunaris_sim::ui_kit3::clip_rects`.
* **`UiScrollBar`**: a picker for `view` and `track`; ghost outlines of where the engine will put the thumb and track (they are placed every tick, so their `pos` and `size` in the file mean nothing).
* **`UiGroup`**: an eye toggle for `shown` in the entity tree and a dimmed look for the whole subtree while it is off (`lunaris_sim::ui_kit3::hidden_set` is the set); the designer must show a hidden panel when it is being edited.
* **`UiDropdown`, `UiOption`**: pickers for `list` and `dropdown`, an ordered list of options that edits their `index`, an "open" preview toggle that shows the list in the designer, a `selected` dropdown, and a "new drop-down" action that makes the header, the list with its `UiGroup` and `UiLayout`, and options.
* **`UiTabs`, `UiTab`**: a tab strip designer: "add tab" makes the button, its panel (with a `UiGroup`) and the link; clicking a tab in the designer previews that panel; per-tab `on_color` and `off_color`.
* **`UiDrag`, `UiDrop`**: a slot grid generator (rows, columns, spacing, a `name` per slot), dragging an item onto a slot to set its `slot`, a kinds editor that gives names to the 32 bits of `kind` and `accept`, and `holds` and `over` shown live in play.
* **`UiTooltip`, `UiTooltipBox`**: a text field and a delay on every widget's detail panel, a preview of the box at a pointer position (placement and flipping at the edges), the box kept out of the designer's way (it is hidden in the file).
* **`UiRadial`**: a scrubber for `value`, handles on the picture for `start` and the ring's hole, a shape switch, and the track and fill colours; drive `value` from a resource in the play view.
* **All of them**: show the new `check` rules in the problem list (each message says what the widget does not do), accept the mouse wheel in the play view (`input.mouse_wheel`, in notches), and map the pointer with `pointer_to_ui` there (the play view still does not, see "Other screens").

## Settings menus and going online from a menu

A game's **settings screen** and **main menu** are made of the widgets above (`UiTabs`, `UiSlider`, `UiToggle`, `UiDropdown`, `UiInput`, buttons) and a few resources the native player (`lunaris play`, an exported game) understands:

| Resource | Effect |
|---|---|
| `display.quality` | 0 classic, 1 low, 2 medium, 3 high, 4 ultra: the GPU quality, applied when it changes |
| `display.fullscreen` | 1 borderless full screen, 0 a window |
| `display.volume` | the master volume of the sound, 0 to 4 (1 = as mixed), applied when it changes |
| `display.fps` | the most pictures a second (0 = no limit; `--fps` on the command line wins; with vsync a cap above the screen's refresh does nothing) |
| `display.vsync` | 1 wait for the display's refresh before showing a picture (the default of a window), 0 show each picture at once; applied when it changes (a V-sync switch); `--novsync` and `--fps` on the command line win |
| `perf.want` / `perf.fps`, `perf.ms` | a game that shows a frame counter sets `perf.want` above 0.5; the player then writes `perf.fps` (pictures a second, over half a second) and `perf.ms` (milliseconds a picture) twice a second, and nothing otherwise (a resource written in every game would change every game's state); not in a lockstep game. Show it with a `Text` of `{perf.fps} FPS` |
| `cursor.hide` / `cursor.lock` | a menu that draws its own pointer hides the system one / a first-person game holds it |
| `settings.save` | the game sets it above 0.5 (an APPLY button) and the player writes the **settings** to `settings.json` in the game's folder and sets it back to 0 |

**Settings are kept between runs:** every resource whose name starts with `opt.` (the game's own: `opt.fov`, `opt.sens`, `opt.invert_y`, ...) or `display.` is written to `settings.json` by `settings.save` and read back into the resources when the game starts, before its first tick. A game gives its resources defaults in the scene file, and the saved ones replace them. **Text fields remember too (2026-10-23):** a `UiInput` with `remember` `NAME` (an address, a player's name; never a secret) has its text kept in the same file as `text.NAME`: it is written when the text has changed and no field is being typed in any more (a click elsewhere, Enter), and put back before the first tick of the next start (the scene's own text is the default). Two fields with the same name share the text. **Keys the player chose (2026-10-23)** are kept the same way, as `bind.ACTION.N` (`KeyBind`, `docs/KEYBINDS.md`; only what differs from the scene). A settings screen is then: a `UiSlider` per number whose value a graph system copies to `opt.*` (and, for the engine's own, to `display.*`); other graph systems apply the game's own choices (`Camera.fov_y` from `opt.fov`, `MouseLook.sensitivity` from `opt.sens`, `Environment.exposure` from `opt.brightness`); and an APPLY button that sets `settings.save`. Reading the saved values into the widgets at start is the same copy the other way for one tick (the graph system sets `UiSlider.value` from the resource while a resource `ui.settings_open` just went on).

**Going online from a menu (`NetConnect`).** `lunaris play SCENE --connect HOST:PORT` connects at start. A game that wants a **menu first** puts one entity with a `NetConnect` in its scene instead (docs/GAME_SERVER.md has the server side). `address_input` and `name_input` are the uids of two `UiInput` text fields (the address as `host:port`, the player's name); a CONNECT button's graph system sets `NetConnect.go`; the player starts the connection, `NetConnect.status` says how it goes (0 nothing, 1 connecting, 2 in, 3 the server said no, 4 lost, 5 no answer within `timeout` seconds or a bad address) and `NetConnect.message` says it in words ("The server is full", "No answer from the server", ...), and `NetPlayer.connected` is on while status is 2. After 3, 4 and 5 the attempt is spent: the menu stays, `go` tries again. `leave` (a LEAVE button) tells the server, takes the other players away and sets the status to 0. `check` warns about a `NetConnect` whose inputs are not `UiInput`s. Code: `crates/lunaris/src/net_scale_cmd.rs` (`menu_driver`), `NetDriver::link` in `crates/lunaris-net/src/scale/play.rs`.

**Typing (a window, `lunaris play`, 2026-10-23).** Before this a text field could only be typed into through the API (`type_text`: one `input.char` a tick): a player at a real keyboard had no way to type an address or a name, because the window only turned keys into `input.<name>` (`a`, `space`, `enter`, ...), never into characters. Now a key that makes text (what the keyboard layout says, so Shift, AltGr and other layouts work) is typed into the field that has the focus; Ctrl+V pastes the clipboard's text (Windows; trimmed; control characters left out; at most `max_len`); Ctrl+Backspace empties the field; holding Backspace repeats. Typed and pasted text goes in **at once, between two ticks** (`ui_kit::type_into_focused`, `clear_focused`, `has_focused_input`): a pasted connect token is hundreds of characters and `input.char` takes one a tick. The keys still reach the game as `input.<name>` too, and nothing is typed when no field has the focus. **While a field has the focus every action of the `InputMap` reads 0** (`gameplay::actions`; the ones that were held are released), so a letter typed into a chat box does not walk, reload, drop or build; the raw `input.<key>` resources are still there for a game that wants them. Not in a lockstep game (`--join`/`--host`: a typed character is not an input every copy gets; menus there are the game's own). Not recorded by `record_start` (typing is the player's interface state, like a settings change, and is not part of the replayed inputs; use `type_text` in a test that must replay). `show_last` keeps a long text inside its box: a token field shows `...` and its last 20 characters while `UiInput.text` holds all of it.

**A secure server (`NetConnect.token_input`, `token`).** A server run with `--secure` (docs/SECURE.md) lets in only players with a connect token: `token_input` is the uid of a `UiInput` where the player pastes the token (or `token` holds the text), and `lunaris play SCENE --connect-token TOKEN` (no `--connect`) hands the menu a token at start. A typed token wins; with a token the player is whoever it names (the name field is ignored), and a text that is no token is `status` 5 with the reason.

## Where the mouse comes from

`lunaris play` sets the three resources from the window; the Lunaris Editor does while it plays (the pointer over the view); a test or an agent sets them with the `input` tool: `{"values": {"mouse_x": 0.5, "mouse_y": 0.6, "mouse_left": true}}`, then steps, then `{"mouse_left": false}`. In a lockstep game (`docs/NETWORK.md`) they travel like any other input. `examples/tests/menu.test.json` clicks the buttons of `examples/menu.lunaris.json` this way and checks the health, the bar and a drag-away release.

## Limits

Rectangles are placed by fractions of the picture (of the safe area with a `UiScale`), or by a `UiLayout` container for rows, columns and grids. No right-to-left or shaped text (Arabic, Indic scripts), no rich text (bold and colours inside one string), no nine-slice pictures. A picture is drawn without mip maps, so shrinking it a lot looks harsh; ship it near the size it is shown. Both renderers draw plain rectangles as whole pixels, so the GPU picture matches the reference renderer's (the comparison test includes the menu and `ui_showcase` scenes). The kit's own limits: the bitmap font is always over every rectangle (so use a `TextFont` where a popup must cover text); a scroll view clips the content of **any depth** but moves it by shifting `pos` (see "Scroll views"); one open drop-down at a time is the usual case (several work, each closes on a press outside); drag and drop is for the mouse (a keyboard or pad cannot pick a thing up yet); a tooltip is for the mouse (nothing shows one for the `UiNav` focus yet); a sector of a `UiRadial` is flat-coloured (no gradient or picture).

## Live graphs: `UiPlot`

A `UiPlot` on an entity with a `Ui` draws a scrolling bar graph of a resource: a CPU load, a frame time, a population. The `Ui` is the frame and background (its colour, border, layer); the plot makes its own bars the first tick (ordinary `Ui` rectangles, so both renderers draw it and nothing else changes), takes a sample every few ticks and scrolls the graph to the left, newest on the right.

```json
{ "Name": {"value": "CPU graph"},
  "Ui": { "pos": [0.05, 0.6], "size": [0.4, 0.2], "color": [0.05, 0.06, 0.1, 0.9], "border": 0.002, "border_color": [0.3, 0.4, 0.6, 1] },
  "UiPlot": { "source": "cpu.percent", "every": 6, "samples": 80, "min": 0, "max": 100, "bar_color": [0.4, 0.8, 1, 1] } }
```

| Field | Meaning |
|---|---|
| `source` | the name of the resource to plot. A resource that is not set counts as 0. |
| `every` | ticks between samples (6 at 60 ticks a second is ten samples a second). |
| `samples` | bars shown, 1 to 128 (the last that many samples). |
| `min`, `max` | the vertical scale. When `max` is not above `min` the scale is automatic: from zero (from the lowest sample when `log` is on) to the largest sample shown. |
| `log` | plot the base-10 logarithm of the value: for something that grows by factors, such as a population that doubles every half hour. |
| `gap`, `bar_color` | the space between bars as a share of a bar's slot, and the bars' colour. |
| `scale_lo`, `scale_hi` | the engine writes the scale in use (read them from a graph system to label the axis); `data`, `head`, `count`, `timer` and `built` are the engine's too. |

A plot inside a `UiLayout` container follows the container; hiding the plot (`Ui.visible` false) hides its bars. The bars are entities with `UiPlotBar` (`plot`, `index`); an entity with no `UiPlot` costs nothing.

