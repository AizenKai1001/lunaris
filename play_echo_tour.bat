@echo off
rem A 17 second walk through four places that sound different: a tiled bathroom (a dripping tap), a carpeted living room (a radio behind a wooden door),
rem a concrete hall (handclaps) and a courtyard under the sky. Wear headphones; --volume 0.5 turns it down. Esc quits. See docs/AUDIO.md.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/echo_tour.lunaris.json --volume 0.6
