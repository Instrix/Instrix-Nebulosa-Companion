# title: Edge with hysteresis
# about: Current rising through a level, re-armed only below a lower one: the simplest start
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

# Current rising through HIGH, once it has been below LOW: noise around
# the level doesn't fire it again and again.

HIGH = 0.020   # A
LOW = 0.015    # A
state = 0      # -1 below LOW, 1 above HIGH, 0 not known yet (across blocks)

def reset():
    global state
    state = 0

def trigger(data):
    global state
    i = data.i
    s = np.where(i >= HIGH, 1, np.where(i < LOW, -1, 0))
    idx = np.flatnonzero(s)          # where it is clearly below or above
    if len(idx) == 0:
        return []
    v = s[idx]
    before = np.concatenate(([state], v[:-1]))
    ups = idx[(v == 1) & (before == -1)]
    state = int(v[-1])
    return [(data.t[k], f"{i[k] * 1e3:.1f} mA") for k in ups]
