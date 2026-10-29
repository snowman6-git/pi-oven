import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// `npm run dev` proxies the API (and its WebSockets) to the Rust server.
export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: { '/api': { target: 'http://127.0.0.1:4141', ws: true } },
  },
  build: { outDir: 'dist', chunkSizeWarningLimit: 900 },
});
