import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const tauriDevHost = process.env.TAURI_DEV_HOST ?? '127.0.0.1';

export default defineConfig({
  plugins: [react()],
  server: {
    host: tauriDevHost,
    port: 1420,
    strictPort: true,
    hmr: {
      protocol: 'ws',
      host: tauriDevHost,
      port: 1421,
    },
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
  },
});
