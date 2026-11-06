# title: Too many wake-ups
# about: More than N bursts within a second: a device that wakes far more often than it should
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# More than MAX_BURSTS bursts starting within WINDOW: it triggers once when
# the rate goes over, and again only after it has come back down.

LEVEL = 0.005     # A
WINDOW = 1.0      # s
MAX_BURSTS = 10

bursts = Runs()
starts = []        # the starts within the last WINDOW
over = False

def reset():
    global starts, over
    bursts.reset()
    starts, over = [], False

def trigger(data):
    global starts, over
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        starts = [s for s in starts if s > b.t0 - WINDOW] + [b.t0]
        if len(starts) > MAX_BURSTS and not over:
            over = True
            found.append((b.t0, f"{len(starts)} in {WINDOW:g} s"))
        elif len(starts) <= MAX_BURSTS:
            over = False
    return found
