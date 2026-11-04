// Showcases: the demo device plays a real-world load and a guided walk sets
// the app up step by step to measure it, one feature at a time. Each step
// runs its `enter` when it comes up (forward or back), so any step can be
// reached on its own; the app stays usable underneath to try things.

import type { DemoScenario } from './demo';
import type { Signal } from './nbp';
import type { TourStep } from './Tour.svelte';
import type { TriggerState } from './trigger.svelte';

type TrigSettings = Partial<
  Pick<TriggerState, 'mode' | 'source' | 'kind' | 'slope' | 'level' | 'low' | 'high' | 'polarity' | 'cond' | 'minWidth' | 'maxWidth' | 'timeout' | 'position' | 'log' | 'filter'>
>;

/** what a showcase may do to the app */
export interface ShowcaseCtx {
  /** Scope or Logger */
  mode(m: 'scope' | 'logger'): void;
  /** s per division (a TIME_DIVS step); follows the newest data */
  live(div: number): void;
  /** trigger settings (the rest as they are) */
  trigger(t: TrigSettings): void;
  /** a math channel, or none */
  math(m: { expr: string; name: string; unit: string } | null): Promise<void>;
  /** a log or linear axis */
  logAxis(sig: Signal, on: boolean): void;
  /** the newest sample's time, s; waits until there are `atLeast` s of data */
  newest(atLeast?: number): Promise<number>;
  /** the trigger on screen (waits up to 5 s for one), s */
  triggered(): Promise<number | null>;
  /** holds, puts a dual marker on [t0, t1] and fits the screen to it */
  dual(t0: number, t1: number): void;
  clearMarkers(): void;
  /** Analysis of the active dual marker (else the screen) on this tab, run at once; null closes it */
  analyze(tab: 'histogram' | 'cdf' | 'psd' | 'maxwindow' | null): void;
  meter(scope: 'window' | 'run' | 'selection'): void;
  bottomTab(t: 'markers' | 'events' | 'serial'): void;
  /** unfolds a sidebar section */
  openSection(id: string): void;
  autoset(): Promise<void>;
  /** clears the logger's session (it starts over) */
  newSession(): Promise<void>;
  /** a synthetic serial feed into the monitor (the simultaneous-serial demo) */
  serialFeed(on: boolean): void;
  /** side-channel tab: open it, run a capture, switch view, align, recover the key */
  scaOpen(): void;
  scaCapture(plan: 'aes-tvla' | 'aes-random' | 'password', traces: number, samples: number): Promise<void>;
  scaView(v: 'traces' | 'ttest' | 'timing' | 'cpa' | 'export'): void;
  scaAlign(a?: number, b?: number): Promise<void>;
  scaRecoverKey(): Promise<void>;
  /** reads the app (tutorials check what the reader did) */
  look: {
    markers(): readonly { id: number; t0: number; t1: number | null }[];
    active(): number | null;
    live(): boolean;
    divIndex(): number;
    end(): number;
    trig: TriggerState;
    bottomTab(): 'markers' | 'events' | 'serial';
    meterScope(): string;
    appMode(): 'scope' | 'logger' | 'sca';
    mathOn(): boolean;
    mathExpr(): string;
    logView(): { follow: boolean; last: number };
    analysisOpen(): boolean;
    /** serial monitor: whether the message trigger is armed, and how many lines matched */
    serialTriggerOn(): boolean;
    serialMatches(): number;
    serialMsgs(): number;
    scaTraces(): number;
    scaView(): string;
    scaCapturing(): boolean;
    scaTtestMax(): number;
    scaAligned(): boolean;
    scaKeyDone(): boolean;
    scaKeyCorrect(): boolean;
  };
}

export interface Showcase {
  /** a tutorial teaches one feature, hands-on; a showcase measures a real load */
  kind?: 'tutorial' | 'showcase';
  id: string;
  title: string;
  /** one line: the situation */
  blurb: string;
  /** the features it shows */
  features: string[];
  scenario: DemoScenario;
  steps(c: ShowcaseCtx): TourStep[];
}

/** the start of the most recent whole period of `period` s, at least `back` s before t */
const lastPeriod = (t: number, period: number, back = 0) => Math.floor((t - back) / period) * period;

export const SHOWCASES: Showcase[] = [
  {
    id: 'ble',
    title: 'Battery life of a BLE beacon',
    blurb: 'A coin-cell beacon sleeps at 3 µA and advertises every 100 ms. How long will a CR2032 last?',
    features: ['Log axis', 'Dual markers', 'Charge ∫', 'Meter', 'Battery life'],
    scenario: 'ble',
    steps: (c) => [
      {
        title: 'Battery life of a BLE beacon',
        body: 'The demo device now plays a Bluetooth beacon: asleep at a few µA, waking every 100 ms to advertise. We will measure one advertising event and work out the battery life. <kbd>→</kbd> next, <kbd>←</kbd> back, <kbd>Esc</kbd> leave. You can use the app while this card is up.',
        enter: () => {
          c.mode('scope');
          c.trigger({ mode: 'roll' });
          c.logAxis('i', false);
          c.clearMarkers();
          c.live(0.02);
        },
      },
      {
        target: 'plots',
        title: 'Spikes on a flat line',
        body: 'At 20 ms/div: one wake-up every 100 ms, about 8 mA at the top. Between them the current looks like zero, but that sleep current is most of the beacon’s life.',
        enter: () => {
          c.logAxis('i', false);
          c.live(0.02);
        },
      },
      {
        target: 'plots',
        title: 'A log axis shows both',
        body: 'The current axis is now <b>logarithmic</b>: the 3 µA sleep and the 8 mA bursts are both readable, six decades apart. Right-click any axis to switch it yourself.',
        enter: () => c.logAxis('i', true),
      },
      {
        target: 'plots',
        title: 'One advertising event',
        body: 'A <b>dual marker</b> over one event, zoomed in. There is a CPU wake-up, then three radio bursts, one per advertising channel (37, 38, 39). The box on the plot gives its mean and <b>∫</b>, the charge it took. Drag across any plot to make your own marker.',
        enter: async () => {
          const t = await c.newest(0.3);
          const k = lastPeriod(t, 0.1, 0.15);
          c.clearMarkers();
          c.dual(k - 0.3e-3, k + 6e-3);
        },
      },
      {
        target: 'lists',
        title: 'Every marker, measured',
        body: 'The marker list gives each marker’s Δt, mean current, charge and energy. One event takes about 18 µC. At 10 events a second that is 180 µA on average, against 3 µA of sleep, so the radio decides the battery life.',
      },
      {
        target: 'meter',
        title: 'The meter and battery life',
        body: 'Back to live at 200 ms/div. The meter averages the whole screen, across many events and the sleep between them. <b>Battery life</b> divides the capacity you type in (a CR2032 is about 225 mAh) by that mean.',
        enter: () => {
          c.clearMarkers();
          c.live(0.2);
          c.meter('window');
        },
      },
      {
        target: 'meter',
        title: 'Your turn',
        body: 'Try a different interval: drag a dual marker over exactly one second and compare its charge. Or press <b>Run</b> in the meter to average since the stream started. That average gets better the longer it runs.',
      },
    ],
  },
  {
    id: 'glitch',
    title: 'Catch a rare current spike',
    blurb: 'A steady 12 mA load spikes to ~100 mA for 80 µs, once every second or two. Catch one, then all of them.',
    features: ['Edge trigger', 'Normal / Single', 'Pre-trigger', 'Event log'],
    scenario: 'glitch',
    steps: (c) => [
      {
        title: 'Catch a rare current spike',
        body: 'The demo device now draws a steady 12 mA, except for a short spike to about 100 mA every second or two. That is the kind of thing that resets a board now and then. We will catch the spike with a trigger.',
        enter: () => {
          c.mode('scope');
          c.trigger({ mode: 'roll' });
          c.logAxis('i', false);
          c.clearMarkers();
          c.bottomTab('markers');
          c.live(0.2);
        },
      },
      {
        target: 'plots',
        title: 'Too fast to see',
        body: 'At 200 ms/div a spike is narrower than one pixel. The min–max band still keeps it (it covers every sample in each pixel column), so you see a thin line now and then, but not its shape.',
        enter: () => {
          c.trigger({ mode: 'roll' });
          c.live(0.2);
        },
      },
      {
        target: 'trigger',
        title: 'A trigger on the current',
        body: 'Trigger: a <b>rising edge</b> on current through <b>50 mA</b>, in <b>Normal</b> mode. Each new spike replaces the last one on screen, and the screen holds while it waits for the next. Time/div is 20 µs, with 30 % of the screen before the trigger.',
        enter: () => {
          c.openSection('trigger');
          c.trigger({ source: 'i', kind: 'edge', slope: 'rise', level: 0.05, mode: 'normal', position: 0.3, log: true });
          c.live(20e-6);
        },
      },
      {
        target: 'plots',
        title: 'The spike, up close',
        body: 'Its shape and width, with the voltage dipping as it pulls. The horizontal line at 50 mA is the trigger level: <b>drag it</b> up or down. The badge in the bar above says <b>WAIT</b> or <b>TRIG’D</b>.',
      },
      {
        target: 'lists',
        title: 'Every spike, logged',
        body: 'The <b>Events</b> tab lists each time the trigger fired, with its time, width, peak, mean and charge. Click an event to see it on the plots. Right-click for more, or export the log as CSV.',
        enter: () => c.bottomTab('events'),
      },
      {
        target: 'trigger',
        title: 'Single: one and stop',
        body: '<b>Single</b> takes the next spike and freezes, like a scope’s single shot. To arm it again, choose Single again. A <b>Pulse width</b> trigger would catch only spikes shorter or longer than a width you set.',
        enter: () => c.trigger({ mode: 'single' }),
      },
    ],
  },
  {
    id: 'pwm',
    title: 'A PWM motor: duty, spectrum, worst case',
    blurb: 'A motor driver switching at 2 kHz. Find its period, its duty and its harmonics.',
    features: ['Autoset', 'CDF / histogram', 'Spectrum (PSD)', 'Worst window'],
    scenario: 'pwm',
    steps: (c) => [
      {
        title: 'A PWM motor',
        body: 'The demo device now plays a DC motor driver from 5 V. It switches at 2 kHz with 30 % on-time, and its current ramps while it is on. Here is what <b>Autoset</b> and <b>Analysis</b> make of it.',
        enter: () => {
          c.mode('scope');
          c.analyze(null);
          c.trigger({ mode: 'roll' });
          c.logAxis('i', false);
          c.clearMarkers();
          c.live(0.01);
        },
      },
      {
        target: 'plots',
        title: 'A blur at 10 ms/div',
        body: 'Twenty periods on screen. The band fills from the off current up to the peak, and the shape is lost.',
        enter: () => {
          c.analyze(null);
          c.trigger({ mode: 'roll' });
          c.live(0.01);
        },
      },
      {
        target: 'strip',
        title: 'Autoset',
        body: '<b>Autoset</b> found the levels and the period. It set a trigger halfway up and a time/div that shows a few periods, steady on screen. The note beside the button says what it did.',
        enter: async () => {
          c.analyze(null);
          await c.autoset();
        },
      },
      {
        target: 'analysis',
        title: 'The distribution',
        body: '<b>Analysis</b> (the button next to Autoset, or <kbd>A</kbd>) works on every raw sample of the screen, or of a dual marker. The <b>CCDF</b> gives the share of samples above each current. About 30 % sit above a few mA, which is the on-time, so it reads the duty cycle directly. They spread from 80 to 140 mA: the ramp. The <b>Histogram</b> tab shows the same data as counts.',
        enter: async () => {
          c.trigger({ mode: 'roll' });
          c.live(0.05);
          await c.newest(1);
          c.analyze('cdf');
        },
      },
      {
        target: 'analysis',
        title: 'Spectrum',
        body: 'The <b>spectrum</b> (Welch PSD): a peak at 2 kHz and its harmonics, falling off. A switching frequency that drifts, or ripple from another source, shows up here.',
        enter: () => c.analyze('psd'),
      },
      {
        target: 'analysis',
        title: 'Worst window',
        body: '<b>Max window</b> finds the stretch of a given width with the highest mean: what a regulator or a fuse has to handle. This PWM is even, so its worst 1 ms is barely above the mean. A bursty load gives a big ratio. Click <b>Show it</b> to put a marker on the worst window.',
        enter: () => c.analyze('maxwindow'),
      },
    ],
  },
  {
    id: 'sleep',
    title: 'Deep sleep, six decades down',
    blurb: 'A microcontroller sleeps at under 1 µA and wakes at 4 mA. Measure both, and the charge it uses.',
    features: ['Auto-ranging', 'Log axis', 'Statistics of a span', 'Math channel'],
    scenario: 'sleep',
    steps: (c) => [
      {
        title: 'Deep sleep, six decades down',
        body: 'The demo device now plays a microcontroller in deep sleep at about 0.9 µA. Every second it wakes for 8 ms at 4 mA, and a clock tick blips every 250 ms. Nebulosa switches current ranges sample by sample, so one capture holds both ends.',
        enter: async () => {
          c.mode('scope');
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          await c.math(null);
          c.logAxis('i', false);
          c.live(0.2);
        },
      },
      {
        target: 'plots',
        title: 'Linear: the sleep vanishes',
        body: 'A wake-up every second. The sleep current is 1/4000 of the peak, flat on the floor.',
        enter: () => {
          c.logAxis('i', false);
          c.live(0.2);
        },
      },
      {
        target: 'plots',
        title: 'Log: all of it',
        body: 'On a log axis: the sleep floor near 1 µA, the clock ticks at 600 µA, and the 4 mA wake-ups.',
        enter: () => c.logAxis('i', true),
      },
      {
        target: 'meter',
        title: 'The sleep current, exactly',
        body: 'A dual marker over half a second of pure sleep. The meter now shows <b>that span</b> (Sel): its mean, σ, min and max, from every sample in it.',
        enter: async () => {
          const t = await c.newest(1.6);
          const k = lastPeriod(t, 1, 1.05);
          c.clearMarkers();
          c.dual(k + 0.3, k + 0.49);
          c.meter('selection');
        },
      },
      {
        target: 'math',
        title: 'A math channel',
        body: 'The <b>math channel</b> takes any expression of I, V, P and t, computed on every sample. Here <code>integ(I)/3.6</code> is the charge since it was set, in mAh. It becomes a 4th plot, and it shows up in the meter, markers, triggers and exports too.',
        enter: async () => {
          c.clearMarkers();
          c.meter('window');
          c.live(0.5);
          c.openSection('math');
          await c.math({ expr: 'integ(I)/3.6', name: 'Charge', unit: 'mAh' });
        },
      },
      {
        target: 'plots',
        title: 'Charge, step by step',
        body: 'The charge climbs a step with each wake-up and barely moves while asleep. Its slope is the average current. To turn the math channel off, use <b>Off</b> in its panel.',
      },
    ],
  },
  {
    id: 'boot',
    title: 'Power-up inrush',
    blurb: 'Switched on every 3 s: an inrush spike into the capacitors, boot, flash writes. Capture one power-up.',
    features: ['Single trigger on V', 'Pre-trigger', 'Zoom', 'Markers'],
    scenario: 'boot',
    steps: (c) => [
      {
        title: 'Power-up inrush',
        body: 'The demo device now plays a board that is switched on every 3 s. Its input capacitors charge in a big spike, then it boots, writes flash and settles to idle. A one-shot trigger catches a single power-up.',
        enter: () => {
          c.mode('scope');
          c.trigger({ mode: 'roll' });
          c.logAxis('i', false);
          c.clearMarkers();
          c.live(0.5);
        },
      },
      {
        target: 'plots',
        title: 'Rolling',
        body: 'At 500 ms/div: off, on, off. The spike at each turn-on is too short to see its shape.',
        enter: () => {
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          c.live(0.5);
        },
      },
      {
        target: 'trigger',
        title: 'Single, on the voltage',
        body: 'Trigger on <b>voltage</b> rising through 1.5 V, in <b>Single</b> mode, with 10 % of the screen before it (the <b>position</b>). At 50 ms/div it waits for the next power-up, takes it and stops.',
        enter: () => {
          c.openSection('trigger');
          c.clearMarkers();
          c.trigger({ source: 'v', kind: 'edge', slope: 'rise', level: 1.5, mode: 'single', position: 0.1, log: false });
          c.live(0.05);
        },
      },
      {
        target: 'plots',
        title: 'One power-up',
        body: 'Boot at 45 mA, then flash writes at 70 mA, then idle at 6 mA with a blip every 100 ms. The spike at the very start is the inrush.',
      },
      {
        target: 'plots',
        title: 'The inrush, zoomed',
        body: 'A dual marker over the first 2 ms, fitted to the screen. The peak is about 600 mA, gone in a millisecond. Its <b>∫</b> is the charge into the capacitors. If it trips a supply’s current limit, the board won’t start.',
        enter: async () => {
          const t = await c.triggered();
          if (t == null) return;
          c.clearMarkers();
          c.dual(t - 0.2e-3, t + 2e-3);
        },
      },
      {
        target: 'lists',
        title: 'Measure the phases',
        body: 'Drag across the plot to add markers on the boot and the flash writes. Each one gets its duration, mean and charge here. Press <kbd>Space</kbd> to go back to live.',
      },
    ],
  },
  {
    id: 'battery',
    title: 'A battery discharge in the logger',
    blurb: 'A Li-ion cell drains under radio bursts. Watch the whole run, then drill into single samples.',
    features: ['Logger', 'Min–max blocks', 'Hover values', 'Zoom to samples', 'Logging to file'],
    scenario: 'battery',
    steps: (c) => [
      {
        title: 'A battery discharge',
        body: 'The demo device now plays a device on a Li-ion cell: 8 mA, with 120 mA radio bursts for 0.4 s every 2 s. The cell sags under each burst and runs down. To save you the hours, a full discharge takes 4 minutes here. The <b>Logger</b> keeps the whole run.',
        enter: async () => {
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          await c.newSession();
          c.mode('logger');
        },
      },
      {
        target: 'modes',
        title: 'Scope and Logger',
        body: 'The <b>Scope</b> shows the recent raw samples, microseconds to seconds. The <b>Logger</b> keeps a summary of every interval for the whole session, for days if you want.',
        enter: () => c.mode('logger'),
      },
      {
        target: 'logger',
        title: 'The session',
        body: 'Each block is one log interval: the line is its mean, the band its min–max. The voltage dips under each burst (the cell’s internal resistance). Leave it running a few minutes and the whole trace slides down as the cell drains: press <b>All</b> to see it. <b>Hover</b> to read a block’s mean, min, max and σ.',
      },
      {
        target: 'logger',
        title: 'Zoom to single samples',
        body: '<b>Wheel</b> to zoom, <b>drag</b> to zoom into a span, <b>double-click</b> for everything. On recent data, zooming in goes past the blocks to the raw samples. The header says what each block is.',
      },
      {
        target: 'logger-panel',
        title: 'Log to a file',
        body: 'For long runs: choose the interval and press <b>Start logging to file</b>. The desktop app writes a Nebulosa log, and <b>File output</b> adds CSV, HDF5 and more, per interval or every sample. The board’s SD card can log on its own too.',
        enter: () => c.openSection('logger'),
      },
    ],
  },
  {
    id: 'serial',
    title: 'Serial log alongside power',
    blurb: 'Capture a device’s serial prints as timed markers on the power log, so a spike lines up with what the firmware said.',
    features: ['Serial monitor', 'Log markers', 'Sidecar CSV'],
    scenario: 'iot',
    steps: (c) => [
      {
        title: 'Serial log alongside power',
        body: 'Many boards print over a serial port while they run. The <b>Serial monitor</b> reads those lines and marks each on the logger at the moment it arrived, so you can tell which message goes with which current spike. This demo feeds synthetic lines; a real board uses a serial port.',
        enter: async () => {
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          await c.newSession();
          c.mode('logger');
          c.serialFeed(true);
        },
      },
      {
        target: 'monitor',
        title: 'The monitor',
        body: 'On real hardware you pick a port and baud and press <b>Open port</b> (Web Serial in the browser, a serial port in the desktop app). Here the lines arrive on their own. Each one is timestamped against the measurements.',
        enter: () => c.openSection('monitor'),
      },
      {
        target: 'logger',
        title: 'Messages as markers',
        body: 'Every message is a dashed line on the logger, labelled at the top. A “tx packet” line sits right on the current burst that sent it. Click a message in the list to open that moment in the scope.',
      },
      {
        target: 'logger-panel',
        title: 'Saved with the log',
        body: 'When you log to a file (desktop app), the messages are written to a <code>.serial.csv</code> next to it — unix time, measurement time, and the text — so the two line up again later. Leaving stops the demo feed.',
        enter: () => c.openSection('logger'),
      },
    ],
  },
  {
    id: 'aes',
    title: 'Recover an AES key from power',
    blurb: 'Watch the workbench capture power traces, flag the leak with a t-test, and pull the full AES-128 key out by correlation.',
    features: ['Capture', 'T-test (TVLA)', 'CPA', 'Key recovery'],
    scenario: 'target',
    steps: (c) => [
      {
        title: 'Recover an AES key from power',
        body: 'The demo device is a microcontroller whose current leaks as it runs AES. This showcase drives the <b>Side-channel</b> workbench for you: capture, detect the leak, recover the key. It works at 2 MS/s because the target is slow — real silicon must be clocked down to match.',
        enter: async () => {
          c.trigger({ mode: 'roll' });
          c.scaOpen();
          await c.scaCapture('aes-tvla', 300, 1200);
          c.scaView('traces');
        },
      },
      {
        target: 'sca-views',
        title: 'The traces',
        body: 'Each operation is one trace, cut from the power at the trigger. The mean trace shows the 16 first-round S-box lookups; the ±σ band is wide across them because the power depends on the secret — the leak.',
        enter: () => c.scaView('traces'),
      },
      {
        target: 'sca-views',
        title: 'A t-test proves the leak',
        body: 'Welch’s t per sample between a fixed and a random plaintext: wherever |t| passes 4.5 the power is data-dependent. No key is needed to show a device leaks — this is the standard TVLA test.',
        enter: () => c.scaView('ttest'),
      },
      {
        target: 'sca-cpa',
        title: 'Correlation recovers the key',
        body: 'Now the attack. For each key byte, 256 guesses are correlated against the traces through a Hamming-weight model of the S-box; the right guess stands far above the rest. Here is the whole 16-byte key, recovered and checked against the demo’s.',
        enter: async () => {
          await c.scaRecoverKey();
        },
      },
      {
        target: 'sca-views',
        title: 'Your turn',
        body: 'Try fewer traces and watch the correlation shrink toward the noise, or <b>Align</b> before attacking. <b>Timing</b> breaks the demo’s password check, and <b>Export</b> hands the traces to ChipWhisperer, Lascar or SCAred. The hands-on <b>Break AES</b> tutorial walks you through it step by step.',
      },
    ],
  },
];
