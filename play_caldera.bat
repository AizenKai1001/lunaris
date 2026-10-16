@echo off
rem Plays Caldera, the island adventure, in a native window (the GPU renderer, high quality): WASD or a pad's left stick walks, the mouse or the right stick turns the camera, Shift runs, Space jumps, click swings the sword, right click rolls, E uses (talk, rest and save at the fire, place the shards, light the beacon), L is the quest log, Esc pauses (the title has Quit). Find the six ember shards, carry them to the altar in the camp, light the beacon. It saves itself at the fire; Continue on the title loads it.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/caldera/boot.lunaris.json %*
