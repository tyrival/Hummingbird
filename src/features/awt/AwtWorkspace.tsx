import {
  Badge,
  Button,
  Card,
  Group,
  Text,
  Title,
} from '@mantine/core';
import { notifications } from '@mantine/notifications';
import {
  IconFileDescription,
  IconFolderOpen,
  IconSettings,
  IconUpload,
} from '@tabler/icons-react';
import { type JSX, useEffect, useRef, useState } from 'react';
import {
  listenForInputDropResults,
  openOutputDirectory,
  openTaskOutputDirectory,
  selectInputFile,
} from '../../api/tauri';
import type { AppErrorDto, SelectedInputDto } from '../../api/types';
import { LogPanel, type TaskControlState, type TaskTerminal } from '../../components/LogPanel';
import { useExtractionTask } from './useExtractionTask';

interface AwtWorkspaceProps {
  outputDirectory: string;
  onOpenSettings: () => void;
  onTaskActiveChange: (active: boolean) => void;
}

export function AwtWorkspace({
  outputDirectory,
  onOpenSettings,
  onTaskActiveChange,
}: AwtWorkspaceProps): JSX.Element {
  const [selectedInput, setSelectedInput] = useState<SelectedInputDto | null>(null);
  const [inputConsumed, setInputConsumed] = useState(false);
  const task = useExtractionTask(onTaskActiveChange);
  const taskActiveRef = useRef(task.active);
  const resetTask = task.reset;

  useEffect(() => {
    taskActiveRef.current = task.active;
  }, [task.active]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenForInputDropResults((result) => {
      if (disposed) return;
      if (result.status === 'success') {
        if (taskActiveRef.current) return;
        setSelectedInput(result.input);
        setInputConsumed(false);
        resetTask();
      } else {
        showError(result.error, '无法使用拖入的文件');
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => {
      // Native event registration is unavailable in browser-only previews.
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [resetTask]);

  const chooseInput = async () => {
    if (task.active) return;
    try {
      const selected = await selectInputFile();
      if (selected) {
        setSelectedInput(selected);
        setInputConsumed(false);
        task.reset();
      }
    } catch (error) {
      showError(error as AppErrorDto, '文件选择失败');
    }
  };

  const startOrStop = async () => {
    if (task.active) {
      if (!window.confirm('确定停止当前提取任务吗？未完成的结果不会保存。')) return;
      try {
        await task.requestCancel();
      } catch (error) {
        showError(error as AppErrorDto, '停止任务失败');
      }
      return;
    }
    if (!selectedInput) return;
    setInputConsumed(true);
    await task.start(selectedInput.path);
  };

  const openOutput = async () => {
    try {
      await openOutputDirectory();
    } catch (error) {
      showError(error as AppErrorDto, '无法打开输出目录');
    }
  };

  const openCompletedOutput = async (outputPath: string) => {
    try {
      await openTaskOutputDirectory(outputPath);
    } catch (error) {
      showError(error as AppErrorDto, '无法打开结果目录');
    }
  };

  const progressValue = task.totalChunks > 0
    ? (task.completedChunks / task.totalChunks) * 100
    : 0;

  const terminal: TaskTerminal | null = (() => {
    if (task.terminal?.type === 'completed') {
      return {
        type: 'completed',
        outputPath: task.terminal.outputPath,
        recordCount: task.terminal.recordCount,
      };
    }
    if (task.terminal?.type === 'cancelled') {
      return { type: 'cancelled' };
    }
    if (task.terminal?.type === 'failed') {
      return { type: 'failed', message: task.terminal.error.message };
    }
    return null;
  })();

  const taskControl: TaskControlState = {
    active: task.active,
    canStart: !!selectedInput && !inputConsumed,
    stage: task.stage,
    completedChunks: task.completedChunks,
    totalChunks: task.totalChunks,
    progressValue,
    terminal,
  };

  return (
    <main className="awt-workspace app-workspace" aria-labelledby="awt-heading">
      <header className="workspace-header">
        <div>
          <Text c="blue" fw={700} size="xs" tt="uppercase">Register template workspace</Text>
          <Title id="awt-heading" order={2}>AWT模板生成</Title>
          <Text c="dimmed" mt={4} size="sm">
            从设备说明书或寄存器表提取参数，并生成标准 12 列 CSV 模板。
          </Text>
        </div>
        <Button
          aria-label="设置"
          leftSection={<IconSettings size={17} />}
          onClick={onOpenSettings}
          variant="subtle"
        >
          设置
        </Button>
      </header>

      <Card className="input-card" padding="lg" radius="lg" withBorder>
        <Group align="flex-start" wrap="nowrap">
          <div className="input-card__icon"><IconUpload aria-hidden size={24} /></div>
          <div className="input-card__content">
            <Text fw={650}>选择输入文档</Text>
            <Text c="dimmed" mt={3} size="sm">
              支持 PDF、DOCX、XLS、XLSX、CSV，DOC 请先另存为 DOCX；单个文件不超过 50 MB，也可拖入窗口。
            </Text>
            {selectedInput && (
              <div className="selected-file" data-testid="selected-file">
                <IconFileDescription aria-hidden size={20} />
                <div className="selected-file__details">
                  <Text fw={600} lineClamp={1} size="sm">{selectedInput.fileName}</Text>
                  <Text c="dimmed" className="selected-file__path" size="xs">
                    {selectedInput.path}
                  </Text>
                </div>
                <Badge color="gray" variant="light">{fileFormat(selectedInput.fileName)}</Badge>
                <Text c="dimmed" size="xs">{formatBytes(selectedInput.sizeBytes)}</Text>
              </div>
            )}
          </div>
          <Button disabled={task.active} miw={100} onClick={() => void chooseInput()} variant="light">
            选择文件
          </Button>
        </Group>
      </Card>

      <LogPanel
        entries={task.logs}
        onOpenOutput={openCompletedOutput}
        onStartStop={() => void startOrStop()}
        task={taskControl}
      />

      <footer className="workspace-footer">
        <Text c="dimmed" size="xs">输出目录</Text>
        <Text className="workspace-footer__path" ff="monospace" size="xs">{outputDirectory}</Text>
        <Button
          leftSection={<IconFolderOpen size={16} />}
          onClick={() => void openOutput()}
          size="compact-sm"
          variant="subtle"
        >
          打开输出目录
        </Button>
      </footer>
    </main>
  );
}

function showError(error: AppErrorDto, title: string): void {
  notifications.show({ title, message: error.message, color: 'red' });
}

function fileFormat(fileName: string): string {
  const extension = fileName.split('.').pop();
  return extension ? extension.toUpperCase() : 'FILE';
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
