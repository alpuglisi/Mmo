// The game WebSocket: typed messages, request ids paired with `result`
// replies, reconnect with exponential backoff and a connection state.
// Protocol 2 binary delta states (wire.ts) are rebuilt into whole `state`
// messages here, so the rest of the client sees the same messages from any
// server.

import { parseServerMsg, type ClientMsg, type ResultMsg, type ServerMsg } from './protocol';
import { DeltaDecoder } from './wire';

export type ConnState = 'connecting' | 'open' | 'reconnecting' | 'closed';

export interface SocketOptions {
  /** Builds the URL for each attempt (the token may change). */
  url: () => string;
  onMessage: (m: ServerMsg) => void;
  onState: (s: ConnState, detail: { attempt: number; retryInMs?: number }) => void;
  /**
   * Called when an attempt closes without ever opening. Return false to stop
   * retrying (e.g. the token was rejected).
   */
  onFailedAttempt?: (attempt: number) => Promise<boolean> | boolean;
  /** Overridable for tests. */
  WebSocketImpl?: typeof WebSocket;
  minDelayMs?: number;
  maxDelayMs?: number;
  resultTimeoutMs?: number;
  /** Least time between two `resync` requests while delta states don't decode. */
  resyncIntervalMs?: number;
}

/** Delay before retry number `attempt` (1-based): doubling, capped, ±20 % jitter. */
export function backoffDelay(attempt: number, minMs = 500, maxMs = 15000, rand: () => number = Math.random): number {
  const d = Math.min(maxMs, minMs * 2 ** Math.max(0, attempt - 1));
  return Math.round(d * (0.8 + 0.4 * rand()));
}

interface Pending {
  resolve: (r: ResultMsg) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class GameSocket {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private attempt = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;
  private lastResync = -Infinity;
  /** Protocol 2 frames that did not decode (each one asks for a keyframe, at most once per interval). */
  decodeErrors = 0;
  state: ConnState = 'closed';

  constructor(private opts: SocketOptions) {}

  start(): void {
    this.stopped = false;
    this.open();
  }

  stop(): void {
    this.stopped = true;
    if (this.retryTimer) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    const ws = this.ws;
    this.ws = null;
    ws?.close();
    this.failPending('closed', 'disconnected');
    this.setState('closed');
  }

  /**
   * Send a command. Resolves with the server's `result` (ok or not); a
   * local failure (not connected, no reply in time) resolves as a failed
   * result too, so callers have one path for errors.
   */
  send(msg: ClientMsg): Promise<ResultMsg> {
    const id = this.nextId++;
    const ws = this.ws;
    if (!ws || ws.readyState !== 1 /* OPEN */) {
      return Promise.resolve(localError(msg.type, id, 'not_connected', 'not connected to the server'));
    }
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve(localError(msg.type, id, 'timeout', 'no reply from the server'));
      }, this.opts.resultTimeoutMs ?? 10000);
      this.pending.set(id, { resolve, timer });
      ws.send(JSON.stringify({ ...msg, id }));
    });
  }

  private open(): void {
    const Impl = this.opts.WebSocketImpl ?? WebSocket;
    this.setState(this.attempt === 0 ? 'connecting' : 'reconnecting');
    let opened = false;
    const ws = new Impl(this.opts.url());
    ws.binaryType = 'arraybuffer';
    this.ws = ws;
    // Each connection has its own delta states, starting with a keyframe.
    const decoder = new DeltaDecoder();
    this.lastResync = -Infinity;
    ws.onopen = () => {
      opened = true;
      this.attempt = 0;
      this.setState('open');
    };
    ws.onmessage = (ev: MessageEvent) => {
      if (ev.data instanceof ArrayBuffer) {
        this.onFrame(ws, decoder, ev.data);
        return;
      }
      if (typeof ev.data !== 'string') return;
      const m = parseServerMsg(ev.data);
      if (!m) return;
      if (m.type === 'result' && m.id !== undefined) {
        const p = this.pending.get(m.id);
        if (p) {
          clearTimeout(p.timer);
          this.pending.delete(m.id);
          p.resolve(m);
        }
      }
      this.opts.onMessage(m);
    };
    ws.onclose = () => {
      if (this.ws !== ws) return; // superseded or stopped
      this.ws = null;
      this.failPending('disconnected', 'connection lost before the reply');
      if (this.stopped) return;
      void this.scheduleRetry(opened);
    };
    ws.onerror = () => {
      /* onclose follows */
    };
  }

  /** A protocol 2 state: rebuild it, or ask for a keyframe if it doesn't apply. */
  private onFrame(ws: WebSocket, decoder: DeltaDecoder, data: ArrayBuffer): void {
    if (this.ws !== ws) return;
    let state;
    try {
      state = decoder.apply(data);
    } catch {
      this.decodeErrors += 1;
      const now = Date.now();
      if (now - this.lastResync >= (this.opts.resyncIntervalMs ?? 2000) && ws.readyState === 1 /* OPEN */) {
        this.lastResync = now;
        ws.send(JSON.stringify({ type: 'resync' }));
      }
      return;
    }
    this.lastResync = -Infinity;
    this.opts.onMessage({ type: 'state', ...state });
  }

  private async scheduleRetry(wasOpen: boolean): Promise<void> {
    this.attempt += 1;
    if (!wasOpen && this.opts.onFailedAttempt) {
      const go = await this.opts.onFailedAttempt(this.attempt);
      if (!go || this.stopped) {
        this.stopped = true;
        this.setState('closed');
        return;
      }
    }
    const delay = backoffDelay(this.attempt, this.opts.minDelayMs, this.opts.maxDelayMs);
    this.setState('reconnecting', delay);
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      if (!this.stopped) this.open();
    }, delay);
  }

  private failPending(code: string, message: string): void {
    for (const [id, p] of this.pending) {
      clearTimeout(p.timer);
      p.resolve(localError('', id, code, message));
    }
    this.pending.clear();
  }

  private setState(s: ConnState, retryInMs?: number): void {
    this.state = s;
    this.opts.onState(s, { attempt: this.attempt, retryInMs });
  }
}

function localError(command: string, id: number, code: string, message: string): ResultMsg {
  return { type: 'result', id, command, ok: false, error: { code, message } };
}
