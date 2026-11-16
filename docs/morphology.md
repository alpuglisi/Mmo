# Morphology: body-plan builder, geometry and effects

Nov 16, 2026 · design spec (no code yet). Builds on
[`morphology-contract.md`](morphology-contract.md), the shared data model the
parallel work packages code against. This spec **extends** the contract; where
it changes a field it says so in [Contract changes (v1)](#contract-changes-v1)
and the contract file carries the same note.

The owner wants three things:

- an avatar on the founder page that changes as traits change;
- new morphological traits: legs, eyes and their placement, ears, tail shape,
  snout and so on;
- a builder where the final **geometry** decides advantages and disadvantages.
  The owner's example: eyes on each side of the head leave a blind spot in
  front, and a third eye in the middle closes it.

Two earlier owner decisions still hold: combinations stay open (nothing is
banned), and downsides depend on context and on the other traits.

## Decisions in one screen

1. **We use a constrained, parametric builder, not free-form placement.** We
   agree with the coordinator. Every feature is a fixed slot with a few
   parameters: a count, a placement angle, a size and a shape from a short
   list. Dragging on the avatar edits those parameters and never stores raw
   coordinates. Free coordinates would turn a genome into an unordered list of
   features. Two such lists can't be matched up locus by locus, so an
   offspring could inherit something like "the average of an eye at 40° and a
   nostril". Parameters always give valid geometry.
2. **Geometry becomes a handful of derived numbers once, at birth**
   (`Traits`/`Morphology`), as stats are today. No geometry is computed per
   tick. The only per-tick addition is a sector lookup against the heading,
   which the vision package adds.
3. **All new effects are scaled to the default body.** At the default
   morphology every new multiplier is exactly 1 and every new additive term is
   0, so today's presets, calibration and README numbers still hold. New
   mechanics that change default play, such as directional vision or tail
   drag in burrows, sit behind `Balance` tunables that start at 0 and are
   switched on in a calibration pass.
4. **Geometry axes are free. Extra structures cost points.** Placement and
   shape are trade-offs, like the behavior genes. Each extra eye, extra pair of
   legs or extra pair of ears is an upgrade and costs a flat price per
   structure. Upkeep (food) and mass give the running cost in play. Section 4
   says how calibration checks this.
5. **Discrete loci use "wild type dominant" dominance** (refines the
   contract): the default allele is expressed over any other. This keeps new
   mutations hidden in carriers, as the hidden-recessives design intends, and
   keeps mixed matings fertile and sensible.

## 1. Feature model

### Phase 2 features (contract v1)

The parameters and defaults are in the contract table. This section adds how
each is laid out on the body.

| Feature | Zone | Layout rule |
| --- | --- | --- |
| Eyes | Head ring (azimuth 0–180°, mirrored) | An odd count puts one eye on the midline at 0°. Pair *k* (k = 0, 1, 2) sits at ±min(170°, `eye_azimuth` + 50°·k), so extra pairs step backward. Each eye's field is 140° + 60° × `eye_size` wide (**v1 change**, see below). |
| Ears | Top and sides of the head | Pairs only (**v1: 0, 2 or 4**). The first pair sits at the side or top of the skull, the second pair 25° further back. |
| Legs | Torso underside, evenly spaced from shoulder to hip | 0 is legless (moves on its belly), 2 is a biped (hind pair only), 4, 6 or 8. With wings and 2 legs, the wings are the forelimbs (a bird plan). With wings and 4 or more legs, the wings are an extra limb set (a dragon plan), which adds mass and upkeep (section 3). |
| Tail | Rear of the torso | Present when `Tail` ≥ 0.05. Its length is the `Tail` gene and its shape is the `tail_shape` enum. |
| Snout | Front of the head | `snout_length` runs from 0 (flat face) to 1 (long muzzle). |
| Fangs, claws, hooves, foot width | Mouth, feet | These keep their existing genes. The foot type is derived (as in the contract). |

**Body frame.** The torso is an ellipse whose size comes from mass, as in
today's sprite. Zones are fixed arcs on it: the head ring, the dorsal line, the
ventral line where legs attach, and the rear. A feature's position is a
parameter along its zone, so the builder can only place features where they
make sense, and rendering, the sim and the avatar read the same numbers.

**Symmetry.** Features are mirrored pairs, and dragging one eye moves its
mirror. Asymmetry (an owl's offset ears, a flounder's eyes) is Phase 3 at the
earliest, as one `*_asym` locus per paired feature with a small upkeep. Open
question Q3.

### Phase 3 candidates (not in the contract yet)

Each candidate needs its own loci. This matters because of the allele budget
limit in section 2.

| Feature | Parameters | Main effects |
| --- | --- | --- |
| Horns | `horn_size` 0–1, `horn_form` enum straight/curved/branched | Defense, fight-back and display. Downsides: mass, upkeep, and drag in dense forest (a straight rack snags the most). This replaces today's cosmetic rule that large herbivores get horns. |
| Wing shape | `wing_aspect` 0–1 (broad to narrow) | Narrow wings fly faster and migrate cheaper but maneuver badly in forest. Broad wings take off easily and handle forest. |
| Body elongation | `body_length` 0–1 | A long body burrows and swims better but turns worse, loses heat over its larger surface, and with 4 or fewer legs loses stability. |
| Claw placement | `claw_bias` front/hind | Front claws dig, climb and grip prey. Hind claws kick and strike like talons. |
| Covering pattern | `pattern` enum plain/spots/stripes | Camouflage strength depends on the biome (spots in forest, stripes in grass). Free of points and outside the genetic distance. |

## 2. Genome encoding

**Loci.** The contract adds 7 loci next to the existing 48 genes. (The README
says 46. That figure is out of date: `Gene::COUNT` is 48.) The new loci are:

- 4 discrete loci: `eye_count`, `ear_count`, `leg_count` and `tail_shape`;
- 1 discrete enum locus: `ear_shape`;
- 2 continuous additive loci: `eye_azimuth`, stored as azimuth/180, and
  `snout_length`.

The existing `Eyes`, `Ears`, `LegLength`, `Tail` and `Fangs` genes become the
size parameter of their feature, with **no change of meaning**. This is the
whole migration of the existing sliders. No existing gene is removed or
renamed.

**Allele budget (a risk for the genome package).** Young are written against
their parents' genomes with one choice bit per allele in a `u128`
(`genome::wire::Parents`, `assert!(ALLELES <= 128)`):

- today: 2 × (48 + 4) = 104 alleles;
- with the 7 contract loci: 118;
- after that, only 5 more loci fit.

Phase 3 therefore has to widen the choice bits (to `[u128; 2]`) or move the
morphology loci into a segment with its own choice bits. Both need a snapshot
version bump. We recommend a separate `morph` segment from the start, because
it also keeps the existing loci's random streams untouched (next paragraph).

**Random streams.** Founder variation and mutation of the new loci should draw
from a stream of their own, seeded from the other alleles. This is the
`Genome::own_rng` pattern used for `Obedience`. With it, a world whose
creatures all have the default morphology founds, breeds and mutates the old
genes exactly as before. That preserves the README's calibrated numbers and
lets a regression test check bit for bit.

**Dominance for discrete loci** (contract: "one allele chosen by a dominance
rule"; this spec defines the rule):

1. The default (wild-type) allele is expressed over any other.
2. Between two non-default alleles, the one closer to the default wins. For
   `leg_count` with default 4: 6 beats 8 and 2 beats 0. On a tie (2 vs 6),
   the lower count wins.
3. Enums use a fixed order:
   - `tail_shape`: thin > bushy > prehensile > fin > club;
   - `ear_shape`: round > pointed > long > fan.

What this does in play:

- A new count mutation is hidden in carriers. It shows only when homozygous,
  so inbreeding exposes it and outcrossing hides it, the same lever as the
  load loci.
- A six-legged founder line crossed with a four-legged rival gives four-legged
  young that carry 6. This is the design doc's "dilution" cost of loose
  breeding.
- Heterozygotes always express a real body plan. "2.5 legs" never happens.
- Founders get **no** variation on discrete loci (a design is homozygous). The
  continuous loci vary by `FOUNDER_VARIATION` as usual.

**Mutation.**

- Continuous morphology loci use the world's `MutationParams` (rate 0.004,
  sd 0.07).
- Discrete loci mutate at `Balance::morph_step_rate`, recommended 0.0005 per
  allele per generation (an eighth of the continuous rate). A mutation moves
  the count one step up or down (±2 for legs and ears, ±1 for eyes) or moves
  an enum to an adjacent value, clamped to the range.
- A mutated allele carries the owner of the organism it appears in, like every
  other mutation, so a new eye credits the player whose animal grew it.

**Inheritance between different layouts.** Each locus means the same thing in
every body (homologous slots), so inheritance is always locus by locus:

- `eye_azimuth` is "the angle of the first pair" whatever the count, so
  averaging it between a 2-eyed and a 3-eyed parent is meaningful.
- The midline eye and later pairs have no position of their own: they are
  placed relative to the first pair (section 1). There is nothing to average.
- In a single-eyed body (count 1), `eye_azimuth` is unused. It is still
  inherited and drifts as hidden variation, which is shown again if a later
  mutation brings back a pair.
- Because the layout rules apply to any combination, a nonsensical offspring
  can't occur. The worst case is a valid but poor body, and selection deals
  with it.

**Kin, species and fertility.** The goal is no change for default-morphology
populations and a real effect for different body plans.

- **Whole-genome distance.** New continuous loci are added to the numerator
  of `Genome::distance` with weight 1. The denominator stays
  `DISTANCE_GENES`, so identical new loci don't dilute distances and today's
  thresholds (kin at 0.08, sterile at 0.3) still hold. Each discrete locus
  adds `min(1, steps / 2)` to the same numerator.
- **Body-plan gap.** `leg_count` and `eye_count` also enter
  `mating::body_plan_gap` at **0.1 per step** of expressed difference (a step
  is 2 legs or 1 eye):
  - one step (4 vs 6 legs) gives 0.1, still fully fertile (the limit is 0.12);
  - two steps (4 vs 8, or 2 vs 6) give 0.2, half fertile;
  - three steps or more give 0.3 or more: sterile and not kin.

  Combinations stay open, but very different body plans become separate
  species, as the design doc's speciation intends. Carriers express the wild
  type, so a rising mutation doesn't make its carriers infertile.
- Ear and tail shape count only in the whole-genome distance.

**Origin tags at migration.** An old snapshot (version 19 or earlier) restores
with default morphology. The new alleles copy the origin tags of an anchor
locus, as `Obedience` copied `Wanderlust`'s tags:

| New locus | Copies tags from |
| --- | --- |
| eye loci | `Eyes` |
| ear loci | `Ears` |
| `leg_count` | `LegLength` |
| `tail_shape` | `Tail` |
| `snout_length` | `Nose` |

Every organism gains 14 alleles, so legacy shares shift slightly toward
whoever owns those anchor genes' tags. Deploy the change at a season boundary
(Q5).

## 3. Geometry to effects

**Principles.**

- Every number below is computed once in `morph::derive(&Phenotype)`, which
  `stats::derive` calls. Results land in `Traits` (as a `Morphology` struct)
  or as multipliers on existing stats.
- At the default morphology each multiplier is exactly 1. Unit tests check
  that every preset's `Traits` are unchanged at the default morphology.
- Where a downside is new behavior for today's genes (for example tail drag
  in burrows), it scales by a `Balance` knob that starts at 0.
- Notation:
  - Gene names (`Ears`, `LegLength`, `Tail`, …) mean expressed values.
  - `n = leg_count / 2` is the number of leg pairs.
  - `s = snout_length`.
  - `T = Tail` when a tail is present, else 0.

### Vision effects layer (consumes the vision package)

The vision package owns heading and the per-eye geometry (coverage, binocular
overlap, blind spots). This layer turns that geometry into play effects:

- **Split the base sense into channels.** Today
  `perception = 0.15 + 0.3·Eyes + 0.3·Ears + 0.3·Nose`. The baseline 0.15 is
  split into vision 0.10, hearing 0.03 and smell 0.02, so with default eyes
  the vision channel isn't zero and blind spots matter. Per sector, with
  vision gain `g(θ)`:
  - `perception(θ) = (0.10 + 0.3·Eyes)·g(θ) + hearing + smell`;
  - `g = 1` where at least one eye sees and 0 in a blind sector;
  - an animal that didn't move this tick scans: `g` is blended halfway toward
    the mean, scaled by `Balance::scan`.
- **Grid pruning.** The neighbor grid still prunes with the largest sector
  radius, so the spatial query is unchanged.
- **Depth judgment (hunting).** The hunter's kill probability is multiplied
  by `1 + k_depth · (min(binoc, 90) − binoc_default) / 90`.
  - `k_depth` starts at 0 and is about 0.35 after calibration.
  - `binoc` subtracts snout occlusion (below).
  - This affects hunters and nothing else.
- **Rear blind spot (ambush).** Nothing extra is needed. A hunter approaching
  from a blind sector simply isn't detected, so `ambush_on` already applies.
  A fleeing animal faces away from its pursuer: side-eyed prey keep it in
  view, while forward-eyed prey lose it and stop fleeing early. This
  predator-versus-prey eye trade-off comes out of the rules on its own.
- **Front blind spot (the owner's example).** With side eyes, prey straight
  ahead drops out of view during a chase, so the hunter loses track. A
  midline eye fills the gap and adds overlap (prototype numbers in the
  appendix: side eyes at 90° leave a 40° front gap with the v1 field width;
  adding a third eye closes it).
- **Upkeep.** Each eye beyond 2 adds `0.03·(0.5 + Eyes)`.

### Ears (`ear_count` c ∈ {0, 2, 4}, `ear_shape`, size `Ears`)

| Shape | gain | localization `L0` | area `A_s` |
| --- | --- | --- | --- |
| round (default) | 1.0 | 0.4 | 1.0 |
| pointed | 1.1 | 0.7 | 1.1 |
| long | 1.2 | 0.6 | 1.5 |
| fan | 1.3 | 0.5 | 2.0 |

- **Hearing range.** `hearing = 0.03 + 0.3·Ears·gain·f_c`, where `f_c` is 0
  for no ears, 1 for 2 ears and 1.15 for 4. With no ears the animal still
  feels vibration (0.01). Hearing is omnidirectional, so it is what still
  detects threats in a visual blind spot. Large ears plus side eyes cover each
  other's weaknesses.
- **Localization.** `L = L0 + 0.2·[c = 4]`, 0 with no ears.
- **Effect of localization.** A threat detected by hearing alone, with no
  vision in its sector, is placed with a bearing error of up to `70°·(1 − L)`.
  The animal flees from where it thinks the threat is, which can lead it into
  a hunter.
- **Heat** (contextual with climate and fur). Radiating area is
  `A = Ears·(c/2)·A_s`.
  - Insulation falls by `0.15·A·(1 − 0.4·Fur)` (today it falls by
    `0.15·Ears`, so round ears with 2 ears reproduce that).
  - The heat tolerance of a warm body rises: `high += 6·A` °C.
  - Fan ears are a desert asset and a tundra liability. Fur blunts the
    liability.
- **Silhouette.** Stealth falls by `0.04·Ears·(A_s − 1)`: big ears give a
  standing animal away.
- **Upkeep.** `0.1·Ears·(c/2)`, replacing today's `0.1·Ears`. A body with no
  ears pays none.
- **Alarm calls.** The group trait requires `Ears > 0.5` today. It now also
  requires `c > 0`.

### Legs (`leg_count`, length `LegLength`, foot type from existing genes)

| Pairs n | 0 | 1 (biped) | 2 (default) | 3 | 4 |
| --- | --- | --- | --- | --- | --- |
| gait g | 0.6 | 0.95 | 1.0 | 0.92 | 0.85 |
| stability S0(n) | 1.0 (belly) | 0.45 | 0.7 | 0.85 | 0.95 |

- **Speed.** The leg terms (`0.45·LegLength + 0.25·Hooves`) are multiplied by
  `g`. With no legs the base 0.45 is multiplied by 0.6 and the leg terms are
  dropped, because a legless body moves by undulation.
- **Stability.**
  `S = S0(n)·(1 − 0.3·LegLength) + 0.15·T·[biped]`.
  Long legs are tippy, and a biped balances on its tail. The default body has
  `S_ref = 0.7`. Stability scales four things relative to the default:
  - ice slip and steep-slope cost: `× S_ref / S`;
  - the speed penalty of armor: `0.35·Armor × sqrt(S_ref / S)`, so a hexapod
    carries plates better;
  - resistance to attack: `+0.1·(S − S_ref)`, harder to knock down.
- **Biped balance.** A biped with `T < 0.2` loses `0.15·(1 − T/0.2)` agility.
  A biped needs a tail.
- **Ground pressure.** `foot_area × (n / 2)`. A legless body uses a belly
  area of 1.5. More legs help on mud, snow and sand. A legless body is good on
  sand.
- **Height.** A legless body is `0.15 + 0.3·size` tall. A biped gets
  `+0.1·LegLength`, which sees over cover and wades.
- **Terrain abilities.**
  - Climbing is multiplied by `0.6 + 0.2·n` (capped at 1.2). A biped climbs
    at half.
  - A legless body gets burrowing +0.2 and swimming +0.15. Long legs reduce
    burrowing by `0.2·LegLength`.
  - A legless body gets stealth +0.1 (low profile).
- **Mass and food.**
  - Mass is multiplied by `1 + 0.04·(n − 2)·(0.5 + LegLength)`.
  - Upkeep changes by `+0.05·(n − 2)`, which can go negative but is floored
    at −0.05.
  - Endurance falls by 0.04 for each pair beyond 2.
- **Wings as extra limbs** (wings present and n ≥ 2). Mass is multiplied by
  `1 + 0.15·Wings` and upkeep rises by `0.1·Wings`. Wings that replace the
  forelimbs (a biped with wings) pay neither, but climb at half.

In short: extra legs pay off on mud, snow, slopes, ice, in climbing and under
armor, and cost speed, food and mass. Bipeds trade stability for height and
need a tail. Legless bodies trade speed for stealth, burrowing, swimming and
sand.

### Tail (`tail_shape`, length T)

| Shape | agility | other benefit | cost |
| --- | --- | --- | --- |
| thin (default) | +0.3·T (today) | none | none beyond today |
| bushy | +0.2·T | warm bodies: insulation +0.15·T while resting; display: stealth −0.05·T | none |
| fin | +0.1·T | swim +0.4·T | land speed −0.05·T |
| club | +0.05·T | defense +0.3·T; fight-back +0.3·T | mass ×(1 + 0.15·T); speed −0.05·T |
| prehensile | +0.2·T | climb +0.3·T; grip +0.15·T | upkeep +0.05·T |

Each of these downsides scales by a `Balance` knob that starts at 0 (the
`traitText.ts` "tail: downside TBD" item):

- **Drag in burrows and dense vegetation:** burrow speed and the dense-forest
  cost are multiplied by `1 − 0.2·T`.
- **Heat loss in the cold** (thin and fin tails): insulation −0.05·T.

### Snout (`snout_length` s, default 0.5)

Every term is written relative to 0.5.

| Effect | Formula | Context |
| --- | --- | --- |
| Smell | the nose channel `0.3·Nose` is multiplied by `0.7 + 0.6·s` | A long muzzle smells better. |
| Bite | the fang term `0.3·Fangs` is multiplied by `1.3 − 0.6·s` | A short face bites hardest; a long snout is weak. |
| Snapping small prey | prey lighter than 0.2× the hunter's mass gets escape chance `× (1 − 0.4·(s − 0.5))` | Insect and fish catchers. |
| Occlusion | binocular overlap minus `40°·max(0, s − 0.5)` | A long snout costs depth judgment, so a stalker with a long snout is a contradiction the player must weigh. |
| Cooling | the heat tolerance of a warm body: `high += 3·(s − 0.5)` °C | Panting surface. |
| Digging | burrowing +0.1·(s − 0.5) | Rooting snouts. |

### Mass and upkeep summary

`upkeep` (which multiplies metabolism, as today) gains these terms:

- eyes: `+0.03·(0.5 + Eyes)` per eye beyond 2;
- ears: `0.1·Ears·(c/2)` instead of `0.1·Ears`;
- legs: `+0.05·(n − 2)`, floored at −0.05;
- extra-limb wings: `+0.1·Wings`;
- prehensile tail: `+0.05·T`.

Mass gains the leg, extra-wing and club factors. Mass already feeds speed,
ground pressure, wing loading, food per hunger point and prey value, so heavy
builds pay everywhere without new rules.

## 4. Pricing and calibration

- **Rule.** Points pay for structures, not geometry.

  | Item | Price (points) |
  | --- | --- |
  | Each eye beyond 2 | 3 |
  | Each leg pair beyond 2 | 5 |
  | Second ear pair | 3 |
  | Tail shapes, `eye_azimuth`, `snout_length`, fewer structures | 0 (trade-offs) |

  Fewer structures cost nothing and refund nothing, like lowering any trait
  below its default today. The existing size genes keep their prices (eyes 4,
  ears 4, leg length 25, tail 4). These starting prices are guesses to be
  replaced by calibration.
- **Pricing by derived property was considered and rejected for now.** That
  would mean paying for coverage gained, so a midline eye would cost more on a
  side-eyed head. It is the most "geometric" option, but the price of one
  feature would change when another feature moves, which is hard to explain
  in the editor. Context comes from play instead: upkeep, mass and the
  situational rules. Q1 asks the owner.
- **`calibrate.rs`** gets morphology options:
  1. **Marginal:** each sample design gets +1 step of each count, each
     alternative tail and ear shape, `eye_azimuth` ±40° and `snout_length`
     ±0.4. Structure prices follow the existing rule (about 30 points per
     unit of log legacy share).
  2. **Tournament:** random full-budget designs draw morphology too. The
     regression of log share on points includes structure points.
  3. **Free-axis fairness:** this check is new. Over at least 60 worlds, no
     value of a free axis may win more than its fair share + 5 percentage
     points. This is the same test that caught "small cold-blooded omnivores".
     An axis end that wins anyway gets a price or a stronger downside, not a
     ban.
  4. **Derived-property audit:** regress legacy on the derived numbers
     (coverage, binocular overlap, rear gap, S, hearing, L) to find
     combinations the per-structure prices miss.
- **Keeping "max everything" in check.** Prices only ever add, so
  `maxing_everything_is_over_budget` stays true. Extend that test with all
  counts at maximum. In play, 8 legs + 6 eyes + 4 fan ears adds about
  +0.3 upkeep and +16% mass to an already maxed body, and fan ears are
  stealth-negative in the cold. Mutation can push a lineage past the budget
  after founding, and these running costs then hold it back, as the comment
  in `stats.rs` intends.
- **Enabling directional mechanics.** Switch the knobs on one at a time in
  this order, re-running the standard sweep each time and accepting a change
  when the score stays within ±0.3 of 11.76 (the standard `sweep --size 96 --trials 1 --seeds 12 --rounds 1 --seed 31`):
  1. `scan`;
  2. `k_depth`;
  3. the tail and leg downsides.

  Re-tune `ambush` and `perception_range` if prey survival moves.

## 5. Rendering (sprites and avatar packages)

- **One renderer, three levels of detail.** A pure function
  `creatureShapes(BodyParams)` produces the shapes. `BodyParams` is
  continuous and is filled from three sources:
  1. **Far:** the coarse `look` class (today's 7-letter texture key);
  2. **Near:** quantized morphology fields in `look` (eye count and azimuth
     bin, legs, ear count and shape, tail shape and length bin, snout bin),
     keyed into a capped LRU texture cache of about 256 textures on phones;
  3. **Avatar and inspector:** exact parameters, drawn as SVG on the founder
     page and never as cached GPU textures.

  The level is picked by marker scale: below 1 uses the class sprite.
- **Two views.**
  - The world stays in side view (facing ±x), so eye azimuth shows as eye
    position: a forward eye sits on the face, a side eye on the cheek, and a
    midline eye on the forehead.
  - The builder and the inspector add a **top-down head diagram**: vision
    cones with the binocular zone filled, blind sectors hatched, a hearing
    ring and leg footprints.
  - The selected own organism can show its vision fan on the map, relative to
    the server's heading.

## 6. UX: builder, sliders and avatar together

- **One source of truth.** The `DesignSpec` holds everything. Sliders, feature
  cards and drag handles all write to it, and `update()` redraws the avatar,
  the readout and the costs.
  - The existing sliders for eyes, ears, leg length, tail and fangs move into
    the feature cards as each feature's size control, using the same gene.
  - The other sliders stay where they are.
- **Layout.**
  - **Desktop:** the avatar sticks to the top of the right column, above the
    budget. Under it is the derived-stat readout.
  - **Phone:** a sticky header holds an avatar thumbnail and the budget. A tap
    expands it to a full-width avatar with handles.
- **Body section.** One card each for eyes, ears, legs, tail and snout. A card
  has:
  - a count stepper (− / +) showing the price per structure;
  - shape chips (thin, bushy, fin, club, prehensile);
  - the size slider;
  - its placement slider, mirrored by the drag handle.
- **Drag handles** (Phase 3, built by the builder package):
  - drag an eye along the head ring to set `eye_azimuth`; its mirror follows;
  - drag the tail tip to set `Tail`;
  - drag the snout tip to set `snout_length`;
  - drag a foot down to set `LegLength`.

  Handles project onto the parameter, so an invalid geometry can't be made.
  Implementation notes:
  - Handles use pointer events (mouse, touch and pen alike) with hit targets
    of at least 44 px.
  - One drag is one undo step.
  - Undo and redo hold 100 steps, with buttons plus Ctrl/Cmd+Z and
    Shift+Z.
- **Presets.** Grazer (wide view), Stalker (forward eyes, short snout),
  Strider (long legs), Six-legged climber, Burrower (legless, flat face),
  Swimmer (fin tail). Applying a preset shows the cost change first. Every
  preset is itself just a `DesignSpec`.
- **Live readout.** Each derived number is shown against the default body,
  with an arrow and a short sentence written in the style of `traitText.ts`:
  - **Vision:** view %, depth zone °, blind behind °, blind ahead °;
  - **Hearing:** range and localization;
  - **Movement:** speed, stability, ground pressure and climbing;
  - **Tail:** balance, swimming, display and defense;
  - **Upkeep:** food %.

  Context-dependent captions ("fan ears: sheds heat; in cold biomes costs
  warmth unless furred") reuse the `context` mechanism.
- **Accessibility.**
  - Every handle is also a focusable `role="slider"` that moves with the arrow
    keys. Its `aria-valuetext` gives the result, for example "eyes 90° to the
    side: 300° view, 0° depth, 60° blind ahead".
  - Cones use patterns as well as color.
  - The avatar has a text summary built on `describeLook`.
  - Animations respect `prefers-reduced-motion`.
- **Simple and Builder modes.** A toggle switches between sliders with a
  passive avatar (the default for new players) and the drag builder. Both edit
  the same spec.

## 7. Protocol and persistence

- **`DesignSpec`.** Add an optional `morph` object, for example
  `{"eye_count": 3, "eye_azimuth": 90, "ear_count": 2, "ear_shape": "pointed", "leg_count": 6, "tail_shape": "fin", "snout_length": 0.3}`.
  - Missing fields take their defaults, so old clients and stored designs are
    unchanged.
  - Keeping morphology out of `traits` keeps that map all f32 in [0, 1].
- **`GET /api/genes`.** Add a `morph` section listing each parameter's kind
  (count, enum or axis), allowed values or range, default, price per
  structure, feature group and labels, plus the presets.
- **Server validation.**
  - Counts must be in the allowed set (eyes 1–6, ears 0/2/4, legs
    0/2/4/6/8). Enums must be known. Axes must be finite and in range.
  - Error codes: `bad_morph` (`detail.field`), plus the existing
    `over_budget`.
  - The server recomputes cost and all geometry. A client never sends derived
    values, so there is nothing to forge.
- **Wire to the client.**
  - `look` gains the quantized fields, which old clients ignore.
  - An own organism's detail carries its exact morphology.
  - Lineage stats show histograms for counts and shapes.
  - Market listings show leg and eye counts.
  - Update `wire.fixture.json`.
  - The client copy of `morph::derive` (for the readout) is tested against a
    fixture generated from Rust, so the two can't drift.
- **Snapshot version 20.**
  - The genome wire format gains the morphology loci, as a separate segment
    if the recommendation in section 2 is taken, with its own choice bits in
    `Parents`.
  - `Organism` gains `heading` (vision package).
  - New `Balance` knobs are read as 0 from older versions (the
    `serde_util::before_v20` pattern).
  - Versions up to 19 restore with default morphology and anchor-copied tags.
  - Bump `SAVED_GAME_VERSION` only if the saved JSON changes.

## 8. Phases and work packages

The four contract packages already running are: genome loci (G), directional
vision (V), in-world sprites (S) and the drag builder component (B). The
avatar package (A) maps to the contract too.

**Phase 1: avatar from existing traits** (client only, the smallest valuable
step).

- **P1a `BodyParams` + `bodyFromDesign`/`bodyFromLook`.** Refactor
  `creatureShapes` to take continuous parameters.
  - Files: `client/src/world/creature.ts`, new `body.ts`.
  - Tests: every existing look class gives identical shapes, and each visible
    gene changes the shapes.
  - Done when in-world sprites are pixel-identical to today.
- **P1b SVG avatar in `design.ts`,** redrawn at most once per animation frame
  on input, with sticky layout and a text summary.
  - Files: `client/src/ui/design.ts`, new `ui/avatar.ts`, `style.css`.
  - Done when moving any visible slider changes the avatar within one frame on
    a mid-range phone.

**Phase 2: counts, placement and derived stats** (G, V and S are under way).

| WP | Scope | Files | Tests and acceptance | Depends on |
| --- | --- | --- | --- | --- |
| G | Loci, dominance, step mutation, own random stream, distance and body-plan gap, wire + v19 migration | `genome.rs` (or new `morph.rs`), `mating.rs`, `world/snapshot.rs` | Round trip; carriers express the wild type; distance is unchanged at default; a v19 snapshot restores | contract v1 |
| D | `morph::derive`: ear, leg, tail, snout and upkeep formulas from §3, all exactly neutral at default | `stats.rs`, new `morph.rs`, `config.rs` | Preset `Traits` unchanged at default; contextual unit tests (fan ears in heat vs cold, a hexapod under armor, a biped without a tail) | G |
| V | Heading, sectors, scanning, depth multiplier, hearing localization error | `organism.rs`, `world/decide.rs`, `world/apply.rs` | The owner's example as a test; decide-phase benchmark regresses less than 3% | G, D |
| P | Prices, validation, `calibrate.rs` options and the free-axis check; README "Point costs" | `design.rs`, `examples/calibrate.rs`, README | No free axis end above fair share + 5 pp; the sweep stays within ±0.3 | D, V |
| E | Server: `morph` spec, catalog, quantized look, stats, market | `server/src/design.rs`, `appearance.rs`, `protocol.rs`, `game.rs` | Serialization and validation codes | G |
| S | Sprites from quantized look, LRU cache, LOD | `client/src/world/creature.ts`, `sprites.ts`, `textureCache.ts` | Cache cap respected; class sprites unchanged when zoomed out | E schema |
| C | Editor feature cards, readout, head diagram, Rust-fixture mirror test, `traitText` entries | `client/src/ui/design.ts`, `cost.ts`, `protocol.ts`, `traitText.ts` | Client cost equals server cost; readout matches the fixture | E schema, D |

Parallel tracks once this spec's tables are fixed:

- G → D → V and P, in that order (V's heading plumbing can start now);
- E's schema, then S and C, alongside.

**Phase 3: free placement and more features.**

- Drag handles (B), undo and redo, the presets gallery, the map vision fan.
- New loci: horns, `wing_aspect`, `body_length`, `claw_bias`, second-ear-pair
  offset, asymmetry, patterns. These need the allele-budget change from
  section 2 first.

**Risks.**

1. Directional detection changes predator-prey balance. That is why it ships
   behind tunables and is enabled in a calibration pass.
2. The cost of the decide hot loop. Keep the sector lookup trig-free (sign
   and ratio tests on dot and cross products against a mirrored 8-sector
   table).
3. The allele cap (section 2).
4. Formula drift between client and server (the fixture test).
5. Texture memory on phones (LRU cap, LOD).
6. Too much complexity for new players (Simple mode is the default).
7. A legacy-share shift at migration (deploy at a season boundary).

## 9. Open questions for the owner (each with our recommendation)

1. **Pricing: per structure or per derived benefit?** We recommend per
   structure with geometry free, as in §4. It is simple to read, and context
   comes from play.
2. **Count ranges.** We recommend:
   - eyes 1–6 (a single midline eye allowed, 0 not: blindness would need its
     own sense model);
   - legs 0–8 in pairs;
   - ears 0, 2 or 4.
3. **Asymmetry.** We recommend no asymmetry before Phase 3, then one locus per
   paired feature with an upkeep cost. Mirrored stays the default.
4. **Should different body plans lose fertility?** We recommend yes: 0.1
   body-plan gap per count step, so one step stays fertile and three steps
   make a new species.
5. **Legacy and the new alleles at migration.** We recommend counting them,
   with tags copied from anchor genes and deployment at a season boundary.
6. **Dominance of new counts and shapes.** We recommend wild type dominant:
   mutations hide in carriers and show when inbred. The alternative,
   "derived dominant", would spread new body plans much faster.
7. **Directional vision in possession.** We recommend the same rules, with
   the existing possession perception bonus. The player sees their animal's
   cone, which makes possession more tactical.
8. **Who sees vision cones?** We recommend the owner's own organisms only.
9. **Cosmetic patterns and colors.** We recommend they be genetic and
   inherited, but free and outside the genetic distance (Phase 3).
10. **Wings with 4 or more legs (a dragon plan).** We recommend allowing it
    as an extra limb set that pays mass and upkeep. Two legs plus wings means
    the wings are the forelimbs.
11. **Turn on directional vision by default?** We recommend yes, but only
    after the calibration pass shows the sweep within ±0.3 and no free-axis
    dominance.

## Contract changes (v1)

These are mirrored in `morphology-contract.md`.

| Field | Old → new | Why |
| --- | --- | --- |
| Eye field width | 120° + 80°·size → **140° + 60°·size** | The old formula leaves the default eyes (size 0, azimuth 60°) with exactly 0° binocular overlap and a 120° rear blind spot. Hunters could then never lose depth judgment relative to the default, and turning the tunable on would hit every default animal hard. The new formula gives the default 20° of depth and a 100° rear gap, and keeps the 200° maximum. |
| Multi-pair eye placement | unspecified → pair k at ±min(170°, azimuth + 50°·k) | Counts 4 and 6 need positions. Placing them relative to the first pair means one azimuth locus is enough and inheritance stays meaningful. |
| `ear_count` | integer 0–4 → **one of 0, 2, 4** (mutation step 2) | A midline or odd ear has no mirror and no meaning for localization. |
| Discrete dominance | "a dominance rule" → wild type dominant; nearer the default wins; on a tie, the lower count; fixed enum orders | Deterministic and never nonsense. Hides new mutations in carriers, consistent with the load loci. |
| Snout occlusion | none → binocular overlap minus 40°·max(0, snout − 0.5) | Ties snout geometry to vision, as the owner wants. |
| Allele budget note | none → 7 loci reach 118 of the 128 `Parents` choice bits | Phase 3 loci need the bits widened or a separate segment. |

## Appendix: vision prototype

The prototype script is
`/tmp/claude-0/-home-user-Mmo/a9a7624c-2561-5b05-9349-61edc349657f/scratchpad/vision.py`.
It works on whole degrees. The table uses the v1 field width 140° + 60°·size;
depth is binocular overlap within ±60° after snout occlusion.

| Layout | View | Depth | Blind ahead | Blind behind |
| --- | --- | --- | --- | --- |
| default: 2 eyes, azimuth 60°, size 0 | 260° | 20° | 0° | 100° |
| side eyes: azimuth 90°, size 0 | 280° | 0° | 40° | 40° |
| side eyes + midline eye (3 eyes) | 320° | 80° | 0° | 40° |
| grazer: azimuth 100°, size 0.5 | 330° | 0° | 30° | 0° |
| stalker: azimuth 25°, size 0.6 | 226° | 120° | 0° | 134° |
| stalker + rear pair (4 eyes) | 326° | 120° | 0° | 34° |
| stalker, snout 1.0 | 226° | 100° | 0° | 134° |
| default, snout 1.0 | 260° | 0° | 0° | 100° |

Under the v0 width (120° + 80°·size), the default had 0° of depth and a 120°
rear gap, and side eyes had a 60° front gap.

## Decisions taken (owner delegated)

The owner asked for work to continue without them and for each decision point
to be settled with the recommended option. The open questions above were
settled on 2026-11-16 as recommended:

1. Pricing: per structure (extra eyes, leg pairs, ear pairs cost points);
   placement and shape are free trade-offs.
2. Ranges: eyes 1–6, legs 0–8 in pairs, ears 0/2/4.
3. Asymmetry: not before Phase 3; then one locus per pair with an upkeep cost.
4. Fertility between body plans: yes, 0.1 body-plan gap per count step.
5. Legacy at migration: new alleles count, tags copied from the related
   existing gene; deployed at a season boundary.
6. Dominance: the default (wild-type) allele is dominant.
7. Directional vision while possessing: same rules plus the existing
   possession bonus.
8. Vision cones: visible only to the owner, on their own organisms.
9. Cosmetic patterns: genetic, free, outside the genetic distance.
10. Wings with 4+ legs: allowed as extra limbs costing mass and upkeep; with
    2 legs the wings are the forelimbs.
11. Directional vision on by default only after calibration keeps the
    standard sweep within ±0.3 of its baseline (11.76).
12. Allele cap: morphology loci go in a separate genome segment with their own
    choice bits and random stream, so default-morphology worlds stay
    bit-identical.
13. Genome package deviations (accepted 2026-11-16, keeps existing worlds
    bit-identical): discrete count/shape mutation is a tunable
    `morph_step_rate`, default 0, to be enabled by the morphology balance
    package; `eye_azimuth` and `snout_length` stay out of the genetic distance
    for now; morphology alleles carry origin tags but stay out of the legacy
    tally until the deployment at a season boundary (decision 5).
14. Snapshot versions: v20 trait downsides, v21 body plan effects (package D),
    v22 directional vision, v23 morphology genome loci.
15. Wiring (2026-11-16): the genome's expressed `morph::Morphology`
    (`Organism::morph`) is the one body plan; `morph_effects::BodyPlan` is
    that type. It feeds the eye geometry and ear localization at birth
    (`stats::derive_with`), the body plan effects (behind the `morph_*`
    tunables), LOD cohort representatives and the server's `look`. Earless
    bodies carry no alarm calls (structural). Defaults stay bit-identical;
    with the tunables on, founders' snout and eye-azimuth variation gives
    default-designed lineages small effects around zero.
