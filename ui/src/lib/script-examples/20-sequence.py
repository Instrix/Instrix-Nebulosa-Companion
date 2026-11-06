# title: Burst, then the supply dips
# about: Two signals in order: a current burst starts, then within a time the voltage falls below a level
# group: Timing
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# A burst starting (current up through LEVEL), then the voltage below V_LOW
# within WINDOW of it: it triggers at the burst, labelled with how deep and
# how soon. (The same as the Then B option, with room for more.)

LEVEL = 0.010     # A
V_LOW = 2.9       # V
WINDOW = 3e-3     # s

above = None
pending = []      # bursts being watched: [start, lowest V, when]

def reset():
    global above, pending
    above, pending = None, []

def trigger(data):
    global above
    found = []
    a = data.i > LEVEL
    prev = np.concatenate(([a[0] if above is None else above], a[:-1]))
    above = bool(a[-1])
    pending.extend([float(t), np.inf, 0.0] for t in data.t[a & ~prev])
    for p in pending:
        sel = (data.t >= p[0]) & (data.t < p[0] + WINDOW)
        if sel.any():
            k = int(np.argmin(np.where(sel, data.v, np.inf)))
            if data.v[k] < p[1]:
                p[1], p[2] = float(data.v[k]), float(data.t[k] - p[0])
    done = [p for p in pending if data.t[-1] >= p[0] + WINDOW]
    pending[:] = [p for p in pending if data.t[-1] < p[0] + WINDOW]
    for t0, vmin, after in done:
        if vmin < V_LOW:
            found.append((t0, f"{vmin:.2f} V, {after * 1e3:.1f} ms in"))
    return found
