import { useCallback, useEffect, useRef, useState } from 'react';
import {
  cancelExtraction,
  getTaskStatus,
  listenForTaskEvents,
  normalizeAppError,
  startExtraction,
} from '../../api/tauri';
import type { AppErrorDto, TaskEvent, TaskStage, TaskStatus } from '../../api/types';
import type { LogEntry, LogLevel } from '../../components/LogPanel';

type TerminalResult =
  | { type: 'completed'; outputPath: string; recordCount: number }
  | { type: 'cancelled' }
  | { type: 'failed'; error: AppErrorDto };

interface InternalTaskState {
  taskId: string | null;
  active: boolean;
  stage: TaskStage | null;
  completedChunks: number;
  totalChunks: number;
  logs: LogEntry[];
  terminal: TerminalResult | null;
}

export interface ExtractionTaskState extends Omit<InternalTaskState, 'taskId'> {
  start: (inputPath: string) => Promise<void>;
  requestCancel: () => Promise<void>;
  reset: () => void;
}

let nextLogId = 0;

const emptyState = (): InternalTaskState => ({
  taskId: null,
  active: false,
  stage: null,
  completedChunks: 0,
  totalChunks: 0,
  logs: [],
  terminal: null,
});

function timestamp(): string {
  return new Intl.DateTimeFormat('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  }).format(new Date());
}

function logEntry(level: LogLevel, message: string): LogEntry {
  const entry = { id: nextLogId, timestamp: timestamp(), level, message };
  nextLogId += 1;
  return entry;
}

function appendLog(logs: LogEntry[], entry: LogEntry | null): LogEntry[] {
  return entry ? [...logs.slice(-499), entry] : logs;
}

export function useExtractionTask(
  onActiveChange: (active: boolean) => void,
): ExtractionTaskState {
  const [state, setState] = useState<InternalTaskState>(emptyState);
  const startedDuringInitializationRef = useRef(false);

  useEffect(() => {
    onActiveChange(state.active);
  }, [onActiveChange, state.active]);

  useEffect(() => {
    let disposed = false;
    let initializing = true;
    let unlisten: (() => void) | undefined;
    const buffered: Array<{ event: TaskEvent; log: LogEntry | null }> = [];

    const receive = (event: TaskEvent) => {
      if (disposed) return;
      const eventLog = taskEventLog(event);
      if (initializing) {
        buffered.push({ event, log: eventLog });
        return;
      }
      setState((current) => reduceTaskEvent(current, event, eventLog));
    };

    void (async () => {
      try {
        unlisten = await listenForTaskEvents(receive);
        const status = await getTaskStatus();
        if (disposed) return;
        if (startedDuringInitializationRef.current) {
          initializing = false;
          setState((current) => buffered.reduce(
            (next, pending) => reduceTaskEvent(next, pending.event, pending.log),
            current,
          ));
          return;
        }
        let recovered = stateFromStatus(status);
        const recoveredTerminal = recovered.terminal !== null;
        for (const pending of buffered) {
          if (recoveredTerminal && !isTerminalEvent(pending.event)) continue;
          recovered = reduceTaskEvent(recovered, pending.event, pending.log);
        }
        initializing = false;
        setState(recovered);
      } catch (error) {
        initializing = false;
        const safeError = normalizeAppError(error);
        let recovered: InternalTaskState = { ...emptyState(), active: true };
        for (const pending of buffered) {
          recovered = reduceTaskEvent(recovered, pending.event, pending.log);
        }
        if (recovered.active && buffered.length > 0) {
          recovered = {
            ...recovered,
            logs: appendLog(
              recovered.logs,
              logEntry('warn', `${safeError.message}当前任务状态未知。`),
            ),
          };
        } else if (recovered.active) {
          recovered = {
            ...recovered,
            active: false,
            stage: 'failed',
            logs: appendLog(recovered.logs, logEntry('error', safeError.message)),
            terminal: { type: 'failed', error: safeError },
          };
        }
        setState(recovered);
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const start = useCallback(async (inputPath: string) => {
    startedDuringInitializationRef.current = true;
    const started = {
      ...emptyState(),
      active: true,
      stage: 'validating_input' as const,
      logs: [logEntry('info', '任务已启动。')],
    };
    setState(started);
    try {
      const taskId = await startExtraction(inputPath);
      setState((current) => current.active && current.taskId === null
        ? { ...current, taskId }
        : current);
    } catch (error) {
      const safeError = error as AppErrorDto;
      const cancelled = safeError.code === 'cancelled';
      setState((current) => ({
        ...current,
        active: false,
        stage: cancelled ? 'cancelled' : 'failed',
        logs: appendLog(current.logs, logEntry(cancelled ? 'info' : 'error', safeError.message)),
        terminal: cancelled ? { type: 'cancelled' } : { type: 'failed', error: safeError },
      }));
    }
  }, []);

  const requestCancel = useCallback(async () => {
    await cancelExtraction();
    setState((current) => ({
      ...current,
      logs: appendLog(current.logs, logEntry('info', '已发送停止请求，正在安全结束。')),
    }));
  }, []);

  const reset = useCallback(() => {
    setState(emptyState());
  }, []);

  return {
    active: state.active,
    stage: state.stage,
    completedChunks: state.completedChunks,
    totalChunks: state.totalChunks,
    logs: state.logs,
    terminal: state.terminal,
    start,
    requestCancel,
    reset,
  };
}

function stateFromStatus(status: TaskStatus): InternalTaskState {
  let terminal: TerminalResult | null = null;
  if (!status.active && status.stage === 'completed'
    && typeof status.outputPath === 'string'
    && typeof status.recordCount === 'number') {
    terminal = {
      type: 'completed', outputPath: status.outputPath, recordCount: status.recordCount,
    };
  } else if (!status.active && status.stage === 'cancelled') {
    terminal = { type: 'cancelled' };
  } else if (!status.active && status.stage === 'failed') {
    terminal = {
      type: 'failed',
      error: status.error ?? {
        code: 'parse_failed', message: '任务状态恢复失败。', detail: null,
      },
    };
  }
  return {
    taskId: status.taskId,
    active: status.active,
    stage: status.stage,
    completedChunks: status.completedChunks,
    totalChunks: status.totalChunks,
    logs: [],
    terminal,
  };
}

function reduceTaskEvent(
  current: InternalTaskState,
  event: TaskEvent,
  eventLog: LogEntry | null,
): InternalTaskState {
  if (current.taskId !== null && event.taskId !== current.taskId) return current;
  if (current.taskId === null && !current.active) return current;
  const base = current.taskId === null ? { ...current, taskId: event.taskId } : current;
  switch (event.type) {
    case 'stage':
      return { ...base, stage: event.stage };
    case 'progress':
      return {
        ...base, completedChunks: event.completedChunks, totalChunks: event.totalChunks,
      };
    case 'log':
      return { ...base, logs: appendLog(base.logs, eventLog) };
    case 'completed':
      return {
        ...base,
        active: false,
        stage: 'completed',
        logs: appendLog(base.logs, eventLog),
        terminal: {
          type: 'completed', outputPath: event.outputPath, recordCount: event.recordCount,
        },
      };
    case 'cancelled':
      return {
        ...base,
        active: false,
        stage: 'cancelled',
        logs: appendLog(base.logs, eventLog),
        terminal: { type: 'cancelled' },
      };
    case 'failed':
      return {
        ...base,
        active: false,
        stage: 'failed',
        logs: appendLog(base.logs, eventLog),
        terminal: { type: 'failed', error: event.error },
      };
  }
}

function taskEventLog(event: TaskEvent): LogEntry | null {
  switch (event.type) {
    case 'log':
      return logEntry(event.level, event.message);
    case 'completed':
      return logEntry('success', `已生成 ${event.recordCount} 条记录。`);
    case 'cancelled':
      return logEntry('info', '任务已取消，未保存部分结果。');
    case 'failed':
      return logEntry('error', event.error.message);
    default:
      return null;
  }
}

function isTerminalEvent(event: TaskEvent): boolean {
  return event.type === 'completed' || event.type === 'cancelled' || event.type === 'failed';
}
