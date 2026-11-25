// What the model is about to do to files, read from a tool call's arguments while they still stream in (and from the
// finished ones). pi writes a file only when the call runs, so all of this is "not on the disk yet". Pure.

export interface Draft {
  kind: 'write' | 'edit';
  /** As the model gave it (a heredoc's path has its `cd` in front). */
  path: string;
  /** A write's content so far. */
  text: string;
  edits: { old: string; neu: string }[];
  /** A heredoc that adds to the end of the file. */
  append: boolean;
  /** Written by the shell: what it ends up as is the shell's business (variables, what comes after). */
  shell: boolean;
  /** …and it fills in variables first, so the content is not yet what will be saved. */
  vars: boolean;
}

const MAX_DEPTH = 12;
const cut = Symbol('cut');

/** The value of a JSON text that may stop anywhere: what is there is kept, a half-written name or literal is not. */
export function partialJson(src: string): unknown {
  let i = 0;
  const ws = () => { while (i < src.length && ' \t\r\n'.includes(src[i])) i++; };
  const text = (): string => {
    const from = ++i;
    for (;;) {
      const q = src.indexOf('"', i);
      if (q < 0) { i = src.length; break; }
      let back = 0;
      while (q - 1 - back >= from && src[q - 1 - back] === '\\') back++;
      i = q + (back % 2 ? 1 : 0);
      if (back % 2 === 0) { i = q; break; }
    }
    const whole = src[i] === '"' && i < src.length;
    let raw = src.slice(from, i);
    if (whole) i++;
    else {
      // an escape that has not all come
      let back = 0;
      while (back < raw.length && raw[raw.length - 1 - back] === '\\') back++;
      if (back % 2) raw = raw.slice(0, -1);
      else {
        const u = raw.lastIndexOf('\\u');
        if (u >= 0 && u > raw.length - 6) {
          let b = 0;
          while (u - 1 - b >= 0 && raw[u - 1 - b] === '\\') b++;
          if (b % 2 === 0) raw = raw.slice(0, u);
        }
      }
    }
    try { return JSON.parse(`"${raw}"`) as string; } catch { /* a raw line break or a wrong escape */ }
    return raw.replace(/\\(u[0-9a-fA-F]{4}|.)/g, (_, e: string) =>
      e[0] === 'u' && e.length === 5 ? String.fromCharCode(parseInt(e.slice(1), 16)) : ({ n: '\n', t: '\t', r: '\r', b: '\b', f: '\f' } as Record<string, string>)[e] ?? e);
  };
  const value = (depth: number): unknown => {
    ws();
    const c = src[i];
    if (c === undefined || depth > MAX_DEPTH) { i = src.length; return cut; }
    if (c === '"') return text();
    if (c === '{') {
      const out: Record<string, unknown> = {};
      i++;
      for (;;) {
        ws();
        if (src[i] === ',') { i++; continue; }
        if (src[i] === '}') { i++; return out; }
        if (src[i] !== '"') { i = src.length; return out; }
        const key = text();
        ws();
        if (src[i] !== ':') { i = src.length; return out; }
        i++;
        const v = value(depth + 1);
        if (v === cut) return out;
        if (key !== '__proto__') out[key] = v;
      }
    }
    if (c === '[') {
      const out: unknown[] = [];
      i++;
      for (;;) {
        ws();
        if (src[i] === ',') { i++; continue; }
        if (src[i] === ']') { i++; return out; }
        if (i >= src.length) return out;
        const v = value(depth + 1);
        if (v === cut) return out;
        out.push(v);
      }
    }
    const from = i;
    while (i < src.length && !',]} \t\r\n'.includes(src[i])) i++;
    if (i === from) { i = src.length; return cut; }
    try { return JSON.parse(src.slice(from, i)); } catch { i = i < src.length ? i : src.length; return cut; }
  };
  const v = value(0);
  return v === cut ? undefined : v;
}

const str = (v: unknown) => (typeof v === 'string' ? v : '');

/** The files a call is about to change, as far as its arguments say. Empty for other tools. */
export function draftsOf(tool: string, args: unknown): Draft[] {
  if (!args || typeof args !== 'object') return [];
  const a = args as Record<string, unknown>;
  if (tool === 'bash') return heredocs(str(a.command));
  const path = str(a.path) || str(a.file_path);
  if (!path) return [];
  if (tool === 'write') return [{ kind: 'write', path, text: str(a.content), edits: [], append: false, shell: false, vars: false }];
  if (tool !== 'edit') return [];
  let list: unknown = a.edits;
  if (typeof list === 'string') list = partialJson(list); // some models send the list as text
  const edits = (Array.isArray(list) ? list : [])
    .filter((e): e is Record<string, unknown> => !!e && typeof e === 'object')
    .map((e) => ({ old: str(e.oldText) || str(e.old_string), neu: str(e.newText) || str(e.new_string) }));
  if (!edits.length && (str(a.oldText) || str(a.newText))) edits.push({ old: str(a.oldText), neu: str(a.newText) });
  return [{ kind: 'edit', path, text: '', edits, append: false, shell: false, vars: false }];
}

const MARK = /<<(-?)[ \t]{0,8}(?:'([^'\n]{1,60})'|"([^"\n]{1,60})"|\\?([A-Za-z_][\w.-]{0,60}))/;
const WORD = String.raw`(?:'([^'\n]{1,400})'|"([^"\n$\`]{1,400})"|([^\s<>|&;"'()$\`*?{}\\]{1,400}))`;
const INTO = new RegExp(String.raw`(?:^|[^\d&>])(>>?)[ \t]{0,8}${WORD}`);
const TEE = new RegExp(String.raw`\btee[ \t]{1,8}(-a[ \t]{1,8}|--append[ \t]{1,8})?${WORD}`);
const CD = new RegExp(String.raw`^[ \t]{0,40}cd[ \t]{1,8}${WORD}[ \t]{0,8}(?:$|&&|;)`);

/** The heredocs of a shell command that write a file: `cat > F <<'EOF'`, `cat <<EOF >> F`, `tee F <<EOF`. A program fed
    by one (`python - <<EOF`) writes what it likes: only the folder watch can tell. */
export function heredocs(command: string): Draft[] {
  if (!command.includes('<<')) return [];
  const out: Draft[] = [];
  const lines = command.split('\n');
  let cwd = '';
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n].length > 2000 ? lines[n].slice(0, 2000) : lines[n];
    const cd = CD.exec(line);
    if (cd) cwd = join(cwd, cd[1] ?? cd[2] ?? cd[3]);
    // (a line that has not ended may still get its file's name, or the rest of it)
    if (n === lines.length - 1) break;
    const plain = line.replaceAll('<<<', '   ');
    const m = MARK.exec(plain);
    if (!m) continue;
    const tag = m[2] ?? m[3] ?? m[4];
    const body: string[] = [];
    let closed = false;
    for (n++; n < lines.length; n++) {
      const l = m[1] ? lines[n].replace(/^\t{1,200}/, '') : lines[n];
      if (l === tag) { closed = true; break; }
      body.push(l);
    }
    const last = body.length - 1;
    // the tag itself may be on its way
    if (!closed && last >= 0 && tag.startsWith(body[last])) body.pop();
    const head = plain.slice(0, m.index) + ' ' + plain.slice(m.index + m[0].length);
    const tee = TEE.exec(head);
    const into = tee ? null : INTO.exec(head);
    const hit = tee ?? (into && /(?:^|[\s;&|(])cat(?=\s|$|>)/.test(head) ? into : null);
    if (!hit) continue;
    const path = hit[2] ?? hit[3] ?? hit[4];
    if (path.startsWith('/dev/') || path.startsWith('&')) continue;
    out.push({
      kind: 'write', path: join(cwd, path), text: body.join('\n') + (closed || (last >= 0 && body.length === last) ? '\n' : ''),
      edits: [], append: tee ? !!tee[1] : hit[1] === '>>', shell: true,
      vars: !!m[4] && !m[0].includes('\\') && body.some((l) => /[$`]/.test(l)),
    });
  }
  return out;
}

function join(dir: string, p: string): string {
  return !dir || p.startsWith('/') || p.startsWith('~') ? p : `${dir.replace(/\/{1,40}$/, '')}/${p}`;
}

/** A path as the file tree names it (from the session's folder), or null for one outside it. */
export function inFolder(path: string, cwd: string): string | null {
  let p = path.startsWith('@') ? path.slice(1) : path;
  if (p.startsWith('/')) {
    const base = cwd.endsWith('/') ? cwd : cwd + '/';
    if (!p.startsWith(base)) return null;
    p = p.slice(base.length);
  } else if (p.startsWith('~')) return null;
  const out: string[] = [];
  for (const part of p.split('/')) {
    if (part === '' || part === '.') continue;
    if (part === '..') { if (!out.length) return null; out.pop(); } else out.push(part);
  }
  return out.length ? out.join('/') : null;
}

export interface Line { k: 'add' | 'del' | 'ctx' | 'gap'; n?: number; x: string }

export function countLines(s: string): number {
  let n = 0;
  for (let i = s.indexOf('\n'); i >= 0; i = s.indexOf('\n', i + 1)) n++;
  return s === '' || s.endsWith('\n') ? n : n + 1;
}

/** The last `max` lines of a text as added lines, numbered. */
export function addedLines(text: string, max: number): { from: number; lines: Line[] } {
  // (only the lines shown are cut out: this runs several times a second on a text that may be long)
  const end = text.endsWith('\n') ? text.length - 1 : text.length;
  const total = countLines(text);
  let start = end;
  let n = 0;
  for (; n < max && start >= 0 && n < total; n++) start = start === 0 ? -1 : text.lastIndexOf('\n', start - 1);
  const from = total - n;
  return { from, lines: total ? text.slice(start + 1, end).split('\n').map((x, i) => ({ k: 'add', n: from + i + 1, x })) : [] };
}

/** One replacement: the lines both start with (and end with, once it is all there) are context. No line numbers: only
    the result knows where in the file it is. */
export function editLines(e: { old: string; neu: string }, whole: boolean): Line[] {
  const o = e.old === '' ? [] : e.old.split('\n');
  const n = e.neu === '' ? [] : e.neu.split('\n');
  let a = 0;
  // (while the new text is coming, its last line may go on)
  const sure = whole ? n.length : n.length - 1;
  while (a < o.length && a < sure && o[a] === n[a]) a++;
  let b = 0;
  if (whole) while (b < o.length - a && b < n.length - a && o[o.length - 1 - b] === n[n.length - 1 - b]) b++;
  return [
    ...o.slice(0, a).map((x): Line => ({ k: 'ctx', x })),
    ...o.slice(a, o.length - b).map((x): Line => ({ k: 'del', x })),
    ...n.slice(a, n.length - b).map((x): Line => ({ k: 'add', x })),
    ...(b ? o.slice(o.length - b).map((x): Line => ({ k: 'ctx', x })) : []),
  ];
}

export function draftCounts(d: Draft, whole: boolean): { added: number; removed: number } {
  if (d.kind === 'write') return { added: countLines(d.text), removed: 0 };
  let added = 0;
  let removed = 0;
  for (const e of d.edits) for (const l of editLines(e, whole)) { if (l.k === 'add') added++; else if (l.k === 'del') removed++; }
  return { added, removed };
}

/** pi's own diff (`+ 52 text`, `- 48 text`, `  47 text`, `     ...`) as lines. */
export function piLines(diff: string): Line[] {
  return diff.split('\n').map((raw): Line => {
    const m = /^([+\- ])[ ]{0,12}(\d{1,9}) ?(.*)$/.exec(raw);
    if (!m) return /^\s{0,20}\.\.\./.test(raw) ? { k: 'gap', x: '' } : { k: raw[0] === '+' ? 'add' : raw[0] === '-' ? 'del' : 'ctx', x: raw.slice(1) };
    return { k: m[1] === '+' ? 'add' : m[1] === '-' ? 'del' : 'ctx', n: Number(m[2]), x: m[3] };
  });
}

/** A unified diff (git's) as lines. */
export function gitLines(diff: string): Line[] {
  const out: Line[] = [];
  let a = 0;
  let b = 0;
  let inside = false;
  for (const raw of diff.replace(/\n$/, '').split('\n')) {
    const h = /^@@ -(\d{1,9})(?:,\d{1,9})? \+(\d{1,9})/.exec(raw);
    if (h) {
      if (inside) out.push({ k: 'gap', x: '' });
      a = Number(h[1]);
      b = Number(h[2]);
      inside = true;
    } else if (!inside || raw[0] === '\\') continue;
    else if (raw.startsWith('diff ')) inside = false;
    else if (raw[0] === '+') out.push({ k: 'add', n: b++, x: raw.slice(1) });
    else if (raw[0] === '-') out.push({ k: 'del', n: a++, x: raw.slice(1) });
    else { out.push({ k: 'ctx', n: b++, x: raw.slice(1) }); a++; }
  }
  return out;
}
