export interface NumericInterpretation {
  int8: string | null;
  uint8: string | null;
  int16: string | null;
  uint16: string | null;
  int32: string | null;
  uint32: string | null;
  int64: string | null;
  uint64: string | null;
}

export type RegisterMergeResult = {
  ok: true;
  bytes: number[];
  hex: string;
  ascii: string;
  binary: Array<{ hex: string; bits: boolean[] }>;
  big: NumericInterpretation;
  little: NumericInterpretation;
} | { ok: false; error: string };

function readUnsigned(bytes: number[], width: number, littleEndian: boolean): bigint | null {
  if (bytes.length < width) return null;
  const ordered = bytes.slice(0, width);
  if (littleEndian) ordered.reverse();
  return ordered.reduce((value, byte) => (value << 8n) | BigInt(byte), 0n);
}

function values(bytes: number[], littleEndian: boolean): NumericInterpretation {
  const value = (width: number, signed: boolean): string | null => {
    const unsigned = readUnsigned(bytes, width, littleEndian);
    if (unsigned == null) return null;
    if (!signed) return unsigned.toString();
    const bits = BigInt(width * 8);
    const sign = 1n << (bits - 1n);
    return (unsigned >= sign ? unsigned - (1n << bits) : unsigned).toString();
  };
  return {
    int8: value(1, true), uint8: value(1, false),
    int16: value(2, true), uint16: value(2, false),
    int32: value(4, true), uint32: value(4, false),
    int64: value(8, true), uint64: value(8, false),
  };
}

export function parseRegisterMergeHex(input: string): RegisterMergeResult {
  const compact = input.replace(/[\s]/g, '');
  if (/[^0-9a-f]/i.test(compact)) return { ok: false, error: '请输入有效的十六进制字符。' };
  if (compact.length % 2 !== 0) return { ok: false, error: 'HEX 字符数量必须为偶数。' };
  const bytes = compact.match(/../g)?.map((pair) => Number.parseInt(pair, 16)) ?? [];
  return {
    ok: true,
    bytes,
    hex: bytes.map((byte) => byte.toString(16).toUpperCase().padStart(2, '0')).join(' '),
    ascii: bytes.map((byte) => byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : '·').join(''),
    binary: bytes.map((byte) => ({
      hex: byte.toString(16).toUpperCase().padStart(2, '0'),
      bits: Array.from({ length: 8 }, (_, index) => (byte & (1 << (7 - index))) !== 0),
    })),
    big: values(bytes, false),
    little: values(bytes, true),
  };
}
