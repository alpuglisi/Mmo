// Client-side game model: the latest state, the offer table and toasts,
// fed by server messages. UI code subscribes; it never talks to the socket
// for reads.

import { commandError } from './errors';
import { normalizePrefs, toasts, visibleFeed } from './alertPrefs';
import {
  DEFAULT_ALERT_PREFS, isOpen, type AlertKind, type AlertPrefs, type AlertView, type CurrentSeason, type ErrorInfo, type MatingOutcome, type MatingRecord, type OfferView, type OrgId,
  type ServerMsg, type StateView,
} from './protocol';
import type { ConnState } from './socket';
import { friendlyError } from './ui/friendlyErrors';
import { offerUpdateText } from './ui/matingHistory';

/** Commands whose errors their form shows itself (the matings form; the species-name dialog). */
const FORM_WORDED = new Set(['preview_mating', 'request_mating', 'name_species']);
/** Mating commands: their errors toast in plain words (`friendlyError`). */
const MATING_COMMANDS = new Set(['respond_mating', 'complete_mating', 'cancel_mating', 'buy_listing', 'bid_auction']);

export interface OfferEntry {
  offer: OfferView;
  outcome?: MatingOutcome;
  error?: ErrorInfo;
  /** Arrived as a `mating_request` (someone asked us). */
  incoming: boolean;
}

export interface Toast {
  /** 'warn': done, with a caution (an order accepted with warnings: not red, playtest 8). */
  kind: 'error' | 'warn' | 'info';
  text: string;
  /** How many alike messages (same words, other #ids or numbers) this one stands for. */
  count?: number;
  /**
   * News the server pushed (alerts, mating requests and updates), not the
   * reply to the player's own command: on phones the newest reply stays
   * shown above them (playtest 12: a claim's warning was pushed out by the
   * alerts it set off).
   */
  news?: boolean;
  /** When it was shown (ms, `Date.now()`), for how long it stays. */
  at?: number;
}

/** Two toasts are alike when only their ids and numbers differ ("Founder #3 was eaten."). */
const toastShape = (t: string): string => t.replace(/\d+(\.\d+)?/g, '0');

/** One line of the alerts feed: a server alert or an offer event. */
export interface FeedEntry {
  kind: AlertKind | 'offer';
  text: string;
  /** Simulated tick (0 when unknown). */
  tick: number;
  day?: number;
  organism?: OrgId;
}

export type Topic = 'state' | 'offers' | 'conn' | 'select' | 'toast' | 'welcome' | 'alerts' | 'session_ended' | 'season';

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
  /** The protocol of the connection (3: binary delta states with own headings and eyes; 2: without; 1: whole JSON states). */
  proto = 1;
  state: StateView | null = null;
  /** The ranked season under way, as last pushed (`watch_season`). */
  season: CurrentSeason | null = null;
  offers = new Map<number, OfferEntry>();
  /**
   * Finished matings from `GET /api/matings/history` (newest first), plus
   * matings within the lineage made since: the Matings tab's history, so it
   * survives a reload and shows what happened while the tab was closed.
   */
  matingLog: MatingRecord[] = [];
  selected: OrgId | null = null;
  conn: ConnState = 'closed';
  retryInMs: number | undefined;
  toasts: Toast[] = [];
  /** The alerts feed, oldest first (every kind; see `visibleFeed`). */
  feed: FeedEntry[] = [];
  /** Which alert kinds show and toast (loaded from the server). */
  prefs: AlertPrefs = normalizePrefs(DEFAULT_ALERT_PREFS);
  private subs = new Map<Topic, Set<() => void>>();

  on(topic: Topic, fn: () => void): () => void {
    let s = this.subs.get(topic);
    if (!s) this.subs.set(topic, (s = new Set()));
    s.add(fn);
    return () => s!.delete(fn);
  }

  /**
   * Call every subscriber of `topic`. One that throws (e.g. drawing on a lost
   * WebGL context) is logged and skipped: the others still run, and the
   * caller (the socket's message handler) carries on with the next message.
   */
  emit(topic: Topic): void {
    for (const fn of [...(this.subs.get(topic) ?? [])]) {
      try {
        fn();
      } catch (e) {
        console.error(`[model] a '${topic}' subscriber failed:`, e);
      }
    }
  }

  setMatingLog(records: MatingRecord[]): void {
    this.matingLog = records;
    this.emit('offers');
  }

  /** A mating within the lineage just happened (the server's history has it too). */
  addMating(record: MatingRecord): void {
    this.matingLog = [record, ...this.matingLog];
    this.emit('offers');
  }

  toast(kind: Toast['kind'], text: string, news = false): void {
    // A burst of alike messages (a predator working through the founders)
    // shows as the newest one with a count, not a column of copies.
    const last = this.toasts[this.toasts.length - 1];
    if (last && last.kind === kind && !!last.news === news && toastShape(last.text) === toastShape(text)) {
      last.text = text;
      last.at = Date.now();
      last.count = (last.count ?? 1) + 1;
      this.emit('toast');
      return;
    }
    this.toasts.push({ kind, text, at: Date.now(), ...(news ? { news } : {}) });
    if (this.toasts.length > 6) this.toasts.shift();
    this.emit('toast');
  }

  /** Texts to toast only through `toastFresh` (the founding notes, also sent as alerts). */
  private onceTexts = new Set<string>();
  sayOnce(texts: readonly string[]): void {
    for (const x of texts) this.onceTexts.add(x);
  }
  /** Toast `text` as the newest, once: an earlier copy (never drawn) goes. */
  toastFresh(kind: Toast['kind'], text: string): void {
    this.toasts = this.toasts.filter((t) => t.text !== text);
    this.toast(kind, text, true);
  }

  /** Drop error toasts of a command that has since succeeded ("Label: …"), so a fixed mistake doesn't linger beside the success. */
  dismissErrors(label: string): void {
    const before = this.toasts.length;
    this.toasts = this.toasts.filter((t) => !(t.kind === 'error' && t.text.startsWith(`${label}:`)));
    if (this.toasts.length !== before) this.emit('toast');
  }

  addFeed(entries: FeedEntry[]): void {
    const before = this.feed.length === 0 ? '' : key(this.feed[this.feed.length - 1]);
    this.feed = mergeFeed(this.feed, entries);
    const after = this.feed.length === 0 ? '' : key(this.feed[this.feed.length - 1]);
    if (before !== after || entries.length > 0) this.emit('alerts');
  }

  /** The feed entries the preferences let through, oldest first. */
  visibleFeed(): FeedEntry[] {
    return visibleFeed(this.feed, this.prefs);
  }

  setPrefs(prefs: AlertPrefs): void {
    this.prefs = normalizePrefs(prefs);
    this.emit('alerts');
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
        this.proto = m.proto ?? 1;
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
        // The matings form shows previews and words its own request errors.
        if (!m.ok && m.error && !FORM_WORDED.has(m.command)) {
          this.toast('error', MATING_COMMANDS.has(m.command) ? friendlyError(m.error) : commandError(m.command, m.error));
        }
        break;
      case 'mating_request': {
        this.offers.set(m.offer.id, { offer: m.offer, incoming: true });
        // An offer to the lineage has no partner yet (0): "one of yours", no link (playtest 5: "#0#0").
        const toLineage = m.offer.partner === 0;
        const text = `${m.offer.from_name} asks to mate their #${m.offer.mine} with ${toLineage ? 'one of yours (you pick which)' : `your #${m.offer.partner}`}`;
        this.toast('info', text, true);
        this.addFeed([{ kind: 'offer', text, tick: this.tick(), day: this.state?.clock.day, ...(toLineage ? {} : { organism: m.offer.partner }) }]);
        this.emit('offers');
        break;
      }
      case 'alert':
        this.addFeed([alertEntry(m.alert)]);
        if (toasts(this.prefs, m.alert.kind) && !this.onceTexts.has(m.alert.text)) this.toast('info', m.alert.text, true);
        break;
      case 'session_ended':
        this.emit('session_ended');
        break;
      case 'season': {
        const { type: _t, ...season } = m;
        this.season = season;
        this.emit('season');
        break;
      }
      case 'mating_update': {
        const prev = this.offers.get(m.offer.id);
        this.offers.set(m.offer.id, {
          offer: m.offer,
          outcome: m.outcome,
          error: m.error,
          incoming: prev?.incoming ?? m.offer.to_player === this.player,
        });
        this.trimClosed();
        // Only a change a player acts on is said (playtest 11: a deadline moved
        // every half day while an animal slept, 58 updates and toasts for one offer).
        if (prev && offerKey(prev.offer, prev.outcome, prev.error) === offerKey(m.offer, m.outcome, m.error)) {
          this.emit('offers');
          break;
        }
        // Still accepted, nothing ended: a substitute or a moved meeting (playtest 13: "accepted" said
        // again for each substitute; the server's alert names the substitute).
        if (prev && prev.offer.status === 'accepted' && m.offer.status === 'accepted' && !m.error && !m.outcome) {
          this.emit('offers');
          break;
        }
        const text = offerUpdateText(m.offer, m.outcome, m.error, this.player);
        this.toast(m.error && m.offer.status !== 'accepted' ? 'error' : 'info', text, true);
        this.addFeed([{ kind: 'offer', text, tick: this.tick(), day: m.day ?? this.state?.clock.day }]);
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

/** What about an offer is worth telling: not its deadline or the animals' positions. */
export function offerKey(o: OfferView, outcome: MatingOutcome | undefined, error: ErrorInfo | undefined): string {
  return JSON.stringify([o.status, o.mine, o.partner, o.lost ?? null, o.substitute_sex ?? null, outcome ?? null, error?.code ?? null, error?.message ?? null]);
}
