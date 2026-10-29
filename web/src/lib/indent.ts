// Tab and Shift+Tab in the message box: indent or outdent (two spaces), instead of moving focus out of the box.
// Pure, so it can be tested without a browser.

export interface Edit { text: string; start: number; end: number }

const UNIT = '  ';

export function indentEdit(text: string, start: number, end: number, outdent: boolean): Edit {
  // A caret, indenting: just put the spaces at the caret.
  if (!outdent && start === end) return { text: text.slice(0, start) + UNIT + text.slice(end), start: start + UNIT.length, end: end + UNIT.length };

  // Otherwise whole lines: from the start of the line the selection begins in, to the end of the line it ends in
  // (a selection that ends right after a line break does not take the next line with it).
  const first = text.lastIndexOf('\n', start - 1) + 1;
  const last = end > start && text[end - 1] === '\n' ? end - 1 : end;
  const stop = (i: number) => { const n = text.indexOf('\n', i); return n < 0 ? text.length : n; };
  const block = text.slice(first, stop(last));
  let firstDelta = 0, total = 0;
  const lines = block.split('\n').map((line, i) => {
    let next: string;
    if (outdent) {
      const gone = line.startsWith(UNIT) ? UNIT.length : line.startsWith(' ') || line.startsWith('\t') ? 1 : 0;
      next = line.slice(gone);
    } else next = UNIT + line;
    const d = next.length - line.length;
    if (i === 0) firstDelta = d;
    total += d;
    return next;
  });
  const out = text.slice(0, first) + lines.join('\n') + text.slice(first + block.length);
  const lineGone = (n: number) => Math.max(first, n); // the caret can't go before its line
  return { text: out, start: lineGone(start + firstDelta), end: lineGone(end + total) };
}
