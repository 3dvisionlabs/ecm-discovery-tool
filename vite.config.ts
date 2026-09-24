import { defineConfig } from 'vite';

// Fixed port for `tauri dev` (devUrl in src-tauri/tauri.conf.json)
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**', '**/target/**'] },
  },
  build: {
    target: ['es2022', 'chrome111', 'safari16'],
    // Fonts stay separate files (loaded via 'self' under the CSP)
    assetsInlineLimit: 0,
  },
});
