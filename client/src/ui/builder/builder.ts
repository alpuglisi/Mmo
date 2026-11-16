// The body builder: edits a `Morphology` by dragging features on two
// drawings of the creature.
//
//   Top view (the head from above, front up): drag an eye round the head to
//   set the pair angle (its mirrored partner follows), drag the knob on the
//   rim to widen or narrow every eye's field, add or remove eyes (an odd one
//   sits on the midline). The fields of view, the binocular overlap in front
//   and the blind spots are drawn and counted (`vision.ts`).
//   Side view: ears (count, shape, size), legs (count, length), tail (none or
//   a shape, length), snout length and fangs, each length or size on a
//   handle.
//
// Every handle takes mouse, touch and pen (pointer events) and the keyboard
// (Tab to it; arrows move or resize, Shift for bigger steps, Home / End;
// + / − add or remove the feature it belongs to). Ctrl+Z / Ctrl+Shift+Z (or
// Ctrl+Y) undo and redo while focus is in the builder. Standalone: the
// caller mounts `el` and listens to `onChange`.

import './builder.css';
import { h } from '../dom';
import {
  clampMorphology,
  defaultMorphology,
  eyeFieldWidth,
  eyePlacements,
  EAR_COUNT_MAX,
  EAR_SHAPES,
  EYE_COUNT_MAX,
  EYE_COUNT_MIN,
  EYE_PLACEMENT_MAX,
  LEG_COUNTS,
  PAIR_STEP,
  sameMorphology,
  TAIL_SHAPES,
  type EarShape,
  type Morphology,
  type TailShape,
} from '../../morphology';
import { DEFAULT_VISION, signedAngle, visionOf, visionSummary, type Arc, type VisionStats } from '../../vision';

// The readout helpers live in `vision.ts` (the design editor shows them too).
export { DEFAULT_VISION, visionSummary };
import { History } from './history';
import { DEFAULT_PRESETS, type BuilderPreset } from './presets';
import { azimuthOf, earIcon, polar, s, tailIcon, wedgePath } from './svg';
import { sideTracks, sideView, type HandleTrack } from './sideView';

export type BuilderFeature = 'eyes' | 'ears' | 'legs' | 'tail' | 'snout' | 'fangs';

export interface BuilderChange {
  /** What made the change. */
  source: 'drag' | 'key' | 'control' | 'preset' | 'reset' | 'undo' | 'redo' | 'set';
  /** The feature that changed, when one did. */
  feature: BuilderFeature | null;
  /**
   * False while a drag is still moving (the value will change again soon);
   * true for every finished change. Expensive listeners (a server cost
   * estimate) can wait for `final`.
   */
  final: boolean;
}

export interface BodyBuilderOptions {
  /** Starting value (clamped; missing fields take the defaults). */
  value?: Partial<Morphology>;
  onChange?: (value: Morphology, change: BuilderChange) => void;
  presets?: readonly BuilderPreset[];
  /**
   * Asked before a preset is applied, with the value it would give (e.g. to
   * show the cost change first). Resolve false to cancel.
   */
  confirmPreset?: (preset: BuilderPreset, next: Morphology) => boolean | Promise<boolean>;
  /** Prefix for element ids, so two builders can share a page. Default `bb`. */
  idPrefix?: string;
  /**
   * Points per extra structure (the server catalog's `morph` costs), named
   * on the add buttons. Absent or 0: nothing is said about points.
   */
  prices?: { eye?: number; legPair?: number; earPair?: number };
}

/** " (+3 points each beyond two)", or nothing for a free structure. */
function priceNote(points: number | undefined, beyond: string): string {
  return points && points > 0 ? ` (+${Number(points.toFixed(1))} points each beyond ${beyond})` : '';
}

export interface BodyBuilder {
  readonly el: HTMLElement;
  /** The current value (a fresh copy). */
  readonly value: Morphology;
  /** The current vision numbers. */
  readonly vision: VisionStats;
  readonly canUndo: boolean;
  readonly canRedo: boolean;
  /**
   * Change the value from outside (e.g. a design slider moved). Recorded as
   * an undo step unless `record: false` (which also clears the history);
   * `onChange` is not called unless `emit: true`.
   */
  set(value: Partial<Morphology>, opts?: { record?: boolean; emit?: boolean }): void;
  undo(): boolean;
  redo(): boolean;
  /** Back to the defaults (an undoable step). */
  reset(): void;
  /** Apply a preset (after `confirmPreset`, if given); resolves whether it was applied. */
  applyPreset(id: string): Promise<boolean>;
  destroy(): void;
}

// --- geometry of the drawings (SVG user units)

/** Top view: radius of the field-of-view rim, of the head, of the eye ring. */
const RIM = 96;
const HEAD_R = 38;
const EYE_RING = 35;
/**
 * Invisible hit radius round every handle, in each drawing's units: at least
 * 22 px (a 44 px target) at a phone's drawing width (the top view is
 * ~1.4 px per unit there, the side view ~1.05).
 */
const HIT_TOP = 17;
const HIT_SIDE = 21;
/** Picking a tail shape makes a stub tail at least this long, so the choice shows. */
const TAIL_SHOWN = 0.3;

const EAR_LABEL: Record<EarShape, string> = { round: 'Round', pointed: 'Pointed', long: 'Long', fan: 'Fan' };
const TAIL_LABEL: Record<TailShape, string> = { thin: 'Thin', bushy: 'Bushy', fin: 'Fin', club: 'Club', prehensile: 'Grasping' };

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
const round = (v: number): number => Math.round(v);
const midOf = (a: Arc): number => a.start + a.width / 2;

/** One draggable handle's behaviour for the current value. */
interface HandleSpec {
  feature: BuilderFeature;
  label: string;
  x: number;
  y: number;
  /** Knob radius (drawn). */
  r: number;
  now: number;
  min: number;
  max: number;
  text: string;
  /** The change for a pointer at (x, y) in the drawing's units; null = not draggable. */
  drag: ((x: number, y: number) => Partial<Morphology>) | null;
  /** The change for a key; null = not handled. */
  key: (key: string, big: boolean) => Partial<Morphology> | null;
}

let instances = 0;

export function createBodyBuilder(opts: BodyBuilderOptions = {}): BodyBuilder {
  const p = opts.idPrefix ?? 'bb';
  const uid = `${p}-${++instances}`;
  const presets = opts.presets ?? DEFAULT_PRESETS;
  const history = new History<Morphology>(clampMorphology(opts.value ?? {}), { same: sameMorphology, mergeMs: 1200, limit: 100 });
  let vision = visionOf(history.value);
  const m = (): Morphology => history.value;

  function emit(change: BuilderChange): void {
    opts.onChange?.({ ...m() }, change);
  }

  /** Record a change (merging into `group`'s step) and redraw; true when something changed. */
  function commit(patch: Partial<Morphology>, change: BuilderChange, group: string | null = null): boolean {
    const next = clampMorphology({ ...m(), ...patch }, m());
    if (!history.push(next, group)) return false;
    render();
    emit(change);
    return true;
  }

  // ------------------------------------------------------------ skeleton

  const btn = (id: string, text: string, title: string, onClick: () => void, cls = 'secondary'): HTMLButtonElement =>
    h('button', { type: 'button', id: `${p}-${id}`, class: `${cls} bb-btn`, title, 'aria-label': title, onclick: onClick as EventListener }, text);

  const undoBtn = btn('undo', '↶ Undo', 'Undo (Ctrl+Z)', () => doUndo());
  const redoBtn = btn('redo', '↷ Redo', 'Redo (Ctrl+Shift+Z)', () => doRedo());
  const resetBtn = btn('reset', 'Reset', 'Reset to the default body', () => doReset());
  const presetBtns = presets.map((pr) => {
    const b = h('button', { type: 'button', class: 'secondary bb-btn bb-preset', 'data-preset': pr.id, title: pr.hint }, pr.label);
    b.addEventListener('click', () => void applyPreset(pr.id));
    return b;
  });
  const toolbar = h('div', { class: 'bb-toolbar', role: 'toolbar', 'aria-label': 'Body builder' },
    h('div', { class: 'bb-tools' }, undoBtn, redoBtn, resetBtn),
    h('div', { class: 'bb-presets' }, h('span', { class: 'bb-presets-label muted' }, 'Presets'), ...presetBtns));

  /** A − count + stepper. */
  const stepper = (id: string, feature: BuilderFeature, name: string, dec: () => void, inc: () => void) => {
    const count = h('output', { id: `${p}-${id}-count`, class: 'bb-count', 'aria-live': 'polite' }, '');
    const minus = btn(`${id}-minus`, '−', `Remove ${name}`, dec);
    const plus = btn(`${id}-plus`, '+', `Add ${name}`, inc);
    const row = h('div', { class: 'bb-stepper', 'data-feature': feature, role: 'group', 'aria-label': `${name}s` }, minus, count, plus);
    return { row, count, minus, plus };
  };

  // --- top view
  const topSvg = s('svg', {
    class: 'bb-svg bb-top', viewBox: `${-RIM - HIT_TOP - 2} ${-RIM - HIT_TOP - 2} ${2 * (RIM + HIT_TOP + 2)} ${2 * (RIM + HIT_TOP + 2)}`,
    role: 'group', 'aria-label': 'Head seen from above, front at the top: eyes and their fields of view', id: `${p}-top`,
  });
  topSvg.appendChild(s('defs', {},
    s('pattern', { id: `${uid}-hatch`, patternUnits: 'userSpaceOnUse', width: 6, height: 6, patternTransform: 'rotate(45)' },
      s('rect', { width: 6, height: 6, class: 'bb-hatch-bg' }),
      s('line', { x1: 0, y1: 0, x2: 0, y2: 6, class: 'bb-hatch-line' })),
    // Depth zone: dots, so it reads without colour too.
    s('pattern', { id: `${uid}-dots`, patternUnits: 'userSpaceOnUse', width: 5, height: 5 },
      s('rect', { width: 5, height: 5, class: 'bb-dots-bg' }),
      s('circle', { cx: 2.5, cy: 2.5, r: 1.1, class: 'bb-dots-dot' }))));
  const topDraw = s('g', { class: 'bb-draw' });
  const topHandles = s('g', { class: 'bb-handles' });
  topSvg.append(topDraw, topHandles);

  const eyeStep = stepper('eye', 'eyes', 'eye', () => addEyes(-1), () => addEyes(1));
  const eyeNote = priceNote(opts.prices?.eye, 'two');
  if (eyeNote) for (const a of ['title', 'aria-label']) eyeStep.plus.setAttribute(a, `Add eye${eyeNote}`);
  /** A readout: the value, and an arrow against the default body. `higherIsBetter` colours the arrow. */
  const stat = (key: string, title: string, hint: string, higherIsBetter: boolean) => {
    const v = h('dd', { id: `${p}-stat-${key}`, 'data-stat': key }, '');
    const delta = h('span', { class: 'bb-delta', id: `${p}-delta-${key}` }, '');
    return { el: h('div', { class: `bb-stat bb-stat-${key}`, title: hint }, h('dt', {}, title), v, delta), v, delta, higherIsBetter };
  };
  const stats = {
    coverage: stat('coverage', 'View', 'Share of the full circle seen by at least one eye', true),
    overlap: stat('overlap', 'Depth', 'Degrees within 60° of straight ahead seen by two eyes at once (binocular overlap), less what a long snout hides', true),
    rear: stat('rear-blind', 'Blind behind', 'Width of the unseen arc straight behind', false),
    front: stat('front-blind', 'Blind ahead', 'Width of the unseen arc straight ahead', false),
  };
  const summary = h('p', { class: 'bb-summary', id: `${p}-summary`, 'aria-live': 'polite' }, '');

  const eyesPanel = h('section', { class: 'bb-panel bb-eyes', 'aria-labelledby': `${p}-eyes-title` },
    h('div', { class: 'bb-panel-head' }, h('h3', { id: `${p}-eyes-title` }, 'Eyes'), h('span', { class: 'muted small' }, 'top view, front ↑')),
    topSvg,
    h('div', { class: 'bb-row' }, h('span', { class: 'bb-row-label' }, 'Eyes'), eyeStep.row),
    h('dl', { class: 'bb-stats' }, stats.coverage.el, stats.overlap.el, stats.rear.el, stats.front.el),
    summary,
    h('p', { class: 'bb-hint muted small' }, 'Drag an eye round the head; drag the rim knob to widen the fields. Keyboard: Tab to a handle, arrows move it, + / − add or remove.'));

  // --- side view
  const sideSvg = s('svg', {
    class: 'bb-svg bb-side', viewBox: '-14 -18 318 212', role: 'group',
    'aria-label': 'Body seen from the side, facing right: ears, legs, tail, snout and fangs', id: `${p}-side`,
  });
  const sideDraw = s('g', { class: 'bb-draw' });
  const sideHandles = s('g', { class: 'bb-handles' });
  sideSvg.append(sideDraw, sideHandles);

  const earStep = stepper('ear', 'ears', 'pair of ears', () => addEars(-1), () => addEars(1));
  const earNote = priceNote(opts.prices?.earPair, 'one pair');
  if (earNote) for (const a of ['title', 'aria-label']) earStep.plus.setAttribute(a, `Add pair of ears${earNote}`);
  const choice = (group: string, value: string, label: string, icon: SVGSVGElement | null, onPick: () => void) => {
    const b = h('button', { type: 'button', class: 'bb-choice', 'data-group': group, 'data-value': value, 'aria-pressed': 'false', title: label, 'aria-label': label });
    if (icon) b.appendChild(icon);
    else b.textContent = label;
    b.addEventListener('click', onPick);
    return b;
  };
  const earShapeBtns = EAR_SHAPES.map((sh) => choice('ear-shape', sh, `${EAR_LABEL[sh]} ears`, earIcon(sh), () => commit({ ear_shape: sh }, { source: 'control', feature: 'ears', final: true })));
  const legBtns = LEG_COUNTS.map((n) => {
    const extra = Math.max(0, n - 4) / 2;
    const pts = (opts.prices?.legPair ?? 0) * extra;
    const label = pts > 0 ? `${n} legs (+${Number(pts.toFixed(1))} points)` : `${n} legs`;
    const b = choice('legs', String(n), label, null, () => commit({ leg_count: n }, { source: 'control', feature: 'legs', final: true }));
    b.textContent = String(n);
    return b;
  });
  const tailBtns = [
    choice('tail', 'none', 'No tail', tailIcon(null), () => commit({ tail_present: false }, { source: 'control', feature: 'tail', final: true })),
    ...TAIL_SHAPES.map((sh) => choice('tail', sh, `${TAIL_LABEL[sh]} tail`, tailIcon(sh), () => commit({ tail_present: true, tail_shape: sh, tail_length: Math.max(m().tail_length, TAIL_SHOWN) }, { source: 'control', feature: 'tail', final: true }))),
  ];
  const choices = [...earShapeBtns, ...legBtns, ...tailBtns];
  const group = (feature: BuilderFeature, label: string, ...kids: HTMLElement[]) =>
    h('div', { class: 'bb-row', 'data-feature': feature }, h('span', { class: 'bb-row-label' }, label), h('div', { class: 'bb-choices', role: 'group', 'aria-label': label }, ...kids));

  const bodyPanel = h('section', { class: 'bb-panel bb-body', 'aria-labelledby': `${p}-body-title` },
    h('div', { class: 'bb-panel-head' }, h('h3', { id: `${p}-body-title` }, 'Body'), h('span', { class: 'muted small' }, 'side view, facing →')),
    sideSvg,
    h('div', { class: 'bb-row' }, h('span', { class: 'bb-row-label' }, 'Ears'), earStep.row),
    group('ears', 'Ear shape', ...earShapeBtns),
    group('legs', 'Legs', ...legBtns),
    group('tail', 'Tail', ...tailBtns),
    h('p', { class: 'bb-hint muted small' }, 'Drag the round knobs to size the ears, legs, tail, snout and fangs.'));

  const el = h('div', { class: 'bb', id: p }, toolbar, h('div', { class: 'bb-panels' }, eyesPanel, bodyPanel));

  // ------------------------------------------------------------ handles

  const specs = new Map<string, HandleSpec>();
  const handleEls = new Map<string, SVGGElement>();
  let gestures = 0;

  function svgPoint(svg: SVGSVGElement, e: PointerEvent): { x: number; y: number } | null {
    const ctm = svg.getScreenCTM();
    if (!ctm) return null;
    const pt = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    return { x: pt.x, y: pt.y };
  }

  function makeHandle(key: string, svg: SVGSVGElement, layer: SVGGElement): SVGGElement {
    const g = s('g', { class: 'bb-handle', tabindex: 0, role: 'slider', 'data-handle': key, id: `${p}-h-${key}` });
    g.append(s('circle', { class: 'bb-hit', r: svg === topSvg ? HIT_TOP : HIT_SIDE }), s('circle', { class: 'bb-knob', r: 6 }));
    // `touch-action: none` isn't honoured on SVG children everywhere: stop a
    // finger on a handle from panning the page (the pointer events still come).
    g.addEventListener('touchstart', (e) => {
      if (specs.get(key)?.drag) e.preventDefault();
    }, { passive: false });
    let drag: { id: number; group: string; moved: boolean } | null = null;
    g.addEventListener('pointerdown', (e) => {
      if (e.button !== 0 && e.pointerType === 'mouse') return;
      g.focus({ preventScroll: true });
      const spec = specs.get(key);
      if (!spec?.drag) return;
      e.preventDefault();
      try {
        g.setPointerCapture(e.pointerId);
      } catch {
        /* synthetic events have no active pointer */
      }
      drag = { id: e.pointerId, group: `drag-${++gestures}`, moved: false };
      g.classList.add('bb-dragging');
    });
    g.addEventListener('pointermove', (e) => {
      if (!drag || e.pointerId !== drag.id) return;
      const spec = specs.get(key);
      const pt = svgPoint(svg, e);
      if (!spec?.drag || !pt) return;
      if (commit(spec.drag(pt.x, pt.y), { source: 'drag', feature: spec.feature, final: false }, drag.group)) drag.moved = true;
    });
    const end = (e: PointerEvent) => {
      if (!drag || e.pointerId !== drag.id) return;
      const d = drag;
      drag = null;
      g.classList.remove('bb-dragging');
      history.seal();
      if (d.moved) emit({ source: 'drag', feature: specs.get(key)?.feature ?? null, final: true });
    };
    g.addEventListener('pointerup', end);
    g.addEventListener('pointercancel', end);
    g.addEventListener('lostpointercapture', end);
    g.addEventListener('keydown', (e) => {
      const spec = specs.get(key);
      if (!spec || e.ctrlKey || e.metaKey || e.altKey) return;
      const patch = spec.key(e.key, e.shiftKey);
      if (!patch) return;
      e.preventDefault();
      e.stopPropagation();
      commit(patch, { source: 'key', feature: spec.feature, final: true }, `key-${key}`);
    });
    layer.appendChild(g);
    return g;
  }

  function syncHandles(next: Map<string, HandleSpec & { svg: SVGSVGElement; layer: SVGGElement }>): void {
    for (const [key, g] of handleEls) {
      if (!next.has(key)) {
        g.remove();
        handleEls.delete(key);
      }
    }
    specs.clear();
    for (const [key, spec] of next) {
      specs.set(key, spec);
      let g = handleEls.get(key);
      if (!g) {
        g = makeHandle(key, spec.svg, spec.layer);
        handleEls.set(key, g);
      }
      g.setAttribute('transform', `translate(${spec.x.toFixed(2)} ${spec.y.toFixed(2)})`);
      g.setAttribute('aria-label', spec.label);
      g.setAttribute('aria-valuemin', String(spec.min));
      g.setAttribute('aria-valuemax', String(spec.max));
      g.setAttribute('aria-valuenow', String(Math.round(spec.now * 100) / 100));
      g.setAttribute('aria-valuetext', spec.text);
      g.setAttribute('data-feature', spec.feature);
      g.classList.toggle('bb-fixed', !spec.drag);
      g.querySelector('.bb-knob')!.setAttribute('r', String(spec.r));
    }
  }

  /** Arrow keys for a 0–1 value: up / right raise it. */
  const unitKeys = (field: keyof Morphology, v: number) => (key: string, big: boolean): Partial<Morphology> | null => {
    const step = big ? 0.2 : 0.05;
    const to = (x: number) => ({ [field]: Math.round(clamp(x, 0, 1) * 100) / 100 }) as Partial<Morphology>;
    switch (key) {
      case 'ArrowUp': case 'ArrowRight': return to(v + step);
      case 'ArrowDown': case 'ArrowLeft': return to(v - step);
      case 'PageUp': return to(v + 0.2);
      case 'PageDown': return to(v - 0.2);
      case 'Home': return to(0);
      case 'End': return to(1);
      default: return null;
    }
  };

  /** A handle for a 0–1 field that slides from (x0,y0) at 0 to (x1,y1) at 1. */
  function linear(feature: BuilderFeature, field: keyof Morphology, label: string, x0: number, y0: number, x1: number, y1: number): HandleSpec {
    const v = m()[field] as number;
    sideDraw.appendChild(s('line', { x1: x0, y1: y0, x2: x1, y2: y1, class: 'bb-track' }));
    return {
      feature, label, x: x0 + (x1 - x0) * v, y: y0 + (y1 - y0) * v, r: 6, now: v, min: 0, max: 1, text: `${Math.round(v * 100)}%`,
      drag: (x, y) => {
        const dx = x1 - x0;
        const dy = y1 - y0;
        const t = ((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy);
        return { [field]: Math.round(clamp(t, 0, 1) * 100) / 100 } as Partial<Morphology>;
      },
      key: unitKeys(field, v),
    };
  }

  // ------------------------------------------------------------ drawing

  function renderTop(cur: Morphology, out: Map<string, HandleSpec & { svg: SVGSVGElement; layer: SVGGElement }>): void {
    topDraw.replaceChildren();
    const width = eyeFieldWidth(cur.eye_size);
    const eyes = eyePlacements(cur);
    const pairs = Math.floor(cur.eye_count / 2);
    const add = (el: SVGElement) => topDraw.appendChild(el);

    add(s('circle', { r: RIM, class: 'bb-rim' }));
    for (const a of vision.covered) add(s('path', { d: wedgePath(a.start, a.width, RIM), class: 'bb-cover' }));
    for (const a of vision.blind) {
      const kind = a === vision.rearBlind ? 'rear' : a === vision.frontBlind ? 'front' : 'side';
      add(s('path', { d: wedgePath(a.start, a.width, RIM), class: `bb-blind bb-blind-${kind}`, fill: `url(#${uid}-hatch)` }));
    }
    for (const a of vision.overlap) add(s('path', { d: wedgePath(a.start, a.width, RIM), class: 'bb-overlap', fill: `url(#${uid}-dots)` }));
    for (const e of eyes) add(s('path', { d: wedgePath(e.azimuth - width / 2, width, RIM), class: 'bb-cone' }));
    // What a long snout hides of the depth zone, straight ahead.
    if (vision.occludedDeg > 0) add(s('path', { d: wedgePath(-vision.occludedDeg / 2, vision.occludedDeg, RIM), class: 'bb-occluded' }));
    // Compass ticks.
    for (const a of [0, 90, 180, -90]) {
      const [x0, y0] = polar(a, RIM - 5);
      const [x1, y1] = polar(a, RIM);
      add(s('line', { x1: x0, y1: y0, x2: x1, y2: y1, class: 'bb-tick' }));
    }
    // Labels on the arcs that matter.
    const label = (a: Arc, text: string, cls: string, r = 64) => {
      const [x, y] = polar(midOf(a), r);
      add(s('text', { x, y, class: `bb-arc-label ${cls}`, 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, text));
    };
    if (vision.rearBlind) label(vision.rearBlind, `${round(vision.rearBlindDeg)}° blind`, 'bb-label-blind');
    if (vision.frontBlind) label(vision.frontBlind, `${round(vision.frontBlindDeg)}°`, 'bb-label-blind', 74);
    const frontOverlap = vision.overlap.find((a) => Math.abs(signedAngle(midOf(a))) < 1) ?? vision.overlap[0];
    if (frontOverlap && !vision.frontBlind) label(frontOverlap, `${round(vision.overlapDeg)}° depth`, 'bb-label-overlap', 74);

    // The head: snout forward, then the skull.
    const snout = 8 + 26 * cur.snout_length;
    add(s('ellipse', { cx: 0, cy: -(HEAD_R - 6 + snout / 2), rx: 13, ry: snout / 2 + 6, class: 'bb-head' }));
    add(s('circle', { r: HEAD_R, class: 'bb-head' }));
    add(s('text', { x: -RIM - HIT_TOP, y: -RIM - 4, class: 'bb-compass' }, 'front ↑'));

    const eyeR = 4 + 4 * cur.eye_size;
    const result = `${round(vision.coveredDeg)}° view, ${round(vision.overlapDeg)}° depth, ${round(vision.frontBlindDeg)}° blind ahead, ${round(vision.rearBlindDeg)}° blind behind`;
    eyes.forEach((e, i) => {
      const [x, y] = polar(e.azimuth, EYE_RING);
      const az = Math.abs(e.azimuth);
      if (e.pair === null) {
        out.set(`eye-${i}`, {
          svg: topSvg, layer: topHandles, feature: 'eyes', x, y, r: eyeR,
          label: 'Midline eye (fixed straight ahead): arrows resize all eyes',
          now: cur.eye_size, min: 0, max: 1, text: `midline eye, size ${Math.round(cur.eye_size * 100)}%: ${result}`,
          drag: null, key: unitKeys('eye_size', cur.eye_size),
        });
        return;
      }
      const offset = PAIR_STEP * e.pair;
      out.set(`eye-${i}`, {
        svg: topSvg, layer: topHandles, feature: 'eyes', x, y, r: eyeR,
        label: `${e.side === 'right' ? 'Right' : 'Left'} eye${pairs > 1 ? ` (pair ${e.pair + 1})` : ''}: drag round the head; arrows up / down move it forward / back`,
        now: round(cur.eye_azimuth), min: 0, max: 180,
        text: `eyes ${round(az)}° to the side: ${result}`,
        // Pair k sits at the pair angle + 50°·k: dragging any eye sets the pair angle.
        drag: (px, py) => ({ eye_azimuth: round(clamp(Math.abs(azimuthOf(px, py)) - offset, 0, EYE_PLACEMENT_MAX)) }),
        key: (key, big) => {
          const step = big ? 15 : 5;
          const az0 = Math.min(cur.eye_azimuth, EYE_PLACEMENT_MAX);
          switch (key) {
            case 'ArrowUp': case 'ArrowLeft': return { eye_azimuth: clamp(az0 - step, 0, EYE_PLACEMENT_MAX) };
            case 'ArrowDown': case 'ArrowRight': return { eye_azimuth: clamp(az0 + step, 0, EYE_PLACEMENT_MAX) };
            case 'Home': return { eye_azimuth: 0 };
            case 'End': return { eye_azimuth: EYE_PLACEMENT_MAX };
            default: return null;
          }
        },
      });
    });

    // Field-width knob: on the rim at the outer edge of the reference eye's field.
    const ref = eyes.find((e) => e.side === 'right') ?? eyes[0];
    const edge = ref.azimuth + width / 2;
    const [kx, ky] = polar(edge, RIM);
    const [ex, ey] = polar(ref.azimuth, EYE_RING);
    add(s('line', { x1: ex, y1: ey, x2: kx, y2: ky, class: 'bb-track' }));
    out.set('eye-size', {
      svg: topSvg, layer: topHandles, feature: 'eyes', x: kx, y: ky, r: 6,
      label: 'Eye size: drag along the rim to widen or narrow every field of view',
      now: cur.eye_size, min: 0, max: 1, text: `size ${Math.round(cur.eye_size * 100)}%, field ${round(width)}° per eye`,
      drag: (px, py) => {
        const w = 2 * Math.abs(signedAngle(azimuthOf(px, py) - ref.azimuth));
        return { eye_size: Math.round(clamp((w - 140) / 60, 0, 1) * 100) / 100 };
      },
      key: unitKeys('eye_size', cur.eye_size),
    });
  }

  function renderSide(cur: Morphology, out: Map<string, HandleSpec & { svg: SVGSVGElement; layer: SVGGElement }>): void {
    sideDraw.replaceChildren();
    const add = (el: SVGElement) => sideDraw.appendChild(el);
    const at = (spec: HandleSpec) => ({ ...spec, svg: sideSvg, layer: sideHandles });

    // The creature from the art library (`sideView.ts`), its torso pinned in
    // place so the handles stay put while other features change.
    const view = sideView(cur);
    add(s('line', { x1: -10, y1: view.groundY, x2: 300, y2: view.groundY, class: 'bb-ground' }));
    const creature = s('g', { class: 'bb-creature', transform: view.transform, 'aria-hidden': 'true' });
    creature.innerHTML = view.markup(`${uid}-av`);
    add(creature);

    // Handles ride the features' tips: each track runs from the tip at 0 to the tip at 1.
    const track = (feature: BuilderFeature, field: keyof Morphology, label: string, t: HandleTrack) =>
      at(linear(feature, field, label, t.from[0], t.from[1], t.to[0], t.to[1]));
    const tracks = sideTracks(cur);
    out.set('snout', track('snout', 'snout_length', 'Snout length', tracks.snout));
    out.set('fangs', track('fangs', 'fang_size', 'Fang size (none below 10%)', tracks.fangs));
    if (tracks.ears) out.set('ears', track('ears', 'ear_size', 'Ear size', tracks.ears));
    if (tracks.legs) out.set('legs', track('legs', 'leg_length', 'Leg length', tracks.legs));
    if (tracks.tail) out.set('tail', track('tail', 'tail_length', 'Tail length', tracks.tail));
  }

  function render(): void {
    const cur = m();
    vision = visionOf(cur);
    const next = new Map<string, HandleSpec & { svg: SVGSVGElement; layer: SVGGElement }>();
    renderTop(cur, next);
    renderSide(cur, next);
    syncHandles(next);

    const setStat = (st: (typeof stats)[keyof typeof stats], value: number, base: number, fmt: (x: number) => string, unit: string) => {
      st.v.textContent = fmt(value) + unit;
      st.v.dataset.value = String(value);
      const d = Math.round(Number(fmt(value)) - Number(fmt(base)));
      const trend = d === 0 ? 'same' : (d > 0) === st.higherIsBetter ? 'better' : 'worse';
      st.delta.textContent = d === 0 ? '= default' : `${d > 0 ? '↑' : '↓'}${Math.abs(d)}${unit === '%' ? ' pts' : '°'} vs default`;
      st.el.dataset.trend = trend;
    };
    const pct0 = (x: number) => (x * 100).toFixed(0);
    const deg = (x: number) => String(round(x));
    setStat(stats.coverage, vision.coverage, DEFAULT_VISION.coverage, pct0, '%');
    setStat(stats.overlap, vision.overlapDeg, DEFAULT_VISION.overlapDeg, deg, '°');
    setStat(stats.rear, vision.rearBlindDeg, DEFAULT_VISION.rearBlindDeg, deg, '°');
    setStat(stats.front, vision.frontBlindDeg, DEFAULT_VISION.frontBlindDeg, deg, '°');
    summary.textContent = visionSummary(vision);

    eyeStep.count.textContent = String(cur.eye_count);
    eyeStep.minus.disabled = cur.eye_count <= EYE_COUNT_MIN;
    eyeStep.plus.disabled = cur.eye_count >= EYE_COUNT_MAX;
    earStep.count.textContent = String(cur.ear_count);
    earStep.minus.disabled = cur.ear_count <= 0;
    earStep.count.title = `${cur.ear_count} ears`;
    earStep.plus.disabled = cur.ear_count >= EAR_COUNT_MAX;
    for (const b of choices) {
      const v = b.dataset.value;
      const on = b.dataset.group === 'ear-shape' ? v === cur.ear_shape
        : b.dataset.group === 'legs' ? v === String(cur.leg_count)
          : v === (cur.tail_present ? cur.tail_shape : 'none');
      b.setAttribute('aria-pressed', String(on));
      if (b.dataset.group === 'ear-shape') b.disabled = cur.ear_count === 0;
    }
    undoBtn.disabled = !history.canUndo;
    redoBtn.disabled = !history.canRedo;
    for (const b of presetBtns) {
      const pr = presets.find((x) => x.id === b.dataset.preset)!;
      b.setAttribute('aria-pressed', String(sameMorphology(clampMorphology({ ...cur, ...pr.value }), cur)));
    }
    el.dataset.eyes = String(cur.eye_count);
  }

  // ------------------------------------------------------------ actions

  function addEyes(d: number): void {
    commit({ eye_count: m().eye_count + d }, { source: 'control', feature: 'eyes', final: true });
  }
  function addEars(d: number): void {
    commit({ ear_count: clamp(m().ear_count + 2 * d, 0, EAR_COUNT_MAX) as Morphology['ear_count'] }, { source: 'control', feature: 'ears', final: true });
  }
  function addFeature(feature: BuilderFeature, d: number): boolean {
    const cur = m();
    const src = { source: 'key' as const, feature, final: true };
    switch (feature) {
      case 'eyes': return commit({ eye_count: cur.eye_count + d }, src);
      case 'ears': return commit({ ear_count: clamp(cur.ear_count + 2 * d, 0, EAR_COUNT_MAX) as Morphology['ear_count'] }, src);
      case 'legs': return commit({ leg_count: clamp(cur.leg_count + 2 * d, 0, 8) as Morphology['leg_count'] }, src);
      case 'tail': return commit({ tail_present: d > 0 }, src);
      default: return false;
    }
  }
  function doUndo(): boolean {
    if (!history.undo()) return false;
    render();
    emit({ source: 'undo', feature: null, final: true });
    return true;
  }
  function doRedo(): boolean {
    if (!history.redo()) return false;
    render();
    emit({ source: 'redo', feature: null, final: true });
    return true;
  }
  function doReset(): void {
    history.seal();
    commit(defaultMorphology(), { source: 'reset', feature: null, final: true });
  }
  async function applyPreset(id: string): Promise<boolean> {
    const pr = presets.find((x) => x.id === id);
    if (!pr) return false;
    const before = m();
    const next = clampMorphology({ ...before, ...pr.value }, before);
    if (opts.confirmPreset && !(await opts.confirmPreset(pr, next))) return false;
    history.seal();
    commit(next, { source: 'preset', feature: null, final: true });
    return true;
  }

  el.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (mod && !e.altKey && (k === 'z' || k === 'y')) {
      e.preventDefault();
      if (k === 'y' || e.shiftKey) doRedo();
      else doUndo();
      return;
    }
    if (mod || e.altKey) return;
    if (e.key === '+' || e.key === '=' || e.key === '-' || e.key === '−' || e.key === '_') {
      const target = e.target as Element | null;
      const feature = (target?.closest('[data-feature]')?.getAttribute('data-feature') ?? 'eyes') as BuilderFeature;
      if (addFeature(feature, e.key === '+' || e.key === '=' ? 1 : -1)) e.preventDefault();
    }
  });

  render();

  return {
    el,
    get value() {
      return { ...m() };
    },
    get vision() {
      return vision;
    },
    get canUndo() {
      return history.canUndo;
    },
    get canRedo() {
      return history.canRedo;
    },
    set(value, o = {}) {
      const next = clampMorphology({ ...m(), ...value }, m());
      if (o.record === false) history.reset(next);
      else {
        history.seal();
        history.push(next);
      }
      render();
      if (o.emit) emit({ source: 'set', feature: null, final: true });
    },
    undo: doUndo,
    redo: doRedo,
    reset: doReset,
    applyPreset,
    destroy() {
      el.remove();
      specs.clear();
      handleEls.clear();
    },
  };
}
