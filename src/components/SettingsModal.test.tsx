import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as tauriApi from '../api/tauri';
import type { SettingsDto } from '../api/types';
import { SettingsModal } from './SettingsModal';

vi.mock('../api/tauri', () => ({
  getAppVersion: vi.fn(),
  saveSettings: vi.fn(),
  selectOutputDirectory: vi.fn(),
}));

const api = vi.mocked(tauriApi);
const settings: SettingsDto = {
  schemaVersion: 1,
  migrationVersion: 1,
  baseUrl: 'https://api.example.test/v1',
  apiKey: 'super-secret',
  model: 'deepseek-chat',
  timeoutSeconds: 600,
  maxTokens: 16384,
  outputDirectory: 'output',
  chunkMaxChars: 30000,
  contextChars: 3000,
  lastInputDir: '/private/last-input',
};

function renderModal(onSaved = vi.fn(), onCheckUpdate = vi.fn()) {
  return render(
    <MantineProvider>
      <Notifications />
      <SettingsModal
        onCheckUpdate={onCheckUpdate}
        opened
        onClose={vi.fn()}
        settings={settings}
        onSaved={onSaved}
      />
    </MantineProvider>,
  );
}

describe('SettingsModal', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.getAppVersion.mockResolvedValue('0.1.0');
    api.saveSettings.mockImplementation(async (value) => value);
  });

  it('masks the API key and keeps lastInputDir out of the form', () => {
    renderModal();

    expect(screen.getByLabelText('API 密钥')).toHaveAttribute('type', 'password');
    expect(screen.getByLabelText('API 密钥')).toHaveValue('super-secret');
    expect(screen.queryByText('super-secret')).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue('/private/last-input')).not.toBeInTheDocument();
  });

  it('validates all numeric ranges and required fields before saving', async () => {
    renderModal();
    fireEvent.change(screen.getByLabelText(/API 地址/), { target: { value: '' } });
    fireEvent.change(screen.getByLabelText(/模型名称/), { target: { value: '' } });
    fireEvent.change(screen.getByLabelText('请求超时（秒）'), { target: { value: '0' } });
    fireEvent.change(screen.getByLabelText('最大输出 Token'), { target: { value: '0' } });
    fireEvent.change(screen.getByRole('textbox', { name: /输出目录/ }), { target: { value: '' } });
    fireEvent.click(screen.getByText('高级设置'));
    fireEvent.change(screen.getByLabelText('单块最大字符数'), { target: { value: '7999' } });
    fireEvent.change(screen.getByLabelText('跨块上下文字符数'), { target: { value: '3001' } });
    fireEvent.click(screen.getByRole('button', { name: '保存设置' }));

    expect(await screen.findByText('请输入 API 地址')).toBeInTheDocument();
    expect(screen.getByText('请输入模型名称')).toBeInTheDocument();
    expect(screen.getAllByText('请输入正整数')).toHaveLength(2);
    expect(screen.getByText('请选择输出目录')).toBeInTheDocument();
    expect(screen.getByText('请输入 8000 到 60000 之间的整数')).toBeInTheDocument();
    expect(screen.getByText('请输入 0 到 3000 之间的整数')).toBeInTheDocument();
    expect(api.saveSettings).not.toHaveBeenCalled();
  });

  it('selects an output directory and preserves hidden settings while saving', async () => {
    api.selectOutputDirectory.mockResolvedValue('/tmp/hummingbird-output');
    const onSaved = vi.fn();
    renderModal(onSaved);

    fireEvent.click(screen.getByRole('button', { name: '浏览输出目录' }));
    expect(await screen.findByDisplayValue('/tmp/hummingbird-output')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '保存设置' }));

    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    expect(api.saveSettings).toHaveBeenCalledWith({
      ...settings,
      outputDirectory: '/tmp/hummingbird-output',
    });
    expect(onSaved).toHaveBeenCalledWith({
      ...settings,
      outputDirectory: '/tmp/hummingbird-output',
    });
  });

  it('exposes the same manual update check callback from settings', () => {
    const onCheckUpdate = vi.fn();
    renderModal(vi.fn(), onCheckUpdate);

    fireEvent.click(screen.getByRole('button', { name: '从设置检查更新' }));

    expect(onCheckUpdate).toHaveBeenCalledOnce();
  });
});
