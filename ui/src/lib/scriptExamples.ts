// Example trigger scripts (the Script type, Python): script-examples/*.py,
// each complete, in the order of their numbers. Their first lines name them:
//   # title: ...
//   # about: ...   (one line: what it finds)
//   # group: ...   (where it's listed)
// src-tauri/src/script_examples_test.py runs every one on synthetic data.

export interface ScriptExample {
  id: string;
  title: string;
  about: string;
  group: string;
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
      group: meta('group') || 'Examples',
      // the title / about / group lines are for this list, not the editor
      code: text.replace(/^# title: .*\n# about: .*\n(# group: .*\n)?#\n/, ''),
    };
  });

/** the examples by group, in order */
export const EXAMPLE_GROUPS: { group: string; examples: ScriptExample[] }[] = SCRIPT_EXAMPLES.reduce(
  (gs, e) => {
    const g = gs.find((x) => x.group === e.group);
    if (g) g.examples.push(e);
    else gs.push({ group: e.group, examples: [e] });
    return gs;
  },
  [] as { group: string; examples: ScriptExample[] }[],
);

/** a script's name: its example's title, if it is one unchanged */
export function scriptName(code: string): string {
  return SCRIPT_EXAMPLES.find((e) => e.code.trim() === code.trim())?.title ?? 'Custom script';
}
