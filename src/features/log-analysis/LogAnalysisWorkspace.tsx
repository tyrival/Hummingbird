import {
  Button,
  Card,
  Group,
  Text,
  Title,
} from '@mantine/core';
import { notifications } from '@mantine/notifications';
import {
  IconDownload,
  IconFile,
  IconFolderOpen,
  IconServer,
  IconSettings,
} from '@tabler/icons-react';
import { type JSX, useCallback, useEffect, useRef, useState } from 'react';
import {
  cancelLogAnalysis,
  getAnalyseConfig,
  getSettings,
  listenForAnalyseEvents,
  listRemoteLogs,
  saveSettings,
  selectAnalyseDir,
  selectLogFolder,
  startLogAnalysis,
} from '../../api/tauri';
import type { AnalyseConfig, AnalyseEvent, LogSummary, RemoteFile, SshServerConfig, TimeBucket } from '../../api/types';
import { LogPanel, type TaskControlState, type TaskTerminal } from '../../components/LogPanel';
import { AnalysisResults } from './AnalysisResults';
import { DownloadModal } from './DownloadModal';
import { ServerListModal } from './ServerListModal';

interface LogAnalysisWorkspaceProps {
  logAnalyseDir: string;
  onLogAnalyseDirChange: (dir: string) => void;
  onOpenSettings: () => void;
}

export function LogAnalysisWorkspace({
  logAnalyseDir,
  onLogAnalyseDirChange,
  onOpenSettings,
}: LogAnalysisWorkspaceProps): JSX.Element {
  const [config, setConfig] = useState<AnalyseConfig | null>(null);
  const [serverModalOpen, setServerModalOpen] = useState(false);
  const [downloadModalOpen, setDownloadModalOpen] = useState(false);
  const [selectedServer, setSelectedServer] = useState<SshServerConfig | null>(null);
  const [remoteFiles, setRemoteFiles] = useState<RemoteFile[]>([]);
  const [localPaths, setLocalPaths] = useState<string[]>([]);
  const [analysing, setAnalysing] = useState(false);
  const [analysisComplete, setAnalysisComplete] = useState(false);
  const [progressPct, setProgressPct] = useState(0);
  const [aiReports, setAiReports] = useState<string[]>([]);
  const [summary, setSummary] = useState<LogSummary | null>(null);
  const [heatmap, setHeatmap] = useState<TimeBucket[]>([]);
  const [taskLogs, setTaskLogs] = useState<string[]>([]);

  const unlistenRef = useRef<(() => void) | null>(null);
  const mountedRef = useRef(false);

  useEffect(() => {
    void getAnalyseConfig().then(setConfig);
  }, []);

  useEffect(() => {
    if (mountedRef.current) return;
    mountedRef.current = true;
    void listenForAnalyseEvents((event: AnalyseEvent) => {
      switch (event.type) {
        case 'stage':
          setAnalysing(true);
          setTaskLogs((prev) => [...prev, `[${event.stage}] ${getStageLabel(event.stage)}`]);
          break;
        case 'progress':
          setAnalysing(true);
          setProgressPct(event.completed);
          if (event.detail) setTaskLogs((prev) => [...prev, event.detail]);
          break;
        case 'ai_chunk':
          setAiReports((prev) => [...prev, event.content]);
          setTaskLogs((prev) => [...prev, 'AI 分析完成一个批次']);
          break;
        case 'completed':
          setAnalysing(false);
          setAnalysisComplete(true);
          setProgressPct(100);
          try {
            setSummary(JSON.parse(event.summaryJson) as LogSummary);
            setHeatmap(JSON.parse(event.heatmapJson) as TimeBucket[]);
          } catch { /* ignore parse errors */ }
          setTaskLogs((prev) => [...prev, '分析完成']);
          break;
        case 'cancelled':
          setAnalysing(false);
          setAnalysisComplete(true);
          setTaskLogs((prev) => [...prev, '已取消']);
          break;
        case 'failed':
          setAnalysing(false);
          setTaskLogs((prev) => [...prev, `失败: ${event.error.message}`]);
          notifications.show({
            title: '分析失败',
            message: event.error.message,
            color: 'red',
          });
          break;
      }
    }).then((unlisten) => {
      unlistenRef.current = unlisten;
    });
    return () => {
      unlistenRef.current?.();
    };
  }, []);

  const handleSelectServer = useCallback(async (server: SshServerConfig) => {
    setSelectedServer(server);
    setServerModalOpen(false);
    try {
      const files = await listRemoteLogs(server);
      setRemoteFiles(files);
    } catch (e: unknown) {
      notifications.show({
        title: '连接失败',
        message: (e as { message?: string }).message ?? '无法列出远程文件',
        color: 'red',
      });
      setSelectedServer(null);
    }
  }, []);

  const handleSelectLocal = useCallback(async () => {
    try {
      const paths = await selectLogFolder();
      if (paths.length > 0) {
        setLocalPaths((prev) => [...prev, ...paths]);
      }
    } catch { /* cancelled */ }
  }, []);

  const handleOpenDownload = useCallback(() => {
    setDownloadModalOpen(true);
  }, []);

  const handleDownloaded = useCallback((paths: string[]) => {
    setLocalPaths((prev) => {
      const existing = new Set(prev);
      return [...prev, ...paths.filter((p) => !existing.has(p))];
    });
  }, []);

  const handleChooseAnalyseDir = useCallback(async () => {
    try {
      const dir = await selectAnalyseDir();
      if (dir) {
        onLogAnalyseDirChange(dir);
        const current = await getSettings();
        void saveSettings({ ...current, logAnalyseDir: dir }).catch(() => {});
      }
    } catch { /* cancelled */ }
  }, [onLogAnalyseDirChange]);

  const handleAnalyse = useCallback(async () => {
    if (localPaths.length === 0) return;
    setAiReports([]);
    setSummary(null);
    setHeatmap([]);
    setAnalysisComplete(false);
    setTaskLogs([]);
    try {
      await startLogAnalysis(localPaths);
    } catch (e: unknown) {
      notifications.show({
        title: '启动分析失败',
        message: (e as { message?: string }).message ?? '未知错误',
        color: 'red',
      });
    }
  }, [localPaths]);

  const handleCancel = useCallback(async () => {
    try { await cancelLogAnalysis(); } catch { /* ignore */ }
  }, []);

  const startOrStop = () => {
    if (analysing) void handleCancel();
    else void handleAnalyse();
  };

  const progressValue = progressPct;

  const folderName = (p: string) => {
    const i = p.lastIndexOf('/');
    return i >= 0 ? p.substring(0, i) : p;
  };

  const terminal: TaskTerminal | null = analysisComplete
    ? { type: 'completed', outputPath: '', recordCount: aiReports.length }
    : null;

  const taskControl: TaskControlState = {
    active: analysing,
    canStart: localPaths.length > 0,
    stage: analysing ? 'calling_ai' : null,
    completedChunks: progressPct,
    totalChunks: 100,
    progressValue,
    terminal,
  };

  return (
    <main aria-label="平台日志分析" className="awt-workspace app-workspace">
      <header className="workspace-header">
        <div>
          <Text c="blue" fw={700} size="xs" tt="uppercase">log analysis workspace</Text>
          <Title order={2}>平台日志分析</Title>
          <Text c="dimmed" mt={4} size="sm">
            从远程服务器或本地加载 iot-exchange 容器日志，进行 AI 辅助分析。
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

      <Card className="input-card" padding="lg" radius="lg" style={{ flexShrink: 0 }} withBorder>
        <Group align="flex-start" wrap="nowrap">
          <div className="input-card__icon"><IconServer aria-hidden size={24} /></div>
          <div className="input-card__content">
            <Text fw={650}>日志数据源</Text>
            <Text c="dimmed" mt={3} size="sm">
              SSH 连接远程服务器获取日志，或直接选择本地 .log 文件。支持 .gz 压缩包自动解压。
            </Text>
            {selectedServer && (
              <div className="selected-file" data-testid="selected-server" style={{ marginTop: 8 }}>
                <IconServer aria-hidden size={20} />
                <div className="selected-file__details">
                  <Text fw={600} lineClamp={1} size="sm">{selectedServer.name}</Text>
                  <Text c="dimmed" className="selected-file__path" size="xs">
                    {selectedServer.user}@{selectedServer.host}:{selectedServer.port} — {selectedServer.appRoot}
                  </Text>
                </div>
                {remoteFiles.length > 0 && (
                  <Button
                    leftSection={<IconDownload size={16} />}
                    miw={120}
                    onClick={handleOpenDownload}
                    variant="default"
                  >
                    下载远程日志
                  </Button>
                )}
              </div>
            )}
            {localPaths.length > 0 && (
              <div className="selected-file">
                <IconFile aria-hidden size={20} />
                <div className="selected-file__details">
                  <Text fw={600} size="sm">共 {localPaths.length} 个日志文件</Text>
                  <Text c="dimmed" className="selected-file__path" size="xs">
                    {folderName(localPaths[0])}
                  </Text>
                </div>
                <Button
                  color="gray"
                  onClick={() => setLocalPaths([])}
                  size="compact-xs"
                  variant="subtle"
                >
                  清除
                </Button>
              </div>
            )}
          </div>
          <Group gap="xs" wrap="nowrap">
            <Button
              miw={100}
              onClick={() => setServerModalOpen(true)}
              variant="default"
            >
              服务器列表
            </Button>
            <Button
              disabled={analysing}
              miw={100}
              onClick={handleSelectLocal}
              variant="default"
            >
              选择本地文件夹
            </Button>
          </Group>
        </Group>
      </Card>

      {analysisComplete && aiReports.length > 0 ? (
        <AnalysisResults
          aiReports={aiReports}
          heatmap={heatmap}
          summary={summary}
        />
      ) : (
        <LogPanel
          buttonLabel="开始分析"
          entries={taskLogs.map((msg, i) => ({
            id: i,
            timestamp: '',
            level: 'info' as const,
            message: msg,
          }))}
          onOpenOutput={() => {}}
          onStartStop={startOrStop}
          task={taskControl}
        />
      )}

      <ServerListModal
        onClose={() => setServerModalOpen(false)}
        onSelect={handleSelectServer}
        onServersChange={(servers: SshServerConfig[]) =>
          setConfig((prev) => prev ? { ...prev, sshServers: servers } : null)
        }
        opened={serverModalOpen}
        servers={config?.sshServers ?? []}
      />
      <DownloadModal
        key={selectedServer?.name ?? '__none__'}
        onClose={() => setDownloadModalOpen(false)}
        onDownloaded={handleDownloaded}
        opened={downloadModalOpen}
        remoteFiles={remoteFiles}
        server={selectedServer}
      />
      <footer className="workspace-footer">
        <Text c="dimmed" size="xs">存储路径</Text>
        <Text className="workspace-footer__path" ff="monospace" size="xs">
          {logAnalyseDir || '~/Hummingbird/analyse'}
        </Text>
        <Button
          leftSection={<IconFolderOpen size={16} />}
          onClick={() => void handleChooseAnalyseDir()}
          size="compact-sm"
          variant="subtle"
        >
          更改
        </Button>
      </footer>
    </main>
  );
}

function getStageLabel(stage: string): string {
  const map: Record<string, string> = {
    parsing: '解析日志文件',
    aggregating: '统计分析',
    ai_analysis: 'AI 分析',
  };
  return map[stage] ?? stage;
}
