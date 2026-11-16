// The creature art library: hand-authored, parametric vector parts in one
// house style (STYLE.md). The founder avatar (`world/avatar.ts`) assembles a
// creature from these; the builder's side view and the gallery reuse that.

export * from './geom';
export * from './palette';
export * from './render';
export { body, type BodyParams, type BodyPart } from './parts/body';
export { head, neck, eyeSpot, eyeShapes, fang, tusk, horn, type HeadParams, type HeadPart, type EyeParams } from './parts/head';
export { headTop, type HeadTopEye, type HeadTopParams, type HeadTopPart } from './parts/headTop';
export { ear, earAxis, earLength, type EarPart } from './parts/ears';
export { leg, foot, type FootStyle, type LegParams, type LegPart, type FootPart } from './parts/legs';
export { tail, tailTip, type TailParams, type TailPart } from './parts/tail';
export { wing, flaps, type WingParams, type WingPart } from './parts/wings';
export * from './parts/cover';
export { GALLERY, type GalleryCreature } from './gallery';
