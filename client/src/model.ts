// Client-side game model: the latest state, the offer table and toasts,
// fed by server messages. UI code subscribes; it never talks to the socket
// for reads.

import {
  isOpen, type AlertKind, type AlertView, type ErrorInfo, type MatingOutcome, type OfferView, type OrgId, type ServerMsg, type StateView,
} from './protocol';
import type { ConnState } from './socket';

export interface OfferEntry {
  offer: OfferView;
  outcome?: MatingOutcome;
  error?: ErrorInfo;
  /** Arrived as a `mating_request` (someone asked us). */
  incoming: boolean;
}

export interface Toast {
  kind: 'error' | 'info';
  text: string;
}

/** One line of the alerts feed: a server alert or an offer event. */
export interface FeedEntry {
  kind: AlertKind | 'offer';
  text: string;
  /** Simulated tick (0 when unknown). */
  tick: number;
  day?: number;
  organism?: OrgId;
}

export type Topic = 'state' | 'offers' | 'conn' | 'select' | 'toast' | 'welcome' | 'alerts' | 'session_ended';

/** Closed offers kept for display. */
const MAX_CLOSED = 20;
/** Feed entries kept. */
export const MAX_FEED = 80;

const key = (e: FeedEntry) => `${e.tick}|${e.kind}|${e.text}`;

/** Merge `incoming` into `feed` (oldest first), dropping duplicates and keeping the newest `MAX_FEED`. */
export function mergeFeed(feed: FeedEntry[], incoming: FeedEntry[]): FeedEntry[] {
  const seen = new Set(feed.map(key));
  const out = [...feed];
  for (const e of incoming) {
    if (seen.has(key(e))) continue;
    seen.add(key(e));
    out.push(e);
  }
  out.sort((a, b) => a.tick - b.tick);
  return out.slice(-MAX_FEED);
}

export function alertEntry(a: AlertView): FeedEntry {
  return { kind: a.kind, text: a.text, tick: a.tick, day: a.day, organism: a.organism };
}

export class GameModel {
  player = 0;
  name = '';
  welcomeLineage: number | null = null;
  state: StateView | null = null;
  offers = new Map<number, OfferEntry>();
  selected: OrgId | null = null;
  conn: ConnState = 'closed';
  retryInMs: number | undefined;
  toasts: Toast[] = [];
  /** The alerts feed, oldest first. */
  feed: FeedEntry[] = [];
  private subs = new Map<Topic, Set<() => void>>();

  on(topic: Topic, fn: () => void): () => void {
    let s = this.subs.get(topic);
    if (!s) this.subs.set(topic, (s = new Set()));
    s.add(fn);
    return () => s!.delete(fn);
  }

  emit(topic: Topic): void {
    for (const fn of this.subs.get(topic) ?? []) fn();
  }

  toast(kind: Toast['kind'], text: string): void {
    this.toasts.push({ kind, text });
    if (this.toasts.length > 6) this.toasts.shift();
    this.emit('toast');
  }

  addFeed(entries: FeedEntry[]): void {
    const before = this.feed.length === 0 ? '' : key(this.feed[this.feed.length - 1]);
    this.feed = mergeFeed(this.feed, entries);
    const after = this.feed.length === 0 ? '' : key(this.feed[this.feed.length - 1]);
    if (before !== after || entries.length > 0) this.emit('alerts');
  }

  private tick(): number {
    return this.state?.clock.tick ?? 0;
  }

  select(id: OrgId | null): void {
    this.selected = id;
    this.emit('select');
  }

  setConn(s: ConnState, retryInMs?: number): void {
    this.conn = s;
    this.retryInMs = retryInMs;
    this.emit('conn');
  }

  apply(m: ServerMsg): void {
    switch (m.type) {
      case 'welcome':
        this.player = m.player;
        this.name = m.name;
        this.welcomeLineage = m.lineage;
        this.emit('welcome');
        break;
      case 'state': {
        const { type: _t, ...view } = m;
        this.state = view;
        this.syncOffers(view.offers);
        this.emit('state');
        break;
      }
      case 'result':
        // A preview is shown in the matings form, not as a toast.
        if (!m.ok && m.error && m.command !== 'preview_mating') this.toast('error', `${m.command || 'command'}: ${m.error.message} (${m.error.code})`);
        break;
      case 'mating_request': {
        this.offers.set(m.offer.id, { offer: m.offer, incoming: true });
        const text = `${m.offer.from_name} asks to mate their #${m.offer.mine} with your #${m.offer.partner}`;
        this.toast('info', text);
        this.addFeed([{ kind: 'offer', text, tick: this.tick(), organism: m.offer.partner }]);
        this.emit('offers');
        break;
      }
      case 'alert':
        this.addFeed([alertEntry(m.alert)]);
        if (m.alert.kind === 'death' || m.alert.kind === 'need' || m.alert.kind === 'market') this.toast('info', m.alert.text);
        break;
      case 'session_ended':
        this.emit('session_ended');
        break;
      case 'mating_update': {
        const prev = this.offers.get(m.offer.id);
        this.offers.set(m.offer.id, {
          offer: m.offer,
          outcome: m.outcome,
          error: m.error,
          incoming: prev?.incoming ?? m.offer.to_player === this.player,
        });
        this.trimClosed();
        const what = m.outcome ? (m.outcome.conceived ? 'completed, conceived' : 'completed, no conception') : m.offer.status;
        const text = `Mating offer ${m.offer.id}: ${what}${m.error ? ` (${m.error.message})` : ''}`;
        this.toast(m.error && m.offer.status !== 'accepted' ? 'error' : 'info', text);
        this.addFeed([{ kind: 'offer', text, tick: this.tick() }]);
        this.emit('offers');
        break;
      }
    }
  }

  /** The state lists the open offers; drop open entries it no longer has. */
  private syncOffers(open: OfferView[]): void {
    const ids = new Set(open.map((o) => o.id));
    let changed = false;
    for (const o of open) {
      const prev = this.offers.get(o.id);
      if (!prev || prev.offer.status !== o.status) {
        this.offers.set(o.id, { ...prev, offer: o, incoming: prev?.incoming ?? o.to_player === this.player });
        changed = true;
      }
    }
    for (const [id, e] of this.offers) {
      if (isOpen(e.offer.status) && !ids.has(id)) {
        this.offers.delete(id);
        changed = true;
      }
    }
    if (changed) this.emit('offers');
  }

  private trimClosed(): void {
    const closed = [...this.offers.values()].filter((e) => !isOpen(e.offer.status));
    for (const e of closed.slice(0, Math.max(0, closed.length - MAX_CLOSED))) this.offers.delete(e.offer.id);
  }
}
