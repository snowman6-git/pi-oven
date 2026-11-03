#!/usr/bin/env node
// Checks the language files in src/locales against ko.json (the source):
//   - keys missing from a language (they fall back to English, then Korean) and keys ko.json does not have
//   - {placeholders} that differ from ko.json (a missing one loses information, an unknown one shows up literally)
//   - keys the code asks for that ko.json does not have
// usage: npm run i18n            (report; fails only on broken placeholders or unknown keys in code)
//        npm run i18n -- --strict (also fails on missing keys)
import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', 'src');
const dir = join(root, 'locales');
const strict = process.argv.includes('--strict');

const FORMS = new Set(['zero', 'one', 'two', 'few', 'many', 'other']);
const isLeaf = (v) => typeof v === 'string' || (v && typeof v === 'object' && 'other' in v && Object.keys(v).every((k) => FORMS.has(k)));
// an object with an "other" key and anything else is a group that the plural rule would swallow
function badGroups(o, p = '', out = []) {
  for (const [k, v] of Object.entries(o)) {
    if (!v || typeof v !== 'object') continue;
    if ('other' in v && !Object.keys(v).every((x) => FORMS.has(x))) out.push(p + k);
    badGroups(v, `${p}${k}.`, out);
  }
  return out;
}
function flat(o, p = '', out = new Map()) {
  for (const [k, v] of Object.entries(o)) {
    if (!p && k === '_meta') continue;
    if (isLeaf(v)) out.set(p + k, v);
    else flat(v, `${p}${k}.`, out);
  }
  return out;
}
const holes = (v) => new Set((typeof v === 'string' ? [v] : Object.values(v)).flatMap((s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1])));

const koJson = JSON.parse(readFileSync(join(dir, 'ko.json'), 'utf8'));
const source = flat(koJson);
let bad = 0;
for (const g of badGroups(koJson)) { console.log(`ko.json: "${g}" has a key named "other" next to other keys; rename it (it reads as a plural)`); bad++; }

for (const file of readdirSync(dir).filter((f) => f.endsWith('.json') && f !== 'ko.json').sort()) {
  const json = JSON.parse(readFileSync(join(dir, file), 'utf8'));
  const lang = flat(json);
  const missing = [...source.keys()].filter((k) => !lang.has(k));
  const extra = [...lang.keys()].filter((k) => !source.has(k));
  const holesOff = [...lang].filter(([k, v]) => source.has(k)).flatMap(([k, v]) => {
    const want = [...holes(source.get(k))].sort().join(',');
    const got = [...holes(v)].sort().join(',');
    return want === got ? [] : [`${k}: {${got}} but ko.json has {${want}}`];
  });
  console.log(`${file}  ${json._meta?.name ?? '(no _meta.name)'}  ${lang.size - extra.length}/${source.size} keys`);
  for (const k of missing) console.log(`  missing   ${k}`);
  for (const k of extra) console.log(`  unknown   ${k}`);
  for (const m of holesOff) console.log(`  placeholder ${m}`);
  if (!json._meta?.name) { console.log('  _meta.name is missing (the name shown in the language list)'); bad++; }
  bad += holesOff.length + (strict ? missing.length : 0);
}

// keys written out in the code
const used = new Set();
const walk = (d) => {
  for (const e of readdirSync(d, { withFileTypes: true })) {
    const p = join(d, e.name);
    if (e.isDirectory()) { if (e.name !== 'locales') walk(p); continue; }
    if (!/\.(ts|svelte)$/.test(e.name) || e.name === 'strings.ts') continue;
    const text = readFileSync(p, 'utf8');
    for (const m of text.matchAll(/\b(?:t|tr)\(\s*'([\w.]+)'/g)) used.add(m[1]);
    for (const m of text.matchAll(/\bS\.(\w+)/g)) used.add(`common.${m[1]}`);
  }
};
walk(root);
const unknown = [...used].filter((k) => !source.has(k));
for (const k of unknown) console.log(`code asks for ${k}, which ko.json does not have`);
bad += unknown.length;

process.exit(bad ? 1 : 0);
