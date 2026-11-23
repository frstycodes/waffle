# Synthesises Waffle's sound cues into assets/sounds/*.mp3 (needs `lame`): python3 tools/sound_cues.py [names…]
# Each plays Discord's own notes on Discord's timing (measured from its cues), so they read the same,
# in Waffle's own voice: every note glides into its pitch (a soft "bloop"), left and right sit a few
# cents apart, and a small room rings after.
import math, os, struct, subprocess, tempfile, wave

RATE = 48000
NAMES = {"C": 0, "C#": 1, "D": 2, "D#": 3, "E": 4, "F": 5, "F#": 6, "G": 7, "G#": 8, "A": 9, "A#": 10, "B": 11}

RINGING_STEP = 0.165
RINGING_NOTES, RINGING_ONSETS = [], []
for bar, (lead, melody) in enumerate([(None, "D5+D6 C4+C5 G4+G5 D5+D6 C4+C5"), (None, "D5+D6 C4+C5 G4+G5 D5+D6 C4+C5 G4+G5"),
                                      ("E5+E6", "B4+B5 C4+C5 G4+G5 B4+B5 C4+C5"), ("E5+E6", "B4+B5 C4+C5 G4+G5 B4+B5 C4+C5 G4+G5")]):
    start = bar * 8 * RINGING_STEP
    RINGING_NOTES.append("C3~" + (f"+{lead}" if lead else ""))
    RINGING_ONSETS.append(start)
    for i, chord in enumerate(melody.split()):
        RINGING_NOTES.append(chord)
        RINGING_ONSETS.append(start + (i + 1) * RINGING_STEP)
    if len(melody.split()) == 5:
        RINGING_NOTES.append("G3~")
        RINGING_ONSETS.append(start + 6 * RINGING_STEP)

# name: (notes, `~` held 8x longer through what follows, `+` joining ones struck together, onset of each in seconds, seconds until it fades out (where Discord's does), glide direction, peak, brightness)
CUES = {
    # The toggles are open fifths stepping a fourth at a time, each cut off by the next, as Discord's are.
    "mute":               (["C5+G5", "C4+G4"], [0, 0.1], 0.38, 0, 0.42, 0.08),
    "unmute":             (["C4+G4", "G4+D5", "C5+G5"], [0, 0.095, 0.17], 0.44, 0, 0.42, 0.08),
    "deafen":             (["G4+D5", "C4+G4", "G3+D4", "C3+G3"], [0, 0.09, 0.17, 0.245], 0.56, 0, 0.42, 0.10),
    "undeafen":           (["C3+G3", "G3+D4", "C4+G4", "G4+D5"], [0, 0.095, 0.155, 0.235], 0.56, 0, 0.42, 0.10),
    "user_join":          (["C4", "G4"], [0, 0.075], 0.48, +1, 0.45, 0.22),
    "user_leave":         (["D4", "G3"], [0, 0.08], 0.46, -1, 0.45, 0.18),
    "user_moved":         (["C4", "F4", "G4", "C5"], [0, 0.06, 0.13, 0.195], 0.52, +1, 0.45, 0.20),
    "disconnect":         (["F4", "C5", "F3", "F4"], [0, 0.125, 0.215, 0.215], 0.64, -1, 0.45, 0.18),
    "stream_started":     (["D3", "B3", "A3", "E4", "A4", "E5"], [0, 0.06, 0.15, 0.2, 0.27, 0.36], 0.70, +1, 0.5, 0.22),
    "stream_ended":       (["E4", "A3", "B3", "D3"], [0, 0.05, 0.1, 0.2], 0.56, -1, 0.5, 0.18),
    "stream_user_joined": (["D4", "D3", "G3", "A3"], [0, 0.075, 0.17, 0.26], 0.58, +1, 0.45, 0.18),
    "stream_user_left":   (["A3", "A2", "D3"], [0, 0.1, 0.18], 0.50, -1, 0.45, 0.16),
    "message":            (["G4", "G5"], [0, 0.085], 0.26, +1, 0.36, 0.25),
    "mention":            (["D5", "D6"], [0, 0.08], 0.32, +1, 0.42, 0.30),
    # Activities: a low swell under a rising A minor arpeggio to launch; C-E falling to the octave below to end;
    # a bright C-E-G for someone joining your game, C dropping an octave for someone leaving it.
    "activity_launch":    (["A2+E3", "A4", "C5", "G5"], [0, 0.10, 0.17, 0.24], 0.62, +1, 0.45, 0.20),
    "activity_end":       (["C5", "E5", "C4"], [0, 0.11, 0.22], 0.52, -1, 0.45, 0.18),
    "activity_user_join": (["C5", "E5", "G5"], [0, 0.08, 0.2], 0.42, +1, 0.40, 0.22),
    "activity_user_left": (["C5", "C4"], [0, 0.08], 0.38, -1, 0.40, 0.18),
    "ptt_start":          (["G4"], [0], 0.08, +1, 0.40, 0.15),
    "ptt_stop":           (["G4", "F#3"], [0, 0.03], 0.11, -1, 0.40, 0.15),
    # Ringtones, looped while a call rings: octave-stacked chords falling C, C, G twice, out; a plucked
    # two-bar melody over a held C3/G3 bass, in.
    "call_calling":       (["C4+C5+C6", "C3+C4", "G3+G4+G5", "C4+C5+C6", "C3+C4", "G3+G4+G5"],
                           [0, 0.17, 0.3, 0.68, 0.85, 0.98], 1.45, 0, 0.45, 0.12),
    "call_ringing":       (RINGING_NOTES, RINGING_ONSETS, 5.3, 0, 0.45, 0.18),
}
# Loops play back to back, so their files run exactly one period, silence included.
LOOPS = {"call_calling": 2.42, "call_ringing": 5.33}
# A small, warm stereo room after the notes, like Discord's: Freeverb's combs and allpasses, shrunk.
ROOM_RT60 = 1.0     # seconds for the tail to fall 60 dB
ROOM_WET = 0.32     # tail level against the notes, for cues 0.5 s and longer
ROOM_DAMP = 0.35    # how much each pass through the room darkens it
ROOM_PREDELAY = 0.012
TAIL = 0.6          # most seconds of tail kept after the notes fade (1.5x the notes at most), cut under -46 dB
UNDER = 0.35    # a chord keeps ringing this loud (-9 dB) under the next, as Discord's do
DETUNE = 4      # cents each channel sits off pitch
GLIDE = 0.022   # each note slides into pitch over this long...
BEND = 0.06     # ...from this far off (a fraction of the pitch).


def hz(note):
    midi = NAMES[note[:-1]] + 12 * (int(note[-1]) + 1)
    return 440 * 2 ** ((midi - 69) / 12)


def render(notes, onsets, length, glide, peak, bright, period=None):
    tail = int(min(TAIL, length * 1.5) * RATE)
    # Short blips get less room, as Discord's do: a mute click shouldn't hang in the air.
    wet = ROOM_WET * min(1, length / 0.5)
    left = room(play(notes, onsets, length, glide, bright, DETUNE) + [0.0] * tail, 0, wet)
    right = room(play(notes, onsets, length, glide, bright, -DETUNE) + [0.0] * tail, 23, wet)
    top = max(max(abs(v) for v in left), max(abs(v) for v in right))
    last = max(n for n in range(len(left)) if max(abs(left[n]), abs(right[n])) > top * 0.005)
    fade = int(0.03 * RATE)
    gain = [peak / top * min(1, (last - n) / fade) for n in range(last)]
    samples = [(left[n] * g, right[n] * g) for n, g in enumerate(gain)]
    if period is not None:
        samples = (samples + [(0.0, 0.0)] * int(period * RATE))[:int(period * RATE)]
    return samples


# One channel's notes, `cents` off pitch so left and right beat slowly against each other into width.
def play(notes, onsets, length, glide, bright, cents):
    # The last note rings 1.6x the first and fades to -40 dB (4.6 time constants) right at `length`.
    decay = (length - onsets[-1]) / 4.6 / 1.6
    out = [0.0] * int(length * RATE)
    # A cue of chords plays them legato: each drops to `UNDER`, over 12 ms, as the next one strikes.
    legato = any("+" in chord for chord in notes)
    for i, chord in enumerate(notes):
        # Later notes ring a little longer, so a phrase ends on its last note.
        tau = decay * (1 + 0.6 * i / max(1, len(notes) - 1))
        cut = int((onsets[i + 1] - onsets[i]) * RATE) if legato and i + 1 < len(notes) else None
        for note in chord.split("+"):
            held = note.endswith("~")
            strike(out, hz(note.rstrip("~")) * 2 ** (cents / 1200), int(onsets[i] * RATE), tau * (8 if held else 1), glide, bright, None if held else cut, 1 / len(chord.split("+")))
    return out


def strike(out, f, start, tau, glide, bright, cut, level):
    phase, fade = 0.0, int(0.012 * RATE)
    for n in range(int(tau * 5 * RATE)):
        t = n / RATE
        bend = 1 - glide * BEND * math.exp(-t / GLIDE * 3)
        phase += 2 * math.pi * f * bend / RATE
        env = min(1, t / 0.004) * math.exp(-t / tau)
        tone = math.sin(phase) + bright * math.exp(-t / (tau * 0.35)) * math.sin(2 * phase) \
            + 0.35 * bright * math.exp(-t / (tau * 0.2)) * math.sin(3 * phase)
        if cut is not None:
            env *= max(UNDER, min(1, (cut + fade - n) / fade))
        if start + n < len(out):
            out[start + n] += env * tone * level


# One channel of the room: the dry notes plus a damped comb-and-allpass tail. `spread` offsets the
# delay lines between channels so left and right decorrelate into width.
def room(dry, spread, wet_level):
    scale, pre = RATE / 44100 * 0.7, int(ROOM_PREDELAY * RATE)
    wet = [0.0] * len(dry)
    for size in (1116, 1188, 1277, 1356):
        d = int((size + spread) * scale)
        g, buf, lp = 10 ** (-3 * d / (ROOM_RT60 * RATE)), [0.0] * d, 0.0
        for n in range(len(dry)):
            y = buf[n % d]
            lp += (1 - ROOM_DAMP) * (y - lp)
            buf[n % d] = (dry[n - pre] if n >= pre else 0.0) + lp * g
            wet[n] += y / 4
    for size in (556, 441):
        d = int((size + spread) * scale)
        buf = [0.0] * d
        for n in range(len(wet)):
            y = buf[n % d]
            buf[n % d] = wet[n] + y * 0.5
            wet[n] = y - wet[n] * 0.5
    return [v + w * wet_level for v, w in zip(dry, wet)]


# Names on the command line render only those cues (`python3 tools/sound_cues.py activity_launch`).
def main():
    import sys
    only = set(sys.argv[1:])
    root = os.path.join(os.path.dirname(__file__), "..", "assets", "sounds")
    os.makedirs(root, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for name, spec in CUES.items():
            if only and name not in only:
                continue
            samples = render(*spec, period=LOOPS.get(name))
            path = os.path.join(tmp, name + ".wav")
            with wave.open(path, "wb") as w:
                w.setnchannels(2)
                w.setsampwidth(2)
                w.setframerate(RATE)
                w.writeframes(b"".join(struct.pack("<hh", int(l * 32767), int(r * 32767)) for l, r in samples))
            subprocess.run(["lame", "--quiet", "-V", "4", path, os.path.join(root, name + ".mp3")], check=True)
            print(f"{name}: {len(samples) / RATE * 1000:.0f}ms")


if __name__ == "__main__":
    main()
