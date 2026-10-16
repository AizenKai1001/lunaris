"""Builds "Caldera", a third-person island adventure in 3D, only by calling the engine's tools: every scene file of `examples/caldera/` is written by `lunaris serve` from this script,
the way an AI agent would build it. The pictures, models and sounds come from `tools/make_caldera_world.py`, `make_caldera_models.py` and `make_caldera_audio.py`; the rules are Lunaris Script
(`tools/caldera_scripts.py`); the texts are in English and German (`examples/caldera/lang/`).

  boot.lunaris.json      the start scene: the music (four `Persist` players) and the mixer, then the title
  title.lunaris.json     the island from the air, Play / Continue / Settings / Quit (keyboard, mouse and pad)
  island.lunaris.json    the game: the island, the camp, six ember shards, five stone golems and a boss, the night's wraiths, the HUD, the pause menu, saving at the fire
  gameover.lunaris.json  Retry (from the last fire) or back to the title
  win.lunaris.json       the end screen after the beacon cutscene

    python -I tools/make_caldera_world.py && python -I tools/make_caldera_models.py && python -I tools/make_caldera_audio.py        (once: the pictures, models and sound)
    python3 tools/build_caldera.py [scene ...]      (needs target/release/lunaris; the island is the slow one)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import caldera_lib as L  # noqa: E402
import caldera_scenes as SC  # noqa: E402


def main():
    only = sys.argv[1:]
    os.makedirs(L.D, exist_ok=True)
    SC.stub_scenes()
    L.start()
    results = {}
    for name, fn in (("island", SC.build_island), ("title", SC.build_title), ("gameover", SC.build_gameover), ("win", SC.build_win), ("boot", SC.build_boot)):
        if not only or name in only:
            results[name] = fn()
    L.finish()
    L.write_lang()
    missing = SC.check_language_keys() if not only else 0
    print(f"done ({L.n_requests} requests, {L.failed} failed, {missing} text keys missing from the language files)")
    return 1 if L.failed or missing else 0


if __name__ == "__main__":
    sys.exit(main())
