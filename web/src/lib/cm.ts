// The text editor itself (CodeMirror 6). Loaded only when a file is first opened, so the conversation
// view does not carry it. Colours come from the app's own tokens, so it follows the theme (dark, light).

import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { HighlightStyle, bracketMatching, indentOnInput, indentUnit, StreamLanguage, syntaxHighlighting, type LanguageSupport } from '@codemirror/language';
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search';
import { Compartment, EditorState, StateEffect, StateField, Transaction, type Extension, type Text } from '@codemirror/state';
import { Decoration, drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers, placeholder, type ViewUpdate } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';

export { Text } from '@codemirror/state';
export type { EditorState };
export { EditorView };
export const wrapping = new Compartment();
export const language = new Compartment();

// Lines that just changed on disk glow for a moment, so a change made under the person's eyes is seen.
const addFlash = StateEffect.define<number[]>();
const clearFlash = StateEffect.define<null>();
const flashField = StateField.define({
  create: () => Decoration.none,
  update(set, tr) {
    set = set.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(addFlash)) set = set.update({ add: e.value.map((pos) => Decoration.line({ class: 'cm-remote-line' }).range(pos)), sort: true });
      else if (e.is(clearFlash)) set = Decoration.none;
    }
    return set;
  },
  provide: (f) => EditorView.decorations.from(f),
});

export interface Edit { from: number; to: number; insert: string }
const remote = [Transaction.addToHistory.of(false), Transaction.remote.of(true)];

/** A change from outside, into the editor in view: not in the undo history, selection and scroll kept, changed lines flashed. */
export function applyRemote(view: EditorView, changes: Edit[]) {
  view.dispatch({ changes, annotations: remote });
  let delta = 0;
  const lineStarts = new Set<number>();
  for (const c of [...changes].sort((a, b) => a.from - b.from)) {
    const from = c.from + delta, to = from + c.insert.length;
    delta += c.insert.length - (c.to - c.from);
    const doc = view.state.doc;
    for (let ln = doc.lineAt(Math.min(from, doc.length)).number; ln <= doc.lineAt(Math.min(to, doc.length)).number; ln++) lineStarts.add(doc.line(ln).from);
  }
  view.dispatch({ effects: addFlash.of([...lineStarts]), annotations: remote });
  setTimeout(() => { if (view.dom.isConnected) view.dispatch({ effects: clearFlash.of(null), annotations: remote }); }, 1700);
}

/** The same for a file whose editor is not in view. */
export const applyRemoteToState = (state: EditorState, changes: Edit[]) => state.update({ changes, annotations: remote }).state;

const theme = EditorView.theme({
  '&': { height: '100%', color: 'var(--ink)', backgroundColor: 'var(--paper)', fontSize: '13px' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.6', overflow: 'auto' },
  '.cm-content': { caretColor: 'var(--accent)', padding: '10px 0 40vh' },
  '.cm-line': { padding: '0 14px 0 8px' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)', borderLeftWidth: '2px' },
  '.cm-gutters': { backgroundColor: 'var(--paper)', color: 'var(--ink-3)', border: 'none', paddingLeft: '4px' },
  '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--rule) 35%, transparent)' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent', color: 'var(--ink-2)' },
  '&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': { backgroundColor: 'color-mix(in srgb, var(--accent) 28%, transparent)' },
  '.cm-matchingBracket': { backgroundColor: 'color-mix(in srgb, var(--accent) 22%, transparent)', outline: 'none' },
  '.cm-selectionMatch': { backgroundColor: 'color-mix(in srgb, var(--heat) 22%, transparent)' },
  '.cm-searchMatch': { backgroundColor: 'color-mix(in srgb, var(--heat) 30%, transparent)', outline: '1px solid color-mix(in srgb, var(--heat) 60%, transparent)' },
  '.cm-searchMatch.cm-searchMatch-selected': { backgroundColor: 'color-mix(in srgb, var(--accent) 40%, transparent)' },
  '.cm-panels': { backgroundColor: 'var(--paper-sunk)', color: 'var(--ink)', borderColor: 'var(--rule)' },
  '.cm-panels-bottom': { borderTop: '1px solid var(--rule)' },
  '.cm-panels-top': { borderBottom: '1px solid var(--rule)' },
  '.cm-search': { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '4px 6px', padding: '8px 10px', fontFamily: 'var(--font-ui)', fontSize: '13px' },
  '.cm-search br': { display: 'none' },
  '.cm-search input, .cm-search button': { fontFamily: 'inherit', fontSize: '12.5px', margin: 0 },
  '.cm-search input[type=text]': { height: '28px', padding: '0 8px', borderRadius: '6px', border: '1px solid var(--rule)', backgroundColor: 'var(--raised)', color: 'var(--ink)' },
  '.cm-search input:focus': { outline: '2px solid color-mix(in srgb, var(--accent) 55%, transparent)', outlineOffset: '-1px' },
  '.cm-search button': { height: '28px', padding: '0 10px', borderRadius: '6px', border: '1px solid var(--rule)', backgroundColor: 'var(--raised)', color: 'var(--ink)', backgroundImage: 'none', cursor: 'pointer' },
  '.cm-search label': { color: 'var(--ink-2)', display: 'inline-flex', alignItems: 'center', gap: '4px' },
  '.cm-search [name=close]': { position: 'static', marginLeft: 'auto', border: 'none', backgroundColor: 'transparent', color: 'var(--ink-3)', fontSize: '18px', padding: '0 6px' },
  '.cm-placeholder': { color: 'var(--ink-3)' },
});

// the same palette the code blocks in the conversation use
const colours = HighlightStyle.define([
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: 'var(--ink-3)', fontStyle: 'italic' },
  { tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.definitionKeyword, t.moduleKeyword, t.modifier, t.self, t.null], color: 'var(--accent)' },
  { tag: [t.string, t.special(t.string), t.regexp, t.attributeValue, t.inserted], color: 'var(--ok)' },
  { tag: [t.number, t.bool, t.atom, t.constant(t.name), t.unit], color: 'var(--heat)' },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.definition(t.function(t.variableName)), t.className, t.typeName, t.tagName], color: 'var(--ink)', fontWeight: '650' },
  { tag: [t.propertyName, t.attributeName, t.labelName], color: 'var(--ink)' },
  { tag: [t.meta, t.processingInstruction, t.annotation, t.namespace], color: 'var(--ink-2)' },
  { tag: [t.operator, t.punctuation, t.separator, t.bracket], color: 'var(--ink-2)' },
  { tag: [t.heading, t.strong], fontWeight: '700' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: [t.link, t.url], color: 'var(--accent)', textDecoration: 'underline' },
  { tag: t.deleted, color: 'var(--bad)' },
  { tag: t.invalid, color: 'var(--bad)' },
]);

/** The language for a file name, loaded on demand (each is its own chunk). */
export async function languageFor(path: string): Promise<Extension> {
  const name = path.slice(path.lastIndexOf('/') + 1).toLowerCase();
  const ext = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1) : '';
  const legacy = async (load: () => Promise<any>, key: string): Promise<Extension> => StreamLanguage.define((await load())[key]);
  switch (ext) {
    case 'js': case 'mjs': case 'cjs': case 'jsx':
      return (await import('@codemirror/lang-javascript')).javascript({ jsx: true });
    case 'ts': case 'mts': case 'cts': case 'tsx':
      return (await import('@codemirror/lang-javascript')).javascript({ jsx: ext === 'tsx', typescript: true });
    case 'json': case 'jsonc': case 'webmanifest':
      return (await import('@codemirror/lang-json')).json();
    case 'html': case 'htm': case 'svelte': case 'vue': case 'xml': case 'svg':
      return (await import('@codemirror/lang-html')).html();
    case 'css': case 'scss': case 'less':
      return (await import('@codemirror/lang-css')).css();
    case 'md': case 'markdown':
      return (await import('@codemirror/lang-markdown')).markdown();
    case 'py': case 'pyw':
      return (await import('@codemirror/lang-python')).python();
    case 'rs':
      return (await import('@codemirror/lang-rust')).rust();
    case 'yml': case 'yaml':
      return (await import('@codemirror/lang-yaml')).yaml();
    case 'sh': case 'bash': case 'zsh':
      return legacy(() => import('@codemirror/legacy-modes/mode/shell'), 'shell');
    case 'toml':
      return legacy(() => import('@codemirror/legacy-modes/mode/toml'), 'toml');
    case 'go':
      return legacy(() => import('@codemirror/legacy-modes/mode/go'), 'go');
    case 'c': case 'h': case 'cpp': case 'cc': case 'hpp': case 'java': case 'cs':
      return legacy(() => import('@codemirror/legacy-modes/mode/clike'), ext === 'c' || ext === 'h' ? 'c' : ext === 'java' ? 'java' : ext === 'cs' ? 'csharp' : 'cpp');
    case 'sql':
      return legacy(() => import('@codemirror/legacy-modes/mode/sql'), 'standardSQL');
    case 'lua':
      return legacy(() => import('@codemirror/legacy-modes/mode/lua'), 'lua');
    case 'rb':
      return legacy(() => import('@codemirror/legacy-modes/mode/ruby'), 'ruby');
    case 'dockerfile':
      return legacy(() => import('@codemirror/legacy-modes/mode/dockerfile'), 'dockerFile');
  }
  if (name === 'dockerfile') return legacy(() => import('@codemirror/legacy-modes/mode/dockerfile'), 'dockerFile');
  if (name === 'makefile' || name.startsWith('.env') || name === '.gitignore') return [];
  return [];
}

/** Two spaces unless the file itself plainly uses tabs or four. */
export function indentFor(text: string): string {
  const lines = text.slice(0, 20000).split('\n');
  let tabs = 0, two = 0, four = 0;
  for (const l of lines) {
    if (l.startsWith('\t')) tabs++;
    else {
      const m = l.match(/^( +)\S/);
      if (m) { if (m[1].length % 4 === 0) four++; else if (m[1].length % 2 === 0) two++; }
    }
  }
  if (tabs > two + four) return '\t';
  return four > two * 1.5 && four > 3 ? '    ' : '  ';
}

export interface Hooks {
  onChange(update: ViewUpdate): void;
  onSave(): void;
}

export function createState(doc: string, indent: string, hooks: Hooks, wrap: boolean): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightActiveLine(),
      history(),
      drawSelection(),
      indentOnInput(),
      bracketMatching(),
      highlightSelectionMatches(),
      search({ top: false }),
      indentUnit.of(indent),
      EditorState.tabSize.of(indent === '\t' ? 4 : indent.length),
      syntaxHighlighting(colours),
      theme,
      wrapping.of(wrap ? EditorView.lineWrapping : []),
      language.of([]),
      flashField,
      placeholder('빈 파일입니다.'),
      keymap.of([
        { key: 'Mod-s', preventDefault: true, run: () => (hooks.onSave(), true) },
        indentWithTab,
        ...searchKeymap,
        ...historyKeymap,
        ...defaultKeymap,
      ]),
      EditorView.updateListener.of((u) => hooks.onChange(u)),
      EditorView.contentAttributes.of({ 'aria-label': '파일 편집기', spellcheck: 'false', autocapitalize: 'off', autocorrect: 'off' }),
    ],
  });
}

export const makeView = (parent: HTMLElement, state: EditorState) => new EditorView({ parent, state });
export type { LanguageSupport };

