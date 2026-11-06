# title: Burst shape changed
# about: Each burst compared with the first one (correlation): a different shape, a firmware path that changed
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The first burst is the template. Every burst after it (with a margin
# around it) is resampled to N points and correlated with the template;
# below MIN_CORR, it's a different shape.

LEVEL = 0.005     # A
N = 64
MIN_CORR = 0.9
PAD = 0.2         # of the width, either side

bursts = Runs()
hist_t = np.zeros(0)   # the last HOLD s of samples: a burst's own samples
hist_i = np.zeros(0)
HOLD = 0.5
waiting = []           # bursts whose margin after hasn't come in yet
template = None

def reset():
    global hist_t, hist_i, waiting, template
    bursts.reset()
    hist_t, hist_i, waiting, template = np.zeros(0), np.zeros(0), [], None

def shape(t0, t1):
    sel = (hist_t >= t0) & (hist_t < t1)
    x = np.interp(np.linspace(t0, t1, N), hist_t[sel], hist_i[sel])
    return (x - x.mean()) / (x.std() + 1e-12)

def trigger(data):
    global hist_t, hist_i, waiting, template
    hist_t = np.concatenate((hist_t, data.t))
    hist_i = np.concatenate((hist_i, data.i))
    keep = hist_t > data.t[-1] - HOLD
    hist_t, hist_i = hist_t[keep], hist_i[keep]
    found = []
    waiting += [(b.t0 - PAD * b.width, b.t1 + PAD * b.width) for b in bursts.feed(data, data.i > LEVEL)]
    while waiting and waiting[0][1] <= hist_t[-1]:
        t0, t1 = waiting.pop(0)
        s = shape(t0, t1)
        if template is None:
            template = s
            continue
        c = float(np.mean(s * template))
        if c < MIN_CORR:
            found.append((t0, f"correlation {c:.2f}"))
    return found
