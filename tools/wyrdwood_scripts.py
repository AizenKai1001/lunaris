"""The rules of Wyrdwood as Lunaris Script (see `tools/build_wyrdwood_world.py`): the camera, the hero (walking, running, sneaking, jumping, rolling, swimming, climbing, swinging what is in the hand, eating), the food that sets
health and stamina, felling trees and breaking rocks with the right tool, picking things up, the HUD, and (from other modules) the sky and the inventory. The engine's `CharacterMotion` does the swimming, the roll, the ladder and
the ledge; this only sets the input and reads the state back for the animation.

The shape of it is the reference game's: no hunger bar, health and stamina that three foods raise for a while, tools in a hotbar that wear out, trees that need an axe and rocks that need a pick.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import wyrdwood_build  # noqa: E402
import wyrdwood_inv  # noqa: E402
import wyrdwood_items as I  # noqa: E402
import wyrdwood_sky  # noqa: E402

CORE = """
/// The orbit camera: the mouse (or the right stick) turns it round the hero, the wheel pulls it in and out. Nothing while a menu is open.
system cam_look each CameraRig order 9 {
    let live = select($invopen > 0 || $paused > 0, 0, 1);
    let sens = 1.0 * 0.0032 * live;
    CameraRig.yaw -= $input.mouse_dx * sens + $action.look_x * 2.6 * dt * live;
    CameraRig.pitch = clamp(CameraRig.pitch + $input.mouse_dy * sens * 0.8 - $action.look_y * 1.7 * dt * live, 0.05, 1.25);
    CameraRig.distance = clamp(CameraRig.distance - $input.mouse_wheel * 0.6 * live * select($build.on > 0.5, 0, 1), 3.2, 12);
    $cam.yaw = CameraRig.yaw;
    $cam.pitch = CameraRig.pitch;
}

/// The pointer is free while a menu is open and held by the game otherwise.
system pointer each Environment order 4 {
    $cursor.lock = select($invopen > 0 || $paused > 0, 0, 1);
}
"""

HERO = """
/// Walk, run, sneak, jump, roll, swing what is held. `CharacterMotion` does the swimming (with `vertical`: Space rises, C dives), the roll, the ladder and the ledge; this sets the input and reads the state back for the animation.
/// Stamina counts in the same numbers as the food that raises it: a roll takes 10 of a base 50, a swing 6, running 8 a second.
system hero_move each Hero, CharacterBody, CharacterMotion, Transform, HitPoints order 10 {
    Hero.t += dt;
    Hero.cd = max(Hero.cd - dt, 0);
    Hero.exhaust = max(Hero.exhaust - dt, 0);
    Hero.regen = max(Hero.regen - dt, 0);
    Hero.hurt = max(Hero.hurt - dt * 4, 0);
    let busy = $invopen > 0 || $paused > 0;
    let mx = select(busy, 0, $action.move_x);
    let my = select(busy, 0, $action.move_y);
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
    let sneak = $action.sneak > 0.5 && !swimming && !climbing && Hero.state == 0;
    let sprint = $action.sprint > 0.5 && moving && Hero.stamina > 0 && Hero.exhaust <= 0 && !swimming && !climbing && !sneak && Hero.state == 0 && !busy;
    let speed = select(sneak, 2.2, select(sprint, 8.0, select(ln > 0.62, 5.2, 3.0))) * select(Hero.state == 1, 0.25, select(Hero.state == 4, 0.15, select(Hero.state == 5, 0.1, 1)));
    Hero.moving = select(moving || climbing, 1, 0);
    Hero.run = select(ln > 0.62, 1, 0);
    Hero.sprint = select(sprint, 1, 0);
    Hero.sneak = select(sneak, 1, 0);
    Hero.swim = select(swimming, 1, 0);
    Hero.climb = select(climbing, 1, 0);
    Hero.rolling = select(rolling, 1, 0);
    Hero.tool = select($held.class == 1, 1, 0);
    Hero.air = select(CharacterBody.grounded || swimming || climbing, 0, Hero.air + dt);
    $swing = 0;
    CharacterBody.walk[0] = dx * speed;
    CharacterBody.walk[1] = dz * speed;
    CharacterMotion.swim_speed = select($action.sprint > 0.5, 4.2, 2.6);
    CharacterMotion.vertical = select(climbing, my, select(busy, 0, $action.jump - $action.dive));
    if Hero.state == 1 {
        $swing = select(Hero.t >= 0.34 && Hero.t - dt < 0.34, 1, 0);
        if Hero.t >= 0.8 {
            Hero.state = 0;
            Hero.t = 0;
        }
    }
    if Hero.state == 4 {
        if Hero.t >= 1.1 {
            Hero.state = 0;
            Hero.t = 0;
        }
    }
    if Hero.state == 5 {
        if Hero.t >= 0.8 {
            Hero.state = 0;
            Hero.t = 0;
        }
    }
    if sprint {
        Hero.stamina -= 8 * dt;
        Hero.regen = 1.0;
        if Hero.stamina <= 0 {
            Hero.stamina = 0;
            Hero.exhaust = 1.8;
        }
    }
    if swimming {
        Hero.stamina -= select(moving, 3.5, 0) * dt;
        Hero.regen = 0.8;
        if CharacterMotion.under {
            Hero.stamina -= 6 * dt;
        }
    }
    if Hero.state == 0 && !rolling && !busy {
        if $action.roll.pressed > 0.5 && Hero.stamina >= 10 && !swimming && !climbing && CharacterBody.grounded {
            CharacterMotion.roll = true;
            CharacterMotion.roll_dir[0] = Hero.fx;
            CharacterMotion.roll_dir[1] = Hero.fz;
            Hero.stamina -= 10;
            Hero.regen = 1.0;
            emit "roll";
        } else {
            if $action.attack.pressed > 0.5 && Hero.stamina >= 6 && !swimming && !climbing && Hero.cd <= 0 && ($held.class == 1 || $held.class == 2 || $held.class == 4) {
                Hero.state = 1;
                Hero.t = 0;
                Hero.cd = 0.9;
                Hero.stamina -= 6;
                Hero.regen = 0.8;
                emit "swing";
            } else {
                if $action.jump.pressed > 0.5 && CharacterBody.grounded && !swimming && !climbing {
                    CharacterBody.jump = 6.4;
                    Hero.stamina -= 4;
                    Hero.regen = 0.6;
                    emit "jump";
                }
            }
        }
    }
    if Hero.regen <= 0 {
        Hero.stamina = min(Hero.stamina + select(sneak, 8, 11) * dt, $stmax);
    }
    Hero.stamina = min(Hero.stamina, $stmax);
    // face where the hero goes, eased (not on a ladder, where it faces the ladder); the model faces +z when its yaw is 0
    if !climbing {
        let wx = select(rolling, CharacterMotion.roll_dir[0], select(moving, dx, Hero.fx));
        let wz = select(rolling, CharacterMotion.roll_dir[1], select(moving, dz, Hero.fz));
        let turn = 1 - exp(0 - select(rolling, 40, 13) * dt);
        Hero.fx += (wx - Hero.fx) * turn;
        Hero.fz += (wz - Hero.fz) * turn;
        let fl = max(hypot(Hero.fx, Hero.fz), 0.001);
        Hero.fx = Hero.fx / fl;
        Hero.fz = Hero.fz / fl;
        let yaw = atan2(Hero.fx, Hero.fz);
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
    $hp = HitPoints.value;
    $stamina = Hero.stamina;
}

/// Three foods at a time: each gives health and stamina for its time; the base is a small 25 and 50, and the foods are what make a body strong. No hunger, nothing to starve of.
system food each Hero, HitPoints order 8 {
    let hp0 = select($food.0.t > 0, $food.0.hp, 0);
    let hp1 = select($food.1.t > 0, $food.1.hp, 0);
    let hp2 = select($food.2.t > 0, $food.2.hp, 0);
    let st0 = select($food.0.t > 0, $food.0.st, 0);
    let st1 = select($food.1.t > 0, $food.1.st, 0);
    let st2 = select($food.2.t > 0, $food.2.st, 0);
    $food.0.t = max($food.0.t - dt, 0);
    $food.1.t = max($food.1.t - dt, 0);
    $food.2.t = max($food.2.t - dt, 0);
    $hpmax = %(base_hp)s + hp0 + hp1 + hp2;
    $stmax = %(base_st)s + st0 + st1 + st2;
    HitPoints.max = $hpmax;
    HitPoints.value = min(HitPoints.value, $hpmax);
    HitPoints.regen = 0.25 + (hp0 + hp1 + hp2) * 0.012;
    if $eat.try > 0 {
        let sid = %(slot_id)s;
        let isfood = select(%(is_food)s > 0, 1, 0);
        if isfood > 0 && Hero.state == 0 && $rm.0.n == 0 {
            // the slot with the least time left takes the new food (an empty one first)
            let a = $food.0.t;
            let b = $food.1.t;
            let cc = $food.2.t;
            let pick = select(a <= b && a <= cc, 0, select(b <= cc, 1, 2));
            let fh = %(food_hp)s;
            let fs = %(food_st)s;
            let ft = %(food_t)s;
            if pick == 0 {
                $food.0.id = sid;
                $food.0.hp = fh;
                $food.0.st = fs;
                $food.0.t = ft;
                $food.0.max = ft;
            }
            if pick == 1 {
                $food.1.id = sid;
                $food.1.hp = fh;
                $food.1.st = fs;
                $food.1.t = ft;
                $food.1.max = ft;
            }
            if pick == 2 {
                $food.2.id = sid;
                $food.2.hp = fh;
                $food.2.st = fs;
                $food.2.t = ft;
                $food.2.max = ft;
            }
            $rm.0.id = sid;
            $rm.0.n = 1;
            Hero.state = 4;
            Hero.t = 0;
            HitPoints.value = min(HitPoints.value + fh * 0.5, $hpmax);
            emit "eat";
        }
        $eat.try = 0;
    }
}
""" % {
    "base_hp": I.BASE_HEALTH,
    "base_st": I.BASE_STAMINA,
    "slot_id": wyrdwood_inv.slot_pick("($eat.try - 1)", "id", I.HOT_SLOTS),
    "is_food": wyrdwood_inv.chain("sid", {num: (1 if it[3] == "food" else 0) for num, it in I.BY_ID.items()}),
    "food_hp": wyrdwood_inv.chain("sid", {num: it[7][0] for num, it in I.BY_ID.items() if it[7]}),
    "food_st": wyrdwood_inv.chain("sid", {num: it[7][1] for num, it in I.BY_ID.items() if it[7]}),
    "food_t": wyrdwood_inv.chain("sid", {num: it[7][2] for num, it in I.BY_ID.items() if it[7]}),
}

HARVEST = """
/// A swing of what is held hits what is in front of the hero within reach: an axe fells trees, a pick breaks rocks (with anything else it does nothing to them); a tree or rock that has taken enough breaks into logs or stones.
system chop each Choppable, Transform, HitPoints order 20 {
    Choppable.shake = max(Choppable.shake - dt * 2.4, 0);
    if $swing > 0 && HitPoints.value > 0 {
        let dx = Transform.pos[0] - $hero.x;
        let dz = Transform.pos[2] - $hero.z;
        let d = hypot(dx, dz);
        let facing = (dx * $hero.fx + dz * $hero.fz) / max(d, 0.001);
        if d < 2.7 + Choppable.reach && facing > 0.3 {
            let right = select(Choppable.kind == 1, select($held.class == 1, 1, 0), select(Choppable.kind == 2, select($held.class == 2, 1, 0), 0));
            if right > 0 {
                HitPoints.value -= $held.power;
                $hitfx = 1;
                Choppable.shake = 1;
                emit "chop";
            } else {
                $toast = 1;
                $toast.kind = select(Choppable.kind == 1, 1, 2);
                emit "clank";
            }
        }
    }
    if HitPoints.value <= 0 {
        if Choppable.kind == 1 {
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 1.6, Transform.pos[2]) velocity (1.6, 3.2, 0.2);
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 2.0, Transform.pos[2]) velocity (-1.0, 3.6, 1.4);
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 1.2, Transform.pos[2]) velocity (-0.8, 3.0, -1.5);
            spawn_prefab "Log" at (Transform.pos[0], Transform.pos[1] + 2.4, Transform.pos[2]) velocity (0.4, 3.4, 1.8);
        } else {
            spawn_prefab "Stone" at (Transform.pos[0], Transform.pos[1] + 1.0, Transform.pos[2]) velocity (1.4, 3.0, 0.4);
            spawn_prefab "Stone" at (Transform.pos[0], Transform.pos[1] + 1.2, Transform.pos[2]) velocity (-1.2, 3.4, 1.0);
            spawn_prefab "Stone" at (Transform.pos[0], Transform.pos[1] + 0.8, Transform.pos[2]) velocity (-0.4, 2.8, -1.6);
            spawn_prefab "Flint" at (Transform.pos[0], Transform.pos[1] + 1.0, Transform.pos[2]) velocity (0.6, 3.0, -0.8);
        }
        emit "fell";
        despawn;
    }
}

/// What lies on the ground is picked up by walking over it (after a moment, and if there is room): into a stack of the same, or an empty slot.
system pickup each Pickup, Transform order 21 {
    Pickup.t += dt;
    let flat = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
    let d = hypot(flat, Transform.pos[1] - ($hero.y + 0.9));
    // one in six ticks is a thing's own (by its uid): two things picked up in the same tick would both add to the same inventory
    if Pickup.t > 0.7 && d < 1.8 && $invopen == 0 && uid %% 6 == tick %% 6 {
%(add)s
        if pk_left < Pickup.n {
            emit "pick";
        }
        Pickup.n = pk_left;
        if pk_left <= 0 {
            despawn;
        }
    }
}

/// The message about the wrong tool.
system toast_ui each ToastText, Text order 31 {
    $toast.t = max($toast.t - dt, 0);
    if $toast > 0 {
        $toast = 0;
        $toast.t = 2.2;
    }
    Text.visible = $toast.t > 0;
}
""" % {"add": wyrdwood_inv.add_code("pk", "Pickup.kind", "Pickup.n")}

HUD = """
/// What the hero holds is shown while its slot is selected (`BoneAttach` things in the hand: `Held` + item number).
system held_vis each HeldPart, MeshRenderer order 22 {
    MeshRenderer.visible = $held.id == HeldPart.item;
}

system hud_bars each HudHp, Ui order 30 {
    Ui.fill = clamp($hp / max($hpmax, 1), 0, 1);
    Ui.size[0] = 0.07 + 0.0041 * $hpmax;
}

system hud_stamina each HudSt, Ui order 30 {
    Ui.fill = clamp($stamina / max($stmax, 1), 0, 1);
    Ui.size[0] = 0.07 + 0.0041 * $stmax;
}

/// The three foods: a small bar each, running out with its time.
system hud_food each HudFood, Ui order 30 {
    if HudFood.n == 0 {
        Ui.visible = $food.0.t > 0;
        Ui.fill = clamp($food.0.t / max($food.0.max, 1), 0, 1);
    }
    if HudFood.n == 1 {
        Ui.visible = $food.1.t > 0;
        Ui.fill = clamp($food.1.t / max($food.1.max, 1), 0, 1);
    }
    if HudFood.n == 2 {
        Ui.visible = $food.2.t > 0;
        Ui.fill = clamp($food.2.t / max($food.2.max, 1), 0, 1);
    }
}
"""

ALL = wyrdwood_sky.SKY + CORE + HERO + HARVEST + HUD + wyrdwood_inv.SCRIPT + wyrdwood_build.SCRIPT

HERO_MACHINE = "\n".join([
    "state idle clip=Idle_Loop", "state walk clip=Walk_Loop speed=1.1", "state run clip=Jog_Fwd_Loop speed=1.15", "state sprint clip=Sprint_Loop speed=1.1", "state air clip=Jump_Loop", "state roll clip=Roll once speed=1.5",
    "state swim clip=Swim_Fwd_Loop", "state swim_idle clip=Swim_Idle_Loop", "state climb clip=Walk_Loop speed=0.6", "state chop clip=TreeChopping_Loop speed=1.7", "state slash clip=Sword_Regular_A once speed=1.2",
    "state die clip=Death01 once", "state hurt clip=Hit_Chest once speed=1.2", "state eat clip=Consume once", "state build clip=Interact once speed=1.2", "state sneak clip=Crouch_Fwd_Loop", "state sneak_idle clip=Crouch_Idle_Loop",
    "* -> die when Hero.state == 3 fade 0.1",
    "* -> hurt when Hero.hurt > 0.5 fade 0.05",
    "* -> roll when Hero.rolling == 1 fade 0.05",
    "* -> chop when Hero.state == 1 and Hero.tool == 1 fade 0.08",
    "* -> slash when Hero.state == 1 and Hero.tool != 1 fade 0.06",
    "* -> eat when Hero.state == 4 fade 0.1",
    "* -> build when Hero.state == 5 fade 0.1",
    "* -> swim when Hero.swim == 1 and Hero.moving == 1 fade 0.2",
    "* -> swim_idle when Hero.swim == 1 and Hero.moving == 0 fade 0.2",
    "* -> climb when Hero.climb == 1 fade 0.15",
    "* -> air when Hero.air > 0.14 and Hero.swim == 0 fade 0.1",
    "* -> sprint when Hero.sprint == 1 and Hero.state == 0 and Hero.air < 0.14 fade 0.12",
    "* -> sneak when Hero.sneak == 1 and Hero.moving == 1 and Hero.state == 0 fade 0.12",
    "* -> sneak_idle when Hero.sneak == 1 and Hero.moving == 0 and Hero.state == 0 fade 0.12",
    "* -> run when Hero.moving == 1 and Hero.run == 1 and Hero.state == 0 and Hero.air < 0.14 fade 0.12",
    "* -> walk when Hero.moving == 1 and Hero.state == 0 and Hero.air < 0.14 fade 0.12",
    "* -> idle when Hero.moving == 0 and Hero.state == 0 and Hero.air < 0.14 fade 0.2",
])
