# title: Write each burst to a file
# about: Each burst's statistics appended to a CSV file of your own; no triggers (or only the odd ones)
# group: Logging
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Every burst as a row of a CSV file (FILE, in your home folder unless it
# is a full path): its start, width, charge, peak current and lowest
# voltage. It triggers on none; return them (or only some) to see them too.

import os, csv, time

LEVEL = 0.005                     # A
FILE = os.environ.get("NEBULOSA_BURSTS_CSV", "nebulosa-bursts.csv")

bursts = Runs()
out = None

def reset():
    bursts.reset()

def writer():
    global out
    if out is None:
        path = os.path.join(os.path.expanduser("~"), FILE)
        new = not os.path.exists(path)
        f = open(path, "a", newline="")
        out = (f, csv.writer(f))
        if new:
            out[1].writerow(["wall_time", "t_s", "width_s", "charge_C", "i_max_A", "v_min_V"])
        print("writing", path)
    return out

def trigger(data):
    rows = [(time.time(), f"{b.t0:.6f}", f"{b.width:.6e}", f"{b.charge:.6e}", f"{b.i_max:.6e}", f"{b.v_min:.4f}")
            for b in bursts.feed(data, data.i > LEVEL)]
    if rows:
        f, w = writer()
        w.writerows(rows)
        f.flush()
    return []
