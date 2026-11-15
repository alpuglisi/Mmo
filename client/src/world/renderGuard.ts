// Keeping the map alive through GPU trouble, PixiJS-free so it is unit-tested
// without a renderer.
//
// Mobile browsers drop WebGL contexts (backgrounding, memory pressure; the
// iOS simulator in CI too). PixiJS 8 listens for `webglcontextlost` /
// `webglcontextrestored` on its canvas, calls preventDefault on the loss
// (so the browser may restore it) and, on restore, re-runs its systems'
// `contextChange`: buffers, shaders and textures with CPU-side data (the
// terrain's BufferImageSource) come back on their own. It does not pause
// the ticker meanwhile (drawing a lost context throws, and one throw in a
// ticker callback stops the ticker for good), nor can it bring back the
// pixels of render textures (`generateTexture`), which live only on the
// GPU. `ContextGuard` fills those gaps: on loss the view pauses drawing and
// drops its generated textures; on restore it rebuilds them from the current
// state; and if no restore comes within a few seconds it asks the view for a
// new renderer.
//
// `ErrorLog` and `contain` keep a drawing error from escaping into the
// caller (the state handler, the ticker): it is logged, throttled, and the
// caller carries on.

export type GuardState = 'live' | 'lost' | 'recreating';

export interface GuardHooks {
  /** The context is gone: stop drawing and drop GPU-only resources. */
  onLost(): void;
  /** The same context is back (PixiJS has re-initialized its systems): rebuild and resume. */
  onRestored(): void;
  /** No restore came in time: make a new renderer (and canvas) and draw the current state on it. */
  recreate(): Promise<void>;
}

export interface GuardOptions {
  /** How long to wait for `webglcontextrestored` before recreating the renderer. */
  restoreTimeoutMs?: number;
  /** Delay before trying again after a failed recreate. */
  retryMs?: number;
  log?: ErrorLog;
  setTimeout?: (fn: () => void, ms: number) => unknown;
  clearTimeout?: (h: unknown) => void;
}

export const RESTORE_TIMEOUT_MS = 3000;

export class ContextGuard {
  state: GuardState = 'live';
  /** Counts, for tests and debugging. */
  readonly stats = { lost: 0, restored: 0, recreated: 0 };
  private timer: unknown = null;
  private detach: (() => void) | null = null;
  private disposed = false;
  private readonly timeoutMs: number;
  private readonly retryMs: number;
  private readonly log: ErrorLog;
  private readonly setT: (fn: () => void, ms: number) => unknown;
  private readonly clearT: (h: unknown) => void;

  constructor(private hooks: GuardHooks, opts: GuardOptions = {}) {
    this.timeoutMs = opts.restoreTimeoutMs ?? RESTORE_TIMEOUT_MS;
    this.retryMs = opts.retryMs ?? this.timeoutMs;
    this.log = opts.log ?? new ErrorLog();
    this.setT = opts.setTimeout ?? ((fn, ms) => globalThis.setTimeout(fn, ms));
    this.clearT = opts.clearTimeout ?? ((h) => globalThis.clearTimeout(h as ReturnType<typeof setTimeout>));
  }

  /**
   * Listen for loss and restore on `canvas` (replacing any earlier canvas).
   * Attach after PixiJS has set up its own listeners, so that on restore
   * its systems are re-initialized before `onRestored` runs.
   */
  attach(canvas: EventTarget): void {
    this.detach?.();
    const lost = (e: Event) => {
      // PixiJS calls this too; without it the browser never restores.
      e.preventDefault();
      this.lost('webglcontextlost');
    };
    const restored = () => this.restored();
    canvas.addEventListener('webglcontextlost', lost);
    canvas.addEventListener('webglcontextrestored', restored);
    this.detach = () => {
      canvas.removeEventListener('webglcontextlost', lost);
      canvas.removeEventListener('webglcontextrestored', restored);
    };
  }

  get paused(): boolean {
    return this.state !== 'live';
  }

  /** The context was lost (the event, or a drawing error on a lost context). Idempotent. */
  lost(reason: string): void {
    if (this.disposed || this.state !== 'live') return;
    this.state = 'lost';
    this.stats.lost++;
    console.warn(`[world] WebGL context lost (${reason}): drawing paused`);
    this.log.run('context lost', () => this.hooks.onLost());
    this.arm(this.timeoutMs);
  }

  /** The context came back. */
  restored(): void {
    if (this.disposed || this.state !== 'lost') return;
    this.disarm();
    this.state = 'live';
    this.stats.restored++;
    console.info('[world] WebGL context restored: redrawing');
    if (!this.log.run('context restored', () => this.hooks.onRestored())) {
      // Rebuilding failed: treat the context as unusable.
      this.lost('rebuild failed after restore');
    }
  }

  dispose(): void {
    this.disposed = true;
    this.disarm();
    this.detach?.();
    this.detach = null;
  }

  private arm(ms: number): void {
    this.disarm();
    this.timer = this.setT(() => {
      this.timer = null;
      void this.recreate();
    }, ms);
  }

  private disarm(): void {
    if (this.timer !== null) this.clearT(this.timer);
    this.timer = null;
  }

  private async recreate(): Promise<void> {
    if (this.disposed || this.state !== 'lost') return;
    this.state = 'recreating';
    console.warn('[world] WebGL context not restored: recreating the renderer');
    try {
      await this.hooks.recreate();
      if (this.disposed) return;
      this.state = 'live';
      this.stats.recreated++;
    } catch (e) {
      this.log.report('recreate renderer', e);
      if (this.disposed) return;
      this.state = 'lost';
      this.arm(this.retryMs);
    }
  }
}

/**
 * Logs errors without flooding the console: the first of each label at
 * once, then at most one line per `intervalMs` with the count since.
 */
export class ErrorLog {
  /** Errors reported, by label (for tests and debugging). */
  readonly counts = new Map<string, number>();
  private last = new Map<string, { at: number; suppressed: number }>();

  constructor(
    private intervalMs = 5000,
    private now: () => number = () => Date.now(),
    private sink: (msg: string, err: unknown) => void = (msg, err) => console.error(msg, err),
  ) {}

  report(label: string, err: unknown): void {
    this.counts.set(label, (this.counts.get(label) ?? 0) + 1);
    const t = this.now();
    const prev = this.last.get(label);
    if (prev && t - prev.at < this.intervalMs) {
      prev.suppressed++;
      return;
    }
    const more = prev && prev.suppressed > 0 ? ` (+${prev.suppressed} more since)` : '';
    this.last.set(label, { at: t, suppressed: 0 });
    this.sink(`[world] ${label} failed${more}:`, err);
  }

  /** Run `fn`, reporting (not throwing) its error; true if it completed. */
  run(label: string, fn: () => void): boolean {
    try {
      fn();
      return true;
    } catch (e) {
      this.report(label, e);
      return false;
    }
  }
}
