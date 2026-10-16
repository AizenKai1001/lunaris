"""Makes the sounds of Hollowlight by arithmetic (examples/hollowlight/sounds/*.wav, mono 22,050 Hz, 16 bit, standard library only): the buzz of a fluorescent tube, the low drone of a pump a long way off, drips
that fall in a big room, water lapping at a tile edge. Every loop holds a whole number of cycles of its parts, so it repeats without a click.

    python -I tools/make_hollowlight_sounds.py
"""
import math
import os
import random
import struct
import wave

RATE = 22050
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "hollowlight", "sounds")


def write(name, samples):
    os.makedirs(OUT, exist_ok=True)
    peak = max(1e-9, max(abs(s) for s in samples))
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s / peak * 0.8)) * 32767)) for s in samples))
    print(name, f"{len(samples) / RATE:.1f} s")


def lowpass(x, a):
    y, acc = [], 0.0
    for v in x:
        acc += a * (v - acc)
        y.append(acc)
    return y


def hum(seconds=8):
    """A tube's buzz: 100 Hz and its overtones, a slow breathing in the level, a little hiss."""
    rng = random.Random(1)
    n = RATE * seconds
    noise = lowpass([rng.uniform(-1, 1) for _ in range(n)], 0.5)
    out = []
    for i in range(n):
        t = i / RATE
        breathe = 0.85 + 0.15 * math.sin(2 * math.pi * 0.25 * t)
        s = sum(a * math.sin(2 * math.pi * f * t + k) for k, (f, a) in enumerate(((100, 1.0), (200, 0.55), (300, 0.38), (400, 0.2), (500, 0.26), (700, 0.12), (1100, 0.05))))
        out.append(breathe * s + 0.12 * noise[i])
    return out


def drone(seconds=12):
    """A pump far away: 41 Hz and 82 Hz, a swell, soft noise underneath."""
    rng = random.Random(2)
    n = RATE * seconds
    noise = lowpass([rng.uniform(-1, 1) for _ in range(n)], 0.04)
    return [(0.8 + 0.2 * math.sin(2 * math.pi * t / seconds)) * (math.sin(2 * math.pi * 41 * t) + 0.5 * math.sin(2 * math.pi * 82 * t + 1.0)) + 6 * noise[i] for i, t in enumerate(i / RATE for i in range(n))]


def drips(seconds=9):
    """Three drips at uneven times, each a short falling 'plink' with a few echoes."""
    n = RATE * seconds
    out = [0.0] * n
    for t0, f0, amp in ((0.6, 1500, 1.0), (3.9, 1100, 0.8), (6.2, 1850, 0.9)):
        for echo, (delay, gain) in enumerate(((0.0, 1.0), (0.23, 0.35), (0.51, 0.18), (0.9, 0.08))):
            start = int((t0 + delay) * RATE)
            for k in range(int(0.22 * RATE)):
                if start + k >= n:
                    break
                t = k / RATE
                f = f0 * (1.0 - 0.3 * min(t / 0.1, 1.0))
                out[start + k] += amp * gain * math.exp(-t * 28.0) * math.sin(2 * math.pi * f * t)
    return out


def lap(seconds=10):
    """Water at a tile edge: soft filtered noise that swells and falls."""
    rng = random.Random(3)
    n = RATE * seconds
    noise = lowpass([rng.uniform(-1, 1) for _ in range(n)], 0.12)
    return [noise[i] * (0.35 + 0.65 * (0.5 + 0.5 * math.sin(2 * math.pi * (i / RATE) / 5.0 + 1.0 * math.sin(2 * math.pi * (i / RATE) / 10.0)))) for i in range(n)]


def thump(seconds=3.75):
    """A very large foot coming down: a low note that falls from 38 Hz to 21 Hz, a crack of rumble, a long tail."""
    rng = random.Random(4)
    n = int(RATE * seconds)
    rumble = lowpass([rng.uniform(-1, 1) for _ in range(n)], 0.02)
    out = []
    for i in range(n):
        t = i / RATE
        f = 21 + 17 * math.exp(-t * 5.0)
        out.append(math.exp(-t * 2.2) * math.sin(2 * math.pi * f * t) * (1 - math.exp(-t * 90)) + 7 * rumble[i] * math.exp(-t * 3.0))
    return out


def wind(seconds=14):
    """Wind over open ground: soft noise that swells in gusts, a thin whistle that rises and falls, the ends crossfaded so it loops."""
    rng = random.Random(6)
    n = RATE * seconds
    ext = int(RATE * 1.0)
    noise = lowpass([rng.uniform(-1, 1) for _ in range(n + ext)], 0.06)
    hiss = lowpass([rng.uniform(-1, 1) for _ in range(n + ext)], 0.3)
    body = []
    for i in range(n + ext):
        t = i / RATE
        gust = 0.35 + 0.65 * (0.5 + 0.5 * math.sin(2 * math.pi * t / 7.0 + math.sin(2 * math.pi * t / 14.0) * 1.5))
        whistle = 0.05 * math.sin(2 * math.pi * (520 + 80 * math.sin(2 * math.pi * t / 14.0)) * t) * gust ** 2
        body.append(gust * (noise[i] * 3.0 + hiss[i] * 0.45) + whistle)
    out = body[:n]
    for i in range(ext):  # the tail past the end fades in over the start
        k = i / ext
        out[i] = out[i] * k + body[n + i] * (1 - k)
    return out


if __name__ == "__main__":
    write("hum.wav", hum())
    write("drone.wav", drone())
    write("drips.wav", drips())
    write("lap.wav", lap())
    write("thump.wav", thump())
    write("wind.wav", wind())
