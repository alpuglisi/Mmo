// The body builder's side view ("Body" panel): the founder avatar drawn from
// the creature art library, at a fixed scale with its torso pinned in place,
// plus the tracks its drag handles ride (each feature's tip at 0 and at 1).
// Pure (no DOM), so the handle geometry is unit-tested.

import type { Morphology } from '../../morphology';
import type { DesignSpec } from '../../protocol';
import type { Point } from '../../art/geom';
import { avatarDrawing, avatarMarkup, morphologyOf, type AvatarAnchors } from '../../world/avatar';

/**
 * The creature the builder shows: a plain, small, warm-blooded omnivore (its
 * colours and coverings are the design editor's business; the builder is
 * about the body plan).
 */
export const SIDE_DESIGN: DesignSpec = {
  name: 'Body', thermo: 'warm', birth: 'live', litter_size: 2, founders: 10,
  traits: { body_size: 0.35, carnivory: 0.4 },
};

/**
 * Creature units → side-view units: a fixed scale and offset, with the torso
 * centre pinned at `BODY_Y` (longer legs lower the ground instead of raising
 * the body, so a leg drag moves the foot under the finger). Sized so the
 * largest plan (longest legs, ears, tail and snout) fits the panel's viewBox
 * (-14 -18 318 212).
 */
export const SIDE = { k: 0.92, x: 168, bodyY: 96 } as const;

/** How far below the fang tip its knob sits (side-view units). */
export const FANG_KNOB_DROP = 16;

export interface HandleTrack {
  from: Point;
  to: Point;
}

function drawingFor(m: Morphology) {
  return avatarDrawing(morphologyOf(SIDE_DESIGN, m));
}

/** A creature point in side-view units, for a drawing whose torso centre is at `cy`. */
export function toSide(p: Point, a: AvatarAnchors): Point {
  return [SIDE.x + SIDE.k * p[0], SIDE.bodyY + SIDE.k * (p[1] - a.body.cy)];
}

export interface SideView {
  /** SVG transform for the creature group. */
  transform: string;
  /** The ground line's height in side-view units. */
  groundY: number;
  /** The creature's markup (clip-path ids prefixed with `id`). */
  markup: (id: string) => string;
}

export function sideView(m: Morphology): SideView {
  const d = drawingFor(m);
  const ty = SIDE.bodyY - SIDE.k * d.anchors.body.cy;
  return {
    transform: `translate(${SIDE.x} ${Math.round(ty * 100) / 100}) scale(${SIDE.k})`,
    groundY: ty,
    markup: (id) => avatarMarkup(d, SIDE.k, { animate: false, id }),
  };
}

/** Where each length / size handle runs for the current body plan (null: that feature is absent). */
export function sideTracks(m: Morphology): { snout: HandleTrack; fangs: HandleTrack; ears: HandleTrack | null; legs: HandleTrack | null; tail: HandleTrack | null; horns: HandleTrack } {
  const tip = (patch: Partial<Morphology>, pick: (a: AvatarAnchors) => Point | null): Point | null => {
    const a = drawingFor({ ...m, ...patch }).anchors;
    const p = pick(a);
    return p ? toSide(p, a) : null;
  };
  const trackOf = (field: keyof Morphology, pick: (a: AvatarAnchors) => Point | null, extra: Partial<Morphology> = {}): HandleTrack | null => {
    const from = tip({ ...extra, [field]: 0 }, pick);
    const to = tip({ ...extra, [field]: 1 }, pick);
    return from && to ? { from, to } : null;
  };
  const fangs = trackOf('fang_size', (a) => a.fangTip)!;
  // The fang knob rides the fang tip a little below it, clear of the snout's knob.
  const below = (p: Point): Point => [p[0], p[1] + FANG_KNOB_DROP];
  return {
    snout: trackOf('snout_length', (a) => a.snoutTip)!,
    fangs: { from: below(fangs.from), to: below(fangs.to) },
    ears: m.ear_count > 0 ? trackOf('ear_size', (a) => a.earTip) : null,
    legs: m.leg_count > 0 ? trackOf('leg_length', (a) => a.foot) : null,
    tail: m.tail_present ? trackOf('tail_length', (a) => a.tailTip, { tail_present: true }) : null,
    horns: trackOf('horn_size', (a) => a.hornTip)!,
  };
}
