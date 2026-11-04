<script lang="ts">
  // Learn: hands-on tutorials (one feature each), showcases (real loads), and the window tour.
  import { SHOWCASES, type Showcase } from './showcases';
  import { TUTORIALS } from './tutorials';

  let {
    realDevice,
    onpick,
    ontour,
    onclose,
  }: {
    /** a board is connected: a showcase disconnects it for the demo device */
    realDevice: string | null;
    onpick: (s: Showcase) => void;
    ontour: () => void;
    onclose: () => void;
  } = $props();

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="backdrop" role="presentation" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" role="dialog" aria-label="Learn" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Learn</h2>
      <button class="ghost small" onclick={onclose} aria-label="Close">✕</button>
    </header>
    <p class="intro">
      Everything runs on the demo device. A card guides you step by step, and you can use the app while it is up.
      {#if realDevice}<br /><b class="warn">Starting one disconnects {realDevice}.</b>{/if}
    </p>

    <h3>Tutorials <span>one feature at a time: each step gives you something to do, and ticks when it’s done</span></h3>
    <div class="grid small">
      <button class="card tour" onclick={ontour}>
        <span class="title">The window</span>
        <span class="blurb">What each part is for, in a dozen short steps.</span>
        <span class="go">Start ▶</span>
      </button>
      {#each TUTORIALS as s (s.id)}
        <button class="card" onclick={() => onpick(s)}>
          <span class="title">{s.title}</span>
          <span class="blurb">{s.blurb}</span>
          <span class="go">Start ▶</span>
        </button>
      {/each}
    </div>

    <h3>Showcases <span>real measurements, set up for you step by step</span></h3>
    <div class="grid">
      {#each SHOWCASES as s (s.id)}
        <button class="card" onclick={() => onpick(s)}>
          <span class="title">{s.title}</span>
          <span class="blurb">{s.blurb}</span>
          <span class="chips">
            {#each s.features as f (f)}<span class="chip">{f}</span>{/each}
          </span>
          <span class="go">Start ▶</span>
        </button>
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
    width: min(900px, 94vw);
    max-height: 90vh;
    overflow: auto;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    padding: 14px 18px 18px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
  }
  .intro {
    margin: 6px 0 4px;
    font-size: 13px;
    color: var(--muted);
    line-height: 1.45;
  }
  .warn {
    color: var(--warn, #e0a030);
    font-weight: 600;
  }
  h3 {
    margin: 16px 0 8px;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  h3 span {
    margin-left: 8px;
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
    color: var(--muted);
  }
  .grid.small {
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
    gap: 10px;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 12px 14px;
    text-align: left;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    color: inherit;
    cursor: pointer;
    font: inherit;
  }
  .card:hover,
  .card:focus-visible {
    border-color: var(--accent);
  }
  .title {
    font-weight: 600;
    font-size: 14px;
  }
  .blurb {
    font-size: 12.5px;
    line-height: 1.4;
    color: var(--muted);
    flex: 1;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid var(--border);
    color: var(--muted);
  }
  .go {
    align-self: flex-end;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  .tour {
    border-style: dashed;
  }
</style>
