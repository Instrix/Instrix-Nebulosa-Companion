/**
 * A yes / no question. In the desktop app the dialog plugin replaces
 * window.confirm with one that returns a Promise (always truthy, so
 * `if (!confirm(...))` never stopped anything); in a browser it returns a
 * boolean. Awaiting covers both.
 */
export async function ask(message: string): Promise<boolean> {
  return !!(await Promise.resolve(window.confirm(message) as boolean | Promise<boolean>));
}
