import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  cancelExtraction,
  cancelExtractionAndWait,
  checkForUpdate,
  downloadUpdate,
  installDownloadedUpdate,
  getSettings,
  listenForInputDropResults,
  listenForTaskEvents,
  listenForUpdateDownloadEvents,
  openOutputDirectory,
  openTaskOutputDirectory,
  prepareExit,
  relaunchApp,
  saveSettings,
  selectInputFile,
  startExtraction,
} from './tauri';
import type {
  InputDropResult,
  SettingsDto,
  TaskEvent,
  UpdateDownloadEvent,
} from './types';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

describe('Tauri adapter', () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it('returns typed command results without exposing raw invoke to callers', async () => {
    const settings: SettingsDto = {
      schemaVersion: 1,
      migrationVersion: 1,
      baseUrl: 'https://api.example.test/v1',
      apiKey: '',
      model: 'deepseek-chat',
      timeoutSeconds: 600,
      maxTokens: 16384,
      outputDirectory: 'output',
      chunkMaxChars: 30000,
      contextChars: 3000,
      lastInputDir: null,
    };
    mockedInvoke.mockResolvedValue(settings);

    await expect(getSettings()).resolves.toEqual(settings);
    expect(mockedInvoke).toHaveBeenCalledWith('get_settings', undefined);
  });

  it('preserves structured AppError DTO rejections', async () => {
    const error = {
      code: 'authentication_failed',
      message: '凭据无效',
      detail: null,
    };
    mockedInvoke.mockRejectedValue(error);

    await expect(getSettings()).rejects.toEqual(error);
  });

  it('normalizes unknown rejected values to a safe AppError DTO', async () => {
    mockedInvoke.mockRejectedValue(new Error('native details must not reach UI parsing'));

    await expect(getSettings()).rejects.toEqual({
      code: 'network_failed',
      message: '操作失败，请重试。',
      detail: null,
    });
  });

  it('forwards typed task-event payloads to subscribers', async () => {
    const event: TaskEvent = {
      type: 'progress',
      taskId: 'task-1',
      completedChunks: 1,
      totalChunks: 2,
    };
    const unsubscribe = vi.fn();
    let handler: ((event: { payload: TaskEvent }) => void) | undefined;
    mockedListen.mockImplementation(async (_name, listener) => {
      handler = listener as (event: { payload: TaskEvent }) => void;
      return unsubscribe;
    });
    const onEvent = vi.fn();

    const unlisten = await listenForTaskEvents(onEvent);
    handler?.({ payload: event });

    expect(mockedListen).toHaveBeenCalledWith('task-event', expect.any(Function));
    expect(onEvent).toHaveBeenCalledWith(event);
    expect(unlisten).toBe(unsubscribe);
  });

  it('uses the exact Rust command names and camelCase arguments', async () => {
    const settings = {
      schemaVersion: 1,
      migrationVersion: 1,
      baseUrl: 'https://api.example.test/v1',
      apiKey: '',
      model: 'deepseek-chat',
      timeoutSeconds: 600,
      maxTokens: 16384,
      outputDirectory: 'output',
      chunkMaxChars: 30000,
      contextChars: 3000,
      lastInputDir: null,
    } satisfies SettingsDto;
    mockedInvoke.mockResolvedValue(undefined);

    await saveSettings(settings);
    await selectInputFile();
    await startExtraction('/tmp/manual.pdf');
    await cancelExtraction();
    await openOutputDirectory();
    await cancelExtractionAndWait();
    await openTaskOutputDirectory('/tmp/result.csv');
    await prepareExit();

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'save_settings', { settings });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'select_input_file', undefined);
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, 'start_extraction', {
      inputPath: '/tmp/manual.pdf',
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(4, 'cancel_extraction', undefined);
    expect(mockedInvoke).toHaveBeenNthCalledWith(5, 'open_output_directory', undefined);
    expect(mockedInvoke).toHaveBeenNthCalledWith(6, 'cancel_extraction_and_wait', undefined);
    expect(mockedInvoke).toHaveBeenNthCalledWith(7, 'open_task_output_directory', {
      outputPath: '/tmp/result.csv',
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(8, 'prepare_exit', undefined);
  });

  it('only forwards Rust-authorized input-drop-result payloads', async () => {
    const result: InputDropResult = {
      status: 'success',
      input: { path: '/tmp/manual.pdf', fileName: 'manual.pdf', sizeBytes: 42 },
    };
    let handler: ((event: { payload: InputDropResult }) => void) | undefined;
    mockedListen.mockImplementation(async (_name, listener) => {
      handler = listener as (event: { payload: InputDropResult }) => void;
      return () => undefined;
    });
    const onResult = vi.fn();

    await listenForInputDropResults(onResult);
    handler?.({ payload: result });

    expect(mockedListen).toHaveBeenCalledWith('input-drop-result', expect.any(Function));
    expect(onResult).toHaveBeenCalledWith(result);
  });

  it('uses the update command contract and forwards native download progress', async () => {
    mockedInvoke
      .mockResolvedValueOnce({ available: false, currentVersion: '0.1.0' })
      .mockResolvedValueOnce('downloaded')
      .mockResolvedValueOnce('installed')
      .mockResolvedValueOnce(undefined);
    const progress: UpdateDownloadEvent = { type: 'chunk', chunkLength: 512 };
    let handler: ((event: { payload: UpdateDownloadEvent }) => void) | undefined;
    mockedListen.mockImplementation(async (_name, listener) => {
      handler = listener as (event: { payload: UpdateDownloadEvent }) => void;
      return () => undefined;
    });
    const onProgress = vi.fn();

    await checkForUpdate(true);
    await downloadUpdate('0.2.0');
    await installDownloadedUpdate('0.2.0');
    await relaunchApp();
    await listenForUpdateDownloadEvents(onProgress);
    handler?.({ payload: progress });

    expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'check_for_update', { manual: true });
    expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'download_update', {
      expectedVersion: '0.2.0',
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(3, 'install_downloaded_update', {
      expectedVersion: '0.2.0',
    });
    expect(mockedInvoke).toHaveBeenNthCalledWith(4, 'relaunch_app', undefined);
    expect(mockedListen).toHaveBeenCalledWith('update-download-event', expect.any(Function));
    expect(onProgress).toHaveBeenCalledWith(progress);
  });

});
