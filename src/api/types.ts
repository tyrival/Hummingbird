export const ERROR_CODES = [
  'file_not_found',
  'file_too_large',
  'unsupported_format',
  'no_extractable_text',
  'parse_failed',
  'invalid_settings',
  'network_failed',
  'authentication_failed',
  'context_too_large',
  'empty_ai_response',
  'invalid_ai_csv',
  'save_failed',
  'task_active',
  'no_active_task',
  'cancelled',
  'update_failed',
  'update_blocked',
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

export interface AppErrorDto {
  code: ErrorCode;
  message: string;
  detail: string | null;
}

export type TaskStage =
  | 'validating_input'
  | 'extracting_text'
  | 'preparing_chunks'
  | 'calling_ai'
  | 'merging_results'
  | 'saving_output'
  | 'completed'
  | 'cancelled'
  | 'failed';

export type TaskEvent =
  | { type: 'stage'; taskId: string; stage: TaskStage }
  | { type: 'log'; taskId: string; level: 'debug' | 'info' | 'warn' | 'error'; message: string }
  | { type: 'progress'; taskId: string; completedChunks: number; totalChunks: number }
  | { type: 'completed'; taskId: string; outputPath: string; recordCount: number }
  | { type: 'cancelled'; taskId: string }
  | { type: 'failed'; taskId: string; error: AppErrorDto };

export interface TaskStatus {
  taskId: string | null;
  active: boolean;
  completedChunks: number;
  totalChunks: number;
  stage: TaskStage | null;
  outputPath?: string | null;
  recordCount?: number | null;
  error?: AppErrorDto | null;
  cleanupPending?: boolean;
  safeToExit?: boolean;
}

export interface SettingsDto {
  schemaVersion: number;
  migrationVersion: number;
  baseUrl: string;
  apiKey: string;
  model: string;
  timeoutSeconds: number;
  maxTokens: number;
  outputDirectory: string;
  chunkMaxChars: number;
  contextChars: number;
  lastInputDir: string | null;
}

export interface SelectedInputDto {
  path: string;
  fileName: string;
  sizeBytes: number;
}

export type InputDropResult =
  | { status: 'success'; input: SelectedInputDto }
  | { status: 'error'; error: AppErrorDto };

export interface UpdateInfoDto {
  available: boolean;
  currentVersion: string;
  version: string | null;
  notes: string | null;
  publishedAt: string | null;
  installMode: 'in_app' | 'manual_deb';
  releasePageUrl: string;
}

export type UpdateDownloadEvent =
  | { type: 'started'; contentLength: number | null }
  | { type: 'chunk'; chunkLength: number }
  | { type: 'finished' };

export type UpdateDownloadResult = 'downloaded' | 'opened_release_page';
export type UpdateInstallResult = 'installed';
