"""Run: python -I clients/python/test_lunaris.py   (needs target/release/lunaris)"""
import os
import subprocess
import sys
import time
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lunaris import Embedded, Engine, LunarisError, _find_exe  # noqa: E402


class ClientTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.eng = Engine(root=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

    @classmethod
    def tearDownClass(cls):
        cls.eng.close()

    def test_every_advertised_tool_is_a_method(self):
        self.assertGreaterEqual(len(self.eng.tools), 36)
        for name in ("spawn", "query", "define_system", "render", "step"):
            self.assertTrue(callable(getattr(self.eng, name)), name)

    def test_spawn_query_and_keyword_arguments(self):
        self.eng.reset()
        self.eng.define_component(name="Health", fields=[{"name": "hp", "type": "f32", "default": 100}])
        self.eng.spawn(count=3, components={"Transform": {}, "Health": {}})
        q = self.eng.query(with_=["Health"], select=["Health"])
        self.assertEqual(q["total"], 3)
        self.assertEqual(q["entities"][0]["components"]["Health"]["hp"], 100)

    def test_errors_carry_code_path_and_hint(self):
        self.eng.reset()
        with self.assertRaises(LunarisError) as cm:
            self.eng.spawn(components={"Transfrom": {}})
        self.assertEqual(cm.exception.code, "unknown_component")
        self.assertIn("Transform", cm.exception.hint)

    def test_determinism_from_python(self):
        hashes = []
        for _ in range(2):
            self.eng.reset(seed=5)
            self.eng.spawn(count=50, components={"Transform": {}, "Velocity": {"lin": [1, 0, 0]}, "Wander": {"strength": 2}})
            hashes.append(self.eng.step(ticks=120)["hash"])
        self.assertEqual(hashes[0], hashes[1])

    def test_render_returns_png_bytes(self):
        self.eng.reset()
        self.eng.load_scene(path="examples/showcase.lunaris.json")
        frame = self.eng.render(width=64, height=36)
        self.assertTrue(frame["png"].startswith(b"\x89PNG\r\n\x1a\n"))
        self.assertGreater(len(frame["visible"]), 0)

    def test_unknown_tool_is_a_clear_attribute_error(self):
        with self.assertRaises(AttributeError):
            self.eng.not_a_tool()


class EmbeddedTests(unittest.TestCase):
    """The engine as a library inside this process (the C interface), checked against the engine in a child process."""

    def test_same_methods_same_errors_same_hashes(self):
        repo = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
        emb = Embedded(root=repo, seed=5)
        with Engine(root=repo) as probe:
            self.assertEqual(set(emb.tools), set(probe.tools))
        emb.spawn(count=50, components={"Transform": {}, "Velocity": {"lin": [1, 0, 0]}, "Wander": {"strength": 2}})
        h_emb = emb.step(ticks=120)["hash"]
        with Engine(root=repo, seed=5) as child:
            child.spawn(count=50, components={"Transform": {}, "Velocity": {"lin": [1, 0, 0]}, "Wander": {"strength": 2}})
            h_child = child.step(ticks=120)["hash"]
        self.assertEqual(h_emb, h_child, "the library and the child process simulate identically")
        with self.assertRaises(LunarisError) as cm:
            emb.spawn(components={"Transfrom": {}})
        self.assertEqual(cm.exception.code, "unknown_component")
        emb.reset()
        emb.load_scene(path="examples/showcase.lunaris.json")
        self.assertTrue(emb.render(width=64, height=36)["png"].startswith(b"\x89PNG"))
        emb.close()
        with self.assertRaises(RuntimeError):
            emb.describe()


if __name__ == "__main__":
    unittest.main(verbosity=2)
