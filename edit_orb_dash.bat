@echo off
rem Opens the Lunaris editor on Orb Dash in your browser. Press Play, then WASD or the arrow keys.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe edit examples/orb_dash.lunaris.json --open
