# Evolution MMO

A persistent multiplayer world where each player designs a founding organism
and steers its lineage through breeding rather than direct editing. The full
design is in [`docs/game-design.md`](docs/game-design.md).

This repository is at **build step 1** of that design: prototype the
simulation rules (genetics, needs, terrain) headless, with no players attached,
and measure per-organism tick cost before building the server.

![World on day 45: player lineages (colored) and the wild ecosystem (pale = herbivores, dark red = predators)](docs/img/world-day45.png)

## Layout

```
crates/
  sim/       evo-sim: the rules engine (library, no I/O)
  sim-cli/   evo-sim binary: run worlds, print reports, benchmark, render PNGs
docs/
  game-design.md
```

### `evo-sim` modules

| Module | Design-doc section | What it does |
| --- | --- | --- |
| `genome` | Scoring, Genetics | 38 diploid genes (body, behavior *and* mate preference). Every allele carries an origin tag: founders get the designer's ID, mutations get the owner of the organism they appear in, and tags never change. |
| `design` | Organism design | Founder designs: thermoregulation → birth type → offspring count → traits, on a 100-point budget. Founders vary around the design. |
| `stats` | Stats layer, Derived physical properties | Phenotype → speed, strength, perception, stealth, metabolism… and height, weight, ground pressure, buoyancy, wing loading, grip. Thermoregulation mismatch (fur on a cold-blooded body blocks basking). |
| `terrain` | Seasons and biomes, Movement and terrain | Noise-generated biomes, latitude/altitude climate, four seasons, day/night, wind. Snow, ice bridges, plant regrowth, carcass decay. `traverse()` returns Blocked / Passable{speed, energy} from body vs. tile. |
| `mating` | Override cost | Root-sum-of-squares preference mismatch, `cost = k·max(0, mismatch−1)²`, outbreeding depression by genetic distance, and sterility between different body plans (thermoregulation, birth mode, body size or diet more than 0.3 apart; `cargo run --release --example distances` prints the founders' matrix). Display coloration is condition-dependent (an honest signal). |
| `group` | Critical-mass group traits | Daily regional carrier frequency → logistic benefit. Implemented: cooperative hunting, warning coloration, herd defense, alarm calls. |
| `legacy` | Scoring: genetic legacy | Share of origin-tagged alleles among living breeding adults, raw and weighted by offspring raised to adulthood. |
| `world` | Needs and behavior, everything else | The tick loop: utility-AI decisions (parallel), then movement, grazing, hunting, scavenging, drinking, courtship, pregnancy, eggs and incubation, nursing, growth, death. |
| `world::commands` | Player controls | Directed matings (consent, meeting distance, seasonal slots, override cost), possession, migration orders. |

### Tick structure

1. **Hourly:** weather, temperature, snow and ice, plant regrowth, water-distance field, egg incubation.
2. **Daily:** remove the dead, recompute group traits, take a legacy snapshot, reset mating slots at a season change.
3. **Decide (parallel, read-only):** each organism scores actions by need urgency × its behavior genes.
4. **Apply (sequential, shuffled):** act on the decision, update needs, resolve death and birth.

Every random draw comes from a `(seed, stream, tick, organism)` stream, so a
run reproduces exactly from its seed regardless of thread count (there is a
test for this).

## Running

Needs a recent stable Rust.

```sh
cargo test --release                                  # unit and world tests
cargo run --release --bin evo-sim -- designs          # sample designs and point costs
cargo run --release --bin evo-sim -- run --days 120 --report 10 --snapshot out/world
cargo run --release --bin evo-sim -- bench --scale 4 --days 3
cargo run --release --bin evo-sim -- sweep --trials 16 --seeds 3 --rounds 2 --out sweep.csv
cargo run --release --bin evo-sim -- run --set sprint=2.0 --set crowding=0.05   # try a balance tweak
cargo run --release --example trace -- 3 200          # follow one fox (lineage 3)
cargo run --release --example calibrate -- tournament --worlds 20   # point-cost check (CSV)
```

`run` founds a wild ecosystem (voles, antelope, newts, foxes) plus four sample
player lineages (pack hunters, herd grazers, toxic frogs, burrowing
omnivores), then prints population, legacy share, deaths by cause, active
group traits and gene drift per lineage.

## Benchmark (first measurement)

On a 4-core container, 128×128 map, ~1,700 organisms:

| Threads | ms per tick | µs per organism-tick |
| --- | --- | --- |
| 1 | 4.3 | 2.6 |
| 4 | 2.2 | 1.35 |

The single-thread figure falls within the design doc's 2–5 µs estimate. Only
the decision phase runs in parallel so far, which is why 4 threads give about
1.9×. Parallelizing the apply phase by region is the obvious next speed-up.
The later nearest-first perception scan brought the single-thread cost to
about 1.5–1.6 µs per organism-tick on the same benchmark.

## Balance

Ecosystem rates live in `Balance` (`crates/sim/src/config.rs`) and can be
overridden from the command line with `--set NAME=VALUE`. `evo-sim sweep`
tunes them by random search: each round perturbs every parameter of the
current best, runs every trial on several seeds in parallel, and recenters on
the winner with half the spread.

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
| **Current (12 unseen seeds)** | **11.3** | 100% | 83% | 100% | 100% | 92% | 92% | 100% | 100% |

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

**Still open:**

- Single worlds are chaotic: changing one parameter in the fourth decimal
  place can flip which marginal lineage survives. Judge changes by survival
  across many seeds, never one.
- Antelope and herd grazers are the weakest lineages now that every predator
  is viable (mean alive ~20 and ~40). Hybrids between those two
  near-identical designs still occur, as do fox × pack hunter hybrids, since
  those pairs share a body plan.
- A 12-seed, 120-day validation takes about 35 seconds on 4 cores.

Not yet implemented from the design doc: dominance and hidden recessives,
mating-transmitted disease, the other five group traits, seasonal strategies
(hibernation, migration, estivation as genes), level-of-detail statistical
populations, and A* pathfinding (movement is currently greedy steering with
obstacle avoidance).

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

**Dominant designs.** No trait combination dominates; what wins out of
proportion are free or cheap structural choices. Across the three 60-world
tournaments, small (< 0.25), cold-blooded, omnivorous designs with incubated
eggs or live birth and big litters win far more often (a fair share is 25% of
worlds): cold-blooded designs win 32–33% vs 16–18% for warm, small 37–43% vs
13–21% for large, omnivores 29–46%, small omnivores 38–67%. The legacy score counts breeding heads,
and a body that eats less fits more heads on the same food. Pricing cold blood
(20) and smallness (40 per unit) did not shrink the gap measurably: points buy
too little for a price to offset it without pushing the toxic frogs (97.4 of
100) over budget. The fix belongs in the rules, not the price list: weight
legacy by biomass or tie the cold-blooded metabolism discount to temperature
more steeply, and give omnivores a digestive penalty on both diets. Doc
counters that already work: warm-blooded designs with abandoned eggs survive
only 62% of the time (thermoregulation mismatch), and herd grazers lose herd
defense when shrunk below 0.45.


## Suggested next steps

1. **Slow breeders under predation:** antelope and herd grazers are now the
   weakest lineages (sub-adults and orphans are the main losses).
2. **Sweep robustness:** score trials on more seeds (single worlds are
   chaotic), and screen trials on short runs before giving the best full runs.
3. Parallel apply by region, then measure at the doc's prototype target
   (~25k organisms on 256×256).
4. Seasonal strategies as genes, then the remaining group traits.
5. Step 2 of the build order: the Tokio/Axum server around `World`, using
   `world::commands` for player actions.
