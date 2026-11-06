# title: Bursts by charge
# about: Current bursts that use more than a charge (µC): the expensive radio transmissions
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# A burst: current above LEVEL. It triggers (at its start) when its charge,
# the integral of the current over it, is more than MIN_CHARGE.

LEVEL = 0.005        # A
MIN_CHARGE = 20e-6   # C

bursts = Runs()

def reset():
    bursts.reset()

def trigger(data):
    return [(b.t0, f"{b.charge * 1e6:.1f} µC in {b.width * 1e3:.2f} ms")
            for b in bursts.feed(data, data.i > LEVEL)
            if b.charge > MIN_CHARGE]
