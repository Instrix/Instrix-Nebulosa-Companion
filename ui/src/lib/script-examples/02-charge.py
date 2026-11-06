# title: Bursts by charge
# about: Current bursts that use more than a charge (µC): the expensive radio transmissions
#
# trigger(data) gets the new samples a block at a time and returns where
# the triggers are: a list of times (s), or (time, label) pairs; labels show
# in the event list and the CSV. Globals live on between blocks; reset(), if
# there is one, runs when the trigger starts over.
#
#   data.t  times (s)    data.i  current (A)   data.v  voltage (V)
#   data.p  power (W)    data.m  math channel (None without one)
#   data.dt s per sample data.runs  [(t0, count)]: stretches without a gap
#
# np is numpy. Runs() follows stretches where a condition holds across
# blocks: Runs().feed(data, mask) gives the ones that ended, each with t0, t1,
# width, n, i_mean / i_min / i_max (and v_, p_), charge (C), energy (J).

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
