<script setup lang="ts">
import { ForeignReference } from './types';
import { QueryHistoryEntry } from './types';
import { SavedQuery } from './types';
import { type MenuPosition } from './ContextMenu';
import { SqlTarget } from './editorNavigation';
import { type OutputEvent } from './QueryOutput';
import { useEditorPreferences } from './editorPreferences';
import { shortcutIds, shortcutDefinitions, shortcutLabel, eventBinding, type Shortcut } from './shortcuts';
import WindowControls from './WindowControls.vue';
import { version as appVersion } from '../package.json';
import AppUpdater from './AppUpdater.vue';
import { hasCustomTitlebar } from './WindowControls';
import { shallowRef, computed, watchSyncEffect } from 'vue';
import { stateSetter, watchMounted } from './vueLifecycle';
import QueryOutput from './QueryOutput.vue';
import {
  appendLog,
  sqlEvent,
  resultEvents,
  type OutputLog,
} from './QueryOutput';
import ContextMenu from './ContextMenu.vue';
import { menuPosition } from './ContextMenu';
import { tableObjectsSql, parseTableObjects } from './tableObjects';
import { TableMetadata } from './TableMetadataFolders';
import DatabaseExplorer from './DatabaseExplorer.vue';
import Select from './Select.vue';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
import {
  AlignLeft,
  Pin,
  History,
  Database,
  Plus,
  Play,
  Square,
  FolderOpen,
  Table2,
  X,
  Settings2,
  Terminal,
  CircleHelp,
  Check,
  LoaderCircle,
  FileCode2,
} from '@lucide/vue';
import IslandWorkspace from './IslandWorkspace.vue';
import WorkspaceControls from './WorkspaceControls.vue';
import { useWorkspacePanels } from './IslandWorkspace';
import DialogBoundary from './DialogBoundary.vue';
import Editor from './Editor.vue';
import { type EditorHandle, type ExecutionMark } from './Editor';
import PagedResults from './PagedResults.vue';
import { defaultPageSize } from './PagedResults';
import { discardEditingDraft } from './useGridEditing';
import QuerySettings from './QuerySettings.vue';
import QueryHistory from './QueryHistory.vue';
import QueryPane from './QueryPane.vue';
import { useQueryLibrary } from './useQueryLibrary';
import type {
  Column,
  ColumnKey,
  Connection,
  QueryResult,
  ResultPage,
  QueryTab,
  Table,
} from './types';
import { columnsSql, databasesSql, previewSql, toCsv } from './sql';
import { defaultDatabase, defaultSchema, readExplorerScope, scopeStorageKey, scopeNames, schemasSql, schemaObjectsSql, readMetadataPages, isRoutine, type ScopeChoice } from './explorerScope';
import { routineDefinitionSql, routineDefinitions } from './routines';
import { relationshipsSql, parseRelationships, type CompletionMetadata } from './completionMetadata';
import {
  uid,
  resultVersion,
  initialTab,
  tableKey,
  errorText,
  mod,
  fileFilter,
  restoredTabs,
} from './App';
import ConnectionDialog from './ConnectionDialog.vue';
defineOptions({
  inheritAttrs: false,
});
const editorPreferences = useEditorPreferences();
const bindingLabel = (id: Shortcut) => shortcutLabel(editorPreferences.binding(id));
const developmentBuild = import.meta.env.DEV;
const _store = useWorkspacePanels();
const panels = computed(() => _store.panels);
const togglePanel = computed(() => _store.togglePanel);
const reveal = shallowRef<
  | {
      connection: Connection;
      table: Table;
      id: string;
    }
  | undefined
>();
const setReveal = stateSetter(reveal);
const connections = shallowRef<Connection[]>([]);
const setConnections = stateSetter(connections);
const connectionsReady = shallowRef(false);
const setConnectionsReady = stateSetter(connectionsReady);
const tabs = shallowRef<QueryTab[]>(restoredTabs());
const setTabs = stateSetter(tabs);
const active = shallowRef(tabs.value[0]?.id ?? '');
const setActive = stateSetter(active);
const explorerSelection = shallowRef({
  connectionId: tabs.value[0]?.connectionId ?? '',
  database: tabs.value[0]?.database ?? '',
});
const setExplorerSelection = stateSetter(explorerSelection);
const tab = computed(() => {
  const _tabs = tabs.value;
  const _active = active.value;
  return _tabs.find((t) => t.id === _active) ?? _tabs.at(0);
});
const selectedContext = computed(() => {
  const _tab = tab.value;
  const _explorerSelection = explorerSelection.value;
  return _tab ?? _explorerSelection;
});
const connection = computed(() => {
  const _connections = connections.value;
  const _selectedContext = selectedContext.value;
  return _connections.find((c) => c.id === _selectedContext.connectionId);
});
const effective = computed(() => {
  const _connection = connection.value;
  const _selectedContext2 = selectedContext.value;
  return _connection
    ? {
        ..._connection,
        database: _selectedContext2.database || _connection.database,
      }
    : undefined;
});
const context = computed(() => {
  const _effective = effective.value;
  return _effective ? JSON.stringify([_effective.id, _effective.database]) : '';
});
const databases = shallowRef<Record<string, string[]>>({});
const setDatabases = stateSetter(databases);
const explorerScope = shallowRef(readExplorerScope());
const schemaCache = shallowRef<Record<string, string[]>>({});
const routineCache = shallowRef<Record<string, Table[]>>({});
function selectedDatabases(c: Connection, available = databases.value[c.id] ?? [defaultDatabase(c)]) {
  const fallback = available.includes(defaultDatabase(c)) ? defaultDatabase(c) : c.database;
  return scopeNames(explorerScope.value.databases[c.id], available, [fallback]);
}
function selectedSchemas(c: Connection, available = schemaCache.value[JSON.stringify([c.id, c.database])] ?? [defaultSchema(c.engine)]) {
  return scopeNames(explorerScope.value.schemas[JSON.stringify([c.id, c.database])], available, [defaultSchema(c.engine)]);
}
async function changeExplorerScope(c: Connection, level: 'databases' | 'schemas', choice: ScopeChoice) {
  const key = level === 'databases' ? c.id : JSON.stringify([c.id, c.database]);
  explorerScope.value = { ...explorerScope.value, [level]: { ...explorerScope.value[level], [key]: choice } };
  try { localStorage.setItem(scopeStorageKey, JSON.stringify(explorerScope.value)); }
  catch { setError('Could not save explorer selection. It will apply for this session only.'); }
  completionCache.clear();
  const targets = level === 'databases' ? selectedDatabases(c).map(database => ({ ...c, database })) : [c];
  // Finish old requests before reloading the new scope; stale responses are ignored below.
  await Promise.all([...metadataRequests.value.values()]);
  await Promise.all(targets.map(connection => browse(connection, true)));
}
const tableCache = shallowRef<Record<string, Table[]>>({});
const setTableCache = stateSetter(tableCache);
const objectCache = shallowRef<Record<string, Record<string, TableMetadata>>>(
  {},
);
const setObjectCache = stateSetter(objectCache);
const objectRequests = shallowRef(new Set<string>());
const columnCache = shallowRef<Record<string, Record<string, Column[]>>>({});
const setColumnCache = stateSetter(columnCache);
const results = shallowRef<Record<string, QueryResult>>({});
const setResults = stateSetter(results);
const pinnedResults = shallowRef<
  Record<
    string,
    {
      owner: string;
      connection: Connection;
      name: string;
    }
  >
>({});
const setPinnedResults = stateSetter(pinnedResults);
const selectedResult = shallowRef<Record<string, string>>({});
const setSelectedResult = stateSetter(selectedResult);
const executionMarks = shallowRef<Record<string, ExecutionMark | undefined>>(
  {},
);
const setExecutionMarks = stateSetter(executionMarks);
const outputs = shallowRef<Record<string, OutputLog>>({});
const setOutputs = stateSetter(outputs);
function logOutput(id: string, database: string, ...events: OutputEvent[]) {
  setOutputs((prev) => ({
    ...prev,
    [id]: appendLog(prev[id], database, events),
  }));
}
watchMounted(
  () => {
    const _tabs2 = tabs.value;
    const _pinnedResults = pinnedResults.value;
    return [_tabs2, _pinnedResults];
  },
  () => {
    const _pinnedResults2 = pinnedResults.value;
    const _tabs3 = tabs.value;
    const removed = Object.keys(_pinnedResults2).filter(
      (id) => !_tabs3.some((tab) => tab.id === _pinnedResults2[id].owner),
    );
    if (!removed.length) return;
    setPinnedResults((prev) =>
      Object.fromEntries(
        Object.entries(prev).filter(([id]) => !removed.includes(id)),
      ),
    );
    setResults((prev) =>
      Object.fromEntries(
        Object.entries(prev).filter(([id]) => !removed.includes(id)),
      ),
    );
  },
);
const retainedCaches = shallowRef(new Set<string>());
watchMounted(
  () => {
    const _results = results.value;
    return [_results];
  },
  () => {
    const _results2 = results.value;
    const current = new Set(
      Object.values(_results2).flatMap((result) =>
        result.cacheId ? [result.cacheId] : [],
      ),
    );
    for (const cacheId of retainedCaches.value)
      if (!current.has(cacheId))
        void invoke('release_results', {
          cacheId,
        }).catch(() => {});
    retainedCaches.value = current;
  },
);
watchMounted(
  () => [],
  () => () => {
    for (const cacheId of retainedCaches.value)
      void invoke('release_results', {
        cacheId,
      }).catch(() => {});
  },
);
const running = shallowRef<Record<string, string>>({});
const setRunning = stateSetter(running);
const editPending = shallowRef<Record<string, boolean>>({});
const setEditPending = stateSetter(editPending);
const fetching = shallowRef<Record<string, boolean>>({});
const setFetching = stateSetter(fetching);
const runBlockReason = computed(() => {
  if (!effective.value) return 'Choose a connection before running SQL.';
  const id = tab.value?.id;
  if (!id) return 'Open a query before running SQL.';
  if (running.value[id]) return 'A query is already running. Wait for it to finish or cancel it.';
  if (fetching.value[id]) return 'A result operation is in progress. Wait for it to finish before running another query.';
  return '';
});
const loading = shallowRef(0);
const setLoading = stateSetter(loading);
const error = shallowRef('');
const setError = stateSetter(error);
const notice = shallowRef('');
const setNotice = stateSetter(notice);
const dialog = shallowRef<Connection | 'new' | null>(null);
const setDialog = stateSetter(dialog);
const help = shallowRef(false);
const setHelp = stateSetter(help);
const tabMenu = shallowRef<{
  query: QueryTab;
  position: MenuPosition;
} | null>(null);
const setTabMenu = stateSetter(tabMenu);
const renameTab = shallowRef<QueryTab | null>(null);
const setRenameTab = stateSetter(renameTab);
const renameName = shallowRef('');
const setRenameName = stateSetter(renameName);
const renameBusy = shallowRef(false);
const setRenameBusy = stateSetter(renameBusy);
const renameError = shallowRef('');
const setRenameError = stateSetter(renameError);
const historyOpen = shallowRef(false);
const setHistoryOpen = stateSetter(historyOpen);
const historyVersion = shallowRef(0);
const setHistoryVersion = stateSetter(historyVersion);
const settingsOpen = shallowRef(false);
const setSettingsOpen = stateSetter(settingsOpen);
const closedResultSets = shallowRef<
  Record<
    string,
    {
      execution: string;
      indexes: number[];
    }
  >
>({});
const setClosedResultSets = stateSetter(closedResultSets);
const resultIndex = shallowRef(0);
const setResultIndex = stateSetter(resultIndex);
const passwords = shallowRef(new Map<string, string>());
const activeRequests = shallowRef(new Map<string, string>());
const tables = computed(() => {
  const _tableCache = tableCache.value;
  const _context = context.value;
  const c = effective.value;
  if (!c || !selectedDatabases(c).includes(c.database)) return [];
  return (_tableCache[_context] ?? []).filter(t => selectedSchemas(c).includes(t.schema));
});
const columns = computed(() => {
  const _columnCache = columnCache.value;
  const _context2 = context.value;
  return _columnCache[_context2];
});
const schema = computed(() => {
  const _tables = tables.value;
  const _columns = columns.value;
  return Object.fromEntries(
    _tables.flatMap((t) => {
      const names = (_columns?.[tableKey(t.schema, t.name)] ?? []).map(
        (c) => c.name,
      );
      return [
        [t.name, names],
        [t.schema + '.' + t.name, names],
      ];
    }),
  );
});
const selectedResultId = computed(() => {
  const _tab2 = tab.value;
  const _selectedResult = selectedResult.value;
  return _tab2 ? (_selectedResult[_tab2.id] ?? _tab2.id) : '';
});
const resultId = computed(() => {
  const _selectedResultId = selectedResultId.value;
  const _tab3 = tab.value;
  return _selectedResultId === 'output' ? (_tab3?.id ?? '') : _selectedResultId;
});
const result = computed(() => {
  const _results3 = results.value;
  const _resultId = resultId.value;
  return _results3[_resultId];
});
const resultConnection = computed(() => {
  const _pinnedResults3 = pinnedResults.value;
  const _resultId2 = resultId.value;
  const _effective2 = effective.value;
  return _pinnedResults3[_resultId2]?.connection ?? _effective2;
});
const outputVisible = computed(() => {
  const _selectedResultId2 = selectedResultId.value;
  return _selectedResultId2 === 'output';
});
function resultSetClosed(id: string, index: number) {
  const _closedResultSets = closedResultSets.value;
  const _results4 = results.value;
  const entry = _closedResultSets[id],
    result = _results4[id];
  return (
    !!entry &&
    !!result &&
    entry.execution === (result.cacheId ?? resultVersion(result)) &&
    entry.indexes.includes(index)
  );
}
function selectResult(id: string, index = 0) {
  const _tab4 = tab.value;
  const _fetching = fetching.value;
  if (!_tab4 || _fetching[_tab4.id]) return;
  setSelectedResult((prev) => ({
    ...prev,
    [_tab4.id]: id,
  }));
  setResultIndex(index);
}
function pinResult() {
  const _tab5 = tab.value;
  const _result = result.value;
  const _resultConnection = resultConnection.value;
  const _pinnedResults4 = pinnedResults.value;
  const _resultId3 = resultId.value;
  const _closedResultSets2 = closedResultSets.value;
  const _resultIndex = resultIndex.value;
  if (!_tab5 || !_result || !_resultConnection || _pinnedResults4[_resultId3])
    return;
  const id = uid();
  setPinnedResults((prev) => ({
    ...prev,
    [id]: {
      owner: _tab5.id,
      connection: {
        ..._resultConnection,
      },
      name: _result.browser?.table ?? _tab5.name,
    },
  }));
  setResults((prev) => {
    const next = {
      ...prev,
      [id]: _result,
    };
    delete next[_tab5.id];
    return next;
  });
  if (_closedResultSets2[_resultId3])
    setClosedResultSets((prev) => ({
      ...prev,
      [id]: prev[_resultId3],
    }));
  selectResult(id, _resultIndex);
}
function closeResultTab(id: string, index: number) {
  const _tab6 = tab.value;
  const _fetching2 = fetching.value;
  const _running = running.value;
  const _results5 = results.value;
  const _pinnedResults5 = pinnedResults.value;
  const _outputVisible = outputVisible.value;
  const _resultId4 = resultId.value;
  const _resultIndex2 = resultIndex.value;
  if (!_tab6 || _fetching2[_tab6.id] || _running[_tab6.id]) return;
  const execution = _results5[id];
  if (!execution) return;
  const remaining = [
    _tab6.id,
    ...Object.keys(_pinnedResults5).filter(
      (key) => _pinnedResults5[key].owner === _tab6.id,
    ),
  ].flatMap((key) =>
    (_results5[key]?.sets ?? []).flatMap((set, i) =>
      set.columns.length &&
      !resultSetClosed(key, i) &&
      !(key === id && i === index)
        ? [
            {
              id: key,
              index: i,
            },
          ]
        : [],
    ),
  );
  if (!remaining.some((item) => item.id === id)) {
    setPinnedResults((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
    setResults((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
    setClosedResultSets((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
  } else {
    const indexes = execution.sets.flatMap((_, i) =>
      i === index || resultSetClosed(id, i) ? [i] : [],
    );
    setClosedResultSets((prev) => ({
      ...prev,
      [id]: {
        execution: execution.cacheId ?? resultVersion(execution),
        indexes,
      },
    }));
  }
  if (!_outputVisible && _resultId4 === id && _resultIndex2 === index) {
    const next = remaining.find((item) => item.id === id) ?? remaining[0];
    selectResult(next?.id ?? 'output', next?.index ?? 0);
  }
}
const currentRequest = computed(() => {
  const _tab7 = tab.value;
  const _running2 = running.value;
  return _tab7 ? _running2[_tab7.id] : undefined;
});
const set = computed(() => {
  const _result2 = result.value;
  const _resultIndex3 = resultIndex.value;
  return _result2?.sets[Math.min(_resultIndex3, _result2.sets.length - 1)];
});
const queryStorage = useQueryLibrary(
  () => {
    const _tabs4 = tabs.value;
    return _tabs4;
  },
  () => {
    const _connections2 = connections.value;
    return _connections2;
  },
  () => {
    const _connectionsReady = connectionsReady.value;
    return _connectionsReady;
  },
  setTabs,
  setError,
);
watchMounted(
  () => [],
  () => {
    if (isTauri())
      invoke<Connection[]>('load_connections')
        .then((value) => {
          setConnections(value);
          setConnectionsReady(true);
        })
        .catch((e) => setError(errorText(e)));
  },
);
const updateTab = (id: string, patch: Partial<QueryTab>) => {
  setTabs((tabs) =>
    tabs.map((t) =>
      t.id === id
        ? {
            ...t,
            ...patch,
          }
        : t,
    ),
  );
  if (patch.connectionId !== undefined || patch.database !== undefined) {
    setResults((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
  }
};
function selectDatabase(connectionId: string, database: string) {
  const _tab8 = tab.value;
  const _fetching3 = fetching.value;
  if (_tab8 && (activeRequests.value.has(_tab8.id) || _fetching3[_tab8.id]))
    return;
  setExplorerSelection({
    connectionId,
    database,
  });
  if (_tab8)
    updateTab(_tab8.id, {
      connectionId,
      database,
    });
}
const rawQuery = (
  c: Connection,
  sql: string,
  requestId = uid(),
  paged = false,
) => {
  if (!isTauri())
    return Promise.reject(
      new Error(
        'Database connections are available in the desktop app. Start it with npm run desktop.',
      ),
    );
  return invoke<QueryResult>('query', {
    connection: c,
    password: passwords.value.get(c.id) ?? null,
    sql,
    requestId,
    ...(paged
      ? {
          paged: true,
          pageSize: defaultPageSize() || 500,
        }
      : {}),
  });
};
const metadataRequests = shallowRef(new Map<string, Promise<boolean>>());
function browse(c: Connection, force = false): Promise<boolean> {
  const key = JSON.stringify([c.id, c.database]);
  const pending = metadataRequests.value.get(key);
  if (pending) return pending;
  const request = loadMetadata(c, force).finally(() =>
    metadataRequests.value.delete(key),
  );
  metadataRequests.value.set(key, request);
  return request;
}
async function refreshSchema(c: Connection, schema: string): Promise<boolean> {
  const key = JSON.stringify([c.id, c.database]);
  await metadataRequests.value.get(key);
  setLoading(n => n + 1);
  setError('');
  try {
    const snapshot = explorerScope.value;
    const rows = await readMetadataPages(sql => rawQuery(c, sql), schemaObjectsSql([schema]), c.engine);
    if (snapshot.databases[c.id] !== explorerScope.value.databases[c.id] || snapshot.schemas[key] !== explorerScope.value.schemas[key]) return true;
    const objects = rows.map(r => ({ schema: r[0]!, name: r[1]!, kind: r[2]! }));
    setTableCache(prev => ({ ...prev, [key]: [...(prev[key] ?? []).filter(t => t.schema !== schema), ...objects.filter(t => !isRoutine(t.kind))] }));
    routineCache.value = { ...routineCache.value, [key]: [...(routineCache.value[key] ?? []).filter(t => t.schema !== schema), ...objects.filter(t => isRoutine(t.kind))] };
    setColumnCache(prev => ({ ...prev, [key]: Object.fromEntries(Object.entries(prev[key] ?? {}).filter(([table]) => JSON.parse(table)[0] !== schema)) }));
    setObjectCache(prev => ({ ...prev, [key]: Object.fromEntries(Object.entries(prev[key] ?? {}).filter(([table]) => JSON.parse(table)[0] !== schema)) }));
    completionCache.clear();
    return true;
  } catch (error) { setError(errorText(error)); return false; }
  finally { setLoading(n => n - 1); }
}
async function loadMetadata(c: Connection, force = false) {
  const _tableCache2 = tableCache.value;
  const key = JSON.stringify([c.id, c.database]);
  if (_tableCache2[key] && !force) return true;
  setLoading((n) => n + 1);
  setError('');
  try {
    const snapshot = explorerScope.value;
    const read = (sql: string) => readMetadataPages(sql => rawQuery(c, sql), sql, c.engine);
    const dbs = await read(databasesSql(c.engine));
    setDatabases(prev => ({ ...prev, [c.id]: dbs.map(r => r[0]!).filter(Boolean) }));
    if (!selectedDatabases(c).includes(c.database)) return true;
    const schemas = (await read(schemasSql(c.engine))).map(r => r[0]!).filter(Boolean);
    schemaCache.value = { ...schemaCache.value, [key]: schemas };
    const selected = selectedSchemas(c, schemas);
    const rows = selected.length ? await read(schemaObjectsSql(selected)) : [];
    if (snapshot.databases[c.id] !== explorerScope.value.databases[c.id] ||
        snapshot.schemas[key] !== explorerScope.value.schemas[key]) return true;
    const objects = rows.map(r => ({ schema: r[0]!, name: r[1]!, kind: r[2]! }));
    setTableCache(prev => ({ ...prev, [key]: objects.filter(t => !isRoutine(t.kind)) }));
    routineCache.value = { ...routineCache.value, [key]: objects.filter(t => isRoutine(t.kind)) };
    if (force) {
      completionCache.clear();
      setColumnCache(prev => ({ ...prev, [key]: {} }));
      setObjectCache(prev => ({ ...prev, [key]: {} }));
    }
    return true;
  } catch (e) {
    setError(errorText(e));
    return false;
  } finally {
    setLoading((n) => n - 1);
  }
}
watchMounted(
  () => {
    const _context3 = context.value;
    return [_context3];
  },
  () => {
    const _effective3 = effective.value;
    if (_effective3) void browse(_effective3);
  },
);
const completionCache = new Map<string, Promise<CompletionMetadata>>();
const completionCatalog = computed(() => {
  const c = effective.value;
  if (!c) return undefined;
  return { tables: tables.value, load: (t: Table) => {
    const key = JSON.stringify([c, t.schema, t.name]);
    const cached = completionCache.get(key);
    if (cached) return cached;
    const request = Promise.all([
      rawQuery(c, columnsSql(t.schema,t.name)),
      rawQuery(c, tableObjectsSql(t.schema,t.name,c.engine)),
      rawQuery(c, relationshipsSql(t,c.engine)),
    ]).then(([cols,objects,relationships]) => {
      if (cols.truncated || objects.truncated || relationships.truncated) throw new Error('Completion metadata is incomplete');
      const keys = parseTableObjects(objects.sets[0]?.rows ?? []);
      return {
        columns: (cols.sets[0]?.rows ?? []).map(r => ({ name:r[0]!, type:r[1]!, nullable:r[2]==='YES', primaryKey:keys.some(k=>k.kind==='primary'&&k.columns.includes(r[0]!)), foreignKey:keys.some(k=>k.kind==='foreign'&&k.columns.includes(r[0]!)) })),
        relationships: parseRelationships(relationships.sets[0]?.rows ?? []),
      };
    }).catch(error => { if(completionCache.get(key)===request)completionCache.delete(key); throw error; });
    completionCache.set(key,request);
    return request;
  }};
});
async function expandTable(c: Connection, t: Table, force = false) {
  if (force) completionCache.clear();
  const _columnCache2 = columnCache.value;
  const _objectCache = objectCache.value;
  const key = tableKey(t.schema, t.name),
    context = JSON.stringify([c.id, c.database]);
  const requestKey = context + key;
  if (
    objectRequests.value.has(requestKey) ||
    (!force &&
      _columnCache2[context]?.[key] &&
      _objectCache[context]?.[key]?.objects &&
      !_objectCache[context]?.[key]?.error)
  )
    return;
  objectRequests.value.add(requestKey);
  const update = (value: TableMetadata) =>
    setObjectCache((prev) => ({
      ...prev,
      [context]: {
        ...prev[context],
        [key]: value,
      },
    }));
  update({
    loading: true,
  });
  try {
    const [cols, objects] = await Promise.allSettled([
      rawQuery(c, columnsSql(t.schema, t.name)),
      rawQuery(c, tableObjectsSql(t.schema, t.name, c.engine)),
    ]);
    if (cols.status === 'fulfilled')
      setColumnCache((prev) => ({
        ...prev,
        [context]: {
          ...prev[context],
          [key]: (cols.value.sets[0]?.rows ?? []).map((r) => ({
            name: r[0]!,
            type: r[1]!,
            nullable: r[2] === 'YES',
          })),
        },
      }));
    const errors: string[] = [];
    if (cols.status === 'rejected')
      errors.push('Could not load columns: ' + errorText(cols.reason));
    if (objects.status === 'rejected')
      errors.push(
        'Could not load keys and indexes: ' + errorText(objects.reason),
      );
    if (
      (cols.status === 'fulfilled' && cols.value.truncated) ||
      (objects.status === 'fulfilled' && objects.value.truncated)
    )
      errors.push('Metadata is incomplete: row limit reached.');
    update({
      objects:
        objects.status === 'fulfilled'
          ? parseTableObjects(objects.value.sets[0]?.rows ?? [])
          : undefined,
      error: errors.join(' ') || undefined,
    });
  } finally {
    objectRequests.value.delete(requestKey);
  }
}
function revealTable(table: Table) {
  const _effective4 = effective.value;
  const _panels = panels.value;
  const _togglePanel = togglePanel.value;
  if (!_effective4) return;
  if (!_panels.explorer) _togglePanel('explorer');
  setReveal({
    connection: {
      ..._effective4,
    },
    table,
    id: uid(),
  });
}
async function editorMetadata(target: SqlTarget) {
  const _effective5 = effective.value;
  const _columnCache3 = columnCache.value;
  const _objectCache2 = objectCache.value;
  if (!_effective5) return 'Choose a connection to load metadata.';
  const c = {
      ..._effective5,
    },
    ctx = JSON.stringify([c.id, c.database]),
    key = tableKey(target.table.schema, target.table.name);
  let cols = _columnCache3[ctx]?.[key],
    metadata = _objectCache2[ctx]?.[key];
  if (!cols || !metadata?.objects) {
    const [colResult, objectResult] = await Promise.allSettled([
      rawQuery(c, columnsSql(target.table.schema, target.table.name)),
      rawQuery(
        c,
        tableObjectsSql(target.table.schema, target.table.name, c.engine),
      ),
    ]);
    if (colResult.status === 'rejected') throw colResult.reason;
    cols = (colResult.value.sets[0]?.rows ?? []).map((r) => ({
      name: r[0]!,
      type: r[1]!,
      nullable: r[2] === 'YES',
    }));
    metadata = {
      objects:
        objectResult.status === 'fulfilled'
          ? parseTableObjects(objectResult.value.sets[0]?.rows ?? [])
          : undefined,
      error:
        objectResult.status === 'rejected'
          ? 'Keys unavailable: ' + errorText(objectResult.reason)
          : undefined,
    };
    setColumnCache((prev) => ({
      ...prev,
      [ctx]: {
        ...prev[ctx],
        [key]: cols!,
      },
    }));
    setObjectCache((prev) => ({
      ...prev,
      [ctx]: {
        ...prev[ctx],
        [key]: metadata!,
      },
    }));
  }
  const objects = metadata.objects ?? [];
  if (target.column) {
    const col = cols.find((col) =>
      c.engine === 'sqlserver'
        ? col.name.toLowerCase() === target.column!.toLowerCase()
        : col.name === target.column,
    );
    if (!col) return 'Column metadata unavailable.';
    return [
      col.type + (col.nullable ? ' · nullable' : ' · NOT NULL'),
      ...objects
        .filter(
          (o) =>
            o.columns.includes(col.name) &&
            ['primary', 'foreign'].includes(o.kind),
        )
        .map(
          (o) =>
            (o.kind === 'primary' ? 'Primary key: ' : 'Foreign key: ') + o.name,
        ),
      metadata.error,
    ]
      .filter(Boolean)
      .join('\n');
  }
  return [
    c.database + ' · ' + target.table.kind,
    cols.length + ' columns',
    ...objects
      .filter((o) => o.kind === 'primary')
      .map((o) => 'Primary key: ' + o.columns.join(', ')),
    metadata.error,
  ]
    .filter(Boolean)
    .join('\n');
}
async function openEditorData(table: Table) {
  const _effective6 = effective.value;
  if (!_effective6 || !queryStorage.library) return;
  const c = {
      ..._effective6,
    },
    id = uid(),
    requestId = uid(),
    sql = previewSql(table.schema, table.name, c.engine);
  const next: QueryTab = {
    id,
    name: table.name + '.sql',
    sql,
    savedSql: '',
    connectionId: c.id,
    database: c.database,
    draft: false,
  };
  setTabs((prev) => [...prev, next]);
  setActive(id);
  setResultIndex(0);
  activeRequests.value.set(id, requestId);
  setRunning((prev) => ({
    ...prev,
    [id]: requestId,
  }));
  const startedAt = new Date().toISOString(),
    started = performance.now();
  let failure: string | null = null;
  logOutput(id, c.database, sqlEvent(c.database, sql));
  try {
    const result = await rawQuery(c, sql, requestId, true);
    setResults((prev) => ({
      ...prev,
      [id]: result,
    }));
    logOutput(id, c.database, ...resultEvents(result));
    if (!result.cancelled)
      setExecutionMarks((prev) => ({
        ...prev,
        [id]: {
          document: sql,
          from: 0,
          duration: result.elapsedMs,
        },
      }));
    if (result.cacheId)
      void invoke<ColumnKey[]>('result_column_keys', {
        cacheId: result.cacheId,
      })
        .then((columnKeys) =>
          setResults((prev) =>
            prev[id]?.cacheId === result.cacheId
              ? {
                  ...prev,
                  [id]: {
                    ...prev[id],
                    sets: prev[id].sets.map((set, index) =>
                      index === 0
                        ? {
                            ...set,
                            columnKeys,
                          }
                        : set,
                    ),
                  },
                }
              : prev,
          ),
        )
        .catch(() => {});
  } catch (error) {
    failure = errorText(error);
    logOutput(id, c.database, {
      kind: 'error',
      text: failure!,
      duration: Math.round(performance.now() - started),
    });
    setSelectedResult((prev) => ({
      ...prev,
      [id]: 'output',
    }));
  } finally {
    activeRequests.value.delete(id);
    setRunning((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
    const entry: QueryHistoryEntry = {
      id: requestId,
      queryId: id,
      sql,
      connectionId: c.id,
      serverName: c.name,
      database: c.database,
      engine: c.engine,
      startedAt,
      durationMs: Math.round(performance.now() - started),
      status: failure
        ? /cancelled/i.test(failure)
          ? 'cancelled'
          : 'error'
        : 'results',
      error: failure,
    };
    void invoke('record_query_history', {
      entry,
    })
      .then(() => setHistoryVersion((v) => v + 1))
      .catch((e) => setNotice('Could not save query history: ' + errorText(e)));
  }
}
async function openExplorerObject(c: Connection, object?: Table) {
  const target = { connectionId: c.id, database: c.database };
  if (!object || !isRoutine(object.kind)) {
    newTab(object ? previewSql(object.schema, object.name, c.engine) : '', object ? object.name + '.sql' : undefined, target);
    return;
  }
  setError('');
  try {
    const result = await rawQuery(c, routineDefinitionSql(object, c.engine));
    // The catalog groups PostgreSQL overloads by name; open each signature separately.
    for (const definition of routineDefinitions(result))
      newTab(definition.sql, `${object.schema}.${definition.signature}.sql`, target);
  } catch (error) {
    setError(`Could not open ${object.schema}.${object.name}: ${errorText(error)}`);
  }
}
function newTab(sql = '', name?: string, target = selectedContext.value) {
  if (!queryStorage.library) return;
  const next: QueryTab = {
    id: uid(),
    name: name ?? queryStorage.nextDraftName(),
    sql,
    savedSql: '',
    connectionId: target.connectionId,
    database: target.database,
    draft: !name,
  };
  setTabs((prev) => [...prev, next]);
  setActive(next.id);
  setResultIndex(0);
}
async function run(sql: string, from = 0) {
  const _tab9 = tab.value;
  const _effective7 = effective.value;
  const _fetching4 = fetching.value;
  const _results6 = results.value;
  if (!_tab9) return;
  if (runBlockReason.value || !_effective7) {
    setError(runBlockReason.value || 'Choose a connection before running SQL.');
    return;
  }
  if (!sql.trim() || activeRequests.value.has(_tab9.id) || _fetching4[_tab9.id])
    return;
  if (pagedResults.value?.discardEdits() === false) {
    setError('Wait for the current result operation to finish before running another query.');
    return;
  }
  const id = _tab9.id,
    requestId = uid();
  for (const [resultKey, previous] of Object.entries(_results6)) {
    if ((resultKey === id || pinnedResults.value[resultKey]?.owner === id) && previous.cacheId) discardEditingDraft(previous.cacheId);
  }
  setEditPending(previous => ({ ...previous, [id]: false }));
  setExecutionMarks((prev) => ({
    ...prev,
    [id]: undefined,
  }));
  const startedAt = new Date().toISOString(),
    started = performance.now();
  let historyStatus: QueryHistoryEntry['status'] = 'error',
    historyError: string | null = null;
  activeRequests.value.set(id, requestId);
  setRunning((prev) => ({
    ...prev,
    [id]: requestId,
  }));
  setError('');
  setNotice('');
  logOutput(id, _effective7.database, sqlEvent(_effective7.database, sql));
  try {
    if (_results6[id]?.cacheId) {
      await invoke('release_results', {
        cacheId: _results6[id].cacheId,
      });
      setResults((prev) => ({
        ...prev,
        [id]: {
          ...prev[id],
          cancelled: true,
          complete: true,
        },
      }));
    }
    const result = await rawQuery(_effective7, sql, requestId, true);
    if (!result.cancelled)
      setExecutionMarks((prev) => ({
        ...prev,
        [id]: {
          document: _tab9.sql,
          from,
          duration: result.elapsedMs,
        },
      }));
    historyStatus = result.complete ? 'completed' : 'results';
    logOutput(id, _effective7.database, ...resultEvents(result));
    setResults((prev) => ({
      ...prev,
      [id]: result,
    }));
    setSelectedResult((prev) => ({
      ...prev,
      [id]: result.sets.some((set) => set.columns.length) ? id : 'output',
    }));
    setResultIndex(
      Math.max(
        0,
        result.sets.findIndex((set) => set.columns.length > 0),
      ),
    );
    if (result.cacheId) {
      const cacheId = result.cacheId;
      const applyKeys = (metadata: Partial<QueryResult['sets'][number]>) =>
        setResults((prev) =>
          Object.fromEntries(
            Object.entries(prev).map(([key, result]) => [
              key,
              result.cacheId === cacheId
                ? {
                    ...result,
                    sets: result.sets.map((set, index) =>
                      index === 0
                        ? {
                            ...set,
                            ...metadata,
                          }
                        : set,
                    ),
                  }
                : result,
            ]),
          ),
        );
      void invoke<ColumnKey[]>('result_column_keys', {
        cacheId,
      })
        .then((columnKeys) =>
          applyKeys({
            columnKeys: columnKeys ?? [],
          }),
        )
        .catch((error) =>
          applyKeys({
            keyMetadataError: errorText(error),
          }),
        );
    }
  } catch (e) {
    historyError = errorText(e);
    historyStatus = /cancelled/i.test(historyError) ? 'cancelled' : 'error';
    setError(historyError);
    logOutput(id, _effective7.database, {
      kind: 'error',
      text: historyError!,
      duration: Math.round(performance.now() - started),
    });
    setSelectedResult((prev) => ({
      ...prev,
      [id]: 'output',
    }));
  } finally {
    const entry: QueryHistoryEntry = {
      id: requestId,
      queryId: id,
      sql,
      connectionId: _effective7.id,
      serverName: _effective7.name,
      database: _effective7.database,
      engine: _effective7.engine,
      startedAt,
      durationMs: Math.round(performance.now() - started),
      status: historyStatus,
      error: historyError,
    };
    void invoke('record_query_history', {
      entry,
    })
      .then(() => setHistoryVersion((v) => v + 1))
      .catch((e) => setNotice('Could not save query history: ' + errorText(e)));
    activeRequests.value.delete(id);
    setRunning((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
  }
}
async function browseResults(
  filters: NonNullable<QueryResult['browser']>['filters'],
  sort: NonNullable<QueryResult['browser']>['sort'],
  afterEdit = false,
) {
  const _tab0 = tab.value;
  const _result3 = result.value;
  const _fetching5 = fetching.value;
  const _resultConnection2 = resultConnection.value;
  const _resultId5 = resultId.value;
  if (
    !_tab0 ||
    !_result3?.cacheId ||
    activeRequests.value.has(_tab0.id) ||
    (_fetching5[_tab0.id] && !afterEdit)
  )
    return;
  const id = _tab0.id,
    cacheId = _result3.cacheId,
    requestId = uid();
  const database = _resultConnection2?.database ?? _tab0.database;
  const started = performance.now();
  logOutput(id, database, {
    text: 'Refreshing results…',
  });
  activeRequests.value.set(id, requestId);
  setRunning((prev) => ({
    ...prev,
    [id]: requestId,
  }));
  setError('');
  setResults((prev) => ({
    ...prev,
    [_resultId5]: {
      ...prev[_resultId5],
      cancelled: true,
    },
  }));
  try {
    const next = await invoke<QueryResult>(_result3.sets[0]?.joinEditable ? 'refresh_join_results' : 'browse_table', {
      cacheId,
      requestId,
      filters,
      sort,
      pageSize: defaultPageSize() || 500,
    });
    setResults((prev) =>
      prev[_resultId5]?.cacheId === cacheId
        ? {
            ...prev,
            [_resultId5]: {
              ...next,
              sets: next.sets.map((set, index) => ({
                ...set,
                columnKeys: prev[_resultId5].sets[index]?.columnKeys,
              })),
            },
          }
        : prev,
    );
    logOutput(
      id,
      database,
      ...(next.browser
        ? [sqlEvent(database, next.browser.sql, next.browser.parameters)]
        : []),
      ...resultEvents(next),
    );
    setResultIndex(0);
  } catch (error) {
    setError(errorText(error));
    logOutput(id, database, {
      kind: 'error',
      text: errorText(error),
      duration: Math.round(performance.now() - started),
    });
  } finally {
    activeRequests.value.delete(id);
    setRunning((prev) => {
      const next = {
        ...prev,
      };
      delete next[id];
      return next;
    });
  }
}
async function openReference(
  reference: ForeignReference,
  column: string,
  rowIndex: number,
) {
  const _tab1 = tab.value;
  const _result4 = result.value;
  const _fetching6 = fetching.value;
  const _resultConnection3 = resultConnection.value;
  if (
    !_tab1 ||
    !_result4?.cacheId ||
    activeRequests.value.has(_tab1.id) ||
    _fetching6[_tab1.id] ||
    !queryStorage.library
  )
    return;
  const sourceId = _tab1.id,
    cacheId = _result4.cacheId,
    requestId = uid();
  const navigationConnection = _resultConnection3,
    navigationStartedAt = new Date().toISOString();
  const connectionId = _resultConnection3?.id ?? _tab1.connectionId,
    database = _resultConnection3?.database ?? _tab1.database;
  activeRequests.value.set(sourceId, requestId);
  setRunning((prev) => ({
    ...prev,
    [sourceId]: requestId,
  }));
  setError('');
  try {
    const response = await invoke<{
      result: QueryResult;
      sql: string;
      name: string;
    }>('open_reference', {
      cacheId,
      column,
      referenceId: reference.id,
      rowIndex,
      requestId,
    });
    const next: QueryTab = {
      id: uid(),
      name: queryStorage.nextDraftName(),
      sql: response.sql,
      savedSql: '',
      connectionId,
      database,
      draft: true,
    };
    setTabs((prev) => [...prev, next]);
    setActive(next.id);
    setResultIndex(0);
    setResults((prev) => ({
      ...prev,
      [next.id]: response.result,
    }));
    logOutput(
      next.id,
      database,
      sqlEvent(database, response.sql),
      ...resultEvents(response.result),
    );
    if (navigationConnection) {
      const entry: QueryHistoryEntry = {
        id: requestId,
        queryId: next.id,
        sql: response.sql,
        connectionId,
        serverName: navigationConnection.name,
        database,
        engine: navigationConnection.engine,
        startedAt: navigationStartedAt,
        durationMs: response.result.elapsedMs,
        status: response.result.complete ? 'completed' : 'results',
        error: null,
      };
      void invoke('record_query_history', {
        entry,
      })
        .then(() => setHistoryVersion((v) => v + 1))
        .catch((e) =>
          setNotice('Could not save query history: ' + errorText(e)),
        );
    }
    if (!response.result.sets[0]?.rows.length)
      setNotice(
        'No referenced record was found. It may have been deleted or hidden by database permissions.',
      );
    const newCacheId = response.result.cacheId;
    void invoke<ColumnKey[]>('result_column_keys', {
      cacheId: newCacheId,
    })
      .then((columnKeys) =>
        setResults((prev) =>
          prev[next.id]?.cacheId === newCacheId
            ? {
                ...prev,
                [next.id]: {
                  ...prev[next.id],
                  sets: prev[next.id].sets.map((set, i) =>
                    i === 0
                      ? {
                          ...set,
                          columnKeys,
                        }
                      : set,
                  ),
                },
              }
            : prev,
        ),
      )
      .catch(() => {});
  } catch (error) {
    setError(errorText(error));
  } finally {
    activeRequests.value.delete(sourceId);
    setRunning((prev) => {
      const next = {
        ...prev,
      };
      delete next[sourceId];
      return next;
    });
  }
}
function updatePagedResult(id: string, cacheId: string, result: QueryResult) {
  setResults((prev) =>
    prev[id]?.cacheId === cacheId && !prev[id].cancelled
      ? {
          ...prev,
          [id]: {
            ...result,
            sets: result.sets.map((set, index) => ({
              ...set,
              columnKeys: prev[id].sets[index]?.columnKeys,
              keyMetadataError: prev[id].sets[index]?.keyMetadataError,
            })),
            elapsedMs: prev[id].elapsedMs,
          },
        }
      : prev,
  );
}
async function nextResult() {
  const _tab10 = tab.value;
  const _result5 = result.value;
  const _resultConnection4 = resultConnection.value;
  const _resultId6 = resultId.value;
  if (!_tab10 || !_result5?.cacheId) return;
  const id = _tab10.id,
    cacheId = _result5.cacheId;
  const started = performance.now(),
    database = _resultConnection4?.database ?? _tab10.database;
  logOutput(id, database, {
    text: 'Fetching next result set…',
  });
  setFetching((prev) => ({
    ...prev,
    [id]: true,
  }));
  try {
    const page = await invoke<ResultPage>('result_page', {
      cacheId,
      setIndex: _result5.sets.length,
      offset: 0,
      pageSize: defaultPageSize() || 500,
    });
    if (page.result) {
      const duration = Math.round(performance.now() - started);
      logOutput(id, database, {
        kind: 'success',
        text:
          page.result.sets.length > _result5.sets.length
            ? `${page.rows.length} rows retrieved starting from 1 in ${duration} ms · Result ${_result5.sets.length + 1}`
            : `No more result sets · completed in ${duration} ms`,
        duration,
      });
      updatePagedResult(_resultId6, cacheId, page.result);
      if (page.result.sets.length > _result5.sets.length)
        setResultIndex(_result5.sets.length);
      else setNotice('No more result sets.');
    }
  } catch (error) {
    setError(errorText(error));
    logOutput(id, database, {
      kind: 'error',
      text: errorText(error),
      duration: Math.round(performance.now() - started),
    });
  } finally {
    setFetching((prev) => ({
      ...prev,
      [id]: false,
    }));
  }
}
const refreshIntervals = shallowRef<Record<string, number>>({});
const setRefreshIntervals = stateSetter(refreshIntervals);
async function cancelCurrent() {
  const _tab11 = tab.value;
  const _running3 = running.value;
  const _result6 = result.value;
  const _resultConnection5 = resultConnection.value;
  const _resultId7 = resultId.value;
  if (!_tab11) return;
  const id = _tab11.id,
    requestId = _running3[id] ?? _result6?.cacheId;
  if (!requestId) return;
  try {
    await invoke('cancel_query', {
      requestId,
    });
    logOutput(id, _resultConnection5?.database ?? _tab11.database, {
      text: _running3[id]
        ? 'Cancellation requested.'
        : 'Result fetching cancelled.',
    });
    if (!_running3[id])
      setResults((prev) =>
        prev[_resultId7]?.cacheId === requestId
          ? {
              ...prev,
              [_resultId7]: {
                ...prev[_resultId7],
                cancelled: true,
                complete: true,
              },
            }
          : prev,
      );
  } catch (error) {
    setError(errorText(error));
  }
}
async function prepareUpdate() {
  if (activeRequests.value.size || Object.values(fetching.value).some(Boolean) || Object.values(editPending.value).some(Boolean))
    throw new Error('Finish queries and apply or revert grid edits before updating.');
  // Prevent scheduled table refreshes from starting while the installer exits.
  setRefreshIntervals({});
  await queryStorage.flush();
  if (activeRequests.value.size || Object.values(fetching.value).some(Boolean) || Object.values(editPending.value).some(Boolean))
    throw new Error('Finish queries and apply or revert grid edits before updating.');
}
async function saveTab() {
  try {
    await queryStorage.flush();
  } catch (error) {
    setError(errorText(error));
  }
}
async function openQuery() {
  const _selectedContext4 = selectedContext.value;
  try {
    const path = await open({
      multiple: false,
      filters: fileFilter,
    });
    if (!path) return;
    const sql = await readTextFile(path);
    const next = {
      ...initialTab(),
      name: path.split(/[\\/]/).pop()!,
      path,
      sql,
      savedSql: sql,
      connectionId: _selectedContext4.connectionId,
      database: _selectedContext4.database,
      draft: false,
      fresh: false,
    };
    setTabs((prev) => [...prev, next]);
    setActive(next.id);
  } catch (e) {
    setError(errorText(e));
  }
}
async function closeTab(id: string) {
  const _tabs5 = tabs.value;
  const _fetching7 = fetching.value;
  const _tab12 = tab.value;
  const closing = _tabs5.find((t) => t.id === id);
  if (!closing || activeRequests.value.has(id) || _fetching7[id]) return;
  try {
    await queryStorage.prepareClose([id]);
  } catch (error) {
    setError(errorText(error));
    return;
  }
  if (_tab12?.id === id)
    setExplorerSelection({
      connectionId: closing.connectionId,
      database: closing.database,
    });
  setTabs((current) => current.filter((t) => t.id !== id));
  setActive((current) => (current === id ? '' : current));
  setResults((prev) => {
    const next = {
      ...prev,
    };
    delete next[id];
    return next;
  });
}
async function changeTabDetails(
  query: QueryTab,
  patch: {
    name?: string;
    pinned?: boolean;
  },
) {
  if (query.sql.trim()) {
    await queryStorage.flush();
    await queryStorage.updateQuery(query.id, patch);
  } else
    updateTab(query.id, {
      ...patch,
      ...(patch.name
        ? {
            draft: false,
            fresh: false,
          }
        : {}),
    });
}
async function closeTabs(ids: string[]) {
  const _fetching8 = fetching.value;
  const closable = ids.filter(
    (id) => !activeRequests.value.has(id) && !_fetching8[id],
  );
  try {
    await queryStorage.prepareClose(closable);
  } catch (e) {
    setError(errorText(e));
    return;
  }
  setTabs((current) => current.filter((t) => !closable.includes(t.id)));
  setActive((current) => (closable.includes(current) ? '' : current));
  setResults((current) =>
    Object.fromEntries(
      Object.entries(current).filter(([id]) => !closable.includes(id)),
    ),
  );
}
async function exportCsv(rows: (string | null)[][]) {
  const _set = set.value;
  if (!_set) return;
  try {
    const path = await save({
      defaultPath: 'results.csv',
      filters: [
        {
          name: 'CSV',
          extensions: ['csv'],
        },
      ],
    });
    if (path) {
      await writeTextFile(path, toCsv(_set.columns, rows));
      setNotice('Displayed rows exported.');
    }
  } catch (e) {
    setError(errorText(e));
  }
}
const pagedResults = shallowRef<InstanceType<typeof PagedResults> | null>(null);
const editorHandle = shallowRef<EditorHandle | null>(null);
const shortcuts = shallowRef({
  newTab,
  tab: tab.value,
  tabs: tabs.value,
});
watchSyncEffect(() => {
  const _tab13 = tab.value;
  const _tabs6 = tabs.value;
  return (shortcuts.value = {
    newTab,
    tab: _tab13,
    tabs: _tabs6,
  });
});
watchMounted(
  () => [],
  () => {
    const key = (event: KeyboardEvent) => {
      if (
        event.repeat ||
        event.defaultPrevented ||
        document.querySelector('[aria-modal="true"]')
      )
        return;
      const binding = eventBinding(event);
      const matches = (id: Shortcut) => binding === editorPreferences.binding(id);
      if (matches('previous') || matches('next')) {
        const { tabs, tab } = shortcuts.value;
        if (tabs.length) {
          event.preventDefault();
          const index = tabs.findIndex((t) => t.id === tab?.id);
          setActive(
            tabs[
              (index + (matches('next') ? 1 : tabs.length - 1)) %
                tabs.length
            ].id,
          );
          setResultIndex(0);
        }
      }
      if (matches('settings')) {
        event.preventDefault();
        setSettingsOpen(true);
      }
      if (
        matches('history') &&
        shortcuts.value.tab
      ) {
        event.preventDefault();
        setHistoryOpen((v) => !v);
      }
      if (matches('newTab')) {
        event.preventDefault();
        shortcuts.value.newTab();
      }
    };
    document.addEventListener('keydown', key, true);
    return () => document.removeEventListener('keydown', key, true);
  },
);
function openHistory(entry: QueryHistoryEntry) {
  const _connections3 = connections.value;
  if (!queryStorage.library) return;
  const next: QueryTab = {
    id: uid(),
    name: queryStorage.nextDraftName(),
    sql: entry.sql,
    savedSql: '',
    connectionId: entry.connectionId,
    database: entry.database,
    draft: true,
  };
  setTabs((prev) => [...prev, next]);
  setActive(next.id);
  setResultIndex(0);
  setHistoryOpen(false);
  if (!_connections3.some((c) => c.id === entry.connectionId))
    setNotice(
      'The original connection is no longer saved. Select a connection before running this query.',
    );
}
function reopenQuery(query: SavedQuery) {
  const _tabs7 = tabs.value;
  const existing = _tabs7.find((t) => t.id === query.id);
  if (!existing)
    setTabs((prev) => [
      ...prev,
      {
        ...query,
        savedSql: query.sql,
      },
    ]);
  setActive(query.id);
}
const document = globalThis.document;
</script>

<template>
  <div class="app">
    <header
      :class="'topbar' + (hasCustomTitlebar() ? ' custom-titlebar' : '')"
      :data-tauri-drag-region="hasCustomTitlebar() ? true : undefined"
    >
      <div
        class="brand"
        :data-tauri-drag-region="hasCustomTitlebar() ? true : undefined"
      >
        <img src="/goblin.svg" alt="" />
        <strong>DataGoblin</strong>
        <span class="app-version" :title="`DataGoblin version ${appVersion}`">v{{ appVersion }}</span>
        <span v-if="developmentBuild" class="development-label">Development</span>
        <span class="alpha">EARLY ACCESS</span>
      </div>
      <WorkspaceControls
        :panels="panels"
        @toggle="togglePanel"
      ></WorkspaceControls>
      <div class="top-actions">
        <AppUpdater :blocked="Object.values(running).some(Boolean) || Object.values(fetching).some(Boolean) || Object.values(editPending).some(Boolean)" :before-install="prepareUpdate" />
        <button
          class="icon-button"
          title="Settings"
          @click="() => setSettingsOpen(true)"
        >
          <Settings2 :size="17"></Settings2>
        </button>
        <button
          class="icon-button"
          title="Keyboard shortcuts"
          @click="() => setHelp(true)"
        >
          <CircleHelp :size="17"></CircleHelp>
        </button>
        <WindowControls @error="setError"></WindowControls>
      </div>
    </header>
    <IslandWorkspace :hasResults="!!tab" :panels="panels">
      <template #explorer>
        <DatabaseExplorer
          :reveal="reveal"
          :connections="connections"
          :databases="databases"
          :tables="tableCache"
          :schemas="schemaCache"
          :routines="routineCache"
          :scope="explorerScope"
          :onScopeChange="changeExplorerScope"
          :onRefreshSchema="refreshSchema"
          :columns="columnCache"
          :objects="objectCache"
          @browse="browse"
          @columns="expandTable"
          :onSelect="
            ((_tab14) => (c) => {
              setExplorerSelection({
                connectionId: c.id,
                database: c.database,
              });
              if (_tab14 && !_tab14.connectionId)
                updateTab(_tab14.id, {
                  connectionId: c.id,
                  database: c.database,
                });
            })(tab!)
          "
          @open="openExplorerObject"
          @edit="setDialog"
          @new="() => setDialog('new')"
        ></DatabaseExplorer>
      </template>
      <template #editor>
        <main>
          <div class="tabs">
            <template v-for="t in tabs" :key="t.id">
              <div
                :class="'tab ' + (t.id === tab?.id ? 'active' : '')"
                @contextmenu="
                  (event) => {
                    event.preventDefault();
                    setTabMenu({
                      query: t,
                      position: menuPosition(event),
                    });
                  }
                "
                @keydown="
                  (event) => {
                    if (
                      event.key === 'ContextMenu' ||
                      (event.shiftKey && event.key === 'F10')
                    ) {
                      event.preventDefault();
                      const origin = event.target as HTMLElement;
                      const rect = origin.getBoundingClientRect();
                      setTabMenu({
                        query: t,
                        position: {
                          x: rect.left,
                          y: rect.bottom,
                          origin,
                        },
                      });
                    }
                  }
                "
                @mousedown="
                  (event) => {
                    if (event.button === 1) event.preventDefault();
                  }
                "
                @auxclick="
                  (event) => {
                    if (event.button === 1) {
                      event.preventDefault();
                      void closeTab(t.id);
                    }
                  }
                "
              >
                <button
                  @click="
                    () => {
                      setActive(t.id);
                      setResultIndex(0);
                    }
                  "
                >
                  <FileCode2 :size="14"></FileCode2>
                  <span class="tab-name" :title="t.name">
                    <template v-if="t.pinned">
                      <span aria-label="Pinned query">
                        <Pin :size="11"></Pin>
                      </span>
                    </template>
                    {{ t.name }}
                  </span>
                  <template v-if="t.sql !== t.savedSql">
                    <span class="dirty"></span>
                  </template>
                </button>
                <button
                  class="icon-button"
                  title="Close query"
                  :disabled="!!running[t.id]"
                  @click="() => closeTab(t.id)"
                >
                  <X :size="12"></X>
                </button>
              </div>
            </template>
            <button
              class="icon-button add-tab"
              title="New query"
              @click="() => newTab()"
            >
              <Plus :size="16"></Plus>
            </button>
          </div>
          <template v-if="tab">
            <div class="toolbar">
              <button
                class="run-button"
                aria-label="Run all"
                :disabled="!!runBlockReason"
                :title="runBlockReason || 'Run all SQL in this tab; use keyboard shortcut for current statement'"
                @click="
                  () => {
                    const _tab14 = tab!;
                    return void run(_tab14.sql);
                  }
                "
              >
                <Play :size="19"></Play>
              </button>
              <span class="separator" aria-hidden="true"></span>
              <button
                class="icon-button cancel-query-button"
                :disabled="
                  editPending[tab.id] ||
                  (!running[tab.id] &&
                    !(
                      result?.cacheId &&
                      result.complete === false &&
                      !result.cancelled
                    ))
                "
                title="Cancel query"
                @click="() => void cancelCurrent()"
              >
                <Square :size="19"></Square>
              </button>
              <button
                class="icon-button"
                title="Query history"
                aria-label="Query history"
                :aria-pressed="historyOpen"
                @click="() => setHistoryOpen((v) => !v)"
              >
                <History :size="17"></History>
              </button>
              <button
                class="icon-button"
                aria-label="Format SQL"
                :title="`Format SQL (selection or entire query) · ${bindingLabel('format')}`"
                @click="() => editorHandle?.format()"
              >
                <AlignLeft :size="17"></AlignLeft>
              </button>
              <span class="toolbar-context" :title="effective?.host">
                {{ effective ? effective.name : 'No connection selected' }}
              </span>
              <template v-if="effective">
                <label class="query-database-picker">
                  <Database :size="14"></Database>
                  <Select
                    label="Query database"
                    title="Database for this query"
                    searchable
                    :disabled="!!running[tab.id] || !!fetching[tab.id]"
                    :value="effective.database"
                    :onChange="
                      ((_effective8) => (value) => {
                        return selectDatabase(_effective8.id, value);
                      })(effective!)
                    "
                    :options="
                      ((_effective8, _databases) =>
                        [
                          ...new Set([
                            _effective8.database,
                            ...(_databases[_effective8.id] ?? []),
                          ]),
                        ].map((value) => ({
                          value,
                          label: value,
                        })))(effective, databases)
                    "
                  ></Select>
                </label>
              </template>
              <span class="dialect">
                {{ effective?.engine === 'sqlserver' ? 'T-SQL' : 'PostgreSQL' }}
              </span>
            </div>
          </template>
          <template v-if="!isTauri()">
            <div class="banner info">
              Browser shell only. Run
              <code>npm run desktop</code>
              for database connections and file access.
            </div>
          </template>
          <template v-if="error">
            <div class="banner error" role="alert">
              <span>{{ error }}</span>
              <button
                class="icon-button"
                title="Dismiss error"
                @click="() => setError('')"
              >
                <X :size="14"></X>
              </button>
            </div>
          </template>
          <template v-if="notice">
            <div class="banner info" role="status">
              <span>{{ notice }}</span>
              <button
                class="icon-button"
                title="Dismiss notification"
                @click="() => setNotice('')"
              >
                <X :size="14"></X>
              </button>
            </div>
          </template>
          <template v-if="tab">
            <section class="editor-pane">
              <Editor
                ref="editorHandle"
                @format-error="setError"
                :tables="tables"
                :completion-catalog="completionCatalog"
                :columns="columns"
                @reveal="revealTable"
                @metadata="editorMetadata"
                @open-data="(table) => void openEditorData(table)"
                :execution="executionMarks[tab.id]"
                :key="tab.id"
                :value="tab.sql"
                :engine="effective?.engine ?? 'postgres'"
                :schema="schema"
                :onChange="
                  ((_tab14) => (sql) => {
                    return updateTab(_tab14.id, {
                      sql,
                    });
                  })(tab!)
                "
                @run="(sql, from) => void run(sql, from)"
                @save="() => void saveTab()"
              ></Editor>
            </section>
          </template>
          <template v-else>
            <section class="workspace-empty">
              <div class="empty-mark"><FileCode2 :size="28"></FileCode2></div>
              <h2>No queries open</h2>
              <p>
                Create a query, import a SQL file, or reopen one from Saved
                Queries.
              </p>
              <div>
                <button
                  class="primary-button"
                  :disabled="!queryStorage.library"
                  @click="() => newTab()"
                >
                  <Plus :size="15"></Plus>
                  Create query
                </button>
                <button
                  class="secondary-button"
                  @click="() => void openQuery()"
                >
                  <FolderOpen :size="15"></FolderOpen>
                  Import SQL file
                </button>
              </div>
            </section>
          </template>
        </main>
      </template>
      <template #queries>
        <QueryPane
          :library="queryStorage.library"
          :activeId="tab?.id ?? ''"
          @open="reopenQuery"
          @new="() => newTab()"
          @settings="() => setSettingsOpen(true)"
          @update="queryStorage.updateQuery"
          :onDelete="
            ((_fetching9) => async (id) => {
              if (activeRequests.has(id) || _fetching9[id])
                throw new Error(
                  'Finish the running query or apply/revert pending edits before deleting it.',
                );
              await queryStorage.deleteQuery(id);
              setResults((current) => {
                const next = {
                  ...current,
                };
                delete next[id];
                return next;
              });
              setActive((current) => (current === id ? '' : current));
            })(fetching!)
          "
        ></QueryPane>
      </template>
      <template #results>
        <template v-if="tab">
          <section class="results-pane">
            <div class="result-toolbar result-tabs-toolbar">
              <span class="result-panel-title">Database Output</span>
              <div
                class="result-tabs"
                role="tablist"
                aria-label="Query results"
                @keydown="
                  (event) => {
                    if (
                      !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(
                        event.key,
                      )
                    )
                      return;
                    const buttons = Array.from(
                      (
                        event.currentTarget as HTMLInputElement
                      ).querySelectorAll<HTMLButtonElement>(
                        '[role=\u0022tab\u0022]:not(:disabled)',
                      ),
                    );
                    const index = buttons.indexOf(
                      document.activeElement as HTMLButtonElement,
                    );
                    const next =
                      event.key === 'Home'
                        ? 0
                        : event.key === 'End'
                          ? buttons.length - 1
                          : (index +
                              (event.key === 'ArrowRight' ? 1 : -1) +
                              buttons.length) %
                            buttons.length;
                    event.preventDefault();
                    buttons[next]?.focus();
                    buttons[next]?.click();
                  }
                "
              >
                <button
                  role="tab"
                  :aria-selected="outputVisible"
                  :disabled="!!fetching[tab.id]"
                  @click="() => selectResult('output')"
                >
                  <Terminal :size="13"></Terminal>
                  Output
                  <template v-if="outputs[tab.id]?.failed">
                    <span class="output-error-dot"></span>
                  </template>
                </button>
                <template
                  v-for="id in [
                    tab.id,
                    ...Object.keys(pinnedResults).filter(
                      (id) => pinnedResults[id].owner === tab!.id,
                    ),
                  ]"
                >
                  <template v-for="(set, index) in results[id]?.sets ?? []">
                    <template
                      v-if="set.columns.length && !resultSetClosed(id, index)"
                    >
                      <div
                        :class="
                          'result-tab-item' +
                          (!outputVisible &&
                          resultId === id &&
                          Math.min(resultIndex, results[id].sets.length - 1) ===
                            index
                            ? ' selected'
                            : '')
                        "
                        :key="id + ':' + index"
                        @mousedown="
                          (event) => {
                            if (event.button === 1) event.preventDefault();
                          }
                        "
                        @auxclick="
                          (event) => {
                            if (event.button === 1) {
                              event.preventDefault();
                              event.stopPropagation();
                              closeResultTab(id, index);
                            }
                          }
                        "
                      >
                        <button
                          role="tab"
                          :aria-selected="
                            !outputVisible &&
                            resultId === id &&
                            Math.min(
                              resultIndex,
                              results[id].sets.length - 1,
                            ) === index
                          "
                          :disabled="!!fetching[tab.id]"
                          @click="() => selectResult(id, index)"
                          :title="
                            pinnedResults[id]
                              ? 'Pinned result · ' +
                                pinnedResults[id].connection.database
                              : undefined
                          "
                        >
                          <template v-if="pinnedResults[id]">
                            <Pin :size="12"></Pin>
                          </template>
                          <template v-else>
                            <Table2 :size="13"></Table2>
                          </template>
                          <span>
                            {{
                              results[id].browser?.table
                                ? [
                                    pinnedResults[id]?.connection.database ??
                                      effective?.database,
                                    results[id].browser.table,
                                  ]
                                    .filter(Boolean)
                                    .join('.')
                                : (pinnedResults[id]?.name ?? tab.name)
                            }}{{
                              results[id].sets.length > 1
                                ? ` · ${index + 1}`
                                : ''
                            }}
                          </span>
                        </button>
                        <button
                          class="result-tab-close"
                          title="Close result tab"
                          :aria-label="
                            pinnedResults[id]
                              ? 'Close pinned result ' + pinnedResults[id].name
                              : 'Close result ' + (index + 1)
                          "
                          :disabled="!!fetching[tab.id] || !!running[tab.id]"
                          @click="() => closeResultTab(id, index)"
                        >
                          <X :size="12"></X>
                        </button>
                      </div>
                    </template>
                    <template v-else></template>
                  </template>
                </template>
              </div>
              <div class="result-toolbar-context">
                <template
                  v-if="
                    running[tab.id] && (outputVisible || !set?.columns.length)
                  "
                >
                  <LoaderCircle :size="13" class="spin"></LoaderCircle>
                </template>
                <span class="result-meta">
                  {{
                    outputVisible
                      ? outputs[tab.id]?.database
                      : resultConnection?.database
                  }}{{
                    (outputVisible ? outputs[tab.id] : result) &&
                    ` · ${outputVisible ? outputs[tab.id]?.duration : result?.elapsedMs} ms`
                  }}
                </span>
                <template
                  v-if="!outputVisible && result && !pinnedResults[resultId]"
                >
                  <button
                    class="icon-button"
                    aria-label="Pin result"
                    :disabled="!!running[tab.id] || !!fetching[tab.id]"
                    @click="pinResult"
                  >
                    <Pin :size="13"></Pin>
                  </button>
                </template>
                <template
                  v-if="
                    !outputVisible &&
                    result?.cacheId &&
                    result.complete === false &&
                    !result.cancelled
                  "
                >
                  <button
                    class="text-button"
                    :disabled="!!fetching[tab.id] || !!running[tab.id]"
                    title="Fetch remaining rows in the current result to reach the next result set"
                    @click="() => void nextResult()"
                  >
                    Next result set
                  </button>
                </template>
              </div>
            </div>
            <template v-if="outputVisible">
              <QueryOutput :log="outputs[tab.id]"></QueryOutput>
            </template>
            <template v-else>
              <template v-if="result?.truncated">
                <div class="banner info">
                  Result cache limit reached (1,000,000 rows or 128 MiB). Only
                  cached rows can be paged. Connection closed; later batch
                  statements may not have completed.
                </div>
              </template>
              <template v-if="set?.columns.length">
                <PagedResults
                  ref="pagedResults"
                  :onLog="
                    ((_tab14, _resultConnection6) => (event) => {
                      return logOutput(
                        _tab14.id,
                        _resultConnection6?.database ?? _tab14.database,
                        event,
                      );
                    })(tab!, resultConnection!)
                  "
                  :logDatabase="resultConnection?.database ?? tab.database"
                  @cancel="() => void cancelCurrent()"
                  :cancellable="
                    !!running[tab.id] ||
                    !!(
                      result?.cacheId &&
                      result.complete === false &&
                      !result.cancelled
                    )
                  "
                  :refreshInterval="refreshIntervals[resultId] ?? 0"
                  :onRefreshInterval="
                    ((_resultId8) => (seconds) => {
                      return setRefreshIntervals((prev) => ({
                        ...prev,
                        [_resultId8]: seconds,
                      }));
                    })(resultId!)
                  "
                  :onEditingChange="
                    ((_tab14) => (dirty) => {
                      return setEditPending((prev) =>
                        prev[_tab14.id] === dirty
                          ? prev
                          : {
                              ...prev,
                              [_tab14.id]: dirty,
                            },
                      );
                    })(tab!)
                  "
                  :onReload="
                    ((_result7) =>
                      _result7?.browser
                        ? () =>
                            browseResults(
                              _result7.browser!.filters,
                              _result7.browser!.sort,
                              true,
                            )
                        : _result7?.sets[0]?.joinEditable ? () => browseResults([], null, true) : undefined)(result)
                  "
                  @reference="openReference"
                  :engine="resultConnection?.engine"
                  :browser="result?.browser"
                  @browse="browseResults"
                  :browserBusy="!!running[tab.id]"
                  :key="
                    resultId +
                    ':' +
                    resultIndex +
                    ':' +
                    (result?.cacheId ?? resultVersion(result!))
                  "
                  :data="set"
                  :cacheId="result?.cacheId"
                  :setIndex="Math.min(resultIndex, result!.sets.length - 1)"
                  :truncated="result!.truncated"
                  :initialSize="set.pageSize ?? result?.pageSize ?? 500"
                  :closed="result?.cancelled"
                  :onUpdate="
                    ((_resultId8, _result7) => (next) => {
                      return updatePagedResult(
                        _resultId8,
                        _result7!.cacheId!,
                        next,
                      );
                    })(resultId!, result!)
                  "
                  :onBusy="
                    ((_tab14) => (busy) => {
                      return setFetching((prev) =>
                        prev[_tab14.id] === busy
                          ? prev
                          : {
                              ...prev,
                              [_tab14.id]: busy,
                            },
                      );
                    })(tab!)
                  "
                  @export="exportCsv"
                ></PagedResults>
              </template>
              <template v-else>
                <template v-if="result">
                  <div class="result-empty">
                    <Check :size="28"></Check>
                    <h3>Query completed</h3>
                    <p>
                      {{
                        set?.affected != null
                          ? set.affected + ' rows affected'
                          : 'No result set returned.'
                      }}
                    </p>
                  </div>
                </template>
                <template v-else>
                  <div class="result-empty">
                    <div class="empty-mark">
                      <Terminal :size="25"></Terminal>
                    </div>
                    <h3>Ready to run</h3>
                    <p>Run your query to view and explore its results.</p>
                    <span>{{ editorPreferences.vim ? 'Use :RunLine to run the current line' : bindingLabel('run') + ' to run the current statement' }}</span>
                  </div>
                </template>
              </template>
            </template>
          </section>
        </template>
        <template v-else></template>
      </template>
      <template #footer>
        <footer>
          <span class="status-dot"></span>
          {{
            currentRequest
              ? 'Running query…'
              : tab && editPending[tab.id]
                ? 'Pending cell changes · Apply or Revert'
                : tab && fetching[tab.id]
                  ? 'Fetching rows…'
                  : queryStorage.status
          }}
          <span class="footer-right">
            Auto-commit · Results fetched on demand
          </span>
        </footer>
      </template>
    </IslandWorkspace>
    <template v-if="tabMenu">
      <ContextMenu
        :position="tabMenu.position"
        :label="'Query actions for ' + tabMenu.query.name"
        @close="() => setTabMenu(null)"
        :actions="
          ((_running4, _tabMenu, _fetching9, _tabs8) => [
            {
              label: 'Close',
              disabled:
                !!_running4[_tabMenu.query.id] ||
                !!_fetching9[_tabMenu.query.id],
              action: () => void closeTab(_tabMenu.query.id),
            },
            {
              label: 'Close other tabs',
              disabled: !_tabs8.some(
                (t) =>
                  t.id !== _tabMenu.query.id &&
                  !t.pinned &&
                  !_running4[t.id] &&
                  !_fetching9[t.id],
              ),
              action: () =>
                void closeTabs(
                  _tabs8
                    .filter((t) => t.id !== _tabMenu.query.id && !t.pinned)
                    .map((t) => t.id),
                ),
            },
            {
              label: 'Close all unpinned tabs',
              disabled: !_tabs8.some(
                (t) => !t.pinned && !_running4[t.id] && !_fetching9[t.id],
              ),
              action: () =>
                void closeTabs(
                  _tabs8.filter((t) => !t.pinned).map((t) => t.id),
                ),
            },
            {
              label: _tabMenu.query.pinned ? 'Unpin' : 'Pin',
              separator: true,
              action: () => {
                void changeTabDetails(_tabMenu.query, {
                  pinned: !_tabMenu.query.pinned,
                }).catch((e) => setError(errorText(e)));
              },
            },
            {
              label: 'Rename',
              action: () => {
                setRenameTab(_tabMenu.query);
                setRenameName(_tabMenu.query.name);
                setRenameError('');
              },
            },
            {
              label: 'Duplicate',
              action: () =>
                newTab(_tabMenu.query.sql, undefined, _tabMenu.query),
            },
          ])(running, tabMenu, fetching, tabs)
        "
      ></ContextMenu>
    </template>
    <template v-if="renameTab">
      <DialogBoundary @close="() => setRenameTab(null)" :disabled="renameBusy">
        <div class="overlay">
          <form
            class="modal"
            role="dialog"
            aria-modal="true"
            aria-label="Rename query tab"
            @submit="
              (event) => {
                const _renameTab = renameTab!;
                const _renameName = renameName!;
                event.preventDefault();
                setRenameBusy(true);
                void changeTabDetails(_renameTab, {
                  name: _renameName.trim(),
                })
                  .then(() => setRenameTab(null))
                  .catch((e) => setRenameError(errorText(e)))
                  .finally(() => setRenameBusy(false));
              }
            "
          >
            <h2>Rename query</h2>
            <label>
              Name
              <input
                autofocus
                aria-label="Query tab name"
                :value="renameName"
                @input="
                  (event) =>
                    setRenameName((event.target as HTMLInputElement).value)
                "
                required
              />
            </label>
            <template v-if="renameError">
              <p role="alert">{{ renameError }}</p>
            </template>
            <div class="modal-footer">
              <button
                type="button"
                :disabled="renameBusy"
                @click="() => setRenameTab(null)"
              >
                Cancel
              </button>
              <button
                class="primary-button"
                :disabled="renameBusy || !renameName.trim()"
              >
                Rename
              </button>
            </div>
          </form>
        </div>
      </DialogBoundary>
    </template>
    <template v-if="historyOpen && tab">
      <QueryHistory
        :key="tab.id"
        :queryId="tab.id"
        :queryName="tab.name"
        :version="historyVersion"
        @open="openHistory"
        @close="() => setHistoryOpen(false)"
      ></QueryHistory>
    </template>
    <template v-if="dialog">
      <ConnectionDialog
        :initial="dialog === 'new' ? undefined : dialog"
        @close="() => setDialog(null)"
        :onTest="
          ((_dialog) => async (c, password) => {
            await invoke('query', {
              connection: c,
              password: password === '' && _dialog !== 'new' ? null : password,
              sql: 'SELECT 1',
              requestId: uid(),
            });
          })(dialog!)
        "
        :onSave="
          (
            (_dialog, _connections4, _tab14) =>
            async (c, password, remember) => {
              if (!isTauri())
                throw new Error('Open the desktop app to save connections.');
              if (!remember) {
                if (password === '' && _dialog !== 'new')
                  throw new Error(
                    'Enter the password to switch this connection to session-only storage.',
                  );
                await invoke('forget_password', {
                  id: c.id,
                });
              }
              if (password !== '' || _dialog === 'new') {
                if (remember)
                  await invoke('save_password', {
                    id: c.id,
                    password,
                  });
                passwords.set(c.id, password);
              }
              const next = [..._connections4.filter((x) => x.id !== c.id), c];
              await invoke('save_connections', {
                connections: next,
              });
              setConnections(next);
              setTableCache((prev) =>
                Object.fromEntries(
                  Object.entries(prev).filter(
                    ([key]) => JSON.parse(key)[0] !== c.id,
                  ),
                ),
              );
              setColumnCache((prev) =>
                Object.fromEntries(
                  Object.entries(prev).filter(
                    ([key]) => JSON.parse(key)[0] !== c.id,
                  ),
                ),
              );
              setObjectCache((prev) =>
                Object.fromEntries(
                  Object.entries(prev).filter(
                    ([key]) => JSON.parse(key)[0] !== c.id,
                  ),
                ),
              );
              setExplorerSelection({
                connectionId: c.id,
                database: c.database,
              });
              if (_tab14 && !_tab14.connectionId)
                updateTab(_tab14.id, {
                  connectionId: c.id,
                  database: c.database,
                });
              setDialog(null);
              await browse(c, true);
            }
          )(dialog!, connections!, tab!)
        "
      ></ConnectionDialog>
    </template>
    <template v-if="help">
      <DialogBoundary @close="() => setHelp(false)">
        <div
          class="overlay"
          @mousedown="
            (e) => {
              if (
                (e.target as HTMLInputElement) ===
                (e.currentTarget as HTMLInputElement)
              )
                setHelp(false);
            }
          "
        >
          <section
            class="modal shortcuts"
            role="dialog"
            aria-modal="true"
            aria-label="Keyboard shortcuts"
          >
            <div class="modal-heading">
              <h2>Keep your hands on the keyboard.</h2>
              <button
                class="icon-button"
                title="Close shortcuts"
                @click="() => setHelp(false)"
              >
                <X :size="18"></X>
              </button>
            </div>
            <template
              v-for="[key, action] in [
                ...shortcutIds.map(id => [bindingLabel(id), shortcutDefinitions[id][0]]),
                ['Ctrl + Space', 'Show autocomplete'],
                ['sel + Tab', 'Expand SELECT template'],
                [mod + ' + /', 'Toggle line comment'],
                [mod + ' + F', 'Find in query'],
                [mod + ' + Z', 'Undo'],
                ['Escape', 'Dismiss dialog or popover'],
              ]"
              :key="key"
            >
              <div class="shortcut-row">
                <span>{{ action }}</span>
                <kbd>{{ key }}</kbd>
              </div>
            </template>
            <p class="muted">
              Expand a table in the explorer to load its columns into
              autocomplete.
            </p>
          </section>
        </div>
      </DialogBoundary>
    </template>
    <template v-if="settingsOpen">
      <QuerySettings
        :library="queryStorage.library"
        @change="queryStorage.changeFolder"
        @close="() => setSettingsOpen(false)"
      ></QuerySettings>
    </template>
  </div>
</template>
