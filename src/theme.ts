import { createTheme, localStorageColorSchemeManager } from '@mantine/core';

export const hummingbirdColorSchemeManager = localStorageColorSchemeManager({
  key: 'hummingbird-color-scheme',
});

export const hummingbirdTheme = createTheme({
  primaryColor: 'blue',
  defaultRadius: 'md',
  fontFamily: 'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  headings: {
    fontFamily: 'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    fontWeight: '650',
  },
});
