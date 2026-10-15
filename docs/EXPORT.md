# Packaging a game: `export_game` and `lunaris export`

A game made in the workspace is a scene plus the files it names. Exporting writes both into one folder that plays by itself, so you can zip it and hand it to a friend.

```
lunaris export examples/ui_showcase.lunaris.json --out dist/showcase --name showcase
lunaris call export_game '{"out":"dist/showcase","name":"showcase"}' --scene examples/ui_showcase.lunaris.json    # the same, as a tool call
```

An agent calls the tool `export_game` on the world it has built: the folder gets the world **as it is now** as the start scene.

## What ends up in the folder

```
dist/showcase/
  showcase.lunaris.json     the start scene (the world as it was exported, with its systems, prefabs and resources)
  examples/fonts/pixel.ttf   every file the scene names, at the same relative path it has in the workspace
  lunaris.exe               the player program (copied from the `lunaris` that ran the export; `--no-player` / "player": false skips it)
  play.bat, play.sh         launchers: `lunaris play showcase.lunaris.json`
  README.txt                how to play, and a reminder that the files keep their own licences
  export.json               what was exported: the scene, its state hash, every file with its size, and anything missing
```

**How files are found.** Every string in the scene that is a path of an existing file in the workspace is copied: textures (PNG, JPEG), models (`.glb`, and for a `.gltf` its buffers and pictures), sounds (WAV, Ogg, MP3), fonts, heightmaps, WebAssembly modules, other scenes. A game with a `Localization` also gets every `lang/*.json`. A string that ends like a game file (`.png`, `.glb`, `.ogg`, `.ttf`, ...) and names nothing is listed as `missing` in the result and in `export.json`: the exported game draws the same placeholder the workspace does, so it is a bug in the scene, not in the export. A file only a **script computes** the name of (a graph system that builds `"sounds/" + n + ".ogg"`) is not found; keep one entity that names each such file, or copy the folder by hand.

**A game of several scenes.** A scene the game can switch to (`SceneSwitch.path`) is copied with everything it uses, and so is the scene that one names, down the chain: export the first scene and the whole game comes. A text field names files too: every line of a `MusicPlayer` playlist (`music/theme.wav volume=0.8`) counts by its first word. A `Localization.language` that is a path (`mygame/lang/en` is the file `mygame/lang/en.json`) brings that folder's `.json` files, the other languages.

**Checked first.** `export_game` runs `check` and refuses a game that has errors (a missing texture on a mesh, an unreadable font) so you do not ship something broken; `force: true` / `--force` exports anyway and says how many errors it ignored.

**Safe.** It writes only inside the folder you name (which must be inside the workspace, never the workspace itself), deletes nothing, and never changes the workspace. A folder that already has files in it is refused unless `overwrite: true` / `--overwrite`; then files with the same names are replaced and everything else is left alone.

## Playing it

On Windows, run `play.bat` (options after it go to the player: `play.bat --size 1920x1080 --quality high --mute`). The player draws with the GPU renderer and falls back to the CPU one (`--cpu`). The same folder works with `lunaris verify`/`run` for playtests: the scene's relative paths are the same everywhere.

To prove that a folder plays by itself, start its player as a hidden self-test and let it press keys: `./lunaris play depths.lunaris.json --hidden --frames 260 --mute --press enter@45:4 --report --save-frame shot.png` (it opens no window, so it needs no display: it runs over ssh and in CI). `tools/check_depths_export.py` does exactly this for `examples/depths`, plays the workspace copy the same way and requires the same state hash from both.

## Limits

- The player program is the engine's own `lunaris` binary: it is copied only when the export is run by `lunaris` itself (the CLI, or the MCP server), not by a program that embeds the engine as a library.
- No compression, encryption or single-file archive: the folder is plain files, so it is easy to inspect, patch and diff. (A game that must hide its assets needs a packer in front of this.)
- Native plugins (`.dll`/`.so`) are not copied (they run only with `--allow-plugins`, which is the player's choice); sandboxed WebAssembly modules are.
- Saved games (`saves/`) are never exported.
- The player is the `lunaris` program that did the export: exported from a Linux `lunaris`, the folder holds a Linux player and `play.sh` starts it (an X11 window, GPU renderer); from a Windows one, `play.bat` does. macOS has no player yet.
