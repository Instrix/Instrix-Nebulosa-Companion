<script lang="ts">
  import { ask } from './ask';
  // Saved configurations: save the current settings as a named profile, apply
  // one (all of it or only chosen sections), set one to load on boot, and
  // import / export profiles as .nbcfg files. The advanced load lets you pick
  // which sections of a file to apply.
  import { SECTIONS, listConfigs, saveConfig, deleteConfig, defaultConfigName, setDefaultConfig, asConfig, type Config, type SectionId } from './config';

  let {
    gather,
    onapply,
    onclose,
  }: {
    /** the current settings as a config with this name */
    gather: (name: string) => Config;
    /** apply a config, all of it or only the picked sections */
    onapply: (cfg: Config, pick?: Set<SectionId>) => void;
    onclose: () => void;
  } = $props();

  let configs = $state(listConfigs());
  let def = $state(defaultConfigName());
  let newName = $state('');
  let note = $state('');

  // the advanced loader: a file's config and which sections to apply
  let loaded = $state<Config | null>(null);
  let pick = $state<Set<SectionId>>(new Set());
  let fileInput = $state<HTMLInputElement>();

  const sectionsIn = (c: Config) => SECTIONS.filter((s) => c.sections[s.id] != null);

  function refresh() {
    configs = listConfigs();
    def = defaultConfigName();
  }

  function save() {
    const name = newName.trim();
    if (!name) return;
    saveConfig(gather(name));
    newName = '';
    note = `Saved “${name}”`;
    refresh();
  }

  function applyAll(name: string) {
    const c = configs[name];
    if (c) {
      onapply(c);
      note = `Applied “${name}”`;
    }
  }

  async function remove(name: string) {
    if (!(await ask(`Delete the configuration “${name}”?`))) return;
    deleteConfig(name);
    refresh();
  }

  function makeDefault(name: string) {
    setDefaultConfig(def === name ? null : name);
    refresh();
  }

  function exportConfig(name: string) {
    const c = configs[name];
    if (!c) return;
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([JSON.stringify(c, null, 2)], { type: 'application/json' }));
    a.download = `${name.replace(/[^\w.-]+/g, '_')}.nbcfg`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  }

  async function onFile(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    (e.target as HTMLInputElement).value = '';
    if (!file) return;
    try {
      const c = asConfig(JSON.parse(await file.text()));
      if (!c) {
        note = 'Not a Nebulosa configuration file';
        return;
      }
      loaded = c;
      pick = new Set(sectionsIn(c).map((s) => s.id));
    } catch {
      note = 'Could not read the file';
    }
  }

  function applyLoaded(all: boolean) {
    if (!loaded) return;
    onapply(loaded, all ? undefined : pick);
    note = all ? `Applied all of “${loaded.name}”` : `Applied ${pick.size} section${pick.size === 1 ? '' : 's'} of “${loaded.name}”`;
  }

  function saveLoaded() {
    if (!loaded) return;
    saveConfig(loaded);
    refresh();
    note = `Saved “${loaded.name}”`;
  }

  function toggle(id: SectionId) {
    const n = new Set(pick);
    if (n.has(id)) n.delete(id);
    else n.add(id);
    pick = n;
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
  <div class="dialog" role="dialog" aria-label="Configurations" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>Configurations</h2>
      <button class="ghost small" onclick={onclose} aria-label="Close">✕</button>
    </header>
    <p class="muted small">A configuration is a bundle of settings. Save the current ones, apply a saved one, pick one to load on boot, or import a file and apply only part of it.</p>

    <section>
      <h3>Save the current settings</h3>
      <div class="row">
        <input placeholder="Name…" bind:value={newName} onkeydown={(e) => e.key === 'Enter' && save()} />
        <button class="primary small" onclick={save} disabled={!newName.trim()}>Save</button>
      </div>
    </section>

    <section>
      <h3>Saved</h3>
      {#if Object.keys(configs).length === 0}
        <p class="muted small">None yet.</p>
      {:else}
        <div class="list">
          {#each Object.entries(configs) as [name, c] (name)}
            <div class="item">
              <div class="who">
                <b>{name}</b>
                {#if def === name}<span class="chip">boot</span>{/if}
                <span class="muted small">{sectionsIn(c).length} sections{c.saved ? ` · ${new Date(c.saved).toLocaleDateString()}` : ''}</span>
              </div>
              <div class="acts">
                <button class="small primary" onclick={() => applyAll(name)}>Apply</button>
                <button class="ghost small" class:on={def === name} onclick={() => makeDefault(name)} title="Apply this on boot">{def === name ? '★ boot' : '☆ boot'}</button>
                <button class="ghost small" onclick={() => exportConfig(name)} title="Export to a file">Export</button>
                <button class="ghost small danger" onclick={() => remove(name)} aria-label="Delete">✕</button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>

    <section>
      <h3>Load from a file <span class="muted small">— apply all, or only some sections</span></h3>
      <div class="row">
        <button class="small" onclick={() => fileInput?.click()}>Choose .nbcfg…</button>
        <input type="file" accept=".nbcfg,.json" bind:this={fileInput} onchange={onFile} hidden />
        {#if loaded}<span class="muted small">{loaded.name}</span>{/if}
      </div>
      {#if loaded}
        <div class="picks">
          {#each sectionsIn(loaded) as s (s.id)}
            <label class="pick" title={s.detail}>
              <input type="checkbox" checked={pick.has(s.id)} onchange={() => toggle(s.id)} />
              <span>{s.label}</span>
            </label>
          {/each}
        </div>
        <div class="row">
          <button class="primary small" onclick={() => applyLoaded(false)} disabled={!pick.size}>Apply selected ({pick.size})</button>
          <button class="small" onclick={() => applyLoaded(true)}>Apply all</button>
          <button class="ghost small" onclick={saveLoaded}>Save to list</button>
        </div>
      {/if}
    </section>

    {#if note}<p class="note">{note}</p>{/if}
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
    width: min(620px, 94vw);
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
  h3 {
    margin: 14px 0 6px;
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  h3 span {
    text-transform: none;
    letter-spacing: 0;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .row input:not([type]) {
    flex: 1;
    min-width: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: var(--r);
    background: var(--bg);
    flex-wrap: wrap;
  }
  .who {
    display: flex;
    align-items: baseline;
    gap: 7px;
    min-width: 0;
  }
  .acts {
    display: flex;
    gap: 4px;
  }
  .chip {
    font-size: 10px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--accent);
    color: #fff;
  }
  .picks {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px 12px;
    margin: 6px 0 8px;
  }
  .pick {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .note {
    margin: 10px 0 0;
    font-size: 12px;
    color: var(--ok);
  }
  .danger {
    color: var(--danger);
  }
  .on {
    color: var(--accent);
  }
</style>
