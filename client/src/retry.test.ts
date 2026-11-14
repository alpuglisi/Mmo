import { describe, expect, it } from 'vitest';
import { ApiError } from './api';
import { isRetryable, retryDelay, withRetry } from './retry';

const busy = (ms?: number) => new ApiError(503, 'busy', 'busy', { code: 'busy', message: 'busy', ...(ms === undefined ? {} : { retry_after_ms: ms }) });
const limited = (ms: number) => new ApiError(429, 'rate_limited', 'slow down', { code: 'rate_limited', message: '', retry_after_ms: ms });
const half = () => 0.5; // no jitter

describe('login retry', () => {
  it('retries only load refusals', () => {
    expect(isRetryable(busy(300))).toBe(true);
    expect(isRetryable(limited(2000))).toBe(true);
    expect(isRetryable(new ApiError(401, 'bad_credentials', 'no', null))).toBe(false);
    expect(isRetryable(new ApiError(409, 'name_taken', 'no', null))).toBe(false);
    expect(isRetryable(new ApiError(0, 'network', 'down', null))).toBe(false);
    expect(isRetryable(new Error('x'))).toBe(false);
  });

  it('waits for the hint or a doubling backoff, with jitter and a cap', () => {
    expect(retryDelay(busy(), 0, { random: half })).toBe(500);
    expect(retryDelay(busy(), 2, { random: half })).toBe(2000);
    expect(retryDelay(busy(3000), 0, { random: half })).toBe(3000);
    expect(retryDelay(busy(), 6, { random: half, retries: 10 })).toBe(10_000);
    expect(retryDelay(busy(1000), 0, { random: () => 0 })).toBe(500);
    expect(retryDelay(busy(1000), 0, { random: () => 0.999 })).toBe(1499);
    // Out of retries, or a hint too long to wait for (the user tries later).
    expect(retryDelay(busy(), 5, { random: half })).toBeNull();
    expect(retryDelay(limited(60_000), 0, { random: half })).toBeNull();
    expect(retryDelay(new ApiError(401, 'bad_credentials', 'no', null), 0)).toBeNull();
  });

  it('retries until the call succeeds or gives up', async () => {
    const waits: number[] = [];
    const sleep = async (ms: number) => { waits.push(ms); };
    let calls = 0;
    const ok = await withRetry(async () => {
      calls++;
      if (calls < 3) throw busy(250);
      return 'token';
    }, { random: half, sleep });
    expect(ok).toBe('token');
    expect(waits).toEqual([500, 1000]);

    calls = 0;
    const err = await withRetry(async () => { calls++; throw busy(); }, { random: half, sleep, retries: 2 }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(calls).toBe(3);

    calls = 0;
    await expect(withRetry(async () => { calls++; throw new ApiError(401, 'bad_credentials', 'no', null); }, { sleep })).rejects.toThrow('no');
    expect(calls).toBe(1);
  });
});
