# title: Duty cycle out of range
# about: The share of time above a level, every window, outside a range: a PWM, a radio's air time
# group: Timing
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Every WINDOW: the share of samples with the current above LEVEL. It
# triggers when that leaves [D_LOW, D_HIGH], and again when it comes back.

LEVEL = 0.04       # A
WINDOW = 0.05      # s
D_LOW, D_HIGH = 0.25, 0.35

n_on = n_all = 0
w_start = None
inside = True

def reset():
    global n_on, n_all, w_start, inside
    n_on = n_all = 0
    w_start, inside = None, True

def trigger(data):
    global n_on, n_all, w_start, inside
    found = []
    if w_start is None:
        w_start = data.t[0]
    t, on = data.t, data.i > LEVEL
    a = 0
    while True:
        end = w_start + WINDOW
        b = int(np.searchsorted(t, end))
        n_on += int(on[a:b].sum())
        n_all += b - a
        if b >= len(t):
            break
        d = n_on / max(1, n_all)
        ok = D_LOW <= d <= D_HIGH
        if ok != inside:
            inside = ok
            found.append((w_start, f"duty {d * 100:.0f} %" + ("" if ok else " (out)")))
        n_on = n_all = 0
        w_start, a = end, b
    return found
