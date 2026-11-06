# title: Brown-out (voltage sag)
# about: The supply below a voltage for longer than a time, labelled with its lowest point
# group: Supply
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

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
