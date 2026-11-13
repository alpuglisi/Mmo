import { defineConfig } from 'vitest/config';

// `npm run dev` proxies the API and the WebSocket to a local evo-server
// (default 127.0.0.1:8080; override with EVO_SERVER=http://host:port).
const server = process.env.EVO_SERVER ?? 'http://127.0.0.1:8080';

export default defineConfig({
  // Relative asset paths, so the bundle works from evo-server, a sub-path or
  // a Capacitor shell alike.
  base: './',
  server: {
    proxy: {
      '/api': server,
      '/ws': { target: server.replace(/^http/, 'ws'), ws: true },
    },
  },
  build: {
    outDir: 'dist',
    // PixiJS alone is ~500 kB minified; that's expected.
    chunkSizeWarningLimit: 1500,
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
