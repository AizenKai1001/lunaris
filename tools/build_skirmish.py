"""Write examples/skirmish.lunaris.json: a small arena game made only of data and Lunaris Script, to show the gameplay blocks working together (and to find
what does not yet). WASD walks (`InputMap` actions), Space fires bolts (a `PrefabSpawner` on the hero's muzzle), grunts come out of three gates
(`PrefabSpawner`), hunt the hero over the arena (`NavAgent` + `NavAvoid`), bite when they touch it (contacts into `HitPoints` with grace time), die to bolts
(`died` ends them and scores), the camera follows (`CameraRig`), the sun sets (`TimeOfDay`), a health bar and a score are the HUD (`Ui`, `Text`), and when the hero falls
the world slows down (`time.scale`).

    python3 tools/build_skirmish.py      (it needs the `lunaris` binary: target/release/lunaris)
"""
import json
import subprocess
import sys

BIN = "target/release/lunaris"
reqs = []


def call(name, args):
    reqs.append({"name": name, "arguments": args})


def spawn(uid, name, **components):
    call("spawn", {"uid": uid, "components": {"Name": {"value": name}, **components}})


call("define_component", {"name": "Hero", "fields": []})
call("define_component", {"name": "Enemy", "fields": []})
call("define_component", {"name": "Bolt", "fields": []})
call("define_component", {"name": "Muzzle", "fields": []})
call("define_component", {"name": "HeroBar", "fields": []})
call("define_component", {"name": "GameOver", "fields": []})
call("define_component", {"name": "Gate", "fields": []})

# prefabs
call("define_prefab", {"name": "Bolt", "components": {
    "Name": {"value": "Bolt"}, "Transform": {"scale": [0.3, 0.3, 0.3]}, "Velocity": {}, "Bolt": {}, "Lifetime": {"seconds": 1.1},
    "Collider": {"shape": 0, "size": [1, 1, 1], "trigger": True},
    "MeshRenderer": {"mesh": "sphere", "color": [1.0, 0.85, 0.3, 1], "emissive": 3.0}}})
call("define_prefab", {"name": "Grunt", "components": {
    "Name": {"value": "Grunt"}, "Transform": {}, "Enemy": {}, "CharacterBody": {"radius": 0.4},
    "NavAgent": {"speed": 3.2, "radius": 0.4, "arrive": 0.2}, "NavAvoid": {"strength": 1.2}, "HitPoints": {"value": 30, "max": 30},
    "Collider": {"shape": 0, "size": [1.8, 1.8, 1.8], "trigger": True}},
    "children": [{"name": "Body", "components": {"LocalTransform": {"pos": [0, 0.3, 0], "scale": [0.45, 0.45, 0.45]}, "MeshRenderer": {"mesh": "sphere", "color": [0.85, 0.2, 0.18, 1], "roughness": 0.5}}},
                 {"name": "Eye", "components": {"LocalTransform": {"pos": [0, 0.36, -0.2], "scale": [0.16, 0.16, 0.16]}, "MeshRenderer": {"mesh": "sphere", "color": [1, 0.95, 0.7, 1], "emissive": 2.0}}}]})

# the world
spawn(1, "Environment", Environment={"sky": True, "clouds": 0.45, "fog": 0.004, "bloom": 0.35, "ssao": 1.0})
spawn(2, "Sun", Transform={"rot": [-0.5, 0.3, 0.1, 0.8]}, Light={"kind": 0, "intensity": 3.0}, TimeOfDay={"time": 17.2, "day_seconds": 900, "azimuth": 0.9})
spawn(3, "Ground", Transform={"pos": [0, -0.5, 0], "scale": [44, 1, 44]}, MeshRenderer={"mesh": "cube", "texture": "stone", "uv_scale": [20, 20], "color": [0.55, 0.58, 0.6, 1], "roughness": 0.9}, Collider={"shape": 1, "size": [1, 1, 1]})
for i, (pos, scale) in enumerate([([0, 1, -22.5], [45, 2, 1]), ([0, 1, 22.5], [45, 2, 1]), ([-22.5, 1, 0], [1, 2, 45]), ([22.5, 1, 0], [1, 2, 45])]):
    spawn(4 + i, f"Wall {i + 1}", Transform={"pos": pos, "scale": scale}, MeshRenderer={"mesh": "cube", "texture": "bricks", "uv_scale": [12, 1], "color": [0.8, 0.7, 0.65, 1]}, Collider={"shape": 1, "size": [1, 1, 1]})
for i, (x, z) in enumerate([(-9, -8), (10, -5), (-4, 9), (8, 10)]):
    spawn(8 + i, f"Pillar {i + 1}", Transform={"pos": [x, 1.5, z], "scale": [1.8, 3, 1.8]}, MeshRenderer={"mesh": "cylinder", "texture": "stone", "color": [0.7, 0.72, 0.78, 1]}, Collider={"shape": 1, "size": [1, 1, 1]})
for i, (x, z) in enumerate([(-18, -18), (18, -18), (0, 18)]):
    spawn(12 + i, f"Gate {i + 1}", Transform={"pos": [x, 0, z]}, Gate={},
          PrefabSpawner={"prefab": "Grunt", "interval": 2.4, "max_alive": 4, "radius": 1.2, "offset": [0, 0, 0]},
          MeshRenderer={"mesh": "cylinder", "color": [0.35, 0.1, 0.45, 1], "emissive": 0.8})

# the hero, its muzzle and the camera
spawn(20, "Hero", Transform={"pos": [0, 0, 0]}, Hero={}, CharacterBody={"radius": 0.4, "height": 1.8}, HitPoints={"value": 100, "max": 100, "regen": 4, "regen_delay": 3},
      Collider={"shape": 0, "size": [1.8, 1.8, 1.8], "trigger": True})
spawn(21, "Hero body", Parent={"target": 20}, LocalTransform={"pos": [0, 0.9, 0], "scale": [0.5, 0.9, 0.5]}, Transform={"pos": [0, 0.9, 0]}, MeshRenderer={"mesh": "cylinder", "color": [0.2, 0.55, 0.95, 1], "roughness": 0.4})
spawn(22, "Hero head", Parent={"target": 20}, LocalTransform={"pos": [0, 1.9, 0], "scale": [0.55, 0.55, 0.55]}, Transform={"pos": [0, 1.9, 0]}, MeshRenderer={"mesh": "sphere", "color": [1, 0.85, 0.7, 1], "roughness": 0.45})
spawn(23, "Muzzle", Muzzle={}, Parent={"target": 20}, LocalTransform={"pos": [0, 1.0, 0]}, Transform={"pos": [0, 1.0, 0]},
      PrefabSpawner={"prefab": "Bolt", "interval": 0.2, "speed": 18, "active": False, "offset": [0, 0, 0]})
spawn(24, "Camera", Transform={"pos": [0, 11, 10]}, Camera={"fov_y": 50, "clear": [0.5, 0.65, 0.85, 1]}, CameraRig={"target": 20, "follow_speed": 4, "offset": [0, 11, 9], "look_offset": [0, 0.5, 0]})
spawn(25, "Input", InputMap={"bindings": "move_x = d, a:-1\nmove_y = s, w:-1\nfire = space, mouse_left"})

# the interface
spawn(30, "Health back", Ui={"pos": [0.03, 0.04], "size": [0.3, 0.04], "color": [0.1, 0.1, 0.1, 0.6], "border": 0.002})
spawn(31, "Health bar", HeroBar={}, Ui={"pos": [0.034, 0.047], "size": [0.292, 0.026], "color": [0.85, 0.2, 0.2, 1], "back_color": [0.2, 0.05, 0.05, 0.8], "fill": 1.0, "layer": 1})
spawn(32, "Score", Text={"value": "Score {score}   Health {hero.hp}", "pos": [0.5, 0.04], "size": 0.045, "align": 1, "color": [1, 1, 1, 1], "shadow": True})
spawn(33, "Game over", GameOver={}, Ui={"pos": [0.25, 0.4], "size": [0.5, 0.18], "color": [0.0, 0.0, 0.0, 0.7], "text": "GAME OVER   score {score}", "text_size": 0.05, "layer": 5, "visible": False})
spawn(34, "Hint", Text={"value": "WASD walk   Space fire", "pos": [0.5, 0.94], "size": 0.03, "align": 1, "color": [1, 1, 1, 0.8], "shadow": True})

call("set_resource", {"name": "aim.z", "value": -1})
call("set_resource", {"name": "aim.x", "value": 0})

SCRIPT = """
/// The keys become the hero's walk, and the last way it walked is the way it aims.
system hero_walk each Hero, CharacterBody order 10 {
    CharacterBody.walk[0] = $action.move_x * 6;
    CharacterBody.walk[1] = $action.move_y * 6;
    if $action.move_x != 0 || $action.move_y != 0 {
        $aim.x = $action.move_x;
        $aim.z = $action.move_y;
    }
}

/// What the grunts need to know about the hero, and the hero's health bar.
system hero_status each Hero, Transform, HitPoints order 11 {
    $hero.x = Transform.pos[0];
    $hero.z = Transform.pos[2];
    $hero.hp = floor(HitPoints.value);
    if HitPoints.dead {
        $time.scale = 0.25;
    }
}

/// The muzzle turns the way the hero aims and fires while the key is held.
system muzzle each Muzzle, LocalTransform, PrefabSpawner order 12 {
    let yaw = atan2(-$aim.x, -$aim.z);
    LocalTransform.rot[1] = sin(yaw * 0.5);
    LocalTransform.rot[3] = cos(yaw * 0.5);
    PrefabSpawner.active = $action.fire > 0.5 && $hero.hp > 0;
}

/// Grunts head for the hero: every 20 ticks each asks for a new path (staggered by uid so they do not all plan at once).
system hunt each Enemy, NavAgent order 20 {
    if tick % 20 == uid % 20 {
        NavAgent.target[0] = $hero.x;
        NavAgent.target[1] = 0;
        NavAgent.target[2] = $hero.z;
        NavAgent.active = true;
    }
}

/// A grunt that touches the hero bites, once in a while (the hero has a moment of grace after each bite).
system grunt_bites on_contact a(Hero, HitPoints) b(Enemy) order 30 {
    if a.HitPoints.invulnerable <= 0 && !a.HitPoints.dead {
        a.HitPoints.value -= 12;
        a.HitPoints.invulnerable = 0.7;
        emit "hurt";
    }
}

/// A bolt that reaches a grunt hurts it and is used up.
system bolt_hits on_contact a(Bolt) b(Enemy, HitPoints) order 31 {
    b.HitPoints.value -= 15;
    emit "hit";
    despawn a;
}

/// A dead grunt scores and goes.
system grunt_dies each Enemy, HitPoints order 40 {
    if HitPoints.dead {
        $score += 1;
        emit "pop";
        despawn;
    }
}

system hud_bar each HeroBar, Ui order 50 {
    Ui.fill = $hero.hp / 100;
}

system hud_game_over each GameOver, Ui order 50 {
    Ui.visible = $hero.hp <= 0;
}
"""
call("define_script", {"source": SCRIPT})
call("set_resource", {"name": "score", "value": 0})
call("set_resource", {"name": "hero.hp", "value": 100})
call("check", {})
call("save_scene", {"path": "examples/skirmish.lunaris.json"})

lines = [json.dumps({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": r}) for i, r in enumerate(reqs)]
p = subprocess.run([BIN, "serve", "--root", ".", "--no-gpu"], input="\n".join(lines) + "\n", capture_output=True, text=True, timeout=240)
bad = 0
for line in p.stdout.splitlines():
    try:
        m = json.loads(line)
    except ValueError:
        continue
    res = m.get("result") or {}
    if "error" in m or res.get("isError"):
        bad += 1
        print("request", m.get("id"), reqs[m["id"] - 1]["name"], "failed:", json.dumps(m.get("error") or res)[:400])
    elif reqs[m["id"] - 1]["name"] == "check":
        for c in res.get("content") or []:
            if c.get("type") == "text":
                print("check:", c["text"][:600])
print(f"wrote examples/skirmish.lunaris.json ({len(reqs)} requests, {bad} failed)")
sys.exit(1 if bad else 0)
