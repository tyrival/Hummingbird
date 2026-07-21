import {
  Accordion,
  Button,
  Group,
  Modal,
  NumberInput,
  PasswordInput,
  Stack,
  Text,
  TextInput,
} from '@mantine/core';
import { notifications } from '@mantine/notifications';
import { type FormEvent, type JSX, useEffect, useState } from 'react';
import {
  getAppVersion,
  saveSettings,
} from '../api/tauri';
import type { AppErrorDto, SettingsDto } from '../api/types';

interface SettingsModalProps {
  opened: boolean;
  onCheckUpdate: () => void;
  onClose: () => void;
  settings: SettingsDto;
  onSaved: (settings: SettingsDto) => void;
}

type EditableField =
  | 'baseUrl'
  | 'model'
  | 'timeoutSeconds'
  | 'maxTokens'
  | 'chunkMaxChars'
  | 'contextChars';

type FormErrors = Partial<Record<EditableField, string>>;

export function SettingsModal({
  opened,
  onCheckUpdate,
  onClose,
  settings,
  onSaved,
}: SettingsModalProps): JSX.Element {
  if (!opened) return <></>;

  return (
    <SettingsModalContent
      key={settingsIdentity(settings)}
      onCheckUpdate={onCheckUpdate}
      onClose={onClose}
      onSaved={onSaved}
      settings={settings}
    />
  );
}

interface SettingsModalContentProps {
  onCheckUpdate: () => void;
  onClose: () => void;
  settings: SettingsDto;
  onSaved: (settings: SettingsDto) => void;
}

function SettingsModalContent({
  onCheckUpdate,
  onClose,
  settings,
  onSaved,
}: SettingsModalContentProps): JSX.Element {
  const [draft, setDraft] = useState(settings);
  const [errors, setErrors] = useState<FormErrors>({});
  const [saving, setSaving] = useState(false);
  const [version, setVersion] = useState('…');

  useEffect(() => {
    void getAppVersion().then(setVersion).catch(() => setVersion('未知'));
  }, []);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const nextErrors = validateSettings(draft);
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length > 0) return;
    setSaving(true);
    try {
      const saved = await saveSettings(draft);
      onSaved(saved);
      notifications.show({ title: '设置已保存', message: '新的配置将在下次任务中生效。', color: 'teal' });
      onClose();
    } catch (error) {
      const safeError = error as AppErrorDto;
      notifications.show({ title: '保存设置失败', message: safeError.message, color: 'red' });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      centered
      closeOnEscape={!saving}
      closeOnClickOutside={!saving}
      onClose={() => {
        if (!saving) onClose();
      }}
      opened
      size="lg"
      title="设置"
      transitionProps={{ duration: 0 }}
    >
      <form noValidate onSubmit={(event) => void submit(event)}>
        <Stack gap="sm">
          <TextInput
            error={errors.baseUrl}
            label="API 地址"
            onChange={(event) => setDraft({ ...draft, baseUrl: event.currentTarget.value })}
            placeholder="https://api.example.com/v1"
            required
            value={draft.baseUrl}
          />
          <PasswordInput
            label="API 密钥"
            onChange={(event) => setDraft({ ...draft, apiKey: event.currentTarget.value })}
            value={draft.apiKey}
            visibilityToggleButtonProps={{ 'aria-label': '显示或隐藏 API 密钥' }}
          />
          <TextInput
            error={errors.model}
            label="模型名称"
            onChange={(event) => setDraft({ ...draft, model: event.currentTarget.value })}
            required
            value={draft.model}
          />
          <Group grow align="flex-start">
            <NumberInput
              allowDecimal={false}
              allowNegative={false}
              error={errors.timeoutSeconds}
              label="请求超时（秒）"
              min={1}
              onChange={(value) => setDraft({ ...draft, timeoutSeconds: numberValue(value) })}
              value={draft.timeoutSeconds}
            />
            <NumberInput
              allowDecimal={false}
              allowNegative={false}
              error={errors.maxTokens}
              label="最大输出 Token"
              min={1}
              onChange={(value) => setDraft({ ...draft, maxTokens: numberValue(value) })}
              value={draft.maxTokens}
            />
          </Group>

          <Accordion variant="separated">
            <Accordion.Item value="advanced">
              <Accordion.Control>高级设置</Accordion.Control>
              <Accordion.Panel>
                <Stack gap="sm">
                  <NumberInput
                    allowDecimal={false}
                    allowNegative={false}
                    description="默认 12000；说明书总长度不受此项限制。"
                    error={errors.chunkMaxChars}
                    label="单块最大字符数"
                    max={60000}
                    min={8000}
                    onChange={(value) => setDraft({ ...draft, chunkMaxChars: numberValue(value) })}
                    value={draft.chunkMaxChars}
                  />
                  <NumberInput
                    allowDecimal={false}
                    allowNegative={false}
                    description="默认 1500，仅用于跨块语义衔接，不会重复输出记录。"
                    error={errors.contextChars}
                    label="跨块上下文字符数"
                    max={3000}
                    min={0}
                    onChange={(value) => setDraft({ ...draft, contextChars: numberValue(value) })}
                    value={draft.contextChars}
                  />
                </Stack>
              </Accordion.Panel>
            </Accordion.Item>
          </Accordion>

          <Group justify="space-between" mt="sm">
            <Group gap="xs">
              <Text c="dimmed" size="xs">Hummingbird {version}</Text>
              <Button
                aria-label="从设置检查更新"
                onClick={onCheckUpdate}
                size="compact-xs"
                type="button"
                variant="subtle"
              >
                检查更新
              </Button>
            </Group>
            <Group gap="xs">
              <Button disabled={saving} onClick={onClose} type="button" variant="default">
                取消
              </Button>
              <Button loading={saving} type="submit">保存设置</Button>
            </Group>
          </Group>
        </Stack>
      </form>
    </Modal>
  );
}

const settingsIdentities = new WeakMap<SettingsDto, number>();
let nextSettingsIdentity = 1;

function settingsIdentity(settings: SettingsDto): number {
  const existing = settingsIdentities.get(settings);
  if (existing !== undefined) return existing;
  const identity = nextSettingsIdentity;
  nextSettingsIdentity += 1;
  settingsIdentities.set(settings, identity);
  return identity;
}

function numberValue(value: string | number): number {
  return typeof value === 'number' ? value : Number(value);
}

function validateSettings(settings: SettingsDto): FormErrors {
  const errors: FormErrors = {};
  if (!settings.baseUrl.trim()) {
    errors.baseUrl = '请输入 API 地址';
  } else if (!/^https?:\/\//i.test(settings.baseUrl)) {
    errors.baseUrl = 'API 地址必须以 http:// 或 https:// 开头';
  }
  if (!settings.model.trim()) errors.model = '请输入模型名称';
  if (!isPositiveInteger(settings.timeoutSeconds)) errors.timeoutSeconds = '请输入正整数';
  if (!isPositiveInteger(settings.maxTokens)) errors.maxTokens = '请输入正整数';
  if (!Number.isInteger(settings.chunkMaxChars)
    || settings.chunkMaxChars < 8000
    || settings.chunkMaxChars > 60000) {
    errors.chunkMaxChars = '请输入 8000 到 60000 之间的整数';
  }
  if (!Number.isInteger(settings.contextChars)
    || settings.contextChars < 0
    || settings.contextChars > 3000) {
    errors.contextChars = '请输入 0 到 3000 之间的整数';
  }
  return errors;
}

function isPositiveInteger(value: number): boolean {
  return Number.isInteger(value) && value > 0;
}
