@echo off
rem Plays Orb Dash in a native window: WASD or the arrow keys, Esc quits, F11 toggles full screen.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/orb_dash.lunaris.json
