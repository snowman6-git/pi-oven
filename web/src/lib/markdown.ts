import DOMPurify from 'dompurify';
import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import c from 'highlight.js/lib/languages/c';
import cpp from 'highlight.js/lib/languages/cpp';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import go from 'highlight.js/lib/languages/go';
import ini from 'highlight.js/lib/languages/ini';
import java from 'highlight.js/lib/languages/java';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import markdown from 'highlight.js/lib/languages/markdown';
import python from 'highlight.js/lib/languages/python';
import rust from 'highlight.js/lib/languages/rust';
import sql from 'highlight.js/lib/languages/sql';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';
import { Marked } from 'marked';
import { S } from './strings';

const langs: Record<string, any> = {
  bash, c, cpp, css, diff, go, ini, java, javascript, json, markdown, python, rust, sql, typescript, xml, yaml,
};
for (const [name, def] of Object.entries(langs)) hljs.registerLanguage(name, def);
const aliases: Record<string, string> = {
  sh: 'bash', shell: 'bash', zsh: 'bash', js: 'javascript', jsx: 'javascript', ts: 'typescript', tsx: 'typescript',
  py: 'python', rs: 'rust', html: 'xml', svelte: 'xml', vue: 'xml', toml: 'ini', yml: 'yaml', md: 'markdown', 'c++': 'cpp',
};

const esc = (s: string) => s.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);

export function highlight(code: string, lang?: string): string {
  const name = lang ? (aliases[lang.toLowerCase()] ?? lang.toLowerCase()) : '';
  if (name && hljs.getLanguage(name)) return hljs.highlight(code, { language: name, ignoreIllegals: true }).value;
  return esc(code);
}

const md = new Marked({
  gfm: true,
  renderer: {
    code({ text, lang }) {
      const label = (lang ?? '').split(/\s/)[0];
      return (
        `<div class="code"><div class="code-bar"><span>${esc(label)}</span>` +
        `<button type="button" data-copy>${S.copy}</button></div>` +
        `<pre><code class="hljs">${highlight(text, label)}</code></pre></div>`
      );
    },
  },
});

DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank');
    node.setAttribute('rel', 'noopener noreferrer');
  }
});

// Messages that are finished are drawn again whenever their session is shown or the window is resized into another
// layout; parsing, highlighting and sanitising the same text each time is wasted work. Streaming text changes every
// frame and is never worth keeping, so the caller says which it is.
const memo = new Map<string, string>();
const MEMO_ENTRIES = 300;
const MEMO_MAX_CHARS = 40_000;

function render(src: string): string {
  const html = md.parse(src, { async: false }) as string;
  return DOMPurify.sanitize(html, { ADD_TAGS: ['button'], ADD_ATTR: ['data-copy'] });
}

export function renderMarkdown(src: string, finished = false): string {
  if (!finished || src.length > MEMO_MAX_CHARS) return render(src);
  const hit = memo.get(src);
  if (hit !== undefined) {
    memo.delete(src); // most recently used goes last
    memo.set(src, hit);
    return hit;
  }
  const out = render(src);
  memo.set(src, out);
  if (memo.size > MEMO_ENTRIES) memo.delete(memo.keys().next().value as string);
  return out;
}
