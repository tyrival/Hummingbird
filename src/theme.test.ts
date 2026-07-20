import { describe, expect, it } from 'vitest';
import { hummingbirdColorSchemeManager, hummingbirdTheme } from './theme';

describe('Hummingbird theme', () => {
  it('uses blue-gray desktop tokens and defaults to the system color scheme', () => {
    localStorage.removeItem('hummingbird-color-scheme');

    expect(hummingbirdTheme.primaryColor).toBe('blue');
    expect(hummingbirdColorSchemeManager.get('auto')).toBe('auto');
  });
});
