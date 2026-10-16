"""Writes the sound of Caldera to examples/caldera/ from nothing but arithmetic and a fixed random seed: the same bytes every time, no licence. 16 kHz, 16 bit, mono.

  music_title.wav   slow pads over a bass, a plucked tune with an echo (the title, the end)
  music_day.wav     a light pentatonic tune over a gentle pulse (the island by day)
  music_night.wav   a low drone, far bells and a cold shimmer (the island by night)
  music_boss.wav    a driving pulse, a low saw drone and falling figures (the Caldera Warden)
  amb_waves.wav     surf: filtered noise swelling and ebbing (placed on the beaches)
  amb_fire.wav      a campfire: a low roar with crackles (placed at the fire)
  amb_lava.wav      the crater: a deep rumble with slow bubbles (placed at the lava)

    python -I tools/make_caldera_audio.py
"""
import math
import os
import random
import struct
import wave

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "caldera")
RATE = 16000
TAU = 2 * math.pi


def tri(p):
    return 4 * abs(p - math.floor(p + 0.5)) - 1


def saw(p):
    return 2 * (p - math.floor(p + 0.5))


def sq(p):
    return 1.0 if (p - math.floor(p)) < 0.5 else -1.0


def mix(buf, start, length, fn, amp, attack=0.05, release=0.3):
    n0 = int(start * RATE)
    for i in range(int(length * RATE)):
        j = (n0 + i) % len(buf)
        t = i / RATE
        e = max(0.0, min(1.0, t / attack, (length - t) / release))
        buf[j] += fn(t) * e * amp


def note(buf, start, length, freq, amp, shape="tri", attack=0.4, release=0.8, detune=(1.0, 1.004, 0.996)):
    def fn(t):
        if shape == "tri":
            return sum(tri(freq * d * t) for d in detune) / len(detune)
        if shape == "saw":
            return sum(saw(freq * d * t) for d in detune) / len(detune)
        return math.sin(TAU * freq * t)
    mix(buf, start, length, fn, amp, attack, release)


def pluck(buf, start, freq, amp, decay=0.7):
    n0 = int(start * RATE)
    for i in range(int(decay * 4 * RATE)):
        j = (n0 + i) % len(buf)
        t = i / RATE
        e = math.exp(-t / decay) * min(1.0, t / 0.004)
        buf[j] += (math.sin(TAU * freq * t) + 0.35 * math.sin(TAU * 2 * freq * t) * math.exp(-t / 0.25) + 0.12 * math.sin(TAU * 3 * freq * t)) * e * amp


def bell(buf, start, freq, amp, decay=2.0):
    n0 = int(start * RATE)
    for i in range(int(decay * 3 * RATE)):
        j = (n0 + i) % len(buf)
        t = i / RATE
        e = math.exp(-t / decay) * min(1.0, t / 0.003)
        buf[j] += (math.sin(TAU * freq * t) + 0.6 * math.sin(TAU * freq * 2.76 * t) * math.exp(-t / 0.6) + 0.35 * math.sin(TAU * freq * 5.4 * t) * math.exp(-t / 0.3)) * e * amp


def thump(buf, start, amp, f0=120.0, f1=38.0, decay=0.13):
    n0 = int(start * RATE)
    for i in range(int(0.35 * RATE)):
        j = (n0 + i) % len(buf)
        t = i / RATE
        f = f1 + (f0 - f1) * math.exp(-t / 0.03)
        buf[j] += math.sin(TAU * f * t) * math.exp(-t / decay) * amp


def noise_burst(buf, start, length, amp, rng, cutoff=0.4):
    n0 = int(start * RATE)
    y = 0.0
    for i in range(int(length * RATE)):
        j = (n0 + i) % len(buf)
        t = i / RATE
        y += cutoff * (rng.uniform(-1, 1) - y)
        buf[j] += y * math.exp(-t / (length * 0.3)) * amp


def lowpass(buf, cutoff):
    a = 1 - math.exp(-TAU * cutoff / RATE)
    out, y = [], 0.0
    for x in buf:
        y += a * (x - y)
        out.append(y)
    return out


def echo(buf, delay, feedback, mixv):
    d = int(delay * RATE)
    out = list(buf)
    for i in range(d, len(buf)):
        out[i] += out[i - d] * feedback
    return [a * (1 - mixv) + b * mixv for a, b in zip(buf, out)]


def save(name, buf, peak=0.8, fade=0.0):
    top = max(1e-9, max(abs(s) for s in buf))
    data = bytearray()
    n = len(buf)
    f = int(fade * RATE)
    for i, s in enumerate(buf):
        k = 1.0 if not f else min(1.0, i / f, (n - 1 - i) / f)
        data += struct.pack("<h", int(max(-1.0, min(1.0, s / top * peak * k)) * 32767))
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(bytes(data))
    print(f"  {name}: {n / RATE:.0f} s, {len(data) // 1024} KB")


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def music_title():
    secs = 32.0
    buf = [0.0] * int(secs * RATE)
    chords = [(50, [57, 60, 64, 69]), (46, [58, 62, 65, 69]), (53, [60, 65, 69, 72]), (48, [55, 60, 64, 67])]  # Dm, Bb, F, C over their basses
    for k, (bass, notes) in enumerate(chords):
        start = k * 8.0
        note(buf, start, 8.5, hz(bass - 12), 0.34, "tri", 0.6, 1.4)
        for n in notes:
            note(buf, start, 8.8, hz(n), 0.1, "tri", 1.4, 1.8)
    tune = [74, 72, 69, 72, 74, 77, 76, 74, 72, 70, 69, 72, 70, 67, 65, 67]
    for i, n in enumerate(tune):
        pluck(buf, 1.0 + i * 1.9, hz(n + 12 * (i % 5 == 4)), 0.16, 0.9)
    buf = echo(buf, 0.62, 0.42, 0.45)
    save("music_title.wav", buf, 0.78, 0.0)


def music_day():
    secs = 24.0
    rng = random.Random(7)
    buf = [0.0] * int(secs * RATE)
    scale = [0, 2, 4, 7, 9]  # C major pentatonic
    for b in range(0, 16):  # a soft pulse and a bass, an eighth every 1.5 s
        t0 = b * 1.5
        note(buf, t0, 1.6, hz(48 + [0, 0, 7, 5][b % 4] - 12 * (b % 4 == 3)), 0.22, "tri", 0.1, 0.8)
        if b % 2 == 0:
            thump(buf, t0, 0.07, 90, 45, 0.1)
    for b in range(0, 48):
        t0 = b * 0.5
        if rng.random() < 0.78:
            n = 72 + scale[rng.randrange(5)] + 12 * (rng.random() < 0.3)
            pluck(buf, t0, hz(n), 0.1, 0.55)
    for k in range(4):
        note(buf, k * 6.0, 6.5, hz(64 + [0, 5, 7, 2][k]), 0.08, "tri", 1.2, 1.6)
    buf = echo(buf, 0.375, 0.35, 0.35)
    save("music_day.wav", buf, 0.72, 0.0)


def music_night():
    secs = 28.0
    rng = random.Random(11)
    buf = [0.0] * int(secs * RATE)
    note(buf, 0.0, secs + 1, hz(33), 0.4, "saw", 4.0, 4.0, (1.0, 1.003, 0.997))
    note(buf, 0.0, secs + 1, hz(45), 0.18, "tri", 5.0, 5.0)
    for k in range(7):
        bell(buf, 1.5 + k * 3.7 + rng.random() * 0.6, hz([69, 67, 64, 62, 60, 71, 65][k]), 0.12, 2.4)
    for k in range(3):
        note(buf, 2.0 + k * 9.0, 8.0, hz([76, 77, 73][k]), 0.05, "sin", 3.0, 3.5)
    buf = lowpass(buf, 2200)
    buf = echo(buf, 0.9, 0.5, 0.5)
    save("music_night.wav", buf, 0.7, 0.0)


def music_boss():
    secs = 24.0
    rng = random.Random(13)
    buf = [0.0] * int(secs * RATE)
    beat = 60.0 / 128
    for b in range(int(secs / beat)):
        t0 = b * beat
        thump(buf, t0, 0.5, 130, 36, 0.16 if b % 4 == 0 else 0.1)
        if b % 2 == 1:
            noise_burst(buf, t0, 0.12, 0.12, rng, 0.5)
    for k in range(6):
        note(buf, k * 4.0, 4.4, hz([38, 38, 41, 38, 36, 34][k]), 0.34, "saw", 0.05, 0.5, (1.0, 1.006, 0.994))
    fig = [62, 61, 59, 57, 56, 55, 53, 50]
    for rep in range(3):
        for i, n in enumerate(fig):
            pluck(buf, 8.0 * rep + i * beat * 2, hz(n), 0.12, 0.35)
    buf = lowpass(buf, 3500)
    buf = echo(buf, beat * 0.75, 0.3, 0.25)
    save("music_boss.wav", buf, 0.82, 0.0)


def amb_waves():
    secs = 12.0
    rng = random.Random(3)
    n = int(secs * RATE)
    raw = [rng.uniform(-1, 1) for _ in range(n)]
    y1 = lowpass(raw, 700)
    out = []
    for i, x in enumerate(y1):
        t = i / RATE
        swell = 0.55 + 0.45 * math.sin(TAU * t / 6.0 - 1.0) ** 2 * (0.8 + 0.2 * math.sin(TAU * t / 2.3))
        out.append(x * swell)
    save("amb_waves.wav", out, 0.6, 0.15)


def amb_fire():
    secs = 8.0
    rng = random.Random(5)
    n = int(secs * RATE)
    base = lowpass([rng.uniform(-1, 1) for _ in range(n)], 260)
    out = [x * 0.7 for x in base]
    for _ in range(70):
        noise_burst(out, rng.uniform(0, secs), rng.uniform(0.01, 0.05), rng.uniform(0.2, 0.7), rng, 0.8)
    save("amb_fire.wav", out, 0.6, 0.1)


def amb_lava():
    secs = 10.0
    rng = random.Random(9)
    n = int(secs * RATE)
    base = lowpass([rng.uniform(-1, 1) for _ in range(n)], 120)
    out = [x * 1.0 for x in base]
    for k in range(8):
        thump(out, rng.uniform(0, secs), 0.5, 70, 28, 0.25)
    save("amb_lava.wav", out, 0.6, 0.2)


def main():
    os.makedirs(OUT, exist_ok=True)
    print("sound ->", os.path.relpath(OUT))
    music_title()
    music_day()
    music_night()
    music_boss()
    amb_waves()
    amb_fire()
    amb_lava()


if __name__ == "__main__":
    main()
