# title: Average power over budget
# about: The power averaged over the last second above a budget: a device that drains its battery too fast
# group: Energy
#
# trigger(data) gets the new samples a block at a time and returns the
# trigger times: a list of times (s), or (time, label) pairs; labels show in
# the event list and the CSV. The editor's Reference has the rest (data,
# Runs(), reset(), globals between blocks).

# The power averaged over the last WINDOW (in STEP pieces, so it moves
# along smoothly) above BUDGET: it triggers going over, and again only
# after it has come back under.

WINDOW = 1.0      # s
STEP = 0.01       # s
BUDGET = 0.010    # W

pieces = []       # mean power of each STEP, the last WINDOW of them
acc = cnt = 0.0
p_start = None
over = False

def reset():
    global pieces, acc, cnt, p_start, over
    pieces, acc, cnt, p_start, over = [], 0.0, 0, None, False

def trigger(data):
    global acc, cnt, p_start, over
    found = []
    if p_start is None:
        p_start = data.t[0]
    t, p = data.t, data.p
    a = 0
    while True:
        b = int(np.searchsorted(t, p_start + STEP))
        acc += float(p[a:b].sum())
        cnt += b - a
        if b >= len(t):
            break
        pieces.append(acc / max(1, cnt))
        del pieces[:-int(round(WINDOW / STEP))]
        acc, cnt = 0.0, 0
        p_start += STEP
        a = b
        if len(pieces) * STEP >= WINDOW:
            avg = sum(pieces) / len(pieces)
            if avg > BUDGET and not over:
                over = True
                found.append((p_start, f"{avg * 1e3:.1f} mW over {WINDOW:g} s"))
            elif avg <= BUDGET:
                over = False
    return found
