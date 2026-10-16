"""The rules of Caldera, written as Lunaris Script (docs/SCRIPT.md). `tools/build_caldera.py` defines these in the scenes with `define_script`.

Conventions: `$g.*` is the state of a run (a new game or Continue starts it again, `SceneReset` clears it), `$opt.*` the settings (they stay), everything else is the scene's own
traffic between systems. A camera's yaw turns the walking keys: forward is (-sin yaw, -cos yaw) and right is (cos yaw, -sin yaw), the way `CameraRig` places the camera behind its target.
"""

# ------------------------------------------------------------------------------------------------ fades and scene switches (every scene)
FADER = """
/// The first tick of a scene.
system scene_init each Fader order 0 {
    if Fader.t == 0 {
        Fader.t = 0.0001;
@INIT@
    }
}

/// Anything that wants to leave sets `$leave` to the number of a switch; the picture fades to black, then the switch is thrown.
system fader each Fader, ColorGrade order 60 {
    Fader.t += dt;
    if $leave > 0 && Fader.leaving == 0 {
        Fader.leaving = 1;
        Fader.target = $leave;
    }
    if Fader.leaving > 0 {
        Fader.out += dt / 0.6;
        if Fader.out >= 1 && Fader.fired == 0 {
            Fader.fired = 1;
            $fire = Fader.target;
        }
    }
    ColorGrade.fade[3] = max(clamp(1 - Fader.t / 0.9, 0, 1), clamp(Fader.out, 0, 1));
}

system fire_switch each SwitchId, SceneSwitch order 70 {
    if $fire > 0 && $fire == SwitchId.n {
        SceneSwitch.go = true;
        $fire = 0;
    }
}
"""


def fader_script(init=""):
    return FADER.replace("@INIT@", init)


# ------------------------------------------------------------------------------------------------ the island: time, pause, camera
CORE = """
/// The hour of the day, and whether it is night (wraiths walk, golems wake from further away).
system clock each TimeOfDay order 2 {
    $hour = TimeOfDay.time;
    $night = select(TimeOfDay.time >= 19.5 || TimeOfDay.time < 5.2, 1, 0);
    $dark = clamp(select(TimeOfDay.time > 12, smoothstep(17.5, 20.5, TimeOfDay.time), smoothstep(6.5, 4.0, TimeOfDay.time)), 0, 1);
}

/// The mist takes the colour of the hour: pale blue by day, nearly black at night.
system fog_mood each HeightFog order 6 {
    HeightFog.color[0] = lerp(0.8, 0.05, $dark);
    HeightFog.color[1] = lerp(0.86, 0.06, $dark);
    HeightFog.color[2] = lerp(0.93, 0.11, $dark);
    HeightFog.density = lerp(0.002, 0.006, $dark);
}

/// The eye opens wide only in the dark: by day the exposure may not climb past what a bright picture needs (a forest in the shade would turn pale), at night it may go up to 3.5.
system eye_mood each AutoExposure order 6 {
    AutoExposure.max = lerp(1.45, 3.5, $dark);
    AutoExposure.min = 0.6;
}

/// Night falls: a word of warning, once a night.
system night_toast each Fader order 5 {
    if $night > 0 && $night.seen == 0 && $cutscene == 0 {
        $night.seen = 1;
        $toast = 1;
        $toast.kind = 4;
    }
    if $night == 0 {
        $night.seen = 0;
    }
}

/// Escape (or Start on a pad) pauses: time stops (`time.scale` 0), a second press goes on.
system pause_toggle each Fader order 4 {
    if $action.pause.pressed > 0.5 && $leave == 0 && $cutscene == 0 && $dead == 0 && $talking == 0 {
        if $paused == 0 {
            $paused = 1;
            $time.scale = 0;
            emit "select";
        } else {
            $paused = 0;
            $settings = 0;
            $time.scale = 1;
        }
    }
    $cursor.lock = select($paused > 0 || $leave > 0 || $dead > 0, 0, 1);
}

system pause_show each PauseUi, Ui order 6 {
    Ui.visible = $paused != 0 && $settings == 0;
}

system pause_show_text each PauseUi, Text order 6 {
    Text.visible = $paused != 0 && $settings == 0;
}

system pause_buttons each PauseBtn, Ui order 7 {
    if Ui.clicked && $paused != 0 && $settings == 0 {
        emit "select";
        if PauseBtn.kind == 1 {
            $paused = 0;
            $time.scale = 1;
        }
        if PauseBtn.kind == 2 {
            $settings = 1;
        }
        if PauseBtn.kind == 3 {
            $paused = 0;
            $time.scale = 1;
            $leave = 3;
        }
    }
}

/// The orbit camera: the mouse (or the right stick) turns it round the hero, the wheel pulls it in and out.
system cam_look each CameraRig order 9 {
    let live = select($paused > 0 || $leave > 0 || $cutscene > 0 || $dead > 0 || $talking > 0, 0, 1);
    let sens = $opt.sens * 0.0032 * live;
    CameraRig.yaw -= $input.mouse_dx * sens + $action.look_x * 2.6 * dt * live;
    CameraRig.pitch = clamp(CameraRig.pitch + $input.mouse_dy * sens * 0.8 - $action.look_y * 1.7 * dt * live, 0.05, 1.25);
    CameraRig.distance = clamp(CameraRig.distance - $input.mouse_wheel * 0.6 * live, 3.2, 12);
    if $cutscene == 0 {
        $cam.yaw = CameraRig.yaw;
    }
}

/// A red flash and a shake when the hero is hurt; a shake when something big lands.
system shake_cam each CameraRig order 62 {
    if $shake > 0 {
        CameraRig.shake = max(CameraRig.shake, $shake);
        $shake = 0;
    }
}

system hurt_flash each HurtFlash, Ui order 62 {
    $flash = max($flash - dt * 2.2, 0);
    Ui.color[3] = $flash * 0.38;
    Ui.visible = $flash > 0;
}

/// Hit-stop: when a blow lands the world slows to a crawl for a few ticks (counted in ticks, since the scaled `dt` would stretch them).
system hitstop each Fader order 3 {
    if $hitstop > 0 {
        $hitstop -= 1;
        $time.scale = 0.08;
    } else {
        if $hitstop == 0 && $paused == 0 && $time.scale < 1 && $cutscene == 0 && $dead == 0 {
            $time.scale = 1;
        }
    }
}
"""

# ------------------------------------------------------------------------------------------------ the hero
HERO = """
/// Walk, sprint, jump, roll, swing the sword: one state a time (`Hero.state`: 0 free, 1 swinging, 2 rolling, 3 dead). The `StateMachine` of the hero turns the state into a clip.
system hero_move each Hero, CharacterBody, Transform, HitPoints order 10 {
    Hero.t += dt;
    Hero.cd = max(Hero.cd - dt, 0);
    Hero.exhaust = max(Hero.exhaust - dt, 0);
    Hero.regen = max(Hero.regen - dt, 0);
    let busy = $paused > 0 || $leave > 0 || $talking > 0 || $cutscene > 0;
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
    let sprint = $action.sprint > 0.5 && moving && Hero.stamina > 0 && Hero.exhaust <= 0 && Hero.swim == 0 && Hero.state == 0;
    let speed = select(Hero.swim == 1, 2.8, select(sprint, 7.4, 4.4)) * select(Hero.state == 1, 0.18, 1);
    Hero.moving = select(moving, 1, 0);
    Hero.sprint = select(sprint, 1, 0);
    $swing = 0;
    if HitPoints.dead {
        Hero.state = 3;
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
    } else {
        if Hero.state == 2 {
            CharacterBody.walk[0] = Hero.rx * 9.2;
            CharacterBody.walk[1] = Hero.rz * 9.2;
            if Hero.t >= 0.62 {
                Hero.state = 0;
                Hero.t = 0;
            }
        } else {
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
            CharacterBody.walk[0] = dx * speed;
            CharacterBody.walk[1] = dz * speed;
            if Hero.state == 0 && !busy {
                if $action.roll.pressed > 0.5 && Hero.stamina >= 18 && Hero.swim == 0 && CharacterBody.grounded {
                    Hero.state = 2;
                    Hero.t = 0;
                    Hero.combo = 1 - Hero.combo;
                    Hero.stamina -= 18;
                    Hero.regen = 0.7;
                    HitPoints.invulnerable = 0.62;
                    Hero.rx = select(moving, dx / max(hypot(dx, dz), 0.001), Hero.fx);
                    Hero.rz = select(moving, dz / max(hypot(dx, dz), 0.001), Hero.fz);
                    emit "roll";
                } else {
                    if $action.attack.pressed > 0.5 && Hero.stamina >= 8 && Hero.swim == 0 && Hero.cd <= 0 {
                        Hero.state = 1;
                        Hero.t = 0;
                        Hero.combo = 1 - Hero.combo;
                        Hero.cd = 0.62;
                        Hero.stamina -= 10;
                        Hero.regen = 0.6;
                        emit "swing";
                    } else {
                        if $action.jump.pressed > 0.5 && CharacterBody.grounded && Hero.swim == 0 {
                            CharacterBody.jump = 6.4;
                            emit "jump";
                        }
                    }
                }
            }
        }
    }
    if Hero.regen <= 0 && Hero.state != 3 {
        Hero.stamina = min(Hero.stamina + 24 * dt, 100);
    }
    // face where the hero goes (or where it rolls), eased
    let wx = select(Hero.state == 2, Hero.rx, select(moving, dx, Hero.fx));
    let wz = select(Hero.state == 2, Hero.rz, select(moving, dz, Hero.fz));
    let turn = 1 - exp(0 - select(Hero.state == 2, 40, 13) * dt);
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
    $hero.x = Transform.pos[0];
    $hero.y = Transform.pos[1];
    $hero.z = Transform.pos[2];
    $hero.fx = Hero.fx;
    $hero.fz = Hero.fz;
    $hero.state = Hero.state;
    $hero.grounded = select(CharacterBody.grounded, 1, 0);
    $hp = floor(HitPoints.value);
    $stamina = Hero.stamina;
}

/// In deep water the hero floats at chest height and swims; in the shallows it wades.
system swim each Hero, CharacterBody, InWater, Transform order 11 {
    if InWater.depth > 1.2 && Hero.state != 3 {
        if Hero.swim == 0 {
            emit "splash";
            $splash = 1;
            $splash.x = Transform.pos[0];
            $splash.y = InWater.surface;
            $splash.z = Transform.pos[2];
        }
        Hero.swim = 1;
        CharacterBody.gravity_scale = 0;
        Transform.pos[1] += (InWater.surface - 1.28 - Transform.pos[1]) * min(1, 7 * dt);
    } else {
        Hero.swim = 0;
        CharacterBody.gravity_scale = 1;
    }
    $hero.depth = InWater.depth;
}

system swim_off each Hero, CharacterBody without InWater order 11 {
    Hero.swim = 0;
    CharacterBody.gravity_scale = 1;
    $hero.depth = 0;
}

/// Blows from golems and wraiths arrive as `$dmg`; a rolling hero (invulnerable) shrugs them off.
system hero_hurt each Hero, HitPoints order 12 {
    if $dmg > 0 {
        if HitPoints.invulnerable <= 0 && !HitPoints.dead {
            HitPoints.value -= $dmg;
            HitPoints.invulnerable = 0.8;
            $shake = 0.7;
            $flash = 1;
            emit "hurt";
        }
        $dmg = 0;
    }
}

/// Close to a fire the hero slowly recovers.
system hero_heal each Hero, HitPoints, Transform order 13 {
    if $dist.fire < 11 && !HitPoints.dead {
        HitPoints.value = min(HitPoints.max, HitPoints.value + 2.5 * dt);
    }
}

system hero_death each Hero, HitPoints order 14 {
    if HitPoints.dead && $dead == 0 {
        $dead = 1;
        Hero.t = 0;
        emit "lose";
    }
    if $dead == 1 {
        Hero.t += dt;
        $dead.t = Hero.t;
        $time.scale = 0.45;
        if Hero.t > 1.6 && $leave == 0 {
            $leave = 2;
        }
    }
}

/// The lantern the hero carries: it shows the wraiths, and burns brighter with every shard.
system hero_lantern each HeroLight, Light order 15 {
    Light.intensity = (0.4 + 2.6 * $dark) * select($dead > 0, 0, 1);
    Light.range = 5.5 + 0.9 * $g.shards + 0.9 * $g.dep;
}
"""

# ------------------------------------------------------------------------------------------------ golems (and the boss)
GOLEM = """
/// How far the hero is (on the ground), for the golem's systems and the boss's behaviour tree.
system golem_sense each Golem, Transform order 18 {
    Golem.dist = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
}

/// The golems' wits: asleep until the hero comes into their sight, then hunting along the ground, swinging when close, staggering when hit.
/// mode 0 asleep, 1 waking, 2 hunting, 3 swinging, 4 slamming (boss), 5 staggered, 6 breaking apart, 7 resting (boss), 8 charging (boss)
system golem_ai each Golem, CharacterBody, Transform, HitPoints, Sensor, NavAgent order 20 {
    Golem.t += dt;
    Golem.cd = max(Golem.cd - dt, 0);
    Golem.repath = max(Golem.repath - dt, 0);
    Sensor.radius = select($night > 0, Golem.wake_r * 1.5, Golem.wake_r);
    if HitPoints.dead {
        if Golem.mode != 6 {
            Golem.mode = 6;
            Golem.t = 0;
            emit "golem_die";
        }
    }
    let toward_x = select(Golem.dist > 0.01, ($hero.x - Transform.pos[0]) / Golem.dist, 0);
    let toward_z = select(Golem.dist > 0.01, ($hero.z - Transform.pos[2]) / Golem.dist, 1);
    Golem.tx = Golem.fx;
    Golem.tz = Golem.fz;
    if Golem.mode == 0 {
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        if Sensor.seen != 0 && $dead == 0 {
            Golem.mode = 1;
            Golem.t = 0;
            emit "golem_wake";
            $shake = max($shake, 0.35);
        }
    }
    if Golem.mode == 1 {
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        if Golem.t >= 1.4 {
            Golem.mode = 2;
            Golem.t = 0;
        }
    }
    if Golem.mode == 2 {
        if $dead > 0 || Golem.dist > Golem.leash {
            // the hero is gone: go back to sleep where the golem began
            Golem.mode = select(Golem.dist > Golem.leash, 0, 2);
            NavAgent.target[0] = Golem.home_x;
            NavAgent.target[1] = Transform.pos[1];
            NavAgent.target[2] = Golem.home_z;
            NavAgent.active = true;
        } else {
            if Golem.repath <= 0 && Golem.dist > 2.6 {
                NavAgent.target[0] = $hero.x;
                NavAgent.target[1] = $hero.y;
                NavAgent.target[2] = $hero.z;
                NavAgent.active = true;
                Golem.repath = 0.45;
            }
            Golem.tx = toward_x;
            Golem.tz = toward_z;
            if Golem.dist <= 2.9 && Golem.cd <= 0 && Golem.boss == 0 {
                Golem.mode = 3;
                Golem.t = 0;
                Golem.hitdone = 0;
                NavAgent.target[0] = Transform.pos[0];
                NavAgent.target[1] = Transform.pos[1];
                NavAgent.target[2] = Transform.pos[2];
                NavAgent.active = true;
                emit "golem_swing";
            }
            if Golem.boss == 1 {
                // the boss' behaviour tree says which attack it wants (`Golem.attack`)
                if Golem.attack == 1 && Golem.cd <= 0 {
                    Golem.mode = 4;
                    Golem.t = 0;
                    Golem.hitdone = 0;
                    NavAgent.target[0] = Transform.pos[0];
                    NavAgent.target[1] = Transform.pos[1];
                    NavAgent.target[2] = Transform.pos[2];
                    NavAgent.active = true;
                    emit "golem_swing";
                }
                if Golem.attack == 2 && Golem.cd <= 0 {
                    Golem.mode = 8;
                    Golem.t = 0;
                    Golem.hitdone = 0;
                    Golem.dirx = toward_x;
                    Golem.dirz = toward_z;
                    NavAgent.target[0] = Transform.pos[0];
                    NavAgent.target[1] = Transform.pos[1];
                    NavAgent.target[2] = Transform.pos[2];
                    NavAgent.active = true;
                    emit "golem_roar";
                }
                if Golem.rest == 1 {
                    Golem.mode = 7;
                    Golem.t = 0;
                    NavAgent.target[0] = Transform.pos[0];
                    NavAgent.target[1] = Transform.pos[1];
                    NavAgent.target[2] = Transform.pos[2];
                    NavAgent.active = true;
                }
            }
        }
    }
    if Golem.mode == 3 {
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        Golem.tx = toward_x;
        Golem.tz = toward_z;
        if Golem.t >= 0.5 && Golem.hitdone == 0 {
            Golem.hitdone = 1;
            if Golem.dist < 3.6 {
                $dmg += Golem.dmg;
                emit "golem_thud";
            }
        }
        if Golem.t >= 1.0 {
            Golem.mode = 2;
            Golem.t = 0;
            Golem.cd = 1.1;
        }
    }
    if Golem.mode == 4 {
        // the slam: both fists come down, the ground shakes round the boss
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        Golem.tx = toward_x;
        Golem.tz = toward_z;
        if Golem.t >= 0.95 && Golem.hitdone == 0 {
            Golem.hitdone = 1;
            $shake = 1;
            emit "boss_slam";
            $fx.kind = 3;
            $fx.x = Transform.pos[0] + Golem.fx * 2.2;
            $fx.y = Transform.pos[1];
            $fx.z = Transform.pos[2] + Golem.fz * 2.2;
            $fxgo = 1;
            if Golem.dist < 5.8 {
                $dmg += Golem.dmg;
            }
        }
        if Golem.t >= 1.5 {
            Golem.mode = 2;
            Golem.t = 0;
            Golem.cd = 0.8;
        }
    }
    if Golem.mode == 8 {
        // the charge: it roars, then runs straight along the way it was facing
        if Golem.t < 0.7 {
            CharacterBody.walk[0] = 0;
            CharacterBody.walk[1] = 0;
            Golem.tx = Golem.dirx;
            Golem.tz = Golem.dirz;
        } else {
            CharacterBody.walk[0] = Golem.dirx * 10.5;
            CharacterBody.walk[1] = Golem.dirz * 10.5;
            Golem.tx = Golem.dirx;
            Golem.tz = Golem.dirz;
            if Golem.dist < 3.2 && Golem.hitdone == 0 {
                Golem.hitdone = 1;
                $dmg += Golem.dmg * 1.2;
                $shake = 0.8;
                emit "golem_thud";
            }
        }
        if Golem.t >= 1.7 {
            Golem.mode = 2;
            Golem.t = 0;
            Golem.cd = 0.9;
        }
    }
    if Golem.mode == 5 {
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        if Golem.t >= 0.7 {
            Golem.mode = 2;
            Golem.t = 0;
        }
    }
    if Golem.mode == 7 {
        // the boss is out of breath: it kneels and takes more from the sword
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        if Golem.rest == 0 || Golem.t >= 6 {
            Golem.mode = 2;
            Golem.t = 0;
            Golem.cd = 0.5;
        }
    }
    if Golem.mode == 6 {
        CharacterBody.walk[0] = 0;
        CharacterBody.walk[1] = 0;
        if Golem.t >= 1.05 {
            $fx.kind = 2;
            $fx.x = Transform.pos[0];
            $fx.y = Transform.pos[1] + 1.2;
            $fx.z = Transform.pos[2];
            $fxgo = 1;
            $g.kills += 1;
            $shake = 0.9;
            emit "golem_break";
            if Golem.boss == 1 {
                $g.boss = 1;
            }
            despawn;
        }
    }
    // turn to face, eased
    Golem.fx += (Golem.tx - Golem.fx) * (1 - exp(0 - 9 * dt));
    Golem.fz += (Golem.tz - Golem.fz) * (1 - exp(0 - 9 * dt));
    let fl = max(hypot(Golem.fx, Golem.fz), 0.001);
    Golem.fx = Golem.fx / fl;
    Golem.fz = Golem.fz / fl;
    let yaw = atan2(0 - Golem.fx, 0 - Golem.fz);
    Transform.rot[1] = sin(yaw * 0.5);
    Transform.rot[3] = cos(yaw * 0.5);
}

/// The sword: the foe in front of the hero (the `Sensor` of the hero says which) takes the blow on the tick the blade comes down.
system foe_hurt each Golem, HitPoints, Transform order 22 {
    if $swing == 1 && $tgt == uid && !HitPoints.dead && Golem.mode != 6 {
        let amount = select(Golem.mode == 7, 70, 34);
        HitPoints.value -= amount;
        $hitstop = 5;
        $shake = max($shake, 0.45);
        $fx.kind = 1;
        $fx.x = Transform.pos[0] - $hero.fx * 0.6;
        $fx.y = Transform.pos[1] + 1.4;
        $fx.z = Transform.pos[2] - $hero.fz * 0.6;
        $fxgo = 1;
        emit "golem_hit";
        if Golem.mode == 0 {
            Golem.mode = 1;
            Golem.t = 0;
        }
        if Golem.mode == 2 || Golem.mode == 3 {
            if Golem.boss == 0 && HitPoints.value > 0 {
                Golem.mode = 5;
                Golem.t = 0;
            }
        }
    }
}

/// An ember glow from the cracks of an awake golem, red while the boss is out of breath.
system golem_glow each Golem, Light, HitPoints order 24 {
    Light.intensity = select(Golem.mode == 0 || Golem.mode == 6, 0, select(Golem.mode == 7, 3.6, 2.0));
    Light.color[1] = select(Golem.mode == 7, 0.2, 0.55);
    Light.color[2] = select(Golem.mode == 7, 0.08, 0.2);
    if Golem.boss == 1 {
        $boss.mode = Golem.mode;
        $boss.hp = HitPoints.value;
    }
}
"""

# ------------------------------------------------------------------------------------------------ wraiths (the night)
WRAITH = """
/// Wraiths come out of the dark at night, one every few seconds, up to six, and are gone at dawn. A dormant wraith waits at the bottom of the world.
system wraith_spawn each Wraith, Transform, Steer, HitPoints order 24 {
    if Wraith.state == 0 {
        if $night > 0 && $wraith.timer <= 0 && Wraith.slot == $wraith.next && $dead == 0 && $cutscene == 0 && $dist.fire > 14 {
            let a = rand(Wraith.slot + 1) * 6.2832;
            Transform.pos[0] = $hero.x + sin(a) * 17;
            Transform.pos[1] = $hero.y + 1.4;
            Transform.pos[2] = $hero.z + cos(a) * 17;
            Wraith.state = 1;
            Wraith.t = 0;
            Wraith.cd = 1.2;
            HitPoints.value = 40;
            HitPoints.dead = false;
            Steer.active = true;
            $wraith.next = (Wraith.slot + 1) % 6;
            $wraith.timer = 7;
            emit "wraith_rise";
        }
    }
}

system wraith_timer each Fader order 23 {
    $wraith.timer = max($wraith.timer - dt, 0);
}

/// A wraith drifts at the hero (`Steer` seeks it), bobbing at head height; it bites on touch; the sword breaks it; a fire or the day sends it away.
system wraith_ai each Wraith, Transform, Steer, HitPoints, Velocity order 25 {
    if Wraith.state == 1 {
        Wraith.t += dt;
        Wraith.cd = max(Wraith.cd - dt, 0);
        let d = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
        Wraith.dist = d;
        Transform.pos[1] += ($hero.y + 1.1 + 0.25 * sin(Wraith.t * 2 + Wraith.slot) - Transform.pos[1]) * min(1, 3 * dt);
        Steer.max_speed = select(d < 7, 3.9, 3.0);
        if d < 1.7 && Wraith.cd <= 0 && $dead == 0 {
            $dmg += 12;
            Wraith.cd = 1.5;
            emit "wraith_bite";
        }
        let flee = $dist.fire < 12 || $night == 0 || HitPoints.dead || $dead > 0;
        if flee {
            Wraith.state = 2;
            Wraith.t = 0;
        }
        if $swing == 1 && $tgt == uid {
            HitPoints.value -= 40;
            $hitstop = 3;
            $fx.kind = 4;
            $fx.x = Transform.pos[0];
            $fx.y = Transform.pos[1];
            $fx.z = Transform.pos[2];
            $fxgo = 1;
            emit "wraith_hit";
        }
        if HitPoints.value <= 0 {
            Wraith.state = 2;
            Wraith.t = 0;
            $g.kills += 1;
        }
    }
    if Wraith.state == 2 {
        // gone in smoke: back to the bottom of the world, ready to rise again
        Wraith.t += dt;
        if Wraith.t > 0.01 {
            $fx.kind = 5;
            $fx.x = Transform.pos[0];
            $fx.y = Transform.pos[1];
            $fx.z = Transform.pos[2];
            $fxgo = 1;
            Transform.pos[1] = 0 - 200;
            Steer.active = false;
            Velocity.lin[0] = 0;
            Velocity.lin[2] = 0;
            Wraith.state = 0;
        }
    }
}

/// A wraith can only be seen in the light of a fire or of the hero's lantern: the nearer the light, the clearer it is.
system wraith_vis each Wraith, MeshRenderer, Transform order 26 {
    let near_hero = smoothstep(5.8 + 0.9 * $g.shards, 2.6, hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z));
    let near_fire = smoothstep(19, 11, hypot(Transform.pos[0] - $camp.x, Transform.pos[2] - $camp.z));
    let a = max(near_hero, near_fire) * select(Wraith.state == 1, 0.8, 0);
    MeshRenderer.color[3] = a;
    MeshRenderer.visible = a > 0.04;
}
"""

# ------------------------------------------------------------------------------------------------ shards, the camp, the altar and the beacon
WORLD = """
/// The hero and the golems for the senses of everything else: which foe the sword would hit (the nearest in front: the `Sensor` of the hero).
system hero_target each Hero, Sensor order 16 {
    $tgt = Sensor.seen;
    $tgt.d = Sensor.distance;
}

system fire_dist each Campfire, Transform order 17 {
    $camp.x = Transform.pos[0];
    $camp.z = Transform.pos[2];
    $dist.fire = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
}

/// The fire flickers and crackles; a sleepy warm light at day, a wide one at night.
system fire_flicker each Campfire, Light order 18 {
    Light.intensity = (2.4 + 2.2 * $dark) * (0.88 + 0.12 * noise(tick * 0.09, uid, 3) + 0.06 * noise(tick * 0.31, uid, 5));
}

/// The ember shards: they float and turn; the hero takes one by walking into it (a golem's shard waits until the golem is gone).
system shard_spin each Shard, Transform, Light order 40 {
    Shard.t += dt;
    Transform.pos[1] = Shard.base_y + 0.15 * sin(Shard.t * 2.0 + Shard.n);
    let yaw = Shard.t * 1.2;
    Transform.rot[1] = sin(yaw * 0.5);
    Transform.rot[3] = cos(yaw * 0.5);
    Light.intensity = select(Shard.locked > 0, 0.3, 1.4 + 0.5 * sin(Shard.t * 3));
}

system shard_unlock each Shard order 39 {
    if Shard.locked > 0 && $g.boss > 0 {
        Shard.locked = 0;
        emit "shard_appear";
    }
}

system shard_pickup each Shard, Transform order 41 {
    if Shard.locked == 0 {
        let dy = Transform.pos[1] - $hero.y - 0.9;
        let d = sqrt((Transform.pos[0] - $hero.x) * (Transform.pos[0] - $hero.x) + dy * dy + (Transform.pos[2] - $hero.z) * (Transform.pos[2] - $hero.z));
        $near.shard = min($near.shard, d);
        if d < 1.9 {
            $g.shards += 1;
            if Shard.n == 1 { $g.s1 = 1; }
            if Shard.n == 2 { $g.s2 = 1; }
            if Shard.n == 3 { $g.s3 = 1; }
            if Shard.n == 4 { $g.s4 = 1; }
            if Shard.n == 5 { $g.s5 = 1; }
            if Shard.n == 6 { $g.s6 = 1; }
            $fx.kind = 6;
            $fx.x = Transform.pos[0];
            $fx.y = Transform.pos[1];
            $fx.z = Transform.pos[2];
            $fxgo = 1;
            $toast = 1;
            $toast.kind = 1;
            emit "shard";
            despawn;
        }
    }
}

system near_reset each Fader order 38 {
    $near.shard = 999;
    $prompt = 0;
}

/// What the hero could do here, in one number for the prompt on the screen: 1 talk, 2 rest at the fire, 3 place the shards, 4 light the beacon.
system prompts_fire each Campfire, Transform order 42 {
    if $dist.fire < 4.2 && $dead == 0 && $cutscene == 0 && $talking == 0 {
        $prompt = 2;
        if $action.interact.pressed > 0.5 && $paused == 0 {
            $game.save = 1;
            $toast = 1;
            $toast.kind = 2;
            emit "save";
        }
    }
}

system prompts_altar each Altar, Transform order 43 {
    let d = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
    $dist.altar = d;
    if d < 4.2 && $dead == 0 && $cutscene == 0 && $talking == 0 {
        if $g.shards > 0 {
            $prompt = 3;
            if $action.interact.pressed > 0.5 && $paused == 0 {
                $g.dep += $g.shards;
                $g.shards = 0;
                $toast = 1;
                $toast.kind = 3;
                $game.save = 1;
                emit "deposit";
                $fx.kind = 6;
                $fx.x = Transform.pos[0];
                $fx.y = Transform.pos[1] + 1.4;
                $fx.z = Transform.pos[2];
                $fxgo = 1;
            }
        } else {
            if $g.dep >= 6 && $g.lit == 0 {
                $prompt = 4;
            }
        }
    }
}

/// The six sockets of the altar fill as shards are placed.
system altar_sockets each Socket, MeshRenderer, Light order 44 {
    let lit = $g.dep >= Socket.n;
    MeshRenderer.emissive = select(lit, 0.9, 0.0);
    MeshRenderer.color[0] = select(lit, 1.0, 0.25);
    MeshRenderer.color[1] = select(lit, 0.62, 0.22);
    MeshRenderer.color[2] = select(lit, 0.2, 0.2);
    Light.intensity = select(lit, 0.9, 0);
}

/// E at the beacon, with the six shards in the altar: it lights, and the cutscene begins.
system beacon_use each Beacon, Transform order 45 {
    let d = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
    if d < 5.5 && $g.dep >= 6 && $g.lit == 0 && $dead == 0 && $paused == 0 && $talking == 0 {
        $prompt = 4;
        if $action.interact.pressed > 0.5 {
            $g.lit = 1;
            $cutscene = 1;
            $cam.target = uid;
            emit "beacon";
        }
    }
}

/// The beacon's light and flame come with the cutscene.
system beacon_glow each BeaconFire, Light order 46 {
    Light.intensity = select($g.lit > 0, 12 * smoothstep(0, 2.5, $lit.t) * (0.9 + 0.1 * noise(tick * 0.2, uid, 2)), 0);
}

/// The cutscene is a `Timeline` on the camera: it starts when the beacon is lit.
system cutscene_start each Timeline order 46 {
    if $cutscene == 1 && !Timeline.playing && !Timeline.done {
        Timeline.playing = true;
    }
}

system beacon_clock each Fader order 47 {
    if $g.lit > 0 {
        $lit.t += dt;
    }
}

system beacon_flames each BeaconFlame, Particles order 47 {
    Particles.playing = $g.lit > 0;
    Particles.visible = $g.lit > 0;
}

/// The pooled effects: a spark where the sword lands, debris where a golem breaks, a ring of dust where the boss slams, smoke where a wraith goes, a shower of ember light where a shard is taken.
system fx_pool each FxPool, Particles, Transform order 61 {
    if $fxgo > 0 && FxPool.kind == $fx.kind {
        Transform.pos[0] = $fx.x;
        Transform.pos[1] = $fx.y;
        Transform.pos[2] = $fx.z;
        Particles.time = 0;
        Particles.playing = true;
        Particles.visible = true;
    }
}

system fx_clear each Fader order 63 {
    $fxgo = 0;
}

system splash_fx each SplashFx, Particles, Transform order 61 {
    if $splash > 0 {
        Transform.pos[0] = $splash.x;
        Transform.pos[1] = $splash.y;
        Transform.pos[2] = $splash.z;
        Particles.time = 0;
        Particles.playing = true;
    }
}

system splash_clear each Fader order 63 {
    $splash = 0;
}
"""

# ------------------------------------------------------------------------------------------------ the NPC and its conversation
NPC = """
system npc_near each Npc, Transform order 12 {
    let d = hypot(Transform.pos[0] - $hero.x, Transform.pos[2] - $hero.z);
    $dist.npc = d;
}

system talk_state each Npc, Dialogue order 13 {
    $talking = select(Dialogue.active, 1, 0);
}

/// E speaks to the keeper of the fire when the hero is close.
system npc_interact each Npc, Dialogue order 43 {
    if $dist.npc < 3.4 && $talking < 0.5 && $dead == 0 && $cutscene == 0 && $paused == 0 {
        if $prompt == 0 {
            $prompt = 1;
            if $action.interact.pressed > 0.5 {
                Dialogue.active = true;
                emit "select";
            }
        }
    }
}

/// The quest log (L, or Back on a pad): the six shards and where they are, struck through as they are found.
system quest_toggle each QuestUi, Ui order 48 {
    if $action.quest.pressed > 0.5 && $paused == 0 && $dead == 0 && $cutscene == 0 {
        $questlog = 1 - $questlog;
        emit "select";
    }
    Ui.visible = $questlog > 0;
}

system quest_text each QuestUi, Text order 48 {
    Text.visible = $questlog > 0;
}

system quest_row each QuestRow, Ui order 49 {
    let got = select(QuestRow.n == 1, $g.s1, select(QuestRow.n == 2, $g.s2, select(QuestRow.n == 3, $g.s3, select(QuestRow.n == 4, $g.s4, select(QuestRow.n == 5, $g.s5, $g.s6)))));
    Ui.color[0] = select(got > 0, 1.0, 0.18);
    Ui.color[1] = select(got > 0, 0.66, 0.16);
    Ui.color[2] = select(got > 0, 0.2, 0.2);
    Ui.visible = $questlog > 0;
}

system quest_row_text each QuestRowText, Text order 49 {
    let got = select(QuestRowText.n == 1, $g.s1, select(QuestRowText.n == 2, $g.s2, select(QuestRowText.n == 3, $g.s3, select(QuestRowText.n == 4, $g.s4, select(QuestRowText.n == 5, $g.s5, $g.s6)))));
    Text.visible = $questlog > 0;
    Text.color[3] = select(got > 0, 0.45, 1.0);
}
"""

# ------------------------------------------------------------------------------------------------ the interface of the island
HUD = """
system hud_hp each HudHp, Ui order 50 {
    let f = clamp($hp / 100, 0, 1);
    Ui.fill = f;
    Ui.color[0] = 0.86;
    Ui.color[1] = 0.12 + 0.5 * f;
    Ui.color[2] = 0.16;
}

system hud_stamina each HudSt, Ui order 50 {
    Ui.fill = clamp($stamina / 100, 0, 1);
    Ui.color[1] = select($stamina < 18, 0.35, 0.78);
}

system hud_shard each HudShard, Ui order 50 {
    let have = select($g.dep >= HudShard.n, 2, select($g.dep + $g.shards >= HudShard.n, 1, 0));
    Ui.color[0] = select(have == 2, 1.0, select(have == 1, 1.0, 0.16));
    Ui.color[1] = select(have == 2, 0.66, select(have == 1, 0.85, 0.15));
    Ui.color[2] = select(have == 2, 0.2, select(have == 1, 0.4, 0.2));
    Ui.color[3] = select(have == 0, 0.55, 1.0);
}

system hud_show each HudItem, Ui order 51 {
    Ui.visible = $cutscene == 0 && $paused == 0 && $dead == 0;
}

system hud_show_text each HudItem, Text order 51 {
    Text.visible = $cutscene == 0 && $paused == 0 && $dead == 0;
}

/// The prompt for what E does here.
system prompt_text each PromptText, Text order 52 {
    Text.visible = $prompt == PromptText.n && $talking == 0 && $cutscene == 0 && $paused == 0 && $dead == 0;
}

/// A toast that stays a few seconds: 1 shard taken, 2 saved, 3 shards placed, 4 night falls.
system toast_clock each Fader order 52 {
    $toast.t = select($toast > 0, 0, $toast.t + dt);
    if $toast > 0 {
        $toast.k = $toast.kind;
        $toast = 0;
    }
}

system toast_text each ToastText, Text order 53 {
    Text.visible = $toast.k == ToastText.n && $toast.t < 3.4 && $cutscene == 0;
    Text.color[3] = clamp(3.4 - $toast.t, 0, 1);
}

/// The compass: N E S W slide along a strip as the camera turns, with a marker for the altar and for a shard that hums close by.
system compass_mark each CompassMark, Text order 54 {
    let target_x = select(CompassMark.kind == 0, select(CompassMark.n == 0, 0, select(CompassMark.n == 1, 10000, select(CompassMark.n == 2, 0, 0 - 10000))), $camp.x);
    let target_z = select(CompassMark.kind == 0, select(CompassMark.n == 0, 0 - 10000, select(CompassMark.n == 1, 0, select(CompassMark.n == 2, 10000, 0))), $camp.z);
    let bearing = atan2(target_x - $hero.x, 0 - (target_z - $hero.z));
    let rel = atan2(sin(bearing + $cam.yaw), cos(bearing + $cam.yaw));
    Text.pos[0] = 0.5 + rel / 1.9 * 0.38;
    Text.visible = abs(rel) < 0.95 && $cutscene == 0 && $paused == 0 && $dead == 0 && (CompassMark.kind == 0 || $g.shards > 0 || $g.dep >= 6);
}

/// Maren's name hangs over her head only when the hero is near.
system npc_label each NpcLabel, Text order 54 {
    Text.visible = $dist.npc < 12 && $cutscene == 0 && $paused == 0 && $dead == 0;
}

/// The minimap is a picture of the whole island drawn by a camera high above it (a `CameraView` that is redrawn only once in 600 pictures: a redraw costs 35 ms of CPU);
/// the marker of the hero moves over it.
system map_blip each MapBlip, Ui order 55 {
    Ui.visible = $cutscene == 0 && $paused == 0 && $dead == 0;
    Ui.pos[0] = 0.822 + (0.5 + $hero.x / 448) * 0.156 - 0.0045;
    Ui.pos[1] = 0.026 + (0.5 + $hero.z / 448) * 0.277 - 0.008;
}

/// The boss' health bar shows while it fights.
system boss_bar each BossBar, Ui order 54 {
    Ui.visible = $boss.mode > 0 && $boss.mode < 6 && $cutscene == 0 && $paused == 0 && $dead == 0;
    Ui.fill = clamp($boss.hp / 420, 0, 1);
    Ui.color[1] = select($boss.mode == 7, 0.62, 0.18);
}

system boss_bar_text each BossBarText, Text order 54 {
    Text.visible = $boss.mode > 0 && $boss.mode < 6 && $cutscene == 0 && $paused == 0 && $dead == 0;
}

/// The keys, for the first half minute.
system hint_text each Hint, Text order 53 {
    Hint.t += dt;
    Text.visible = Hint.t < 30 && $cutscene == 0 && $paused == 0 && $dead == 0;
    Text.color[3] = clamp(30 - Hint.t, 0, 1) * 0.85;
}

system dead_text each DeadUi, Text order 56 {
    Text.visible = $dead > 0;
}

/// Death: the picture goes dark and red, then the game over screen.
system dead_overlay each DeadUi, Ui order 56 {
    Ui.visible = $dead > 0;
    Ui.color[3] = clamp($dead.t * 0.6, 0, 0.75);
}
"""

# ------------------------------------------------------------------------------------------------ the settings screen (the title and the pause menu)
SETTINGS = """
system settings_show each SettingsUi, Ui order 6 {
    Ui.visible = $settings != 0;
}

system settings_show_text each SettingsUi, Text order 6 {
    Text.visible = $settings != 0;
}

/// The settings: the mouse, the volumes, the language (a click on the button cycles through `LanguageList`).
system settings_buttons each SettingBtn, Ui order 7 {
    if Ui.clicked && $settings != 0 {
        emit "select";
        if SettingBtn.kind == 1 {
            $settings = 0;
        }
        if SettingBtn.kind == 2 {
            $language.index = ($language.index + 1) % max($language.count, 1);
        }
    }
}

system setting_sliders each SettingSlider, UiSlider order 8 {
    if SettingSlider.kind == 1 {
        $opt.sens = UiSlider.value;
    }
    if SettingSlider.kind == 2 {
        $bus.music.volume = UiSlider.value;
    }
    if SettingSlider.kind == 3 {
        $bus.sfx.volume = UiSlider.value;
    }
}
"""

# ------------------------------------------------------------------------------------------------ the title, the game over and the win screens
TITLE = """
/// The camera circles the island.
system title_orbit each CameraRig order 9 {
    CameraRig.yaw += 0.05 * dt;
}

system menu_show each MenuUi, Ui order 6 {
    Ui.visible = $settings == 0;
}

system menu_show_text each MenuUi, Text order 6 {
    Text.visible = $settings == 0;
}

system menu_buttons each MenuBtn, Ui order 20 {
    if Ui.clicked && $leave == 0 && $settings == 0 {
        emit "select";
        if MenuBtn.kind == 1 {
            $leave = 1;
        }
        if MenuBtn.kind == 2 {
            $game.error = 0;
            $game.load = 1;
        }
        if MenuBtn.kind == 3 {
            $settings = 1;
        }
        if MenuBtn.kind == 4 {
            $app.quit = 1;
        }
    }
}

system menu_escape each Fader order 5 {
    if $action.pause.pressed > 0.5 && $settings != 0 {
        $settings = 0;
    }
}

system continue_look each ContinueBtn, Ui order 21 {
    Ui.color[3] = select($"game.has.1" > 0, 0.96, 0.4);
}

system nosave_text each NoSaveText, Text order 21 {
    Text.visible = $game.error > 0 && $settings == 0;
}
"""

OVER = """
system over_buttons each OverBtn, Ui order 20 {
    if Ui.clicked && $leave == 0 {
        emit "select";
        if OverBtn.kind == 1 {
            if $"game.has.1" > 0 {
                $game.error = 0;
                $game.load = 1;
            } else {
                $leave = 11;
            }
        }
        if OverBtn.kind == 2 {
            $leave = 3;
        }
    }
}
"""

WIN = """
system win_buttons each WinBtn, Ui order 20 {
    if Ui.clicked && $leave == 0 {
        emit "select";
        if WinBtn.kind == 1 {
            $leave = 11;
        }
        if WinBtn.kind == 2 {
            $leave = 3;
        }
    }
}
"""

# ------------------------------------------------------------------------------------------------ music
MUSIC = """
/// The tracks are one `MusicPlayer` each (in the boot scene, `Persist`); the scene says which should play with `$music`: 1 the title, 2 day, 3 night, 4 the boss.
system music_select each MusicTrack, MusicPlayer order 80 {
    MusicPlayer.playing = $music == MusicTrack.id;
}
"""

MUSIC_ISLAND = """
/// On the island: the boss fight, the night and the day have their own tracks.
system music_pick each Fader order 3 {
    $music = select($boss.mode > 0 && $boss.mode < 6, 4, select($night > 0, 3, 2));
}
"""
