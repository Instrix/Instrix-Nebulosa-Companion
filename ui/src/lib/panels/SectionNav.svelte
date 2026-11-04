<script lang="ts">
  // A sticky jump bar for the settings: one icon per section (click to open it
  // and scroll to it), and a button to open or close them all. Makes the long
  // folding sidebar quick to move around.
  import Icon from './Icon.svelte';

  let { items }: { items: { id: string; icon: string; title: string }[] } = $props();

  let allOpen = $state(false);

  function jump(id: string) {
    dispatchEvent(new CustomEvent('nebulosa:open-section', { detail: id }));
    requestAnimationFrame(() => {
      const el = document.querySelector(`[data-sec="${id}"]`);
      el?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      el?.classList.add('flash');
      setTimeout(() => el?.classList.remove('flash'), 600);
    });
  }

  function toggleAll() {
    allOpen = !allOpen;
    dispatchEvent(new CustomEvent('nebulosa:all-sections', { detail: allOpen }));
  }
</script>

<nav class="snav" aria-label="Jump to a settings section">
  {#each items as it (it.id)}
    <button class="jump" title={it.title} aria-label={it.title} onclick={() => jump(it.id)}>
      <Icon name={it.icon} size={16} />
    </button>
  {/each}
  <span class="sp"></span>
  <button class="jump all" title={allOpen ? 'Collapse all' : 'Expand all'} aria-label={allOpen ? 'Collapse all' : 'Expand all'} onclick={toggleAll}>
    {allOpen ? '⊟' : '⊞'}
  </button>
</nav>

<style>
  .snav {
    position: sticky;
    top: 0;
    z-index: 4;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 1px;
    padding: 4px 2px;
    margin: 0 -16px 4px;
    padding-left: 14px;
    padding-right: 14px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }
  .jump {
    display: grid;
    place-items: center;
    width: 26px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .jump:hover {
    background: var(--bg);
    color: var(--accent);
  }
  .all {
    font-size: 14px;
    line-height: 1;
  }
  .sp {
    flex: 1;
  }
</style>
