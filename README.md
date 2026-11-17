# Evolution MMO

[![CI](https://github.com/alpuglisi/mmo/actions/workflows/ci.yml/badge.svg)](https://github.com/alpuglisi/mmo/actions/workflows/ci.yml)

A persistent multiplayer world where each player designs a founding organism
and steers its lineage through breeding rather than direct editing. The full
design is in [`docs/game-design.md`](docs/game-design.md).

Where the design's build order stands:

- **Step 1, simulation (done, still being tuned):** the rules engine runs
  headless and its per-organism tick cost is measured; see
  [Benchmark](#benchmark) and [Balance](#balance).
- **Step 2, server (well along):** `evo-server`, the authoritative Rust
  server around the simulation: accounts, founding, WebSocket play with
  binary delta states (protocol 2), directed-mating consent, a renown
  economy with a paged stud marketplace and auctions, ranked seasons,
  several worlds per process, and persistence in files or PostgreSQL; see
  [Server](#server).
- **Step 3, client (well along):** a TypeScript web client (PixiJS, Vite)
  with login and world choice, the founder design editor, the world view
  with creature sprites, the Matings, Market, Genetics, Lineage, Seasons
  and World (chronicle) tabs, a phone layout and a first-session tour; Capacitor wraps
  it for iOS and Android (the Android debug APK builds headless and in CI; neither
  app has run on a device yet); see [Client](#client).

What remains is in [Suggested next steps](#suggested-next-steps).

![World on day 45: player lineages (colored) and the wild ecosystem (pale = herbivores, dark red = predators)](docs/img/world-day45.png)

## Layout

```
crates/
  sim/       evo-sim: the rules engine (library, no I/O)
  sim-cli/   evo-sim binary: run worlds, print reports, benchmark, render PNGs
  server/    evo-server: authoritative game server (Tokio + Axum, WebSockets)
client/      web client: TypeScript, PixiJS 8, Vite (npm)
deploy/      docker compose deployment: evo-server, PostgreSQL, Caddy (HTTPS); see Deployment
Dockerfile   image with evo-server and the built client
docs/
  game-design.md
```

### `evo-sim` modules

| Module | Design-doc section | What it does |
| --- | --- | --- |
| `genome` | Scoring, Genetics | 46 diploid genes (body, behavior, mate preference and seasonal strategy). Every allele carries an origin tag: founders get the designer's ID, mutations get the owner of the organism they appear in, and tags never change. Expression is additive except for a per-gene dominance coefficient `h` (`expressed = mean + h·|a−b|/2`): coloration and fur are partly dominant, camouflage partly recessive. Four hidden load loci carry harmful recessives that arise by mutation, pass with their origin tags (and count for legacy), do nothing in carriers and harm homozygotes, so inbreeding exposes them and outcrossing hides them; `Genome::set_load` plants one in a founder or stud. Seasonal genes and load loci are left out of the genetic distance. A separate **morphology segment** (`morph`, `docs/morphology-contract.md`) holds discrete loci for eye count (1–6), ear count (0/2/4), ear shape, leg count (0/2/4/6/8) and tail shape (small-integer alleles with origin tags; one allele expressed, never an average: the wild type over any other, then the one nearer the default, the lower count on a tie, shapes in a fixed order) and additive loci for eye azimuth and snout length; `tail_present` is derived from `Tail` ≥ 0.05. Founders carry the design's counts exactly. The segment has its own random stream (no draws from the shared ones), its own choice bits in the by-parents snapshot form, and stays out of the phenotype; its alleles count in the legacy tally under `Balance::morph_legacy` (on by default, see [Morphology on by default](#morphology-on-by-default)). Expressed counts and shapes add `min(1, steps/2)` per locus to the genetic distance's numerator (divisor unchanged), and eye and leg counts 0.1 per step to the body-plan gap; equal morphologies add exactly 0. Discrete step mutation (`Balance::morph_step_rate`) runs at the spec's 0.0005 by default (0 in saves from before *Morphology on by default*, where worlds run bit for bit as before these loci). `Organism::morph` (`morph::Morphology`) caches the expressed parameters and derived eye field width at birth; it is the one body plan everything reads (see [Body plan wiring](#body-plan-wiring)): the eye geometry and ear localization (`stats::derive_with`), the body plan effects (`morph_effects`, behind their tunables, on by default), LOD cohort representatives and the server's `look`. |
| `design` | Organism design | Founder designs: thermoregulation → birth type → offspring count → traits, on a 100-point budget. Founders vary around the design. |
| `stats` | Stats layer, Derived physical properties | Phenotype → speed, strength, perception, stealth, metabolism… and height, weight, ground pressure, buoyancy, wing loading, grip. Thermoregulation mismatch (fur on a cold-blooded body blocks basking). |
| `morph_effects` | `docs/morphology.md` §3 (package D) | Body plan → derived effects of ear count and shape, leg count, tail shape and snout length, computed once at birth and folded into stats and body behind the `morph_*` tunables (on by default since [Morphology on by default](#morphology-on-by-default); exactly neutral at the default body). The body plan is the organism's genome `Organism::morph`. See [Body plan effects](#body-plan-effects). |
| `terrain` | Seasons and biomes, Movement and terrain | Noise-generated biomes, latitude/altitude climate, four seasons, day/night. Rivers carved by flow accumulation down a pit-free drainage network to lakes, sea or map edge; depressions fed by more rain than evaporates off their surface fill into lakes (priority flood). Rivers in flood overflow onto their floodplain (low land up to two tiles from the channel): standing water that swimmers cross and that strands and drowns small non-swimmers, drowned plants, then silt that speeds regrowth for weeks. Regional weather on a 16-tile grid: fronts drifting east bring wind, storms (chill, costly open ground, costly and risky flight) and rain or snow; the wind has a direction (prevailing westerly, swinging over days and veering regionally) that speeds or slows flyers and gliders by wing loading; slow dry and wet spells and the seasons drive a per-cell soil wetness that fills and drains shallow lakes, marsh and rivers (droughts dry them out, crowding organisms at the rest) and paces plant regrowth; spring snowmelt swells rivers so fast deep water blocks weak swimmers, and rivers freeze in winter. Canopy browse needs height or climbing, and climbers in canopy are safe from ground hunters. Tiles update chunk by chunk, each chunk once per simulated hour. Hooks: `Map::weather_at`, `Tile::wetness`/`Map::dryness_at`, `Map::drought`/`flood`, `Map::terrain_version`/`chunk_version`; `Map::climate_temp`/`climate_wet` are climate anomalies (0 normally) that catastrophes set. `traverse()` returns Blocked / Passable{speed, energy} from body vs. tile. |
| `pathfinding` | Movement and terrain | Bounded A* over tiles for one body: blocked tiles are avoided, slow ones cost time. Used only when the direct step is blocked (see Movement below). |
| `mating` | Override cost | Root-sum-of-squares preference mismatch, `cost = k·max(0, mismatch−1)²`, outbreeding depression by genetic distance, and sterility between different body plans (thermoregulation, birth mode, body size or diet more than 0.3 apart; `cargo run --release --example distances` prints the founders' matrix). Display coloration is condition-dependent (an honest signal). |
| `group` | Critical-mass group traits | Daily regional carrier frequency → logistic benefit. Implemented: cooperative hunting, warning coloration, herd defense, alarm calls, shared burrows, sentries, communal dens, group navigation, shared caches. |
| `legacy` | Scoring: genetic legacy | Share of origin-tagged alleles among living breeding adults, raw and weighted by offspring raised to adulthood. |
| `species` | Genetics (speciation), Scoring (species founded) | Species detection: every few days, single-linkage clusters of sampled breeding adults that would recognize each other as kin and be fertile together (`mating::is_kin` and `fertility` ≥ 0.5), with stable ids, splits, merges and extinctions. See [Species, eras and achievements](#species-eras-and-achievements). |
| `chronicle` | Scoring (seasonal leaderboards, secondary achievements) | Eras with final legacy rankings and niche leaders, catastrophes (severe winter, megadrought, epidemic, flood year) and prestige-only achievements. |
| `world` | Needs and behavior, everything else | The tick loop: utility-AI decisions (parallel), then movement, grazing, hunting, scavenging, drinking, courtship, pregnancy, eggs and incubation, nursing, growth, death. |
| `world::disease` | Genetics: picky vs flexible | Mating-transmitted disease: pathogens emerge at matings (4× between founding lineages), pass between mates (natural and directed) and between nursing mothers and young, drain the acutely sick's health (dulling their display) and cut fertility; infections end in immunity or lifelong symptomless carriage. Also deaths of homozygotes for harmful recessives. Reports count infections, current sick and carriers, harmful alleles per organism, and `disease` / `recessive` deaths per lineage. |
| `world::seasonal` | Seasons and biomes (strategies) | Seasonal strategies as genes, each switched on above expression 0.5 and stronger the higher it is: hibernation and brumation (fatten in autumn, dormant through the cold with near-zero hunger and thirst, no breeding, den findable; thermoregulation mismatches per the affinity table), migration (summer range ↔ winter range further south, death risk per tile; it leaves in autumn once its summer pasture has nearly stopped growing, `migration_onset`), estivation (dormant through summer heat or a regional drought, `Map::dryness_at`), food caching (raidable hoards for winter), short-term torpor on cold nights, a seasonal coat (winter coat molted in autumn and shed in spring, food per molt; a mismatch on cold-blooded bodies) and timed breeding (a narrow early-spring window with strong drive and better conception, no breeding outside it). |
| `world::par_apply` | Scale and performance (parallel regions) | The apply phase on all cores (`WorldConfig::parallel_apply`, default on; `--sequential-apply` for the old fully sequential apply): each turn split into a parallel movement sweep, a sequential interaction sweep in shuffled order, a parallel needs sweep and a sequential death/birth sweep. See [Parallel apply](#parallel-apply). |
| `world::commands` | Player controls | Directed matings (consent, meeting distance, seasonal slots, override cost), possession, migration orders. |
| `world::groups` | Player controls (group commands) | The rest of the doc's group commands: split a herd or pack into groups (up to 4) with their own orders, merge them back, claim or abandon a territory that a group roams, forages in, walks back to and defends, rally a group at a point for a few days; and group commands as genes: an `obedience` gene sets how strongly each animal heeds them, paid for in tameness. See [Groups and territory](#groups-and-territory). |
| `world::chronicle` | Scoring | The daily hook: samples adults (cohorts included) for species detection, plans, applies and ends catastrophes through existing systems (`Map::climate_temp`, `Map::climate_wet`, infections), closes eras and awards achievements. |
| `world::lod` | Scale and performance (level of detail) | Opt-in (`WorldConfig::lod`, `--lod`): full agents only near focus points (`World::set_focus`) and possessed organisms; elsewhere each 16-tile region holds per-lineage statistical cohorts (counts by age and sex, a genome pool, condition) that eat the region's plants, hunt, breed, die and migrate once a day, and expand back into agents when a focus arrives. Legacy and caps count them. See [Level of detail](#level-of-detail). |
| `vision` | Morphology: eyes (`docs/morphology.md`) | Directional vision behind `Balance::directional_vision` (on by default since [Morphology on by default](#morphology-on-by-default); bit-identical at 0): eye layouts (count, pair azimuth, size; snout occlusion) → coverage, binocular depth field, front and rear blind spots and a 2-bit-per-bin bearing table, computed once at birth (`Traits::vision`); each organism faces its last movement (`Organism::heading`). See [Directional vision](#directional-vision). |
| `world::offline` | Open questions (offline protection) | Softer rules for the last members of a lineage whose owner is logged out: the caller names the protected lineages and exempt organisms (`World::set_offline_protection`); at or below the floor (12 living members) kill chance against them is ×0.25 and starvation and thirst ×0.5, for agents and cohorts. And the home-range form (`World::set_home_ranges`): a claimed territory shields the members of its group inside it, kill chance ×0.5 shared out above 20 residents, predation only, stacking with the core down to ×0.2 at most. Off (and bit-identical) by default. See [Offline protection](#offline-protection). |

### Tick structure

1. **Hourly:** regional weather, egg incubation. **Every tick:** a share of the environment chunks (temperature, snow, ice, water levels, plant regrowth), so each chunk updates once per hour; the water-distance field is refreshed hourly.
2. **Daily:** remove the dead, recompute group traits, take a legacy snapshot, reset mating slots at a season change; then the chronicle: catastrophes, species detection (every 5 days), era records and achievements.
3. **Decide (parallel, read-only):** each organism scores actions by need urgency × its behavior genes.
4. **Apply (shuffled order, mostly parallel):** act on the decision, update needs, resolve death and birth. Movement and needs run on all cores, interactions in order (see [Parallel apply](#parallel-apply)).

Every random draw comes from a `(seed, stream, tick, organism)` stream, so a
run reproduces exactly from its seed regardless of thread count (there are
tests for this, with the parallel and the sequential apply).

### Movement

Organisms steer straight at their target. When the direct step is blocked
(lake, cliff, a concave shore), the walker plans a route with A* for its own
body (`terrain::traverse`: swimmers cross deep water, climbers cliffs, ice
holds bodies light enough for its thickness), keeps it on the organism, and
follows it until it arrives, the target moves away from the route's goal,
it is pushed off the route, or the tile ahead closes (ice melts). Searches
expand at most `path_nodes` tiles (a search that runs out heads for the tile
it got closest to); a failed search isn't repeated for the same target until
the terrain changes on the hour, and a target out in water with no shore
next to it fails at once. Planning happens in the apply phase, on all cores
with a search buffer per thread (a search doesn't depend on what the buffer
held), so runs stay thread-count independent. Short sprints (fleeing, the last
lunge at prey) stay greedy. `--set path_nodes=0` restores greedy steering
exactly.

`cargo run --release --example stuck -- 120 1 2 3` measures how often
organisms are stuck: heading for the same reachable spot for 24 ticks
without getting 0.5 tiles closer. Over a year on three 128×128 seeds:

| | Stuck (share of ticks heading somewhere) | Worst lineages | Not moving at all |
| --- | --- | --- | --- |
| Greedy steering | 0.24% | burrowers 1.34%, voles 0.94% (one seed each) | 0.10% |
| Pathfinding | 0.03% | burrowers 0.21%, voles 0.10% (one seed each) | 0.01% |

Most of the remaining "stuck" ticks are the metric's blind spots (a detour
around a lake doesn't shorten the straight-line distance; targets thrash
between nearby spots). The benchmark runs about 0.75 searches per tick, and
its cost is unchanged within noise (callgrind: +0.2% instructions in total).

### Groups and territory

The design doc's group commands ("order migration, claim or abandon
territory, split a group"; `world::groups`, with the migration orders in
`world::commands`). The doc gives no numbers, so these are this
implementation's choices, kept in constants at the top of `groups.rs` (not
`Balance` fields: they tune player levers, not the ecosystem the sweeps
score).

- **Groups.** Every organism carries a group id (`Organism::group`, 0 the
  main group). `split_group` divides a group in two: the organisms named,
  or by default the half on the far side of the group's widest spread (x or
  y, by variance; ties by id; the smaller half when the count is odd). The
  new group takes the lowest free id (at most 4 groups) and starts with no
  orders; `merge_groups` joins two again (the merged group's orders end).
  Young are born into their mother's group. Members of a player's other
  groups no longer count as kin for cohesion (`Action::Group`), herd size
  and company, so the halves don't drift back together (kinship for mating
  and sentries is unchanged). Cohorts of abstract regions (level of detail)
  belong to no group: agents expanded from them start in the main group.
- **Per-group orders.** Migration orders are kept per (player, group).
  `migrate` without a group reaches every group, as before; with one, only
  it. A group has at most one of a migration order, a territory and a
  rally: the newest order replaces the others.
- **Territory** (`claim_territory`): a circle a group holds. Its radius is
  kept between 3 tiles and `4 + 2·√adults` (at most 24): a few animals
  can't hold a valley. Members roam and forage inside it (wander targets
  are pulled in, food searches and remembered patches outside are
  dropped, `Map` sampling unchanged), walk back when outside
  (`Action::Patrol`, utility 0.55 like an ordered migration), and adults
  chase the nearest stranger they see inside it (`Action::Defend`: not the
  player's, not kin, and not a hunter that could beat them; utility
  `0.7 × (0.3 + 0.7 × max(aggression, boldness)) × (1 − hunger)`). A
  stranger inside a territory sees a defender coming at it as a threat (as
  an armed hunter of the defender's power would be) and flees.
- **Costs** (the doc: every lever has a price; holding territory is what
  year-round residents do): the group lives on the food inside the circle
  until hunger passes 80 at the default obedience (then it forages anywhere, "hungry organisms
  prioritize food over everything except immediate danger"), chasing is
  at a sprint, and a group can claim only once every half day
  (`claim_cooldown`). Seasonal migration (a gene) still outranks the
  territory, and danger and thirst outrank both as usual.
- **Rally** (`rally_group`, `cancel_rally`): gather a group at a point.
  The circle it gathers in has radius `2 + √members`, about π tiles of
  ground per member, up to 24 tiles (the largest territory's cap, reached
  at 484 members), so a big herd has room to graze and a rally never holds
  a group tighter than the largest circle it could claim (the cap was 12,
  reached at 100 members; see the measurement below). Members outside it walk to the point
  (`Action::Rally`, utility 0.55 × heed, like an ordered migration and
  walking back into a territory, so obedience weighs it the same way);
  inside it they roam and forage within the circle, exactly as in a
  territory (and stray to forage at the same obedience-dependent hunger),
  but nobody is chased off and there is no cooldown. It stands until it is
  cancelled, replaced by another order (a migration, a claim, a new rally;
  a split or merge ends it like the other orders) or lapses after
  `RALLY_DAYS` (3 days: re-issue it to keep the group there). Lapsed
  rallies are dropped before decide each tick. Saved (snapshot version
  15; older saves load with no rally standing, `serde_util::before_v15`).
- **Obedience: group commands as genes** (the doc's open question, "keep
  as player levers, or make genetic?": both). The commands stay player
  levers; how strongly each animal heeds them is an instinct gene,
  `obedience` (default 0.5, free like the other instincts, appended as the
  last gene and left out of `Genome::distance` so kin and fertility
  thresholds don't move). `heed = 0.5 + obedience` multiplies the utility
  of an ordered migration (0.55), of walking back into a territory and of
  seeing off an intruder, so at the default the commands weigh exactly what
  they were tuned at; at 1 an order outranks moderate hunger and thirst
  (0.825), at 0 it is followed only when nothing presses (0.275). A
  territory holder strays to forage at hunger `0.8 + 0.3 × (obedience −
  0.5)` (0.65 to 0.95). So an obedient group follows orders through hunger,
  and starves under bad ones, while a headstrong one breaks off to eat. The
  price, paid with or without orders, is **tameness**: every animal's urge
  to flee a threat is weighted by `1 − tameness × (obedience − 0.5)`
  (`Balance::tameness` 0.4: the most obedient flee at 0.8×, the most
  headstrong at 1.2×), as tamer animals are in domestication. A lineage
  whose player gives good orders can breed for obedience; one left alone
  pays for it in predation. The obedience locus draws from a stream of its
  own, seeded by the genome's other alleles (`Genome::own_rng`), so
  founding, inheritance and mutation of every other gene are bit for bit
  as before: with `tameness` 0 a world steps exactly as it did without the
  gene. The server sends each group's mean obedience (`GroupView::obedience`)
  and the client's group card names it. Measured against `tameness` 0 (the
  old behaviour): `compare` on seeds 101–148 (96×96, 120 days) scores 11.77
  against 11.66 (+0.11 ± 0.15, not significant; red fox survival 98% vs
  92%), and on smoke-sized worlds (64×64, 60 days, the same 48 seeds) 11.60
  against 11.44 (+0.16 ± 0.16); the 12-seed sweeps score 11.76 from seed 31
  (11.60 with `tameness` 0, the baseline) and 11.72 from 55 (baseline
  11.73). New tunable: `tameness`.
- **What obedience buys with real orders** (`cargo run --release --example
  obedience -- orders 120 $(seq 101 124)`): the standard world (96×96, 120
  days, seeds 101–124) with the herd grazers founded at obedience 0.2 or
  0.8 (nothing else differs: the locus has its own stream), paired by seed,
  under scripted player policies: `rally` (every 2 days, rally at the
  disc near the herd with the most plant food, halved without water),
  `territory` (claim that spot at the largest radius), `pulse` (that rally
  every 6 days, cancelled a day later), `bad-rally` (rally at the barest
  spot) and `none`. High − low, mean ± 95% CI over the 24 seeds:

  | Policy | Herd at day 120 (low / high) | High − low, herd at day 120 | High − low, mean herd | Share inside the order's circle (low / high) | High − low, starved / killed |
  | --- | --- | --- | --- | --- | --- |
  | none | 442 / 446 | +3.9 ± 21.8 | −0.4 ± 6.8 | – | −1.5 ± 9.0 / +2.4 ± 9.1 |
  | pulse | 444 / 478 | +34.1 ± 34.5 | +5.4 ± 11.0 | 0.70 / 0.74 | −13.0 ± 22.9 / +7.6 ± 9.7 |
  | rally | 309 / 282 | −27.1 ± 46.6 | **−11.2 ± 9.4** | 0.88 / 0.91 | +6.1 ± 57.1 / −0.1 ± 7.3 |
  | territory | 391 / 369 | −21.3 ± 51.0 | **−14.9 ± 8.5** | 0.94 / 0.96 | −24.7 ± 50.4 / −2.1 ± 2.3 |
  | bad-rally | 50 / 6 | **−44.1 ± 27.2** | **−38.3 ± 15.2** | 0.54 / 0.65 | **−70.1 ± 30.9** / **+35.0 ± 15.4** |

  (Rows for the rally policies are with the 24-tile rally cap; the 12-tile
  cap's are below.) Obedience does what it was built to do: obedient herds
  keep to the order (+0.02 ± 0.01 of the herd inside a rally circle, +0.12
  ± 0.05 under bad orders, +0.02 ± 0.01 inside a territory, which even the
  headstrong mostly keep to), and they follow bad orders to ruin (a barren
  rally point costs the obedient herd 44 more animals of ~50, the dead
  taken by predators out in the open where the headstrong would have
  scattered to eat). But no scripted policy here made obedience pay: with
  good-looking orders the difference is within noise or against it (an
  obedient herd under a standing rally or holding a territory averaged 11
  and 15 fewer animals over the run: it keeps to a circle it has grazed
  down). The reason is that the orders themselves cost this herd: against
  no orders, a standing rally ends it at 309 instead of 442 (−133 ± 99 at
  0.2, −164 ± 84 at 0.8), a standing territory at 391 (−51 ± 73), and the
  pulse is neutral (+3 ± 54 at 0.2, +33 ± 52 at 0.8). The herd grazers are
  already near their carrying capacity on their own, and every order
  confines them. So obedience is worth breeding for
  where a player has short, specific uses (gathering a scattered herd,
  pulling it out of a hunting ground), and a headstrong herd is the safer
  default against careless orders. Nothing was retuned: the order costs
  are the design's ("every lever has a price"), not a bug.
- **Rally circle for big groups** (next-steps item 7: should a rally hold
  a large group less tightly?). With the cap at 12 tiles a 400-head herd
  had ~1.1 tiles of ground per head inside the circle; the cap is now the
  largest territory's, 24 tiles (`RALLY_MAX_RADIUS = TERRITORY_MAX_RADIUS`;
  the `2 + √members` slope, so every group under 100 members, is
  unchanged, and nothing else differs: no new tunable, no snapshot change,
  and worlds without rallies step exactly as before). Measured with the
  same command and seeds (101–124), the new cap against the old, paired
  by (seed, obedience), mean ± 95% CI:

  | Policy | Obedience | Herd at day 120 (12 → 24) | New − old, herd at day 120 | New − old, mean herd | New − old, share inside |
  | --- | --- | --- | --- | --- | --- |
  | rally | 0.2 | 190 → 309 | **+119.0 ± 95.8** | **+31.1 ± 17.5** | +0.08 ± 0.02 |
  | rally | 0.8 | 217 → 282 | +64.8 ± 89.9 | **+19.0 ± 16.0** | +0.04 ± 0.02 |
  | pulse | 0.2 | 438 → 444 | +5.9 ± 44.5 | +0.3 ± 9.4 | +0.15 ± 0.04 |
  | pulse | 0.8 | 426 → 478 | **+52.4 ± 51.0** | +6.9 ± 7.4 | +0.14 ± 0.03 |
  | bad-rally | 0.2 | 43 → 50 | +7.3 ± 16.4 | +4.1 ± 4.6 | +0.03 ± 0.02 |
  | bad-rally | 0.8 | 5 → 6 | +0.5 ± 2.4 | +2.5 ± 3.4 | +0.01 ± 0.01 |

  Both obedience levels together, a standing rally now leaves +92 ± 88
  more animals at day 120 and +25 ± 16 more on average over the run, with
  more of the herd inside the (larger) circle, and a short pulse is no
  worse (+29 ± 41 at day 120). A standing rally still costs a herd at its
  carrying capacity (above), as any confinement does, but it is no longer
  a trap; a barren rally point is as ruinous as before (the circle is
  bigger but still barren). The 12-tile rows of the table above were:
  rally 190 / 217, high − low +27.1 ± 62.1 at day 120 and +0.9 ± 10.2 on
  average, inside 0.80 / 0.87; pulse 438 / 426, −12.4 ± 46.0 and −1.2 ±
  11.9, inside 0.55 / 0.60; bad-rally 43 / 5, −37.3 ± 18.3 and −36.7 ±
  12.9, inside 0.51 / 0.64. `PER_RUN=1` makes the example print every
  run, to pair two builds like this.
- **Does obedience drift down without orders?** (`-- drift 360 $(seq 101
  112)`, the standard world for a year with `tameness` 0.4 against 0, which
  makes obedience neutral, same seeds): no measurable drift yet. Across all
  lineage-seeds (73) the mean obedience is 0.500 in both (tameness −
  control +0.000 ± 0.004). Only the meadow voles, with the shortest
  generations, show a small decline (0.490 against 0.502, −0.012 ± 0.008).
  Every founder starts at the design default and new variation arrives
  only by mutation, so selection has little to work with in a year;
  lineages that never get orders lose obedience slowly, on the timescale of
  many generations, not within a season.
- **Determinism.** The commands draw no randomness and are applied where
  the server applies every command (between sub-steps, in arrival order).
  Every lookup sits behind an empty map and every organism is in group 0
  until a player splits, so a world without them steps bit for bit as
  before (the 12-seed sweeps still score 11.54 on seeds from 31 and 11.41 from 55; the tests
  `group_commands_are_deterministic_and_unused_ones_change_nothing`, and the
  snapshot round trip with a split herd, one half holding ground).

## Running

Needs a recent stable Rust.

```sh
cargo test --release                                  # unit and world tests
cargo run --release --bin evo-sim -- designs          # sample designs and point costs
cargo run --release --bin evo-sim -- run --days 120 --report 10 --snapshot out/world
cargo run --release --bin evo-sim -- bench --scale 4 --days 3
cargo run --release --bin evo-sim -- bench --size 256 --scale 16 --days 12 --threads 1,2,4 --compare-apply
cargo run --release --bin evo-sim -- sweep --trials 16 --seeds 3 --rounds 2 --out sweep.csv
cargo run --release --bin evo-sim -- sweep --trials 24 --seeds 12 --screen-days 60 --screen-seeds 4 --eta 3   # screened sweep (44% of the cost)
cargo run --release --bin evo-sim -- compare --seed 31 --seeds 24 --set larval_competition=18 vs defaults   # is A better than B?
cargo run --release --bin evo-sim -- compare --seed 31 --adaptive --set larval_competition=18 vs defaults   # add seeds until it resolves
cargo run --release --bin evo-sim -- compare --seed 101 --adaptive --tolerance 0 --migrants vs --migrant-gene migration=0   # what migrating buys the caribou
cargo run --release --bin evo-sim -- rankcheck --trials 24 --seeds 12 --spread 0.25 --out rc.csv   # do screen ranks predict 120-day ranks? noise or screen error?
cargo run --release --bin evo-sim -- rankcheck --from rc.csv   # re-analyze without simulating
cargo run --release --bin evo-sim -- run --set sprint=2.0 --set crowding=0.05   # try a balance tweak
cargo run --release --example trace -- 3 200          # follow one fox (lineage 3)
cargo run --release --example stuck -- 120 1 2        # how often organisms get stuck
cargo run --release --example calibrate -- tournament --worlds 20   # point-cost check (CSV)
cargo run --release --example deaths -- 1,5 120 31 32 33   # who kills antelope and herd grazers, and when
cargo run --release --example predators -- 120 1 2 3     # pack hunters vs their prey: booms, kills by prey rarity
cargo run --release --example obedience -- orders 120 101 102   # what obedience buys a herd with scripted orders
cargo run --release --example obedience -- drift 360 101 102    # does obedience drift down without orders?
cargo run --release --bin evo-sim -- sweep --trials 1 --seeds 12 --rounds 1 --seed 31 --migrants   # with the migrating caribou
cargo run --release --bin evo-sim -- run --size 256 --focus 64,64 --focus 192,192   # level of detail
cargo run --release --example lod -- 256 90 1 64,64 192,192   # LOD vs full simulation
cargo run --release -p evo-sim --features serde --example highpop -- bench /tmp/hp/day0050.snap   # tick cost at 60–70k (after `highpop -- warm /tmp/hp`)
cargo run --release --bin evo-sim -- run --days 361 --report 30 --catastrophes --catastrophe severe_winter@210   # eras, species, a catastrophe
cargo run --release --bin evo-sim -- genetics --out out/genetics   # population-genetics checks, quick (~5 s)
cargo run --release --bin evo-sim -- genetics --long --years 15 --out out/genetics   # full validation (~35 min)
```

`run` founds a wild ecosystem (voles, antelope, newts, foxes) plus four sample
player lineages (pack hunters, herd grazers, toxic frogs, burrowing
omnivores), then prints population, legacy share, deaths by cause, active
group traits, gene drift per lineage, and the chronicle: species (estimated
adults and dominant allele owner), catastrophes, the era clock, finished
eras' rankings and achievements. `--catastrophes` turns on random
catastrophes, `--catastrophe KIND@DAY` forces one (`severe_winter`,
`megadrought`, `epidemic`, `flood_year`), `--era-years N` sets the era length.

### Species, eras and achievements

The design doc's supporting scoring rules (`species`, `chronicle`,
`world::chronicle`; settings in `WorldConfig::scoring`). They are records
only: the legacy score is unchanged, and with no catastrophe a world runs
exactly as before (detection draws no random numbers and changes nothing).

- **Species.** There is no species object; kinship and fertility are
  decided pair by pair. Every `species_interval_days` (5) the world samples
  its breeding adults, stratified by (founding lineage, 16-tile region), up
  to 8 evenly spaced members per stratum and about 1200 in all, cohorts of
  level-of-detail regions being strata of their own (their pool genomes
  stand for their adults). Two samples are linked when they would
  recognize each other as kin and be fertile together (fertility ≥ 0.5); a
  species is a connected component (single linkage, so a cline of
  intergrading forms is one species). Each sample stands for its stratum's
  adults, so species sizes are estimates. Cost: the clustering compares
  pairs only across components and most cross-species pairs fail on the four
  body-plan genes: 6.7 ms per detection at 13.5k organisms.
- **Stable ids.** Each species keeps 12 representative genomes
  (farthest-point picks, covering every branch). A new cluster continues the
  old species it overlaps most (samples linked to a representative); when
  two old species continue as one cluster the larger overlap keeps its id
  and the other is recorded as merged. A cluster that overlaps an old
  species without being its continuation is a **split** (parent: that
  species); one with no overlap is a split of its main lineage's species if
  that lives on, else a **founding** (a new design). New species need 4
  samples; a species missed by 2 detections in a row is extinct (dated to
  its last detection). Names come from the main founding lineage ("Meadow
  vole", then "Meadow vole (2)").
- **Species founded (credit rule).** A split is credited to the player whose
  origin-tagged alleles hold the plurality among the new species' sampled
  adults: the legacy measure, so the genes that make up the new species earn
  it, whoever owns the organisms. A wild plurality credits nobody. The
  species present at the first detection and foundings are not speciation
  and earn nothing. At the start the presets form 6 species from 8 lineages
  (antelope and herd grazers are one kind, and fox and pack hunters
  interbreed through founders that are kin across the two designs).
- **Eras.** `era_years` (3) years each. When one ends, its final legacy
  ranking (the day-boundary snapshot) is recorded with the niche leaders:
  the largest allele share among breeding adults of each diet niche
  (herbivore < 0.3 carnivory ≤ omnivore < 0.6 ≤ carnivore) and each biome
  (where they stand; water biomes together), for niches with at least 20
  adults.
- **Catastrophes**, on existing systems: a **severe winter** (every tile
  10 °C colder for a season from the start of winter, `Map::climate_temp`;
  hibernation thresholds follow), a **megadrought** (rain bias −1.5 for a
  summer and half an autumn, `Map::climate_wet`: soil wetness, lakes, marsh,
  streams and plant growth follow), a **flood year** (rain bias +1.2 for
  three quarters of a spring: soaked soil, swollen lakes and marsh) and an
  **epidemic** (15% of susceptible agents infected with the
  mating-transmitted disease at onset, 2% more a day for half a season;
  cohorts are not affected). Severity scales all of them. **Frequency:** off
  by default in `WorldConfig` (so `sweep`'s 120-day validation and every
  existing run are unchanged; the validation sweep still scores 11.12); with
  `catastrophes` on (the server's default, `--no-catastrophes` to disable),
  each year from the second (`catastrophe_first_year` = 1) has a 40% chance
  of one, drawn from its own random stream, never two at once.
  `World::schedule_catastrophe` forces one.
- **Achievements** (prestige only): *species founded* (credited split);
  *lineage longevity* (a player lineage alive each whole year since it was
  first seen); *niche dominance* (a niche leader with at least 25% of its
  alleles at an era's end, wild leaders earn nothing); *survived a
  catastrophe* (a lineage alive at its start and at its end).

A 360-day run (seed 1, 128×128, `--catastrophes --catastrophe
severe_winter@210`): 6 species throughout, no splits or extinctions in three
years (speciation needs isolation over far longer); no random catastrophe
was drawn in years 2–3; the forced severe winter (days 211–240) took the
world from 3960 to 2661 organisms and all four player lineages survived it;
era 1 ended with wild 46.0%, burrowing omnivores 29.1%, toxic frogs 23.6%,
and the frogs and the burrowers each led two niches. A detection over 868
samples of 13.5k organisms (256×256, scale 10) takes 6.7 ms, every 5 days.

## Continuous integration

`.github/workflows/ci.yml` runs on every push and pull request, in parallel
jobs (the Android emulator job waits for the Android one):

| Job | What it runs |
| --- | --- |
| Rust | `cargo fmt --all --check` (`rustfmt.toml`: width 120), `cargo build --release --workspace`, `cargo test --release --workspace` with a `postgres:16` service container (`DATABASE_URL` set, so the persistence tests also run against `PgStore`; `EVO_REQUIRE_DATABASE=1` makes them fail rather than skip if it goes missing), `cargo clippy --release --all-targets -- -D warnings`, and clippy on `evo-server --no-default-features` (no sqlx) (stable toolchain, cargo cache) |
| Balance smoke check | `scripts/balance-smoke.sh`: `evo-sim sweep --size 64 --trials 1 --seeds 3 --rounds 1 --days 60` with the default `Balance`, failing if fewer than 14 of the 24 lineage-worlds (8 lineages × 3 seeds) still hold 4 members with 2 adults, or the score drops below 7 of 12. The current rules make 23 of 24 and 11.54 (with the morphology on by default), so only a rule change that wrecks coexistence trips it; thresholds are env-overridable (`MIN_SURVIVORS`, `MIN_SCORE`, `SEEDS`, `DAYS`, `SIZE`). |
| Client | `npm ci`, `npm run typecheck`, `npm test`, `npm run build` |
| Android app | builds the client, generates the Capacitor project (`npx cap add android`), installs JDK 21 and the Android SDK (`android-actions/setup-android`: platform-tools, platform 36, build-tools 35.0.0), runs `./gradlew assembleDebug` and uploads `app-debug.apk` as the `evolution-mmo-debug-apk` artifact (14 days); then a second, plain-http build (`EVO_CAP_CLEARTEXT=1 npx cap sync android`, checked for `usesCleartextTraffic="true"` in the merged manifest) as `evolution-mmo-debug-apk-cleartext` for the emulator job |
| Android emulator | after the Android job: enables KVM (GitHub's udev rule), builds `evo-server`, boots an API 34 `google_apis` x86_64 emulator (`reactivecircus/android-emulator-runner`, Pixel 6 profile, SwiftShader GPU; the AVD and a booted snapshot are cached) and runs `client/e2e/android-emulator.sh` on the cleartext APK: login, register, founder design, map and state frames, 25 s in the background and back with the socket recovered (see "Mobile apps" in [Client](#client)). Screenshots, logcat, the server log and `report.json` are uploaded as `android-emulator-smoke` (always) |
| iOS app | on `macos-latest` (45 min limit): builds the client, generates the Capacitor project (`npx cap add ios`, Swift Package Manager), builds it for the simulator unsigned (`client/scripts/ios-build-sim.sh`: `xcodebuild -project ios/App/App.xcodeproj -scheme App -configuration Debug -sdk iphonesimulator CODE_SIGNING_ALLOWED=NO`), checks its `Info.plist` has no App Transport Security exception and uploads `App.app` zipped as `evolution-mmo-ios-simulator-app` (14 days); then a plain-http build (`EVO_CAP_CLEARTEXT=1 npx cap sync ios`, checked for `NSAllowsArbitraryLoadsInWebContent`), builds `evo-server`, installs Appium 3.9.0 with the XCUITest driver 12.22.7 and runs `client/e2e/ios-simulator.sh` on it: the Android scenario (login, register, founder design, map and state frames, home button for 25 s and back with the socket recovered) in an iPhone simulator, plus the server's `/api/metrics` showing the app's socket and state frames. Cargo, the Swift packages, the Appium driver and WebDriverAgent's build are cached. Screenshots, the app's log, Appium's and the server's logs, `metrics.json` and `report.json` are uploaded as `ios-simulator-smoke` (always) |
| End-to-end | builds `evo-server` and the client, installs Playwright and Chromium (CI only; developer machines have them pre-installed), runs `npm run e2e` and uploads the screenshots as an artifact when it fails |
| Docker image and deployment | `hadolint` on the `Dockerfile`, `shellcheck` on `scripts/deploy-smoke.sh`, `docker compose config` on `deploy/`, builds the image (Buildx, GitHub Actions cache, not pushed) and runs `scripts/deploy-smoke.sh`: the compose stack (evo-server, PostgreSQL, Caddy for `localhost`), health endpoint, client page, http → https redirect, registration, a WebSocket upgrade through Caddy, per-client rate limits behind Caddy (`--trusted-proxy`), an account surviving a restart in PostgreSQL, and a non-root image user (see [Deployment](#deployment)) |

Run the same checks locally before pushing:

```sh
cargo build --release && cargo test --release --workspace
cargo fmt --all --check
cargo clippy --release --all-targets -- -D warnings
cargo clippy --release -p evo-server --no-default-features --all-targets -- -D warnings
# PostgreSQL runs of the persistence tests (skipped without DATABASE_URL):
DATABASE_URL=postgres://evo:evo@localhost/evo_test cargo test --release -p evo-server --test persistence
scripts/balance-smoke.sh
(cd client && npm ci && npm run typecheck && npm test && npm run build && npm run e2e)
```

**Determinism in tests.** The server integration tests that need exact
timing use a manual world clock: `ServerConfig::manual()` makes the world
thread tick only on `RunningServer::step(n)` (which pushes state once at the
end) and apply commands as they arrive, so nothing races the 20 ms fast-mode
ticker (`directed_mating_consent_round_trip` runs this way). The browser
smoke test waits on explicit state conditions with generous timeouts rather
than fixed sleeps, and retries the steps the living world can legitimately
interrupt (a click landing on an organism that just moved there, a
possessed organism breaking off its order to drink or flee, a picked mate
dying, conceiving first or its agreement expiring at a season change), but
never loosens the assertions themselves: the directed mating between two
same-design players must complete and conceive.

## Server

`crates/server` (binary `evo-server`) is build step 2: the authoritative
server. Clients only display state and send commands.

```sh
cargo run --release --bin evo-server                          # 128×128 world, 1 tick/s, data in ./data
cargo run --release --bin evo-server -- --fast --memory --size 64   # 20 ms ticks, nothing saved
cargo run --release --bin evo-server -- --bind 0.0.0.0:8080 --data /var/lib/evo --seed 7 --tick-ms 500
cargo run --release --bin evo-server -- --database-url postgres://evo:evo@localhost/evo   # PostgreSQL (or DATABASE_URL=...)
cargo test -p evo-server                                      # unit + end-to-end tests (64×64 worlds)
```

Other flags: `--no-wild` (no wild ecosystem), `--days-per-year N` (short
years for demos), `--state-ms` (state push interval), `--static DIR` (serve
the built web client from `DIR`, e.g. `client/dist`, on every path the API
doesn't use), `--session-idle-min N` / `--session-max-hours N` (session
expiry, default 60 min idle and 24 h), `--no-rate-limits`, `--lod` (level
of detail, below), `--no-catastrophes` (catastrophes are on by default in
the game, never in the first year), `--season-days N` (length of a ranked
season, default 60; see [Ranked seasons](#ranked-seasons)),
`--offline-protection-hours H` (longest protection after a logout, default
12, 0 turns it off; see [Offline protection](#offline-protection)),
`--home-range-hours H` (the home range's full-strength hours after a logout,
default 12, then fading over 2H; 0 turns it off), `--fresh` (ignore the saved world: new
world, stored designs re-founded; see [Persistence](#persistence)), `--no-push`,
`--vapid-private-key KEY` / `--vapid-subject mailto:...` (or `EVO_VAPID_PRIVATE_KEY` /
`EVO_VAPID_SUBJECT`), `--push-per-hour N` and `--push-allow-any-endpoint` (see
[Push notifications](#push-notifications)), `--fcm-service-account FILE` and
`--apns-key FILE --apns-key-id ID --apns-team-id ID [--apns-topic BUNDLE_ID]
[--apns-sandbox]` (or `EVO_FCM_SERVICE_ACCOUNT`, `EVO_APNS_*`: native push in
the apps, off without them; see [Native push in the apps](#native-push-in-the-apps)), `--hash-concurrency N` /
`--hash-queue N` (or `EVO_HASH_CONCURRENCY` / `EVO_HASH_QUEUE`: password
hashes run at once, default half the cores and at least 1, and requests
allowed to wait for one, default 16 per concurrent hash; see Accounts under
[Architecture](#architecture)), `--trusted-proxy ADDR[/BITS][,...]` (or
`EVO_TRUSTED_PROXIES`: reverse proxies, by address or CIDR network, whose
`X-Forwarded-For` gives the client address for the per-client rate limits;
the last entry that is not itself a trusted proxy counts, and the header is
ignored from any other peer).
`RUST_LOG=debug` for more logging. SIGINT (Ctrl-C) or SIGTERM stops the
server gracefully: it stops serving, saves every world (in parallel) and
waits for the store to finish writing.

### Several worlds

One process can host several worlds (shards), so a deployment grows by
adding worlds rather than by making one world bigger. Without a world list
it hosts one world, `main`, exactly as before.

```sh
evo-server --world main,name=Mainland --world isle,seed=7,size=96,name=The Isle
EVO_WORLDS="main;isle,seed=7,size=96,name=The Isle" evo-server
evo-server --worlds-file worlds.json   # [{"id": "main"}, {"id": "isle", "seed": 7, "size": 96, "name": "The Isle"}]
```

A world is `ID[,seed=N][,size=N][,name=TEXT]` (the name runs to the end, so
it may contain commas). Ids are 1–32 of `[a-z0-9_-]` and unique; a missing
seed or size comes from `--seed`/`--size`, and every other setting (`--no-wild`,
`--days-per-year`, `--lod`, tick rates, `--fresh`) applies to all worlds.
The first world is the default. `--world`/`--worlds-file` replace
`EVO_WORLDS`; at most 64 worlds (`worlds.rs`).

- **Each world is a full stack**: its own world thread (`world-<id>`),
  saver thread, store task, published summary, `Metrics` and `NetStats`.
  Worlds share only the Tokio runtime (HTTP and WebSocket handlers),
  the sim thread pool (rayon's global pool, one `sim-<n>` thread per core,
  which runs every world's parallel decide and apply), accounts and
  sessions, the rate limiters, the per-player connection cap (8 over all
  worlds) and the store. Every world is loaded before any starts, so a
  world that can't restore stops the start-up. The worlds' clocks are
  staggered: world `i` of `n` ticks `i/n` of a tick after the first
  (`runner::world_phase`), so their ticks don't all land on the shared
  cores at once (see [Several worlds: capacity](#several-worlds-capacity)).
- **Choosing a world**: every per-world route takes `?world=<id>`
  (default: the first world), the WebSocket too (`/ws?token=…&world=isle`);
  an unknown id is `404 unknown_world` with the list of ids.
  `GET /api/worlds` lists them for the start screen.
- **What is global and what is per world.** Global: the account (name,
  password, player id), login sessions, and settings (alert preferences).
  Per world: everything in the game and its economy: the founder design and
  lineage (a player founds once *per world*), organisms, mating offers and
  agreements, renown balances and the ledger, the stud market and its auctions, the
  leaderboards and daily history, weather-season boards and ranked seasons
  (`/api/seasons*`), eras, species and achievements,
  saved games and terrain. Reasons: one identity and one set of
  preferences is what a player expects across shards, while a shared
  economy would let renown earned in a quiet world buy studs in a busy one,
  couple worlds' persistence (one world's `--fresh` refund or crash replay
  would touch another's balances) and need cross-thread locking on every
  payment. Keeping the economy inside the world thread keeps payments
  single-threaded and exact, as they were with one world.

### Architecture

```
HTTP / WS handlers (Tokio, Axum) ──Request──▶ world thread (owns Game + World)
        ▲                       bounded queue        │ tick loop, fixed rate
        └──── per-connection queue ◀── results, events, state pushes
                                    ◀── watch: world summary + leaderboard
                                         │ daily: Capture (a clone of the game)
                                         ▼
                                   saver thread (bincode + LZ4, deflate)
                                         │
                       store task ◀── daily records, finished trades, saved games
```

- **One world thread** owns the `World` exclusively (no locks around it).
  Handlers send `Request`s over a bounded channel (a full queue answers
  `busy`). Each tick it waits for the deadline (default 1 s, `--tick-ms`),
  answering reads at once (connect, map snapshot) and queueing world changes
  (commands, foundings); then applies the queue **in arrival order**, calls
  `World::step`, runs the game bookkeeping, publishes a summary and pushes
  state. The wait is split into `possess_substeps` (4) parts; at each inner
  boundary the queue is applied too and possessed organisms take a movement
  sub-step (below). A run is reproducible given the same commands in the
  same sub-steps of the same ticks.
- **Possessed organisms tick faster** (the design doc's "slow world clock;
  possessed organisms tick faster"). The world clock stays at sim rate, but
  between ticks every possessed organism following a move order takes
  `possess_substeps - 1` movement sub-steps (`World::substep`), each a
  quarter of a tick's movement, the tick's own move being the last. A move
  order therefore takes effect within a quarter tick (250 ms at the default
  clock) instead of up to a whole tick, and the owner's connections get a
  fresh `state` after each sub-step that moved one of their possessed
  organisms (everyone else keeps the normal state interval), so steering
  looks smooth. Only movement is sub-stepped: decisions, needs, eating,
  fighting and perception stay on the world clock, and the total movement
  per tick is unchanged, so possession gains no speed beyond its small 10%
  bonus. Sub-steps draw no randomness and touch only possessed organisms:
  a world without them is bit-for-bit what it was, and `World::step` takes
  any sub-steps the caller skipped first, so the CLI, tests and a server
  give the same world for the same orders. Why movement sub-steps rather
  than a faster clock for everything or client-side prediction: a faster
  world clock multiplies the cost of every organism to help a handful;
  prediction alone would show movement the server hasn't made (and gets
  terrain and paths wrong); sub-stepping only possessed movement costs a few
  microseconds per sub-step and keeps the server authoritative. The client
  adds a bounded prediction on top (see [Client](#client), "Smooth
  movement"), for which the possessed organism's state carries its `order`
  and `speed`.
  `WorldConfig::possess_substeps` = 1 turns it off; it is off when the
  server runs as fast as possible (`--tick-ms 0`). A manual-clock server
  takes one with `RunningServer::substep`.
- **`Game`** (`game.rs`) is the pure logic around the world: one lineage per
  player, the mating-offer table and consent flow, the economy and the stud
  marketplace (`economy.rs`: balances, ledger, listings; escrow on offers),
  migration orders, views and interest management. It returns messages
  instead of sending them, so it is unit-tested without sockets.
- **Interest management:** a state message carries the player's own organisms
  in full and other organisms only inside a square view window (default
  half-side 16 tiles, at most 32; at most 256 foreign organisms, nearest
  first). The window follows the possessed organism, else the lineage's
  centroid, unless the client pins it with `set_view`. Foreign organisms show
  only what can be seen (position, action, sex, maturity), not needs.
  A push sorts the world once (`StateIndex` in `game.rs`: living organisms
  by owner and by 16-tile grid cell, open offers by player, the shared
  clock, weather and leaderboard), so each connection's state costs what
  it contains rather than a scan of every organism; possession sub-steps
  build one for the players they update. The leaderboard in a state is
  the whole board up to `GameConfig::state_leaderboard_rows` (50) rows;
  a longer one is cut to the top 50 plus the player's own row and the
  rows of the owners of the organisms in view (whose names the client
  shows), in rank order. `GET /api/leaderboard` always has every row.
  See [Load and soak test](#load-and-soak-test).
- **Slow clients:** each connection has a 256-message queue of replies and
  events (`push.rs`). States don't pile up in it: a state waits in a
  one-message slot (holding a single place in the queue) and a newer one
  replaces it until the writer sends it, so a client that reads slower than
  states come (possession sub-steps push the owner a state each quarter
  tick, 150 a second in `--fast` mode) gets the latest state and still
  every event, in order (`skipped_states` counts the replaced ones). A
  connection whose queue fills with replies and events is closed.
- **Accounts:** argon2id password hashes, random 256-bit bearer tokens held in
  memory. Each account owns a `PlayerId` (1, 2, …; 0 is the wild ecosystem).
  A session ends after 60 minutes without use (an authenticated request or a
  WebSocket command) or 24 hours after login (`SessionPolicy`); an open
  WebSocket checks every `min(idle, max_age)/4` (at most 30 s), sends
  `session_ended` and closes.
- **Password hashing is bounded** (`accounts::HashLimiter`): a registration
  or login hashes (or verifies) a password with argon2id, 19 MiB and tens of
  ms of CPU each. At most `--hash-concurrency` run at once on the blocking
  pool (default half the cores, at least 1), at most `--hash-queue` more wait
  for a slot (default 16 per slot), and past that the request is refused at
  once with `503 busy`, `retry_after_ms` (about the time the queue takes to
  drain, 0.25–5 s) and `Retry-After`. A refused registration reserves no
  name or player id; a waiter whose client goes away frees its place, and a
  hash already running keeps its slot until it ends. Token checks (every
  authenticated request, the WebSocket and its liveness checks) never hash,
  so only password logins count. The web client retries `busy` and
  `rate_limited` itself (`client/src/retry.ts`: the server's hint or a
  doubling backoff from 0.5 s, whichever is longer, ×0.5–1.5 jitter, capped
  at 10 s, 5 retries), showing "The server is busy; retrying in N s". The
  queue is behind the per-IP login limit, so a refused attempt still uses
  one of that client's login tokens. Before the bound, Tokio ran up to 512
  hashes at once, and a burst of logins starved every world thread (see
  [Several worlds: capacity](#several-worlds-capacity)).
- **Rate limits** (`limits.rs`, token buckets, `Limits` in `ServerConfig`):
  logins and registrations per client IP (burst 10, then one per 6 s; checked
  before any password hashing, `429 rate_limited` with `retry_after_ms` and
  `Retry-After`), WebSocket commands per connection (20/s, burst 40) and per
  IP over all its connections (60/s, burst 120), group changes
  (`claim_territory`, `abandon_territory`, `split_group`, `merge_groups`)
  per connection on top of that (1/s, burst 6), and the public reads that
  cost the world thread or the store (terrain, map, chronicle, history,
  seasons, lineage stats) per IP (10/s, burst 60); a refused command gets a
  `result` with `rate_limited` and never reaches the world thread. The IP is
  the TCP peer, an IPv6 peer counted by its /64 (behind a reverse proxy that
  would be the proxy: trusting `X-Forwarded-For` is not implemented).
- **Size limits:** HTTP bodies up to 64 KiB (`413` beyond), passwords up to
  1024 bytes, WebSocket messages and frames up to 16 KiB (a larger one
  closes the connection), at most 8 WebSocket connections per player
  (`429 too_many_connections`), map images up to 2048 px a side.
- **Alerts and history** (`game.rs`): the world thread keeps the daily records
  since start (up to 365, thinned with age as the store keeps them) with each
  lineage's births, deaths by cause and mean gene values, and raises per-player alerts: deaths of founders and
  possessed organisms (with the cause), the day's births and deaths,
  population (25, 50, 100, …) and legacy-share (1, 5, 10, 25, 50 %)
  milestones, a new generation, extinction, a possessed organism that is
  parched or starving, catastrophes beginning and ending, era ends (with the
  player's rank) and achievements. The last 50 per player are kept for the dashboard.
  The important ones also become push notices for players not connected to
  the world, handed to the push service without waiting (see
  [Push notifications](#push-notifications)).
- **Possessed organisms' needs:** the sim drops a move order once the
  organism arrives (within 0.75 tiles) and lets danger or severe thirst or
  hunger (`possess_danger`, `possess_urgency` = 85) win over the order until
  the need is down to 30, then resumes it. The server only tells the owner:
  a `need` alert (once a day per organism) at thirst or hunger 75.
- **Level of detail** (`--lod`, off by default): on the hour, each connected
  player's view windows (pinned, else the possessed organism, else the
  lineage centroid, which falls back to the lineage's cohorts when it has no
  agents) become that player's focus (`World::set_focus`); a player with no
  connection has none, so their lineage goes on as statistical cohorts
  wherever no one else is watching (possessed organisms keep their region
  active), and expands back into agents within the hour of reconnecting.
  At startup, before anyone connects, the whole world collapses to cohorts.
  Lineage sizes count cohort members (`World::population_counts`): the
  daily history, milestones, leaderboard `alive`, `state.population`, the
  lineage stats (`alive`, `adults`, `juveniles`; `agents` is the visible
  part) and `GET /api/world` (`organisms`; `agents` separately). Cohort
  members are not agents, so they are never in `organisms` or `nearby`; age
  bands, pregnancies and gene statistics cover agents only.

### HTTP API (JSON)

| Method and path | Auth | What it does |
| --- | --- | --- |
| `POST /api/register` `{name, password}` | | Create an account (name 3–24 of `[A-Za-z0-9_-]`, password ≥ 8 characters and ≤ 1024 bytes). `201 {token, player, name}`; `409 name_taken` |
| `POST /api/login` `{name, password}` | | `{token, player, name}`; `401 bad_credentials` |
| `POST /api/logout` | Bearer | End the session |
| `GET /api/worlds` | | The hosted worlds, the default first: `{default, worlds: [{id, name, width, height, day, year, season, population, lineages, players_online}]}` |
| `POST /api/lineage` (design) | Bearer | Found your lineage (one per player per world). `201 {lineage, founders: [ids], cost}`; `400` with `over_budget {cost, budget}`, `value_out_of_range {gene}`, `reserved_gene`, `litter_size`, `no_founders`, `unknown_gene`, `bad_name`; `409 already_founded` |
| `GET /api/genes` | | Settable genes with default and point cost, the budget and founder/offspring prices, and the morphology parameters (`morph`: name, kind, values or range, default, cost) |
| `GET /api/world` | | Clock, season, weather, map size, organism and lineage counts |
| `GET /api/leaderboard` | | Legacy shares from the latest daily snapshot: `{day, breeding_adults, rows: [{player, name, share, weighted_share, alive}]}` |
| `GET /api/leaderboard/history?limit=30&from=&to=&genes=true` | | The stored daily records, oldest first (legacy shares, lineage sizes, daily births and deaths by cause, player lineages' mean genes; thinned with age: see *Daily history retention*): the newest `limit` (at most 365) with a day in `from..=to` (the newest world's days; both optional); `genes=false` leaves out the gene means |
| `GET /api/leaderboard/seasons?limit=8` | | Seasonal leaderboards from the whole stored history, newest first: `[{year, season, first_day, last_day, days, current, rows: [{player, name, mean_share, final_share, peak_share, mean_weighted_share, peak_alive, days}]}]`, ranked by mean share over the season's recorded days (a missing day counts as 0; a restart, whose world starts again at day 0, starts a new run) |
| `GET /api/terrain` | | The tile grid: `{width, height, terrain_version, biome, elevation, river, base_water, dynamic}`; every layer is one byte per tile, row-major, base64 (biome index 0–7; elevation 0–255 lowest..highest; river 0–255; depths in cm, capped at 255) |
| `GET /api/terrain/dynamic` | | Only what changes (the sim updates it hourly): `{tick, day, hour, season, water (cm), ice, snow (0–255 for 0–0.5 m), veg (share of the tile's maximum), wetness (0.5 normal, < 0.3 drought)}`, about 110 KB of JSON for 128×128 |
| `GET /api/species` | | Detected species, living first by size: `{day, detected_day, samples, alive, species: [{id, name, origin (initial/founding/split), parent?, founded_day, extinct_day?, merged_into?, founder?, founder_name?, dominant, dominant_name, dominant_share, adults, peak_adults, lineages: [{lineage, name, owner, adults}], carnivory, body_size}], founded: [{player, name, count}]}` (`parent` links give the species tree) |
| `GET /api/eras` | | `{era, era_days, first_day, last_day, days_left, eras: [{era, first_day, last_day, breeding_adults, species_alive, catastrophes, rows: [{player, name, share, weighted_share}], niches: [{niche, player, name, share, adults}]}]}`, finished eras newest first with their final ranking |
| `GET /api/catastrophes` | | `{enabled, active?, history: [{id, kind, severity, start_day, end_day, active, population_before, population_after?, players_before, survivors}]}` (only catastrophes that have started) |
| `GET /api/seasons?offset=0&limit=20` | | The world's finished ranked seasons (of every map it had), newest first: `{total, offset, seasons: [{world_id, season, first_day, last_day, length, days, ended_tick, ended_unix, players, winners: [{category, player, name, value}], summary}]}` (limit 1–100) |
| `GET /api/seasons/current?offset=0&limit=10&player=N` | | The ranked season under way: `{world_id, season, first_day, last_day, length, days, days_left, offset, limit, standings: [{category, total, rows: [{player, name, rank, value}]}]}`, each category's rows `offset..offset+limit` plus player N's row (limit 1–200) |
| `GET /api/seasons/:n?map=ID&offset=0&limit=10&player=N` | | A finished season of the world (of its running map unless `map` names an earlier one by `world_id`, e.g. from before a `--fresh`): its head as in the list plus paged `standings` as above; `404 no_season`, `400 bad_season` |
| `GET /api/seasons/player/:player?limit=50` | | In the world: `{player, seasons: [{world_id, season, first_day, last_day, ended_unix, placements: [{category, rank, value, of}]}]}`: the finished seasons the player was ranked in, newest first |
| `GET /api/achievements` | | `{players: [{player, name, species_founded, longevity_days, niche_dominance, catastrophes_survived}], recent: [{day, player, name, kind, text}]}` |
| `GET /api/lineage/stats` | Bearer | The genetics dashboard: counts (alive, adults, young, pregnant, newest generation), `age_bands` (up to six equal bands by sex), `vitals` (births and deaths by cause per recorded day, the last entry today so far, at most 30), `genes` (every gene's mean, spread and founding mean with the daily means, largest drift first, and `bins`: members per tenth of the 0–1 range, a histogram), `origins` (whose alleles the living members carry: `[{player, name, lineage?, alleles, share}]`, largest share first; 0 is the wild), `legacy` (share per recorded day) and recent `alerts` |
| `GET /api/alerts/prefs` | Bearer | The caller's alert preferences, `{hidden: [kind], toast: [kind]}` (kinds as in `alert` messages); the defaults `{hidden: [], toast: ["death", "need", "market"]}` until saved |
| `POST /api/alerts/prefs` | Bearer | Replace them; the reply is the normalized form (sorted, no duplicates, nothing hidden is toasted). An unknown kind is refused (422) |
| `GET /api/push/key` | | `{enabled, public_key?, kinds, native?}`: whether the server sends push notifications, its VAPID public key (base64url, the browser's `applicationServerKey`), the push kinds and the native push services configured (`["fcm", "apns"]`; left out when none is) |
| `POST /api/push/subscribe` | Bearer | A browser's `PushSubscription.toJSON()` (`{endpoint, keys: {p256dh, auth}}`) plus an optional `label` (default: the user agent): stores it for the account, replacing one with the same endpoint. `{endpoint, created_unix, label?}`; `400 bad_subscription` (endpoint not a known push service, keys not a P-256 point and a 16-byte secret); `503 push_disabled` |
| `POST /api/push/unsubscribe` `{endpoint}` | Bearer | Forget one of the caller's devices: `{removed}` |
| `POST /api/push/native/subscribe` `{service, token, label?}` | Bearer | An app's device token: `service` `fcm` (Android, an FCM registration token) or `apns` (iOS, the hex device token). Stores it for the account, replacing the same service and token. `{service, created_unix, label?}`; `400 bad_subscription` (not a token of that service); `503 push_disabled` (push off, or that service has no credentials) |
| `POST /api/push/native/unsubscribe` `{service, token}` | Bearer | Forget one of the caller's app devices (also while push is off): `{removed}` |
| `GET /api/push/subscriptions` | Bearer | The caller's devices, oldest first: `{subscriptions: [{endpoint, created_unix, label?}], devices?: [{service, created_unix, label?}]}` (keys and tokens stay on the server; `devices` left out when there are none) |
| `GET /api/push/prefs` | Bearer | The push kinds the caller switched off, `{off: [kind]}` (`mating`, `death`, `market`, `species`, `season`, `catastrophe`); `{off: []}` until saved |
| `POST /api/push/prefs` | Bearer | Replace them; the reply is normalized (sorted, no duplicates); an unknown kind is refused (422) |
| `GET /api/map.png?scale=4` | | PNG of the world (`render_rgb`, 1–8 px per tile, lowered to keep a side within 2048 px), for debugging |
| `GET /api/metrics` | | Server health (`metrics.rs`): world-thread timings over the last 256 samples as `{n, p50, p99, max}` ms (`tick_ms`: step and bookkeeping; `states_ms`: one state push to every connection; `substep_ms`; `apply_ms`), `ticks`, `conns`, `state_pushes`, `applied`, `max_queued`, `dropped_conns`, `skipped_states`, the last save (`saves`, `save_capture_ms`, `save_encode_ms`, `save_write_ms`, `save_bytes`, `snapshots_pending`), WebSocket traffic (`bytes_out`, `msgs_out`, `state_bytes`, `states_out`, `ws_opened`, `ws_closed`; `state_encode_ns`, the writers' total time encoding states; protocol 2's `delta_conns`, `keyframes_out` and `resyncs`), the process's `cpu_seconds` and `rss_bytes` (Linux), the `allocator` (`jemalloc` by default, `glibc` without the feature) and its `heap` statistics (`allocated`: live allocations; `retained`: resident memory the allocator holds but has not handed out; read at most every 2 s), and password hashing over the whole process, `hashing`: `concurrency` and `queue` (the bound), `in_flight` and `waiting` now, `peak_in_flight` (never above `concurrency`), `hashes` done, `rejected` (refused as busy) and `mean_ms` per hash |
| `GET /ws?token=…` | token | The game WebSocket (or `Authorization: Bearer`) |

Routes about a world (founding, `world`, the leaderboards, terrain, species,
eras, catastrophes, achievements, lineage stats, map, metrics and the
WebSocket) take `?world=<id>` and use the first world without it; an unknown
id is `404 unknown_world`. Accounts, alert and push preferences, push
subscriptions, the gene catalog and `/api/worlds` are global. `/api/metrics` reports one world (its `world`
field; `players_online` is distinct players connected) plus the process's
CPU and memory, which all worlds share.

Errors are `{"code": "...", "message": "...", ...detail}`. A design:

```json
{"name": "Marsh striders", "thermo": "warm", "birth": "live", "litter_size": 2,
 "traits": {"body_size": 0.4, "leg_length": 0.5, "fur": 0.4, "keen_nose": 0.3}, "founders": 20}
```

`thermo` is `warm`/`cold`, `birth` is `live`/`incubated_eggs`/`abandoned_eggs`;
gene names are the sim's labels in `snake_case` (`GET /api/genes` lists them).
An optional `morph` object sets the body plan (`docs/morphology-contract.md`),
every field optional with the contract defaults: `{"eye_count": 3,
"eye_azimuth": 90, "ear_count": 2, "ear_shape": "pointed", "leg_count": 6,
"tail_shape": "fin", "snout_length": 0.3}` (eyes 1–6, ears 0/2/4, legs
0/2/4/6/8, ear shapes `round`/`pointed`/`long`/`fan`, tail shapes
`thin`/`bushy`/`fin`/`club`/`prehensile`, azimuth 0–180°, snout 0–1). A value
out of range or an unknown shape is refused as `bad_morph` (`detail.field`).
Extra structures cost points: 1 per eye beyond two, 2 per leg pair beyond
two, 2 for a second ear pair; fewer structures, shapes, `eye_azimuth` and
`snout_length` are free (*Point costs*, *Body plan structures*). `GET
/api/genes` lists the body plan under `morph` (kind `count`/`enum`/`axis`,
allowed values or range, default, and `cost`: for a count, points per step
above the default, one eye or one pair; 0 for shapes and axes), and also
gives `cost_cold_blooded` and `cost_per_smaller_body`, so a client can show
the whole budget the server charges.

### WebSocket protocol

Every message is a JSON object with a `type`. A client message may carry an
`id`, echoed in its `result`.

Client → server:

| `type` | Fields | |
| --- | --- | --- |
| `possess` | `organism` | Take control of one of your organisms |
| `order_move` | `organism, x, y` | Walk a possessed organism to a point (tiles) |
| `release` | `organism` | Back to genetic behavior |
| `request_mating` | `mine, partner, price?` | Directed mating. Your own pair runs at once; another player's organism sends them a `mating_request`. A `price` (renown) goes into escrow at once (`insufficient_funds` if you can't cover it) |
| `preview_mating` | `mine, partner` | What the mating would give now, nothing spent: `data` = `{compatible, fertility, body_plan_fertility, distance_fertility, body_plan_gaps: [{gene, gap}], mismatch, cost, failure_chance, distance, meeting_distance, problem?}` |
| `respond_mating` | `request, accept` | The partner's owner answers |
| `complete_mating` | `request` | Retry an accepted agreement now |
| `cancel_mating` | `request` | Either side withdraws an open offer |
| `migrate` / `clear_migration` | `x, y, group?` / `group?` | Group command for your adults: every group without `group`, else that group (`no_such_group` past 3); `data` = the clamped `{x, y}`. Replaces the territory of the groups it reaches |
| `claim_territory` | `group?, x, y, radius` | Group `group` (default 0) holds the circle around `x, y`: the radius is kept between 3 and the group's `max_radius`; replaces its migration order; `data` = `{x, y, radius, claimed_tick}`. `claim_cooldown` (`detail.ticks_left`) within half a day of the group's last claim; `no_such_group` for a group without members; `bad_coordinates` for a non-finite number |
| `abandon_territory` | `group?` | Give the territory up (`no_territory` if none) |
| `rally` / `cancel_rally` | `group?, x, y` / `group?` | Group `group` (default 0) gathers at `x, y` and holds within `2 + √members` tiles (at most 24) for 3 days; replaces its migration order or territory; `data` = `{x, y, radius, expires_tick}`. `no_such_group` for a group without members, `bad_coordinates` for a non-finite number; cancelling with none standing gives `no_rally` |
| `split_group` | `group?, organisms?` | Divide a group (default 0) in two: the named organisms (yours, in that group, at most 2000), else the far half across its widest spread; `data` = `{group, moved, kept}`. `group_limit` at 4 groups, `group_too_small` if a side would be empty, `not_in_group`, `not_owned`, `too_many_organisms` |
| `merge_groups` | `from, into?` | Every member of `from` joins `into` (default 0); `data` = `{moved}`; `same_group`, `no_such_group` |
| `set_view` / `clear_view` | `x, y, radius` / – | Pin or release the view window |
| `create_listing` | `organism, fee, expires_days?` | List one of your adults on the stud marketplace (a male as a stud, a female as a dam); `data` = `{listing}` |
| `withdraw_listing` | `listing` | Take your listing down (agreements already bought stand) |
| `buy_listing` | `listing, mine` | Buy one mating with a listed organism for your adult `mine`: an already accepted agreement, the fee in escrow; `data` = `{offer}`. Not for auctions (`auction_listing`) |
| `create_auction` | `organism, reserve, days?, ticks?` | Auction one mating with one of your adults: it closes after `ticks` ticks if given, else `days` simulated days (default 1, at most 30; not both: `bad_duration`); `data` = `{listing}` |
| `bid` | `listing, amount, mine` | Bid on an auction for your adult `mine` (the same checks as `buy_listing`): at least `auction.min_bid` (`bid_too_low`, `detail.min_bid`), held in escrow (raising your own highest bid holds only the difference), refunded at once when outbid; `data` = `{listing}` |
| `browse_market` | `mine?, limit?, cursor?, sort?, kind?, role?, max_price?, lineage?, species?, compatible?, hide_own?, with_own?` | One page: `data` = `{balance, escrow, slots_left, total, next_cursor?, own?, listings: [{id, owner, owner_name, organism, role (stud/dam), sex, fee, created_tick, expires_tick?, sales, busy, pregnant, lineage, lineage_name, generation, mother?, father?, offspring_raised, age_days, health, x, y, traits: [{gene, value}], preview?, kind (fixed/auction), auction?: {reserve, deadline_tick, high_bid?, high_bidder?, bids, min_bid}}]}`. `limit` default 20, at most 50; `next_cursor` (absent on the last page) goes back as `cursor` for the next page in the same order and filters; `total` counts the matches. `sort`: `newest` (default), `price_asc`, `price_desc` (price: the fee, or an auction's highest bid, its reserve before any), `ending` (deadline or expiry, none last), `fertility` (with `mine`: compatible first by expected fertility). Filters: `kind` `fixed`/`auction`, `role` `stud`/`dam`, `max_price`, `lineage`, `species` (its lineages, `GET /api/species`), `compatible` (with `mine`), `hide_own`; `with_own` adds `own`, all your listings. `preview` is `preview_mating` of your `mine` with the listed organism. `bad_cursor` for a cursor the server didn't make |
| `ledger` | `limit?` | `data` = `{balance, escrow, entries: [{seq, player, tick, day, delta, balance, kind, text, offer?, listing?}]}`, newest first (default 50, at most 100); `kind`: `grant`, `income`, `achievement`, `escrow`, `sale`, `refund` |
| `resync` | – | Protocol 2 only: the next state is a keyframe (no `result`; see below) |
| `watch_season` | `on`, `limit?` | `on: true`: a `season` message at once (after the `result`) and then one whenever the ranked season's standings change, at most one a second (rows `0..limit` per category, default 5, at most 200, plus yours); `on: false` stops them. A new connection starts unwatched. Answered between ticks, never queued behind world changes |

Server → client:

| `type` | When |
| --- | --- |
| `welcome` `{player, name, lineage, proto?}` | On connect; `proto: 2` when the connection speaks protocol 2 (absent for protocol 1) |
| `state` | On connect, then every state interval (default 1 s): `clock` (tick, day, year, hour, season, daytime, era, era_days), `weather` (mean wind, storm, drought and flood shares, `catastrophe` while one is under way), `lineage`, `slots_left`, `population` (lineage size, cohort members included), `migration` (the main group's), `groups` (yours by id, the main group always: `{id, members, adults, migration?, territory?: {x, y, radius, claimed_tick}, rally?: {x, y, radius, expires_tick}, max_radius, claim_ready_tick?, obedience}` (`obedience`: the members' mean obedience gene, to hundredths)), `view`, `organisms` (yours: id, lineage, position, action, sex, age, growth, health, needs, possessed, pregnant, generation, `look`, `group` when not 0; when possessed also `speed`, tiles per tick toward an order on open ground (`World::move_speed`), and `order` `[x, y]` while it has one; `heading`, its facing as one of 32 directions, and `vision`, its eyes for the vision cones: see *Vision cones* below), `nearby` (others in the window, also with `look`; never a heading or eyes), `leaderboard` (cut to the top 50 rows plus yours and those of the owners in `nearby` when longer), `offers` (your open offers, with `listing?` and `escrow` when bought from a listing or priced), `balance` and `escrow` (your renown, and how much of it is held on open agreements) |
| `result` `{id, command, ok, error?: {code, message, detail?}, data?}` | After each command. Codes include the sim's `not_found`, `not_owned`, `no_consent`, `not_mature`, `same_sex`, `already_pregnant`, `too_far`, `no_slots_left`, `possessed_by_other`, the group commands' `no_such_group`, `group_limit`, `group_too_small`, `not_in_group`, `same_group`, `no_territory`, `claim_cooldown` (`detail.ticks_left`), `too_many_organisms`, and `incompatible_body_plans` (`detail.genes`: the differing body-plan genes), `genetically_incompatible`, `died`, `bad_message`, `bad_coordinates`, `unknown_offer`, `offer_closed`, `too_many_offers`, `busy`, `rate_limited`; the marketplace's `insufficient_funds` (`detail`: `{balance, needed}`), `bad_price`, `bad_expiry`, `already_listed`, `too_many_listings`, `unknown_listing`, `own_listing`, `own_organism`, `listing_busy`, `organism_busy`, `auction_listing`, `not_auction`, `bid_too_low`, `auction_closed`, `auction_has_bids`, `bad_duration`, `bad_cursor` |
| `mating_request` `{offer}` | Someone asks to mate with your organism |
| `mating_update` `{offer, outcome?, error?}` | An offer changed status (to both sides) |
| `alert` `{alert: {tick, day, kind, text, organism?}}` | `kind`: `daily_summary`, `birth` (first offspring, new generation), `death` (founder or possessed organism), `milestone`, `need` (possessed organism parched or starving), `world` (a catastrophe began or ended, an era ended with your rank), `achievement` (species founded, longevity, niche dominance, survived a catastrophe), `market` (someone bought a mating with your listed organism, a sale paid to you, a price paid or refunded from your escrow, a listing closed by expiry or death, an achievement's renown bonus, a bid on your auction, outbid and refunded, an auction won, sold, unsold or failed) |
| `session_ended` `{reason}` | The session expired; the socket closes next |
| `season` `{world_id, season, first_day, last_day, length, days, days_left, offset, limit, standings}` | To a connection that sent `watch_season`: the ranked season under way (as `GET /api/seasons/current`), at once and on each change |

**Protocol 2: binary delta states** (`wire.rs`, the client's `wire.ts`).
A client that connects with `/ws?token=…&proto=2` gets `welcome` with
`proto: 2` and then every `state` as a **binary** WebSocket message holding
only what changed since the state that connection was last sent; every
other message stays the JSON above. Without the parameter (older clients)
nothing changes: whole JSON states, no `proto` in `welcome`. A client
asking for 2 from an older server gets JSON states and no `proto`, so the
web client handles both.

- **Computed at send time.** Each connection's socket writer keeps a
  `DeltaEncoder` with what it last sent that client and encodes a state
  only when it writes it. A state replaced in the one-slot queue
  (`push.rs`) is never encoded, so the next delta is against what the
  client actually holds and nothing is lost; the world thread builds the
  same `StateView`s as before.
- **Frame:** a kind byte (1 keyframe, 2 delta), the state's `seq` on the
  connection, the `base` seq a delta applies to, a JSON *meta* object, then
  two sections (own organisms, nearby organisms), each a list of removed
  ids and a list of new or changed records: id (delta-coded, ascending), a
  bit mask of the fields that changed and those fields as LEB128 varints,
  numbers as zigzag differences from the client's copy. Quantized:
  positions and `order` to 1/64 tile, needs to 0.1, health and age to 0.01
  (days), growth to 0.001; `speed` stays an exact `f32`; an organism's
  `group` is its own field. The format is documented field by field at
  the top of `wire.rs`.
- **Meta JSON** carries the other fields only when they changed (`clock`,
  `weather`, `view`, `population`, `balance`, `groups` as a whole list, …), leaderboard rows keyed by
  player (`lb_set` / `lb_del`, the client keeps them in rank order),
  offers keyed by id (`offers_set` / `offers_del`), and **palettes**: each
  distinct `look` and action name is sent once per connection (`looks`,
  `actions`) and records refer to it by index, so a `look` costs a byte
  when an organism first comes into view and nothing after.
- **Sub-step states** for a steering player come out as about 50–80 bytes:
  the `view` that follows the possessed organism and that organism's new
  position (plus `action` on the first sub-step after an order).
- **Keyframes and resync.** The first state of a connection is a keyframe,
  and so is one at least every 60 s (`KEYFRAME_EVERY`). A client that can't
  apply a frame (a `base` that isn't the seq it holds, a truncated or
  garbled frame, an index outside a palette) drops states until the next
  keyframe and sends `{"type": "resync"}`; the writer then makes the next
  state a keyframe (at most one request per 2 s from the web client).
  Decoded lists are in id order (`organisms`, `nearby`, `offers`), the
  leaderboard in rank order.
- **Tests:** Rust (`wire.rs`) round-trips ticks and sub-steps of a live
  game through encoder and decoder with states dropped at random (the
  replaced slot) and random keyframes, and checks every decoded state
  equals the original after quantization (`wire::canonical`), that a
  sub-step delta carries only the possessed organism, that a lost frame is
  refused until a keyframe, and that truncated or garbled frames are
  errors, not panics; a server test (`tests/server.rs`) compares a protocol
  2 socket with a protocol 1 socket of the same player push by push and
  checks `resync`. A Rust test writes `client/src/wire.fixture.json`
  (frames of a short session and the states they decode to,
  `UPDATE_WIRE_FIXTURE=1` after a format change), and Vitest
  (`wire.test.ts`) decodes it with the client's decoder, drops a frame,
  garbles and truncates frames, and drives the socket's resync.

**Protocol 3: vision cones** (`vision.rs`, `wire.rs`; decision #8 of
`docs/morphology.md`: cones are shown only to the owner, on their own
organisms). Each own organism carries `heading` (`Organism::heading`, the
last movement, quantized to 32 directions: `k · 11.25°` from +x toward +y)
and `vision`, a compact descriptor fixed at birth: `eyes` (1–6),
`azimuth` (first pair, °), `field` (each eye's width, °), `occlusion`
(snout, °, absent at 0), the sim's `coverage` (%), `binocular` (depth
field, °), `blind` (behind, °), `front_gap` (ahead, °, absent at 0) and
`directional` (present when `Balance::directional_vision` > 0, i.e. blind
spots shorten detection; off, the cones only describe the eyes). Nearby
organisms never carry either, whatever the protocol (tested in
`tests/server.rs` and `wire.rs`). In JSON states (protocol 1) they are two
more optional keys per own organism. A client asking for `proto=3` gets
protocol 2 frames with two more own fields (bit 20 heading, bit 21 an index
into a `visions` palette in the meta JSON, like `looks`); a client asking
for 2 gets exactly the protocol 2 frames (its decoder refuses unknown bits),
and a client asking for 3 from an older server is answered 2 and draws no
cones. Cost: each distinct descriptor once per connection (about 75 bytes of
JSON; a lineage has a handful, they vary with the `Eyes` gene), then one byte
for the palette index when an organism is first sent and one byte when it
turns past an 11.25° step: in the `wire.rs` session test a keyframe of six
own organisms grows by 336 bytes, nearly all of it the four palette entries,
and a delta by a byte per organism that turned. The web client asks for 3.

`look` (`appearance.rs`) is the visible body plan of an adult, computed from
the phenotype with a few comparisons per organism per push: `size` (0 tiny
< 1 kg, 1 small < 8 kg, 2 medium < 40 kg, 3 large), `diet` (herbivore /
omnivore / carnivore by carnivory), `warm` (endothermy), `locomotion` (walk,
swim at swimming ≥ 0.6, glide, fly), `cover` (skin, fur, scales, armor: the
strongest expressed ≥ 0.5) and `coloring` (drab, camo, bright display
colors, or warning = bright and toxic). Only what anyone could see, and
coarse so the client's sprite cache stays small. It also carries the
quantized morphology of `docs/morphology-contract.md`: `eyes` (1–6),
`ears` (0, 2 or 4) and `ear_shape` (round, pointed, long, fan), `legs` (0,
2, 4, 6, 8), `tail` (none, thin, bushy, fin, club, prehensile), and size
buckets 0–2 (small, medium, large) `eye_size`, `ear_size`, `tail_size`,
`snout`, `fangs` (0 none, 1 small, 2 large; none below 0.1), plus
`eye_azimuth` (0 forward under 35°, 2 side-set from 85°), sent only when it
is not the default bucket 1, so default bodies' looks are unchanged (the
client draws forward-set eyes at the front of the head, side-set ones high
and back). They come from
`Morphology::of`, the one function that reads them off the organism: its
genome's expressed morphology (`Organism::morph`: counts, shapes, eye
azimuth and snout from the morphology loci, sizes from the `Eyes`, `Ears`,
`Tail` and `Fangs` genes), so a designed body plan shows in `look` from the
founders on. The fields were
added without a protocol bump: they are new keys in the same JSON object
(and in protocol 2's `looks` palette), which older clients ignore, and a
client reading a server without them draws today's defaults. The API answers
cross-origin requests (any origin, `GET`/`POST`, `Authorization` and
`Content-Type`; auth is a bearer token, never a cookie), so a Capacitor app
or a bundle on another origin can use a remote server.

**Directed-mating consent:** `request_mating` checks ownership, maturity, sex,
pregnancy, compatibility and slots, then creates a `pending` offer (a
price is taken into escrow, see [Economy](#economy-and-stud-marketplace))
and asks the partner's owner. A pair that can't
conceive is refused up front instead of spending a slot: different body
plans (`incompatible_body_plans`, naming the genes, e.g. endothermy and birth
mode) or too distant genomes (`genetically_incompatible`); `preview_mating`
shows the expected fertility, preference mismatch, override cost and failure
chance before asking (the matings panel shows it for the chosen pair). The
female of the pair is held from breeding on her own
(`World::hold_breeding`) once her owner has consented, so she isn't already
pregnant when her partner arrives: the requester's own female from the
request on, the partner's female only once her owner has **accepted** and
the agreement waits for the two to meet. A merely `pending` request never
holds the partner's female: without her owner's consent it must not be able
to freeze another player's organism with requests that owner ignores (pairs
within one lineage need no consent and mate at once). Wild organisms can't consent. On
`accept` the server tries the mating at once with `partner_consent`; if the
two are farther apart than the meeting distance the agreement stays
`accepted` (both get `too_far`) and runs on the first tick they meet, so
escorting a possessed organism to the meeting is the trip the design
describes. Outcomes: `completed` (with mismatch, override cost, failure
chance, fertility, conceived), `declined`, `cancelled`, `failed` (`died`:
an organism died first, with the cause; or the rules refused it) or
`expired` (season change, when slots reset). The requester's seasonal slot
is spent.

### Offline protection

The design doc leaves this open ("Offline protection: home ranges or
surviving founder groups"). The simulation runs while a player sleeps, and
at 1 tick per second a night offline is several hundred simulated days, so
an unattended small lineage could be wiped out with nothing its owner could
do. Implemented as the *surviving founder group* form, deliberately narrow so
that logging out never pays as a strategy:

- **Who.** A player's lineage is protected from the moment their last
  connection to that world closes until they connect again, or for at most
  12 hours (`--offline-protection-hours`, `GameConfig::offline_protection_ticks`;
  converted to ticks at the server's clock; 0 turns protection off). Each
  world tracks its own connections, so a player on world A is offline in
  world B. A restart counts as everyone logging out; a spell already running
  keeps its clock (the spells are in the saved game, `game::offline`), so a
  restart never hands out a fresh 12 hours. A new spell needs a login in
  between; there is no cooldown beyond that, since the floor below already
  keeps protection from helping a lineage that is doing well.
- **When it applies.** Only while the lineage is down to its last 12 living
  members (agents plus cohort members of abstract regions,
  `ProtectionRules::floor`), checked at each day boundary and whenever the
  protected set changes, never mid-tick (so the sequential and parallel apply
  agree). A thriving lineage plays by the normal rules: protection can save a
  lineage from extinction, never win it legacy share.
- **What it does.** A hunter's strike that would kill a protected organism
  succeeds with a quarter of its usual chance (`predation` 0.25; the hunter
  still pays for the attempt and the prey can still escape or fight back),
  and starvation and thirst damage is halved (`hardship` 0.5). Cohorts of
  abstract regions get the same: a quarter of their exposure to hunting
  cohorts and half their starvation and thirst hazards. Cold, heat, old age,
  disease, larval competition, migration losses and catastrophes are
  untouched, and protection does nothing for breeding. Predators lose little:
  12 organisms of a player's are a small part of any hunting range.
- **Not protected.** Organisms in an open directed-mating agreement (either
  side, pending or accepted), listed on the stud marketplace or an auction,
  or put up as the organism of an auction's highest bid: the doc's "the trip
  carries risk" stands, and a player can't shield a stud by logging off.
  Possessed organisms aren't protected either (possession outlives a
  disconnect; an organism still possessed while its owner is away is under
  orders, not sheltering).
- **What the player sees.** On login, if protection actually applied (the
  lineage was at or below the floor while they were away), a `milestone`
  alert: "While you were away, offline protection shielded your small
  lineage for N simulated days …". Nothing new on the wire.
- **Determinism and saves.** The world never decides who is online: the
  server hands it the protected lineages and exempt organisms
  (`World::set_offline_protection`, sent only when they change). The
  protection is not in world snapshots (the field is skipped; snapshot format
  unchanged) and the server sets it again on resume. With nothing protected
  every factor is exactly 1.0, so the simulation is bit-identical to a world
  without the feature (the 12-seed sweep at seed 31 still scores 11.54).
  Tests: `world::offline::tests` (floor, exemptions, protected voles never
  killed with the kill chance cut to 0) and `game::offline::tests` (logout to
  login, the cap, exempt agreement organisms, a restart keeping clocks).

**Home range.** The doc's other form, built beside the surviving core: a
territory a group claimed ([Groups and territory](#groups-and-territory))
shields its residents while the owner is away. It helps a lineage of any
size a little where the core helps a tiny one a lot, so it is narrower in
what it softens and has more limits on when:

- **Which territory.** Picked when the player logs off: of their groups
  holding a territory, the one with the most members (ties to the lower
  group id). So at most one range per player, and only a territory claimed
  before logging off: the spell remembers the group and the claim's tick,
  and the shield ends if that territory is abandoned or replaced. A player
  without a territory at logout gets none for that spell.
- **When.** From 10 minutes after logout (logging off is no escape from a
  fight under way), at full strength until 12 hours (`--home-range-hours`,
  `GameConfig::home_range_ticks`; 0 turns it off), then fading linearly to
  nothing over 24 hours more (2× the setting, `home_range_fade_ticks`), in
  5% steps. Like the core, a login ends the spell and a restart keeps its
  clock and its claim (`Spell::home`, saved with the game; spells saved
  before this have none).
- **Who.** Residents are the living agents of that group standing inside
  the circle, and with level of detail on, the lineage's cohort members in
  abstract regions whose center lies inside it. Exempt organisms (trades,
  market) and possessed ones are not residents. Who is a resident is
  decided on the hour, never mid-tick.
- **What.** Predation only: a strike that would kill a resident succeeds
  with `1 − strength × share × (1 − home_range_predation)` of its usual
  chance (×0.5 at full strength), and a resident cohort's exposure to
  hunting cohorts is scaled the same. Starvation and thirst are untouched,
  so a herd that outgrows its range still starves: nothing is immortal.
  The residents share one shield: up to `home_range_residents` (20) get it
  in full, above that each gets `20 / residents` of it, so a big herd gains
  little more than a small one.
- **Stacking.** An organism under both forms (a lineage down to 12 inside
  its range) gets the product of the two factors, floored at
  `offline_stack_floor` (×0.2) on account of the stacking; either form
  alone keeps its own factor (the core's ×0.25 is untouched).
- **What the player sees.** The group card marks the territory that would
  be shielded (`⌂ home range`, with a line on what it does), the map draws
  it with a gold outer ring and a house at its center, and the legend
  explains it (`GroupView::home_range` on the wire, a flag, absent when
  false). On login, if the range had residents while they were away, the
  same `milestone` alert says for how long.
- **Saves and determinism.** The three rules are new `Balance` fields
  (`home_range_predation` 0.5, `home_range_residents` 20,
  `offline_stack_floor` 0.2; snapshot version 17, older snapshots read them
  as these defaults, `serde_util::before_v17`). The world never shields a
  range on its own: with none set every factor is exactly 1.0 and the
  12-seed sweep at seed 31 still prints 11.76.
- **Effect** (`compare --offline-owner P`: player P claims the largest
  territory its main group may hold around its founders on day 0 and stays
  offline for the whole run on both sides, with the surviving core as
  well; B turns the home range off with `--set home_range_predation=1`):
  `compare --size 96 --seeds 12 --seed 31 --offline-owner 2 --offline-owner 4
  vs --set home_range_predation=1` (herd grazers and burrowing omnivores away
  for 120 days) gives a paired score difference of −0.03 ± 0.04 (n = 12, not
  significant; 9 seeds tied), and no lineage's survival or mean alive moves
  outside its CI (herd grazers −0 ± 3, burrowing omnivores −2 ± 3 alive).
  Even with the shield undiluted (`--set home_range_residents=1000000`) the
  difference is −0.11 ± 0.20, none significant. In this world these lineages
  are limited by food in their range, not by hunters, so softening
  predation inside it buys them nothing measurable: logging off doesn't pay,
  which is the intent. What it is for, a small herd sheltering in its range
  through a bad night, is too rare to show here; it is checked in the unit
  tests (shielded voles are hunted at most a quarter as often). Not tuned.
  Tests: `world::offline::tests` (residents only, strength, one range per
  owner, the shared shield, exemptions, stacking and its floor, shielded
  voles hunted far less, cohorts) and `game::offline::tests` (onset and
  fade, only a territory held at logout, abandonment, the largest group,
  off, a restart).

### Ranked seasons

The design doc's "seasonal leaderboards: snapshot and rank at the end of
each era", as fixed-length competitive seasons (`league.rs`; the server's
`GameConfig::season_days`, `--season-days`, default 60 simulated days: half
a default year, so an era of 3 years holds 6). They are not the weather
seasons (spring…winter, `GET /api/leaderboard/seasons`, the Lineage tab's
"By world season" boards); every `/api/seasons` route means ranked seasons.

- **Boundary.** Season *n* of a world counts the daily legacy snapshots of
  days `first_day..first_day + season_days - 1`. When the daily record of
  the first day past it arrives, the season is frozen into a **season
  record** (final ranks per category, winners, a one-line summary such as
  "Season 2 (days 61–120): ann won the legacy crown with 12.3% mean share.
  Largest lineage: bob (412 organisms). …") and every player gets a `world`
  alert with their places; the next season starts where it ended. A changed
  length applies from the next season.
- **Categories** (from what the game already tracks; nothing new in the
  sim): **legacy** (mean legacy share over the season's snapshots, a missing
  day counting 0, ties to the higher final share: the season's champion),
  **lineage** (most living members on a recorded day), **species** (species
  founded, the chronicle's credited splits), **market** (renown earned from
  stud fees paid out of escrow) and **survival** (catastrophes survived). Only
  players with a value above 0 are ranked; equal values share a place
  (1, 2, 2, 4); the wild is never ranked. The design doc has no tournaments
  (the calibration "tournaments" are an offline balance tool), so there is
  no tournament board. Seasons are prestige only: no renown prize.
- **Restart safety.** The season under way (`SeasonTracker`) and the last
  finished record are in the saved game (`SAVED_GAME_VERSION` 3; older saved
  games start season 1 at their next day). A day is counted at most once. A
  world restored from a save made before a rollover replays it and writes
  the record again under the same `(world id, season)` key, which every store
  treats as a replacement in place (no second record); the record is sent to
  the store task before the save that follows the rollover, and a restored
  game writes its last finished record again at startup, so a record whose
  write was lost comes back. Tested: `game::tests::a_restart_around_a_rollover_neither_loses_nor_double_counts`
  and, on the file store and PostgreSQL, `restart_around_a_season_rollover`
  (drop the stored record, restart, it is back; drop the saves after the
  rollover, restart, the replay replaces the record with the same standings
  and season 2 counts its days once).
- **Seasons, worlds and `--fresh`.** Ranked seasons belong to their world
  (`?world=` on every `/api/seasons*` route, as on the other per-world
  routes). Season numbers are per generated map: a fresh map starts at
  season 1 again. A world's history lists the seasons of every map it had
  (`world_id` tells them apart); `/api/seasons/:n` means the running map's
  unless `?map=<world_id>` names another. (Before several worlds this
  parameter was `?world=`, which now picks the world.)
- **Cached standings** (`standings.rs`). Reads never recompute or reach the
  store per request. The season under way is computed on the world thread
  only when its standings change (`Game::live_season_key`: a generation the
  tracker bumps on every counted day, species, stud fee and survival, the
  season number, the days left and player names), published as one
  `Arc<LiveSeason>` with the summary, and paged from that copy by
  `/api/seasons/current` (which no longer queues a request on the world
  thread) and by the `season` pushes to every `watch_season` connection
  (each gets a page of the shared copy, with its own row found by index;
  pushes are coalesced to one a second). Finished seasons, the season list,
  player histories and the weather-season boards (`/api/leaderboard/seasons`,
  which read the whole stored daily history, bounded by its retention and
  read without gene means) are cached per world in its
  `WorldStore` (shared by the HTTP routes and the persist task) and
  invalidated by the writes that change them: a season record written (a
  rollover, or a replayed one) drops the lists and histories and replaces
  the record, a "not ended" answer included; a daily record drops the
  weather boards. Loads and season writes take one lock, so a load that
  raced a write can't keep a stale answer, and a crowd missing one entry
  loads it once. Invalidation is event-driven rather than a short expiry:
  every change happens in this process and they are rare (one record per
  rollover, one daily record per simulated day), so the cache is exact,
  shows a rollover the moment it is written, and a steady crowd of readers
  never reaches the store. (A second process writing the same world's
  store would not be seen; one process serves a world.) Tested:
  `standings::tests` (invalidation on writes, worlds kept apart, a load
  racing a write), `league::tests` (generation per score change and
  rollover), and `tests/standings.rs` (over HTTP and WebSocket, two worlds,
  days and rollovers). Measured with `examples/standings_load.rs` (128²
  world, 60 players, 4 finished seasons, 100 ms ticks, 200 clients, load
  generator on the same 4-core box; server CPU per request above idle):

  | Route | Before | After |
  | --- | --- | --- |
  | `/api/seasons/current` | 30.7k req/s, 25 µs | 57.3k req/s, 17 µs (off the world thread) |
  | `/api/seasons` (file / PostgreSQL) | 1.5k / 2.6k req/s, 572 / 335 µs | 9.4k / 8.5k req/s, 134 / 119 µs |
  | `/api/seasons/1` | 1.7k / 8.9k req/s, 538 / 111 µs | 34.0k / 30.8k req/s, 34 / 25 µs |
  | `/api/seasons/player/:p` | 1.7k / 3.4k req/s, 551 / 131 µs | 49.0k / 42.9k req/s, 17 / 15 µs |
  | `/api/leaderboard/seasons` | 120 req/s, 8.8 ms | 7.1k req/s, 155 µs |
  | Seasons-tab poll (3 requests) | 1.8k polls/s | 15.9k polls/s |

  At the web client's real rate (200 open tabs, one poll each 5 s), the
  seasons tab costs 1.9% of a core instead of 5.6%, the lineage tab's
  weather boards 2.1% instead of 35.5%. With `watch_season` the tab sends
  no standings requests at all; 400 watching sockets cost about 1.5% of a
  core more than 400 sockets that don't watch (7 pushes each in 30 s).
  Under closed-loop load the world thread's tick p99 rises after the
  change only because the server then answers 5–30× more requests on the
  same 4 cores; at equal load it is unchanged or lower.

### Economy and stud marketplace

The design doc's "other player must consent and may set a price" and "stud
marketplace for trading genetics", on a minimal economy (`economy.rs`,
`game.rs`).

- **Renown** is an in-game resource earned from play, never bought: a
  one-time **founding grant** of 50 when a player first founds a lineage;
  **daily income** at each legacy snapshot, `2 + round(100 × (share +
  weighted_share) / 2)` while the lineage has living members (a 2-renown
  stipend plus one renown per percentage point of the mean of raw and
  weighted legacy share; 10% raw and 6% weighted earn 10 a day, an extinct
  lineage nothing); **achievement bonuses**: species founded 100, niche
  dominance 40, survived a catastrophe 25, each year of lineage longevity
  20. Mating slots per season stay the scarce resource: renown only buys
  access to someone else's organisms, and every directed mating (bought or
  not) still spends the requester's slot.
- **Escrow.** A directed request with a price, or a listing purchase,
  moves the price from the payer's balance into escrow on the agreement at
  once (`insufficient_funds` otherwise). When the mating runs (`completed`,
  conceived or not: the preview shows fertility and failure chance up
  front) the escrow is paid to the partner's owner; every other ending
  (`declined`, `cancelled` by either side, `failed`, e.g. an organism eaten
  on the way, `expired` at the season change) refunds it. Trades are stored
  with the listing and the settlement (`paid` or `refunded`).
- **Ledger.** Every balance change is an entry (`grant`, `income`,
  `achievement`, `escrow` (negative), `sale`, `refund`) with the balance
  after it, a text, the offer and listing; sequence numbers are unique per
  world. The last 100 per player are kept in the game (the `ledger`
  command), all of them in the store.
- **Listings.** `create_listing` puts one of your adults on the market at a
  fixed fee (0 allowed; up to 1,000,000), optionally expiring after 1–365
  simulated days; at most 8 listings per player, one per organism. A
  listing is the owner's consent given in advance: `buy_listing` runs the
  same checks as a request (maturity, opposite sex, pregnancy,
  compatibility, the buyer's slots and open-offer limit, funds) and creates
  an agreement that is already `accepted`, with the fee in escrow; it tries
  the mating at once and otherwise waits for the two to meet, exactly like
  an accepted request (escort it from the Matings tab). A stud can serve
  several buyers at once; a dam takes one buyer at a time (`listing_busy`
  while promised), and a listed dam is held from breeding on her own while
  listed (she may be listed while pregnant; buyers wait until she has given
  birth). A buyer's organism can't be in two open agreements
  (`organism_busy`). Listings close when withdrawn, at their expiry or
  when the organism dies (the owner gets a `market` alert); agreements
  already bought stand.
- **Auctions** (`create_auction`, `bid`) are listings of the other kind:
  one mating with the organism goes to the highest bid when the auction
  closes, after 1 tick to 30 days. Bids start at the reserve (at least 1)
  and must beat the highest by 5% (at least 1 renown). A bid runs the
  purchase checks for the bidder's organism (`mine`) and moves the amount
  into escrow on the auction (`escrow` ledger entry with the listing and no
  offer); raising your own highest bid holds only the difference; an outbid
  bidder is refunded at once (`refund`) and alerted. The seller can't bid
  (`own_listing`) and can withdraw an auction only before the first bid
  (`auction_has_bids`); a fixed-fee `buy_listing` refuses auctions. While
  open, an auctioned dam (not allowed while pregnant) and the highest
  bidder's female are held from breeding. At the deadline the highest bid
  (never below the reserve) becomes an accepted agreement between the two
  organisms with the bid as its escrowed price (the bid is released from
  the auction and held on the agreement: a `refund` and an `escrow` entry),
  which settles like a purchase: paid to the seller when they mate,
  refunded otherwise. With no bid the lot returns to the seller; if the
  winner's organism can no longer mate with the lot (dead, pregnant, in
  another agreement, incompatible) the bid is refunded (`failed`); if the
  lot dies first the bid is refunded (`ended`). Open auctions and their
  highest bids are in the saved game with the other listings; each ending
  is stored (`auctions`, migration 6), and a `--fresh` start refunds bids
  still held in the discarded world, once, like open agreements' escrow.
- **Browsing** is paged: `browse_market` returns one page (20 by default,
  at most 50) of the open listings with what a buyer sees of the organism
  (key trait values, lineage and parents, generation, age, health,
  offspring raised, position, and an auction's reserve, highest bid, bid
  count, deadline and lowest next bid) and, given one of your organisms,
  the `preview_mating` compatibility with each. The server filters (kind,
  role, highest price, lineage, species, compatible with `mine`, leave out
  your own), sorts (newest, price either way, closing soonest, best
  fertility with `mine`) and pages with a keyset cursor: the last listing's
  sort key and id, so a page after it neither repeats nor skips listings
  when others are added or withdrawn meanwhile (a fertility order can shift
  as organisms age). A request with no fields (an older client) gets the
  newest 20 listings of every kind, so its reply stays bounded.

### Push notifications

Players hear about important events while the game's tab is closed, with
standard Web Push (`webpush/`): the client's service worker subscribes with
the Push API and the server's VAPID public key, the server encrypts each
message for the subscription (RFC 8291, `aes128gcm`) and POSTs it to the
browser's push service with a VAPID signature (RFC 8292). The crypto is
RustCrypto (`p256` for ECDH and ES256, `hkdf`, `aes-gcm`), the HTTPS client
`reqwest` on rustls with ring, so no OpenSSL; the `web-push` crate (0.11)
was passed over: it brings OpenSSL (the `ece` crate's default backend, and
libcurl or `hyper-tls` for its clients) and hyper 0.14 next to our hyper 1,
and leaves retries, `Retry-After` and coalescing to the caller anyway. The
encryption is checked byte for byte against the RFC 8291 example.

- **What is pushed** (`PushKind`): `mating` (another player asks to mate
  with one of yours), `death` (a founder or a possessed organism died: the
  game has no other "favourite" organisms), `market` (outbid, an auction won,
  sold or closed without a bid, a bid on your auction, a listing bought, a
  sale paid, a listing expired; not escrow payments you made, refunds after
  an offer, or achievement bonuses), `species` (a species founded by your
  lineage), `season` (a ranked season or an era ended, with your places) and
  `catastrophe` (one began; not its end). The game records a `Notice` next
  to the alert (`Game::take_notices`).
- **Never the world thread's problem.** The runner drains the notices after
  each command and tick, drops those of players with a connection to that
  world (they see it in the game) and hands the rest to its
  `WorldNotifier`: a `try_send` on a bounded channel (8192; a full queue
  drops the notice and counts it). One push service task (per process,
  shared by every world) does the rest.
- **Preferences.** Each kind has a push switch per account (`/api/push/prefs`,
  all on by default: subscribing a device is the opt-in), and an alert kind
  hidden from the feed (alert preferences) is never pushed (`mating` has no
  alert kind and follows its switch only). The service caches a player's
  preferences and whether they have a subscription for 5 minutes; changes
  through this server apply at once.
- **Coalescing and rate limits** (`webpush/coalesce.rs`): a notice opens a
  batch for its player that is released 30 s later, so a burst becomes one
  notification ("3 events · Main", the most important first, one a line,
  "…and N more" past four). Pushes to a player are at least 5 minutes apart
  and at most 4 in any rolling hour (`--push-per-hour`); notices arriving
  meanwhile join the waiting batch. A notification names its world
  (`[Isle] …` lines when a batch spans worlds) and carries the world and tab
  a click opens; its tag (`evo-<world>`) makes a newer one replace it.
- **Delivery** (`webpush/sender.rs`): one task per batch (16 at once), one
  request per subscription with `TTL` 12 h, `Urgency` high for mating and
  death, a VAPID JWT per push service origin (12 h, reused while an hour is
  left). 2xx delivered; 404/410 deletes the subscription; 429 waits
  `Retry-After` (seconds; past 10 minutes it gives up) or backs off; 5xx and
  network errors back off (2 s doubling, ±20 % jitter); 4 attempts per
  message; other statuses are logged and dropped.
- **VAPID key**: `--vapid-private-key` (raw P-256 scalar, base64url), else the
  one in the store (`server_secrets`), else generated on first start and
  stored there, so subscriptions survive restarts; several servers on one
  database agree on the first one written. Set `--vapid-subject` to a real
  `mailto:` or `https:` contact (push services may refuse the placeholder).
- **Subscriptions** are per account, global across worlds, at most 10 (an
  11th drops the oldest); the endpoint identifies one, so a shared device
  subscribing under another account moves to it. By default an endpoint must
  be `https` on a known browser push service (FCM, Mozilla, WNS, Apple), so
  the server can't be made to POST to arbitrary addresses;
  `--push-allow-any-endpoint` lifts that for tests and private push servers.

#### Native push in the apps

The Capacitor apps' WebViews have no Web Push, so the apps register with
their platform's push service through `@capacitor/push-notifications` and
hand the device token to the server (`POST /api/push/native/subscribe`):
Firebase Cloud Messaging for Android, the Apple Push Notification service
for iOS. The service task sends every coalesced notification to a player's
device tokens next to their Web Push subscriptions: the same notices,
preferences, coalescing and rate limits, the same title and body, and the
same click data (world, tab, kind, count, tag, organism; strings), so a tap
opens the same view. Each service is on only when its credentials are
configured; without them the server runs exactly as before (`GET
/api/push/key` has no `native` field and registering is refused with
`503 push_disabled`). Browsers keep Web Push.

- **FCM** (`webpush/fcm.rs`, HTTP v1 API): the service account's RSA key
  signs a JWT (RS256, with `ring`, already in the tree under rustls), traded
  at its `token_uri` for an OAuth2 access token (scope
  `firebase.messaging`, reused until 5 minutes before it runs out), then
  `POST https://fcm.googleapis.com/v1/projects/<project>/messages:send` per
  token: `notification {title, body}`, `data`, `android {priority HIGH for
  mating and death else NORMAL, ttl, collapse_key and notification tag per
  world, channel_id "evo"}`. 404 / `UNREGISTERED`, 400 `INVALID_ARGUMENT`
  about the registration token and 403 `SENDER_ID_MISMATCH` delete the
  token; 401 fetches a new access token and retries; 429 and 5xx retry like
  Web Push.
- **APNs** (`webpush/apns.rs`, token-based): an ES256 provider token (`p256`)
  from the team's `.p8` key, naming the key id and team, reused for 40
  minutes (Apple: at most every 20, at least every 60); `POST
  /3/device/<token>` over HTTP/2 to `api.push.apple.com` (or
  `api.sandbox.push.apple.com` with `--apns-sandbox`) with `apns-topic`
  (the bundle id), `apns-push-type: alert`, `apns-priority` 10 or 5,
  `apns-expiration` (the TTL) and `apns-collapse-id` (one per world); the
  body is `aps {alert {title, body}, sound, thread-id}` and the click data.
  410, 400 `BadDeviceToken` and `DeviceTokenNotForTopic` delete the token;
  403 `ExpiredProviderToken` signs a new provider token and retries; 429 and
  5xx retry.
- **Device tokens** are per account, global across worlds, at most 10 (an
  11th drops the oldest), one per service and token (a shared phone moves to
  the account that registered last), stored next to the Web Push
  subscriptions (`push_devices`). An FCM token must be URL-safe text, an
  APNs token hex (stored lower-case). Both bodies are cut to 3000 bytes so a
  message stays under the services' 4 KB limit.
- **The app** (`client/src/nativePush.ts`, `ui/push.ts`): the settings card
  is the same; Enable asks for the notification permission, registers
  (Android: creates the "Game events" channel first), waits for the token
  and sends it; the token is remembered with the server and player
  (`localStorage`), so the card shows "on" only for that account, sends the
  token again when it opens (it may have rotated) and Turn off unregisters
  and forgets it on the server. A tap (`pushNotificationActionPerformed`,
  kept by the plugin until the listener is added on a cold start) switches
  to the notification's world and tab, as a Web Push click does. The plugin
  is loaded only inside the apps (its own chunk). Registering for FCM
  without the Firebase config crashes the Android app, so the app only
  offers push when `client/android/app/google-services.json` existed at
  build time (or `VITE_EVO_FCM=1`); otherwise the card says push isn't set
  up in this build.
- **Tested** in Rust against a mock server (`crates/server/tests/native_push.rs`:
  the token request and its RS256 assertion verified with the key, the FCM
  message, the APNs request over HTTP/2 with its ES256 token verified, every
  answer above, gone tokens deleted, the HTTP API) and unit tests for the
  JWTs and request bodies; the client flows with fakes
  (`nativePush.test.ts`). Nothing has been delivered to a real phone: that
  needs a Firebase project, an Apple Developer key and devices.

**Setting it up.**

1. *Android (FCM).* In the [Firebase console](https://console.firebase.google.com/)
   create a project (or use one), add an Android app with the package name
   `org.evolutionmmo.client`, and download its `google-services.json` into
   `client/android/app/` (the generated project; git-ignored, so keep a copy
   and put it back after `npx cap add android`). Then `npm run cap:sync`
   (it builds first, which sees the file) and build the app. For the
   server, Project settings → Service accounts → *Generate new private key*
   gives a JSON key file: run `evo-server --fcm-service-account
   /path/to/key.json` (or `EVO_FCM_SERVICE_ACCOUNT=/path/to/key.json`, or the
   JSON itself in the variable). Keep the file private (it can send to every
   install of the app).
2. *iOS (APNs).* In the Apple Developer account, Certificates, Identifiers &
   Profiles → Keys, create a key with *Apple Push Notifications service*
   and download the `.p8` (once); note its key id and your team id. Give
   the app id `org.evolutionmmo.client` the Push Notifications capability,
   and in Xcode (`npx cap open ios`) add the *Push Notifications* capability
   to the App target and forward the token to Capacitor in
   `AppDelegate.swift` (as the plugin's README says):
   `didRegisterForRemoteNotificationsWithDeviceToken` posts
   `.capacitorDidRegisterForRemoteNotifications` with the token, and
   `didFailToRegisterForRemoteNotificationsWithError` posts
   `.capacitorDidFailToRegisterForRemoteNotifications`. Run the server with
   `--apns-key AuthKey_XXXXXXXXXX.p8 --apns-key-id XXXXXXXXXX --apns-team-id
   YYYYYYYYYY` (or `EVO_APNS_KEY` as a path or the PEM text,
   `EVO_APNS_KEY_ID`, `EVO_APNS_TEAM_ID`); `--apns-topic` if the bundle id
   differs; `--apns-sandbox` (`EVO_APNS_SANDBOX=1`) for builds signed for
   development, whose tokens only work against Apple's sandbox.

### Persistence

`Store` (`store.rs`) is an async trait with three implementations: a
JSON-file one (`FileStore`, the default, in `--data DIR`), PostgreSQL
(`PgStore`, `store/pg.rs`, selected by `--database-url URL` or the
`DATABASE_URL` environment variable; `--data`/`--memory` after it override
it) and an in-memory one (`--memory`, tests).

Accounts, alert and push preferences, push subscriptions and server secrets
are global; every other record belongs to one world (`shard`: the
configured world id). In the file store the global
tables stay at the top of the data directory and each world's files live in
`worlds/<id>/` (the paths below); in PostgreSQL every per-world table has a
`shard` column in its key (migration 7, `0007_worlds.sql`; 6 is the
marketplace auctions', 8, `0008_push.sql`, push notifications', 9,
`0009_push_devices.sql`, the apps' device tokens; 10,
`0010_history_retention.sql`, the daily history's `thinned` level).

| Persisted | File | PostgreSQL table |
| --- | --- | --- |
| Accounts (name, player id, argon2 hash) | `accounts.json` | `accounts` (name PK, player unique) |
| Founder designs (one per player per world) | `worlds/<id>/designs.json` | `designs` (PK `(shard, player)`, design `jsonb`, upsert) |
| One record per simulated day and world: legacy shares (raw and weighted), every lineage's size, births and deaths by cause that day, player lineages' mean gene values (the new fields default when reading older lines); thinned and sampled with age (see *Daily history retention* below) | `worlds/<id>/history/<n>.jsonl`, segments of up to 32 lines in write order (a `worlds/<id>/history.jsonl` from before segments is split into them on first use) | `daily_history` (PK `(shard, world_id, day)`, upsert; legacy and lineages `jsonb`; `seq` keeps write order; `thinned` 0–2, migration 10) |
| Finished directed-mating agreements with price and outcome, the listing they were bought from and how the escrow settled (`paid`/`refunded`) | `worlds/<id>/trades.jsonl` | `trades` (`bigserial` id, `shard`; `listing`, `settlement` from migration 2) |
| How each auction ended (seller, organism, reserve, deadline, bids, highest bid, winner and their organism, outcome `sold`/`unsold`/`failed`/`ended`/`withdrawn`, the agreement it became, day); a world replaying a day replaces its record | `worlds/<id>/auctions.jsonl` | `auctions` (PK `(shard, world_id, listing)`, upsert; `id` keeps write order; migration 6, `shard` from 7) |
| Renown balances, upserted on every change | `worlds/<id>/balances.json` | `balances` (PK `(shard, player)`, balance, day, updated_at) |
| Alert preferences (hidden and toasted alert kinds), upserted when a player saves them | `alert_prefs.json` | `alert_prefs` (player PK, hidden and toast `text[]` of kind names, updated_at; migration 3) |
| Web Push subscriptions (player, endpoint, `p256dh` and `auth` keys, label, created), one per endpoint | `push_subscriptions.json` | `push_subscriptions` (endpoint PK, indexed by player; migration 8) |
| Native app device tokens (player, service `fcm`/`apns`, token, label, created), one per service and token | `push_devices.json` | `push_devices` (PK `(service, token)`, indexed by player; migration 9) |
| Push preferences (the kinds switched off), upserted when a player saves them | `push_prefs.json` | `push_prefs` (player PK, off `text[]` of kind names, updated_at; migration 8) |
| Server secrets: the generated VAPID private key | `secrets.json` (mode 0600 on Unix) | `server_secrets` (name PK, value; insert-if-absent, so servers sharing the database agree; migration 8) |
| Finished ranked seasons: head (winners, summary) and full standings; a replayed rollover (same world and season) replaces the record in its place | `worlds/<id>/seasons.jsonl` (last line per key wins) | `seasons` (PK `(shard, world_id, season)`, upsert, `seq` keeps write order, head and standings `jsonb`) and `season_placements` (one row per player and category: rank, value, players ranked; indexed by player for the per-player history; rewritten with its season; migration 5) |
| The ledger: every balance change with world id and sequence number (a replayed entry, same world and sequence, replaces the earlier one) | `worlds/<id>/ledger.jsonl` | `ledger` (PK `(shard, world_id, seq)`, upsert; `id` keeps write order) |
| Saved games: the whole `World` except its static terrain, plus the game state around it (including players' alerts), every simulated day and at a graceful shutdown; the newest 3 kept | `worlds/<id>/snapshots/world-<tick>.snap` | `snapshots` (PK `(shard, tick)`, day, size, snapshot format version, `terrain_hash` (migration 4; NULL for saves from before world snapshot version 6), saved_at, data `bytea`) |
| Each world's static terrain (biomes, relief, rivers, ground cover: `Terrain::encode`), written once before the first saved game that names it; `--fresh` deletes it with the saved games | `worlds/<id>/terrain/<world id>-<hash>.terrain` | `terrains` (PK `(shard, world_id, hash)`, hash as 16 hex digits, size, saved_at, data `bytea`; migration 4) |

**Migrating single-world data.** Data written before several worlds
belongs to the default world, `main`, and nothing is lost: `FileStore::open`
moves the top-level per-world files and directories (`designs.json`,
`balances.json`, `history.jsonl`, `trades.jsonl`, `ledger.jsonl`, `seasons.jsonl`, `auctions.jsonl`,
`snapshots/`, `terrain/`) into `worlds/main/`, one rename each, so an
interrupted move finishes on the next open; a file found in both places (an
older server ran on the directory after the move) stops the start with a
message instead of guessing. `0007_worlds.sql` adds `shard` to every
per-world table with the default `'main'` for existing rows, drops the
default and rebuilds the keys and indexes with `shard` first (ranked seasons with their placements and auction results included). Keep a world
with the id `main` in the list to go on playing the old world (it is the
default when there is no list). `tests/persistence.rs` writes a real
single-world save and every record kind in the old layout (files) and the
0005 schema (PostgreSQL, through `PgStore::migrate_to`), opens the store,
checks every record is `main`'s and none another world's, then starts a
two-world server that continues the saved world as `main` beside a new
world; for the memory store (which has no old form) it carries a
single-world server's data into a two-world server. `tests/worlds.rs`
checks isolation end to end: commands on another world's organisms fail,
a listing isn't on the other world's market and can't be bought from it,
leaderboards, history, balances and ledgers are per world, and a restart
restores each world from its own save.

**Daily records and restarts.** Every world gets a random id when its map is
generated, kept in its saved games and written into each daily record. A
world restored from a saved game older than its last records (a crash
between saves) replays those days; the replayed record replaces the earlier
one of the same world and day (an upsert in PostgreSQL; `FileStore` appends
and keeps the last line per `(world_id, day)` when reading and compacting),
so the history has one record per day. A `--fresh` world has a new id.

**Daily history retention** (`history.rs`). A daily record has a row per
lineage, with every gene's mean for player lineages: ~690 KB a day at 500
players, which kept whole grew each world's history by ~45 MB an hour, ~1.2
GB a real day at the 1 s clock (the six-world soak). Every store now keeps,
by a record's age (days before the newest record; an older world's records,
before a `--fresh` start, age on from the newest world's day 0):

| Age | Kept |
| --- | --- |
| under 30 days | the whole record |
| 30–364 days (the dashboard's window) | every day; lineages with nothing to report that day (no members, births or deaths) dropped; gene means only on days divisible by 8 |
| 365 days and more | days divisible by 8 × 2^k (k: how many times the age doubled past 365 days: 8 up to 730 days, 16 up to 1,460, 32 up to 2,920, …), without gene means |

So a world keeps 365 consecutive days, the newest 30 whole and ~42 of the
rest with gene means, and ~46 sampled days per doubling of its age past a
year: at 500 players (690 KB whole, 102 KB lean records; `cargo test
--release -p evo-server --lib history_sizes -- --ignored --nocapture`) 61 MB
after the soak's 262 days, 90 MB after a real day (1,800 days; 1.2 GB
before), 113 MB after a month (37 GB before), 129 MB after a year. The
policy only removes detail as a record ages, and a day a larger stride keeps
every smaller one keeps too, so compacting again changes nothing and the
stores compact at their own cadence: the file store when a segment fills
(every 32 days: older segments rewritten through a temp file only when
they change, small neighbours merged, so the file count stays bounded too;
reads walk segments from the newest and stop when answered), PostgreSQL on
days divisible by 32 (`DELETE` the dropped rows, thin the rows whose
`thinned` level is behind: a replayed day resets it to 0), the memory store
on every record. The world thread's 365-day window for the genetics
dashboard is thinned the same way (previously 365 whole records in memory,
~250 MB of JSON at 500 players). What changes for readers: the dashboard's
gene-drift charts have a point every day for the last 30 days and every 8th
day before (they already plot by `history_days`); weather-season boards older
than a year are ranked on the sampled days (`days` counts them, first and
last day are the sampled ones; past ~4 years a season can have no sample and
drop out); `/api/leaderboard/history` returns thinned records as stored.
Reading an old `history.jsonl` works: the file store splits it into
segments after any existing ones and removes it (a crash before the removal
splits it again and the copies replace the first ones), then compacts; an
older server run on the directory afterwards would write a new
`history.jsonl`, which the next start appends the same way.

**PostgreSQL.** sqlx 0.8 (tokio, rustls, no compile-time query macros: the
queries are runtime strings, so building needs no database or
`DATABASE_URL`). The schema is in `crates/server/migrations/`, embedded in
the binary and applied by sqlx's migrator when the server connects (add a
new numbered file for a change; never edit an applied one). Game counters
are unsigned and stored as `BIGINT`. sqlx sits behind the `evo-server`
cargo feature `postgres`, **on by default**: the design doc puts
production persistence in PostgreSQL, so the normal binary should be able
to use a database without a rebuild, and it costs nothing at run time
unless a URL is given. `cargo build -p evo-server --no-default-features`
leaves sqlx (and its TLS stack) out for a faster, smaller build; a database
URL then stops the server with an error. The other default feature, `jemalloc`, makes
jemalloc the process's allocator (`alloc.rs`; its C build needs `make`,
which the Dockerfile installs): glibc malloc kept the memory freed after a
population peak (see *Memory after a population peak* under [Six worlds
for three hours](#six-worlds-for-three-hours-soak)); without the feature
the server uses the system allocator. The persistence tests
(`tests/persistence.rs`) run the store round trip (terrains included),
restart-keeps-accounts and restart-continues-the-saved-world (which also
deletes the stored terrain behind the server's back and requires it to
refuse to start) against the file store and, when
`DATABASE_URL` is set, against PostgreSQL too, each test in a schema of its
own (dropped afterwards); without it the PostgreSQL variants print that
they skipped and pass.

**World snapshots.** With the sim's `serde` feature (the server enables it;
the sim builds without it) `World::snapshot()` returns the whole simulation
as bytes, except the static terrain, and `World::restore(bytes, terrain)`
continues it **exactly**: all randomness is derived from `(seed, stream,
tick, id)`, so state plus tick determine the future. Format
(`world::snapshot`): a fixed header (magic, format version
`SNAPSHOT_VERSION`, flags, a hash of the whole `WorldConfig`, tick,
organism count, raw length, payload checksum, and from version 6 the
terrain's content hash) and the `World` in `bincode`, LZ4-compressed;
from version 11 the map's changing tiles are written apart, after it: per
field, each tile's bits XORed with its western neighbour's, split into byte
planes and deflated (`flate2` on its pure-Rust miniz_oxide backend, fast
level), on a second thread while the rest is encoded. Smooth fields
(temperature, wind, wetness) leave long runs of zero high bytes, so the
tiles take about 60% of what LZ4 made of them interleaved. Version 10
snapshots (the tiles inside the payload) and version 11 and 12 ones still
restore: 10 and 11 lack `Balance::cold_gestation` (added in version 12),
which then reads as 1.4, the rule they ran under (`serde_util::before_v12`),
and all three lack `Balance::nest_predation` (added in version 13), which
reads as 0 (`serde_util::before_v13`). Version 13 ones restore too: versions
10 to 13 lack the obedience gene and `Balance::tameness` (version 14), so
every organism's obedience locus reads as the design default, 0.5, tagged
like its wanderlust alleles (it behaves as it did), and `tameness` as its
default (`serde_util::before_v14`). Version 14 ones restore with no rally
standing (rallies came with version 15; `serde_util::before_v15`), and
versions 10 to 15 lack `Balance::migration_onset` (version 16), which
reads as 1, the rule they ran under (migrators leave on the first day of
autumn; `serde_util::before_v16`). Versions 10 to 16 lack `Balance::home_range`
(version 17), which reads as its defaults (`serde_util::before_v17`).
Versions 10 to 17 have every genome written whole (`serde_util::before_v18`;
see *Genomes by parents* below). Versions 10 to 18 lack `Balance::lactation`
and `Balance::warm_upkeep` (version 19), which read as 0, the rules they
ran under (`serde_util::before_v19`). Versions 10 to 19 lack
`Balance::fang_grazing` and `Balance::scale_cover` (version 20), which read
as 0, the rules they ran under (`serde_util::before_v20`). Versions 10 to
20 lack the `Balance::morph_*` tunables (version 21, *Body plan effects*),
which read as 0, no body plan effects (`serde_util::before_v21`). Versions
10 to 21 lack `Balance::directional_vision` and `Balance::vision_scan`
(version 22, *Directional vision*), which read as their defaults (0 and
1: off), and organisms' headings, which restore as each one's birth heading
(`serde_util::before_v22`). Versions 10 to 22 lack the genomes'
morphology segment (version 23): every organism, egg and father genome
restores with the default morphology, each locus's origin tags copied from
its anchor gene (eye loci from `Eyes`, ear loci from `Ears`, legs from
`LegLength`, tail shape from `Tail`, snout from `Nose`), species
representatives with the default morphology key, and
`Balance::morph_step_rate` as 0 (`serde_util::before_v23`). Versions 10
to 23 lack `Balance::morph_legacy` (version 24), which reads as 0:
morphology alleles stay out of the legacy tally (`serde_util::before_v24`).
Versions 10 to 24 lack `Balance::roaming_forage` (version 25), which reads
as 100, off, the rule they ran under and still the default
(`serde_util::before_v25`).
Saves keep the rules they were made under: a world saved with the
morphology tunables at 0 (every save from before *Morphology on by
default*, which hold them as 0 or read them as 0) restores with them at 0;
only new worlds get the new defaults.
**Static terrain** is stored apart: each tile is split into its fixed
`Ground` (biome, elevation, moisture, annual mean temperature, normal water
depth, river channel, sand, slope, undergrowth, canopy, most food it holds,
floodplain level) and its changing `Tile` (temperature, water depth,
current, ice, snow, mud, food, carcass meat, wind, rain, wetness, flood
silt); lakes are part of the ground (biome and normal depth), wind direction
is on the weather cells. The grounds and the river
catchments form a `Terrain` that the map holds behind an `Arc`: a clone of
the world (the server's saver takes one a day) shares it instead of copying
it, and a snapshot names it by content hash (FNV-1a 64 of its `bincode`).
`Terrain::encode`/`decode` give it its own blob (magic `EVOTERR\0`, format
version, hash, size, checksum, LZ4), stored once per world. Restoring needs
the terrain the header names: none is `SnapshotError::MissingTerrain`,
another one `WrongTerrain`, both naming the hash. Vegetation, water levels,
snow and ice, weather, drought and flood stay in every snapshot. Version 6
made the split (and carries lakes, floodplains, silt and wind direction).
**Saves from before version 6 don't load**: older maps lack the floodplain
and lake data, which map generation now derives differently and can't be
added to them deterministically, so the server treats them like any other
format version and generates a new world. Every state type derives serde (long gene arrays through
`serde_util`; hash maps are written sorted, so equal worlds give equal
bytes). Skipped and rebuilt on restore: the id → index map, the spatial grid,
A* scratch buffers, phase timings, and each organism's genome-derived caches
(phenotype, traits, recessive load: pure functions of the genome,
recomputed to the bit by `Organism::refresh_derived` as organisms are
read). Caches that steer the future (A* paths, hourly density, cohort
representatives, LOD passability, cache and clutch lookups) are stored.
From version 7 genomes are written compactly but exactly
(`genome::wire`: the genome's distinct origin tags once, then per allele a
2-bit class (exactly 0.0, exactly 1.0, same as the previous allele, or
stored), the origin as a `ceil(log2 tags)`-bit index (no bits with one
tag), and stored values bit-packed as a 3-bit exponent and the 23-bit
mantissa for values in [2^-7, 1), whole otherwise: about 30 bytes plus 3.25
per distinct value, against 816 bytes in memory); a pregnancy's father
genome is written as the index of the father while he is in the world
(`serde_util::father`) and shared with him again on restore; and the
payload's integers are varints. In memory an organism's genome is an
`Arc<Genome>` (immutable once born), shared by clones of the world and by
pregnancies, so `Organism` is 688 bytes instead of 1,496. Any change to a serialized type needs a `SNAPSHOT_VERSION` bump;
a snapshot of another version is refused with both versions named. Version
3 added the possession sub-step counter and `WorldConfig::possess_substeps`;
version 8 added the balance parameters `threat_caution`, `alert_guard` and
`pasture_search` to the saved config; version 9 the hunters' search image; version 10 added groups and
territories (`Organism::group`, migration orders per player and group,
claimed territories and claim times); version 11 writes the tiles apart
(above); version 12 adds the balance parameter `cold_gestation`, version 13
`nest_predation`; version 14 adds the `obedience` gene (a locus in every
genome and a value in every species representative) and the balance
parameter `tameness`; version 15 the world's rally orders; version 16 the
balance parameter `migration_onset`; version 17 the `home_range` balance
parameters; version 18 writes the genomes of
young and eggs against their parents' (*Genomes by parents*, below);
version 19 adds the balance parameters `lactation` and `warm_upkeep`;
version 20 `fang_grazing` and `scale_cover` (trait downsides); version 21
the body plan effects tunables `morph_effects`, `morph_tail_drag`,
`morph_tail_chill` and `morph_long_legs`; version 22 each organism's
heading and the balance parameters `directional_vision` and `vision_scan`;
version 23 the genomes' morphology segment (written whole after the
stored values, and against the parents as a second section with its own
exceptions and choice bits), species representatives' morphology keys and
the balance parameter `morph_step_rate`; version 24 the balance parameter
`morph_legacy`; version 25 the balance parameter `roaming_forage`.
`cargo test -p evo-sim --features serde snapshot` runs a busy world (LOD
with cohorts, an epidemic catastrophe, pregnancies, clutches, a possessed
organism with an order, disease, winter dormancy, caches, paths), snapshots
it mid-hour and mid-sub-step, restores it (and clones it, as the server's
saver does, which shares the terrain) and restores it once more from an
encoded terrain blob, runs all of them two more days and requires identical
worlds, byte for byte; it also checks version, checksum and header errors,
missing and wrong terrains, terrain blob errors, and that older versions
are refused by version.

| Agents (standard world, `examples/snapshot`) | Map | Stored (raw bincode) | Snapshot | Restore |
| --- | --- | --- | --- | --- |
| 1.5k | 256² | 3.8 MB (9.0 MB) | 22 ms | 14 ms |
| 5.7k | 256² | 7.7 MB (16.6 MB) | 48 ms | 40 ms |
| 21k | 512² | 29 MB (63 MB) | 215 ms | 188 ms |
| 26k | 512² | 34 MB (73 MB) | 250 ms | 219 ms |
| 37k cohort members, no agents (LOD) | 512² | 26 MB (63 MB) | 202 ms | 194 ms |

(Version 4, terrain included.) About 1.3 KB per agent stored (genome with
origin tags, phenotype and trait caches, path) plus about 13 MB for a 512²
map. Timed on a shared 4-core machine; `cargo run --release -p evo-sim
--features serde --example snapshot -- 512 5 2`. The same run times
`World::clone`, all the server's world thread now pays for a save: **40 ms
against 255 ms** for the snapshot at 26k agents.

**Static terrain stored apart** (same example and machine, the last
version 4 build and the terrain split interleaved, measured before lakes
and floodplains were merged in, under load from other jobs):

| 512², 2 days | Stored (raw) before → after | `World::clone` before → after | Restore before → after |
| --- | --- | --- | --- |
| 26.6k agents (scale 5) | 34.5 MB (72.9) → **29.8 MB** (60.3) | 37.6 → **32.3 ms** | 215 → 219 ms |
| 5.5k agents (scale 1) | 14.9 MB (35.3) → **10.2 MB** (22.8) | 11.6 → **3.9 ms** | 90 → 55 ms |

The terrain blob is 4.9 MB (12.6 MB raw), stored once per world and
encoded in about 35 ms. In a busy world agents, not terrain, are most of a
snapshot (about 1.1 KB each), so the daily save shrinks by 14% at 26k
agents and by a third at 5.5k. A clone now shares the terrain (the map's
clone fell from 9.4 to 1.2 ms) and gives the A* scratch buffers (17 bytes
per tile, `PathScratch`, contents never carried between searches) empty,
as a restore always did; the rest of a clone is the organisms (about 27 ms
at 26k agents). Simulation results are unchanged: the 12-seed sweep
(`evo-sim sweep --size 96 --trials 1 --seeds 12 --rounds 1 --seed 31
--threads 1`) prints the same table before and after, timing aside.

**Save stall on the world thread** (`cargo run --release -p evo-server
--example save_stall -- 512 5 2`: 26k agents, 512², medians of 7,
interleaved, on the same shared 4-core machine under load from other jobs,
so absolute numbers are noisy):

| | Tick that saves | Of which the save |
| --- | --- | --- |
| Before: encode on the world thread | 584 ms | about 440 ms (36.5 MB) |
| Now: `Capture` handed to the saver | 112 ms | 33 ms (the clone) |
| Ticks while the saver encodes | 104 ms | (share the cores with it) |

A plain tick measured 146 ms in the same run (load noise; 60 ms when quieter).
With the terrain stored apart, two interleaved rounds of the same example
gave a capture of 32.8 and 32.5 ms against 37.9 and 38.9 ms for the last
version 4 build, and a saved game of 31.9 MB against 36.5 MB (the 4.9 MB
terrain stored once per world).

**Leaner organisms (version 7)**: genome-derived caches left out, compact
genomes, father genomes by reference, varint integers, and genomes shared
behind an `Arc` (so a clone copies 688 bytes per organism, not 1,496, and
no genome). 512², scale 5, 2 days, 26.6k agents, the last version 6 build
and this one interleaved on the same shared 4-core machine (noisy):

| | Version 6 | Version 7 |
| --- | --- | --- |
| Snapshot stored (raw bincode), `examples/snapshot` | 30.1 MB (61.4 MB), 1,090 B/agent | **15.3 MB** (24.2 MB), 555 B/agent |
| `World::clone`, two runs | 30.5, 30.2 ms | **8.6, 7.9 ms** |
| `World::snapshot` / `World::restore` | 209–212 / 196–216 ms | 157–162 / 107–114 ms |
| Saved game (`examples/save_stall`) | 31.6 MB | **17.7 MB** |
| Capture on the world thread, three runs (medians of 5) | 47.5, 30.3, 29.3 ms | **17.2, 17.0, 9.2 ms** |

What is left of a 512² snapshot: genomes about 6.9 MB (two days in, most
alleles are still distinct founder values, which don't compress; in an
older world more of them are shared copies), the map's changing tiles
about 5.9 MB, the rest of the organisms about 2.5 MB. A restore is faster
too: recomputing the caches costs less than reading them. Simulation
results are unchanged: the 12-seed sweep (`evo-sim sweep --size 96 --trials
1 --seeds 12 --rounds 1 --seed 31 --threads 1`) prints the same table before
and after, timing aside, and the snapshot test also requires the restored
caches, paths and pregnancies to match the original's exactly, father
genomes to come back shared, and a clone to share every genome.

**Tiles apart (world snapshot 11, saved game 5)**: `snapshot::encode` of
a game, the last saved game 4 build and this one interleaved, two rounds
each, on the same shared 4-core machine under heavy load from other jobs
(load average 8–12, so times are noisy; medians of 15, or 7 at 512²):

| World | Saved game before → after | Encode before / after | File write + fsync before / after |
| --- | --- | --- | --- |
| 96², server default settings, day 30, 826 agents | 0.537 → **0.473 MB** (−12%) | 4.6–8.6 / 5.1–5.9 ms | 4–8 / 6–10 ms |
| 128² (`--size` default), day 30, 1,460 agents | 0.950 → **0.830 MB** (−13%) | 7.8–11.1 / 7.4–12.8 ms | 5–10 / 5–8 ms |
| 512², scale 5, day 2, 27k agents | 15.39 → **13.15 MB** (−15%) | 113–152 / 97–179 ms | 22–28 / 20–29 ms |

At 512² the tiles went from 5.6 MB (LZ4, interleaved) to 3.4 MB; the rest
of the world (9.8 MB, mostly genomes) is unchanged: deflate would take only
2% more off it at four times LZ4's time, and the pure-Rust zstd encoder
(`ruzstd`, fastest level) did worse than deflate there and on the tiles,
slower. Encoding costs about as much CPU as before (the tiles' planes and
deflate, ~45 ms at 512², replace their share of bincode and LZ4) but runs on
two threads, so with a free core a save encodes faster (97 against 130 ms
in the quieter round). A delta of the tiles against the map regenerated
from the seed was measured and rejected: weather, water and vegetation
have moved everywhere by the first save, and XOR against the generated
tiles came out 4% larger than against the western neighbour (it only helps
mud, water depth and current). The static terrain is deterministic from
the seed but stays stored in full, once per world (5.1 MB at 512²): it is
not part of each save, and a stored copy keeps old worlds restorable when
map generation changes.

**Tile deltas between saves: measured and rejected.** Saving the tiles as
a delta against an earlier save (full saves as keyframes, deltas between
them) was measured before building it (`cargo run --release -p evo-sim
--features serde --example tile_delta -- 512 5 2 3`: the keyframe on day 2,
then one save a day as the server makes them, 26–27k agents, the same
shared 4-core machine):

| Save after the keyframe | Snapshot | Tiles as stored (XOR west) | XOR the keyframe's tile | Keyframe, then west | Best of the three per field | Saved |
| --- | --- | --- | --- | --- | --- | --- |
| 1 day (day 3) | 16.82 MB | 3.351 MB | 3.367 MB | 3.347 MB | 3.007 MB | 0.34 MB (2.0%) |
| 2 days (day 4) | 17.57 MB | 3.350 MB | 3.481 MB | 3.445 MB | 3.034 MB | 0.32 MB (1.8%) |
| 3 days (day 5) | 17.51 MB | 3.322 MB | 3.532 MB | 3.486 MB | 3.038 MB | 0.28 MB (1.6%) |

A day of weather, water and growth changes nearly every tile: a delta only
helps the slow fields (water depth 320 → 185 KB, current 104 → 55, mud 83
→ 38, silt 38 → 28 KB at a day) and does worse on the fast ones (wind 643
→ 772, rain 468 → 653, wetness 585 → 702, snow 88 → 136 KB), so even
choosing the better encoding per field takes 2% off a save, falling as the
keyframe ages. Encoding costs the same (38–64 ms against 49–56 ms for the
tiles as stored, with snapshots of 150–240 ms), so save time doesn't move
either. Far below what would pay for saves that depend on each other
(keyframe-aware pruning in both stores, restores that need two saves), so
saves stay self-contained. Even a free delta would take only the tiles'
share, about 20% of a 17 MB save; the rest is organisms, mostly genomes.

**Genomes by parents (world snapshot 18).** Where a save's bytes go was
measured first (`cargo run --release -p evo-sim --features serde --example
genome_bytes -- 512 5 2 3`: every world field and every organism field as a
column, raw and LZ4-compressed alone, then the genomes in candidate
encodings; 512², scale 5, the saves of days 2 to 5). The 13.3 → 17.5 MB
growth after day 2 is not the organisms: it is the **clutches**, about
20,000 eggs from day 3, each a whole genome (4.3–5.5 MB raw, 3.7–4.8 MB
after LZ4). The organisms' genomes (6.7–7.2 MB, LZ4 takes 2% off) barely
change, and they really are the founders' values: a third of the alleles
are exactly 0.0, and of the rest 93% are values no other allele in the world
has (1.85 million distinct bit patterns in 2.88 million alleles on day 2, no
organism yet with a parent in the world). So a dictionary of exact bit
patterns can't pay: a global one would take 15.0 MB (21-bit indices plus
the values), one per locus 7.6–8.0 MB, column-wise raw bits 7.7 MB
deflated, against 7.1 MB as stored; residuals against each lineage's median
at the locus with entropy-coded lengths would come to 24 bits a value
against 26 (−15% of the genomes, about 1 MB, for an entropy coder in the
save path). Origin tags are already nearly free (0.14 MB). What does repeat
exactly is inheritance: an egg (or a young organism) has at every locus one
of its mother's two alleles and one of its father's, unchanged unless one of
its ~0.3 alleles mutated. So from version 18 a genome whose parents are both
in the world (earlier in the list, for an organism; anywhere among the
organisms, for a clutch's eggs) is written against them
(`genome::wire::Parents`): a byte counting the exceptions, each exception
as allele index, value bits and origin, then one choice bit per allele whose
parent is heterozygous at that locus (82 of 102 on average), about 11 bytes
against about 250; any other genome, or one with more than 16 exceptions, is
written whole as before. An organism's genome now precedes it in the
payload, and a clutch's eggs precede it (`serde_util::organisms`,
`serde_util::clutches`, `by_parents`); restores resolve each genome as it is
read, and are bit-exact (the busy-world test also requires genomes by
parents and whole ones, and `version_16_snapshots_restore_with_whole_genomes`
an older save to restore to the same world and snapshot the same way).

| `World::snapshot`, 512², scale 5 (both layouts interleaved in one process by a throwaway harness, medians of 9; 7 on day 10) | Version 16 | Version 17 | Snapshot CPU, all threads, 16 → 17 |
| --- | --- | --- | --- |
| Day 2, 27.0k agents, no eggs yet | 13.31 MB | 13.37 MB (+0.4%) | 155 → 162 ms |
| Day 3, 26.4k agents, 16k eggs | 16.82 MB | **13.62 MB (−19%)** | 211 → 196 ms |
| Day 4, 25.9k agents, 20k eggs | 17.57 MB | **13.70 MB (−22%)** | 216 → 202 ms |
| Day 5, 25.3k agents, 21k eggs | 17.51 MB | **13.67 MB (−22%)** | 219 → 200 ms |
| Day 10, 55.7k agents (26k with both parents in the world) | 22.73 MB | **16.55 MB (−27%)** | 262 → 281 ms |
| 128², scale 1, day 30, 1.4k agents (half with both parents) | 0.80 MB | **0.50 MB (−37%)** | |

The machine was under heavy load from other jobs (load average 20–24 on
4 cores), so wall times swung by ±50% between repetitions; CPU time (all
threads, and the calling thread alone) is the steadier measure, and it moved
by −9% to +7%: the smaller payload makes LZ4 cheaper, which pays for the
encoding against parents until most organisms are young (day 10). Restores
took about as long (213–287 ms against 216–483 ms, medians). The 25% target
was met only from about day 10 on and in older, smaller worlds; on day 2 there
is nothing to refer to (founders, no eggs), and on days 3 to 5 the saving is
19–22%: the rest of those saves is the founders' genomes (6.7 MB,
incompressible to within 15% as above), the tiles (3.4 MB) and the rest of
the organisms (3.4 MB). Simulation results are unchanged (the 12-seed sweep
prints 11.76 as before).

**Saved games** (`snapshot.rs` in the server): magic, the game-side state
as JSON (player → lineage, open offers with their escrow, migration orders,
alerts, watched organisms, milestone progress, chronicle entries already
announced, the birth and death counters at the last day boundary, and the
economy: balances, recent ledger, listings with open auctions and their highest bids, who had the founding grant)
and the world snapshot. The game JSON carries a layout `version`
(`SAVED_GAME_VERSION`, 2 since the economy, 3 since ranked seasons: the
season under way and the last finished record, 4 since auctions: open
auctions and their highest bids among the listings, none in versions 2
and 3, 5 since the JSON is deflated (magic `EVOSAV5\0`; a version 4 saved
game, magic `EVOSAVE\0` with the JSON as is and a version 10 world
snapshot, still restores, `snapshot::tests::version_4_saved_games_still_load`
on a stored fixture); a version-1 saved game still
restores, with balances and recent ledger entries from the store instead
(and no escrow on its open offers). A new world (no saved game, or
`--fresh`) also starts from the stored balances, and players who have one
don't get the founding grant again. `--fresh` refunds the escrow of the
discarded world's open agreements (`Game::refund_discarded`): those open
with escrow in its newest saved game, plus any escrowed in the store's
ledger after that save, less any that settled (paid or refunded) after it;
each refund is a `refund` ledger entry in the new world naming the old
offer and world, written before startup goes on, and an earlier refund of
the same offer is recognised, so a second `--fresh` from the same saved game
pays nothing twice. `--fresh` then deletes the discarded world's saved games
(their ticks are higher than the new world's, and a restart restores the
highest tick). A crash between saves restores the
balances of the last saved game, and the replayed days write the same
ledger sequence numbers again, replacing their earlier entries.
The world thread saves after the tick that starts a new simulated day and
on `Request::Shutdown`. It only takes a `snapshot::Capture` (a clone of the
`World`, sharing its terrain, plus the game state) and hands it to the
**background saver** (`saver.rs`, a dedicated thread), which encodes it
(bincode + LZ4, the tiles deflated on a second thread: the expensive part)
and passes the bytes to the store task,
so a save no longer stalls ticks (numbers below). Each save carries the
world's terrain blob, encoded once per world and terrain hash and then
shared; the store task writes it (`Store::put_terrain`, a no-op when the
store has it) before the first saved game that names it in each run, and
skips that saved game if it can't. One saver thread encodes one capture at a time
and sends them in capture order, and the store task writes in the order it
receives, so an older save never lands after a newer one; a capture that
arrives while an earlier one is still waiting replaces it (the older would
be superseded anyway, and memory stays bounded at two copies). The saver
also waits until the store task has written the save it sent last before
it takes the next capture, so a slow store (or a full disk) holds at most
one encoded save in the persist channel instead of a growing queue of
30–60 MB blobs, and a panic while encoding loses that save only. On shutdown
the world thread waits for the saver to encode the last save before it
replies, and the server waits for the store task to write it
(`Store::put_snapshot`; `FileStore` writes a temp file, syncs it and
renames it into place, then drops all but the newest 3; `PgStore` upserts
the row and deletes older ones in one transaction). On startup the
newest saved game that decodes is restored (older ones are fallbacks),
with the terrain its header names, read from the store by the saved game's
world id and that hash (`snapshot::terrain_key`). If none restores and one
of them names a terrain the store lacks or can't read, the server stops
with an error instead of generating a new world that would soon replace
them (put the terrain back, or `--fresh`);
the dashboard history is reloaded from the store, and only stored
designs whose player has no lineage in the world are founded. A saved world
for another seed, size, ticks per day, days per year or LOD setting stops the
server with an error (start with those settings, or `--fresh`); other changed
settings (balance, caps, catastrophes) apply to the restored world. With no
readable saved game (none yet, `--fresh`, or another format version) the
server generates a world from the seed and re-founds every stored design in
player order, as before (a design that no longer fits the budget is skipped
with a warning).

**Not persisted:** login sessions and connections (players log in again;
the level-of-detail focus of their views is dropped, so lineages nobody
watches fold into cohorts at the next day, as when everyone disconnects).
Possession survives a restart. Records written after the last saved game
(a crash between saves) are replayed by the restored world.

### Security notes

A review of the server (accounts, HTTP, WebSocket, game commands, economy,
store) found and fixed:

- **`--fresh` lost escrow, and could bring the old world back.** Open
  agreements' escrow in the discarded world is now refunded (see
  Persistence), and the discarded saved games are deleted: before, they kept
  their higher ticks, so the next plain restart restored the old world (with
  the escrow still held there).
- **Unbounded request sizes.** HTTP bodies were limited only by axum's 2 MB
  default and passwords not at all (argon2 hashes any length): now 64 KiB and
  1024 bytes. WebSocket frames were bounded only by tungstenite's 16 MB
  default (a frame is read whole before the 16 KiB message limit applies):
  now 16 KiB too. `GET /api/map.png?scale=8` on a 1024-tile map rendered a
  200 MB image on the world thread: the side is capped at 2048 px.
- **Unauthenticated work for the world thread.** Terrain, map, chronicle,
  history, seasons and lineage stats requests were unlimited and could fill
  the world thread's queue (starving players' commands): now rate-limited
  per client. One player could open any number of WebSockets, each a view
  the world thread builds a state for every interval: now 8.
- **Rate-limit bypasses.** An IPv6 client could use a fresh address of its
  /64 for every attempt: IPv6 peers are keyed by /64. The limiter's key map
  could grow without bound while every key was busy, and once past 4096 keys
  pruned the whole map on every request (O(n) per request): pruning is now
  amortized and the map is capped (65 536 keys; beyond, buckets are dropped,
  briefly forgetting some limits rather than running out of memory).
- **Login timing.** A login for an unknown name returned at once, one for a
  wrong password after an argon2 verify, which told which names exist:
  unknown names now verify against a dummy hash. (Registration still says
  `name_taken`, by design.)
- **Unpaid agreements.** If taking a price into escrow failed after the
  funds check, the offer was still opened with nothing held; it is now
  refused. Prices above the cap are refused before the pair checks.

A second, pre-launch review (anonymous clients and hostile players) found
and fixed:

- **Slow and idle connections were never closed.** `axum::serve` builds its
  hyper connections without a timer, so a request head trickled in a byte
  at a time (slowloris), or a connection that sent nothing, stayed open
  forever, and one address could open any number of them until the process
  ran out of file descriptors. The accept loop is now our own (`serve.rs`):
  a request head must arrive within 30 s (`ServerConfig::header_timeout`;
  idle keep-alive connections close after it too) and one client address
  (its /64 for IPv6) holds at most 128 open connections
  (`Limits::conns_per_ip`, lifted by `--no-rate-limits`); more are closed
  on accept. A trusted reverse proxy (`--trusted-proxy`) is not capped, as
  it carries everyone's connections. Tests:
  `slow_and_idle_connections_are_closed_and_capped_per_client`,
  `a_trusted_proxy_is_not_capped_per_address`.
- **Cache keys of any size.** `GET /api/seasons/:n?map=…` caches a miss
  under the `map` string; with hyper's ~400 KB request-head limit, one
  client within the read rate could fill the 1024-entry cache with about
  400 MB of keys. Map ids longer than 64 bytes are refused (`400 bad_map`).
- **World-thread work without a rate.** Opening a WebSocket (the world
  thread builds a full state for it, and an online/offline transition) and
  `POST /api/lineage` were not rate-limited: one player could churn their 8
  connections to keep the world thread busy. Both now take from the
  per-client read rate.
- **Malformed WebSocket messages** were answered without touching the
  command rate; they now count against it (`rate_limited` past it).
- **Security headers.** Pages and API answers now carry
  `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`,
  `Referrer-Policy: no-referrer` and `Content-Security-Policy:
  frame-ancestors 'none'; object-src 'none'; base-uri 'self'`. A script
  policy is left out on purpose: PixiJS 8 compiles shaders with `new
  Function` unless the client imports its `pixi.js/unsafe-eval` shim, so
  `script-src 'self'` would break rendering; the client never assigns
  HTML from data (no `innerHTML`), which is the XSS defence today.
- **Negative SQL offsets.** `?offset=18446744073709551615` became `OFFSET
  -1` in PostgreSQL (a 500 carrying the database's error text); counters
  past `i64::MAX` now saturate.
- **A credential echoed in an error.** `--apns-key`/`--fcm-service-account`
  given inline without its marker was printed whole in the "could not
  read" error; only the option is named now.

Checked again and judged acceptable (on top of the list below): every
WebSocket command acts only on the caller's own organisms, groups, offers
and listings (`possess`/`release`/`order_move` check ownership or
possession in the sim; a split names only the caller's organisms of that
group; only an offer's recipient answers it, only its parties complete or
cancel it, only the owner withdraws a listing; bids and purchases check the
buyer's organism); push unsubscribe and native unregister check ownership;
bid arithmetic can't underflow (a raise must beat the holder's own bid by
at least 1); outbound push requests go only to the known push hosts with
redirects off; nothing logs tokens, passwords, keys or URLs with tokens.

Checked and judged acceptable:

- **Double settlement.** Every command runs on the world thread one at a
  time, and an agreement is removed in the same step that settles it, so a
  second cancel, complete, decline or refund finds nothing (`unknown_offer`);
  a listed dam takes one buyer at a time. Only an offer's two parties can
  answer, complete or cancel it, only owners list, withdraw or use their
  organisms. Tests: `escrow_settles_once_and_only_the_parties_act_on_an_offer`.
- **Numbers.** Prices, fees and limits are `u64`/`u32`: negative, fractional
  or oversized values fail to parse (`bad_message`), prices and fees are
  capped at 1 000 000, debits are checked and credits saturate, so balances
  can't wrap; coordinates must be finite and are clamped to the map.
- **Tokens.** 256 bits from the OS RNG, compared by a `HashMap` lookup:
  SipHash with a per-process random key means an attacker can't steer which
  entries are compared, and a byte-wise comparison happens only on a full
  64-bit hash match, so there is no usable timing signal; a constant-time
  compare would add nothing. Sessions expire (idle 1 h, at most 24 h) and
  live only in memory. The WebSocket accepts the token in the query string
  because browsers can't set headers there; the server doesn't log URLs, but
  a proxy in front might.
- **Password hashing.** argon2id with the crate defaults (19 MiB, 2 passes,
  1 lane; OWASP's minimum), a random 16-byte salt, PHC strings; hashing and
  verifying run on the blocking pool behind the login rate limit and a
  bound on concurrent hashes (`--hash-concurrency`, `--hash-queue`).
- **CORS.** Any origin, no credentials: auth is a bearer token the page
  holds, never a cookie, so another origin can't act as a user it has no
  token for.
- **Static files.** `tower-http`'s `ServeDir` rejects `..` and absolute
  components; tested with encoded and backslash variants.
- **SQL.** Every PostgreSQL query is a constant string with bound
  parameters; nothing from a client is formatted into SQL.
- **Panics from client input.** Commands index offers and organisms only
  after looking them up; malformed JSON gets `bad_message` (WebSocket) or a
  4xx (HTTP).
- **Push notifications.** The server POSTs to URLs players give it, so a
  subscription endpoint must be `https` on a known browser push service
  host (or a subdomain of one), port 443, without credentials; redirects are
  not followed (`--push-allow-any-endpoint` is for tests and private push
  services only). Subscription keys are checked (a P-256 point, a 16-byte
  secret) before they are stored; the subscription list never returns them.
  A player can unsubscribe only their own endpoints. The generated VAPID
  private key is a server secret (`secrets.json` 0600, or `server_secrets`);
  anyone holding it could push to every subscriber, so back it up like the
  database. Pushes are rate-limited per player (above), so the game can't
  be used to flood a device.
- **Reverse proxies.** Behind a proxy every client connects from the
  proxy's address, which would make the per-client limits one limit for
  everybody: `--trusted-proxy` names the proxies whose `X-Forwarded-For` is
  believed (the last entry not itself a proxy; from any other peer the
  header is ignored, so a client can't pick its own key). The compose
  deployment trusts only Caddy's fixed address.
- **Left as is:** names are case-sensitive (`Ann` and `ann` are different
  accounts);
  `GET /api/leaderboard/seasons` reads up to 100 000 daily records per call
  (rate-limited now; a cache would be the next step); `accounts.json` is
  written with the process umask; after a crash and a restart from an older
  saved game, the store's balances and ledger can hold entries from the
  abandoned timeline until they are overwritten, which a later `--fresh`
  refund (that trusts the store after the newest save) inherits.
  From the second review: logout ends only the token it is given (no "log
  out everywhere"; sessions end within 24 h anyway); logins are limited
  per address, not per account (per-account throttling would let anyone
  lock a player out), so a botnet can guess one account's password at the
  hash bound's pace; a push subscription or device token that another
  account presents moves to it (they are capability secrets the browser or
  OS keeps; whoever has one can already receive those pushes); a WebSocket
  leaves the per-address connection count once upgraded (players are capped
  at 8); `GET /api/metrics` is public; renown passed between two accounts
  of one person still counts as sales for the season's ranking (wash
  trading; the economy itself can't be inflated, since every sale is a
  transfer); `ApiError::internal` still returns the error text (store and
  channel errors) to the client.

## Deployment

[`deploy/README.md`](deploy/README.md) is the guide for running a public
server on one machine: the repository's multi-stage `Dockerfile` (evo-server
in release and the client bundle in a `debian:trixie-slim` image, non-root,
health check on `/api/metrics`) and `deploy/docker-compose.yml` with
PostgreSQL and Caddy (automatic HTTPS, WebSocket passthrough), configured
from `deploy/.env` (`.env.example`: domain, database password, VAPID
subject, FCM/APNs key files). It covers sizing from the
[capacity](#several-worlds-capacity) numbers, the first start, upgrades
(migrations apply themselves at start-up), backups and restore of the
database (which holds the world saves too), logs, and push setup.

## Client

`client/` is build step 3: the browser client, TypeScript with PixiJS 8 for
the map and plain DOM/CSS for panels (no UI framework), bundled by Vite into
static files that `evo-server` can serve and Capacitor wraps for iOS and
Android (see "Mobile apps" below). It
only displays state and sends commands; the server stays authoritative.

```sh
cd client
npm install
npm run dev          # Vite on :5173, proxies /api and /ws to 127.0.0.1:8080 (EVO_SERVER=http://host:port to change)
npm run build        # typecheck + static bundle in client/dist
npm run typecheck
npm test             # Vitest unit tests
npm run e2e          # Playwright smoke, phone-emulation, onboarding, push and Android-driver tests (needs `npm run build` and `cargo build --release -p evo-server`)
```

Serve the built client from the game server (one origin, no proxy):

```sh
(cd client && npm run build)
cargo run --release --bin evo-server -- --fast --memory --static client/dist   # open http://127.0.0.1:8080
```

**Screens.**

- **Login / register** (`ui/login.ts`). On a server with several worlds
  it shows a **World** picker (from `GET /api/worlds`, with each world's
  size, day, population and players online; hidden when there is one
  world). It preselects the world the typed name played last on this server,
  else the last world chosen here (`worldChoice.ts`, `localStorage`), and
  the choice goes into the saved session, so a reload reconnects to the
  same world; `api.ts` adds `world=<id>` to every per-world call and the
  socket. The game's top bar names the world when there are several (log
  out to switch). The phone layout gets the same card at full width
  (`e2e/mobile.mjs` picks a world on a Pixel 7). The token is kept in
  `localStorage`; the server holds tokens in memory, so after a server
  restart the socket fails while the API answers, and the client drops back
  to the login screen. So does a `session_ended` message (expiry) or a 401
  from the dashboard endpoint.
- **Founder design** (`ui/design.ts`), shown when `welcome` says the player
  has no lineage: thermoregulation, birth type, offspring per birth,
  founders, a slider per gene from `GET /api/genes` (paid traits, then the
  free behavior, diet and preference genes), and a live point total against
  the 100-point budget with a per-gene cost. `cost.ts` mirrors the sim's
  `point_cost` and `validate` (same checks and error codes), so an invalid
  design can't be submitted; server errors (`over_budget` with its numbers,
  `already_founded`, …) are shown as returned.
- **World** (`ui/game.ts`, `world/worldView.ts`, `world/terrain.ts`,
  `world/terrainArt.ts`, `world/terrainLayer.ts`; the look is written down
  in `client/src/world/STYLE.md`): the tile map from `GET /api/terrain`,
  painted procedurally (no image assets) into soft, textured ground: the
  per-tile colours are blended bilinearly at a slightly noise-warped
  position, so biome edges and shorelines curve instead of following the
  grid; lakes come from a blurred water mask (rounded shores with a foam line,
  shallows lighter, depth darker, the bank's shadow on the water), rivers
  from a sharp channel mask linked across diagonal steps; grass and sand get
  grain, forests embossed crowns, mountains ridged relief, deserts wind
  ripples, ice cracks and snow drifts, all lit from the top-left. A base
  texture of the whole map (4 texels per tile up to a 2048 side) is painted
  once at load; over it, 16×16-tile chunks at 8 or 16 texels per tile (by
  device pixels per tile) are painted for the part on screen, a few rows
  per frame within a 3 ms budget, nearest the center first, kept in a
  24 MB LRU cache and repainted only when their tiles change (the base shows
  until a chunk is ready). The light of the hour tints the map (moonlit blue
  at night, rose at dawn, amber at dusk). The dynamic layers
  are refetched every 4 s from `GET /api/terrain/dynamic`; the base repaints
  progressively and the chunks whose tiles changed repaint. Colors: biome, darkened where grazed out; grass, forest,
  marsh and tundra tinted by season (fresh spring green, summer yellow-green,
  autumn brown, dull winter); land in drought yellows toward straw; lakes
  darken with depth, rivers are bright blue (a dry bed is sand, a dried marsh
  or lake shore cracked mud); flooded land is muddy blue; ice is pale blue;
  snow blends to white; hill shading from the north-west. The legend (a
  card over the map, toggled by the toolbar's **Legend** button, open by
  default on wide screens and closed on phones, the choice remembered) gives
  the drought, flood, snow and ice shares and keys the markers. Markers:
  a soft contact shadow and a thin owner ring at the feet (yellow for yours),
  a coral second ring when possessed, a green pip when pregnant, a white
  breathing ellipse for the selection; action icons show fleeing and hunting
  on anyone, resting and courting on yours only, and zoomed far out only
  fleeing. The view window is drawn as corner brackets. Organisms from `state`
  are drawn as **creature sprites by body plan** (`world/creature.ts`,
  `world/spriteArt.ts`, `world/sprites.ts`) from the server's `look`, in the
  style of the founder avatar: each look becomes an avatar model and is
  drawn by the creature art library (`client/src/art/`, "soft field guide":
  merged silhouette with one ink outline, one highlight and one shade rim,
  light from the top left) at its sprite level of detail (no fine
  textures), with a bolder outline (1.15 px at any size) and big heads so
  it reads at 15-40 px. Body radius by size class (juveniles at 65 % with a
  bigger head); diet in the muzzle (deep and round on herbivores, horns
  when large; square with a brow and fangs on carnivores), warm-blooded
  bodies round and upright, cold-blooded ones long and low with sprawled
  legs; fur (a soft clumped rim; feathers with wings), scales, armor plates
  or bare skin; drab, camouflage blotches, a bright crest and spots or
  yellow and ink-black warning bands; legs, or a dorsal fin and a tail fin
  for swimmers, gliding flaps or wings. The owner is an ellipse at the feet
  (yours yellow and stronger, wild grey, other players a stable hue each); a
  ring round the body when possessed (red) or pregnant (green); and an icon
  for what it is doing (z resting or dormant, ! fleeing, crosshair hunting,
  heart seeking a mate). Sprites face the way they last moved (your own by
  the server's heading; the texture faces right and is mirrored). The
  morphology in `look` shows too: 0–8 legs, ears (0, 2 or 4; round, pointed,
  long or fan, three sizes; round ears stay flush on cold-blooded and
  swimming bodies), eyes (count, size, and placement: forward-set near the
  snout with the far eye showing, side-set high and back), a thin, bushy,
  fin, club or prehensile tail or none in three lengths, three snout lengths
  and small or large fangs (tusks on herbivores); a look without those
  fields draws the contract defaults (a test checks it draws exactly as the
  defaults sent, and a golden hash pins the model of every class). Each
  distinct drawing is rasterised once onto a canvas texture
  (`art/canvas.ts`, the house-style renderer for canvas: same curves and
  layers as the SVG one, supersampled so it stays crisp as markers grow with
  zoom); a marker is four sprites sharing those textures, so hundreds of
  organisms cost no per-frame drawing.

  **Creature textures at scale.** Every morphology field is quantized (and
  what is not drawn, such as a swimmer's legs or an absent tail's length,
  collapses to one value), so the key space is bounded: 864,000 morphology
  variants per class and stage in theory, but a view holds what its
  organisms are, and organisms of a lineage mostly share a look. A
  simulated crowded view (300 organisms from 40 random lineages, each
  feature mutated in 10 % of them) needs 186 body textures near and 77 far.
  Three measures keep that cheap: **detail levels** (markers drawn below
  scale 1, i.e. zoomed out, use the class sprite with morphology at its
  defaults, `far`, so a wide view shares one texture per class); a
  **bounded cache**, least recently used first, of at most 256 textures and
  48 MB (estimated width × height × 4), which never drops a texture a
  marker shows (so the bound is soft) and on overflow frees down to 75 % so
  eviction is rare; and a **frame budget** (`TEXTURE_BUDGET_MS`, 4 ms):
  markers whose textures are cached always update, the others (yours
  first) wait for a later frame once a frame has spent the budget making
  textures, so a zoom in or a crowd arriving spreads the drawing over a few
  frames instead of hitching. `spriteStats()` reports textures, made,
  evicted, bytes, time making them (total and slowest) and deferred
  markers. A lost WebGL context still clears the whole cache, and markers
  rebuild from their current look on restore.
  Markers live in screen space so they stay readable at any zoom (scaled
  0.75–1.9× with it), and move smoothly between state pushes (see "Smooth
  movement" below). Drag to pan, wheel,
  pinch or +/− to zoom; **Follow** keeps the camera on the server's view window (your
  possessed organism or your lineage's centroid), drawn as a blue square.
  Click an organism to inspect it: yours show action, sex, age, stage,
  health, lineage and generation, and the eight needs as bars; others show
  only what the protocol reveals, plus the body plan in words. **Possess** /
  **Release** (or hold the mouse / long-press on one of yours); while
  possessing, clicking empty map sends `order_move` (target line drawn).
  The toolbar groups its tools (camera: Follow, zoom; group commands;
  view window; Legend), icons with labels.
  **Migrate…** then a click sends the group command; **Pin view** sends
  `set_view` at the camera center. Group commands: the toolbar's group
  menu picks which group **Migrate…**, **Clear** (the migration), **Territory…**,
  **Rally…** and **Split** act on ("All" reaches every group; the territory,
  rally and split tools want one group, the only one if there is just one).
  **Rally…** (toolbar or a group's card) is one tap on the map, which sends
  `rally`; the card then offers **Cancel rally**, and the rally is drawn
  as a ring in the group's color around a filled point. **Territory…**
  is two taps: the center (the largest circle the group may hold is
  previewed), then the edge, which sends `claim_territory` with that radius;
  **Split** sends `split_group` and picks the new group. Each group's
  territory is drawn as a tinted disc in its color (the picked group's
  outline bolder) and its migration target as a ringed cross; the
  inspector names an organism's group. Each group's card says how it heeds
  orders, from its mean obedience gene ("headstrong" up to 0.35,
  "obedient" from 0.65, "heeds orders" between). On phones the same tools sit in the
  toolbar's **Tools** menu and both taps work by touch. The logic (names, colors,
  the menu, the two-tap draw, the commands sent) is in `groups.ts`.
- **Matings** tab: request a directed mating (one of your adults, a partner
  id filled from the inspector's "Request mating…", optional price in renown,
  held in escrow);
  incoming requests with Accept / Decline; your open offers with Cancel and,
  once accepted, **Complete now** and **Escort** (possess your organism and
  walk it to the partner's last seen position, the trip to the meeting);
  recent outcomes with conception, mismatch, fertility and failure chance.
- **Market** tab (`ui/market.ts`, data prep in `ui/marketData.ts`): the
  renown balance (with escrow) and slots left; the **stud marketplace**,
  paged (20 listings a page, **Load more** for the next, "Showing 20 of
  57"; the 5 s refresh while open re-fetches the pages loaded): pick one of
  your adults and every listing shows its compatibility with it
  (`browse_market` with `mine`); filter by role (studs/dams), kind (fixed
  fee/auctions), maximum price and "compatible only", and order by best
  match (compatible first by fertility), newest, cheapest, priciest or
  closing soonest, all applied by the server; listings show the owner and
  lineage, an appearance summary from the key traits ("herbivore, furred,
  large"), trait bars, generation and parents, age, health, offspring
  raised, position, sales, expiry, and a **Buy** button that says why it is
  disabled (your own listing, already promised, wrong sex, pregnant, can't
  conceive, no slots, not enough renown); an auction shows its reserve or
  highest bid (yours or not), bid count and time left, and a bid field
  (preset to the lowest accepted bid) with a **Bid** button that says why
  it is disabled the same way (also: below the lowest bid, not enough
  renown counting what you already hold on it); **Show** centers the map on the
  organism. Below: list one of your adults at a fixed fee (optional expiry
  in days) or at auction (reserve, length in days), your listings (all of
  them, from `with_own`) with **Withdraw** (disabled for an auction with a
  bid), and the **ledger** (recent totals and
  entries with signed amounts and the running balance). Bought agreements
  appear in the Matings tab to escort; sales, payments and refunds arrive
  as `market` alerts (toasts and the alerts feed).
- **Genetics** tab (`ui/dashboard.ts`, `ui/charts.ts`), from
  `GET /api/lineage/stats` every 5 s while open: count tiles; a population
  pyramid by age band and sex; births (up) and deaths stacked by cause (down)
  per day; legacy share and weighted share over the recorded days; gene
  drift sparklines for the six genes that moved most since founding (dashed
  founding value); **whose genes** your organisms carry (a stacked bar and
  list of allele shares by origin: you, the wild, other players and their
  lineages, the smallest folded into "others"); and a table of every gene
  with its mean, SD and **distribution**, a ten-bin histogram over 0–1 with
  the founding mean dashed. Hand-rolled SVG (no chart library), fixed
  categorical colors, a `<title>` tooltip on every mark. Below it the
  **alerts feed**: server alerts (births, deaths of founders and possessed
  organisms, milestones, needs, daily summaries) merged with mating offers
  and their outcomes, newest first, organism ids linking to the inspector;
  an unread badge on the tab. **Alert preferences** (a collapsible table
  above the feed) switch each alert kind's *Show* (in the feed and the
  badge) and *Toast* (pop up on arrival); deaths, needs and market alerts
  toast by default. **Push notifications** (a second collapsible card,
  `ui/push.ts`) say what this device can do (no Push API or a native app
  shell, no https, push off on the server, notifications blocked, off, on),
  offer *Enable on this device* / *Turn off on this device*, and switch each
  push kind (mating offers, deaths, market, new species, season results,
  catastrophes) for the account; the card also counts the account's
  devices. Enable asks for the notification permission only when pressed.
  A click on a notification opens the game on its world and tab (an open
  game window switches world, restarting the session there, and tab; a new
  one opens with `?world=…&tab=…`).

  *Allele origins* need no new simulation state: every allele already
  carries the id of the player it originated with (founder alleles the
  designer's, mutations the owner of the organism they appeared in), and
  the tag passes unchanged through any mating, which is what the legacy
  score counts. The server tallies the tags over the lineage's living agents
  (gene and hidden-recessive loci) when the dashboard asks, so the snapshot
  format is unchanged. Histograms and origins cover agents only, like the
  other gene statistics (cohort members of abstract regions have no
  genomes). *Alert preferences* are kept server-side in the `Store` so they
  follow the account across devices and reinstalls; the server still records
  and pushes every alert and the client applies the preferences (filtering
  the feed and deciding toasts), so a kind switched back on shows its recent
  alerts again and the game code needs no change. Offer events are not
  alerts and always show.
- **Lineage** tab: lineage size (with level of detail on, also how many of
  them are agents on the map; the Genetics tab adds an "On the map" tile),
  adults and young, pregnancies, newest
  generation, mating slots left this season, migration target, a **Groups**
  card per group (size, adults, what it is doing, and Pick, Claim
  territory…, Abandon territory, Split, Merge into A, Center), the legacy
  leaderboard (share and weighted share per player, yours highlighted), the
  **seasonal leaderboards** (`GET /api/leaderboard/seasons`, the current
  season open) and a clickable list of your organisms. The top bar shows the
  clock, season, day/night, weather, a red catastrophe banner while one is
  under way (from `state.weather.catastrophe`), the renown balance (with
  escrow; a click opens the Market tab) and the connection state.
- **Seasons** tab (`ui/seasons.ts`, pure helpers unit-tested in
  `seasons.test.ts`), from `watch_season` pushes (the standings under way),
  `/api/seasons` and `/api/seasons/player/:me` (on opening, at each rollover
  the pushes show, at most every 5 s, and every 30 s; the current season
  too while the socket is down; a push redraws only the season under way,
  so opened seasons below stay put): the ranked season under way
  (clock, days left, your places, the top 5 per category plus your row), your
  record (seasons ranked, legacy crowns, titles, best place per category,
  recent placements), and the hall of fame (most titles; finished seasons
  newest first with summary and winners, each opening onto its standings
  from `/api/seasons/:n`, "Older seasons" for the next 10). On phones the
  seven tabs share the bar at 11 px (`e2e/mobile.mjs` checks that the bar and
  the opened standings don't overflow on iPhone 13 and Pixel 7; the smoke
  test runs 3-day seasons and checks the tab and the API).
- **World** tab (`ui/chronicle.ts`), from `/api/species`, `/api/eras`,
  `/api/catastrophes` and `/api/achievements` every 10 s while open: the
  active catastrophe and what it does, the era clock, finished eras'
  leaderboards (your rank, niche leaders), the species tree (daughter
  species under their parents; diet, estimated adults or extinction, dominant
  allele owner and share, founder), the catastrophes with population before
  and after and how many player lineages survived, and achievement tallies
  per player with the latest achievements.
- **First-session onboarding** (`ui/onboarding.ts`; steps, state machine,
  hint triggers and storage pure in `onboarding.ts`, unit-tested in
  `onboarding.test.ts`). After the first login and founding a short guided
  tour opens by itself: welcome (the three levers and the legacy score), the
  map and camera toolbar, finding your lineage (Lineage tab), the inspector
  (one of your organisms is selected), possessing (pointing at Possess, or
  the floating button on phones), matings and the Market, Seasons, alerts
  (the Genetics tab's badge and feed), and Help. Each step highlights its
  target (a ring that dims the rest but never takes the pointer) beside a
  small non-modal dialog (`role="dialog"`, labelled by its title) with Back,
  Next / Finish and Skip; focus goes to Next, Esc skips, ←/→ step, and on
  closing everything is removed and focus returns where it was. On a wide
  screen a step opens its tab; on a phone the sheet stays closed, the card
  spans the width less 16 px gutters above or below the target, and nothing
  scrolls sideways. After the tour, **one-time hints** appear in the map's
  corner the first time something new happens: a pregnancy in your lineage,
  a death (a `death` alert), a species split (a "Species founded"
  achievement) and a mating offer received; "Show me" opens the tab that
  explains it. Hints announce through a polite live region and never take
  focus. A **Help** button in the top bar ("?" on phones) opens a modal
  dialog (focus trapped, Esc or a click outside closes it) with the controls,
  **Replay the tour** and **Show tips again**. Progress (tour new / active
  at step *n* / completed / skipped, and the hints seen) is kept per account
  in `localStorage` (`evo.onboarding.v1.<player>.<name>`, every access in
  try/catch; without storage it lasts for the page), so the tour shows once
  per user and device and resumes at its step after a reload mid-tour. It
  is client-only: the server is unchanged. `?tour=off` (or
  `?onboarding=off`) turns the tour and the hints off for the page; the
  other e2e tests open the client with it.

**Smooth movement** (`world/motion.ts`, pure and unit-tested; wired in
`worldView.ts`). The server pushes `state` once per state interval, and
after each possession sub-step (a quarter tick) to the owner of a walking
possessed organism; drawn as received, markers would hop.

- *Interpolation, every marker.* A new server position starts a glide from
  where the marker is drawn to that position, lasting as long as the
  previous move took (from when its position last changed; clamped to
  50–1200 ms). An organism that moves once a tick is therefore always
  moving and arrives as the next position comes in: continuous, one message
  behind. Repeated positions (a state that isn't a move) keep the glide
  going; a jump of more than 6 tiles (a reconnect) snaps. Per marker this is
  a handful of numbers and a lerp per frame, nothing allocated per state.
- *Prediction, the possessed organism.* The state carries its `speed`
  (tiles per tick on open ground: genes, size, condition, sprint and the
  possession bonus, from `World::move_speed`) and its move `order`; the
  client measures the tick length from `clock.tick` against arrival times
  (`TickClock`). The drawn organism walks on from its last server position
  straight toward the order at that speed, starting the moment the player
  clicks (before the server confirms the order). Bounds, so it never shows
  the organism where the server wouldn't put it: at most half a tick of
  movement ahead of the last server position; stopping at the arrival
  distance (0.75 tiles) and before the first tile that isn't plainly open
  ground in the client's terrain (water, a mountain, flooded land, an
  unfrozen river: the server knows the body and routes around them,
  straight-line prediction can't); and no prediction at all once the server
  has the order but its action isn't `ordered` (thirst, hunger or danger
  took over), when its last move left the straight line (a route around an
  obstacle), when it hasn't moved for 1.5 ticks (blocked), after arrival or
  release, when the server refuses the order, or when an unconfirmed order
  is 1.5 s old. *Reconciliation:* the drawn position eases toward the
  predicted one (or, without prediction, toward its glide) with a 90 ms time
  constant, so a server correction slides; it snaps only beyond 3 tiles.
  The camera, when following, follows the drawn position, and the order
  line is drawn from it (to the server's order after a reload).

**Phones and touch.** Below 760 px wide the map fills the screen and the
sidebar becomes a **bottom sheet** with a tab bar at the bottom (48 px
targets, safe-area insets): a tab tap slides the sheet up over the map, a
second tap on the open tab (or the handle, or a tap on empty map) closes
it. Tap an organism to select it; a floating **Possess #id / Release #id**
button appears for your own, and a long press (450 ms held still, or the browser's own long-press gesture) on one of
yours possesses it directly. One-finger drag pans, two-finger pinch zooms
about the fingers' midpoint (and pans with it). The top bar shrinks to the
clock, connection and log out; the toolbar keeps the camera buttons (icons)
and folds the group commands and **Pin view** into a **Tools** menu; the
legend starts hidden behind the **Legend** button; only the two newest toasts show.

**Mobile apps (Capacitor).** `capacitor.config.ts` wraps `dist/` as
`org.evolutionmmo.client`; the native projects are generated locally and
not committed (`client/android/`, `client/ios/` are git-ignored):

```sh
cd client
npm run build
npx cap add android        # once; needs Android Studio / SDK to build
npx cap add ios            # once; needs macOS with Xcode to build (Swift Package Manager project)
npm run cap:sync           # build + copy dist/ and plugins into both projects (after every client change)
npx cap open android       # or: npx cap run android / npx cap open ios
```

Without Android Studio, the command-line SDK is enough for a debug APK
(this is what the CI job does; the SDK goes under the git-ignored `target/`):

```sh
# from the repository root
export ANDROID_HOME=$PWD/target/android-sdk
mkdir -p $ANDROID_HOME/cmdline-tools
curl -sSfL -o /tmp/cmdline-tools.zip https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip
unzip -q /tmp/cmdline-tools.zip -d $ANDROID_HOME/cmdline-tools && mv $ANDROID_HOME/cmdline-tools/cmdline-tools $ANDROID_HOME/cmdline-tools/latest
yes | $ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager --licenses >/dev/null
$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager "platform-tools" "platforms;android-36" "build-tools;35.0.0"
(cd client && npm run build && npx cap add android)    # or `npx cap sync android` once added
(cd client/android && ./gradlew assembleDebug)         # → app/build/outputs/apk/debug/app-debug.apk
```

Verified on Linux with JDK 21 (Capacitor 8.6.2's generated project as is:
AGP 8.13.0, Gradle 8.14.3, minSdk 24, compile/target SDK 36; no project
changes needed): `assembleDebug` succeeds and gives a 5.9 MB APK for
`org.evolutionmmo.client` (4.4 MB with only the `INTERNET` permission before
the push plugin; see below); with
`EVO_CAP_CLEARTEXT=1 npx cap sync android` the built manifest carries
`android:usesCleartextTraffic="true"` and the app's config
`allowMixedContent: true` (a plain `npm run cap:sync` turns both off again,
so rerun it with the variable when you switch). It has not been played on
a physical device yet; an emulator smoke run (below) covers the basics.
iOS needs macOS with Xcode. Without an Apple account, an unsigned
simulator build is (this is what the `ios` CI job does):

```sh
cd client && npm run build && npx cap add ios          # or `npx cap sync ios` once added
scripts/ios-build-sim.sh                               # → ios-out/sim/App.app (Debug, iphonesimulator, CODE_SIGNING_ALLOWED=NO)
xcrun simctl install booted ../ios-out/sim/App.app && xcrun simctl launch booted org.evolutionmmo.client
```

`npx cap add ios` works on Linux too (it only writes the project: checked,
Capacitor 8.6.2 makes `ios/App/App.xcodeproj` with a `CapApp-SPM` package
that includes the push plugin), but the build itself has only run in CI.

**Push notifications in the apps** use FCM and APNs, not Web Push; the
Android app needs `android/app/google-services.json` from a Firebase project
before `npm run cap:sync`, the iOS app the Push Notifications capability and
two `AppDelegate` hooks (see [Native push in the apps](#native-push-in-the-apps)).
Without them the apps build and run as before, and the settings card says
push isn't set up. The debug APK with the plugin (Firebase Messaging) builds
the same way (it adds `ACCESS_NETWORK_STATE`, `POST_NOTIFICATIONS`,
`WAKE_LOCK` and the C2DM receive permission); no `google-services.json` is
needed to build it (checked with `assembleDebug` on Linux, JDK 21, without a
Gradle daemon left over from another SDK location: `--no-daemon`).

**Backgrounded apps.** A phone that sleeps, or switches between Wi-Fi and
cellular, can leave the game socket dead without a close event, and a
backoff that grew while away could keep a resumed app waiting up to 15 s.
The server pushes a state every state interval (1 s by default), so
`GameSocket` abandons an open socket that has heard nothing for 20 s and
reopens at once, and `main.ts` calls `socket.wake()` when the page becomes
visible again, gets focus, comes back `online`, or receives Capacitor's
`resume`: a pending retry runs immediately, and an open socket that
delivers nothing within 4 s is reopened (unit-tested in `socket.test.ts`
with a fake WebSocket, and by the emulator smoke run below; not yet seen
on a physical device).

**Emulator smoke run.** `client/e2e/android-emulator.sh [apk]` checks the
app on an Android emulator (or a device) against a local server, and is
what the `android-emulator` CI job runs:

```sh
cd client && npm run build && EVO_CAP_CLEARTEXT=1 npx cap sync android && (cd android && ./gradlew assembleDebug)
emulator -avd <your AVD> -no-window -gpu swiftshader_indirect -no-snapshot-save &   # needs KVM (or a device on adb)
cargo build --release -p evo-server
client/e2e/android-emulator.sh       # default APK: client/android/app/build/outputs/apk/debug/app-debug.apk
```

It waits for the device to boot, turns animations off, starts `evo-server
--fast --memory --size 64 --no-rate-limits` on `127.0.0.1:8080` (the
emulator reaches it as `http://10.0.2.2:8080`), installs the APK, records
logcat, and per attempt (2 by default, each after `pm clear`) launches the
app and runs `e2e/android-webview.mjs`. That script forwards the WebView's
DevTools socket (`webview_devtools_remote_<pid>`; WebView debugging is on
in debug builds) and drives the page over the DevTools protocol with
`e2e/android/cdp.mjs`, a small dependency-free client (Playwright's
`connectOverCDP` wants a whole browser, which a WebView socket is not).
The scenario (`e2e/android/scenario.mjs`): the login screen appears (Server
field open on a native shell); the Server field gets the test server and a
new player registers (typed with `Input.insertText`, buttons tapped with
real input events); the founder design founds a lineage; the map is up with
own organisms on a `connected` socket and state frames counted by a probe
around `WebSocket`; the tour, when it opens, is skipped; a page screenshot
of the map must show more than a flat color; then HOME, 25 s away (over the
20 s watchdog), the launcher intent brings the app back, and within 30 s the
socket must be connected again with new ticks and a frame under 3 s old
(the reconnects, the page's visibility/focus events and the recovery time
go into `report.json`). Screenshots (whole screen from `adb screencap` and
page from DevTools), `logcat.txt`, `logcat-app.txt` (Capacitor, chromium
and crash lines), `server.log` and `report.json` land in `OUT` (default
`client/e2e/out/android`). Env: `ADB`, `ANDROID_SERIAL`, `EVO_PORT`,
`EVO_BIND`/`EVO_APP_SERVER` (a device on the LAN: `EVO_BIND=0.0.0.0
EVO_APP_SERVER=http://<host IP>:8080`), `EVO_APP_PAGE` (page URL regex, for
an `EVO_CAP_LIVE_URL` build), `ATTEMPTS`, `BG_SECONDS`.

The same scenario runs against desktop Chromium in `npm run e2e`
(`e2e/android-desktop.mjs`): a phone-sized page served from its own origin
like the app's, the server typed into the Server field, the raw DevTools
client on `--remote-debugging-port`; "background" freezes the page (Page
Lifecycle) and silently drops its connections through a proxy without
closing them, so it must come back through a reconnect. The shell script
was checked against a fake `adb` forwarding to that desktop Chromium
(install, `pm clear`, forward retries, a failed first attempt and a passing
second, HOME/resume, screencap, logcat filtering). Written on a machine
without KVM, so the emulator itself only runs in CI.

**Simulator smoke run (iOS).** `client/e2e/ios-simulator.sh [App.app]`
runs the same scenario in an iOS simulator, and is what the `ios` CI job
runs (macOS with Xcode only):

```sh
cd client && npm run build && EVO_CAP_CLEARTEXT=1 npx cap sync ios && scripts/ios-build-sim.sh
npm install -g appium@3.9.0 && appium driver install xcuitest@12.22.7
cargo build --release -p evo-server
client/e2e/ios-simulator.sh          # default app: ios-out/sim/App.app
```

It picks a simulator (`SIM_UDID`, or the first available device matching
`SIM_DEVICE`, default `^iPhone`, on the newest iOS runtime) and boots it,
starts `evo-server` on `127.0.0.1:8080` (the simulator shares the Mac's
network, so the app reaches it as `http://127.0.0.1:8080`; the cleartext
build's ATS exception lets the WKWebView use plain http), starts Appium,
records the app's log (`simctl spawn … log stream`, process `App`; Capacitor
prints the page's console there), and per attempt (2 by default, each on a
fresh install) runs `e2e/ios-webview.mjs`. iOS has no DevTools protocol:
Appium's XCUITest driver reaches the WKWebView through the simulator's Web
Inspector service (a Debug build sets `CAPACITOR_DEBUG`, so the web view is
inspectable) and runs JavaScript in its `WEBVIEW_*` context, and
`e2e/ios/webdriver.mjs` (a small dependency-free WebDriver client) gives
`e2e/android/scenario.mjs` a DevTools-shaped session over it: `evaluate`
through `execute/sync` (a promise is parked in the page and polled), taps
as mouse events and `click()` on the element at the point (or a real
XCUITest tap with `EVO_IOS_NATIVE_TAP=1`), typing with
`execCommand('insertText')`, screenshots from `simctl io … screenshot` (the
web view fills the screen), console lines and uncaught errors from a hook
in the page. Backgrounding is the home button (`mobile: pressButton`),
resuming `mobile: activateApp` (with `simctl launch` as the fallback for
both); iOS suspends the app within seconds, so the socket must come back
through `wake()` or the watchdog. Afterwards the server's `/api/metrics`
must show at least one socket opened and state frames sent. Screenshots,
`app.log`, `appium.log`, `server.log`, `metrics.json` and `report.json`
land in `OUT` (default `client/e2e/out/ios`). Env: `SIM_UDID`,
`SIM_DEVICE`, `EVO_PORT`, `EVO_APP_SERVER`, `EVO_APP_PAGE` (default
`^capacitor://localhost`), `APPIUM`, `APPIUM_PORT`, `WDA_DERIVED_DATA`,
`ATTEMPTS`, `BG_SECONDS`. Written on Linux: the script and the WebDriver
session were checked against a fake `xcrun` and a fake Appium that serves
the page from desktop Chromium (simulator choice from `simctl list -j`, an
install that fails once, a first session that fails and a passing second
attempt, home/activate, screenshots, the metrics check), and the ATS hook
with `npx cap add ios`/`sync` and unit tests (`scripts/ios-ats.test.mjs`),
but the simulator, Xcode build, Appium and WebDriverAgent only run in CI.

The app's page is `https://localhost` (Android) or `capacitor://localhost`
(iOS), so it talks to a remote game server. The origin comes from, in
order: `?server=https://host:port` on the page URL (remembered), the
**Server** field on the login screen (remembered; a native app opens it
until set), or a build default `VITE_EVO_SERVER=https://game.example.org
npm run build`; otherwise the page's own origin (`setApiBase()` in
`api.ts`, `serverOrigin.ts`). The WebSocket follows the same origin
(`wss://` for https). For a plain-http LAN server during development, run
`EVO_CAP_CLEARTEXT=1 npm run cap:sync` (cleartext and mixed content on
Android; on iOS an App Transport Security exception in
`ios/App/App/Info.plist`: `NSAllowsArbitraryLoadsInWebContent` and
`NSAllowsLocalNetworking`, which `scripts/ios-ats.mjs` adds after every
`cap copy`/`sync`/`add` through the `capacitor:copy:after` hook in
`package.json`, and takes out again on a sync without the variable, leaving
any other ATS keys alone; so sync without it before a release build);
`EVO_CAP_LIVE_URL=http://192.168.1.20:5173`
loads the page from `npm run dev -- --host` for live reload.

**Architecture.**

```
main.ts      login → design (no lineage) → game; one GameSocket from login to logout
session.ts   the saved login (token and world), in localStorage when available
worldChoice.ts  which world to preselect: per user and server, from GET /api/worlds
onboarding.ts   tour steps, state machine, hint triggers and progress storage (pure; ui/onboarding.ts draws them)
protocol.ts  wire types written by hand from crates/server/src/protocol.rs (keep in sync) + parseServerMsg
api.ts       fetch wrapper for the HTTP API (ApiError carries code and detail)
socket.ts    GameSocket: ids paired with `result`, 10 s reply timeout, reconnect with
             exponential backoff (0.5 s doubling to 15 s, ±20 % jitter), connection state;
             reopens a socket silent for 20 s; `wake()` on resume skips the backoff;
             asks for protocol 3 and rebuilds its binary delta states into whole `state`s
wire.ts      protocol 2/3 decoder (mirror of crates/server/src/wire.rs; wire.fixture.json pins them)
model.ts     GameModel: latest state, offer table (requests, updates, outcomes), alerts feed, toasts; pub/sub
alertPrefs.ts  alert preferences: normalize, per-kind show/toast switches, feed filter
push.ts      push notifications: device status, per-kind switches, subscribe/unsubscribe (browser APIs behind PushDeps)
nativePush.ts  native push in the apps (FCM / APNs via @capacitor/push-notifications behind NativePushDeps), tap targets
sw.ts        the service worker (built to dist/sw.js): shows pushes, focuses or opens the game on a click
swLogic.ts   the service worker's decisions (payload check, notification, target URL, which window), unit-tested
cost.ts      design point cost and validation (mirror of crates/sim/src/design.rs)
ui/          DOM screens and panels; panels re-render at most 4×/s and rebuild buttons only
             when their content changes, so they stay clickable under 10 Hz state pushes
world/       PixiJS view: camera, tile-map texture (terrain.ts decodes and colors the grid), overlays,
             markers, hit testing, mouse/touch gestures (tap, long press, drag, pinch);
             creature.ts: body plan → texture key (pure, tested); spriteArt.ts: look → art-library drawing;
             sprites.ts: drawings → cached canvas textures (art/canvas.ts);
             visionCones.ts: your selected organism's vision cones from its eyes and heading (pure, tested)
serverOrigin.ts  which game server to talk to (URL, saved, build default, same origin)
capacitor.config.ts  the iOS / Android wrapper
```

Every failed `result` (server error codes such as `not_owned`,
`too_far`, `no_slots_left`, or local `not_connected` / `timeout`) shows as
a toast. `window.evo` exposes the model, socket and view for debugging and
the end-to-end test.

**Tests.** Vitest: the Market tab's data prep (the appearance
summary, the `browse_market` page request from the filters, merging pages, the page
count text, why a purchase or a bid is blocked, the auction line, balance
and expiry text, ledger rows and totals, parsing `browse_market` and
`ledger` data (pages, cursors, own listings, auctions, and replies of servers before
paging), the state's economy fields and `market` alerts); creature sprites (a distinct cache key for each of the
1,536 appearance classes × life stage, every shape inside its texture frame,
size order and juvenile scale, each field changing the drawing, swimmers
without legs, the default look for older servers, action icons, the body
plan in words; morphology: all classes drawn exactly as before without the
fields, with the contract defaults and at far detail, defaults and clamping
of wire values, leg/ear/eye counts drawn, every ear and tail shape and size
bucket changing the drawing, one key per distinct drawing and shared keys
for what is not drawn, every variant inside its texture), the texture cache
(one make per key, disposal, 600 markers from five plans → five textures,
least-recently-used eviction by count and by cost, never a texture in use,
make timing, keys of a simulated crowded view) and the marker scale; the server origin
(normalizing input, URL over saved over build default); the World tab's data prep (species tree
order and depth, statuses, bad parent links, the catastrophe banner and
labels, niche labels, the player's era ranks); protocol parsing against messages shaped like
serde's output, including malformed and unknown ones and the `alert` and
`session_ended` messages; terrain decoding (layer sizes, refusing a wrong
size, newer-only dynamic updates) and tile colors (snow, ice, drought,
floods, dry river beds, dried lakes, the season tint) and painting; chart
data prep (scales, axis maxima, gap-aware line paths, deaths stacked by
cause with rare causes folded into "other", the age pyramid, gene drift
ranking, legacy percentages, histogram bars, allele-origin rows with colors
and the folded tail) and the alerts feed merge; smooth movement (the
measured tick length, the open-ground test, prediction speed, lead cap,
arrival and terrain stop, frame-rate-independent easing and the snap,
glides over the previous move interval that stay continuous, start from the
drawn point and snap long jumps, 600 markers per frame, and the predictor:
an immediate start on a local order, confirmation, repeated positions not
pulling it back, stopping on a refused or stale order, a need or danger,
arrival, release, a detour, a stall or a channel of deep water); alert preferences
(normalizing like the server, show/toast switches, the feed filter that keeps
offer events, the model toasting only preferred kinds); the point cost and
validation against the server's own examples (the README design costs
15.1, exactly-at-budget passes, one more founder fails); socket id pairing,
backoff, reconnect and give-up; the offer table through request, accept
and outcome; the matings panel's compatibility summary; the Seasons tab's
helpers; the world picker's memory (`worldChoice.test.ts`); the onboarding
tour's state machine, steps, progress storage, hint triggers and card
placement; the vision cones (`visionCones.test.ts`: headings to screen
angles, the arcs agreeing with the server's numbers, rotation with the
heading with the right eye clockwise, the gap ahead of side eyes and snout
occlusion, wedge polygons, hatch lines clipped inside the wedge and riding
with the animal, depth dots, sprite facing, easing the short way round, the
panel's on/off label); and the protocol 2/3 decoder against the Rust fixture
(`wire.test.ts`, above, own organisms' heading and eyes included).
`e2e/vision.mjs` founds a lineage and checks protocol 3 (heading and eyes on
every own organism, none on neighbors), clicks an own organism and finds its
cones on the marker (hatched blind spot, dotted depth zone), the panel's
numbers and "directional vision is off" label, the toggle (hidden, shown,
remembered), then possesses and walks it in different directions and checks
the cones turn with the server heading and the sprite faces its way;
screenshot `e2e/out/vision-cones.png`. `e2e/smoke.mjs` (Playwright, pre-installed Chromium) starts
`evo-server --fast --memory --static client/dist` on a free port, registers,
checks the live cost against the prices in `/api/genes` (and that the
server's founding response agrees) and the client-side over-budget refusal, founds a lineage, waits for markers and
the tile map (64×64 texture, at least 3 biomes and 20 tile colors, a dynamic
refresh, the terrain legend), registers a second player with the same body plan
and preferences and, while both lineages are young, runs a directed mating
through both players' panels (compatibility preview, which must say
compatible → request with price for the closest pair, preferring the
requester's female → incoming → accept, clicked the moment the request shows
→ both see the update → escort, both players walking their organism toward
the other, until it completes with fertility above 0.5; it must conceive
when the cost is 0, since a fertile pair inside both preferences has no
failure chance; an organism lost to predators or thirst on the way, a
partner female that conceives before bob accepts (a pending request holds
nothing) or a season change expiring the agreement means another pair is
tried, up to five times), releases the walkers, then buys from the **stud
marketplace** through both players' Market tabs (the first player lists
one of her females as a dam (a listed dam is held from breeding and the
buyer is a male, so no pregnancy spoils the pair; if she is pregnant the
buyer waits for the birth) → the second picks his male, the listing shows
"Compatible" → **Buy** → an accepted agreement with the fee in escrow on
both sides and a `market` alert to the lister → both walk toward each
other for up to 12 s, then the buyer cancels; the ledgers must match the
outcome: the lister paid on `completed`, the buyer refunded otherwise →
the balance pill and ledger rows show → the lister withdraws the listing
from its card under "My listings" (from `browse_market`'s `with_own`, so
it shows whatever page the market is on; if the listing closed meanwhile,
the dam having died, she lists another adult, and a card that doesn't show
is checked again and the tab reopened before retrying), checked through
`own` → the lister auctions an adult
from the form, the buyer finds it under "auctions" with "reserve 3, no
bids yet" and bids the reserve with one of his adults of the other sex:
the bid is in his escrow and the lister's card shows it with **Withdraw**
disabled (if the world refuses the bid, the lister withdraws the auction)),
clicks one of its own organisms (the one with
the most room around its marker, checked with the view's hit test, retried
if the selection isn't its own), possesses it, clicks a dry-land spot clear
of markers and checks it walks toward the target (closest approach tracked,
an arrival counts, the order re-issued if a click misses or the organism
breaks off for a need) while the state carries its `speed` and the drawn
(predicted) marker stays within 3 tiles of the server position, migrates, sees an error toast, splits the herd
from the toolbar (the new group picked in the group menu), draws a territory for it with two clicks (in
the state and drawn on the map; group cards in the Lineage tab; a back-to-back second claim refused with
`claim_cooldown`) and merges the groups back, checks the seasonal
leaderboard and the genetics dashboard (at least four charts with drawn
lines, drift sparklines, every gene in the table with histogram bars, the
allele-origin card naming you, alerts in the feed), hides daily summaries in
the alert preferences (gone from the feed, and `GET /api/alerts/prefs` has
them hidden) and the World tab (species listed, the era clock, an achievements row). Every
step waits on explicit state with generous timeouts. Screenshots go to `SCREENSHOT_DIR`. `e2e/winter_shot.mjs`
is a screenshot helper (not a test): a 16-day-year world played into winter.
`e2e/mobile.mjs` (run by `npm run e2e` after the smoke test; alone with
`npm run e2e:mobile`) emulates an iPhone 13 and a Pixel 7 in Chromium
(viewport, touch, mobile UA), each against a fresh `--no-wild` server (so its
lineage outlives the test): the login form
fits; the map spans the width with the tab bar at the bottom and the sheet
closed, no sideways scroll; a tab tap opens the sheet and a second closes
it; a touch tap selects an own organism and shows the floating button,
which possesses; a long press (DevTools touch events) possesses; a
one-finger drag pans and stops following; a pinch out zooms in at least 2×
and a pinch in zooms back out; the inspector shows in the sheet; the legend
starts closed and toggles; **Split** in the Tools menu makes a second group and two
taps claim a territory for it, with no sideways scroll. Then a 1280×800 desktop: sidebar on the right with tabs on top,
phone controls hidden, sprite cache stats (textures shared per class), and
a gallery of 20 body plans drawn with the game's sprite code, the same
body plans at the marker scales the map uses (0.75 far, 1, 1.4, 1.9), then a
labelled gallery of morphology variants and a burst of 150 new body
textures, logging the time per texture and the cache size, and 500
creatures on screen, logging the frame time. Screenshots:
`iphone13-*.png`, `pixel7-*.png`, `desktop-world.png`,
`desktop-sprite-gallery.png`, `desktop-sprite-scales.png`,
`desktop-morphology-gallery.png`, `desktop-perf-500.png`
(`EVO_E2E_ONLY=desktop` runs just that section).
`e2e/onboarding.mjs` (after the mobile test in `npm run e2e`; alone with
`npm run e2e:onboarding`) opens the client without `?tour=off`: on a
1280×800 desktop a new player gets the tour after founding with focus on
Next, walks steps with highlights (the inspector step opens the Organism
tab), skips, the map and tabs take input, a reload keeps it closed; Help is
modal (Tab stays inside, Esc closes it and focus returns to Help), Replay
reopens the tour and Esc skips it; after "Show tips again" an injected death
alert shows the death hint once ("Show me" opens Genetics), not again, nor
after a reload. On a Pixel 7 every step's card and highlight fit the screen
with no sideways scroll, Finish completes the tour, the tab bar works and a
reload keeps it closed. Screenshots: `onboarding-*.png`.
`e2e/push.mjs` (last in `npm run e2e`; alone with `npm run e2e:push`)
starts a server with `--push-allow-any-endpoint` and runs the full Chromium
in new headless mode (the headless shell reports notifications as denied
whatever is granted; without the full build it falls back to the shell with
the permission stubbed), the notifications permission granted. A real
`pushManager.subscribe()` would register with Google's push service, which
no test may contact, so `PushManager` is stubbed: `subscribe()` returns a
subscription with a real P-256 key (WebCrypto) and an endpoint on
127.0.0.1's discard port. With the real service worker active, it checks
that the settings card lists every kind, Enable subscribes with the
server's 65-byte key and POSTs the subscription, which
`/api/push/subscriptions` then lists; a kind switched off is saved; after a
reload the device is still on and the kind still off; Turn off removes it
on the server; and with notifications blocked, or no `PushManager`, the
card explains and offers no button. Screenshot: `push-settings.png`.
The server side of delivery is tested in Rust (`crates/server/tests/push.rs`,
and `native_push.rs` for FCM and APNs) against mock push services.

**Not yet:** native apps run on devices (the Android debug APK builds,
locally and in CI; iOS has not been built), native push received on a real
phone (implemented with FCM and APNs, see [Native push in the
apps](#native-push-in-the-apps), but never run against the real services;
Safari on iOS has Web Push only for a site added to the home screen,
`manifest.webmanifest` is there for it, untested on a device), animated sprites, alert preferences applied
server-side to the feed (muted kinds are still sent over the socket; pushes
do honour them), and allele origins traced to the founding *lineage* rather
than the player (a player founds one lineage per world, so the two coincide
until re-founding exists).

## Benchmark

```sh
evo-sim bench --size 256 --scale 16 --days 12 --threads 1,2,4 --compare-apply
```

`bench` founds the standard world with `--scale` times the founders (and
caps), warms up a day, then gives the simulated days to each (threads, apply
mode) setup in rotation, so all setups see similar populations and the same
machine load, and reports the median day: ms per tick, per-phase cost
(`World::phase_times`), µs per organism-tick and the time the parallel
apply spends in its sequential sweeps.

All numbers below come from a 4-core container shared with two or three
other busy agents (load average 7–14 on 4 cores; the benchmark session's
scheduler weight was raised for the multi-thread runs), so they are noisy
lower bounds on what a dedicated machine does. Speed-up figures are medians
of two runs, the rest single runs.

### Performance (long experiments)

```sh
cargo run --release --features serde -p evo-sim --example perfbench            # one world, stepped from the main thread
cargo run --release --features serde -p evo-sim --example perfbench -- --seeds 4  # four worlds at once, as experiments run
```

`perfbench` runs a 96×96 world for two simulated years (240 days) with the
default balance (`presets::standard_world`) and with
`Balance::carnivore_diet()` + `presets::niche_world`, and prints the wall
time, the time per phase and a determinism fingerprint (FNV-1a of the final
snapshot). Every optimisation below leaves the fingerprints
(`e37d27fc27ac144e` default, `c32bf3e8057496e8` meat, seed 31), the sweep
check (`evo-sim sweep --size 96 --trials 1 --seeds 12 --rounds 1 --seed 31`:
score 11.60) and the `predlong` output byte for byte unchanged.

What changed:

- `World::step` enters the rayon pool once per tick when called from outside
  it (`evo-sim run`, the server's runner) instead of handing each of a dozen
  parallel passes to the pool and waiting.
- Kin and mate tests first try bounds on the genetic distance through a
  per-lineage reference point (the mean genes, refreshed every 240 ticks):
  the distance is a metric, so `|d(a,c) - d(b,c)| <= d(a,b) <= d(a,c) + d(b,c)`.
  The full 40-gene bound (`DistanceBounds::new`) is summed only when those
  don't settle the test (its share of the profile fell from 12% to 4%).
- Light parallel passes (grid rebuild, the parallel apply's local-act and
  burrow sweeps, a small map's environment chunks) run on the calling thread
  below 8,192 items: waking the pool cost more than it saved.
- Off the day, the environment chunks and the grid rebuild run at once
  (`rayon::join`; they share nothing), and the chunk update computes each
  column's and row's weather interpolation once instead of per tile.
- `evo-sim` (the simulation crate) builds in one codegen unit in release
  (about 5%).

Measured on the 4-core container (noisy; same-session runs of the original
and the new build, seed 31):

| Benchmark | Before | After | Speed-up |
| --- | --- | --- | --- |
| `perfbench`, one world, default, 2 years | 23.3 s | 21.1 s | 1.10× |
| `perfbench`, one world, carnivore diet + niches, 2 years | 24.3 s | 23.0 s | 1.06× |
| `perfbench --seeds 4`, default, 2 years | 55.0 s | 51.8 s | 1.06× |
| `perfbench`, one world, default, first 80 days | 5.97 s | 3.98 s | 1.50× |
| `predlong -- 2 31` | 22.3 s | 19.8 s | 1.13× |
| `predlong -- 2 31 --meat` | 24.5 s | 21.9 s | 1.12× |
| `predlong -- 1` (seeds 31–34 at once) | 14.6 s | 13.1 s | 1.11× |
| `evo-sim sweep` check (12 seeds × 120 days) | 48.8 s | 46.7 s | 1.05× |
| `evo-sim run --days 120` (128×128) | 11.7 s | 11.2 s | 1.04× |

A later pass made the decision phase's neighbor query branchless
(`SpatialGrid::query_into`: every candidate is written and only those in
range kept, the same ones in the same order), worth about 10% of decide
time in crowds, again with identical results.

**Exact limits.** Perception sums neighbor positions (the kin centroid and
the estimate beyond the nearest 32) in the order the nearest-32 selection
leaves the list, so an exact method must reproduce that permutation: it
can't drop candidates or reorder cells. Two approximations were measured
and dropped: counting far grid cells from per-cell summaries (with 4-tile
cells and 6–8-tile ranges, under 1% of far cells lie wholly in range), and
fewer close looks (the cost moves to the far-neighbor loop and the
selection: no gain).

**Staggered decisions** (`Balance::decide_interval`, off by default, so
default worlds are unchanged): at `k >= 2` a calm organism decides afresh
every `k` ticks (staggered by id) and keeps its last intent in between,
following its target organism (`world::stagger`). Every organism decides on
the day's first tick, and so does every hunter and any organism that is
possessed, fleeing, courting, provisioning or defending, that saw a threat,
or whose target died (a first version without the hunter rule cost the
pack hunters 3 of 4 seeds). It is not saved in snapshots. Measured
(`--set decide_interval=K` on `perfbench`, `sweep`, `predlong`):

| | off | `k = 2` | `k = 3` |
| --- | --- | --- | --- |
| µs per organism-tick, default, 2 years, 1 thread | 1.84 | 1.60 | 1.55 |
| µs per organism-tick, meat, 2 years, 1 thread | 1.85 | 1.58 | 1.48 |
| sweep check wall time (12 seeds × 120 days) | 46.0 s | 35.6 s | 34.9 s |
| sweep check score (95% CI) | 11.68 (± 0.33) | 11.74 (± 0.27) | 11.59 (± 0.42) |
| `predlong -- 3`, seeds 31–42: fox / pack hunters survive | 5/12, 7/12 | 6/12, 5/12 | |
| `predlong -- 3 --meat`, seeds 31–34: fox / pack hunters | 4/4, 4/4 | 3/4, 4/4 | |

The scores and survival counts lie within seed noise of the defaults (four
seeds swing the fox from 1/4 to 3/4 between equally valid trajectories),
but the setting changes reaction times, so it stays off until a longer
comparison (`evo-sim compare --set decide_interval=2 vs defaults`) says
it is safe.

Where the time goes now (one world, single thread, two years): perception
in `decide` is about two thirds (neighbor scan, nearest-32 selection, the
close look at each), apply a quarter, environment chunks and the grid
rebuild a few percent each. Late in a run herds are dense and each
organism scans 50–100 neighbors; that scan, not kinship, is now the
largest single cost. Tried and dropped (no measurable gain): trimming the
grid query to the cells the circle reaches, software prefetch of
organisms in grid order, bounding the body-plan gap through the reference
point as well, fat LTO. Parallel scaling of one world stays modest
(single thread 41 s, four threads 21 s for the default two years): the
apply phase's parallel sweeps are about 0.2 ms per tick, too short to pay
for waking the pool's workers, so experiments get the most from running
seeds side by side, which they already do (`predlong`, `sweep`, `compare`,
`genetics` run their worlds in parallel).

### Profile at the prototype target

The doc's prototype stage is a 256×256 map with ~5k player and ~20k wild
organisms. `--scale 16` starts 24,900 organisms and runs at about 21–23k
(scale 4: 6,200 growing to 9–10k). Single thread, ms per tick:

| | Environment | Daily | Grid | Decide | Apply | Total | µs per organism-tick |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ~6k organisms (scale 4, quiet start of the session) | 1.8 | 0.1 | 0.3 | 8.4 | 4.4 | 14.9 | 2.5 |
| ~22k organisms (scale 16) | 2.3 | 0.2 | 1.4 | 61 | 14.1 | 79 | 3.4 |

The decision phase dominates (three quarters at scale 16) and its
per-organism cost grows with local density (1.4 µs at scale 4, 2.7 µs at
scale 16): perception scans more neighbors in a crowd. Apply is 0.6–0.75 µs
per organism, much of it movement and A* route planning (callgrind).
Environment chunks cost 0.03–0.04 ms per 1,000 tiles; daily bookkeeping and
the grid rebuild are small.

### Faster decide phase

Perception spent most of its time on kinship: a 39-gene genetic distance
summed in one serial chain per neighbor, plus the body-plan genes read
from the scattered genome, for up to 32 neighbors, each on its own
organism. Without changing a single result (`evo-sim sweep --size 96
--trials 1 --seeds 12 --rounds 1 --seed 31 --threads 1` prints exactly the
same before and after; a test checks the new kinship tests against the
genome-based ones over every pair in a world):

- The grid rebuild packs each organism's distance genes (`decide::Genes`,
  160 bytes) next to its slot, so a crowd's genes are read together, and
  keeps the slots' positions in their own array for the scan.
- Kinship and mate distance are monotone in the genetic distance, so
  `look` brackets it with a vectorized sum (any order of the same terms is
  within a few millionths of the serial one) and decides from the bounds;
  the exact serial sum runs only when the bounds straddle a threshold
  (almost never).
- Kin are settled from the grid alone; a stranger's organism is read only
  if the grid's `ARMED` flag and mass say it could be a threat or prey,
  and a possible mate's only if its `COURTING` and sex flags allow it.
- `Map::find_water` checks tiles nearest first (ties in the old scan's
  row order) and stops at the first.

Medians of interleaved runs on the same shared 4-core machine (load 5–9):

| 256² map | Decide before | Decide after | Tick before | Tick after | µs per organism-tick |
| --- | --- | --- | --- | --- | --- |
| scale 4 (~5.6k), 1 thread | 5.8 ms | 3.9 ms | 12.2 ms | 10.3 ms | 2.18 → 1.85 |
| scale 16 (~22k), 1 thread | 56.0 ms | 34.3 ms | 72.2 ms | 50.7 ms | 3.31 → 2.32 |
| scale 16 (~22k), 4 threads | 30.2 ms | 17.9 ms | 45.4 ms | 33.6 ms | 2.19 → 1.64 |

Decide is 35–40% cheaper in a crowd (it executes about 40% fewer
instructions per call under callgrind). The figures elsewhere in this
section predate this change.

### Parallel apply

The apply phase used to run every organism's turn in one sequential pass in
a seeded shuffled order. `world::par_apply` splits each turn: `start`
(intent, movement, route planning), `act` (eating plants, meat and caches,
hunting, mating, drinking, clutch raids, nursing: everything that touches
other organisms, tiles, clutches, caches or counters), `settle` (dormancy,
needs, death rolls, disease, growth) and `finish` (death, parent credit,
birth). `start` and `settle` touch only the organism itself, so:

1. Hunters and their prey, suitors and their mates (any decision with a
   target organism) are *engaged*: they take their whole turn in their place
   in the shuffled order, exactly as before. 2–3% of turns.
2. Everyone else moves in a **parallel** sweep (route search buffers per
   thread),
3. acts in a **sequential** sweep in the shuffled order (engaged organisms
   take their whole turn here),
4. settles in a **parallel** sweep, each organism drawing from its own
   `(seed, SETTLE, tick, organism id)` stream,
5. and dies, credits its parents and gives birth in a **sequential** sweep
   in the shuffled order.

The parallel sweeps run each organism on its own, held outside
`World::organisms` (a rule reaching for another organism would panic, not
race), against a world nothing writes meanwhile; tile food, meat, kills,
births, counters and index allocation only change in the sequential
sweeps. So results are deterministic and independent of thread count (tests:
1 vs 4 threads bit-identical for both apply modes; two hunters striking one
prey in the same tick kill it once, count one kill and leave one carcass;
births, hatchings and deaths each counted exactly once over 8 days).

They are not identical to the sequential apply's: a free organism now moves
before anyone acts and settles after everyone has acted. One effect of that
mattered and is undone: a young being nursed sees a free mother whose turn
comes after its own where she stood before moving, as the sequential apply
would (without it, mothers always walked off first, and herd grazers ran
10–25% lower in the balance sweep). `--sequential-apply`
(`WorldConfig::parallel_apply = false`) restores the old apply exactly
(bit-identical to before this change). Region colouring was the other
option; it would have meant moving every rule onto a split-borrow view of
the world plus a deferral pass for interactions across region borders, while
splitting the turn keeps the rules as they are.

**Speed-up** (ms per tick, medians of two runs; the parallel apply's
sequential sweeps in brackets):

| | Threads | Apply, sequential | Apply, parallel | Tick, sequential apply | Tick, parallel apply | µs per organism-tick (parallel) |
| --- | --- | --- | --- | --- | --- | --- |
| ~22k (256², scale 16) | 1 | 14.1 | 14.2 (3.6) | 79 | 75 | 3.6 |
| | 2 | 13.6* | 10.5 (3.6) | 54* | 52 | 2.3 |
| | 4 | 13.6* | 7.9 (3.3) | 37* | 28.7 | 1.4 |
| ~9k (256², scale 4) | 1 | 7.5 | 6.1 (1.1) | 25 | 18.5 | 2.5 |
| | 4 | 7.9* | 5.1 (1.5) | 16.4 | 15.3 | 1.4 |

\* The sequential apply doesn't use the extra threads; the spread from the
1-thread figure is machine load. At scale 4 the population grows during the
run, so setups differ by up to 10% in organisms; compare µs per
organism-tick. At the prototype target, 4 threads now make the apply phase
1.8× and the whole tick 2.6× faster than one thread (decide alone: 3.3×). The sequential sweeps are a quarter of the apply phase
(0.16–0.24 µs per organism, mostly cache misses from visiting organisms in
random order), which caps the apply phase at about 2.3× on 4 cores.

**Capacity at 1 tick per second**, 4 threads, parallel apply:

| World | Organisms (agents) | ms per tick | µs per agent-tick |
| --- | --- | --- | --- |
| 256², scale 16 | 21k (21k) | 28.7 | 1.4 |
| 256², scale 16, LOD, 2 focus points | 26k (6.5k) | 11.0 | 1.7 |
| 256², scale 16, LOD, 4 focus points | 21k (14k) | 18.8 | 1.3 |
| 512², scale 16 (1 thread: 343 ms, 4.4 µs) | 73k (73k) | 127 | 1.7 |
| 512², scale 16, LOD, 8 focus points | 99k (28k) | 41.7 | 1.5 |

At 1.4–1.7 µs per agent-tick this machine would hold roughly **550–700k
agents at 1 tick per second** (extrapolated; the largest world measured is
78k, and the per-agent cost rises with the working set: 4.4 µs single-thread
at 512² against 3.6 at 256²), or **230–290k on one core**, which includes
terrain, weather and bookkeeping: the low end of the doc's 200k–500k per
core *before* overhead, and well above its "a third of that after
overhead". Level of detail doesn't make agents cheaper; it makes most
organisms cohorts, which cost almost nothing beyond the fixed environment
update (10 ms per tick for 512²), so total capacity is agent capacity
divided by the share of organisms near players.

### High density: decide, act sweep, sub-step states

Profiled at 512², `bench --scale 16` (81k organisms, 0.31 per tile, the
largest world above) with `perf` on one thread, then the server's
sub-steps with 500 steering players. Where the tick went: decide 60%
(`look` 31%, of which a third was one cache miss per neighbor reading
`Organism::group` for the herd test, and 10% the vectorized gene distance
on the grid's packed genes, bound by memory; the neighbor query 10%);
the parallel apply 30% (movement 11%, settle 6%, the sequential act and
finish sweeps 8–9%); the grid rebuild 4.5%; environment 3%. Founding the
world took longer than a day of ticks: 4–5 minutes, because every 12-founder
wild group flood-filled the whole map's passable regions and scanned every
organism 200 times. Each change below leaves every result bit for bit as it
was (`evo-sim sweep --size 96 --trials 1 --seeds 12 --rounds 1 --seed 31`
prints 11.54 and `--seed 55` 11.41, the same output as before apart from
timings; the bench's organism counts and route searches are identical;
`balance-smoke.sh` still scores 11.21):

- **Decide:** the grid's entries carry the owner and group, so a kin
  neighbor is settled without touching its organism, and the scan skips the
  viewer by its slot rather than by reading each entry's index.
- **Grid rebuild:** entries (several cache lines of each organism) are
  computed on all cores and only the counting sort stays sequential.
- **Act sweep:** before it, a parallel pass marks free organisms whose act
  could only drink or do nothing (`par_apply::local_act`: below 30 hunger,
  not eating, gathering, migrating, chasing, nursing or about to nurse;
  60–70% of free organisms at this density). They touch nothing but
  themselves and nothing in the sweep reads what they change, so the
  sweep skips them and they drink in the parallel settle sweep. The sweep
  prefetches the organisms of the turns six ahead, and the finish sweep
  visits only organisms with a death, a coming of age or a birth due
  (found in the settle sweep). Determinism and thread independence are as
  before: the sequential sweeps still run in the shuffled order on one
  shared stream.
- **Founding:** the home search gathers the probe's passable regions,
  threats and prey once per lineage and looks them up in an 8-tile grid,
  summing in organism order: 4–5 minutes became about 5 seconds for this
  world (it also costs a live server less per founding player).
- **Sub-step and push states (server):** `action` and `sex` of own and
  nearby organisms are static strings (`Cow<'static, str>`) and leaderboard
  names shared (`Arc<str>`): a state no longer allocates two strings per
  organism in view and one per leaderboard row; `StateIndex` reads each
  organism once. JSON and decoded states are unchanged (the wire fixture and
  the protocol 1 / 2 comparisons pass as they were).

Medians of six interleaved runs per binary, old and new alternating, ms per
tick (`bench --size 512 --scale 16 --days 4 --threads 1,4`, two days per
setup). The machine was shared with other agents throughout, load average
2–18 on 4 cores (mostly 9–16), so the 4-thread figures are especially
noisy:

| Phase | 1 thread, before | 1 thread, after | 4 threads, before | 4 threads, after |
| --- | --- | --- | --- | --- |
| Environment | 10.0 | 10.2 | 13.5 | 12.7 |
| Grid | 11.3 | 11.7 | 13.1 | 11.1 |
| Decide | 133.2 | 126.5 | 95.4 | 86.6 |
| Apply | 67.7 | 64.1 | 71.7 | 57.4 |
| of which sequential sweeps | 20.1 | **9.9** | 28.6 | **13.8** |
| Tick | 223.9 | 213.8 | 194.9 | 169.8 |

The sequential sweeps, the part of the apply that no core count shortens,
halved. A quieter pair of single runs (load 6–7, same tick code before)
gave decide 117.8 → 106.2 ms and sequential sweeps 14.5 → 8.4 ms on one
thread; under heavier load the decide change sinks into the noise. What
the server's world thread spends on states (`game::tests::substep_cost`, a
512² world, 8.3k organisms, 500 players each steering one organism;
medians of five interleaved runs, load 5–8): a push to every connection
6.7 → 4.4 ms, a possession sub-step 7.4 → 5.1 ms.

**Sub-step states reuse what the push built** (`game::states`). The push
after a tick indexes the world (`StateIndex`, which now also keeps each
organism's owner and position, so picking the neighbors in a window reads
no organism) and keeps the index in a `StateCache` for the rest of the
tick, with every neighbor view it made (one per organism, shared by every
window it is in) and the own-organism views of players who steer. Between
ticks the world changes only through sub-steps and the commands applied
before each, so a sub-step rebuilds none of it: organisms a sub-step can
move (the possessed ones, and any possessed or ordered since) and organisms
a command may change (any it names, the pair of a directed mating run at
once, all of a player's after a group order) are dirty; each moves its
entry in the index (one cell to another at most) and drops its cached
views, and a player's own list is patched for its dirty organisms. What is
cheap is recomputed for every state (view window, neighbor pick and nearest
cut, offers, balances, escrow, groups, the leaderboard cut, now by rank
lookup instead of a scan of the board). A founding or a new tick drops the
cache. A test (`cached_substep_states_match_the_old_build`) runs two games
from one seed side by side, one with the old build kept verbatim in the
test, sends 16 players' random commands to both between every sub-step
(views, possession and orders, own-pair matings that run at once, offers,
market, auctions, group orders, a second connection, a founding mid-tick)
and checks that every state each client gets, as JSON and as protocol 2
frames, is byte for byte the same; disabling any one invalidation makes it
fail. Sim results are untouched (`sweep … --seed 31` prints 11.76 as
before).

Paired before/after runs, old and new binaries alternating; the 4-core box
was shared with three other agents' builds and tests (load average 3–6
during the micro-benchmark, 0.2–6 during the load test), everything under
`nice`. `substep_cost` (500 steering players, medians of five interleaved
runs):

| World | push, before | push, after | sub-step, before | sub-step, after |
| --- | --- | --- | --- | --- |
| 8.3k organisms (3 days) | 5.6 ms | **4.2 ms** | 6.3 ms | **3.9 ms** |
| 19.5k organisms (`EVO_SUBSTEP_DAYS=12`) | 9.4 ms | 8.8 ms\* | 9.4 ms | **6.1 ms**\* |

\* medians of the paired ratios (0.94 and 0.65) times the old median; the
absolute medians drifted with the load (push 8.2–16.7 ms old, 7.7–13.3 new).
`loadgen --players 500 --ramp 40 --duration 150` against fresh copies of
one 512² world warmed to 22k organisms (32.5k at the end), three runs per
binary, medians:

| | before | after |
| --- | --- | --- |
| State push p50 / p99 | 12.6 / 19.2 ms | **10.6 / 15.2 ms** |
| Possession sub-step p99 | 18.2 ms | **14.1 ms** |
| Tick p50 / p99 (step + bookkeeping) | 35.9 / 64.3 ms | 34.3 / 65.9 ms |
| Command p50 / p99 | 136 / 249 ms | 136 / 249 ms |
| Server CPU / RSS max | 0.25 cores / 442 MB | 0.23 cores / 450 MB |
| Skipped states, dropped connections | 0 | 0 |

Every pair agreed in direction (push p50 −16 to −20%, sub-step p99 −11 to
−36%); the tick, which holds no state work, is unchanged within noise (the
first pair ran under load 4–6: tick p50 63 / 62 ms). What is left per state
is what a state contains: picking the neighbors, copying their views, the
leaderboard's top rows and the own list, about 8 µs for 500 players; going
further would mean states that share their unchanged parts (an `Arc` per
part, a protocol type change) rather than copies.

### A server-sized world at 60–70k organisms

The soak below found that 500 heavy players grow a 512² world to 60–72k
organisms, where a tick cost 170–260 ms (p50) and 830–930 ms (p99) with six
worlds on four cores, against 64–70 / 150–190 ms at ~45k, and that the same
population cost more later in the run. To measure that without a server:

```sh
cargo run --release -p evo-sim --features serde --example highpop -- warm /tmp/hp --until 62000 --every 15 --keep 4
cargo run --release -p evo-sim --features serde --example highpop -- bench /tmp/hp/day0050.snap --ticks 96 --threads 1
cargo run --release -p evo-sim --features serde --example highpop -- stats /tmp/hp/day0050.snap
```

`warm` builds the world as `evo-server` does (`game::build_world`: the wild
ecosystem scaled to the map, catastrophes on) and founds 500 player
lineages with the load generator's designs (12 founders, one body plan, a
tenth cold-blooded egg layers), then runs it without commands. It grew much as
the soak's worlds did (11.7k founders; 25k on day 12, 45k on day 36, 62k on
day 50 with 23k in player lineages, 93k on day 80, then 67k on day 95 in
winter with 17k), and saves snapshots from the day it passes `--until`.
`bench` restores one and times every tick and every phase
(`World::phase_times`, ticks p50 / p99 / mean, the daily tick apart, route
searches) and prints a hash of the final snapshot: the check that an
optimisation changes nothing, at any thread count. `stats` counts what a
decision's neighbor scan meets.

**Why it grows faster than the population.** Every organism's decision
scans the organisms within its perception radius (up to 8 tiles), looks
closely at the nearest 32 and screens the rest; the scan, the screening
and the selection of the nearest grow with local density, so a tick costs
roughly population × neighbors, and neighbors grow with the population and
with how tightly the herds pack:

| Snapshot | Organisms (in player lineages) | Neighbors within 8 tiles: mean / p50 / p99 | More than 32 | Decide, 1 thread (before) |
| --- | --- | --- | --- | --- |
| day 50, summer | 62.1k (23.1k) | 79 / 71 / 199 | 91% | 124 ms |
| day 65, autumn | 82.2k (31.6k) | 104 / 94 / 260 | 96% | |
| day 80, autumn | 93.5k (38.7k) | 120 / 108 / 295 | 97% | |
| day 95, winter | 66.8k (17.5k) | 124 / 101 / 401 | 94% | 186 ms |

That is the soak's "same population, later in the run": on day 95 the
world holds as many organisms as on day 50 but in tighter winter herds
(5k dormant in dens, fewer player lineages and organisms), each
organism has 57% more neighbors (twice as many at p99), and the decide
phase cost 50% more. 70% of neighbors are of another lineage.

**Where the time went** (day 50, one thread; phases from the timers, the
rest from callgrind): decide 59% of the tick (the close looks at the
nearest 32 more than half of it, the 39-gene distance 30% of a look; the
neighbor query, the selection of the nearest and the screening of the
rest a fifth), the parallel apply 31% (route searches alone 9% of all
instructions, 70% of that in searches that fail after the full
1,500-node budget), environment 5%, grid 4%. The daily tick added 55–60 ms of
bookkeeping on one core (140–210 ms on the days with species detection)
on top of a normal tick, which is the p99 of every run: one tick in 48 is
a daily one. Of that, 42 ms were the legacy tally (a map update per allele
of every breeding adult), 80 ms the species clustering (6,085 samples: the
500 lineages make far more strata than `species_max_samples`, and every
pair of samples was compared), 9 ms removing the dead.

Changes, each leaving every result bit for bit as it was:

- **Decide in grid order.** Decisions are made in the spatial grid's slot
  order instead of organism order (each depends only on the world and its
  own random stream) and scattered back, so neighbors that decide one
  after another scan the same cells and their packed genes stay in cache.
- **Cheaper close looks.** The 39-gene distance is summed only when a test
  needs it (a body-plan gap at the kinship limit rules kinship out at any
  distance), with eight running sums instead of four, and the body-plan
  genes sit first in the packed genes so the gap reads four fixed slots.
- **Legacy tally:** each genome's alleles are counted per origin with a
  short linear scan (`Genome::count_origins`), all adults on all cores
  (`LegacySnapshot::compute_par`), in integers; all tallies are whole
  numbers, so the totals are exactly the old ones.
- **Species clustering:** samples are bucketed by body plan (a pair can
  only link if every body-plan gene differs by less than the kinship gap),
  pairs already in one component are skipped, and the distance is bounded
  from a vectorized sum, falling back to the exact test only when the
  bounds straddle a threshold. The components are the same (a test runs
  the old pairwise union-find beside it over every preset body plan and
  four link rules).
- **Environment:** the chunks due this tick update on all cores, one chunk
  row each (a chunk writes only its own tiles), and the hourly
  distance-to-water field is a two-pass distance transform instead of a
  breadth-first search (a test compares them).
- **Maps:** the organism index and the per-tile maps (clutches, caches,
  dens) hash with a multiply-rotate hasher instead of SipHash (nothing
  iterates them in an order that matters); route search keeps each tile's
  state in one record.

Medians of interleaved runs, old and new binaries alternating (96 ticks,
two simulated days including one species detection; 5 runs on day 50, 3 on
day 95). The box was shared (load average 1–2.5 from other agents), so
compare the columns, not other tables. ms per tick:

| Day 50 (62–64k), phase | 1 thread, before | 1 thread, after | 4 threads, before | 4 threads, after |
| --- | --- | --- | --- | --- |
| Environment | 11.1 | 9.5 | 11.8 | **6.1** |
| Daily (mean over all ticks) | 2.09 | 0.78 | 2.18 | 0.66 |
| Grid | 8.3 | 8.6 | 4.1 | 4.0 |
| Decide | 123.6 | **97.8** | 32.4 | 28.1 |
| Apply (of which sequential) | 66.5 (9.3) | 64.7 (8.2) | 27.5 (9.4) | 24.6 (7.8) |
| Tick mean | 211.3 | **181.6** (−14%) | 77.8 | **63.4** (−19%) |
| Tick p50 / p99 | 205 / 286 | 180 / 238 | 73.6 / 134 | 61.9 / **90** |
| Daily phase, ordinary / species day | 55–60 / 138–212 | 21–24 / 52–69 | 55–84 / 146–178 | 14–19 / 45–58 |

| Day 95 (67k, winter), phase | 1 thread, before | 1 thread, after | 4 threads, before | 4 threads, after |
| --- | --- | --- | --- | --- |
| Environment | 12.3 | 11.1 | 11.9 | 7.3 |
| Decide | 186.4 | **128.1** | 43.6 | 39.2 |
| Apply (of which sequential) | 87.0 (14.0) | 81.6 (11.9) | 33.2 (12.8) | 32.8 (11.5) |
| Tick mean | 298.2 | **231.7** (−22%) | 95.1 | 85.0 (−11%) |
| Tick p50 / p99 | 287 / 376 | 227 / 314 | 92.3 / 151 | 82.4 / **118** |
| Daily phase, ordinary / species day | 59–61 / 126–148 | 28–33 / 54–60 | 57–61 / 130–138 | 17–19 / 43–62 |

On one thread, the measure of what a world costs a shared box, a tick at
60–70k is 14–22% cheaper, most where the herds are densest; on four
threads the daily bookkeeping spike is a quarter of what it was and tick
p99 fell by a fifth to a third. The soak's *Confirmation run* (six worlds
of 500 players for 90 minutes; [Six worlds for three
hours](#six-worlds-for-three-hours-soak)) measured it in the server: at
60–70k six worlds took 2.3–2.5 cores (2.8–2.9 before) with tick p99
0.27–0.30 s (0.8–1.2 s before) and command p99 at its floor. Every run's
final state hash matched the old binary's at 1 and 4 threads; `sweep
--size 96 --trials 1 --seeds 12 --rounds 1 --seed 31` prints 11.76 with
output identical to before apart from the timing; `run` with the parallel
apply, with `--sequential-apply` and with level of detail, migrants and catastrophes prints the same as
before.

Not done, because they would change results (open items):

- **The neighbor scan itself.** What still grows with density is the scan:
  every neighbor within the radius is collected, the nearest 32 selected
  (`select_nth_unstable`) and the rest screened. It could be fused into
  the grid query and skip the selection, but the kin centroid and the
  far neighbors' position sums are float sums in the order the selection
  leaves the neighbors, so any other order moves results at the rounding
  level. Summing positions in fixed point (or in slot order) would free
  it; that is a (tiny) rule change.
- **Failed route searches.** 36 of ~160 searches a tick fail after
  expanding the whole 1,500-node budget (a target across water with
  nothing closer reachable) and are repeated every simulated hour by each
  organism stuck that way: 6–7% of the tick. A failure could be reused
  until the terrain it explored changes, but a tile's crossing cost
  follows snow, mud and water depth continuously and the terrain versions
  only track 0.1 steps of some of them, so reuse would not be exact; a
  cheaper reachability test (or a smaller budget for repeats) changes
  movement.
- **Removing the dead each day** (`retain` moving every survivor, most of
  the 8 ms the daily phase still spends on one core before its tallies):
  keeping organisms in place with a free list would change the index
  order, and with it the order of everything indexed by it.

### Launch numbers

The doc's next step was to measure at the prototype target and then set
launch numbers. From the measurements above:

- **Prototype** (256², 10–50 players, ~25k organisms): 29 ms per tick on 4
  cores, 75 ms on one. No level of detail needed; there is room to run the
  world clock several times faster than 1 tick per second while testing.
- **Launch** (512², 100–200 players): keep the doc's 20k–40k player
  organisms (about 200 breeding adults per lineage, `lineage_cap` ≈ 250–300
  organisms) plus ~100k wild. That is about 140k agents at a higher density
  than measured (0.53 per tile against 0.28–0.38; decide cost rises with
  density, say 2–2.5 µs per agent-tick on 4 cores): roughly 0.3 s of a 1 s
  tick on 4 cores, 0.6–0.9 s on one. Plan **one world per 4–8 dedicated
  cores**, which leaves room for networking, persistence and possessed
  organisms ticking faster. With 100–200 players on 512² the players' active
  areas (about 9,000 tiles each) cover most of the map, so size servers for
  full agents and treat level of detail as headroom for quiet regions and
  offline players.
- **Stretch** (1024², 500 players, ~400k): about 0.7 s per tick on 4 cores
  by extrapolation, too close to the 1 s clock. Needs 16 cores (untested
  beyond 4; the sequential remainder, about 0.4 µs per organism for the
  apply sweeps, environment and grid, caps any core count at a few million
  organism-ticks per second) or level of detail with players clustered;
  more worlds rather than bigger ones remains the safer way to grow.
- Watch: the decision phase is the cost to cut next (density-driven
  perception, now bound by memory); the apply's sequential act sweep, halved
  by running local acts in parallel (see [High density](#high-density-decide-act-sweep-sub-step-states)),
  is the next part to split by region if the apply phase becomes the
  bottleneck.

**Balance check.** `evo-sim sweep --size 96 --trials 1 --seeds 12 --rounds 1
--threads 1` (score / surviving lineage-worlds of 96):

| Seeds | Sequential apply | Parallel apply |
| --- | --- | --- |
| 31–42 | 11.14 / 90 | 11.26 / 91 |
| 43–54 | 10.90 / 90 | 10.94 / 90 |
| 55–66 | 11.14 / 90 | 11.00 / 89 |
| Mean | 11.06 / 270 of 288 | 11.07 / 270 of 288 |

The two schedules differ by less than one seed set from the next (0.24), the
usual chaos band.

### Load and soak test

`crates/server/examples/loadgen.rs` drives simulated players against a real
server over the real protocol:

```sh
cargo build --release -p evo-server --bins --examples
target/release/examples/loadgen --players 200 --duration 150 --ramp 40 --warm-until 22000
target/release/examples/loadgen --players 200 --duration 1800 --fast --warm-until 22000   # soak
target/release/examples/loadgen --url http://host:8080 --players 50                       # a running server
target/release/examples/loadgen --players 200 --proto 1 --deflate                         # JSON states, deflate estimate
target/release/examples/loadgen --worlds 2 --players 400                                  # two worlds in one process
target/release/examples/loadgen --url http://host:8080 --world isle --players 50          # one world of a running server
target/release/examples/loadgen --worlds 6 --players 3000 --ramp 240 --duration 10800 --report 30 \
  --warm-until 22000 --csv soak.csv                                                       # long soak, a CSV row per world per report
```

`--csv FILE` appends a row per world at every report (tick, push, sub-step
and apply timings, ticks per second, that world's players' command latency
p50/p99/max *since the last report*, egress, queue depth, dropped
connections, skipped states, the last save's capture/encode/write times and
size, process CPU and RSS, the allocator's live and retained heap, and the load generator's own counters), so a
multi-hour run can be compared hour by hour; the summary's medians would
hide a trend.

`--world ID` sends every player to that world and samples its metrics;
`--worlds N` starts the server with N worlds (`main`, `w2`, …, seeds
`--seed`+i), spreads the players over them round-robin, prints a line per
extra world at every report and adds a `worlds` entry per world (tick,
push and sub-step timings, ticks per second, outbound MB/s, command
latency of that world's players, connections, organisms) to the summary;
`out_mb_s_all_worlds` is the process's total. With `--warm-until N` every
world is grown to N organisms. The summary's other timings are the first
world's; CPU and memory are the whole process.

The players speak protocol 2 (binary delta states, as the web client) by
default; `--proto 1` makes them old clients that get whole JSON states.
`--deflate` also feeds every state through a deflate stream per connection
to show what permessage-deflate would save. The summary has the server's
time per encoded state (`state_encode_us`), protocol 2's keyframes,
resyncs and decode errors, and the bytes each client got by message kind.

It starts `evo-server` (512² by default) with a file store in `--data DIR`
and `--no-rate-limits`, since every simulated player comes from 127.0.0.1
(the per-IP login, command and read limits would otherwise throttle them;
against a server with limits on, run the players from many hosts). With
`--warm-until N` it first grows the world in fast mode to N organisms, stops
it (saving) and restarts it from that save with the test's clock. Each
player registers, logs in, founds a lineage (12 founders), opens **one**
WebSocket (well under the 8-per-player limit) and, about 30 times a minute,
moves its view, possesses and steers an organism (30% of actions), requests
and previews matings (own pairs and other players' organisms in view, some
priced), answers mating requests, browses the market, lists an adult, buys
listings, reads its ledger and sets migrations. That is a heavy player:
nearly everyone is steering, so each gets the possession sub-step states
too (2.7 states a second instead of 1). Every `--report` seconds it reads
the new `GET /api/metrics` (world-thread timings, queue depth, dropped
connections, skipped state frames, save timings, bytes out, process CPU and
memory) and prints a line; at the end a summary (`--json FILE` for all of
it), a graceful stop and a scan of the server log for panics.

**Results** of the first round, all JSON states (protocol 2 below; 512²,
warmed to 22k organisms and growing to 28–34k, 1 s ticks,
1 s state interval, 150 s after a 20–60 s ramp; 4-core container shared
with other busy agents, load average 6–14, server and load generator on the
same machine, so these are pessimistic):

| | 50 players | 200, before | 200 | 500, before | 500 |
| --- | --- | --- | --- | --- | --- |
| Ticks per second (target 1) | 1.0 | 1.0 | 1.0 | **0.3** | 1.0 |
| Tick (step + bookkeeping) p50 / p99 | 78 / 167 ms | 77 / 166 ms | 81 / 216 ms | 150 / 325 ms | 114 / 284 ms |
| One state push to all, p50 / p99 | 2.6 / 8 ms | 102 / 141 ms | 7.4 / 16 ms | 323 / 576 ms | 25 / 76 ms |
| Possession sub-step, p99 | 20 ms | 164 ms | 52 ms | 617 ms | 121 ms |
| State message p50 / p99 | 16 / 66 KB | 35 / 80 KB | 21 / 66 KB | 69 / 110 KB | 31 / 68 KB |
| Outbound, total (per client) | 3.0 MB/s (61 KB/s) | 23 MB/s (117 KB/s) | 17 MB/s (84 KB/s) | 45 MB/s (90 KB/s)\* | 57 MB/s (115 KB/s) |
| Commands per second | 24 | 98 | 98 | 250 | 243 |
| Command → result p50 / p99 | 132 / 253 ms | 145 / 273 ms | 147 / 263 ms | 748 / 1700 ms | 203 / 748 ms |
| Most requests queued at one boundary | 14 | 40 | 43 | 477 | 236 |
| Server CPU (cores) / RSS max | 0.1 / 263 MB | 0.5 / 389 MB | 0.2 / 390 MB | 0.6 / 611 MB | 0.5 / 704 MB |
| Save: capture (world thread) / encode / write | 87 / 292 / 74 ms | 50 / 457 / 86 ms | 81 / 445 / 87 ms | 378 / 1554 / 89 ms | 45 / 1284 / 235 ms |
| Dropped connections, skipped states, panics | 0 | 0 | 0 | 0 | 0 |

"Before" is the same server without the fixes below (same metrics); \* it
pushed fewer states because the world fell to 0.3 ticks per second. Command
latency has a floor of about half a sub-step (125 ms at the 1 s clock with 4
sub-steps): commands apply at the next sub-step boundary. Saves are 30–36 MB
at this size. The load generator used 0.1–0.6 cores.

**Soak:** 200 players in fast mode (`--fast`: 20 ms ticks, states every
100 ms) for 30 minutes on the same machine. The world could not hold 20 ms
(the box was saturated, load average 15–18, the server got about 0.6 cores),
so it ran as fast as it could, about 2 ticks a second: 3,350 ticks, 70
simulated days, 67 saves, population 31k → 54k → 45k, 5.7 states a second
per client (45 KB median), 93 commands a second, 52 MB/s out. No dropped
connections, no skipped state frames, no unexpected closes, at most 122
requests queued at one boundary. Memory followed the population and the
save copies (378 MB at the start, 500–565 MB at 50k organisms between
saves, 680–790 MB just after a save) and plateaued; nothing grew with time
alone. State pushes stayed at 25 / 82 ms (p50 / p99) for 200 connections at
50k organisms. That run also hit a full disk and found the saver crash
below; a second 20-minute soak with the fixes (3,700 ticks, 77 days, 90
saves, 42–56k organisms, 8.3 states a second per client, 88 MB/s out,
commands 114 / 382 ms p50 / p99) ended with no dropped connections, no
skipped states, not one warning or error in the log, a clean shutdown and
RSS flat at 565 MB for its last ten minutes.

**What it found and what changed:**

- **State building was O(connections × organisms).** Each state scanned
  every organism twice (own organisms, view window), again for the
  possessed organism and the lineage centroid, and every offer, so one push
  to 500 connections at 30k organisms took 320–580 ms of the world thread,
  and the possession sub-steps (a push to every steering player, three
  times a tick) as much again: the world fell to 0.3 ticks per second. Now
  a push builds one `StateIndex` (organisms by owner and by 16-tile cell,
  open offers by player, the shared clock, weather and leaderboard) in one
  pass, and each state costs only what it contains: 13× faster at 200
  players, 13× at 500 (25 ms), and the clock holds. A test checks that the
  indexed states are identical to the old scan's for every connection
  (pinned views at and off the map edge, the `max_nearby` cut, offers,
  possession sub-steps).
- **The leaderboard was a third to half of every state** (all players,
  ~95 bytes a row: 48 KB at 500 players) and the client showed all of it.
  States now carry the top 50 rows plus the player's own and those of the
  owners in view (the client looks up owner names there); `GET
  /api/leaderboard` keeps every row. A test checks the cut.
- Every queued message was cloned once per recipient, including states,
  which have one; the last (usually only) recipient now takes the message.
- **Logging could kill the saver.** The soak filled the disk (other jobs
  share it): the log write failed, `tracing-subscriber` reported that with
  `eprintln!`, which panicked on the full stderr, inside the saver thread,
  and saves stopped for good. `log_internal_errors(false)` now drops log
  lines that can't be written, and a panic while encoding skips that save
  only.
- **Encoded saves could pile up behind a slow store** (the persist channel
  is unbounded and a save is 30–60 MB). The saver now waits until the store
  task has written the save it sent before taking the next capture; newer
  captures replace the waiting one meanwhile. A test checks it.
- `GET /api/metrics` exposes all of the above for operators.

**Delta states (protocol 2).** Bandwidth was the first limit: a steering
player got about 115 KB/s, an idle one 20–30 KB/s, because the 256 nearest
foreign organisms with their `look` were most of each state and steering
players got a whole state at every sub-step. Measured candidates, same
warmed world, 150 s after the ramp (the box shared with other jobs, load
average 4–8):

| | 200, JSON (before) | 200, protocol 2 | 500, JSON (before) | 500, protocol 2 |
| --- | --- | --- | --- | --- |
| Outbound, total (per client) | 17.3 MB/s (87 KB/s) | **2.7 MB/s (14 KB/s)** | 58 MB/s (116 KB/s) | **7.4 MB/s (15 KB/s)** |
| State traffic per client | 75 KB/s | **0.9 KB/s** | 103 KB/s | **1.2 KB/s** |
| State message p50 / p99 / max | 21 / 66 / 68 KB | 57 B / 5.9 / 11 KB | 32 / 68 / 70 KB | 76 B / 6.7 / 13 KB |
| Server time per encoded state (mean) | 173 µs | 86–105 µs | – | 86 µs |
| One state push to all, p50 / p99 | 5.8 / 16 ms | 6.3 / 11 ms | 19 / 37 ms | 20 / 47 ms |
| Tick p50 / p99; sub-step p99 | 61 / 101; 29 ms | 59 / 109; 35 ms | 62 / 107; 37 ms | 51 / 113; 49 ms |
| States per client per second | 2.7 | 2.7 | 2.8 | 2.8 |
| Command → result p50 / p99 | 141 / 250 ms | 143 / 252 ms | 153 / 260 ms | 162 / 256 ms |
| Server CPU (cores) / RSS max | 0.2 / 425 MB | 0.2 / 420 MB | 0.47 / 665 MB | 0.52 / 636 MB |
| Dropped, skipped, decode errors, resyncs | 0 | 0 | 0 | 0 |

"JSON (before)" is the server before this change; its encode time comes
from the new server with `--proto 1` players (17.2 MB/s, the same traffic).
"State traffic" is the state bytes the clients received; the rest of the
total is command replies and events.

- **What each technique gives** (bytes of state traffic): deflate with
  context takeover (`--deflate`) shrinks the JSON states **18×** (the same
  keys and `look`s over and over); delta states with `look` palettes,
  quantized positions and the binary records shrink them **80–85×** on
  their own (a tick's delta is mostly two-byte position changes, a sub-step
  only the possessed organism), and deflate would take another 2.8× off
  those. The WebSocket stack here (axum 0.7, tungstenite 0.24) has no
  permessage-deflate, and delta states alone already take state traffic
  below the rest, so protocol 2 has no compression.
- **CPU:** encoding a delta costs less than serializing the JSON did (a
  micro-benchmark, `wire::tests::encoding_cost`: 256 nearby and 12 own
  organisms, 54 leaderboard rows: JSON 110 µs and 64 KB; a tick's delta
  18 µs and 1.8 KB; an unchanged state 11 µs; a keyframe 36 µs and 8.5 KB);
  in the writers, under load, 86–105 µs against 173 µs. The world thread
  does what it did before (it still builds every `StateView`); the
  encoder runs in the connection writers, keyed to what was sent.
- **What was left:** with states down to about 1 KB/s a client, the load
  generator's own market browsing was most of the traffic: `browse_market`
  returned every open listing with its traits (up to 200, hundreds open at
  200–500 players), 10–11 KB/s a client at ~3 browses a minute. It is now
  paged (below). A player who doesn't browse gets about 1–1.5 KB/s:
  0.9–1.2 KB/s of states plus replies and events.

**Paged market browsing.** `browse_market` now returns a page of 20 (see
[Economy](#economy-and-stud-marketplace)); the load generator's players ask
for the first page without their own listings, sometimes the next page
(the cursor from the last reply) or the cheapest or closing-soonest first,
and 30% of their new listings are auctions (they bid the lowest accepted
bid on auctions they find). Same warmed world (24k organisms, copied for
each run), 200 players, 150 s after a 20 s ramp, 1 s ticks, the old server
and load generator against the new (box shared with other jobs):

| | before | after |
| --- | --- | --- |
| Outbound, total (per client) | 2.74 MB/s (13.7 KB/s) | **0.47 MB/s (2.3 KB/s)** |
| `browse_market` replies per client | 11.2 KB/s | **1.2 KB/s** |
| State traffic per client | 0.91 KB/s | 0.91 KB/s |
| `browse_market` calls / p50 / p99 | 1941 / 151 / 262 ms | 1839 / 137 / 250 ms |
| Commands per second; command p50 / p99 | 98.6; 148 / 254 ms | 98.4; 138 / 252 ms |
| Server CPU (cores) | 0.18 | 0.15 |
| Dropped connections, panics | 0 | 0 |

A browse reply went from ~190 KB to ~20 KB; the server still filters
and sorts every open listing for each page (a few thousand at most: 8 per
player), which costs less than serializing 200 of them did.

Next on the world thread is the sim tick itself (the sub-step states now
reuse the push's index and views: see [High
density](#high-density-decide-act-sweep-sub-step-states)).

**Capacity recommendation** (512², 25–35k organisms, 1 s ticks):

- **Up to 200 players per world on 4 cores.** The world thread is busy
  about 15% of the time (tick ~60–80 ms, pushes ~6 ms, sub-steps ~35 ms
  p99) and command latency stays at its sub-step floor (p99 ~250 ms).
  With protocol 2 clients the server needs about 3 MB/s (22 Mbit/s) of
  egress for 200 heavy players before paged browsing, 0.5 MB/s with it (states are
  ~1 KB/s a client); old JSON clients need 87 KB/s each (17 MB/s).
- **500 players per world** held the clock on this shared 4-core box
  (tick p99 ~110 ms, pushes p99 ~45 ms, command p99 ~260 ms) with 7.4 MB/s
  (60 Mbit/s) out on protocol 2, against 58 MB/s (470 Mbit/s) with JSON
  states: a 100 Mbit/s link is enough when clients speak protocol 2.
  This round planned 4–6 dedicated cores per world; the multi-world runs
  ([below](#several-worlds-capacity)) measured about 2 cores per 500-player
  world, so grow with more worlds instead of bigger ones; bandwidth is no
  longer what sets the size of a world.
- Memory: about 0.4 GB at 200 players and 0.7 GB at 500 including save
  copies; budget 1 GB per world. Saves take 45–90 ms of the world thread
  (the copy) and 0.3–1.3 s of the saver thread per simulated day.
- Keep rate limits on in production (the test turns them off only because
  all its players share one IP).
- **More worlds per process** (`--world`, above) scale out instead of up:
  each world brings its own world thread and saver, so worlds tick in
  parallel and only share cores, memory and the network. A quick check on
  this shared, noisy 4-core box (load average ~9 from other jobs during the
  runs; 256² worlds, no warm-up, ~2.5k organisms each, 90 s after a 20 s
  ramp, `loadgen --players 100` against `loadgen --worlds 2 --players 200`):

  | | one world, 100 players | two worlds, 100 players each |
  | --- | --- | --- |
  | tick p50 / p99 (ms) | 8.8 / 40.9 | main 10.7 / 35.6, w2 9.7 / 66.4 |
  | state push p99 (ms) | 7.0 | 5.7, 8.1 |
  | command latency p50 / p99 (ms) | 135 / 254 | 137 / 258 |
  | server CPU (cores) | 0.04 | 0.08 |
  | RSS max (MB) | 203 | 226 |
  | out per world (MB/s) | 0.74 | 0.71 (main) |

  Per-world timings and command latency stay where one world had them, CPU
  doubles with the load and memory grows by the second world's size: the
  second world costs what a second process would, without the second
  process. The p99 spread is the box's noise (one stalled sample sets a
  p99 of the 256-tick window). The 512², 500-player runs below replace the
  4–6 cores per world above with 1.5 worlds per core (6 on 4 cores); past
  that run more processes (each with its own world list) behind the same
  accounts store.

### Several worlds: capacity

The 500-player run repeated with several worlds in one process
(`loadgen --worlds N`): 512² worlds warmed to 22k organisms each (one
warm-up with `--worlds 2 --warm-until 22000`, copied for every run; the
single world is that warm-up's `main`), growing to 30–35k during the run;
1 s ticks, 1 s states, protocol 2, 150 s after the ramp (40 s for 500
players, 80 s for 1,000, so registrations arrive at 12.5 a second in every
run). Same 4-core container, shared with other agents' CPU-bound jobs:
the load average is given for each run, and **the box was oversubscribed
in all of them** (4 cores, load 4–11 from other jobs when each run started);
these are pessimistic numbers. Timings are the medians of the 10 s reports
after the ramp, as in the tables above; command latency is over all of a
world's players' commands. Measured before prey switching (world snapshot
version 9) was merged.

| | 1 world × 500 | 2 worlds × 250 | 2 worlds × 500 |
| --- | --- | --- | --- |
| Load average (1 min) during the run: min / median / max | 7.6 / 9.2 / 10.3 | 4.2 / 8.8 / 10.9 | 8.8 / 11.5 / 13.2 |
| Ticks per second (target 1) | 1.0 | 1.0, 1.0 | 1.0, 1.0 |
| Organisms at the end | 33.4k | 30.4k, 31.7k | 34.9k, 35.5k |
| Tick p50 / p99 (ms) | 82 / 204 | 135 / 268, 137 / 352 | 115 / 268, 108 / 257 |
| One state push to all, p99 (ms) | 48 | 44, 39 | 83, 80 |
| Possession sub-step, p99 (ms) | 57 | 64, 58 | 82, 92 |
| Command → result p50 / p99 (ms) | 146 / 270 | 145 / 273, 145 / 281 | 148 / 358, 159 / 464 |
| Outbound MB/s, per world (total) | 1.36 | 0.61, 0.64 (1.28) | 1.30, 1.41 (2.74) |
| Outbound per client | 2.7 KB/s | 2.6 KB/s | 2.7 KB/s |
| Server CPU (cores), whole process | 0.27 | 0.33 | 0.57 |
| RSS, all players on / at the end (MB) | 482 / 525 | 835 / 912 | 866 / 999 |
| Skipped state frames, dropped connections, panics | 34, 0, 0 | 40, 0, 0 | 45, 0, 0 |

Per-world pairs are `main`, `w2`. Each column was run twice (and the
baseline twice more); the second runs agree except where noted:

- **2 × 250, first run at load 15–20** (another agent's build started):
  tick p99 1.5–1.6 s, command p99 7 s, 3,485 skipped states and RSS 1.8 GB.
  A rerun at load ~9 (the column) and a third (load 5–14: tick p50 65 ms,
  one stall putting p99 at 455 ms and command p99 at 1.4 s) did not repeat
  it. With 4 cores and 9–20 runnable threads, a world thread's tick is
  mostly waiting for a core: CPU use stayed at 0.3 cores either way.
- **2 × 500, first run with a 40 s ramp** (25 registrations a second,
  load up to 23): **both world threads stopped ticking for about 40 s**
  during the ramp, RSS rose to 2.9 GB and command p99 reached 12 s; after
  the ramp both worlds held 1 tick per second. The cause is the login path,
  not the worlds: every registration and login hashes a password with
  Argon2 (19 MiB, tens of ms of CPU) on Tokio's blocking pool, which runs
  up to 512 at once, so a burst of logins becomes hundreds of CPU-bound
  threads that starve the world threads and hold ~19 MB each. With the
  ramp at 12.5 a second (as in every other run) the same 1,000 players
  ramped without a stall (load 12–14 and 9–13 in the two runs). Hashing
  is now bounded (see below).
- One world × 500 matches the earlier single-world runs (tick 82 / 163–204,
  command p99 253–270 ms, 0.26–0.27 cores) in both runs.

What it shows:

- **A second world costs what the first did.** Two 500-player worlds used
  2.1× the CPU of one (0.57 against 0.27 cores), 1.9× the memory (about
  0.5 GB per world including save copies) and exactly twice the egress
  (2.7 KB/s a client wherever the client is); per-world push and sub-step
  times rose by about 70% (80–83 against 48 ms) and command p99 to 360–460 ms,
  which on this box is the two worlds and the other agents competing for
  cores, not a shared lock: nothing in the server serializes worlds.
- **Splitting players over worlds** (2 × 250 against 1 × 500) halves each
  world's push cost but adds a second sim tick, so it uses a little more
  CPU (0.33 against 0.27 cores) and nearly twice the memory for the same
  players; it buys nothing in latency at this size. Split by world size
  and community, not for performance.
- **Recommendation (512², 25–35k organisms, 1 s ticks):** **500 players
  per world**. This round's "about 2 cores per world" was the shared box's
  limit, not the server's: on a quiet box, with the worlds' clocks
  staggered, one process holds **6 worlds per 4 cores** with every latency
  at its floor, and 8 per 4 cores is where it stops scaling (see
  [Three to eight worlds](#three-to-eight-worlds-where-one-process-stops-scaling)
  for the measurements and the rule of thumb). Budget 0.5 GB of memory and
  1.4 MB/s (11 Mbit/s) of egress per 500-player world with protocol 2
  clients. Login storms no longer stall the worlds (below); the default
  hash bound (2 at once on 4 cores, ~45 hashes a second) admits about 22
  new players a second (a registration and a login each) and makes the
  rest wait a few seconds.

#### Three to eight worlds: where one process stops scaling

1, 2, 3, 4, 6 and 8 worlds of 500 players each in one process
(`loadgen --worlds N --players 500N`, ramp at 12.5 registrations a second,
150 s after the ramp), 512² worlds warmed to 22k organisms (1–4 worlds:
one 4-world warm-up; 6 and 8: an 8-world warm-up, 22–27k), growing to
34–36k (36–42k with 6 and 8) during the run; 1 s ticks, 1 s states,
protocol 2. This time the 4-core box was **quiet**: nothing else ran
(load average under 0.5 before the first run), except the load generator
itself, which used 0.15 cores per 500 players on the same cores. Per-world
values are the medians of the 10 s reports after the ramp; where worlds
differ, the range over the worlds. Per-thread CPU comes from sampling
`/proc/<pid>/task` (the sim pool's threads are now named `sim-<n>`).

| Worlds × 500 players | 1 | 2 | 3 | 4 | 6 | 8 |
| --- | --- | --- | --- | --- | --- | --- |
| Ticks per second (target 1), before / after | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 / **0.99–1.0** |
| Tick p50 / p99 (ms), clocks aligned (before) | 36 / 58 | 61 / 89 | 87–90 / 121–128 | 108–115 / 163–179 | 183–204 / 368–383 | 253–328 / 533–592 |
| Tick p50 / p99 (ms), staggered (after) | 36 / 58 | 47–48 / 63–71 | 41–46 / 64–79 | 59–77 / 100–114 | 51–53 / 73–119 | 86–94 / 300–433 |
| State push p99 (ms), before / after | 18 | 29–33 / 18–20 | 37–39 / 21–22 | 38–43 / 20–27 | 55–60 / 21–36 | 62–80 / 56–83 |
| Command → result p99 (ms), before / after | 249 | 250 / 249 | 251 / 250 | 251–257 / 250–257 | 365–387 / 253–255 | 655–726 / 378–508 |
| Skipped state frames, before / after | 0 | 0 / 0 | 0 / 0 | 0 / 0 | **8,691** / 0 | **30,050** / 252–1,236 |
| Server CPU (cores), whole process | 0.27 | 0.54 | 0.81–0.90 | 1.07–1.20 | 1.66–1.69 | 2.23–2.30 |
| … world threads / sim pool / Tokio (before) | 0.10 / 0.08 / 0.09 | 0.20 / 0.17 / 0.17 | 0.30 / 0.26 / 0.25 | 0.40 / 0.34 / 0.32 | 0.62 / 0.53 / 0.45 | 0.87 / 0.76 / 0.62 |
| Load generator CPU (cores) | 0.15 | 0.30 | 0.43–0.52 | 0.55–0.60 | 0.85–0.92 | 1.09–1.16 |
| Load average (1 min) at the end, after | 0.2 | 0.2 | 2.8 | 1.7–6.8 | 5.4 | 10.3 |
| RSS, all players on / max (MB) | 398 / 429 | 663–679 / 776–790 | 972–1,007 / 1,140–1,182 | 1,327–1,460 / 1,574–1,639 | 2,009–2,081 / 2,291–2,348 | 2,739–3,029 / 3,118–3,229 |
| Outbound, all worlds (MB/s) | 1.35 | 2.7 | 4.15 | 5.6–5.7 | 8.7 | 11.7–12.0 |

"Before" is this branch's server with every world's clock started at the
same moment (as before); "after" staggers them (below). Both runs of a
column share the warm-up. 4 and 8 worlds "after" were run twice (with the
first and final build of the change); the ranges cover both. Dropped
connections and panics were 0 in every run; command p50 was 135–177 ms
(its floor, half a sub-step, plus the round trip) throughout.

What it found:

- **The worlds' clocks were a serialization point.** With the clocks
  aligned, each world added 25–35 ms to *every* world's tick (p50 36, 61,
  88, 110, 195, 290 ms for 1–8 worlds) while the process used a quarter to
  a half of the cores. Every world thread starts its clock when it is
  spawned, and they are spawned together, so all worlds ticked in the same
  millisecond, every second: their parallel decide and apply phases queued
  in the one sim pool (rayon's global pool runs the work a thread injects
  mostly after the work already in it, so the worlds' parallel phases ran
  nearly one after the other) and their sequential parts queued for the
  cores beside them. The mean load was low; the burst once a second was
  not. At 6 worlds the burst held state pushes long enough to skip 8.7k
  state frames, at 8 worlds 30k, and command p99 rose to 0.4 and 0.7 s.
- **Fix: staggered clocks.** World `i` of `n` now starts its clock `i/n` of
  a tick later (`RunnerConfig::phase`, `runner::world_phase`), so ticks
  land on the shared cores one at a time. Tick p50 with 6 worlds fell from
  ~195 to ~52 ms, p99 from ~375 to 73–119 ms, command p99 back to its
  floor (255 ms) and skipped states to 0; with 2–6 worlds every latency is
  within a few ms of its single-world value. Nothing in the sim changed:
  only when, in wall time, each world ticks (sim results are bit-identical).
  Two alternatives measured worse and were not kept: spreading the worlds
  over one sub-step interval (250 ms) so that no tick met another world's
  sub-step put the ticks closer together (8 worlds: tick p50 100–260 ms);
  a rayon pool per world (each world thread `install`ed in its own
  4-thread pool) was slower with 4 worlds (tick p50 72–84 against 66–77,
  p99 119–134 against 110–114) at the same CPU and cost 460 MB more RSS
  (per-thread route buffers and allocator arenas), and with 8 worlds traded
  a lower tick p99 (228–270 ms) for a higher p50 (93–104) and command p99
  (410–429 against 378–396 ms). The shared pool is not the bottleneck once
  the ticks don't coincide.
- **Where it stops: 8 worlds on 4 cores.** With 8 worlds the process used
  2.3 cores and the load generator 1.2, 3.5 of 4 cores on average and all
  of them in bursts (load average ~10; the sim threads waited 1.6–1.7 s
  per second for a core, against 0.4–0.6 s with 4–6 worlds): ticks held
  (0.99–1.0 per second) but tick p99 rose to 300–430 ms, command p99 to
  380–500 ms and a few hundred state frames were skipped. That is CPU saturation; no lock
  or shared thread shows in the profile (the world threads use 0.10–0.12
  cores each at every N, the Tokio runtime's 4 workers 0.08 cores per
  world, the store and saver threads next to nothing).
- **Cost per world is flat.** CPU 0.27–0.29 cores per 500-player world
  (world thread 0.10, sim pool 0.09, network 0.08), RSS 0.39 GB per world
  plus ~50 MB, egress 1.4–1.5 MB/s per world (2.8–3 KB/s per client) at
  every N.

**Capacity rule of thumb** (512², 500 heavy players and 30–40k organisms
per world, 1 s ticks, protocol 2 clients):

- **CPU:** 0.3 cores per world on average, but the work comes in bursts
  (a tick, three sub-steps and a push per second per world), so keep the
  process's mean below about 60% of its cores: **1.5 worlds per core**
  (6 worlds on 4 cores measured with every latency at its floor, with the
  load generator's 0.9 cores on the same box). A dedicated 4-core box
  would likely hold 8 (2.3 cores without the load generator), unmeasured.
  That is at 30–40k organisms per world: over hours, 500 heavy players
  grow a 512² world to 60–72k, where a world cost ~0.5 cores (6 worlds
  took 2.8–2.9) and tick p99 reached ~1 s on this box (see [the
  soak](#six-worlds-for-three-hours-soak)). With the cheaper tick of
  [A server-sized world at 60–70k organisms](#a-server-sized-world-at-6070k-organisms)
  a 60–70k world now costs **0.4 cores** (measured: six took 2.3–2.5 in the
  soak's *Confirmation run*, every latency at or near its floor, with the
  load generator's 0.6 cores on the same box), but six worlds still
  saturated 4 cores at the 70–73k peak (2.8 cores, tick p99 0.7 s,
  command p99 0.5 s). If worlds run that full, budget **1.25 worlds per
  core** (5 on 4 cores) with headroom for the peak; a dedicated 4-core box
  (no load generator) would likely hold 6, unmeasured.
- **Memory:** 0.4 GB per world measured at 30–40k organisms and again at
  60–70k with jemalloc (2.45 GB for six worlds and the process, flat for
  hours), **budget 0.5 GB** (save copies, peaks) plus 0.1 GB for the
  process.
- **Bandwidth:** **1.5 MB/s (12 Mbit/s) per world** of egress; a 100 Mbit/s
  link carries 8 worlds, a 1 Gbit/s link any box's CPU limit.
- **Worlds per process:** **at most 1.5 per core, 6 on a 4-core box**
  (1.25 per core if the worlds grow to 60–75k organisms); past
  that, run another process (or box) with its own world list behind the
  same store. The sim pool and the Tokio runtime both size themselves to
  the cores, and nothing else is shared per world, so a bigger box should
  scale with its cores; only 4 cores were measured. Worlds with fewer
  players should cost less network CPU (0.08 of the 0.28 cores) but about
  the same sim time; unmeasured.

#### Six worlds for three hours (soak)

The open question from the runs above: does a 6-world process that holds
every latency at its floor for 150 s keep doing so for hours? One
`evo-server` with 6 512² worlds × 500 simulated players (3,000 WebSockets),
1 s ticks, 1 s states, protocol 2, warmed to 22–25k organisms per world
(130 s in fast mode), 240 s ramp, then **3.0 hours** (10,800 s after the ramp; 230 saves
per world, ~262 simulated days, a little over two simulated years); `--csv` every 30 s,
the box's load average and free disk every 5 minutes. Same 4-core
container. **Caveat: the box was not quiet for all of it.** Other agents
ran mobile and browser e2e smokes (headless Chromium, Android/iOS harnesses)
on the same cores at times, certainly over the last 25 minutes (the load
average reached 10–12 while the server used 1.6 cores and the load
generator 0.3, so 2+ cores went to other processes); the load-average
samples at 1–2.5 show when they did not.

Medians of the 30 s rows; ranges are over the 6 worlds:

| | first 15 min after the ramp | last 15 min |
| --- | --- | --- |
| Organisms per world | 44.6–48.3k (growing from 36k to 55k) | 46.7–57.5k |
| Ticks per second (target 1) | 1.0 | 1.0 |
| Tick p50 / p99 (ms) | 64–70 / 149–191 | 167–260 / 829–932 |
| State push p99 (ms) | 34–49 | 113–163 |
| Command → result p50 / p99 (ms) | 136–137 / 259–265 | 154–163 / 686–846 |
| Save: capture / encode / write (ms) | 35–69 / 347–505 / 54–73 | 101–132 / 933–1,356 / 97–141 |
| Save size (MB) | 13.0–13.7 | 16.5–19.8 |
| Server CPU (cores) / load generator | 2.0 / 1.0 | 1.6 / 0.3 |
| RSS median / max (MB) | 2,550 / 2,910 | 4,836 / 4,901 |
| Load average (1 min) | 5–7 | 10.6–12.5 (other agents' jobs) |

By half hour (all worlds; "organisms" is the sum over the 6):

| Hours after start | 0–0.5 | 0.5–1 | 1–1.5 | 1.5–2 | 2–2.5 | 2.5–3 | 3–3.07 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Organisms, all worlds | 307k | 414k | 312k | 364k | 351k | 250k | 325k |
| Tick p99 (ms) | 222 | 822 | 374 | 456 | 468 | 448 | 1,356 |
| Command p99 (ms) | 265 | 716 | 322 | 422 | 392 | 282 | 1,203 |
| Server CPU (cores) | 2.1 | 2.8 | 2.0 | 2.2 | 2.2 | 1.5 | 1.6 |
| RSS min / max (MB) | 2,044 / 3,527 | 3,537 / 4,129 | 4,089 / 4,452 | 4,423 / 4,682 | 4,596 / 4,727 | 4,669 / 4,883 | 4,874 / 4,901 |
| Save size, median (MB) | 14.6 | 20.9 | 18.6 | 20.4 | 20.5 | 16.9 | 18.4 |

Over the whole run: all 3,000 players stayed connected (0 unexpected
closes, 0 dropped connections, 0 protocol 2 decode errors, 0 resyncs), 11.2
million commands (10.5 million accepted, 0.63 million refused by the rules:
`too_far`, `already_pregnant`, ...; all but the ~120 in flight at the stop
answered), all 1,380 saves written, no panic, a clean shutdown, and not one
error in the log; the only warnings were 17 "store is slow" (the saver
waiting for the previous write) between 0.35 and 0.8 h, at the population
peak. 2.0 million state frames (about 5% of those due; the clients got
41 million) were coalesced away,
nearly all at the peak and in the last 15 minutes.

What it shows:

- **The population, not the clock, sets the cost, and with 500 heavy
  players a 512² world does not stay at 30–40k.** The worlds grew from 22k
  to 71k organisms each by 0.8 h (simulated autumn of year 0; players'
  lineages were 15k of them), then cycled with the seasons between about
  40k (winter) and 62k (a simulated year is 120 days, 96 minutes). At
  60–72k per world six worlds saturate the box: the process used 2.8–2.9
  cores (plus 0.9 for the load generator), tick p50 rose to 270–470 ms,
  p99 to 0.8–1.2 s, command p99 to 0.7–1 s, and the state frames were
  skipped; ticks still held at 1 per second. When the population fell back
  everything recovered without a restart.
- **Same population, later in the run, costs more, but not steadily
  more.** Binned by organisms per world (per-report medians over the
  worlds): at 45–50k tick p50/p99 was 70/199 ms in the first hour and
  163–164/350–359 ms in hours 1–2.7 (and 169/350 ms at 1.2 h with the
  load average at 1.6, so nothing else was running); at 55–60k, 92/311
  against 226–285/402–530 ms. But at 40–45k it was 58/138 in the first hour
  and 73–79/198–207 at 2–2.7 h, and the hourly medians do not climb: the
  cost follows what the world is doing (season, species mix and where the
  herds are; player organisms fell from 15k to 2–5k per world as most of
  the 500 player lineages died out), not time alone. Since measured: it is
  how tightly the herds pack. At the same population, a winter world has
  57% more neighbors per organism within the 8-tile perception radius
  than a summer one, and a decision's cost follows its neighbors (see
  [A server-sized world at 60–70k organisms](#a-server-sized-world-at-6070k-organisms),
  which also made that case 22% cheaper). What part of the
  last 15 minutes is the other agents' load cannot be separated; command
  p99 was at or near its floor (250–290 ms) whenever the box was quiet
  and the population under ~50k.
- **Memory: rose with the population, then crept.** RSS was 2.0 GB with all
  players on, 4.1 GB at the population peak (0.9 h), and then crept from
  4.13 to 4.90 GB over the last 2.1 hours while the worlds held fewer
  organisms than at the peak (+0.25–0.37 GB an hour, flat for half an
  hour at a time). The fitted slope over the whole run (+0.66 GB/h) is
  mostly the first hour's population growth. The saved world state did not
  creep (save size follows the organism count: 0.29–0.38 KB per organism;
  the one field that grew with time is fixed below), so the creep is
  outside the sim state; the likely cause is glibc malloc keeping the
  peak's freed memory in its per-thread arenas (no custom allocator; ~20
  threads allocate: Tokio, the sim pool, 6 world threads, the saver) or
  per-connection buffers keeping their high-water capacity. **Since
  confirmed and fixed** (glibc kept the peak's freed memory; jemalloc is
  now the allocator): *Memory after a population peak* below.
- **Found and fixed: the world kept every day's legacy snapshot.**
  `World::legacy` got a `LegacySnapshot` (two maps of every player's
  allele share) each simulated day and never dropped one, though only the
  latest is read (scores, the daily record, the chronicle; the store keeps
  the history). After this run's 262 days it held 111,766 map entries,
  325 KB of the 17.2 MB compressed save; at the 1 s clock a world lives
  1,800 days a real day, so a month-old 500-player world would carry on
  the order of 0.6 GB of them in memory, clone them at every capture and
  write ~70 MB more per save. It now keeps the last 8 days
  (`legacy::LEGACY_KEPT`; sim results unchanged: nothing reads the older
  ones; a test checks the window; `calibrate` collects its own daily
  series). It explains none of the RSS creep above (a few MB per world).
- **Found and fixed: `history.jsonl` grew ~45 MB an hour per world** at
  500 players (one daily record a simulated day, ~700 KB: a row per lineage
  with its mean genes), about 1 GB per world per real day with the file
  store, and the history endpoints read the whole file. Snapshots stay
  bounded (3 per world, 17–21 MB each here). The daily history now has a
  retention policy in every store (*Daily history retention* in
  [Persistence](#persistence): 30 whole days, a year of every day with gene
  means every 8th day, then geometrically sparser samples), the file store
  keeps it in 32-line segment files that reads walk from the newest, and
  `/api/leaderboard/history` takes a day range (`from`, `to`) and caps
  `limit` at 365: at 500 players a world's history is ~90 MB after a real
  day and ~130 MB after a real year instead of growing ~1.2 GB a day
  (`tests/history.rs` runs a world for 830 days and checks the bound). The
  world thread's in-memory window of 365 daily records is thinned the same
  way (~250 MB of JSON per 500-player world before, ~80 MB now); it was
  still filling during this run, so it may account for part of the RSS
  growth above (not measured).

**Memory after a population peak.** To find the creep above without
another 3-hour run: one process with 2 512² worlds × 500 players (1,000
WebSockets), the clock at 2× (`--tick-ms 500`, states every 1 s), both
worlds warmed to 22.4k organisms once and that save copied for each run,
120 s ramp, 30 minutes; one run per allocator, one after another, nothing
else heavy on the box (load average ~3). Each run went through the same
population curve: 45k organisms (both worlds) after the ramp, a peak of
143–145k at 22 minutes, 105–109k at the end. `GET /api/metrics` now reports
the allocator's own statistics (`heap`: live allocations and what it holds
besides; glibc's `mallinfo2`, jemalloc's `stats`) next to RSS, so a leak
(live heap growing) can be told from allocator retention (RSS growing while
the live heap does not). MB:

| | glibc (before) | glibc, `MALLOC_ARENA_MAX=2` | mimalloc | **jemalloc** |
| --- | --- | --- | --- | --- |
| RSS at the peak / at the end / max | 1,349 / 1,433 / 1,433 | 965 / 1,000 / 1,015 | 1,158 / 1,211 / 1,358 | **882 / 865 / 1,032** |
| RSS 2 min after the ramp (72k organisms) | 756 | 513 | 652 | **479** |
| Live heap at the peak / at the end | 777 / 734 | 780 / 733 | – | 796 / 742 |
| Held by the allocator, not live, at the end | 726 | 285 | – | 190 |
| RSS slope after the peak (MB/h) | **+717** | +335 | +156 | **−343** |
| Live heap slope after the peak (MB/h) | −411 | −544 | – | −588 |
| Tick p50 / p99, medians of the 30 s rows (ms) | 87 / 187 | 82 / 193 | 80 / 155 | 81 / 155 |
| … at 120–145k organisms | 96 / 206 | 102 / 213 | 95 / 201 | 93 / 192 |
| Command p99 (ms) / server CPU (cores) | 151 / 1.47 | 160 / 1.48 | 140 / 1.26 | 140 / 1.34 |

- **It is the allocator, not a leak.** With glibc, after the peak the live
  heap fell (−411 MB/h) while RSS kept climbing (+717 MB/h): at the end
  half of the resident memory (726 of 1,433 MB) was free memory glibc held
  in its arenas. That is the soak's creep, faster here because the clock
  ran at 2× and the peak was steeper. The live heap fell with the
  population in every run, so nothing in the server or the sim grows on
  its own.
- **jemalloc fixes it at no cost in tick time:** RSS follows the
  population back down (−343 MB/h after the peak, 865 MB at the end
  against 1,433), 35–40% less memory throughout, tick and command times
  equal or a little better (the differences are within this box's noise).
  `MALLOC_ARENA_MAX=2` halves glibc's retention but still creeps (+335
  MB/h) and its tick p99 at the peak was the worst; mimalloc creeps less
  than glibc but holds 300–350 MB more than jemalloc. So `evo-server` now
  uses jemalloc (`tikv-jemallocator`), behind the default-on cargo feature
  `jemalloc` (`alloc.rs`; `--no-default-features` builds with the system
  allocator; the Docker image installs `make` for jemalloc's C build). No
  dropped connections, unexpected closes, decode errors or panics in any
  run (the glibc run's final summary was cut off at 29.5 minutes; its CSV
  is complete to there).
- **Code review for anything else that grows** (server and sim): the
  per-world and per-player maps are pruned (offers, watched and alerted
  organisms on death, alerts and ledger capped per player, closed
  listings, sessions, rate-limit buckets, standings cache) or keyed by
  what is saved, and the saved game did not creep; per-connection state
  (the delta encoder's records, its `look` and action palettes, which are
  small closed sets) is bounded by what one state holds; the world-thread
  queues and metric windows are fixed size. Nothing unbounded was found
  besides the two fixed above (legacy snapshots, daily history).

**Confirmation run** (jemalloc, bounded history and legacy snapshots, the
cheaper high-population tick of [A server-sized world at 60–70k
organisms](#a-server-sized-world-at-6070k-organisms)): the same setup as
the 3-hour soak (the `--worlds 6 --players 3000 --ramp 240 --report 30
--warm-until 22000 --csv` command of [Load and soak
test](#load-and-soak-test); 6 512² worlds × 500 players, 1 s ticks, 1 s
states, protocol 2, warmed to 22.1–24.7k organisms in 128 s) for **90
minutes** after the ramp (`--duration 5400`). This time the 4-core box was
**quiet** for all of it: sampled every minute, no process besides the
server and the load generator used more than 1% of a core (load average
0.9–9.6, median 5.3, all of it the server's and the load generator's
threads). The population followed the same curve: 44–48k per world 15
minutes after the ramp, a peak of 67–73k per world (429k in all) at 0.67 h,
a winter low of 282k in all at 1.1 h, 52–63k per world at the end.
Medians of the 30 s rows; ranges are over the 6 worlds:

| | first 15 min after the ramp | 15 min around the population peak | last 15 min |
| --- | --- | --- | --- |
| Organisms per world | 43.9–48.0k | 66.3–71.7k | 51.9–63.3k |
| Ticks per second (target 1) | 1.0 | 1.0 | 1.0 |
| Tick p50 / p99 (ms) | 60–64 / 128–148 | 169–257 / 570–733 | 84–167 / 152–295 |
| State push p99 (ms) | 35–44 | 121–164 | 42–68 |
| Command → result p50 / p99 (ms) | 131–132 / 250–252 | 139–148 / 386–500 | 128–133 / 250–260 |
| Save: capture / encode / write (ms) | 46–52 / 352–436 / 44–60 | 90–158 / 912–1,101 / 70–83 | 66–99 / 644–780 / 52–66 |
| Save size (MB) | 12.9–13.7 | 20.6–22.2 | 18.3–20.6 |
| Server CPU (cores) / load generator | 1.89 / 0.71 | 2.73 / 0.63 | 2.08 / 0.39 |
| RSS median / max (MB) | 1,716 / 1,975 | 2,461 / 2,615 | 2,394 / 2,498 |
| Heap: live (allocated) / held by jemalloc besides (MB) | 1,716 / 223 | 2,375 / 315 | 2,214 / 387 |

Against the 3-hour soak at the same population (per-report medians over
the worlds, binned by organisms per world; "first hour" is the first hour
after the ramp here, of the run there):

| Organisms per world | 45–50k, first hour | 45–50k, later | 55–60k, first hour | 55–60k, later | 60–70k | 70–73k |
| --- | --- | --- | --- | --- | --- | --- |
| Tick p50 / p99 (ms), 3-hour soak | 70 / 199 | 163–164 / 350–359 | 92 / 311 | 226–285 / 402–530 | 270–470 / 800–1,200 (60–72k) | |
| Tick p50 / p99 (ms), now | 68 / 145 | 102 / 189 | 102 / 283 | 123 / 207 | 102–106 / 273–301 | 242 / 679 |
| Command p99 (ms), now (soak at 60–72k: 700–1,000) | 250 | 250 | 256 | 254 | 254–291 | 462 |
| Server CPU (cores), now (soak at 60–72k: 2.8–2.9) | 1.92 | 1.80 | 2.28 | 2.08 | 2.30–2.52 | 2.77 |

Over the whole run: all 3,000 players stayed connected (0 unexpected
closes, 0 dropped connections, 0 decode errors, 0 resyncs), 6.68 million
commands (6.19 million accepted, 0.49 million refused by the rules; all but
the 91 in flight at the stop answered), 707 saves, no panic, a clean
shutdown, no error in the log; the only warnings were 9 "store is slow"
(at 0.3 h and at the peak, 0.7 h). 182k state frames were coalesced away
(about 0.5% of those due, against about 5% in the 3-hour soak), the first
at 0.2 h (1.4k in the first 15 minutes), most at the peak.

- **The cheaper tick shows at 60–70k.** Where the 3-hour soak saturated
  the box (60–72k per world: 2.8–2.9 cores, tick p99 0.8–1.2 s, command
  p99 0.7–1 s), six worlds now used **2.3–2.5 cores (0.38–0.42 per
  world)**, tick p50 / p99 were 102–106 / 273–301 ms and command p99 stayed
  at its floor (254–291 ms), with the load generator's 0.6 cores on the
  same box. That confirms the estimate from the sim benchmark (~0.4 cores
  per world, ~2.4 for six). Later in the run the same population costs far
  less extra than before (55–60k: tick p99 207 ms after the first hour
  against 402–530). At 45–50k, early, CPU is about 5% lower (1.89 against
  2.0 cores) and tick p99 a quarter lower.
- **Six worlds still saturate 4 cores at the peak, now at 70–73k.** For
  the 12 minutes the worlds were above 70k, the process used 2.8 cores
  (2.6–3.0; plus 0.6 for the load generator), tick p99 rose to 0.6–0.7 s, command
  p99 to 0.4–0.5 s and state frames were skipped; ticks held at 1 per
  second (the slowest 30 s report 0.96) and everything recovered as the
  population fell, without a restart. The ceiling moved from about 60k to
  about 70k organisms per world.
- **Memory stays flat.** RSS was 1.37 GB with all players on, 2.47 GB at
  the population peak (2.62 GB max), and **−77 MB/h** after the peak
  (fitted; live heap −170 MB/h) against the 3-hour soak's +250–370 MB/h
  creep; over the last hour it stayed within 2.34–2.45 GB while the
  population fell to 282k and rose again to 348k. At the same point of the
  run (1–1.5 h, ~310k organisms in all) RSS was 2.32–2.55 GB against
  4.09–4.45 GB, 44% less, and 15 minutes after the ramp 1.7 GB against
  2.55. jemalloc held 0.2–0.46 GB beyond the live heap. The live heap is
  about 10% higher on the way down than at the same population on the way
  up (2.22 GB at 315k organisms at 0.9 h, 2.05 GB at 328k at 0.27 h; the
  365-day in-memory daily-record window is still filling at this age), but
  it falls with the population and did not grow over the last hour. At
  60–70k organisms a world takes about **0.4 GB** (2.45 GB for six with
  the process), the same as at 30–40k: the 0.5 GB budget holds.

#### Login bursts with bounded hashing

The fast ramp again, with `--hash-concurrency` at its default (2 on this
4-core box, queue 32): 2 worlds × 500, the 1,000 players registering and
logging in over 40 s (25 a second, 50 hashes a second) and over 10 s (100
a second), 100 s after the ramp, before (the previous commit's server) and
after, from the same 512² warm-up (22k organisms per world, snapshot
version 9). Same shared 4-core box, loaded by other jobs; the loadgen now
retries `busy` and `rate_limited` as the client does. "Slowest 10 s" is the
fewest ticks a world made in one 10 s report over the whole run, ramp
included; tick p99 is the median of the reports after the ramp.

| | before, 25/s (run 1) | before, 25/s (run 2) | after, 25/s (run 1) | after, 25/s (run 2) | before, 100/s | after, 100/s |
| --- | --- | --- | --- | --- | --- | --- |
| Load average (1 min): min / median / max | 6.0 / 7.3 / 9.3 | 6.3 / 9.8 / 10.4 | 6.0 / 8.3 / 8.8 | 6.2 / 7.0 / 8.2 | 9.0 / 17 / 34 | 7.8 / 10.7 / 12.4 |
| Slowest 10 s, ticks per second (target 1) | 0.6 | **0.1** (main ticked 0.2–0.5 for 30 s) | 0.9 | 0.9 | — | 0.8 |
| Worst tick p99 over the run (ms) | 335 | 479 | 326 | 297 | — | 380 |
| Tick p99 after the ramp, `main`, `w2` (ms) | 298, 252 | 384, 373 | 269, 277 | 229, 223 | — | 346, 276 |
| Command p99 (ms) | 932 | 2,692 | 652 | 499 | — | 810 |
| RSS, max (MB) | 1,046 | 1,289 | 829 | 882 | **killed** at 12.8 GB | 949 |
| Busy replies retried / hashes / peak at once | — | — | 843 / 2,000 / 2 | 42 / 2,000 / 2 | — | 4,582 / 2,000 / 2 |
| Every player in (s after the start) | 50 | 50 | 60 | 50 | none | 70 |

- **Before**, a 25-a-second burst slowed both worlds in the run at load
  ~10 (`main` made 2–5 ticks per 10 s for 30 s, `w2` once only 1; command
  p99 2.7 s);
  at load ~7 it cost 40% of the ticks for 10 s. At 100 a second the
  unbounded hashes took the process to 12.8 GB and the kernel killed it
  (out of memory) within seconds: no player got in.
- **After**, at most 2 hashes ever ran (`peak_in_flight`), the worlds kept
  0.8–1 tick a second through every burst, and RSS stayed under 1 GB. The
  cost is latency for the players logging in: at 25 a second, demand
  (50 hashes a second at ~40 ms each) is just over what 2 slots do, so some
  were told `busy` and got in up to 10 s later; at 100 a second all 1,000
  were in 70 s after the start, 60 s after the ramp ended, with 4.6
  `busy` retries per player and no failures.
- A server with more cores to spare for logins can raise
  `--hash-concurrency`; each slot costs one core and 19 MiB while a burst
  lasts. Watch `hashing.rejected` and `hashing.waiting` in `/api/metrics`.

## Level of detail

Opt-in (`WorldConfig::lod.enabled`, default off: with it off a run is
bit-identical to before). Regions are the 16-tile environment chunks. A
region is **active** (full agents) while any part of it is within
`active_radius` (40) tiles of a focus point (`World::set_focus`, `--focus
X,Y`) or a possessed organism, and becomes **abstract** beyond
`active_radius + hysteresis` (16 more).

- **Collapse** (daily): agents and clutches in abstract regions fold into
  one cohort per (region, lineage): counts per 5-day birth bucket and sex, a
  genome pool (every member's exact genome up to `pool_size` = 64, a uniform
  sample above), mean hunger, health, fat, offspring raised. Agents that
  wander into an abstract region are absorbed the same way.
- **Abstract day**: cohorts eat their region's vegetation (removed from its
  tiles, so regrowth and seasons drive them), hunt prey cohorts in their own
  and the 8 surrounding regions (type II response; prey by mass, pack
  hunting, toxins, stealth, burrows, dens, guarded young; hunters survive on
  plants between kills but breed on meat), burn fat and go dormant, starve,
  go thirsty without water in reach, die of old age, attrition and larval
  competition as agents do, give birth by litter, gestation, recovery,
  season, condition and density (genomes recombined from the pool, mutations
  tagged with the owner), find mates in neighboring regions, and migrate to
  neighbors (entering an active region as agents). Births and deaths go into
  the normal counters.
- **Expand** (within the hour a focus arrives): members become agents with
  ages drawn within their bucket, on tiles their body can use, with pool
  genomes (exact while the pool holds everyone, else recombinants of two pool
  genomes, so alleles and origin tags are conserved in expectation), and
  dependent young next to a mother.
- Legacy snapshots and population caps count cohort members; reports show
  agents and cohorts. Group traits and disease state are agent-only.
- The server enables it with `evo-server --lod` and keeps each connected
  player's focus on their view windows (see [Server](#server)).

Measured with `cargo run --release --example lod -- 256 90 1 64,64 192,192`
(one thread, standard world, two focus points, default radius):

| | ms per tick | organisms at day 90 | agents at day 90 |
| --- | --- | --- | --- |
| Full simulation | 18.4 | 8,983 | 8,983 |
| LOD | 8.4 (2.2× faster) | 12,466 | 4,417 |

Per-lineage populations stay close for the first two months (mean
|log ratio| over lineages and 10-day checkpoints 0.19; at day 90 0.31):
voles 3,619 vs 4,367, foxes 1,135 vs 1,034, pack hunters 105 vs 83, frogs 461
vs 511; newts (2,486 vs 4,897), antelope and burrowers run high in cohorts.
Legacy shares agree to about a point (wild 85.3% vs 88.6%, players 1–4
1.5/2.0/4.5/6.8% vs 0.8/0.9/3.3/6.4%). With the *whole* 128×128 world
abstract (the hardest case; seeds 1–2, 120 days) the error is 0.42 and the
cost 0.3 ms per tick against about 5 ms (16×). The cohort update itself is cheap: a
256×256 world with no agents at all costs 1.5 ms per tick, almost all of it
the environment chunks, so the speed-up is the share of organisms that stay
abstract (`bench --size 256 --scale 1`: 4.5 ms per tick full, 2.8 with two
focus points, 2.1 with one focus of radius 24). The rates in `LodConfig`
were calibrated by hand against full runs on 128×128 seeds 1–2.

## Balance

Ecosystem rates live in `Balance` (`crates/sim/src/config.rs`) and can be
overridden from the command line with `--set NAME=VALUE`. `evo-sim sweep`
tunes them by random search: each round perturbs every parameter of the
current best, runs every trial on several seeds in parallel, and recenters on
the winner with half the spread.

**Judging scores statistically.** Single worlds are chaotic, so a score is
only meaningful with its spread over seeds:

- Every sweep trial's score is printed with the half-width of its 95%
  confidence interval (Student t over its seeds), and every finalist with its
  **paired difference to the round's center** (`Δ center`, ± its 95% CI,
  `*` where the CI excludes 0). All trials of a round run on the same seeds,
  so the paired difference cancels most of the map-to-map variation, which
  dominates a trial's own CI. When a round's winner is not significantly
  better than its center the sweep says so. The best trial gets a per-lineage
  table: survival with its Wilson 95% interval and mean alive ± 95% CI. The
  CSV keeps its original columns and appends `days`, `seeds`, `score se`,
  `score ci95`, `delta center`, `delta ci95` and per lineage `wilson lo`,
  `wilson hi` and `se alive`.
- **Screens (successive halving).** `--screen-days 40` first runs every
  trial for 40 days on `--screen-seeds` seeds (default `--seeds`), keeps the
  best `ceil(trials / eta)` (`--eta`, default 3) plus the round center (the
  reference for the paired differences), and continues only those worlds to
  the full `--days`, on `--seeds` seeds (worlds on screen seeds are resumed,
  not rerun, and resuming gives exactly the straight run's world). A comma
  list (`--screen-days 20,50`) gives several screens. The score is
  normalised by run length, so screens rank trials on the same scale; the
  sweep prints each screen's cut and the world-days it saved. Without
  `--screen-days` a sweep runs as before: `sweep --size 96 --trials 1 --seeds
  12 --rounds 1 --seed 31` still prints 11.61. With the budget the screens
  free, give the finalists more seeds: `--seeds 12 --screen-seeds 4`.
- **`evo-sim compare`** answers "is this change better?": `compare --set
  larval_competition=18 vs defaults` (or `--set a=1 vs --set a=2`; B
  defaults to the default balance) runs both on the same seeds (default 12
  from `--seed`, 96×96, 120 days), prints per-seed scores and the lineages
  each lost, the paired difference A − B with its 95% CI, t and win/loss
  count, what the CI would be unpaired, per-lineage paired differences in
  survival, mean alive and legacy (the owner's share of all alleles in
  breeding adults at the end, in percentage points; the four wild lineages
  share one owner, so their rows show the wild total), and a verdict,
  including roughly how many seeds a difference of the observed size would
  need to resolve. `--migrant-gene GENE=V` (repeatable, gene names with
  underscores for spaces; before `vs` for A, after it for B; implies
  `--migrants`) founds the migrating caribou with that gene changed, so a
  design's gene can be measured paired on the same seeds and rules:
  `compare --migrants vs --migrant-gene migration=0` is what migrating buys
  the herd (see *Migration and winter ranges* in [Balance](#balance)). Adopt a change only
  when the paired CI excludes 0 on seeds no tuning run saw (31 onward).
- **Adaptive `compare`** (`--adaptive`, or `--max-seeds N` which implies
  it) adds seeds until the answer is clear instead of guessing a seed count:
  it runs `--seeds` seeds (default 12), then batches of `--batch` (default
  `--seeds`), both sides of a batch in parallel across cores like the plain
  compare, and stops at the first look where the paired difference's
  interval excludes 0 (a winner), lies entirely inside ±`--tolerance`
  (default 0.3 points, "no difference that matters"; 0 turns it off), or the
  cap is reached (default 48 seeds, so looks at 12, 24, 36, 48). Seeds are
  the same contiguous run from `--seed` as a plain compare, so its first 12
  seeds are exactly `compare --seeds 12`. Decisions:
  - **Sequential level, not a naive 95% CI at every look.** Stopping the
    first time a plain 95% interval excludes 0 gives a false winner far more
    often than 5%: on simulated null differences with looks at 12/24/36/48
    it did so 12.8% of the time. Each look therefore uses Pocock's
    group-sequential level for the planned number of looks (2.94% for 2,
    1.82% for 4, 1.06% for 10; 5% overall), with Student's t (`t_crit`, an
    exact inverse via the incomplete beta) instead of the normal. A unit test
    measures the overall rate on 4000 simulated null comparisons: 5.6%
    (Pocock is exact for normal statistics at equal spacing; the t
    adaptation is close). The printed per-seed table and "Paired difference"
    line keep the one-shot 95% CI for reference; the `Adaptive:` line gives
    the sequential interval and the verdict uses it. Pocock rather than
    O'Brien-Fleming because the point is to stop early on clear differences;
    the price is a slightly wider interval at the last look.
  - **Equivalence stop.** "Within tolerance" means the whole sequential
    interval fits in ±tolerance (an interval test of equivalence), not a
    narrow interval around a non-zero mean. Seed-to-seed SD of a paired
    difference is about 0.8–0.9 points, so ±0.3 is reachable around 40–48
    seeds when the true difference is near 0; ±0.5 at about 24. A larger
    `--max-seeds` adds looks and so lowers the per-look level.
  - Each look prints the running difference and its naive CI with elapsed
    time; the verdict names which stop fired and after how many seeds.

  Example (`compare --seed 31 --adaptive --set larval_competition=18 vs
  defaults`, current rules, 4 cores shared with other jobs): at 12 seeds
  A − B = −0.48 (naive ±0.54, unresolved); at 24 seeds −0.82, sequential 95%
  CI −1.26 … −0.37 (naive ±0.36), B 21 seeds to 2, so it stops with "B is
  better" after 48 of the up to 96 worlds (518 s). The same check on a
  short 48×48, 15-day toy (`--seeds 4 --max-seeds 12 --set hunger_rate=90`)
  shows the guard working: the naive 12-seed CI (−0.66 … −0.05) excludes 0
  but the sequential one (−0.72 … +0.01) does not, so it reports unresolved
  at the cap.

**Validation of `compare`** (`compare --seed 31 --seeds 24 --set
larval_competition=18 vs defaults`, the value the pace-of-life term replaced
with 13; 48 worlds, 6 minutes on a busy 4-core machine): A 10.69 ± 0.51,
B (defaults) 11.29 ± 0.33, paired difference **−0.60 ± 0.35** (95% CI −0.95
… −0.25, t = −3.6), B better on 19 seeds, A on 4, one tie. Unpaired, the
same worlds would give ±0.59, which would barely resolve it; pairing cuts the
interval by 40%. Per lineage the cause is significant: at 18 toxic frogs hold
+162 ± 37 more members and newts +84 ± 46 (the fast breeders swell, as the
original change predicted), while no lineage's survival changes
significantly (antelope +12 ± 19 points). So 13 stays, now on statistical
grounds, not two single 12-seed sums. Seeds 31–42 of B reproduce the
standard check's 11.61.

A screened sweep (`sweep --trials 12 --seeds 6 --rounds 1 --spread 0.25
--screen-days 40 --screen-seeds 3 --eta 4`) simulated 3840 world-days
instead of 8640 (44%). At 40 days nearly every trial still scores 11.9–12
(best 12.00, cut at 11.99), so short screens mostly discard trials that
collapse early and rank the top poorly; its winner beat the center by
+0.20 ± 1.07, not significant on 6 seeds. Use screens to cut the obvious
losers cheaply (a high `--eta`, or 60 days for a sharper rank), give the
finalists many seeds, and confirm a winner with `compare` on held-out seeds.

**Do screen ranks predict full-run ranks? (`evo-sim rankcheck`).** The
command builds one sweep round's trials (`perturb`, trial 0 the center),
runs every trial on the same seeds once to the full `--days`, scoring each
world at the checkpoints on the way (`--screen-days`, default 20,40,60,80;
resumed, so the checkpoints cost nothing extra and equal straight runs of
that length), and reports per checkpoint the Spearman correlation of trial
means with the full-run means (ρ trials) and of single worlds (ρ worlds),
how many of the full-run top `ceil(n/eta)` a screen there would keep, and
whether it keeps the full-run best. It also gives the split-half
reliability of the full-run ranking itself (even vs odd seeds, stepped up
with Spearman-Brown), the best ρ any screen could reach on that many
seeds. `--out` writes every world's score at every checkpoint. Two runs,
24 trials × 4 seeds (1–4), 96×96, 120 days, 23 minutes in all on a shared
4-core machine:

| | spread 0.5 (round 1) | | | spread 0.25 (round 2) | | |
| --- | --- | --- | --- | --- | --- | --- |
| checkpoint | ρ trials | ρ worlds | top 8 kept (eta 3) | ρ trials | ρ worlds | top 8 kept |
| 20 days | 0.53 | 0.35 | 6/8 | 0.16 | 0.03 | 2/8 |
| **40 days** | **0.65** | 0.53 | 5/8 (best lost) | **0.31** | 0.27 | 3/8 |
| 60 days | 0.78 | 0.71 | 6/8 | 0.54 | 0.50 | 4/8 |
| 80 days | 0.83 | 0.74 | 5/8 | 0.68 | 0.76 | 6/8 |
| 120-day split-half reliability (4 seeds) | 0.89 | | | 0.55 | | |
| 120-day trial means | 7.67–11.98 (SD 1.16) | | | 10.38–11.96 (SD 0.46) | | |

With 24 trials a ρ is uncertain by roughly ±0.2–0.3 (Fisher 95% interval for
0.65: 0.33 … 0.83; for 0.31: −0.11 … 0.64), so read the pattern, not the
second digit. What it says:

- **40-day ranks predict 120-day ranks moderately in a wide first round and
  barely in a narrow later one** (0.65 and 0.31). Part of the second number
  is the reference itself: with 4 seeds the 120-day ranking of near-equal
  trials (spread of means 0.46 against a seed-to-seed SD of 0.70) only
  agrees with itself at 0.55, so no screen could do much better there.
- **The screen is a loser filter, not a ranker.** At 40 days 6 (round 1)
  and 9 (round 2) of the 24 trials still score a perfect 12.00, so the top
  of a 40-day ranking is a tie (broken by trial index) and a 40-day screen
  cannot tell good from best. What it does reliably is cut collapses: with
  eta 3 it removed 7 and 6 of the 8 worst 120-day trials, and the kept set
  contained the 120-day best in round 2 and missed it by 0.31 points (less
  than the 0.48 SE of a 4-seed mean) in round 1.
- **60 days is the better screen for the same budget.** A 40-day screen
  with eta 3 runs 56% of a full sweep's world-days; a 60-day screen with
  eta 3, or a 40-day screen with eta 2, runs 67%. The 60-day screen ranked
  better (ρ 0.78 / 0.54), cut 8/8 and 7/8 of the worst third and kept the
  best in both runs; the gentle 40-day eta 2 screen also kept the best
  (it keeps half) but on a weaker rank. 20-day screens are close to useless
  in later rounds (ρ 0.16).
- So for the sweep's screening stage: in round 1 (spread 0.5) use
  `--screen-days 40 --eta 3` or better `--screen-days 60`; in narrower
  rounds use `--screen-days 60` (or 80) with a mild eta (2–3), and spend the
  saved budget on finalist seeds (`--seeds 12 --screen-seeds 4`), since the
  bottleneck there is noise in the full-run ranking itself. A winner still
  needs `compare` (now `compare --adaptive`) on held-out seeds. The sweep's
  defaults are unchanged (no screens unless asked).

**Noise or screen error? (`rankcheck` with 12 seeds).** With 4 or more
seeds `rankcheck` now also splits the seeds into 200 random halves (95%
intervals bootstrap the trials) and simulates whole screened sweeps on the
measured trials. Per checkpoint it reports the screen's own split-half ρ,
the *cross* ρ (screen on one half against the full run on the other half, so
no seed noise is shared) and the **true-score ρ** = cross ρ / √(screen
split-half ρ × full-run split-half ρ): the agreement left once both rankings
are freed of seed noise (≈ 1 means only noise separates screen and full
run; well below 1 means the short run measures something else, which more
seeds cannot fix). It also gives the full-run reliability with its CI and the
seeds needed for 0.8 and 0.9, and, from the simulated sweeps (worlds
resampled per trial around shrunken trial means, screen worlds resumed as in
`sweep`), the chance the true best survives a top-`ceil(n/eta)` cut, the
chance the winner is the true best, and the points lost (regret) against an
unscreened sweep. `--from PATH` re-analyzes a CSV written by `--out`
without simulating. Two narrow rounds (spread 0.25, 24 trials × 12 seeds,
96×96, 120 days; `--seed 1` uses the same trials as the 4-seed round-2 run
above, `--seed 101` an independent set), about 2.5 CPU-hours in all:

| | seed 1 (seeds 1–12) | seed 101 (seeds 101–112) |
| --- | --- | --- |
| 120-day trial means | 10.65–11.98 (SD 0.37; seed SD 0.70) | 10.87–11.84 (SD 0.27; seed SD 0.63) |
| 120-day split-half ρ (6 vs 6 seeds) | 0.73 (0.50…0.86) | 0.53 (0.14…0.73) |
| reliability of the 12-seed / 4-seed ranking | **0.85** (0.66…0.93) / 0.65 | **0.69** (0.25…0.84) / 0.42 |
| seeds for reliability 0.8 / 0.9 | 9 / 20 | 22 / 49 |
| ρ trials (same seeds) 20 / 40 / 60 / 80 / 100 days | −0.03 / 0.22 / 0.88 / 0.94 / 0.96 | 0.44 / 0.64 / 0.73 / 0.77 / 0.83 |
| true-score ρ at 20 days | −0.02 (−0.86…0.56) | 0.67 (0.19…1.02) |
| true-score ρ at 40 days | **0.29** (−0.33…0.75) | **0.79** (0.39…1.42) |
| true-score ρ at 60 days | 1.18 (1.04…2.28) | 1.01 (0.78…2.74) |
| true-score ρ at 80 / 100 days | 1.00 (0.92…1.09) / 0.98 (0.91…1.01) | 0.93 (0.77…1.16) / 0.94 (0.76…1.04) |

(At 20–60 days most worlds still score 12.00, so the screen's split-half ρ
rests on a few early dips and a true-score ρ above 1 means "about 1".)
Simulated sweeps, finalists on 12 seeds, eta 3 unless noted; cost in
world-days against an unscreened 12-seed sweep; "best kept" = the true best
survives the cut, "pick" = the winner is the true best, regret in score
points:

| design | cost | seed 1: best kept · pick · regret | seed 101: best kept · pick · regret |
| --- | --- | --- | --- |
| no screen, 12 seeds | 100% | – · 90% · 0.01 | – · 28% · 0.13 |
| no screen, 6 seeds | 50% | – · 67% · 0.13 | – · 11% · 0.23 |
| no screen, 4 seeds | 33% | – · 43% · 0.29 | – · 6% · 0.30 |
| 40 days × 4 seeds | 41% | 2% · 2% · 0.14 | 14% · 9% · 0.19 |
| **60 days × 4 seeds** | **44%** | **100% · 90% · 0.01** | **62% · 35% · 0.14** |
| 60 days × 4 seeds, eta 4 | 39% | 93% · – · – | 40% · – · – |
| 80 days × 4 seeds | 48% | 96% · 88% · 0.02 | 43% · 22% · 0.14 |
| 100 days × 4 seeds | 52% | 100% · 90% · 0.01 | 59% · 25% · 0.13 |

What it says:

- **Full-run noise is real but not the whole story.** With 12 seeds the
  120-day ranking of a narrow round reproduces itself at reliability 0.85
  and 0.69; with 4 seeds only 0.65 and 0.42 (so the old 4-seed numbers
  above understated every screen). Reliability 0.8 takes 9–22 seeds when the
  trials' true spread (SD 0.16–0.27 after shrinkage) is below the 0.2 SE of a
  12-seed mean.
- **40-day (and 20-day) screens add screen error of their own, and how much
  depends on the round.** Their true-score ρ is 0.29 in one round and 0.79
  in the other, both with wide intervals: what scores well at 40 days is at
  best loosely what scores well at 120, and no seed count fixes that. In the
  simulated sweeps a 40-day screen loses the true best most of the time and
  costs 0.14–0.19 points of regret, against 0.01–0.13 unscreened.
- **From 60 days on a screen ranks like the full run** (true-score ρ ≈ 1 in
  both rounds; 0.93–1.00 at 80–100 days): what remains is seed noise, which
  the screen's 4 seeds and the finalists' 12 seeds handle. A 60-day × 4-seed
  screen with eta 3 costs 44% of an unscreened 12-seed sweep and loses
  nothing measurable (regret 0.01 vs 0.01, 0.14 vs 0.13); spending the same
  budget unscreened (6 seeds, 50%) costs 0.12 and 0.10 points more. eta 4
  saves only 5 points of budget and keeps the best 93% / 40% of the time.
- **A very narrow round is past what a sweep resolves.** In the seed-101
  round even the unscreened 12-seed sweep picks the true best only 28% of
  the time (regret 0.13), whatever the screen.

**Guidance.** Never screen shorter than 60 days in a narrow round (spread
≤ 0.25); in a wide first round 40 days still works as a loser filter (ρ 0.65
above), but 60 is as cheap at eta 3 and safer. The default recipe for a
screened round is `--seeds 12 --screen-days 60 --screen-seeds 4 --eta 3`
(44% of the world-days, same winner quality as unscreened); use 80 days if a
round's early scores are all 12.00. Give finalists at least 12 seeds (9–22
for a reliable narrow-round ranking). When `rankcheck` (or the sweep's own
CIs) shows a full-run reliability below about 0.7 with 12 seeds, stop
narrowing: the remaining differences are a few hundredths of a point, and
the candidate should go to `compare --adaptive` on held-out seeds. The
sweep's defaults are unchanged (no screens unless asked; `--seeds 3` is for
quick looks), since one recipe does not fit wide and narrow rounds and the
evidence covers two narrow rounds.

A run's score (max 12) gives each of the 8 lineages 1 point for surviving the
year and up to 0.5 for holding its founding size, minus up to 3 for time spent
pinned at a population cap. Per-species births and deaths by cause are in
every `run` report.

Validation uses 12 seeds (31–42) that no tuning run or agent touched, on a
96×96 map for 120 days:

| | Score | Vole | Antelope | Newt | Fox | Pack hunters | Herd grazers | Toxic frogs | Burrowers |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Original rules (first sweep, 3–4 seeds) | 6.5 | 100% | 100% | 100% | 0% | 0% | 100% | 100%, capped | 75% |
| After the first tuning sweep (3 seeds) | 9.3 | 100% | 100% | 100% | 100% | 67% | 100% | 100%, capped | 67% |
| Greedy steering (12 unseen seeds) | 11.3 | 100% | 83% | 100% | 100% | 92% | 92% | 100% | 100% |
| Pathfinding (12 unseen seeds) | 11.1 | 100% | 67% | 100% | 100% | 100% | 92% | 100% | 92% |
| Environment upgrade (12 unseen seeds) | 11.05 | 100% | 75% | 92% | 100% | 92% | 92% | 100% | 100% |
| Seasonal strategies (12 unseen seeds) | 10.94 | 100% | 58% | 92% | 100% | 100% | 92% | 100% | 100% |
| Dominant-design counters, seeds 31–42 (55–66) | 10.73 (11.08) | 100% | 33% (58%) | 92% (92%) | 100% | 100% | 92% (92%) | 100% | 100% |
| Final balance pass, seeds 31–42 (55–66) | 11.38 (11.79) | 100% | 92% (100%) | 92% (100%) | 100% | 100% | 100% (100%) | 100% | 92% (100%) |
| Base before vigilance at water (snapshot v7), seeds 31–42 (55–66) | 11.61 (11.46) | 100% | 92% (92%) | 100% | 100% | 100% | 100% (92%) | 100% | 100% |
| Vigilance at water, stripped-ground foraging (snapshot v8), seeds 31–42 (55–66) | 11.61 (11.37) | 100% | 100% (100%) | 100% | 100% | 92% (100%) | 100% (92%) | 100% | 100% |
| **Current: prey switching by search image, seeds 31–42 (55–66)** | **11.54 (11.41)** | 100% | 100% (100%) | 100% | 100% | 92% (100%) | 100% (92%) | 100% | 100% |

Pathfinding cost 0.2 points, within the noise of a no-op change (`sprint`
2.45 → 2.4501 under greedy steering scores 11.04 with antelope at 67%).
Mean numbers lean toward predators: voles 257 → 188, newts 252 → 179, frogs
417 → 337, herd grazers 40 → 19, foxes 54 → 64, pack hunters 20 → 30.
Deaths by cause on single seeds show no consistent mechanism yet (predation
up on one of three, down on another), so judge it on more seeds before
retuning.

Seasonal strategies, merged onto greedy steering, scored 11.3 there too (antelope
75%, pack hunters 100%). On top of pathfinding and the new environment the
0.1 drop is the appended genes reshuffling randomness, not the strategies: with the
presets' strategies switched off (`dormancy_temp=-100 cache_capacity=0`) the
same seeds score 10.92, antelope (which use no strategy) also at 58%.

Toxic frogs now average about half their cap instead of sitting at it, and no
lineage spends the run pinned at a cap.

Almost all of the gain came from rule bugs that per-species birth and death
statistics exposed, not from tuning numbers:

| Problem | Fix |
| --- | --- |
| Exhausted predators kept chasing and starved at a crawl | Rest before hunting; ambush bonus vs. unaware prey |
| Predators kept eating toxic frogs and dying | One poisoning by bright prey teaches avoidance |
| Hundreds of voles drowned | Newborns and steps never land on tiles the body can't use |
| Wandering targeted unreachable tiles (deep water), stranding predators at shores | Wander only to enterable tiles |
| Voles and newts interbred (and foxes with pack hunters) | Different body plans (endothermy, birth mode, size, diet) are sterile |
| Foxes treated voles as kin and wouldn't hunt them | Kin recognition also requires a compatible body plan |
| Only the first 48 neighbors were seen, in arbitrary order, so crowds hid predators | Nearest-first scan with exact pruning (also ~15% faster) |
| A strike almost always killed: no contest of speed or agility | Alert prey can slip a strike and bolt (`escape`, `escape_window`) |
| Burrowers fled in the open instead of going to ground | Threatened burrowers hide; attacks on sheltered burrowers mostly fail |
| Calves died in their first days; mothers couldn't defend them | Nearby mothers add their defense; threatened calves run to them |
| Predators hunted other predators as freely as prey | Prey that fights back is weighed as injury risk and hurts attackers |
| Fast breeders (frogs, newts, voles) pinned their caps | Larval competition, pace-of-life adult mortality, density-dependent breeding |
| Wild species shared one cap, so voles took the newts' room | Wild cap split per species |
| Founders could start beside predators or with water behind them | Placement keeps a predator buffer and needs safely reachable water |

A final 32-trial sweep over all 28 parameters (seeds 1–6) found nothing that
beat the current defaults on the unseen seeds (11.07 vs 11.29), so the
defaults stayed as they are.

Dominance, hidden recessives and mating-transmitted disease (defaults in
`Balance`) score 11.06 on the same 12 seeds (dominance alone 11.02): within
the seed-to-seed chaos, antelope 67%, pack hunters 100%, herd grazers 83%,
all else 100%. At the defaults disease is endemic but rare (tens of
infections per world, a handful of deaths) and harmful recessives reach about
0.05 per organism in a year; disease and recessive draws use their own random
stream, so they change a world only through their effects.

**Still open:**

- Single worlds are chaotic: changing one parameter in the fourth decimal
  place can flip which marginal lineage survives. Judge changes by survival
  across many seeds, never one.
- Antelope are still the most marginal lineage, though no longer lost on
  the held-out sets (12 of 12 on both after vigilance at water, below). A
  world loses them where pack hunters boom (24 founders to 100–145 by day
  80, fed by grazers and voles) and take the slow-breeding antelope as
  by-catch. Prey switching (below) halves how often a prey lineage is
  wiped out, but the booms themselves are as large as before (hunter
  peak-to-mean about 1.6 either way), and the one breeding rule tried on
  the predator side (embryo resorption) suppressed the hunters too much;
  ratio-dependent litters only scale the hunters down (below). On 48
  seeds (101–148) antelope are now lost once. Hybrids between antelope and herd grazers
  still occur, as do fox × pack hunter hybrids, since those pairs share a
  body plan.
- The prey-side changes below lean the balance toward prey: voles, newts,
  frogs and burrowers hold significantly more members and predators lose a
  seed or two more often (pack hunters 92% vs 100% on seeds 101–124). The
  score difference is not significant, but its sign is negative. Measured
  on 48 seeds (*Predators under the prey-side rules*, below), the rules
  together move about four lineage losses from prey to predators, and the
  pack hunters they lose are founding packs that starve in their first
  weeks. A smaller pack-hunter design (body size 0.22) now saves those
  packs without losing prey (*A pack-hunter design whose budget works*).
- A 12-seed, 120-day validation takes about 35 seconds on 4 cores.

**Shared burrows and sentries.** Shared burrows: carriers are diggers
(claws > 0.5 or burrowing > 0.4) with group tolerance > 0.5; solitary diggers
dilute it. Where it holds, a carrier's effective burrowing rises toward 1
(`shared_burrow`), so modest diggers can go to ground and shelter, attacks
on it fail more often, it hides better, and the burrow buffers more cold and
heat. Dependent young of a carrier mother share her burrow. Sentries: a new
`vigilance` gene (4 points per unit); carriers have vigilance > 0.5 and group
tolerance > 0.6, and other group members dilute it. Each carrier is on duty
for a random `sentry_share` of two-hour shifts and, if fed and with kin
around, stands watch: it stays put, does not forage, eats faster
(`sentry_cost`) and sees further (`sentry_vigilance`). A threat it sees
itself becomes a warning that kin group members hear as a threat of their
own (scaled by the regional benefit; warnings are never relayed), and group
members where sentries hold lose no perception to breeding drive. Sample
designs: burrowing omnivores now have group tolerance 0.6 (shared burrows),
herd grazers vigilance 0.7 (sentries). On two 12-seed sweeps (seeds 31 and
77), switching both rules off with `--set shared_burrow=0 --set
sentry_share=0` scored 22.11 in total against 22.18 with them on (measured
before the seasonal-strategies merge): neutral.
Herd grazers lose a little on watch duty, since adult grazers are rarely
threatened.

**Communal dens, group navigation and shared caches** (`group`, effects in
`world::seasonal`; tunables `communal_den`, `group_navigation`,
`shared_cache`). Each needs its strategy expressed (gene ≥ 0.5) plus group
tolerance > 0.5. Communal dens: winter sleepers (hibernation or brumation)
are eligible; a dormant carrier with dormant carrier den mates within a few
tiles (den cells rebuilt hourly; full effect at 3 mates) has its dormant
hunger and thirst cut by up to `communal_den` (0.5), so its fat lasts
longer; loners and solitary sleepers get nothing. Group navigation: migrants
are eligible; a migrating carrier with 2+ kin around, where the trait holds
at its summer range or where it is, has its journey risk cut by up to
`group_navigation` (0.6, replacing the old ad-hoc group-tolerance halving),
uses up to a fifth less food on the way, hurries after its group when the
group is ahead, and picks its winter pasture from 13 spots instead of 9.
Shared caches: plant-eating group members are eligible, so members that
never cache are free riders that dilute it. Where it holds (benefit > 0.5),
carriers gather into one cache per lineage per 16-tile region, up to
`shared_cache` (6) full private hoards, spoiling at half the rate; any
plant-eating group member of the lineage eats from it in the lean seasons,
free riders included, and anyone hungry on its tile can raid it. As free
riders spread, the frequency drops below threshold and carriers fall back to
private hoards: the rise-and-collapse cycle. The burrowing omnivores (and
toxic frogs, brumators with group tolerance 0.6) are already communal-den
carriers, so no preset changed for that. Voles at group tolerance 0.6 build
shared caches (about 100 food units in 6–7 regions) but scored 10.80 vs 11.03
on the 12-seed sweep (they crowd out newts and herd grazers), so they stay
at 0.4; tolerant vole variants occasionally start small shared caches on
their own. On the 12-seed sweep (seed 31) the defaults score 11.03 vs 10.96
before; with `communal_den=0 shared_cache=0` runs are identical to before.

**Seasonal coat, timed breeding and breeding-season risk** (`world::seasonal`;
tunables `molt_cost`, `coat_insulation`, `breeding_window`,
`timed_conception`, `breeding_recklessness`). Two new seasonal genes, priced
like torpor at 4 points per unit and kept out of the genetic distance with
the other seasonal genes. Seasonal coat: a carrier starts in the coat of the
season it is born or founded in (free), then molts toward the winter coat in
autumn and back to the summer coat in spring over `MOLT_DAYS` (6), each full
molt costing `molt_cost` (12) × expression hunger points; deep sleepers
don't molt. The winter coat adds up to `coat_insulation` (0.3) insulation to
a warm-blooded body, which lowers its cold threshold (`12 - 30 × ins`) and
its heat ceiling (`30 - 8 × ins`) alike, so a hot spell in a winter coat
hurts. On a cold-blooded body it adds no insulation and blocks basking like
fur. Without the gene insulation stays fixed. Timed breeding: breeding drive
builds three times as fast as the spring rate in the first `breeding_window`
(0.4) of spring and drains at 60 a day outside it, when timed breeders
neither court nor mate; a mating in the window has its fertility raised by
`timed_conception` (0.3) × expression (better conception; occasional twins
in single-young species). Everyone else breeds year-round at the seasonal
factor, as before. Breeding-season risk: high breeding drive already cost
perception (`0.3 × drive`, waived for group members where sentries hold);
it now also blunts flight, cutting the flee urge by up to
`breeding_recklessness` (0.15), offset by sentries the same way. A timed
breeder's drive is only high in its window, so the reckless period is short.

Sample designs: the red fox (wild, no budget) molts into a winter coat
(seasonal coat 0.8). Measured on the 12-seed sweep at seed 31 (seed 77 in
brackets), all with the new genes: fox coat 11.12 vs 11.04 without
(10.88 vs 10.73), fox 100% both ways with slightly more alive. Rejected:
herd grazers with timed breeding 0.8 (11.04 vs 10.89 at the time, same
survival, but mean alive at day 120 fell from 76 to 27: one birth wave a
year against several), pack hunters with a coat (10.80 vs 10.89; 11.07 vs
11.04 later, neutral), and migrating antelope (migration 0.8 with their
group tolerance 0.8, so group navigation applies): antelope survival 33% vs
58% (42% vs 67%), mean alive 6 vs 32 (30 vs 36), so they stay year-round.
Breeding recklessness costs the prey: `breeding_recklessness` 0 / 0.1 /
0.15 / 0.3 scored 11.19 / 10.88 / 11.04 / 10.89 at seed 31 and 11.23 /
10.99 / 10.73 / 11.06 at seed 77 (antelope and herd grazers lose most);
`--set breeding_recklessness=0` restores the old flight behavior. Founding
also changed: a home spot with a threat within the founders' scatter plus 8
tiles is now penalized heavily (the soft `home_buffer` slope alone let a
fox start 2.7 tiles from burrowing omnivores on seed 4 once the new genes
reshuffled founder randomness). Overall, the 12-seed sweep at seed 31 scores 11.12 against 11.14 before
this change (seed 77: 10.88 against 10.32), within the reshuffle noise
(with the new genes in place but no rule in use, seed 31 already lost its
pack hunters within 30 days, as the 240-day run on seed 31 shows).
Not covered by the level-of-detail cohorts:
they use mean insulation and the seasonal breeding factor as before.

All the doc's seasonal add-ons are now in.

**Floodplains, wind direction and lakes.** The three environment gaps left
above are closed, all deterministic (no new map randomness; storm gusts draw
from a new `rng::stream::WIND`, one stream per tick and organism):

- **Lakes.** Map generation's priority flood now keeps the level each
  depression fills to. A connected depression at least 0.002 elevation ranks
  deep holds a lake where the rain draining into it (the most any of its
  tiles passes on) beats evaporation: one tile of surface per 4 units of
  inflow, filling from the bottom up, so a well-fed basin fills to its rim
  and a parched one stays dry. Lake tiles are `ShallowWater` (no new tile
  type, so the terrain endpoint and client are unchanged), 0.2–0.9 m deep
  and following regional wetness like other shallow lakes; they pass their
  inflow on, so rivers leave over the rim. They are ordinary water sources
  for `find_water`, `can_drink_at` and `water_dist`. On 96-tile maps:
  63–622 lake tiles (seeds 55, 31, 9: 63, 147, 622).
- **Floodplains.** Dry, gentle land (slope < 0.3) within two steps of a
  river and at most 0.02 ranks above it gets `Tile::plain`, the river level
  at which it floods (1.5× normal flow for the lowest bank, +0.3 a step
  further out, up to +0.6 for the highest), and shares the river's
  catchment. Above it the tile stands under `0.6 m ×` the excess (at most
  0.9 m) of still water: tall bodies wade, swimmers swim, small
  non-swimmers are blocked and drown if caught (the existing stranded and
  drowning rules), and paths and the water field update with
  `terrain_version`. A flooded tile loses 2% of its plants an hour and
  gains `Tile::silt`; after the water goes, silt raises regrowth by up to
  100% and fades (half-life about ten days). About 13% of land is
  floodplain; over the first 120 days a 96-tile map has 50–145 floodplain
  tiles under water on average (up to 450 at a spring peak).
- **Wind direction.** Each weather cell has a direction
  (`WeatherCell::dir`, carried by `Weather::dir` from `Map::weather_at`):
  the prevailing westerly swings map-wide over a few days and veers
  regionally with a pattern drifting like the fronts. `terrain::wind_effect`
  gives flyers and gliders a speed and energy multiplier from the tail- or
  headwind component, felt more by light wings (`1 / (1 + wing loading /
  2)`), with extra cost in strong wind and much more in a storm. Wind
  tolerance follows wing loading: a flyer is grounded above
  `0.6 + 0.15 × wing loading` (at most 0.92), so heavier flyers can fly
  into a storm, where each tick a gust injures them with chance 5% (−10
  health, never fatal on its own, and −10 energy). Gliding is now a
  movement mode (`terrain::glide`): a body that can glide but isn't flying
  gets ×1.3 speed and ×0.5 energy (times the wind effect) on a step from
  height (a canopy it climbs, a slope over 0.4, or ground falling away
  ahead), and can't launch in wind above `0.5 + 0.05 × wing loading` (at
  most 0.75). No sample design flies or glides yet.

Held-out sweeps: 11.61 / 11.46 (seeds 31 / 55) against 11.53 / 11.61 before
(sum 23.07 vs 23.14). With the floodplain and wind but no lakes they scored
11.27 / 11.64, so the shifts are map reshuffle noise. Four tests whose
outcome hung on one map's geography were made robust (summing froglet
deaths over four maps, the colony's mean breeding suppression, another seed
for the species-id test, a possessed organism started five tiles from
water). Snapshots move to format version 6 together with the static
terrain split (`Ground::plain`, `Tile::silt`, `WeatherCell::dir`,
`Weather::dir`).

**Final balance pass: antelope, herd grazers and birth modes.** The new
`examples/deaths.rs` (`cargo run --release --example deaths -- 1,5 120
31 32 ...`) tallies every death of the given lineages by cause, life stage
(calf: growth < 0.7, sub-adult, adult), season, what the victim was doing,
whether its mother was near, the herd-defense benefit where it died and the
killer's lineage (`Organism::killer`, `Counters::kills_by_lineage`);
`calibrate --deaths` now also prints eggs laid, hatched, chilled and raided
per entry (`Counters::eggs`). On seeds 31–42 before the pass, 92% of the
antelope's 992 deaths and 82% of the herd grazers' 2,415 were predation. The
victims were mostly adults: 589 antelope adults (pack hunters 367, foxes
222) and 998 grazer adults. Calves came next (217 antelope, 647 grazers,
nearly half of the grazer calves taken by foxes). Of the adults, 40% of the antelope
(236) and 23% of the grazers (231) were killed while courting: only 119 and
138 died fleeing. Autumn, spring and summer were about equal and winter was
low. Five rule causes:

| Problem | Fix |
| --- | --- |
| Full breeding drive outranked fleeing from anything but the closest hunter, so prey in breeding condition (most adults of year-round breeders) died courting | A threat in sight puts courting on hold: the urge to court is `1 - courtship_caution × threat` (`courtship_caution` 1). `breeding_recklessness` still blunts flight itself |
| Half the foxes carried cooperative hunting (pack instinct at the 0.5 default), so they could take prey three times their size: 222 adult antelope | Red fox preset at pack instinct 0.1 (solitary; the wild fox has no budget) |
| Herd defense was diluted by every plant eater, voles and frogs included, so it almost never held (most grazer kills happened at benefit 0) | Herd-defense eligibility needs body size > 0.45 as well as carnivory < 0.4, matching the doc's "herbivore + large size + group tolerance" |
| Only a dependent calf next to its mother was guarded; sub-adults and orphans (about half of the young antelope that were killed) had nothing | Where herd defense holds and 3+ kin are around, the herd guards its young like a parent would (`herd_guard` 1 × care × benefit): "predators can't single out individuals" |
| An incubating parent could court and conceive on the nest, and once the eggs hatched she could breed again the same day | No courting while tied to a nest (`Organism::can_breed`); incubators recover from the hatch, as live bearers do from the birth (`brood_recovery` 1) |

After the rule fixes, on the same 12 seeds, foxes kill 5 adult antelope
instead of 222. Antelope killed while courting fell from 236 to 85, and
grazer calves taken fell from 647 to 211. Antelope deaths to predation
dropped from 909 to 718, even though the antelope population is several
times larger. Alive at day 120 (sum over the 12 seeds) went from 125 to 824
antelope and from 946 to 3,000 herd grazers. More grazers now starve
(250 → 560): food, not predators, limits them now.

Birth modes. Two 48-world tournaments on fresh seeds (9601+, 9701+; 4 random
full-budget designs per world, a fair share is 25% of entries) had incubated
eggs win 35% of their entries, live birth 27% and abandoned eggs 13%. Among
abandoned-egg designs, warm-blooded ones won 8%, which is the doc's
thermoregulation mismatch working, and cold-blooded ones won 16%. The fixes,
all from the doc's birth-type and offspring tradeoffs:

- **Larval competition by pace of life.** Hatchlings of abandoned eggs died
  of crowding at `(kin / larval_competition)^4` whatever their kind, so a
  two-egg clutch of large young suffered like a frog's ten. The hazard is
  now `(kin × sqrt(pace of life) / larval_competition)^4` (doc: "many:
  fragile, most die young"), with `larval_competition` 18 → 13 so that the
  toxic frogs (pace 0.52) see about the same hazard as before. At 18 with
  the pace term, frogs and newts swell (sweep seed 31: 11.05, frogs 617
  alive; seed 55: 11.23, frogs 691) against 11.40 and 11.74 at 13.
  `larval_stage` (1) can end the larval stage before maturity.
- **No courting on the nest, and brood recovery** (table above). With
  recovery running from laying, a brooder cycled almost twice as fast as a
  live bearer: about 2 + 5 days against 8 + 6 for a mid-sized litter of 6.
- **Cold-blooded gestation.** Embryos develop at body temperature: a
  cold-blooded live bearer now carries her young as much longer (×1.4) as
  cold-blooded eggs take to incubate. (Later set back to the warm rate,
  `cold_gestation` 1.0: see *Thermoregulation × birth mode*.)

On the same 96 worlds (pooled; the brood-recovery rule was designed after
seeing them) the shares moved from abandoned/incubated/live 13% / 35% / 27%
to 21% / 29% / 25%. Cold-blooded abandoned eggs went from 16% to 28%.
Warm-blooded abandoned eggs stay at about 10%, as the doc mismatch intends.
On 48 more worlds that no rule or tuning had seen (seeds 9801+):
abandoned / incubated / live
9.5% / 35.8% / 29.0% before and 15.9% / 31.3% / 27.4% after. That is the same
direction with a smaller step, and incubated eggs are still the strongest mode.

Held-out sweeps (`sweep --size 96 --trials 1 --seeds 12 --rounds 1 --days
120`, seeds 31–42 and 55–66) score 11.38 and 11.79 (before: 10.73 and
11.08). Antelope are at 92% / 100% (33% / 58%) with 69 / 72 mean alive (10
/ 17), herd grazers at 100% / 100% (92% / 92%), and every other lineage is
at 92–100%. Ablations on the two sets (before brood recovery, which by
itself moved them from 23.14 to 23.17 in total): `herd_guard=0` 23.29 and
`courtship_caution=0` 23.18 against 23.14. Both are neutral within the seed
chaos and stay on as doc mechanics, because they cut the deaths they target.

Parameter tuning came after the rule fixes: a 2-round, 16-trial sweep on
seeds 1–6 (spread 0.25). Its best set scored 11.96 there against 11.72 for
the defaults, but on the held-out sets it scored 11.55 / 11.54 against
11.40 / 11.74, and the pack hunters and foxes fell to 83–92%. It was not
adopted. The only changed default is `larval_competition` (18 → 13), which
goes with the new pace-of-life term. New tunables: `courtship_caution`,
`herd_guard`, `brood_recovery`, `larval_stage`.

**Egg thermal risk** (doc: eggs "can be raided, chilled or overheated";
tunables `egg_overheat`, `brood_warmth`, `brood_cost`). Before this, a
sitting warm-blooded parent held her eggs at 32 °C whatever the weather, for
free, and eggs could only chill. Now:

- **Nest microclimate.** Eggs feel their nest tile's temperature, with water
  (0.5) and canopy (0.3) damping the air's swing toward the tile's annual
  mean. Warm-blooded eggs chill below 24 °C and overheat above 39 °C;
  cold-blooded ones chill below 10 °C and overheat above 34 °C, at
  `egg_overheat` (0.02) per °C-hour, the clutch dying at 1. Overheated
  eggs are counted in `EggCounts::overheated` (`calibrate --deaths`
  prints them).
- **Brooding is limited and costs the parent.** A warm-blooded parent lifts
  her eggs toward 32 °C by at most `brood_warmth` (20 °C), so on a nest
  below 4 °C the eggs cool even under her; a cold-blooded one warms them by
  at most 4 °C. In the heat either can shade them by at most 3 °C. Warming
  costs `brood_cost` (1.0) × her hourly hunger per 10 °C, shading the same
  share of the thirst rate: a brooder in a cold place eats up to twice as
  fast and leaves the nest to feed more often, exposing the eggs.
- **Abandoned eggs rely on the nest site.** A mother leaving her eggs lays
  them on the passable tile within 3 tiles (without another clutch) whose
  microclimate would harm them least over today's day/night cycle, the
  nearest among equals: in summer, cold-blooded eggs go into water or
  shade. Brooders nest where they are.

Measured with `calibrate tournament --deaths` (90 days, 4 random
full-budget designs per world, win = highest final legacy share; fair is
25%). Tuning set, seeds 10001–10096 (96 worlds), abandoned / incubated /
live win share: 15.6% / 31.3% / 29.5% before, 16.3% / 31.3% / 28.6% after.
Mean final share of incubated designs fell from 0.104 to 0.095 (live 0.097
→ 0.100), and it fell monotonically with `brood_cost` (0.25 / 0.6 / 1.2:
0.104 / 0.101 / 0.095). The win share hardly moves because winning egg
layers fill their population cap regardless: winners hatching only 17% of
their eggs still won with 47% of the legacy. Held-out set, seeds
10201–10248 (48 worlds): 14.9% / 29.4% / 31.6% before, 17.9% / 25.0% /
33.3% after; cold-blooded abandoned eggs went from 27.6% to 34.5% (they
gain from nest sites, chilled 12% → 7%, overheated 2%), warm-blooded
abandoned eggs stay at 5%. Pooled over the 144 worlds: 15.4% / 30.7% /
30.2% before and 16.8% / 29.1% / 30.2% after. Egg fates there: warm
incubated eggs hatch 38% (48% before; chilled 55%), cold incubated 76% (81%;
5% overheated). With 60–70 entries per mode a win share has a standard
error of about 5 points, so these are directional.

Held-out sweeps: 11.53 / 11.61 (seeds 31 / 55) against 11.38 / 11.79
before (sum 23.13 vs 23.17), every lineage at 92–100%. With `brood_cost`
0.25 and `brood_warmth` 24 they scored 11.32 / 11.77. The level-of-detail
cohorts don't model eggs separately and are unchanged. Snapshots move to
format version 4 (`Clutch::overheat`, `EggCounts::overheated`).

**Vigilance at water, stripped-ground foraging and a migrating sample
design.** Decisions here use `evo-sim compare` with 24 seeds (101–124
unless noted; none of them used for diagnosis), and the held-out sweeps
(seeds 31–42 and 55–66) as confirmation.

*Antelope: diagnosis.* `examples/deaths` on seeds 31–42 (base): 890 of the
antelope's 1,116 deaths were predation, 582 of them adults taken by pack
hunters. By action at death the adults were fleeing (194), drinking (124),
resting (118), grazing (71) or courting (67); 399 of the 590 adult kills
had fewer than 3 kin around (no herd defense). Instrumenting the strike on
four seeds showed what the drinkers and resters had in common: almost all
of them **had the hunter in sight** (`Needs::safety` about 38–46, a threat
of about 0.4) and kept drinking or resting anyway; they were tired (energy
about 31–36 against 71 for the living adults); half of the resters and
most of the fleers were at water; the drinkers were mostly still walking
to it. Because "caught off guard" was implemented as "not fleeing", an
antelope that saw the pack coming but kept drinking lost 40% of its
resistance and of its chance to slip the strike, as if it had seen
nothing, and the strike killed it with p ≈ 0.9. Doc grounding: "hungry
organisms prioritize food over everything except immediate danger";
Sentries (group species + sentry gene) "stay alert while others breed".

| Change | Rule |
| --- | --- |
| Vigilance at water and at rest (`threat_caution` 1) | A threat in sight scales the urges to drink and to rest by `1 - threat_caution × threat`; the parched still drink (the extra urge above thirst 80 is kept) |
| Aware prey is only partly ambushed (`alert_guard` 0.5) | Prey that had a threat in sight but wasn't fleeing keeps half of the `ambush` penalty instead of all of it (`World::ambush_on`) |
| Antelope stand sentry | The wild antelope preset gets vigilance 0.7 (no budget), like the herd grazers: their herd takes turns on watch, and they no longer dilute the grazers' sentries as eligible non-carriers |

Measured (A − B, paired 95% CI):

| Comparison | Score | Antelope survival | Antelope mean alive |
| --- | --- | --- | --- |
| Package vs base (antelope vigilance and both rules vs none), seeds 101–124 | +0.06 ± 0.24 | +4 ± 9 pts | +12 ± 27 |
| Same, seeds 125–148 | +0.03 ± 0.32 | +12 ± 14 pts | **+35 ± 20** |
| Both rules on vs off (antelope vigilance on both sides), merged code | −0.20 ± 0.26 | +0 | −1 ± 19 |
| All three rules (with `pasture_search`) on vs off | −0.25 ± 0.35 | +4 ± 9 pts | +17 ± 21 |
| `threat_caution` without `alert_guard` (both with `pasture_search`) vs all off | **−0.45 ± 0.32** | −12 ± 19 pts | +7 ± 14 |

So `threat_caution` alone makes prey flee more and get caught tired and
alone (rejected as a stand-alone change; `alert_guard` 0 is significantly
worse), and the package's antelope gain is clear on one 24-seed set and
positive but unresolved on the other. Voles gain significantly in every
comparison (+140 to +200), newts and frogs in most; with all three rules
on, red foxes lose 12–20 members. On the held-out sweeps antelope went from 11 of 12 to
12 of 12 on both sets, mean alive 59 → 81 and 52 → 72.

Rejected (no significant effect on the score or on antelope, 24 seeds
each; not in the code): **herd drinking** (herd members join kin they see
heading for water: +0.12 ± 0.27, antelope −6 ± 16; kin around drinkers
barely changed, 2.4 → 2.6), **pack wariness** (prey reads a pack hunter's
cooperative bonus into the threat: −0.02 ± 0.23, antelope −4 ± 23),
**safety in numbers** (isolated group animals flee sooner: −0.02 ± 0.28,
antelope −1 ± 11), **herd cohesion** (herd members wander around their
herd's centroid: −0.07 ± 0.30, antelope +3 ± 24), **hunters read herd
defense** into their odds (−0.03 ± 0.21, antelope −2 ± 17) and antelope
sentries alone (+0.07 ± 0.19, antelope −0 ± 14). Per-seed runs show why
none moves much: the seeds that lose antelope are the seeds where pack
hunters boom on other prey (seed 123: pack hunters 39 → 145 by day 81,
herd grazers 75 → 4 by day 101, antelope 49 → 0; seed 104: pack hunters starve to 4,
antelope reach 243).

*Herd grazers: food limit or foraging bug?* Both. A probe of every grazer
starvation on seeds 31, 37 and 41 (over 80% in winter) measured the best
reachable vegetation around the body: about half had **two bites or more
within 5 tiles** (usually on a neighboring tile) while walking to a target
5–27 tiles away, and about a fifth had nothing within 15 tiles but food
within 30. Two foraging bugs: `find_tile` samples 8 random spots in the
near box, so scattered remnants next door are usually missed for a patch
further out, and a stripped area wider than 3× the foraging reach left the
herd random-walking in 5-tile hops. Fix (`pasture_search` 6): a forager on
a bare tile first steps to the best of its 8 neighbors with a bite on it,
and if nothing is found within 3× its reach it samples once more at 6×
before wandering. On seeds 101–108 grazer starvation fell from 542 to 377
deaths (14.5% → 11.2% of births). The rest is a real winter carrying
capacity: food within 5 tiles is gone and the remaining deaths are
commuting between bare pasture and water. Compare on vs off: −0.24 ± 0.31
(not significant), herd grazers −0 ± 49, burrowing omnivores +49 ± 18 and
toxic frogs +59 ± 49 (significant), pack hunters lost 2 more seeds. Kept
as a bug fix.

*Migrating caribou* (`presets::migrant_design`, 54.9 of 100 points):
warm-blooded, live birth of 2, body size 0.72, leg length 0.6, hooves 0.6,
fat stores 0.5, ears and vocal 0.6 (alarm calls), fur 0.6, group tolerance
0.9, boldness 0.3, migration 0.8, 36 founders, so every founder is a
group-navigation carrier. At body size 0.55 (antelope size) the herd was
lost on 3 of 8 test seeds to pack hunters on its autumn trek and in its
winter range (autumn predation 300 of 478 deaths on two seeds); at 0.72 it
is out of reach of most packs. A test (`caribou_migrate_south_as_a_group`)
checks that by late autumn the herd is on the move south with most migrants
navigating as a group. Founding it changes the other lineages' worlds, and
an extra lineage would raise the score's maximum from 12 to 13.5, so it is
an **extra lineage**: `run`, `sweep` and `compare` take `--migrants`
(`presets::extended_world`, owner 5, the ninth lineage), report it in the
lineage tables, and score only the standard eight (`RunMetrics::scored`).
Scores with and without `--migrants` are on the same 12-point scale but are
different worlds, so compare like with like. Held-out sweeps with
`--migrants`: 11.62 / 11.40 (without: 11.61 / 11.37), caribou 12 of 12 on
both sets (mean alive 167 and 214) and 24 of 24 on seeds 101–124. Rejected:
steering navigators' winter pasture away from dense hunters
(`migrant_wariness`): caribou −3 ± 28, score −0.19 ± 0.32.

Held-out sweeps, all changes: 11.61 / 11.37 against 11.61 / 11.46 on the
base (sum 22.98 vs 23.07); antelope 12/12 and 12/12 (base 11/12 and
11/12), pack hunters 11/12 on seeds 31–42 (base 12/12). `--set
threat_caution=0 --set alert_guard=0 --set pasture_search=0` restores the
old rules (the antelope preset keeps its vigilance). Snapshots move to
format version 8 (three new `Balance` fields in the saved config). New
tunables: `threat_caution`, `alert_guard`, `pasture_search`.

**Pack-hunter booms: prey switching by search image.** Decisions use
`evo-sim compare` on seeds 101–124 and 149–172 (24 each; neither used for
diagnosis), and the held-out sweeps as confirmation. Diagnosis used the new
`examples/predators.rs` (`cargo run --release --example predators -- 120
1 2 3 ...`), which follows the pack hunters against their prey (head
counts, hunger and prey kg per hunter every 10 days; kills per prey
lineage by how common that prey was; hunger while chasing; births and
deaths by prey kg per hunter; pregnant hunters' hunger), on seeds 1–24
and 201–248. On seeds 1–24 a pack-hunter population peaks at 1.57× its
mean on average (up to 191 hunters from 24 founders), and two seeds lost a
prey lineage. The candidate mechanisms against that data:

| Mechanism | What the data says | Verdict |
| --- | --- | --- |
| Satiation (a full hunter stops hunting) | Already there: adult hunters chase in 4–7% of ticks below hunger 30 and 70–80% above 60 (the hunt is a hunger-driven food choice) | Nothing to add |
| Prey switching away from rare prey | Absent, in fact reversed. Kills per hunter per prey-day are highest for a kind when it is rare: herd grazers below 5% of the prey alive 3.4–9.5 per 10k, against 0.6–1.0 at 10–20%; voles 1.7–3.5 below 5% against 1.2–1.5 at 20–30%. On seed 14 the antelope went from 44 to 0 under a steady 35–70 hunters that kept taking about 10 every 10 days while voles boomed to 970: antelope are worth 6.6 voles (score ∝ mass^0.75), so any hunter that saw one took it | Adopted (below) |
| Cost of litters when prey per hunter is low | Weak gradient. Pregnant hunters spend 52–64% of their time at hunger ≥ 80 whatever the prey per hunter (feast and famine), and starving (100) only 11% → 20% from plenty to scarcity. Births per 100 hunter-days fall only from about 2.1 to 1.2 at the lowest prey per hunter | Tested as embryo resorption; rejected |
| Starvation lag | Starvation is 73% of hunter deaths (thirst 21%); deaths per 100 hunter-days rise from about 1.1 to 2.0–2.2 at low prey per hunter, births fall to 1.2: the bust is slow but present | Not changed |
| Density pressure among hunters | Not measured directly; packs share carcasses by design | Not tried |

**Search image** (`search_image` 0.7, `Organism::search_image`). A hunter
remembers the lineage of its last kill and overlooks prey of any other
kind with chance `search_image`, drawn per hunter, prey and day (so the
same vole is ignored all day, not re-rolled every tick). Hunters mostly kill
whatever is common, so most carry its image, and a rare kind is taken less
than its encounter rate would have it: Tinbergen's search image, the
classic cause of prey switching (a type III functional response), which
fits the doc's "each organism's AI makes decisions from its own weights"
and needs no global knowledge. A starving hunter (hunger ≥ 90,
`SEARCH_IMAGE_HUNGER`) drops its image and takes whatever it finds. Without
that exception (first version below) hunters whose prey kind had crashed
kept overlooking the prey that was left, and small founding packs starved
on several diagnosis seeds (pack hunters lost on 1 and collapsed below 12
on 3 of seeds 201–248, against 0 and 1 without the rule). A test
(`search_image_overlooks_other_prey_unless_starving`) checks the overlook
rate and the starving exception.

Measured (A − B, paired 95% CI; lineages lost summed over the 24 seeds):

| Comparison | Seeds | Score | Prey lineages lost (A / B) | Pack hunters lost (A / B) | Significant mean-alive changes |
| --- | --- | --- | --- | --- | --- |
| Search image 0.7, no starving exception, vs off | 101–124 | +0.15 ± 0.27 | 1 / 5 | 3 / 2 | voles +118, antelope +19, herd grazers +53 |
| Same | 149–172 | −0.15 ± 0.36 | 2 / 3 | 2 / 1 | voles +132 |
| **Search image 0.7 with the starving exception vs off (adopted)** | 101–124 | **+0.07 ± 0.38** | **2 / 5** | **1 / 2** | voles +131, antelope +18, newts +66 |
| Same | 149–172 | **−0.06 ± 0.30** | **2 / 3** | **2 / 1** | voles +93, newts +63 |
| Embryo resorption (`resorption` 0.5: a starving mother loses half her pregnancy's fertility a day) vs off, both with the search image | 101–124 | **−0.37 ± 0.28** | 1 / 2 | 3 / 1 | pack hunters −28, foxes −16, every prey +37 to +202 |
| Same | 149–172 | **−0.61 ± 0.31** | 3 / 2 | 4 / 2 | pack hunters −18, foxes −21, prey +14 to +212 |

So the search image keeps the score (both differences far inside the
noise) and cuts the prey lineages lost from 8 to 4 in 48 worlds (on the
diagnosis seeds 201–248: 1 against 4), with pack hunters lost equally
often (3 and 3 in the compares; 1 and 0 on seeds 201–248). Rare-prey risk falls where
it was highest: kills per 10k prey-days per hunter of antelope below 5% of
the prey 1.32 / 1.33 against 1.52 / 1.52 (seeds 101–124 / 149–172,
`examples/predators`). The booms themselves barely change: on the same 48
worlds hunters peak at 80 against 86 (mean 50 against 53), peak-to-mean
1.63 against 1.65. Resorption is significantly worse on both sets: it
throttles the hunters' and foxes' breeding everywhere (pregnant hunters are
hungry whatever the prey supply), so prey pile up toward their caps; it is
not in the code.

Held-out sweeps: 11.54 / 11.41 (seeds 31 / 55) against 11.61 / 11.37
before (sum 22.95 vs 22.98), the same lineages lost (pack hunters on one
seed of 31–42, herd grazers on one of 55–66). `--set search_image=0`
restores the old hunting. The snapshot round-trip test now re-tags the
father of a pregnancy explicitly (with fewer pregnancies at its snapshot
tick, every fifth organism no longer hit one). The client wire fixture is
unchanged. Snapshots move to format version 9
(`Organism::search_image` and the new `Balance` field). New tunable:
`search_image`.

**Birth modes, re-measured.** The 29.1% win share of incubated eggs
(Suggested next steps) was measured before vigilance at water,
stripped-ground foraging and prey switching. Re-run on the current rules
with `calibrate tournament --deaths` (90 days, 4 random full-budget designs
per world, win = highest final legacy share, fair 25%; `--world0` splits
a tournament across processes without changing its designs):

| Worlds | Abandoned / incubated / live: win share | Mean final share |
| --- | --- | --- |
| Tuning set, seeds 10001–10096 (96) | 23.8% / 24.4% / 27.2% | 0.060 / 0.074 / 0.090 |
| Held-out set, seeds 10201–10248 (48) | 19.4% / 26.3% / 28.8% | 0.064 / 0.075 / 0.085 |
| Both (the 144 worlds behind 16.8% / 29.1% / 30.2%) | 22.4% / 25.0% / 27.8% | 0.061 / 0.074 / 0.088 |
| Fresh seeds 30001–30096 (96) | 23.0% / 31.7% / 20.5% | 0.067 / 0.091 / 0.076 |
| **All 240** | **22.7% / 27.7% / 24.9%** | **0.063 / 0.081 / 0.083** |

With about 310 entries per mode a win share has a standard error of about
2.5 points, so incubated eggs are now within noise of fair (27.7 ± 5),
their mean final share is below live birth's, and on the very worlds that
gave 29.1% they win 25.0%. No rule aimed at them: the later prey-side rules
changed the worlds. Two sets of 96 worlds differ by 7 points (24.4% vs
31.7%), which is the sampling noise of one tournament, so the item is closed
on the pooled numbers, and no birth-mode rule was added. What is left is an
interaction of thermoregulation and birth mode (240 worlds, win share):
cold-blooded abandoned eggs 36.9% (warm 9.1%, the doc's mismatch), cold
incubated 28.2% (warm 27.2%), cold live birth 15.1% (warm 34.4%).
Cold-blooded live bearers carry their young 1.4× longer (cold-blooded
gestation) and lose; cold-blooded egg layers that leave their eggs at the
best nest site win. *Thermoregulation × birth mode* (below) takes it up.

**Thermoregulation × birth mode.** Paired re-runs of the same tournament
designs (`calibrate tournament --deaths`, tuning seeds 10001–10096, 96
worlds, 384 entries) with one rule changed at a time, against the
unchanged rules (win share of the six thermoregulation × birth classes;
paired difference of mean final share, ± one standard error):

| Change (one at a time) | Cold abandoned / incubated / live | Warm abandoned / incubated / live | What moved |
| --- | --- | --- | --- |
| None | 39.4% / 24.5% / 16.7% | 8.3% / 24.3% / 36.7% | |
| Cold live bearers gestate as fast as warm ones | 38.0% / 26.4% / 24.1% | 9.7% / 21.6% / 31.7% | cold live share +0.013 ± 0.004, adults 114 → 134, born 232 → 301 |
| Abandoned-egg layers not exempt from breeding suppression | 31.0% / 37.7% / 18.5% | 9.7% / 21.6% / 35.0% | cold abandoned laid 1028 → 575 per lineage, share −0.003 ± 0.003 |
| Cold-blooded eggs develop at their temperature (none below 10 °C, full speed from 22 °C) | 35.2% / 24.5% / 22.2% | 9.7% / 21.6% / 38.3% | cold incubated share −0.007 ± 0.003 |
| Both of the first two | 29.6% / 35.8% / 25.9% | 9.7% / 18.9% / 35.0% | |

What each gains or loses:

- **Cold-blooded live bearers lose to their gestation.** Carrying 1.4×
  longer (8 · (0.6 + size) · (1 + 0.3 · litter) days, ×1.4) leaves them
  fewer births per adult (232 born per lineage against 360 for warm live
  bearers) and nothing else differs much: their deaths are few and spread
  over causes (36 eaten, 22 thirst, 14 attrition per lineage). Gestating
  at the warm rate alone gives them back 8 points of wins and a
  significant share. It is also what live-bearing ectotherms do: a
  basking mother keeps her embryos near her active temperature, which is
  why lizards and snakes of cold climates bear live young; eggs in a nest
  get no such help.
- **Cold-blooded abandoned eggs gain from breeding fastest.** A mother
  lays 2 days after mating and recovers 3–9 days, against a live bearer's
  7–15 days of gestation plus recovery and a brooder's incubation; cold
  eggs rarely chill (10 °C floor, 14% of eggs lost to cold, heat and
  raids, against 83% of warm-blooded abandoned eggs chilled), and their
  mothers skip density-dependent breeding (larval competition thins their
  young instead). They lay 3× as many eggs as cold incubators (1028 per
  lineage against 336), and half their young starve as larvae (383 of 689
  hatched). Applying breeding suppression to them as well halves their
  laying and cuts their wins by 8 points, but their mean share barely
  moves (−0.003 ± 0.003) and the wins go to cold incubators (24.5% →
  37.7%), so it trades one cold egg mode's edge for the other's; with the
  gestation change the six classes spread wider than with that change
  alone (sum of squared distances from 25%: 510 against 462).
  Temperature-dependent development of cold eggs costs brooders more than
  abandoned eggs (a brooder's 4 °C lift doesn't make up for it). Neither
  was kept.

The rule kept: **`cold_gestation`** (1.0; the old rule was 1.4): a
cold-blooded live bearer carries her young `cold_gestation` times as long
as a warm-blooded one of the same size and litter (`Traits::gestation`);
cold-blooded eggs still take 1.4× as long, and cold-blooded growth and
lifespan are unchanged. Judged on 96 fresh worlds (seeds 50001–50096,
designs unseen by any probe), paired with `--set cold_gestation=1.4`:

| Win share (mean final share) | `cold_gestation` 1.4 | 1.0 |
| --- | --- | --- |
| Cold abandoned eggs (63 entries) | 36.5% (0.103) | 34.9% (0.104) |
| Cold incubated eggs (58) | 25.9% (0.080) | 25.9% (0.080) |
| Cold live birth (73) | 19.2% (0.072) | 27.4% (0.085; +0.012 ± 0.003) |
| Warm abandoned eggs (65) | 10.8% (0.036) | 7.7% (0.036) |
| Warm incubated eggs (68) | 25.0% (0.086) | 25.0% (0.082) |
| Warm live birth (57) | 35.1% (0.093) | 29.8% (0.092) |

Pooled over both sets (192 worlds) cold-blooded live bearers win 18.1% →
26.0% and cold-blooded abandoned eggs 38.1% → 36.6%: both toward fair,
the second only slightly. `compare` on seeds 101–148 (48,
`--set cold_gestation=1.4 vs defaults`) scores 11.39 for both, identical
on every seed: the sample designs have no cold-blooded live bearer, so the
ecosystem and the standard sweeps are untouched (the client wire fixture
too). Snapshots move to format version 12 (the new `Balance` field;
versions 10 and 11 still restore with it at 1.4). New tunable:
`cold_gestation`. What was left was
cold-blooded abandoned eggs' fast cycle, taken up next.

**The target** for the six thermoregulation × birth classes is not exact
fairness (25% each in four-way tournaments, which no rule set gets within
the seed noise of about ±5 points): it is that **no class wins more than
about 30–32% of tournaments on fresh seeds**, and that none collapses.
Classes below 25% are fine; warm-blooded abandoned eggs sit at about 10%
on purpose (the doc's thermoregulation mismatch: their eggs chill). It is
judged on fresh sets pooled to 384 worlds or more, not on one 96-world
set: with equal classes, 79–84% of 96-world sets show some class above
32% by chance (26% at 384 worlds; see *Warm-blooded classes: what the
three levers showed*). A rule change for this is kept only if, pooled
over fresh sets, it brings the top class to 32% or less and pushes no
other class above 32%, moves the paired mean final share of the class
it aims at down without raising another class's above it, and costs no
significant `compare` score; the smallest such change is preferred.

**Cold-blooded abandoned eggs: `nest_predation`.** After `cold_gestation`
they were the top class (38.0% on the tuning seeds 10001–10096, 34.9% on
50001–50096) with the highest mean share. Their edge is the cycle (lay 2
days after mating, recover 3–9 days, exempt from breeding suppression)
compounded by eggs that are rarely lost (14% to cold, heat and raids):
1004 eggs laid per lineage, 3× a cold incubator's. Three fixes were tried
on the tuning seeds, paired against the unchanged rules:

| Fix (tuning seeds, 96 worlds) | Cold abandoned / incubated / live | Warm abandoned / incubated / live | Cold abandoned share | Warm abandoned share |
| --- | --- | --- | --- | --- |
| None | 38.0% / 26.4% / 24.1% | 9.7% / 21.6% / 31.7% | 0.083 | 0.034 |
| Yolk: an abandoned-egg mother recovers her eggs' incubation time longer | 32.4% / 30.2% / 24.1% | 8.3% / 21.6% / 36.7% | −0.011 ± 0.003 | −0.006 ± 0.003 |
| Egg predation by clutch density, 0.03 per neighboring clutch a day | 31.0% / 30.2% / 24.1% | 9.7% / 24.3% / 33.3% | −0.003 ± 0.003 | −0.002 ± 0.001 |
| The same, 0.06 | 29.6% / 28.3% / 29.6% | 9.7% / 23.0% / 33.3% | −0.010 ± 0.003 | −0.005 ± 0.002 |
| The same, 0.02 × neighbors² | 31.0% / 30.2% / 24.1% | 9.7% / 23.0% / 35.0% | −0.005 ± 0.003 | −0.002 ± 0.001 |

- **Yolk recovery** (the README's candidate: tie her recovery to the eggs
  she provisions; eggs left alone carry yolk for their whole development,
  which takes 1.4× as long when cold-blooded) cut cold abandoned laying
  from 1004 to 618 eggs per lineage and their share significantly, but it
  costs warm-blooded abandoned eggs as much in proportion (they lay as
  often, and lose most eggs to cold anyway), and the wins went to warm
  live bearers (36.7%). Rejected.
- **Egg predation on nesting grounds** hits the over-producers: abandoned
  eggs go to the best site within a few tiles of where the mother stands,
  so a lineage that lays every few days packs its clutches together, and
  egg predators that search where eggs are dense find them. At 0.06 cold
  abandoned eggs lost 316 eggs per lineage to it (raids 70 → 316 of 900
  laid) and their larvae starved less (336 → 171, larval competition
  being density dependent), so adults fell 127 → 107. Warm abandoned
  clutches are sparser and mostly chill first. Making it quadratic in the
  clutch count (to spare sparse nesters) spared warm-blooded abandoned eggs
  no better than linear. Kept at 0.06, the stronger linear setting: its
  effect on the target class is significant where 0.03's is not.

Judged on fresh seeds (designs unseen by any probe), paired with
`--set nest_predation=0`:

| Win share (mean final share) | 50001–50096, 0 | 0.06 | 60001–60096, 0 | 0.06 | Both (192), 0 | 0.06 |
| --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 34.9% (0.104) | 28.6% (0.090; −0.014 ± 0.003) | 28.2% (0.085) | 26.8% (0.075; −0.011 ± 0.004) | 31.3% | 27.6% |
| Cold incubated eggs | 25.9% (0.080) | 27.6% (0.088) | 27.9% (0.078) | 27.9% (0.074) | 27.0% | 27.8% |
| Cold live birth | 27.4% (0.085) | 26.0% (0.090) | 28.4% (0.081) | 29.9% (0.082) | 27.9% | 27.9% |
| Warm abandoned eggs | 7.7% (0.036) | 10.8% (0.035) | 12.2% (0.045) | 12.2% (0.046) | 9.6% | 11.4% |
| Warm incubated eggs | 25.0% (0.082) | 29.4% (0.085) | 31.7% (0.104) | 28.6% (0.101) | 28.2% | 29.0% |
| Warm live birth | 29.8% (0.092) | 28.1% (0.095) | 18.2% (0.074) | 21.2% (0.074) | 23.6% | 24.4% |

On both fresh sets every class is at 30% or less (top: 29.9%), from a
top class of 34.9% and 31.7%. On the tuning seeds the top class went from
38.0% to 33.3% (warm live bearers, 31.7% before), the one place a class
stays above 32%; over all 288 worlds the classes span 10.8–28.4% (before
9.7–33.7%), and the only significant changes in mean share are cold
abandoned eggs (−0.011 ± 0.002) and warm abandoned eggs (−0.002 ± 0.001). `compare --adaptive` from seed 101
(`--set nest_predation=0.06 vs defaults`, before the default changed)
stopped after 24 seeds at −0.09 (sequential CI −0.29 … +0.11, within the
±0.3 tolerance): no difference that matters. Held-out sweeps (12 seeds,
96×96): seed 31 11.60 (was 11.67, marsh newts 11 of 12), seed 55 11.73
(was 11.62); `scripts/balance-smoke.sh` 11.55 (was 11.96; newts lost on
1 of its 3 seeds), 23 of 24 lineage-worlds. Level-of-detail cohorts have
no clutches, so the rule applies to agents only, like egg chilling.
Snapshots move to format version 13 (versions 10–12 restore with
`nest_predation` 0). New tunable: `nest_predation`.

Do eggs count toward population? They don't, by design, and it changes no
result here. The legacy score counts breeding adults (the doc), and the
sweep's survival counts living organisms at the end of the run, so a
lineage reduced to eggs survives only if the eggs hatch. Eggs also don't
count toward the per-owner cap: live births are clipped to the room left
at birth, clutches when they hatch, so both fill the places free when the
young arrive. Only 12 of the 960 tournament entries ended at the cap (843
on a 96×96 map; 6 live bearers, 5 abandoned-egg and 1 incubating design),
too few to explain any mode's wins.

**Predators under the prey-side rules.** The open question from the
antelope work: do the prey-side rules cost predators seeds
systematically? `compare` on seeds 101–148 (48), all four off
(`threat_caution=0 alert_guard=0 pasture_search=0 search_image=0`) against
the defaults:

| | Off | On (defaults) |
| --- | --- | --- |
| Score | 11.58 | 11.39 (difference −0.19 ± 0.21, not significant) |
| Prey lineages lost (antelope + herd grazers) | 6 (4 + 2) | 1 (1 + 0) |
| Predator lineages lost (pack hunters + foxes) | 2 (1 + 1) | 5 (4 + 1) |
| Mean alive, significant differences (on − off) | | voles +327, newts +112, herd grazers +94, burrowers +62, toxic frogs +56, antelope +40; pack hunters −12, foxes −13 |

So yes: the rules move about four lineage losses from prey to predators
and cost both predators members. The pack hunters lost with the rules on
are not lost to a bust after a boom: on seeds 108, 128, 136 and 140
(`examples/predators`) the founding pack shrinks from 24 to 2–8 by day
40–60 while every prey lineage grows. Over the first 60 days on those four
seeds and 101–104, 102 of 215 pack-hunter deaths were thirst and 100
starvation; of the thirst deaths, 75 were at hunger 90 or more, 91 with
energy below 30 and 80 while walking to water; 87 died 3–14 tiles from
it, and 71 had no threat in sight. They are the doc's hunger cascade (hunger above 90
drains energy and slows the walk) in hunters that catch too little in
their first weeks, not vigilance keeping them from water.

Two predator-side rules were tried on the same 48 seeds; neither is in the
code:

| Rule | Score (A − B) | Lost (A / B) | Significant mean-alive changes | Effect on booms |
| --- | --- | --- | --- | --- |
| **Ratio-dependent litters** (`prey_scarcity` 0.5: a meat eater's litter × `r²/(r²+0.5²)`, `r` the prey kg per kg of her own kind in the 3×3 density cells around her; median `r` at a pack-hunter birth was 1.0, foxes 8.3) | +0.03 ± 0.18 | pack hunters 3 / 4, antelope 1 / 1, foxes 0 / 1 | pack hunters −8 | Seeds 101–124: hunter peak 81 → 65 and mean 50 → 40, peak-to-mean 1.67 → 1.72: hunters scale down, booms keep their shape |
| **Earlier search-image exception** (a hunter drops its search image at hunger 70 instead of 90, before its own hunger cascade starts) | +0.10 ± 0.14 | pack hunters 3 / 4, antelope 0 / 1, newts 1 / 0 | none | — |

Neither helps the prey that booms threaten nor the founding packs, so
neither earns its complexity.

**Why founding hunters catch so little.** Temporary counters in `decide`
and `attack` (not kept) followed grown pack hunters over their first 60
days on the four lost seeds (108, 128, 136, 140) and four kept ones
(101–104):

| Per grown hunter | Lost seeds | Kept seeds |
| --- | --- | --- |
| Kills a day | 0.07–0.18 | 0.15–0.20 |
| Hunger met by meat / by plants / accrued, a day | 0.7–2.2 / 24–29 / 32–35 | 1.2–2.5 / 25–34 / 32–34 |
| Food decisions with prey in sight | 7–19% | 10–21% |
| Time hunting (chasing or roaming) / courting / drinking | 25–46% / 21–41% / 8–29% | 33–51% / 30–45% / 5–16% |
| Chase ticks that end in a strike; strikes slipped; strikes that kill | 13–22%; 49–61%; 30–45% | 16–20%; 48–57%; 36–46% |

- Hunting is not where these hunters' food comes from. A 2.6 kg hunter
  (carnivory 0.9) catches about one prey every 5–7 days, mostly newts and
  voles (0.3–0.4 kg; targets average 0.3–0.85 kg); an antelope (4.8 kg) is
  struck about once per 10 chases of one and slips half the strikes. Meat
  covers about 2 of the 33 hunger points a day it needs (the hunter eats a
  bite of each kill, then leaves for courting; the rest rots or goes to
  others). About 85% comes from plants nibbled at 0.3 of a full graze while
  courting, drinking or resting, at a plant efficiency of 0.1.
- The rates differ little between lost and kept seeds. The lost seeds are
  the tail of the same thin budget. On 128 and 140 water is far (a fifth to
  a quarter of hunter time is spent walking to it), and on 136 the kill
  rate is half. Seeds 102 and 104 also fall to 10 by day 60 and recover
  later.
- The cascade: a hungry hunter picks the food option, which for a
  carnivore above 0.7 with no prey in sight (80–90% of the time) is
  roaming under `Action::Hunt`. Roaming eats nothing, so hunger buys time
  spent with no income. Strikes, chase energy and prey seen are not
  outliers. Of the prey in range, 60% go unseen (stealth, camouflage),
  10% are sheltered, and 7–15% each are dropped for warning colours,
  search image or injury risk.

Three targeted fixes were tried, each on seeds 101–148 against the
defaults; none is in the code:

| Rule | Score (A − B) | Lost (A / B) | Significant mean-alive changes |
| --- | --- | --- | --- |
| **Roaming forage** (a hunter roaming with no prey in sight nibbles plants at 0.3 when hunger > 30, like other actions) | −0.38 ± 0.24, B better | pack hunters 0 / 4, foxes 0 / 1, antelope 4 / 1, herd grazers 2 / 0 | pack hunters +124, foxes +100; voles −319, herd grazers −126, frogs −68, antelope −53 |
| **Starving roaming forage** (the same only at hunger > 70) | +0.14 ± 0.22, not significant | pack hunters 0 / 4, foxes 0 / 1, antelope 1 / 1, herd grazers 3 / 0 | pack hunters +77, foxes +68; voles −273, newts −115, frogs −90, herd grazers −89, antelope −44 |
| **Feeding at the kill** (a meat eater on a carcass keeps eating instead of courting, grouping, wandering or resting while hunger > 10) | screened only: on six of the seeds above meat rose from 1.6 to 2.1 hunger points a day and packs at day 60 moved both ways (5 → 12, 7 → 2, 8 → 6, 2 → 26, 10 → 14, 10 → 7) | — | — |

Both forage rules save every founding pack, but they do it by making the
hunters (and foxes) better fed all the time. That moves the lineage losses
back from predators to prey, mostly herd grazers. Feeding at the kill
can't help much while kills are this rare. So the founding packs' problem
is the design's food budget, not one bad rule. A pack hunter of this
size lives on what it nibbles, and anything that feeds it more costs the
prey the rules were tuned to protect. Saving founding packs without
costing prey would need a rule that acts only in the founding phase or
only on a pack in decline, which is a design choice, not a balance fix.

**A pack-hunter design whose budget works.** The fix went into the sample
design instead of the rules: pack hunters are now body size 0.22 (0.8 kg,
weasel-sized) instead of 0.42 (2.6 kg), 41.7 points instead of 38.5. What
a hunter gets from a kill or a mouthful of plants is food units divided by
`food_per_hunger` (mass^0.75), while its hunger points a day only grow
with `size_burn` (mass^-0.25), so the same vole or the same nibble covers
about mass^-0.5 of its day: 1.8 times as much at 0.8 kg as at 2.6 kg. The
smaller hunter can't take an adult antelope any more (cooperative limit
4.5 × own mass is 3.6 kg) and lives on voles and newts, the prey it was
catching anyway. Five design changes were tried against the defaults; a
4-seed screen on the lost seeds (108, 128, 136, 140) saved 2–3 of the 4
packs for every change (any change re-rolls a tail event), so it told the
candidates apart badly, and the 48-seed runs (101–148) decided:

| Pack-hunter design change | Score (A − B) | Lost (A / B) | Significant mean-alive changes |
| --- | --- | --- | --- |
| **Body size 0.32** (1.4 kg) | +0.14 ± 0.20 | pack hunters 1 / 4, foxes 2 / 1, antelope 1 / 1 | pack hunters +16; voles −77, antelope −21 |
| **Body size 0.32, carnivory 0.75** (doubles plant income) | −0.20 ± 0.25 | pack hunters 0 / 4, antelope 3 / 1, foxes 3 / 1, herd grazers 1 / 0 | pack hunters +87, newts +58; antelope −27 |
| **Litter 2, 30 founders** (less pregnancy cost, more founders) | −0.21 ± 0.26 | pack hunters 3 / 4, foxes 3 / 1, burrowers 1 / 0, antelope 0 / 1 | pack hunters −17; voles +83, newts +54 |
| **Camouflage 0.4** (screened only) | — | screen: 2 of 4 lost packs kept | — |
| **Body size 0.22** (kept) | **+0.33 ± 0.20, A better** | pack hunters 0 / 4, antelope 0 / 1, foxes 1 / 1 | pack hunters +12, herd grazers +146, antelope +27; voles −169, newts −98, burrowers −14 |

Body size 0.32 makes the hunters fox-sized, and with carnivory 0.9
against the foxes' 0.85 the two designs' founders count each other as
their own kind (`is_kin`; the presets test rejects it), so it was not an
option on its own. Its kin-safe variant with carnivory 0.75 feeds the
hunters on plants and, like roaming forage, moves the losses to prey.
Body size 0.22 saves every founding pack while no prey lineage is lost:
the hunters press harder on voles and newts, which are the most numerous
lineages, and less on antelope and herd grazers. On held-out seeds
(`compare --adaptive` from 201) it stopped after 24 seeds at +0.45
(sequential CI +0.09 … +0.80), pack hunters lost 0 / 1, herd grazers
0 / 2, newts 1 / 1. Held-out sweeps (12 seeds, 96×96): seed 31 11.67
(was 11.54), seed 55 11.62 (was 11.41); `scripts/balance-smoke.sh` 11.96
(was 11.21), 24 of 24 lineage-worlds. Three group tests that used a pack
hunter as the threat to an antelope or herd grazer now give it the old
2.6 kg body: the 0.8 kg hunter can't threaten them (`can_threaten` needs
more than 0.3 of the prey's mass). The snapshot round-trip test's busy
world moved from seed 22 to 23, since seed 22 no longer formed LOD
cohorts.

**Trait downsides: an audit, and contextual costs for fangs and scales**
(`fang_grazing`, `scale_cover`). Sliders should mean something at low,
middle and high values, and a trait pushed high should cost something
besides points (the doc: "most traits carry a built-in cost"). The audit
of every paid trait gene (`trait_cost`), from `stats::derive`, `terrain`,
`decide`, `apply`, `group` and `seasonal`; "upkeep" is the share it adds
to the hunger rate (`stats::derive`):

| Gene (points) | What it does | What it costs besides points |
| --- | --- | --- |
| Body size (0) | Strength, defense, mass, height (wading, snow, reach) | Speed, endurance, agility, stealth; more food; slower maturity; fewer young per kg |
| Muscle (7) | Speed, strength | Mass (food), metabolism +30%, upkeep 0.15; sinks (buoyancy) |
| Fat stores (2) | Endurance, insulation, buoyancy, fat capacity for dormancy | Mass, speed; stored fat slows escape |
| Leg length (25) | Speed, endurance, height | **Points only** (priced high: valuable everywhere) |
| Claws (6) | Strength, agility, grip (ice, slopes), digging above 0.5 | Upkeep 0.1 only |
| Hooves (16) | Speed, endurance | Agility; slip on ice unless claws grip; smaller feet sink in mud, snow, sand |
| Foot width (6) | Low ground pressure (mud, snow, sand), some swimming | **Points only** |
| Tail (4) | Agility | **Points only** |
| Swimming (6) | Crosses deep water and currents | **Points only** (worthless away from water) |
| Wings (24) | Glide above 0.25, fly above 0.5 if light enough | Upkeep 0.2; grounded by storms; slowed in forest; heavy bodies can't lift |
| Climbing (5) | Canopy refuge and browse (≥ 0.4), cliffs, slips through vegetation, grip | **Points only** (worthless without trees or cliffs) |
| Burrowing (6) | Shelter above 0.5 (hidden, out of reach, buffered temperature), shared burrows | **Points only** (sheltering means resting) |
| Fur (3) | Insulation (warm-blooded) | Upkeep 0.05; lowers the heat ceiling (overheats); on a cold-blooded body blocks basking |
| Scales (4) | Defense +0.3 | Was **points only**; now displaces fur's warmth on a furred warm body (below) |
| Armor (2) | Defense, width | Mass, speed, agility, sinks, upkeep 0.1, wider (forest, burrows) |
| Fangs (7) | Strength +0.3 | Was upkeep 0.1 only; now worse plant feeding, scaled by the plant diet (below) |
| Large eyes, keen nose (4) | Perception | Upkeep 0.1 only |
| Large ears (4) | Perception; alarm calls with vocal | Upkeep 0.1; shed heat (−0.15 insulation: cold costs more, heat less) |
| Camouflage (5) | Stealth (seen later, ambushes) | **Points only** |
| Toxins (18) | Defense; above 0.3 hurts and deters biters | Upkeep 0.1 |
| Coloration (5) | Mate signal; warning signal with toxins (above 0.6) | Stealth −0.35 |
| Vocal ability (5) | Alarm calls above 0.5 (with ears above 0.5, in a group) | **Points only** (a group threshold trait) |
| Seasonal strategies, add-ons, vigilance | See *Seasonal strategies* | Each has its own (fattening, lost season, journeys, raids, molt cost, breeding window, food for watch) |

Points-only traits split in two. Situational abilities (swimming,
climbing, burrowing, foot width, vocal) are worth nothing where the
terrain or group doesn't call for them, so the points are their cost.
Fangs and scales were plain upgrades with no natural price, and the
owner's fangs example ("fangs that are too long would have some
downside") named one. Both now cost in context, through ratios and
products of genes like the existing pairs (fur × cold blood, hooves ×
claws on ice), with no bans on combinations:

- **Fangs: a size, with a threshold and a grazing cost.** Below 0.1
  (`stats::FANG_THRESHOLD`) a body has no fangs at all: no bite bonus,
  no upkeep, no grazing cost, and nothing should be drawn; above it the
  value is fang size. Long fangs get in the way of cropping and chewing
  plants: plant food is worth `1 - fang_grazing × fangs × (1 −
  carnivory)` of its value (`Traits::plant_efficiency`), on top of the
  diet's own share. At the default `fang_grazing` 0.5, sabres halve a
  grazer's plant food, cost an even omnivore a quarter at full size, and
  a hunter at carnivory 0.85 7.5% (the red fox's 0.5 fangs: 3.75%). A
  first form without the `(1 − carnivory)` factor (a flat 40% of plant
  value at full size) hit hunters hardest in tournaments (−0.014 ± 0.006
  mean final share at carnivory 0.85, 192 worlds): hunters live off
  plants between kills and on thin margins, so it was replaced.
  Applies to agents; level-of-detail cohorts graze on `1 − carnivory`
  alone, ignoring the generalist tax as well.
- **Scales displace fur.** Scales take skin a furred body would cover
  with fur: a warm-blooded body loses `scale_cover × scales × fur
  warmth` of insulation (`Traits::insulation`; fur warmth is 0.6 per
  unit). At the default 0.5, full scales on full fur cost 0.3 of
  insulation (about 9 °C off the cold comfort limit). Scales on a
  cold-blooded or naked body cost nothing. Applies to agents and, through
  the cohort probe's genes, to cohorts.

Old saves keep the old rules: snapshots move to format version 20, and
versions 10–19 read both parameters as 0 (`serde_util::before_v20`). The
fang threshold is a phenotype rule, recomputed on restore like every
trait: an old world's few organisms with drifted fangs below 0.1 lose
their bite bonus of at most 0.03 strength.

Judged against the unchanged rules:

- `compare --adaptive` from seed 101 (`vs --set fang_grazing=0 --set
  scale_cover=0`): +0.02 (sequential CI −0.16 … +0.20, 12 seeds, within
  ±0.3); no lineage lost; burrowing omnivores +27 ± 20 mean alive, the
  rest within noise.
- The 12-seed sweep from seed 31 scores 11.74 (11.76 before;
  `scripts/balance-smoke.sh` thresholds untouched).
- Tournaments on the four fresh sets 70001–, 80001–, 90001–, 100001–
  (384 worlds, 1536 entries, 90 days), paired with the baselines of
  *Warm-blooded classes* (same designs, unchanged rules; win share,
  mean final share and its paired change ± one standard error):

| Pooled 384 worlds | Before | After (paired change) |
| --- | --- | --- |
| Cold abandoned eggs | 23.5% (0.078) | 22.0% (0.078; −0.000 ± 0.001) |
| Cold incubated eggs | 30.9% (0.097) | 32.1% (0.097; +0.000 ± 0.002) |
| Cold live birth | 28.8% (0.082) | 29.2% (0.082; −0.000 ± 0.002) |
| Warm abandoned eggs | 7.4% (0.033) | 6.7% (0.033; −0.001 ± 0.001) |
| Warm incubated eggs | 32.0% (0.098) | 31.6% (0.097; −0.001 ± 0.002) |
| Warm live birth | 29.8% (0.106) | 31.0% (0.108; +0.002 ± 0.002) |

  No class's mean share moves by more than one standard error, and the
  pooled top class stays at 32% (cold incubators, 32.1%, against warm
  incubators' 32.0% before). Per set the top class is 38.9% (90001–, cold
  incubators; 40.7% before), 32.8% (100001–), 38.8% (70001–, warm
  incubators; 35.8% before) and 38.7% (80001–, warm live; 35.5%
  before), the one-set noise described there.
- Where the costs fall (paired change of mean final share): designs with
  fangs and carnivory 0 −0.009 ± 0.005 (61 entries), 0.1 −0.004 ± 0.004,
  0.5 −0.008 ± 0.004, 0.85 −0.001 ± 0.002 (78); designs without fangs
  +0.001 ± 0.001. Warm designs with both scales and fur are rare among
  random designs (16 entries, +0.006 ± 0.006: too few to see the cost);
  other scaled designs −0.002 ± 0.002.

Kept: `fang_grazing` 0.5, `scale_cover` 0.5. Not changed: the other
points-only traits (above), and tail, whose cost would be contextual on
locomotion (a balance aid for climbers, swimmers and runners) and needs
its agility moved out of the birth-time stats; a candidate for a later
pass. No stats are on the wire; the client wire fixture was regenerated
only because its short session's world now plays out differently.
The snapshot round-trip test's busy world is now snapshotted 12 hours
later the same day (tick 13 × 48 + 29), since it held no clutch at the old
tick. New tunables: `fang_grazing`, `scale_cover`.

### Body plan effects

`morph_effects` (package D of [`docs/morphology.md`](docs/morphology.md),
§3, minus vision). The body plan (`morph_effects::BodyPlan`) is the
organism's expressed genome morphology, `morph::Morphology`
(`Organism::morph`, see [Body plan wiring](#body-plan-wiring)); sizes stay
the existing `Ears`, `LegLength` and `Tail` genes.
`morph_effects::derive` turns a body plan into a `MorphEffects` (every
derived number and stat change), and `morph_effects::apply(traits, pheno,
plan, balance)` folds it into the organism's `Traits` once, when
`World::insert` adds a newborn, founder or expanded cohort member, and
again after a restore (from the restored config). Every term is written
relative to the default body, so at the default plan (snout exactly 0.5)
every factor is exactly 1 and every change exactly 0 whatever the genes
(`default_body_is_exactly_neutral` checks every preset bit for bit).
Snout length is a continuous locus that founders vary around the design's
0.5 (founder variation 0.05 per allele), so with `morph_effects` on a
default-designed lineage does feel small snout effects either way
(`default_world_feels_only_its_snouts` checks that a standard world's
bodies differ from their bare traits only through the snout, and not at
all with the tunables at 0).

Tunables (all 1, the spec's values, by default since [Morphology on by
default](#morphology-on-by-default); saves from before read 0, and with all
four at 0 `apply` returns at once, so results are bit-identical):

| Tunable | Scales | Spec value |
| --- | --- | --- |
| `morph_effects` | every count, shape and snout effect below (neutral at the default body) | 1 |
| `morph_tail_drag` | tail drag in burrows and dense vegetation (affects today's tailed bodies) | 1 |
| `morph_tail_chill` | heat loss of thin and fin tails (affects today's tailed bodies) | 1 |
| `morph_long_legs` | long legs tippy and digging worse (affects today's long-legged bodies) | 1 |

Notation: `n` leg pairs, `T` = `Tail` when a tail is present (else 0),
`s` snout length, `c` ear count, `k` = `morph_effects`. Each additive
change is multiplied by `k`, each factor `f` becomes `1 + k·(f − 1)`.

- **Ears** (shape: gain / localization `L0` / area `A_s`: round 1.0 / 0.4 /
  1.0, pointed 1.1 / 0.7 / 1.1, long 1.2 / 0.6 / 1.5, fan 1.3 / 0.5 / 2.0).
  Hearing channel `0.03 + 0.3·Ears·gain·f_c` (`f_c` 1 for 2 ears, 1.15 for
  4; 0.01 with no ears) replaces today's `0.03 + 0.3·Ears` share of
  perception. Localization `L = L0 + 0.2·[4 ears]` (0 without ears).
  Radiating area `A = Ears·(c/2)·A_s`: insulation `−0.15·(A − Ears)·(1 −
  0.4·Fur)` (only the change from today's `0.15·Ears` is fur-blunted, so
  the default stays exact), a warm body's heat tolerance `+6·(A − Ears)`
  °C, stealth `−0.04·Ears·(A_s − 1)`, upkeep `+0.1·Ears·(c/2 − 1)`. Alarm
  calls also need `c > 0` (structural, no tunable).
- **Legs** (`n` 0–4: gait 0.6 / 0.95 / 1.0 / 0.92 / 0.85, `S0` 1.0 / 0.45
  / 0.7 / 0.85 / 0.95). Speed: the leg terms `0.45·LegLength + 0.25·Hooves`
  × gait; legless: base 0.45 × 0.6 and no leg terms. Stability `S =
  S0·(1 − 0.3·LegLength·morph_long_legs) + 0.15·T·[biped]` against
  `S_ref` 0.7; `S_ref/S` multiplies ice slip and the slope speed penalty
  (folded into the body's effective grip, `1 − (1 − grip)·S_ref/S`, the
  only two uses of grip), armor's speed penalty × `sqrt(S_ref/S)`, defense
  `+0.1·(S − S_ref)`. Biped with `T < 0.2`: agility `−0.15·(1 − T/0.2)`.
  Foot area × `n/2` (legless: belly 1.5), so ground pressure ×
  `cbrt(mass factor)·foot/foot'`. Height: legless `0.15 + 0.3·size`, biped
  `+0.1·LegLength`. Climbing × 0.6 / 0.5 / 1 / 1.2 / 1.2. Legless: burrowing
  +0.2, swimming +0.15, stealth +0.1. Mass × `1 + 0.04·(n − 2)·(0.5 +
  LegLength)`; upkeep `+max(−0.05, 0.05·(n − 2))`; endurance `−0.04` per
  pair beyond 2. Wings: the default four-legged winged body is the
  reference, so wings that replace forelimbs (biped) or a legless winged
  body shed the extra-limb cost: mass ÷ `(1 + 0.15·Wings)`, upkeep
  `−0.1·Wings`. `morph_long_legs` also takes `0.2·LegLength` off a legged
  body's burrowing.
- **Tail shape** (agility per `T`: thin 0.3 as today, bushy 0.2, fin 0.1,
  club 0.05, prehensile 0.2). Bushy: a warm body resting gets insulation
  `+0.15·T`, stealth `−0.05·T`. Fin: swimming `+0.4·T`, speed `−0.05·T`.
  Club: defense `+0.3·T`, fight-back `+0.3·T` (`decide::fight_back`, so
  hunters weigh it too), mass × `(1 + 0.15·T)`, speed `−0.05·T`.
  Prehensile: climbing `+0.3·T`, grip `+0.15·T`, upkeep `+0.05·T`. Drag
  (`morph_tail_drag`): burrowing × `(1 − 0.2·T)`, and an effective width
  `+T/3` in vegetation, which takes `0.2·T·veg_density` off the speed
  factor there. Chill (`morph_tail_chill`, thin and fin): insulation
  `−0.05·T`.
- **Snout** (all relative to 0.5). Smell channel `0.3·Nose × (0.7 + 0.6·s)`;
  bite: the fang term of strength × `(1.3 − 0.6·s)`; prey lighter than 0.2×
  the hunter's mass escapes × `(1 − 0.4·(s − 0.5))`; a warm body's heat
  tolerance `+3·(s − 0.5)` °C; burrowing `+0.1·(s − 0.5)`; binocular
  overlap hidden `40°·max(0, s − 0.5)` (a readout for the vision package).
- **Upkeep** changes add to `upkeep`, so metabolism changes by `(0.6 +
  0.3·Muscle)·(warm ? 1 : 0.35)·Δupkeep`.
- **Level-of-detail cohorts** (`world::lod` representatives) get the same
  effects: the probe organism's traits (passability, browse) and every
  sampled genome's stats (mass, metabolism, insulation, stealth, burrowing)
  are computed from each genome's own morphology with `apply`, as at an
  agent's birth, and re-applied to the probe after a restore. A bushy
  tail's resting warmth counts at 0.75 of the cohort's day (the resting
  share agents' mean activity of 0.9 implies) in the cohort's insulation. A
  warm body's heat tolerance (fan ears, long snout) has no counterpart:
  cohorts model only cold stress, no heat stress at all. With the tunables
  at 0 the representatives are bit-identical
  (`cohort_representatives_feel_the_body_plan`).

**Interfaces.** The vision package reads `Traits::morph`
(`MorphContext`): `hearing` and `smell` are the effective perception
channels (with eyes at `0.10 + 0.3·Eyes`, the three add up to
`Stats::perception`), `localization` is `L`, and
`morph_effects::heard_bearing(true_bearing, L, u)` places a threat heard
in a visual blind spot at `true_bearing + u·70°·(1 − L)` for a caller's
deterministic draw `u` in [−1, 1] (`bearing_error_deg` gives the bound);
`MorphEffects::snout_occlusion_deg` is the overlap the snout hides.

**Client fixture.** `crates/sim/tests/fixtures/morph_effects.json` holds
28 sample body plans (inputs: plan, every gene; outputs: every derived
number, and the base and effective stats and body) at full strength (all
four tunables 1). `morph_effects::tests::fixture_is_current` fails when the
code changes without it; regenerate with `EVO_UPDATE_FIXTURES=1 cargo test
-p evo-sim --lib morph_effects::tests::fixture`.

**Measured.** With the defaults the 12-seed sweep from seed 31 still
prints 11.76 (11.74 once merged after the trait downsides, which moved it). `compare --seed 101 --seeds 3 --set morph_effects=1 vs
defaults` gave +0.00 on every seed (identical worlds: every body was
default before the genome's morphology loci were wired in; preset and wild
designs still are, apart from their founders' snout variation). `compare --adaptive` from seed
101 with all four tunables at 1 (`--set morph_effects=1 --set
morph_tail_drag=1 --set morph_tail_chill=1 --set morph_long_legs=1 vs
defaults`; tail drag and chill and long-leg tippiness do touch today's
bodies) stopped after 24 seeds at −0.01 (sequential CI −0.23 … +0.22,
within ±0.3); no lineage's survival or mean alive moved significantly,
burrowing omnivores gained +0.71 ± 0.57 pp of legacy. Contextual unit
tests (`morph_effects::tests`): six legs are faster on slopes and ice and
carry armor better but pay speed, endurance, food and mass; legless
bodies are slower, stealthier, lower, burrow and swim better and keep
more speed on sand; a tailless biped loses agility; fin tails swim faster
in deep water; club tails add defense and fight-back; long tails drag in
dense vegetation and burrows (not in the open); long legs are tippy on
slopes; fan ears raise heat tolerance and cost insulation (less with
fur); four ears hear and locate better, no ears only feel vibration; a
long snout smells better and snaps small prey, a flat face bites
hardest. The calibration pass of `docs/morphology.md` §4 turned all four
on (see [Morphology on by default](#morphology-on-by-default)).

Old saves keep the old rules: snapshots move to format version 21, and
versions 10–20 read all four tunables as 0 (`serde_util::before_v21`).
New tunables: `morph_effects`, `morph_tail_drag`, `morph_tail_chill`,
`morph_long_legs`.

### Body plan wiring

The genome's morphology segment drives everything that has a body plan;
there is one source of truth, `morph::Morphology`, computed at birth (and on
restore) from the genome's expressed loci and the phenotype
(`Organism::morph`). What reads it:

| Reader | What | Behind |
| --- | --- | --- |
| `stats::derive_with(pheno, morph)` | the eye geometry (`Traits::vision`: count, azimuth, size, snout occlusion) and the ears' localization in `Traits::morph` | the geometry is read only by directional vision, `directional_vision` (1); no rule reads localization yet |
| `morph_effects::apply` (`World::insert`, restore) | ear count and shape, leg count, tail shape and snout effects on stats and body | `morph_effects`, `morph_tail_drag`, `morph_tail_chill`, `morph_long_legs` (all 1) |
| `GroupTrait::is_carrier_with` | an earless body carries no alarm calls | structural (always on; only designed earless bodies are affected) |
| LOD cohorts (`Rep::compute`) | the same effects for cohort representatives | the `morph_*` tunables |
| kinship, species, fertility | the morphology key (`morph_terms`) | always on; exactly 0 between equal body plans |
| server `look` (`appearance::Morphology::of`) | eyes, eye placement, ears and ear shape, legs, tail shape, sizes, snout, fangs | always on (cosmetic) |

`stats::derive(pheno)` remains for code with only a phenotype: it uses
`Morphology::wild_type(pheno)` (the contract defaults with that phenotype's
sizes). Every preset and wild species has the default discrete body plan,
so with every morphology tunable at 0 the worlds run bit for bit as before
the morphology work (the sweep from seed 31 prints 11.74); with today's
defaults (all on) it prints 11.60, see [Morphology on by
default](#morphology-on-by-default). A designed body plan (the editor's Builder mode sends `morph`)
founds organisms that carry it, look it (`designed_morphology_shows_in_state_frames`,
and the browser smoke test checks a third eye in `look` and in the sprite
key) and pass it on by the dominance rule (`world::morph_tests`: young of a
six-legged, three-eyed, fin-tailed design are the same; a cross with a
default lineage shows the wild type and carries the design, which returns
in a quarter of the second generation). Its stat effects and eye geometry
play out under the tunables, on by default since the calibration pass of
`docs/morphology.md` §4.

### Directional vision

`vision`, `Balance::directional_vision`, on (1) by default since
[Morphology on by default](#morphology-on-by-default). Eyes see fields
around the way the animal faces, as in the morphology spec (`docs/morphology.md`, contract v1):

- **Heading.** `Organism::heading` is the unit vector of its last movement
  (set in `step_toward` whenever it moves; kept while it stands still), and a
  fixed direction from its id until it first moves (`vision::birth_heading`).
  It is in snapshots from version 22 (`serde_util::before_v22`); older saves
  restore with each organism facing its birth heading and with
  `directional_vision` 0 and `vision_scan` 1.
- **Geometry, once at birth** (`Traits::vision`, `Vision::new`). Eyes: an odd
  count puts one on the midline, pair *k* sits at ±min(170°, azimuth + 50°·k),
  each sees `140° + 60°·eye_size`. Derived exactly (the eye count only changes
  at field edges): coverage, binocular depth field (degrees within ±60° seen
  by two or more eyes, less the snout's `40°·max(0, snout − 0.5)`), front gap
  and rear blind spot, plus a 32-bin table of 0 / 1 / ≥2 eyes over the folded
  bearing (2 bits per bin, one `u64`). Per neighbor the cost is one dot and
  one cross product, a division (a trig-free pseudo-angle) and a table lookup
  (`Vision::eyes_on`). `vision::eye_layout` reads the organism's body plan
  (`Organism::morph`: eye count, the first pair's azimuth and the snout from
  the genome's morphology loci, the `Eyes` gene as size). The defaults (2
  eyes at ±60°, snout 0.5) give `20 + 60·Eyes` degrees of depth field and a
  `100 − 60·Eyes` degree rear blind spot; founders vary the azimuth and snout
  loci around the design's values like any additive locus, so with
  `directional_vision` on, default-designed lineages differ a little too.
- **Detection** (`decide::look`). Perception splits into channels: sight
  `0.10 + 0.3·Eyes`, hearing `0.03 + 0.3·Ears`, smell `0.02 + 0.3·Nose`. With
  `k = directional_vision`, a neighbor in a blind sector (no eye on it) is
  noticed at `1 − k·visual_share·(1 − g)` of the usual distance, where
  `visual_share = sight / perception` (0.42 for eyes, ears and nose at 0.5)
  and `g` is 0, or `0.5·vision_scan·coverage` for an animal standing still (no
  movement target: it scans; `vision_scan` 1). Hearing and smell keep working,
  so blind spots shorten detection rather than erase it. This only ever
  shortens the detection distance, so the far-neighbor pruning bounds hold.
  It applies to threats, prey and territory defenders alike, and to possessed
  animals too.
- **Depth judgment** (`World::attack`). A hunter's kill chance is multiplied
  by `1 + 0.35·k·(min(binocular, 90) − 20) / 90` (`Vision::depth_factor`):
  1 for the contract's default eyes (size 0), up to +0.27 for a forward-eyed
  stalker, below 1 for side eyes. Ambush needs no rule of its own: a hunter
  coming from a blind sector simply isn't noticed, so `ambush_on` applies.

Measurements (12 seeds, 96×96, 120 days; with today's fixed layout only the
`Eyes` gene varies, so this measures the mechanic on default-eyed animals):

| | Off (default) | `directional_vision=1` |
| --- | --- | --- |
| Sweep, seed 31 (`sweep --size 96 --trials 1 --seeds 12 --rounds 1`) | 11.76 ± 0.28 (unchanged; 11.74 once merged after the trait downsides) | 11.53 ± 0.50 (marsh newts and pack hunters each lost on one seed) |
| `compare --adaptive` from seed 101, A − B | | −0.05 (sequential CI −0.25 … +0.15; stopped at 12 seeds: no difference that matters) |

In the paired compare every lineage survived on every seed under both; mean
alive moved within noise: marsh newts −62 ± 69, toxic frogs −41 ± 74, herd
grazers −15 ± 56, meadow voles −2 ± 101, antelope +6 ± 26, red foxes −3 ± 16,
pack hunters +1 ± 21, burrowing omnivores +8 ± 13 (the only significant
change is the burrowers' legacy, +1.0 ± 0.9 pp). The rear blind spot costs
the newts and frogs (small eyes, so big blind spots) a little; hunters gain
little because they also have blind spots and the default depth multiplier is
about 1. The real trade-offs appear once designs choose eye layouts.

**On the map.** The owner sees the cones of their selected organism (else
the possessed one): the server sends each own organism's quantized heading
and eye descriptor (protocol 3, *Vision cones* under the WebSocket
protocol), never anyone else's. The organism panel shows the view, depth
and blind numbers, a "Show vision" toggle, and whether the mechanic is on in
this world (off by default: the cones then describe the eyes, detection does
not depend on facing).

Cost (callgrind, `evo-sim bench --days 5 --threads 1`, same world): off,
`World::step` +0.36% instructions over the build without the mechanic
(heading updates and the birth-time geometry; decide +0.26%), the world's
results bit-identical. On, the decide phase costs 6,236 instructions per
call against 6,265 off (the world differs, so only per-call figures
compare): the per-neighbor lookup is lost in the noise.

### Morphology on by default

The calibration pass of `docs/morphology.md` §4 (decision 17). Every
morphology mechanic that was built behind a tunable at 0 is now on by
default, at the spec's value:

| Group | Tunables (default now) |
| --- | --- |
| Body plan effects | `morph_effects`, `morph_tail_drag`, `morph_tail_chill`, `morph_long_legs` (1) |
| Directional vision | `directional_vision` (1; `vision_scan` 1, unchanged) |
| Step mutation of counts and shapes | `morph_step_rate` (0.0005) |
| Morphology alleles in the legacy tally | `morph_legacy` (1, new: snapshot version 24) |

`morph_legacy` is new: above 0.5 the legacy tally (`LegacySnapshot`, cohorts
included) and niche leaders count each genome's 14 morphology alleles under
their own origin tags (`Genome::count_origins_with` /
`tally_origins_with`), as the spec's decision 5 asks; the allele origins
shown to players (`genetics::allele_origins`) and species samples still
count the 100 gene alleles only. A founder's morphology alleles carry its
designer's tag like its genes, so a pure lineage's share is unchanged; the
morphology segment only shifts shares through hybrids and mutations.
`Balance::morphology_off()` gives any rule set with all seven off (default
worlds then run bit for bit as before the morphology work).

**Old saves keep their rules.** The first six tunables are in every save
since versions 21–23 (written as the 0 they ran under), and older saves read
them as 0; `morph_legacy` is new in snapshot version 24 and versions 10–23
read it as 0 (`serde_util::before_v24`). So a restored world keeps the rules
it was saved under, and only new worlds start with the morphology on; a
server that wants an old world under the new rules sets them by hand at a
season boundary (the spec's decision 5 deploys the legacy change then).

**Measured** (each group alone and together, against the previous defaults;
12-seed sweeps at 96×96, 120 days; `compare --adaptive` from seed 101 at
±0.3 tolerance; "lost" is a lineage dying out on one seed):

| | Sweep, seed 31 | Held-out sweep, seed 55 | `compare --adaptive` A − B (sequential 95% CI, seeds) |
| --- | --- | --- | --- |
| Previous defaults (all off) | 11.74 ± 0.33 | 11.81 ± 0.25 | |
| Body plan effects | 11.40 ± 0.56 (newts, foxes, pack hunters each lost once) | 11.75 ± 0.31 | −0.09 (−0.26 … +0.08, 36) |
| Directional vision | 11.50 ± 0.48 (newts, pack hunters lost once) | 11.83 ± 0.25 | −0.10 (−0.23 … +0.03, 24) |
| Step mutation 0.0005 | 11.74 ± 0.33 | 11.82 ± 0.25 | +0.00 (identical worlds, 12) |
| Legacy inclusion | unchanged (scoring only; no rule reads legacy) | unchanged | |
| **All (the new defaults)** | **11.60 ± 0.40** (pack hunters lost once) | **11.52 ± 0.38** (newts, pack hunters lost once) | −0.11 (−0.28 … +0.05, 24) |

No group, and not the combination, has a difference that matters in the
paired compare (every sequential interval lies inside ±0.3). The combination
keeps both sweeps within ±0.3 of their baselines (−0.14 and −0.29). The body
plan effects alone are 0.34 under at seed 31, from three single-seed losses
in three different lineages; neither half reproduces it (snout and shape
effects alone, `morph_effects` 1: 11.64; tail drag, tail chill and long legs
alone: 11.76), the held-out sweep is −0.06 and the paired compare −0.09, so
it is sweep noise and no coefficient was tuned. In the paired compares no
lineage's survival moved significantly; significant mean-alive and legacy
moves were small: body effects, meadow voles +70 ± 56 mean alive and herd
grazers −1.4 ± 1.4 pp legacy; vision, toxic frogs +39 ± 34 mean alive
(+2.2 ± 2.0 pp) and burrowing omnivores −1.6 ± 1.3 pp; all together, none.
Step mutation left the 12 compare worlds identical: a mutant count is a
hidden recessive (the wild type is dominant), so in 120 days it changes
nobody's expressed body.

Tournaments on the four fresh sets 70001–, 80001–, 90001–, 100001– (384
worlds, 1536 entries, 90 days; `calibrate tournament` with all seven on),
paired with the baselines of *Trait downsides* (same designs, which
reproduce bit for bit at the old defaults; win share, mean final share and
its paired change ± one standard error). The random designs keep the
default body plan, so this tests whether default bodies are disturbed
(snout and eye-azimuth variation, tail drag and chill, long legs, blind
spots):

| Pooled 384 worlds | Before | After (paired change) |
| --- | --- | --- |
| Cold abandoned eggs | 22.0% (0.078) | 22.4% (0.079; +0.001 ± 0.002) |
| Cold incubated eggs | 32.1% (0.097) | 32.9% (0.099; +0.002 ± 0.002) |
| Cold live birth | 29.2% (0.082) | 24.8% (0.079; −0.003 ± 0.002) |
| Warm abandoned eggs | 6.7% (0.033) | 7.7% (0.033; +0.000 ± 0.001) |
| Warm incubated eggs | 31.6% (0.097) | 33.6% (0.103; +0.006 ± 0.002) |
| Warm live birth | 31.0% (0.108) | 31.0% (0.109; +0.001 ± 0.003) |

The pooled top class moves from 32.1% (cold incubators) to 33.6% (warm
incubators, the one change beyond two standard errors, mostly from set
90001–); per set the top class is 37.3% (70001–, warm incubators; 38.8%
before), 33.9% (80001–, warm live; 38.7%), 41.5% (90001–, warm incubators;
38.9% cold incubators before) and 32.9% (100001–; 32.8%), within the
one-set noise seen before. By trait (paired change of mean final share):
designs with a tail +0.001 ± 0.002 (263 entries), long legs +0.001 ±
0.003 (249), burrowing −0.000 ± 0.002 (272), large eyes +0.002 ± 0.002
(279), hunters (carnivory ≥ 0.45) +0.001 ± 0.001, all +0.001 ± 0.001.

**Cost.** Callgrind, `evo-sim bench --days 10 --threads 1` (96×96, about
2,200 organisms): `World::step` takes 18.64 G instructions with the
defaults against 19.20 G with `morphology_off` (2,220 against 2,216
organisms on average), −3% per organism-tick (the worlds differ, so this is
within noise; the per-neighbor vision lookup was measured at no cost in
*Directional vision*), well inside the 5% budget. Body plan effects are
applied once at birth and the legacy tally once a day.

The standard sweep's new baseline is **11.60** from seed 31 (11.52 from 55);
`scripts/balance-smoke.sh` thresholds are unchanged. `evo-sim sweep` now
prints parameters in full where three decimals would be more than 1% off,
so its "Try it" line keeps `morph_step_rate=0.0005`. The client wire
fixture was regenerated (its short session plays out differently).

### Predators over the years

The genetics harness's ecology level (`evo-sim genetics --long --level
ecology`, 15 years) lost the red fox and the pack hunters on all four
worlds, between day 200 and day 520 (a year is 120 days). Every tuning
measure here runs 120 days, one year, and at 120 days both predators are
fine (sweep from seed 31: foxes 12 of 12, pack hunters 11 of 12). Over
longer runs (`examples/predlong`, read only, below; standard world 96×96):

| Run | Red fox | Pack hunters |
| --- | --- | --- |
| Sweep from seed 31, 8 seeds, 730 days (6 years): survived | 0 of 8 | 0 of 8 (score 6.67) |
| `predlong 3`, seeds 31–34: died out on day | 309, 320, 303; one alive (3) | 330, 254; two alive (2, 5) |
| Same, 2 years, `Balance::morphology_off()` | 240; three alive | 185; three alive |

**Not the morphology.** With every morphology mechanic off (the rules
before *Morphology on by default*, bit for bit) the predators fall the
same way, so the extinction predates it; directional vision, depth for
strikes and the body plan effects are not the cause.

**The mechanism** (seeds 31–34, `predlong`):

1. *Predators live on plants.* Temporary feeding counters (not kept): about
   95% of a fox's or pack hunter's food is plants nibbled at 0.3 of a graze
   while courting, resting or drinking, 5% meat (as *Why founding hunters
   catch so little* found for founding packs). A predator catches 0.02–0.2
   prey a day; 92% of its hunting ticks are roaming with no prey in sight
   (half the voles are in their burrows; newts brumate in winter).
2. *Roaming eats nothing.* A carnivore above 0.7 that is hungry with no prey
   in sight roams under `Action::Hunt`, which, unlike every other errand,
   neither nibbles nor drinks on the way.
3. *Mothers roam, males court.* A pregnant female can't court, and one with
   young barely does, so when hungry she roams: pregnant and nursing
   females hunt 65–75% of the time at mean hunger 70–80 (thirst 40–47),
   while males court 40–70% of the time, nibbling, at hunger 37–49. Adult
   females die of starvation and thirst out of proportion (pregnant ones
   at 40% of them), and the adult sex ratio runs from 1:1 at founding to
   1:2–1:5 within the first year; the collapses followed end with males
   only (fox on seed 34: 11 F / 13 M on day 30, 5 / 22 on day 60, 0 / 15 on
   day 210, no birth after day 180 while 15–20 males still lived). Without
   any breeding (a probe) both sexes die at the same rate.
4. *Winter cuts the plants.* Starvation peaks in every winter (a third to
   half of the predators die in the first one); summers rebuild less each year once the
   herbivores fill to their caps, and with too few females the second or
   third winter ends the lineage.

**Levers tried**, 3–4 years on seeds 31–34 (survived: fox, pack hunters,
of 4; the prey count is the meadow voles at the end):

| Change | Survived | Note |
| --- | --- | --- |
| Defaults | 1, 2 (3 years) | |
| `carnivore_hunger_discount` 0.55 / 0.65 | 4, 3 (3 yr) / 2, 1 (4 yr) | 0.65: packs boom to 470, then crash to females 0 |
| `meat_per_kg` 8, `escape` 0.5, `search_image` 0, `stealth_effect` 0.2, `perception_range` 6 | 0–2, 0–3 | |
| `lactation` 1 / 0.5, `pregnancy_cost` 0.4 / 0 | 2, 4 / 1, 3 / 3, 3 / 4, 4 (2 yr) | still falling, still male-biased |
| No milk cost (probe) / a mother nurses only below hunger 60 or 40 (probe) | 4, 4 (2 yr) / 2, 2; 2, 3 | still falling |
| A female conceives only below hunger 40–60 (probe) | 0–2, 1–2 | |
| Pregnant live bearers at full speed (probe; 0.7 now) | 3, 0 | |
| **`roaming_forage` 70** (below) | **4, 4** | voles down to 0 and 2 on two seeds by year 3, newts 0 on one |
| `roaming_forage` 85; 75 and 80 with roaming drinking too | 3, 3 (3 yr); 1, 2 and 1, 2 (4 yr) | 85 still falling |
| Roaming forage for females only, at 60 / 85 (probe) | 4, 4 / 2, 2 (4 yr) | 60: sex ratio flips to 172 F / 4 M; voles, antelope and herd grazers die out on seed 31 |

Feeding the predators more (any forage or hunger lever) lets them
persist only by making them plant-fed generalists whose numbers no
longer follow their prey's, and then they wipe out the voles and newts
(apparent competition); fed less, they run out of females. No single
tunable keeps both predators and their prey over the years: the
predators' food budget (plants while courting, almost no meat, nothing in
winter) is a design problem, the one *Why founding hunters catch so
little* already named, now seen over the years rather than in the founding
phase. Candidates for that work: predators that live on meat (more prey
found, so their numbers follow prey), a winter food source for carnivores
(autumn fat, caching, carcasses), and something that ends a boom before
the prey do (territories or predator density in breeding).

**Decision 1: no default changes.** The standard sweep (11.60), compare and
tournaments are unchanged. `roaming_forage` (`Balance`, new, off at 100)
is the starving-roaming-forage rule of *Why founding hunters catch so
little*: a hunter roaming with no prey in sight nibbles plants at 0.3 of a
graze once its hunger is above it. It is kept as a tunable for that design
work: at 70 the sweep from seed 31 scores 11.46 (80: 11.51, 85: 11.68;
voles 114 / 123 / 214 mean alive against 380), every predator survives
120 days, and both predators survive 3 years on every seed, at the
voles' cost. Snapshots move to format version 25 (the new field; versions
10–24 read it as 100, `serde_util::before_v25`).

`cargo run --release --example predlong -- 3 31 32 33 34` prints the
diagnosis: per predator every 60 days (`--every`) head counts, births,
hunger, deaths by cause, adult females and males, kills by prey and the
action shares; per seed the adults' time budget by sex and state
(pregnant, mother of unweaned young) and deaths by sex, stage and cause;
the seeds each predator survived. `--set NAME=V` and `--off`
(`morphology_off`) change the rules.

### Predators that live on meat

The design work *Predators over the years* asked for: predators fed by
their prey (so their numbers follow it), a winter food source, and
something that ends a boom before the prey run out. Six rules, each a
`Balance` tunable that is off at 0, **all off by default**
(`Balance::carnivore_diet()` turns them on at the values below; `evo-sim
... --set NAME=V`, `predlong --meat`). They apply to obligate carnivores
(carnivory above 0.7: the red fox, the pack hunters and any such player
design), except carrion and the slower rot, which feed every meat eater:

| Tunable (tested value) | Rule |
| --- | --- |
| `carcass_keep` (0.75) | Carcasses rot a quarter as fast: tile meat loses 26% a day above 15 °C and 11% below (70% and 38% before), so one kill feeds the killer, its family and other scavengers for days. |
| `carnivore_fat` (60) | Gorging: meat beyond hunger goes into fat (up to 60 hunger points, times one plus fat stores) in any season, not only before a dormancy, and a carnivore that knows of a carcass keeps eating while it has room. Fat pays for hunger above 50 as for any animal. |
| `carcass_scent` (8) | A hungry carnivore with no prey in sight goes to the best carcass within 8 × (0.5 + nose) tiles before roaming, remembers its last kill (`forage_target`) and goes back to it, and a pregnant one goes to her mate's last kill; a roaming hunter also drinks where it passes water. Before, carcasses were found by sampling 8 random tiles, and roaming neither ate nor drank. |
| `carrion` (1) | A death other than predation or drowning leaves the body as a carcass (half for a starved one): winter deaths are the scavengers' winter food. |
| `provisioning` (1) | A fed father (hunger below 50, fat 5 or more) takes food to his hungry pregnant mate or dependent young in sight and feeds them from his fat at the nursing rate (the provider is engaged with the fed one, as a hunter with its prey, so the parallel apply keeps the sequential order). |
| `breeding_territory` (12) | A female doesn't breed while a pregnant female or an unweaned young of her own lineage is within 12 tiles: one litter at a time per territory, so each predator is limited by its own numbers. |

Snapshots move to format version 26 (the six fields; versions 10–25 read
them as 0, `serde_util::before_v26`, test
`version_25_snapshots_restore_without_carnivore_diet`). With the defaults
every world runs bit for bit as before (sweep 11.60, the same predlong).

**Over the years** (`predlong`, seeds 31–38, 96×96; survived = any alive,
viable = both sexes):

| Run | Red fox survived (viable) | Pack hunters survived (viable) | Prey lost |
| --- | --- | --- | --- |
| Defaults, 3 years | 1 of 8 (0) | 3 of 8 (0) | none |
| Defaults, 6 years | 0 of 8 | 0 of 8 | newts on 36 |
| `--meat`, 4 years | 6 of 8 (4) | 6 of 8 (6) | none (lowest: voles 75, newts 28, omnivores 61) |
| `--meat`, 6 years | 3 of 8 (2: 16 and 137 foxes) | 6 of 8 (6) | none (lowest: voles 34, newts 45, omnivores 78) |

Meat went from 8% of a fox's food (first year, defaults; temporary
counters, not kept) to about half of both predators' food over 4 years,
and the sex ratio no longer ends in males only where they persist (fox
1:1 and 1:1.7, pack 1:1.4 to 1:2.6 at year 6). Fathers' provisioning is
small in volume (under 1% of the predators' food), but in a 4-year run
without it the pack hunters survived on 4 seeds instead of 6.

**Why off by default.** The acceptance was both predators persisting in
most seeds over 4–6 years without prey loss. The pack hunters do; the fox
does not: once both live on the same prey (voles and newts), the pack
hunters, which also take the herd grazers and their carrion and so reach
150–275, exclude the fox on most seeds by year 6 (competitive exclusion
on a shared prey; it was masked before because both lived on plants).
At 120 days the rules cost a little: the standard sweep from seed 31
scores 11.42 (11.60; a fox and a pack lost once more each in the founding
year), `compare --seed 101 --adaptive vs` the old rules −0.26 (sequential
95% CI −0.55 … +0.03 after 48 seeds: not significant, not within ±0.3),
fox and pack survival 92% / 94% against 98% / 100%. Tournaments
(`calibrate tournament`, 90 days, sets 70001– and 80001–, 96 worlds each,
the same designs with the rules on and off): the top thermoregulation ×
birth class wins 24.0% and 24.0% with them (26.0% and 24.0% without), no
class moves by more than 3 points, and hunters' (carnivory ≥ 0.45) mean
final share is 0.071 / 0.089 against 0.070 / 0.091.

The genetics harness's ecology level on fresh seeds (`evo-sim genetics
--long --level ecology --years 6`, 4 worlds of 96×96, seeds 1–4; new
`--meat` option) says the same more sharply. Defaults: the fox dies out
on day 200–340 and the pack hunters on day 230–520 in all four worlds.
With the rules: the fox still dies out in all four (day 200–370); the
pack hunters live in all four (46–442 at year 6), and they press the prey
hard: voles down to 4 and 53 in two worlds, the newts gone in one (day
390). All strict genetics checks pass either way. The quick ecology run
(64×64, 2 years) loses the fox by day 60–80 either way and the pack
hunters by day 140 without the rules (4 alive at the end with them).

**What did not work** (3–6 years, seeds 31–38):

| Change | Fox | Pack | Note |
| --- | --- | --- | --- |
| Kept carcasses, gorging, scent and roaming to the last kill site only (3 yr) | 1 | 7 | the fox's kills are too few; males still court and nibble, females starve |
| Less nibbling for obligate carnivores (plants at half / a fifth while courting, resting, drinking), with the above | 5 (males only) / 0 | 6 / 0 | at a fifth almost nobody breeds (hunger stays above 70: drive doesn't build); meat was a fifth of a fox's food |
| Carrion added (3 yr) | 7 | 7 | pack hunters boom to 150 on herd carrion, voles 84 on one seed |
| Carrion and provisioning, no territory (4 yr) | 2 | 8 | packs 77–347, voles 0 and 24 on two seeds, omnivores 0 on one |
| Territory only once a lineage numbers 2× / 3× its founders (spares the founding year: compare −0.23 / −0.04) | 2 / 2 (6 yr) | 7 / 8 | packs 200–400; voles, newts or omnivores lost on 3 seeds |
| Territory radius × (0.5 + group tolerance) (6 yr) | 4 | 5 | voles lost on 2 seeds, newts on 1 |
| Up to two litters per territory | | | compare −0.28 (B better after 36 seeds) |

Left for the next round: what separates the two predators' niches (the
pack hunters are fox-sized: 0.95 against 1.0 kg, so they share prey; a
bigger pack hunter that lives on the herds, or a fox that eats more
plants and insects than an obligate carnivore, would let them coexist)
and the founding-year cost of the territory rule. LOD cohorts
(`world::lod`) don't model these rules.

### Predators in their niches

The next round *Predators that live on meat* left: separate the two
predators' niches, and add brakes so a predator can't hunt its prey to the
last animal. Decided up front, from real ecology: the pack hunters become
larger cooperative hunters living mainly on the herds, the red fox a small,
flexible mesopredator that eats fruit and plants in season as well as voles
and newts; predators switch away from prey that has grown rare (a type-III
functional response), and litters follow the mother's condition. The
behaviours are heritable: the `Balance` tunables are only global scales and
switches, and how strongly each animal does a behaviour comes from its
existing genes (no new genes, no species named in the simulation code), so
selection can tune them.

**The rules, gene by gene** (`world::apply`, `world::decide`):

| Rule (tunable) | What the genes set |
| --- | --- |
| All of the carnivore-diet rules | `meat_reliance`: from carnivory, 0 up to 0.45 (`HUNTER_CARNIVORY`), rising evenly to 1 at 0.7 (`OBLIGATE`) and above. Before, the rules applied in full above 0.7 and not at all below; a flexible hunter (the niche fox at 0.65: 0.8) now gets most of them, a burrowing omnivore (0.5) a fifth |
| Gorging (`carnivore_fat`) | capacity `carnivore_fat × reliance × (0.5 + fat stores)` (60 at fat stores 0.5; before `× (1 + fat stores)`) |
| Carcass scent (`carcass_scent`) | range `× reliance × (0.5 + nose)`; a flexible hunter off a good patch of plants follows its nose to a carcass too |
| Provisioning (`provisioning`) | care = the higher of pack instinct and group tolerance: urge and amount `× 2 × care`; any fed, non-pregnant adult provides, for its own pregnant mate or dependent young in full and, by its pack instinct, for the other pregnant females and dependent young of its kind (helpers at the den) |
| Breeding territory (`breeding_territory`) | radius `× reliance × (1 + aggression − group tolerance)`, clamped to 0.25–2: a lone, aggressive hunter holds a wide territory, a tolerant pack shares its range. **And a leak fixed**: a female in a taken territory only stopped courting, while males still courted and bred her; now she isn't courted either (`look`), and the territory really is one litter at a time |
| Prey switching (`prey_switching`, new) | a hunter gives up a prey species for the day with chance `min(1, 2 × pickiness) × (1 − r²)`, `r` = its head count over `prey_switching ×` its founding count (capped at 1); drawn once per hunter, prey lineage and day (a hash of the hunter's id, the prey lineage and the day), so it passes over all of that species, its clutches included (`raid_clutch`) |
| Litters follow condition (`condition_litter`, new) | a mother carries `1 − condition_litter × flex × (1 − nourished)` of her litter, `nourished = (90 − hunger + fat) / 60` (fat stores carry a hungry mother), `flex = min(1, 2 × litter fraction)` (the litter size gene: a big litter shrinks, a single young can't); the pregnancy's food cost follows the same share, so a starving mother resorbs young rather than starve |

**The designs** (`presets::niche_world`, `niche_fox`, `niche_pack_hunters`;
the standard designs are unchanged): the fox at carnivory 0.65 (0.85),
fangs 0.3 (0.5: long fangs crop plants badly), fat stores 0.5, group
tolerance 0.2, aggression 0.7; the pack hunters at body size 0.30 (1.1 kg;
0.22, 0.8 kg), carnivory 0.95 (0.9), fat stores 0.5 (39.5 points, 41.7
before). Carnivory 0.65 against 0.95 is a full body-plan gap (0.3): the
two neither interbreed nor count as one kind (with the old fox the brakes
let the fox absorb the pack hunters' genes: its group tolerance rose by
0.57 and its pack instinct by 0.68 over 6 years).

**Over the years** (`predlong --meat`, seeds 31–38, 96×96, 6 years;
survived = any alive, viable = both sexes at the end; every row has the
brakes on unless noted):

| Run | Fox survived (viable) | Pack survived (viable) | Prey lost |
| --- | --- | --- | --- |
| *Predators that live on meat* (v26 rules, old designs) | 3 (2) | 6 (6) | none |
| Fox 0.65; pack 0.45 (3 kg), no brakes | 7 (7) | 1 (1) | antelope, herds (2 seeds), newts |
| Fox 0.65; pack 0.45 | 7 (7) | 1 (0) | none |
| Fox 0.65; pack 0.36 / 0.40 | 8 (8) / 7 (7) | 2 (2) / 1 (1) | none |
| Same with helpers at the den, pack 0.36 / 0.45 | 8 (7) / 7 (7) | 2 (2) / 1 (1) | newts / antelope (1 seed) |
| Fox 0.26; pack 0.30 / 0.34 at carnivory 0.95 | 8 (8) | 2 (2) / 2 (1) | none / newts (1) |
| Fox 0.65; old pack (0.22, 0.9) / 0.22 at 0.95 | 8 (8) | 3 (3) / 2 (2) | newts (1) / none |
| Old fox and pack, brakes on | 3 (3) | 7 (7) | newts and burrowing omnivores (1) |
| Territorial fox (group tolerance 0.2, aggression 0.7); old pack / pack 0.28 at 0.95 | 8 (8) / 6 (6) | 2 (2) / 3 (3) | none / newts (1) |
| **Territory leak fixed**, old pack | 7 (7) | 7 (7) | newts (1) |
| **Territory leak fixed, pack 0.30 at 0.95 (the niche designs)** | **6 (6)** | **7 (7)** | **none** (lowest: voles 53, antelope 13, newts 10, omnivores 20) |
| Same, pack 0.34 / fox 0.28 | 6 (6) / 6 (5) | 6 (5) / 5 (5) | none |

The big pack hunter failed as the small one had in *Predators over the
years*: at 1.8–3 kg a vole or newt covers too little of its day, it rarely
brings down a grown antelope or grazer before its pack holds, and pregnant
and nursing females hunt 60–80% of the time at hunger 65–90 and starve
while the males court (sex ratio 1:3 within the first year). What made
both predators persist was the territory fix: with every predator limited
by its own numbers neither booms to exclude the other. In the niche run
the fox lives on voles (60% of its kills) and newts (36%) and plants; the
pack hunters kill newts (41%) and voles (30%) by count but herd calves
(17%) and antelope (7%) by weight. Pack hunters end at 43–291, foxes at
7–92 (two seeds near the edge: 7 and 8).

**Selection on the genes** (mean expressed value, start → end of 6 years,
mean over the surviving seeds): the pack hunters' carnivory falls on all 7
seeds (0.94 → 0.90, −0.038: above 0.95 a body eats no plants at all, and
the nibbling pays); the fox's carnivory falls slightly (−0.017, on one
seed 0.64 → 0.56) and its group tolerance and pack instinct rise (+0.04
each; on one seed by 0.15). Fat stores, litter size, pickiness,
aggression, nose and body size move by 0.01 or less on average, in no
consistent direction: no selection visible on the brakes' genes in 6
years (pickiness, which a hunter pays for in lost meals, is the one to
watch over eras).

**Why still off by default.** The long-run acceptance (both predators
viable on 6 of 8 seeds, no prey lost) is met by the niche designs with
the rules on; the 120-day sweep is not. `evo-sim sweep --size 96 --trials
1 --seeds 12 --rounds 1 --seed 31 --niches --meat` scores **10.88** (±0.62;
defaults 11.60; pack hunters survive on 10 of 12 seeds against 11): with
predators limited by territories and the fox partly on plants, the prey
boom in the first year (voles 705, newts 627 against 380 and 244) and the
newts and toxic frogs sit at their population caps on 37 of 120 days
(about 0.9 points of the sweep's cap penalty).
Tried at 120 days (12 seeds; capped days per seed): without the litter
rule 10.99, without prey switching 10.80, without territories 10.68 (44 capped
days, the foxes near their own cap), with the fox at carnivory 0.85 11.02, the old pack
11.10, founders exempt from territories 28 days (foxes 120, pack hunters 32), the new
designs under the old rules 43 days (pack hunters 11 alive). With the
standard designs the rules score 11.19 (`--meat`). The genetics harness's
ecology level on fresh seeds (`evo-sim genetics --long --level ecology
--meat`, 12 years, 4 worlds 96×96, seeds 1–4) passes every strict check
(7 of 7, 9 of 9 soft) but keeps both predators in only one world (fox 94,
pack hunters 225), the fox in one more (111) and the pack hunters in one
more (203); no prey lineage is lost (lowest at the end: newts 134). `compare
--adaptive` and the tournaments were not rerun: the defaults are unchanged
(the sweep still scores 11.60 bit for bit, so they are too).

Snapshots move to format version 27 (the two new tunables; versions 10–26
read them as 0, `serde_util::before_v27`, test
`version_26_snapshots_restore_without_predator_brakes`). The gene-scaled
rules, the territory fix and the helpers change only worlds that turn the
rules on. `Balance::carnivore_diet()` now turns on all eight;
`carnivore_diet_off()` turns them off. `evo-sim run|sweep|compare
--niches` founds the niche designs and `--meat` turns the rules on (both
for `predlong --meat` and `genetics --meat`; `predlong --meat-only` keeps
the standard designs). `predlong` also prints each predator's sexes at the
end, each prey's lowest count and the gene means above.

Left for the next round: the first-year prey boom (a founding-year grace
for territories spared the pack hunters but let the foxes boom), the
fragile fox on fresh seeds, and LOD cohorts, which don't model any of
these rules.

## Genetics validation

`evo-sim genetics` (`crates/sim-cli/src/genetics/`) checks over many
generations and replicate seeds that the population genetics behave as
theory predicts. Full results and the analysis are in
[`docs/genetics-validation.md`](docs/genetics-validation.md).

- **Genome level** (`wf.rs`): Wright–Fisher populations of fixed size on
  the real genome code (`Genome::inherit`, `mutate_morph`, dominance and
  expression, load loci, origin tags), isolated from the ecology. Strict
  checks with 99.9% intervals: heterozygosity decay `H₀(1 − 1/2N)ᵗ`,
  allele-frequency variance, fixation probability and exact Wright–Fisher
  absorption times, mutation–drift equilibrium (infinite-alleles
  heterozygosity, additive allele variance `(2N − 1)μσ²`), the stationary
  distribution of the step-mutated morphology counts and that no impossible
  value ever appears, mutation rates and step SD per segment, origin tags,
  F2 ratios (3:1 for the morphology loci, the contract rule for non-default
  pairs, 1:2:1 and `0.5 + 0.15h` for the dominance genes), offspring =
  midparent and Mendelian segregation variance, mutation–selection balance
  of the hidden recessives and inbreeding exposure, the legacy share as a
  martingale and its decay under owner-tagged mutation, and independent
  assortment of all 1711 locus pairs.
- **Ecology level** (`eco.rs`): standard worlds over many years, read-only
  between steps: mutation rates and tags of real births, valid morphology,
  heterozygosity, divergence from the founders and between map halves and
  hybridizing lineages, neutral-locus drift. Soft checks (direction and
  magnitude) plus strict invariants.

```sh
cargo run --release --bin evo-sim -- genetics                 # quick genome level (~5 s)
cargo run --release --bin evo-sim -- genetics --long --years 15 --out out/genetics   # both levels
cargo run --release --bin evo-sim -- genetics --long --level genome --only "recessive load"
cargo test --release -p evo-sim-cli -- --ignored long_    # the long runs as tests
```

It writes `report.md`, `checks.csv` and (ecology) `ecology.csv`, and exits
non-zero if a strict check fails. `cargo test` runs the quick genome level
and a 30-day world (`quick_genome_checks_pass`, `short_world_invariants`).

**Latest long run** (15 min on 4 cores: genome level 2000 drift replicates,
1M-child mutation counts, 400k-offspring linkage cross; ecology 4 worlds of
96×96 × 15 years, ~475,000 newborns inspected): **122 of 122 strict checks
pass, 9 of 9 soft checks pass.** No bug in the genome code was found (two
first-run failures were errors in the harness's own expectations, now
fixed). Highlights: heterozygosity decays as `(1 − 1/2N)ᵗ` within 1%;
fixation probabilities equal initial frequencies; mutation rates per segment
match the configuration in Wright–Fisher and in real world births (genes
0.00402 vs 0.004 over 27M alleles); no impossible morphology value ever
appears; F2 ratios are 3:1 under the contract dominance rule; the recessive
load sits at mutation–selection balance (q = 0.042 vs 0.043) and full-sib
mating exposes 5.8× as many homozygotes; legacy shares are martingales; all
1711 locus pairs assort independently. One latent property is documented,
not changed: the obedience and morphology loci draw from streams seeded by
the child's gene alleles, so genomes with identical gene alleles would share
those draws (0 of 559,676 litter-mate pairs did in the worlds). In worlds,
interfertile lineages diverge only without gene flow (antelope and herd
grazers, which hybridize, converge), and a 96-tile map is one gene pool per
lineage.

## Point costs

The costs in `design.rs` are calibrated with `examples/calibrate.rs`
(single-threaded, 96×96, 120 days, scored by mean legacy share after day 10):

- **Marginal value** (`marginal`, 6 seeds, 162 worlds): each of the four sample
  designs gets one change at a time (+0.4 of a trait, +16 founders, litter ±1,
  body size −0.2) in an otherwise standard world, with options rotated so each
  meets a different mix of rivals. A trait's price is about 30 points per unit
  of log legacy share it buys per unit of trait (half its mean over the four
  designs, half its best), the rate at which extra founders buy legacy. A
  second pass at +0.7 priced the threshold traits (flight needs wings > 0.5).
- **Tournament** (`tournament`, 60 worlds × 4 random designs that each spend
  the full budget): a regression of log share on points spent per trait, with
  world fixed effects, shows which traits are still under- or overpriced. It
  was run under the old costs, a first draft and the final draft (seeds 101+,
  201+, 301+; new seeds each time).
- **Body plan** (`--morph` on either mode): one option per morphology
  change in `marginal`, a random body plan per design in `tournament`, and
  the free-axis check; see *Body plan structures* below.

| Points per unit above default | |
| --- | --- |
| Leg length 25, wings 24, toxins 18, hooves 16 | Valuable in every design, or a threshold ability (flight, warning coloration) |
| Muscle 7, fangs 7, claws 6, foot width 6, swimming 6, burrowing 6 | Valuable in one niche |
| Camouflage 5, climbing 5, coloration 5, vocal 5, tail 4, scales 4, nose 4, eyes 4, ears 4, fur 3 | Small or mixed effects |
| Armor 2, fat stores 2, body size 0 | Cost more fitness than they bring; bigger bodies already mean fewer breeders |
| 0.7 per founder above 20, 2.5 per extra offspring | Founders were the best buy at 0.4; litter slightly underpriced at 1.5 |
| 40 per unit of body size below 0.3, 20 for cold-blooded | Free choices that won out of proportion (below) |

Under the new costs the clearest mispricings in the old table are mostly gone
(per-point coefficients: founders +0.055 → +0.031, leg length +0.049 →
+0.020, hooves +0.032 → +0.003, keen nose −0.035 → −0.012; leg length went
from 22 to 25 after the last run). Armor and fat
stores lose fitness at any price (their speed and agility penalties outweigh
defense and insulation over one year), so they keep only a floor price.

**Body plan structures (`calibrate --morph`).** Extra structures cost
points per structure; shapes, placement and snout length are free
(`docs/morphology.md` §4, decisions 1 and 11). Prices only matter when the
body plan has effects, so every run had the morphology tunables on at their
spec values (`--set morph_effects=1 --set morph_tail_drag=1 --set
morph_tail_chill=1 --set morph_long_legs=1 --set directional_vision=1`);
the defaults are untouched (another package decides when to turn them on).
Three measurements, 90 days on 96×96:

- **Marginal** (`marginal --morph`, seeds 1001–1008, 176 worlds, 32
  entries per option): each sample design gets one body-plan change at a
  time, paired with its own `control` on the same seed. Change in log mean
  legacy share (± 95% half-width, about 2 × the per-seed SE):

  | Option | Δ log share | Option | Δ log share |
  | --- | --- | --- | --- |
  | +1 eye (midline) | −0.015 ± 0.04 | tail thin (0.4) vs none | −0.034 ± 0.07 |
  | +2 eyes (extra pair) | −0.027 ± 0.03 | tail bushy | −0.023 ± 0.03 |
  | −1 eye (single eye) | −0.029 ± 0.05 | tail fin | −0.071 ± 0.05 |
  | +1 leg pair (6 legs) | −0.025 ± 0.04 | tail club | −0.033 ± 0.04 |
  | −1 leg pair (biped) | −0.097 ± 0.05 | tail prehensile | −0.002 ± 0.04 |
  | −2 leg pairs (legless) | −0.626 ± 0.13 | eyes 40° forward | −0.052 ± 0.05 |
  | +1 ear pair (4 ears) | −0.018 ± 0.05 | eyes 40° to the side | +0.006 ± 0.08 |
  | −1 ear pair (earless) | +0.001 ± 0.05 | snout −0.4 | −0.048 ± 0.05 |
  | ears pointed / long / fan | −0.011 / −0.008 / −0.008 | snout +0.4 | +0.018 ± 0.04 |
  | +16 founders (11.2 points) | +0.300 ± 0.06 | | |

  (The sample designs have no tail, so each tail row gives the design a
  0.4 tail of that shape against the tailless control; a shape's own effect
  is its row minus `tail thin`.) No extra structure buys legacy: by the
  README rule (30 points per unit of log share, half the mean over the four
  designs and half the best) an extra eye is worth about 0 points, a leg
  pair 0.1, an ear pair 0.7 (1.9 on final share). Their running costs (upkeep per eye and leg
  pair, leg mass, the 6-leg gait) eat what coverage, stability and
  localization bring in these worlds. Fewer structures lose: a biped −0.10,
  a legless body −0.63 (all four designs), one eye −0.03; earless is even.
- **Tournament** (`tournament --morph`, 384 worlds, 1536 entries; seeds
  20001–20096 and 30001–30192 under the spec's guesses 3/5/3, 40001–40096
  under the final prices): every random design also draws a random body
  plan (each count the default half the time, else uniform; shapes uniform;
  azimuth and snout uniform; half of them a tail of 0.1–0.8), paid for
  before the traits. The regression of *Point costs* (log final share,
  world fixed effects, controls, SEs clustered by world) gives per
  structure: extra eye +0.026 ± 0.048, extra leg pair +0.042 ± 0.095,
  second ear pair −0.087 ± 0.168; fewer: one eye −0.15 ± 0.25, biped
  −0.19 ± 0.21, legless −0.77 ± 0.21, earless +0.00 ± 0.22. All structure
  coefficients are within noise of zero, as in the marginal pass, and of
  the priced traits' median (+0.002 per point).
- **Free-axis fairness** (the 384 worlds; fair share 25%, limit 30%): win
  share by value of each free axis.

  | Axis | Win share by value |
  | --- | --- |
  | Tail shape (tailed, n ≈ 150 each) | thin 25.2%, bushy 33.3%, fin 28.4%, club 21.4%, prehensile 22.2% |
  | Ear shape (with ears, n ≈ 290) | round 24.3%, pointed 24.7%, long 26.1%, fan 23.6% |
  | Eye azimuth | 0–40° 23.6%, 40–80° 25.9%, 80–120° 22.8%, 120–180° 26.5% |
  | Snout length | 0–0.25 25.5%, 0.25–0.5 23.4%, 0.5–0.75 25.6%, 0.75–1 25.3% |
  | Fewer structures (free) | 1 eye 18.0%, biped 21.4%, legless 14.1%, earless 26.1% |

  Bushy tails crossed the line (33.3% ± 6.5). A paired re-run settled it
  (`tournament --swap-tail bushy=thin`, the same 192 worlds of 30001– and
  40001– with every bushy tail turned thin; worlds without a bushy entry
  replay bit-identically): the 94 tailed bushy entries win 34.0% with
  their bushy tails and 35.1% with thin ones, and their mean final share
  moves +0.003 ± 0.004 (warm +0.003 ± 0.005, cold +0.003 ± 0.006). The
  designs that drew bushy tails happened to be strong; the shape itself
  buys nothing, so it stays free. Tail fin is the free option that loses
  on land (−0.07 marginal, −0.20 for pack hunters), as a swimmer's shape
  should. The marginal pass agrees: no free change moves log share by more
  than ±0.07, against +0.30 for 11.2 points of founders.

**Prices.** Structures that cost more fitness than they bring keep a floor
price, like armor and fat stores, so that they are never free and "all
counts at maximum" still only adds to a design's cost:

| Structure | Spec guess | Price (points) |
| --- | --- | --- |
| Each eye beyond 2 (midline eye, extra pair) | 3 | **1** (`COST_PER_EXTRA_EYE`) |
| Each leg pair beyond 2 | 5 | **2** (`COST_PER_EXTRA_LEG_PAIR`) |
| Second ear pair | 3 | **2** (`COST_PER_EXTRA_EAR_PAIR`) |
| Fewer structures, tail and ear shapes, `eye_azimuth`, `snout_length` | 0 | 0 (no refund) |

The ear pair keeps 2 because its best case (burrowing omnivores, +0.15 on
final share) is the largest of the three. Fewer structures get no refund:
every one of them loses legacy or breaks even, so a refund would only
subsidize earless bodies. 6 eyes, 8 legs and 4 ears together cost 10
points. `design::morph_cost` charges them, `/api/genes` reports them in the
`morph` rows (`cost` per count step above the default: one eye, or one pair
of legs or ears) next to the new `cost_cold_blooded` and
`cost_per_smaller_body`, and the client's `designCost` charges the same
rows (`client/src/cost.fixture.json`, written by a server test, pins the
two together). The sample designs keep the default body plan, so their
costs and the standard sweeps are unchanged (prices never enter the
simulation). Not done yet: the derived-property audit of §4 (regressing
legacy on coverage, overlap, stability and localization).

**Dominant designs.** No trait combination dominates; what wins out of
proportion are free or cheap structural choices. Across the three 60-world
calibration tournaments, small (< 0.25), cold-blooded, omnivorous designs
with incubated eggs or live birth and big litters won far more often (a fair
share is 25% of worlds): cold-blooded 32–33% vs 16–18% for warm, small 37–43%
vs 13–21% for large, omnivores 29–46%, small omnivores 38–67%. Pricing cold
blood (20) and smallness (40 per unit) did not shrink the gap: points buy too
little to offset it without pushing the toxic frogs over budget. The legacy
score (breeding heads) stays as it is; the fix is in the rules.

Three fresh 48-world tournaments on the old rules (seeds 5000+, 9100+, 9200+,
`calibrate tournament --deaths`) located the causes. Cold blood was not a
consistent winner there (25% vs 25% pooled), but size and diet were: small
won 35% vs 15% for large, omnivores 40% vs 16–23% for the other diets, small
omnivores 53%. Paired re-runs of the same designs with one rule changed
showed what drives it. Small bodies field the most breeding adults (201 vs 64
for large per lineage). Omnivores (carnivory 0.5) were not food-limited: cutting
their food value to a quarter of each food cost carnivores twice as
much share as it cost them. They won on three side effects of
carnivory: the hunger discount for meat eaters (22% for an omnivore), fighting
back hard enough that hunters with `injury_aversion` 6 almost never attack
them (9% of omnivore deaths were predation vs 38% for herbivores), and
founding placement, where the crowding term counted only diets within 0.3 of
their own, so omnivores settled in the best grazing land without seeing any
competitors. The rule changes (tunables in `Balance`):

- `generalist_tax` 0.5: plants yield `1 - carnivory` and meat `carnivory` of
  their value, times `1 - tax · (4c(1-c))³` (`stats::generalist_factor`). An
  even omnivore gets a quarter of each food instead of half; carnivory 0.1
  keeps 98% and 0.85 keeps 93%. The same taxed meat share sets the meat
  eater's hunger discount, how hard prey fights back, food choice and
  founding food value. Founding now counts as competitors every diet that
  overlaps by more than 0.45 (`1 - |Δcarnivory|`), so omnivores see grazers
  and hunters alike.
- `size_burn` 0.25: hunger and thirst rise at `(adult kg)^-0.25` times the
  base rate, on top of mass^0.75 food per hunger point. Small bodies run hot
  and carry small reserves, so they need more per kg and starve or dry out
  sooner (a 0.36 kg body burns 2× as fast as a 6.3 kg one).
- `cold_heat_burn` 1.0: a cold-blooded body above full activity (22 °C) burns
  100% more per 10 °C (`apply::cold_blooded_thermo`). Cold blood stays cheap
  when the body is cool and sluggish and costs more where it is hot.

After, on the same 80 world-and-design sets (seeds 5000–5031 and 9200–9247),
every class is within a few points of fair:

| Win share | Before | After |
| --- | --- | --- |
| Cold / warm | 27% / 24% | 26% / 24% |
| Small / mid / large | 33% / 26% / 17% | 24% / 27% / 23% |
| Carnivory 0 / 0.1 / 0.5 / 0.85 | 20% / 16% / 37% / 25% | 21% / 24% / 26% / 28% |
| Small omnivores | 43% | 30% |

Mean legacy share moves the same way (omnivores 0.142 → 0.113, small 0.133 →
0.119, large 0.075 → 0.091), and no reverse dominance appears. The
ecosystem holds: the standard sweep (`sweep --size 96 --trials 1 --seeds 12
--rounds 1 --seed 31`) scores 10.96 (11.12 before). Survival per lineage is
unchanged or better (pack hunters 92% → 100%) except antelope (67% → 50%,
6 of 12 seeds) and herd grazers (92% → 83%); voles (100%, 217 alive) and
newts (100%, 214) barely move. Burrowing omnivores stay at 100% survival
but average 119 alive instead of 426; the toxic frogs (cold, carnivory 0.1)
are unchanged (100%, 448). Birth mode (incubated eggs winning 35–38%,
abandoned eggs 13–14%) was narrowed in the final balance pass and by egg
thermal risk (Balance section); re-measured on 240 worlds after the later
rules, incubated eggs no longer win out of proportion (27.7%, mean share
below live birth's; see *Birth modes, re-measured* in Balance). Doc counters that already work: warm-blooded designs with
abandoned eggs survive only 62% of the time (thermoregulation mismatch), and
herd grazers lose herd defense when shrunk below 0.45.


**Seasonal strategies** (`world::seasonal`, tunables `dormancy_temp` …
`cache_capacity` in `Balance`): the sample designs use them where they
clearly fit: voles cache, newts and toxic frogs brumate, burrowing omnivores
hibernate (and the extra Migrating caribou migrate, see Balance). Antelope were tried as migrators, but on both 12-seed sets
migration cost them survival (67% vs 75% and 50% vs 67%), mostly to
predators met on the autumn trek, so they stay year-round. Seasonal genes
are left out of the genetic distance (`genome::IN_DISTANCE`): counted, they
diluted it enough to make foxes and pack hunters kin. Appending genes still
reshuffles founder and mutation randomness, which alone moved a 12-seed score by up to
0.4 (seeds 43–54: 11.13 before, 10.70 with the new genes and no strategies
in use, 10.71 with the presets' strategies).

**Migration and winter ranges** (`migration_onset`). What does migrating
buy a design? Paired runs of the Migrating caribou with and without the
gene (`compare --migrants vs --migrant-gene migration=0`, same seeds and
rules; the gene does nothing else, so the worlds differ only by the herd's
journeys):

| Rules, horizon (seeds) | Score A − B | Caribou mean alive | Caribou legacy (A; A − B) |
| --- | --- | --- | --- |
| Old rules, 120 days (101–148, `--adaptive --tolerance 0`) | −0.04 ± 0.16 (sequential CI −0.23 … +0.15) | −34 ± 27 * | 11.22%; −0.13 ± 0.97 pp |
| Old rules, 240 days (101–124) | +0.18 ± 0.34 | −81 ± 34 * | 8.24%; −0.11 ± 0.93 pp |
| `migration_onset` 0.2, 120 days (101–148, adaptive) | −0.03 ± 0.17 (−0.23 … +0.18) | −28 ± 29 | 11.50%; +0.15 ± 1.00 pp |
| `migration_onset` 0.2, 240 days (101–124) | −0.01 ± 0.27 | −80 ± 36 * | 7.89%; −0.47 ± 0.99 pp |

(± is the 95% CI of the paired difference; * excludes 0; survival 100%
on both sides everywhere.) Migrating buys the caribou nothing: no legacy,
and fewer members. A diagnostic run (seeds 101–106, the herd's mean
latitude at mid-season, spring … winter; the summer range is at y ≈ 28 of
96, the winter range 27 tiles south) shows why:

- **Non-migrants winter in the south anyway.** Without the gene the herd
  sits at y 27.5 / 29.4 / 29.3 / 54.9: when its northern pasture stops
  growing (regrowth follows temperature, `terrain::growth_warmth`: none at
  or below 0 °C, full from 12 °C; the seasonal mean is 2 + 26 · latitude
  °C ± 12) hunger and pasture search walk it south in early winter, to
  the latitude a migrator heads for, with no journey risk. The winter
  range already has the better cold-season pasture: at the summer range
  the late-winter mean is about −3 °C (no growth, snow), at the winter
  range about +5 °C (40% of full regrowth). So a pasture-by-latitude rule
  would not give winter ranges a reason to exist; it is there already.
- **Migrants leave too early.** With the gene the herd sits at 26.6 /
  26.9 / 40.7 / 52.7: it sets off on the first day of autumn, while its
  summer pasture still grows, grazes its winter range down through autumn
  and spends the winter on less (standing forage under the herd in late
  winter 1.27 against the non-migrants' 1.90, hunger 35 against 28), and
  it pays the trek (148 of about 1,050 deaths on the journey).

Tried for winter-range pasture, neither kept: migrators choosing their
winter spot by the forage standing there rather than its capacity
(`veg_max`): no change (late-winter hunger 35.6 against 35.3, 6 seeds);
and day-length plant growth (±50%: faster in the northern summer, slower
in the northern winter, the reverse in the south), which feeds every
herbivore and still left non-migrants ahead (240 days, 6 seeds: 444 alive
against 361, legacy 9.9% against 9.7%).

The rule kept: **`migration_onset`** (0.2; 1 is the old rule): in autumn a
migrator leaves for its winter range only once plant regrowth at its
summer range (`growth_warmth` of the seasonal mean there) has fallen to
that share of the full rate, and in winter regardless, so the winter
range's pasture is saved for the cold season. The caribou now leave about
day 80 instead of 60 (mid-autumn latitude 29.7 instead of 40.7). Judged
with `compare --adaptive --tolerance 0` on seeds 101–148 with
`--migrants`, `--set migration_onset=0.2` against the old rule: score
+0.02 ± 0.10 (sequential CI −0.10 … +0.14, 48 seeds), no lineage's
survival or mean alive changed significantly, caribou +5 ± 14 alive and
+0.28 ± 0.54 pp legacy: harmless to non-migrants, and not enough to make
migrating pay for the caribou (table above). In tournaments it pays:
fresh seeds 70001–70096 and 80001–80096 (`calibrate tournament`, 90 days,
192 worlds, 768 entries), paired with `--set migration_onset=1`:

| Win share | 70001–, old | 0.2 | 80001–, old | 0.2 | Both (192), old | 0.2 |
| --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 19.7% | 19.7% | 23.1% | 26.2% | 21.4% | 22.9% |
| Cold incubated eggs | 31.7% | 31.7% | 29.6% | 26.8% | 30.6% | 29.1% |
| Cold live birth | 22.2% | 23.8% | 30.4% | 30.4% | 26.1% | 26.9% |
| Warm abandoned eggs | 7.8% | 9.4% | 4.5% | 4.5% | 6.1% | 6.9% |
| Warm incubated eggs | 35.8% | 35.8% | 27.0% | 28.6% | 31.5% | 32.3% |
| Warm live birth | 32.8% | 29.5% | 37.1% | 35.5% | 35.0% | 32.5% |
| Migrators (gene > 0.5; 81 + 64 entries) | 21.0% | 23.5% | 14.1% | 21.9% | 17.9% | 22.8% |

Designs that migrate gain significantly (mean final share +0.0064 ±
0.0015, one standard error; +0.0050 ± 0.0016 and +0.0080 ± 0.0026 on the
two sets); no class's mean share moves by more than about two standard
errors (cold live bearers +0.0029 ± 0.0013, the rest within one and a
half). A 90-day tournament ends with autumn, so most migrators no
longer leave inside it: what they gain is mostly the autumn trek they
skip. Pooled over both sets the classes span 6.9–32.5% (old rule
6.1–35.0%); on each set alone one warm-blooded class is above 32% with
either rule (warm incubated eggs 35.8% on the first, unchanged; warm live
birth 35.5% on the second, 37.1% before), the noise of one 96-world set
being about ±5 points (see item 4 of [Suggested next
steps](#suggested-next-steps)). The standard world has no migrators, so the
standard sweeps and the smoke check are untouched (12-seed sweeps 11.76
from seed 31 and 11.72 from 55, `scripts/balance-smoke.sh` 11.18 with 22 of
24 lineage-worlds), as is the client wire fixture. Snapshots move to format
version 16 (versions 10–15 restore with `migration_onset` 1, the old
rule). New tunable: `migration_onset`. Tooling: `compare --migrant-gene`
and the legacy column in `compare` (see *Judging scores statistically*
above).

**Warm-blooded live bearers: `lactation` (tried, left off).** Item 4 of
[Suggested next steps](#suggested-next-steps) named warm live bearers the
first suspect: 35.0% of wins on the tuning seeds 10001–10096 and 35.5%
on 80001–80096. What they turn into wins (`calibrate tournament
--deaths`, per entry, unchanged rules):

| Per entry (tuning seeds, 96 worlds) | Born | Died young | Adults at the end | Mean final share | Win share |
| --- | --- | --- | --- | --- | --- |
| Cold live birth | 296 | 38 | 136 | 0.088 | 25.9% |
| Warm incubated eggs (502 laid, 275 chilled) | 188 | 20 | 104 | 0.074 | 25.7% |
| Warm live birth | 357 | 30 | 171 | 0.106 | 35.0% |

They raise the most young of any caring mode, lose none as eggs, and
their young mature fastest (warm blood), so they end with the most
breeding adults and the highest mean share on every set (0.097–0.127).
Their wins come from large litters (4–12 young: 46% wins, against 6–13%
for litters of 1–3) of small herbivores, the vole's way. Nursing costs
them little: a mother's milk (1.2× the food her young gain) rarely
binds. The rule tried, **`lactation`**: after a litter a warm-blooded
live bearer conceives again only once her young are `lactation` of the
way to weaning (growth 0.7, when nursing and her guard end; 7–19 days),
on top of her recovery (lactating mammals don't come into heat).
Cold-blooded live bearers and egg layers are unaffected. A second
candidate, doubling a warm-blooded mother's nursing cost (a temporary
probe), changed nothing measurable on 80001–80096 (warm live births 406
→ 404 per entry, share +0.002 ± 0.004), so the nursing price is not
where their edge is. Paired with the unchanged rules (win share; mean
final share's paired difference, ± one standard error):

| Win share (share Δ) | 10001–, off | 0.5 | 1 | 70001–, off | 1 | 80001–, off | 1 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 31.0% | 31.0% (−0.000) | 33.8% (+0.007) | 19.7% | 15.2% (+0.004) | 26.2% | 24.6% (+0.008) |
| Cold incubated eggs | 26.4% | 30.2% (+0.001) | 28.3% (+0.007) | 31.7% | 27.0% (+0.003) | 26.8% | 31.0% (+0.009) |
| Cold live birth | 25.9% | 25.9% (+0.007) | 25.9% (+0.012) | 23.8% | 23.8% (+0.005) | 30.4% | 26.8% (+0.007) |
| Warm abandoned eggs | 8.3% | 11.1% (+0.003) | 6.9% (+0.004) | 9.4% | 7.8% (+0.002) | 4.5% | 6.0% (+0.001) |
| Warm incubated eggs | 25.7% | 21.6% (+0.000) | 25.7% (+0.009) | 35.8% | 41.8% (+0.008) | 28.6% | 27.0% (+0.008) |
| Warm live birth | 35.0% | 33.3% (−0.006 ± 0.003) | 31.7% (−0.010 ± 0.004) | 29.5% | 34.4% (−0.002 ± 0.004) | 35.5% | 35.5% (+0.003 ± 0.005) |

Judged on two fresh sets (designs unseen by any probe), `lactation` 1
against 0:

| Win share (mean final share) | 90001–90096, off | 1 | 100001–100096, off | 1 | Both (192), off | 1 |
| --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 22.0% (0.068) | 23.7% (0.074) | 26.2% (0.094) | 26.2% (0.102) | 24.2% | 25.0% |
| Cold incubated eggs | 40.7% (0.103) | 35.2% (0.117) | 25.9% (0.113) | 29.3% (0.120) | 33.0% | 32.1% |
| Cold live birth | 30.8% (0.088) | 27.7% (0.096) | 30.3% (0.086) | 28.8% (0.093) | 30.5% | 28.2% |
| Warm abandoned eggs | 6.2% (0.029) | 6.2% (0.032) | 9.6% (0.037) | 9.6% (0.041) | 7.8% | 7.8% |
| Warm incubated eggs | 28.3% (0.096) | 32.1% (0.112) | 34.2% (0.104) | 34.2% (0.109) | 31.7% | 33.3% |
| Warm live birth | 28.8% (0.097) | 31.5% (0.099; +0.002 ± 0.003) | 24.5% (0.101) | 22.4% (0.096; −0.004 ± 0.003) | 27.0% | 27.9% |

Over all 480 worlds of the five sets, `lactation` 1 cuts warm live
births by 15–17% per entry on every set, but their adults by only 5–14%
(fewer births
mean fewer of their young starve or die of thirst), and it slows every
warm-blooded live bearer in the world: the wild voles, antelope and
foxes, and so the very herbivores the winning designs compete with. Every
other class gains share (+0.003 to +0.009 ± 0.001–0.002) and warm live
bearers keep theirs (−0.002 ± 0.002): relative to the field they lose
about 0.01, but their wins don't move (30.8% → 31.5%) and the freed room
goes to incubators (warm incubated eggs 30.6% → 32.1%, 41.8% on
70001–). `compare --adaptive` from seed 101 (`--set lactation=1 vs
defaults`) stopped after 24 seeds at −0.07 (sequential CI −0.25 …
+0.12, within ±0.3), though herd grazers lost 227 ± 37 mean alive and
5.1 ± 1.6 pp of legacy; the 12-seed sweep from seed 31 scores 11.63
(11.76 with the rule off). Rejected: warm live bearers' wins don't
fall, and on 70001– it hands warm incubators 41.8%. `lactation` stays a
tunable at 0 (see *Warm-blooded classes: what the three levers showed*,
below).

**Endothermy upkeep** (`warm_upkeep`, off). Can the cost of being
warm-blooded itself pull the warm-blooded classes back, warm live bearers
first (item 4 of [Suggested next steps](#suggested-next-steps))? Paired
`calibrate tournament --deaths` runs on the tuning seeds 10001–10096 first
showed what warm live bearers turn into wins: more births and adults, not
winter survival (a 90-day tournament ends with autumn; their deaths are
starvation 35 and thirst 56 per lineage, exposure 0). Against cold live
bearers they have 357 born and 171 adults per lineage against 296 and 136,
from maturing 1.4× sooner (the doc's "growth: fast"). The edge sits in
small, large-litter herbivores: warm live bearers with body size below 0.3
win 52% of their entries, with litters above 3 46% (cold-blooded small
bodies: 30%). Their food need is already the doc's "high, constant" (a
warm body burns about 3–6× a cold one at rest), but food rarely limits
them inside 90 days. Four forms of the cost were screened on the tuning
seeds (win share; paired change of mean final share ± one standard error;
only the flat upkeep was kept as a tunable, the other three were
temporary code):

| Rule (tuning seeds, 96 worlds) | Cold abandoned / incubated / live | Warm abandoned / incubated / live | Warm incubated share | Warm live share |
| --- | --- | --- | --- | --- |
| None | 31.0% / 26.4% / 25.9% | 8.3% / 25.7% / 35.0% | 0.074 | 0.106 |
| Warm hunger ×1.15 (`warm_upkeep` 0.15) | 33.8% / 34.0% / 25.9% | 4.2% / 23.0% / 33.3% | −0.008 ± 0.003 | −0.006 ± 0.004 |
| ×1.2 (0.2) | 35.2% / 28.3% / 25.9% | 9.7% / 21.6% / 31.7% | −0.011 ± 0.003 | −0.011 ± 0.004 |
| ×1.4 (0.4) | 39.4% / 28.3% / 24.1% | 6.9% / 21.6% / 31.7% | −0.014 ± 0.003 | −0.013 ± 0.005 |
| Cold burn of a warm body 0.10 per °C below its comfort (was 0.04) | 26.8% / 28.3% / 33.3% | 11.1% / 21.6% / 33.3% | −0.003 ± 0.003 | −0.004 ± 0.002 |
| Small endotherms burn more: ×(2 kg / mass)^0.2 below 2 kg | 32.4% / 26.4% / 25.9% | 9.7% / 23.0% / 35.0% | −0.005 ± 0.004 | −0.005 ± 0.005 |
| Warm breeding drive only below hunger 40 (was 70) | 32.4% / 32.1% / 22.2% | 8.3% / 23.0% / 35.0% | −0.005 ± 0.003 | +0.004 ± 0.003 |

- **Cold burn and size** barely move warm live bearers: inside a 90-day
  tournament (spring to autumn) the cold term is small, and the heat-loss
  form spares no one in particular.
- **Breeding condition** does nothing to live bearers (their births are
  limited by mates and recovery, not by drive) and costs brooders, who
  can't eat on the nest.
- **A flat upkeep** is the only form that cuts warm live bearers'
  share, and it costs warm brooders as much; the wins go to the cold
  classes. `compare --adaptive` from seed 101 (`--set warm_upkeep=V vs
  defaults`) bounds it: 0.2 −0.04 (sequential CI −0.27 … +0.20, 12
  seeds, within ±0.3, though foxes −13 ± 10 and pack hunters −22 ± 15
  mean alive), 0.3 −0.39 (CI −0.74 … −0.05, 36 seeds, significant; pack
  hunters survive 89% against 100%), 0.4 −0.53 (significant). The cost
  falls on carnivores, whose food is hardest to find. The size form at
  0.2 also passes (−0.04, CI −0.17 … +0.08).

So 0.2, the largest upkeep `compare` allows, was judged on the fresh sets
(baselines are the rules off; 70001– and 80001– as in *Migration and
winter ranges*), paired with `--set warm_upkeep=0`:

| Win share (mean final share) | 90001–, off | 0.2 | 100001–, off | 0.2 | 70001–, off | 0.2 | 80001–, off | 0.2 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 22.0% (0.068) | 25.4% (0.070) | 26.2% (0.094) | 21.5% (0.091) | 19.7% (0.070) | 24.2% (0.072) | 26.2% (0.080) | 27.7% (0.085) |
| Cold incubated eggs | 40.7% (0.103) | 44.4% (0.107) | 25.9% (0.113) | 31.0% (0.117) | 31.7% (0.088) | 28.6% (0.087) | 26.8% (0.089) | 29.6% (0.089) |
| Cold live birth | 30.8% (0.088) | 33.8% (0.088) | 30.3% (0.086) | 30.3% (0.092) | 23.8% (0.072) | 27.0% (0.073) | 30.4% (0.082) | 26.8% (0.084) |
| Warm abandoned eggs | 6.2% (0.029) | 5.0% (0.026) | 9.6% (0.037) | 9.6% (0.036) | 9.4% (0.039) | 4.7% (0.031) | 4.5% (0.030) | 3.0% (0.026) |
| Warm incubated eggs | 28.3% (0.096) | 30.2% (0.101) | 34.2% (0.104) | 34.2% (0.100) | 35.8% (0.090) | 34.3% (0.080) | 28.6% (0.099) | 28.6% (0.096) |
| Warm live birth | 28.8% (0.097) | 20.5% (0.083) | 24.5% (0.101) | 24.5% (0.088) | 29.5% (0.099) | 31.1% (0.090) | 35.5% (0.127) | 35.5% (0.124) |

Pooled (paired change of mean final share ± one standard error):

| Win share (mean final share) | Fresh 90001– + 100001– (192), off | 0.2 | All four fresh sets (384), off | 0.2 | All five with tuning (480), off | 0.2 |
| --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 24.2% (0.081) | 23.4% (−0.000 ± 0.002) | 23.5% (0.078) | 24.7% (+0.001 ± 0.001) | 25.2% (0.079) | 27.0% (+0.000 ± 0.001) |
| Cold incubated eggs | 33.0% (0.108) | 37.5% (+0.004 ± 0.003) | 30.9% (0.097) | 32.9% (+0.002 ± 0.002) | 30.1% (0.094) | 32.1% (+0.002 ± 0.001) |
| Cold live birth | 30.5% (0.087) | 32.1% (+0.003 ± 0.002) | 28.8% (0.082) | 29.6% (+0.002 ± 0.002) | 28.3% (0.083) | 28.9% (+0.002 ± 0.001) |
| Warm abandoned eggs | 7.8% (0.033) | 7.2% (−0.002 ± 0.001) | 7.4% (0.033) | 5.6% (−0.004 ± 0.001) | 7.6% (0.033) | 6.5% (−0.002 ± 0.001) |
| Warm incubated eggs | 31.7% (0.101) | 32.5% (−0.000 ± 0.003) | 32.0% (0.098) | 32.0% (−0.004 ± 0.002) | 30.6% (0.092) | 29.7% (−0.005 ± 0.002) |
| Warm live birth | 27.0% (0.099) | 22.1% (−0.014 ± 0.003) | 29.8% (0.106) | 27.8% (−0.010 ± 0.002) | 30.8% (0.106) | 28.5% (−0.010 ± 0.002) |

The upkeep does what it aims at: warm live bearers' mean share falls
significantly everywhere (−0.010 ± 0.002 over all 480 worlds, to 0.096,
level with the best cold class), and warm brooders' less. But the
wins go to cold-blooded brooders, the top class of the fresh pair
already (40.7% on 90001–): pooled over the two new fresh sets they rise
from 33.0% to 37.5%, over all four fresh sets from 30.9% to 32.9%, and
warm abandoned eggs sink further below their intended ~10% (4.7% and 3.0%
on 70001– and 80001–). The upkeep makes the top pooled class worse
(30.8% → 32.1%), not better, and `compare` leaves it no room to grow
(0.3 is significant). Not adopted: **`warm_upkeep` stays 0** (the old
rule), as a tunable for later passes (it applies to level-of-detail
cohorts too).

**Warm-blooded brooding cost: no change.** Warm incubated eggs topped
70001–70096 (35.8%), so the cost of incubating for a warm-blooded parent
was checked: what she converts into wins, and whether a dearer brood
evens it. Diagnosis (`calibrate tournament --deaths`, with a temporary
per-clutch count of the hours the brooder sat on the nest, was off it, or
was dead; per lineage, 70001–70096 / 80001–80096):

| Per lineage | Warm incubated | Cold incubated | Warm live |
| --- | --- | --- | --- |
| Eggs laid | 641 / 801 | 387 / 391 | — |
| Chilled | 359 / 461 (56–58%) | 15 / 18 | — |
| Raided | 36 / 32 | 48 / 39 | — |
| Hatched or born | 230 / 237 | 268 / 252 | 363 / 406 |
| Died young | 41 / 21 | 34 / 33 | 43 / 45 |
| Clutch-hours with the brooder on / off / dead | 78–79% / 20–21% / 1% | 72–75% / 22–25% / 3% | — |
| Adults at day 90 | 116 / 145 | 127 / 122 | 162 / 172 |

Warm brooders already pay for warmth: they lay twice as often as cold
brooders (their incubation is 1.4× shorter) and lose more than half their
eggs to cold, under them or while they feed. What they convert into wins
is the young that do hatch: few die before breeding age (9–18%) and
nearly all their members are breeding adults, which is what the legacy
share counts; raids are rare (4–6%), because a brooder's clutch is exempt
from `nest_predation` and their nests are sparse. Their mean final share
(0.090 / 0.099) is below warm live bearers' (0.099 / 0.127) on both sets.

Tried on 70001–70096 (and 80001–80096), paired with the unchanged rules
(win share; paired difference of mean final share, ± one standard error):

| Change | Warm incubated | Warm live | Cold incubated | Note |
| --- | --- | --- | --- | --- |
| Clutch open to `nest_predation` while the brooder is off the nest or dead (as an unguarded one) | 35.8% → 38.8% (+0.002 ± 0.002) | 29.5% → 32.8% | 31.7% → 28.6% | raids +17 eggs per warm lineage: brooders' nests have few neighbors |
| A brooder on her nest sits tight (60% less urge to flee) | 35.8% → 35.8% (+0.001 ± 0.002) | 31.1% | 28.6% | hunters rarely reach brooders |
| `brood_cost` 2 (from 1) | 35.8% → 35.8% (−0.000 ± 0.003) | 32.8% | 27.0% | hatched 230 → 198 |
| `brood_cost` 3 | 35.8% → 34.3% (−0.012 ± 0.004) | 32.8% | 27.0% | hatched 230 → 169 |
| `brood_cost` 4 | 35.8% → 31.3% (−0.017 ± 0.004) | 36.1% (+0.009 ± 0.002) | 31.7% | 80001–: warm incubated 28.6% → 20.6%, warm live 35.5% → 35.5% |

Only `brood_cost` moves them (it costs warm brooders most: a cold one
warms her eggs by at most 4 °C), and what they lose goes to warm live
bearers, who compete for the same designs' worlds. `brood_cost` 3 (the
milder setting, smaller hand-over) was judged on every set, paired with
the current rules (fresh 90001–90096 and 100001–100096 never used
before; their baselines are the live-birth pass's runs, byte-identical
where both ran):

| Win share | 70001– | 80001– | 90001– | 100001– | Tuning 10001– | Fresh (192) | All (480) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Cold abandoned eggs | 19.7 → 19.7 | 26.2 → 26.2 | 22.0 → 16.9 | 26.2 → 27.7 | 31.0 → 29.6 | 24.2 → 22.6 | 25.2 → 24.2 |
| Cold incubated eggs | 31.7 → 27.0 | 26.8 → 29.6 | 40.7 → 37.0 | 25.9 → 29.3 | 26.4 → 30.2 | 33.0 → 33.0 | 30.1 → 30.4 |
| Cold live birth | 23.8 → 27.0 | 30.4 → 30.4 | 30.8 → 30.8 | 30.3 → 28.8 | 25.9 → 29.6 | 30.5 → 29.8 | 28.3 → 29.3 |
| Warm abandoned eggs | 9.4 → 9.4 | 4.5 → 7.5 | 6.2 → 8.8 | 9.6 → 9.6 | 8.3 → 9.7 | 7.8 → 9.2 | 7.6 → 9.0 |
| Warm incubated eggs | 35.8 → 34.3 | 28.6 → 22.2 | 28.3 → 30.2 | 34.2 → 31.5 | 25.7 → 21.6 | 31.7 → 31.0 | 30.6 → 27.9 |
| Warm live birth | 29.5 → 32.8 | 35.5 → 35.5 | 28.8 → 31.5 | 24.5 → 24.5 | 35.0 → 33.3 | 27.0 → 28.7 | 30.8 → 31.8 |
| Warm incubated mean share, paired | −0.012 ± 0.004 | −0.012 ± 0.005 | −0.007 ± 0.005 | −0.013 ± 0.003 | −0.016 ± 0.004 | −0.011 ± 0.003 | −0.012 ± 0.002 |

Over all 480 worlds warm incubators lose a significant share (0.092 →
0.080) and 2.7 points of wins, but the leading class is warm live birth
(30.8%), which gains (31.8%, mean share +0.0018 ± 0.0011), and on the
fresh pair the leader is cold incubated eggs (33.0%), which the lever
doesn't reach. `brood_cost` 3 also widens the mean-share spread at the top (0.106 → 0.108 for warm live birth) rather than
narrowing it. `compare --adaptive` from seed 101 (`--set brood_cost=3 vs
defaults`) stopped after 24 seeds at +0.06 ± 0.15 (sequential CI −0.12 …
+0.24): harmless to the score, though the burrowing omnivores (the warm
incubated sample design) lost 48 ± 17 members and 3.0 ± 1.0 points of
legacy. No rule or default changed. If a rule slows warm live bearers (who then cede wins to
warm incubators), `brood_cost` 3 is the measured counterweight on the
incubator side.

**Warm-blooded classes: what the three levers showed.** The lactation,
endothermy-upkeep and brooding-cost passes above share the fresh sets
90001–90096 and 100001–100096 and their baselines, and reach one
conclusion:

- **All three levers are negative.** Each moves wins from one class to
  another instead of evening them: `lactation` 1 slows every
  warm-blooded live bearer, wild competitors included, and the freed
  room goes to warm incubators; `brood_cost` 3 cuts warm incubators and
  hands their wins to warm live bearers; `warm_upkeep` 0.2 cuts warm
  live bearers and brooders and hands the wins to cold incubators. None
  is adopted: `lactation` and `warm_upkeep` stay tunables at 0 (the old
  rules) and `brood_cost` stays 1.
- **One 96-world set cannot carry a per-set target.** With about 60
  entries a class, a class's win share on one set has a standard error
  near 6 points, and the top of five such classes is inflated. Null
  simulations with the five classes other than warm abandoned eggs
  exactly equal show some class above 32% in 79–84% of 96-world sets
  (median top class about 34–35%), in 54% of 192-world sets and 26% of
  384-world sets. That is why a different class topped each real set
  under unchanged rules (warm live bearers on 10001– and 80001–, warm
  incubators on 70001– and 100001–, cold incubators at 40.7% on 90001–).
- **Pooled, the current rules meet the target.** Over the 480 worlds of
  the five sets 10001, 70001, 80001, 90001 and 100001 the win shares are
  cold abandoned eggs 25.2%, cold incubated 30.1%, cold live 28.3%, warm
  abandoned 7.6%, warm incubated 30.6%, warm live 30.8%: no class above
  31%, none collapsed, warm abandoned eggs near their intended ~10%. The
  acceptance rule in *Thermoregulation × birth mode* is now read on
  pooled fresh sets (see there).
- **Watch item.** Warm live bearers have the highest mean final share
  (0.106 against 0.092–0.094 for the other caring classes), from small,
  large-litter herbivores (litters of 4–12 win 46% of their entries). A
  future rule that slows them should be tested together with
  `brood_cost` 3, the measured counterweight on the incubator side, so
  their wins don't simply pass to warm incubators.

The standard sweeps are untouched by all three passes (12-seed sweep
from seed 31: 11.76, bit-identical), as is the client wire fixture.
Snapshots move to format version 19 (versions 10–18 restore with
`lactation` and `warm_upkeep` both 0). New tunables: `lactation`,
`warm_upkeep`.


**Fangs and scales after their contextual costs: prices kept.** The
tournament regression of *Point costs* (log final share on points spent
per trait, world fixed effects, controls for class, body size, diet,
litter and founders; standard errors clustered by world) on the four
fresh sets 70001–, 80001–, 90001–, 100001– (384 worlds, the same designs
before and after *Trait downsides*):

| Per-point coefficient | Before | After |
| --- | --- | --- |
| Fangs (7) | −0.009 ± 0.014 | −0.024 ± 0.014 |
| Scales (4) | −0.003 ± 0.023 | +0.002 ± 0.023 |
| Mean over the 31 priced traits | −0.034 | −0.038 |

Fangs lose about 0.1 log share per unit of fangs (0.015 per point × 7),
all of it on designs that eat plants; fanged hunters lose nothing
(carnivory 0.85: −0.001 ± 0.002 share). A lower price would subsidize
fangs for the hunters that don't pay the cost, and fangs still sit above
the mean of the priced traits, so **fangs stay at 7 and scales at 4**.

## Suggested next steps

In priority order; each item is open (done work is described in the
sections above).

1. **Mobile apps on devices.** The Android debug APK now builds headless
   with the command-line SDK and in CI (the `android` job uploads it as an
   artifact; see "Mobile apps" in [Client](#client)), the cleartext switch
   for http dev servers is checked in the built manifest, and the socket
   now recovers from a backgrounded app (silent-socket watchdog, `wake()`
   on resume). An emulator smoke run is in CI (the `android-emulator` job:
   login over the Server field to an http server with `EVO_CAP_CLEARTEXT=1`,
   founder design, map and state frames, 25 s in the background and back
   with the socket recovered; screenshots and logcat as artifacts). An iOS
   job on `macos-latest` builds the app for the simulator unsigned
   (uploaded as an artifact) and runs the same scenario in an iPhone
   simulator through Appium, with the ATS exception for http only in its
   `EVO_CAP_CLEARTEXT=1` build; it was written without a Mac, so its first
   CI runs are its first runs (expect to fix details there: the Xcode and
   simulator versions on the runner, Appium reaching the web view, the
   home button and resume). Still open: install the APK on a real Android
   device and play a session (login with a remote
   server over https and a LAN http server with `EVO_CAP_CLEARTEXT=1`,
   touch gestures, the bottom sheet, the onboarding tour, reconnects after
   backgrounding and network switches); play the same session on an
   iPhone (needs signing with an Apple Developer account);
   extend the emulator and simulator runs with what a device run shows matters (touch
   gestures on the map, the bottom sheet, a network switch with
   `adb shell svc wifi`/`data`, an https server).
2. **Push notifications on real devices.** Web Push works in browsers, and
   native push in the apps is implemented (FCM for Android, APNs for iOS;
   see [Native push in the apps](#native-push-in-the-apps)) and tested
   against mock services only. What remains is trying it for real: a
   Firebase project with `google-services.json` in an Android build and the
   server's service-account key, an APNs key and an iOS build with the Push
   Notifications capability and the `AppDelegate` hooks; then check on
   phones that Enable registers, an offline player's notices arrive (app in
   the background and closed), a tap opens the right world and tab, a newer
   notification of a world replaces the older, an uninstalled app's token is
   pruned, and the foreground presentation; and Web Push from an iOS
   home-screen web app.
3. **Capacity with several worlds.** Measured 1–8 512² worlds of 500
   players in one process on a quiet 4-core box (see [Three to eight
   worlds](#three-to-eight-worlds-where-one-process-stops-scaling)): the
   worlds' aligned clocks were the one serialization point (each world
   added 25–35 ms to every world's tick) and are now staggered; after that
   6 worlds hold every latency at its floor and 8 saturate the box (tick
   p99 0.3–0.4 s, command p99 0.4–0.5 s). Rule of thumb: 0.3 cores, 0.5 GB
   and 1.5 MB/s per world, at most 1.5 worlds per core (at 30–40k
   organisms; 0.4 cores and 1.25 per core at 60–70k). A 3-hour soak of
   6 × 500 ([Six worlds for three
   hours](#six-worlds-for-three-hours-soak)) ran without a disconnect,
   error or panic, but showed that 500 heavy players grow a 512² world to
   60–72k organisms, where 6 worlds saturate 4 cores (tick p99 ~1 s,
   commands p99 0.7–1 s, ticks still 1/s), and found the unbounded daily
   legacy list and daily history (both fixed). A 90-minute confirmation
   run of the same soak with jemalloc and the cheaper high-population tick
   (*Confirmation run* in that section) measured 0.4 cores and 0.4 GB per
   world at 60–70k organisms with every latency at or near its floor, RSS
   flat after the population peak (−77 MB/h, against +250–370 MB/h with
   glibc), no disconnect, error or panic; six worlds still saturate 4 cores
   at the 70–73k peak (tick p99 0.7 s, command p99 0.5 s), so the rule for
   worlds that full is now 1.25 worlds per core. Still open: the same runs
   on a bigger dedicated box (8–16 cores, load generator on another
   machine) to check that a process scales with its cores and whether a
   dedicated 4-core box holds six full worlds through the peak; a soak of
   a day or more to see the live heap level off once the 365-day daily
   window is full (this run ended on simulated day ~150).
4. **Birth-mode balance watch.** Against the target in
   *Thermoregulation × birth mode* ([Balance](#balance): no class above
   about 30–32% of tournament wins on fresh seeds) the classes spanned
   about 11–30% on fresh seeds 50001–60096 (`cold_gestation`,
   `nest_predation`). On two newer fresh sets (70001–70096 and
   80001–80096, with `migration_onset`; see *Migration and winter ranges*
   in [Balance](#balance)) they span 6.9–32.5% pooled, but each set alone
   has one warm-blooded class at 35–36% (warm incubated eggs on the
   first, warm live bearers on the second, with or without
   `migration_onset`), and warm live bearers reach 33% on the tuning
   seeds 10001–10096. Two more fresh sets (90001–90096, 100001–100096)
   put cold incubated eggs (40.7%) and warm incubated eggs (34.2%) on
   top. That is noise: with equal classes 79–84% of 96-world sets show
   one above 32% (54% at 192 worlds, 26% at 384), and pooled over the
   five sets (480 worlds) no class is above 31% (cold abandoned /
   incubated / live 25.2 / 30.1 / 28.3%, warm 7.6 / 30.6 / 30.8%), so
   the target is met; it is now judged on pooled fresh sets of 384
   worlds or more and on paired mean final share. Three levers were
   tried and left off, each only moving wins between classes:
   `lactation` (to warm incubators), `brood_cost` 3 (to warm live
   bearers) and `warm_upkeep` 0.2 (to cold incubators); see
   *Warm-blooded classes: what the three levers showed* in
   [Balance](#balance). Still watch warm live bearers, whose mean final
   share is the highest (0.106 against 0.092–0.094): test any rule that
   slows them together with `brood_cost` 3 as counterweight, and
   re-check on pooled fresh sets after rule changes that touch breeding.
5. **World-thread cost.** At high density decide is still most of a tick
   and is now bound by memory (neighbor genes and positions; see [High
   density](#high-density-decide-act-sweep-sub-step-states)); going further
   means changing results (cap or subsample the neighbors scanned, judged
   with `compare`) or packing the grid's genes smaller (bounds widened to
   match). The sequential act sweep is half what it was; splitting the rest
   by region is next if the apply becomes the bottleneck (it must stay
   deterministic and independent of the thread count; not attempted).
   Sub-step states now reuse the push's index and organism views with
   per-organism invalidation (push p50 −16%, sub-step p99 −22% at 500
   players, results byte-identical); what remains per state is copying what
   it contains, which only shared (`Arc`) state parts would avoid.
6. **Bandwidth and saves.** permessage-deflate would take another 2.8× off
   protocol 2 states, but the WebSocket stack (axum 0.7, tungstenite 0.24)
   doesn't offer it; revisit when it does (states are already ~1 KB/s a
   client, so this is low priority). (Saving the map's tiles as a delta
   against an earlier save was measured and rejected: 2% off a save; see
   *Tile deltas between saves* in [Persistence](#persistence).) Saves:
   eggs' and young organisms' genomes are now written against their
   parents' (world snapshot 18: −19 to −22% on days 3–5 of a 512² world,
   −27% on day 10, −37% at 128² on day 30; see *Genomes by parents*). What
   is left is founders' values, which no dictionary shrinks (measured); an
   entropy coder for residuals against each lineage's median would take
   about 1 MB more off a 512² save and isn't worth its complexity. Closed.
7. **Design-doc features not built.** Group commands as genes is built as
   the obedience gene, with a rally command (gather a group at a point for
   up to 3 days) for it to weigh beside migration and territory, and
   measured with scripted orders ([Groups and territory](#groups-and-territory)):
   obedient herds keep to orders better and are ruined by bad ones, but no
   scripted policy made high obedience pay, because standing orders cost a
   herd near its carrying capacity more than they give, and obedience
   hasn't drifted measurably in a year without orders. A rally now holds
   a large group less tightly (decided and done: its circle's cap is the
   largest territory's, 24 tiles instead of 12, which leaves a herd under
   a standing rally +92 ± 88 animals larger at day 120; see *Rally circle
   for big groups*). Follow-up: measure with real players' orders once
   there are some (the example's policies are guesses). (The seasons leaderboard is already served from cached
   standings: see *Cached standings*.) Offline protection is built in both
   forms ([Offline protection](#offline-protection)): the surviving core and
   the home range (a claimed territory shields its residents from hunters
   while the owner is away, from 10 minutes after logout, fading after 12
   hours). Both run on sensible defaults, not tuned: tune them with
   `compare --offline-owner` once there is player data on how often small
   lineages die offline and how players use territories. Duplicate mutations: the
   code already answers the doc's question as "separate tags per origin"
   (`genome`: a mutation carries the tag of the organism's owner when it
   appears, tags never change). Alleles are continuous, so two players never
   produce exactly the same allele and "credit whoever had it first" would
   need a new notion of when two alleles count as the same (a value bucket
   per gene and a world-wide registry of first appearances, touching legacy
   scoring and the snapshot format): a scoring-design change, not part of
   offline protection, left open until the design doc picks a rule.
8. **Tooling.** Done: `compare --adaptive` (sequential, Pocock-level
   looks until the CI excludes 0, fits within ±tolerance or hits
   `--max-seeds`) and `rankcheck` (40-day screens predict 120-day ranks at
   ρ ≈ 0.65 in a wide round, ≈ 0.31 in a narrow one), now with seed splits
   and simulated screened sweeps: with 12 seeds in two narrow rounds the
   12-seed full-run ranking reproduces at reliability 0.85 and 0.69, 40-day
   screens carry screen error of their own (true-score ρ 0.29 and 0.79),
   and from 60 days the screen ranks like the full run (≈ 1); so screen at
   60 days × 4 seeds with eta 3 and 12 finalist seeds (44% of the cost, no
   loss), never at 40 in narrow rounds (see *Noise or screen error?*).
   Also `compare --migrant-gene` with a legacy column (what a gene buys a
   design, paired), and with it: migrating buys
   the caribou nothing (legacy −0.13 ± 0.97 pp, mean alive −34 ± 27 over
   120 days; −81 ± 34 over 240), and `migration_onset` (leave once the
   summer pasture stops growing) helps migrating tournament designs but
   not the caribou (*Migration and winter ranges* in
   [Balance](#balance)). The winter range already has the better
   cold-season pasture; what keeps migration from paying is that
   non-migrants reach it too, walking south as their pasture freezes,
   without the journey risk (`migration_risk` applies to a migrator's
   trek only), while a migrating herd crowds one range latitude. Open:
   a rule that makes the gene pay against that drift (journey risk for
   long walks by anyone, or a winter-range spread that matches the
   non-migrants' grazing), judged with `compare --migrants vs
   --migrant-gene migration=0` over two years.
