# title: Inrush at power-on
# about: The current's peak in the first milliseconds after the supply comes up: the inrush into the capacitors
# group: Supply
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# When the voltage comes up through ON, the highest current in the next
# WINDOW is the inrush; more than LIMIT triggers (at the power-on).

ON = 2.0          # V
WINDOW = 5e-3     # s
LIMIT = 0.2       # A

was_on = None
pending = None    # a power-on being watched: [time, highest current so far]

def reset():
    global was_on, pending
    was_on, pending = None, None

def trigger(data):
    global was_on, pending
    found = []
    on = data.v > ON
    prev = np.concatenate(([on[0] if was_on is None else was_on], on[:-1]))
    ups = np.flatnonzero(on & ~prev)
    was_on = bool(on[-1])
    for k in ups:
        if pending is None:
            pending = [float(data.t[k]), 0.0]
    if pending is not None:
        t0 = pending[0]
        sel = (data.t >= t0) & (data.t < t0 + WINDOW)
        if sel.any():
            pending[1] = max(pending[1], float(data.i[sel].max()))
        if data.t[-1] >= t0 + WINDOW:
            if pending[1] > LIMIT:
                found.append((t0, f"inrush {pending[1] * 1e3:.0f} mA"))
            pending = None
    return found
