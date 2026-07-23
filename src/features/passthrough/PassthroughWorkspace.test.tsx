import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import type { PassthroughBatchResult } from '../../api/types';
import { PassthroughWorkspace } from './PassthroughWorkspace';

vi.mock('../../api/tauri', () => ({
  cancelPassthroughParse: vi.fn(),
  parsePassthroughMessages: vi.fn(),
  selectInputFile: vi.fn(),
}));

describe('PassthroughWorkspace', () => {
  beforeEach(() => vi.resetAllMocks());

  it('renders separate request and response inputs and allows request-only parsing', async () => {
    vi.mocked(api.parsePassthroughMessages).mockResolvedValue({ results: [], sourceWarning: null });
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    fireEvent.change(screen.getByLabelText('请求报文'), { target: { value: '0103016D001255E6' } });
    expect(screen.getByLabelText('回复报文')).toHaveValue('');
    expect(screen.queryByText('请求报文')).not.toBeInTheDocument();
    expect(screen.queryByText('回复报文')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    expect(api.parsePassthroughMessages).toHaveBeenCalledWith({
      requestHex: '0103016D001255E6',
      responseHex: null,
      source: null,
    });
  });

  it('keeps a response-only submission in the log without calling the backend', async () => {
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    fireEvent.change(screen.getByLabelText('回复报文'), { target: { value: '01032400' } });
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    expect(api.parsePassthroughMessages).not.toHaveBeenCalled();
    expect(await screen.findByRole('region', { name: '处理日志' })).toBeInTheDocument();
    expect(screen.getByText('解析回复报文前请先填写对应的请求报文。')).toBeInTheDocument();
  });

  it('rejects more response frames than request frames before invoking the backend', async () => {
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    fireEvent.change(screen.getByLabelText('请求报文'), { target: { value: '0103016D001255E6' } });
    fireEvent.change(screen.getByLabelText('回复报文'), { target: { value: '01030400010002AA&&01030400030004BB' } });
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    expect(api.parsePassthroughMessages).not.toHaveBeenCalled();
    expect(await screen.findByText('回复报文数量不能超过请求报文数量。')).toBeInTheDocument();
  });

  it('allows parsing without a source file', async () => {
    vi.mocked(api.parsePassthroughMessages).mockResolvedValue({ results: [], sourceWarning: null });
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    fireEvent.change(screen.getByLabelText('请求报文'), { target: { value: '010300000001840A' } });
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    expect(api.parsePassthroughMessages).toHaveBeenCalledWith({
      requestHex: '010300000001840A',
      responseHex: null,
      source: null,
    });
  });

  it('defaults to AWT template and clears its file when switching to AI manual recognition', async () => {
    vi.mocked(api.selectInputFile).mockResolvedValue({ path: '/tmp/template.csv', fileName: 'template.csv', sizeBytes: 10 });
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    expect(screen.getByRole('radio', { name: 'AWT模板' })).toBeChecked();
    const options = screen.getAllByRole('radio');
    expect(options.map((option) => option.getAttribute('value'))).toEqual(['awt_template', 'manual']);
    fireEvent.click(screen.getByRole('button', { name: '选择 CSV' }));
    expect(await screen.findByText('template.csv')).toBeInTheDocument();
    fireEvent.click(screen.getByText('AI识别说明书'));
    expect(screen.queryByText('template.csv')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /选择 PDF/ })).toBeInTheDocument();
  });

  it('uses the cross-platform monospace class for Hex input', () => {
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    expect(screen.getByLabelText('请求报文')).toHaveClass('passthrough-command-input');
    expect(screen.getByLabelText('回复报文')).toHaveClass('passthrough-command-input');
  });

  it('matches the AWT workspace heading hierarchy and typography', () => {
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    const eyebrow = screen.getByText('Passthrough message workspace');
    expect(eyebrow).toHaveAttribute('data-size', 'xs');
    expect(screen.getByRole('heading', { level: 2, name: '王大佬帮看下这报文什么意思？' })).toBeInTheDocument();
    expect(screen.getByText('解析中台报文里看不懂的十六进制透传报文')).toHaveAttribute('data-size', 'sm');
  });

  it('allows an in-flight manual extraction to be stopped', async () => {
    let rejectParse: ((reason: unknown) => void) | undefined;
    vi.mocked(api.selectInputFile).mockResolvedValue({ path: '/tmp/manual.pdf', fileName: 'manual.pdf', sizeBytes: 10 });
    vi.mocked(api.parsePassthroughMessages).mockImplementation(() => new Promise((_resolve, reject) => {
      rejectParse = reject;
    }));
    vi.mocked(api.cancelPassthroughParse).mockResolvedValue();
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    fireEvent.click(screen.getByText('AI识别说明书'));
    fireEvent.click(screen.getByRole('button', { name: /选择 PDF/ }));
    expect(await screen.findByText('manual.pdf')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('请求报文'), { target: { value: '010300000001840A' } });
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    fireEvent.click(await screen.findByRole('button', { name: '停止提取' }));
    expect(api.cancelPassthroughParse).toHaveBeenCalledTimes(1);
    rejectParse?.({ code: 'cancelled', message: '任务已取消', detail: null });
    expect(await screen.findByRole('button', { name: '开始解析' })).toBeEnabled();
    expect(screen.getByRole('region', { name: '处理日志' })).toBeInTheDocument();
    expect(screen.getByText('解析已取消。')).toBeInTheDocument();
  });

  it('uses the confirmed two-line command header and non-shrinking result layout', () => {
    const view = render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    const title = screen.getByText('透传命令');
    const protocols = screen.getByText(/支持协议.*Modbus RTU · DL\/T 645 · CJ\/T 188/);
    const header = title.closest('.passthrough-command-header');
    expect(header).toContainElement(protocols);
    expect(header).toContainElement(screen.getByRole('button', { name: '开始解析' }));
    expect(protocols).toHaveAttribute('data-size', 'xs');
    expect(protocols).toHaveStyle({ color: 'var(--mantine-color-dimmed)' });
    expect(view.container.querySelector('.workspace-footer')).not.toBeInTheDocument();
    expect(view.container.querySelector('.passthrough-input-card')).toHaveStyle({ flexShrink: '0' });
    expect(view.container.querySelector('.passthrough-results')).toHaveStyle({ flexShrink: '0' });
    expect(screen.getByRole('button', { name: '选择 CSV' })).toBeInTheDocument();
  });

  it('shows parsing logs, then replaces them with result cards after success', async () => {
    let resolveParse: ((result: PassthroughBatchResult) => void) | undefined;
    vi.mocked(api.parsePassthroughMessages).mockImplementation(() => new Promise((resolve) => {
      resolveParse = resolve;
    }));
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    expect(screen.queryByRole('region', { name: '处理日志' })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('请求报文'), { target: { value: '010300000001840A' } });
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    expect(await screen.findByRole('region', { name: '处理日志' })).toBeInTheDocument();
    expect(screen.getByText('开始解析，共 1 个输入片段。')).toBeInTheDocument();
    expect(screen.getByText('未选择辅助资料，执行确定性协议解析。')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '开始提取' })).not.toBeInTheDocument();
    resolveParse?.({
      sourceWarning: null,
      results: [{
        index: 0, role: 'request', rawSegment: '010300000001840A', cleanedHex: '010300000001840A', protocol: 'modbusRtu',
        summary: '从站 1 执行 Modbus read 操作。', fields: [], registers: [], explanations: [],
        checksum: { kind: 'modbusCrc16', received: '0A84', calculated: '0A84', valid: true }, warnings: [], error: null,
      }],
    });
    expect(await screen.findByText('请求报文 1')).toBeInTheDocument();
    expect(screen.queryByRole('region', { name: '处理日志' })).not.toBeInTheDocument();
  });

  it('keeps the log panel visible when parsing fails', async () => {
    vi.mocked(api.parsePassthroughMessages).mockRejectedValue({ code: 'invalid_passthrough_input', message: '透传报文输入无效。', detail: null });
    render(<MantineProvider><PassthroughWorkspace /></MantineProvider>);
    fireEvent.change(screen.getByLabelText('请求报文'), { target: { value: 'ZZ' } });
    fireEvent.click(screen.getByRole('button', { name: '开始解析' }));
    expect(await screen.findByRole('region', { name: '处理日志' })).toBeInTheDocument();
    expect(await screen.findByText('透传报文输入无效。')).toBeInTheDocument();
    expect(screen.queryByText('请求报文 1')).not.toBeInTheDocument();
  });
});
