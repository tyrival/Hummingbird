import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow, type CloseRequestedEvent } from '@tauri-apps/api/window';
import {
  ERROR_CODES,
  type AppErrorDto,
  type ErrorCode,
  type InputDropResult,
  type SelectedInputDto,
  type SettingsDto,
  type TaskEvent,
  type TaskStatus,
  type UpdateDownloadEvent,
  type UpdateDownloadResult,
  type UpdateInfoDto,
  type UpdateInstallResult,
} from './types';

type CommandArguments = Record<string, unknown>;

const FALLBACK_ERROR: AppErrorDto = {
  code: 'network_failed',
  message: '操作失败，请重试。',
  detail: null,
};

function isErrorCode(value: unknown): value is ErrorCode {
  return typeof value === 'string' && (ERROR_CODES as readonly string[]).includes(value);
}

export function normalizeAppError(value: unknown): AppErrorDto {
  if (typeof value !== 'object' || value === null) {
    return FALLBACK_ERROR;
  }

  const candidate = value as Record<string, unknown>;
  if (isErrorCode(candidate.code) && typeof candidate.message === 'string') {
    return {
      code: candidate.code,
      message: candidate.message,
      detail: typeof candidate.detail === 'string' ? candidate.detail : null,
    };
  }

  return FALLBACK_ERROR;
}

export async function invokeCommand<T>(
  command: string,
  args?: CommandArguments,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw normalizeAppError(error);
  }
}

export async function listenForTaskEvents(
  listener: (event: TaskEvent) => void,
): Promise<UnlistenFn> {
  return listen<TaskEvent>('task-event', ({ payload }) => listener(payload));
}

export async function listenForInputDropResults(
  listener: (result: InputDropResult) => void,
): Promise<UnlistenFn> {
  return listen<InputDropResult>('input-drop-result', ({ payload }) => listener(payload));
}

export const getSettings = (): Promise<SettingsDto> =>
  invokeCommand<SettingsDto>('get_settings');

export const saveSettings = (settings: SettingsDto): Promise<SettingsDto> =>
  invokeCommand<SettingsDto>('save_settings', { settings });

export const selectInputFile = (): Promise<SelectedInputDto | null> =>
  invokeCommand<SelectedInputDto | null>('select_input_file');

export const selectOutputDirectory = (): Promise<string | null> =>
  invokeCommand<string | null>('select_output_directory');

export const startExtraction = (inputPath: string): Promise<string> =>
  invokeCommand<string>('start_extraction', { inputPath });

export const cancelExtraction = (): Promise<void> =>
  invokeCommand<void>('cancel_extraction');

export const cancelExtractionAndWait = (): Promise<TaskStatus> =>
  invokeCommand<TaskStatus>('cancel_extraction_and_wait');

export const prepareExit = (): Promise<TaskStatus> =>
  invokeCommand<TaskStatus>('prepare_exit');

export const getTaskStatus = (): Promise<TaskStatus> =>
  invokeCommand<TaskStatus>('get_task_status');

export const openOutputDirectory = (): Promise<void> =>
  invokeCommand<void>('open_output_directory');

export const openTaskOutputDirectory = (outputPath: string): Promise<void> =>
  invokeCommand<void>('open_task_output_directory', { outputPath });

export const getAppVersion = (): Promise<string> =>
  invokeCommand<string>('get_app_version');

export const checkForUpdate = (manual: boolean): Promise<UpdateInfoDto> =>
  invokeCommand<UpdateInfoDto>('check_for_update', { manual });

export const downloadUpdate = (
  expectedVersion: string,
): Promise<UpdateDownloadResult> =>
  invokeCommand<UpdateDownloadResult>('download_update', { expectedVersion });

export const installDownloadedUpdate = (
  expectedVersion: string,
): Promise<UpdateInstallResult> =>
  invokeCommand<UpdateInstallResult>('install_downloaded_update', { expectedVersion });

export const relaunchApp = (): Promise<void> =>
  invokeCommand<void>('relaunch_app');

export async function listenForUpdateDownloadEvents(
  listener: (event: UpdateDownloadEvent) => void,
): Promise<UnlistenFn> {
  return listen<UpdateDownloadEvent>('update-download-event', ({ payload }) => listener(payload));
}

export async function listenForCloseRequests(
  listener: (event: CloseRequestedEvent) => void | Promise<void>,
): Promise<UnlistenFn> {
  return getCurrentWindow().onCloseRequested(listener);
}

export async function destroyMainWindow(): Promise<void> {
  await getCurrentWindow().destroy();
}

// ── Log Analysis ─────────────────────────────────────────────

import type {
  AnalyseConfig,
  AnalyseEvent,
  AnalyseStatus,
  RemoteFile,
  SshServerConfig,
} from './types';

export const getAnalyseConfig = (): Promise<AnalyseConfig> =>
  invokeCommand<AnalyseConfig>('get_analyse_config');

export const saveSshServers = (servers: SshServerConfig[]): Promise<SshServerConfig[]> =>
  invokeCommand<SshServerConfig[]>('save_ssh_servers', { servers });

export const testSshConnection = (server: SshServerConfig): Promise<string> =>
  invokeCommand<string>('test_ssh_connection', { server });

export const listRemoteLogs = (
  server: SshServerConfig,
  relativePath?: string,
): Promise<RemoteFile[]> =>
  invokeCommand<RemoteFile[]>('list_remote_logs_command', { server, relativePath });

export const downloadLogs = (
  server: SshServerConfig,
  remoteFiles: string[],
  relativePath?: string,
): Promise<string[]> =>
  invokeCommand<string[]>('download_logs_command', { server, remoteFiles, relativePath });

export const startLogAnalysis = (filePaths: string[]): Promise<void> =>
  invokeCommand<void>('start_log_analysis', { filePaths });

export const cancelLogAnalysis = (): Promise<void> =>
  invokeCommand<void>('cancel_log_analysis');

export const getAnalyseStatus = (): Promise<AnalyseStatus> =>
  invokeCommand<AnalyseStatus>('get_analyse_status');

export const selectLogFolder = (): Promise<string[]> =>
  invokeCommand<string[]>('select_log_folder');

export const selectKeyFile = (): Promise<string> =>
  invokeCommand<string>('select_key_file');

export async function listenForAnalyseEvents(
  listener: (event: AnalyseEvent) => void,
): Promise<UnlistenFn> {
  return listen<AnalyseEvent>('analyse-event', ({ payload }) => listener(payload));
}
