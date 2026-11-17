import { describe, expect, it } from 'vitest';
import { cardFileName, shareCard, shareMethod, watchUrl, type ShareNav } from './share';

const file = new File([new Uint8Array([1, 2, 3])], 'x.png', { type: 'image/png' });

describe('shareMethod', () => {
  it('shares the file where the browser can, else the link, else downloads', () => {
    const share = async () => {};
    expect(shareMethod({ share, canShare: () => true }, file)).toBe('files');
    expect(shareMethod({ share, canShare: () => false }, file)).toBe('link');
    expect(shareMethod({ share }, file)).toBe('link');
    expect(shareMethod({}, file)).toBe('download');
    expect(shareMethod({ share, canShare: () => true }, null)).toBe('link');
  });
});

describe('cardFileName', () => {
  it('makes a safe, short slug', () => {
    expect(cardFileName('Ann\'s Striders!', 'tree')).toBe('anns-striders-tree.png');
    expect(cardFileName('  Éclair  café ', 'portrait')).toBe('eclair-cafe-portrait.png');
    expect(cardFileName('???', 'tree')).toBe('evolution-tree.png');
    expect(cardFileName('x'.repeat(100), 'tree').length).toBeLessThanOrEqual(40 + '-tree.png'.length);
  });
});

describe('watchUrl', () => {
  it('replaces the query with the watch parameters', () => {
    expect(watchUrl('https://evo.example/?world=isle&tab=family#x', 'isle', 4)).toBe('https://evo.example/?watch=isle&lineage=4');
    expect(watchUrl('https://evo.example/app/', '', null)).toBe('https://evo.example/app/?watch=main');
  });
});

describe('shareCard', () => {
  const req = { blob: new Blob([new Uint8Array([1])], { type: 'image/png' }), fileName: 'a.png', title: 'A', text: 'Look', url: 'https://x/?watch=main' };

  it('passes the file and the link to the share sheet', async () => {
    const calls: ShareData[] = [];
    const nav: ShareNav = { share: async (d) => void calls.push(d), canShare: () => true };
    expect(await shareCard(req, nav)).toEqual({ method: 'files' });
    expect(calls[0].files?.[0].name).toBe('a.png');
    expect(calls[0].text).toContain(req.url);
  });

  it('treats a closed share sheet as cancelled, not as a failure', async () => {
    const nav: ShareNav = { share: async () => { throw new DOMException('closed', 'AbortError'); }, canShare: () => true };
    expect(await shareCard(req, nav)).toEqual({ method: 'files', cancelled: true });
  });
});
