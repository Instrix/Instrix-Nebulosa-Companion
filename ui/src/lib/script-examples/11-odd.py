# title: Odd ones out (radio node)
# about: Every burst checked at once: retries, aborted wake-ups, slow starts, supply dips, late or missing ones, labelled
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# For the radio node (the Advanced triggers showcase): a transmission every
# PERIOD. Each burst (anything over AWAKE) is checked several ways at once,
# and the odd ones are labelled with what's odd about them.

AWAKE = 0.003     # A: woken up at all
TX = 0.010        # A: transmitting
PERIOD = 0.1      # s
MAX_WIDTH = 4e-3  # s: longer is a retry
DIP = 0.15        # V: the supply sagging more than this during it
MIN_WIDTH = 20e-6 # s: shorter is noise around AWAKE, not a burst

bursts = Runs()
last = None       # the last transmission's start

def reset():
    global last
    bursts.reset()
    last = None

def trigger(data):
    global last
    found = []
    for b in bursts.feed(data, data.i > AWAKE):
        if b.width < MIN_WIDTH:
            continue
        odd = []
        if b.i_max < TX:
            odd.append(f"aborted ({b.i_max * 1e3:.1f} mA)")
        else:
            if b.width > MAX_WIDTH:
                odd.append(f"retry ({b.width * 1e3:.1f} ms)")
            if b.i_mean < 0.8 * b.i_max:
                odd.append("slow start")
            if b.v_max - b.v_min > DIP:
                odd.append(f"dip to {b.v_min:.2f} V")
            if last is not None:
                gap = b.t0 - last
                if gap > 1.5 * PERIOD:
                    odd.append(f"{round(gap / PERIOD) - 1} missing before")
                elif gap > 1.2 * PERIOD:
                    odd.append(f"late (+{(gap - PERIOD) * 1e3:.0f} ms)")
                elif gap < 0.8 * PERIOD:
                    odd.append(f"early ({(gap - PERIOD) * 1e3:.0f} ms)")
            last = b.t0
        if odd:
            found.append((b.t0, ", ".join(odd)))
    return found
