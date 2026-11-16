# Morphology contract (draft v0)

Shared data model for the parallel morphology work packages. `docs/morphology.md`
(the full spec) builds on this; if the spec changes a field, it updates this file
and says so. Defaults reproduce today's creatures (existing sliders keep working).

All angles are degrees on the head, 0 = straight ahead, +90 = right side, ±180 =
straight back. Features are **mirrored pairs by default**: an even count places
pairs at ±azimuth; an odd count puts the extra one on the midline (azimuth 0).

| Feature | Parameter | Type / range | Default | Comes from (today) |
| --- | --- | --- | --- | --- |
| Eyes | `eye_count` | integer 1–6 | 2 | — (new) |
| | `eye_azimuth` | 0–180 (pair angle; 0 = both forward) | 60 | — (new) |
| | `eye_size` | 0–1 | existing `Eyes` gene | `Eyes` |
| | eye field width | derived: 120° + 80° × eye_size (per eye) | | |
| Ears | `ear_count` | integer 0–4 | 2 | — (new) |
| | `ear_shape` | enum: `round`, `pointed`, `long`, `fan` | `round` | — (new) |
| | `ear_size` | 0–1 | existing `Ears` gene | `Ears` |
| Legs | `leg_count` | one of 0, 2, 4, 6, 8 | 4 | — (new) |
| | `leg_length` | 0–1 | existing `LegLength` | `LegLength` |
| | foot | derived: hooves / claws / webbed (FootWidth) / plain | | `Hooves`, `Claws`, `FootWidth` |
| Tail | `tail_present` | bool | true when `Tail` ≥ 0.05, else false | `Tail` |
| | `tail_shape` | enum: `thin`, `bushy`, `fin`, `club`, `prehensile` | `thin` | — (new) |
| | `tail_length` | 0–1 | existing `Tail` gene | `Tail` |
| Snout | `snout_length` | 0–1 | 0.5 | — (new) |
| Fangs | `fang_size` | 0–1, none below 0.1 | existing `Fangs` | `Fangs` |
| Wings, covering, colours | unchanged | | existing genes | |

Genome encoding (for the genome package): counts and enums are **discrete loci**
(alleles are small integers; expressed = one allele chosen by a dominance rule, not
an average, so offspring never get "2.5 legs"); mutation changes a count by one step
(legs by two) at a low rate; continuous parameters use the existing additive loci.

Derived properties (for the simulation packages), precomputed per organism at birth:
vision coverage (fraction of 360° seen), front binocular overlap (degrees),
rear blind-spot width (degrees), hearing range factor, leg stability/speed factors,
tail balance/swim/defence factors.
