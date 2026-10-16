"""The player of Hollowlight, shared by the levels: a body that walks, a pivot that looks (the mouse), a body camera on the chest (it sways with the step, rolls, breathes), a flashlight, the lens and picture
effects of a body camera, and the recorder's on-screen text (REC dot, clock, date, battery). `install` spawns it by the engine's tools and returns the Lunaris Script that runs it.

    import hollowlight_rig as R
    script = R.install(call, spawn, x, y, z)       # then add the level's own systems and `call("define_script", {"source": script + ...})`

Keys: WASD walk, Shift sprint (stamina), Ctrl creep, Space jump, F flashlight. The camera is the entity 54 (child of the pivot 52, child of the body 51), the flashlight 53.
"""

FONT = {"file": "examples/fonts/pixel.ttf"}
KEYS = ["move_x = d, a:-1, gamepad.lx", "move_y = w, s:-1, gamepad.ly", "jump = space, gamepad.a", "sprint = shift, gamepad.ls", "sneak = control, gamepad.rs", "flash = f, gamepad.y", "deadzone = 0.15"]

SCRIPT = """
/// Which way is ahead and to the right on the ground, from the look.
system fp_axes each MouseLook order 9 {
    $look.fx = MouseLook.forward[0];
    $look.fz = MouseLook.forward[2];
    $look.rx = MouseLook.right[0];
    $look.rz = MouseLook.right[2];
}

/// Walk, sprint (stamina), creep, jump; F is the flashlight.
system fp_walk each Player, CharacterBody, Transform order 10 {
    $player.x = Transform.pos[0];
    $player.y = Transform.pos[1];
    $player.z = Transform.pos[2];
    let want = $action.sprint > 0.5 && Player.tired < 0.5 && (abs($action.move_x) + abs($action.move_y)) > 0.1;
    let speed = select(want, 6.4, select($action.sneak > 0.5, 1.8, 3.4));
    CharacterBody.walk[0] = ($look.rx * $action.move_x + $look.fx * $action.move_y) * speed;
    CharacterBody.walk[1] = ($look.rz * $action.move_x + $look.fz * $action.move_y) * speed;
    if want {
        Player.stamina = max(Player.stamina - 14 * dt, 0);
    } else {
        Player.stamina = min(Player.stamina + 9 * dt, 100);
    }
    Player.tired = select(Player.stamina <= 0, 1, select(Player.stamina > 35, 0, Player.tired));
    if $action.jump.pressed > 0.5 && CharacterBody.grounded {
        CharacterBody.jump = 4.6;
    }
    if $action.flash.pressed > 0.5 {
        $flash.on = 1 - $flash.on;
    }
    $stamina = Player.stamina;
    $walk.v = speed * select((abs($action.move_x) + abs($action.move_y)) > 0.1 && CharacterBody.grounded, 1, 0);
}

/// The camera is on the chest: it sways with the step, rolls a little, breathes.
system bodycam each BodyCam, LocalTransform order 15 {
    $bc.phase = $bc.phase + dt * (0.9 + $walk.v * 1.5);
    let amp = $walk.v / 3.4;
    LocalTransform.pos[0] = 0.012 * sin($bc.phase) * amp;
    LocalTransform.pos[1] = -0.28 + 0.03 * amp * sin($bc.phase * 2.0) + 0.004 * sin(tick * 0.047);
    LocalTransform.pos[2] = 0.1;
    let rz = 0.025 * amp * sin($bc.phase) + 0.002 * sin(tick * 0.031);
    let rx = 0.014 * amp * sin($bc.phase * 2.0 + 0.8) + 0.003 * sin(tick * 0.043);
    LocalTransform.rot[0] = rx * 0.5;
    LocalTransform.rot[1] = 0;
    LocalTransform.rot[2] = rz * 0.5;
    LocalTransform.rot[3] = 1;
}

/// The recorder's clock: hours, minutes and seconds as digits for the on-screen text; the red dot blinks.
system bodycam_clock each Player order 16 {
    $bc.t = $bc.t + dt;
    let tt = floor($bc.t);
    let h = floor(tt / 3600) % 24;
    let m = floor(tt / 60) % 60;
    let sc = tt % 60;
    $bc.h1 = floor(h / 10);
    $bc.h0 = h % 10;
    $bc.m1 = floor(m / 10);
    $bc.m0 = m % 10;
    $bc.s1 = floor(sc / 10);
    $bc.s0 = sc % 10;
}

system rec_dot each RecDot, Ui order 31 {
    Ui.visible = ($bc.t - floor($bc.t)) < 0.65;
}

system flashlight each Flashlight, Light order 11 {
    Light.intensity = select($flash.on > 0.5, 9.0 + 0.25 * sin(tick * 0.9), 0);
}
"""


def install(call, spawn, x, y, z, date="2026-10-16", start_seconds=3 * 3600 + 14 * 60 + 7, camera_name="BODYCAM 07"):
    """Spawn the player (uids 50 to 54), the lens and the recorder's text; returns the script."""
    call("define_component", {"name": "Player", "fields": [{"name": "stamina", "type": "f32", "default": 100}, {"name": "tired", "type": "f32", "default": 0}]})
    call("define_component", {"name": "Flashlight", "fields": []})
    call("define_component", {"name": "BodyCam", "fields": []})
    call("define_component", {"name": "RecDot", "fields": []})
    call("spawn", {"uid": 50, "components": {"Name": {"value": "Input"}, "InputMap": {"bindings": chr(10).join(KEYS)}}})
    call("spawn", {"uid": 51, "components": {"Name": {"value": "Player"}, "Player": {}, "Transform": {"pos": [x, y, z]}, "CharacterBody": {"radius": 0.35, "height": 1.8, "step_height": 0.4, "max_slope": 50}}})
    call("spawn", {"uid": 52, "components": {"Name": {"value": "Head"}, "Parent": {"target": 51}, "LocalTransform": {"pos": [0, 1.62, 0]}, "Transform": {"pos": [x, y + 1.62, z]}, "MouseLook": {"sensitivity": 0.0022, "pitch_min": -1.45, "pitch_max": 1.45}}})
    call("spawn", {"uid": 54, "components": {"Name": {"value": "Body camera"}, "BodyCam": {}, "Parent": {"target": 52}, "LocalTransform": {"pos": [0, -0.28, 0.1]}, "Transform": {"pos": [x, y + 1.34, z - 0.1]},
                                              "Camera": {"fov_y": 92, "near": 0.1, "far": 6000, "active": True}}})
    call("spawn", {"uid": 53, "components": {"Name": {"value": "Flashlight"}, "Flashlight": {}, "Parent": {"target": 54}, "LocalTransform": {"pos": [0.12, 0.04, -0.1]}, "Transform": {"pos": [x + 0.12, y + 1.4, z - 0.2]},
                                              "Light": {"kind": 2, "color": [1.0, 0.95, 0.82], "intensity": 9.0, "range": 60.0, "angle": 26.0, "softness": 0.45}}})
    call("set_resource", {"name": "flash.on", "value": 1})
    call("set_resource", {"name": "cursor.lock", "value": 1})
    call("set_resource", {"name": "bc.t", "value": start_seconds})
    spawn("Body camera lens", Lens={"distortion": 0.3, "zoom": 0.62, "chromatic": 0.006, "soften": 0.8, "block": 2, "scan": 0.1})
    spawn("Rec dot", RecDot={}, Ui={"pos": [0.04, 0.047], "size": [0.0105, 0.0187], "color": [0.95, 0.08, 0.08, 1.0], "layer": 3}, UiStyle={"radius": 0.01})
    spawn("Rec text", Text={"value": "REC", "pos": [0.058, 0.038], "size": 0.034, "align": 0, "color": [0.96, 0.96, 0.96, 0.92], "shadow": True}, TextFont=FONT)
    spawn("Clock text", Text={"value": "{bc.h1}{bc.h0}:{bc.m1}{bc.m0}:{bc.s1}{bc.s0}", "pos": [0.04, 0.088], "size": 0.034, "align": 0, "color": [0.96, 0.96, 0.96, 0.92], "shadow": True}, TextFont=FONT)
    spawn("Camera name", Text={"value": f"{camera_name}   {date}", "pos": [0.04, 0.93], "size": 0.028, "align": 0, "color": [0.96, 0.96, 0.96, 0.85], "shadow": True}, TextFont=FONT)
    spawn("Battery", Ui={"pos": [0.9, 0.045], "size": [0.05, 0.022], "color": [0.85, 0.85, 0.85, 0.9], "back_color": [0.1, 0.1, 0.1, 0.6], "fill": 0.72, "border": 0.0015, "border_color": [0.95, 0.95, 0.95, 0.9], "layer": 3})
    return SCRIPT
