/**
 * Pure TypeScript Offline QR Code Generator (Model 2, Byte Mode, Error Correction Level L)
 * Completely local and self-contained — zero external network requests or dependencies.
 */

// Galois Field GF(2^8) tables and math with primitive polynomial 0x11D (285)
const GF_EXP = new Uint8Array(512);
const GF_LOG = new Uint8Array(256);

(() => {
  let val = 1;
  for (let i = 0; i < 255; i++) {
    GF_EXP[i] = val;
    GF_EXP[i + 255] = val;
    GF_LOG[val] = i;
    val = (val << 1) ^ (val & 0x80 ? 0x11d : 0);
  }
})();

function gfMul(x: number, y: number): number {
  if (x === 0 || y === 0) return 0;
  return GF_EXP[GF_LOG[x] + GF_LOG[y]];
}

function rsGeneratorPoly(degree: number): Uint8Array {
  let poly = new Uint8Array([1]);
  for (let i = 0; i < degree; i++) {
    const next = new Uint8Array(poly.length + 1);
    const alpha = GF_EXP[i];
    for (let j = 0; j < poly.length; j++) {
      next[j] ^= gfMul(poly[j], alpha);
      next[j + 1] ^= poly[j];
    }
    poly = next;
  }
  return poly;
}

function rsCompute(data: Uint8Array, numEc: number): Uint8Array {
  const gen = rsGeneratorPoly(numEc);
  const remainder = new Uint8Array(numEc);
  for (let i = 0; i < data.length; i++) {
    const factor = data[i] ^ remainder[numEc - 1];
    for (let j = numEc - 1; j > 0; j--) {
      remainder[j] = remainder[j - 1] ^ gfMul(gen[j], factor);
    }
    remainder[0] = gfMul(gen[0], factor);
  }
  const ec = new Uint8Array(numEc);
  for (let i = 0; i < numEc; i++) {
    ec[i] = remainder[numEc - 1 - i];
  }
  return ec;
}

interface VersionSpec {
  version: number;
  totalCodewords: number;
  dataCodewords: number;
  ecPerBlock: number;
  blocksGroup1: number;
  dataPerBlockGroup1: number;
  blocksGroup2: number;
  dataPerBlockGroup2: number;
  alignments: number[];
}

// Standard QR Model 2 EC Level L specifications (Versions 1-20)
const VERSIONS: VersionSpec[] = [
  { version: 1, totalCodewords: 26, dataCodewords: 19, ecPerBlock: 7, blocksGroup1: 1, dataPerBlockGroup1: 19, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [] },
  { version: 2, totalCodewords: 44, dataCodewords: 34, ecPerBlock: 10, blocksGroup1: 1, dataPerBlockGroup1: 34, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 18] },
  { version: 3, totalCodewords: 70, dataCodewords: 55, ecPerBlock: 15, blocksGroup1: 1, dataPerBlockGroup1: 55, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 22] },
  { version: 4, totalCodewords: 100, dataCodewords: 80, ecPerBlock: 20, blocksGroup1: 1, dataPerBlockGroup1: 80, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 26] },
  { version: 5, totalCodewords: 134, dataCodewords: 108, ecPerBlock: 26, blocksGroup1: 1, dataPerBlockGroup1: 108, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 30] },
  { version: 6, totalCodewords: 172, dataCodewords: 136, ecPerBlock: 18, blocksGroup1: 2, dataPerBlockGroup1: 68, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 34] },
  { version: 7, totalCodewords: 196, dataCodewords: 156, ecPerBlock: 20, blocksGroup1: 2, dataPerBlockGroup1: 78, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 22, 38] },
  { version: 8, totalCodewords: 242, dataCodewords: 194, ecPerBlock: 24, blocksGroup1: 2, dataPerBlockGroup1: 97, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 24, 42] },
  { version: 9, totalCodewords: 292, dataCodewords: 232, ecPerBlock: 30, blocksGroup1: 2, dataPerBlockGroup1: 116, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 26, 46] },
  { version: 10, totalCodewords: 346, dataCodewords: 274, ecPerBlock: 18, blocksGroup1: 2, dataPerBlockGroup1: 68, blocksGroup2: 2, dataPerBlockGroup2: 69, alignments: [6, 28, 50] },
  { version: 11, totalCodewords: 404, dataCodewords: 324, ecPerBlock: 20, blocksGroup1: 4, dataPerBlockGroup1: 81, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 30, 54] },
  { version: 12, totalCodewords: 466, dataCodewords: 370, ecPerBlock: 24, blocksGroup1: 2, dataPerBlockGroup1: 92, blocksGroup2: 2, dataPerBlockGroup2: 93, alignments: [6, 32, 58] },
  { version: 13, totalCodewords: 532, dataCodewords: 428, ecPerBlock: 26, blocksGroup1: 4, dataPerBlockGroup1: 107, blocksGroup2: 0, dataPerBlockGroup2: 0, alignments: [6, 34, 62] },
  { version: 14, totalCodewords: 581, dataCodewords: 461, ecPerBlock: 30, blocksGroup1: 3, dataPerBlockGroup1: 115, blocksGroup2: 1, dataPerBlockGroup2: 116, alignments: [6, 26, 46, 66] },
  { version: 15, totalCodewords: 655, dataCodewords: 523, ecPerBlock: 22, blocksGroup1: 5, dataPerBlockGroup1: 87, blocksGroup2: 1, dataPerBlockGroup2: 88, alignments: [6, 26, 48, 70] },
  { version: 16, totalCodewords: 733, dataCodewords: 589, ecPerBlock: 24, blocksGroup1: 5, dataPerBlockGroup1: 98, blocksGroup2: 1, dataPerBlockGroup2: 99, alignments: [6, 26, 50, 74] },
  { version: 17, totalCodewords: 815, dataCodewords: 647, ecPerBlock: 28, blocksGroup1: 1, dataPerBlockGroup1: 107, blocksGroup2: 5, dataPerBlockGroup2: 108, alignments: [6, 30, 54, 78] },
  { version: 18, totalCodewords: 901, dataCodewords: 721, ecPerBlock: 30, blocksGroup1: 5, dataPerBlockGroup1: 120, blocksGroup2: 1, dataPerBlockGroup2: 121, alignments: [6, 30, 56, 82] },
  { version: 19, totalCodewords: 991, dataCodewords: 795, ecPerBlock: 28, blocksGroup1: 3, dataPerBlockGroup1: 113, blocksGroup2: 4, dataPerBlockGroup2: 114, alignments: [6, 30, 58, 86] },
  { version: 20, totalCodewords: 1085, dataCodewords: 861, ecPerBlock: 28, blocksGroup1: 3, dataPerBlockGroup1: 107, blocksGroup2: 5, dataPerBlockGroup2: 108, alignments: [6, 34, 62, 90] },
];

function selectVersion(dataLength: number): VersionSpec {
  for (const v of VERSIONS) {
    const lengthBits = v.version >= 10 ? 16 : 8;
    const overheadBytes = Math.ceil((4 + lengthBits + 4) / 8);
    if (dataLength + overheadBytes <= v.dataCodewords) {
      return v;
    }
  }
  return VERSIONS[VERSIONS.length - 1];
}

class BitBuffer {
  private buffer: number[] = [];
  public length = 0;

  put(num: number, length: number) {
    for (let i = 0; i < length; i++) {
      this.putBit(((num >>> (length - i - 1)) & 1) === 1);
    }
  }

  putBit(bit: boolean) {
    const bufIndex = Math.floor(this.length / 8);
    if (this.buffer.length <= bufIndex) {
      this.buffer.push(0);
    }
    if (bit) {
      this.buffer[bufIndex] |= 0x80 >>> (this.length % 8);
    }
    this.length++;
  }

  getBytes(): Uint8Array {
    return new Uint8Array(this.buffer);
  }
}

function encodeData(utf8Bytes: Uint8Array, spec: VersionSpec): Uint8Array {
  const bb = new BitBuffer();
  // Byte mode indicator: 0100
  bb.put(4, 4);
  // Character count indicator
  const countBits = spec.version >= 10 ? 16 : 8;
  bb.put(utf8Bytes.length, countBits);
  // Data bytes
  for (let i = 0; i < utf8Bytes.length; i++) {
    bb.put(utf8Bytes[i], 8);
  }
  // Terminator: up to 4 zero bits
  const totalDataBits = spec.dataCodewords * 8;
  const remainingBits = totalDataBits - bb.length;
  bb.put(0, Math.min(4, Math.max(0, remainingBits)));
  // Align to 8-bit byte boundary
  while (bb.length % 8 !== 0) {
    bb.putBit(false);
  }
  // Pad bytes alternating 0xEC and 0x11
  let padToggle = false;
  while (bb.length < totalDataBits) {
    bb.put(padToggle ? 0x11 : 0xec, 8);
    padToggle = !padToggle;
  }

  const dataBytes = bb.getBytes();
  const blocks: Uint8Array[] = [];
  const ecBlocks: Uint8Array[] = [];

  let offset = 0;
  for (let i = 0; i < spec.blocksGroup1; i++) {
    const slice = dataBytes.subarray(offset, offset + spec.dataPerBlockGroup1);
    blocks.push(slice);
    ecBlocks.push(rsCompute(slice, spec.ecPerBlock));
    offset += spec.dataPerBlockGroup1;
  }
  for (let i = 0; i < spec.blocksGroup2; i++) {
    const slice = dataBytes.subarray(offset, offset + spec.dataPerBlockGroup2);
    blocks.push(slice);
    ecBlocks.push(rsCompute(slice, spec.ecPerBlock));
    offset += spec.dataPerBlockGroup2;
  }

  // Interleave data codewords
  const result: number[] = [];
  const maxDataLen = Math.max(spec.dataPerBlockGroup1, spec.dataPerBlockGroup2);
  for (let j = 0; j < maxDataLen; j++) {
    for (let i = 0; i < blocks.length; i++) {
      if (j < blocks[i].length) {
        result.push(blocks[i][j]);
      }
    }
  }

  // Interleave error correction codewords
  for (let j = 0; j < spec.ecPerBlock; j++) {
    for (let i = 0; i < ecBlocks.length; i++) {
      result.push(ecBlocks[i][j]);
    }
  }

  return new Uint8Array(result);
}

// 15-bit format strings for EC Level L (Masks 0 through 7)
const FORMAT_INFO_L = [
  0x77c4, 0x72f3, 0x7daa, 0x789d, 0x662f, 0x6318, 0x6c41, 0x6976,
];

function createMatrix(spec: VersionSpec, codewords: Uint8Array): boolean[][] {
  const size = spec.version * 4 + 17;
  const matrix: (boolean | null)[][] = Array.from({ length: size }, () =>
    Array(size).fill(null)
  );

  const setModule = (r: number, c: number, v: boolean) => {
    if (r >= 0 && r < size && c >= 0 && c < size) {
      matrix[r][c] = v;
    }
  };

  // 1. Finder patterns (7x7) + Separator
  const drawFinder = (row: number, col: number) => {
    for (let r = -1; r <= 7; r++) {
      for (let c = -1; c <= 7; c++) {
        const qr = row + r;
        const qc = col + c;
        if (qr < 0 || qr >= size || qc < 0 || qc >= size) continue;
        if (r >= 0 && r <= 6 && c >= 0 && c <= 6) {
          const isOuter = r === 0 || r === 6 || c === 0 || c === 6;
          const isInner = r >= 2 && r <= 4 && c >= 2 && c <= 4;
          matrix[qr][qc] = isOuter || isInner;
        } else {
          matrix[qr][qc] = false;
        }
      }
    }
  };

  drawFinder(0, 0);
  drawFinder(0, size - 7);
  drawFinder(size - 7, 0);

  // 2. Timing patterns
  for (let i = 8; i < size - 8; i++) {
    if (matrix[6][i] === null) matrix[6][i] = i % 2 === 0;
    if (matrix[i][6] === null) matrix[i][6] = i % 2 === 0;
  }

  // 3. Alignment patterns (version >= 2)
  const coords = spec.alignments;
  for (const r of coords) {
    for (const c of coords) {
      if (matrix[r][c] !== null) continue; // Skip finder areas
      for (let dr = -2; dr <= 2; dr++) {
        for (let dc = -2; dc <= 2; dc++) {
          const isEdge = Math.abs(dr) === 2 || Math.abs(dc) === 2;
          const isCenter = dr === 0 && dc === 0;
          matrix[r + dr][c + dc] = isEdge || isCenter;
        }
      }
    }
  }

  // 4. Dark module
  matrix[4 * spec.version + 9][8] = true;

  // 5. Reserve format info areas
  for (let i = 0; i < 9; i++) {
    if (matrix[8][i] === null) matrix[8][i] = false;
    if (matrix[i][8] === null) matrix[i][8] = false;
  }
  for (let i = size - 8; i < size; i++) {
    if (matrix[8][i] === null) matrix[8][i] = false;
    if (matrix[i][8] === null) matrix[i][8] = false;
  }

  // 6. Place data bits in zigzag pattern
  let bitIdx = 0;
  const totalBits = codewords.length * 8;
  let upwards = true;

  for (let right = size - 1; right > 0; right -= 2) {
    if (right === 6) right--; // Skip vertical timing column
    const cols = [right, right - 1];

    for (let step = 0; step < size; step++) {
      const r = upwards ? size - 1 - step : step;
      for (const c of cols) {
        if (matrix[r][c] === null) {
          let bit = false;
          if (bitIdx < totalBits) {
            const byte = codewords[Math.floor(bitIdx / 8)];
            bit = ((byte >>> (7 - (bitIdx % 8))) & 1) === 1;
            bitIdx++;
          }
          // Mask 0: (row + col) % 2 == 0
          if ((r + c) % 2 === 0) {
            bit = !bit;
          }
          matrix[r][c] = bit;
        }
      }
    }
    upwards = !upwards;
  }

  // 7. Write format info (Mask 0, EC L)
  const format = FORMAT_INFO_L[0];
  const formatBits: boolean[] = [];
  for (let i = 0; i < 15; i++) {
    formatBits.push(((format >>> (14 - i)) & 1) === 1);
  }

  // Top-left finder
  setModule(8, 0, formatBits[0]);
  setModule(8, 1, formatBits[1]);
  setModule(8, 2, formatBits[2]);
  setModule(8, 3, formatBits[3]);
  setModule(8, 4, formatBits[4]);
  setModule(8, 5, formatBits[5]);
  setModule(8, 7, formatBits[6]);
  setModule(8, 8, formatBits[7]);
  setModule(7, 8, formatBits[8]);
  setModule(5, 8, formatBits[9]);
  setModule(4, 8, formatBits[10]);
  setModule(3, 8, formatBits[11]);
  setModule(2, 8, formatBits[12]);
  setModule(1, 8, formatBits[13]);
  setModule(0, 8, formatBits[14]);

  // Top-right and bottom-left split
  for (let i = 0; i < 7; i++) {
    setModule(8, size - 1 - i, formatBits[14 - i]);
  }
  for (let i = 0; i < 8; i++) {
    setModule(size - 8 + i, 8, formatBits[7 - i]);
  }

  return matrix.map((row) => row.map((cell) => cell ?? false));
}

/**
 * Generates an offline SVG QR Code data URL (zero network requests, pure TypeScript).
 */
export function generateQrSvg(text: string, size = 200): string {
  const encoder = new TextEncoder();
  const utf8 = encoder.encode(text);
  const spec = selectVersion(utf8.length);
  const codewords = encodeData(utf8, spec);
  const matrix = createMatrix(spec, codewords);

  const moduleCount = matrix.length;
  const margin = 2;
  const totalDim = moduleCount + margin * 2;

  let pathData = '';
  for (let r = 0; r < moduleCount; r++) {
    for (let c = 0; c < moduleCount; c++) {
      if (matrix[r][c]) {
        pathData += `M${c + margin},${r + margin}h1v1h-1z`;
      }
    }
  }

  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${totalDim} ${totalDim}" width="${size}" height="${size}"><rect width="100%" height="100%" fill="#ffffff"/><path d="${pathData}" fill="#000000"/></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
