# title: Outliers (n σ)
# about: Samples far from the usual level: n times the robust spread (median and MAD) of the block
# group: Signal quality
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

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
