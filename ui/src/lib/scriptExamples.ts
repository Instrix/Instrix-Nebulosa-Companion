// Example trigger scripts (the Script type, Python): script-examples/*.py,
// each complete. Their first lines name them:
//   # title: ...
//   # about: ...   (one line: what it finds)
// src-tauri's script tests run every one on synthetic data.

export interface ScriptExample {
  id: string;
  title: string;
  about: string;
  code: string;
}

const files = import.meta.glob('./script-examples/*.py', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

export const SCRIPT_EXAMPLES: ScriptExample[] = Object.entries(files)
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([path, text]) => {
    const meta = (k: string) => text.match(new RegExp(`^# ${k}: (.*)$`, 'm'))?.[1].trim() ?? '';
    return {
      id: path.replace(/^.*\/\d*-?|\.py$/g, ''),
      title: meta('title') || path,
      about: meta('about'),
      // the title / about lines are for this list, not the editor
      code: text.replace(/^# title: .*\n# about: .*\n#\n/, ''),
    };
  });

/** a script's name: its example's title, if it is one unchanged */
export function scriptName(code: string): string {
  return SCRIPT_EXAMPLES.find((e) => e.code.trim() === code.trim())?.title ?? 'Custom script';
}
