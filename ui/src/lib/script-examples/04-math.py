# title: On the math channel
# about: A threshold on the math channel (set one in Math channel), or on power in mW without one
# group: Basics
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The math channel (data.m: whatever expression is set in the Math channel
# section) above LIMIT, once per stretch above it. Without a math channel it
# uses the power in mW instead.

LIMIT = 50.0   # in the math channel's unit (here: mW)

above = Runs()

def reset():
    above.reset()

def trigger(data):
    x = data.m if data.m is not None else data.p * 1000
    return [(r.t0, f"{LIMIT:g}+ for {r.width * 1e3:.2f} ms")
            for r in above.feed(data, x > LIMIT)]
