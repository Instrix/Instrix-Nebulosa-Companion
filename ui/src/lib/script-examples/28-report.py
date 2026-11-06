# title: A report every so often
# about: A labelled event every 10 s with that stretch's statistics: a running log in the Events list
# group: Logging
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Every PERIOD: one event, labelled with what that stretch measured: the
# mean and highest current, the lowest voltage, the energy. Export the
# Events list (CSV) for a table of it.

PERIOD = 10.0     # s

start = None
n = 0
i_sum = i_max = e = 0.0
v_min = np.inf

def reset():
    global start, n, i_sum, i_max, e, v_min
    start, n, i_sum, i_max, e, v_min = None, 0, 0.0, 0.0, 0.0, np.inf

def trigger(data):
    global start, n, i_sum, i_max, e, v_min
    found = []
    if start is None:
        start = float(data.t[0])
    t = data.t
    a = 0
    while True:
        b = int(np.searchsorted(t, start + PERIOD))
        if b > a:
            i, v, p = data.i[a:b], data.v[a:b], data.p[a:b]
            n += b - a
            i_sum += float(i.sum())
            i_max = max(i_max, float(i.max()))
            v_min = min(v_min, float(v.min()))
            e += float(p.sum() * data.dt)
        if b >= len(t):
            break
        found.append((start, f"mean {i_sum / max(1, n) * 1e3:.2f} mA, max {i_max * 1e3:.1f} mA, min {v_min:.3f} V, {e * 1e3:.1f} mJ"))
        start += PERIOD
        n, i_sum, i_max, e, v_min = 0, 0.0, 0.0, 0.0, np.inf
        a = b
    return found
