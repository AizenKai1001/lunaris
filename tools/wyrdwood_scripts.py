"""The rules of Wyrdwood's first island as Lunaris Script (see `tools/build_wyrdwood.py`): the clock and the mood of the hour, the camera, the hero (walking, running, jumping, rolling, swimming, climbing
and swinging the axe, with the engine's `CharacterMotion` doing the swimming, the roll, the ladder and the ledge), felling trees and breaking rocks, picking up what falls, and the HUD."""

CORE = """
/// The orbit camera: the mouse (or the right stick) turns it round the hero, the wheel pulls it in and out.
system cam_look each CameraRig order 9 {
    let sens = 1.0 * 0.0032;
    CameraRig.yaw -= $input.mouse_dx * sens + $action.look_x * 2.6 * dt;
    CameraRig.pitch = clamp(CameraRig.pitch + $input.mouse_dy * sens * 0.8 - $action.look_y * 1.7 * dt, 0.05, 1.25);
    CameraRig.distance = clamp(CameraRig.distance - $input.mouse_wheel * 0.6, 3.2, 12);
    $cam.yaw = CameraRig.yaw;
}
"""

HERO = """
/// Walk, run, jump, roll, swing the axe. The engine's `CharacterMotion` does the swimming (with `vertical`: Space rises, C dives), the roll (the request `roll` and the direction), the ladder (walk into it) and
/// the ledge (a jump against it); this system only sets the input and reads the state back for the animation.
system hero_move each Hero, CharacterBody, CharacterMotion, Transform, HitPoints order 10 {
    Hero.t += dt;
    Hero.cd = max(Hero.cd - dt, 0);
    Hero.exhaust = max(Hero.exhaust - dt, 0);
    Hero.regen = max(Hero.regen - dt, 0);
    let mx = $action.move_x;
    let my = $action.move_y;
    let ln = hypot(mx, my);
    let k = select(ln > 1, 1 / ln, 1);
    let nx = mx * k;
    let ny = my * k;
    let c = cos($cam.yaw);
    let s = sin($cam.yaw);
    let dx = c * nx - s * ny;
    let dz = 0 - s * nx - c * ny;
    let moving = ln > 0.12;
    let swimming = CharacterMotion.swimming;
    let climbing = CharacterMotion.climbing;
    let rolling = CharacterMotion.rolling;
    let sprint = $action.sprint > 0.5 && moving && Hero.stamina > 0 && Hero.exhaust <= 0 && !swimming && !climbing && Hero.state == 0;
    let speed = select(sprint, 7.4, 4.4) * select(Hero.state == 1, 0.2, 1);
    Hero.moving = select(moving || climbing, 1, 0);
    Hero.sprint = select(sprint, 1, 0);
    Hero.swim = select(swimming, 1, 0);
    Hero.climb = select(climbing, 1, 0);
    Hero.rolling = select(rolling, 1, 0);
    Hero.air = select(CharacterBody.grounded || swimming || climbing, 0, Hero.air + dt);
    $swing = 0;
    CharacterBody.walk[0] = dx * speed;
    CharacterBody.walk[1] = dz * speed;
    CharacterMotion.swim_speed = select($action.sprint > 0.5, 4.2, 2.6);
    CharacterMotion.vertical = select(climbing, my, $action.jump - $action.dive);
    if Hero.state == 1 {
        $swing = select(Hero.t >= 0.21 && Hero.t - dt < 0.21, 1, 0);
        if Hero.t >= 0.58 {
            Hero.state = 0;
            Hero.t = 0;
        }
    }
    if sprint {
        Hero.stamina -= 15 * dt;
        Hero.regen = 0.9;
        if Hero.stamina <= 0 {
            Hero.stamina = 0;
            Hero.exhaust = 1.6;
        }
    }
    if swimming {
        Hero.stamina -= select(moving, 6, 0) * dt;
        Hero.regen = 0.6;
        if CharacterMotion.under {
            Hero.stamina -= 10 * dt;
        }
    }
    if Hero.state == 0 && !rolling {
        if $action.roll.pressed > 0.5 && Hero.stamina >= 18 && !swimming && !climbing && CharacterBody.grounded {
            CharacterMotion.roll = true;
            CharacterMotion.roll_dir[0] = Hero.fx;
            CharacterMotion.roll_dir[1] = Hero.fz;
            Hero.stamina -= 18;
            Hero.regen = 0.7;
            emit "roll";
        } else {
            if $action.attack.pressed > 0.5 && Hero.stamina >= 8 && !swimming && !climbing && Hero.cd <= 0 {
                Hero.state = 1;
                Hero.t = 0;
                Hero.cd = 0.62;
                Hero.stamina -= 10;
                Hero.regen = 0.6;
                emit "swing";
            } else {
                if $action.jump.pressed > 0.5 && CharacterBody.grounded && !swimming && !climbing {
                    CharacterBody.jump = 6.4;
                    emit "jump";
                }
            }
        }
    }
    if Hero.regen <= 0 {
        Hero.stamina = min(Hero.stamina + 24 * dt, 100);
    }
    // face where the hero goes, eased (not on a ladder, where it faces the ladder)
    if !climbing {
        let wx = select(rolling, CharacterMotion.roll_dir[0], select(moving, dx, Hero.fx));
        let wz = select(rolling, CharacterMotion.roll_dir[1], select(moving, dz, Hero.fz));
        let turn = 1 - exp(0 - select(rolling, 40, 13) * dt);
        Hero.fx += (wx - Hero.fx) * turn;
        Hero.fz += (wz - Hero.fz) * turn;
        let fl = max(hypot(Hero.fx, Hero.fz), 0.001);
        Hero.fx = Hero.fx / fl;
        Hero.fz = Hero.fz / fl;
        let yaw = atan2(0 - Hero.fx, 0 - Hero.fz);
        Transform.rot[0] = 0;
        Transform.rot[2] = 0;
        Transform.rot[1] = sin(yaw * 0.5);
        Transform.rot[3] = cos(yaw * 0.5);
    }
    $hero.x = Transform.pos[0];
    $hero.y = Transform.pos[1];
    $hero.z = Transform.pos[2];
    $hero.fx = Hero.fx;
    $hero.fz = Hero.fz;
    $hero.swim = Hero.swim;
    $hero.depth = CharacterMotion.depth;
    $hp = floor(HitPoints.value);
    $stamina = Hero.stamina;
}
"""

HARVEST = """
/// A swing of the axe hits what is in front of the hero within reach; a tree or rock that has taken enough blows breaks into logs or stones.
system chop each Choppable, Transform, HitPoints order 20 {
    Choppable.shake = max(Choppable.shake - dt * 2.4, 0);
    if $swing > 0 && HitPoints.value > 0 {
        let dx = Transform.pos[0] - $hero.x;
        let dz = Transform.pos[2] - $hero.z;
        let d = hypot(dx, dz);
        let facing = (dx * $hero.fx + dz * $hero.fz) / max(d, 0.001);
        if d < 2.7 + Choppable.reach && facing > 0.3 {
            HitPoints.value -= 1;
            Choppable.shake = 1;
            emit "chop";
        }
    }
    if HitPoints.value <= 0 {
        if Choppable.kind == 1 {
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 1.6, Transform.pos[2]) velocity (1.6, 3.2, 0.2);
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 2.0, Transform.pos[2]) velocity (-1.0, 3.6, 1.4);
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 1.2, Transform.pos[2]) velocity (-0.8, 3.0, -1.5);
        } else {
            spawn_prefab "Stone" at (Transform.pos[0], Transform.pos[1] + 1.0, Transform.pos[2]) velocity (1.4, 3.0, 0.4);
            spawn_prefab "Stone" at (Transform.pos[0], Transform.pos[1] + 1.2, Transform.pos[2]) velocity (-1.2, 3.4, 1.0);
            spawn_prefab "Stone" at (Transform.pos[0], Transform.pos[1] + 0.8, Transform.pos[2]) velocity (-0.4, 2.8, -1.6);
        }
        emit "fell";
        despawn;
    }
}

/// What falls is picked up by walking over it, after a moment.
system pickup each Pickup, Transform order 21 {
    Pickup.t += dt;
    let flat = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
    let d = hypot(flat, Transform.pos[1] - ($hero.y + 0.9));
    if Pickup.t > 0.7 && d < 1.8 {
        if Pickup.kind == 1 {
            $wood += 1;
        } else {
            $stone += 1;
        }
        emit "pick";
        despawn;
    }
}
"""

HUD = """
system hud_bars each HudHp, Ui order 30 {
    Ui.fill = clamp($hp / 100, 0, 1);
}

system hud_stamina each HudSt, Ui order 30 {
    Ui.fill = clamp($stamina / 100, 0, 1);
}
"""

import wyrdwood_sky  # noqa: E402

ALL = wyrdwood_sky.SKY + CORE + HERO + HARVEST + HUD

HERO_MACHINE = "\n".join([
    "state idle clip=idle", "state walk clip=walk speed=1.5", "state run clip=run speed=1.35", "state air clip=air", "state attack clip=attack once", "state roll clip=roll once", "state swim clip=swim", "state climb clip=walk speed=0.7",
    "* -> roll when Hero.rolling == 1 fade 0.05",
    "* -> attack when Hero.state == 1 fade 0.05",
    "* -> swim when Hero.swim == 1 and Hero.state == 0 fade 0.2",
    "* -> climb when Hero.climb == 1 and Hero.state == 0 fade 0.15",
    "* -> air when Hero.air > 0.14 and Hero.state == 0 and Hero.swim == 0 fade 0.1",
    "* -> run when Hero.sprint == 1 and Hero.state == 0 and Hero.swim == 0 and Hero.air < 0.14 fade 0.12",
    "* -> walk when Hero.moving == 1 and Hero.state == 0 and Hero.swim == 0 and Hero.climb == 0 and Hero.air < 0.14 fade 0.12",
    "* -> idle when Hero.moving == 0 and Hero.state == 0 and Hero.swim == 0 and Hero.air < 0.14 fade 0.2",
])
