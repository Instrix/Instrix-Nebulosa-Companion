# title: Missed heartbeat
# about: A periodic burst that comes late: longer than 1.5× its usual period, learned as it runs
# group: Timing
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# A device that wakes up periodically: each wake-up is a current burst. It
# triggers when a burst comes later than FACTOR times the usual period
# (the median of the last ones), or never: then at the deadline, once.

LEVEL = 0.005    # A: a burst
FACTOR = 1.5

bursts = Runs()
last = None      # the last burst's start, s
periods = []     # the latest periods, s
missed = False   # already fired for this wait

def reset():
    global last, periods, missed
    bursts.reset()
    last, periods, missed = None, [], False

def trigger(data):
    global last, missed
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        if last is not None:
            p = b.t0 - last
            usual = np.median(periods) if len(periods) >= 3 else None
            if usual and p > FACTOR * usual and not missed:
                found.append((b.t0, f"late: {p * 1e3:.0f} ms (usually {usual * 1e3:.0f})"))
            periods.append(p)
            del periods[:-20]
        last = b.t0
        missed = False
    # still nothing: fire at the deadline, once
    if last is not None and len(periods) >= 3 and not missed and bursts.current is None:
        deadline = last + FACTOR * np.median(periods)
        if data.t[-1] > deadline:
            missed = True
            found.append((deadline, "missed"))
    return found
