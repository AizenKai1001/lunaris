"""Write examples/dungeon.lunaris.json: a top-down dungeon crawl whose level is MADE by the engine's level tools instead of drawn by hand:

  generate_map      rooms and corridors from a seed (the same dungeon on every machine), with a start, a far exit and spread-out spawn points
  autotile          the walls become the right edge and corner tiles of examples/dungeon_tiles.png
  tilemap_colliders the walls become a few merged static boxes the hero and the guards cannot pass

then the hero is put at `start_world`, guards and coins at `spawns_world` and the exit at `exit_world`. WASD or a game pad walks; guards that see the hero (a `Sensor`
with line of sight through the walls' boxes) chase it for a few seconds after they lose it; touching one hurts (`HitPoints`); coins score; the exit wins.

    python -I tools/make_dungeon_tiles.py                     (once: the tileset)
    python3 tools/build_dungeon.py      (needs target/release/lunaris)
"""
import json
import subprocess

BIN = "target/release/lunaris"
SEED = 21

proc = subprocess.Popen([BIN, "serve", "--root", ".", "--no-gpu"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
failed = 0
n = 0


def call(name, args):
    """One tool call; returns the tool's own JSON answer."""
    global failed, n
    n += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": "tools/call", "params": {"name": name, "arguments": args}}) + "\n")
    proc.stdin.flush()
    while True:
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"the engine stopped during {name}")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("id") == n:
            break
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        failed += 1
        print("request", n, name, "failed:", json.dumps(m.get("error") or res)[:500])
        return {}
    for c in res.get("content") or []:
        if c.get("type") == "text":
            try:
                return json.loads(c["text"])
            except ValueError:
                return {"text": c["text"]}
    return {}


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


for comp in ("Hero", "Enemy", "Coin", "Exit", "Wall", "WinBanner", "LoseBanner"):
    fields = [{"name": "alert", "type": "f32", "default": 0}] if comp == "Enemy" else []
    call("define_component", {"name": comp, "fields": fields})

# ---- the level: made, then dressed, then made solid
W, H = 48, 32
spawn(10, "Dungeon", Transform={"pos": [0, 0, -0.05]}, Tilemap={"texture": "examples/dungeon_tiles.png", "sheet": [17, 1], "size": [W, H], "tile": 1.0})
level = call("generate_map", {"uid": 10, "mode": "rooms", "seed": SEED, "width": W, "height": H, "floor": 0, "wall": 1, "points": 11, "clear": 8})
call("autotile", {"uid": 10, "base": 1, "mode": "4", "solid": [1], "edge": "solid"})
walls = call("tilemap_colliders", {"uid": 10, "solid": [[1, 16]], "components": {"Wall": {}}})
print("dungeon:", level.get("floor_cells"), "open cells,", len(level.get("rooms", [])), "rooms,", walls.get("boxes"), "wall boxes for", walls.get("solid_cells"), "wall cells; exit", level.get("distance"), "steps from the start")
start, exit_at, spots = level["start_world"], level["exit_world"], level["spawns_world"]

# ---- the world (the wall boxes took the uids after the map's, so everything else has a high one)
spawn(2, "Physics", PhysicsSettings={"gravity": [0, 0, 0]})
spawn(3, "Camera", Transform={"pos": [start[0], start[1], 10]}, Camera={"ortho": True, "ortho_height": 13, "clear": [0.03, 0.025, 0.05, 1]},
      CameraRig={"target": 1000, "follow_speed": 5, "offset": [0, 0, 10], "look_offset": [0, 0, 0]})
spawn(4, "Input", InputMap={"bindings": "move_x = d, a:-1, gamepad.lx, arrowright, arrowleft:-1\nmove_y = w, s:-1, gamepad.ly, arrowup, arrowdown:-1\ndeadzone = 0.15"})

spawn(1000, "Hero", Transform={"pos": [start[0], start[1], 0]}, Velocity={}, Hero={}, HitPoints={"value": 100, "max": 100, "regen": 2, "regen_delay": 4},
      RigidBody={"mass": 1, "restitution": 0, "friction": 0, "gravity_scale": 0}, Collider={"shape": 0, "size": [0.7, 0.7, 0.7]},
      Sprite={"size": [0.8, 0.8], "texture": "circle", "color": [0.35, 0.65, 1.0, 1]})
spawn(1001, "Hero eye", Parent={"target": 1000}, LocalTransform={"pos": [0.12, 0.1, 0.01]}, Transform={"pos": [start[0] + 0.12, start[1] + 0.1, 0.01]},
      Sprite={"size": [0.22, 0.22], "texture": "circle", "color": [1, 1, 1, 1]})

spawn(1010, "Exit", Exit={}, Transform={"pos": [exit_at[0], exit_at[1], 0]}, Collider={"shape": 0, "size": [1.2, 1.2, 1.2], "trigger": True},
      Sprite={"size": [1.4, 1.4], "texture": "ring", "color": [1.0, 0.85, 0.3, 1]})
spawn(1011, "Exit glow", Transform={"pos": [exit_at[0], exit_at[1], -0.01]}, Sprite={"size": [2.4, 2.4], "texture": "soft", "color": [1.0, 0.8, 0.3, 0.35]})

for i, (x, y) in enumerate(spots[:6]):
    spawn(1100 + i, f"Guard {i + 1}", Transform={"pos": [x, y, 0]}, Velocity={}, Enemy={}, HitPoints={"value": 30, "max": 30},
          Sensor={"radius": 8, "fov": 360, "filter": "Hero", "los": True, "eye": 0},
          RigidBody={"mass": 1, "restitution": 0, "friction": 0, "gravity_scale": 0}, Collider={"shape": 0, "size": [0.7, 0.7, 0.7]},
          Sprite={"size": [0.8, 0.8], "texture": "circle", "color": [0.9, 0.22, 0.2, 1]})
for i, (x, y) in enumerate(spots[6:]):
    spawn(1200 + i, f"Coin {i + 1}", Coin={}, Transform={"pos": [x, y, 0]}, Collider={"shape": 0, "size": [0.6, 0.6, 0.6], "trigger": True},
          Sprite={"size": [0.45, 0.45], "texture": "circle", "color": [1.0, 0.85, 0.2, 1]})

# ---- the interface
spawn(1020, "Score", Text={"value": "Coins {score}   Health {hero.hp}", "pos": [0.5, 0.03], "size": 0.045, "align": 1, "color": [1, 1, 1, 1], "shadow": True})
spawn(1021, "Hint", Text={"value": "WASD or a pad: walk   find the golden ring", "pos": [0.5, 0.94], "size": 0.03, "align": 1, "color": [1, 1, 1, 0.75], "shadow": True})
spawn(1022, "Lost", LoseBanner={}, Ui={"pos": [0.25, 0.4], "size": [0.5, 0.18], "color": [0, 0, 0, 0.7], "text": "YOU FELL   coins {score}", "text_size": 0.05, "layer": 5, "visible": False})
spawn(1023, "Won", WinBanner={}, Ui={"pos": [0.25, 0.4], "size": [0.5, 0.18], "color": [0, 0, 0, 0.7], "text": "YOU ESCAPED   coins {score}", "text_size": 0.05, "layer": 5, "visible": False})

SCRIPT = """
/// The keys (or the pad) become the hero's walk; it stands still when it has fallen or won.
system hero_walk each Hero, Velocity, HitPoints order 10 {
    if HitPoints.dead || $won != 0 {
        Velocity.lin[0] = 0;
        Velocity.lin[1] = 0;
    } else {
        Velocity.lin[0] = $action.move_x * 5;
        Velocity.lin[1] = $action.move_y * 5;
    }
}

/// What the guards need to know about the hero, and the health in the HUD.
system hero_status each Hero, Transform, HitPoints order 11 {
    $hero.x = Transform.pos[0];
    $hero.y = Transform.pos[1];
    $hero.hp = floor(HitPoints.value);
    if HitPoints.dead {
        $time.scale = 0.25;
    }
}

/// A guard that sees the hero is alerted for a few seconds; an alerted guard runs at the hero, otherwise it stands guard.
system guard_ai each Enemy, Sensor, Transform, Velocity order 20 {
    if Sensor.seen != 0 {
        Enemy.alert = 3;
    }
    if Enemy.alert > 0 {
        let dx = $hero.x - Transform.pos[0];
        let dy = $hero.y - Transform.pos[1];
        let d = max(hypot(dx, dy), 0.001);
        Velocity.lin[0] = dx / d * 3;
        Velocity.lin[1] = dy / d * 3;
        Enemy.alert -= dt;
    } else {
        Velocity.lin[0] = 0;
        Velocity.lin[1] = 0;
    }
}

/// A guard that touches the hero hurts it, once in a while (a moment of grace after each hit, and a shove away from the guard).
system guard_bites on_contact a(Hero, HitPoints) b(Enemy) order 30 {
    if a.HitPoints.invulnerable <= 0 && !a.HitPoints.dead {
        a.HitPoints.value -= 15;
        a.HitPoints.invulnerable = 0.8;
        emit "hurt";
    }
}

system coin_pickup on_contact a(Hero) b(Coin) order 31 {
    $score += 1;
    emit "pickup";
    despawn b;
}

system reach_exit on_contact a(Hero) b(Exit) order 32 {
    $won = 1;
}

system win_banner each WinBanner, Ui order 50 {
    Ui.visible = $won != 0;
}

system lose_banner each LoseBanner, Ui order 50 {
    Ui.visible = $hero.hp <= 0;
}
"""
call("define_script", {"source": SCRIPT})
for name, value in (("score", 0), ("won", 0), ("hero.hp", 100), ("hero.x", start[0]), ("hero.y", start[1])):
    call("set_resource", {"name": name, "value": value})
check = call("check", {})
print("check:", json.dumps(check)[:700])
call("save_scene", {"path": "examples/dungeon.lunaris.json"})
proc.stdin.close()
proc.wait(timeout=30)
print(f"wrote examples/dungeon.lunaris.json ({n} requests, {failed} failed)")
