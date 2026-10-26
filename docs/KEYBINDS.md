# Rebinding keys

A game that has an `InputMap` (`docs/GAMEPLAY.md`) can let the player choose the keys: one `KeyBind` button for each key of each action, and the engine does the rest. The text of the `InputMap` is rewritten, so nothing else in the game
has to know: the action reads the new key the very next tick. The keys that differ from the game's own are kept in `settings.json` and come back at the next start. Texts that name a key (`Press {key:use} to open`) follow the change.

## The button

`KeyBind` goes on a `Ui` button:

```json
{"Ui": {"pos": [0.6, 0.3], "size": [0.12, 0.05], "button": true}, "KeyBind": {"action": "jump", "index": 0}}
```

* `action` is the action's name in the `InputMap`, `index` which of its keys (from 0, in the order the text has them). Only keys count: pad terms (`gamepad.a`) and `$resources` are skipped, so `move_y = w, s:-1, gamepad.ly` has the keys
  `w` (0) and `s` (1), and rebinding `s` keeps its `:-1`.
* The engine writes the button's text: the key now (`E`, `SPACE`, `LEFT CLICK`, `CTRL`, `NUM 5`), and `PRESS A KEY` while it waits. You style the button; the engine never touches its colours (`KeyBind.listening` is true while it waits: a graph
  system can light it).
* A click starts the wait. The next key or mouse button that is pressed becomes the action's key. Esc cancels. The click itself is not taken, and neither is a key that is held down when the click happens (let go, then press). A click on another button of the screen (a tab, BACK, another key button) is not taken either: it ends the wait and does its own work (another key button then waits); the left mouse button is a key when the pointer is over no button (an empty place of the panel).
  Shift, Ctrl and Alt are taken when they are pressed alone; with another key, that key is taken.
* A key that another action already has: if that one has a button of its own (a `KeyBind` for that action and key), the two **trade places** (nothing is left without a key); if it has none, the key is refused (`rebind.denied` is 1 and nothing changes). So give a
  button to every key that can be taken, and leave without one what the player must not lose (Esc for the pause menu).
* `default` is the key the scene had. Leave it empty: the engine fills it in on the first tick from the text it finds. `rebind.reset` above 0 puts every button back to its default (and `settings.save` is set).

The names a key can have are `lunaris_sim::rebind::BINDABLE`: letters, digits, `space`, `enter`, `tab`, `backspace`, `escape`, the arrows, `shift`, `control`, `alt`, `f1` to `f10` and `f12`, `minus`, `equal`, the brackets, `semicolon`, `quote`,
`backquote`, `backslash`, `comma`, `period`, `slash`, `capslock`, `insert`, `delete`, `home`, `end`, `pageup`, `pagedown`, the numpad (`numpad0` ... `numpad9`, `numpadadd` ...), `mouse_left`, `mouse_right`, `mouse_middle`, `mouse_back`,
`mouse_forward`. (F11 is the window's own: it toggles full screen.) A pad is not rebound here.

## While it waits

`rebind.state` is the engine's: 0 nothing, 1 a button was clicked and a key is still down, 2 the next key will be taken, 3 a key was taken (or Esc cancelled) and is not let go of yet. While it is above 0 **every action reads 0** (the same
rule as for a focused text field, `docs/UI.md`): the key meant for the button must not also shoot, jump or open the menu, and neither must the key just taken, until it is let go of.

Order inside a tick: the key bindings run first (the stage `key bindings`), then the actions, then the interface widgets. That order matters: the widgets use up `input.escape`, `input.enter` and `input.backspace` in every tick (a text field's keys), so a key that is waited for has to be seen before them; and the click on a key button is the one the widgets saw in the tick before.

A key whose release never arrives (a lost key-up) cannot hold the game's actions at 0 for ever: the wait for the keys to be let go of (states 1 and 3) gives up after five seconds (`rebind.hold` counts the ticks) and the game is as it was.

The game's resources: `rebind.cancel` above 0 ends a wait (set it when the screen closes); `rebind.reset` above 0 restores the defaults. The engine's: `rebind.count` (how many changes were made, for a click sound), `rebind.denied`, `settings.save`
(set after a change: the native player writes the file at once).

## Kept between runs

`lunaris play` keeps what differs from the scene in `settings.json` as `bind.ACTION.N` (the Nth key of the action) with the key's name as the value: `"bind.jump.0": "x"`. The file is read before the first tick and the keys are put back one by one with
the same trade rule; a line that no longer fits the game (the action or the button is gone, the name is not a key) is left alone and dropped at the next save. A key that is back to the scene's own is not written.

## Texts that name a key

`{key:ACTION}` in a `Text` or in the text of a `Ui` is the first key of the action as the `InputMap` has it now, `{key:ACTION:N}` the Nth (`{key:move_y:1}` is the back key). What is not there is left as it was written. A prompt, a hint line
or a controls page that says `{key:use}` never goes stale. (A rectangle that was sized for a short label does not grow: leave room for a label like `LEFT CLICK`.)

## Limits

* The bindings are part of the world, so this is not for a lockstep or rollback game, where every player's copy must play the same way. In a game server (`lunaris server`) the keys are the client's own: the server never sees them.
* One map: the lowest uid that has an `InputMap` (as for the actions). One key per button, no chords, no mouse wheel, no pad.

## Testing

`input {values: {x: true}}` presses a key the same way a window does, so a test clicks the button (`mouse_x`, `mouse_y`, `mouse_left` down, then up), steps, presses a key and reads the `InputMap` (`get`) and the button's `Ui.text`.
`crates/lunaris-sim/src/rebind_tests.rs` has the engine's own checks; a game's own smoke test can play them through its real settings screen.
