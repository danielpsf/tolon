#!/usr/bin/env python3
"""Synthesise Tolon's alarm sounds. Pure standard library, no randomness, no samples.

Usage: python3 synth.py [output_dir]   (default: the folder above this script)

Both sounds are additive synthesis: a few decaying sine partials with a soft raised-cosine
attack and a short fade at the very end, so neither clicks. Mono, 22.05 kHz, 16-bit PCM.
"""
import math
import struct
import sys
import wave
from pathlib import Path

RATE = 22_050
PEAK = 0.5  # fraction of full scale; a gentle bell, not an alarm
ATTACK = 0.015  # seconds
TAIL_FADE = 0.12  # seconds, closes the decay to exact silence


def note(buf, start, freq, partials, length):
    """Add one struck note: partials are (frequency ratio, level, decay seconds)."""
    first = int(start * RATE)
    for i in range(min(int(length * RATE), len(buf) - first)):
        t = i / RATE
        env = 0.5 - 0.5 * math.cos(math.pi * min(t / ATTACK, 1.0))
        s = sum(
            level * math.exp(-t / decay) * math.sin(2 * math.pi * freq * ratio * t)
            for ratio, level, decay in partials
        )
        buf[first + i] += env * s


def render(seconds, notes):
    buf = [0.0] * int(seconds * RATE)
    for start, freq, partials, length in notes:
        note(buf, start, freq, partials, length)
    fade = int(TAIL_FADE * RATE)
    for i in range(fade):
        buf[len(buf) - 1 - i] *= i / fade
    top = max(abs(x) for x in buf)
    return [x * PEAK / top for x in buf]


# Slightly inharmonic partials, like a small bell; the fundamental rings longest.
BELL = [(1.0, 1.0, 0.55), (2.0, 0.45, 0.35), (2.76, 0.25, 0.22), (4.07, 0.12, 0.12)]
# Near-harmonic and brighter, so it reads as a chime.
CHIME = [(1.0, 1.0, 0.30), (2.0, 0.30, 0.18), (3.0, 0.10, 0.10)]

SOUNDS = {
    "soft-bell": render(1.5, [(0.0, 587.33, BELL, 1.5)]),  # D5
    "chime": render(1.0, [(0.0, 659.25, CHIME, 1.0), (0.2, 987.77, CHIME, 0.8)]),  # E5 then B5
}


def write(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", round(x * 32767)) for x in samples))


if __name__ == "__main__":
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent
    for name, samples in SOUNDS.items():
        write(out / f"{name}.wav", samples)
        print(f"{name}.wav  {len(samples) / RATE:.2f} s")
