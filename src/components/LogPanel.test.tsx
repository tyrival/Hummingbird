import { MantineProvider } from '@mantine/core';
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { LogPanel } from './LogPanel';

const idleTask = {
  active: false,
  canStart: false,
  stage: null,
  completedChunks: 0,
  totalChunks: 0,
  progressValue: 0,
  terminal: null,
};

describe('LogPanel', () => {
  it('renders only the newest 500 entries', { timeout: 15000 }, () => {
    const entries = Array.from({ length: 501 }, (_, index) => ({
      id: index,
      timestamp: '12:34:56',
      level: 'info' as const,
      message: `日志-${index}`,
    }));

    render(
      <MantineProvider>
        <div style={{ display: 'flex', flexDirection: 'column', height: 400 }}>
          <LogPanel entries={entries} onStartStop={vi.fn()} task={idleTask} />
        </div>
      </MantineProvider>,
    );

    expect(screen.queryByText('日志-0')).not.toBeInTheDocument();
    expect(screen.getByText('日志-500')).toBeInTheDocument();
    expect(screen.getAllByRole('listitem')).toHaveLength(500);
  });
});
