@echo off
rem Lunaris Editor: the engine's editor, a Windows application. Opens the scene given on the command line, or an empty one.
rem     "Lunaris Editor.bat" examples/echo_tour.lunaris.json
cd /d "%~dp0"
if not exist target\release\lunaris-editor.exe (
  echo Building the editor first: cargo build --release -p lunaris-studio
  cargo build --release --offline -p lunaris-studio || exit /b 1
)
start "" target\release\lunaris-editor.exe %*
