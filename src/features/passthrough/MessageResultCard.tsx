import { Alert, Badge, Button, Card, Checkbox, Group, Table, Text } from '@mantine/core';
import type { JSX } from 'react';
import { useState } from 'react';
import type { PassthroughMessageResult } from '../../api/types';
import { RegisterMergeModal } from './RegisterMergeModal';

const protocolNames = {
  modbusRtu: 'Modbus RTU',
  dlt645: 'DL/T 645',
  cjt188: 'CJ/T 188',
  unknown: '协议待确认',
};

function displayedFieldHex(field: PassthroughMessageResult['fields'][number]): string {
  if (field.name === 'platformSerial' && field.rawHex.length >= 4) {
    return `${field.rawHex.slice(0, 2)} ${field.rawHex.slice(2, -2)} ${field.rawHex.slice(-2)}`;
  }
  return field.rawHex;
}

function explanationText(explanation: PassthroughMessageResult['explanations'][number] | null): string {
  if (!explanation) return '—';
  if (explanation.meaning) return explanation.meaning;
  const warnings = explanation.warnings.map((warning) => warning.message).join('；');
  return warnings || '—';
}

export function MessageResultCard({ result }: { result: PassthroughMessageResult }): JSX.Element {
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [mergeOpened, setMergeOpened] = useState(false);
  const checksumLabel = result.checksum?.valid === true
    ? '校验通过'
    : result.checksum?.valid === false ? '校验失败' : '无校验结论';
  const writeMultipleResponse = result.fields.some(
    (field) => field.name === 'function' && field.rawHex === '10',
  ) && result.registers.some((register) => register.rawHex.length === 0);
  const selectedRegisters = result.registers.filter((register, index) => selected.has(index) && register.rawHex.length > 0);
  const mergedHex = selectedRegisters.flatMap((register) => register.rawHex.match(/../g) ?? []).join(' ');

  const registerLabel = (index: number) => {
    const register = result.registers[index];
    return register.address == null ? register.identifier ?? `第 ${index + 1} 项` : `0x${register.address.toString(16).toUpperCase().padStart(4, '0')}`;
  };

  return (
    <Card className="passthrough-result" padding="lg" radius="lg" style={{ flexShrink: 0 }} withBorder>
      <Group justify="space-between">
        <Group gap="xs">
          <Text fw={700}>{result.role === 'response' ? '回复报文' : '请求报文'} {result.index + 1}</Text>
          <Badge variant="light">{protocolNames[result.protocol]}</Badge>
          <Badge color={result.checksum?.valid === false ? 'red' : 'gray'} variant="light">
            {checksumLabel}
          </Badge>
        </Group>
      </Group>
      {result.error ? <Alert color="red" mt="md">{result.error.message}</Alert> : null}
      {result.warnings.map((warning) => (
        <Alert color="yellow" key={warning.code} mt="sm">{warning.message}</Alert>
      ))}
      <div className="passthrough-fields">
        {result.fields.map((field) => (
          <Text className="passthrough-field" key={`${field.name}-${field.byteStart}`} size="sm">
            <span className="passthrough-hex">{displayedFieldHex(field)}</span>：{field.displayValue}
          </Text>
        ))}
      </div>
      <Group justify="space-between" mt="md">
        <Text fw={650}>寄存器说明</Text>
        <Button disabled={selectedRegisters.length === 0} onClick={() => setMergeOpened(true)} size="compact-sm" variant="light">合并解析</Button>
      </Group>
      <Table mt="md" striped withTableBorder>
        <Table.Thead><Table.Tr><Table.Th aria-label="选择" /><Table.Th>地址/标识</Table.Th><Table.Th>参量名称</Table.Th><Table.Th>原始值</Table.Th><Table.Th>解析值</Table.Th><Table.Th>单位</Table.Th><Table.Th>说明</Table.Th></Table.Tr></Table.Thead>
        <Table.Tbody>
          {result.registers.length === 0 ? (
            <Table.Tr><Table.Td colSpan={7}><Text c="dimmed" ta="center">未解析到寄存器</Text></Table.Td></Table.Tr>
          ) : result.registers.flatMap((register, index) => {
            const explanations = result.explanations.filter((item) => item.address === register.address);
            const rows = explanations.length > 0 ? explanations : [null];
            return rows.map((explanation, explanationIndex) => (
              <Table.Tr key={`${register.address ?? register.identifier}-${index}-${explanationIndex}`}>
                <Table.Td>{explanationIndex === 0 ? <Checkbox aria-label={`选择寄存器 ${registerLabel(index)}`} checked={selected.has(index)} disabled={!register.rawHex} onChange={(event) => {
                  const checked = event.currentTarget.checked;
                  setSelected((current) => {
                    const next = new Set(current);
                    if (checked) next.add(index); else next.delete(index);
                    return next;
                  });
                }} size="xs" /> : null}</Table.Td>
                <Table.Td>{register.address == null ? register.identifier ?? '—' : `0x${register.address.toString(16).toUpperCase().padStart(4, '0')}`}</Table.Td>
                <Table.Td>{explanation?.parameterName ?? explanation?.parameterCode ?? '—'}</Table.Td>
                <Table.Td>{register.rawHex || (writeMultipleResponse ? '响应帧未携带写入值' : '—')}</Table.Td>
                <Table.Td>{explanation?.convertedValue ?? '—'}</Table.Td>
                <Table.Td>{explanation?.unit ?? '—'}</Table.Td>
                <Table.Td>{explanationText(explanation)}</Table.Td>
              </Table.Tr>
            ));
          })}
        </Table.Tbody>
      </Table>
      <RegisterMergeModal initialHex={mergedHex} onClose={() => setMergeOpened(false)} opened={mergeOpened} registerCount={selectedRegisters.length} />
    </Card>
  );
}
