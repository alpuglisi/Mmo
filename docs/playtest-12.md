# Playtest 12: six AI players, after the playtest-11 fixes (2026-11-19)

Six AI agents played on one server (`--size 128 --seed 67 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) on main at 078e247 (all of round 12:
nest raids honour grace/offline/shield, crowded-map foundings, reinforcements
clear of rival animals, pack cohesion 0.3, heat warning, smoothed-world share,
wild rebound, graced hunting, re-found capped at 10, sleeping-deal pause cap and
no shield while asleep, post-mating shield, change-only mating updates, meeting
point slides only as needed with a note, walk warning, lineage-offer distance and
candidates, fee-free listings, taper on escrow, per-group restore quotes,
Damerau/blend/species name checks, name checks after the price, leak bands,
alert cool-downs and fold detail, split/merge peaks, cause-specific declines,
rally undo, migrate hunter warnings, a new starter design, client and server
Lows). Five players played 52–60 minutes (game days ~5–740); **Rulebender12 ran
only ~13 real minutes (days 16–151)** although its report claims ~55. Full logs:
`docs/playtest-12-logs/`; screenshots: `docs/screenshots/playtest-12/`. The
server stayed up; no page errors or failed requests.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| Herdmaster12 | 20.7–36.6% (1,358–2,051) | Cold live-bearing toxic bright flier again: won era 1 (8.1% mean) and era 2 (25.8%), seasons 3–11, 4 of 6 era-2 niche prizes |
| LateComer12 | 14–17% peak (869–1,000+) | A copy of the flier (carnivory 0.34) on era 2's first day: era 2 #2 (6.3% mean, 16.1% at the end), omnivore niche prize |
| ApexHunter12 | 1.8% (peak 136) | Pack sat ~100 for 400 days; adult sex ratio skewed (31 of 89 female); carnivore niche prize |
| PocketNomad12 | 0.7% (peak 420) | **The new starter** held #2–3 for ~4 years but lost 55–65% each winter; retired at 65 and re-founded |
| GeneBroker12 | 0.6% | 4 trades incl. the first fee-free listing sale; no other player ever listed |
| Rulebender12 | 0.3% | Short run; probed names, cooldowns, leaks, re-found cap |

## Playtest-11 fixes confirmed working

Graced packs hunt unprotected rivals (first bite ends their grace); toxin and
predation deaths in decline alerts; reinforcements land clear of rival animals
and say so; wild recovers after droughts; no territory alert swing in 500 days
(one player still saw one); split/merge alerts correct; rally cancel restores the
territory; migrate warns of hunters in bands; restore quotes per group; heat
warning in the designer; re-found capped at 10; sleeping deals pause and
sleepers aren't shielded; no mating updates while asleep; meeting point slides
with a note; walk warning; lineage offers show `apart`; taper counts escrow;
fee-free listing sale paid in full; swapped letters, blends and species
near-copies refused; free name oracle gone; players_online/apply timings hidden;
phone toasts two lines with tap-to-expand, Log out not clipped, picker in view,
Food layer sprites smaller, Lineage-tab taps correct, share-during-boom note;
landing placement 10–26 tiles from rivals; the new starter far better than
playtest 11's (~40) — #2–3 for four years, peak 420.

## Findings, by priority

### High

1. **The leader's share still runs away — through winter maturation and young-world
   smoothing**: each winter the autumn young become breeding adults while wild adults
   die, so the world's breeding total stays flat and the 120-day smoothing never
   engages (it only acts once its mean has caught up — in a young or just-boomed world
   it sits far below the current size). Herdmaster12: 16.2% → 29.0% (days 338–363)
   and 17.2% → 34.4% (year 4) while its head count fell; shares summed to 100%.
   Suggested: compare against a slowly decaying recent peak (rise fast, fall slow).
2. **The cold toxic bright flier dominates again**: two copies held 44.6% of the world
   and ~94% of player share; every warm herd ended at 0.3–2.6%; a copy founded 360
   days late reached 14% with little management. It also won four of six era-2 niche
   prizes by covering every biome.
3. **The starter starves through every winter sleep** (hibernation 0.5 with fat 0.6
   loses 50–60% each winter, mostly starvation while asleep; sleepers can't be moved
   or fed), though the designer calls it "a safe first lineage"; toxicity didn't stop
   the pack eating 24 sleepers in 24 days.

### Medium

4. **Names**: word-order reversal of player names passes every check ("Master Herd",
   "Hunter Apex", "Bender Rule", "Comer Late", species "Nomad Pocket" — now
   permanent); two neighbour swaps ("Hedrmatser"); blends with a rival lineage name
   ("Apex Drifter").
5. **Territory radar through migrate warnings**: "Destination: it is inside X's
   territory at (26, 111) (radius 14)" and "The way crosses X's territory" for any
   point on the map, no sight filter; the cooldown is per group, so N groups give N
   probes a second.
6. **Pack**: stays ~100 (playtest 11: 250); a lasting adult sex-ratio skew (31 of 89
   female) holds births to ~2 a day; still scatters without a rally even with cohesion
   0.3; one unordered bite ended its grace with 76 days left and the alert gave no
   reason.
7. **Era fairness swung the other way**: founding on an era's first day leaves a late
   joiner ~4× behind on the era mean (6.3% vs 25.8%) while holding half the leader's
   share at the end; the leader's day-1 share decides the era.
8. **Breeders' offered animals die before an answer** (7 of 16 offers failed while
   pending: eaten, starved, old age, winter sleep); no other player listed anything,
   so the market is one-sided; incoming lineage-offer cards say "three quarters of the
   way" though lineage offers meet halfway; outgoing lineage offers show no distance.
9. **restore_pasture delivers ~14.5% less food than quoted, every time** (2,622 → 2,243;
   2,564 → 2,194; the circle's centre shifts a tile); a near-zero restore (2 food) still
   costs the full price with no hint.
10. **Need alerts still flood in a crash** (~one per game day across hungry / dying off /
    thirst / hunted, each with its own cool-down); "being hunted" every 3 days for ~1%
    losses; crash alerts with inconsistent baselines ("278 → 164" then "247 → 131"
    seconds apart; "112 → 30", "65 → 30", "155 → 32"); "dying off slowly" for a 1% dip
    and for a 29% fall; winter-sleep starvation blamed on grazed-bare territory.
11. **Phone hides the reply to the player's own action** (only the 2 newest toasts show;
    a claim's "But…" warning is pushed out by the alerts it triggers); migrations end
    without telling the player (only "last order" on the group card).
12. **Landing toast now shown 0 times** (over-fixed: only in the feed).

### Low

Grace wording stale ("yours can't be ordered to attack theirs" — graced hunting is on)
in the founding reply, designer and protocol comment; exact grace days left for every
graced player in anonymous `/api/world` (incl. a retired lineage); anonymous
`/api/metrics` state_encode_ns reveals connections; listing purchase streams the
seller animal's position and meet_note reveals out-of-sight predators;
`others_on_the_way` not sight-filtered; designer counts all 40 requested founders on a
re-found (only 10 land); designer grace preview 60 days vs founding alert 130; ledger
lacks the escrow payout line (totals.escrow stuck at 12); shop still says "Sales pay out
less a 10% market fee" (listings are fee-free); Market "40% of pairs can conceive" then
"None of the sampled pairs can breed"; retire dialog "Its 901 animals" vs 1,307 alive;
species labelled herbivore at carnivory 0.34; claim cooldown not waived after a bare
territory; claim "supports 152" then "grazed bare" seconds later; claim cooldown
inconsistent between groups; grazed-bare alert for a territory no longer held; migrate
reply mixes route lengths (25 vs 19 tiles); daily summaries stamped a day early; founding
day 18 vs landing alert day 17; species `parent` points at another player's species
(shared starter); offers show no animal age (one died of old age in 30 s); heat figure
ignores body size; Ranks title wraps on phones; ~80% asleep by mid-summer (orders wasted,
unexplained); "Reinforcements: not now" reason only in a tooltip on phones; invisible
group pick for Clear; Merge gives no confirmation; lower-case "group a (main)"; "the
drought" at 0.22; designer "A hunter needs speed, strength or bite" for a fully armed
hunter; possession outlives its socket; repeated group prey notes; "attrition"
unexplained; net vs gross in decline lines; "rises to 100% … (day 15)" reads as an end
date; re-found interbreeding card lists a 0%-fertile species; `limiting: body_plan` at
fertility 1.0; "still crashing" drops the toxin count; float noise in reinforce `at`;
auction days 20 → 62.5; renown piles up (1,464–3,980); retired species vanish from the
World tab with no history line; hidden died-out banner text in the DOM.
