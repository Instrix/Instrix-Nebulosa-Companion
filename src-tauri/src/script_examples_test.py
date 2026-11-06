# The trigger script examples (ui/src/lib/script-examples) on synthetic data:
# each must find what it says, and nothing else. Run by the Rust test
# examples_find_what_they_say (script.rs), or by hand:
#
#   python3 src-tauri/src/script_examples_test.py
#
# Each example runs as the app runs it (the runner's Data, Runs and events),
# its samples fed in uneven blocks, so state across blocks is tested too.
import math
import os
import struct
import sys
import tempfile

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
EXAMPLES = os.path.join(HERE, "..", "..", "ui", "src", "lib", "script-examples")

# the runner as a module (it swaps stdout for stderr: put it back)
_out = sys.stdout
runner = {"__name__": "runner"}
exec(compile(open(os.path.join(HERE, "script_runner.py")).read(), "script_runner.py", "exec"), runner)
sys.stdout = _out


def block(dt, t0, i, v, m=None):
    """nbp-core trigger::encode_block"""
    n = len(i)
    b = struct.pack("<IIId", n, 1, 1 if m is not None else 0, dt) + struct.pack("<dI", t0, n)
    for a in (i, v) + ((m,) if m is not None else ()):
        b += np.asarray(a, dtype="<f4").tobytes()
    return b


def run(name, dt, i, v, m=None):
    """the example `name` on the samples (dt s apart from 0): what it found, [(t, label)]"""
    path = [f for f in os.listdir(EXAMPLES) if f.endswith(f"-{name}.py")]
    assert len(path) == 1, name
    ns = {"__name__": "nebulosa_trigger", "np": np, "math": math, "Runs": runner["Runs"]}
    exec(compile(open(os.path.join(EXAMPLES, path[0])).read(), "<trigger script>", "exec"), ns)
    i, v = np.asarray(i, float), np.asarray(v, float)
    found, a, k = [], 0, 0
    sizes = [7777, 1, 3001, 12345, 50000]
    while a < len(i):
        b = min(len(i), a + sizes[k % len(sizes)])
        d = runner["Data"](block(dt, a * dt, i[a:b], v[a:b], None if m is None else m[a:b]))
        for e in runner["events"](ns["trigger"](d)):
            found.append((e["t"], e.get("label")))
        a, k = b, k + 1
    return found


rng = np.random.default_rng(1)
noise = lambda n, s: rng.normal(0, s, n)
flat = lambda n, x: np.full(n, float(x))


def bursts(n, dt, starts, widths, level, base=50e-6, rise=10e-6, heights=None):
    """bursts at `starts` (s) of `widths` (s), each `level` A (or its own height), on `base`"""
    t = np.arange(n) * dt
    i = flat(n, base)
    for k, (s, w) in enumerate(zip(starts, widths)):
        h = heights[k] if heights is not None else level
        on = (t >= s) & (t < s + w)
        i[on] = base + (h - base) * np.minimum(1, (t[on] - s) / rise)
    return i


FAILS = []


def check(name, got, want, what=""):
    ok = want(got) if callable(want) else got == want
    print(f"{'ok  ' if ok else 'FAIL'} {name}: {len(got)} found {[l for _, l in got][:6]} {what}")
    if not ok:
        FAILS.append(name)


dt = 1e-5  # 100 kS/s unless a test needs finer

# ---- Basics -------------------------------------------------------------------
i = []
for _ in range(5):
    i += [0.0] * 500 + [0.030] * 300 + [0.019, 0.021] * 100
check("edge", run("edge", dt, i, flat(len(i), 3.3)), lambda g: len(g) == 5)

# a 3 mA plateau for 20 ms (it stays: found), and fast edges through the band (not)
n = 20000
i = bursts(n, dt, [0.02, 0.06, 0.1], [0.002, 0.02, 0.002], 0.02, rise=50e-6, heights=[0.02, 0.003, 0.02])
check("window", run("window", dt, i, flat(n, 3.3)), lambda g: len(g) == 1 and g[0][1].startswith("3.00 mA"))

# widths 2 ms (fine), 0.5 ms (short), 6 ms (long)
n = 40000
i = bursts(n, dt, [0.05, 0.15, 0.25, 0.35], [2e-3, 0.5e-3, 6e-3, 2e-3], 0.02)
g = run("width", dt, i, flat(n, 3.3))
check("width", g, lambda g: [l.split(":")[0] for _, l in g] == ["short", "long"])

# math: 3 times over 50 (in the math channel's unit), once more with no math channel (mW)
n = 30000
m = bursts(n, dt, [0.05, 0.12, 0.2], [1e-3, 1e-3, 1e-3], 80.0, base=10.0)
check("math", run("math", dt, flat(n, 1e-3), flat(n, 3.3), m), lambda g: len(g) == 3)
check("math (no channel: mW)", run("math", dt, bursts(n, dt, [0.1], [1e-3], 0.03), flat(n, 3.3)), lambda g: len(g) == 1)

# ---- Bursts -------------------------------------------------------------------
i = []
for w in [200, 50, 200, 50, 200, 50]:
    i += [0.0] * 1000 + [0.020] * w
i += [0.0] * 100
g = run("charge", dt, i, flat(len(i), 3.3))
check("charge", g, lambda g: len(g) == 3 and g[0][1].startswith("40.0 µC"))

i = []
for _ in range(4):
    for _ in range(3):
        i += [0.010] * 40 + [0.0] * 110
    i += [0.0] * (10000 - 450)
check("events", run("events", dt, i, flat(len(i), 3.3)), lambda g: [l for _, l in g] == ["3 bursts, 12.0 µC"] * 4)

# every 100 ms, and one burst 10 ms after another
n = 60000
st = [0.05 + 0.1 * k for k in range(5)] + [0.16]
check("double", run("double", dt, bursts(n, dt, sorted(st), [1e-3] * 6, 0.02), flat(n, 3.3)), lambda g: len(g) == 1 and g[0][1].startswith("10.0 ms"))

# 5 a second, then for a second 20 a second
dt4 = 1e-4
st = [0.1 + 0.2 * k for k in range(15)] + [3.05 + 0.05 * k for k in range(20)] + [4.2 + 0.2 * k for k in range(5)]
n = int(5.5 / dt4)
check("rate", run("rate", dt4, bursts(n, dt4, st, [2e-3] * len(st), 0.02, rise=1e-4), flat(n, 3.3)), lambda g: len(g) == 1 and g[0][1].startswith("11 in"))

# the same trapezoid every 100 ms; one with a dip in its middle
n = 100000
t = np.arange(n) * dt
i = flat(n, 50e-6)
for k in range(10):
    s = 0.02 + 0.1 * k
    for a, b, h in [(s, s + 2e-3, 0.02)]:
        on = (t >= a) & (t < b)
        x = (t[on] - a) / 2e-3
        i[on] = h * np.minimum(1, np.minimum(x, 1 - x) * 8)
    if k == 6:
        mid = (t >= s + 0.7e-3) & (t < s + 1.3e-3)
        i[mid] = 0.006
i += noise(n, 1e-4)
check("shape", run("shape", dt, i, flat(n, 3.3)), lambda g: len(g) == 1 and abs(g[0][0] - 0.62) < 0.002)

# charge: 30 bursts, 1 % more each after the 15th
n = int(3.2 / dt4)
st = [0.05 + 0.1 * k for k in range(30)]
w = [2e-3 * (1 + 0.04 * max(0, k - 15)) for k in range(30)]
check("trend", run("trend", dt4, bursts(n, dt4, st, w, 0.02, rise=1e-4), flat(n, 3.3)), lambda g: len(g) == 1)


def radio(seconds, dt):
    """the demo's radio scenario (demoGen.ts radioPeriod), without its noise"""
    t = np.arange(int(seconds / dt)) * dt
    i = flat(len(t), 50e-6)
    v = 3.15 + 0.12 * np.sin(2 * np.pi * t / 20)
    for k in range(int(seconds / 0.1)):
        runt = k % 10 == 8
        start = 0.04 if k % 10 == 6 else 0.01
        width = 0.6e-3 if runt else (6e-3 if k % 10 == 3 else 2e-3)
        rise = 1.5e-3 if (not runt and k % 7 == 2) else 30e-6
        top = 5e-3 if runt else 15e-3
        s = t - k * 0.1 - start
        on = (s >= 0) & (s < width)
        i[on] = 50e-6 + (top - 50e-6) * np.minimum(1, s[on] / rise) * np.minimum(1, (width - s[on]) / 30e-6)
        if not runt and k % 13 == 5:
            d = (s >= 1e-3) & (s < 1.8e-3)
            v[d] -= 0.4 * np.sin(np.pi * (s[d] - 1e-3) / 0.8e-3)
    # the demo's noise: 3 % on the current while awake (a slow start dithers around the levels)
    i = i * (1 + np.where(i > 1e-3, rng.uniform(-0.015, 0.015, len(i)), rng.uniform(-0.075, 0.075, len(i))))
    return i, v - i * 2


# at 1 MS/s, as the device: a slow start's ramp moves less per sample than its noise
i, v = radio(2.6, 1e-6)
g = run("odd", 1e-6, i, v)
labels = [l for _, l in g]
# periods 0..25: retries 3, 13, 23; aborted 8, 18; slow starts 2, 9, 16 (23 is a retry: too long a
# burst for its ramp to count); a dip at 5 (18 is aborted); late 6, 16, so early 7, 17; one missing before 9, 19
want = {"retry": 3, "aborted": 2, "slow start": 3, "dip": 1, "late": 2, "early": 2, "missing": 2}
counts = {k: sum(k in l for l in labels) for k in want}
check("odd", g, lambda g: counts == want, counts)

# ---- Supply -------------------------------------------------------------------
v = []
for w in [10, 2, 10, 2]:
    v += [3.3] * 3000 + [2.8] * w
v += [3.3] * 100
check("brownout", run("brownout", dt, flat(len(v), 0.001), v), lambda g: len(g) == 2)

# 20 mA bursts drop it 40 mV (2 Ω); one drops it 100 mV (5 Ω)
n = 60000
st = [0.05 + 0.1 * k for k in range(5)]
i = bursts(n, dt, st, [2e-3] * 5, 0.02)
v = 3.3 - i * 2
on3 = (np.arange(n) * dt >= st[3]) & (np.arange(n) * dt < st[3] + 2e-3)
v[on3] = 3.3 - i[on3] * 5
check("droop", run("droop", dt, i, v), lambda g: len(g) == 1 and g[0][1].startswith("5.0 Ω"))

# off for 0.8 s every 3 s; a 0.3 ms dip (not a cycle)
dt3 = 1e-4
n = int(7 / dt3)
t = np.arange(n) * dt3
v = np.where((t % 3) < 0.8, 0.0, 3.3)
v[(t > 2.0) & (t < 2.0003)] = 0.2
i = np.where(v > 2, 0.01, 0.0)
# off from 0, 3 and 6 s for 0.8 s: three power-ons (the dip too short to count)
check("powercycle", run("powercycle", dt3, i, v), lambda g: [round(x, 1) for x, _ in g] == [0.8, 3.8, 6.8] and g[0][1] == "on after 800 ms off")

# power-on with a 0.5 A inrush decaying in 0.4 ms; a second with only 0.1 A
n = 100000
t = np.arange(n) * dt
v = np.where(((t > 0.1) & (t < 0.5)) | (t > 0.7), 3.3, 0.0)
i = np.where(v > 0, 0.006, 0.0)
i += np.where((t > 0.1) & (t < 0.5), 0.5 * np.exp(-np.clip(t - 0.1, 0, None) / 0.4e-3), 0)
i += np.where(t > 0.7, 0.1 * np.exp(-np.clip(t - 0.7, 0, None) / 0.4e-3), 0)
check("inrush", run("inrush", dt, i, v), lambda g: len(g) == 1 and abs(g[0][0] - 0.1) < 1e-3)

# ---- Timing -------------------------------------------------------------------
i = []
for gap in [1000, 1000, 1000, 1000, 1000, 2500, 1000, 1000, 4000]:
    i += [0.020] * 10 + [0.0] * (gap - 10)
g = run("heartbeat", dt4, i, flat(len(i), 3.3))
check("heartbeat", g, lambda g: [l.split(":")[0] for _, l in g] == ["late", "missed"])

# every 100 ms ± 1 ms; one 20 ms late (and so the next 20 ms early)
n = int(2.2 / dt4)
st = [0.05 + 0.1 * k + (0.02 if k == 12 else 0) + 0.001 * ((k * 7) % 3 - 1) for k in range(20)]
check("jitter", run("jitter", dt4, bursts(n, dt4, st, [2e-3] * 20, 0.02, rise=1e-4), flat(n, 3.3)), lambda g: len(g) == 2)

# PWM at 2 kHz, then 2.5 kHz for 0.2 s, then 2 kHz
dt6 = 2e-6
n = int(0.6 / dt6)
t = np.arange(n) * dt6
f = np.where((t > 0.2) & (t < 0.4), 2500, 2000)
ph = np.cumsum(f * dt6) % 1
i = np.where(ph < 0.3, 0.1, 0.002)
check("frequency", run("frequency", dt6, i, flat(n, 5.0)), lambda g: len(g) == 2 and "(out)" in g[0][1])
d = np.where((t > 0.2) & (t < 0.4), 0.5, 0.3)
i = np.where(np.cumsum(np.full(n, 2000 * dt6)) % 1 < d, 0.1, 0.002)
check("duty", run("duty", dt6, i, flat(n, 5.0)), lambda g: len(g) == 2 and g[0][1].startswith("duty 50"))

i, v = radio(2.6, dt)
check("sequence", run("sequence", dt, i, v), lambda g: len(g) == 1 and abs(g[0][0] - 0.51) < 1e-3)  # period 5's dip

# ---- Energy -------------------------------------------------------------------
# 1 mW, then 20 mW for 2 s, then 1 mW: over a 10 mW budget once
n = int(6 / dt4)
t = np.arange(n) * dt4
i = np.where((t > 2) & (t < 4), 0.02 / 3.3 * 3.3 / 1, 0.001 / 3.3 * 1)
p_v = flat(n, 1.0)
check("budget", run("budget", dt4, i, p_v), lambda g: len(g) == 1 and 2.4 < g[0][0] < 3.1)
# 10 mJ seconds, one 40 mJ
i = np.where((t >= 3) & (t < 4), 0.04, 0.01)
check("energy", run("energy", dt4, i, p_v), lambda g: [x for x, _ in g] == [3.0])

# asleep at 5 µA, one sleep at 80 µA
n = int(1.0 / dt4)
st = [0.05 + 0.1 * k for k in range(10)]
i = bursts(n, dt4, st, [2e-3] * 10, 0.02, base=5e-6, rise=1e-4)
t = np.arange(n) * dt4
i[(t > 0.452) & (t < 0.55)] = 80e-6
check("leak", run("leak", dt4, i, flat(n, 3.3)), lambda g: len(g) == 1 and g[0][1] == "asleep at 80 µA")

# ---- Signal quality -------------------------------------------------------------------
i = 0.005 + 1e-4 * ((np.arange(30000) * 7919) % 101 / 50.0 - 1.0)
for k in [5000, 15000, 25000]:
    i[k] += 0.005
check("spike", run("spike", dt, i, flat(len(i), 3.3)), lambda g: len(g) == 3)

i = []
for up in [1000, 10, 1000, 10]:
    i += [0.0] * 500 + list(0.030 * np.arange(up) / up) + [0.030] * 200 + list(0.030 * (1 - np.arange(1000) / 1000))
check("slew", run("slew", dt, i, flat(len(i), 3.3)), lambda g: len(g) == 2)

n = int(1.0 / dt)
t = np.arange(n) * dt
v = 3.3 + noise(n, 1e-4) + np.where((t > 0.5) & (t < 0.7), noise(n, 1e-3), 0)
check("noise", run("noise", dt, flat(n, 0.001), v), lambda g: len(g) == 2 and "usually" in g[0][1])

v = np.array([3.3 + 0.005 * math.sin(2 * math.pi * (200e3 if 30000 <= k < 60000 else 10e3) * k * 1e-6) for k in range(90000)])
g = run("ripple", 1e-6, flat(len(v), 0.001), v)
check("ripple", g, lambda g: len(g) == 2 and "(out)" in g[0][1] and "(out)" not in g[1][1])

# ---- Logging -------------------------------------------------------------------
n = int(25 / 1e-3)
check("report", run("report", 1e-3, flat(n, 0.002), flat(n, 3.3)), lambda g: len(g) == 2 and g[0][1].startswith("mean 2.00 mA"))

with tempfile.TemporaryDirectory() as d:
    os.environ["NEBULOSA_BURSTS_CSV"] = os.path.join(d, "b.csv")
    n = 60000
    g = run("csv", dt, bursts(n, dt, [0.05 + 0.1 * k for k in range(5)], [2e-3] * 5, 0.02), flat(n, 3.3))
    rows = open(os.environ["NEBULOSA_BURSTS_CSV"]).read().strip().split("\n")
    check("csv", g, lambda g: g == [] and len(rows) == 6, f"{len(rows) - 1} rows")

# ---- every example tested ------------------------------------------------------------
tested = {"edge", "window", "width", "math", "charge", "events", "double", "rate", "shape", "trend", "odd", "brownout", "droop", "powercycle", "inrush", "heartbeat", "jitter", "frequency", "duty", "sequence", "budget", "energy", "leak", "spike", "slew", "noise", "ripple", "report", "csv"}
names = {f.split("-", 1)[1][:-3] for f in os.listdir(EXAMPLES) if f.endswith(".py")}
if names - tested:
    print("FAIL untested:", sorted(names - tested))
    FAILS.append("untested")
print("FAILED:", FAILS if FAILS else "none")
sys.exit(1 if FAILS else 0)
