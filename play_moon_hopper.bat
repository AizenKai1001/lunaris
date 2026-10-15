@echo off
rem Plays Moon Hopper (a 2D platformer) in a native window: A D or arrows to run, Space W or Up to jump, Esc quits.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/moon_hopper.lunaris.json
