// Retrying a login or registration the server refused for load, not for
// what was sent: 503 `busy` (too many password hashes queued) or 429
// `rate_limited`, both with `retry_after_ms`. The wait is the server's hint
// or a doubling backoff, whichever is longer, with jitter so a crowd of
// clients refused together doesn't come back together.

import { ApiError } from './api';

export interface RetryOptions {
  /** Retries after the first try. */
  retries?: number;
  /** First backoff, ms (doubles each retry). */
  baseMs?: number;
  /** Longest wait, ms; a server hint longer than this is not waited for. */
  maxMs?: number;
  /** In [0, 1): jitter. */
  random?: () => number;
  sleep?: (ms: number) => Promise<void>;
  /** Called before each wait. */
  onWait?: (ms: number, retry: number) => void;
}

/** Whether the error is the server asking to come back later. */
export function isRetryable(e: unknown): e is ApiError {
  return e instanceof ApiError && ((e.status === 503 && e.code === 'busy') || (e.status === 429 && e.code === 'rate_limited'));
}

/**
 * The wait before retry number `retry` (0-based) after error `e`, or null
 * when it should not be retried.
 */
export function retryDelay(e: unknown, retry: number, opts: RetryOptions = {}): number | null {
  const { retries = 5, baseMs = 500, maxMs = 10_000, random = Math.random } = opts;
  if (retry >= retries || !isRetryable(e)) return null;
  const hint = typeof e.body?.retry_after_ms === 'number' ? e.body.retry_after_ms : 0;
  if (hint > maxMs) return null;
  const wait = Math.min(maxMs, Math.max(hint, baseMs * 2 ** retry));
  return Math.round(wait * (0.5 + random()));
}

/** Run `fn`, retrying while the server says it is busy (see `retryDelay`). */
export async function withRetry<T>(fn: () => Promise<T>, opts: RetryOptions = {}): Promise<T> {
  const sleep = opts.sleep ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
  for (let retry = 0; ; retry++) {
    try {
      return await fn();
    } catch (e) {
      const ms = retryDelay(e, retry, opts);
      if (ms === null) throw e;
      opts.onWait?.(ms, retry);
      await sleep(ms);
    }
  }
}
