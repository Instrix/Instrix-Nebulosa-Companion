<script lang="ts">
  // Not connected: the devices found (USB, Ethernet), a host by hand or WebUSB pairing, the recordings.
  import { isNative, type Candidate } from '../device';
  import * as native from '../native';
  import { fmtBytes } from '../si';
  import * as web from '../webusb';

  let {
    candidates,
    scanning,
    busy,
    recFiles,
    recDir,
    onscan,
    onconnect,
    onpair,
    onopen,
    onopenfile,
  }: {
    candidates: Candidate[];
    scanning: boolean;
    busy: boolean;
    recFiles: native.RecFile[];
    recDir: string;
    onscan: () => void;
    onconnect: (c: Candidate) => void;
    /** WebUSB: the browser's chooser */
    onpair: () => void;
    /** a recording or log from the list */
    onopen: (path: string) => void;
    onopenfile: () => void;
  } = $props();

  let manualHost = $state('');
</script>

<section>
  <h2>
    Devices
    <button class="ghost small" onclick={onscan} disabled={scanning}>{scanning ? 'Scanning…' : 'Refresh'}</button>
  </h2>
  {#each candidates as c (c.key)}
    <button class="device" onclick={() => onconnect(c)} disabled={busy}>
      <span>{c.label}</span>
      <small>{c.detail}</small>
    </button>
  {:else}
    <p class="muted small">{scanning ? 'Looking…' : 'No devices found.'}</p>
  {/each}

  {#if isNative}
    <form
      class="row"
      onsubmit={(e) => {
        e.preventDefault();
        if (manualHost.trim()) onconnect(native.tcpCandidate(manualHost.trim()));
      }}
    >
      <input placeholder="IP address or host" bind:value={manualHost} />
      <button type="submit" disabled={busy || !manualHost.trim()}>Connect</button>
    </form>
  {:else if web.webUsbAvailable}
    <button class="primary" onclick={onpair} disabled={busy}>Pair USB device…</button>
    <p class="muted small">Browser build: USB only. Ethernet needs the desktop app.</p>
  {:else}
    <p class="warn small">This browser has no WebUSB. Use Chrome or Edge, or the desktop app.</p>
  {/if}
</section>
{#if isNative}
  <section>
    <h2>
      Recordings
      <button class="ghost small" onclick={onopenfile} disabled={busy}>Open file…</button>
    </h2>
    <div class="recs">
      {#each recFiles as f (f.path)}
        <button class="device" onclick={() => onopen(f.path)} disabled={busy} title={f.path}>
          <span>{f.name.replace(/\.nbrec$/, '')}</span>
          <small>{fmtBytes(f.bytes)} · {new Date(f.modified).toLocaleString()}</small>
        </button>
      {:else}
        <p class="muted small">None yet. Connect and press REC.</p>
      {/each}
    </div>
    {#if recDir}<small class="muted path" title={recDir}>{recDir}</small>{/if}
  </section>
{/if}
