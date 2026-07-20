// @vitest-environment node
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('Tauri development server configuration', () => {
  it('uses the Tauri dev URL port with strict binding and WebSocket HMR', () => {
    const source = readFileSync(new URL('../vite.config.ts', import.meta.url), 'utf8');

    expect(source).toContain("host: tauriDevHost");
    expect(source).toContain('port: 1420');
    expect(source).toContain('strictPort: true');
    expect(source).toContain("protocol: 'ws'");
    expect(source).toContain('port: 1421');
  });
});
