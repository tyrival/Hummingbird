import {
  Button,
  Checkbox,
  Group,
  Modal,
  Progress,
  Stack,
  Text,
} from '@mantine/core';
import {
  IconCheck,
  IconDownload,
  IconPlayerStop,
  IconX,
} from '@tabler/icons-react';
import { type JSX, useCallback, useMemo, useRef, useState } from 'react';
import { downloadLogs } from '../../api/tauri';
import type { RemoteFile, SshServerConfig } from '../../api/types';

interface DownloadModalProps {
  opened: boolean;
  server: SshServerConfig | null;
  remoteFiles: RemoteFile[];
  onClose: () => void;
  onDownloaded: (paths: string[]) => void;
}

type FileStatus = 'idle' | 'downloading' | 'done' | 'error';

export function DownloadModal({
  opened,
  server,
  remoteFiles,
  onClose,
  onDownloaded,
}: DownloadModalProps): JSX.Element {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [states, setStates] = useState<Map<string, FileStatus>>(new Map());
  const [downloading, setDownloading] = useState(false);
  const stopRef = useRef(false);

  const sortedFiles = useMemo(() => {
    const extractParts = (name: string) => {
      const parts = name.replace(/\.gz$/, '').split('.');
      return parts.map((p) => (/^\d+$/.test(p) ? parseInt(p, 10) : p));
    };
    return [...remoteFiles].sort((a, b) => {
      const aIsGz = a.name.endsWith('.gz');
      const bIsGz = b.name.endsWith('.gz');
      if (aIsGz !== bIsGz) return aIsGz ? 1 : -1; // .log before .gz
      // Both same type: natural descending by suffix parts
      const aParts = extractParts(a.name);
      const bParts = extractParts(b.name);
      const len = Math.max(aParts.length, bParts.length);
      for (let i = 0; i < len; i++) {
        const pa = aParts[i];
        const pb = bParts[i];
        if (pa === undefined) return -1;
        if (pb === undefined) return 1;
        if (typeof pa === 'number' && typeof pb === 'number') {
          if (pa !== pb) return pb - pa; // descending
        } else {
          const cmp = String(pb).localeCompare(String(pa));
          if (cmp !== 0) return cmp;
        }
      }
      return 0;
    });
  }, [remoteFiles]);

  const toggleFile = useCallback((name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const toggleAll = useCallback(() => {
    setSelected((prev) => {
      if (prev.size === sortedFiles.length) return new Set();
      return new Set(sortedFiles.map((f) => f.name));
    });
  }, [sortedFiles]);

  const allSelected = selected.size === sortedFiles.length && sortedFiles.length > 0;

  const handleDownload = useCallback(async () => {
    if (!server || selected.size === 0) return;
    setDownloading(true);
    stopRef.current = false;

    const selectedNames = sortedFiles
      .map((f) => f.name)
      .filter((name) => selected.has(name));
    const downloadedPaths: string[] = [];
    const newStates = new Map(states);

    for (const name of selectedNames) {
      if (stopRef.current) break;
      newStates.set(name, 'downloading');
      setStates(new Map(newStates));

      try {
        const paths = await downloadLogs(server, [name]);
        downloadedPaths.push(...paths);
        newStates.set(name, 'done');
      } catch {
        newStates.set(name, 'error');
      }
      setStates(new Map(newStates));
    }

    setDownloading(false);
    if (downloadedPaths.length > 0) {
      onDownloaded(downloadedPaths);
    }
  }, [server, selected, states, sortedFiles, onDownloaded]);

  const handleStop = useCallback(() => {
    stopRef.current = true;
  }, []);

  const doneCount = [...states.values()].filter((s) => s === 'done').length;
  const errorCount = [...states.values()].filter((s) => s === 'error').length;
  const total = selected.size;
  const hasFinished = doneCount + errorCount >= total && total > 0;

  const statusIcon = (s: FileStatus) => {
    if (s === 'downloading') return <Text c="blue" size="xs">⏳</Text>;
    if (s === 'done') return <IconCheck color="var(--mantine-color-teal-6)" size={16} />;
    if (s === 'error') return <IconX color="var(--mantine-color-red-6)" size={16} />;
    return null;
  };

  return (
    <Modal
      onClose={() => { if (!downloading) onClose(); }}
      opened={opened}
      size="md"
      title="下载日志"
    >
      <Stack gap="sm">
        {sortedFiles.length === 0 ? (
          <Text c="dimmed" size="sm" ta="center">暂无远程日志文件</Text>
        ) : (
          <>
            <Group gap="xs">
              <Checkbox
                checked={allSelected}
                disabled={downloading}
                label={`全选 (${selected.size}/${sortedFiles.length})`}
                onChange={toggleAll}
                size="sm"
              />
            </Group>
            <Stack gap={4} style={{ maxHeight: 360, overflowY: 'auto' }}>
              {sortedFiles.map((f) => (
                <Group
                  gap="xs"
                  key={f.name}
                  style={{ padding: '4px 0' }}
                  wrap="nowrap"
                >
                  <Checkbox
                    checked={selected.has(f.name)}
                    disabled={downloading}
                    onChange={() => toggleFile(f.name)}
                    size="sm"
                  />
                  <Text size="sm" style={{ flex: 1 }} truncate>{f.name}</Text>
                  <span style={{ width: 20, textAlign: 'center', flexShrink: 0 }}>
                    {statusIcon(states.get(f.name) ?? 'idle')}
                  </span>
                </Group>
              ))}
            </Stack>
            {downloading && (
              <Progress
                animated
                size="sm"
                value={((doneCount + errorCount) / total) * 100}
              />
            )}
            <Group justify="space-between">
              <Text c="dimmed" size="xs">
                {downloading
                  ? `已完成 ${doneCount}，失败 ${errorCount}，共 ${total}`
                  : hasFinished
                    ? `下载完成：${doneCount} 成功，${errorCount} 失败`
                    : `已选 ${selected.size} 个文件`}
              </Text>
              <Group gap="xs">
                {downloading ? (
                  <Button
                    color="red"
                    leftSection={<IconPlayerStop size={16} />}
                    onClick={handleStop}
                    size="compact-sm"
                    variant="default"
                  >
                    停止
                  </Button>
                ) : (
                  <Button
                    disabled={selected.size === 0}
                    leftSection={<IconDownload size={16} />}
                    onClick={() => void handleDownload()}
                    size="compact-sm"
                  >
                    下载
                  </Button>
                )}
              </Group>
            </Group>
          </>
        )}
      </Stack>
    </Modal>
  );
}
