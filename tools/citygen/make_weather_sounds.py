"""Makes the weather's and the world's sounds from nothing (no recordings): a rain that loops, a wind that loops, a day's birds and a night's crickets that loop, and a roll of thunder.

    python tools/citygen/make_weather_sounds.py citygen-data/sounds

Rain: white noise with the low end and the top taken off (a hiss that is mostly 1 to 6 kHz) with soft drips on top; wind: noise bent into a slow swell (low-passed at 300 Hz, the cut-off wandering);
thunder: noise with a long low rumble, a crack at the start and a slow fall. All are 16-bit mono WAVs; the two loops are cross-faded at their ends so that they repeat without a click.
"""
import os
import struct
import sys
import wave

import numpy as np

RATE = 22050


def one_pole_lp(x, cutoff):
    """Low-pass by one pole; `cutoff` in Hz may be an array (a wandering cut-off)."""
    k = np.clip(2 * np.pi * np.asarray(cutoff, dtype=np.float64) / RATE, 0.0005, 1.0)
    k = np.broadcast_to(k, x.shape)
    out = np.empty_like(x)
    acc = 0.0
    for i in range(len(x)):
        acc += k[i] * (x[i] - acc)
        out[i] = acc
    return out


def loop_fade(x, seconds=1.0):
    """Make `x` repeat without a click: the tail is mixed over the head."""
    n = int(seconds * RATE)
    head, tail = x[:n].copy(), x[-n:].copy()
    t = np.linspace(0.0, 1.0, n)
    body = x[n:-n] if len(x) > 2 * n else x[n:]
    mixed = head * t + tail * (1.0 - t)
    return np.concatenate([mixed, body])


def write_wav(path, x, peak=0.9):
    x = x / (np.max(np.abs(x)) + 1e-9) * peak
    data = (x * 32767).astype("<i2")
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data.tobytes())
    print(path, f"{len(x) / RATE:.1f} s", f"{os.path.getsize(path) / 1024:.0f} KB")


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    rng = np.random.default_rng(7)

    # rain: hiss between about 800 Hz and 7 kHz, with a slow breathing in its loudness, and drips
    n = RATE * 9
    white = rng.standard_normal(n)
    hiss = one_pole_lp(white, 7000.0) - one_pole_lp(white, 800.0)
    breath = 0.85 + 0.15 * np.sin(np.linspace(0, 2 * np.pi * 3, n) + 1.0) * np.sin(np.linspace(0, 2 * np.pi * 7, n))
    drips = np.zeros(n)
    for pos in rng.integers(0, n - 400, size=RATE * 9 // 6):
        length = int(rng.integers(60, 220))
        env = np.exp(-np.arange(length) / (length / 5.0))
        tone = np.sin(2 * np.pi * rng.uniform(1800, 4200) * np.arange(length) / RATE)
        drips[pos:pos + length] += env * tone * rng.uniform(0.05, 0.25)
    rain = hiss * breath * 0.6 + drips
    write_wav(os.path.join(out, "rain.wav"), loop_fade(rain, 1.2), 0.85)

    # wind: noise that swells and falls, the cut-off wandering between 120 and 420 Hz
    n = RATE * 12
    white = rng.standard_normal(n)
    t = np.linspace(0, 1, n)
    cutoff = 250.0 + 150.0 * np.sin(2 * np.pi * 2.0 * t + 0.5) * np.sin(2 * np.pi * 0.5 * t + 2.0) + 40.0 * np.sin(2 * np.pi * 5.0 * t)
    wind = one_pole_lp(white, cutoff)
    swell = 0.65 + 0.35 * np.sin(2 * np.pi * 3.0 * t + 0.3) * np.sin(2 * np.pi * 1.0 * t + 1.1)
    write_wav(os.path.join(out, "wind.wav"), loop_fade(wind * swell, 1.5), 0.8)

    # the day: a faint wind, a far hum of a city, and birds now and then (short sweeps with a flutter)
    n = RATE * 14
    t = np.arange(n) / RATE
    white = rng.standard_normal(n)
    hum = one_pole_lp(white, 140.0) * 0.5
    air = one_pole_lp(white, 520.0) * (0.12 + 0.08 * np.sin(2 * np.pi * 0.35 * t + 1.0))
    day = hum + air
    for start in np.sort(rng.uniform(0.5, 13.0, size=22)):
        base = rng.uniform(2200, 4200)
        length = rng.uniform(0.07, 0.22)
        i0, i1 = int(start * RATE), min(int((start + length) * RATE), n)
        tt = np.arange(i1 - i0) / RATE
        sweep = base * (1.0 + 0.35 * np.sin(2 * np.pi * rng.uniform(14, 30) * tt) * 0.08 + rng.uniform(-0.25, 0.4) * tt / length)
        env = np.sin(np.pi * np.clip(tt / length, 0, 1)) ** 1.5
        chirp = np.sin(2 * np.pi * np.cumsum(sweep) / RATE) * env * rng.uniform(0.15, 0.4)
        day[i0:i1] += chirp
        if rng.random() < 0.6:
            j0 = i1 + int(0.07 * RATE)
            j1 = min(j0 + (i1 - i0), n)
            day[j0:j1] += chirp[: j1 - j0] * 0.8
    write_wav(os.path.join(out, "amb_day.wav"), loop_fade(day, 1.5), 0.7)

    # the night: crickets (trains of pulses on a high tone, several out of step), the same far hum, an owl-ish low note now and then
    n = RATE * 14
    t = np.arange(n) / RATE
    white = rng.standard_normal(n)
    night = one_pole_lp(white, 120.0) * 0.4
    for k in range(7):
        f = rng.uniform(4200, 5200)
        rate = rng.uniform(26, 38)
        gate = (np.sin(2 * np.pi * rate * t + rng.uniform(0, 6.3)) > 0.35).astype(np.float64)
        burst = (np.sin(2 * np.pi * rng.uniform(0.18, 0.4) * t + rng.uniform(0, 6.3)) > -0.2).astype(np.float64)
        night += np.sin(2 * np.pi * f * t) * gate * burst * 0.09
    for start in rng.uniform(1.0, 12.0, size=2):
        i0 = int(start * RATE)
        length = int(0.5 * RATE)
        tt = np.arange(min(length, n - i0)) / RATE
        night[i0:i0 + len(tt)] += np.sin(2 * np.pi * (380 - 40 * tt) * tt) * np.sin(np.pi * np.clip(tt / 0.5, 0, 1)) * 0.12
    write_wav(os.path.join(out, "amb_night.wav"), loop_fade(night, 1.5), 0.7)

    # thunder: a crack, then a long low rumble that falls and rolls
    n = RATE * 6
    white = rng.standard_normal(n)
    t = np.arange(n) / RATE
    rumble = one_pole_lp(white, 110.0) * np.exp(-t / 1.6) * (0.7 + 0.3 * np.sin(2 * np.pi * 3.3 * t))
    crack = one_pole_lp(white, 3500.0) * np.exp(-t / 0.06)
    mid = one_pole_lp(white, 600.0) * np.exp(-t / 0.5)
    thunder = rumble * 1.2 + crack * 0.7 + mid * 0.6
    fade = np.minimum(1.0, (n - np.arange(n)) / (RATE * 0.5))
    write_wav(os.path.join(out, "thunder.wav"), thunder * fade, 0.95)


if __name__ == "__main__":
    main()
