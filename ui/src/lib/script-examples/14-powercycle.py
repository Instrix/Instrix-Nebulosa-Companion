# title: Power cycles
# about: The supply going away and coming back: each power-on, with how long it was off
# group: Supply
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The voltage below OFF (V) is off. Each power-on triggers, with how long it
# was off; a stretch shorter than MIN_OFF is a dip, not a power cycle.

OFF = 0.5        # V
MIN_OFF = 1e-3   # s

offs = Runs()

def reset():
    offs.reset()

def trigger(data):
    return [(r.t1, f"on after {r.width * 1e3:.0f} ms off")
            for r in offs.feed(data, data.v < OFF)
            if r.width >= MIN_OFF]
