# title: Frequency out of band
# about: A periodic signal (PWM, a clock's load) whose frequency leaves a band, measured every window
# group: Timing
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Rising edges of the current through LEVEL; every WINDOW, the frequency
# from them. It triggers when that leaves [F_LOW, F_HIGH], and again when it
# comes back.

LEVEL = 0.04              # A
WINDOW = 0.05             # s
F_LOW, F_HIGH = 1.8e3, 2.2e3   # Hz

above = None      # the last sample's state, across blocks
edges = []        # rising edges in this window, s
w_start = None
inside = True

def reset():
    global above, edges, w_start, inside
    above, edges, w_start, inside = None, [], None, True

def trigger(data):
    global above, edges, w_start, inside
    found = []
    a = data.i > LEVEL
    prev = np.concatenate(([a[0] if above is None else above], a[:-1]))
    above = bool(a[-1])
    ups = data.t[a & ~prev]
    if w_start is None:
        w_start = data.t[0]
    edges += list(ups)
    while data.t[-1] >= w_start + WINDOW:
        mine = [e for e in edges if e < w_start + WINDOW]
        edges = [e for e in edges if e >= w_start + WINDOW]
        if len(mine) >= 2:
            f = (len(mine) - 1) / (mine[-1] - mine[0])
            ok = F_LOW <= f <= F_HIGH
            if ok != inside:
                inside = ok
                found.append((w_start, f"{f / 1e3:.2f} kHz" + ("" if ok else " (out)")))
        w_start += WINDOW
    return found
