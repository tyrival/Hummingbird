import { useCallback, useState } from 'react';
import { cancelPassthroughParse, parsePassthroughMessages } from '../../api/tauri';
import type {
  AppErrorDto,
  PassthroughBatchResult,
  PassthroughParseRequest,
} from '../../api/types';
import type { LogEntry, LogLevel } from '../../components/LogPanel';

export type ParserState = 'idle' | 'extracting_source' | 'parsing' | 'cancelling' | 'completed' | 'cancelled' | 'failed';

function timestamp(): string {
  return new Date().toLocaleTimeString('zh-CN', { hour12: false });
}

function logEntry(id: number, level: LogLevel, message: string): LogEntry {
  return { id, level, message, timestamp: timestamp() };
}

export function usePassthroughParser() {
  const [state, setState] = useState<ParserState>('idle');
  const [result, setResult] = useState<PassthroughBatchResult | null>(null);
  const [error, setError] = useState<AppErrorDto | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);

  const appendLog = useCallback((level: LogLevel, message: string) => {
    setLogs((current) => {
      const id = (current.at(-1)?.id ?? -1) + 1;
      return [...current, logEntry(id, level, message)].slice(-500);
    });
  }, []);

  const parse = useCallback(async (request: PassthroughParseRequest) => {
    const requestCount = request.requestHex.split('&&').filter((part) => part.trim().length > 0).length;
    const responseCount = request.responseHex?.split('&&').filter((part) => part.trim().length > 0).length ?? 0;
    const inputCount = requestCount + responseCount;
    const sourceMessage = request.source?.kind === 'manual'
      ? '正在提取寄存器说明书并生成临时寄存器映射。'
      : request.source?.kind === 'awt_template'
        ? '正在加载 AWT 模板。'
        : '未选择辅助资料，执行确定性协议解析。';

    setState(request.source?.kind === 'manual' ? 'extracting_source' : 'parsing');
    setError(null);
    setResult(null);
    setLogs([
      logEntry(0, 'info', `开始解析，共 ${inputCount} 个输入片段。`),
      logEntry(1, 'info', sourceMessage),
      logEntry(2, 'info', '正在执行协议识别和校验。'),
    ]);
    try {
      const next = await parsePassthroughMessages(request);
      if (next.sourceWarning) appendLog('warn', next.sourceWarning);
      if (next.mappingDiagnostics) {
        appendLog('info', `说明书提取 ${next.mappingDiagnostics.extractedCount} 条寄存器定义，当前报文命中 ${next.mappingDiagnostics.matchedCount} 个地址。`);
        if (next.mappingDiagnostics.unmatchedAddresses.length > 0) {
          appendLog('warn', `未命中地址：${next.mappingDiagnostics.unmatchedAddresses.map((address) => `0x${address.toString(16).toUpperCase().padStart(4, '0')}`).join('、')}`);
        }
      }
      next.results.forEach((message, index) => {
        message.warnings.forEach((warning) => appendLog('warn', `报文 ${index + 1}：${warning.message}`));
      });
      appendLog('success', `解析完成，共生成 ${next.results.length} 个报文结果。`);
      setResult(next);
      setState('completed');
    } catch (caught) {
      const nextError = caught as AppErrorDto;
      if (nextError.code === 'cancelled') {
        appendLog('warn', '解析已取消。');
        setState('cancelled');
      } else {
        appendLog('error', nextError.message);
        setError(nextError);
        setState('failed');
      }
    }
  }, [appendLog]);

  const reject = useCallback((message: string) => {
    const nextError: AppErrorDto = { code: 'invalid_passthrough_input', message, detail: null };
    setResult(null);
    setError(nextError);
    setLogs([logEntry(0, 'error', message)]);
    setState('failed');
  }, []);

  const cancel = useCallback(async () => {
    setState('cancelling');
    try {
      await cancelPassthroughParse();
    } catch (caught) {
      const nextError = caught as AppErrorDto;
      appendLog('error', nextError.message);
      setError(nextError);
      setState('failed');
    }
  }, [appendLog]);

  return { cancel, error, logs, parse, reject, result, state };
}
