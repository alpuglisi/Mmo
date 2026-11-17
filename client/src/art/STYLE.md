# Creature art library: style guide

Every creature in the game is drawn from the parts in this folder in one style,
so a body plan made of any mix of parts still looks like **one animal**, not a
pile of shapes. All paths are original and authored here as code: no image
generation or outside artwork. Raster images, where needed, are rendered from
these SVGs.

## Art direction: "soft field guide"

A friendly naturalist's sketchbook, cleaned up for a game UI.

| Rule | Value |
| --- | --- |
| View | Strict side view, facing **right** (+x). Far-side limbs, far ear and far wing are drawn behind, in the `far` tone ramp. |
| Light | Always from the **top left**. |
| Shading | Flat. Each surface gets exactly **one highlight** step (top-left rim) and **one shadow** step (bottom-right rim), nothing else: no gradients, no blur. |
| Outline | One even ink line round the **merged** silhouette (`OUTLINE_PX` = 1.9 px on screen at any creature size). No seams where parts overlap; inner lines (thigh contour, mouth, feather rows) are thinner and in the shade tone, not ink. |
| Shapes | Rounded and chunky; no straight sticks, no sharp corners except claws, teeth, horn tips and the tips of pointed ears. Heads and eyes are generous (small creatures bigger still). |
| Palette | Limited, from tokens (below). Hue-shifted tones: shadows cool toward violet, highlights warm toward yellow; never plain darken / lighten. |
| Detail | Textures (hair, scales, feathers, creases) are sparse strokes that suggest the covering; the silhouette edge does most of the work (soft fur clumps, rounded feather scallops, a clean edge for skin and scales, raised plates for armor). |

## Palette (`palette.ts`)

`creaturePalette()` builds one paint box per creature:

- **body** `Tones` = `{ base, shade, light }`; **far** = the same ramp pushed into shadow.
- **belly** (countershading: belly, chest, muzzle, throat), **pattern** (saddle, camouflage blotches), **accent** (display crest, spots, warning bands).
- Fixed materials shared by every creature: `INK` `#221a16` (warm near-black), `BONE` (claws, teeth, horns, club knob), `HOOF`, `NOSE`, `INNER` (inner ear), `membrane` (wing / fin / web, tinted from the body), `iris` (`IRIS_WARM` dark amber, `IRIS_COLD` gold with a slit pupil), `GLINT` (eye highlight).

The body hue comes from the world sprite's palette for the colouring class
(`world/creature.ts` `palette`), so the portrait matches the map. The
coloration gene (`vividness`) then moves saturation continuously from drab to
vivid. Warning colouring: yellow body with ink-black bands; camouflage:
mottled blotches over a saddle; bright: accent crest and spots.

## Rendering (`render.ts`)

A drawing is a list of **groups**, back to front. A group is a set of
silhouette **pieces** (closed outlines) plus clipped **details** and unclipped
**overlays**. Per group:

1. **Outline**: every piece filled and stroked in ink at twice the outline width.
2. **Base**: pieces filled with the base tone, clipped to their union; then the details (markings, materials, textures), clipped.
3. **Glazes**: the shade tone over the bottom-right rim and the highlight tone over the top-left rim (masks: the silhouette minus itself shifted), drawn *after* the details, so markings and materials are lit by the same light.
4. **Overlays**: eyes, teeth, fangs, tusks, horns, claws, armor plates, each with its own thin ink line.

Group order for a creature: ground shadow, far wing, far legs, tail (sways),
far ears, **main** (torso + neck + head + near ears + near legs + feet + ruff /
mane / crest / flaps, all one silhouette), near wing. Ids for clips and masks
are prefixed (`SvgOptions.id`): two drawings on one page need different
prefixes.

## Parts and anchors (`parts/`)

Units: creature units, ground at `y = 0`, `y` down, facing `+x`. The largest
body is about 300 units long. Every part is a pure function returning points.

| Part | File | Inputs | Anchors it gives |
| --- | --- | --- | --- |
| Torso | `body.ts` | half length / height, centre height, chest depth, belly sag (fat), withers hump (muscle), boxiness (stocky) | `neckTop`, `neckLow`, `tailRoot`, `withers`, `rump`, `edge(t)`, `inside()` |
| Head, neck | `head.ts` | cranium centre / radius, snout length (continuous), carnivory (herbivore deep round muzzle → carnivore square muzzle, strong jaw and brow), cold-blooded (flat wedge) | `snoutTip`, `nose`, `mouth[3]`, `fangRoot`, `tuskRoot`, `hornBase`, `cheek`, `throat`, `nape`, `muzzle` |
| Eyes | `head.ts` `eyeSpot` / `eyeShapes` | azimuth (forward / default / side, contract v1 pairs), size, count 1–6 | glint position (tests find eyes by the `glint` tag) |
| Head from above | `headTop.ts` | cranium radius, snout tip distance, carnivory, cold, ear count / shape / size, eye spots, azimuths and radii, furry | `snoutTip`, `nose`, `earTips`; groups `body` (drawn under the eye panel's overlays) and `head`, eye shapes per eye (the panel draws the head merged with the body's outline and masked back to the head, so no ink seam crosses the neck; a furred body gets chunky locks and hair strokes). Ears reuse `ears.ts` lying out from the skull, foreshortened. Used by the builder's eye panel (`ui/builder/eyeView.ts`) |
| Ears | `ears.ts` | shape round / pointed / long / fan, size, count 0 / 2 / 4 | `tip = base + axis × length` (linear in size) |
| Legs, feet | `legs.ts` | hip / shoulder, length, width, front / hind, sprawl (cold), muscle; foot plain / hoof / claw / webbed, width, claw length, dig / climb | `ankle` (on the ground), thigh `contour` |
| Tail | `tail.ts` | root, length, shape thin / bushy / fin / club / prehensile, root width, warm (carried) / cold (trailing), furry (tuft, plume), feathered (fan) | `tip` (linear in length), centreline and widths (for rings) |
| Wings | `wings.ts` | root, span, feathered (folded along the flank) or membrane (raised, fingered), far | — |
| Coverings, markings | `cover.ts` | the torso; amount; palette | fur / feather silhouette rims, hair, scales + belly scutes, feather rows, skin creases, armor plates; countershading, saddle, camouflage, spots, warning bands, dorsal frill |

How parts attach (see `world/avatar.ts` `avatarDrawing`): the head sits ahead of
and above the torso's front; the neck joins `neckTop` / `neckLow` to the head's
`nape` / `throat`; legs hang from the torso at `cy + 0.3 ry`, spread from hip to
shoulder (a biped's one pair sits under the middle); the near legs' muscle
masses overlap the torso so the merged outline grows them out of the body; the
tail root sits inside the rump (its group is behind, so the join is hidden);
ears stand on the cranium, the far ear of each pair peeking out behind.

Anchors that a drag handle rides (`AvatarAnchors`: snout tip, fang tip, ear
tip, front foot, tail tip) move **linearly** with their size, so the builder's
knob stays on the feature; `avatar.test.ts` and `ui/builder/sideView.test.ts`
pin this.

## Covering = caption

`coveringOf(model)` decides the covering the avatar draws, and `captionOf`
puts the same word in the preview caption: the server's class
(`look.cover`), except that a furred body with wings or gliding flaps grows
**feathers**. Never draw one covering while the caption names another.

## Sprites (32–64 px)

`avatarDrawing(model, { lod: 'sprite' })` keeps the silhouette, the rim
shading, the eyes (no lids) and the bold markings, and drops fine textures
(hair, scales, feather rows, creases, whiskers, toe lines). `avatarSpriteSvg`
renders it into a square of any size with a proportionally heavier outline.

The world map's sprites are drawn this way (`world/spriteArt.ts`): each
distinct appearance key (the quantised `look`, `world/creature.ts`
`creatureKey`) becomes an `AvatarModel` (`spriteModel`: size buckets to gene
values, diet to carnivory, locomotion to wings or flaps, coverings,
colourings), drawn with `lod: 'sprite'` at about 1/40 px per unit per body
radius, the ground line on the owner ring, with a 1.15 px outline whatever
the size (bolder than the portrait relative to the creature) and rims of
about 0.13 and 0.3 body radii. Swimmers add a dorsal fin. `world/sprites.ts`
rasterises it once per key with `canvas.ts`, the canvas twin of `render.ts`
(the same Catmull-Rom curves and the same four layers, far-side groups
glazed in their `far` ramp like every other group, as in the builder's side
view), facing right, and a marker flips it for headings to the left. The far
legs, ears and wing sit `FAR_SHIFT_PX` (about one outline width) forward and
up, so they peek out past the near parts' ink line and read as the darker far
side instead of hiding under it.
Zoomed out, markers use the class drawing (morphology at its defaults), so
few textures serve a wide view. `e2e/mobile.mjs` saves the map galleries
(`desktop-sprite-gallery.png`, `desktop-sprite-scales.png` at the real
marker scales, `desktop-morphology-gallery.png`): check them with the
library gallery when a part changes.

## Checking your work

Look at the output, not just the tests: `/?builder=gallery` (the dev harness)
draws every creature in `gallery.ts` with its caption and its 64 / 32 px
sprites, and `e2e/builder.mjs` saves it as `e2e/out/gallery-*.png` on a desktop
and two phones. When you change a part, check all of them: one part must not
look out of family next to the others.
