# World art direction

One look for everything drawn on the map: **soft painted ground under
crisp, flat-shaded creatures**, a quiet frame of overlays, and one mark
language at the creatures' feet. The creature art (`client/src/art/`,
`creature.ts`, `avatar.ts`) uses soft flat shading with one highlight and one
shadow tone and a uniform dark outline; the map supports it by staying
lower in contrast and saturation than the creatures, so they read first.

Tokens live in `palette.ts`; change them there, not inline.

## Light

- **One light, from the top-left.** Hill shading, forest crowns and rock
  ridges are embossed toward the top-left; shadows fall down-right (contact
  shadows under creatures are offset that way, banks shade the water below
  and right of them).
- **Time of day** tints the whole terrain layer (`lightAt(hour)`): neutral
  day, rose dawn (around 05:45), amber dusk (19:00), cool moonlit blue at
  night (`LIGHT.night`). Eased over ~1.5 s. Markers and overlays are *not*
  tinted, so they stay legible at night.

## Ground palette (spring, full vegetation)

| token | rgb | use |
|---|---|---|
| `GROUND.grass` | 126 166 88 | grassland |
| `GROUND.forest` | 58 112 70 | forest floor under crowns |
| `GROUND.marsh` | 92 124 88 | marsh |
| `GROUND.desert` | 222 200 146 | desert, with wind ripples |
| `GROUND.tundra` | 166 172 150 | tundra |
| `GROUND.mountain` / `peak` | 130 119 104 / 172 164 152 | rock, high rock |
| `GROUND.sand` | 222 205 158 | beaches (what a lake shore blends through) |
| `GROUND.bank` | 176 160 120 | river banks, dry river beds |
| `GROUND.mud` | 150 130 95 | dried lake and marsh floors |
| `GROUND.bare` | 150 136 100 | grazed-out ground |
| `GROUND.straw` | 205 180 112 | drought |

**Seasons** recolour plant cover only (`SEASON_GREEN`, blended into grass
60 %, forest 35 %, marsh and tundra 25 %): fresh green spring, golden-green
summer, rust autumn, dull olive winter. Snow (`SNOW`, shaded `SNOW_SHADE`)
and ice (`ICE`) lie on top as drifts with noisy edges.

## Water

`WATER.shore → shallow → deep → abyss` by depth; `river` is lighter,
`flood` muddy grey-blue. Shores are rounded (blurred mask), get a thin
`WATER.foam` line and a damp darker band on the land side. Water colour
changes are soft gradients, never tile blotches.

## Texture

Procedural, from two 256² tileable value-noise tables (no image assets):
fine grain on grass and sand (±16 %), embossed crowns about half a tile
across in forests, ridged relief on rock, ripples on desert and water,
cracks on ice. Detail fades out at coarse resolutions instead of aliasing.
Biome *colours* blend across tile edges; biome *textures* switch at the
(noise-warped) tile edge.

## Rendering and budget

`terrainArt.ts` paints (pure, unit-tested); `terrainLayer.ts` shows: a base
texture of the whole map at ≤ 4 texels per tile, and 16×16-tile chunks at 8
or 16 texels per tile over the screen, painted a few rows per frame within
`TERRAIN_BUDGET_MS` (3 ms), centre first, in a 24 MB LRU cache. A texel is
a function of world position only, so chunks never seam. A new map shows
a one-texel-per-tile painting at once and refines a slice per frame;
repainted chunks are revealed together (no patchwork of old and new), and
change detection is quantized so slow grazing drift does not repaint
everything at every 4 s terrain refresh.

## Marks at the feet (creatures)

Nothing rings the body; everything sits in the ellipse at the feet.

- **Contact shadow**: `MARK.shadow` at `shadowAlpha` 0.32, three stacked
  ellipses (a cheap blur), offset down-right.
- **Owner ring**: thin white ellipse tinted by owner (`colors.ts`); yours
  1.8 px with a faint fill, others 1.2 px; wild is grey so it nearly
  disappears.
- **Possessed**: a second ring just outside, coral `MARK.possessed`, with
  a dark under-stroke.
- **Pregnant**: a small pip beside the feet, green `MARK.pregnant`,
  outlined `OUTLINE`.
- **Selected**: a white ellipse a little wider than the owner ring, dark
  under-stroke, breathing ±6 %.
- **Action icons** (from `creature.ts`) at 80 % above the head: fleeing and
  hunting on anyone, resting and courting on your own only; zoomed far out
  only fleeing.

## Overlays and line weights

`LINE`: hair 1, thin 1.5, base 2, bold 2.5 screen px. `OUTLINE` 0x15181c for
every outline (same as the creatures). Light strokes on the map get a dark
under-stroke (≈ +1.75 px, 35–45 % alpha) so they read on snow and on water.

- **View window**: corner brackets (1.75 px, `MARK.view` 70 %) plus a 14 %
  hairline, never a filled or bright box.
- **Territories / rallies**: group colour, discs at 12 % (20 % picked),
  1.5 px outlines (3 px picked).
- **World edge**: a soft dark falloff outside the map.

## HUD over the map

Frosted dark cards (`rgba(13,17,23,0.72–0.78)`, 6 px backdrop blur, 8–10 px
radius, soft drop shadow). The toolbar groups joined buttons with 16 px
line icons (`ui/icons.ts`, 1.75 px round strokes) and labels on desktop;
on phones icons only, with the group commands in a **Tools** menu. The
legend is one collapsible card (open by default on desktop, closed on
phones).
