@echo off
rem Two-player ROLLBACK demo on one computer: a host and two windows (player 1 and player 2), WASD and Space in the focused window.
rem Player 2's network is made worse on purpose (80 ms each way, 30 ms jitter, some messages late): watch the window title, it counts
rem the rollbacks. A third window joins as a spectator a few seconds into the game: it replays the game from tick 0 at full speed, then
rem follows it live. Close the host to end the game (see docs/NETWORK.md).
cd /d "%~dp0"
if not exist target\release\lunaris.exe (
  echo Building first: cargo build --release -p lunaris
  cargo build --release --offline -p lunaris || exit /b 1
)
start "Lunaris host" target\release\lunaris.exe host --players 2 --port 7000 --rollback
timeout /t 1 /nobreak > nul
start "Lunaris player 1" target\release\lunaris.exe play examples/duel.lunaris.json --join 127.0.0.1:7000 --rollback --name Orange --size 800x450
start "Lunaris player 2 (bad network)" target\release\lunaris.exe play examples/duel.lunaris.json --join 127.0.0.1:7000 --rollback --name Blue --size 800x450 --sim-latency 80 --sim-jitter 30 --sim-loss 3
timeout /t 5 /nobreak > nul
start "Lunaris spectator" target\release\lunaris.exe play examples/duel.lunaris.json --join 127.0.0.1:7000 --watch --name Eve --size 800x450
