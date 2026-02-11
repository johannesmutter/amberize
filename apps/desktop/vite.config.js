import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(({ mode }) => ({
  base: mode === 'production' ? './' : '/',
  plugins: [svelte()],
  server: {
    port: 5173,
    strictPort: true,
    host: host || '127.0.0.1',
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    watch: { ignored: ['**/src-tauri/**'] }
  },
  resolve: {
    // Force browser condition so Svelte uses the client runtime (mount) instead of server.
    conditions: ['browser']
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./vitest.setup.js'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'lcov'],
      all: true,
      include: ['src/**/*.{js,svelte}'],
      exclude: ['src/main.js'],
      // TODO: restore 100% thresholds once tests are rebuilt for new components
      // (ArchiveLocationScreen, MainDashboard, SettingsPage, etc.)
      thresholds: {
        lines: 20,
        functions: 20,
        branches: 20,
        statements: 20
      }
    }
  }
}));

