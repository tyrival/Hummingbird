import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as tauriApi from './api/tauri';
import type { SettingsDto, TaskStatus } from './api/types';
import { StrictMode } from 'react';
import App from './App';

vi.mock('./api/tauri', () => ({
  cancelExtraction: vi.fn(),
  cancelExtractionAndWait: vi.fn(),
  checkForUpdate: vi.fn(),
  destroyMainWindow: vi.fn(),
  downloadUpdate: vi.fn(),
  installDownloadedUpdate: vi.fn(),
  getAppVersion: vi.fn(),
  getSettings: vi.fn(),
  getTaskStatus: vi.fn(),
  listenForCloseRequests: vi.fn(),
  listenForInputDropResults: vi.fn(),
  listenForTaskEvents: vi.fn(),
  listenForUpdateDownloadEvents: vi.fn(),
  openOutputDirectory: vi.fn(),
  openTaskOutputDirectory: vi.fn(),
  prepareExit: vi.fn(),
  relaunchApp: vi.fn(),
  saveSettings: vi.fn(),
  selectInputFile: vi.fn(),
  selectOutputDirectory: vi.fn(),
  startExtraction: vi.fn(),
}));

const api = vi.mocked(tauriApi);
const settings: SettingsDto = {
  schemaVersion: 1,
  migrationVersion: 1,
  baseUrl: 'https://api.example.test/v1',
  apiKey: 'secret',
  model: 'deepseek-chat',
  timeoutSeconds: 600,
  maxTokens: 16384,
  outputDirectory: 'output',
  chunkMaxChars: 30000,
  contextChars: 3000,
  lastInputDir: '/private/input',
};

type CloseEvent = { preventDefault: () => void };
let closeListener: ((event: CloseEvent) => void | Promise<void>) | undefined;

function renderApp(strict = false) {
  const content = (
    <>
      <Notifications />
      <App />
    </>
  );
  return render(
    <MantineProvider>
      {strict ? <StrictMode>{content}</StrictMode> : content}
    </MantineProvider>,
  );
}

describe('application shell', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    closeListener = undefined;
    api.getSettings.mockResolvedValue(settings);
    api.getTaskStatus.mockResolvedValue({
      taskId: null,
      active: false,
      completedChunks: 0,
      totalChunks: 0,
      stage: null,
    });
    api.getAppVersion.mockResolvedValue('0.1.0');
    api.checkForUpdate.mockResolvedValue({
      available: false,
      currentVersion: '0.1.0',
      version: null,
      notes: null,
      publishedAt: null,
      installMode: 'in_app',
      releasePageUrl: 'https://github.com/tyrival/Hummingbird-Releases/releases/latest',
    });
    api.prepareExit.mockResolvedValue({
      taskId: null, active: false, completedChunks: 0, totalChunks: 0, stage: null,
      cleanupPending: false, safeToExit: true,
    });
    api.listenForCloseRequests.mockImplementation(async (listener) => {
      closeListener = listener as (event: CloseEvent) => void | Promise<void>;
      return () => undefined;
    });
    api.listenForInputDropResults.mockResolvedValue(() => undefined);
    api.listenForTaskEvents.mockResolvedValue(() => undefined);
    api.listenForUpdateDownloadEvents.mockResolvedValue(() => undefined);
  });

  it('renders labeled rounded navigation and changes the selected workspace', async () => {
    renderApp();

    const awt = screen.getByRole('button', { name: 'AWT模板生成' });
    const passthrough = screen.getByRole('button', { name: '透传命令识别' });
    expect(awt).toHaveTextContent('AWT模板生成');
    expect(passthrough).toHaveTextContent('透传命令识别');
    expect(awt).toHaveAttribute('aria-current', 'page');
    expect(screen.getByRole('heading', { name: 'AWT模板生成' })).toBeInTheDocument();

    fireEvent.click(passthrough);
    expect(passthrough).toHaveAttribute('aria-current', 'page');
    expect(awt).not.toHaveAttribute('aria-current');
  });

  it('shows a truly empty semantic passthrough workspace', () => {
    renderApp();
    fireEvent.click(screen.getByRole('button', { name: '透传命令识别' }));

    const workspace = screen.getByRole('main', { name: '透传命令识别' });
    expect(screen.getAllByRole('main')).toHaveLength(1);
    expect(workspace).toBeEmptyDOMElement();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });

  it('loads settings, opens the settings modal and reflects a saved output directory', async () => {
    api.saveSettings.mockImplementation(async (value) => value);
    api.selectOutputDirectory.mockResolvedValue('/tmp/new-output');
    renderApp();

    expect(await screen.findByText('output')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '设置' }));
    expect(await screen.findByRole('dialog', { name: '设置' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '浏览输出目录' }));
    expect(await screen.findByDisplayValue('/tmp/new-output')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '保存设置' }));

    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledOnce());
    expect(await screen.findByText('/tmp/new-output')).toBeInTheDocument();
  });

  it('checks silently on startup and opens the updater when a newer version exists', async () => {
    vi.useFakeTimers();
    api.checkForUpdate.mockResolvedValue({
      available: true,
      currentVersion: '0.1.0',
      version: '0.2.0',
      notes: '修复更新流程',
      publishedAt: '2026-07-20T08:00:00Z',
      installMode: 'in_app',
      releasePageUrl: 'https://github.com/tyrival/Hummingbird-Releases/releases/latest',
    });

    renderApp();
    expect(api.checkForUpdate).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(2999));
    expect(api.checkForUpdate).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(1));
    expect(api.checkForUpdate).toHaveBeenCalledTimes(1);
    expect(api.checkForUpdate).toHaveBeenCalledWith(false);
    expect(screen.getByRole('dialog', { name: '发现新版本' })).toBeInTheDocument();
    vi.useRealTimers();
  });

  it('offers a manual check entry and reports when the current version is latest', async () => {
    renderApp();

    fireEvent.click(screen.getByRole('button', { name: '检查更新' }));

    await waitFor(() => expect(api.checkForUpdate).toHaveBeenCalledWith(true));
    expect(await screen.findByText('当前已是最新版本。')).toBeInTheDocument();
  });

  it('cancels the delayed background check when the app unmounts', async () => {
    vi.useFakeTimers();
    const view = renderApp();
    view.unmount();

    await act(async () => vi.advanceTimersByTimeAsync(3000));

    expect(api.checkForUpdate).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it('runs the delayed background check only once under React StrictMode', async () => {
    vi.useFakeTimers();
    renderApp(true);

    await act(async () => vi.advanceTimersByTimeAsync(3000));

    expect(api.checkForUpdate).toHaveBeenCalledTimes(1);
    expect(api.checkForUpdate).toHaveBeenCalledWith(false);
    vi.useRealTimers();
  });

  it('prevents the native close first, checks live status, and closes idle without confirmation', async () => {
    renderApp();
    await waitFor(() => expect(closeListener).toBeDefined());
    const confirm = vi.spyOn(window, 'confirm');
    const event = { preventDefault: vi.fn() };

    await act(async () => closeListener?.(event));

    expect(confirm).not.toHaveBeenCalled();
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(api.getTaskStatus).toHaveBeenCalled();
    expect(api.prepareExit).toHaveBeenCalledOnce();
    expect(api.destroyMainWindow).toHaveBeenCalledOnce();
    confirm.mockRestore();
  });

  it('intercepts an active close and only destroys the window after confirmation', async () => {
    api.getTaskStatus.mockResolvedValue({
      taskId: 'task-1', active: true, completedChunks: 1, totalChunks: 2, stage: 'calling_ai',
    });
    api.prepareExit.mockResolvedValue({
      taskId: 'task-1', active: false, completedChunks: 1, totalChunks: 2,
      stage: 'cancelled', outputPath: null, recordCount: null, error: null,
      cleanupPending: false, safeToExit: true,
    });
    const confirm = vi.spyOn(window, 'confirm').mockReturnValueOnce(false).mockReturnValueOnce(true);
    renderApp();
    await waitFor(() => expect(screen.getByRole('button', { name: '停止' })).toBeInTheDocument());
    const first = { preventDefault: vi.fn() };
    const second = { preventDefault: vi.fn() };

    await act(async () => closeListener?.(first));
    expect(first.preventDefault).toHaveBeenCalledOnce();
    expect(api.destroyMainWindow).not.toHaveBeenCalled();

    await act(async () => closeListener?.(second));
    expect(second.preventDefault).toHaveBeenCalledOnce();
    expect(api.prepareExit).toHaveBeenCalledOnce();
    expect(api.destroyMainWindow).toHaveBeenCalledOnce();
    confirm.mockRestore();
  });

  it('keeps intercepting duplicate close requests while confirmed shutdown is pending', async () => {
    api.getTaskStatus.mockResolvedValue({
      taskId: 'task-1', active: true, completedChunks: 1, totalChunks: 2, stage: 'calling_ai',
    });
    let finishCancel: ((status: TaskStatus) => void) | undefined;
    api.prepareExit.mockImplementation(() => new Promise((resolve) => {
      finishCancel = resolve;
    }));
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    renderApp();
    await waitFor(() => expect(screen.getByRole('button', { name: '停止' })).toBeInTheDocument());
    const first = { preventDefault: vi.fn() };
    const duplicate = { preventDefault: vi.fn() };

    const shutdown = closeListener?.(first);
    await act(async () => closeListener?.(duplicate));

    expect(first.preventDefault).toHaveBeenCalledOnce();
    expect(duplicate.preventDefault).toHaveBeenCalledOnce();
    expect(confirm).toHaveBeenCalledOnce();
    finishCancel?.({
      taskId: 'task-1', active: false, completedChunks: 1, totalChunks: 2,
      stage: 'cancelled', outputPath: null, recordCount: null, error: null,
      cleanupPending: false, safeToExit: true,
    });
    await act(async () => shutdown);
    confirm.mockRestore();
  });

  it('does not destroy the window when cancel-and-wait reports a cleanup failure', async () => {
    api.getTaskStatus.mockResolvedValue({
      taskId: 'task-1', active: true, completedChunks: 1, totalChunks: 2, stage: 'saving_output',
    });
    api.prepareExit.mockRejectedValue({
      code: 'save_failed', message: '保存文件失败。', detail: null,
    });
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    renderApp();
    await waitFor(() => expect(closeListener).toBeDefined());
    const event = { preventDefault: vi.fn() };

    await act(async () => closeListener?.(event));

    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(api.destroyMainWindow).not.toHaveBeenCalled();
    expect(await screen.findByText('保存文件失败。')).toBeInTheDocument();
    confirm.mockRestore();
  });

  it('does not destroy the window while an update operation is in progress', async () => {
    api.prepareExit.mockRejectedValue({
      code: 'update_blocked',
      message: '当前有任务、更新或清理操作正在进行，请完成后重试。',
      detail: null,
    });
    renderApp();
    await waitFor(() => expect(closeListener).toBeDefined());

    await act(async () => closeListener?.({ preventDefault: vi.fn() }));

    expect(api.destroyMainWindow).not.toHaveBeenCalled();
    expect(await screen.findByText('当前有任务、更新或清理操作正在进行，请完成后重试。'))
      .toBeInTheDocument();
  });

  it('retries pending cleanup on a second close and only then destroys the window', async () => {
    api.prepareExit
      .mockRejectedValueOnce({ code: 'save_failed', message: '保存文件失败。', detail: null })
      .mockResolvedValueOnce({
        taskId: 'task-old', active: false, completedChunks: 0, totalChunks: 0,
        stage: 'failed', cleanupPending: false, safeToExit: true,
      });
    renderApp();
    await waitFor(() => expect(closeListener).toBeDefined());

    await act(async () => closeListener?.({ preventDefault: vi.fn() }));
    expect(api.destroyMainWindow).not.toHaveBeenCalled();
    await act(async () => closeListener?.({ preventDefault: vi.fn() }));

    expect(api.prepareExit).toHaveBeenCalledTimes(2);
    expect(api.destroyMainWindow).toHaveBeenCalledOnce();
  });
});
