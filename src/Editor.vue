<script setup lang="ts">
import { type SqlTarget } from './editorNavigation';
import { type MenuPosition } from './ContextMenu';
import { shallowRef, computed, watchSyncEffect } from 'vue';
import { stateSetter, watchMounted } from './vueLifecycle';
import { syntaxTree } from '@codemirror/language';
import { Compartment, EditorState, Prec } from '@codemirror/state';
import {
  EditorView,
  hoverTooltip,
  keymap,
} from '@codemirror/view';
import { vim, getCM } from '@replit/codemirror-vim';
import { configureMappings, vimRun } from './editorVim';
import { useEditorPreferences } from './editorPreferences';
import { basicSetup } from 'codemirror';
import { sqlEditorLanguage } from './sqlCompletion';
import {
  autocompletion,
  snippet,
  acceptCompletion,
  completionStatus,
  closeCompletion,
} from '@codemirror/autocomplete';
import { indentWithTab, selectAll } from '@codemirror/commands';
import ContextMenu from './ContextMenu.vue';
import { sqlTargetAt } from './editorNavigation';
import { useTheme } from './theme';
import { editorAppearance } from './editorAppearance';
import { statementAt } from './sql';
import { formatEditorSql } from './sqlFormat';
import { Props } from './Editor';
import { executionMarkers, setExecutionMarker } from './editorExecution';
import { activeStatementOutline } from './editorStatement';
defineOptions({
  inheritAttrs: false,
});
const props = defineProps<Props>();
const menu = shallowRef<{
  target: SqlTarget;
  position: MenuPosition;
} | null>(null);
const setMenu = stateSetter(menu);
const _store = useTheme();
const dark = computed(() => _store.dark);
watchMounted(
  () => [props.tables, props.engine],
  () => setMenu(null),
);
const preferences = useEditorPreferences();
const vimConfiguration = new Compartment();
const vimExtension = () => {
  if (!preferences.vim) return [];
  configureMappings(preferences.mappings);
  return Prec.highest(vim({ status: true }));
};
const appearance = shallowRef(new Compartment());
const host = shallowRef<HTMLDivElement | null>(null);
const editorView = shallowRef<EditorView | null | null>(null);
const sqlConfiguration = shallowRef(new Compartment());
const callbacks = shallowRef(props);
watchSyncEffect(() => (callbacks.value = props));
function selectEditorText(event: KeyboardEvent) {
  if (preferences.vim && event.key === 'Escape' && editorView.value) closeCompletion(editorView.value);
  if (preferences.vim && !event.metaKey) return;
  if (!(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.key.toLowerCase() !== 'a' || event.isComposing) return;
  const view = editorView.value;
  if (!view) return;
  event.preventDefault();
  event.stopPropagation();
  view.focus();
  selectAll(view);
}
const formatQuery = () => {
  const view = editorView.value;
  if (!view) return;
  try {
    formatEditorSql(view, callbacks.value.engine);
  } catch (error) {
    callbacks.value.onFormatError?.(
      `Could not format SQL: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  view.focus();
};
const shortcutConfiguration = new Compartment();
const editorShortcuts = () => Prec.highest(keymap.of([
  {
    key: preferences.binding('run'),
    run: (view) => {
      if (preferences.vim && preferences.binding('run') === 'Mod-Enter' && !navigator.platform.includes('Mac')) return false;
      const selection = view.state.selection.main;
      const doc = view.state.doc.toString();
      const text = selection.empty
        ? statementAt(doc, selection.head, callbacks.value.engine)
        : view.state.sliceDoc(selection.from, selection.to);
      const from = selection.empty
        ? Math.max(0, doc.lastIndexOf(text, selection.head))
        : selection.from;
      callbacks.value.onRun(text, from);
      return true;
    },
  },
  {
    key: preferences.binding('format'),
    run: () => {
      formatQuery();
      return true;
    },
  },
  {
    key: preferences.binding('save'),
    run: () => {
      callbacks.value.onSave();
      return true;
    },
  },
]));
watchMounted(() => [preferences.binding('run'), preferences.binding('format'), preferences.binding('save')], () => {
  editorView.value?.dispatch({ effects: shortcutConfiguration.reconfigure(editorShortcuts()) });
});
defineExpose(
  (() => ({
    format: formatQuery,
  }))(),
);
watchMounted(
  () => [],
  () => {
    const _dark = dark.value;
    if (!host.value) return;
    const view = new EditorView({
      parent: host.value,
      state: EditorState.create({
        doc: callbacks.value.value,
        extensions: [
          vimConfiguration.of(vimExtension()),
          vimRun.of((text, from) => callbacks.value.onRun(text, from)),
          shortcutConfiguration.of(editorShortcuts()),
          basicSetup,
          executionMarkers,
          sqlConfiguration.value.of(
            [sqlEditorLanguage(props.engine, props.schema, props.completionCatalog), activeStatementOutline(props.engine)],
          ),
          autocompletion({
            activateOnTypingDelay: 100,
            interactionDelay: 0,
          }),
          hoverTooltip(
            async (view, pos) => {
              const p = callbacks.value;
              const target = sqlTargetAt(
                view.state.doc.toString(),
                pos,
                p.engine,
                p.tables ?? [],
                p.columns,
                true,
              );
              if (!target || !p.onMetadata) return null;
              let detail: string;
              try {
                detail = await p.onMetadata(target);
              } catch {
                detail = 'Could not load metadata.';
              }
              return {
                pos: target.from,
                end: target.to,
                above: true,
                create() {
                  const dom = document.createElement('div');
                  dom.className = 'sql-metadata-tooltip';
                  const title = document.createElement('strong');
                  title.textContent = [
                    target.table.schema,
                    target.table.name,
                    target.column,
                  ]
                    .filter(Boolean)
                    .join('.');
                  const body = document.createElement('div');
                  body.textContent = detail;
                  const hint = document.createElement('small');
                  hint.textContent =
                    'Ctrl / Cmd + click to reveal in Database Explorer';
                  dom.append(title, body, hint);
                  return {
                    dom,
                  };
                },
              };
            },
            {
              hoverTime: 350,
            },
          ),
          EditorView.domEventHandlers({
            click(event, view) {
              if (!event.ctrlKey && !event.metaKey) return false;
              const pos = view.posAtCoords({
                x: event.clientX,
                y: event.clientY,
              });
              const p = callbacks.value;
              const target =
                pos == null
                  ? null
                  : sqlTargetAt(
                      view.state.doc.toString(),
                      pos,
                      p.engine,
                      p.tables ?? [],
                      p.columns,
                    );
              if (!target || !p.onReveal) return false;
              event.preventDefault();
              p.onReveal(target.table);
              return true;
            },
            contextmenu(event, view) {
              const pos = view.posAtCoords({
                x: event.clientX,
                y: event.clientY,
              });
              const p = callbacks.value;
              const target =
                pos == null
                  ? null
                  : sqlTargetAt(
                      view.state.doc.toString(),
                      pos,
                      p.engine,
                      p.tables ?? [],
                      p.columns,
                    );
              if (!target || !p.onOpenData) return false;
              event.preventDefault();
              setMenu({
                target,
                position: {
                  x: event.clientX,
                  y: event.clientY,
                  origin: view.contentDOM,
                },
              });
              return true;
            },
          }),
          Prec.highest(
            keymap.of([
              {
                key: 'Tab',
                run: (view) => {
                  if (preferences.vim && !getCM(view)?.state.vim?.insertMode) return false;
                  const cursor = view.state.selection.main;
                  if (
                    cursor.empty &&
                    completionStatus(view.state) === 'active' &&
                    acceptCompletion(view)
                  )
                    return true;
                  if (
                    cursor.empty &&
                    syntaxTree(view.state).resolveInner(cursor.head, -1)
                      .name === 'Identifier'
                  ) {
                    const word = view.state
                      .sliceDoc(0, cursor.head)
                      .match(/\bsel$/i);
                    if (word) {
                      snippet('select * from ${table};')(
                        view,
                        null,
                        cursor.head - 3,
                        cursor.head,
                      );
                      return true;
                    }
                  }
                  return acceptCompletion(view);
                },
              },
              { ...indentWithTab, run: view => preferences.vim && !getCM(view)?.state.vim?.insertMode ? false : indentWithTab.run!(view) },
            ]),
          ),
          EditorView.updateListener.of((update) => {
            if (update.docChanged)
              callbacks.value.onChange(update.state.doc.toString());
          }),
          appearance.value.of(editorAppearance(_dark)),
        ],
      }),
    });
    editorView.value = view;
    const cm = getCM(view);
    if (cm) cm.save = () => callbacks.value.onSave();
    return () => {
      editorView.value = null;
      view.destroy();
    };
  },
);
watchMounted(
  () => [preferences.vim],
  () => {
    const view = editorView.value;
    if (!view) return;
    view.dispatch({ effects: vimConfiguration.reconfigure(vimExtension()) });
    const cm = getCM(view);
    if (cm) cm.save = () => callbacks.value.onSave();
  },
);
watchMounted(
  () => [props.engine, props.schema, props.completionCatalog],
  () => {
    editorView.value?.dispatch({
      effects: sqlConfiguration.value.reconfigure(
        [sqlEditorLanguage(props.engine, props.schema, props.completionCatalog), activeStatementOutline(props.engine)],
      ),
    });
  },
);
watchMounted(
  () => {
    const _dark2 = dark.value;
    return [_dark2];
  },
  () => {
    const _dark3 = dark.value;
    editorView.value?.dispatch({
      effects: appearance.value.reconfigure(editorAppearance(_dark3)),
    });
  },
);
const activeExecution = computed(() =>
  props.execution?.document === props.value ? props.execution : undefined,
);
watchMounted(
  () => {
    const _activeExecution = activeExecution.value;
    return [_activeExecution];
  },
  () => {
    const _activeExecution2 = activeExecution.value;
    const view = editorView.value;
    if (!view) return;
    view.dispatch({
      effects: setExecutionMarker.of(_activeExecution2),
    });
  },
);
</script>

<template>
  <div class="editor" ref="host" @keydown.capture="selectEditorText"></div>
  <template v-if="menu">
    <ContextMenu
      :position="menu.position"
      label="SQL object actions"
      @close="() => setMenu(null)"
      :actions="
        ((_menu) => [
          {
            label: 'Reveal in Database Explorer',
            action: () => callbacks.onReveal?.(_menu.target.table),
          },
          {
            label: 'Open data',
            action: () => callbacks.onOpenData?.(_menu.target.table),
          },
        ])(menu)
      "
    ></ContextMenu>
  </template>
</template>
