# Security policy

Lunaris is early software (version 0.0.x). It runs game code and tool calls that you give it, so please read how it protects you and what it does not.

## What the engine does

* **The tool server** (`lunaris serve`) talks over standard input and output only: it opens no network port. The editor (`lunaris edit`) is a desktop window, not a server.
* **Files**: every path a tool accepts goes through one sandbox function (`lunaris_json::safe_relative`) and stays inside the workspace folder; paths that climb out (`..`), absolute paths and drive letters are refused.
* **Native plugins** (`.dll` / `.so` systems) are off unless you start the engine with `--allow-plugins`. A plugin is native code: only allow plugins you trust. **WebAssembly modules** run in an interpreter with no access to the machine and a fuel limit.
* **Multiplayer** (`lunaris host`) listens on the address you give it (default: this machine only) and expects other copies of the same game; it is not hardened against hostile players: do not expose a host to the internet without a reason.
* The player and the exported games read the scene files next to them and nothing else.

## Reporting a problem

Please report a vulnerability **privately** with GitHub's "Report a vulnerability" button on the repository's Security tab, rather than in a public issue. Say what you did, what happened and what you expected; a scene or a command that shows it is the most useful. Reports are answered as soon as the maintainers can, usually within a few days. There is no bug bounty.

## What is not a vulnerability

* A scene or script that makes the game slow, crash itself or loop for ever (the simulation runs what it is given).
* Anything that needs `--allow-plugins` and a hostile plugin.
