import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { PassthroughMessageResult } from '../../api/types';
import { MessageResultCard } from './MessageResultCard';

const result: PassthroughMessageResult = {
    index: 0,
    role: 'request',
  rawSegment: '0420035400050A000D020301071A0000004968',
  cleanedHex: '0420035400050A000D020301071A0000004968',
  protocol: 'modbusRtu',
  summary: '从站 4 使用私有功能码 0x20。',
  fields: [
    { name: 'platformSerial', byteStart: 0, byteEnd: 17, rawHex: '68323630363235303933333030303168', displayValue: '仪表序列号 26062509330001', source: 'code' },
    { name: 'slave', byteStart: 17, byteEnd: 18, rawHex: '01', displayValue: 'Modbus 地址 1', source: 'code' },
    { name: 'function', byteStart: 18, byteEnd: 19, rawHex: '10', displayValue: '写多个寄存器', source: 'code' },
    { name: 'startAddress', byteStart: 19, byteEnd: 21, rawHex: 'E000', displayValue: '起始寄存器地址 0xE000', source: 'code' },
    { name: 'quantity', byteStart: 21, byteEnd: 23, rawHex: '0006', displayValue: '寄存器数量 6', source: 'code' },
    { name: 'crc', byteStart: 23, byteEnd: 25, rawHex: '77CB', displayValue: 'CRC16，校验通过', source: 'code' },
  ],
  registers: [{ address: 0x354, identifier: null, rawHex: '000D', source: 'code' }],
  explanations: [],
  checksum: { kind: 'modbusCrc16', received: '6849', calculated: '6849', valid: true },
  warnings: [],
  error: null,
};

describe('MessageResultCard', () => {
  it('shows ordered protocol fields directly before the register table', () => {
    const view = render(<MantineProvider><MessageResultCard result={result} /></MantineProvider>);
    expect(screen.queryByText('从站 4 使用私有功能码 0x20。')).not.toBeInTheDocument();
    expect(screen.getByText('0x0354')).toBeInTheDocument();
    expect(screen.queryByText('电压')).not.toBeInTheDocument();
    expect(screen.queryByText('技术详情')).not.toBeInTheDocument();
    expect(screen.getByText(/仪表序列号 26062509330001/)).toBeInTheDocument();
    const fields = view.container.querySelector('.passthrough-fields');
    expect(fields).toHaveTextContent('68 3236303632353039333330303031 68：仪表序列号 26062509330001');
    expect(fields).toHaveTextContent('01：Modbus 地址 1');
    expect(fields).toHaveTextContent('10：写多个寄存器');
    expect(fields).toHaveTextContent('77CB：CRC16，校验通过');
  });

  it('labels missing values in a write-multiple response truthfully', () => {
    render(<MantineProvider><MessageResultCard result={{ ...result, registers: [{ address: 0xE000, identifier: null, rawHex: '', source: 'code' }] }} /></MantineProvider>);
    expect(screen.getByText('响应帧未携带写入值')).toBeInTheDocument();
  });

  it('always renders the register table when no rows are available', () => {
    render(<MantineProvider><MessageResultCard result={{ ...result, registers: [] }} /></MantineProvider>);
    expect(screen.getByRole('columnheader', { name: '地址/标识' })).toBeInTheDocument();
    expect(screen.getByText('未解析到寄存器')).toBeInTheDocument();
  });

  it('does not claim a register is missing from materials when no material was selected', () => {
    render(<MantineProvider><MessageResultCard result={result} /></MantineProvider>);
    expect(screen.queryByText('资料中未找到对应寄存器')).not.toBeInTheDocument();
  });

  it('shows a material lookup warning when the backend reports one', () => {
    render(<MantineProvider><MessageResultCard result={{
      ...result,
      explanations: [{
        address: 0x0354,
        parameterCode: null,
        parameterName: null,
        rawHex: '000D',
        convertedValue: null,
        unit: null,
        meaning: null,
        source: 'code',
        warnings: [{ code: 'register_not_found', message: '资料中未找到对应寄存器。' }],
      }],
    }} /></MantineProvider>);
    expect(screen.getByText('资料中未找到对应寄存器。')).toBeInTheDocument();
  });

  it('separates unit from meaning and renders every packed-field explanation', () => {
    render(<MantineProvider><MessageResultCard result={{
      ...result,
      registers: [{ address: 0x016D, identifier: null, rawHex: '0100', source: 'code' }],
      explanations: [
        { address: 0x016D, parameterCode: 'DOC_第1时段费率号', parameterName: '第1时段费率号', rawHex: '0100', convertedValue: '1', unit: null, meaning: '高字节', source: 'manual', warnings: [] },
        { address: 0x016D, parameterCode: 'DOC_第1时段起始分', parameterName: '第1时段起始分', rawHex: '0100', convertedValue: '0', unit: 'min', meaning: '低字节', source: 'manual', warnings: [] },
      ],
    }} /></MantineProvider>);
    expect(screen.getByRole('columnheader', { name: '解析值' })).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: '单位' })).toBeInTheDocument();
    expect(screen.getByRole('columnheader', { name: '说明' })).toBeInTheDocument();
    expect(screen.getByText('高字节')).toBeInTheDocument();
    expect(screen.getByText('低字节')).toBeInTheDocument();
    expect(screen.getAllByText('0x016D')).toHaveLength(2);
  });

  it('does not allow a result card to be compressed by the workspace flex layout', () => {
    const view = render(<MantineProvider><MessageResultCard result={result} /></MantineProvider>);
    expect(view.container.querySelector('.passthrough-result')).toHaveStyle({ flexShrink: '0' });
  });

  it('selects physical registers and opens merge parsing with their ordered raw bytes', async () => {
    render(<MantineProvider><MessageResultCard result={{ ...result, registers: [
      { address: 0x0014, identifier: null, rawHex: '089D', source: 'code' },
      { address: 0x0015, identifier: null, rawHex: '0896', source: 'code' },
    ] }} /></MantineProvider>);
    const merge = screen.getByRole('button', { name: '合并解析' });
    expect(merge).toBeDisabled();
    fireEvent.click(screen.getByRole('checkbox', { name: '选择寄存器 0x0014' }));
    fireEvent.click(screen.getByRole('checkbox', { name: '选择寄存器 0x0015' }));
    expect(merge).toBeEnabled();
    fireEvent.click(merge);
    expect(await screen.findByLabelText('合并 HEX')).toHaveValue('08 9D 08 96');
  });
});
