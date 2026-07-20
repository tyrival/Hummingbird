import {
  IconFeather,
  IconFileDescription,
  IconRefresh,
  IconTemplate,
} from '@tabler/icons-react';
import type { JSX } from 'react';

export type Workspace = 'awt' | 'passthrough';

interface AppSidebarProps {
  activeWorkspace: Workspace;
  checkingUpdate: boolean;
  onCheckUpdate: () => void;
  onWorkspaceChange: (workspace: Workspace) => void;
  taskActive: boolean;
}

const items = [
  { id: 'awt' as const, label: 'AWT模板生成', icon: IconTemplate },
  { id: 'passthrough' as const, label: '透传命令识别', icon: IconFileDescription },
];

export function AppSidebar({
  activeWorkspace,
  checkingUpdate,
  onCheckUpdate,
  onWorkspaceChange,
  taskActive,
}: AppSidebarProps): JSX.Element {
  return (
    <aside className="app-sidebar" aria-label="工作区导航">
      <div className="app-sidebar__brand">
        <span className="app-sidebar__brand-icon" aria-hidden="true">
          <IconFeather size={22} stroke={1.8} />
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
      </div>
    </aside>
  );
}
