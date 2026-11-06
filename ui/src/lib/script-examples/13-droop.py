# title: Supply too weak (droop per amp)
# about: The supply's resistance from each burst (volts lost per amp drawn): a tired battery, a bad contact
# group: Supply
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# For each burst: how far the voltage falls from just before it (QUIET_V,
# the voltage while not drawing) per amp drawn: the source's resistance.
# More than R_MAX ohms triggers.

LEVEL = 0.005    # A: a burst
R_MAX = 3.0      # Ω

bursts = Runs()
quiet_v = None   # the voltage at rest, lately

def reset():
    global quiet_v
    bursts.reset()
    quiet_v = None

def trigger(data):
    global quiet_v
    found = []
    rest = data.i < LEVEL
    for b in bursts.feed(data, ~rest):
        if quiet_v is not None and b.i_max > 0:
            r = (quiet_v - b.v_min) / b.i_max
            if r > R_MAX:
                found.append((b.t0, f"{r:.1f} Ω ({(quiet_v - b.v_min) * 1e3:.0f} mV at {b.i_max * 1e3:.0f} mA)"))
    if rest.any():
        quiet_v = float(np.median(data.v[rest][-2000:]))
    return found
