// The "?" of each sidebar section: a quick rundown of its options and how to
// use them. Keyed by the Section id.

export interface HelpItem {
  /** the option, as labelled in the panel */
  name: string;
  text: string;
  /**
   * where it is in the section, for the snapshot's box and arrow: the text a
   * label or button starts with, "~text" it contains, or "$css selector";
   * several are boxed together
   */
  at?: string | string[];
  /** a short list under the text: [term, what it does] */
  list?: [string, string][];
}

export interface SectionHelp {
  /** what the section is for, one or two sentences */
  intro: string;
  items: HelpItem[];
  /** how to use it: short steps or tips */
  tips?: string[];
}

export const SECTION_HELP: Record<string, SectionHelp> = {
  device: {
    intro: 'What you are connected to: firmware, FPGA gateware, the link and the base sample rate.',
    items: [
      { name: 'Firmware / FPGA', text: 'Versions running on the board. A "format 4" FPGA is too old to decode; update it.', at: ['Firmware', 'FPGA'] },
      { name: 'Link', text: 'USB, Ethernet or the demo. The base rate is the rate before decimation.', at: ['Link', 'Base rate'] },
      { name: 'Software update…', text: 'Installs new firmware and gateware on the board.', at: 'Software update' },
    ],
    tips: ['Connecting never starts a measurement: press ▶ Start at the top of the sidebar.'],
  },
  board: {
    intro: "The board's own state: power input, the isolated supplies of the measurement side, clock and self-test.",
    items: [
      { name: 'USB-C / PD', text: 'Power negotiated with the supply. PD errors mean the supply could not give what was asked.', at: 'USB-C' },
      { name: 'Analog front end', text: 'The isolated supplies H and L. Off: no measurement, the plots read zero.', at: 'Analog front end' },
      { name: 'Clock / Sync clock', text: "The Clock row: the board's RTC against this computer. Sync clock sets it to this computer's time, which file timestamps use.", at: '=Sync clock' },
      { name: 'Self-test', text: 'Checks crystals, supplies, flash, SD card, screen, FPGA and Ethernet. Run again after a fix.', at: '=Self-test' },
    ],
  },
  acquisition: {
    intro: 'What is sampled and how fast. Source and range change only while stopped.',
    items: [
      { name: 'Source', text: 'ADC: the real measurement. The stubs are test patterns from the FPGA, handy to check the link.', at: 'Source' },
      { name: 'Decimation N', text: 'Keeps 1 of N samples: 2 MS/s ÷ N. Lower rates mean longer history in the same buffer and smaller files.', at: 'Decimation' },
      { name: 'Mode', text: 'Average: each sample is the mean of N (less noise, short spikes smoothed). Subsample: every Nth sample as is (spikes kept or missed).', at: 'Mode' },
      { name: 'Auto range', text: 'The front end switches current range by itself between Min and Max. Off: fixed at Range.', at: 'Auto range' },
    ],
    tips: ['▶ Start / ■ Stop sits at the top of the sidebar.', 'Throughput and lost data show in the footer; on USB Full-Speed use N ≥ 16.'],
  },
  display: {
    intro: 'Which signals the plots show and how.',
    items: [
      { name: 'Signals', text: 'Current I, voltage V, power P and the math channel M. Each gets its own plot.', at: '$.chips' },
      { name: 'Trace', text: 'Each pixel covers many samples: the line is their mean, the bands their min–max and ±1σ. Toggle each part.', at: 'Trace' },
      { name: 'Auto / Fixed', text: 'Auto fits the y axis to the data; Fixed keeps the Min and Max you type.', at: ['$.axis:has(button[title="Logarithmic scale"]) .seg button:nth-child(2)', '$.axis:has(button[title="Logarithmic scale"]) .seg button:nth-child(3)'] },
      { name: 'Log', text: 'Logarithmic y axis: sleep currents and active bursts on one plot.', at: '$button[title="Logarithmic scale"]' },
    ],
    tips: ['Wheel: time per division; Shift + wheel pans. Space: Live / Hold.', 'Drag across a plot for a dual marker; Ctrl + drag zooms in on the span; double-click for a single marker.'],
  },
  trigger: {
    intro: 'Freeze the scope on an event, or keep it running and just detect and list events.',
    items: [
      { name: '⤢ Own window', text: 'Opens this section in its own window: bigger, laid out in columns, and movable, so you can change settings while you watch the plots. Drag its title bar to move it, its corner to resize it; ⤡ or Esc puts it back. It reopens where you left it.', at: '$.pop' },
      { name: 'Roll', text: 'Free-running, like a chart recorder: the newest data scrolls in, nothing waits. Tick "Log events" to still detect and list them.', at: '$.seg.wide button:nth-child(1)' },
      { name: 'Auto', text: 'Shows each trigger centred at the position you set; when none comes for a while it keeps updating anyway, so you never stare at a frozen screen.', at: '$.seg.wide button:nth-child(2)' },
      { name: 'Normal', text: 'Updates only on a trigger and holds the last one until the next: for events that are rare or irregular.', at: '$.seg.wide button:nth-child(3)' },
      { name: 'Single', text: 'Waits for one trigger, holds it, and stops. Arm to wait for the next: for one-off events you want to keep.', at: '$.seg.wide button:nth-child(4)' },
      { name: 'Source', text: 'The signal watched: I, V, P or the math channel.', at: 'Source' },
      {
        name: 'Type',
        text: 'What counts as an event:',
        at: 'Type',
        list: [
          ['Edge', 'the signal crosses the level: rising, falling or either'],
          ['Window', 'it enters or leaves the band between Low and High'],
          ['Pulse width', 'it goes above (or below) the level and comes back; any width, or only longer / shorter / between'],
          ['Timeout', 'it has stayed above (or below) the level longer than a time: a hang, a missing heartbeat'],
        ],
      },
      { name: 'Edge / When / Above-Below', text: "The type's direction: rise, fall or either for Edge; enter, exit or both for Window; above or below for Pulse and Timeout.", at: ['$.axis:has(button[title="Either"])', 'When', 'Above'] },
      { name: 'Sketch', text: 'In its own window: what this trigger fires on — the signal, the level dashed, ● where it fires. It follows Type, Edge and Above / Below.', at: '$.sketch' },
      { name: 'Level', text: 'The threshold (Window: Low and High). 50% puts it halfway between the screen min and max. You can also drag the trigger line on the plot.', at: ['Level', 'Low'] },
      { name: 'Noise, holdoff, filter', text: "Noise reject: re-arms only after the signal goes back this far, so noise on the level doesn't fire again and again. Holdoff: minimum time between triggers. Filter: triggers on a moving average of the source.", at: 'Noise, holdoff, filter' },
      { name: 'Position', text: 'Where the trigger sits on the screen: how much before it (pre-trigger) and after it you see.', at: 'Position' },
      { name: 'Arm', text: 'Single only: wait for the next trigger.', at: '$button.primary' },
    ],
    tips: [
      '⌖ Set trigger above the plots, then click a plot: source and level from where you click.',
      'Events go to the Events tab below the plots; click one to jump there.',
      'Some options show only in some modes; this picture shows them all.',
    ],
  },
  math: {
    intro: 'A channel computed from the others, live, plotted and logged like the rest.',
    items: [
      { name: '⤢ Own window', text: 'Opens this section in its own window: bigger, laid out in columns, and movable, so you can change settings while you watch the plots. Drag its title bar to move it, its corner to resize it; ⤡ or Esc puts it back. It reopens where you left it.', at: '$.pop' },
      { name: 'Expression', text: 'I (A), V (V), P (W), t (s); + - * / ^; abs sqrt exp ln log10 min max; avg(x, 1ms), integ(x), ddt(x). Numbers take SI prefixes: 10m, 1u.', at: 'Expression' },
      { name: 'Choose…', text: 'Ready-made expressions: power in mW, load resistance, charge (mAh), energy (mWh), 1 ms average, dI/dt…', at: 'Example' },
      { name: 'Name / Unit', text: 'How it is labelled on the plot and in files.', at: ['Name', 'Unit'] },
    ],
    tips: ['Pick an example from Choose…, then edit it. Enter applies.'],
  },
  calibration: {
    intro: 'Turns ADC codes into amps and volts. The firmware scaling is used by default.',
    items: [
      { name: 'Scale / offset', text: 'I = code × full scale / 16384; V = code × scale + offset.', at: ['Range 0', 'Voltage offset'] },
      { name: 'Apply', text: 'Uses your values from now on; plots and files take them.', at: '=Apply' },
      { name: 'Firmware defaults', text: 'Back to the scaling the board reports.', at: '=Firmware defaults' },
    ],
    tips: ['Measure a known load and supply, then adjust until the readings match.'],
  },
  logger: {
    intro: 'The Logger keeps statistics per interval for the whole session, and writes to a file two ways: continuously, or a triggered capture around a rare event.',
    items: [
      { name: '⤢ Own window', text: 'Opens this section in its own window: bigger, laid out in columns, and movable, so you can change settings while you watch the plots. Drag its title bar to move it, its corner to resize it; ⤡ or Esc puts it back. It reopens where you left it.', at: '$.pop' },
      { name: 'Continuous / Triggered', text: 'Continuous: Start writes until you stop. Triggered: a rolling window in memory, nothing written; when the trigger fires, a capture saves from a while before it and goes on.', at: '$.logstart' },
      { name: 'Interval', text: 'Each entry holds min, max, mean and σ of every sample in it. Memory stays bounded: older entries merge.', at: 'Interval' },
      { name: 'Per interval / Every sample', text: 'Per interval: one row of statistics each interval (small files). Every sample: each sample as measured, no averaging (large files; the size per hour is shown).', at: '$.logdata' },
      { name: '● Start logging to file', text: 'Continuous: writes to ~/Documents/Nebulosa in the format set in File output, until stopped. Stops cleanly if the disk runs low (desktop app).', at: '● Start' },
      { name: 'Status', text: 'Triggered: Not watching, WATCHING (the trigger runs, nothing written), ARMED (the next trigger starts a capture) or CAPTURING.', at: '$.capstatus' },
      { name: 'Starts on', text: "The trigger below (the Scope's, shared), or a serial message matching the monitor's Trigger on a message.", at: 'Starts on' },
      { name: 'Trigger', text: 'Source, type, level and the rest, as in the Scope. On the Logger plots: drag the dashed level line, or ⌖ then click a plot; ▼ marks each event.', at: ['$.capfields .axis', '$.capfields select'] },
      { name: 'Before / After', text: 'Any duration (30s, 2min, 1m30s, 1h, 2:05). Before reaches back as far as memory holds: the sample buffer for every sample, the session log (a million intervals) per interval. After: a duration, or empty for until stopped.', at: ['Before', 'After'] },
      { name: 'Again after each capture', text: 'Arms again after each capture: a file each.', at: 'Again after each capture' },
      { name: '▶ Watch / ◎ Arm', text: 'Watch runs the trigger and shows its events and the window on the plots, without writing: set it up by eye. Arm: the next trigger starts a capture. Disarm gives up waiting.', at: ['▶ Watch', '◎ Arm'] },
      { name: 'New session', text: 'Clears the in-memory log and starts over.', at: 'New session' },
    ],
    tips: [
      'For a rare glitch: Triggered, set the trigger, Watch until only the glitch fires, Before 10 s, then Arm and leave it.',
      'In the browser, captures download when they end; continuous logging needs the desktop app.',
      'Learn › Catch a rare event walks through it.',
    ],
  },
  output: {
    intro: 'What the log file looks like.',
    items: [
      { name: '⤢ Own window', text: 'Opens this section in its own window: bigger, laid out in columns, and movable, so you can change settings while you watch the plots. Drag its title bar to move it, its corner to resize it; ⤡ or Esc puts it back. It reopens where you left it.', at: '$.pop' },
      { name: 'Format', text: 'Nebulosa (.nblog/.nbrec): smallest, reopens in the app. CSV, text columns, JSON Lines or HDF5 for other tools.', at: 'Format' },
      { name: 'Per interval / Every sample', text: 'Statistics per interval, or one row per sample at the full rate.', at: '$.seg.wide' },
      { name: 'Columns', text: 'Pick each column: what (time, I, V, P, M…), the statistic (per interval), the unit and decimals. Reorder with the arrows.', at: '$.fields' },
      { name: 'Presets', text: 'Ready-made column sets; edit them after.', at: '$.fields-head select' },
      { name: 'Time', text: 'Seconds from the start, Unix time, or ISO date and time.', at: 'Time' },
    ],
    tips: ['The preview below shows the first lines as they would be written.'],
  },
  monitor: {
    intro: "A serial terminal next to the measurements: every received line becomes a marker on the plots, so you see what the firmware was doing.",
    items: [
      { name: '⤢ Own window', text: 'Opens this section in its own window: bigger, laid out in columns, and movable, so you can change settings while you watch the plots. Drag its title bar to move it, its corner to resize it; ⤡ or Esc puts it back. It reopens where you left it.', at: '$.pop' },
      { name: 'Port / Scan', text: 'Pick the serial adapter; Scan looks again after you plug one in.', at: '$.mon-row:has(select)' },
      { name: 'Baud / Data / Parity / Stop', text: 'Must match the target (115 200 8N1 is the usual). Custom… takes any rate the adapter supports.', at: '$.mon-grid' },
      { name: 'Flow control / RTS / DTR', text: 'Hardware handshake, and the two control lines (some boards reset on DTR).', at: ['Flow control', 'RTS', 'DTR'] },
      { name: 'Open port', text: 'Opens it with these settings; Disconnect closes it. Port settings change while closed.', at: '~Open port' },
      { name: 'Split received on', text: 'The line ending that ends a received message: LF, CR, CRLF or any of them.', at: 'Split received on' },
      { name: 'Send', text: 'Sends the text with the chosen line ending appended.', at: '$.mon-send' },
      { name: 'Trigger on a message', text: 'A text or regex: a match fires a trigger and can freeze the scope there.', at: 'Trigger on a message' },
    ],
    tips: [
      'The Serial tab under the scope lists the messages; click one to see that moment.',
      'Pause freezes the list; the port keeps receiving.',
    ],
  },
};
