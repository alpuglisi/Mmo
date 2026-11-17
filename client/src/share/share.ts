// Sharing a card: the Web Share API with the PNG attached where the browser
// can share files (phones, the apps' WebViews), else sharing the link, else
// the fallbacks: download the PNG and copy the watch link.

/** What a browser offers (a `Navigator`, or a test double). */
export interface ShareNav {
  share?: (data: ShareData) => Promise<void>;
  canShare?: (data: ShareData) => boolean;
  clipboard?: { writeText(s: string): Promise<void> };
}

export type ShareMethod = 'files' | 'link' | 'download';

/** How a card can be shared here: with the file, as a link only, or by downloading it. */
export function shareMethod(nav: ShareNav, file: File | null): ShareMethod {
  if (nav.share && file && nav.canShare?.({ files: [file] })) return 'files';
  if (nav.share) return 'link';
  return 'download';
}

/** A safe file name from a title: lowercase words joined by dashes, `.png`. */
export function cardFileName(title: string, kind: string): string {
  const slug = title
    .normalize('NFKD')
    .replace(/[^\w\s-]/g, '')
    .trim()
    .toLowerCase()
    .replace(/[\s_]+/g, '-')
    .slice(0, 40);
  return `${slug || 'evolution'}-${kind}.png`;
}

/**
 * The read-only watch link of a world, following a lineage if given
 * (`?watch=<world>&lineage=<n>`; `ui/spectate.ts`). `base` is the page's URL.
 */
export function watchUrl(base: string, world: string, lineage: number | null): string {
  const u = new URL(base);
  u.search = '';
  u.hash = '';
  u.searchParams.set('watch', world || 'main');
  if (lineage !== null) u.searchParams.set('lineage', String(lineage));
  return u.toString();
}

export interface ShareRequest {
  blob: Blob;
  fileName: string;
  title: string;
  text: string;
  /** A link to go with it (the watch link). */
  url?: string;
}

export interface ShareOutcome {
  method: ShareMethod;
  /** The user closed the share sheet. */
  cancelled?: boolean;
  /** The link was copied (download fallback). */
  copied?: boolean;
}

/** Share `req` the best way this browser can. */
export async function shareCard(req: ShareRequest, nav: ShareNav = navigator): Promise<ShareOutcome> {
  const file = typeof File === 'function' ? new File([req.blob], req.fileName, { type: 'image/png' }) : null;
  const method = shareMethod(nav, file);
  try {
    if (method === 'files') {
      await nav.share!({ files: [file!], title: req.title, text: req.url ? `${req.text} ${req.url}` : req.text });
      return { method };
    }
    if (method === 'link' && req.url) {
      await nav.share!({ title: req.title, text: req.text, url: req.url });
      return { method };
    }
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') return { method, cancelled: true };
    // Refused (no user gesture, policy): fall back to a download.
  }
  downloadBlob(req.blob, req.fileName);
  let copied = false;
  if (req.url && nav.clipboard) {
    copied = await nav.clipboard.writeText(req.url).then(() => true, () => false);
  }
  return { method: 'download', copied };
}

/** Save a blob as a file (an `<a download>` click). */
export function downloadBlob(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.rel = 'noopener';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
