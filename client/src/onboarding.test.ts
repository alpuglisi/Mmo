import { describe, expect, it } from 'vitest';
import type { FeedEntry, OfferEntry } from './model';
import {
  autoStarts, CARD_MARGIN, dueHints, HINT_IDS, HINTS, loadProgress, NEW_PROGRESS, onboardingDisabled, parseProgress, placeCard,
  progressKey, saveProgress, tourReduce, tourSteps, type HintInput, type KeyValueStore, type Progress, type TourEvent,
} from './onboarding';
import type { OfferView } from './protocol';

const fresh = (): Progress => ({ ...NEW_PROGRESS, hints: [] });

function memoryStore(): KeyValueStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

describe('tour state machine', () => {
  const N = 4;
  const run = (p: Progress, ...events: TourEvent[]) => events.reduce((q, e) => tourReduce(q, e, N), p);

  it('starts at the first step, walks forward and back, and completes past the last step', () => {
    let p = run(fresh(), { type: 'start' });
    expect(p).toMatchObject({ tour: 'active', step: 0 });
    p = run(p, { type: 'back' });
    expect(p.step).toBe(0); // no step before the first
    p = run(p, { type: 'next' }, { type: 'next' }, { type: 'back' });
    expect(p.step).toBe(1);
    p = run(p, { type: 'next' }, { type: 'next' });
    expect(p).toMatchObject({ tour: 'active', step: 3 });
    p = run(p, { type: 'next' });
    expect(p).toMatchObject({ tour: 'completed', step: 0 });
    expect(autoStarts(p)).toBe(false);
  });

  it('skips from any step, and from a tour that never opened', () => {
    expect(run(fresh(), { type: 'start' }, { type: 'next' }, { type: 'skip' })).toMatchObject({ tour: 'skipped', step: 0 });
    expect(run(fresh(), { type: 'skip' }).tour).toBe('skipped');
  });

  it('ignores moves on a finished tour, and replays from the start', () => {
    const done = run(fresh(), { type: 'skip' });
    expect(run(done, { type: 'next' }, { type: 'back' }, { type: 'goto', step: 2 }, { type: 'skip' })).toEqual(done);
    expect(run(done, { type: 'start' })).toMatchObject({ tour: 'active', step: 0 });
    const completed: Progress = { tour: 'completed', step: 0, hints: ['death'] };
    expect(run(completed, { type: 'start' })).toEqual({ tour: 'active', step: 0, hints: ['death'] });
  });

  it('resumes an active tour at its step (a reload mid-tour), clamped to the steps there are', () => {
    expect(run({ tour: 'active', step: 2, hints: [] }, { type: 'start' }).step).toBe(2);
    expect(run({ tour: 'active', step: 9, hints: [] }, { type: 'start' }).step).toBe(N - 1);
    expect(run({ tour: 'active', step: 0, hints: [] }, { type: 'goto', step: 7 }).step).toBe(N - 1);
  });

  it('opens by itself only for a new player or a tour left open', () => {
    expect(autoStarts(fresh())).toBe(true);
    expect(autoStarts({ tour: 'active', step: 1, hints: [] })).toBe(true);
    expect(autoStarts({ tour: 'skipped', step: 0, hints: [] })).toBe(false);
    expect(autoStarts({ tour: 'completed', step: 0, hints: [] })).toBe(false);
  });

  it('keeps the hints seen through every transition', () => {
    const p: Progress = { tour: 'new', step: 0, hints: ['offer'] };
    for (const e of [{ type: 'start' }, { type: 'next' }, { type: 'skip' }] as TourEvent[]) expect(tourReduce(p, e, N).hints).toEqual(['offer']);
  });
});

describe('tour steps', () => {
  it('covers the map, lineage, inspector, possession, market, seasons and alerts, ending at Help', () => {
    const steps = tourSteps({ touch: false, name: 'ada' });
    expect(steps.map((s) => s.id)).toEqual(['welcome', 'map', 'lineage', 'inspector', 'possess', 'market', 'seasons', 'alerts', 'done']);
    expect(steps[0].title).toContain('ada');
    expect(steps.at(-1)!.targets).toEqual(['#help-btn']);
    expect(new Set(steps.map((s) => s.id)).size).toBe(steps.length);
  });

  it('says tap and long-press on touch screens', () => {
    const touch = tourSteps({ touch: true, name: '' });
    const mouse = tourSteps({ touch: false, name: '' });
    expect(touch.find((s) => s.id === 'possess')!.body).toContain('long-press');
    expect(mouse.find((s) => s.id === 'possess')!.body).toContain('hold the mouse');
    expect(touch.find((s) => s.id === 'inspector')!.body.startsWith('Tap')).toBe(true);
    expect(touch[0].title).toBe('Welcome');
  });
});

describe('progress storage', () => {
  it('round-trips per account and treats junk as a new player', () => {
    const store = memoryStore();
    const k = progressKey(3, 'Ada');
    expect(k).toBe('evo.onboarding.v1.3.ada');
    expect(loadProgress(k, store)).toEqual(fresh());
    const p: Progress = { tour: 'active', step: 4, hints: ['death', 'offer'] };
    expect(saveProgress(k, p, store)).toBe(true);
    expect(loadProgress(k, store)).toEqual(p);
    expect(loadProgress(progressKey(4, 'Ada'), store)).toEqual(fresh());
    for (const raw of ['{', 'null', '42', '"x"', '{"tour":"done","step":-1,"hints":"x"}']) expect(parseProgress(raw)).toEqual(fresh());
    expect(parseProgress('{"tour":"skipped","step":2.5,"hints":["death","bogus","death"]}')).toEqual({ tour: 'skipped', step: 0, hints: ['death'] });
  });

  it('survives storage that throws or is missing', () => {
    const broken: KeyValueStore = {
      getItem: () => { throw new Error('SecurityError'); },
      setItem: () => { throw new Error('QuotaExceededError'); },
    };
    expect(loadProgress('k', broken)).toEqual(fresh());
    expect(saveProgress('k', fresh(), broken)).toBe(false);
    expect(loadProgress('k', null)).toEqual(fresh());
    expect(saveProgress('k', fresh(), null)).toBe(false);
  });

  it('is turned off by ?tour=off or ?onboarding=off', () => {
    expect(onboardingDisabled('?tour=off')).toBe(true);
    expect(onboardingDisabled('?x=1&onboarding=0')).toBe(true);
    expect(onboardingDisabled('?tour=OFF')).toBe(true);
    expect(onboardingDisabled('')).toBe(false);
    expect(onboardingDisabled('?tour=on')).toBe(false);
  });
});

describe('hint triggers', () => {
  const ME = 7;
  const offer = (to: number, status: OfferView['status']): OfferEntry => ({
    offer: { id: 1, from_player: 9, to_player: to, from_name: 'bo', to_name: 'me', mine: 10, partner: 20, status } as OfferView,
    incoming: to === ME,
  });
  const entry = (kind: FeedEntry['kind'], text = 'x'): FeedEntry => ({ kind, text, tick: 1 });
  const input = (over: Partial<HintInput> = {}): HintInput => ({ player: ME, organisms: [], feed: [], offers: [], ...over });

  it('fires nothing for a quiet lineage', () => {
    expect(dueHints(input({ organisms: [{ pregnant: false }], feed: [entry('birth'), entry('daily_summary'), entry('achievement', 'Survivor: your lineage came through a drought.')] }), [])).toEqual([]);
  });

  it('fires on the first pregnancy, death, species split and offer received', () => {
    expect(dueHints(input({ organisms: [{ pregnant: false }, { pregnant: true }] }), [])).toEqual(['pregnancy']);
    expect(dueHints(input({ feed: [entry('death', 'Founder #4 starved.')] }), [])).toEqual(['death']);
    expect(dueHints(input({ feed: [entry('achievement', 'Species founded: Gnarls split off as a species of its own, carrying mostly your alleles.')] }), [])).toEqual(['species']);
    expect(dueHints(input({ offers: [offer(ME, 'pending')] }), [])).toEqual(['offer']);
  });

  it('ignores offers we sent and offers already answered', () => {
    expect(dueHints(input({ offers: [offer(9, 'pending'), offer(ME, 'accepted'), offer(ME, 'declined')] }), [])).toEqual([]);
  });

  it('fires each hint once, in a fixed order', () => {
    const all = input({ organisms: [{ pregnant: true }], feed: [entry('achievement', 'Species founded: X split off.'), entry('death')], offers: new Map([[1, offer(ME, 'pending')]]).values() });
    expect(dueHints(all, [])).toEqual([...HINT_IDS]);
    expect(dueHints(input({ organisms: [{ pregnant: true }], feed: [entry('death')] }), ['pregnancy'])).toEqual(['death']);
    expect(dueHints(input({ organisms: [{ pregnant: true }], feed: [entry('death')] }), ['pregnancy', 'death'])).toEqual([]);
  });

  it('has text and a tab for every hint', () => {
    for (const id of HINT_IDS) expect(HINTS[id].title.length > 0 && HINTS[id].body.length > 20 && HINTS[id].tab.length > 0).toBe(true);
  });
});

describe('tour card placement', () => {
  const rect = (left: number, top: number, width: number, height: number) => ({ left, top, width, height, right: left + width, bottom: top + height });
  const inside = (p: { left: number; top: number }, w: number, h: number, vw: number, vh: number) =>
    p.left >= CARD_MARGIN - 0.01 && p.top >= CARD_MARGIN - 0.01 && p.left + w <= vw - CARD_MARGIN + 0.01 && p.top + h <= vh - CARD_MARGIN + 0.01;

  it('centers a card with no target', () => {
    expect(placeCard(null, 300, 200, 1000, 800, false)).toEqual({ left: 350, top: 300 });
  });

  it('puts the card beside a sidebar tab on wide screens', () => {
    const p = placeCard(rect(900, 50, 60, 30), 340, 200, 1280, 800, false);
    expect(p.left + 340).toBeLessThanOrEqual(900);
    expect(inside(p, 340, 200, 1280, 800)).toBe(true);
  });

  it('centers over a target that fills the screen (the map)', () => {
    const p = placeCard(rect(0, 40, 940, 760), 340, 200, 1280, 800, false);
    expect(inside(p, 340, 200, 1280, 800)).toBe(true);
    expect(placeCard(rect(0, 50, 390, 700), 358, 220, 390, 844, true)).toEqual({ left: 16, top: (844 - 220) / 2 });
  });

  it('on phones goes above a target low on the screen and below one high up, full width', () => {
    const vw = 390, vh = 844, w = vw - 2 * CARD_MARGIN;
    const low = placeCard(rect(100, 800, 50, 44), w, 200, vw, vh, true);
    expect(low.top + 200).toBeLessThanOrEqual(800);
    expect(low.left).toBe(CARD_MARGIN);
    const high = placeCard(rect(8, 40, 370, 40), w, 200, vw, vh, true);
    expect(high.top).toBeGreaterThanOrEqual(80);
    expect(inside(high, w, 200, vw, vh) && inside(low, w, 200, vw, vh)).toBe(true);
  });

  it('never leaves the viewport', () => {
    for (const [vw, vh, narrow] of [[1280, 800, false], [390, 664, true], [412, 839, true], [800, 500, false]] as const) {
      const w = Math.min(360, vw - 2 * CARD_MARGIN);
      for (const r of [rect(-50, -50, 20, 20), rect(vw - 5, vh - 5, 40, 40), rect(vw / 2, vh / 2, 10, 10), rect(0, 0, vw, vh)]) {
        expect(inside(placeCard(r, w, 240, vw, vh, narrow), w, 240, vw, vh)).toBe(true);
      }
    }
  });
});
