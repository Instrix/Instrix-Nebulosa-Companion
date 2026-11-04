<script lang="ts" module>
  // Small 16px line icons for the sidebar section headers. One <path> set per
  // name, drawn in currentColor so they take the header's text colour.
  const ICONS: Record<string, string> = {
    // a microcontroller / chip
    device: 'M6 6h8v8H6zM9 3v2M12 3v2M9 17v2M12 17v2M3 9h2M3 12h2M17 9h2M17 12h2',
    // a board with a power plug
    board: 'M3 7h18v11H3zM7 7V4M13 7V4M9 11h6M9 14h4',
    // a live waveform
    acquisition: 'M3 12h3l2-6 3 13 3-9 2 4h5',
    // a trigger edge / bolt
    trigger: 'M3 16h6V8h6v8h6',
    // an eye (what is shown)
    display: 'M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6-10-6-10-6z M12 15a3 3 0 100-6 3 3 0 000 6z',
    // f(x) — the math channel
    math: 'M5 20V7a3 3 0 013-3h1M4 12h6M14 9l6 6M20 9l-6 6',
    // a target — calibration
    calibration: 'M12 3v3M12 18v3M3 12h3M18 12h3M12 16a4 4 0 100-8 4 4 0 000 8z',
    // stacked records — the logger
    logger: 'M4 7c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zM4 7v10c0 1.7 3.6 3 8 3s8-1.3 8-3V7M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3',
    // a terminal — the serial monitor
    monitor: 'M3 5h18v14H3zM6 9l3 3-3 3M12 15h5',
    // a file with an arrow — file output
    output: 'M13 3H6v18h12V8zM13 3v5h5M12 11v6M9 14l3 3 3-3',
    // a plug — connect
    connect: 'M9 3v5M15 3v5M7 8h10v3a5 5 0 01-10 0zM12 16v5',
  };

  export function hasIcon(name?: string) {
    return !!name && name in ICONS;
  }
</script>

<script lang="ts">
  let { name, size = 15 }: { name: string; size?: number } = $props();
</script>

{#if name in ICONS}
  <svg
    class="icon"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path d={ICONS[name]} />
  </svg>
{/if}

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
