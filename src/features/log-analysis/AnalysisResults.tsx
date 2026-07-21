import { Badge, Group, Modal, Paper, Stack, Table, Text, Title } from '@mantine/core';
import { useMemo, useState, type JSX } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { CategoryCount, LogSummary, TimeBucket } from '../../api/types';

interface AnalysisResultsProps {
  summary: LogSummary | null;
  heatmap: TimeBucket[];
  aiReports: string[];
}

export function AnalysisResults({
  summary,
  heatmap,
  aiReports,
}: AnalysisResultsProps): JSX.Element {
  const [detail, setDetail] = useState<{ title: string; rows: [string, string, string, string][] } | null>(null);

  const snDetailRows = useMemo(() => {
    if (!summary) return [] as [string, string, string, string][];
    const snTotal: Record<string, number> = {};
    const snErrors: Record<string, { errorType: string; count: number }[]> = {};
    for (const se of summary.snErrors) {
      snTotal[se.sn] = (snTotal[se.sn] ?? 0) + se.count;
      (snErrors[se.sn] ??= []).push({ errorType: se.errorType, count: se.count });
    }
    const rows: [string, string, string, string][] = [];
    const sortedSns = Object.keys(snTotal).sort((a, b) => snTotal[b] - snTotal[a]);
    for (const sn of sortedSns) {
      const errs = snErrors[sn].sort((a, b) => b.count - a.count);
      for (const err of errs) {
        rows.push([sn, err.errorType, err.count.toLocaleString(), snTotal[sn].toLocaleString()]);
      }
    }
    return rows;
  }, [summary]);

  if (!summary) return <></>;

  return (
    <Stack gap="md">
      <Paper p="md" radius="md" withBorder>
        <Title mb="sm" order={4}>概览</Title>
        <Group gap="lg">
          <StatBox label="总行数" value={summary.totalLines.toLocaleString()} />
          <StatBox label="解析条目" value={summary.entryCount.toLocaleString()} />
          <StatBox
            label="设备SN"
            onClick={() => setDetail({ title: '设备 SN 错误详情', rows: snDetailRows })}
            value={summary.uniqueSns.length.toString()}
          />
        </Group>
      </Paper>

      <Paper p="md" radius="md" withBorder>
        <Title mb="sm" order={4}>错误分类</Title>
        <Stack gap={4}>
          {summary.categoryCounts.slice(0, 10).map((cc: CategoryCount) => (
            <Bar key={cc.category} label={cc.category} max={summary.categoryCounts[0]?.count ?? 1} value={cc.count} />
          ))}
        </Stack>
      </Paper>

      {heatmap.length > 0 && (
        <Paper p="md" radius="md" withBorder>
          <Title mb="sm" order={4}>时间热力图</Title>
          <Group gap={1} align="flex-end" style={{ flexWrap: 'wrap', justifyContent: 'flex-start' }}>
            {heatmap.map((b) => {
              const maxVal = Math.max(...heatmap.map((h) => h.count), 1);
              const ratio = b.count / maxVal;
              const hue = Math.round(220 - ratio * 200);
              return (
                <div
                  key={b.hour}
                  style={{ display: 'flex', alignItems: 'flex-end', height: 64 }}
                  title={`${b.hour}\n${b.count} 条`}
                >
                  <div
                    style={{
                      width: 14,
                      height: Math.max(3, Math.round(ratio * 64)),
                      backgroundColor: `hsl(${hue}, 70%, ${60 - ratio * 30}%)`,
                      borderRadius: 2,
                    }}
                  />
                </div>
              );
            })}
          </Group>
        </Paper>
      )}

      {aiReports.length > 0 && (
        <Paper p="md" radius="md" withBorder>
          <Title mb="sm" order={4}>AI 分析报告</Title>
          <Stack gap="sm">
            {aiReports.map((report, i) => (
              <div
                key={i}
                className="markdown-report"
                style={{ fontSize: 14, lineHeight: 1.8 }}
              >
                <ReactMarkdown remarkPlugins={[remarkGfm]}>
                  {report}
                </ReactMarkdown>
              </div>
            ))}
          </Stack>
        </Paper>
      )}

      <Modal
        onClose={() => setDetail(null)}
        opened={detail !== null}
        size="70%"
        title={detail?.title}
      >
        <Table striped>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>设备 SN</Table.Th>
              <Table.Th>错误类型</Table.Th>
              <Table.Th style={{ whiteSpace: 'nowrap' }} ta="right">报错</Table.Th>
              <Table.Th style={{ whiteSpace: 'nowrap' }} ta="right">合计</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {detail?.rows.map(([sn, errType, cnt, total], i, arr) => {
              const prev = i > 0 ? arr[i - 1] : null;
              const isFirstOfSn = !prev || prev[0] !== sn;
              return (
                <Table.Tr key={`${sn}-${errType}-${i}`}>
                  <Table.Td fw={isFirstOfSn ? 600 : undefined}>
                    {isFirstOfSn ? sn : ''}
                  </Table.Td>
                  <Table.Td>{errType}</Table.Td>
                  <Table.Td style={{ whiteSpace: 'nowrap' }} ta="right">{cnt}</Table.Td>
                  <Table.Td style={{ whiteSpace: 'nowrap' }} ta="right">{isFirstOfSn ? total : ''}</Table.Td>
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
      </Modal>
    </Stack>
  );
}

function StatBox({
  label,
  value,
  onClick,
}: {
  label: string;
  value: string;
  onClick?: () => void;
}): JSX.Element {
  if (onClick) {
    return (
      <div
        onClick={onClick}
        style={{
          cursor: 'pointer',
          padding: '6px 10px',
          borderRadius: 8,
          border: '1px solid var(--mantine-color-gray-3)',
        }}
      >
        <Text c="dimmed" size="xs">{label}</Text>
        <Text fw={700} size="lg">{value} <Badge color="blue" ml={6} size="xs" variant="light">详情</Badge></Text>
      </div>
    );
  }
  return (
    <div style={{ padding: '4px 8px' }}>
      <Text c="dimmed" size="xs">{label}</Text>
      <Text fw={700} size="lg">{value}</Text>
    </div>
  );
}

function Bar({ label, max, value }: { label: string; max: number; value: number }): JSX.Element {
  const pct = Math.round((value / max) * 100);
  return (
    <Group gap="sm" wrap="nowrap">
      <Text size="xs" style={{ flexShrink: 0, width: 170 }}>{label}</Text>
      <div style={{ flex: 1, height: 13, background: 'var(--mantine-color-gray-2)', borderRadius: 4, overflow: 'hidden' }}>
        <div style={{ height: '100%', width: `${pct}%`, background: 'var(--mantine-color-blue-5)', borderRadius: 4, transition: 'width 300ms' }} />
      </div>
      <Text size="xs" style={{ flexShrink: 0, width: 44, textAlign: 'right' }}>{value}</Text>
    </Group>
  );
}
