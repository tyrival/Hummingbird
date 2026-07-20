import {
  Alert,
  Badge,
  Button,
  Card,
  Group,
  Progress,
  Text,
  Title,
} from '@mantine/core';
import { notifications } from '@mantine/notifications';
import {
  IconAlertCircle,
  IconCheck,
  IconFileDescription,
  IconFolderOpen,
  IconPlayerPlay,
  IconSettings,
  IconSquare,
  IconUpload,
} from '@tabler/icons-react';
import { type JSX, useEffect, useRef, useState } from 'react';
import {
  listenForInputDropResults,
  openOutputDirectory,
  openTaskOutputDirectory,
  selectInputFile,
} from '../../api/tauri';
import type { AppErrorDto, SelectedInputDto, TaskStage } from '../../api/types';
import { LogPanel } from '../../components/LogPanel';
import { useExtractionTask } from './useExtractionTask';

interface AwtWorkspaceProps {
  outputDirectory: string;
  onOpenSettings: () => void;
  onTaskActiveChange: (active: boolean) => void;
}

const stageLabels: Record<TaskStage, string> = {
  validating_input: '正在校验文件',
  extracting_text: '正在提取文档文字',
  preparing_chunks: '正在准备文档分块',
  calling_ai: '正在调用 AI',
  merging_results: '正在合并寄存器结果',
  saving_output: '正在保存 CSV',
  completed: '处理完成',
  cancelled: '任务已取消',
  failed: '处理失败',
};

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
  const completedResult = task.terminal?.type === 'completed' ? task.terminal : null;

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
          <Button disabled={task.active} onClick={() => void chooseInput()} variant="light">
            选择文件
          </Button>
        </Group>
      </Card>

      <Card className="task-card" padding="lg" radius="lg" withBorder>
        <Group justify="space-between" mb="sm">
          <div>
            <Text fw={650} size="sm">{task.stage ? stageLabels[task.stage] : '等待开始'}</Text>
            <Text c="dimmed" size="xs">
              {task.totalChunks > 0
                ? `${task.completedChunks} / ${task.totalChunks} 块`
                : '进度按实际文档分块更新'}
            </Text>
          </div>
          <Button
            color={task.active ? 'red' : 'blue'}
            disabled={!task.active && (!selectedInput || inputConsumed)}
            leftSection={task.active ? <IconSquare size={15} /> : <IconPlayerPlay size={17} />}
            onClick={() => void startOrStop()}
            variant={task.active ? 'light' : 'filled'}
          >
            {task.active ? '停止' : '开始提取'}
          </Button>
        </Group>
        <Progress
          aria-label="文档分块进度"
          animated={task.active && task.totalChunks === 0}
          color={task.terminal?.type === 'failed' ? 'red' : 'blue'}
          radius="xl"
          size="sm"
          value={progressValue}
        />
      </Card>

      {completedResult && (
        <Alert color="teal" icon={<IconCheck size={18} />} title="模板生成完成">
          <Group justify="space-between" wrap="nowrap">
            <Text size="sm">
              已生成 {completedResult.recordCount} 条记录，保存至 {completedResult.outputPath}
            </Text>
            <Button
              color="teal"
              onClick={() => void openCompletedOutput(completedResult.outputPath)}
              size="xs"
              variant="light"
            >
              打开目录
            </Button>
          </Group>
        </Alert>
      )}
      {task.terminal?.type === 'cancelled' && (
        <Alert color="gray" icon={<IconSquare size={16} />} title="任务已停止">
          任务已取消，未保存部分结果。
        </Alert>
      )}
      {task.terminal?.type === 'failed' && (
        <Alert color="red" icon={<IconAlertCircle size={18} />} role="alert" title="处理失败">
          {task.terminal.error.message}
        </Alert>
      )}

      <LogPanel entries={task.logs} />

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
