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

| | Sweep score | Foxes survive | Pack hunters survive | Burrowers survive |
| --- | --- | --- | --- | --- |
| Original rules and defaults | 6.5 | 0% | 0% | 75% |
| Fixed rules, tuned defaults (96×96, seeds 1–3) | 8.7 | 67% | 100% | 67% |
| Same, validated on 128×128 with unseen seeds 11–13 | 9.3 | 100% | 67% | 67% |

Most of the gain came from rule fixes the sweep's per-species stats exposed,
not from the numbers:

- Exhausted predators kept hunting instead of resting and starved at a crawl.
  They now rest first, and prey that isn't fleeing is easier to catch (ambush).
- Predators kept biting toxic frogs and dying. One poisoning by brightly
  colored prey now teaches a predator to avoid bright prey.
- Newborns were placed, and steps could end, in water too deep for the body
  (hundreds of voles drowned).
- Crowding raises hunger for organisms with many kin nearby, scaled by
  `1 - group tolerance`.

Tuning then raised the carnivore hunger discount, plant regrowth and movement
speed, and lowered thirst and the stealth effect.

**Still open:** toxic frogs reach their population cap in every run (nothing
eats them and they need little food, and crowding at its current strength
doesn't hold them back), and burrowing omnivores and pack hunters still die
out in about a third of worlds. A sweep takes about 30 minutes on 4 cores
(each 120-day run is ~75 CPU-seconds).

Not yet implemented from the design doc: dominance and hidden recessives,
mating-transmitted disease, the other five group traits, seasonal strategies
(hibernation, migration, estivation as genes), level-of-detail statistical
populations, and A* pathfinding (movement is currently greedy steering with
obstacle avoidance). The point costs in `design.rs` are placeholders, as the
doc lists them as an open question.

## Suggested next steps

1. **Faster sweeps:** screen trials on one short run and give only the best
   the full seeds and days; cache a species ID per organism instead of
   comparing genomes against every neighbor each tick.
2. **Balance:** density dependence for unpredated fast breeders, and lineage
   survival in the worlds where pack hunters or burrowers still die out.
3. Parallel apply by region, then measure at the doc's prototype target
   (~25k organisms on 256×256).
4. Seasonal strategies as genes, then the remaining group traits.
5. Step 2 of the build order: the Tokio/Axum server around `World`, using
   `world::commands` for player actions.
