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
  'invalid_passthrough_input',
  'invalid_passthrough_source',
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
  logAnalyseDir: string;
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

export type PassthroughSourceKind = 'manual' | 'awt_template';

export interface PassthroughSource {
  kind: PassthroughSourceKind;
  path: string;
  fileName: string;
}

export interface PassthroughParseRequest {
  requestHex: string;
  responseHex: string | null;
  source: PassthroughSource | null;
}

export type PassthroughProtocol = 'modbusRtu' | 'dlt645' | 'cjt188' | 'unknown';

export interface PassthroughField {
  name: string;
  byteStart: number;
  byteEnd: number;
  rawHex: string;
  displayValue: string;
  source: 'code' | 'manual' | 'awtTemplate' | 'aiExplanation';
}

export interface PassthroughRegister {
  address: number | null;
  identifier: string | null;
  rawHex: string;
  source: 'code' | 'manual' | 'awtTemplate' | 'aiExplanation';
}

export interface PassthroughChecksum {
  kind: string;
  received: string | null;
  calculated: string;
  valid: boolean | null;
}

export interface PassthroughMessageResult {
  index: number;
  role: 'request' | 'response';
  rawSegment: string;
  cleanedHex: string | null;
  protocol: PassthroughProtocol;
  summary: string;
  fields: PassthroughField[];
  registers: PassthroughRegister[];
  explanations: Array<{
    address: number | null;
    parameterCode: string | null;
    parameterName: string | null;
    unit: string | null;
    rawHex: string;
    convertedValue: string | null;
    meaning: string | null;
    source: 'code' | 'manual' | 'awtTemplate' | 'aiExplanation';
    warnings: Array<{ code: string; message: string }>;
  }>;
  checksum: PassthroughChecksum | null;
  warnings: Array<{ code: string; message: string }>;
  error: { code: string; message: string } | null;
}

export interface PassthroughBatchResult {
  results: PassthroughMessageResult[];
  sourceWarning: string | null;
  mappingDiagnostics?: {
    extractedCount: number;
    matchedCount: number;
    unmatchedAddresses: number[];
  } | null;
}

// ── Log Analysis ─────────────────────────────────────────────

export interface SshServerConfig {
  name: string;
  host: string;
  port: number;
  user: string;
  password?: string;
  privateKey?: string;
  appRoot: string;
}

export interface AnalyseConfig {
  logAnalyseDir: string;
  sshServers: SshServerConfig[];
}

export interface RemoteFile {
  name: string;
  path: string;
  sizeBytes: number;
  modified: number;
}

export interface CategoryCount {
  category: string;
  count: number;
}

export interface LogSummary {
  totalLines: number;
  entryCount: number;
  timeStart: string | null;
  timeEnd: string | null;
  categoryCounts: CategoryCount[];
  uniqueSns: string[];
  uniqueProjects: string[];
  connectionLeaks: number;
  dispatchDisabledRules: string[];
  threadCount: number;
  snErrors: SnErrorCount[];
}

export interface SnErrorCount {
  sn: string;
  errorType: string;
  count: number;
}

export interface TimeBucket {
  hour: string;
  count: number;
}

export interface ThreadStuckInfo {
  thread: string;
  startTime: string;
  endTime: string;
  durationMs: number;
}

export interface AnalyseStatus {
  active: boolean;
  stage: string | null;
  progressPct: number;
  detail: string;
}

export type AnalyseEvent =
  | { type: 'stage'; taskId: string; stage: string }
  | { type: 'progress'; taskId: string; completed: number; total: number; detail: string }
  | { type: 'ai_chunk'; taskId: string; batch: number; content: string }
  | { type: 'completed'; taskId: string; summaryJson: string; heatmapJson: string }
  | { type: 'cancelled'; taskId: string }
  | { type: 'failed'; taskId: string; error: AppErrorDto };
