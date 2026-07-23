import { describe, expect, it } from 'vitest';
import { parseRegisterMergeHex } from './registerMerge';

describe('parseRegisterMergeHex', () => {
  it('parses all bytes while numeric types consume only their natural prefix width', () => {
    const result = parseRegisterMergeHex('08 9D\n08 96 08 A2 00 01');
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.hex).toBe('08 9D 08 96 08 A2 00 01');
    expect(result.ascii).toBe('········');
    expect(result.big.uint8).toBe('8');
    expect(result.big.uint16).toBe('2205');
    expect(result.little.int16).toBe('-25336');
    expect(result.little.uint16).toBe('40200');
    expect(result.little.uint32).toBe('2517146888');
    expect(result.big.uint64).not.toBeNull();
    expect(result.binary).toHaveLength(8);
    expect(result.binary[0]).toEqual({ hex: '08', bits: [false, false, false, false, true, false, false, false] });
  });

  it('rejects invalid and odd-length input and treats empty input as a valid empty result', () => {
    expect(parseRegisterMergeHex('08 GG')).toEqual({ ok: false, error: '请输入有效的十六进制字符。' });
    expect(parseRegisterMergeHex('089')).toEqual({ ok: false, error: 'HEX 字符数量必须为偶数。' });
    const empty = parseRegisterMergeHex('  \n');
    expect(empty.ok && empty.bytes).toEqual([]);
  });
});
