@echo off
rem Two-player lockstep demo on one computer: a host and two windows (player 1 and player 2); WASD and Space in the focused window. Close any window to end the game.
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
start "Lunaris host" target\release\lunaris.exe host --players 2 --port 7000
timeout /t 1 /nobreak > nul
start "Lunaris player 1" target\release\lunaris.exe play examples/duel.lunaris.json --join 127.0.0.1:7000 --name Orange --size 800x450
start "Lunaris player 2" target\release\lunaris.exe play examples/duel.lunaris.json --join 127.0.0.1:7000 --name Blue --size 800x450
