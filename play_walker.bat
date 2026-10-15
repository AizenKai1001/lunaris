@echo off
rem Plays the walker example in a native window (GPU renderer, high quality): Esc quits, F11 toggles full screen. WASD and Space where the scene uses them.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/walker.lunaris.json
