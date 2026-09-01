// vitest.config.ts
// Phase 3.1 — Test configuration for Vitest framework
// Integrates with existing Vite setup and configures test environment

import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    environment: 'node',
    include: ['src/**/*.test.ts', 'src-shared/**/*.test.ts'],
    exclude: ['node_modules', 'dist', 'target'],
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json', 'html'],
      include: ['src/**/*.ts', 'src-shared/**/*.ts'],
      exclude: ['**/*.test.ts', '**/index.ts', 'node_modules'],
    },
  },
});
