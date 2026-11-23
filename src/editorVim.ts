import { Facet } from '@codemirror/state';
import { toggleLineComment } from '@codemirror/commands';
import type { EditorView } from '@codemirror/view';
import { Vim } from '@replit/codemirror-vim';

export const vimRun = Facet.define<(sql: string, from: number) => void>();
export const defaultMappings = `map <C-CR> :RunLine<CR>
imap <C-CR> :RunLine<CR>
map <S-CR> :RunScript<CR>
imap <S-CR> :RunScript<CR>
map <Leader>c :ToggleComment<CR>`;

const commands = {
  RunLine(view: EditorView) {
    const selection = view.state.selection.main;
    const range = selection.empty ? view.state.doc.lineAt(selection.head) : selection;
    view.state.facet(vimRun)[0](view.state.sliceDoc(range.from, range.to), range.from);
  },
  RunScript(view: EditorView) { view.state.facet(vimRun)[0](view.state.doc.toString(), 0); },
  ToggleComment: toggleLineComment,
};
for (const [name, run] of Object.entries(commands)) {
  Vim.defineEx(name, undefined, cm => run(cm.cm6));
  Vim.defineAction(name, cm => run(cm.cm6));
}

Vim.defineAction('clipboardPaste', async (cm, args) => {
  let text: string;
  try { text = (await navigator.clipboard.readText()).replace(/\r\n/g, '\n'); }
  catch { cm.openNotification(document.createTextNode('Could not read the system clipboard.'), { bottom: true }); return; }
  if (!text || !cm.cm6.hasFocus) return;
  const registers = Vim.getRegisterController();
  registers.unnamedRegister.setText(text, text.endsWith('\n'));
  for (const digit of String(args.repeat || 1)) {
    if ((args.repeat || 1) > 1) Vim.handleKey(cm, digit, 'mapping');
  }
  Vim.handleKey(cm, args.after ? '<DataGoblinPasteAfter>' : '<DataGoblinPasteBefore>', 'mapping');
});

function parseMappings(source: string) {
  return source.split(/\r?\n/).flatMap((line, i) => {
    const text = line.trim();
    if (!text || text.startsWith('"')) return [];
    const match = /^([nvi]?)(map|noremap)\s+(\S+)\s+(.+)$/.exec(text);
    if (!match) throw Error(`Line ${i + 1}: expected a Vim mapping such as nmap <Leader>r :RunLine<CR>.`);
    const [, mode, kind, lhs, rhs] = match;
    const action = /^:(\w+)(?:<CR>)?$/i.exec(rhs)?.[1];
    if (rhs.startsWith(':') && (!action || !Object.hasOwn(commands, action))) throw Error(`Line ${i + 1}: unknown editor command.`);
    return [{ mode, kind, lhs: lhs.replace(/<leader>/ig, '<DataGoblinLeader>'), rhs, action }];
  });
}
let active: string | undefined;
export function configureMappings(source: string) {
  if (active === source) return;
  const mappings = parseMappings(source);
  Vim.mapclear();
  for (const mode of ['normal', 'visual']) {
    Vim.map('<Space>', '<DataGoblinLeader>', mode);
    for (const key of ['y', 'Y']) Vim.noremap(key, '"+' + key, mode);
    for (const after of [true, false]) {
      Vim.mapCommand(after ? 'p' : 'P', 'action', 'clipboardPaste', { after }, { context: mode });
      Vim.mapCommand(after ? '<DataGoblinPasteAfter>' : '<DataGoblinPasteBefore>', 'action', 'paste', { after }, { context: mode, isEdit: true });
    }
  }
  for (const {mode, kind, lhs, rhs, action} of mappings) {
    const contexts = mode === 'i' ? ['insert'] : mode === 'v' ? ['visual'] : mode === 'n' ? ['normal'] : ['normal', 'visual'];
    for (const context of contexts) {
      if (action) Vim.mapCommand(lhs, 'action', action, {}, { context });
      else if (kind === 'noremap') Vim.noremap(lhs, rhs, context);
      else Vim.map(lhs, rhs, context);
    }
  }
  active = source;
}


