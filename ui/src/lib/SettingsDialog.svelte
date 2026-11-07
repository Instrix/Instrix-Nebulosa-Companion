<script lang="ts">
  // The Companion's own settings (not the instrument's: those are in the side
  // panel and saved with the configurations): the theme, the UI scale, the
  // layout, and the way to the configurations and the walkthrough.
  import { SCALES, THEMES, canScale, setScale, setTheme, uiScale, uiTheme } from './uiScale.svelte';
  import { autoCheck, setAutoCheck } from './appUpdate';
  import { BETAS, beta, setBeta } from './beta.svelte';
  import { isRemote } from './ipc';
  import { isNative } from './device';
  import { DEFAULT_PORT, curlExample, loadRemoteCfg, remoteStart, remoteStatus, remoteStop, saveRemoteCfg, type RemoteStatus } from './remote';
  import { openExternal } from './appUpdate';
  import { onMount } from 'svelte';

  let {
    leftOpen,
    rightOpen,
    ontoggleside,
    onresetlayout,
    onconfigs,
    onupdates,
    ontour,
    onclose,
  }: {
    leftOpen: boolean;
    rightOpen: boolean;
    ontoggleside: (side: 'left' | 'right') => void;
    /** the panel widths and the list height back to their defaults */
    onresetlayout: () => void;
    onconfigs: () => void;
    onupdates: () => void;
    ontour: () => void;
    onclose: () => void;
  } = $props();

  let note = $state('');
  let updatesOnBoot = $state(autoCheck());

  // ---- remote control ----
  const canRemote = isNative && !isRemote;
  let rc = $state(loadRemoteCfg());
  let rs = $state<RemoteStatus | null>(null);
  let rErr = $state('');
  let rBusy = $state(false);
  let copied = $state('');

  onMount(() => {
    if (canRemote) remoteStatus().then((s) => (rs = s), () => undefined);
  });

  async function applyRemote(next: Partial<typeof rc>) {
    rc = { ...rc, ...next, port: Math.max(0, Math.min(65535, Math.round(Number(next.port ?? rc.port) || 0))) };
    saveRemoteCfg(rc);
    rErr = '';
    rBusy = true;
    try {
      rs = rc.on ? await remoteStart(rc) : await remoteStop();
      rc = loadRemoteCfg(); // the token it chose
    } catch (e) {
      rErr = String(e);
      rs = await remoteStatus().catch(() => null);
    } finally {
      rBusy = false;
    }
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = what;
      setTimeout(() => copied === what && (copied = ''), 1500);
    } catch {
      copied = '';
    }
  }

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
  <div class="dialog" role="dialog" aria-label="Settings" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Settings</h2>
      <button class="ghost small" onclick={onclose} aria-label="Close">✕</button>
    </header>

    <section>
      <h3>Theme</h3>
      <div class="themes" role="radiogroup" aria-label="Theme">
        {#each THEMES as t (t.id)}
          {@const [bg, panel, border, text, accent] = t.swatch}
          <button class="theme" class:on={uiTheme.value === t.id} role="radio" aria-checked={uiTheme.value === t.id} onclick={() => setTheme(t.id)} title={t.detail}>
            <span class="preview" class:split={t.id === 'system'} style="--p-bg: {bg}; --p-panel: {panel}; --p-border: {border}; --p-text: {text}; --p-accent: {accent}">
              <span class="p-side"></span>
              <span class="p-main">
                <span class="p-line"></span>
                <span class="p-line short"></span>
                <span class="p-btn"></span>
              </span>
            </span>
            <span class="name">{t.label}</span>
            <span class="muted small detail">{t.detail}</span>
          </button>
        {/each}
      </div>
    </section>

    <section>
      <h3>UI scale</h3>
      {#if canScale}
        <div class="row">
          <select value={uiScale.value} onchange={(e) => setScale(Number((e.currentTarget as HTMLSelectElement).value))}>
            {#each SCALES as v (v)}
              <option value={v}>{Math.round(v * 100)} %</option>
            {/each}
          </select>
          <span class="muted small">Also Ctrl + / Ctrl − / Ctrl 0</span>
        </div>
      {:else}
        <p class="muted small">In a browser the browser's zoom sets it: Ctrl + / Ctrl − / Ctrl 0.</p>
      {/if}
    </section>

    <section>
      <h3>Layout <span class="muted small">— for the view you are in</span></h3>
      <label class="check"><input type="checkbox" checked={leftOpen} onchange={() => ontoggleside('left')} /> Settings panel on the left <kbd>[</kbd></label>
      <label class="check"><input type="checkbox" checked={rightOpen} onchange={() => ontoggleside('right')} /> Meter panel on the right <kbd>]</kbd></label>
      <div class="row">
        <button
          class="small"
          onclick={() => {
            onresetlayout();
            note = 'Panel sizes reset';
          }}>Reset panel sizes</button
        >
        {#if note}<span class="muted small">{note}</span>{/if}
      </div>
    </section>

    <section>
      <h3>Updates <span class="muted small">— version {__APP_VERSION__}</span></h3>
      <label class="check"
        ><input
          type="checkbox"
          bind:checked={updatesOnBoot}
          onchange={() => setAutoCheck(updatesOnBoot)}
        /> Check GitHub for a newer version on start</label
      >
      <div class="row">
        <button class="small" onclick={onupdates}>Check for updates now…</button>
      </div>
    </section>

    {#if canRemote}
      <section>
        <h3>Remote control <span class="muted small">— the Companion in a browser, and an API for scripts</span></h3>
        <label class="check"><input type="checkbox" checked={rc.on} disabled={rBusy} onchange={(e) => applyRemote({ on: (e.currentTarget as HTMLInputElement).checked })} /> Serve the Companion on a port</label>
        <p class="disclaimer small" role="note">
          <b>⚠ Never expose this to the internet.</b> Don't forward the port on your router, put it behind a public proxy or tunnel, or open it in a firewall to the outside. It is meant for this computer or a trusted local network only: whoever reaches it with the link has full control of the instrument and of files and scripts on this computer, and the connection is not encrypted.
        </p>
        <div class="row">
          <label class="inline">Port
            <input class="port" type="number" min="0" max="65535" value={rc.port} disabled={rBusy} onchange={(e) => applyRemote({ port: Number((e.currentTarget as HTMLInputElement).value) || DEFAULT_PORT })} />
          </label>
          <label class="check inline"><input type="checkbox" checked={rc.lan} disabled={rBusy} onchange={(e) => applyRemote({ lan: (e.currentTarget as HTMLInputElement).checked })} /> Also from the local network</label>
        </div>
        {#if rc.lan}
          <p class="warn small">On the network, anyone with the link controls the instrument, reads and writes files and runs trigger scripts on this computer. The link is not encrypted (http): use it on a network you trust only.</p>
        {/if}
        {#if rErr}<p class="error small">{rErr}</p>{/if}
        {#if rs?.running}
          <p class="muted small">Open a link in a browser: it mirrors this window (the same device, settings and streaming; each its own view). {rs.peers} UI{rs.peers === 1 ? '' : 's'} on it now, this one included.</p>
          {#each rs.urls as u (u)}
            <div class="row link">
              <code class="url" title={u}>{u}</code>
              <button class="ghost small" onclick={() => copy(u, u)}>{copied === u ? 'Copied' : 'Copy'}</button>
              <button class="ghost small" onclick={() => openExternal(u)}>Open</button>
            </div>
          {/each}
          <details>
            <summary class="muted small">The API (JSON over HTTP)</summary>
            <p class="muted small">
              <code>GET|POST /api/v1/&lt;action&gt;</code>, the token as <code>Authorization: Bearer …</code> or <code>?t=…</code>; arguments as a JSON body or the query. Actions: status, devices, connect, demo, disconnect, start, stop, measure, value, settings, set, markers, marker, clear_markers, record, log, mode, screenshot.
            </p>
            <div class="row link">
              <code class="url">{curlExample(rs, 'measure?seconds=1')}</code>
              <button class="ghost small" onclick={() => copy(curlExample(rs!, 'measure?seconds=1'), 'curl')}>{copied === 'curl' ? 'Copied' : 'Copy'}</button>
            </div>
          </details>
          <div class="row">
            <button class="small" disabled={rBusy} onclick={() => applyRemote({ token: '' })} title="Links and scripts with the old token stop working">New token</button>
          </div>
        {/if}
      </section>
    {:else if isRemote}
      <section>
        <h3>Remote control</h3>
        <p class="muted small">This browser is remote-controlling the Companion: the device, its settings and streaming are shared with the app's window; the view, theme and layout are this browser's own.</p>
      </section>
    {/if}

    <section>
      <h3>Beta features <span class="muted small">— unfinished: may change or go away</span></h3>
      {#each BETAS as b (b.id)}
        <label class="check beta">
          <input type="checkbox" checked={beta[b.id]} onchange={(e) => setBeta(b.id, (e.currentTarget as HTMLInputElement).checked)} />
          <span><b>{b.label}</b><br /><span class="muted small">{b.detail}</span></span>
        </label>
      {/each}
    </section>

    <section>
      <h3>More</h3>
      <div class="row">
        <button class="small" onclick={onconfigs} title="Save, load and apply setting profiles">Configurations…</button>
        <button class="small" onclick={ontour}>Show the walkthrough</button>
      </div>
    </section>
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
    width: min(640px, 94vw);
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
    margin: 0 0 8px;
    font-size: 13px;
  }
  section {
    margin-top: 16px;
  }
  .small {
    font-size: 12px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 6px;
  }
  .row select {
    width: auto;
  }
  label.check {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 4px 0;
  }
  label.check.beta {
    align-items: flex-start;
  }
  label.check.beta input {
    margin-top: 3px;
  }
  label.inline {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
  }
  .port {
    width: 90px !important;
  }
  .row.link {
    flex-wrap: nowrap;
  }
  .url {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    padding: 3px 6px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    user-select: all;
  }
  .disclaimer {
    margin: 6px 0;
    padding: 6px 8px;
    border: 1px solid var(--warn);
    border-left-width: 3px;
    border-radius: var(--r-sm);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    line-height: 1.35;
  }
  .warn {
    color: var(--warn);
  }
  .error {
    color: var(--danger);
  }
  details {
    margin-top: 6px;
  }
  kbd {
    font-size: 11px;
    padding: 0 4px;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--muted);
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 8px;
  }
  .theme {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 4px;
    padding: 8px;
    text-align: left;
    background: var(--bg);
  }
  .theme.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  .name {
    font-weight: 600;
  }
  .detail {
    line-height: 1.25;
  }
  .preview {
    display: flex;
    height: 54px;
    border-radius: var(--r-sm);
    overflow: hidden;
    border: 1px solid var(--p-border);
    background: var(--p-bg);
  }
  .preview.split {
    background: linear-gradient(135deg, #f6f7f9 50%, var(--p-bg) 50%);
  }
  .p-side {
    width: 28%;
    background: var(--p-panel);
    border-right: 1px solid var(--p-border);
  }
  .preview.split .p-side {
    background: #ffffff;
    border-right-color: #e3e6eb;
  }
  .p-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 8px;
  }
  .p-line {
    height: 4px;
    width: 80%;
    border-radius: 2px;
    background: var(--p-text);
    opacity: 0.85;
  }
  .p-line.short {
    width: 50%;
    opacity: 0.5;
  }
  .p-btn {
    margin-top: auto;
    width: 36%;
    height: 9px;
    border-radius: 3px;
    background: var(--p-accent);
  }
</style>
