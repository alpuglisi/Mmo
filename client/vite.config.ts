import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

// `npm run dev` proxies the API and the WebSocket to a local evo-server
// (default 127.0.0.1:8080; override with EVO_SERVER=http://host:port).
const server = process.env.EVO_SERVER ?? 'http://127.0.0.1:8080';

// Native push in the Android app needs its Firebase config: the app only
// registers with FCM when google-services.json was in the generated project
// at build time (`npm run cap:sync` builds first), or VITE_EVO_FCM=1 says so.
const fcm = process.env.VITE_EVO_FCM !== undefined
  ? process.env.VITE_EVO_FCM === '1'
  : existsSync(fileURLToPath(new URL('./android/app/google-services.json', import.meta.url)));

export default defineConfig({
  define: {
    __EVO_FCM__: JSON.stringify(fcm),
  },
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
    rollupOptions: {
      // The page, and the service worker (src/sw.ts) at a fixed name next
      // to index.html: its scope is the directory it is served from.
      input: {
        index: fileURLToPath(new URL('./index.html', import.meta.url)),
        sw: fileURLToPath(new URL('./src/sw.ts', import.meta.url)),
      },
      output: {
        entryFileNames: (chunk) => (chunk.name === 'sw' ? 'sw.js' : 'assets/[name]-[hash].js'),
      },
    },
    // PixiJS alone is ~500 kB minified; that's expected.
    chunkSizeWarningLimit: 1500,
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
