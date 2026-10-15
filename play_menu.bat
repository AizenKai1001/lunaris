@echo off
rem Plays the menu example in a native window: click the buttons (the pointer is input.mouse_x/y, the left button input.mouse_left). Esc quits.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
target\release\lunaris.exe play examples/menu.lunaris.json
