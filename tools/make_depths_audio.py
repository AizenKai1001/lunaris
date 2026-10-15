"""Writes the two music loops of the "Depths" example (examples/depths/music_theme.wav and music_depths.wav) from nothing but arithmetic and a fixed random seed: the same bytes
every time, no licence. 22.05 kHz, 16 bit, mono.

  music_theme.wav    the menu and the end: four slow minor chords (Am F C G) as soft pads, a bass, and a plucked melody with an echo
  music_depths.wav   the levels: a low drone, a slow double heartbeat, a thin wandering tone, far-off water drops and a draught

    python -I tools/make_depths_audio.py
"""
import math
import os
import random
import struct
import wave

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "depths")
RATE = 22050
TAU = 2 * math.pi


def tri(p):
    return 4 * abs(p - math.floor(p + 0.5)) - 1


def env_ar(t, length, attack, release):
    return max(0.0, min(1.0, t / attack, (length - t) / release))


def add_note(buf, start, length, freq, amp, shape="tri", attack=1.2, release=1.4, detune=(1.0, 1.004, 0.996)):
    n0 = int(start * RATE)
    for i in range(int(length * RATE)):
        j = n0 + i
        if j >= len(buf):
            break
        t = i / RATE
        e = env_ar(t, length, attack, release)
        if shape == "tri":
            v = sum(tri(freq * d * t) for d in detune) / len(detune)
        else:
            v = math.sin(TAU * freq * t)
        buf[j] += v * e * amp


def add_pluck(buf, start, freq, amp, decay=0.9):
    n0 = int(start * RATE)
    for i in range(int(decay * 4 * RATE)):
        j = n0 + i
        if j >= len(buf):
            break
        t = i / RATE
        e = math.exp(-t / decay) * min(1.0, t / 0.004)
        buf[j] += (math.sin(TAU * freq * t) + 0.35 * math.sin(TAU * 2 * freq * t) * math.exp(-t / 0.25) + 0.12 * math.sin(TAU * 3 * freq * t)) * e * amp


def echo(buf, delay, feedback, mix):
    d = int(delay * RATE)
    out = list(buf)
    for i in range(d, len(buf)):
        out[i] += out[i - d] * feedback
    return [a * (1 - mix) + b * mix for a, b in zip(buf, out)]


def lowpass(buf, cutoff):
    a = 1 - math.exp(-TAU * cutoff / RATE)
    out, y = [], 0.0
    for x in buf:
        y += a * (x - y)
        out.append(y)
    return out


def save(name, buf, peak=0.82):
    top = max(1e-9, max(abs(s) for s in buf))
    fade = int(0.6 * RATE)
    data = bytearray()
    for i, s in enumerate(buf):
        k = min(1.0, i / fade, (len(buf) - 1 - i) / fade)
        data += struct.pack("<h", int(max(-1.0, min(1.0, s / top * peak * k)) * 32767))
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(bytes(data))
    print(f"{name}: {len(buf) / RATE:.1f} s, {len(data) // 1024} KiB")


def theme():
    seconds = 28.8
    buf = [0.0] * int(seconds * RATE)
    chords = [([110.0, 130.81, 164.81], 55.0, [440.0, 523.25, 659.25, 523.25]), ([87.31, 110.0, 130.81], 43.65, [349.23, 440.0, 523.25, 440.0]),
              ([130.81, 164.81, 196.0], 65.41, [392.0, 523.25, 659.25, 523.25]), ([98.0, 123.47, 146.83], 49.0, [392.0, 493.88, 587.33, 493.88])]
    for k, (notes, bass, arp) in enumerate(chords):
        t0 = k * 7.2
        for f in notes:
            add_note(buf, t0, 8.4, f, 0.16)
        add_note(buf, t0, 7.0, bass, 0.34, shape="sine", attack=0.4, release=1.0)
        for b in range(8):  # an eighth-note arpeggio, one every 0.9 s
            add_pluck(buf, t0 + 0.45 + b * 0.9, arp[b % 4] * (2 if b % 4 == 3 else 1), 0.14 if b % 2 == 0 else 0.1)
    return echo(buf, 0.45, 0.42, 0.45)


def depths():
    seconds = 32.0
    n = int(seconds * RATE)
    rng = random.Random(21)
    buf = [0.0] * n
    for i in range(n):
        t = i / RATE
        drone = math.sin(TAU * 73.42 * t) * 0.5 + math.sin(TAU * 110.0 * t + 0.4 * math.sin(TAU * 0.11 * t)) * 0.28 + math.sin(TAU * 36.7 * t) * 0.25
        trem = 0.75 + 0.25 * math.sin(TAU * 0.125 * t)
        wander = math.sin(TAU * (293.66 + 25 * math.sin(TAU * 0.07 * t)) * t) * 0.07 * math.sin(math.pi * t / seconds) ** 2 * (0.6 + 0.4 * math.sin(TAU * 5.2 * t))
        buf[i] = drone * trem * 0.5 + wander
    # a slow double heartbeat every four seconds
    for beat in range(8):
        for off, amp in ((0.0, 0.7), (0.32, 0.45)):
            n0 = int((beat * 4.0 + off) * RATE)
            for i in range(int(0.5 * RATE)):
                if n0 + i < n:
                    t = i / RATE
                    buf[n0 + i] += math.sin(TAU * (62 - 30 * t) * t) * math.exp(-t / 0.09) * amp * 0.55
    # water far away
    for _ in range(9):
        start = rng.uniform(0.5, seconds - 2.5)
        base = rng.uniform(900, 1700)
        n0 = int(start * RATE)
        for i in range(int(0.3 * RATE)):
            t = i / RATE
            if n0 + i < n:
                buf[n0 + i] += math.sin(TAU * (base - 700 * min(1.0, t / 0.09)) * t) * math.exp(-t / 0.05) * 0.13
    buf = echo(buf, 0.62, 0.5, 0.5)
    # a draught: noise through a low-pass, swelling and fading
    wind = lowpass([rng.uniform(-1, 1) for _ in range(n)], 420)
    return [b + w * 0.9 * (0.4 + 0.6 * math.sin(TAU * 0.09 * i / RATE + 1) ** 2) for i, (b, w) in enumerate(zip(buf, wind))]


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    save("music_theme.wav", theme())
    save("music_depths.wav", depths())
