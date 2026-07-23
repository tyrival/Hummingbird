import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { RegisterMergeModal } from './RegisterMergeModal';

describe('RegisterMergeModal', () => {
  it('parses edited hex only after the parse button is clicked and preserves results on errors', () => {
    render(<MantineProvider><RegisterMergeModal initialHex="08 9D 08 96" onClose={vi.fn()} opened registerCount={2} /></MantineProvider>);
    expect(screen.getByText('40200')).toBeInTheDocument();
    const input = screen.getByLabelText('合并 HEX');
    fireEvent.change(input, { target: { value: '00 01' } });
    expect(screen.getByText('40200')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '解析' }));
    expect(screen.getAllByText('256').length).toBeGreaterThan(0);
    fireEvent.change(input, { target: { value: '0Z' } });
    fireEvent.click(screen.getByRole('button', { name: '解析' }));
    expect(screen.getByText('请输入有效的十六进制字符。')).toBeInTheDocument();
    expect(screen.getAllByText('256').length).toBeGreaterThan(0);
  });

  it('resets edited input when the modal is closed and reopened', () => {
    const view = render(<MantineProvider><RegisterMergeModal initialHex="08 9D" onClose={vi.fn()} opened registerCount={1} /></MantineProvider>);
    fireEvent.change(screen.getByLabelText('合并 HEX'), { target: { value: '00 01' } });
    view.rerender(<MantineProvider><RegisterMergeModal initialHex="08 9D" onClose={vi.fn()} opened={false} registerCount={1} /></MantineProvider>);
    view.rerender(<MantineProvider><RegisterMergeModal initialHex="08 9D" onClose={vi.fn()} opened registerCount={1} /></MantineProvider>);
    expect(screen.getByLabelText('合并 HEX')).toHaveValue('08 9D');
  });
});
