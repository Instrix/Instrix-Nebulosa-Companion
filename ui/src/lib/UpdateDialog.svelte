<script lang="ts">
  // Check for updates: this version against the latest release on GitHub,
  // its notes, and the download for this computer (opened in the browser).
  import { onMount } from 'svelte';
  import { RELEASES_URL, assetsFor, checkForUpdate, openExternal, skipVersion, skippedVersion, type UpdateCheck } from './appUpdate';

  let {
    found = null,
    onclose,
  }: {
    /** a check already made (the boot check found one); else this checks now */
    found?: UpdateCheck | null;
    onclose: () => void;
  } = $props();

  let res = $state<UpdateCheck | null>(null);
  let err = $state('');
  let busy = $state(false);
  let skipped = $state(skippedVersion());

  const downloads = $derived(res ? assetsFor(res.latest.assets) : []);

  async function check() {
    busy = true;
    err = '';
    try {
      res = await checkForUpdate();
    } catch (e) {
      err = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    if (found) res = found;
    else check();
  });

  async function open(url: string) {
    try {
      await openExternal(url);
    } catch (e) {
      err = (e as Error).message;
    }
  }

  function skip() {
    if (!res) return;
    skipVersion(res.latest.version);
    skipped = res.latest.version;
    onclose();
  }

  const mb = (n: number) => `${(n / 1048576).toFixed(n < 10485760 ? 1 : 0)} MB`;
  const date = (s: string) => (s ? new Date(s).toLocaleDateString() : '');

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
  <div class="dialog" role="dialog" aria-label="Updates" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>{res?.newer ? 'An update is available' : 'Updates'}</h2>
      <button class="ghost small" onclick={onclose} aria-label="Close">✕</button>
    </header>

    <dl>
      <dt>This version</dt>
      <dd>{__APP_VERSION__}</dd>
      <dt>Latest</dt>
      <dd>
        {#if busy}<span class="muted">checking…</span>
        {:else if res}<b>{res.latest.version}</b> <span class="muted small">{date(res.latest.published)}</span>
        {:else}—{/if}
      </dd>
    </dl>

    {#if err}
      <p class="error small">Could not check: {err}.</p>
    {:else if res && !res.newer}
      <p class="ok">✓ You have the latest version.</p>
    {/if}

    {#if res?.newer}
      {#if downloads.length}
        <div class="row">
          <button class="primary" onclick={() => open(downloads[0].url)}>Download {downloads[0].name.replace(/^nebulosa-companion_[^_]+_/, '')}</button>
          <span class="muted small">{mb(downloads[0].size)}</span>
        </div>
        {#if downloads.length > 1}
          <details>
            <summary class="muted small">Other downloads for this system</summary>
            <ul class="assets">
              {#each downloads.slice(1) as a (a.name)}
                <li><button class="ghost small linkish" onclick={() => open(a.url)}>{a.name}</button> <span class="muted small">{mb(a.size)}</span></li>
              {/each}
            </ul>
          </details>
        {/if}
      {/if}
      {#if res.latest.notes.trim()}
        <h3>What's new</h3>
        <pre class="notes">{res.latest.notes.trim()}</pre>
      {/if}
    {/if}

    <div class="row actions">
      <button class="small" onclick={check} disabled={busy}>{busy ? 'Checking…' : 'Check again'}</button>
      <button class="ghost small" onclick={() => open(res?.newer ? res.latest.url : RELEASES_URL)}>Release page</button>
      <span class="grow"></span>
      {#if res?.newer && skipped !== res.latest.version}
        <button class="ghost small" onclick={skip} title="Don't mention this version again on boot">Skip this version</button>
      {/if}
      <button class="small" onclick={onclose}>Close</button>
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
    width: min(560px, 94vw);
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
    font-size: 16px;
  }
  h3 {
    margin: 14px 0 6px;
    font-size: 13px;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 14px;
    margin: 12px 0;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
  }
  .small {
    font-size: 12px;
  }
  .ok {
    color: var(--ok);
  }
  .error {
    color: var(--danger);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 8px;
  }
  .actions {
    margin-top: 16px;
  }
  .grow {
    flex: 1;
  }
  details {
    margin-top: 6px;
  }
  .assets {
    margin: 4px 0 0;
    padding-left: 18px;
  }
  .linkish {
    padding: 0;
    border: 0;
    color: var(--accent);
    text-decoration: underline;
  }
  .notes {
    margin: 0;
    max-height: 220px;
    overflow: auto;
    white-space: pre-wrap;
    font: 12px/1.45 ui-monospace, 'SF Mono', Menlo, Consolas, monospace;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    padding: 8px;
    user-select: text;
  }
</style>
