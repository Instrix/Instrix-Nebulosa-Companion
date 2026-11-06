# title: Bursts grouped into events
# about: Bursts close together counted as one event (a BLE advertising event: a burst per channel), with its charge
# group: Bursts
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# Bursts (current above LEVEL) less than GAP apart are one event: a BLE
# advertising event is a burst on each of its channels. It triggers at each
# event's start, labelled with how many bursts it had and its charge.

LEVEL = 0.002   # A
GAP = 0.005     # s

bursts = Runs()
event = None    # the one being gathered: start, bursts, charge, end of the last burst

def reset():
    global event
    bursts.reset()
    event = None

def done(e):
    return (e["t0"], f"{e['n']} bursts, {e['q'] * 1e6:.1f} µC")

def trigger(data):
    global event
    found = []
    for b in bursts.feed(data, data.i > LEVEL):
        if event and b.t0 - event["end"] < GAP:
            event["n"] += 1
            event["q"] += b.charge
            event["end"] = b.t1
        else:
            if event:
                found.append(done(event))
            event = {"t0": b.t0, "n": 1, "q": b.charge, "end": b.t1}
    # over once GAP has passed without another burst
    if event and bursts.current is None and data.t[-1] - event["end"] > GAP:
        found.append(done(event))
        event = None
    return found
