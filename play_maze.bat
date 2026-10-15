@echo off
rem Plays the maze example in a native window: a Seeker walks a patrol through the maze by itself (navigation). Esc quits.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/maze.lunaris.json
