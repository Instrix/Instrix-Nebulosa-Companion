# title: Fast edge (dI/dt)
# about: Current changing faster than a rate (A/s), e.g. an inrush or a short
# group: Signal quality
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The current's slope, smoothed over SMOOTH seconds, above RATE (A/s): an
# inrush, a short, a load switched on hard.

RATE = 50.0        # A/s (50 mA per ms)
SMOOTH = 20e-6     # s

tail = None        # the last samples of the previous block, for the slope across the edge
fast = Runs()

def reset():
    global tail
    tail = None
    fast.reset()

def trigger(data):
    global tail
    k = max(1, int(round(SMOOTH / data.dt)))
    i = data.i
    ext = i if tail is None else np.concatenate((tail, i))
    s = np.zeros(len(ext))
    s[k:] = (ext[k:] - ext[:-k]) / (k * data.dt)    # A/s, over k samples
    slope = s[-len(i):]
    tail = ext[-k:]
    found = []
    for r in fast.feed(data, np.abs(slope) > RATE):
        here = (data.t >= r.t0) & (data.t < r.t1)
        top = np.max(np.abs(slope[here])) if here.any() else RATE
        found.append((r.t0, f"{top * 1e-3:.1f} A/ms"))
    return found
