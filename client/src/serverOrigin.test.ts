import { describe, expect, it } from 'vitest';
import { normalizeOrigin, pickOrigin } from './serverOrigin';

describe('server origin', () => {
  it('normalizes user input to an origin', () => {
    expect(normalizeOrigin('')).toBe('');
    expect(normalizeOrigin('  ')).toBe('');
    expect(normalizeOrigin('game.example.org')).toBe('https://game.example.org');
    expect(normalizeOrigin('http://192.168.1.20:8080/')).toBe('http://192.168.1.20:8080');
    expect(normalizeOrigin('https://game.example.org:8443/play?x=1')).toBe('https://game.example.org:8443');
    expect(normalizeOrigin('ftp://game.example.org')).toBeNull();
    expect(normalizeOrigin('http://')).toBeNull();
  });

  it('prefers the URL, then the saved origin, then the build default', () => {
    expect(pickOrigin({ query: 'http://a:1', saved: 'http://b:2', buildDefault: 'http://c:3' })).toEqual({ origin: 'http://a:1', save: true });
    expect(pickOrigin({ query: null, saved: 'http://b:2', buildDefault: 'http://c:3' })).toEqual({ origin: 'http://b:2', save: false });
    expect(pickOrigin({ query: null, saved: null, buildDefault: 'http://c:3' })).toEqual({ origin: 'http://c:3', save: false });
    expect(pickOrigin({ query: null, saved: null, buildDefault: undefined })).toEqual({ origin: '', save: false });
    // `?server=` with nothing resets to the page's own origin; junk is ignored.
    expect(pickOrigin({ query: '', saved: 'http://b:2', buildDefault: undefined })).toEqual({ origin: '', save: true });
    expect(pickOrigin({ query: 'ftp://x', saved: 'http://b:2', buildDefault: undefined })).toEqual({ origin: 'http://b:2', save: false });
  });
});
