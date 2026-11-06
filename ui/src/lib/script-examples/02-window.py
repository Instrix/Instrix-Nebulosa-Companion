# title: Stuck in a band
# about: The current staying inside a band longer than a time: a half-on state, a load that didn't switch fully
# group: Basics
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The current inside [LOW, HIGH] for longer than MIN_TIME: passing through
# the band on an edge is quick and doesn't count; staying there does.

LOW, HIGH = 0.001, 0.005   # A
MIN_TIME = 5e-3            # s

stays = Runs()

def reset():
    stays.reset()

def trigger(data):
    inside = (data.i >= LOW) & (data.i <= HIGH)
    return [(r.t0, f"{r.i_mean * 1e3:.2f} mA for {r.width * 1e3:.1f} ms")
            for r in stays.feed(data, inside)
            if r.width > MIN_TIME]
