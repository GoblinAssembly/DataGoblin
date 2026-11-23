// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from './testUtils';
import { shallowRef } from 'vue';
import { startCompletion, completionStatus } from '@codemirror/autocomplete';
import { EditorView } from '@codemirror/view';
import { getCM, Vim } from '@replit/codemirror-vim';
import Editor from './Editor.vue';
import QuerySettings from './QuerySettings.vue';
import ThemeProvider from './ThemeProvider.vue';
import { vimPreferenceKey } from './editorPreferences';

afterEach(() => { cleanup(); localStorage.clear(); });
async function mount(value = 'select 42 from Invoice;') {
  const save = vi.fn(), run = vi.fn();
  const settings = shallowRef(true);
  const result = await render(() => <ThemeProvider>
    <button onClick={() => settings.value = true}>Editor settings</button>
    <Editor value={value} engine='sqlserver' schema={{}} onChange={vi.fn()} onSave={save} onRun={run} />
    <>{settings.value && <QuerySettings library={null} onChange={vi.fn()} onClose={() => settings.value = false} />}</>
  </ThemeProvider>);
  const view = EditorView.findFromDOM(result.container.querySelector('.cm-editor')!)!;
  await fireEvent.click(screen.getByRole('tab', { name: 'Editor' }));
  return { ...result, view, save, run };
}
it('toggles live, persists the preference, and preserves text, selection and undo history', async () => {
  const { view } = await mount();
  expect(getCM(view)).toBeNull();
  view.dispatch({ changes: { from: 0, to: 6, insert: 'SELECT' }, selection: { anchor: 7 } });
  await fireEvent.click(screen.getByLabelText('Vim mode'));
  await waitFor(() => expect(getCM(view)).toBeTruthy());
  expect(localStorage.getItem(vimPreferenceKey)).toBe('true');
  expect(view.state.doc.toString()).toBe('SELECT 42 from Invoice;');
  expect(view.state.selection.main.head).toBe(7);
  await fireEvent.click(screen.getByTitle('Close settings'));
  view.focus();
  await fireEvent.keyDown(view.contentDOM, { key: 'u' });
  expect(view.state.doc.toString()).toBe('select 42 from Invoice;');
  await fireEvent.click(screen.getByRole('button', { name: 'Editor settings' }));
  await fireEvent.click(screen.getByRole('tab', { name: 'Editor' }));
  await fireEvent.click(screen.getByLabelText('Vim mode'));
  await waitFor(() => expect(getCM(view)).toBeNull());
  expect(localStorage.getItem(vimPreferenceKey)).toBe('false');
  expect(view.state.doc.toString()).toBe('select 42 from Invoice;');
});
it('restores Vim and supports motions, change-word, Escape, increment, run and save', async () => {
  localStorage.setItem(vimPreferenceKey, 'true');
  const { view, save, run } = await mount();
  const cm = getCM(view)!;
  expect(cm.state.vim?.insertMode).toBe(false);
  await fireEvent.click(screen.getByTitle('Close settings'));
  view.focus();
  await fireEvent.keyDown(view.contentDOM, { key: 'w' });
  expect(view.state.selection.main.head).toBe(7);
  await fireEvent.keyDown(view.contentDOM, { key: 'a', ctrlKey: true });
  expect(view.state.doc.toString()).toBe('select 43 from Invoice;');
  for (const key of ['c', 'i', 'w']) await fireEvent.keyDown(view.contentDOM, { key });
  expect(cm.state.vim?.insertMode).toBe(true);
  expect(view.state.doc.toString()).toBe('select  from Invoice;');
  view.dispatch(view.state.replaceSelection('99'));
  await fireEvent.keyDown(view.contentDOM, { key: 'Escape' });
  expect(cm.state.vim?.insertMode).toBe(false);
  await fireEvent.keyDown(view.contentDOM, { key: 'Enter', ctrlKey: true });
  expect(run).toHaveBeenCalledWith('select 99 from Invoice;', 0);
  await fireEvent.keyDown(view.contentDOM, { key: 's', ctrlKey: true });
  expect(save).toHaveBeenCalledOnce();
  Vim.handleEx(cm, 'write');
  expect(save).toHaveBeenCalledTimes(2);
});

it('keeps SQL completion in insert mode and exits cleanly with Escape', async () => {
  Object.defineProperty(Range.prototype, 'getClientRects', { configurable: true, value: () => [] });
  Object.defineProperty(Range.prototype, 'getBoundingClientRect', { configurable: true, value: () => new DOMRect() });
  localStorage.setItem(vimPreferenceKey, 'true');
  const { view } = await mount('sel');
  await fireEvent.click(screen.getByTitle('Close settings'));
  view.focus();
  await fireEvent.keyDown(view.contentDOM, { key: 'A', shiftKey: true });
  expect(getCM(view)?.state.vim?.insertMode).toBe(true);
  startCompletion(view);
  await waitFor(() => expect(completionStatus(view.state)).toBe('active'));
  await fireEvent.keyDown(view.contentDOM, { key: 'Tab' });
  expect(view.state.doc.toString()).toBe('select * from table;');
  await fireEvent.keyDown(view.contentDOM, { key: 'Escape' });
  expect(getCM(view)?.state.vim?.insertMode).toBe(false);
  expect(completionStatus(view.state)).toBeNull();
});

it('runs a line or script, toggles comments, and uses the system clipboard', async () => {
  localStorage.setItem(vimPreferenceKey, 'true');
  const sql = 'select 1;\nselect 2;';
  const { view, run } = await mount(sql);
  const clipboard = { writeText: vi.fn().mockResolvedValue(undefined), readText: vi.fn().mockResolvedValue('external') };
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: clipboard });
  await fireEvent.click(screen.getByTitle('Close settings'));
  view.focus();
  view.dispatch({ selection: { anchor: 10 } });
  const key = (key: string, modifiers = {}) => fireEvent.keyDown(view.contentDOM, { key, ...modifiers });
  await key('Enter', { ctrlKey: true });
  expect(run).toHaveBeenLastCalledWith('select 2;', 10);
  await key('Enter', { shiftKey: true });
  expect(run).toHaveBeenLastCalledWith(sql, 0);
  await key(' ');
  await key('c');
  expect(view.state.doc.line(2).text).toContain('--');
  await key(' ');
  await key('c');
  await key('y');
  await key('y');
  expect(clipboard.writeText).toHaveBeenCalledWith('select 2;\n');
  await key('p');
  await waitFor(() => expect(view.state.doc.toString()).toContain('external'));
});

it('saves a custom leader binding and runs the selected text', async () => {
  localStorage.setItem(vimPreferenceKey, 'true');
  const { view, run } = await mount('select 1; select 2;');
  await fireEvent.click(screen.getByRole('tab', { name: 'Keybindings' }));
  const mappings = 'vmap <Leader>r :RunLine<CR>';
  (screen.getByLabelText('Vim mappings') as HTMLTextAreaElement).value = mappings;
  await fireEvent(screen.getByLabelText('Vim mappings'), new Event('change', { bubbles: true }));
  expect(screen.queryByRole('alert')?.textContent).toBeUndefined();
  expect(localStorage.getItem('datagoblin.editor.vimMappings')).toBe(mappings);
  await fireEvent.click(screen.getByTitle('Close settings'));
  view.focus();
  for (const key of ['v', 'l', ' ', 'r']) await fireEvent.keyDown(view.contentDOM, { key });
  expect(run).toHaveBeenCalledWith('se', 0);
});



it('records a Save shortcut in Settings and updates the open editor', async () => {
  const { view, save } = await mount();
  await fireEvent.click(screen.getByRole('tab', { name: 'Keybindings' }));
  const button = screen.getByRole('button', { name: 'Rebind Save SQL file' });
  await fireEvent.click(button);
  await fireEvent.keyDown(button, { key: 'F8' });
  expect(button.textContent?.trim()).toBe('F8');
  expect(localStorage.getItem('datagoblin.editor.shortcuts')).toContain('F8');
  await fireEvent.click(screen.getByTitle('Close settings'));
  view.focus();
  await fireEvent.keyDown(view.contentDOM, { key: 's', ctrlKey: true });
  expect(save).not.toHaveBeenCalled();
  await fireEvent.keyDown(view.contentDOM, { key: 'F8' });
  expect(save).toHaveBeenCalledOnce();
});
