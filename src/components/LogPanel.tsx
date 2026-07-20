import { Badge, Group, ScrollArea, Stack, Text } from '@mantine/core';
import type { JSX } from 'react';

export type LogLevel = 'debug' | 'info' | 'success' | 'warn' | 'error';

export interface LogEntry {
  id: number;
  timestamp: string;
  level: LogLevel;
  message: string;
}

const levelMeta: Record<LogLevel, { label: string; color: string }> = {
  debug: { label: '调试', color: 'gray' },
  info: { label: '信息', color: 'blue' },
  success: { label: '完成', color: 'teal' },
  warn: { label: '警告', color: 'yellow' },
  error: { label: '错误', color: 'red' },
};

export function LogPanel({ entries }: { entries: LogEntry[] }): JSX.Element {
  const visibleEntries = entries.slice(-500);

  return (
    <section className="log-panel" aria-label="处理日志">
      <Group justify="space-between" px="md" py="sm">
        <Text fw={600} size="sm">处理日志</Text>
        <Text c="dimmed" size="xs">最近 {visibleEntries.length} 条</Text>
      </Group>
      <ScrollArea className="log-panel__scroll" type="auto" offsetScrollbars>
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
