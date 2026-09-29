import { defineConfig } from 'vite';

// Fixed port for `tauri dev` (devUrl in app/tauri.conf.json)
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    target: ['es2022', 'chrome111', 'safari16'],
    // Fonts stay separate files (loaded via 'self' under the CSP)
    assetsInlineLimit: 0,
  },
});
