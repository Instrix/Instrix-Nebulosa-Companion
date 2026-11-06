# title: Ripple frequency (FFT)
# about: The voltage's strongest ripple frequency leaving a band: a switcher that changed mode
# group: Signal quality
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Every WINDOW of voltage: its spectrum (FFT), its strongest frequency. It
# triggers when that leaves [F_LOW, F_HIGH], and again when it comes back.

WINDOW = 10e-3          # s per spectrum
F_LOW, F_HIGH = 1e3, 100e3   # Hz
MIN_RIPPLE = 1e-3       # V: quieter than this, no frequency to speak of

buf = np.zeros(0)
t_buf = None
inside = True

def reset():
    global buf, t_buf, inside
    buf, t_buf, inside = np.zeros(0), None, True

def trigger(data):
    global buf, t_buf, inside
    if t_buf is None:
        t_buf = data.t[0]
    buf = np.concatenate((buf, data.v))
    n = max(16, int(round(WINDOW / data.dt)))
    found = []
    while len(buf) >= n:
        w, buf = buf[:n], buf[n:]
        t0, t_buf = t_buf, t_buf + n * data.dt
        w = w - w.mean()
        if np.ptp(w) < MIN_RIPPLE:
            continue
        spec = np.abs(np.fft.rfft(w * np.hanning(n)))
        f = np.fft.rfftfreq(n, data.dt)[1 + np.argmax(spec[1:])]
        ok = F_LOW <= f <= F_HIGH
        if ok != inside:
            inside = ok
            found.append((t0, f"{f / 1e3:.1f} kHz" + ("" if ok else " (out)")))
    return found
