# The script trigger's runner: the desktop app starts it with the user's
# Python and talks to it over stdin / stdout.
#
#   app → {"code": "..."}                 the script; it must define trigger(data)
#   app ← {"ok": true, "python": "3.12.1", "numpy": "2.1.0"}   or {"ok": false, "error": "..."}
#   app → {"block": N} + N bytes          new samples (nbp-core trigger::encode_block)
#   app ← {"events": [{"t": .., "label": ..}, ...], "out": "..."}   or {"error": "...", "out": "..."}
#   app → {"reset": true}                 the trigger starts over: reset() if the script has one
#   app ← {"ok": true}
#
# The script's module stays loaded between blocks: its globals keep state
# (the last sample of the previous block, a running count...). Its namespace
# has np (numpy, if installed), math and Runs (stretches where a condition
# holds, followed across blocks).
import contextlib
import io
import json
import math
import platform
import struct
import sys
import traceback
from array import array

proto = sys.stdout
stdin = sys.stdin.buffer
# a stray print outside trigger() must not break the protocol
sys.stdout = sys.stderr

try:
    import numpy as np
except Exception:  # numpy is optional: arrays of the array module then
    np = None


def reply(obj):
    proto.write(json.dumps(obj, allow_nan=False, default=str) + "\n")
    proto.flush()


def tb():
    """the traceback, from the script's own frames on"""
    lines = traceback.format_exc().splitlines()
    for k, line in enumerate(lines):
        if '"<trigger script>"' in line:
            return "Traceback (most recent call last):\n" + "\n".join(lines[k:])
    return "\n".join(lines)


class Data:
    """New samples since the last call, in time order:
    t (s), i (A), v (V), p (W), m (the math channel; None without one) as
    numpy arrays (array.array without numpy); dt (s per sample);
    runs: [(t0, count)], stretches without a gap (one, mostly)."""

    def __init__(self, b):
        n, nruns, has_m = struct.unpack_from("<III", b, 0)
        (self.dt,) = struct.unpack_from("<d", b, 12)
        off = 20
        self.runs = []
        for _ in range(nruns):
            t0, c = struct.unpack_from("<dI", b, off)
            self.runs.append((t0, c))
            off += 12

        def f32():
            nonlocal off
            a = b[off : off + 4 * n]
            off += 4 * n
            if np is not None:
                return np.frombuffer(a, dtype="<f4").astype(float)
            x = array("f")
            x.frombytes(a)
            if sys.byteorder != "little":
                x.byteswap()
            return x

        self.i = f32()
        self.v = f32()
        self.m = f32() if has_m else None
        if np is not None:
            self.p = self.i * self.v
            self.t = np.concatenate([t0 + np.arange(c) * self.dt for t0, c in self.runs]) if self.runs else np.zeros(0)
        else:
            self.p = array("d", (a * b for a, b in zip(self.i, self.v)))
            self.t = array("d", (t0 + k * self.dt for t0, c in self.runs for k in range(c)))

    def __len__(self):
        return len(self.t)

    def __repr__(self):
        return f"Data({len(self)} samples from {self.t[0] if len(self) else 0:.6f} s)"


class Run:
    """a stretch where the condition held: t0 (s, its first sample), t1 (s, the first
    sample after it), width (s), n (samples); per signal its mean, min and max
    (i_mean, i_min, i_max, v_..., p_...); charge (C, the integral of i) and energy (J)"""

    def __init__(self, t0, dt):
        self.t0, self.t1, self.dt, self.n = t0, t0, dt, 0
        self.si = self.sv = self.sp = 0.0
        self.i_min = self.v_min = self.p_min = math.inf
        self.i_max = self.v_max = self.p_max = -math.inf

    def add(self, i, v, p):
        if not len(i):
            return
        self.n += len(i)
        self.si += float(i.sum())
        self.sv += float(v.sum())
        self.sp += float(p.sum())
        self.i_min, self.i_max = min(self.i_min, float(i.min())), max(self.i_max, float(i.max()))
        self.v_min, self.v_max = min(self.v_min, float(v.min())), max(self.v_max, float(v.max()))
        self.p_min, self.p_max = min(self.p_min, float(p.min())), max(self.p_max, float(p.max()))

    width = property(lambda r: r.t1 - r.t0)
    charge = property(lambda r: r.si * r.dt)
    energy = property(lambda r: r.sp * r.dt)
    i_mean = property(lambda r: r.si / r.n if r.n else math.nan)
    v_mean = property(lambda r: r.sv / r.n if r.n else math.nan)
    p_mean = property(lambda r: r.sp / r.n if r.n else math.nan)

    def __repr__(self):
        return f"Run({self.t0:.6f} s, {self.width * 1e3:.3f} ms, {self.n} samples)"


class Runs:
    """Stretches where a condition holds, followed across blocks:
        bursts = Runs()
        for r in bursts.feed(data, data.i > 0.005): ...   # the ones that ended in this block
    `current` is the one still going (or None); reset() forgets it. Needs numpy."""

    def __init__(self):
        self.current = None

    def reset(self):
        self.current = None

    def feed(self, data, mask):
        if np is None:
            raise RuntimeError("Runs needs numpy: pip install numpy")
        mask = np.asarray(mask, dtype=bool)
        n = len(mask)
        if n == 0:
            return []
        if n != len(data):
            raise ValueError(f"the condition has {n} values, the block {len(data)} samples")
        x = mask.astype(np.int8)
        d = np.diff(np.concatenate(([1 if self.current is not None else 0], x, [0])))
        starts = list(np.flatnonzero(d == 1))
        ends = list(np.flatnonzero(d == -1))
        segs = []
        if self.current is not None:
            segs.append((self.current, 0, ends.pop(0)))
        for a, b in zip(starts, ends):
            segs.append((None, a, b))
        out = []
        self.current = None
        for run, a, b in segs:
            if run is None:
                run = Run(float(data.t[a]), data.dt)
            run.add(data.i[a:b], data.v[a:b], data.p[a:b])
            if b == n and mask[-1]:
                run.t1 = float(data.t[-1]) + data.dt
                self.current = run  # still going
            else:
                run.t1 = float(data.t[b]) if b < n else float(data.t[-1]) + data.dt
                out.append(run)
        return out


def events(r):
    """what trigger() returned, as events: None, a time, or a list of times,
    (time, label) pairs or dicts {t, label, width, peak, mean, integral}"""
    if r is None:
        return []
    if isinstance(r, (int, float)) or (np is not None and np.isscalar(r)):
        r = [r]
    out = []
    for e in r:
        if isinstance(e, dict):
            d = {k: e[k] for k in ("t", "label", "width", "peak", "mean", "integral") if e.get(k) is not None}
        elif isinstance(e, (tuple, list)):
            d = {"t": e[0]}
            if len(e) > 1 and e[1] is not None:
                d["label"] = e[1]
        else:
            d = {"t": e}
        for k in ("t", "width", "peak", "mean", "integral"):
            if k in d:
                d[k] = float(d[k])
                if not math.isfinite(d[k]):
                    del d[k]
        if "label" in d:
            d["label"] = str(d["label"])
        if "t" in d:
            out.append(d)
    return out


def main():
    init = json.loads(stdin.readline() or b"{}")
    ns = {"__name__": "nebulosa_trigger", "np": np, "math": math, "Runs": Runs}
    try:
        exec(compile(init.get("code", ""), "<trigger script>", "exec"), ns)
        fn = ns.get("trigger")
        if not callable(fn):
            raise NameError("the script must define a function trigger(data)")
    except Exception:
        reply({"ok": False, "error": tb()})
        return
    reply({"ok": True, "python": platform.python_version(), "numpy": getattr(np, "__version__", None)})
    while True:
        line = stdin.readline()
        if not line:
            return
        if not line.strip():
            continue
        req = json.loads(line)
        out = io.StringIO()
        try:
            if req.get("reset"):
                with contextlib.redirect_stdout(out):
                    if callable(ns.get("reset")):
                        ns["reset"]()
                reply({"ok": True, "out": out.getvalue()[-4000:]})
                continue
            data = Data(stdin.read(int(req["block"])))
            with contextlib.redirect_stdout(out):
                r = fn(data)
            reply({"events": events(r), "out": out.getvalue()[-4000:]})
        except Exception:
            reply({"error": tb(), "out": out.getvalue()[-4000:]})


main()
