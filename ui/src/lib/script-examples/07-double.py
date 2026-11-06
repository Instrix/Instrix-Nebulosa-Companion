# title: Two bursts too close
# about: A burst right after another (closer than a gap): a retransmission, a bounced button
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Two bursts whose starts are less than MIN_GAP apart: it triggers at the
# second one.

LEVEL = 0.005      # A
MIN_GAP = 0.02     # s

bursts = Runs()
last = None

def reset():
    global last
    bursts.reset()
    last = None

def trigger(data):
    global last
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        if last is not None and b.t0 - last < MIN_GAP:
            found.append((b.t0, f"{(b.t0 - last) * 1e3:.1f} ms after the last"))
        last = b.t0
    return found
