/** An incremental server-sent-events reader: feed it text as it arrives, get back the `data` of each finished event.
    Lines end with \n, \r\n or \r; an empty line ends an event; `:` comments and other fields are skipped. */
export function sseParser(): (chunk: string) => string[] {
  let buf = '';
  let data: string[] = [];
  return (chunk) => {
    buf += chunk;
    const out: string[] = [];
    for (;;) {
      const m = /\r\n|\r|\n/.exec(buf);
      if (!m) break;
      if (m[0] === '\r' && m.index + 1 === buf.length) break; // may be the first half of \r\n
      const line = buf.slice(0, m.index);
      buf = buf.slice(m.index + m[0].length);
      if (line === '') {
        if (data.length) out.push(data.join('\n'));
        data = [];
      } else if (line.startsWith('data:')) {
        data.push(line.slice(line.startsWith('data: ') ? 6 : 5));
      }
    }
    return out;
  };
}
