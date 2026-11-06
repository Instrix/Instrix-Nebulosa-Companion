# title: Energy per second
# about: Each whole second's energy (J) above a limit, labelled with it
# group: Energy
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Each whole second (from 0 s on the clock): its energy, the integral of the
# power. More than LIMIT triggers at the start of that second.

LIMIT = 0.02     # J

sec = None       # the second being added up
joules = 0.0

def reset():
    global sec, joules
    sec, joules = None, 0.0

def trigger(data):
    global sec, joules
    found = []
    s = np.floor(data.t).astype(np.int64)
    for k in np.unique(s):
        e = float(data.p[s == k].sum() * data.dt)
        if sec is not None and k != sec:
            if joules > LIMIT:
                found.append((float(sec), f"{joules * 1e3:.1f} mJ"))
            joules = 0.0
        sec = int(k)
        joules += e
    return found
