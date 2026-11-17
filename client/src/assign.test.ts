import { describe, expect, it } from 'vitest';
import { askAssignment, chooseWorld, lobbyOrigin, parseAssignment, type Assignment } from './assign';

const ok = (body: unknown, status = 200) =>
  (async () => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })) as unknown as typeof fetch;

describe('parseAssignment', () => {
  it('reads server and lobby answers', () => {
    expect(parseAssignment({ world: 'isle', reason: 'fill', players_online: 3, capacity: 10 })).toEqual({ world: 'isle', reason: 'fill' });
    expect(parseAssignment({ server: 'http://b:8080/', world: 'w', reason: 'lineage' })).toEqual({ server: 'http://b:8080', world: 'w', reason: 'lineage' });
  });
  it('refuses anything else', () => {
    for (const v of [null, 3, {}, { world: '' , reason: 'fill' }, { world: 'a', reason: 'other' }, { world: 'a', reason: 'fill', server: 'ftp://x' }]) {
      expect(parseAssignment(v)).toBeNull();
    }
  });
});

describe('chooseWorld', () => {
  const a = (world: string, reason: Assignment['reason']): Assignment => ({ world, reason });
  it('puts the player first, then their lineage, then memory, then the fill', () => {
    expect(chooseWorld({ picked: 'p', assigned: a('l', 'lineage'), remembered: 'm' })).toBe('p');
    expect(chooseWorld({ picked: null, assigned: a('l', 'lineage'), remembered: 'm' })).toBe('l');
    expect(chooseWorld({ picked: null, assigned: a('f', 'fill'), remembered: 'm' })).toBe('m');
    expect(chooseWorld({ picked: null, assigned: a('f', 'spread'), remembered: null })).toBe('f');
    expect(chooseWorld({ picked: null, assigned: null, remembered: null })).toBeNull();
  });
});

describe('lobbyOrigin', () => {
  it('takes the query (remembered), then the saved one, then the build default', () => {
    expect(lobbyOrigin({ query: 'lobby.example.org', saved: 'http://old', buildDefault: 'http://d' })).toEqual({ origin: 'https://lobby.example.org', save: true });
    expect(lobbyOrigin({ query: null, saved: 'http://old:8000', buildDefault: 'http://d' })).toEqual({ origin: 'http://old:8000', save: false });
    expect(lobbyOrigin({ query: null, saved: null, buildDefault: 'http://d' })).toEqual({ origin: 'http://d', save: false });
    expect(lobbyOrigin({ query: null, saved: null, buildDefault: undefined })).toEqual({ origin: null, save: false });
  });
});

describe('askAssignment', () => {
  it('asks the lobby when there is one, and keeps its server', async () => {
    let asked = '';
    const f = (async (url: string) => { asked = url; return ok({ server: 'http://b:1', world: 'w', reason: 'fill' })(url); }) as unknown as typeof fetch;
    const a = await askAssignment({ name: 'ann', server: 'http://a:1', lobby: 'http://lobby:8000', fetchFn: f });
    expect(asked).toBe('http://lobby:8000/lobby/assign?name=ann');
    expect(a).toEqual({ server: 'http://b:1', world: 'w', reason: 'fill' });
  });
  it('else asks the game server, and ignores a server it names', async () => {
    let asked = '';
    const f = (async (url: string) => { asked = url; return ok({ server: 'http://elsewhere', world: 'main', reason: 'lineage' })(url); }) as unknown as typeof fetch;
    const a = await askAssignment({ name: ' bo b', server: '', lobby: null, fetchFn: f });
    expect(asked).toBe('/api/assign?name=bo%20b');
    expect(a).toEqual({ world: 'main', reason: 'lineage' });
  });
  it('falls back to nothing on errors, full worlds and slow answers', async () => {
    expect(await askAssignment({ name: 'a', server: '', lobby: null, fetchFn: ok({ code: 'full' }, 503) })).toBeNull();
    const down = (async () => { throw new TypeError('network'); }) as unknown as typeof fetch;
    expect(await askAssignment({ name: 'a', server: '', lobby: null, fetchFn: down })).toBeNull();
    const slow = ((_u: string, init?: RequestInit) => new Promise<Response>((_, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new Error('aborted')));
    })) as unknown as typeof fetch;
    expect(await askAssignment({ name: 'a', server: '', lobby: null, fetchFn: slow, timeoutMs: 20 })).toBeNull();
  });
});
