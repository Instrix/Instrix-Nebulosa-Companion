// Tutorials: one feature at a time, hands-on, on the demo device. Each step
// says what to do (`task`); when the app shows it done (`done`), the step
// ticks and moves on. Steps that need a starting point set it up (`enter`).

import type { Showcase } from './showcases';
import { TIME_DIVS } from './view.svelte';

/** the dual marker the reader is working with: the active one, else the newest */
function theDual(markers: readonly { id: number; t0: number; t1: number | null }[], active: number | null) {
  const duals = markers.filter((m) => m.t1 != null);
  return duals.find((m) => m.id === active) ?? duals[duals.length - 1] ?? null;
}

export const TUTORIALS: Showcase[] = [
  {
    kind: 'tutorial',
    id: 't-markers',
    title: 'Markers',
    blurb: 'Place single and dual markers, read their mean and charge, move, fit and delete them.',
    features: ['Single / dual', 'Δt, mean, ∫', 'Marker list', 'Edit'],
    scenario: 'iot',
    steps: (c) => {
      let snap = { t0: 0, t1: 0, id: 0, div: 0, end: 0, n: 0 };
      return [
        {
          title: 'Markers',
          body: 'Markers measure a moment or a span of the plots. The demo device is a sensor: about 200 µA idle, with a 25 mA burst every half second. Do each task on the card and it ticks itself off. <b>Next</b> skips a task.',
          enter: () => {
            c.mode('scope');
            c.trigger({ mode: 'roll' });
            c.clearMarkers();
            c.bottomTab('markers');
            c.live(0.1);
          },
        },
        {
          target: 'strip',
          title: 'Hold the screen',
          body: 'Live, the plots roll on and markers slide away with their data. Hold the screen first.',
          task: 'Press <kbd>Space</kbd>, or click <b>LIVE</b> so it reads <b>HOLD</b>.',
          done: () => !c.look.live(),
        },
        {
          target: 'plots',
          title: 'A single marker',
          body: 'A single marker marks one moment, and reads each signal’s value there.',
          task: '<b>Double-click</b> a plot, or press <kbd>M</kbd>.',
          done: () => c.look.markers().some((m) => m.t1 == null),
        },
        {
          target: 'plots',
          title: 'A dual marker',
          body: 'A dual marker spans a stretch of time. It gives Δt, and for every sample in it the mean, plus the <b>∫</b>: charge for current, energy for power.',
          task: '<b>Drag across</b> one burst on any plot, or press <kbd>D</kbd>.',
          done: () => c.look.markers().some((m) => m.t1 != null),
        },
        {
          target: 'plots',
          title: 'Reading it',
          body: 'The box at its right edge shows <b>μ</b> (the mean) and <b>∫</b> on each plot. The meter on the right now shows statistics of this span (<b>Sel</b>): σ, min, max.',
        },
        {
          target: 'plots',
          title: 'Adjust it',
          body: 'In the bar above the plots, drag a marker’s body to move it, or its edges to resize it. Click a marker to make it the active one.',
          task: 'Drag an <b>edge</b> of the dual marker in the bar above the plots.',
          enter: () => {
            const d = theDual(c.look.markers(), c.look.active());
            snap = { ...snap, id: d?.id ?? 0, t0: d?.t0 ?? 0, t1: d?.t1 ?? 0 };
          },
          done: () => {
            const d = c.look.markers().find((m) => m.id === snap.id);
            return !!d && (d.t0 !== snap.t0 || d.t1 !== snap.t1);
          },
        },
        {
          target: 'lists',
          title: 'The marker list',
          body: 'Every marker, with its time, Δt, means and integrals. <b>Analyze</b> opens histograms and spectra of a dual marker, ⤢ fits the screen to it and ✕ deletes it.',
          task: 'Click <b>⤢</b> on the dual marker’s row.',
          enter: () => {
            snap = { ...snap, div: c.look.divIndex(), end: c.look.end() };
          },
          done: () => c.look.divIndex() !== snap.div || c.look.end() !== snap.end,
        },
        {
          target: 'plots',
          title: 'More on right-click',
          body: 'Right-click a marker, on the plot or in the list, for <b>Analyze</b>, export of its samples, fit and delete. Right-click empty plot to add a marker exactly there.',
        },
        {
          target: 'lists',
          title: 'Delete',
          body: '<kbd>Del</kbd> removes the active marker, ✕ removes one in the list, and <b>Clear</b> removes them all. When you are done, <kbd>Space</kbd> goes back to live.',
          task: 'Delete a marker.',
          enter: () => {
            snap = { ...snap, n: c.look.markers().length };
          },
          done: () => c.look.markers().length < snap.n,
        },
      ];
    },
  },
  {
    kind: 'tutorial',
    id: 't-trigger',
    title: 'Triggers',
    blurb: 'Hold a repeating signal still: modes, level, position, edge, pulse width, single shot.',
    features: ['Auto / Normal / Single', 'Level', 'Position', 'Pulse width'],
    scenario: 'iot',
    steps: (c) => {
      let level = 0;
      return [
        {
          title: 'Triggers',
          body: 'A trigger picks a moment in the signal, such as a current rising through a level, and puts it at the same place on screen each time. The demo device bursts to 25 mA for 40 ms every half second.',
          enter: () => {
            c.mode('scope');
            c.clearMarkers();
            c.openSection('trigger');
            c.trigger({ mode: 'roll', source: 'i', kind: 'edge', slope: 'rise', level: 0.01, position: 0.5, log: false, cond: 'any' });
            c.live(0.1);
          },
        },
        {
          target: 'trigger',
          title: 'The display mode',
          body: '<b>Roll</b>: a chart recorder, no trigger. <b>Auto</b>: the latest trigger, free-running when none comes. <b>Normal</b>: the latest trigger, holding the last one while it waits. <b>Single</b>: one trigger, then stop.',
          task: 'In the Trigger panel, choose <b>Normal</b>.',
          done: () => c.look.trig.mode === 'normal',
        },
        {
          target: 'plots',
          title: 'Time/div',
          body: 'The burst’s rising edge now sits in the middle of the screen (the vertical line), steady. Zoom in to see the edge itself.',
          task: '<b>Wheel</b> over a plot (or the + − by time/div) down to <b>10 ms/div</b> or less.',
          done: () => TIME_DIVS[c.look.divIndex()] <= 0.01,
        },
        {
          target: 'plots',
          title: 'The level',
          body: 'The horizontal line on the current plot is the trigger level. <b>50%</b> in the panel puts it halfway between the screen’s min and max.',
          task: 'Drag the level line up or down, or type a level in the panel, e.g. <b>5 mA</b>.',
          enter: () => {
            level = c.look.trig.level;
          },
          done: () => c.look.trig.level !== level,
        },
        {
          target: 'trigger',
          title: 'Position',
          body: '<b>Position</b> is how much of the screen comes before the trigger. Little of it, to see what follows an event. Most of it, to see what led up to one.',
          task: 'Move the <b>Position</b> slider to about 10 %.',
          done: () => c.look.trig.position <= 0.2,
        },
        {
          target: 'trigger',
          title: 'Rise or fall',
          body: 'An edge trigger fires on the way up (↑), on the way down (↓), or on either (↕).',
          task: 'Choose <b>↓ Fall</b>: the screen now lines up on each burst’s end.',
          done: () => c.look.trig.slope === 'fall',
        },
        {
          target: 'trigger',
          title: 'Pulse width',
          body: '<b>Pulse width</b> fires on a pulse above (or below) the level that is longer or shorter than a width. The bursts last 40 ms: “longer than 30 ms” catches them, “longer than 50 ms” never fires and the badge stays on <b>WAIT</b>.',
          task: 'Set <b>Type</b> to <b>Pulse width</b>, <b>Width</b> to <b>longer than</b>, and type <b>30 ms</b>.',
          done: () => c.look.trig.kind === 'pulse' && c.look.trig.cond === 'gt',
        },
        {
          target: 'trigger',
          title: 'Single shot',
          body: '<b>Single</b> takes the next trigger and stops: the badge reads <b>STOP</b>. <b>Arm</b> waits for another one. <b>Window</b> (in or out of a band) and <b>Timeout</b> (stuck too long) are the other types. Under <b>Noise, holdoff, filter</b>: hysteresis, a dead time after each trigger, and smoothing.',
          task: 'Choose <b>Single</b> and wait for the capture.',
          done: () => c.look.trig.mode === 'single' && c.look.trig.status === 'stop',
        },
      ];
    },
  },
  {
    kind: 'tutorial',
    id: 't-events',
    title: 'Capturing events',
    blurb: 'Log every spike while the screen rolls: the event list, widths and charge, jump to one, export.',
    features: ['Log events', 'Event list', 'Jump to event', 'CSV'],
    scenario: 'glitch',
    steps: (c) => [
      {
        title: 'Capturing events',
        body: 'The trigger can log every time it fires, even when you are not looking at the plots. Each firing is an <b>event</b> with its time. Pulse-width and window events also get a width, peak, mean and charge. The demo device draws 12 mA, with a short spike to ~100 mA every second or two.',
        enter: () => {
          c.mode('scope');
          c.clearMarkers();
          c.bottomTab('markers');
          c.openSection('trigger');
          c.trigger({ mode: 'roll', source: 'i', kind: 'edge', slope: 'rise', level: 0.05, position: 0.3, log: false, polarity: 'high', cond: 'any' });
          c.live(0.5);
        },
      },
      {
        target: 'trigger',
        title: 'Log in Roll mode',
        body: 'The trigger is set to a rising edge through 50 mA. In Roll mode the screen keeps rolling; <b>Log events</b> lists every firing anyway.',
        task: 'Tick <b>Log events</b> in the Trigger panel.',
        done: () => c.look.trig.log,
      },
      {
        target: 'lists',
        title: 'The event list',
        body: 'Each spike adds a row: its number, time and kind.',
        task: 'Open the <b>Events</b> tab below the plots.',
        done: () => c.look.bottomTab() === 'events',
      },
      {
        target: 'trigger',
        title: 'Measure each event',
        body: 'An edge only has a time. A <b>Pulse width</b> trigger measures the whole pulse above the level: its width, peak, mean and charge, in the list.',
        task: 'Set <b>Type</b> to <b>Pulse width</b>.',
        done: () => c.look.trig.kind === 'pulse',
      },
      {
        target: 'lists',
        title: 'Go to one',
        body: 'Clicking an event holds the screen and puts that moment at the trigger position, so you can zoom in and measure it.',
        task: 'Click an event in the list.',
        done: () => !c.look.live(),
      },
      {
        target: 'lists',
        title: 'Keep them',
        body: '<b>CSV</b> exports the whole list, <b>Clear</b> starts it over, and right-click gives more. The core keeps the last 100 000 events. For days of monitoring, run it in the Logger with file logging on.',
      },
    ],
  },
  {
    kind: 'tutorial',
    id: 't-nav',
    title: 'Moving around the scope',
    blurb: 'Live and hold, time/div, panning, the overview, the right-click menu, the layout.',
    features: ['Space', 'Wheel', 'Pan', 'Overview'],
    scenario: 'iot',
    steps: (c) => {
      let snap = { div: 0, end: 0 };
      return [
        {
          title: 'Moving around the scope',
          body: 'The scope keeps the last seconds to minutes of every sample in memory (set by <b>Sample buffer</b>). You can look anywhere in it.',
          enter: () => {
            c.mode('scope');
            c.trigger({ mode: 'roll' });
            c.clearMarkers();
            c.live(0.1);
          },
        },
        {
          target: 'strip',
          title: 'Live and hold',
          body: '<b>LIVE</b> follows the newest data. <b>HOLD</b> stays put while the data keeps coming in behind.',
          task: 'Press <kbd>Space</kbd>.',
          done: () => !c.look.live(),
        },
        {
          target: 'plots',
          title: 'Time per division',
          body: 'The wheel steps time/div in 1-2-5 steps, around the pointer when held: from 1 µs to 60 s per division.',
          task: '<b>Wheel</b> over a plot.',
          enter: () => {
            snap.div = c.look.divIndex();
          },
          done: () => c.look.divIndex() !== snap.div,
        },
        {
          target: 'plots',
          title: 'Pan',
          body: '<b>Shift + wheel</b>, or <kbd>←</kbd> <kbd>→</kbd>, moves through time.',
          task: 'Pan with <b>Shift + wheel</b> or the arrow keys.',
          enter: () => {
            snap.end = c.look.end();
          },
          done: () => Math.abs(c.look.end() - snap.end) > 1e-9,
        },
        {
          target: 'overview',
          title: 'The overview',
          body: 'The strip above the plots is everything in memory. The box is what the plots show.',
          task: 'Drag the box somewhere else.',
          enter: () => {
            snap.end = c.look.end();
          },
          done: () => Math.abs(c.look.end() - snap.end) > 1e-9,
        },
        {
          target: 'plots',
          title: 'Right-click',
          body: 'Right-click a plot for a menu: a marker here, trigger at this level, axis log, linear or fixed, zoom, and export.',
        },
        {
          target: 'strip',
          title: 'Back to live',
          body: '<b>Autoset</b> picks a trigger and time/div from the signal. <b>Reset view</b> puts the plots back as they were. <kbd>[</kbd> and <kbd>]</kbd> hide the side panels, and the dividers drag.',
          task: 'Press <kbd>Space</kbd> to follow the newest data again.',
          done: () => c.look.live(),
        },
      ];
    },
  },
  {
    kind: 'tutorial',
    id: 't-meter',
    title: 'The meter',
    blurb: 'Statistics of the screen, the run or a selection; charge and energy; battery life.',
    features: ['Window / Run / Sel', 'Statistics', 'Accumulators', 'Battery life'],
    scenario: 'ble',
    steps: (c) => [
      {
        title: 'The meter',
        body: 'The panel on the right turns every sample into numbers. The demo device is a BLE beacon: microamps asleep, milliamps every 100 ms.',
        enter: () => {
          c.mode('scope');
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          c.meter('window');
          c.live(0.2);
        },
      },
      {
        target: 'meter',
        title: 'Window, Run, Sel',
        body: '<b>Window</b>: what the screen shows. <b>Run</b>: everything since the stream started, the best average for a repeating load. <b>Sel</b>: the active dual marker.',
        task: 'Click <b>Run</b>.',
        done: () => c.look.meterScope() === 'run',
      },
      {
        target: 'plots',
        title: 'A selection',
        body: 'Drag across a plot: the meter switches to <b>Sel</b> on its own and shows that span.',
        task: '<b>Drag across</b> a plot.',
        done: () => c.look.meterScope() === 'selection',
      },
      {
        target: 'meter',
        title: 'Choose the statistics',
        body: '<b>⚙</b> chooses which numbers show: σ, min, max, peak-to-peak, RMS, sample count, duration. <b>Hold</b> freezes them.',
        task: 'Click <b>⚙</b> and tick another statistic.',
      },
      {
        target: 'meter',
        title: 'Charge, energy, battery life',
        body: '<b>Accumulators</b> add up charge and energy since you cleared them, in C and Ah, J and Wh. <b>Battery life</b> divides a capacity by the mean current of the chosen span.',
        task: 'Type a capacity in mAh (a CR2032 is about 225).',
      },
    ],
  },
  {
    kind: 'tutorial',
    id: 't-math',
    title: 'The math channel',
    blurb: 'A fourth signal from an expression of I, V, P and t: examples, your own, turning it off.',
    features: ['Expressions', 'Examples', 'Units'],
    scenario: 'iot',
    steps: (c) => {
      let expr = '';
      return [
        {
          title: 'The math channel',
          body: 'An expression of <b>I</b>, <b>V</b>, <b>P</b> and <b>t</b>, computed on every sample. It plots like any signal, and works in the meter, markers, triggers, analysis and exports.',
          enter: async () => {
            c.mode('scope');
            c.trigger({ mode: 'roll' });
            c.live(0.2);
            await c.math(null);
            c.openSection('math');
          },
        },
        {
          target: 'math',
          title: 'Start from an example',
          body: 'The examples cover the usual: power in mW, load resistance, charge and energy so far, a moving average, dI/dt, log current.',
          task: 'Pick <b>Charge since start (mAh)</b> from the examples and click <b>Apply</b>.',
          done: () => c.look.mathOn(),
        },
        {
          target: 'plots',
          title: 'A fourth plot',
          body: 'It has its own plot, axis and colour, and a card in the meter. Here it is charge, climbing a step with each burst.',
        },
        {
          target: 'math',
          title: 'Your own',
          body: 'Functions: <code>avg(x, 10ms)</code>, <code>integ(x)</code>, <code>ddt(x)</code>, <code>abs</code>, <code>min</code>, <code>max</code>, <code>log10</code>, <code>sqrt</code>… Numbers take SI prefixes: <code>1u</code>, <code>10ms</code>. Name and unit label it.',
          task: 'Write <code>avg(I, 10ms)*1000</code>, name it <b>I 10 ms</b> with unit <b>mA</b>, and click <b>Update</b>.',
          enter: () => {
            expr = c.look.mathOn() ? c.look.mathExpr() : '';
          },
          done: () => c.look.mathOn() && c.look.mathExpr() !== expr,
        },
        {
          target: 'math',
          title: 'Off',
          body: 'The expression is kept for next time.',
          task: 'Click <b>Off</b>.',
          done: () => !c.look.mathOn(),
        },
      ];
    },
  },
  {
    kind: 'tutorial',
    id: 't-analysis',
    title: 'Analysis and export',
    blurb: 'Histogram, CDF, spectrum and worst window of a span; CSV and PNG.',
    features: ['Analyze', 'Spectrum', 'Max window', 'CSV / PNG'],
    scenario: 'pwm',
    steps: (c) => [
      {
        title: 'Analysis and export',
        body: 'Analysis works on every raw sample of a span: the active dual marker, or else the screen. The demo device is a 2 kHz PWM motor driver.',
        enter: () => {
          c.mode('scope');
          c.analyze(null);
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          c.live(0.01);
        },
      },
      {
        target: 'plots',
        title: 'Pick a span',
        task: 'Press <kbd>Space</kbd> to hold, then <b>drag across</b> a few periods.',
        body: 'A dual marker sets what gets analysed.',
        done: () => c.look.markers().some((m) => m.t1 != null),
      },
      {
        target: 'strip',
        title: 'Open Analysis',
        body: 'The <b>Analyze</b> button is next to Autoset. It is also in the meter, on each dual marker’s row, and in the right-click menu.',
        task: 'Press <kbd>A</kbd>, or click <b>Analyze marker…</b>',
        done: () => c.look.analysisOpen(),
      },
      {
        target: 'analysis',
        title: 'Four views',
        body: '<b>Histogram</b>: how the values spread. <b>CDF / CCDF</b>: the share below or above each value. <b>Spectrum</b>: the frequencies in it. <b>Max window</b>: the worst stretch of a given width, which <b>Show it</b> marks. Choose a signal, then <b>Run</b>.',
        task: 'Open the <b>Spectrum</b> tab and click <b>Run</b>: the peaks are 2 kHz and its harmonics.',
      },
      {
        target: 'strip',
        title: 'Export',
        body: '<b>PNG</b> saves the plots as an image. In the desktop app, <b>CSV</b> saves every sample of the active dual marker (or the screen), and <b>● REC</b> records everything to a file you can open later. Right-click a marker to export just its samples.',
        task: 'Close Analysis (✕).',
        done: () => !c.look.analysisOpen(),
      },
    ],
  },
  {
    kind: 'tutorial',
    id: 't-logger',
    title: 'The logger',
    blurb: 'The whole session at a glance: blocks, hover, zoom down to samples, follow, logging to file.',
    features: ['Logger view', 'Zoom', 'Follow', 'Files'],
    scenario: 'battery',
    steps: (c) => [
      {
        title: 'The logger',
        body: 'The scope holds the recent raw samples; the logger keeps a summary of every interval for the whole session, for days. The demo device is a battery under radio bursts.',
        enter: async () => {
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          await c.newSession();
          c.mode('scope');
        },
      },
      {
        target: 'modes',
        title: 'Switch to the logger',
        body: 'Scope and Logger share the same stream: both run all the time.',
        task: 'Click <b>Logger</b>.',
        done: () => c.look.appMode() === 'logger',
      },
      {
        target: 'logger',
        title: 'Blocks',
        body: 'Each block is one interval: the line is its mean, the band its min–max, so short spikes still show. The header says how long a block is; zoomed out, blocks merge.',
        task: '<b>Hover</b> the plots to read a block’s mean, min, max and σ.',
      },
      {
        target: 'logger',
        title: 'Zoom in',
        body: 'Drag to zoom into a span, or wheel. On recent data, still in the sample buffer, you can zoom past the blocks to single samples.',
        task: '<b>Drag across</b> part of a plot.',
        done: () => !c.look.logView().follow,
      },
      {
        target: 'logger',
        title: 'Follow again',
        body: '<b>All</b>, <b>1 min</b>, <b>1 h</b>… show the latest stretch and follow it. Double-click shows everything.',
        task: 'Click <b>1 min</b>.',
        done: () => c.look.logView().follow && c.look.logView().last === 60,
      },
      {
        target: 'logger-panel',
        title: 'Intervals and files',
        body: 'The interval (1 ms to 10 s) sets the block size: shorter is finer and takes more memory. <b>New session</b> starts over. <b>Continuous</b> logging (desktop app) writes from <b>Start</b> until you stop; <b>Triggered</b> waits for an event and saves the moments around it — the next tutorial. <b>File output</b> chooses CSV, HDF5 and more, per interval or every sample.',
        enter: async () => {
          await c.capture({ mode: 'continuous' });
          c.openSection('logger');
        },
      },
    ],
  },
  {
    kind: 'tutorial',
    id: 't-windows',
    title: 'Bigger panels',
    blurb: 'Open a busy settings section in its own window: everything at once, moved off the plots, changed while you watch.',
    features: ['⤢ Own window', 'Move', 'Live', 'Dock'],
    scenario: 'iot',
    steps: (c) => {
      let snap = { x: 0, y: 0, w: 0, h: 0, level: 0 };
      const w = () => c.look.window('trigger');
      return [
        {
          title: 'Bigger panels',
          body: 'Some sections have a lot in them — the trigger, the logger, the file format, the serial monitor, the math channel. Each has a <b>⤢</b> on its heading that opens it in its own window, bigger and movable, while the plots stay live beside it.',
          enter: () => {
            c.dock('trigger');
            c.mode('scope');
            c.trigger({ mode: 'normal', source: 'i', kind: 'edge', slope: 'rise', level: 0.015, position: 0.3 });
            c.live(0.1);
            c.openSection('trigger');
          },
        },
        {
          target: 'trigger',
          title: 'Pop it out',
          body: 'The Trigger section, in the sidebar. Its <b>⤢</b> is at the right of its heading, beside the <b>?</b>.',
          task: 'Click <b>⤢</b> on the Trigger heading.',
          done: () => w() != null,
        },
        {
          target: 'win-trigger',
          title: 'Everything at once',
          body: 'Wider, the settings flow into columns, and the extras (noise, holdoff, filter) are unfolded. A sketch shows what the trigger fires on — try another <b>Type</b> or <b>Edge</b> and watch it change. The sidebar keeps a line to bring it back.',
        },
        {
          target: 'win-trigger',
          title: 'Out of the way',
          body: 'The window floats over the app. Put it where it doesn’t hide what you’re looking at.',
          task: 'Drag the window by its <b>title bar</b>.',
          enter: () => {
            const r = w();
            if (r) snap = { ...snap, x: r.x, y: r.y };
          },
          done: () => {
            const r = w();
            return !!r && Math.hypot(r.x - snap.x, r.y - snap.y) > 30;
          },
        },
        {
          target: 'plots',
          title: 'Still live',
          body: 'The plots go on as you change settings. The dashed line is the trigger level: change it in the window, or drag the line on the plot — the window follows.',
          task: 'Change the <b>Level</b>, in the window or by dragging the line.',
          enter: () => {
            snap = { ...snap, level: c.look.trig.level };
          },
          done: () => Math.abs(c.look.trig.level - snap.level) > 1e-9,
        },
        {
          target: 'win-trigger',
          title: 'Put it back',
          body: '<b>⤡</b> (or <kbd>Esc</kbd> in the window) puts it back in the sidebar. A window reopens where you left it, and stays open across restarts until you dock it.',
          task: 'Click <b>⤡</b> in the window’s title bar.',
          done: () => w() == null,
        },
        {
          target: 'sidebar',
          title: 'The others',
          body: 'The same <b>⤢</b> is on the <b>Logger</b>, <b>File output</b>, <b>Serial monitor</b> and <b>Math channel</b> sections. <kbd>Ctrl</kbd> <kbd>K</kbd> › <i>… in its own window</i> opens any of them from the keyboard. Several can be open at once; a click brings one to the front.',
        },
      ];
    },
  },
  {
    kind: 'tutorial',
    id: 't-capture',
    title: 'Catch a rare event',
    blurb: 'A rolling window that saves only when the trigger fires: set the trigger by eye on the Logger, watch it, arm it, and get the moments before and after.',
    features: ['Triggered capture', 'Pre-trigger', 'Drag the level', 'Watch / Arm'],
    scenario: 'rare',
    steps: (c) => {
      let snap = { level: 0, count: 0 };
      return [
        {
          title: 'Catch a rare event',
          body: 'The demo device is a sensor with a fault: once in a while, every 10 to 30 s, its radio shorts, the supply sags and the board resets. Logging for hours to find a few seconds is a waste; a <b>triggered capture</b> keeps a rolling window in memory and saves only when the trigger fires — with what came <i>before</i> it.',
          enter: async () => {
            c.trigger({ mode: 'roll', log: false });
            await c.capture({ mode: 'continuous', source: 'trigger', pre: 2, post: 2, again: false, interval: 0.01, data: 'summary' });
            c.captureState('idle');
            await c.newSession();
            c.mode('logger');
            c.openSection('logger');
          },
        },
        {
          target: 'log-start',
          title: 'Two ways to log',
          body: '<b>Continuous</b> writes from Start until Stop. <b>Triggered</b> writes nothing until an event, then saves from a while before it.',
          task: 'Click <b>◎ Triggered</b>.',
          done: () => c.look.capture().mode === 'triggered',
        },
        {
          target: 'capture-trigger',
          title: 'The trigger',
          body: 'The same trigger as the Scope’s, with the same settings: a source, a type, a level. It starts at a <b>rising edge</b> on current through <b>10 mA</b>.',
          enter: () => c.trigger({ source: 'i', kind: 'edge', slope: 'rise', level: 0.01, hysteresis: 0, holdoff: 0, filter: 0 }),
        },
        {
          target: 'capture-status',
          title: 'Watch it first',
          body: '<b>Watch</b> runs the trigger and shows what it does on the plots, without writing anything: a <b>▼</b> for each event, the level as a dashed line.',
          task: 'Click <b>▶ Watch</b>.',
          done: () => c.look.capture().state !== 'idle',
        },
        {
          target: 'logger',
          title: 'Too eager',
          body: 'At 10 mA every radio burst (25 mA, twice a second) fires it: a ▼ on each. Those are normal. The fault pulls over 300 mA; set the level above the bursts. The line can be dragged, like the Scope’s.',
          task: '<b>Drag the dashed line</b> on the current plot up above the bursts (over 50 mA), or type <code>100m</code> in Level.',
          done: () => c.look.trig.level > 0.04,
        },
        {
          target: 'logger',
          title: 'Wait for the fault',
          body: 'Now nothing fires during normal work. The next brown-out comes within half a minute: a tall spike, a drop to nothing (the reset), then the boot. ⌖ beside Level also sets it: click it, then a point on a plot.',
          task: 'Wait for a <b>▼</b> on a fault.',
          enter: () => {
            snap = { level: c.look.trig.level, count: c.look.trig.count };
          },
          done: () => c.look.trig.count > snap.count,
        },
        {
          target: 'capture-window',
          title: 'Before and after',
          body: '<b>Before</b>: how much from before the trigger a capture starts with. It comes out of memory — the sample buffer for every sample, the session log per interval — and the shaded band at the right of the plots shows it. <b>After</b>: how long it goes on — or tick <b>No end</b> to keep going until you stop it.',
          enter: () => c.capture({ pre: 2, post: 2 }),
        },
        {
          target: 'capture-status',
          title: 'Arm',
          body: 'Armed, the next trigger starts a capture. Leave it running overnight if the fault is rarer.',
          task: 'Click <b>◎ Arm</b>.',
          enter: () => {
            snap = { ...snap, count: c.look.capture().made };
          },
          done: () => c.look.capture().state === 'armed' || c.look.capture().made > snap.count,
        },
        {
          target: 'logger',
          title: 'Captured',
          body: 'When the fault fires, the capture starts 2 s before it and ends 2 s after: the blue span on the plots, with the trigger as a line. In the desktop app it is a file in Documents/Nebulosa; in the browser it downloads.',
          task: 'Wait for the next fault.',
          done: () => c.look.capture().made > snap.count && !c.look.capture().capturing,
        },
        {
          target: 'logger-panel',
          title: 'More',
          body: '<b>Again after each capture</b> arms again after each one, a file each. <b>Starts on</b> can also be a <b>serial message</b> — the board’s own error print (the showcase <i>Catch a brown-out reset</i>). <b>Every sample</b> saves the raw samples, as far back as the sample buffer holds.',
        },
      ];
    },
  },
  {
    kind: 'tutorial',
    id: 't-serial',
    title: 'Serial monitor',
    blurb: 'Capture a board’s serial prints as markers on the power log, trigger on a message, and send to it.',
    features: ['Markers', 'Trigger on text', 'Send', 'RTS / DTR'],
    scenario: 'iot',
    steps: (c) => [
      {
        title: 'Serial monitor',
        body: 'Boards usually print what they are doing over a serial port. The monitor timestamps each line against the measurements and marks it on the logger, so you can see which message goes with which current spike. This runs on a synthetic feed; a real board uses a serial port.',
        enter: async () => {
          c.trigger({ mode: 'roll' });
          c.clearMarkers();
          await c.newSession();
          c.mode('logger');
          c.serialFeed(true);
          c.openSection('monitor');
        },
      },
      {
        target: 'monitor',
        title: 'Open a port',
        body: 'On real hardware you set the <b>baud</b>, framing (data bits, parity, stop bits), flow control and the <b>RTS / DTR</b> lines, then <b>Open port</b> — a serial port in the desktop app, Web Serial in the browser. Here the lines arrive on their own.',
      },
      {
        target: 'logger',
        title: 'Lines become markers',
        body: 'Every message is a dashed marker on the logger, labelled at the top, at the moment it arrived. A “tx packet” line sits on the current burst that sent it. Click a message in the list to open that moment in the scope.',
      },
      {
        target: 'monitor',
        title: 'Trigger on a message',
        body: 'Open <b>Line handling and trigger</b>. <b>Trigger on a message</b> watches for a line that contains your text (or matches a regular expression) and marks it in <b>red</b> — for finding an error print, a state change, a reset.',
        task: 'Tick <b>Trigger on a message</b> and type <code>tx packet</code>.',
        done: () => c.look.serialTriggerOn(),
      },
      {
        target: 'logger',
        title: 'Matches stand out',
        body: 'Each matching line is now a red marker, and the section header counts the hits. <b>Freeze the logger on a match</b> holds the view on one, to study what the power did around it.',
        task: 'Watch for the red markers (the count rises).',
        done: () => c.look.serialMatches() > 0,
      },
      {
        target: 'monitor',
        title: 'Send and control lines',
        body: 'When a port is open, a <b>send box</b> appears with a line-ending (<kbd>\n</kbd>, <kbd>\r</kbd>, …) to talk back to the board — a command, a prompt. <b>RTS</b> and <b>DTR</b> toggle live, so you can reset or bootstrap a board from here. <b>Echo</b> shows what you sent as its own marker.',
      },
      {
        target: 'logger-panel',
        title: 'Saved with the log',
        body: 'When you log to a file (desktop app), the messages are written to a <code>.serial.csv</code> next to it — unix time, measurement time, kind and text — so the prints and the power line up again later.',
        enter: () => c.openSection('logger'),
      },
    ],
  },
  {
    kind: 'tutorial',
    id: 't-sca',
    beta: 'sca',
    title: 'Break AES with power analysis',
    blurb: 'Capture power traces, see the leak (TVLA), then recover the whole AES key by correlation — on the demo target.',
    features: ['Capture', 'T-test', 'CPA', 'Key recovery'],
    scenario: 'target',
    steps: (c) => [
      {
        title: 'Break AES with power analysis',
        body: 'The demo device is now a microcontroller whose current leaks as it runs AES, like a real one. You will capture its power, confirm the leak, and recover the key — the <b>Side-channel</b> workbench, hands-on. (At 2 MS/s this works because the demo target is slow; real silicon must be clocked down too.)',
        enter: () => {
          c.trigger({ mode: 'roll' });
          c.scaOpen();
        },
      },
      {
        target: 'sca-capture',
        title: 'Capture traces',
        body: 'Each <b>capture</b> runs the target many times and cuts the power around each operation into a trace. <b>AES — fixed vs random</b> sends a fixed plaintext half the time and a random one the rest (a TVLA split), which both detects the leak and, later, breaks the key.',
        task: 'Press <b>● Capture</b> and wait for it to fill (a few seconds).',
        done: () => c.look.scaTraces() > 0 && !c.look.scaCapturing(),
      },
      {
        target: 'sca-views',
        title: 'The traces',
        body: 'The <b>Traces</b> view (open now) draws the mean and ±σ over all traces, with a few drawn. The bumps are the 16 S-box lookups of the first round; the spread across them is data-dependent — that is the leak.',
      },
      {
        target: 'sca-views',
        title: 'The t-test (TVLA)',
        body: 'The <b>T-test</b> is Welch’s t per sample between the fixed and random groups. Where |t| passes <b>4.5</b> (the dashed lines) the power depends on the data — proof of a leak, no key needed. A clean target stays inside the lines.',
        task: 'Open the <b>T-test (TVLA)</b> tab.',
        done: () => c.look.scaView() === 'ttest',
      },
      {
        target: 'sca-views',
        title: 'Correlation attack',
        body: 'The <b>Correlation (CPA)</b> view breaks the key. For each of the 256 guesses of a key byte, it correlates the Hamming weight of the first-round S-box output against every sample; the real byte correlates best. <b>Recover key</b> does all 16 in turn.',
        task: 'Open the <b>Correlation (CPA)</b> tab, then press <b>Recover key</b>.',
        done: () => c.look.scaKeyDone(),
      },
      {
        target: 'sca-views',
        title: 'The key falls out',
        body: 'The bar chart shows one guess far above the rest — the key byte. The readout at the top is the recovered key; on the demo it is checked against the real one (✓). On real traces nothing is known: the tallest bar, well clear of the pack, is the answer.',
      },
      {
        target: 'sca-views',
        title: 'Take it further',
        body: '<b>Align</b> lines traces up before the statistics (for jitter). <b>Timing</b> breaks a password that compares byte by byte. <b>Export</b> writes the campaign as a NumPy <code>.npz</code> for ChipWhisperer’s analyzer, Lascar or SCAred. For a real board, clock it slow and trigger on the operation.',
      },
    ],
  },
];
