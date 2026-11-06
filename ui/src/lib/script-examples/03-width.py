# title: Pulse width out of spec
# about: Bursts shorter or longer than they should be: a cut-short or a stretched transmission
# group: Basics
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# A burst (current above LEVEL) narrower than MIN_W or wider than MAX_W.

LEVEL = 0.005            # A
MIN_W, MAX_W = 1e-3, 4e-3  # s

bursts = Runs()

def reset():
    bursts.reset()

def trigger(data):
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        if b.width < MIN_W:
            found.append((b.t0, f"short: {b.width * 1e3:.2f} ms"))
        elif b.width > MAX_W:
            found.append((b.t0, f"long: {b.width * 1e3:.2f} ms"))
    return found
