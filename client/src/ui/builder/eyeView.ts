// The body builder's eye view: the head seen from above, snout up, with
// what each eye sees drawn around it (the "Eyes" panel).
//
//   - Each eye sits on the head's outline and throws its own translucent
//     wedge of view, from the eye out to the rim; where wedges cross they
//     read darker.
//   - Depth vision (the part of the front ±60° that two eyes see at once,
//     `vision.ts`) is green and dotted, inside the eyes' crossing wedges.
//   - Blind spots are greyed and hatched with an eye-slash icon; a long
//     snout's shadow in front is shown and named.
//   - Plain labels ("Depth vision / 20° ahead", "Blind spot / 100° behind",
//     "Sees 260° around") and a legend that matches the map's vision cones
//     (`world/visionCones.ts`, the `.vision-legend` classes in style.css).
//
// The numbers are `vision.ts`'s (the sim's model, angles from the head's
// centre); the wedges start at the eyes, so they meet the model's arcs
// exactly at the rim. Colours are CSS custom properties (`--ev-*` on
// `.bb`, builder.css) so the art can restyle them.
//
// The builder owns the handles (pointer + keyboard): this view draws, and
// says where each handle goes and what dragging it changes.

import './eyeView.css';
import { eyeFieldWidth, eyePlacements, EYE_PLACEMENT_MAX, PAIR_STEP, type EyePlacement, type Morphology } from '../../morphology';
import { arcContains, signedAngle, type Arc, type VisionStats } from '../../vision';
import { INK } from '../../art/palette';
import { groupSvg, shapeSvg } from '../../art/render';
import { headTop, type HeadTopPart } from '../../art/parts/headTop';
import { coveringOf, morphologyOf, paintOf } from '../../world/avatar';
import { h } from '../dom';
import type { HandleSpec } from './builder';
import { SIDE_DESIGN } from './sideView';
import { azimuthOf, polar, s, wedgePath } from './svg';

/** Radius of the field-of-view rim, of the head (eyes sit on its outline), of the midline eye's spot on the forehead. */
export const RIM = 96;
export const HEAD_R = 30;
const MID_EYE_R = 17;
/** Room round the rim: side (the width knob's touch target) and top / bottom (two lines of label). */
const PAD_X = 20;
const PAD_TOP = 40;
const PAD_BOTTOM = 30;
const HINT_KEY = 'evo.eyeViewHint';
/**
 * The head's ink outline and rims in view units: the side view's (art
 * library `OUTLINE_PX`, `RIM`) at this panel's slightly larger scale, so the
 * two drawings read as one creature side by side.
 */
const ART = { outline: 1.7, rim: { light: 2.2, shade: 5 } } as const;

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
const round = (v: number): number => Math.round(v);
const f = (n: number): string => (Math.round(n * 100) / 100).toString();
const midOf = (a: Arc): number => a.start + a.width / 2;

export interface EyeView {
  /** The drawing (id `<prefix>-top`). */
  readonly svg: SVGSVGElement;
  /** Where the builder puts the handles (above the drawing). */
  readonly handles: SVGGElement;
  /** The colour key under the drawing. */
  readonly legend: HTMLElement;
  /** Redraw for `cur`; returns the handles to show, by key. */
  render(cur: Morphology, vision: VisionStats): Map<string, HandleSpec>;
  /** Hide the first-use hint for good (the player has dragged or used the keys). */
  dismissHint(): void;
}

function hintSeen(): boolean {
  try {
    return localStorage.getItem(HINT_KEY) === '1';
  } catch {
    return false;
  }
}

/** Where an eye is drawn: on the head's outline, the midline eye on the forehead. */
function eyeSpot(e: EyePlacement): [number, number] {
  return e.side === 'mid' ? [0, -MID_EYE_R] : polar(e.azimuth, HEAD_R);
}

/** An eye's view: from the eye to the rim, `width` degrees centred on its azimuth. */
function fieldPath(ex: number, ey: number, az: number, width: number): string {
  if (width >= 359.999) return wedgePath(0, 360, RIM);
  const [x0, y0] = polar(az - width / 2, RIM);
  const [x1, y1] = polar(az + width / 2, RIM);
  return `M${f(ex)},${f(ey)} L${f(x0)},${f(y0)} A${RIM},${RIM} 0 ${width > 180 ? 1 : 0} 1 ${f(x1)},${f(y1)} Z`;
}

/** A ring sector between radii `r0` and `r1`. */
function bandPath(start: number, width: number, r0: number, r1: number): string {
  if (width >= 359.999) return `${wedgePath(0, 360, r1)} ${wedgePath(0, 360, r0)}`;
  const [ax, ay] = polar(start, r1);
  const [bx, by] = polar(start + width, r1);
  const [cx, cy] = polar(start + width, r0);
  const [dx, dy] = polar(start, r0);
  const big = width > 180 ? 1 : 0;
  return `M${f(ax)},${f(ay)} A${r1},${r1} 0 ${big} 1 ${f(bx)},${f(by)} L${f(cx)},${f(cy)} A${r0},${r0} 0 ${big} 0 ${f(dx)},${f(dy)} Z`;
}

/** An arc along radius `r` from `a0` to `a1` (degrees, clockwise when a1 > a0). */
function arcPath(a0: number, a1: number, r: number): string {
  const [x0, y0] = polar(a0, r);
  const [x1, y1] = polar(a1, r);
  return `M${f(x0)},${f(y0)} A${r},${r} 0 ${Math.abs(a1 - a0) > 180 ? 1 : 0} ${a1 > a0 ? 1 : 0} ${f(x1)},${f(y1)}`;
}

/** A small curved arrow along radius `r`, from `a0` towards `a1`, with its head at `a1`. */
function curvedArrow(a0: number, a1: number, r: number, cls: string): SVGElement {
  const [tx, ty] = polar(a1, r);
  // The head points along the tangent, in the direction of travel.
  const dir = a1 > a0 ? 1 : -1;
  const t = ((a1 + 90 * dir) * Math.PI) / 180;
  const ux = Math.sin(t);
  const uy = -Math.cos(t);
  const nx = -uy;
  const ny = ux;
  const hl = 3.6;
  const hw = 2.6;
  const head = `M${f(tx + ux * hl)},${f(ty + uy * hl)} L${f(tx + nx * hw)},${f(ty + ny * hw)} L${f(tx - nx * hw)},${f(ty - ny * hw)} Z`;
  return s('g', { class: cls }, s('path', { d: arcPath(a0, a1, r), class: 'ev-arrow-line' }), s('path', { d: head, class: 'ev-arrow-head' }));
}

/** The eye-slash "blind" icon, centred on (x, y), about `size` wide. */
function blindIcon(x: number, y: number, size: number): SVGElement {
  const k = size / 20;
  return s('g', { class: 'ev-blind-icon', transform: `translate(${f(x)} ${f(y)}) scale(${f(k)})`, 'aria-hidden': 'true' },
    s('circle', { r: 14, class: 'ev-blind-icon-bg' }),
    s('path', { d: 'M-10,0 Q0,-9 10,0 Q0,9 -10,0 Z', class: 'ev-blind-icon-eye' }),
    s('circle', { r: 3.2, class: 'ev-blind-icon-pupil' }),
    s('line', { x1: -8, y1: 7, x2: 8, y2: -7, class: 'ev-blind-icon-slash' }));
}

/** The head, snout, ears and body from above, in the art library's style (`art/parts/headTop.ts`), painted like the side view's creature. */
function headArt(cur: Morphology, eyes: EyePlacement[], spots: [number, number][], snoutTip: number): HeadTopPart {
  const model = morphologyOf(SIDE_DESIGN, cur);
  return headTop({
    r: HEAD_R, snoutTip, carn: model.carnivory, cold: !model.warm,
    ears: { count: cur.ear_count, shape: cur.ear_shape, size: cur.ear_size },
    eyes: eyes.map((e, i) => ({ x: spots[i][0], y: spots[i][1], az: e.side === 'mid' ? 0 : e.azimuth, r: 5 + 3.8 * cur.eye_size })),
    furry: coveringOf(model) === 'fur', lod: 'portrait',
    horns: { size: model.horns, form: cur.horn_form },
    pattern: cur.pattern,
  }, paintOf(model));
}

/** Library markup (a group or shapes) as SVG nodes. */
function artNode(markup: string, attrs: Record<string, string | number>): SVGGElement {
  const g = s('g', { ...attrs, 'aria-hidden': 'true' });
  g.innerHTML = markup;
  return g;
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

/** The legend: the same keys as the map's vision cones. */
function legendEl(): HTMLElement {
  const key = (cls: string, text: string) => h('span', { class: 'vision-key' }, h('i', { class: `vision-swatch ${cls}`, 'aria-hidden': 'true' }), text);
  return h('p', { class: 'vision-legend ev-legend small' },
    key('seen', 'seen by an eye'), key('depth', 'depth vision (two eyes)'), key('blind', 'blind spot'));
}

export function createEyeView(opts: { idPrefix: string; uid: string }): EyeView {
  const { idPrefix: p, uid } = opts;
  const vb = `${-RIM - PAD_X} ${-RIM - PAD_TOP} ${2 * (RIM + PAD_X)} ${2 * RIM + PAD_TOP + PAD_BOTTOM}`;
  const desc = s('desc', { id: `${p}-top-desc` });
  const svg = s('svg', {
    class: 'bb-svg bb-top ev', viewBox: vb, id: `${p}-top`, role: 'group',
    'aria-label': 'Eyes: the head seen from above, snout at the top. Each eye’s view, the depth vision in front and the blind spots.',
    'aria-describedby': `${p}-top-desc`,
  }, desc);
  svg.appendChild(s('defs', {},
    // Blind: hatching, so it reads without colour too.
    s('pattern', { id: `${uid}-hatch`, patternUnits: 'userSpaceOnUse', width: 6, height: 6, patternTransform: 'rotate(45)' },
      s('rect', { width: 6, height: 6, class: 'ev-hatch-bg' }),
      s('line', { x1: 0, y1: 0, x2: 0, y2: 6, class: 'ev-hatch-line' })),
    // Depth: dots.
    s('pattern', { id: `${uid}-dots`, patternUnits: 'userSpaceOnUse', width: 5, height: 5 },
      s('circle', { cx: 2.5, cy: 2.5, r: 1.05, class: 'ev-dots-dot' }))));
  const draw = s('g', { class: 'ev-draw' });
  const hintLayer = s('g', { class: 'ev-hint-layer', 'aria-hidden': 'true' });
  const handles = s('g', { class: 'bb-handles' });
  svg.append(draw, hintLayer, handles);

  let hintOn = !hintSeen();
  let hintAz: number | null = null;

  function dismissHint(): void {
    if (!hintOn) return;
    hintOn = false;
    hintLayer.replaceChildren();
    try {
      localStorage.setItem(HINT_KEY, '1');
    } catch {
      /* private mode: the hint just comes back next time */
    }
  }
  // Any press on the drawing, or a key on a handle, means the player has found the controls.
  svg.addEventListener('pointerdown', dismissHint, true);
  svg.addEventListener('keydown', (e) => {
    if (e.key.startsWith('Arrow') || e.key === 'Home' || e.key === 'End') dismissHint();
  }, true);

  /** The animated first-use hint: a ghost finger sliding the right eye along the head. */
  function drawHint(az: number): void {
    if (hintAz === az) return;
    hintAz = az;
    hintLayer.replaceChildren();
    const to = clamp(az + 35, 0, EYE_PLACEMENT_MAX);
    const from = to - az < 20 ? az - 35 : az;
    const r = HEAD_R + 1;
    const path = arcPath(from, to, r);
    const [x0, y0] = polar(from, r);
    const [x1, y1] = polar(to, r);
    const motion = (attr: string, a: string, b: string) => s('animate', { attributeName: attr, values: `${a};${b};${b};${a};${a}`, keyTimes: '0;0.4;0.55;0.95;1', dur: '2.4s', repeatCount: 'indefinite' });
    const finger = s('circle', { cx: f(x0), cy: f(y0), r: 7, class: 'ev-hint-finger' }, motion('cx', f(x0), f(x1)), motion('cy', f(y0), f(y1)));
    const [lx, ly] = polar(az + 10, HEAD_R + 36);
    hintLayer.append(s('g', { class: 'ev-hint' },
      s('path', { d: path, class: 'ev-hint-track' }),
      finger,
      s('rect', { x: f(lx - 30), y: f(ly - 8), width: 60, height: 16, rx: 8, class: 'ev-hint-pill' }),
      s('text', { x: f(lx), y: f(ly), class: 'ev-hint-text', 'text-anchor': 'middle', 'dominant-baseline': 'central' }, 'Drag an eye')));
  }

  function render(cur: Morphology, vision: VisionStats): Map<string, HandleSpec> {
    draw.replaceChildren();
    const out = new Map<string, HandleSpec>();
    const add = <T extends SVGElement>(el: T): T => draw.appendChild(el);
    const width = eyeFieldWidth(cur.eye_size);
    const eyes = eyePlacements(cur);
    const pairs = Math.floor(cur.eye_count / 2);
    const spots = eyes.map(eyeSpot);
    const fields = eyes.map((e, i) => fieldPath(spots[i][0], spots[i][1], e.azimuth, width));
    const snout = 6 + 30 * cur.snout_length;
    const snoutTip = HEAD_R + snout;

    // Clip paths: each eye's view; the unseen (everything outside every view).
    const defs = add(s('defs', {}));
    fields.forEach((d, i) => defs.appendChild(s('clipPath', { id: `${uid}-f${i}` }, s('path', { d }))));
    defs.appendChild(s('mask', { id: `${uid}-unseen`, maskUnits: 'userSpaceOnUse', x: -RIM - 2, y: -RIM - 2, width: 2 * RIM + 4, height: 2 * RIM + 4 },
      s('rect', { x: -RIM - 2, y: -RIM - 2, width: 2 * RIM + 4, height: 2 * RIM + 4, fill: '#fff' }),
      ...fields.map((d) => s('path', { d, fill: '#000' }))));

    // The ground round the head, and the body trailing behind (which way is front), under the overlays.
    const art = headArt(cur, eyes, spots, snoutTip);
    add(s('circle', { r: RIM, class: 'ev-ground' }));
    add(artNode(groupSvg(art.body, 'body', { id: uid, outline: ART.outline, rim: ART.rim, animate: false }, INK), { class: 'ev-body' }));

    // Blind spots: greyed and hatched, outside every eye's view.
    const blindG = add(s('g', { class: 'ev-blind', mask: `url(#${uid}-unseen)` }));
    for (const a of vision.blind) {
      const kind = a === vision.rearBlind ? 'rear' : a === vision.frontBlind ? 'front' : 'side';
      blindG.appendChild(s('path', { d: wedgePath(a.start, a.width, RIM), class: `ev-blind-fill ev-blind-${kind}`, fill: `url(#${uid}-hatch)` }));
    }
    // Each eye's view, its own wedge from the eye.
    fields.forEach((d, i) => add(s('path', { d, class: 'ev-field', 'data-eye': i })));
    // Depth vision: the model's overlap arcs, inside where two eyes' wedges cross.
    if (vision.overlap.length > 0) {
      const depthG = add(s('g', { class: 'ev-depth' }));
      const wedges = () => vision.overlap.map((a) => s('path', { d: wedgePath(a.start, a.width, RIM) }));
      for (let i = 0; i < fields.length; i++) {
        for (let j = i + 1; j < fields.length; j++) {
          depthG.appendChild(s('g', { 'clip-path': `url(#${uid}-f${i})` }, s('g', { 'clip-path': `url(#${uid}-f${j})` },
            s('g', { class: 'ev-depth-fill' }, ...wedges()), s('g', { class: 'ev-depth-dots', fill: `url(#${uid}-dots)` }, ...wedges()))));
        }
      }
      for (const a of vision.overlap) add(s('path', { d: arcPath(a.start, a.start + a.width, RIM - 1.5), class: 'ev-depth-rim' }));
    }
    // What a long snout hides of the depth zone, straight ahead: its shadow.
    if (vision.occludedDeg > 0 && vision.overlap.some((o) => arcContains(o, 0))) add(s('path', { d: bandPath(-vision.occludedDeg / 2, vision.occludedDeg, snoutTip, RIM), class: 'ev-occluded' }));
    // Each eye's outline on top, so every wedge can be traced back to its eye.
    fields.forEach((d, i) => add(s('path', { d, class: 'ev-field-edge', 'data-eye': i })));
    add(s('circle', { r: RIM, class: 'ev-rim' }));

    // The head from above, on top: skull, ears, snout forward with the nose at the tip.
    add(artNode(groupSvg(art.head, 'head', { id: uid, outline: ART.outline, rim: ART.rim, animate: false }, INK), { class: 'ev-head' }));

    // Labels: plain words where the zones meet the rim.
    const label = (x: number, y: number, lines: string[], cls: string, anchor = 'middle') => {
      add(s('text', { x: f(x), y: f(y), class: `ev-label ${cls}`, 'text-anchor': anchor },
        ...lines.map((line, i) => s('tspan', { x: f(x), dy: i === 0 ? null : '1.15em', class: ['ev-label-name', 'ev-label-value', 'ev-label-note'][i] }, line))));
    };
    const top = -RIM - 18;
    const bottom = RIM + 12;
    const depthWords = `${round(vision.overlapDeg)}° ahead`;
    // Depth vision is all within 60° of ahead, so it's named at the top unless a blind spot sits there.
    const depthOnTop = !vision.frontBlind && vision.overlap.length > 0;
    if (vision.frontBlind) label(0, top, ['Blind spot', `${round(vision.frontBlindDeg)}° ahead`], 'ev-label-blind');
    else if (depthOnTop && vision.occludedDeg > 0) label(0, top - 11, ['Depth vision', depthWords, `snout hides ${round(vision.occludedDeg)}°`], 'ev-label-depth');
    else if (depthOnTop) label(0, top, ['Depth vision', depthWords], 'ev-label-depth');
    else label(0, top, ['No depth vision', 'eyes don’t overlap'], 'ev-label-none');
    if (vision.rearBlind) label(0, bottom, ['Blind spot', `${round(vision.rearBlindDeg)}° behind`], 'ev-label-blind');
    else if (vision.blindDeg <= 0) label(0, bottom, ['No blind spot', 'sees all the way round'], 'ev-label-good');
    label(-RIM - PAD_X + 2, top, ['Sees', `${round(vision.coveredDeg)}° around`], 'ev-label-seen', 'start');
    // Smaller zones (side blind spots, depth off to one side): a word inside, when there's room.
    for (const a of vision.blind) {
      if (a === vision.rearBlind || a === vision.frontBlind || a.width < 26) continue;
      const [x, y] = polar(midOf(a), RIM - 22);
      add(s('text', { x: f(x), y: f(y), class: 'ev-label ev-label-blind ev-label-small', 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, `blind ${round(a.width)}°`));
    }
    if (!depthOnTop) {
      for (const a of vision.overlap) {
        if (a.width < 26) continue;
        const [x, y] = polar(midOf(a), RIM - 22);
        add(s('text', { x: f(x), y: f(y), class: 'ev-label ev-label-depth ev-label-small', 'text-anchor': 'middle', 'dominant-baseline': 'middle' }, `depth ${round(a.width)}°`));
      }
    }
    // The blind icon in the middle of each wide blind spot.
    for (const a of vision.blind) {
      if (a.width < 30) continue;
      const [x, y] = a === vision.frontBlind ? polar(0, (RIM + snoutTip) / 2 + 4) : polar(midOf(a), RIM * 0.62 + (a === vision.rearBlind ? 8 : 0));
      add(blindIcon(x, y, 18));
    }

    // The field-width knob sits at the rim on the outer edge of the reference eye's view: that edge is its track.
    const refIdx = Math.max(0, eyes.findIndex((e) => e.side === 'right'));
    const ref = eyes[refIdx];
    const [rx0, ry0] = spots[refIdx];
    const edge = ref.azimuth + width / 2;
    const [kx, ky] = polar(edge, RIM);
    add(s('line', { x1: f(rx0), y1: f(ry0), x2: f(kx), y2: f(ky), class: 'ev-edge-track' }));
    add(curvedArrow(edge - 7, edge - 17, RIM, 'ev-arrow ev-arrow-knob'));
    add(curvedArrow(edge + 7, edge + 17, RIM, 'ev-arrow ev-arrow-knob'));

    // The eyes themselves, on the head; slide arrows round the first pair.
    eyes.forEach((e) => {
      if (e.pair === 0) {
        const sign = e.side === 'right' ? 1 : -1;
        const a = e.azimuth;
        // Along the head's outline, either side of the eye: it slides round the head.
        const r = HEAD_R + 1;
        const gap = 22 + 8 * cur.eye_size;
        if (Math.abs(a) > gap + 4) add(curvedArrow(a - sign * gap, a - sign * (gap + 18), r, 'ev-arrow ev-arrow-eye'));
        if (Math.abs(a) < EYE_PLACEMENT_MAX - 10) add(curvedArrow(a + sign * gap, a + sign * (gap + 18), r, 'ev-arrow ev-arrow-eye'));
      }
    });
    art.eyes.forEach((shapes, i) => add(artNode(shapes.map(shapeSvg).join(''), { class: 'ev-eye', 'data-eye': i })));

    // Handles.
    const eyeR = 7 + 4 * cur.eye_size;
    const result = `sees ${round(vision.coveredDeg)}° around, ${round(vision.overlapDeg)}° depth vision, ${round(vision.frontBlindDeg)}° blind ahead, ${round(vision.rearBlindDeg)}° blind behind`;
    eyes.forEach((e, i) => {
      const [x, y] = spots[i];
      if (e.pair === null) {
        out.set(`eye-${i}`, {
          feature: 'eyes', x, y, r: eyeR,
          label: 'Middle eye (fixed straight ahead): arrows resize all eyes',
          now: cur.eye_size, min: 0, max: 1, text: `middle eye, size ${Math.round(cur.eye_size * 100)}%: ${result}`,
          drag: null, key: unitKeys('eye_size', cur.eye_size),
        });
        return;
      }
      const offset = PAIR_STEP * e.pair;
      out.set(`eye-${i}`, {
        feature: 'eyes', x, y, r: eyeR,
        label: `${e.side === 'right' ? 'Right' : 'Left'} eye${pairs > 1 ? ` (pair ${e.pair + 1})` : ''}: drag it round the head (its partner mirrors it); arrows up / down move it forward / back`,
        now: round(cur.eye_azimuth), min: 0, max: 180,
        text: `eyes ${round(Math.abs(e.azimuth))}° to the side: ${result}`,
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
    out.set('eye-size', {
      feature: 'eyes', x: kx, y: ky, r: 7,
      label: 'Width of view: drag the edge of the view along the rim to widen or narrow every eye’s view (bigger eyes)',
      now: cur.eye_size, min: 0, max: 1, text: `eye size ${Math.round(cur.eye_size * 100)}%, each eye sees ${round(width)}°`,
      drag: (px, py) => {
        const w = 2 * Math.abs(signedAngle(azimuthOf(px, py) - ref.azimuth));
        return { eye_size: Math.round(clamp((w - 140) / 60, 0, 1) * 100) / 100 };
      },
      key: unitKeys('eye_size', cur.eye_size),
    });

    // First use: show how to drag (the right eye of the first pair), until the player does.
    const hintEye = eyes.find((e) => e.pair === 0 && e.side === 'right');
    if (hintOn && hintEye) drawHint(hintEye.azimuth);
    else if (hintAz !== null) {
      hintLayer.replaceChildren();
      hintAz = null;
    }

    desc.textContent = `${eyes.length} ${eyes.length === 1 ? 'eye' : 'eyes'}, each seeing ${round(width)}°: ${result}.`;
    return out;
  }

  return { svg, handles, legend: legendEl(), render, dismissHint };
}
