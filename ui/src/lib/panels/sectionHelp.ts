// The "?" of each sidebar section: a quick rundown of its options and how to
// use them. Keyed by the Section id.

export interface HelpItem {
  /** the option, as labelled in the panel */
  name: string;
  text: string;
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
      { name: 'Firmware / FPGA', text: 'Versions running on the board. A "format 4" FPGA is too old to decode; update it.' },
      { name: 'Link', text: 'USB, Ethernet or the demo. The base rate is the rate before decimation.' },
      { name: 'Software update…', text: 'Installs new firmware and gateware on the board.' },
    ],
    tips: ['Connecting never starts a measurement: press ▶ Start at the top of the sidebar.'],
  },
  board: {
    intro: "The board's own state: power input, the isolated supplies of the measurement side, clock and self-test.",
    items: [
      { name: 'USB-C / PD', text: 'Power negotiated with the supply. PD errors mean the supply could not give what was asked.' },
      { name: 'Analog front end', text: 'The isolated supplies H and L. Off: no measurement, the plots read zero.' },
      { name: 'Clock / Sync clock', text: "The board's RTC. Sync sets it to this computer's time, which file timestamps use." },
      { name: 'Self-test', text: 'Checks crystals, supplies, flash, SD card, screen, FPGA and Ethernet. Run again after a fix.' },
    ],
  },
  acquisition: {
    intro: 'What is sampled and how fast. Source and range change only while stopped.',
    items: [
      { name: 'Source', text: 'ADC: the real measurement. The stubs are test patterns from the FPGA, handy to check the link.' },
      { name: 'Decimation N', text: 'Keeps 1 of N samples: 2 MS/s ÷ N. Lower rates mean longer history in the same buffer and smaller files.' },
      { name: 'Mode', text: 'Average: each sample is the mean of N (less noise, short spikes smoothed). Subsample: every Nth sample as is (spikes kept or missed).' },
      { name: 'Auto range', text: 'The front end switches current range by itself between Min and Max. Off: fixed at Range.' },
    ],
    tips: ['▶ Start / ■ Stop sits at the top of the sidebar.', 'Throughput and lost data show in the footer; on USB Full-Speed use N ≥ 16.'],
  },
  display: {
    intro: 'Which signals the plots show and how.',
    items: [
      { name: 'Signals', text: 'Current I, voltage V, power P and the math channel M. Each gets its own plot.' },
      { name: 'Trace', text: 'Each pixel covers many samples: the line is their mean, the bands their min–max and ±1σ. Toggle each part.' },
      { name: 'Auto / Fixed', text: 'Auto fits the y axis to the data; Fixed keeps the Min and Max you type.' },
      { name: 'Log', text: 'Logarithmic y axis: sleep currents and active bursts on one plot.' },
    ],
    tips: ['Wheel: time per division; Shift + wheel pans. Space: Live / Hold.', 'Drag across a plot for a dual marker; double-click for a single one.'],
  },
  trigger: {
    intro: 'Freeze the scope on an event, or just detect and list events while it runs.',
    items: [
      { name: 'Roll / Auto / Normal / Single', text: 'Roll: free-running, no trigger. Auto: triggers, or rolls if nothing comes. Normal: waits for each trigger. Single: one trigger, then holds.' },
      { name: 'Source / Level', text: 'The signal and threshold watched. 50% sets the level halfway between the screen min and max.' },
      { name: 'Edge', text: 'Crossing the level: rising, falling or both.' },
      { name: 'Window', text: 'Entering or leaving the band between Low and High.' },
      { name: 'Pulse width', text: 'Above or below the level for longer / shorter than a time.' },
      { name: 'Timeout', text: 'Fires when the signal has stayed on one side of the level this long.' },
      { name: 'Noise, holdoff, filter', text: 'Hysteresis (noise does not re-fire), minimum time between triggers, and a moving-average filter on the source.' },
    ],
    tips: [
      '⌖ Set trigger, then click a plot: source and level from where you click.',
      'Events go to the Events tab below the plots; click one to jump there.',
    ],
  },
  math: {
    intro: 'A channel computed from the others, live, plotted and logged like the rest.',
    items: [
      { name: 'Expression', text: 'I (A), V (V), P (W), t (s); + - * / ^; abs sqrt exp ln log10 min max; avg(x, 1ms), integ(x), ddt(x). Numbers take SI prefixes: 10m, 1u.' },
      { name: 'Choose…', text: 'Ready-made expressions: power in mW, load resistance, charge (mAh), energy (mWh), 1 ms average, dI/dt…' },
      { name: 'Name / Unit', text: 'How it is labelled on the plot and in files.' },
    ],
    tips: ['Pick an example from Choose…, then edit it. Enter applies.'],
  },
  calibration: {
    intro: 'Turns ADC codes into amps and volts. The firmware scaling is used by default.',
    items: [
      { name: 'Scale / offset', text: 'I = code × full scale / 16384; V = code × scale + offset.' },
      { name: 'Apply', text: 'Uses your values from now on; plots and files take them.' },
      { name: 'Firmware defaults', text: 'Back to the scaling the board reports.' },
    ],
    tips: ['Measure a known load and supply, then adjust until the readings match.'],
  },
  logger: {
    intro: 'Long-term logging: the Logger view keeps statistics per interval for the whole session, and can write to a file.',
    items: [
      { name: 'Interval', text: 'Each entry holds min, max, mean and σ of every sample in it. Memory stays bounded: older entries merge.' },
      { name: 'Per interval / Every sample', text: 'Per interval: one row of statistics each interval (small files). Every sample: each sample as measured, no averaging (large files; the size per hour is shown).' },
      { name: '● Start logging to file', text: 'Writes to ~/Documents/Nebulosa in the format set in File output. Stops cleanly if the disk runs low.' },
      { name: 'New session', text: 'Clears the in-memory log and starts over.' },
    ],
    tips: ['Serial messages are saved alongside the log when the monitor is open.', 'Ctrl 2 switches to the Logger view.'],
  },
  output: {
    intro: 'What the log file looks like.',
    items: [
      { name: 'Format', text: 'Nebulosa (.nblog/.nbrec): smallest, reopens in the app. CSV, text columns, JSON Lines or HDF5 for other tools.' },
      { name: 'Per interval / Every sample', text: 'Statistics per interval, or one row per sample at the full rate.' },
      { name: 'Columns', text: 'Pick each column: what (time, I, V, P, M…), the statistic (per interval), the unit and decimals. Reorder with the arrows.' },
      { name: 'Presets', text: 'Ready-made column sets; edit them after.' },
      { name: 'Time', text: 'Seconds from the start, Unix time, or ISO date and time.' },
    ],
    tips: ['The preview below shows the first lines as they would be written.'],
  },
  monitor: {
    intro: "A serial terminal next to the measurements: every received line becomes a marker on the plots, so you see what the firmware was doing.",
    items: [
      { name: 'Port / Scan', text: 'Pick the serial adapter; Scan looks again after you plug one in.' },
      { name: 'Baud / Data / Parity / Stop', text: 'Must match the target (115 200 8N1 is the usual). Custom… takes any rate the adapter supports.' },
      { name: 'Flow control / RTS / DTR', text: 'Hardware handshake, and the two control lines (some boards reset on DTR).' },
      { name: 'Split received on', text: 'The line ending that ends a received message: LF, CR, CRLF or any of them.' },
      { name: 'Send', text: 'Sends the text with the chosen line ending appended.' },
      { name: 'Trigger on a message', text: 'A text or regex: a match fires a trigger and can freeze the scope there.' },
    ],
    tips: [
      'The Serial tab under the scope lists the messages; click one to see that moment.',
      'Pause freezes the list; the port keeps receiving.',
    ],
  },
};
