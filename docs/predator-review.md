# Predator rules: correctness review

An independent review of the carnivore-diet rules and predator brakes
(README *Predators over the years*, *Predators that live on meat*,
*Predators in their niches*; `Balance::roaming_forage`, `carcass_keep`,
`carnivore_fat`, `carcass_scent`, `carrion`, `provisioning`,
`breeding_territory`, `prey_switching`, `condition_litter`). What I read:
`config.rs`, `world/decide.rs`, `world/apply.rs`, `world/seasonal.rs`,
`world/par_apply.rs`, `terrain.rs` (carcass rot), `presets::niche_world`,
`serde_util::before_v25..v27`, the snapshot tests and the LOD cohort code
where it touches fat and lineage counts.

Every fix has a test in `crates/sim/src/world/predator_tests.rs` that
failed before the fix. All rules stay off by default, and the standard
sweep (`evo-sim sweep --size 96 --trials 1 --seeds 12 --rounds 1 --seed
31`) still scores **11.60**.

## Fixed

| # | Severity | Finding | Fix | Test |
| --- | --- | --- | --- | --- |
| 1 | Medium | **Territory leak within a tick.** `territory_taken` was only checked at decide time (`can_breed`, and in `look` for the female being courted). Two pairs in one territory that both reach each other in the same tick both conceived: the second female's decision was made before the first one became pregnant. | `try_mate` checks `territory_taken(female)` again at the moment of mating. Pregnancies are read live, so a conception earlier in the tick counts. | `a_territory_holds_one_litter_even_within_a_tick` |
| 2 | Medium (egg layers) | **Territory leak for egg layers.** Only pregnancies and unweaned young counted. Once an egg layer had laid, her clutch held no territory until it hatched, so neighbours of her lineage could breed for the whole incubation. This hits the burrowing omnivores (carnivory 0.5, incubated eggs, a fifth of the radius) and any egg-laying player carnivore. | `territory_taken` also counts an unhatched clutch of her lineage within the radius (only for egg-laying females, so live bearers cost nothing extra). | `a_clutch_takes_the_territory_too` |
| 3 | Medium | **Prey switching ignored the world seed.** The daily draw hashed only hunter id, prey lineage and day. Founders have the same ids in every world, so every seed made the same give-up choices on the same days (480 of 480 draws matched between seeds 1 and 2). That makes seeds correlated in sweeps and in predlong. | The world seed is mixed into the salt. | `prey_switching_draws_depend_on_the_seed` |
| 4 | Low–medium | **Providers missed their family in a crowd.** A provider looked for its hungry mate or young only among the neighbours it examined closely: the nearest `FOCUS` (32), plus farther ones that could be a threat, prey or mate. That pruning promises "sets cost, not behavior", but family wasn't one of its cases, so in a den crowd a provider failed to see its pup unless the pup happened to be small enough to count as possible prey. | When the viewer is a provider, the far-neighbour pass also looks at same-lineage neighbours. The extra looks can't change threat, prey or mate choices, because those are still bounded by the same upper-bound tests. | `a_provider_finds_its_young_in_a_crowd` |
| 5 | Low | **Carrion missed some deaths.** The rule (and the README) says any death but predation or drowning leaves a carcass. But only deaths resolved in `finish` did: a hunter poisoned by toxic prey (`attack`) and a migrant lost on a journey (`act`) left nothing. | A `leave_carrion` helper is used by `finish`, the journey death and, once its turn is over, the poisoned hunter. | `a_poisoned_hunter_leaves_carrion` |
| 6 | Doc | `carcass_keep` 0.75 was documented as "30% a day" rot above 15 °C. Tiles update hourly: `1 − (1 − 0.05 × 0.25)^24` = **26%** (11% below 15 °C is right). | README table and `Balance::carcass_keep` doc corrected. | n/a |

Also added a check that found no bug:
`carnivore_rules_are_deterministic_and_resume_from_snapshots` runs the
niche world with every rule on and checks two things. The run is identical
on 1 and 4 threads (so the parallel apply has no race). A snapshot taken on
day 6 and restored continues bit for bit like the uninterrupted run. All
nine tunables are saved, and the rules keep no state outside the snapshot.
They reuse `forage_target`, `fat` and `search_image`, which are already
serialized.

### Effect with the rules on

`predlong <years> <seeds> --meat` (niche designs, all rules on,
96×96):

| Run | Fox survived (viable) | Pack survived (viable) | Prey lost | Lowest prey |
| --- | --- | --- | --- | --- |
| Before, seeds 31–38, 6 years | 6 (6) | 7 (7) | none | voles 53, antelope 13, newts 10, omnivores 20 |
| After, seeds 31–38, 6 years | 6 (4) | 7 (7) | none | voles 47, antelope 10, newts 13, omnivores 11 |
| Before, seeds 39–46, 3 years | 8 (8) | 6 (6) | none | voles 78, antelope 7, newts 42, omnivores 25 |
| After, seeds 39–46, 3 years | 8 (8) | 5 (5) | none | voles 71, antelope 27, newts 38, omnivores 19 |

The "before" rows reproduce the README's niche-design row exactly. Fix 3
draws switching from new random numbers, so every seed's run comes out
different after it. The differences are one seed each way (on seeds 31–38
the fox ends females-only on 34 and 35; on seeds 39–46 the pack hunters are
lost on one more seed), with no consistent direction across the two sets.
That looks like the seed-to-seed spread, but on seeds 31–38 the README's
long-run acceptance (both predators viable on 6 of 8 seeds, no prey lost)
no longer holds for the fox: 4 viable of 8. It holds for the pack hunters,
and the fox survives on 8 of 8 fresh seeds over 3 years. More seeds would
be needed to tell whether the fixes cost the fox anything. No prey
lineage is lost in any run. Final head counts (31–38, after): foxes 1–108,
pack hunters 58–263.

## Fixed in the follow-up (on by default, `Balance::strike_fixes`)

| Severity | Finding | Fix | Test |
| --- | --- | --- | --- |
| Low–medium (pre-existing) | **A poisoned hunter kept acting.** In `attack`, a hunter killed by toxins (`kill(i, Toxin)`) didn't return. The same strike could still go on to kill and leave the prey, and back in `act` the dead hunter ate meat from its tile (`eat_meat` doesn't check `alive`). | `attack` returns after the toxin death, and `act` feeds a hunter only while it lives. | `a_poisoned_hunter_strikes_no_more` |
| Low (pre-existing) | **Double meal on the kill tick.** `attack` called `eat_meat(i, 1.0)` after a kill, and the `Hunt` branch of `act` called it again: up to twice the per-tick intake, the second helping going to fat with gorging. | Only `act` feeds the hunter. | `a_kill_is_eaten_once` |

Gated by snapshot version 29 (`serde_util::before_v29`; older saves read
`strike_fixes` = 0, the old strike). Default sweep: 11.63 (old strike
11.60). `compare --seed 101 --size 96 --adaptive vs --set
strike_fixes=0`: unresolved at 48 seeds, sequential 95% CI −0.36 … +0.11;
pack hunters survive 96% against 100% (−4 ± 6%, not significant).

## Not fixed (reported)

| Severity | Finding | Why not fixed |
| --- | --- | --- |
| Low | **A hunter killed by prey that fought back leaves no carcass**, even with carrion on (it dies by `kill_by` and counts as predation, but nobody eats it). | Matches the rule as written ("other than predation"). A design choice; flagged in case it should change. |
| Low | `prey_scarcity` is documented as using "agents at the last daily count" but reads the live `lineage_alive` count. | Deterministic either way: reads are in the parallel decide, where nothing writes, or in the sequential apply (`raid_clutch`). Doc mismatch only. |
| Low | `carcass_keep` isn't range-checked. Above 1, carcass meat grows every hour (energy from nothing); below 0 it rots faster than default. | It's a tunable set by hand (`--set`); no tunable in `Balance` is validated. Worth a clamp if tunables ever become player-facing. |
| Low | The remembered-kill and mate's-kill shortcuts in `scent_carcass` skip the `roam_bound` check that the scent search applies, so a group-territory member can walk out of its territory to its last kill. | Rare (needs a group territory claim on a carnivore group). |
| Info | Directed mating (`commands::directed_mating`) ignores breeding territories (and `can_breed`, the breeding window and so on). | Player override by design. |
| Info | Territories only count the female's own lineage: two lineages of one kind (another owner's, or a hybrid line) don't block each other. | As documented. |
| Known gap | LOD cohorts model none of these rules. Converting an agent to a cohort drops its pregnancy and blends a gorged carnivore's fat into the cohort's mean. Cohorts fatten only in their fattening season, and their cap ignores `carnivore_fat`. Fat is conserved in aggregate, so this is not energy from nothing. | Already listed as left for later in the README. Another worker is changing `lod.rs`. |

### Checked and found sound

- **Provisioning conserves food.** The provider pays `amount ×
  food_per_hunger(fed) / food_per_hunger(provider)` from its fat (no 1.2
  overhead, unlike nursing). It can't overdraw (`min(fat / ratio)`) or
  overfeed (`min(hunger)`). Pregnant animals and dependent young can't
  provide, so two providers never feed each other. A mother's own young are
  excluded: she nurses them. Provider and fed animal are both engaged, so
  the parallel apply keeps the sequential order.
- **Gorging.** `meat_fat_room` takes the larger of the seasonal room and the
  gorging room. `feed` banks only what fits after hunger, and `eat_meat`
  never takes more than hunger + room. No double counting beyond the
  kill-tick meal, now fixed (`strike_fixes`).
- **Carrion** is placed once per death: `kill` is idempotent, and `finish`
  and `act` only resolve living organisms. Bodies come from the dead
  animal's mass, half for the starved.
- **Condition litters.** The same `litter_condition` scales the
  pregnancy's food cost each tick and the litter at birth. `flex` uses
  `litter_fraction` in [0, 1], and `nourished` is clamped.
- **Gene scaling.** `meat_reliance` is in [0, 1]. The territory temper is
  clamped to 0.25–2. Switching chance is capped at 1. Care (≤ 1) gives at
  most twice the nursing rate.
- **Snapshots.** `before_v25`, `v26` and `v27` guards are applied by
  version on restore and by the old-format writers. Each version's test
  restores its tunables as off.
- **Determinism.** Every rule draw is a hash of ids, a lineage and the day,
  or comes from the organism's own stream. Ties in `beats` and
  `scent_carcass` are broken by index or scan order. No HashMap is
  iterated.
