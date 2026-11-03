import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vitest/config'

// The launch smoke (`npm --prefix app run e2e`): Node, not a DOM, and long timeouts, because it
// starts the built desktop app through tauri-driver. It needs `npm --prefix app run build` and
// `cargo build -p parley-app` first, and tauri-driver with the platform's WebDriver on the PATH.
export default defineConfig({
  root: fileURLToPath(new URL('..', import.meta.url)),
  test: {
    environment: 'node',
    include: ['e2e/**/*.spec.ts'],
    testTimeout: 60_000,
    hookTimeout: 60_000,
    fileParallelism: false,
  },
})
