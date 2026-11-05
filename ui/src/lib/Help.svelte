<script lang="ts" module>
  export type HelpTab = 'start' | 'keys' | 'trouble' | 'about';
</script>

<script lang="ts">
  // Help: getting started, keys and mouse, troubleshooting, about. ? or F1.
  import { isNative } from './device';
  import { hex, type Info } from './nbp';
  // INSTRIX brand logos (Instrix-Docs/brand/logo): the white one only on the dark theme
  import logo from '../assets/instrix-horizontal.svg';
  import logoWhite from '../assets/instrix-horizontal-white.svg';

  let {
    tab = $bindable('start'),
    info = null,
    link = '',
    onclose,
    ontour,
  }: {
    tab?: HelpTab;
    /** the connected device, for About */
    info?: Info | null;
    link?: string;
    onclose: () => void;
    /** start the walkthrough */
    ontour: () => void;
  } = $props();

  const TABS: [HelpTab, string][] = [
    ['start', 'Getting started'],
    ['keys', 'Keys and mouse'],
    ['trouble', 'Troubleshooting'],
    ['about', 'About'],
  ];

  /** Getting started: what to do, in order */
  const START: [string, string][] = [
    [
      'Connect',
      'Plug the board in by USB (SuperSpeed or High-Speed for the full 2 MS/s) or Ethernet. It shows under <b>Devices</b>: click it. No board? <b>Try the demo device</b>.',
    ],
    [
      'Power the analog front end',
      'The measurement side needs 12 V: the barrel jack, or a USB-C charger set to 12 V. In the <b>Board</b> panel, <b>Turn on</b> the analog front end. A banner over the plots says when it is off.',
    ],
    [
      'Choose what to measure',
      '<b>Acquisition</b>: the source (ADC for real measurements), the decimation (N = 1 is every sample, 2 MS/s) and the current ranges (auto, or fixed). <b>Start</b> streams.',
    ],
    [
      'Look at it: the Scope',
      'The plots roll like an oscilloscope. The wheel sets the time per division; <b>Space</b> holds. <b>Autoset</b> picks the trigger, time base and axes from the signal. The overview bar above shows the whole buffer.',
    ],
    [
      'Measure a span',
      'Drag across a plot for a dual marker: its Δt, mean, charge and energy. Double-click for a single marker. The <b>Meter</b> on the right shows statistics of the screen, the run or the selection; <b>Analyze</b> gives histograms, spectra and the worst window.',
    ],
    [
      'Catch events: the trigger',
      'Edge, window, pulse width or timeout on any signal, with noise filtering and holdoff. <b>Normal</b> / <b>Single</b> wait for it; <b>Log events</b> lists every crossing with its width and charge.',
    ],
    [
      'Record for hours or days: the Logger',
      'The <b>Logger</b> view shows the whole session. Log to CSV, TXT, JSON or HDF5 with the fields you choose, per interval or every sample; memory stays bounded. <b>REC</b> keeps every raw sample (.nbrec).',
    ],
    [
      'Catch a rare fault: triggered capture',
      'In the Logger section choose <b>◎ Triggered</b>: nothing is written until the trigger (or a serial message) fires, then the file starts from a while <i>before</i> it. <b>Watch</b> to set the trigger by eye on the plots, then <b>Arm</b>. <b>Learn › Catch a rare event</b> walks through it.',
    ],
    [
      'Keep it up to date',
      '<b>Software update…</b> under Device installs firmware (.nbfw) and gateware (.bit / .bin) over any link; the old gateware stays as a fallback.',
    ],
  ];

  const TROUBLE: [string, string][] = [
    [
      'No device found',
      isNative
        ? 'Linux: install the udev rule (udev/99-nebulosa.rules) and plug the board in again. Windows: bind WinUSB to the device once (Zadig) until the firmware has its own descriptors. Ethernet: same network, or type its IP address.'
        : 'Browsers reach the board by WebUSB only (Chrome, Edge): <b>Pair USB device…</b>. Ethernet needs the desktop app.',
    ],
    ['Readings are flat or noise', 'The analog front end is off, or has no 12 V: see the Board panel. Check the source is the ADC, not a stub.'],
    [
      'lost / stalls in the footer',
      'The link isn’t keeping up: USB Full-Speed carries ~1 MB/s (use N ≥ 16), a hub or a long cable can drop SuperSpeed. Ethernet: a direct cable or a quiet switch.',
    ],
    ['The disk isn’t keeping up', 'Every-sample text logs are big (CSV ~60 MB/s at 2 MS/s): log per interval, use HDF5 or .nbrec, or a faster disk.'],
    ['Plots are cramped', 'Hide the sidebars (<kbd>[</kbd> <kbd>]</kbd>), make the UI smaller (<kbd>Ctrl −</kbd>), drag the dividers.'],
    ['The board misbehaves', 'Board panel → <b>Self-test</b> checks its clocks, supplies, flash, SD card, FPGA, Ethernet and USB PD.'],
  ];

  const GROUPS: [string, [string, string][]][] = [
    [
      'Scope',
      [
        ['Space', 'Live / Hold'],
        ['Wheel', 'Time per division (around the pointer when held)'],
        ['Shift + wheel, ← →', 'Pan (Shift + ← → a whole screen)'],
        ['+ / −', 'Zoom in / out'],
        ['Home / End', 'The oldest samples held / back to live'],
        ['N / Shift + N', 'Next / previous trigger event'],
        [', / .', 'Previous / next serial message'],
        ['< / >', 'Previous / next marker'],
        ['L', 'This moment in the Logger'],
        ['Drag across a plot', 'Dual marker (a window: Δt, mean, ∫)'],
        ['Ctrl (⌘) + drag across a plot', 'Zoom in on that span (holds the view)'],
        ['Double-click a plot', 'Single marker'],
        ['M / D', 'Add a single / dual marker'],
        ['A', 'Analyze the active dual marker (else the screen): histogram, CDF, spectrum, worst window'],
        ['Drag a marker or its edges', 'Move it (in the bar above the plots)'],
        ['Del / Backspace', 'Delete the active marker'],
        ['Esc', 'Deselect'],
        ['Drag the trigger line', 'Set the trigger level'],
        ['Set trigger, then click a plot', 'Trigger on that signal at that level'],
        ['Right-click', 'Menu: markers, trigger here, axis, zoom, export'],
        ['Drag the overview box', 'Move the window through the history'],
      ],
    ],
    [
      'Logger',
      [
        ['Space', 'Live (follow the newest) / Hold'],
        ['Wheel, + / −', 'Zoom'],
        ['Shift + wheel, ← →', 'Pan (Shift + ← → a whole screen)'],
        ['Home / End', 'The start of the session / back to live'],
        [', / .', 'Previous / next serial message'],
        ['Drag', 'Zoom in on a span'],
        ['Double-click', 'Everything'],
        ['Click', 'Open that moment in the scope'],
      ],
    ],
    [
      'Both',
      [
        ['Ctrl K', 'Find a setting or an action'],
        ['Alt ← / Alt →, the mouse’s back / forward', 'Back / forward through the views shown'],
      ],
    ],
    [
      'Layout',
      [
        ['[ / ]', 'Hide / show the settings / the meter'],
        ['Drag a divider', 'Resize panels and plots'],
        ['Double-click a divider', 'Reset its size'],
        ['⤢ on a section heading', 'Its own window (Trigger, Logger, File output, Serial monitor, Math): drag the title to move; it sizes itself to its contents'],
        ['⤡ or Esc in a window', 'Back in the sidebar'],
        ['Ctrl + / Ctrl − / Ctrl 0', 'UI scale: bigger / smaller / 100 %'],
        ['Ctrl 1 / 2 / 3', 'Scope / Logger / Side-channel'],
        ['? or F1', 'Help'],
      ],
    ],
  ];
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" role="presentation" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" role="dialog" aria-label="Help" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Help</h2>
      <div class="tabs" role="tablist">
        {#each TABS as [k, label] (k)}
          <button role="tab" aria-selected={tab === k} class:on={tab === k} onclick={() => (tab = k)}>{label}</button>
        {/each}
      </div>
      <button class="ghost small" onclick={onclose}>✕</button>
    </header>

    {#if tab === 'start'}
      <ol class="steps">
        {#each START as [title, body] (title)}
          <li><b>{title}</b><p>{@html body}</p></li>
        {/each}
      </ol>
      <div class="foot">
        <span class="muted">A guided look at each part of the window:</span>
        <button class="primary" onclick={ontour}>Start the walkthrough</button>
      </div>
    {:else if tab === 'trouble'}
      <dl class="trouble">
        {#each TROUBLE as [q, a] (q)}
          <dt>{q}</dt>
          <dd>{@html a}</dd>
        {/each}
      </dl>
      <p class="muted small">Before a release, docs/hardware-test.md lists what to check on a real board.</p>
    {:else if tab === 'about'}
      <div class="about">
        <img class="instrix light" src={logo} alt="INSTRIX" />
        <img class="instrix dark" src={logoWhite} alt="INSTRIX" />
        <div class="brand"><b>Nebulosa Companion</b></div>
        <p>The desktop and browser app for the INSTRIX Nebulosa power profiler: current, voltage and power at up to 2 MS/s, a scope, a logger for days, triggers, analysis and export.</p>
        <dl>
          <dt>Version</dt><dd>{__APP_VERSION__}{#if __APP_COMMIT__}{' '}<small class="muted">({__APP_COMMIT__})</small>{/if}</dd>
          <dt>Built</dt><dd>{__APP_BUILT__}</dd>
          <dt>Runs as</dt><dd>{isNative ? 'Desktop app (Tauri)' : 'Browser (WebUSB)'}</dd>
          {#if info}
            <dt>Device</dt><dd>{link}</dd>
            <dt>Firmware</dt><dd>{info.fw_version_str} <small class="muted">{info.build}</small></dd>
            <dt>Gateware</dt><dd>{hex(info.fpga_id)}</dd>
            <dt>Protocol</dt><dd>NBP v{info.proto_version}</dd>
          {:else}
            <dt>Device</dt><dd class="muted">not connected</dd>
          {/if}
        </dl>
        <p class="muted small">© INSTRIX. All rights reserved.</p>
      </div>
    {:else}
    <div class="cols">
      {#each GROUPS as [title, rows] (title)}
        <section>
          <h3>{title}</h3>
          <dl>
            {#each rows as [k, what] (k)}
              <dt><kbd>{k}</kbd></dt>
              <dd>{what}</dd>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: #0008;
    display: grid;
    place-items: center;
    z-index: 60;
  }
  .dialog {
    width: min(860px, 94vw);
    max-height: 90vh;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 14px 18px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 10px;
    flex-wrap: wrap;
  }
  header > button {
    margin-left: auto;
  }
  .tabs {
    display: flex;
    gap: 2px;
    flex-wrap: wrap;
  }
  .tabs button {
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    background: none;
    padding: 4px 10px;
    font-size: 13px;
    color: var(--muted);
  }
  .tabs button.on {
    color: inherit;
    border-bottom-color: var(--accent);
  }
  .steps {
    margin: 0;
    padding-left: 22px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
  }
  .steps p,
  .about p {
    margin: 2px 0 0;
    line-height: 1.45;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    margin-top: 14px;
    font-size: 13px;
  }
  .trouble {
    grid-template-columns: 180px 1fr;
    gap: 10px 14px;
  }
  .trouble dt {
    white-space: normal;
    font-weight: 600;
  }
  .about {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
    max-width: 560px;
  }
  .about .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 18px;
  }
  .instrix {
    height: 26px;
    width: auto;
    align-self: flex-start;
    margin: 4px 0 2px;
  }
  .instrix.dark {
    display: none;
  }
  :global(:root[data-theme='dark']) .instrix.light {
    display: none;
  }
  :global(:root[data-theme='dark']) .instrix.dark {
    display: block;
  }
  .about dt {
    color: var(--muted);
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 12px;
  }
  .dialog :global(kbd) {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    padding: 1px 5px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: var(--r-sm);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  h3 {
    margin: 8px 0 6px;
    font-size: 12px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
    gap: 4px 24px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 5px 10px;
    margin: 0;
    font-size: 13px;
  }
  dt {
    white-space: nowrap;
  }
  dd {
    margin: 0;
  }
  kbd {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    padding: 1px 5px;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: var(--r-sm);
  }
</style>
