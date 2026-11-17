// A small animated-GIF encoder, for capture.mjs when ffmpeg is missing.
// Frames are RGBA; colours go to a fixed 6×7×6 palette (252 colours, with a
// 4×4 ordered dither so gradients don't band), then GIF89a LZW. No
// dependencies.

const LEVELS = [6, 7, 6];

/** The palette: 6 reds × 7 greens × 6 blues, padded to 256 entries. */
export function palette() {
  const p = new Uint8Array(256 * 3);
  let i = 0;
  for (let r = 0; r < LEVELS[0]; r++)
    for (let g = 0; g < LEVELS[1]; g++)
      for (let b = 0; b < LEVELS[2]; b++) {
        p[i++] = Math.round((r * 255) / (LEVELS[0] - 1));
        p[i++] = Math.round((g * 255) / (LEVELS[1] - 1));
        p[i++] = Math.round((b * 255) / (LEVELS[2] - 1));
      }
  return p;
}

const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/** Palette indices of an RGBA frame (ordered dither). */
export function quantize(rgba, width, height) {
  const out = new Uint8Array(width * height);
  const [lr, lg, lb] = LEVELS.map((l) => l - 1);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = y * width + x;
      const t = (BAYER[(y & 3) * 4 + (x & 3)] + 0.5) / 16 - 0.5;
      const q = (v, l) => Math.min(l, Math.max(0, Math.round((v / 255) * l + t)));
      const r = q(rgba[i * 4], lr), g = q(rgba[i * 4 + 1], lg), b = q(rgba[i * 4 + 2], lb);
      out[i] = (r * LEVELS[1] + g) * LEVELS[2] + b;
    }
  }
  return out;
}

/** GIF LZW of 8-bit indices, as sub-blocks. */
export function lzw(indices) {
  const minCode = 8;
  const clear = 1 << minCode, end = clear + 1;
  const bytes = [];
  let cur = 0, bits = 0;
  let size = minCode + 1;
  const emit = (code) => {
    cur |= code << bits;
    bits += size;
    while (bits >= 8) {
      bytes.push(cur & 255);
      cur >>>= 8;
      bits -= 8;
    }
  };
  let dict = new Map();
  let next = end + 1;
  emit(clear);
  let prefix = indices[0];
  for (let i = 1; i < indices.length; i++) {
    const k = indices[i];
    const key = prefix * 256 + k;
    const hit = dict.get(key);
    if (hit !== undefined) {
      prefix = hit;
      continue;
    }
    emit(prefix);
    if (next < 4096) {
      dict.set(key, next++);
      if (next > 1 << size && size < 12) size++;
    } else {
      emit(clear);
      dict = new Map();
      next = end + 1;
      size = minCode + 1;
    }
    prefix = k;
  }
  emit(prefix);
  emit(end);
  if (bits > 0) bytes.push(cur & 255);
  const out = [minCode];
  for (let i = 0; i < bytes.length; i += 255) {
    const n = Math.min(255, bytes.length - i);
    out.push(n, ...bytes.slice(i, i + n));
  }
  out.push(0);
  return out;
}

/**
 * An animated GIF of `frames` ({ rgba: Uint8Array|Uint8ClampedArray }) of
 * `width` × `height`, `delayMs` per frame, looping forever.
 */
export function encodeGif(frames, width, height, delayMs) {
  const parts = [];
  const u16 = (v) => [v & 255, (v >> 8) & 255];
  parts.push(Buffer.from('GIF89a', 'ascii'));
  // Logical screen: global colour table of 256 entries.
  parts.push(Buffer.from([...u16(width), ...u16(height), 0xf7, 0, 0]));
  parts.push(Buffer.from(palette()));
  // Loop forever (NETSCAPE2.0).
  parts.push(Buffer.from([0x21, 0xff, 11, ...Buffer.from('NETSCAPE2.0', 'ascii'), 3, 1, 0, 0, 0]));
  const delay = Math.max(2, Math.round(delayMs / 10));
  for (const f of frames) {
    parts.push(Buffer.from([0x21, 0xf9, 4, 0, ...u16(delay), 0, 0]));
    parts.push(Buffer.from([0x2c, 0, 0, 0, 0, ...u16(width), ...u16(height), 0]));
    parts.push(Buffer.from(lzw(quantize(f.rgba, width, height))));
  }
  parts.push(Buffer.from([0x3b]));
  return Buffer.concat(parts);
}
