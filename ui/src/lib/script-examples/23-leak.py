# title: Sleep current too high
# about: The current between bursts (asleep) above a limit: a peripheral left on, a leak
# group: Energy
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# A quiet stretch (current under AWAKE) longer than MIN_QUIET whose mean is
# above LEAK: something stays on while it should sleep.

AWAKE = 0.001     # A
MIN_QUIET = 0.02  # s
LEAK = 20e-6      # A

quiet = Runs()

def reset():
    quiet.reset()

def trigger(data):
    return [(r.t0, f"asleep at {r.i_mean * 1e6:.0f} µA")
            for r in quiet.feed(data, data.i < AWAKE)
            if r.width > MIN_QUIET and r.i_mean > LEAK]
