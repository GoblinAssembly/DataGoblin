export const shortcutDefinitions = {
  run: ['Run selection or current statement', 'Mod-Enter'],
  save: ['Save SQL file', 'Mod-s'],
  format: ['Format SQL selection or entire query', 'Mod-Alt-l'],
  settings: ['Settings', 'Mod-,'],
  history: ['Query history', 'Mod-Shift-h'],
  newTab: ['New query', 'Mod-Alt-n'],
  previous: ['Previous query tab', 'Mod-PageUp'],
  next: ['Next query tab', 'Mod-PageDown'],
} as const;
export type Shortcut = keyof typeof shortcutDefinitions;
export const shortcutIds = Object.keys(shortcutDefinitions) as Shortcut[];

export function eventBinding(event: KeyboardEvent) {
  const key = event.key;
  if (event.isComposing || event.getModifierState?.('AltGraph') || !key ||
      ['Control', 'Meta', 'Alt', 'Shift', 'Dead', 'Unidentified'].includes(key)) return '';
  const modifiers = [event.ctrlKey || event.metaKey ? 'Mod' : '', event.altKey ? 'Alt' : '', event.shiftKey ? 'Shift' : ''].filter(Boolean);
  if (!modifiers.length && !/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(key)) return '';
  return [...modifiers, key === ' ' ? 'Space' : key.length === 1 ? key.toLowerCase() : key].join('-');
}

export function shortcutLabel(binding: string) {
  return binding.replace(/^Mod-/, navigator.platform.includes('Mac') ? 'Cmd-' : 'Ctrl-')
    .replace(/[a-z]$/, letter => binding.split('-').at(-1)?.length === 1 ? letter.toUpperCase() : letter)
    .replace(/-(?=.)/g, ' + ');
}
