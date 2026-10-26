# `ui_audit`: a listing of the on-screen interface for quality checks

The `ui_audit` tool says what a picture of a given size would show of the interface, without drawing it: for every visible `Ui` and `Text` its box and the box of its letters in pixels. It is for tests and agents that look for text running out of its button, texts over each other, elements off the screen and letters too small to read at another window size (a game's own `ui_overlap_check.py` is a user of it).

```json
{"tool": "ui_audit", "arguments": {"width": 1280, "height": 720}}
```

`width` and `height` (default 1920 x 1080, 16 to 8192). The reply is `{width, height, count, elements: [...]}`; each element:

| Field | Meaning |
|---|---|
| `uid`, `name`, `parent`, `layer` | the entity, its `Name`, its `Parent` (0 for none) and `Ui.layer` (0 for a `Text`) |
| `is_text` | a `Text` (drawn over every rectangle) and not a `Ui` |
| `rect` | a `Ui`'s own rectangle in pixels `[x0, y0, x1, y1]` after the `UiScale` safe area and `UiEdge` pins (`null` for a `Text`) |
| `ink` | the box the letters cover (a union of the glyph quads for a TrueType text, so side bearings are not in it; the 5 x 8 font's box as the reference renderer draws it); `null` when nothing is drawn |
| `label`, `template` | the words with `{resource}`, `{key:ACTION}` and `{#key}` filled in, and as written (a `{key:..}` that is still in `label` names an action the `InputMap` does not have; an unset resource reads 0) |
| `font`, `em_px` | the `TextFont` file (empty for the built-in font) and the letters' em in pixels |
| `button`, `color`, `back_color`, `text_color`, `image`, `shadow`, `anchored`, `clip` | what the element is and draws |

**How:** `lunaris_render::ui_audit::audit` calls `gather_scene` (the code both renderers draw from) and sorts what comes back: the letters of TrueType text are the shapes whose picture is a font page (512 x 512, not smoothed), the built-in font's boxes follow `draw_text`, a `Ui`'s box is its fractions mapped through the safe area and pin exactly as `ui::gather` does. A `UiGroup` that is hidden takes its children out of the list, as it does from the picture. Nothing changes in the world (the tool is read-only: the revision does not move). Cost: one `gather_scene` (about the cost of a picture's gather, no drawing). `UiAnchor` pins use the rectangle the pin gave.

**Limits:** it does not know a panel picture's own frame (a few pixels inside the box), nor what a graph system will do next tick; side bearings are not counted in `ink`; a `Text` has no box of its own, only its letters.

Tests: `crates/lunaris-render/src/ui_audit_tests.rs` (4: a label wider than its button, TrueType letters and wrapping with the real font, the safe area and `UiEdge`/`UiGroup`, placeholders and an unknown key action).
