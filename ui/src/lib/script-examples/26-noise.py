# title: Noisier than usual
# about: The noise (σ over short windows) several times its usual level: a failing regulator, interference
# group: Signal quality
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The standard deviation of the voltage over each WINDOW. The first BASE
# windows set the usual noise; more than K times that triggers, and again
# when it settles.

WINDOW = 0.01     # s
BASE = 20
K = 4

buf = np.zeros(0)
t_buf = None
sigmas = []
noisy = False

def reset():
    global buf, t_buf, sigmas, noisy
    buf, t_buf, sigmas, noisy = np.zeros(0), None, [], False

def trigger(data):
    global buf, t_buf, noisy
    found = []
    if t_buf is None:
        t_buf = data.t[0]
    buf = np.concatenate((buf, data.v))
    n = max(8, int(round(WINDOW / data.dt)))
    while len(buf) >= n:
        w, buf = buf[:n], buf[n:]
        t0, t_buf = t_buf, t_buf + n * data.dt
        s = float(np.std(w))
        if len(sigmas) < BASE:
            sigmas.append(s)
            continue
        usual = float(np.median(sigmas))
        if (s > K * usual) != noisy:
            noisy = not noisy
            found.append((t0, f"σ {s * 1e3:.2f} mV" + (" (usually {:.2f})".format(usual * 1e3) if noisy else " (settled)")))
    return found
