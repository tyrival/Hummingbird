import {
  Alert,
  Button,
  Card,
  Group,
  SegmentedControl,
  Stack,
  Text,
  Textarea,
  Title,
} from '@mantine/core';
import { IconFileDescription, IconPlayerPlay, IconPlayerStop, IconX } from '@tabler/icons-react';
import { type JSX, useState } from 'react';
import { selectInputFile } from '../../api/tauri';
import type { PassthroughSourceKind, SelectedInputDto } from '../../api/types';
import { LogPanel, type TaskControlState } from '../../components/LogPanel';
import { MessageResultCard } from './MessageResultCard';
import { usePassthroughParser } from './usePassthroughParser';

export function PassthroughWorkspace(): JSX.Element {
  const [requestHex, setRequestHex] = useState('');
  const [responseHex, setResponseHex] = useState('');
  const [sourceKind, setSourceKind] = useState<PassthroughSourceKind>('awt_template');
  const [selectedSource, setSelectedSource] = useState<SelectedInputDto | null>(null);
  const parser = usePassthroughParser();
  const busy = parser.state === 'parsing' || parser.state === 'extracting_source' || parser.state === 'cancelling';
  const canCancel = parser.state === 'extracting_source' || parser.state === 'cancelling';
  const showLog = parser.state !== 'idle' && parser.state !== 'completed';
  const logTask: TaskControlState = {
    active: busy,
    canStart: false,
    stage: null,
    completedChunks: 0,
    totalChunks: 0,
    progressValue: 0,
    terminal: parser.state === 'failed'
      ? { type: 'failed', message: parser.error?.message ?? '解析失败' }
      : parser.state === 'cancelled'
        ? { type: 'cancelled' }
        : null,
  };
  const activeLogLabel = parser.state === 'extracting_source'
    ? '正在提取资料'
    : parser.state === 'cancelling'
      ? '正在停止'
      : '正在解析报文';

  const switchSource = (value: string) => {
    setSourceKind(value as PassthroughSourceKind);
    setSelectedSource(null);
  };

  const chooseSource = async () => {
    const selected = await selectInputFile();
    if (selected) setSelectedSource(selected);
  };

  const startParsing = () => {
    if (!requestHex.trim() && responseHex.trim()) {
      parser.reject('解析回复报文前请先填写对应的请求报文。');
      return;
    }
    const requestCount = requestHex.split('&&').filter((part) => part.trim()).length;
    const responseCount = responseHex.split('&&').filter((part) => part.trim()).length;
    if (responseCount > requestCount) {
      parser.reject('回复报文数量不能超过请求报文数量。');
      return;
    }
    void parser.parse({
    requestHex,
    responseHex: responseHex.trim() ? responseHex : null,
    source: selectedSource ? {
      kind: sourceKind,
      path: selectedSource.path,
      fileName: selectedSource.fileName,
    } : null,
    });
  };

  return (
    <main className="passthrough-workspace awt-workspace app-workspace" aria-labelledby="passthrough-heading">
      <header className="workspace-header">
        <div>
          <Text c="blue" fw={700} size="xs" tt="uppercase">Passthrough message workspace</Text>
          <Title id="passthrough-heading" order={2}>王大佬帮看下这报文什么意思？</Title>
          <Text c="dimmed" mt={4} size="sm">
            解析中台报文里看不懂的十六进制透传报文
          </Text>
        </div>
      </header>
      <Card
        className="input-card passthrough-input-card"
        padding="lg"
        radius="lg"
        style={{ flexShrink: 0 }}
        withBorder
      >
        <Group className="passthrough-command-header" justify="space-between" wrap="nowrap">
          <div>
            <Text fw={650}>透传命令</Text>
            <Text c="dimmed" mt={3} size="xs">
              支持协议&nbsp;&nbsp;Modbus RTU · DL/T 645 · CJ/T 188
            </Text>
          </div>
          <Button
            color={canCancel ? 'red' : undefined}
            disabled={parser.state === 'cancelling' || (!canCancel && (busy || (!requestHex.trim() && !responseHex.trim())))}
            leftSection={canCancel ? <IconPlayerStop size={16} /> : <IconPlayerPlay size={16} />}
            onClick={() => canCancel ? void parser.cancel() : startParsing()}
          >
            {parser.state === 'extracting_source' ? '停止提取' : parser.state === 'cancelling' ? '正在停止…' : busy ? '解析中…' : '开始解析'}
          </Button>
        </Group>
        <div className="passthrough-message-pair">
          <Textarea
            aria-label="请求报文"
            classNames={{ input: 'passthrough-command-input' }}
            disabled={busy}
            minRows={7}
            onChange={(event) => setRequestHex(event.currentTarget.value)}
            placeholder="粘贴请求 Hex 报文"
            value={requestHex}
          />
          <Textarea
            aria-label="回复报文"
            classNames={{ input: 'passthrough-command-input' }}
            disabled={busy}
            minRows={7}
            onChange={(event) => setResponseHex(event.currentTarget.value)}
            placeholder="粘贴回复 Hex 报文（需要对应请求）"
            value={responseHex}
          />
        </div>
        <Group className="passthrough-source-row" mt="md" wrap="nowrap">
          <SegmentedControl
            data={[{ label: 'AWT模板', value: 'awt_template' }, { label: 'AI识别说明书', value: 'manual' }]}
            disabled={busy}
            onChange={switchSource}
            value={sourceKind}
          />
          <Button disabled={busy} onClick={() => void chooseSource()} variant="default">
            选择{sourceKind === 'manual' ? ' PDF、DOCX、XLS、XLSX、CSV' : ' CSV'}
          </Button>
          {selectedSource ? (
            <Group className="passthrough-source-file" gap="xs" wrap="nowrap">
              <IconFileDescription size={17} />
              <Text lineClamp={1} size="sm">{selectedSource.fileName}</Text>
              <Button aria-label="清除资料" onClick={() => setSelectedSource(null)} p={4} variant="subtle"><IconX size={15} /></Button>
            </Group>
          ) : null}
        </Group>
      </Card>
      {showLog ? (
        <div className="passthrough-log">
          <LogPanel
            activeLabel={activeLogLabel}
            entries={parser.logs}
            onStartStop={() => undefined}
            showActionButton={false}
            task={logTask}
          />
        </div>
      ) : (
        <Stack className="passthrough-results" style={{ flexShrink: 0 }}>
          {parser.result?.sourceWarning ? <Alert color="yellow">{parser.result.sourceWarning}</Alert> : null}
          {parser.result?.results.map((result) => <MessageResultCard key={`${result.role}-${result.index}`} result={result} />)}
        </Stack>
      )}
    </main>
  );
}
