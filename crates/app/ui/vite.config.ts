import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

// Tauri loads the dev server in development and `dist/` in release builds.
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // Oldest engine we ship on: macOS WKWebView (Windows WebView2 is newer Chromium).
    target: 'safari15',
    outDir: 'dist',
    emptyOutDir: true,
  },
});
