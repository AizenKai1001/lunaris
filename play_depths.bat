@echo off
rem Plays Depths, the dungeon crawler, in a native window: WASD or a pad, Space swings the sword, E talks, Esc pauses (the title has Quit). It saves itself to saves/slot1.json when a level begins; Continue on the title loads it.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/depths/boot.lunaris.json %*
