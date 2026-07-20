import {
  Alert,
  Button,
  Group,
  Modal,
  Progress,
  Stack,
  Text,
  Title,
} from '@mantine/core';
import { IconAlertCircle, IconDownload, IconRefresh } from '@tabler/icons-react';
import { type JSX, useEffect, useMemo, useState } from 'react';
import {
  downloadUpdate,
  installDownloadedUpdate,
  listenForUpdateDownloadEvents,
  relaunchApp,
} from '../api/tauri';
import type { AppErrorDto, UpdateDownloadEvent, UpdateInfoDto } from '../api/types';

interface UpdateModalProps {
  opened: boolean;
  onClose: () => void;
  taskActive: boolean;
  update: UpdateInfoDto | null;
}

type UpdatePhase =
  | 'idle'
  | 'downloading'
  | 'downloaded'
  | 'installing'
  | 'installed'
  | 'manual_opened'
  | 'restarting';

interface DownloadProgress {
  contentLength: number | null;
  downloaded: number;
  finished: boolean;
}

const emptyProgress: DownloadProgress = {
  contentLength: null,
  downloaded: 0,
  finished: false,
};

export function UpdateModal({
  opened,
  onClose,
  taskActive,
  update,
}: UpdateModalProps): JSX.Element {
  const [phase, setPhase] = useState<UpdatePhase>('idle');
  const [progress, setProgress] = useState<DownloadProgress>(emptyProgress);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [listenerReady, setListenerReady] = useState(false);

  useEffect(() => {
    if (!opened) return undefined;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenForUpdateDownloadEvents((event) => {
      if (!disposed) setProgress((current) => reduceProgress(current, event));
    }).then((dispose) => {
      if (disposed) {
        dispose();
      } else {
        unlisten = dispose;
        setListenerReady(true);
      }
    }).catch(() => {
      if (!disposed) {
        setErrorMessage('无法订阅更新进度，请关闭窗口后重试。');
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [opened]);

  const percent = useMemo(() => progressPercent(progress), [progress]);
  if (!opened || !update?.available || !update.version) return <></>;

  const busy = phase === 'downloading' || phase === 'installing' || phase === 'restarting';
  const manualDeb = update.installMode === 'manual_deb';
  const expectedVersion = update.version;

  const beginInstall = async () => {
    const prompt = manualDeb
      ? '将打开 Hummingbird 公开发布页面，请确认继续。'
      : `将下载并验证 Hummingbird ${update.version}，请确认继续。`;
    if (!window.confirm(prompt)) return;
    setErrorMessage(null);
    setProgress(emptyProgress);
    setPhase('downloading');
    try {
      const result = await downloadUpdate(expectedVersion);
      setPhase(result === 'downloaded' ? 'downloaded' : 'manual_opened');
    } catch (error) {
      setErrorMessage((error as AppErrorDto).message ?? '更新失败，请稍后重试。');
      setPhase('idle');
    }
  };

  const install = async () => {
    if (!window.confirm('更新包已下载并通过签名验证，是否现在安装？安装过程可能关闭应用。')) {
      return;
    }
    setErrorMessage(null);
    setPhase('installing');
    try {
      await installDownloadedUpdate(expectedVersion);
      setPhase('installed');
    } catch (error) {
      setErrorMessage((error as AppErrorDto).message ?? '安装失败，请重新下载后再试。');
      setPhase('idle');
    }
  };

  const restart = async () => {
    if (!window.confirm('更新已安装，是否立即重启 Hummingbird？')) return;
    setErrorMessage(null);
    setPhase('restarting');
    try {
      await relaunchApp();
    } catch (error) {
      setErrorMessage((error as AppErrorDto).message ?? '无法安全重启，请稍后重试。');
      setPhase('installed');
    }
  };

  return (
    <Modal
      centered
      closeOnClickOutside={!busy}
      closeOnEscape={!busy}
      onClose={() => {
        if (!busy) onClose();
      }}
      opened
      title="发现新版本"
      transitionProps={{ duration: 0 }}
    >
      <Stack gap="md">
        <div>
          <Title order={4}>{update.currentVersion} → {update.version}</Title>
          {update.publishedAt ? (
            <Text c="dimmed" mt={3} size="sm">{formatPublishedDate(update.publishedAt)}</Text>
          ) : null}
        </div>

        {update.notes ? (
          <Text component="div" size="sm" style={{ whiteSpace: 'pre-wrap' }}>
            {update.notes}
          </Text>
        ) : (
          <Text c="dimmed" size="sm">此版本没有提供更新说明。</Text>
        )}

        {manualDeb ? (
          <Alert color="blue" icon={<IconAlertCircle size={17} />}>
            DEB 安装需要手动升级。点击后将打开公开发布页面，请下载最新 DEB 并使用系统包管理器安装。
          </Alert>
        ) : null}
        {taskActive ? (
          <Alert color="yellow" icon={<IconAlertCircle size={17} />}>
            当前提取任务结束后才能安装更新。
          </Alert>
        ) : null}
        {errorMessage ? (
          <Alert color="red" icon={<IconAlertCircle size={17} />}>{errorMessage}</Alert>
        ) : null}

        {phase === 'downloading' ? (
          <Stack gap={5}>
            <Progress animated={!progress.finished} value={percent ?? 100} />
            <Text c="dimmed" size="xs">
              {percent === null ? '正在下载更新…' : `${percent}%`}
            </Text>
          </Stack>
        ) : null}
        {phase === 'manual_opened' ? (
          <Alert color="teal">发布页面已打开</Alert>
        ) : null}
        {phase === 'downloaded' || phase === 'installing' ? (
          <Alert color="teal">更新包已下载并通过签名验证，等待安装确认。</Alert>
        ) : null}
        {phase === 'installed' ? (
          <Alert color="teal">更新已安装，重启后即可使用新版本。</Alert>
        ) : null}

        <Group justify="flex-end">
          <Button disabled={busy} onClick={onClose} variant="default">稍后</Button>
          {phase === 'installed' || phase === 'restarting' ? (
            <Button
              leftSection={<IconRefresh size={16} />}
              loading={phase === 'restarting'}
              onClick={() => void restart()}
            >
              立即重启
            </Button>
          ) : phase === 'downloaded' || phase === 'installing' ? (
            <Button
              leftSection={<IconDownload size={16} />}
              loading={phase === 'installing'}
              onClick={() => void install()}
            >
              安装更新
            </Button>
          ) : (
            <Button
              disabled={
                taskActive
                || phase === 'manual_opened'
                || (!manualDeb && !listenerReady)
              }
              leftSection={<IconDownload size={16} />}
              loading={phase === 'downloading'}
              onClick={() => void beginInstall()}
            >
              {manualDeb
                ? '打开下载页面'
                : listenerReady
                  ? '下载更新'
                  : '正在准备更新…'}
            </Button>
          )}
        </Group>
      </Stack>
    </Modal>
  );
}

function reduceProgress(
  current: DownloadProgress,
  event: UpdateDownloadEvent,
): DownloadProgress {
  switch (event.type) {
    case 'started':
      return { contentLength: event.contentLength, downloaded: 0, finished: false };
    case 'chunk':
      return { ...current, downloaded: current.downloaded + event.chunkLength };
    case 'finished':
      return { ...current, finished: true };
  }
}

function progressPercent(progress: DownloadProgress): number | null {
  if (progress.finished) return 100;
  if (!progress.contentLength || progress.contentLength <= 0) return null;
  return Math.min(100, Math.round((progress.downloaded / progress.contentLength) * 100));
}

function formatPublishedDate(value: string): string {
  const match = /^\d{4}-\d{2}-\d{2}/.exec(value);
  return match?.[0] ?? value;
}
