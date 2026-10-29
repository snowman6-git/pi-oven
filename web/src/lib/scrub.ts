// Showcase mode, for text: take out what a screenshot or a shared screen should not show. Each kind of thing is
// switched on its own. It only changes what is drawn; nothing stored or sent is touched.

import { findSecrets, mask, type CompiledRule } from './points.ts';

export interface ScrubOpts {
  /** The project folders (full paths) and the home folder, as this machine knows them. */
  cwds: string[];
  home: string;
  paths: boolean;
  user: boolean;
  secrets: boolean;
  hosts: boolean;
  /** The person's own pin rules; what they find is hidden with the other secrets. */
  rules?: CompiledRule[];
}

/** Past this size only the cheap rules run: a build log can be megabytes. */
const BIG = 400_000;

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

export function scrub(text: string, o: ScrubOpts): string {
  if (!text) return text;
  let t = text;
  if (o.secrets) {
    // an address may carry a token in it
    t = t.replace(/([?&#](?:token|key|secret|password|auth|access_token|api_key)[A-Za-z_]*=)([^&#\s"'<>]{6,})/gi, (_, k, v) => k + mask(v));
    if (t.length < BIG) for (const s of findSecrets(t, o.rules)) t = t.split(s.value).join(mask(s.value));
  }
  if (o.hosts) {
    t = t.replace(/(\bhttps?:\/\/)([^\/\s:?#"'<>)]+)/gi, (m, scheme, host) => (/^(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])$/i.test(host) ? m : scheme + 'server'));
    t = t.replace(/\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b/g, (m, a, b, c, d) => {
      const n = [a, b, c, d].map(Number);
      if (n.some((x) => x > 255) || n[0] === 127 || n[0] === 0) return m;
      return '•.•.•.•';
    });
  }
  if (o.paths) {
    // the longest folder first, so a folder inside another is not half replaced
    for (const dir of [...new Set(o.cwds)].filter((d) => d.length > 1).sort((a, b) => b.length - a.length)) {
      t = t.replace(new RegExp(escape(dir.replace(/\/+$/, '')) + '(?![\\w.-])', 'g'), '<프로젝트>');
    }
  }
  if (o.user) {
    if (o.home.length > 1) t = t.replace(new RegExp(escape(o.home.replace(/\/+$/, '')) + '(?![\\w.-])', 'g'), '~');
    t = t.replace(/\/home\/[^\/\s:"'<>]+/g, '/home/user').replace(/\/Users\/[^\/\s:"'<>]+/g, '/Users/user').replace(/([A-Za-z]:\\Users\\)[^\\\s:"'<>]+/g, '$1user');
  }
  return t;
}
