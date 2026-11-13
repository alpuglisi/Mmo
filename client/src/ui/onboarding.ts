// First-session onboarding, the DOM side: the guided tour (a non-modal card
// pointing at one part of the screen per step), the one-time hint cards and
// the Help dialog that replays the tour. State, steps and triggers live in
// `../onboarding.ts` (pure, unit-tested).
//
// Nothing here blocks the game: the tour's highlight ignores the pointer,
// its card is a small non-modal dialog, and every element is removed when
// closed. Only the Help dialog is modal (and traps focus) while open.

import './onboarding.css';
import type { GameModel } from '../model';
import {
  autoStarts, dueHints, HINTS, loadProgress, placeCard, progressKey, saveProgress, tourReduce, tourSteps,
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

  function check(): void {
    if (deps.disabled || tourEls || helpEls) return;
    if (progress.tour === 'new' || progress.tour === 'active') return;
    const input = { player: model.player, organisms: model.state?.organisms ?? [], feed: model.feed, offers: model.offers.values() };
    for (const id of dueHints(input, [...progress.hints, ...queue])) queue.push(id);
    if (!hintEl && queue.length > 0) showHint(queue.shift()!);
  }

  function showHint(id: HintId): void {
    const def = HINTS[id];
    progress = { ...progress, hints: [...progress.hints, id] };
    save();
    const dismiss = h('button', { class: 'secondary', id: 'hint-dismiss', type: 'button' }, 'Got it');
    const show = h('button', { id: 'hint-show', type: 'button' }, 'Show me');
    const titleId = `hint-title-${id}`;
    const card = h('div', { class: 'hint-card', id: 'onboarding-hint', role: 'region', 'aria-labelledby': titleId, 'data-hint': id },
      h('strong', { id: titleId }, def.title), h('p', {}, def.body), h('div', { class: 'row' }, show, dismiss));
    // Announced politely; focus stays where the player is.
    live.textContent = `Tip: ${def.title}. ${def.body}`;
    dismiss.addEventListener('click', () => nextHint());
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
    if (!hintEl) return;
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
        h('li', {}, 'Matings and the Market pair your adults with others; Seasons ranks players; alerts badge the Genetics tab.')),
      h('div', { class: 'row wrap' }, replay, tips, close));
    const backdrop = h('div', { class: 'help-backdrop', id: 'help' }, dialog);
    backdrop.addEventListener('mousedown', (e) => {
      if (e.target === backdrop) closeHelp(true);
    });
    replay.addEventListener('click', () => startTour());
    tips.addEventListener('click', () => {
      progress = { ...progress, hints: [] };
      queue.length = 0;
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
