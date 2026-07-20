import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as tauriApi from '../api/tauri';
import type { UpdateDownloadEvent, UpdateInfoDto } from '../api/types';
import { UpdateModal } from './UpdateModal';

vi.mock('../api/tauri', () => ({
  downloadUpdate: vi.fn(),
  installDownloadedUpdate: vi.fn(),
  listenForUpdateDownloadEvents: vi.fn(),
  relaunchApp: vi.fn(),
}));

const api = vi.mocked(tauriApi);
const update: UpdateInfoDto = {
  available: true,
  currentVersion: '0.1.0',
  version: '0.2.0',
  notes: '支持在线更新\n修复长说明书处理',
  publishedAt: '2026-07-20T08:00:00Z',
  installMode: 'in_app',
  releasePageUrl: 'https://github.com/tyrival/Hummingbird-Releases/releases/latest',
};

let progressListener: ((event: UpdateDownloadEvent) => void) | undefined;

function renderModal(info = update, taskActive = false) {
  return render(
    <MantineProvider>
      <Notifications />
      <UpdateModal
        opened
        onClose={vi.fn()}
        taskActive={taskActive}
        update={info}
      />
    </MantineProvider>,
  );
}

describe('UpdateModal', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    progressListener = undefined;
    api.listenForUpdateDownloadEvents.mockImplementation(async (listener) => {
      progressListener = listener;
      return () => undefined;
    });
    api.downloadUpdate.mockResolvedValue('downloaded');
    api.installDownloadedUpdate.mockResolvedValue('installed');
    api.relaunchApp.mockResolvedValue(undefined);
  });

  it('shows the semantic version, publication date and multiline release notes', () => {
    renderModal();

    expect(screen.getByRole('dialog', { name: '发现新版本' })).toBeInTheDocument();
    expect(screen.getByText('0.1.0 → 0.2.0')).toBeInTheDocument();
    expect(screen.getByText('2026-07-20')).toBeInTheDocument();
    expect(screen.getByText(/支持在线更新/)).toHaveTextContent(
      '支持在线更新 修复长说明书处理',
    );
  });

  it('requires confirmation before downloading and again before relaunching', async () => {
    const confirm = vi.spyOn(window, 'confirm')
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true)
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true)
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true);
    renderModal();

    await waitFor(() => expect(screen.getByRole('button', { name: '下载更新' })).toBeEnabled());
    fireEvent.click(screen.getByRole('button', { name: '下载更新' }));
    expect(api.downloadUpdate).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: '下载更新' }));
    await waitFor(() => expect(api.downloadUpdate).toHaveBeenCalledWith('0.2.0'));
    expect(api.installDownloadedUpdate).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: '安装更新' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '安装更新' }));
    expect(api.installDownloadedUpdate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '安装更新' }));
    await waitFor(() => expect(api.installDownloadedUpdate).toHaveBeenCalledWith('0.2.0'));
    expect(api.relaunchApp).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: '立即重启' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '立即重启' }));
    expect(api.relaunchApp).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '立即重启' }));
    await waitFor(() => expect(api.relaunchApp).toHaveBeenCalledOnce());
    confirm.mockRestore();
  });

  it('renders cumulative download progress from native events', async () => {
    let finish: ((value: 'downloaded') => void) | undefined;
    api.downloadUpdate.mockImplementation(() => new Promise((resolve) => {
      finish = resolve;
    }));
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    renderModal();
    await waitFor(() => expect(progressListener).toBeDefined());

    fireEvent.click(screen.getByRole('button', { name: '下载更新' }));
    act(() => progressListener?.({ type: 'started', contentLength: 1000 }));
    act(() => progressListener?.({ type: 'chunk', chunkLength: 250 }));
    expect(screen.getByText('25%')).toBeInTheDocument();
    act(() => progressListener?.({ type: 'finished' }));
    expect(screen.getByText('100%')).toBeInTheDocument();
    finish?.('downloaded');
  });

  it('opens the public release page for DEB without offering an in-app restart', async () => {
    api.downloadUpdate.mockResolvedValue('opened_release_page');
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    renderModal({ ...update, installMode: 'manual_deb' });

    expect(screen.getByText(/DEB 安装需要手动升级/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '打开下载页面' }));
    await waitFor(() => expect(api.downloadUpdate).toHaveBeenCalledWith('0.2.0'));
    expect(await screen.findByText('发布页面已打开')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '立即重启' })).not.toBeInTheDocument();
  });

  it('disables installation while an extraction task is active', async () => {
    renderModal(update, true);

    expect(screen.getByText('当前提取任务结束后才能安装更新。')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('button', { name: '下载更新' })).toBeDisabled());
  });

  it('keeps download disabled until the native progress listener is ready', async () => {
    let ready: ((unlisten: () => void) => void) | undefined;
    api.listenForUpdateDownloadEvents.mockImplementation(() => new Promise((resolve) => {
      ready = resolve;
    }));
    renderModal();

    expect(screen.getByRole('button', { name: '正在准备更新…' })).toBeDisabled();
    await act(async () => ready?.(() => undefined));
    expect(screen.getByRole('button', { name: '下载更新' })).toBeEnabled();
  });
});
