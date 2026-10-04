import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';
import solid from 'vite-plugin-solid';

// Tauri loads the dev server in development and `dist/` in release builds.
export default defineConfig({
  plugins: [solid(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // The title bar shows the app icon from crates/app/icons.
    fs: { allow: ['.', '../icons'] },
  },
  build: {
    // Oldest engine we ship on: macOS WKWebView (Windows WebView2 is newer Chromium).
    target: 'safari15',
    outDir: 'dist',
    emptyOutDir: true,
  },
});
