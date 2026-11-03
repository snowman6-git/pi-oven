// What kind of file a name is, for the icon beside it. One drawing per kind (so they differ in shape, not only in
// colour), one tint per family. Pure, so it can be tested without a browser.

export type Glyph =
  | 'file' | 'braces' | 'tag' | 'hash' | 'lines' | 'gear' | 'terminal' | 'image' | 'database' | 'lock' | 'branch'
  | 'package' | 'binary' | 'archive' | 'media' | 'code' | 'font' | 'test';

export type Tint = 'ink' | 'blue' | 'violet' | 'teal' | 'green' | 'orange' | 'amber' | 'red';

export interface Kind { glyph: Glyph; tint: Tint; label: string }

const by = (glyph: Glyph, tint: Tint, label: string): Kind => ({ glyph, tint, label });

const EXT: Record<string, Kind> = {
  // scripts and programs
  ts: by('braces', 'blue', 'TypeScript'), tsx: by('braces', 'blue', 'TypeScript JSX'), mts: by('braces', 'blue', 'TypeScript'), cts: by('braces', 'blue', 'TypeScript'),
  js: by('braces', 'amber', 'JavaScript'), jsx: by('braces', 'amber', 'JavaScript JSX'), mjs: by('braces', 'amber', 'JavaScript'), cjs: by('braces', 'amber', 'JavaScript'),
  rs: by('code', 'orange', 'Rust'), py: by('code', 'blue', 'Python'), go: by('code', 'teal', 'Go'), java: by('code', 'red', 'Java'), kt: by('code', 'violet', 'Kotlin'),
  c: by('code', 'blue', 'C'), h: by('code', 'violet', 'C header'), cpp: by('code', 'blue', 'C++'), cc: by('code', 'blue', 'C++'), hpp: by('code', 'violet', 'C++ header'),
  cs: by('code', 'green', 'C#'), rb: by('code', 'red', 'Ruby'), php: by('code', 'violet', 'PHP'), lua: by('code', 'blue', 'Lua'), swift: by('code', 'orange', 'Swift'),
  sh: by('terminal', 'green', 'Shell script'), bash: by('terminal', 'green', 'Shell script'), zsh: by('terminal', 'green', 'Shell script'), fish: by('terminal', 'green', 'Shell script'), ps1: by('terminal', 'blue', 'PowerShell'), bat: by('terminal', 'green', 'Batch file'),
  // markup and style
  html: by('tag', 'orange', 'HTML'), htm: by('tag', 'orange', 'HTML'), xml: by('tag', 'orange', 'XML'), svelte: by('tag', 'orange', 'Svelte'), vue: by('tag', 'green', 'Vue'),
  css: by('hash', 'violet', 'CSS'), scss: by('hash', 'violet', 'SCSS'), less: by('hash', 'violet', 'Less'),
  // data and configuration
  json: by('braces', 'ink', 'JSON'), jsonc: by('braces', 'ink', 'JSON'), json5: by('braces', 'ink', 'JSON'),
  yml: by('gear', 'amber', 'YAML'), yaml: by('gear', 'amber', 'YAML'), toml: by('gear', 'amber', 'TOML'), ini: by('gear', 'amber', 'Config'), conf: by('gear', 'amber', 'Config'), env: by('gear', 'amber', 'Environment'), cfg: by('gear', 'amber', 'Config'),
  sql: by('database', 'teal', 'SQL'), db: by('database', 'teal', 'Database'), sqlite: by('database', 'teal', 'SQLite'), csv: by('database', 'green', 'CSV'), tsv: by('database', 'green', 'TSV'),
  // documents
  md: by('lines', 'ink', 'Markdown'), markdown: by('lines', 'ink', 'Markdown'), txt: by('lines', 'ink', 'Text'), rst: by('lines', 'ink', 'reStructuredText'), log: by('lines', 'ink', 'Log'), pdf: by('lines', 'red', 'PDF'),
  // pictures, sound, fonts
  png: by('image', 'teal', 'Image'), jpg: by('image', 'teal', 'Image'), jpeg: by('image', 'teal', 'Image'), gif: by('image', 'teal', 'Image'), webp: by('image', 'teal', 'Image'),
  svg: by('image', 'orange', 'SVG'), ico: by('image', 'teal', 'Icon'), bmp: by('image', 'teal', 'Image'), avif: by('image', 'teal', 'Image'),
  mp3: by('media', 'violet', 'Audio'), wav: by('media', 'violet', 'Audio'), flac: by('media', 'violet', 'Audio'), ogg: by('media', 'violet', 'Audio'),
  mp4: by('media', 'violet', 'Video'), mkv: by('media', 'violet', 'Video'), mov: by('media', 'violet', 'Video'), webm: by('media', 'violet', 'Video'),
  ttf: by('font', 'ink', 'Font'), otf: by('font', 'ink', 'Font'), woff: by('font', 'ink', 'Font'), woff2: by('font', 'ink', 'Font'),
  // packed and compiled
  zip: by('archive', 'amber', 'Archive'), tar: by('archive', 'amber', 'Archive'), gz: by('archive', 'amber', 'Archive'), tgz: by('archive', 'amber', 'Archive'), xz: by('archive', 'amber', 'Archive'), '7z': by('archive', 'amber', 'Archive'), rar: by('archive', 'amber', 'Archive'),
  bin: by('binary', 'ink', 'Binary'), exe: by('binary', 'ink', 'Executable'), dll: by('binary', 'ink', 'Library'), so: by('binary', 'ink', 'Library'), dylib: by('binary', 'ink', 'Library'),
  wasm: by('binary', 'violet', 'WebAssembly'), o: by('binary', 'ink', 'Object'), a: by('binary', 'ink', 'Library'), class: by('binary', 'red', 'Java class'), pyc: by('binary', 'ink', 'Compiled Python'),
  lock: by('lock', 'ink', 'Lock file'),
};

/** Whole names that mean something whatever their extension. */
const NAMES: Record<string, Kind> = {
  'package.json': by('package', 'green', 'npm package'),
  'cargo.toml': by('package', 'orange', 'Cargo package'),
  'pyproject.toml': by('package', 'blue', 'Python project'),
  'go.mod': by('package', 'teal', 'Go module'),
  'package-lock.json': by('lock', 'ink', 'Lock file'),
  'cargo.lock': by('lock', 'ink', 'Lock file'),
  'bun.lock': by('lock', 'ink', 'Lock file'),
  'yarn.lock': by('lock', 'ink', 'Lock file'),
  'pnpm-lock.yaml': by('lock', 'ink', 'Lock file'),
  '.gitignore': by('branch', 'ink', 'Git'),
  '.gitattributes': by('branch', 'ink', 'Git'),
  '.gitmodules': by('branch', 'ink', 'Git'),
  'dockerfile': by('package', 'blue', 'Docker'),
  'makefile': by('terminal', 'ink', 'Makefile'),
  'license': by('lines', 'ink', 'License'),
  'readme': by('lines', 'ink', 'Readme'),
  'agents.md': by('lines', 'orange', 'Agent instructions'),
  'claude.md': by('lines', 'orange', 'Agent instructions'),
};

export function kindOf(name: string): Kind {
  const lower = name.toLowerCase();
  if (NAMES[lower]) return NAMES[lower];
  if (lower.startsWith('.env')) return EXT.env;
  if (/\.(test|spec)\.[a-z0-9]+$/.test(lower)) return by('test', 'green', 'Test');
  const dot = lower.lastIndexOf('.');
  if (dot > 0 && EXT[lower.slice(dot + 1)]) return EXT[lower.slice(dot + 1)];
  if (dot === 0 && EXT[lower.slice(1)]) return EXT[lower.slice(1)]; // ".env", ".lock" and the like
  return by('file', 'ink', 'File');
}
