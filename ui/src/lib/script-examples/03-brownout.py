# title: Brown-out (voltage sag)
# about: The supply below a voltage for longer than a time, labelled with its lowest point
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

# A sag: the voltage below LIMIT for longer than MIN_TIME. Shorter dips
# (switching noise) are ignored. It triggers at the start of the sag.

LIMIT = 3.0       # V
MIN_TIME = 50e-6  # s

sags = Runs()

def reset():
    sags.reset()

def trigger(data):
    return [(s.t0, f"{s.v_min:.3f} V for {s.width * 1e6:.0f} µs")
            for s in sags.feed(data, data.v < LIMIT)
            if s.width > MIN_TIME]
