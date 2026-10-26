"""Live smoke test of examples/outpost.lunaris.json on the real binary (docs/GAME_SERVER.md, "Try it: the outpost"):
one game server and two hidden clients, each driven through its own `--api` port, UDP on this machine.

    python3 tools/outpost_smoke.py [path/to/lunaris [survival lighthouse hacking secure crawler]]      (from the repository root; takes about ten minutes; scenario names after the binary run only those)

It checks, and prints PASS or FAIL for: both players connect and see each other and the replicated world; the door opens (for the other player too); a crate fills the client's container inventory;
`inv_move` carries the stacks into the bag; using the crate again shuts it; `inv_drop` puts the thing in the hand on the ground, where the other player sees it; a shot lowers the other player's health; the guns are items (the
pistol of the starting bag is the weapon only while it is in the hand, the other player sees it there, the rounds leave the bag when a reload fills the magazine, Q takes the next gun of the bag: docs/LOADOUT.md); `build:foundation` and `place`
put a piece the other player sees; the drones kill a player standing in the open, a corpse is left and the player gets up elsewhere (a correction); the player sits in the car, drives it off (the other player sees it go), is not let out at speed, brakes to a stop and gets out; the player then sits in the helicopter, flies it up (the other
player sees it and the pilot climb), is not let out in the air, brings it down and gets out (the rotors' noise draws the drones to it: they wear it down while the test runs).
A second run on another copy of the scene (no drones, no helicopter) is about the lighthouse: its beam sweeps, a player under cover (the hut) and one out of its reach are left alone and a player who stands in
its beam is detected: the director sends hunters.
A fifth run (only the crawlers, two machines on the ground) is about walkers: they notice the player, walk round the obstacles to it and bite, and cannot reach a player in the hut behind its shut door.
A fourth run is the secure server (`--secure`, docs/SECURE.md): two players with connect tokens play, one without a token never gets in, and a token that was used is refused the second time.
A third run (one drone, no helicopter, no lighthouse) is about the goggles: the player aims at the drone and holds the hack key, takes it (the server says so), sees from it (the camera is at the drone), flies it with the
keys and gives it back (the camera goes home). The exit status is the number of failures.
It copies the scene to a temporary folder first (the client loads scenes only from under its working directory, and the server writes its save beside it).
"""
import json
import math
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXE = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.join(ROOT, "target", "release", "lunaris.exe" if os.name == "nt" else "lunaris")
SCENE = "outpost.lunaris.json"
PORT, A, B = 7851, 7871, 7872
fails = 0


def rpc(port, name, **args):
    s = socket.create_connection(("127.0.0.1", port), timeout=8)
    s.sendall((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}}) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        c = s.recv(65536)
        if not c:
            break
        buf += c
    s.close()
    r = json.loads(buf)
    if "error" in r:
        return {"error": r["error"]}
    try:
        return json.loads(r["result"]["content"][0]["text"])
    except (KeyError, ValueError, IndexError):
        return r["result"]


def get(port, uid, comp):
    return rpc(port, "get", uid=uid, component=comp).get("components", {}).get(comp)


def check(label, ok, detail=""):
    global fails
    print(("PASS  " if ok else "FAIL  ") + label + (f"   ({detail})" if detail else ""))
    if not ok:
        fails += 1


def puppets(api):
    return [e["components"] for e in rpc(api, "query", **{"with": ["NetPuppet"]})["entities"]]


def kinds(api):
    out = {}
    for c in puppets(api):
        k = (c["NetPuppet"]["class"], c["NetPuppet"]["kind"])
        out[k] = out.get(k, 0) + 1
    return out


def mate_health(api):
    return [c["NetPuppet"]["health"] for c in puppets(api) if c["NetPuppet"]["class"] == 0]


def bag(api, uid):
    inv = get(api, uid, "Inventory")
    return [(k, inv["id"][k], inv["count"][k]) for k in range(inv["size"]) if inv["id"][k]]


def tp(api, x, z, yaw=0.0):
    """Walk the character to a place in hops: the server allows about 9 m at once and 16 m/s after that."""
    for _ in range(14):
        t = get(api, 40, "Transform")
        px, pz = t["pos"][0], t["pos"][2]
        d = math.hypot(x - px, z - pz)
        k = min(1.0, 7.5 / d) if d > 1e-6 else 1.0
        t["pos"] = [px + (x - px) * k, 0.0, pz + (z - pz) * k]
        t["rot"] = [0, math.sin(yaw / 2), 0, math.cos(yaw / 2)]
        rpc(api, "set", uid=40, component="Transform", value=t)
        if k >= 1.0:
            break
        time.sleep(0.5)


def pitch(api, v):
    m = get(api, 41, "MouseLook")
    m["pitch"] = v
    rpc(api, "set", uid=41, component="MouseLook", value=m)


def press(api, key, hold=0.35):
    rpc(api, "input", values={key: True})
    time.sleep(hold)
    rpc(api, "input", values={key: False})
    time.sleep(0.4)


def hold(api, slot):
    """Put the slot of the bag in the hand (what the number the player says is, `NetPlayer.extra`: docs/LOADOUT.md)."""
    np_ = get(api, 40, "NetPlayer")
    np_["extra"] = slot
    rpc(api, "set", uid=40, component="NetPlayer", value=np_)
    time.sleep(0.4)


def hand_seen(api):
    """What the client `api` sees in the hand of the other player: `NetPuppet.extra`, the gun's item above the slot (item << 6 | slot, docs/LOADOUT.md)."""
    return [c["NetPuppet"]["extra"] for c in puppets(api) if c["NetPuppet"]["class"] == 0]


def command(api, act, args):
    np_ = get(api, 40, "NetPlayer")
    np_["cmd_args"] = args
    np_["act"] = act
    rpc(api, "set", uid=40, component="NetPlayer", value=np_)
    time.sleep(0.6)


def lighthouse():
    """The lighthouse of the middle of the yard (docs/AI.md): the beam sweeps, cover and distance keep a player out of it, standing in it is being detected and the director sends hunters."""
    def turning():
        return [c["Transform"]["rot"] for c in puppets(B) if c["NetPuppet"]["class"] == 4 and c["NetPuppet"]["kind"] == 70]

    r0 = turning()
    time.sleep(3.0)
    r1 = turning()
    check("the lighthouse's beam sweeps (its puppet turns)", len(r0) == 1 and len(r1) == 1 and max(abs(a - b) for a, b in zip(r0[0], r1[0])) > 0.05, f"{r0} -> {r1}")
    hunters = lambda: kinds(B).get((4, 40), 0)
    # Bo under the hut's roof (the beam would meet it 14 m from the tower, 16 m is the middle of the ring it sweeps), Ada near the walls, out of the beam's reach: a whole turn of the beam and then some
    tp(B, 0.0, -14.0)
    tp(A, 30.0, 0.0)
    time.sleep(0.5)
    t0 = time.time()
    while time.time() - t0 < 26 and hunters() == 0:
        time.sleep(0.5)
    check("a player under cover and one out of reach are left alone through a turn of the beam", hunters() == 0, f"{hunters()} hunters")
    # Ada stands where the beam passes
    tp(A, 16.0, 0.0)
    t0 = time.time()
    while time.time() - t0 < 45 and hunters() == 0:
        time.sleep(0.5)
    check("a player who stands in the beam is detected and the director sends hunters", hunters() >= 1, f"{hunters()} hunters after {time.time() - t0:.0f} s")


def look_at(api, target):
    """Turn the player (its mouse look) so that its eyes look at `target`."""
    me = get(api, 40, "Transform")["pos"]
    d = [target[0] - me[0], target[1] - (me[1] + 1.62), target[2] - me[2]]
    m = get(api, 41, "MouseLook")
    m["yaw"] = math.atan2(-d[0], -d[2])
    m["pitch"] = math.atan2(d[1], math.hypot(d[0], d[2]))
    rpc(api, "set", uid=41, component="MouseLook", value=m)
    return math.sqrt(sum(v * v for v in d))


def hacking():
    """The goggles (docs/AI.md): aim at a drone and hold the hack key, take it, see from it, fly it, give it back."""
    def drone():
        for c in puppets(A):
            if c["NetPuppet"]["class"] == 4 and c["NetPuppet"]["kind"] == 40:
                return c["Transform"]["pos"]
        return None

    tp(B, 0.0, -14.0)  # in the hut
    tp(A, 0.0, 0.0)
    viewing = 0
    t0 = time.time()
    while time.time() - t0 < 30 and viewing == 0:
        p = drone()
        if p is not None and look_at(A, p) < 22:
            rpc(A, "input", values={"h": True})
        else:
            rpc(A, "input", values={"h": False})
        time.sleep(0.1)
        viewing = get(A, 40, "NetPlayer")["viewing"]
    rpc(A, "input", values={"h": False})
    check("aiming at a drone and holding the hack key takes it (the server says so: NetPlayer.viewing)", viewing != 0, f"viewing {viewing} after {time.time() - t0:.1f} s")
    if not viewing:
        return
    time.sleep(0.5)
    cam = get(A, 41, "Transform")["pos"]
    p = drone()
    check("the camera is at the drone", p is not None and math.dist(cam, p) < 3.0, f"camera {[round(v, 1) for v in cam]}, drone {[round(v, 1) for v in p] if p else None}")
    before = drone()
    rpc(A, "input", values={"w": True})
    time.sleep(1.2)
    rpc(A, "input", values={"w": False})
    time.sleep(0.5)
    after = drone()
    moved = math.dist(before, after) if before and after else 0.0
    check("the keys fly it", moved > 3.0, f"the drone went {moved:.1f} m")
    cam = get(A, 41, "Transform")["pos"]
    check("and the camera goes with it", after is not None and math.dist(cam, after) < 3.0, f"camera {[round(v, 1) for v in cam]}, drone {[round(v, 1) for v in after] if after else None}")
    press(A, "r", 0.3)
    time.sleep(0.8)
    np_ = get(A, 40, "NetPlayer")
    me = get(A, 40, "Transform")["pos"]
    cam = get(A, 41, "Transform")["pos"]
    check("R gives it back: the camera is home at the player's eyes", np_["viewing"] == 0 and math.dist(cam, [me[0], me[1] + 1.62, me[2]]) < 0.5, f"viewing {np_['viewing']}, camera {[round(v, 1) for v in cam]}, player {[round(v, 1) for v in me]}")


def crawlers():
    """Walkers (docs/AI.md): a player within sight is noticed and walked up to round the obstacles and bitten; a player in the hut behind its shut door is not reached."""
    def crawler_places():
        return [c["Transform"]["pos"] for c in puppets(A) if c["NetPuppet"]["class"] == 4 and c["NetPuppet"]["kind"] == 42]

    check("both crawlers are shown", len(crawler_places()) == 2, str(crawler_places()))
    tp(B, 0.0, -14.0)  # in the hut, the door shut
    tp(A, -8.0, 6.0)   # eight metres from the first crawler
    t0, near, bitten, low, bo_low = time.time(), 99.0, False, 255, 255
    while time.time() - t0 < 25 and not bitten:
        me = get(A, 40, "Transform")["pos"]
        near = min([math.hypot(p[0] - me[0], p[2] - me[2]) for p in crawler_places()] + [near])
        h = mate_health(B)
        if h:
            low = min(low, h[0])
        hb = mate_health(A)
        if hb:
            bo_low = min(bo_low, hb[0])
        bitten = low < 255
        time.sleep(0.2)
    check("a crawler notices the player and walks up to it", near < 3.0, f"the nearest it came was {near:.1f} m")
    check("and bites", bitten, f"lowest health seen {low}")
    # the other player, under the roof with the door shut, is never reached (the crawlers walk round the hut and cannot get in): watched for a while longer
    t0 = time.time()
    while time.time() - t0 < 12:
        hb = mate_health(A)
        if hb:
            bo_low = min(bo_low, hb[0])
        time.sleep(0.3)
    check("the player behind the shut door is not hurt", bo_low == 255, f"lowest health seen {bo_low}")


def secure_play(start, play_args, tokens):
    """The secure server (docs/SECURE.md): two players with tokens are in and playing; a player without a token and one with a token that was already used never get in."""
    def server_log():
        return open(os.path.join(WORK, "server.log"), encoding="utf-8", errors="replace").read()

    check("the server says it wants connect tokens", "connect token" in server_log())
    # they play: Ada opens the door, Bo sees it open (everything between them is sealed)
    tp(A, 0.0, -8.6)
    pitch(A, 0.0)
    time.sleep(0.8)
    door = lambda api: [c["Door"]["open"] for c in puppets(api) if "Door" in c]
    check("the door starts shut", door(A) == [False] and door(B) == [False])
    press(A, "e")
    time.sleep(1.2)
    check("a player with a token uses the door and the other sees it open", door(A) == [True] and door(B) == [True], f"{door(A)} {door(B)}")
    # a player with no token: never gets in
    extra = [start(play_args("Eve", 7873), "Eve")]
    extra.append(start(play_args("Ada", 7874), "AdaAgain"))  # (Ada's token is spent: it is in use by the first Ada)
    time.sleep(8)
    try:
        eve = get(7873, 40, "NetPlayer")
        ada2 = get(7874, 40, "NetPlayer")
    finally:
        for p in extra:
            p.terminate()
    check("a player without a token never gets in", eve is not None and not eve["connected"], str(eve and eve["connected"]))
    check("a token that is in use is refused the second time", ada2 is not None and not ada2["connected"], str(ada2 and ada2["connected"]))
    check("and the two players who were in are still playing", get(A, 40, "NetPlayer")["connected"] and get(B, 40, "NetPlayer")["connected"])


def helicopter(LAND=float(os.environ.get("HELI_LAND", "1.8"))):  # (the height it is brought down to before the pilot gets out)
    """The helicopter (docs/SEATS.md): sit in the pilot's seat, fly it up, no exit in the air, land it, get out."""
    # the helicopter: Ada walks up to it (it stands at 22, 14), sits in the pilot's seat, flies it up, cannot get out in the air, lands it and gets out
    def heli_y():
        ys = [c["Transform"]["pos"][1] for c in puppets(B) if c["NetPuppet"]["class"] == 4 and c["NetPuppet"]["kind"] == 60]
        return ys[0] if ys else None

    tp(A, 22.0, 18.6)
    pitch(A, 0.0)
    time.sleep(0.8)
    press(A, "e")
    time.sleep(0.8)
    np_ = get(A, 40, "NetPlayer")
    check("using the helicopter sits the player in it (the server says so: NetPlayer.riding)", np_["riding"] != 0, f"riding {np_['riding']}")
    time.sleep(4.5)  # the rotors spool up
    rpc(A, "input", values={"space": True})
    time.sleep(0.9)
    rpc(A, "input", values={"space": False})
    time.sleep(1.5)
    y, own = heli_y(), get(A, 40, "Transform")["pos"][1]
    check("the keys fly it: it climbs under the player's hands and the other player sees it", y is not None and y > 3.0, f"helicopter height {y}")
    mates = [c["Transform"]["pos"][1] for c in puppets(B) if c["NetPuppet"]["class"] == 0]
    check("the pilot rides with it, on both screens", y is not None and abs(own - y) < 3.0 and bool(mates) and abs(mates[0] - y) < 3.0, f"pilot {own:.1f}, seen by the other {mates}, helicopter {y}")
    press(A, "e")
    time.sleep(0.5)
    check("in the air the use button does not get the pilot out", get(A, 40, "NetPlayer")["riding"] != 0)
    for _ in range(120):
        if heli_y() is not None and heli_y() < LAND:
            break
        rpc(A, "input", values={"control": True})
        time.sleep(0.12)
        rpc(A, "input", values={"control": False})
        time.sleep(0.35)
    # (it bounces when it touches down: wait until it lies still near the ground)
    t0, last = time.time(), None
    while time.time() - t0 < 8.0:
        y = heli_y()
        if y is not None and y < 2.2 and last is not None and abs(y - last) < 0.05:
            break
        last = y
        time.sleep(0.4)
    check("it comes down on the keys", heli_y() is not None and heli_y() < 3.0, f"helicopter height {heli_y()}")
    press(A, "e")
    # (the pilot steps out at the exit beside the helicopter, up to a metre or two above the ground, and falls the rest on its own legs)
    t0, trace = time.time(), []
    while time.time() - t0 < 4.0 and (get(A, 40, "NetPlayer")["riding"] != 0 or get(A, 40, "Transform")["pos"][1] >= 2.0):
        trace.append((round(get(A, 40, "Transform")["pos"][1], 1), round(heli_y() or -1, 1)))
        time.sleep(0.2)
    np_ = get(A, 40, "NetPlayer")
    check("on the ground the pilot gets out", np_["riding"] == 0 and get(A, 40, "Transform")["pos"][1] < 2.0, f"riding {np_['riding']}, height {get(A, 40, 'Transform')['pos'][1]:.1f}, (pilot, helicopter) heights {trace}")

WORK = ""


def main(scenario):
    global PORT
    global WORK
    work = tempfile.mkdtemp(prefix="outpost_smoke_")
    WORK = work
    PORT = {"survival": 7851, "lighthouse": 7853, "hacking": 7855, "secure": 7857, "crawler": 7859, "heli": 7861}[scenario]
    sc = json.load(open(os.path.join(ROOT, "examples", SCENE), encoding="utf-8"))
    if scenario in ("survival", "hacking", "secure", "crawler", "heli"):
        # (the lighthouse is its own test: it would send hunters at the players of the others)
        for e in sc["entities"]:
            if e["uid"] == 231:
                e["components"]["Searchlight"]["active"] = False
    crawlers_uids = (240, 241, 250, 251)
    if scenario in ("lighthouse", "secure"):
        sc["entities"] = [e for e in sc["entities"] if e["uid"] not in (200, 201, 220) + crawlers_uids]  # no drones, no helicopter, no crawlers
    if scenario == "crawler":
        sc["entities"] = [e for e in sc["entities"] if e["uid"] not in (200, 201, 220, 221)]  # no drones, no helicopter, no car
    if scenario == "heli":
        sc["entities"] = [e for e in sc["entities"] if e["uid"] not in (200, 201, 221) + crawlers_uids]  # only the helicopter: no drones, no car, no crawlers
    if scenario == "hacking":
        sc["entities"] = [e for e in sc["entities"] if e["uid"] not in (201, 220) + crawlers_uids]  # one drone, no helicopter, no crawlers
    json.dump(sc, open(os.path.join(work, SCENE), "w", encoding="utf-8"))
    flags = 0x08000000 if os.name == "nt" else 0  # no console window
    procs = []

    def start(args, tag):
        return subprocess.Popen([EXE] + args, cwd=work, stdout=open(os.path.join(work, tag + ".log"), "w"), stderr=open(os.path.join(work, tag + ".err"), "w"), creationflags=flags)

    # (the secure run: a key for the server, and a token for each player who is to get in)
    keyfile = os.path.join(work, "server.key")
    tokens = {}
    if scenario == "secure":
        subprocess.run([EXE, "token", "--gen-key", keyfile], check=True, stdout=subprocess.DEVNULL)
        for who in ("Ada", "Bo"):
            tokens[who] = subprocess.run([EXE, "token", "--key", keyfile, "--name", who, "--protocol-id", "4242", "--ttl", "600"], check=True, capture_output=True, text=True).stdout.strip()

    def play_args(name, api):
        a = ["play", SCENE, "--hidden", "--frames", "7200", "--connect", f"127.0.0.1:{PORT}", "--api", str(api)]
        return a + (["--connect-token", tokens[name]] if name in tokens else ["--name", name])

    try:
        server_args = ["server", "--scene", SCENE, "--players", "8", "--seconds", "240", "--port", str(PORT)]
        if scenario == "secure":
            server_args += ["--secure", keyfile, "--protocol-id", "4242"]
        procs.append(start(server_args, "server"))
        time.sleep(3)
        for name, api in (("Ada", A), ("Bo", B)):
            procs.append(start(play_args(name, api), name))
        time.sleep(8)

        a, b = get(A, 40, "NetPlayer"), get(B, 40, "NetPlayer")
        check("both players connect", bool(a["connected"] and b["connected"]) and a["id"] != b["id"], f"ids {a['id']} {b['id']}")
        for who, api in (("Ada", A), ("Bo", B)):
            k = kinds(api)
            drones = {"survival": 2, "lighthouse": 0, "hacking": 1, "secure": 0, "crawler": 0, "heli": 0}[scenario]
            check(f"{who} sees the other player, the door, 6 crates, 4 things, the lighthouse and {drones} drones", k.get((0, 0)) == 1 and k.get((3, 30)) == 1 and k.get((3, 31)) == 6 and k.get((1, 32)) == 4 and k.get((4, 40), 0) == drones and k.get((4, 70)) == 1, str(k))

        if scenario == "lighthouse":
            lighthouse()
            return
        if scenario == "hacking":
            hacking()
            return
        if scenario == "secure":
            secure_play(start, play_args, tokens)
            return
        if scenario == "crawler":
            crawlers()
            return
        if scenario == "heli":
            helicopter()
            return

        # the hut: Ada beside a crate looking down at it, Bo at the back of the hut (out of the drones' sight)
        tp(A, 2.2, -11.9)
        tp(B, -1.0, -15.5)
        pitch(A, -0.6)
        time.sleep(0.8)
        door = lambda api: [c["Door"]["open"] for c in puppets(api) if "Door" in c]
        press(A, "e")  # the crate is nearer than the door: this searches it
        time.sleep(0.5)
        cont = bag(A, 43)
        check("using a crate fills the client's container inventory", get(A, 43, "NetContainer")["open"] and len(cont) >= 1, str(cont))
        # (the player starts with a pistol and 24 rounds in slots 0 and 1: the crate's stacks go to slots 6 and on)
        total = lambda stacks: sum(c for _, _, c in stacks)
        start_bag = bag(A, 40)
        check("the player starts with a pistol and rounds for it (items 5 and 10)", [(i, c) for _, i, c in start_bag] == [(5, 1), (10, 24)], str(start_bag))
        for i in range(len(cont)):
            command(A, 5, [100 + cont[i][0], 6 + i, 99, 0])
        own = bag(A, 40)
        check("inv_move carries the stacks into the bag", total(own) == total(start_bag) + total(cont) and len(own) == len(start_bag) + len(cont), f"bag {own}, crate left {bag(A, 43)}")
        press(A, "e")
        check("using the crate again shuts it", not get(A, 43, "NetContainer")["open"])
        n0 = kinds(B).get((1, 32), 0)
        hold(A, 6)  # (the first thing from the crate)
        press(A, "g")
        time.sleep(0.5)
        check("inv_drop puts a thing on the ground that the other player sees", kinds(B).get((1, 32), 0) == n0 + 1, f"{n0} -> {kinds(B).get((1, 32), 0)}")
        # (one of the stack goes: the drop command's count is 1)
        check("and the thing is the one that was in the hand", [(k, c) for k, _, c in bag(A, 40)] == [(k, c - (1 if k == 6 else 0)) for k, _, c in own], f"{own} -> {bag(A, 40)}")
        hold(A, 0)  # (the pistol again)

        # the door: Ada steps out of the hut's front doorway line and looks straight at it
        tp(A, 0.0, -8.6)
        pitch(A, 0.0)
        time.sleep(0.8)
        check("the door starts shut", door(A) == [False] and door(B) == [False], f"{door(A)} {door(B)}")
        press(A, "e")
        time.sleep(1.2)
        check("using the door opens it, for both players", door(A) == [True] and door(B) == [True], f"{door(A)} {door(B)}")

        # a shot, in the hut
        tp(A, -1.0, -11.8)
        tp(B, -1.0, -15.8)
        pitch(A, 0.0)
        time.sleep(0.6)
        h0 = mate_health(A)
        rpc(A, "input", values={"mouse_left": True})
        time.sleep(0.4)
        rpc(A, "input", values={"mouse_left": False})
        time.sleep(0.8)
        h1 = mate_health(A)
        check("a shot lowers the other player's health", bool(h0 and h1) and h1[0] < h0[0], f"{h0} -> {h1}")

        # building, in a corner of the yard
        tp(A, 25.0, 25.0)
        pitch(A, -0.5)
        time.sleep(0.6)
        press(A, "1")
        press(A, "f")
        time.sleep(0.6)
        check("build:foundation and place make a piece the other player sees", kinds(B).get((3, 1), 0) >= 1, str({k: v for k, v in kinds(B).items() if k[1] in (1, 2)}))

        # the drones: Bo stays in the hut, Ada stands on the line a drone patrols
        corrections = get(A, 40, "NetPlayer")["corrections"]
        corpses = kinds(B).get((3, 9), 0)
        tp(B, -1.0, -15.5)
        tp(A, 0.0, 6.0)
        t0 = time.time()
        low, died, up = 255, False, False
        while time.time() - t0 < 60:
            h = mate_health(B)
            if h:
                low = min(low, h[0])
                if h[0] == 0:
                    died = True
                if died and h[0] == 255:
                    up = True
                    break
            if not died:
                # (a player who has been put elsewhere, by a refused move, is walked back to the line the drone patrols)
                at = get(A, 40, "Transform")["pos"]
                if math.hypot(at[0], at[2] - 6.0) > 3.0:
                    tp(A, 0.0, 6.0)
            time.sleep(0.25)
        check("the drones take a player standing in the open to dead", died, f"lowest health seen {low}")
        check("a corpse is left", kinds(B).get((3, 9), 0) > corpses, f"{corpses} -> {kinds(B).get((3, 9), 0)}")
        check("the player gets up with full health at a spawn point (a correction moves the client)", up and get(A, 40, "NetPlayer")["corrections"] > corrections, f"corrections {corrections} -> {get(A, 40, 'NetPlayer')['corrections']}")

        # the dead player's bag is on the corpse: the player (empty handed now, in its grace of a few seconds) walks back to where it fell and searches it
        check("what the player carried is not in its hands any more", bag(A, 40) == [], str(bag(A, 40)))
        tp(A, 0.0, 7.3)
        pitch(A, -1.0)
        time.sleep(0.6)
        press(A, "e")
        time.sleep(0.6)
        on_corpse = bag(A, 43)
        check("the corpse is a container with what the player carried", get(A, 43, "NetContainer")["open"] and sum(c for _, _, c in on_corpse) >= 2, str(on_corpse))
        for i in range(len(on_corpse)):
            command(A, 5, [100 + on_corpse[i][0], i, 99, 0])
        check("and the player takes it back", sum(c for _, _, c in bag(A, 40)) >= 2, str(bag(A, 40)))

        # the guns (docs/LOADOUT.md): the weapon is the gun in the hand, the others see it there, rounds come out of the bag when the magazine is filled again, Q takes the next gun of the bag
        back = bag(A, 40)
        check("the pistol and its rounds are back in the bag", {i for _, i, _ in back} >= {5, 10}, str(back))
        tp(A, -1.0, -11.8)
        tp(B, -1.0, -15.8)
        pitch(A, 0.0)
        time.sleep(0.8)
        hold(A, 0)
        time.sleep(0.4)
        check("the other player sees the pistol in the hand (item 5 above slot 0)", hand_seen(B) == [(5 << 6) | 0], str(hand_seen(B)))
        hold(A, 1)  # (the rounds: no gun)
        time.sleep(0.4)
        check("with rounds in the hand it sees no gun, only the slot", hand_seen(B) == [1], str(hand_seen(B)))
        h0 = mate_health(A)
        rpc(A, "input", values={"mouse_left": True})
        time.sleep(0.4)
        rpc(A, "input", values={"mouse_left": False})
        time.sleep(0.8)
        check("a trigger pulled with no gun in the hand hurts nobody (the health only goes up, as a body mends)", bool(h0 and mate_health(A)) and mate_health(A)[0] >= h0[0], f"{h0} -> {mate_health(A)}")
        hold(A, 0)
        rounds = lambda: sum(c for _, i, c in bag(A, 40) if i == 10)
        rounds0 = rounds()
        for _ in range(3):
            press(A, "mouse_left", 0.3)
        h1 = mate_health(A)
        check("three shots of the pistol hurt the other player", bool(h0 and h1) and h1[0] < h0[0], f"{h0} -> {h1}")
        check("and the rounds are still in the bag: they are in the magazine", rounds() == rounds0, f"{rounds0} -> {rounds()}")
        press(A, "r", 0.3)
        time.sleep(2.5)
        used = rounds0 - rounds()
        check("a reload takes the rounds out of the bag that it put in the magazine", 3 <= used <= 12, f"{used} rounds left the bag")
        # Q: from slot 3 (nothing there) the next gun of the bag, going round
        hold(A, 3)
        press(A, "q", 0.2)
        guns_at = [k for k, i, _ in bag(A, 40) if 5 <= i <= 9]
        want = min(guns_at, key=lambda k: (k - 4) % 12) if guns_at else 3
        got = get(A, 40, "NetPlayer")["extra"]
        check("Q takes the next gun of the bag in the hand", got == want, f"the guns are in slots {guns_at}: slot {got} in hand, wanted {want}")
        hold(A, 0)
        # the player is told about itself (the STATUS message) and its HUD shows it
        time.sleep(1.0)
        me = get(A, 40, "NetPlayer")
        check("the server tells the player its gun, the magazine and the rounds in its bag", me["gun"] == 5 and 0 <= me["mag"] <= 12 and me["reserve"] == rounds(), f"gun {me['gun']}, magazine {me['mag']}, reserve {me['reserve']}, bag {rounds()}")
        check("and its vitals", 0.0 < me["health"] <= 1.0 and 0.5 < me["food"] <= 1.0 and 0.5 < me["water"] <= 1.0 and 0.0 < me["stamina"] <= 1.0, f"health {me['health']:.2f} food {me['food']:.2f} water {me['water']:.2f} stamina {me['stamina']:.2f}")
        bars = [get(A, 300 + k, "Ui")["fill"] for k in range(4)]
        check("the HUD bars show them", all(abs(b - v) < 0.05 for b, v in zip(bars, [me["health"], me["food"], me["water"], me["stamina"]])) and get(A, 305, "Ui")["visible"], f"bars {[round(b, 2) for b in bars]}, ammunition counter visible {get(A, 305, 'Ui')['visible']}")
        hold(A, 1)
        time.sleep(1.0)
        check("with no gun in the hand the ammunition counter is gone", not get(A, 305, "Ui")["visible"] and get(A, 40, "NetPlayer")["gun"] == 0)
        hold(A, 0)
        # items are used (inv_use, NetHost.uses): two cans lie on the ground at (4, -6); the use button picks them up, U eats one
        tp(A, 4.0, -4.0)
        pitch(A, -0.58)
        time.sleep(0.6)
        press(A, "e")
        time.sleep(0.8)
        cans = [(k, c) for k, i, c in bag(A, 40) if i == 2]
        check("a thing on the ground is picked up with the use button (two cans)", sum(c for _, c in cans) == 2, str(bag(A, 40)))
        if cans:
            hold(A, cans[0][0])
            press(A, "u")
            time.sleep(1.0)
            check("U uses what is in the hand: one can is eaten (the server did it and told the player)", sum(c for k, i, c in bag(A, 40) if i == 2) == 1, str(bag(A, 40)))
            hold(A, 0)

        # the car: Ada walks to it (it stands at -33, 28, out of the drones' sight), sits in the driver's seat, drives off, cannot get out at speed, brakes to a stop and gets out
        def car():
            ps = [c["Transform"]["pos"] for c in puppets(B) if c["NetPuppet"]["class"] == 4 and c["NetPuppet"]["kind"] == 61]
            return ps[0] if ps else None

        tp(A, -33.0, 31.6)
        pitch(A, -0.4)
        time.sleep(0.8)
        start = car()
        press(A, "e")
        time.sleep(0.8)
        np_ = get(A, 40, "NetPlayer")
        check("using the car sits the player in it", np_["riding"] != 0, f"riding {np_['riding']}")
        # (full throttle until the other player sees it do more than 6 m/s along the ground twice running, then let go: it must not reach the wall)
        flat = lambda p, q: math.hypot(p[0] - q[0], p[2] - q[2])
        rpc(A, "input", values={"w": True})
        t0, last, fast = time.time(), (car(), time.time()), 0
        while time.time() - t0 < 8.0 and fast < 2:
            time.sleep(0.15)
            now = (car(), time.time())
            fast = fast + 1 if last[0] and now[0] and flat(last[0], now[0]) / (now[1] - last[1]) > 6.0 else 0
            last = now
        rpc(A, "input", values={"w": False})
        time.sleep(0.1)
        here = car()
        check("the keys drive it, and the other player sees it go", start is not None and here is not None and math.dist(start, here) > 4.0, f"the car went {math.dist(start, here) if start and here else 0:.1f} m, driving for {time.time() - t0:.1f} s")
        press(A, "e")
        time.sleep(0.5)
        check("at speed the use button does not get the driver out", get(A, 40, "NetPlayer")["riding"] != 0)
        for _ in range(40):
            rpc(A, "input", values={"control": True})
            time.sleep(0.25)
            a, b = car(), None
            time.sleep(0.1)
            b = car()
            if a and b and math.dist(a, b) < 0.15:
                break
        rpc(A, "input", values={"control": False})
        time.sleep(0.5)
        press(A, "e")
        time.sleep(1.0)
        np_ = get(A, 40, "NetPlayer")
        check("stopped, the driver gets out beside the car", np_["riding"] == 0 and car() is not None and math.dist(get(A, 40, "Transform")["pos"], car()) < 5.0, f"riding {np_['riding']}")

        helicopter()
    finally:
        for p in procs:
            p.terminate()
        for p in procs:
            try:
                p.wait(5)
            except subprocess.TimeoutExpired:
                p.kill()
        shutil.rmtree(work, ignore_errors=True)
    return fails


if __name__ == "__main__":
    for scenario in sys.argv[2:] or ("survival", "lighthouse", "hacking", "secure", "crawler"):  # (names after the binary's path run only those)
        print(f"--- {scenario}")
        main(scenario)
    sys.exit(fails)
