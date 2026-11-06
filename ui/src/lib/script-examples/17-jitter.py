# title: Period jitter
# about: Bursts earlier or later than their usual period by more than a tolerance
# group: Timing
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The time between bursts against the usual period (the median of the last
# ones): off by more than TOL either way triggers.

LEVEL = 0.005      # A
TOL = 0.1          # 10 % of the period

bursts = Runs()
last = None
periods = []

def reset():
    global last, periods
    bursts.reset()
    last, periods = None, []

def trigger(data):
    global last
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        if last is not None:
            p = b.t0 - last
            if len(periods) >= 5:
                usual = float(np.median(periods))
                if abs(p - usual) > TOL * usual:
                    found.append((b.t0, f"{(p - usual) * 1e3:+.1f} ms"))
            periods.append(p)
            del periods[:-20]
        last = b.t0
    return found
