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
  c: by('code', 'blue', 'C'), h: by('code', 'violet', 'C 헤더'), cpp: by('code', 'blue', 'C++'), cc: by('code', 'blue', 'C++'), hpp: by('code', 'violet', 'C++ 헤더'),
  cs: by('code', 'green', 'C#'), rb: by('code', 'red', 'Ruby'), php: by('code', 'violet', 'PHP'), lua: by('code', 'blue', 'Lua'), swift: by('code', 'orange', 'Swift'),
  sh: by('terminal', 'green', '셸 스크립트'), bash: by('terminal', 'green', '셸 스크립트'), zsh: by('terminal', 'green', '셸 스크립트'), fish: by('terminal', 'green', '셸 스크립트'), ps1: by('terminal', 'blue', 'PowerShell'), bat: by('terminal', 'green', '배치 파일'),
  // markup and style
  html: by('tag', 'orange', 'HTML'), htm: by('tag', 'orange', 'HTML'), xml: by('tag', 'orange', 'XML'), svelte: by('tag', 'orange', 'Svelte'), vue: by('tag', 'green', 'Vue'),
  css: by('hash', 'violet', 'CSS'), scss: by('hash', 'violet', 'SCSS'), less: by('hash', 'violet', 'Less'),
  // data and configuration
  json: by('braces', 'ink', 'JSON'), jsonc: by('braces', 'ink', 'JSON'), json5: by('braces', 'ink', 'JSON'),
  yml: by('gear', 'amber', 'YAML'), yaml: by('gear', 'amber', 'YAML'), toml: by('gear', 'amber', 'TOML'), ini: by('gear', 'amber', '설정'), conf: by('gear', 'amber', '설정'), env: by('gear', 'amber', '환경 변수'), cfg: by('gear', 'amber', '설정'),
  sql: by('database', 'teal', 'SQL'), db: by('database', 'teal', '데이터베이스'), sqlite: by('database', 'teal', 'SQLite'), csv: by('database', 'green', 'CSV'), tsv: by('database', 'green', 'TSV'),
  // documents
  md: by('lines', 'ink', 'Markdown'), markdown: by('lines', 'ink', 'Markdown'), txt: by('lines', 'ink', '텍스트'), rst: by('lines', 'ink', 'reStructuredText'), log: by('lines', 'ink', '로그'), pdf: by('lines', 'red', 'PDF'),
  // pictures, sound, fonts
  png: by('image', 'teal', '이미지'), jpg: by('image', 'teal', '이미지'), jpeg: by('image', 'teal', '이미지'), gif: by('image', 'teal', '이미지'), webp: by('image', 'teal', '이미지'),
  svg: by('image', 'orange', 'SVG'), ico: by('image', 'teal', '아이콘'), bmp: by('image', 'teal', '이미지'), avif: by('image', 'teal', '이미지'),
  mp3: by('media', 'violet', '오디오'), wav: by('media', 'violet', '오디오'), flac: by('media', 'violet', '오디오'), ogg: by('media', 'violet', '오디오'),
  mp4: by('media', 'violet', '비디오'), mkv: by('media', 'violet', '비디오'), mov: by('media', 'violet', '비디오'), webm: by('media', 'violet', '비디오'),
  ttf: by('font', 'ink', '글꼴'), otf: by('font', 'ink', '글꼴'), woff: by('font', 'ink', '글꼴'), woff2: by('font', 'ink', '글꼴'),
  // packed and compiled
  zip: by('archive', 'amber', '압축 파일'), tar: by('archive', 'amber', '압축 파일'), gz: by('archive', 'amber', '압축 파일'), tgz: by('archive', 'amber', '압축 파일'), xz: by('archive', 'amber', '압축 파일'), '7z': by('archive', 'amber', '압축 파일'), rar: by('archive', 'amber', '압축 파일'),
  bin: by('binary', 'ink', '바이너리'), exe: by('binary', 'ink', '실행 파일'), dll: by('binary', 'ink', '라이브러리'), so: by('binary', 'ink', '라이브러리'), dylib: by('binary', 'ink', '라이브러리'),
  wasm: by('binary', 'violet', 'WebAssembly'), o: by('binary', 'ink', '오브젝트'), a: by('binary', 'ink', '라이브러리'), class: by('binary', 'red', 'Java 클래스'), pyc: by('binary', 'ink', '컴파일된 Python'),
  lock: by('lock', 'ink', '잠금 파일'),
};

/** Whole names that mean something whatever their extension. */
const NAMES: Record<string, Kind> = {
  'package.json': by('package', 'green', 'npm 패키지'),
  'cargo.toml': by('package', 'orange', 'Cargo 패키지'),
  'pyproject.toml': by('package', 'blue', 'Python 프로젝트'),
  'go.mod': by('package', 'teal', 'Go 모듈'),
  'package-lock.json': by('lock', 'ink', '잠금 파일'),
  'cargo.lock': by('lock', 'ink', '잠금 파일'),
  'bun.lock': by('lock', 'ink', '잠금 파일'),
  'yarn.lock': by('lock', 'ink', '잠금 파일'),
  'pnpm-lock.yaml': by('lock', 'ink', '잠금 파일'),
  '.gitignore': by('branch', 'ink', 'Git'),
  '.gitattributes': by('branch', 'ink', 'Git'),
  '.gitmodules': by('branch', 'ink', 'Git'),
  'dockerfile': by('package', 'blue', 'Docker'),
  'makefile': by('terminal', 'ink', 'Makefile'),
  'license': by('lines', 'ink', '라이선스'),
  'readme': by('lines', 'ink', '안내'),
  'agents.md': by('lines', 'orange', '에이전트 지침'),
  'claude.md': by('lines', 'orange', '에이전트 지침'),
};

export function kindOf(name: string): Kind {
  const lower = name.toLowerCase();
  if (NAMES[lower]) return NAMES[lower];
  if (lower.startsWith('.env')) return EXT.env;
  if (/\.(test|spec)\.[a-z0-9]+$/.test(lower)) return by('test', 'green', '테스트');
  const dot = lower.lastIndexOf('.');
  if (dot > 0 && EXT[lower.slice(dot + 1)]) return EXT[lower.slice(dot + 1)];
  if (dot === 0 && EXT[lower.slice(1)]) return EXT[lower.slice(1)]; // ".env", ".lock" and the like
  return by('file', 'ink', '파일');
}
