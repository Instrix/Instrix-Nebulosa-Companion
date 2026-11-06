<script lang="ts">
  // Help › Trigger scripts: the whole reference for the Script (Python)
  // trigger type. What the app hands the script, what it may return, the
  // helpers, the rules for state, errors and speed, patterns, the examples.
  // Keep it in step with src-tauri/src/script_runner.py and trigger.svelte.ts.
  import { EXAMPLE_GROUPS } from './scriptExamples';

  const TOC: [string, string][] = [
    ['sr-overview', 'How it works'],
    ['sr-start', 'A first script'],
    ['sr-trigger', 'trigger(data)'],
    ['sr-return', 'What to return'],
    ['sr-data', 'data'],
    ['sr-state', 'State between blocks'],
    ['sr-runs', 'Runs()'],
    ['sr-names', 'Names, imports, print'],
    ['sr-errors', 'Errors and Check'],
    ['sr-speed', 'Keeping up'],
    ['sr-patterns', 'Patterns'],
    ['sr-python', 'Python and safety'],
    ['sr-examples', 'The examples'],
  ];

  let root: HTMLElement | undefined = $state();
  function go(id: string) {
    root?.querySelector(`#${id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
</script>

<div class="ref" bind:this={root}>
  <nav aria-label="Contents">
    {#each TOC as [id, title] (id)}
      <button class="linkish" onclick={() => go(id)}>{title}</button>
    {/each}
  </nav>

  <section id="sr-overview">
    <h3>How it works</h3>
    <p>
      The <b>Script (Python)</b> trigger type is a detector you write. Choose it under Trigger › Type (the Scope's Trigger section, or the Logger's triggered
      capture: they share one trigger), then <b>Edit script…</b>. It needs the desktop app and Python 3 on the computer; numpy is optional but most scripts
      (and <code>Runs()</code>) need it.
    </p>
    <ul>
      <li>The app detects nothing itself. While the trigger runs (Auto, Normal, Single, Log events, or the Logger watching or armed), the samples go to your function <b>a block at a time</b>, in order, as they come: usually 0.05 to 0.25 s, at most 500 000 samples per block.</li>
      <li>Your function returns where the triggers are. Each becomes an <b>event</b>: in the Events list (with its label), on the plots (▼), able to hold the Scope on it and to start a Logger capture.</li>
      <li>The script runs in its own Python process, started when it's first needed and again when the code or the interpreter changes. Its module stays loaded between blocks.</li>
    </ul>
  </section>

  <section id="sr-start">
    <h3>A first script</h3>
    <pre><code>LEVEL = 0.020          # A

last = None            # the previous block's last sample

def trigger(data):
    global last
    i = data.i
    prev = np.concatenate(([i[0] if last is None else last], i[:-1]))
    last = i[-1]
    up = np.flatnonzero((prev &lt; LEVEL) &amp; (i &gt;= LEVEL))   # rising through LEVEL
    return [(data.t[k], f"{"{"}i[k] * 1e3:.1f{"}"} mA") for k in up]</code></pre>
    <p>Every rising edge of the current through 20 mA, labelled with the current there. <b>Check</b> in the editor tries it on the latest samples; <b>Apply</b> runs it.</p>
  </section>

  <section id="sr-trigger">
    <h3>trigger(data)</h3>
    <p>The one function a script must define. The app calls it once per block with <code>data</code>, the new samples (the ones after the previous block's, none twice, none skipped unless the script falls behind: see <a href="#sr-speed" onclick={(e) => (e.preventDefault(), go('sr-speed'))}>Keeping up</a>).</p>
    <dl>
      <dt><code>def trigger(data)</code></dt>
      <dd>Required. Returns the triggers found in this block (or in an earlier one, once it knows: a trigger may be in the past).</dd>
      <dt><code>def reset()</code></dt>
      <dd>Optional. Called when the trigger starts over: its settings change, the script is applied again, or another device is connected. Put the globals back as they were at the start.</dd>
    </dl>
  </section>

  <section id="sr-return">
    <h3>What to return</h3>
    <table>
      <thead><tr><th>Return</th><th>Means</th></tr></thead>
      <tbody>
        <tr><td><code>None</code>, <code>[]</code>, <code>False</code></td><td>No trigger in this block</td></tr>
        <tr><td><code>1.234</code></td><td>One trigger at that time (s)</td></tr>
        <tr><td><code>[t1, t2, …]</code> or a numpy array</td><td>A trigger at each time</td></tr>
        <tr><td><code>[(t, "label"), …]</code></td><td>Each with a label: shown in the Events list's <i>script</i> column and the CSV export</td></tr>
        <tr><td><code>[{"{"}"t": t, "label": …, "width": …, "peak": …, "mean": …, "integral": …{"}"}, …]</code></td><td>Any of these keys besides <code>t</code>: they fill the Events list's width, peak, mean and ∫ columns</td></tr>
      </tbody>
    </table>
    <ul>
      <li>Times are in seconds on the samples' own clock: the values of <code>data.t</code>. Use them, not wall-clock time.</li>
      <li>Values that aren't finite (NaN, ∞) are dropped; labels are made strings.</li>
      <li>Events can come in any order within a return; they're sorted. Each is shown as <b>✎ script</b> in the Events list.</li>
    </ul>
  </section>

  <section id="sr-data">
    <h3>data</h3>
    <table>
      <thead><tr><th>Name</th><th>What</th></tr></thead>
      <tbody>
        <tr><td><code>data.t</code></td><td>Each sample's time, s (float64)</td></tr>
        <tr><td><code>data.i</code></td><td>Current, A</td></tr>
        <tr><td><code>data.v</code></td><td>Voltage, V</td></tr>
        <tr><td><code>data.p</code></td><td>Power, W (<code>i * v</code>)</td></tr>
        <tr><td><code>data.m</code></td><td>The math channel, in its own unit; <code>None</code> without one (Math channel section)</td></tr>
        <tr><td><code>data.dt</code></td><td>Seconds per sample (0.5 µs at 2 MS/s; more with decimation)</td></tr>
        <tr><td><code>data.runs</code></td><td><code>[(t0, count), …]</code>: the stretches without a gap. One, unless samples were lost (a gap in time between them)</td></tr>
        <tr><td><code>len(data)</code></td><td>Samples in the block (at least 1)</td></tr>
      </tbody>
    </table>
    <p>The arrays are numpy <code>float64</code> arrays of equal length (with no numpy: <code>array.array</code>, slow to work with). They're yours: changing them changes nothing in the app.</p>
  </section>

  <section id="sr-state">
    <h3>State between blocks</h3>
    <p>A burst, an edge or a window can straddle two blocks. The module stays loaded, so <b>globals live on</b> from one call to the next: keep what the next block needs (the last sample, a run in progress, the samples of a window not yet full, a count). Declare them <code>global</code> in <code>trigger</code> to change them.</p>
    <ul>
      <li>Define <code>reset()</code> to put them back when the trigger starts over; without it they carry on.</li>
      <li><b>Restart</b> (in the trigger panel) starts the Python process again: every global starts over, as when the script was first loaded.</li>
      <li><b>Check</b> runs in a Python of its own: it never touches the running script's state.</li>
    </ul>
  </section>

  <section id="sr-runs">
    <h3>Runs()</h3>
    <p>Stretches where a condition holds, followed across blocks: the most common building block (bursts, sags, quiet times).</p>
    <pre><code>bursts = Runs()

def reset():
    bursts.reset()

def trigger(data):
    return [(b.t0, f"{"{"}b.charge * 1e6:.1f{"}"} µC") for b in bursts.feed(data, data.i &gt; 0.005)]</code></pre>
    <dl>
      <dt><code>runs.feed(data, mask)</code></dt>
      <dd>A boolean array, one per sample of the block. Returns the runs that <b>ended</b> in this block (a list of <code>Run</code>), in order.</dd>
      <dt><code>runs.current</code></dt>
      <dd>The run still going at the end of the block (a <code>Run</code> with what it has so far), or <code>None</code>.</dd>
      <dt><code>runs.reset()</code></dt>
      <dd>Forgets the run in progress.</dd>
    </dl>
    <table>
      <thead><tr><th>Run</th><th>What</th></tr></thead>
      <tbody>
        <tr><td><code>t0</code>, <code>t1</code></td><td>Its first sample's time, and the first sample's after it, s</td></tr>
        <tr><td><code>width</code>, <code>n</code></td><td><code>t1 - t0</code>, s; the samples in it</td></tr>
        <tr><td><code>i_mean</code>, <code>i_min</code>, <code>i_max</code></td><td>Current over it, A (also <code>v_…</code> in V and <code>p_…</code> in W)</td></tr>
        <tr><td><code>charge</code>, <code>energy</code></td><td>∫ i dt, C; ∫ p dt, J</td></tr>
      </tbody>
    </table>
    <p>A one- or two-sample run is often noise around the level: skip runs narrower than a few µs (<code>if b.width &lt; 20e-6: continue</code>), or use two levels (hysteresis).</p>
  </section>

  <section id="sr-names">
    <h3>Names, imports, print</h3>
    <ul>
      <li>Already there: <code>np</code> (numpy, or <code>None</code> without it), <code>math</code>, <code>Runs</code>.</li>
      <li><code>import</code> anything installed for that Python: scipy, pandas, your own modules (on its path). The top level runs once, when the script loads; keep it quick (a load slower than 15 s fails).</li>
      <li><code>print()</code> inside <code>trigger</code> shows under <b>Printed</b> in the panel and in Check (the last 4000 characters of each call). Printing every block is slow: print what matters.</li>
      <li>Files: a script can read and write them (see the example <i>Write each burst to a file</i>); relative paths are relative to the app's working folder, so use full ones or <code>os.path.expanduser("~")</code>.</li>
    </ul>
  </section>

  <section id="sr-errors">
    <h3>Errors and Check</h3>
    <table>
      <thead><tr><th>When</th><th>What happens</th></tr></thead>
      <tbody>
        <tr><td>It doesn't load (a syntax error, no <code>trigger</code>, an error at the top level)</td><td>Nothing runs; the error shows in the panel (its last lines: click it for the editor, with the line). Fix it and Apply, or Restart.</td></tr>
        <tr><td><code>trigger</code> raises</td><td>That block is skipped and counted as <b>failed</b>; the traceback shows; the next block runs as usual.</td></tr>
        <tr><td>A block takes longer than 5 s</td><td>The Python process is stopped (its globals lost) and the trigger waits: Restart or Apply starts it again.</td></tr>
        <tr><td>Python can't be found</td><td>Says so: install Python 3, or give its path under <b>Python</b>.</td></tr>
      </tbody>
    </table>
    <p><b>Check</b> (in the editor) loads the draft in a fresh Python and runs it on up to the last second of samples: whether it loads, how many triggers it would find there (the first 20 with their labels), what it printed, and the error with its line (<b>Go to line</b>).</p>
  </section>

  <section id="sr-speed">
    <h3>Keeping up</h3>
    <p>At 2 MS/s the script gets two million samples a second. The panel shows its <b>ms per million</b> samples: it must stay well under 500 (half a second for a second of samples) to keep up. If it falls more than 2 s behind, it skips ahead to the newest samples and says how much it skipped.</p>
    <ul>
      <li><b>Use numpy on whole arrays</b>, not Python loops over samples: <code>np.flatnonzero(i &gt; LEVEL)</code> is about a hundred times faster than <code>for x in i</code>.</li>
      <li>Loop over <i>events</i> (edges, runs), not samples: there are few of them.</li>
      <li>Decimation (Acquisition) gives fewer samples per second, if the detail allows it.</li>
      <li>In Normal or Single mode the Scope holds on each trigger; it doesn't slow the script.</li>
    </ul>
  </section>

  <section id="sr-patterns">
    <h3>Patterns</h3>
    <h4>An edge across blocks: keep the last sample</h4>
    <pre><code>last = None
def trigger(data):
    global last
    x = data.v
    prev = np.concatenate(([x[0] if last is None else last], x[:-1]))
    last = x[-1]
    return data.t[(prev &gt;= 3.0) &amp; (x &lt; 3.0)]          # falling through 3.0 V</code></pre>
    <h4>A window of fixed length: buffer the samples</h4>
    <pre><code>buf, t0 = np.zeros(0), None
def trigger(data):
    global buf, t0
    if t0 is None:
        t0 = data.t[0]
    buf = np.concatenate((buf, data.v))
    n = int(0.01 / data.dt)                               # 10 ms
    found = []
    while len(buf) &gt;= n:
        w, buf = buf[:n], buf[n:]
        if np.ptp(w) &gt; 0.05:                              # more than 50 mV p-p in it
            found.append((t0, f"{"{"}np.ptp(w) * 1e3:.0f{"}"} mV p-p"))
        t0 += n * data.dt
    return found</code></pre>
    <h4>Decide later: watch something after it, then report</h4>
    <pre><code>pending = []                                    # [start, lowest V]
def trigger(data):
    # … first, add this block's new starts to pending: [t, np.inf] …
    for p in pending:                           # watch each for 3 ms after its start
        sel = (data.t &gt;= p[0]) &amp; (data.t &lt; p[0] + 3e-3)
        if sel.any():
            p[1] = min(p[1], float(data.v[sel].min()))
    done = [p for p in pending if data.t[-1] &gt;= p[0] + 3e-3]
    pending[:] = [p for p in pending if p not in done]
    return [(t, f"{"{"}v:.2f{"}"} V") for t, v in done if v &lt; 2.9]</code></pre>
    <h4>Learn what's usual as it runs</h4>
    <p>Keep the last values (periods, charges, noise levels) in a list, trimmed (<code>del xs[:-20]</code>), and compare each new one with their median: the examples <i>Missed heartbeat</i>, <i>Period jitter</i> and <i>Noisier than usual</i> do.</p>
  </section>

  <section id="sr-python">
    <h3>Python and safety</h3>
    <ul>
      <li><b>Python</b> (the trigger panel): the interpreter. Empty: <code>python3</code> (<code>python</code> on Windows). A path to a virtual environment's <code>python</code> uses its packages: <code>python3 -m venv ~/nb &amp;&amp; ~/nb/bin/pip install numpy</code>.</li>
      <li>The script runs as you, with your rights, like any program you start: <b>run only scripts you trust</b>. A saved configuration can carry a script's code, but never turns the Script type on or chooses the interpreter: you do.</li>
      <li>Scripts saved in the editor (<b>Save</b>) are kept on this computer; <b>Save file…</b> writes a <code>.py</code> to share.</li>
    </ul>
  </section>

  <section id="sr-examples">
    <h3>The examples</h3>
    <p>In the editor under <b>Examples</b>, each complete and tested on synthetic data. Start from the closest and change its constants (in capitals, at the top).</p>
    {#each EXAMPLE_GROUPS as g (g.group)}
      <h4>{g.group}</h4>
      <dl class="ex">
        {#each g.examples as e (e.id)}
          <dt>{e.title}</dt>
          <dd>{e.about}</dd>
        {/each}
      </dl>
    {/each}
  </section>
</div>

<style>
  .ref {
    font-size: 13px;
    line-height: 1.5;
  }
  nav {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    padding-bottom: 8px;
    margin-bottom: 4px;
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: -14px;
    background: var(--panel);
    z-index: 1;
  }
  .linkish {
    background: none;
    border: 0;
    padding: 2px 0;
    color: var(--accent);
    cursor: pointer;
    font-size: 12px;
  }
  section {
    scroll-margin-top: 44px;
    padding-top: 4px;
  }
  h3 {
    margin: 14px 0 6px;
    font-size: 14px;
    color: inherit;
    text-transform: none;
    letter-spacing: 0;
  }
  h4 {
    margin: 10px 0 4px;
    font-size: 12px;
    color: var(--muted);
  }
  p,
  ul {
    margin: 4px 0;
  }
  ul {
    padding-left: 20px;
  }
  li {
    margin: 2px 0;
  }
  pre {
    margin: 6px 0;
    padding: 8px 10px;
    border-radius: var(--r);
    background: var(--scope-bg, var(--bg));
    border: 1px solid var(--border);
    overflow: auto;
    font: 12px/1.45 ui-monospace, monospace;
  }
  code {
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    margin: 6px 0;
  }
  th,
  td {
    text-align: left;
    vertical-align: top;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 12px;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    margin: 4px 0;
  }
  dt {
    font-weight: 600;
  }
  dd {
    margin: 0;
  }
  dl.ex dt {
    font-weight: 500;
  }
  dl.ex dd {
    color: var(--muted);
  }
</style>
