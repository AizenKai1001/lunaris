"""The weather of the city: a game begins clear, F6 steps through cloudy, rain and fog, the rain falls where the sky is open and not under a roof, a storm throws lightning (run on the machine with the workspace):

    python3 tools/citygen/weather_test.py ROOT SCENE
"""
import json
import subprocess
import sys

BIN = "target/release/lunaris"


def main():
    root, scene = sys.argv[1], sys.argv[2]
    proc = subprocess.Popen([BIN, "serve", "--root", root], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    n = [0]

    def call(name, args=None):
        n[0] += 1
        proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n[0], "method": "tools/call", "params": {"name": name, "arguments": args or {}}}) + chr(10))
        proc.stdin.flush()
        while True:
            m = json.loads(proc.stdout.readline())
            if m.get("id") == n[0]:
                break
        res = m.get("result") or {}
        if res.get("isError"):
            print(name, "FAILED", json.dumps(m)[:400])
        for c in res.get("content") or []:
            if c.get("type") == "text":
                try:
                    return json.loads(c["text"])
                except ValueError:
                    return {"text": c["text"]}
        return {}

    def rows(component):
        q = call("query", {"with": [component], "select": []})
        r = q.get("entities") or q.get("results") or q.get("matches") or []
        return [x.get("uid") if isinstance(x, dict) else x for x in r]

    def comp(uid, name):
        r = call("get", {"uid": uid, "component": name})
        return (r.get("components") or {}).get(name) or {}

    def res(name):
        return call("resources", {}).get("resources", {}).get(name)

    def check(label, ok, detail=""):
        print(("ok   " if ok else "FAIL ") + label + (f"  {detail}" if detail else ""), flush=True)

    def key(k):
        call("input", {"values": {k: True}})
        call("step", {"ticks": 2})
        call("input", {"values": {k: False}})
        call("step", {"ticks": 2})

    call("load_scene", {"path": scene})

    call("set_resource", {"name": "continue_enabled", "value": 0})
    hero = rows("Hero")[0]
    env, tod, rain, wx = rows("Environment")[0], rows("TimeOfDay")[0], rows("RainCloud")[0], rows("Weather")[0]
    call("step", {"ticks": 90})
    check("a game begins clear", res("wx_target") == 0 and res("rain") < 0.01, f"target {res('wx_target')}, rain {res('rain'):.3f}")
    clear_fog, clear_sun = comp(env, "Environment")["fog"], comp(tod, "TimeOfDay")["sun_intensity"]
    check("a clear day has no rain on the picture", comp(wx, "Weather").get("rain", 0) < 0.01, str(comp(wx, "Weather").get("rain")))
    # cloudy, then rain
    key("f6")
    call("step", {"ticks": 900})
    check("F6: cloudy, the sun dims", res("wx_target") == 1 and res("wx") > 0.5 and comp(tod, "TimeOfDay")["sun_intensity"] < clear_sun * 0.7, f"wx {res('wx'):.2f}, sun {comp(tod, 'TimeOfDay')['sun_intensity']:.2f} (clear {clear_sun:.2f})")
    key("f6")
    call("step", {"ticks": 900})
    e, w = comp(env, "Environment"), comp(wx, "Weather")
    check("F6 again: rain, the sky closes and the rain is on the picture", res("wx_target") == 2 and res("rain") > 0.7 and e["clouds"] > 0.85 and w.get("rain", 0) > 0.5, f"rain {res('rain'):.2f}, clouds {e['clouds']:.2f}, Weather.rain {w.get('rain')}")
    check("the ground is wet", w.get("wet", 0) > 0.5, f"Weather.wet {w.get('wet')}")
    check("the rain is loud and the haze thicker", e["fog"] > clear_fog, f"fog {e['fog']:.4f} (clear {clear_fog:.4f})")
    vols = {comp(u, "WeatherSound").get("kind"): comp(u, "AudioSource").get("volume") for u in rows("WeatherSound")}
    check("the rain and the wind sound", vols.get(1.0, 0) > 0.3 and vols.get(2.0, 0) > 0.1, str(vols))
    pos = comp(hero, "Transform")["pos"]
    cloud_pos = comp(rain, "Transform")["pos"]
    check("the roof finder follows the player's head", abs(cloud_pos[0] - pos[0]) < 3 and abs(cloud_pos[2] - pos[2]) < 3 and 8 < cloud_pos[1] - pos[1] < 20, f"cloud {[round(v, 1) for v in cloud_pos]} player {[round(v, 1) for v in pos]}")
    # a roof overhead: the drops go and the rain is muffled
    roof = call("spawn", {"name": "Test roof", "components": {"Transform": {"pos": [pos[0], pos[1] + 4.0, pos[2]]}, "Collider": {"shape": 1, "size": [8.0, 0.4, 8.0]}}})
    call("step", {"ticks": 90})
    vols = {comp(u, "WeatherSound").get("kind"): comp(u, "AudioSource").get("volume") for u in rows("WeatherSound")}
    w = comp(wx, "Weather")
    check("under a roof the rain is off the picture and muffled", res("covered") > 0.9 and w.get("rain", 0) < 0.1 and vols.get(1.0, 1) < 0.35, f"covered {res('covered'):.2f}, Weather.rain {w.get('rain', 0):.2f}, rain volume {vols.get(1.0)}")
    uid = roof.get("uid") or roof.get("created") or (roof.get("uids") or [None])[0]
    if uid:
        call("despawn", {"uid": uid})
    # lightning in the storm
    call("set_resource", {"name": "rain", "value": 1.0})
    peak, thunder = 0.0, False
    for _ in range(100):
        call("step", {"ticks": 60})
        peak = max(peak, res("flash") or 0.0)
        thunder = thunder or (res("thunder_t") or 0) > 0
    check("a storm throws lightning", peak > 0.3 or thunder, f"flash peaked {peak:.2f}, thunder scheduled {thunder}")
    # fog
    key("f6")
    call("step", {"ticks": 1500})
    e = comp(env, "Environment")
    check("F6 again: fog, thick haze and no rain", res("wx_target") == 3 and res("fogw") > 0.8 and e["fog"] > 0.003 and res("rain") < 0.1, f"fogw {res('fogw'):.2f}, fog {e['fog']:.4f}, rain {res('rain'):.2f}")
    key("f6")
    call("step", {"ticks": 1500})
    check("and back to clear", res("wx_target") == 0 and res("wx") < 0.3 and res("fogw") < 0.2, f"wx {res('wx'):.2f}, fogw {res('fogw'):.2f}")
    proc.stdin.close()
    proc.wait(timeout=60)


if __name__ == "__main__":
    main()
