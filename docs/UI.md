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
| `text`, `text_size`, `text_color` | a label centred in the rectangle, with `{resource}` placeholders like `Text`. |
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
| `UiInput` | a `button` | a click focuses it; characters typed (the resource `input.char` holds the code point for one tick, plus `input.backspace`, `input.enter`, `input.escape`; the `type_text` tool does it for an agent) go into `text`; `submitted` is true the tick after Enter; `placeholder` shows while empty; `max_len` characters. |
| `UiNav` | (one entity) | focus for keyboard and gamepad menus: the actions `ui_up`, `ui_down`, `ui_left`, `ui_right` move `current` to the nearest button that way and `ui_accept` clicks it (bind them in an `InputMap`: `ui_accept = enter, gamepad.a`). The focused button shows as hovered. |

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

## Where the mouse comes from

`lunaris play` sets the three resources from the window; the browser editor does while it plays (the pointer over the view); a test or an agent sets them with the `input` tool: `{"values": {"mouse_x": 0.5, "mouse_y": 0.6, "mouse_left": true}}`, then steps, then `{"mouse_left": false}`. In a lockstep game (`docs/NETWORK.md`) they travel like any other input. `examples/tests/menu.test.json` clicks the buttons of `examples/menu.lunaris.json` this way and checks the health, the bar and a drag-away release.

## Limits

Rectangles are placed by fractions of the picture, or by a `UiLayout` container for rows, columns and grids. No right-to-left or shaped text (Arabic, Indic scripts), no rich text (bold and colours inside one string), no nine-slice pictures. A picture is drawn without mip maps, so shrinking it a lot looks harsh; ship it near the size it is shown. Both renderers draw plain rectangles as whole pixels, so the GPU picture matches the reference renderer's (the comparison test includes the menu and `ui_showcase` scenes).
