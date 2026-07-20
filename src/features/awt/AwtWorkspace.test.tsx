import { MantineProvider } from '@mantine/core';
import { Notifications } from '@mantine/notifications';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as tauriApi from '../../api/tauri';
import type { InputDropResult, TaskEvent } from '../../api/types';
import { AwtWorkspace } from './AwtWorkspace';

vi.mock('../../api/tauri', () => ({
  cancelExtraction: vi.fn(),
  getTaskStatus: vi.fn(),
  listenForInputDropResults: vi.fn(),
  listenForTaskEvents: vi.fn(),
  normalizeAppError: vi.fn((error) => error),
  openOutputDirectory: vi.fn(),
  openTaskOutputDirectory: vi.fn(),
  selectInputFile: vi.fn(),
  startExtraction: vi.fn(),
}));

const api = vi.mocked(tauriApi);
let taskListener: ((event: TaskEvent) => void) | undefined;
let dropListener: ((result: InputDropResult) => void) | undefined;

function renderWorkspace(onTaskActiveChange = vi.fn()) {
  return render(
    <MantineProvider>
      <Notifications />
      <AwtWorkspace
        outputDirectory="output"
        onOpenSettings={vi.fn()}
        onTaskActiveChange={onTaskActiveChange}
      />
    </MantineProvider>,
  );
}

describe('AwtWorkspace', () => {
  beforeEach(() => {
    vi.resetAllMocks();
    taskListener = undefined;
    dropListener = undefined;
    api.getTaskStatus.mockResolvedValue({
      taskId: null,
      active: false,
      completedChunks: 0,
      totalChunks: 0,
      stage: null,
    });
    api.listenForTaskEvents.mockImplementation(async (listener) => {
      taskListener = listener;
      return () => undefined;
    });
    api.listenForInputDropResults.mockImplementation(async (listener) => {
      dropListener = listener;
      return () => undefined;
    });
  });

  it('uses the native picker and renders selected path, format and size', async () => {
    api.selectInputFile.mockResolvedValue({
      path: '/Users/test/说明书.pdf',
      fileName: '说明书.pdf',
      sizeBytes: 1536,
    });
    renderWorkspace();

    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));

    expect(await screen.findByText('/Users/test/说明书.pdf')).toBeInTheDocument();
    expect(screen.getByText('PDF')).toBeInTheDocument();
    expect(screen.getByText(/1\.5 KB/)).toBeInTheDocument();
    expect(document.querySelector('input[type="file"]')).toBeNull();
  });

  it('consumes authorized OS drop results and shows invalid-file errors', async () => {
    renderWorkspace();
    await waitFor(() => expect(dropListener).toBeDefined());

    act(() => {
      dropListener?.({
        status: 'success',
        input: { path: '/tmp/device.xlsx', fileName: 'device.xlsx', sizeBytes: 2048 },
      });
    });
    expect(screen.getByText('/tmp/device.xlsx')).toBeInTheDocument();

    act(() => {
      dropListener?.({
        status: 'error',
        error: { code: 'unsupported_format', message: '不支持该文件格式。', detail: null },
      });
    });
    expect(await screen.findByText('不支持该文件格式。')).toBeInTheDocument();
  });

  it('shows active stage and chunk progress, then completed output action', async () => {
    api.selectInputFile.mockResolvedValue({
      path: '/tmp/manual.csv',
      fileName: 'manual.csv',
      sizeBytes: 1024,
    });
    api.startExtraction.mockResolvedValue('task-1');
    const onTaskActiveChange = vi.fn();
    renderWorkspace(onTaskActiveChange);
    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    await screen.findByText('/tmp/manual.csv');
    fireEvent.click(screen.getByRole('button', { name: '开始提取' }));

    expect(await screen.findByRole('button', { name: '停止' })).toBeInTheDocument();
    expect(onTaskActiveChange).toHaveBeenCalledWith(true);
    act(() => {
      taskListener?.({ type: 'stage', taskId: 'task-1', stage: 'calling_ai' });
      taskListener?.({
        type: 'progress',
        taskId: 'task-1',
        completedChunks: 2,
        totalChunks: 5,
      });
    });
    expect(screen.getByText('正在调用 AI')).toBeInTheDocument();
    expect(screen.getByText('2 / 5 块')).toBeInTheDocument();
    act(() => {
      taskListener?.({
        type: 'completed',
        taskId: 'task-1',
        outputPath: '/tmp/output/manual.csv',
        recordCount: 37,
      });
    });

    expect(screen.getByText(/处理完成/)).toBeInTheDocument();
    expect(screen.getAllByText(/已生成 37 条记录/)).toHaveLength(2);
    fireEvent.click(screen.getByRole('button', { name: '打开目录' }));
    expect(api.openTaskOutputDirectory).toHaveBeenCalledWith('/tmp/output/manual.csv');
    expect(api.openOutputDirectory).not.toHaveBeenCalled();
    expect(onTaskActiveChange).toHaveBeenLastCalledWith(false);
  });

  it('does not reuse the one-shot native input authorization after a task starts', async () => {
    api.selectInputFile.mockResolvedValue({
      path: '/tmp/manual.csv', fileName: 'manual.csv', sizeBytes: 1024,
    });
    api.startExtraction.mockResolvedValue('task-1');
    renderWorkspace();
    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    await screen.findByText('/tmp/manual.csv');
    fireEvent.click(screen.getByRole('button', { name: '开始提取' }));
    await screen.findByRole('button', { name: '停止' });
    act(() => taskListener?.({
      type: 'completed', taskId: 'task-1', outputPath: '/tmp/output/manual.csv', recordCount: 1,
    }));

    const startAgain = screen.getByRole('button', { name: '开始提取' });
    expect(startAgain).toBeDisabled();
    fireEvent.click(startAgain);
    expect(api.startExtraction).toHaveBeenCalledOnce();
  });

  it('asks before stopping and treats cancellation as a normal result', async () => {
    api.selectInputFile.mockResolvedValue({
      path: '/tmp/manual.csv',
      fileName: 'manual.csv',
      sizeBytes: 1024,
    });
    api.startExtraction.mockResolvedValue('task-1');
    api.cancelExtraction.mockResolvedValue(undefined);
    const confirm = vi.spyOn(window, 'confirm').mockReturnValueOnce(false).mockReturnValueOnce(true);
    renderWorkspace();
    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    await screen.findByText('/tmp/manual.csv');
    fireEvent.click(screen.getByRole('button', { name: '开始提取' }));
    await screen.findByRole('button', { name: '停止' });

    fireEvent.click(screen.getByRole('button', { name: '停止' }));
    expect(api.cancelExtraction).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: '停止' }));
    expect(api.cancelExtraction).toHaveBeenCalledOnce();
    act(() => taskListener?.({ type: 'cancelled', taskId: 'task-1' }));

    expect(screen.getByText('任务已取消，未保存部分结果。')).toBeInTheDocument();
    expect(screen.getByText('任务已取消')).toBeInTheDocument();
    confirm.mockRestore();
  });

  it('shows only the safe failure summary', async () => {
    api.selectInputFile.mockResolvedValue({
      path: '/tmp/manual.csv', fileName: 'manual.csv', sizeBytes: 1,
    });
    api.startExtraction.mockResolvedValue('task-1');
    renderWorkspace();
    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    await screen.findByText('/tmp/manual.csv');
    fireEvent.click(screen.getByRole('button', { name: '开始提取' }));
    await screen.findByRole('button', { name: '停止' });
    act(() => taskListener?.({
      type: 'failed',
      taskId: 'task-1',
      error: { code: 'authentication_failed', message: 'API 密钥无效。', detail: null },
    }));

    expect(screen.getByText(/处理失败.*API 密钥无效/)).toBeInTheDocument();
    expect(screen.getByText('API 密钥无效。')).toBeInTheDocument();
  });

  it('registers the event listener before status and converges buffered events by task id', async () => {
    const calls: string[] = [];
    let resolveStatus: ((status: Awaited<ReturnType<typeof tauriApi.getTaskStatus>>) => void) | undefined;
    api.listenForTaskEvents.mockImplementation(async (listener) => {
      calls.push('listen');
      taskListener = listener;
      return () => undefined;
    });
    api.getTaskStatus.mockImplementation(() => {
      calls.push('status');
      return new Promise((resolve) => {
        resolveStatus = resolve;
      });
    });
    renderWorkspace();
    await waitFor(() => expect(taskListener).toBeDefined());
    act(() => {
      taskListener?.({
        type: 'failed', taskId: 'other-task',
        error: { code: 'network_failed', message: '其他任务错误', detail: null },
      });
      taskListener?.({
        type: 'completed', taskId: 'task-1', outputPath: '/tmp/output/recovered.csv', recordCount: 9,
      });
      resolveStatus?.({
        taskId: 'task-1', active: true, completedChunks: 1, totalChunks: 2, stage: 'calling_ai',
      });
    });

    expect(calls).toEqual(['listen', 'status']);
    expect(await screen.findAllByText(/已生成 9 条记录/)).toHaveLength(2);
    expect(screen.queryByText('其他任务错误')).not.toBeInTheDocument();
  });

  it('recovers a completed terminal snapshot even when its event was already missed', async () => {
    api.getTaskStatus.mockResolvedValue({
      taskId: 'task-previous', active: false, completedChunks: 4, totalChunks: 4,
      stage: 'completed', outputPath: '/tmp/output/recovered.csv', recordCount: 12, error: null,
    });
    renderWorkspace();

    expect(await screen.findByText(/已生成 12 条记录/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: '打开目录' }));
    expect(api.openTaskOutputDirectory).toHaveBeenCalledWith('/tmp/output/recovered.csv');
  });

  it('ignores a mismatched terminal event without changing the parent active state', async () => {
    api.getTaskStatus.mockResolvedValue({
      taskId: 'task-1', active: true, completedChunks: 1, totalChunks: 2, stage: 'calling_ai',
    });
    const onTaskActiveChange = vi.fn();
    renderWorkspace(onTaskActiveChange);
    await screen.findByRole('button', { name: '停止' });
    onTaskActiveChange.mockClear();

    act(() => taskListener?.({
      type: 'failed', taskId: 'other-task',
      error: { code: 'network_failed', message: '其他任务错误', detail: null },
    }));

    expect(screen.getByRole('button', { name: '停止' })).toBeInTheDocument();
    expect(onTaskActiveChange).not.toHaveBeenCalledWith(false);
    expect(screen.queryByText('其他任务错误')).not.toBeInTheDocument();
  });

  it('uses buffered terminal events when status initialization fails', async () => {
    let rejectStatus: ((error: unknown) => void) | undefined;
    api.getTaskStatus.mockImplementation(() => new Promise((_resolve, reject) => {
      rejectStatus = reject;
    }));
    renderWorkspace();
    await waitFor(() => expect(taskListener).toBeDefined());

    act(() => {
      taskListener?.({
        type: 'completed', taskId: 'task-buffered',
        outputPath: '/tmp/output/buffered.csv', recordCount: 3,
      });
      rejectStatus?.({ code: 'network_failed', message: '状态读取失败。', detail: null });
    });

    expect(await screen.findAllByText(/已生成 3 条记录/)).toHaveLength(2);
    expect(screen.queryByText('状态读取失败。')).not.toBeInTheDocument();
  });

  it('preserves a buffered non-terminal task as active when status is unknown', async () => {
    let rejectStatus: ((error: unknown) => void) | undefined;
    api.getTaskStatus.mockImplementation(() => new Promise((_resolve, reject) => {
      rejectStatus = reject;
    }));
    renderWorkspace();
    await waitFor(() => expect(taskListener).toBeDefined());

    act(() => {
      taskListener?.({
        type: 'stage', taskId: 'task-buffered', stage: 'calling_ai',
      });
      taskListener?.({
        type: 'progress', taskId: 'task-buffered', completedChunks: 2, totalChunks: 5,
      });
      rejectStatus?.({ code: 'network_failed', message: '状态读取失败。', detail: null });
    });

    expect(await screen.findByRole('button', { name: '停止' })).toBeInTheDocument();
    expect(screen.getByText('正在调用 AI')).toBeInTheDocument();
    expect(screen.getByText('2 / 5 块')).toBeInTheDocument();
    expect(screen.getByText('状态读取失败。当前任务状态未知。')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows a safe initialization error when status fails without buffered terminal events', async () => {
    api.getTaskStatus.mockRejectedValue({
      code: 'network_failed', message: '状态读取失败。', detail: null,
    });
    renderWorkspace();

    expect(await screen.findByText(/处理失败.*状态读取失败/)).toBeInTheDocument();
    expect(screen.getByText('状态读取失败。')).toBeInTheDocument();
  });

  it('clears terminal state, stage, progress and logs after selecting a new file', async () => {
    api.selectInputFile
      .mockResolvedValueOnce({ path: '/tmp/old.csv', fileName: 'old.csv', sizeBytes: 1 })
      .mockResolvedValueOnce({ path: '/tmp/new.csv', fileName: 'new.csv', sizeBytes: 2 });
    api.startExtraction.mockRejectedValue({
      code: 'network_failed', message: '网络请求失败。', detail: null,
    });
    renderWorkspace();
    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    await screen.findByText('/tmp/old.csv');
    fireEvent.click(screen.getByRole('button', { name: '开始提取' }));
    expect(await screen.findByText(/处理失败.*网络请求失败/)).toBeInTheDocument();
    expect(screen.getByText('网络请求失败。')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    expect(await screen.findByText('/tmp/new.csv')).toBeInTheDocument();
    expect(screen.queryByText(/处理失败/)).not.toBeInTheDocument();
    expect(screen.getByText('等待开始')).toBeInTheDocument();
    expect(screen.getByText('任务日志将在这里显示')).toBeInTheDocument();
  });

  it('keeps only the newest 500 task log events', async () => {
    api.selectInputFile.mockResolvedValue({
      path: '/tmp/manual.csv', fileName: 'manual.csv', sizeBytes: 1,
    });
    api.startExtraction.mockResolvedValue('task-1');
    renderWorkspace();
    fireEvent.click(screen.getByRole('button', { name: '选择文件' }));
    await screen.findByText('/tmp/manual.csv');
    fireEvent.click(screen.getByRole('button', { name: '开始提取' }));
    await screen.findByRole('button', { name: '停止' });

    act(() => {
      for (let index = 0; index < 501; index += 1) {
        taskListener?.({
          type: 'log', taskId: 'task-1', level: 'info', message: `任务日志-${index}`,
        });
      }
    });

    expect(screen.queryByText('任务日志-0')).not.toBeInTheDocument();
    expect(screen.getByText('任务日志-500')).toBeInTheDocument();
    expect(screen.getAllByRole('listitem')).toHaveLength(500);
  });
});
