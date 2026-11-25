import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import { defineConfig, type Plugin } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Every text file of the build also goes in as `.br` and `.gz`, compressed once here as far as it goes: the server has
// both inside and only picks (src/web.rs). Browsers take brotli over HTTPS (or localhost) only, gzip everywhere. Fonts
// are compressed already, and a file that fits in one packet gains nothing.
function precompress(): Plugin {
  let dir = 'dist';
  return {
    name: 'oven-precompress',
    apply: 'build',
    configResolved(c) { dir = c.build.outDir; },
    closeBundle() {
      for (const rel of readdirSync(dir, { recursive: true }) as string[]) {
        if (!/\.(js|css|html|svg|json)$/.test(rel)) continue;
        const file = join(dir, rel);
        if (statSync(file).size < 1200) continue;
        const raw = readFileSync(file);
        writeFileSync(`${file}.br`, brotliCompressSync(raw, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_LGWIN]: 22, [constants.BROTLI_PARAM_SIZE_HINT]: raw.length, [constants.BROTLI_PARAM_MODE]: constants.BROTLI_MODE_TEXT } }));
        writeFileSync(`${file}.gz`, gzipSync(raw, { level: 9 }));
      }
    },
  };
}

// `npm run dev` proxies the API (and its WebSockets) to the Rust server.
export default defineConfig({
  plugins: [svelte(), precompress()],
  server: {
    proxy: { '/api': { target: 'http://127.0.0.1:4141', ws: true } },
  },
  build: { outDir: 'dist', chunkSizeWarningLimit: 900 },
});
