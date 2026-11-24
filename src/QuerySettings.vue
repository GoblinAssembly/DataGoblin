<script setup lang="ts">
import { QueryLibrary } from './types';
import { shallowRef, computed, useId } from 'vue';
import { stateSetter } from './vueLifecycle';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, Palette, Keyboard, SquarePen, X } from '@lucide/vue';
import { useEditorPreferences } from './editorPreferences';
import { shortcutDefinitions, shortcutIds, shortcutLabel, eventBinding, type Shortcut } from './shortcuts';
import { themes, useTheme } from './theme';
import DialogBoundary from './DialogBoundary.vue';
defineOptions({
  inheritAttrs: false,
});
const _props = defineProps<{
  library: QueryLibrary | null;
  onChange: (folder: string | null) => Promise<void>;
  onClose: () => void;
}>();
const library = computed(() => _props.library);
const onChange = computed(() => _props.onChange);
const onClose = computed(() => _props.onClose);
const recording = shallowRef<Shortcut | null>(null);
function record(event: KeyboardEvent, id: Shortcut) {
  if (recording.value !== id || event.key === 'Tab') return;
  event.preventDefault();
  event.stopPropagation();
  if (event.key !== 'Escape') {
    const binding = eventBinding(event);
    if (!binding) return;
    editorPreferences.setBinding(id, binding);
  }
  recording.value = null;
}
const editorPreferences = useEditorPreferences();
const sections = ['appearance', 'storage', 'editor', 'keybindings'] as const;
const section = shallowRef<(typeof sections)[number]>('appearance');
const setSection = stateSetter(section);
const id = useId();
const _store = useTheme();
const theme = computed(() => _store.theme);
const setTheme = computed(() => _store.setTheme);
const themeError = computed(() => _store.error);
const busy = shallowRef(false),
  error = shallowRef('');
const setBusy = stateSetter(busy);
const setError = stateSetter(error);
async function change(reset: boolean) {
  const _library = library.value;
  const _onChange = onChange.value;
  setBusy(true);
  setError('');
  try {
    const folder = reset
      ? null
      : await open({
          directory: true,
          multiple: false,
          title: 'Choose your query library folder',
          defaultPath: _library?.folder,
        });
    if (!reset && !folder) return;
    await _onChange(folder);
  } catch (error) {
    setError(String(error));
  } finally {
    setBusy(false);
  }
}
const document = globalThis.document;
</script>

<template>
  <DialogBoundary @close="onClose" :disabled="busy">
    <div class="overlay">
      <section
        class="modal settings-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
      >
        <div class="modal-heading">
          <h2>Settings</h2>
          <button
            class="icon-button"
            title="Close settings"
            :disabled="busy"
            @click="onClose"
          >
            <X :size="18"></X>
          </button>
        </div>
        <div class="settings-layout">
          <nav
            class="settings-nav"
            role="tablist"
            aria-label="Settings categories"
            aria-orientation="vertical"
            @keydown="
              (event) => {
                const _section = section!;
                if (
                  !['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)
                )
                  return;
                event.preventDefault();
                const next = sections[event.key === 'Home' ? 0 : event.key === 'End' ? sections.length - 1 : (sections.indexOf(_section) + (event.key === 'ArrowDown' ? 1 : -1) + sections.length) % sections.length];
                setSection(next);
                document.getElementById(`${id}-${next}-tab`)?.focus();
              }
            "
          >
            <button
              :id="`${id}-appearance-tab`"
              role="tab"
              :aria-selected="section === 'appearance'"
              :aria-controls="`${id}-appearance-panel`"
              :tabindex="section === 'appearance' ? 0 : -1"
              @click="() => setSection('appearance')"
            >
              <Palette :size="16"></Palette>
              Appearance
            </button>
            <button
              :id="`${id}-storage-tab`"
              role="tab"
              :aria-selected="section === 'storage'"
              :aria-controls="`${id}-storage-panel`"
              :tabindex="section === 'storage' ? 0 : -1"
              @click="() => setSection('storage')"
            >
              <FolderOpen :size="16"></FolderOpen>
              Query storage
            </button>
            <button :id="`${id}-editor-tab`" role="tab" :aria-selected="section === 'editor'"
              :aria-controls="`${id}-editor-panel`" :tabindex="section === 'editor' ? 0 : -1" @click="setSection('editor')">
              <SquarePen :size="16" />Editor
            </button>
            <button :id="`${id}-keybindings-tab`" role="tab" :aria-selected="section === 'keybindings'"
              :aria-controls="`${id}-keybindings-panel`" :tabindex="section === 'keybindings' ? 0 : -1" @click="setSection('keybindings')">
              <Keyboard :size="16" />Keybindings
            </button>
          </nav>
          <div class="settings-content">
            <section role="tabpanel" :id="`${id}-editor-panel`" :aria-labelledby="`${id}-editor-tab`" :hidden="section !== 'editor'" :tabindex="0">
              <h3>Editor</h3>
              <label class="vim-preference"><input type="checkbox" :checked="editorPreferences.vim" @change="editorPreferences.setVim(($event.target as HTMLInputElement).checked)" />Vim mode</label>
              <p>Use Vim motions and modal editing in SQL queries. Press <kbd>i</kbd> to type and <kbd>Esc</kbd> to return to Normal mode.</p>
              <p class="quiet">Applies immediately to open editors and is remembered on this device. Run and Save shortcuts remain available.</p>
              <p v-if="editorPreferences.error && section === 'editor'" role="alert">{{ editorPreferences.error }}</p>
            </section>
            <section role="tabpanel" :id="`${id}-keybindings-panel`" :aria-labelledby="`${id}-keybindings-tab`" :hidden="section !== 'keybindings'" :tabindex="0">
              <h3>Keybindings</h3>
              <p>Select a shortcut, then press a key combination or function key. Esc cancels.</p>
              <div v-for="id in shortcutIds" :key="id" class="shortcut-row">
                <span>{{ shortcutDefinitions[id][0] }}</span>
                <button class="secondary-button" :aria-label="'Rebind ' + shortcutDefinitions[id][0]"
                  @click="recording = id" @blur="recording = null" @keydown="record($event, id)">
                  {{ recording === id ? 'Press shortcut…' : shortcutLabel(editorPreferences.binding(id)) }}
                </button>
              </div>
              <template v-if="editorPreferences.vim">
                <label for="vim-mappings">Vim mappings</label>
                <textarea id="vim-mappings" rows="6" spellcheck="false" :value="editorPreferences.mappings"
                  @change="editorPreferences.setMappings(($event.target as HTMLTextAreaElement).value)" />
                <p>Space is the leader. Edit the mappings above to bind RunLine, RunScript or ToggleComment, or map keys to Vim keystrokes. Changes apply when you leave the field.</p>
              </template>
              <p v-if="editorPreferences.error" role="alert">{{ editorPreferences.error }}</p>
            </section>
            <section
              role="tabpanel"
              :id="`${id}-appearance-panel`"
              :aria-labelledby="`${id}-appearance-tab`"
              :hidden="section !== 'appearance'"
              :tabindex="0"
            >
              <h3>Appearance</h3>
              <div class="theme-options" role="group" aria-label="Theme">
                <template v-for="option in themes" :key="option.id">
                  <button
                    class="theme-option"
                    :aria-label="option.name"
                    :aria-pressed="theme === option.id"
                    @click="
                      () => {
                        const _setTheme = setTheme!;
                        return _setTheme(option.id);
                      }
                    "
                  >
                    <span
                      class="theme-preview"
                      :data-palette="option.id"
                      aria-hidden="true"
                    >
                      <i></i>
                      <span>
                        <b></b>
                        <b></b>
                        <b></b>
                      </span>
                    </span>
                    <strong>{{ option.name }}</strong>
                    <small>{{ option.description }}</small>
                  </button>
                </template>
              </div>
              <p class="theme-preference-note">
                Applied immediately across the workspace and remembered on this
                device.
              </p>
              <template v-if="themeError">
                <p role="alert">{{ themeError }}</p>
              </template>
            </section>
            <section
              role="tabpanel"
              :id="`${id}-storage-panel`"
              :aria-labelledby="`${id}-storage-tab`"
              :hidden="section !== 'storage'"
              :tabindex="0"
            >
              <h3>Query storage</h3>
              <p>
                Query1, Query2… drafts save automatically in Unsaved. Named
                queries are organized by server and database.
              </p>
              <label>
                Current folder
                <input
                  aria-label="Current query folder"
                  readonly
                  :value="library?.folder ?? 'Library unavailable'"
                />
              </label>
              <p class="muted">
                Default:
                {{ library?.defaultFolder ?? 'DataGoblin app data / queries' }}
              </p>
              <p class="muted">
                Changing folders copies your saved query library to the new
                location. Original files are kept. Imported SQL files are copied
                into the library; exporting a file creates a separate copy.
              </p>
              <template v-if="error">
                <div class="form-message error" role="alert">{{ error }}</div>
              </template>
              <div class="modal-footer">
                <button
                  class="secondary-button"
                  :disabled="busy || !library"
                  @click="() => void change(true)"
                >
                  Use app data folder
                </button>
                <button
                  class="primary-button"
                  :disabled="busy || !library"
                  @click="() => void change(false)"
                >
                  <FolderOpen :size="15"></FolderOpen>
                  {{ busy ? 'Updating…' : 'Choose folder' }}
                </button>
              </div>
            </section>
          </div>
        </div>
      </section>
    </div>
  </DialogBoundary>
</template>

<style scoped>
#vim-mappings { display: block; width: 100%; font-family: monospace; }
.vim-preference { display: flex; align-items: center; gap: 8px; }
.vim-preference input { width: 16px; height: 16px; }
</style>
