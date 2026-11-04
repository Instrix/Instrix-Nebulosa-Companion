// Saved configurations (profiles). A config is a bundle of settings grouped
// into sections; the app can gather the current settings into one, apply one
// (all or only chosen sections), keep named ones, and set one to apply on
// boot. Gathering and applying the actual state lives in App.svelte; here are
// the types, the section list, and the localStorage plumbing.

export const CONFIG_VERSION = 1;

/** the sections a config can carry; each can be applied on its own */
export const SECTIONS = [
  { id: 'view', label: 'View', detail: 'Scope / Logger, time per division, meter scope' },
  { id: 'display', label: 'Display', detail: 'Signals shown, traces (mean / band / σ), axis log / fixed' },
  { id: 'acquisition', label: 'Acquisition', detail: 'Source, decimation, current range, sample buffer' },
  { id: 'trigger', label: 'Trigger', detail: 'Mode, source, level, type and its settings' },
  { id: 'logger', label: 'Logger', detail: 'Log interval and file output' },
  { id: 'serial', label: 'Serial monitor', detail: 'Port framing, flow, line handling, trigger' },
  { id: 'sidechannel', label: 'Side-channel', detail: 'Capture plan, traces, samples, level' },
  { id: 'layout', label: 'Layout & appearance', detail: 'Panel widths, folded sections, UI scale, theme' },
] as const;

export type SectionId = (typeof SECTIONS)[number]['id'];
export const SECTION_IDS = SECTIONS.map((s) => s.id) as SectionId[];

export interface Config {
  v: number;
  name: string;
  /** when it was saved, unix ms */
  saved?: number;
  /** one entry per section present */
  sections: Partial<Record<SectionId, unknown>>;
}

const STORE = 'nebulosa.configs';
const DEFAULT = 'nebulosa.defaultConfig';

/** the saved configs, by name */
export function listConfigs(): Record<string, Config> {
  try {
    const v = JSON.parse(localStorage.getItem(STORE) ?? '{}');
    return v && typeof v === 'object' ? v : {};
  } catch {
    return {};
  }
}

export function saveConfig(cfg: Config) {
  const all = listConfigs();
  all[cfg.name] = cfg;
  try {
    localStorage.setItem(STORE, JSON.stringify(all));
  } catch {
    /* storage unavailable */
  }
}

export function deleteConfig(name: string) {
  const all = listConfigs();
  delete all[name];
  try {
    localStorage.setItem(STORE, JSON.stringify(all));
    if (defaultConfigName() === name) setDefaultConfig(null);
  } catch {
    /* storage unavailable */
  }
}

/** the config name applied on boot, or null */
export function defaultConfigName(): string | null {
  try {
    return localStorage.getItem(DEFAULT);
  } catch {
    return null;
  }
}

export function setDefaultConfig(name: string | null) {
  try {
    if (name) localStorage.setItem(DEFAULT, name);
    else localStorage.removeItem(DEFAULT);
  } catch {
    /* storage unavailable */
  }
}

/** validate a parsed object as a Config (from a file) */
export function asConfig(v: unknown): Config | null {
  if (!v || typeof v !== 'object') return null;
  const c = v as Partial<Config>;
  if (typeof c.name !== 'string' || !c.sections || typeof c.sections !== 'object') return null;
  // keep only known sections
  const sections: Partial<Record<SectionId, unknown>> = {};
  for (const id of SECTION_IDS) if (id in c.sections) sections[id] = (c.sections as Record<string, unknown>)[id];
  return { v: c.v ?? CONFIG_VERSION, name: c.name, saved: c.saved, sections };
}
