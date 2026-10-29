// Bringing a change made on disk (by pi, say) into a file that is open in the editor, without touching
// what the person has typed. Three texts are compared: the base (what the disk held when the file was last
// read or saved), mine (what is in the editor) and theirs (what the disk holds now). When the two sets of
// edits are far enough apart they simply add up; when they are not, nothing is merged and the person decides.

export interface Change { from: number; to: number; insert: string }

/** Lines with their line ends kept, so offsets and joins are exact. */
export function lines(text: string): string[] {
  const out: string[] = [];
  let i = 0;
  while (i < text.length) {
    const n = text.indexOf('\n', i);
    if (n < 0) { out.push(text.slice(i)); break; }
    out.push(text.slice(i, n + 1));
    i = n + 1;
  }
  return out;
}

export interface Hunk { aFrom: number; aTo: number; bFrom: number; bTo: number }

const MAX_EDIT = 1500;

/** Where two arrays of lines differ (Myers' O(ND) shortest edit script; a giant rewrite is one hunk). */
export function diffLines(a: string[], b: string[]): Hunk[] {
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length, endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) { endA--; endB--; }
  if (start === endA && start === endB) return [];
  const A = a.slice(start, endA), B = b.slice(start, endB);
  const n = A.length, m = B.length;
  const whole: Hunk[] = [{ aFrom: start, aTo: endA, bFrom: start, bTo: endB }];
  if (n === 0 || m === 0) return whole;

  const max = Math.min(n + m, MAX_EDIT);
  const off = max + 1;
  const v = new Int32Array(2 * max + 3);
  const trace: Int32Array[] = [];
  let found = -1;
  for (let d = 0; d <= max && found < 0; d++) {
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x = k === -d || (k !== d && v[off + k - 1] < v[off + k + 1]) ? v[off + k + 1] : v[off + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && A[x] === B[y]) { x++; y++; }
      v[off + k] = x;
      if (x >= n && y >= m) { found = d; break; }
    }
  }
  if (found < 0) return whole; // too different to be worth walking

  // walk back through the trace, collecting the matching diagonals
  const moves: [number, number, number, number][] = []; // [fromX, fromY, toX, toY] of each edit step
  let x = n, y = m;
  for (let d = found; d > 0; d--) {
    const vd = trace[d];
    const k = x - y;
    const down = k === -d || (k !== d && vd[off + k - 1] < vd[off + k + 1]);
    const pk = down ? k + 1 : k - 1;
    const px = vd[off + pk], py = px - pk;
    moves.push([px, py, down ? px : px + 1, down ? py + 1 : py]);
    x = px; y = py;
  }
  moves.reverse();
  const hunks: Hunk[] = [];
  for (const [px, py, nx, ny] of moves) {
    const last = hunks[hunks.length - 1];
    if (last && last.aTo === start + px && last.bTo === start + py) { last.aTo = start + nx; last.bTo = start + ny; }
    else hunks.push({ aFrom: start + px, aTo: start + nx, bFrom: start + py, bTo: start + ny });
  }
  return hunks;
}

const offsets = (ls: string[]) => { const o = [0]; for (const l of ls) o.push(o[o.length - 1] + l.length); return o; };

/** The smallest text edits that turn `a` into `b`: line hunks, each trimmed to the characters that really differ. */
export function diffToChanges(a: string, b: string): Change[] {
  if (a === b) return [];
  const al = lines(a), bl = lines(b), ao = offsets(al), bo = offsets(bl);
  return diffLines(al, bl).map((h) => {
    let from = ao[h.aFrom], to = ao[h.aTo];
    let insert = b.slice(bo[h.bFrom], bo[h.bTo]);
    let p = 0;
    const old = a.slice(from, to);
    while (p < old.length && p < insert.length && old[p] === insert[p]) p++;
    let q = 0;
    while (q < old.length - p && q < insert.length - p && old[old.length - 1 - q] === insert[insert.length - 1 - q]) q++;
    from += p; to -= q; insert = insert.slice(p, insert.length - q);
    return { from, to, insert };
  });
}

export function applyChanges(text: string, changes: Change[]): string {
  let out = '', at = 0;
  for (const c of [...changes].sort((x, y) => x.from - y.from)) { out += text.slice(at, c.from) + c.insert; at = c.to; }
  return out + text.slice(at);
}

export type Merge =
  | { ok: true; text: string; /** from what is in the editor to the merged text */ changes: Change[] }
  | { ok: false; reason: 'overlap' | 'large' | 'wide' };

export const MAX_MERGE_CHARS = 500_000;

/** Fewer than this share of the base's lines changing on disk counts as an edit; more is a rewrite. */
const WIDE = 0.5;

export function merge3(base: string, theirs: string, mine: string): Merge {
  if (theirs === base) return { ok: true, text: mine, changes: [] };
  if (mine === base) return { ok: true, text: theirs, changes: diffToChanges(mine, theirs) };
  if (mine === theirs) return { ok: true, text: mine, changes: [] };
  if (base.length > MAX_MERGE_CHARS || theirs.length > MAX_MERGE_CHARS || mine.length > MAX_MERGE_CHARS) return { ok: false, reason: 'large' };

  const bl = lines(base);
  const T = diffLines(bl, lines(theirs));
  const M = diffLines(bl, lines(mine));
  const tl = lines(theirs), ml = lines(mine);

  const changed = T.reduce((n, h) => n + Math.max(h.aTo - h.aFrom, h.bTo - h.bFrom), 0);
  if (changed > WIDE * Math.max(bl.length, 20)) return { ok: false, reason: 'wide' };

  const same = (t: Hunk, m: Hunk) => t.aFrom === m.aFrom && t.aTo === m.aTo && tl.slice(t.bFrom, t.bTo).join('') === ml.slice(m.bFrom, m.bTo).join('');
  // closed intervals: edits that touch, or share a line, are in conflict; one untouched line between is enough
  const clash = (t: Hunk, m: Hunk) => t.aFrom <= m.aTo && m.aFrom <= t.aTo;
  const dropT = new Set<Hunk>();
  for (const t of T) for (const m of M) {
    if (same(t, m)) dropT.add(t); // both made exactly this edit
    else if (clash(t, m)) return { ok: false, reason: 'overlap' };
  }

  const all = [...M.map((h) => ({ h, src: ml })), ...T.filter((h) => !dropT.has(h)).map((h) => ({ h, src: tl }))].sort((x, y) => x.h.aFrom - y.h.aFrom || x.h.aTo - y.h.aTo);
  const out: string[] = [];
  let at = 0;
  for (const { h, src } of all) {
    out.push(...bl.slice(at, h.aFrom), ...src.slice(h.bFrom, h.bTo));
    at = h.aTo;
  }
  out.push(...bl.slice(at));
  const text = out.join('');
  return { ok: true, text, changes: diffToChanges(mine, text) };
}
