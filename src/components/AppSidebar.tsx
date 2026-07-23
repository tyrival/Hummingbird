import {
  IconChartHistogram,
  IconCodeDots,
  IconRefresh,
  IconSettings,
  IconTemplate,
} from '@tabler/icons-react';
import type { JSX } from 'react';

export type Workspace = 'awt' | 'passthrough' | 'log-analysis';

interface AppSidebarProps {
  activeWorkspace: Workspace;
  checkingUpdate: boolean;
  onCheckUpdate: () => void;
  onOpenSettings: () => void;
  onWorkspaceChange: (workspace: Workspace) => void;
  taskActive: boolean;
}

const items = [
  { id: 'awt' as const, label: 'AWT模板生成', icon: IconTemplate },
  { id: 'passthrough' as const, label: '透传报文解析', icon: IconCodeDots },
  { id: 'log-analysis' as const, label: '平台日志分析Beta', icon: IconChartHistogram },
];

export function AppSidebar({
  activeWorkspace,
  checkingUpdate,
  onCheckUpdate,
  onOpenSettings,
  onWorkspaceChange,
  taskActive,
}: AppSidebarProps): JSX.Element {
  return (
    <aside className="app-sidebar" aria-label="工作区导航">
      <div className="app-sidebar__brand">
        <span className="app-sidebar__brand-icon" aria-hidden="true">
          <img alt="" src="/icon.png" />
        </span>
        <h1>Hummingbird</h1>
      </div>
      <nav className="app-sidebar__navigation">
        {items.map(({ id, label, icon: Icon }) => (
          <button
            aria-current={activeWorkspace === id ? 'page' : undefined}
            aria-label={label}
            className="app-sidebar__item"
            disabled={taskActive && id !== 'awt'}
            key={id}
            onClick={() => onWorkspaceChange(id)}
            title={taskActive && id !== 'awt' ? '任务进行中，请先停止任务' : label}
            type="button"
          >
            <Icon aria-hidden="true" size={19} stroke={1.8} />
            <span className="app-sidebar__label">{label}</span>
          </button>
        ))}
      </nav>
      <div className="app-sidebar__footer">
        <button
          aria-label="检查更新"
          className="app-sidebar__item"
          disabled={checkingUpdate}
          onClick={onCheckUpdate}
          title="检查更新"
          type="button"
        >
          <IconRefresh aria-hidden="true" size={19} stroke={1.8} />
          <span className="app-sidebar__label">
            {checkingUpdate ? '正在检查…' : '检查更新'}
          </span>
        </button>
        <button
          aria-label="设置"
          className="app-sidebar__item"
          onClick={onOpenSettings}
          title="设置"
          type="button"
        >
          <IconSettings aria-hidden="true" size={19} stroke={1.8} />
          <span className="app-sidebar__label">设置</span>
        </button>
      </div>
    </aside>
  );
}
