import fs from 'node:fs';
import path from 'node:path';

const MIME = { '.mp4': 'video/mp4', '.mov': 'video/quicktime', '.webm': 'video/webm', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp' };

export function mimeOf(p) {
  return MIME[path.extname(p).toLowerCase()] || 'application/octet-stream';
}

/** Read a media file. In dry-run a missing file becomes a placeholder so the
 * request flow can still be shown; live runs fail loudly. */
export function makeReadMedia({ live, log }) {
  return (p) => {
    if (fs.existsSync(p)) {
      const bytes = fs.readFileSync(p);
      return { bytes, size: bytes.length, mime: mimeOf(p), name: path.basename(p) };
    }
    if (live) throw new Error(`media file not found: ${p}`);
    log?.(`dry-run: media ${p} not found; using a placeholder`);
    const bytes = Buffer.alloc(16);
    return { bytes, size: bytes.length, mime: mimeOf(p), name: path.basename(p), placeholder: true };
  };
}

export function blobOf(media) {
  return new Blob([media.bytes], { type: media.mime });
}
