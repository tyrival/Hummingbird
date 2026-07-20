import { notifications } from '@mantine/notifications';
import { type JSX, useCallback, useEffect, useRef, useState } from 'react';
import {
  checkForUpdate,
  destroyMainWindow,
  getSettings,
  getTaskStatus,
  listenForCloseRequests,
  prepareExit,
} from './api/tauri';
import type { AppErrorDto, SettingsDto, UpdateInfoDto } from './api/types';
import { AppSidebar, type Workspace } from './components/AppSidebar';
import { SettingsModal } from './components/SettingsModal';
import { UpdateModal } from './components/UpdateModal';
import { AwtWorkspace } from './features/awt/AwtWorkspace';
import { PassthroughWorkspace } from './features/passthrough/PassthroughWorkspace';

const initialSettings: SettingsDto = {
  schemaVersion: 1,
  migrationVersion: 1,
  baseUrl: 'http://192.168.32.20:3000/v1',
  apiKey: '',
  model: 'deepseek-chat',
  timeoutSeconds: 600,
  maxTokens: 16384,
  outputDirectory: 'output',
  chunkMaxChars: 30000,
  contextChars: 3000,
  lastInputDir: null,
};

export default function App(): JSX.Element {
  const [activeWorkspace, setActiveWorkspace] = useState<Workspace>('awt');
  const [settings, setSettings] = useState<SettingsDto>(initialSettings);
  const [settingsOpened, setSettingsOpened] = useState(false);
  const [taskActive, setTaskActive] = useState(false);
  const [update, setUpdate] = useState<UpdateInfoDto | null>(null);
  const [updateOpened, setUpdateOpened] = useState(false);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const closingRef = useRef(false);
  const backgroundUpdateStartedRef = useRef(false);

  const handleTaskActiveChange = useCallback((active: boolean) => {
    setTaskActive(active);
  }, []);

  useEffect(() => {
    let disposed = false;
    void getSettings().then((loaded) => {
      if (!disposed) setSettings(loaded);
    }).catch((error: AppErrorDto) => {
      if (!disposed) {
        notifications.show({ title: '读取设置失败', message: error.message, color: 'red' });
      }
    });
    return () => {
      disposed = true;
    };
  }, []);

  const runUpdateCheck = useCallback(async (manual: boolean) => {
    if (manual) setCheckingUpdate(true);
    try {
      const result = await checkForUpdate(manual);
      if (result.available) {
        setUpdate(result);
        setUpdateOpened(true);
      } else if (manual) {
        notifications.show({
          title: '检查更新',
          message: '当前已是最新版本。',
          color: 'teal',
        });
      }
    } catch (error) {
      if (manual) {
        const safeError = error as AppErrorDto;
        notifications.show({
          title: '检查更新失败',
          message: safeError.message ?? '暂时无法检查更新，请稍后重试。',
          color: 'red',
        });
      }
    } finally {
      if (manual) setCheckingUpdate(false);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    const timer = window.setTimeout(() => {
      if (backgroundUpdateStartedRef.current) return;
      backgroundUpdateStartedRef.current = true;
      void checkForUpdate(false).then((result) => {
        if (!disposed && result.available) {
          setUpdate(result);
          setUpdateOpened(true);
        }
      }).catch(() => {
        // Background checks are deliberately silent.
      });
    }, 3000);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenForCloseRequests(async (event) => {
      event.preventDefault();
      if (closingRef.current) return;
      closingRef.current = true;
      try {
        const status = await getTaskStatus();
        if (status.active) {
          if (!window.confirm('当前提取任务仍在进行，确定停止任务并退出 Hummingbird 吗？')) {
            closingRef.current = false;
            return;
          }
        }
        const exitStatus = await prepareExit();
        if (!exitStatus.safeToExit) {
          throw { code: 'save_failed', message: '任务清理尚未完成。', detail: null } satisfies AppErrorDto;
        }
        await destroyMainWindow();
      } catch (error) {
        const safeError = error as AppErrorDto;
        notifications.show({
          title: '无法安全退出',
          message: safeError.message ?? '任务尚未安全结束，请稍后重试。',
          color: 'red',
        });
        closingRef.current = false;
      }
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => {
      // Browser previews do not provide native close events.
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <div className="app-shell">
      <AppSidebar
        activeWorkspace={activeWorkspace}
        checkingUpdate={checkingUpdate}
        onCheckUpdate={() => void runUpdateCheck(true)}
        onWorkspaceChange={setActiveWorkspace}
        taskActive={taskActive}
      />
      {activeWorkspace === 'awt' ? (
        <AwtWorkspace
          onOpenSettings={() => setSettingsOpened(true)}
          onTaskActiveChange={handleTaskActiveChange}
          outputDirectory={settings.outputDirectory}
        />
      ) : (
        <PassthroughWorkspace />
      )}
      <SettingsModal
        onCheckUpdate={() => {
          setSettingsOpened(false);
          void runUpdateCheck(true);
        }}
        onClose={() => setSettingsOpened(false)}
        onSaved={setSettings}
        opened={settingsOpened}
        settings={settings}
      />
      {updateOpened ? (
        <UpdateModal
          onClose={() => setUpdateOpened(false)}
          opened
          taskActive={taskActive}
          update={update}
        />
      ) : null}
    </div>
  );
}
