<script lang="ts">
  // Keys and mouse gestures, in one place (? or F1).
  let { onclose }: { onclose: () => void } = $props();

  const GROUPS: [string, [string, string][]][] = [
    [
      'Scope',
      [
        ['Space', 'Live / Hold'],
        ['Wheel', 'Time per division (around the pointer when held)'],
        ['Shift + wheel, ← →', 'Pan'],
        ['+ / −', 'Zoom in / out'],
        ['Drag across a plot', 'Dual marker (a window: Δt, mean, ∫)'],
        ['Double-click a plot', 'Single marker'],
        ['M / D', 'Add a single / dual marker'],
        ['Drag a marker or its edges', 'Move it (in the bar above the plots)'],
        ['Del / Backspace', 'Delete the active marker'],
        ['Esc', 'Deselect'],
        ['Drag the trigger line', 'Set the trigger level'],
        ['Right-click', 'Menu: markers, trigger here, axis, zoom, export'],
        ['Drag the overview box', 'Move the window through the history'],
      ],
    ],
    [
      'Logger',
      [
        ['Wheel', 'Zoom'],
        ['Shift + wheel', 'Pan'],
        ['Drag', 'Zoom in on a span'],
        ['Double-click', 'Everything'],
        ['Click', 'Open that moment in the scope'],
      ],
    ],
    [
      'Layout',
      [
        ['[ / ]', 'Hide / show the settings / the meter'],
        ['Drag a divider', 'Resize panels and plots'],
        ['Double-click a divider', 'Reset its size'],
        ['Ctrl + / Ctrl − / Ctrl 0', 'UI scale: bigger / smaller / 100 %'],
        ['? or F1', 'This help'],
      ],
    ],
  ];
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" role="presentation" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" role="dialog" aria-label="Keys and mouse" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Keys and mouse</h2>
      <button class="ghost small" onclick={onclose}>✕</button>
    </header>
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
    border-radius: 10px;
    padding: 14px 18px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
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
    border-radius: 4px;
  }
</style>
