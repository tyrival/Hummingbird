import { Button, Group, Modal, SimpleGrid, Stack, Text, TextInput } from '@mantine/core';
import { type JSX, useState } from 'react';
import { parseRegisterMergeHex, type NumericInterpretation, type RegisterMergeResult } from './registerMerge';

interface RegisterMergeModalProps {
  opened: boolean;
  onClose: () => void;
  initialHex: string;
  registerCount: number;
}

const numericRows: Array<[keyof NumericInterpretation, string]> = [
  ['int8', 'Int8'], ['uint8', 'UInt8'], ['int16', 'Int16'], ['uint16', 'UInt16'],
  ['int32', 'Int32'], ['uint32', 'UInt32'], ['int64', 'Int64'], ['uint64', 'UInt64'],
];

function ResultColumn({ title, result, values }: { title: string; result: Extract<RegisterMergeResult, { ok: true }>; values: NumericInterpretation }): JSX.Element {
  return (
    <section className="register-merge-column" aria-label={title}>
      <Text fw={700}>{title}</Text>
      <Stack gap="xs" mt="md">
        <div><Text c="dimmed" fw={650} size="xs">HEX</Text><Text className="register-merge-value" ff="monospace">{result.hex || '—'}</Text></div>
        <div><Text c="dimmed" fw={650} size="xs">ASCII</Text><Text className="register-merge-value" ff="monospace">{result.ascii || '—'}</Text></div>
        {numericRows.map(([key, label]) => <div key={key}><Text c="dimmed" fw={650} size="xs">{label}</Text><Text className="register-merge-value" ff="monospace">{values[key] ?? '—'}</Text></div>)}
        <div>
          <Text c="dimmed" fw={650} size="xs">二进制</Text>
          <Stack gap={4} mt={4}>
            {result.binary.length === 0 ? <Text size="sm">—</Text> : result.binary.map((row, index) => (
              <div className="register-merge-binary-row" key={`${row.hex}-${index}`}>
                <Text ff="monospace" size="sm">{row.hex}</Text>
                <div className="register-merge-bits">{row.bits.map((active, bit) => <span className={active ? 'is-active' : ''} key={bit} />)}</div>
              </div>
            ))}
          </Stack>
        </div>
      </Stack>
    </section>
  );
}

function RegisterMergeModalContent({ onClose, initialHex, registerCount }: Omit<RegisterMergeModalProps, 'opened'>): JSX.Element {
  const [input, setInput] = useState(initialHex);
  const [result, setResult] = useState(() => parseRegisterMergeHex(initialHex));
  const [error, setError] = useState<string | null>(null);

  const parse = () => {
    const next = parseRegisterMergeHex(input);
    if (!next.ok) {
      setError(next.error);
      return;
    }
    setResult(next);
    setError(null);
  };

  const validResult = result.ok ? result : parseRegisterMergeHex('');
  if (!validResult.ok) throw new Error('empty HEX must be valid');

  return (
    <Modal opened onClose={onClose} size="xl" title={`合并解析 · ${registerCount} 个寄存器 · ${validResult.bytes.length} 字节`}>
      <Group align="flex-start" wrap="nowrap">
        <TextInput aria-label="合并 HEX" classNames={{ input: 'passthrough-command-input' }} error={error} onChange={(event) => setInput(event.currentTarget.value)} value={input} style={{ flex: 1 }} />
        <Button onClick={parse}>解析</Button>
      </Group>
      <SimpleGrid className="register-merge-grid" cols={{ base: 1, sm: 2 }} mt="lg" spacing="md">
        <ResultColumn result={validResult} title="高字节在前" values={validResult.big} />
        <ResultColumn result={validResult} title="低字节在前" values={validResult.little} />
      </SimpleGrid>
    </Modal>
  );
}

export function RegisterMergeModal({ opened, ...props }: RegisterMergeModalProps): JSX.Element | null {
  return opened ? <RegisterMergeModalContent {...props} /> : null;
}
