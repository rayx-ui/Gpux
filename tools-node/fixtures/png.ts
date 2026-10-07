import { inflateSync } from 'node:zlib';

/** A decoded image: `data` holds RGBA bytes row by row. */
export interface Image {
  width: number;
  height: number;
  data: Uint8Array;
}

/** Decodes the 8-bit, non-interlaced PNGs Chromium's screenshots produce. */
export function decodePng(png: Buffer): Image {
  if (png.readUInt32BE(0) !== 0x89504e47) throw new Error('not a PNG');
  let offset = 8;
  let width = 0;
  let height = 0;
  let colorType = 0;
  const compressed: Buffer[] = [];
  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString('ascii', offset + 4, offset + 8);
    const body = png.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      const depth = body[8];
      colorType = body[9];
      if (depth !== 8 || body[12] !== 0) {
        throw new Error(`unsupported PNG: depth ${depth}, interlace ${body[12]}`);
      }
    } else if (type === 'IDAT') {
      compressed.push(body);
    } else if (type === 'IEND') {
      break;
    }
    offset += 12 + length;
  }
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[colorType];
  if (!channels) throw new Error(`unsupported PNG color type ${colorType}`);
  const raw = inflateSync(Buffer.concat(compressed));
  const stride = width * channels;
  const rows = new Uint8Array(stride * height);
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let x = 0; x < stride; x++) {
      const left = x >= channels ? rows[y * stride + x - channels] : 0;
      const up = y > 0 ? rows[(y - 1) * stride + x] : 0;
      const upLeft = x >= channels && y > 0 ? rows[(y - 1) * stride + x - channels] : 0;
      let predictor = 0;
      switch (filter) {
        case 0:
          break;
        case 1:
          predictor = left;
          break;
        case 2:
          predictor = up;
          break;
        case 3:
          predictor = (left + up) >> 1;
          break;
        case 4: {
          const p = left + up - upLeft;
          const pa = Math.abs(p - left);
          const pb = Math.abs(p - up);
          const pc = Math.abs(p - upLeft);
          predictor = pa <= pb && pa <= pc ? left : pb <= pc ? up : upLeft;
          break;
        }
        default:
          throw new Error(`bad PNG filter ${filter}`);
      }
      rows[y * stride + x] = (line[x] + predictor) & 0xff;
    }
  }
  const data = new Uint8Array(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    const pixel = rows.subarray(i * channels, (i + 1) * channels);
    const [r, g, b, a] =
      channels === 1
        ? [pixel[0], pixel[0], pixel[0], 255]
        : channels === 2
          ? [pixel[0], pixel[0], pixel[0], pixel[1]]
          : channels === 3
            ? [pixel[0], pixel[1], pixel[2], 255]
            : [pixel[0], pixel[1], pixel[2], pixel[3]];
    data.set([r, g, b, a], i * 4);
  }
  return { width, height, data };
}

/** The RGBA color at a pixel. */
export function pixel(image: Image, x: number, y: number): [number, number, number, number] {
  const i = (y * image.width + x) * 4;
  return [image.data[i], image.data[i + 1], image.data[i + 2], image.data[i + 3]];
}

/** How many distinct colors an image holds, up to `limit`. */
export function distinctColors(image: Image, limit = 64): number {
  const seen = new Set<number>();
  for (let i = 0; i < image.data.length && seen.size < limit; i += 4) {
    seen.add(
      (image.data[i] << 24) | (image.data[i + 1] << 16) | (image.data[i + 2] << 8) | image.data[i + 3],
    );
  }
  return seen.size;
}
