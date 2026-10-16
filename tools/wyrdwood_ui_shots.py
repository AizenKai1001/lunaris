"""Pictures of the inventory and the hotbar of Wyrdwood with some things in them (runs where target/release/lunaris is): python3 tools/wyrdwood_ui_shots.py OUT_PREFIX"""
import json
import subprocess
import sys

PREFIX = sys.argv[1]
proc = subprocess.Popen(["target/release/lunaris", "serve", "--root", "."], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
n = 0


def call(name, args=None):
    global n
    n += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + "\n")
    proc.stdin.flush()
    while True:
        m = json.loads(proc.stdout.readline())
        if m.get("id") == n:
            break
    res = m.get("result") or {}
    if res.get("isError"):
        print(name, "failed", json.dumps(res)[:300])


call("load_scene", {"path": "examples/wyrdwood/wyrdwood.lunaris.json"})
call("step", {"ticks": 3})
items = [(16, 1, 96), (18, 1, 100), (1, 37, 0), (2, 12, 0), (3, 9, 0), (10, 5, 0), (13, 3, 0), (17, 1, 100), (9, 8, 0), (12, 4, 0), (4, 11, 0), (5, 6, 0), (11, 7, 0), (15, 1, 100), (21, 1, 120), (24, 1, 100)]
for i, (item, count, dur) in enumerate(items):
    call("set_resource", {"name": f"inv.{i}.id", "value": item})
    call("set_resource", {"name": f"inv.{i}.n", "value": count})
    call("set_resource", {"name": f"inv.{i}.dur", "value": dur})
call("set_resource", {"name": "hot", "value": 0})
call("step", {"ticks": 2})
call("render", {"quality": "high", "width": 1280, "height": 720, "eye": [-205.5, 45.6, 1181.5], "target": [-210, 44.8, 1176], "fov": 45, "path": f"{PREFIX}_axe.png", "image": False})
call("set_resource", {"name": "hot", "value": 7})
call("step", {"ticks": 2})
call("render", {"quality": "high", "width": 1280, "height": 720, "eye": [-205.5, 45.6, 1181.5], "target": [-210, 44.8, 1176], "fov": 45, "path": f"{PREFIX}_club.png", "image": False})
call("set_resource", {"name": "hot", "value": 0})
for k, (hp, st, secs) in enumerate([(7, 14, 500), (16, 11, 900)]):
    call("set_resource", {"name": f"food.{k}.t", "value": secs})
    call("set_resource", {"name": f"food.{k}.max", "value": 1200})
    call("set_resource", {"name": f"food.{k}.hp", "value": hp})
    call("set_resource", {"name": f"food.{k}.st", "value": st})
call("step", {"ticks": 3})
call("render", {"quality": "high", "width": 1280, "height": 720, "path": f"{PREFIX}_hotbar.png", "image": False})
call("set_resource", {"name": "invopen", "value": 1})
call("set_resource", {"name": "station.wb", "value": 1})
call("step", {"ticks": 4})
call("render", {"quality": "high", "width": 1280, "height": 720, "path": f"{PREFIX}_inventory.png", "image": False})
proc.stdin.close()
proc.wait(timeout=60)
