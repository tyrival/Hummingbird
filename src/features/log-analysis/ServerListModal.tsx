import {
  ActionIcon,
  Button,
  Group,
  Modal,
  PasswordInput,
  Stack,
  Text,
  TextInput,
} from '@mantine/core';
import { IconEdit, IconFile, IconPlus, IconSearch, IconTrash } from '@tabler/icons-react';
import { type JSX, useCallback, useMemo, useState } from 'react';
import { saveSshServers, selectKeyFile, testSshConnection } from '../../api/tauri';
import type { SshServerConfig } from '../../api/types';

interface ServerListModalProps {
  opened: boolean;
  servers: SshServerConfig[];
  onClose: () => void;
  onSelect: (server: SshServerConfig) => void;
  onServersChange: (servers: SshServerConfig[]) => void;
}

function emptyServer(): SshServerConfig {
  return { name: '', host: '', port: 22, user: 'root', password: '', appRoot: '/home/acrel-iot-linux' };
}

export function ServerListModal({
  opened,
  servers,
  onClose,
  onSelect,
  onServersChange,
}: ServerListModalProps): JSX.Element {
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<SshServerConfig | null>(null);
  const [draft, setDraft] = useState<SshServerConfig>(emptyServer());
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [keyFileName, setKeyFileName] = useState('');

  const resetForm = useCallback(() => {
    setSearch('');
    setEditing(null);
    setTestResult(null);
  }, []);

  const filtered = useMemo(
    () =>
      servers.filter((s) =>
        s.name.toLowerCase().includes(search.toLowerCase()),
      ),
    [servers, search],
  );

  const startEdit = useCallback((s?: SshServerConfig) => {
    const server = s ?? emptyServer();
    setEditing(server);
    setDraft({ ...server, privateKey: '' });
    setKeyFileName('');
    setTestResult(null);
  }, []);

  const saveServer = useCallback(async () => {
    const name = draft.name.trim();
    const host = draft.host.trim();
    if (!name || !host) return;
    const updated: SshServerConfig = { ...draft, name, host };
    const list = editing?.name
      ? servers.map((s) => (s.name === editing.name ? updated : s))
      : [...servers, updated];
    const saved = await saveSshServers(list);
    onServersChange(saved);
    setEditing(null);
  }, [draft, editing, servers, onServersChange]);

  const deleteServer = useCallback(async (name: string) => {
    const updated = servers.filter((s) => s.name !== name);
    const saved = await saveSshServers(updated);
    onServersChange(saved);
  }, [servers, onServersChange]);

  const handleTest = useCallback(async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const result = await testSshConnection(draft);
      setTestResult(result);
    } catch (e: unknown) {
      setTestResult(`连接失败: ${(e as { message?: string }).message ?? '未知错误'}`);
    } finally {
      setTesting(false);
    }
  }, [draft]);

  const handlePickKeyFile = useCallback(async () => {
    try {
      const content = await selectKeyFile();
      setDraft((prev) => ({ ...prev, privateKey: content }));
      setKeyFileName(content ? '(已选择)' : '');
    } catch {
      // user cancelled
    }
  }, []);

  return (
    <Modal
      onClose={() => { resetForm(); onClose(); }}
      opened={opened}
      size="lg"
      title="SSH 服务器列表"
    >
      {!editing ? (
        <>
          <TextInput
            leftSection={<IconSearch size={18} />}
            onChange={(e) => setSearch(e.currentTarget.value)}
            placeholder="搜索服务器名称..."
            value={search}
          />
          <Stack gap={6} mt="sm">
            {filtered.map((s) => (
              <Group
                className="server-row"
                gap={0}
                justify="space-between"
                key={s.name}
                onClick={() => onSelect(s)}
                onKeyDown={(e) => e.key === 'Enter' && onSelect(s)}
                p="sm"
                style={{ cursor: 'pointer', borderRadius: 8 }}
                tabIndex={0}
              >
                <span style={{ flex: 1 }}>
                  <Text fw={600} size="sm">{s.name}</Text>
                  <Text c="dimmed" size="xs">
                    {s.user}@{s.host}:{s.port} — {s.appRoot}
                  </Text>
                </span>
                <Group gap={4}>
                  <ActionIcon
                    onClick={(e) => { e.stopPropagation(); startEdit(s); }}
                    size="sm"
                    variant="subtle"
                  >
                    <IconEdit size={16} />
                  </ActionIcon>
                  <ActionIcon
                    color="red"
                    onClick={(e) => { e.stopPropagation(); void deleteServer(s.name); }}
                    size="sm"
                    variant="subtle"
                  >
                    <IconTrash size={16} />
                  </ActionIcon>
                </Group>
              </Group>
            ))}
            {filtered.length === 0 && (
              <Text c="dimmed" size="sm" ta="center">
                暂无服务器，点击下方按钮添加
              </Text>
            )}
          </Stack>
          <Button
            fullWidth
            leftSection={<IconPlus size={18} />}
            mt="md"
            onClick={() => startEdit()}
          >
            添加服务器
          </Button>
        </>
      ) : (
        <Stack gap="sm">
          <TextInput
            label="名称"
            onChange={(e) => setDraft({ ...draft, name: e.currentTarget.value })}
            placeholder="客户A生产环境"
            value={draft.name}
          />
          <Group grow>
            <TextInput
              label="主机"
              onChange={(e) => setDraft({ ...draft, host: e.currentTarget.value })}
              placeholder="192.168.1.100"
              value={draft.host}
            />
            <TextInput
              label="端口"
              onChange={(e) => setDraft({ ...draft, port: Number(e.currentTarget.value) || 22 })}
              placeholder="22"
              type="number"
              value={draft.port}
            />
          </Group>
          <TextInput
            label="用户名"
            onChange={(e) => setDraft({ ...draft, user: e.currentTarget.value })}
            placeholder="root"
            value={draft.user}
          />
          <PasswordInput
            label="密码"
            onChange={(e) => setDraft({ ...draft, password: e.currentTarget.value })}
            placeholder="(留空则不修改)"
            value={draft.password ?? ''}
          />
          <Stack gap={4}>
            <Text size="sm" fw={500}>SSH 私钥 (PEM)</Text>
            {draft.privateKey ? (
              <Group gap="xs">
                <Text c="teal" size="sm" style={{ flex: 1 }} truncate>
                  {keyFileName || '已选择密钥文件'}
                </Text>
                <Button
                  onClick={handlePickKeyFile}
                  size="compact-sm"
                  variant="default"
                >
                  重新选择
                </Button>
              </Group>
            ) : (
              <Button
                leftSection={<IconFile size={16} />}
                onClick={handlePickKeyFile}
                variant="default"
              >
                选择密钥文件
              </Button>
            )}
          </Stack>
          <TextInput
            label="应用根目录"
            onChange={(e) => setDraft({ ...draft, appRoot: e.currentTarget.value })}
            placeholder="/home/acrel-iot-linux"
            value={draft.appRoot}
          />
          {testResult && (
            <Text c={testResult.startsWith('连接失败') ? 'red' : 'teal'} size="sm">
              {testResult}
            </Text>
          )}
          <Group justify="space-between">
            <Button
              color="gray"
              loading={testing}
              onClick={handleTest}
              variant="default"
            >
              测试连接
            </Button>
            <Group>
              <Button onClick={() => setEditing(null)} variant="default">
                取消
              </Button>
              <Button onClick={() => void saveServer()}>
                {editing?.name ? '保存' : '添加'}
              </Button>
            </Group>
          </Group>
        </Stack>
      )}
    </Modal>
  );
}
