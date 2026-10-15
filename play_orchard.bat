@echo off
rem Plays the orchard example in a native window: forty trees from one prefab, balls raining from a spawn_prefab statement. Esc quits.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/orchard.lunaris.json
