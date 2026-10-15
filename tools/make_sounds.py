"""Write the small sound files the examples use (examples/sounds/*.wav), made from nothing but arithmetic and a fixed random seed, so
they are the same every time and need no licence:

    clap.wav    a handclap: a 25 ms burst of noise with a short low thump, then silence (a second long, so that it loops once a second
                and the reverberation of a room is heard in the gap)
    drip.wav    two water drips, a falling sine blip each
    radio.wav   a four second tune on a soft triangle wave

    python -I tools/make_sounds.py
"""
import math
import os
import random
import struct
import wave

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "examples", "sounds")
RATE = 44100


def save(name, samples):
    os.makedirs(OUT, exist_ok=True)
    peak = max(1e-9, max(abs(s) for s in samples))
    data = b"".join(struct.pack("<h", int(max(-1.0, min(1.0, s / peak * 0.8)) * 32767)) for s in samples)
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data)
    print(f"{name}: {len(samples) / RATE:.2f} s")


def clap():
    rng = random.Random(7)
    out = [0.0] * RATE
    lp = 0.0
    for i in range(int(0.12 * RATE)):
        t = i / RATE
        white = rng.uniform(-1, 1)
        lp += 0.6 * (white - lp)  # takes the harshest highs off
        out[i] = lp * math.exp(-t / 0.012) + 0.5 * math.sin(2 * math.pi * 180 * t) * math.exp(-t / 0.03)
    return out


def drip():
    out = [0.0] * int(1.6 * RATE)
    for start in (0.0, 0.9):
        for i in range(int(0.25 * RATE)):
            t = i / RATE
            f = 1500.0 - 700.0 * min(1.0, t / 0.08)
            out[int(start * RATE) + i] += math.sin(2 * math.pi * f * t) * math.exp(-t / 0.05) * min(1.0, t / 0.003)
    return out


def radio():
    notes = [261.63, 329.63, 392.0, 523.25, 392.0, 329.63, 440.0, 349.23]  # C E G C' G E A F
    out = []
    for n, f in enumerate(notes):
        length = int(0.5 * RATE)
        for i in range(length):
            t = i / RATE
            phase = (f * t) % 1.0
            tri = 4 * abs(phase - 0.5) - 1
            env = min(1.0, t / 0.01) * math.exp(-t / 0.35)
            out.append(tri * env * 0.8 + 0.2 * math.sin(2 * math.pi * f * 2 * t) * env)
    return out


if __name__ == "__main__":
    save("clap.wav", clap())
    save("drip.wav", drip())
    save("radio.wav", radio())
