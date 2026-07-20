import { Badge, Button, Group, Progress, ScrollArea, Stack, Text } from '@mantine/core';
import {
  IconAlertCircle,
  IconCheck,
  IconFolderOpen,
  IconPlayerPlay,
  IconSquare,
} from '@tabler/icons-react';
import type { JSX } from 'react';
import type { TaskStage } from '../api/types';

export type LogLevel = 'debug' | 'info' | 'success' | 'warn' | 'error';

export interface LogEntry {
  id: number;
  timestamp: string;
  level: LogLevel;
  message: string;
}

export interface TaskTerminal {
  type: 'completed' | 'cancelled' | 'failed';
  outputPath?: string;
  recordCount?: number;
  message?: string;
}

export interface TaskControlState {
  active: boolean;
  canStart: boolean;
  stage: TaskStage | null;
  completedChunks: number;
  totalChunks: number;
  progressValue: number;
  terminal: TaskTerminal | null;
}

interface LogPanelProps {
  entries: LogEntry[];
  task: TaskControlState;
  onStartStop: () => void;
  onOpenOutput?: (path: string) => void;
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

const levelMeta: Record<LogLevel, { label: string; color: string }> = {
  debug: { label: '调试', color: 'gray' },
  info: { label: '信息', color: 'blue' },
  success: { label: '完成', color: 'teal' },
  warn: { label: '警告', color: 'yellow' },
  error: { label: '错误', color: 'red' },
};

function terminalSummary(terminal: TaskTerminal, onOpenOutput?: (path: string) => void): JSX.Element | null {
  switch (terminal.type) {
    case 'completed':
      return (
        <Group gap="xs" wrap="nowrap">
          <IconCheck size={16} style={{ color: 'var(--mantine-color-teal-6)' }} />
          <Text c="teal" size="sm">
            {stageLabels.completed} · 已生成 {terminal.recordCount} 条记录
          </Text>
          {terminal.outputPath && onOpenOutput ? (
            <Button
              color="teal"
              leftSection={<IconFolderOpen size={14} />}
              onClick={() => onOpenOutput(terminal.outputPath!)}
              size="compact-xs"
              variant="light"
            >
              打开目录
            </Button>
          ) : null}
        </Group>
      );
    case 'cancelled':
      return <Text c="gray" size="sm">{stageLabels.cancelled}</Text>;
    case 'failed':
      return (
        <Group gap="xs" wrap="nowrap">
          <IconAlertCircle size={16} style={{ color: 'var(--mantine-color-red-6)' }} />
          <Text c="red" size="sm">
            {stageLabels.failed} · {terminal.message}
          </Text>
        </Group>
      );
  }
}

export function LogPanel({
  entries,
  task,
  onStartStop,
  onOpenOutput,
}: LogPanelProps): JSX.Element {
  const visibleEntries = entries.slice(-500);

  return (
    <section className="log-panel" aria-label="处理日志">
      <Group justify="space-between" px="md" py="xs" wrap="nowrap">
        <Group gap="sm" wrap="nowrap">
          <Button
            color={task.active ? 'red' : 'blue'}
            disabled={!task.active && !task.canStart}
            leftSection={task.active ? <IconSquare size={15} /> : <IconPlayerPlay size={17} />}
            onClick={onStartStop}
            size="compact-sm"
            variant={task.active ? 'light' : 'filled'}
          >
            {task.active ? '停止' : '开始提取'}
          </Button>
          {task.terminal ? (
            terminalSummary(task.terminal, onOpenOutput)
          ) : task.active && task.stage ? (
            <Text size="sm">{stageLabels[task.stage]}</Text>
          ) : (
            <Text c="dimmed" size="sm">等待开始</Text>
          )}
        </Group>
        <Group gap="sm" wrap="nowrap">
          {task.totalChunks > 0 ? (
            <Text c="dimmed" size="xs">
              {task.completedChunks} / {task.totalChunks} 块
            </Text>
          ) : task.active ? (
            <Text c="dimmed" size="xs">进度按实际文档分块更新</Text>
          ) : null}
        </Group>
      </Group>
      <Progress
        aria-label="文档分块进度"
        animated={task.active && task.totalChunks === 0}
        color={task.terminal?.type === 'failed' ? 'red' : 'blue'}
        radius={0}
        size="xs"
        value={task.progressValue}
      />
      <ScrollArea className="log-panel__scroll" style={{ flex: 1, minHeight: 0 }} type="auto" offsetScrollbars>
        {visibleEntries.length === 0 ? (
          <Text c="dimmed" px="md" py="lg" size="sm">任务日志将在这里显示</Text>
        ) : (
          <Stack component="ol" gap={0} m={0} p={0} className="log-panel__list">
            {visibleEntries.map((entry) => {
              const meta = levelMeta[entry.level];
              return (
                <li className="log-panel__entry" key={entry.id}>
                  <Text c="dimmed" component="time" ff="monospace" size="xs">
                    {entry.timestamp}
                  </Text>
                  <Badge color={meta.color} size="xs" variant="light">{meta.label}</Badge>
                  <Text className="log-panel__message" size="xs">{entry.message}</Text>
                </li>
              );
            })}
          </Stack>
        )}
      </ScrollArea>
    </section>
  );
}
