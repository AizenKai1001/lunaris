"""The sky of Wyrdwood as Lunaris Script, made from tables (the look of the game: long golden mornings and evenings, mist in the valleys, a grey rain, a blue night with a moon; the weather changes by itself).

A day has these moods, found by the hour: night, the blue before the dawn, sunrise, morning, noon, afternoon, sunset, dusk, night. Each mood sets the colour and strength of the sun (or the moon), the colour of the sky at the top
and at the horizon, the colour and thickness of the haze, the bloom and the exposure. The weather (clear, mist, overcast, light rain, rain, storm) is chosen for every three hours of every day from the day's number by noise (the same
world gives the same weather) and eased into the next one over the first hour; it thickens the haze, dims the sun, greys the sky and brings rain. `$wx.rain` (0 to 1) is for a rain emitter and the sound, `$wx.wet` for the game.

The engine's `TimeOfDay` is not used: it rewrites the sky from a fixed palette after the graph systems, and these colours are the game's own.
"""

DAY_SECONDS = 1500.0  # a day lasts 25 minutes

# the moods by hour: (hour, value)
SUN_COLOR = [(0, (0.34, 0.44, 0.78)), (4.8, (0.34, 0.42, 0.74)), (5.8, (0.9, 0.52, 0.42)), (6.9, (1.0, 0.62, 0.34)), (8.5, (1.0, 0.86, 0.66)), (12, (1.0, 0.96, 0.88)), (16, (1.0, 0.9, 0.74)),
             (18.0, (1.0, 0.6, 0.3)), (19.2, (1.0, 0.45, 0.22)), (20.4, (0.5, 0.42, 0.68)), (22, (0.34, 0.44, 0.78)), (24, (0.34, 0.44, 0.78))]
SUN_POWER = [(0, 0.2), (4.8, 0.2), (5.8, 0.5), (6.9, 1.5), (8.5, 2.4), (12, 3.2), (16, 2.7), (18.0, 1.8), (19.2, 1.0), (20.4, 0.35), (22, 0.2), (24, 0.2)]
SKY_TOP = [(0, (0.015, 0.025, 0.07)), (4.8, (0.03, 0.05, 0.14)), (5.8, (0.22, 0.28, 0.5)), (6.9, (0.32, 0.45, 0.72)), (8.5, (0.2, 0.42, 0.8)), (12, (0.17, 0.4, 0.82)), (16, (0.2, 0.4, 0.78)),
           (18.0, (0.3, 0.34, 0.6)), (19.2, (0.26, 0.24, 0.5)), (20.4, (0.08, 0.09, 0.22)), (22, (0.02, 0.03, 0.09)), (24, (0.015, 0.025, 0.07))]
SKY_HORIZON = [(0, (0.05, 0.06, 0.12)), (4.8, (0.1, 0.11, 0.2)), (5.8, (0.85, 0.55, 0.5)), (6.9, (1.0, 0.72, 0.5)), (8.5, (0.85, 0.88, 0.95)), (12, (0.8, 0.86, 0.95)), (16, (0.86, 0.85, 0.88)),
               (18.0, (1.0, 0.62, 0.4)), (19.2, (0.95, 0.42, 0.34)), (20.4, (0.28, 0.2, 0.36)), (22, (0.06, 0.07, 0.13)), (24, (0.05, 0.06, 0.12))]
FOG_COLOR = [(0, (0.04, 0.05, 0.1)), (4.8, (0.07, 0.08, 0.15)), (5.8, (0.7, 0.55, 0.55)), (6.9, (0.9, 0.72, 0.58)), (8.5, (0.78, 0.82, 0.86)), (12, (0.78, 0.84, 0.9)), (16, (0.82, 0.82, 0.82)),
             (18.0, (0.92, 0.66, 0.5)), (19.2, (0.82, 0.45, 0.4)), (20.4, (0.2, 0.17, 0.3)), (22, (0.05, 0.06, 0.11)), (24, (0.04, 0.05, 0.1))]
FOG_DENSITY = [(0, 0.0042), (4.8, 0.005), (5.8, 0.011), (6.9, 0.012), (8.5, 0.007), (12, 0.0035), (16, 0.004), (18.0, 0.0065), (19.2, 0.007), (20.4, 0.0055), (22, 0.0045), (24, 0.0042)]
BLOOM = [(0, 0.2), (5.8, 0.4), (6.9, 0.5), (8.5, 0.3), (12, 0.25), (16, 0.3), (18.0, 0.5), (19.2, 0.55), (20.4, 0.3), (24, 0.2)]
EXPOSURE = [(0, 1.7), (4.8, 1.6), (5.8, 1.3), (6.9, 1.15), (12, 1.1), (18.0, 1.15), (19.2, 1.3), (20.4, 1.5), (22, 1.7), (24, 1.7)]
DARK = [(0, 1.0), (4.8, 0.95), (5.8, 0.5), (6.9, 0.0), (18.0, 0.0), (19.2, 0.25), (20.4, 0.75), (22, 1.0), (24, 1.0)]  # how night it is, 0 to 1

# the weather: (fog thickness x, sun x, cloud cover 0..1, grey 0..1, rain 0..1, wind 0..1)
WEATHER = [
    ("clear", 1.0, 1.0, 0.30, 0.00, 0.0, 0.20),
    ("mist", 3.2, 0.80, 0.50, 0.15, 0.0, 0.10),
    ("overcast", 1.6, 0.50, 0.90, 0.45, 0.0, 0.35),
    ("light rain", 2.0, 0.40, 0.95, 0.55, 0.45, 0.45),
    ("rain", 2.8, 0.28, 1.00, 0.70, 1.0, 0.65),
    ("storm", 3.4, 0.18, 1.00, 0.85, 1.0, 1.0),
]
# the chance of each weather, as upper bounds of the noise value 0..1
CHANCE = [0.42, 0.57, 0.72, 0.86, 0.95, 1.0]
GREY = (0.46, 0.5, 0.54)


def f(x):
    return f"{x:.4g}"


def pw(var, keys):
    """A script expression: the value that goes through `keys` (hour, value) with eased steps, for the number in `var`."""
    expr = f(keys[-1][1])
    for i in range(len(keys) - 2, -1, -1):
        (h0, v0), (h1, v1) = keys[i], keys[i + 1]
        expr = f"select({var} < {h1}, lerp({f(v0)}, {f(v1)}, smoothstep({h0}, {h1}, {var})), {expr})"
    return expr


def pwc(var, keys, c):
    """The same for one channel (0, 1, 2) of a colour."""
    return pw(var, [(h, v[c]) for h, v in keys])


def wx_expr(var, col):
    """The weather number in `var` (0 to 5) as the table's column `col`, by a chain of selects."""
    expr = f(WEATHER[-1][col])
    for i in range(len(WEATHER) - 2, -1, -1):
        expr = f"select({var} < {i + 0.5}, {f(WEATHER[i][col])}, {expr})"
    return expr


def pick_expr(var):
    """The weather (0 to 5) a noise value 0..1 in `var` stands for."""
    expr = "5"
    for i in range(len(CHANCE) - 2, -1, -1):
        expr = f"select({var} < {CHANCE[i]}, {i}, {expr})"
    return expr


def script():
    s = []
    s.append(f"""
/// The clock: a day of {DAY_SECONDS:.0f} seconds. `$hour` 0 to 24, `$day` counts the days, `$dark` is how night it is (0 to 1). The game starts at eight in the morning.
system sky_clock each Environment order 1 {{
    if $clock.init == 0 {{
        $clock.init = 1;
        $clock.t = {8.0 / 24.0 * DAY_SECONDS:.1f};
    }}
    $clock.t += dt;
    $day = floor($clock.t / {DAY_SECONDS});
    $hour = ($clock.t - $day * {DAY_SECONDS}) / {DAY_SECONDS} * 24;
    $dark = {pw("$hour", DARK)};
    $night = select($dark > 0.6, 1, 0);
}}
""")
    s.append(f"""
/// The weather: one for each three hours of each day, from noise of the day (the same world, the same weather), eased into over the first hour of the slot.
system sky_weather each Environment order 2 {{
    let slot = floor($hour / 3);
    let prev_slot = slot - 1;
    let prev_day = select(prev_slot < 0, $day - 1, $day);
    let prev_slot2 = select(prev_slot < 0, 7, prev_slot);
    let rnow = noise($day * 2.713 + slot * 0.931 + 0.31, slot * 1.447 + 0.19, 7) * 0.5 + 0.5;
    let rprev = noise(prev_day * 2.713 + prev_slot2 * 0.931 + 0.31, prev_slot2 * 1.447 + 0.19, 7) * 0.5 + 0.5;
    let now_w = select($wx.force.on > 0.5, $wx.force, {pick_expr("rnow")});
    let prev_w = select($wx.force.on > 0.5, $wx.force, {pick_expr("rprev")});
    let k = smoothstep(0, 1, ($hour - slot * 3));
    $wx.index = select(k > 0.5, now_w, prev_w);
    $wx.fog = lerp({wx_expr("prev_w", 1)}, {wx_expr("now_w", 1)}, k);
    $wx.sun = lerp({wx_expr("prev_w", 2)}, {wx_expr("now_w", 2)}, k);
    $wx.cloud = lerp({wx_expr("prev_w", 3)}, {wx_expr("now_w", 3)}, k);
    $wx.grey = lerp({wx_expr("prev_w", 4)}, {wx_expr("now_w", 4)}, k);
    $wx.rain = lerp({wx_expr("prev_w", 5)}, {wx_expr("now_w", 5)}, k);
    $wx.wind = lerp({wx_expr("prev_w", 6)}, {wx_expr("now_w", 6)}, k);
    $wx.wet = select($wx.rain > 0.3, 1, 0);
}}
""")
    s.append(f"""
/// The sun by day and the moon by night, on one arc: the light is always above the horizon, its colour and strength are the hour's, dimmed by the weather.
system sky_sun each SunTag, Light, Transform order 3 {{
    let h = $hour;
    let ang = (h - 6) / 24 * 6.2831853;
    let elev = 1.08 * sin(ang);
    let up = abs(elev);
    let pitch = 0 - clamp(up, 0.06, 1.2);
    let yaw = 1.2 + (h - 6) / 12 * 3.14159;
    let sy = sin(yaw * 0.5);
    let cy = cos(yaw * 0.5);
    let sx = sin(pitch * 0.5);
    let cx = cos(pitch * 0.5);
    Transform.rot[0] = cy * sx;
    Transform.rot[1] = sy * cx;
    Transform.rot[2] = 0 - sy * sx;
    Transform.rot[3] = cy * cx;
    Light.color[0] = {pwc("h", SUN_COLOR, 0)};
    Light.color[1] = {pwc("h", SUN_COLOR, 1)};
    Light.color[2] = {pwc("h", SUN_COLOR, 2)};
    Light.intensity = {pw("h", SUN_POWER)} * $wx.sun;
}}
""")
    s.append(f"""
/// The sky: colours of the hour, drawn towards a grey by the weather, with the clouds the weather brings.
system sky_colors each Environment order 4 {{
    let h = $hour;
    let g = $wx.grey * (1 - $dark * 0.7);
    let top_r = {pwc("h", SKY_TOP, 0)};
    let top_g = {pwc("h", SKY_TOP, 1)};
    let top_b = {pwc("h", SKY_TOP, 2)};
    let hor_r = {pwc("h", SKY_HORIZON, 0)};
    let hor_g = {pwc("h", SKY_HORIZON, 1)};
    let hor_b = {pwc("h", SKY_HORIZON, 2)};
    let lum = 1 - $dark * 0.85;
    Environment.sky_top[0] = lerp(top_r, {GREY[0]} * lum * 0.8, g);
    Environment.sky_top[1] = lerp(top_g, {GREY[1]} * lum * 0.8, g);
    Environment.sky_top[2] = lerp(top_b, {GREY[2]} * lum * 0.8, g);
    Environment.sky_horizon[0] = lerp(hor_r, {GREY[0]} * lum, g);
    Environment.sky_horizon[1] = lerp(hor_g, {GREY[1]} * lum, g);
    Environment.sky_horizon[2] = lerp(hor_b, {GREY[2]} * lum, g);
    Environment.ground[0] = lerp(0.3, 0.05, $dark);
    Environment.ground[1] = lerp(0.32, 0.06, $dark);
    Environment.ground[2] = lerp(0.3, 0.09, $dark);
    Environment.clouds = $wx.cloud;
    Environment.ambient = lerp(1.0, 0.22, $dark) * lerp(1, 0.7, $wx.grey);
    Environment.bloom = {pw("h", BLOOM)};
    Environment.exposure = {pw("h", EXPOSURE)};
}}

/// The haze: the colour and thickness of the hour, thicker in mist and rain.
system sky_fog each HeightFog order 5 {{
    let h = $hour;
    let g = $wx.grey * 0.6;
    HeightFog.color[0] = lerp({pwc("h", FOG_COLOR, 0)}, {GREY[0]} * (1 - $dark * 0.9), g);
    HeightFog.color[1] = lerp({pwc("h", FOG_COLOR, 1)}, {GREY[1]} * (1 - $dark * 0.9), g);
    HeightFog.color[2] = lerp({pwc("h", FOG_COLOR, 2)}, {GREY[2]} * (1 - $dark * 0.9), g);
    HeightFog.density = {pw("h", FOG_DENSITY)} * $wx.fog;
}}

/// The eye: by day it may not open wide (a forest in the shade would turn pale), at night it may.
system sky_eye each AutoExposure order 5 {{
    AutoExposure.max = lerp(1.45, 2.4, $dark);
    AutoExposure.min = 0.5;
    AutoExposure.target = lerp(0.3, 0.1, $dark) * (1 - 0.3 * $wx.grey);
}}
""")
    return "".join(s)


SKY = script()

if __name__ == "__main__":
    print(SKY)
    print(len(SKY), "characters")
