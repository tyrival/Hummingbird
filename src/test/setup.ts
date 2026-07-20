import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { notifications } from '@mantine/notifications';
import { afterEach } from 'vitest';

if (typeof window !== 'undefined') {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => false,
    }),
  });
}

class ResizeObserverMock {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

globalThis.ResizeObserver = ResizeObserverMock;

afterEach(() => {
  if (typeof document !== 'undefined') {
    notifications.clean();
    cleanup();
  }
});
