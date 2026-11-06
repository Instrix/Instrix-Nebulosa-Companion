# title: Charge creeping up
# about: The bursts' charge growing over time against the first ones: a degrading link, retries creeping in
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The median charge of the last RECENT bursts against the first BASE bursts'
# (the baseline): more than TOL above it triggers, once, until it's back.

LEVEL = 0.005      # A
BASE, RECENT = 10, 5
TOL = 0.2          # 20 %

bursts = Runs()
charges = []
high = False

def reset():
    global charges, high
    bursts.reset()
    charges, high = [], False

def trigger(data):
    global high
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        charges.append(b.charge)
        if len(charges) < BASE + RECENT:
            continue
        base = np.median(charges[:BASE])
        now = np.median(charges[-RECENT:])
        if now > (1 + TOL) * base and not high:
            high = True
            found.append((b.t0, f"+{(now / base - 1) * 100:.0f} %: {now * 1e6:.1f} µC"))
        elif now <= (1 + TOL) * base:
            high = False
    return found
