# title: Outliers (n σ)
# about: Samples far from the usual level: n times the robust spread (median and MAD) of the block
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

# A spike: current further than K times the usual spread from the usual
# level. "Usual" is robust (median and MAD over the block), so the spikes
# don't hide themselves. Each stretch of outliers is one trigger.

K = 8

spikes = Runs()

def reset():
    spikes.reset()

def trigger(data):
    i = data.i
    mid = np.median(i)
    spread = 1.4826 * np.median(np.abs(i - mid)) + 1e-9   # ≈ σ for Gaussian noise
    far = np.abs(i - mid) > K * spread
    return [(r.t0, f"{(r.i_max - mid) / spread:.0f} σ") for r in spikes.feed(data, far)]
