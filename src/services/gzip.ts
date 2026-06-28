export function gzipBytes(input: Uint8Array): Uint8Array {
  const bytes: number[] = [
    0x1f, 0x8b,
    0x08,
    0x00,
    0x00, 0x00, 0x00, 0x00,
    0x00,
    0xff
  ];

  if (input.length === 0) {
    writeStoredBlock(bytes, input, 0, 0, true);
  } else {
    for (let offset = 0; offset < input.length;) {
      const length = Math.min(65_535, input.length - offset);
      const final = offset + length >= input.length;
      writeStoredBlock(bytes, input, offset, length, final);
      offset += length;
    }
  }

  writeUint32LittleEndian(bytes, crc32(input));
  writeUint32LittleEndian(bytes, input.length >>> 0);
  return Uint8Array.from(bytes);
}

function writeStoredBlock(
  output: number[],
  input: Uint8Array,
  offset: number,
  length: number,
  final: boolean
) {
  output.push(final ? 0x01 : 0x00);
  output.push(length & 0xff, (length >>> 8) & 0xff);
  const inverse = (~length) & 0xffff;
  output.push(inverse & 0xff, (inverse >>> 8) & 0xff);
  for (let index = 0; index < length; index += 1) {
    output.push(input[offset + index]);
  }
}

function writeUint32LittleEndian(output: number[], value: number) {
  output.push(
    value & 0xff,
    (value >>> 8) & 0xff,
    (value >>> 16) & 0xff,
    (value >>> 24) & 0xff
  );
}

function crc32(input: Uint8Array) {
  let crc = 0xffffffff;
  for (const byte of input) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}
