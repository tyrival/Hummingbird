import { MantineProvider } from '@mantine/core';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LogPanel } from './LogPanel';

describe('LogPanel', () => {
  it('renders only the newest 500 entries', () => {
    const entries = Array.from({ length: 501 }, (_, index) => ({
      id: index,
      timestamp: '12:34:56',
      level: 'info' as const,
      message: `日志-${index}`,
    }));

    render(
      <MantineProvider>
        <LogPanel entries={entries} />
      </MantineProvider>,
    );

    expect(screen.queryByText('日志-0')).not.toBeInTheDocument();
    expect(screen.getByText('日志-500')).toBeInTheDocument();
    expect(screen.getAllByRole('listitem')).toHaveLength(500);
  });
});
