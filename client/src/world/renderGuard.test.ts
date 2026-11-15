import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ContextGuard, ErrorLog, type GuardHooks } from './renderGuard';

const lose = (c: EventTarget) => {
  const e = new Event('webglcontextlost', { cancelable: true });
  c.dispatchEvent(e);
  return e;
};
const restore = (c: EventTarget) => c.dispatchEvent(new Event('webglcontextrestored'));

describe('ContextGuard', () => {
  let calls: string[];
  let hooks: GuardHooks;
  let recreate: () => Promise<void>;
  const silent = new ErrorLog(5000, () => 0, () => {});

  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.spyOn(console, 'info').mockImplementation(() => {});
    calls = [];
    recreate = async () => {
      calls.push('recreate');
    };
    hooks = {
      onLost: () => calls.push('lost'),
      onRestored: () => calls.push('restored'),
      recreate: () => recreate(),
    };
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('pauses on loss and rebuilds on restore', () => {
    const g = new ContextGuard(hooks, { log: silent });
    const canvas = new EventTarget();
    g.attach(canvas);
    expect(g.paused).toBe(false);
    const e = lose(canvas);
    expect(e.defaultPrevented).toBe(true); // or the browser never restores
    expect(g.state).toBe('lost');
    expect(g.paused).toBe(true);
    lose(canvas); // a second loss event is not a second loss
    expect(calls).toEqual(['lost']);
    restore(canvas);
    expect(g.state).toBe('live');
    expect(calls).toEqual(['lost', 'restored']);
    // Restored in time: no new renderer.
    vi.advanceTimersByTime(10_000);
    expect(calls).toEqual(['lost', 'restored']);
    expect(g.stats).toEqual({ lost: 1, restored: 1, recreated: 0 });
  });

  it('recreates the renderer when no restore comes in time', async () => {
    const g = new ContextGuard(hooks, { log: silent, restoreTimeoutMs: 3000 });
    const canvas = new EventTarget();
    g.attach(canvas);
    lose(canvas);
    vi.advanceTimersByTime(2999);
    expect(calls).toEqual(['lost']);
    await vi.advanceTimersByTimeAsync(1);
    expect(calls).toEqual(['lost', 'recreate']);
    expect(g.state).toBe('live');
    expect(g.stats.recreated).toBe(1);
    // A late restore of the old context is ignored.
    restore(canvas);
    expect(calls).toEqual(['lost', 'recreate']);
  });

  it('retries a failed recreate, staying paused meanwhile', async () => {
    let fail = 2;
    recreate = async () => {
      calls.push('recreate');
      if (fail-- > 0) throw new Error('no WebGL');
    };
    const g = new ContextGuard(hooks, { log: silent, restoreTimeoutMs: 1000, retryMs: 500 });
    g.lost('test');
    await vi.advanceTimersByTimeAsync(1000);
    expect(g.state).toBe('lost');
    expect(g.paused).toBe(true);
    await vi.advanceTimersByTimeAsync(500);
    expect(g.state).toBe('lost');
    await vi.advanceTimersByTimeAsync(500);
    expect(g.state).toBe('live');
    expect(calls).toEqual(['lost', 'recreate', 'recreate', 'recreate']);
    expect(silent.counts.get('recreate renderer')).toBe(2);
  });

  it('treats a failed rebuild after restore as a loss', async () => {
    let failRebuild = true;
    hooks.onRestored = () => {
      calls.push('restored');
      if (failRebuild) throw new Error('shader source');
    };
    const g = new ContextGuard(hooks, { log: silent, restoreTimeoutMs: 1000 });
    g.lost('test');
    g.restored();
    expect(g.state).toBe('lost');
    failRebuild = false;
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toEqual(['lost', 'restored', 'lost', 'recreate']);
    expect(g.state).toBe('live');
  });

  it('follows only the latest canvas, and stops on dispose', async () => {
    const g = new ContextGuard(hooks, { log: silent, restoreTimeoutMs: 1000 });
    const a = new EventTarget();
    const b = new EventTarget();
    g.attach(a);
    g.attach(b);
    lose(a);
    expect(g.state).toBe('live');
    lose(b);
    expect(g.state).toBe('lost');
    g.dispose();
    await vi.advanceTimersByTimeAsync(5000);
    restore(b);
    expect(calls).toEqual(['lost']);
  });

  it('contains an error in the loss hook', () => {
    hooks.onLost = () => {
      throw new Error('destroy failed');
    };
    const log = new ErrorLog(5000, () => 0, () => {});
    const g = new ContextGuard(hooks, { log });
    g.lost('test');
    expect(g.state).toBe('lost');
    expect(log.counts.get('context lost')).toBe(1);
  });
});

describe('ErrorLog', () => {
  it('logs the first error of a label at once and throttles repeats', () => {
    let t = 0;
    const lines: string[] = [];
    const log = new ErrorLog(5000, () => t, (msg) => lines.push(msg));
    for (let i = 0; i < 100; i++) log.report('render', new Error('x'));
    log.report('frame', new Error('y'));
    expect(lines).toEqual(['[world] render failed:', '[world] frame failed:']);
    t = 6000;
    log.report('render', new Error('x'));
    expect(lines.at(-1)).toBe('[world] render failed (+99 more since):');
    expect(log.counts.get('render')).toBe(101);
  });

  it('runs a function, reporting instead of throwing', () => {
    const log = new ErrorLog(5000, () => 0, () => {});
    expect(log.run('ok', () => {})).toBe(true);
    expect(log.run('bad', () => { throw new Error('boom'); })).toBe(false);
    expect(log.counts.get('bad')).toBe(1);
  });
});
