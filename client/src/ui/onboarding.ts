// First-session onboarding, the DOM side: the guided tour (a non-modal card
// pointing at one part of the screen per step), the one-time hint cards and
// the Help dialog that replays the tour. State, steps and triggers live in
// `../onboarding.ts` (pure, unit-tested).
//
// Nothing here blocks the game: the tour's highlight ignores the pointer,
// its card is a small non-modal dialog, and every element is removed when
// closed. Only the Help dialog is modal (and traps focus) while open.

import './onboarding.css';
import { FOUNDING_RAMP_TEXT } from '../designWarnings';
import { FREE_FIRST_WINTER, SEASON_NOTICE_DAYS } from '../seasonNotice';
import type { GameModel } from '../model';
import {
  autoStarts, DEATH_HINT_AFTER_DAYS, dueHints, HINT_IDS, HINTS, hintsAfterReset, loadProgress, placeCard, progressKey, saveProgress, tourReduce, tourSteps,
  CARD_MARGIN, type HintId, type KeyValueStore, type Progress, type TourEvent, type TourStep, type TourTab,
} from '../onboarding';
import { h } from './dom';

export interface OnboardingDeps {
  model: GameModel;
  /** Where the fixed tour and Help elements go (the game screen; removed with it). */
  host: HTMLElement;
  /** Where hint cards go (the map area). */
  hintHost: HTMLElement;
  /** The top bar's Help button (opens the Help dialog; focus returns to it). */
  helpBtn: HTMLElement;
  narrow: () => boolean;
  touch: () => boolean;
  /** Show a tab; `sheet` also opens the phone's bottom sheet. */
  openTab: (t: TourTab, sheet: boolean) => void;
  /** Select one of the player's organisms if none of theirs is selected. */
  selectOwn: () => void;
  /** `?tour=off`: no tour and no hints (Help still replays the tour). */
  disabled: boolean;
  /**
   * The player came back to a lineage they already had (not straight from
   * founding): they have been through the first session, if not in this
   * browser, so the tour does not start by itself (Help replays it). The
   * playtests saw it again in every new browser profile.
   */
  returning?: boolean;
  store?: KeyValueStore | null;
}

export interface Onboarding {
  startTour: () => void;
  tourOpen: () => boolean;
  /** Re-evaluate the hint triggers (call on state, alert and offer changes). */
  check: () => void;
  openHelp: () => void;
  destroy: () => void;
}

export function createOnboarding(deps: OnboardingDeps): Onboarding {
  const { model } = deps;
  const key = progressKey(model.player, model.name);
  let progress: Progress = loadProgress(key, deps.store);
  const save = () => saveProgress(key, progress, deps.store);
  if (deps.returning && progress.tour === 'new') {
    // Back in a new browser: they have played, so the tips have been seen
    // too (they recurred in every new browser profile, playtest 5).
    progress = { ...progress, tour: 'skipped', hints: [...new Set([...progress.hints, ...HINT_IDS])] };
    save();
  }

  // ---------------------------------------------------------------- tour
  let steps: TourStep[] = [];
  let tourEls: { ring: HTMLElement; card: HTMLElement } | null = null;
  let returnFocus: Element | null = null;
  let repositionTimer: ReturnType<typeof setInterval> | undefined;

  const count = h('span', { class: 'tour-count', id: 'tour-count' });
  const title = h('h2', { id: 'tour-title' });
  const body = h('p', { id: 'tour-body' });
  const backBtn = h('button', { class: 'secondary', id: 'tour-back', type: 'button' }, 'Back');
  const nextBtn = h('button', { id: 'tour-next', type: 'button' }, 'Next');
  const skipBtn = h('button', { class: 'secondary', id: 'tour-skip', type: 'button' }, 'Skip tour');
  const closeBtn = h('button', { class: 'tour-close', id: 'tour-close', type: 'button', 'aria-label': 'Close the tour' }, '×');
  backBtn.addEventListener('click', () => dispatch({ type: 'back' }));
  nextBtn.addEventListener('click', () => dispatch({ type: 'next' }));
  skipBtn.addEventListener('click', () => dispatch({ type: 'skip' }));
  closeBtn.addEventListener('click', () => dispatch({ type: 'skip' }));

  function dispatch(e: TourEvent): void {
    progress = tourReduce(progress, e, steps.length);
    save();
    if (progress.tour === 'active') showStep();
    else closeTour();
  }

  function startTour(): void {
    closeHelp(false);
    putBackHint();
    steps = tourSteps({ touch: deps.touch(), name: model.name });
    // A replay starts over; a reload mid-tour resumes.
    progress = tourReduce(progress, { type: 'start' }, steps.length);
    save();
    if (!tourEls) {
      returnFocus = document.activeElement;
      const ring = h('div', { class: 'tour-ring', id: 'tour-ring', 'aria-hidden': 'true' });
      const card = h('div', { class: 'tour-card', id: 'tour', role: 'dialog', 'aria-modal': 'false', 'aria-labelledby': 'tour-title', 'aria-describedby': 'tour-body' },
        h('div', { class: 'tour-head' }, count, closeBtn), title, body,
        h('div', { class: 'tour-actions' }, skipBtn, h('span', { class: 'spacer' }), backBtn, nextBtn));
      card.addEventListener('keydown', onCardKey);
      tourEls = { ring, card };
      deps.host.append(ring, card);
      window.addEventListener('keydown', onTourKey);
      window.addEventListener('resize', position);
      repositionTimer = setInterval(position, 500);
    }
    showStep();
  }

  function showStep(): void {
    const step = steps[progress.step];
    if (!step || !tourEls) return;
    if (step.selectOwn) deps.selectOwn();
    if (step.tab && !deps.narrow()) deps.openTab(step.tab, false);
    count.textContent = `${progress.step + 1} of ${steps.length}`;
    title.textContent = step.title;
    body.textContent = step.body;
    backBtn.disabled = progress.step === 0;
    const last = progress.step === steps.length - 1;
    nextBtn.textContent = last ? 'Finish' : 'Next';
    skipBtn.hidden = last;
    tourEls.card.dataset.step = step.id;
    position();
    requestAnimationFrame(position);
    nextBtn.focus({ preventScroll: true });
  }

  function closeTour(): void {
    if (!tourEls) return;
    tourEls.ring.remove();
    tourEls.card.remove();
    tourEls = null;
    window.removeEventListener('keydown', onTourKey);
    window.removeEventListener('resize', position);
    clearInterval(repositionTimer);
    const back = returnFocus instanceof HTMLElement && returnFocus.isConnected && returnFocus !== document.body ? returnFocus : deps.helpBtn;
    back.focus({ preventScroll: true });
    returnFocus = null;
    check();
  }

  function onTourKey(e: KeyboardEvent): void {
    if (e.key !== 'Escape' || helpEls) return;
    e.preventDefault();
    dispatch({ type: 'skip' });
  }

  function onCardKey(e: KeyboardEvent): void {
    if (e.key === 'ArrowRight') dispatch({ type: 'next' });
    else if (e.key === 'ArrowLeft') dispatch({ type: 'back' });
  }

  /** The first visible target of the step, or null (centered card). */
  function target(step: TourStep): DOMRect | null {
    for (const sel of step.targets) {
      for (const el of document.querySelectorAll(sel)) {
        if ((el as HTMLElement).hidden) continue;
        const r = el.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) return r;
      }
    }
    return null;
  }

  function position(): void {
    if (!tourEls) return;
    const step = steps[progress.step];
    if (!step) return;
    const { ring, card } = tourEls;
    const vw = document.documentElement.clientWidth;
    const vh = window.innerHeight;
    const r = target(step);
    if (r) {
      // Clamped to the viewport, so a large target never makes the page scroll.
      const left = Math.max(2, r.left - 4);
      const top = Math.max(2, r.top - 4);
      const right = Math.min(vw - 2, r.right + 4);
      const bottom = Math.min(vh - 2, r.bottom + 4);
      Object.assign(ring.style, { left: `${left}px`, top: `${top}px`, width: `${Math.max(0, right - left)}px`, height: `${Math.max(0, bottom - top)}px` });
      ring.hidden = false;
    } else {
      ring.hidden = true;
    }
    const w = Math.min(360, vw - 2 * CARD_MARGIN);
    card.style.width = `${w}px`;
    const ch = card.offsetHeight;
    const pos = placeCard(r, w, ch, vw, vh, deps.narrow());
    card.style.left = `${pos.left}px`;
    card.style.top = `${pos.top}px`;
  }

  // ---------------------------------------------------------------- hints
  const queue: HintId[] = [];
  let hintEl: HTMLElement | null = null;
  const live = h('div', { class: 'sr-only', role: 'status', 'aria-live': 'polite', id: 'onboarding-live' });
  deps.hintHost.append(live);

  /** A tip left alone goes away on its own after this long (the map stays clear). */
  const HINT_AUTO_HIDE_MS = 20_000;
  /** After a tip went away on its own, the next waits this long (the player is busy). */
  const HINT_PAUSE_MS = 45_000;
  let pauseUntil = 0;
  /** The day the game opened on this lineage (deaths before it, or just after, bring no tip). */
  let startDay: number | null = null;
  let hideTimer: ReturnType<typeof setTimeout> | undefined;

  function check(): void {
    if (deps.disabled || tourEls || helpEls) return;
    if (Date.now() < pauseUntil) return;
    if (progress.tour === 'new' || progress.tour === 'active') return;
    if (startDay === null && model.state) startDay = model.state.clock.day;
    if (startDay === null) return;
    const input = { player: model.player, organisms: model.state?.organisms ?? [], feed: model.feed, offers: model.offers.values(), deathsFrom: startDay + DEATH_HINT_AFTER_DAYS };
    for (const id of dueHints(input, [...progress.hints, ...queue])) queue.push(id);
    if (!hintEl && queue.length > 0) showHint(queue.shift()!);
  }

  function showHint(id: HintId): void {
    const def = HINTS[id];
    progress = { ...progress, hints: [...progress.hints, id] };
    save();
    const dismiss = h('button', { class: 'secondary', id: 'hint-dismiss', type: 'button' }, 'Got it');
    const show = h('button', { id: 'hint-show', type: 'button' }, 'Show me');
    const close = h('button', { class: 'tour-close hint-close', id: 'hint-close', type: 'button', 'aria-label': 'Close the tip', title: 'Close' }, '×');
    const noMore = h('button', { class: 'link small', id: 'hint-no-more', type: 'button', title: 'Help → "Show tips again" brings them back' }, 'No more tips');
    const titleId = `hint-title-${id}`;
    const card = h('div', { class: 'hint-card', id: 'onboarding-hint', role: 'region', 'aria-labelledby': titleId, 'data-hint': id },
      h('div', { class: 'hint-head' }, h('strong', { id: titleId }, def.title), close),
      h('p', {}, def.body), h('div', { class: 'row' }, noMore, h('span', { class: 'spacer' }), show, dismiss));
    // Announced politely; focus stays where the player is.
    live.textContent = `Tip: ${def.title}. ${def.body}`;
    dismiss.addEventListener('click', () => nextHint());
    close.addEventListener('click', () => nextHint());
    noMore.addEventListener('click', () => {
      // Every tip counts as seen: none shows again until Help's "Show tips again".
      queue.length = 0;
      progress = { ...progress, hints: [...new Set([...progress.hints, ...HINT_IDS])] };
      save();
      hideHint();
    });
    // Left alone, the tip goes away by itself (not while the pointer or
    // focus is on it), and the next one waits a while.
    const arm = () => {
      clearTimeout(hideTimer);
      hideTimer = setTimeout(() => {
        if (hintEl !== card) return;
        if (card.matches(':hover') || card.contains(document.activeElement)) return arm();
        pauseUntil = Date.now() + HINT_PAUSE_MS;
        hideHint();
        setTimeout(() => check(), HINT_PAUSE_MS + 50);
      }, HINT_AUTO_HIDE_MS);
    };
    arm();
    show.addEventListener('click', () => {
      deps.openTab(def.tab, true);
      nextHint();
    });
    card.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        nextHint();
      }
    });
    hintEl = card;
    deps.hintHost.append(card);
  }

  /** The tour interrupts a tip: it shows again afterwards. */
  function putBackHint(): void {
    const id = hintEl?.dataset.hint as HintId | undefined;
    if (!id) return;
    hideHint();
    progress = { ...progress, hints: progress.hints.filter((x) => x !== id) };
    save();
  }

  function hideHint(): void {
    clearTimeout(hideTimer);
    if (!hintEl) return;
    // (The screen-reader copy goes too: it stayed in the page for minutes, playtest 5.)
    live.textContent = '';
    const hadFocus = hintEl.contains(document.activeElement);
    hintEl.remove();
    hintEl = null;
    if (hadFocus) deps.helpBtn.focus({ preventScroll: true });
  }

  function nextHint(): void {
    hideHint();
    check();
  }

  // ---------------------------------------------------------------- help
  /** Topics of the Help dialog's "How the world works" (playtest 2: attacks, grace, territory and toxins were explained nowhere). */
  const HELP_TOPICS: [string, (tap: string) => string][] = [
    ['Attacks', (tap) => `Possess one of yours, press Attack in its bar, then ${tap.toLowerCase()} another player's or a wild animal in sight. It chases and strikes; hungry pack-mates nearby join in. `
      + 'You hear how it ended: killed, escaped, out of sight, gone to ground. It can only take animals up to a weight that grows with the pack around it; the owner of the target is told.'],
    ['Newcomer grace', () => 'A newly founded lineage is protected for its first days (at least some real minutes at a fast clock): other players\' animals cannot attack it, by order or hunting on instinct, and its animals cannot attack theirs either way. Wild predators still hunt it, and it may hunt wild prey. '
      + 'Founding again while it runs, or soon after, does not start it over: the time left carries on. A hunter that wants rival prey can end its grace early in the Lineage tab; it won\'t come back by founding again soon. '
      + 'The Lineage tab shows your grace left, you are told when it ends, and a rival animal in grace shows "protected newcomer" in the inspector.'],
    ['Territories and rallies', () => 'The Territory tool (in Tools on phones) makes a group hold an area: it patrols it and stays inside. Rally gathers a group at a point for a while. '
      + 'Pick places with water: a territory or rally away from water can kill a herd in a drought.'],
    ['Toxins and colours', () => 'A toxic animal poisons whoever bites it, whether or not the attack succeeds; predators that eat it can die (the Genes tab counts "poisoned" deaths). '
      + 'Predators learn: each poisoning teaches the hunter\'s whole lineage to pass that prey up, and bright colours teach it about three times faster. '
      + 'Toxic but drab prey is no free trick: it looks like any other prey, so even taught predators keep taking most of it, and they take less harm once they know the toxin. '
      + 'Toxins pay best with warning colours. When your animals have learned a prey is poisonous, the inspector says it looks poisonous to you.'],
    ['Droughts and catastrophes', () => 'When much of the land dries out a drought notice appears over the map: animals far from lakes and rivers go thirsty. '
      + 'Catastrophes (severe winters, megadroughts, epidemics, flood years) are pinned over the map with the days left.'],
    ['Founding again: the founding ramp', () => `${FOUNDING_RAMP_TEXT} Your renown, records and titles stay when a lineage dies out or you retire it. `
      + 'The design screen says how the world looks right now (season, drought, catastrophe) before you found.'],
    ['Founding before winter: the winter provision', () => 'Winter is the hardest time to start: plants die back and cold animals burn their reserves. '
      + `A lineage founded within ${SEASON_NOTICE_DAYS} days of winter (or during it) arrives fed, with full fat reserves; the design screen says when this applies. `
      + `${FREE_FIRST_WINTER} Warm-blooded designs get through on fur, fat stores and hibernation (the starting design has all three); `
      + 'cold-blooded ones on brumation and fat stores: a cold-blooded hibernator can\'t warm itself awake.'],
    ['Why are my animals dying?', () => 'The Genes tab charts deaths by cause; each cause has a fix. '
      + 'Thirst: keep herds, territories and rallies near lakes and rivers, above all in a drought. '
      + 'Hunger: too many animals on too little food; spread out (split groups, migrate), found fewer, or eat something else. '
      + 'Winter sleep: some sleepers die every winter, more in their first year and when thin; fat stores help, and founders sleep their first winter free. '
      + 'Predators: stay in groups, add speed, armor, toxins or camouflage. '
      + 'Poisoned: your hunters ate toxic prey; bright colours warn them off. '
      + 'Drowned: poor swimmers crossing water; raise swimming or route around lakes. '
      + 'Chilled eggs: incubated eggs need a parent brooding them; abandoned eggs are left exposed by design and many die, which their large clutches make up for.'],
    ['Growth and the soft cap', () => 'Breeding slows as your lineage grows: litters shrink toward the soft cap (half size at it) and members get hungrier past it; births stop only at the hard cap. '
      + 'The Lineage tab shows where you stand.'],
  ];
  let helpEls: { backdrop: HTMLElement; dialog: HTMLElement } | null = null;

  function openHelp(): void {
    if (helpEls) return;
    const touch = deps.touch();
    const tap = touch ? 'Tap' : 'Click';
    const replay = h('button', { id: 'help-replay', type: 'button' }, 'Replay the tour');
    const tips = h('button', { class: 'secondary', id: 'help-reset-tips', type: 'button' }, 'Show tips again');
    const close = h('button', { class: 'secondary', id: 'help-close', type: 'button' }, 'Close');
    const dialog = h('div', { class: 'card help-card', id: 'help-dialog', role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'help-title' },
      h('h2', { id: 'help-title' }, 'Help'),
      h('ul', { class: 'help-list' },
        h('li', {}, touch ? 'Drag to pan, pinch to zoom; Follow keeps the camera on your lineage.' : 'Drag to pan, wheel or + / − to zoom; Follow keeps the camera on your lineage.'),
        h('li', {}, `${tap} an organism to inspect it; yours have a yellow ellipse at their feet.`),
        h('li', {}, touch ? 'Long-press one of yours (or press Possess) to control it, then tap the map to walk.' : 'Hold the mouse on one of yours (or press Possess) to control it, then click the map to walk.'),
        h('li', {}, 'Mates and Market pair your adults with others; Ranks shows the ranked seasons; alerts badge the Genes tab.')),
      h('h3', {}, 'How the world works'),
      h('div', { class: 'help-topics', id: 'help-topics' }, ...HELP_TOPICS.map(([title, body]) =>
        h('details', { class: 'help-topic' }, h('summary', {}, title), h('p', { class: 'small' }, body(tap))))),
      h('div', { class: 'row wrap' }, replay, tips, close));
    const backdrop = h('div', { class: 'help-backdrop', id: 'help' }, dialog);
    backdrop.addEventListener('mousedown', (e) => {
      if (e.target === backdrop) closeHelp(true);
    });
    replay.addEventListener('click', () => startTour());
    tips.addEventListener('click', () => {
      progress = { ...progress, hints: hintsAfterReset((hintEl?.dataset.hint as HintId | undefined) ?? null) };
      queue.length = 0;
      pauseUntil = 0;
      save();
      tips.textContent = 'Tips will show again';
      close.focus({ preventScroll: true });
      tips.disabled = true;
    });
    close.addEventListener('click', () => closeHelp(true));
    helpEls = { backdrop, dialog };
    deps.host.append(backdrop);
    window.addEventListener('keydown', onHelpKey, true);
    replay.focus({ preventScroll: true });
  }

  /** While Help is open (modal): Esc closes it and Tab stays inside, wherever focus is. */
  function onHelpKey(e: KeyboardEvent): void {
    if (!helpEls) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeHelp(true);
    } else if (e.key === 'Tab') {
      trapTab(e, helpEls.dialog);
    }
  }

  function closeHelp(restoreFocus: boolean): void {
    if (!helpEls) return;
    helpEls.backdrop.remove();
    helpEls = null;
    window.removeEventListener('keydown', onHelpKey, true);
    if (restoreFocus) deps.helpBtn.focus({ preventScroll: true });
    check();
  }

  deps.helpBtn.addEventListener('click', openHelp);

  if (!deps.disabled && autoStarts(progress)) startTour();

  return {
    startTour,
    tourOpen: () => tourEls !== null,
    check,
    openHelp,
    destroy: () => {
      clearTimeout(hideTimer);
      pauseUntil = Number.POSITIVE_INFINITY;
      deps.helpBtn.removeEventListener('click', openHelp);
      window.removeEventListener('keydown', onTourKey);
      window.removeEventListener('resize', position);
      clearInterval(repositionTimer);
      tourEls?.ring.remove();
      tourEls?.card.remove();
      tourEls = null;
      helpEls?.backdrop.remove();
      helpEls = null;
      window.removeEventListener('keydown', onHelpKey, true);
      hideHint();
      live.remove();
    },
  };
}

/** Keep Tab and Shift+Tab inside a modal dialog. */
function trapTab(e: KeyboardEvent, root: HTMLElement): void {
  const items = [...root.querySelectorAll<HTMLElement>('button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])')]
    .filter((el) => !(el as HTMLButtonElement).disabled && el.offsetParent !== null);
  if (items.length === 0) return;
  const first = items[0];
  const last = items[items.length - 1];
  if (!root.contains(document.activeElement)) {
    e.preventDefault();
    (e.shiftKey ? last : first).focus();
  } else if (e.shiftKey && document.activeElement === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}
