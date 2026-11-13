// Client-side game model: the latest state, the offer table and toasts,
// fed by server messages. UI code subscribes; it never talks to the socket
// for reads.

import { isOpen, type ErrorInfo, type MatingOutcome, type OfferView, type OrgId, type ServerMsg, type StateView } from './protocol';
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

export type Topic = 'state' | 'offers' | 'conn' | 'select' | 'toast' | 'welcome';

/** Closed offers kept for display. */
const MAX_CLOSED = 20;

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
        if (!m.ok && m.error) this.toast('error', `${m.command || 'command'}: ${m.error.message} (${m.error.code})`);
        break;
      case 'mating_request':
        this.offers.set(m.offer.id, { offer: m.offer, incoming: true });
        this.toast('info', `${m.offer.from_name} asks to mate their #${m.offer.mine} with your #${m.offer.partner}`);
        this.emit('offers');
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
        this.toast(m.error && m.offer.status !== 'accepted' ? 'error' : 'info',
          `Mating offer ${m.offer.id}: ${what}${m.error ? ` (${m.error.message})` : ''}`);
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
