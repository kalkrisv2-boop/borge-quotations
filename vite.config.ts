import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// AUDIT NOTE: this file did not exist before this session — the project had
// no bundler at all (see PROJECT_BASELINE.md Session 9/10 history). Port
// 1420 matches tauri.conf.json's devUrl — confirmed working: Vite booted
// on this exact port and Tauri connected to it successfully in testing.

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Prevent Vite from obscuring Rust errors
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    watch: {
      // AUDIT NOTE (round 2): the function-matcher form didn't resolve the
      // EBUSY crash either (same failure recurred against a different
      // crate's build artifact). Switched to a regex array, which is the
      // fix documented across multiple Tauri-on-Windows GitHub issues for
      // this exact symptom — chokidar's own path normalization for the
      // function-matcher form is inconsistent across versions, but its
      // RegExp.test() path against the raw (non-normalized) path is
      // reliable. Matches both slash directions, and excludes target/
      // explicitly as a second, redundant layer.
      ignored: [/[\\/]src-tauri[\\/]/, /[\\/]target[\\/]/],
    },
  },
  // Env variables starting with the item(s) prefixed with VITE_ or TAURI_
  // are exposed to your frontend via import.meta.env
  envPrefix: ['VITE_', 'TAURI_'],
}));
