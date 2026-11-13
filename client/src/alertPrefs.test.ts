import { describe, expect, it } from 'vitest';
import { normalizePrefs, setPref, shows, toasts, visibleFeed } from './alertPrefs';
import { GameModel, type FeedEntry } from './model';
import { DEFAULT_ALERT_PREFS, type AlertKind, type AlertPrefs } from './protocol';

describe('alert preferences', () => {
  it('normalizes like the server: known kinds, server order, nothing hidden toasted', () => {
    const p = normalizePrefs({ hidden: ['world', 'daily_summary', 'world', 'bogus' as AlertKind], toast: ['world', 'market', 'birth', 'birth'] });
    expect(p).toEqual({ hidden: ['daily_summary', 'world'], toast: ['birth', 'market'] });
    expect(normalizePrefs(null)).toEqual({ hidden: [], toast: [] });
    expect(normalizePrefs(DEFAULT_ALERT_PREFS)).toEqual({ hidden: [], toast: ['death', 'need', 'market'] });
  });

  it('switches show and toast per kind', () => {
    let p: AlertPrefs = normalizePrefs(DEFAULT_ALERT_PREFS);
    expect(shows(p, 'birth') && !toasts(p, 'birth') && toasts(p, 'death')).toBe(true);
    p = setPref(p, 'birth', 'toast', true);
    expect(toasts(p, 'birth')).toBe(true);
    // Hiding a kind stops its toasts too.
    p = setPref(p, 'death', 'show', false);
    expect(shows(p, 'death') || toasts(p, 'death')).toBe(false);
    expect(p).toEqual({ hidden: ['death'], toast: ['birth', 'need', 'market'] });
    // Toasting a hidden kind shows it again.
    p = setPref(p, 'death', 'toast', true);
    expect(shows(p, 'death') && toasts(p, 'death')).toBe(true);
    p = setPref(p, 'market', 'toast', false);
    expect(shows(p, 'market') && !toasts(p, 'market')).toBe(true);
  });

  it('filters the feed but always keeps offer events', () => {
    const feed: FeedEntry[] = [
      { kind: 'daily_summary', text: 'Day 1', tick: 1 },
      { kind: 'offer', text: 'Mating offer 1: accepted', tick: 2 },
      { kind: 'death', text: '#4 died', tick: 3 },
    ];
    expect(visibleFeed(feed, normalizePrefs(DEFAULT_ALERT_PREFS))).toBe(feed);
    const p = normalizePrefs({ hidden: ['daily_summary', 'death'], toast: [] });
    expect(visibleFeed(feed, p).map((e) => e.kind)).toEqual(['offer']);
  });

  it('the model toasts only the kinds the preferences toast, and keeps hidden alerts for later', () => {
    const m = new GameModel();
    const alert = (kind: AlertKind, tick: number) => m.apply({ type: 'alert', alert: { tick, day: 0, kind, text: `${kind} ${tick}` } });
    alert('death', 1);
    alert('birth', 2);
    expect(m.toasts.map((t) => t.text)).toEqual(['death 1']);
    let alerts = 0;
    m.on('alerts', () => alerts++);
    m.setPrefs({ hidden: ['death'], toast: ['birth', 'death'] });
    expect(alerts).toBe(1);
    alert('death', 3);
    alert('birth', 4);
    expect(m.toasts.map((t) => t.text)).toEqual(['death 1', 'birth 4']);
    expect(m.feed.length).toBe(4);
    expect(m.visibleFeed().map((e) => e.text)).toEqual(['birth 2', 'birth 4']);
    m.setPrefs({ hidden: [], toast: [] });
    expect(m.visibleFeed().length).toBe(4);
  });
});
