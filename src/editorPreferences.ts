import { inject, provide, shallowRef, type InjectionKey } from 'vue';

import { configureMappings, defaultMappings } from './editorVim';
import { shortcutDefinitions, shortcutIds, type Shortcut } from './shortcuts';
const shortcutsKey = 'datagoblin.editor.shortcuts';
const mappingsKey = 'datagoblin.editor.vimMappings';
export const vimPreferenceKey = 'datagoblin.editor.vim';
function savedVim(): boolean {
  try { return localStorage.getItem(vimPreferenceKey) === 'true'; } catch { return false; }
}
function createPreferences() {
  const vim = shallowRef(savedVim());
  const mappings = shallowRef(defaultMappings);
  try { mappings.value = localStorage.getItem(mappingsKey) ?? defaultMappings; } catch {}
  const bindings = shallowRef<Partial<Record<Shortcut, string>>>({});
  try {
    const saved = JSON.parse(localStorage.getItem(shortcutsKey) ?? '{}');
    bindings.value = Object.fromEntries(shortcutIds.filter(id => typeof saved?.[id] === 'string').map(id => [id, saved[id]]));
  } catch {}
  const binding = (id: Shortcut) => bindings.value[id] ?? shortcutDefinitions[id][1];
  const error = shallowRef('');
  return {
    binding,
    setBinding(id: Shortcut, value: string) {
      const conflict = shortcutIds.find(other => other !== id && binding(other) === value);
      if (conflict) { error.value = 'Already assigned to ' + shortcutDefinitions[conflict][0] + '.'; return; }
      bindings.value = { ...bindings.value, [id]: value };
      try { localStorage.setItem(shortcutsKey, JSON.stringify(bindings.value)); error.value = ''; }
      catch { error.value = 'Keybinding applied but could not be saved.'; }
    },
    get mappings() { return mappings.value; },
    setMappings(value: string) {
      try {
        configureMappings(value);
        mappings.value = value;
        localStorage.setItem(mappingsKey, value);
        error.value = '';
      } catch (cause) { error.value = String(cause); }
    },
    get vim() { return vim.value; },
    get error() { return error.value; },
    setVim(enabled: boolean) {
      vim.value = enabled;
      try { localStorage.setItem(vimPreferenceKey, String(enabled)); error.value = ''; }
      catch { error.value = 'Vim mode applied, but this preference could not be saved.'; }
    },
  };
}
const key: InjectionKey<ReturnType<typeof createPreferences>> = Symbol('Editor preferences');
export function provideEditorPreferences() { provide(key, createPreferences()); }
export const useEditorPreferences = () => inject(key, createPreferences, true);
