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
  it for iOS and Android (not yet built on devices); see [Client](#client).

What remains is in [Suggested next steps](#suggested-next-steps).

![World on day 45: player lineages (colored) and the wild ecosystem (pale = herbivores, dark red = predators)](docs/img/world-day45.png)

## Layout

```
crates/
  sim/       evo-sim: the rules engine (library, no I/O)
  sim-cli/   evo-sim binary: run worlds, print reports, benchmark, render PNGs
  server/    evo-server: authoritative game server (Tokio + Axum, WebSockets)
client/      web client: TypeScript, PixiJS 8, Vite (npm)
docs/
  game-design.md
```

### `evo-sim` modules

| Module | Design-doc section | What it does |
| --- | --- | --- |
| `genome` | Scoring, Genetics | 46 diploid genes (body, behavior, mate preference and seasonal strategy). Every allele carries an origin tag: founders get the designer's ID, mutations get the owner of the organism they appear in, and tags never change. Expression is additive except for a per-gene dominance coefficient `h` (`expressed = mean + h·|a−b|/2`): coloration and fur are partly dominant, camouflage partly recessive. Four hidden load loci carry harmful recessives that arise by mutation, pass with their origin tags (and count for legacy), do nothing in carriers and harm homozygotes, so inbreeding exposes them and outcrossing hides them; `Genome::set_load` plants one in a founder or stud. Seasonal genes and load loci are left out of the genetic distance. |
| `design` | Organism design | Founder designs: thermoregulation → birth type → offspring count → traits, on a 100-point budget. Founders vary around the design. |
| `stats` | Stats layer, Derived physical properties | Phenotype → speed, strength, perception, stealth, metabolism… and height, weight, ground pressure, buoyancy, wing loading, grip. Thermoregulation mismatch (fur on a cold-blooded body blocks basking). |
| `terrain` | Seasons and biomes, Movement and terrain | Noise-generated biomes, latitude/altitude climate, four seasons, day/night. Rivers carved by flow accumulation down a pit-free drainage network to lakes, sea or map edge; depressions fed by more rain than evaporates off their surface fill into lakes (priority flood). Rivers in flood overflow onto their floodplain (low land up to two tiles from the channel): standing water that swimmers cross and that strands and drowns small non-swimmers, drowned plants, then silt that speeds regrowth for weeks. Regional weather on a 16-tile grid: fronts drifting east bring wind, storms (chill, costly open ground, costly and risky flight) and rain or snow; the wind has a direction (prevailing westerly, swinging over days and veering regionally) that speeds or slows flyers and gliders by wing loading; slow dry and wet spells and the seasons drive a per-cell soil wetness that fills and drains shallow lakes, marsh and rivers (droughts dry them out, crowding organisms at the rest) and paces plant regrowth; spring snowmelt swells rivers so fast deep water blocks weak swimmers, and rivers freeze in winter. Canopy browse needs height or climbing, and climbers in canopy are safe from ground hunters. Tiles update chunk by chunk, each chunk once per simulated hour. Hooks: `Map::weather_at`, `Tile::wetness`/`Map::dryness_at`, `Map::drought`/`flood`, `Map::terrain_version`/`chunk_version`; `Map::climate_temp`/`climate_wet` are climate anomalies (0 normally) that catastrophes set. `traverse()` returns Blocked / Passable{speed, energy} from body vs. tile. |
| `pathfinding` | Movement and terrain | Bounded A* over tiles for one body: blocked tiles are avoided, slow ones cost time. Used only when the direct step is blocked (see Movement below). |
| `mating` | Override cost | Root-sum-of-squares preference mismatch, `cost = k·max(0, mismatch−1)²`, outbreeding depression by genetic distance, and sterility between different body plans (thermoregulation, birth mode, body size or diet more than 0.3 apart; `cargo run --release --example distances` prints the founders' matrix). Display coloration is condition-dependent (an honest signal). |
| `group` | Critical-mass group traits | Daily regional carrier frequency → logistic benefit. Implemented: cooperative hunting, warning coloration, herd defense, alarm calls, shared burrows, sentries, communal dens, group navigation, shared caches. |
| `legacy` | Scoring: genetic legacy | Share of origin-tagged alleles among living breeding adults, raw and weighted by offspring raised to adulthood. |
| `species` | Genetics (speciation), Scoring (species founded) | Species detection: every few days, single-linkage clusters of sampled breeding adults that would recognize each other as kin and be fertile together (`mating::is_kin` and `fertility` ≥ 0.5), with stable ids, splits, merges and extinctions. See [Species, eras and achievements](#species-eras-and-achievements). |
| `chronicle` | Scoring (seasonal leaderboards, secondary achievements) | Eras with final legacy rankings and niche leaders, catastrophes (severe winter, megadrought, epidemic, flood year) and prestige-only achievements. |
| `world` | Needs and behavior, everything else | The tick loop: utility-AI decisions (parallel), then movement, grazing, hunting, scavenging, drinking, courtship, pregnancy, eggs and incubation, nursing, growth, death. |
| `world::disease` | Genetics: picky vs flexible | Mating-transmitted disease: pathogens emerge at matings (4× between founding lineages), pass between mates (natural and directed) and between nursing mothers and young, drain the acutely sick's health (dulling their display) and cut fertility; infections end in immunity or lifelong symptomless carriage. Also deaths of homozygotes for harmful recessives. Reports count infections, current sick and carriers, harmful alleles per organism, and `disease` / `recessive` deaths per lineage. |
| `world::seasonal` | Seasons and biomes (strategies) | Seasonal strategies as genes, each switched on above expression 0.5 and stronger the higher it is: hibernation and brumation (fatten in autumn, dormant through the cold with near-zero hunger and thirst, no breeding, den findable; thermoregulation mismatches per the affinity table), migration (summer range ↔ winter range further south, death risk per tile), estivation (dormant through summer heat or a regional drought, `Map::dryness_at`), food caching (raidable hoards for winter), short-term torpor on cold nights, a seasonal coat (winter coat molted in autumn and shed in spring, food per molt; a mismatch on cold-blooded bodies) and timed breeding (a narrow early-spring window with strong drive and better conception, no breeding outside it). |
| `world::par_apply` | Scale and performance (parallel regions) | The apply phase on all cores (`WorldConfig::parallel_apply`, default on; `--sequential-apply` for the old fully sequential apply): each turn split into a parallel movement sweep, a sequential interaction sweep in shuffled order, a parallel needs sweep and a sequential death/birth sweep. See [Parallel apply](#parallel-apply). |
| `world::commands` | Player controls | Directed matings (consent, meeting distance, seasonal slots, override cost), possession, migration orders. |
| `world::groups` | Player controls (group commands) | The rest of the doc's group commands: split a herd or pack into groups (up to 4) with their own orders, merge them back, claim or abandon a territory that a group roams, forages in, walks back to and defends. See [Groups and territory](#groups-and-territory). |
| `world::chronicle` | Scoring | The daily hook: samples adults (cohorts included) for species detection, plans, applies and ends catastrophes through existing systems (`Map::climate_temp`, `Map::climate_wet`, infections), closes eras and awards achievements. |
| `world::lod` | Scale and performance (level of detail) | Opt-in (`WorldConfig::lod`, `--lod`): full agents only near focus points (`World::set_focus`) and possessed organisms; elsewhere each 16-tile region holds per-lineage statistical cohorts (counts by age and sex, a genome pool, condition) that eat the region's plants, hunt, breed, die and migrate once a day, and expand back into agents when a focus arrives. Legacy and caps count them. See [Level of detail](#level-of-detail). |

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
  it. A group has a migration order or a territory, never both: the newer
  order replaces the other.
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
  until hunger passes 80 (then it forages anywhere, "hungry organisms
  prioritize food over everything except immediate danger"), chasing is
  at a sprint, and a group can claim only once every half day
  (`claim_cooldown`). Seasonal migration (a gene) still outranks the
  territory, and danger and thirst outrank both as usual.
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
cargo run --release --bin evo-sim -- sweep --trials 24 --seeds 12 --screen-days 40 --screen-seeds 4 --eta 4   # screened sweep
cargo run --release --bin evo-sim -- compare --seed 31 --seeds 24 --set larval_competition=18 vs defaults   # is A better than B?
cargo run --release --bin evo-sim -- run --set sprint=2.0 --set crowding=0.05   # try a balance tweak
cargo run --release --example trace -- 3 200          # follow one fox (lineage 3)
cargo run --release --example stuck -- 120 1 2        # how often organisms get stuck
cargo run --release --example calibrate -- tournament --worlds 20   # point-cost check (CSV)
cargo run --release --example deaths -- 1,5 120 31 32 33   # who kills antelope and herd grazers, and when
cargo run --release --example predators -- 120 1 2 3     # pack hunters vs their prey: booms, kills by prey rarity
cargo run --release --bin evo-sim -- sweep --trials 1 --seeds 12 --rounds 1 --seed 31 --migrants   # with the migrating caribou
cargo run --release --bin evo-sim -- run --size 256 --focus 64,64 --focus 192,192   # level of detail
cargo run --release --example lod -- 256 90 1 64,64 192,192   # LOD vs full simulation
cargo run --release --bin evo-sim -- run --days 361 --report 30 --catastrophes --catastrophe severe_winter@210   # eras, species, a catastrophe
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

`.github/workflows/ci.yml` runs on every push and pull request, in four
parallel jobs:

| Job | What it runs |
| --- | --- |
| Rust | `cargo fmt --all --check` (`rustfmt.toml`: width 120), `cargo build --release --workspace`, `cargo test --release --workspace` with a `postgres:16` service container (`DATABASE_URL` set, so the persistence tests also run against `PgStore`; `EVO_REQUIRE_DATABASE=1` makes them fail rather than skip if it goes missing), `cargo clippy --release --all-targets -- -D warnings`, and clippy on `evo-server --no-default-features` (no sqlx) (stable toolchain, cargo cache) |
| Balance smoke check | `scripts/balance-smoke.sh`: `evo-sim sweep --size 64 --trials 1 --seeds 3 --rounds 1 --days 60` with the default `Balance`, failing if fewer than 14 of the 24 lineage-worlds (8 lineages × 3 seeds) still hold 4 members with 2 adults, or the score drops below 7 of 12. The current rules make 21 of 24 and 10.4, so only a rule change that wrecks coexistence trips it; thresholds are env-overridable (`MIN_SURVIVORS`, `MIN_SCORE`, `SEEDS`, `DAYS`, `SIZE`). |
| Client | `npm ci`, `npm run typecheck`, `npm test`, `npm run build` |
| End-to-end | builds `evo-server` and the client, installs Playwright and Chromium (CI only; developer machines have them pre-installed), runs `npm run e2e` and uploads the screenshots as an artifact when it fails |

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
season, default 60; see [Ranked seasons](#ranked-seasons)), `--fresh` (ignore the saved world: new
world, stored designs re-founded; see [Persistence](#persistence)), `--no-push`,
`--vapid-private-key KEY` / `--vapid-subject mailto:...` (or `EVO_VAPID_PRIVATE_KEY` /
`EVO_VAPID_SUBJECT`), `--push-per-hour N` and `--push-allow-any-endpoint` (see
[Push notifications](#push-notifications)), `--hash-concurrency N` /
`--hash-queue N` (or `EVO_HASH_CONCURRENCY` / `EVO_HASH_QUEUE`: password
hashes run at once, default half the cores and at least 1, and requests
allowed to wait for one, default 16 per concurrent hash; see Accounts under
[Architecture](#architecture)).
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
  accounts and sessions, the rate limiters, the per-player connection cap
  (8 over all worlds) and the store. Every world is loaded before any
  starts, so a world that can't restore stops the start-up.
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
                                   saver thread (bincode + LZ4)
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
  since start (up to 365) with each lineage's births, deaths by cause and
  mean gene values, and raises per-player alerts: deaths of founders and
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
| `GET /api/genes` | | Settable genes with default and point cost, the budget and founder/offspring prices |
| `GET /api/world` | | Clock, season, weather, map size, organism and lineage counts |
| `GET /api/leaderboard` | | Legacy shares from the latest daily snapshot: `{day, breeding_adults, rows: [{player, name, share, weighted_share, alive}]}` |
| `GET /api/leaderboard/history?limit=30` | | The stored daily records (legacy shares, lineage sizes, daily births and deaths by cause, player lineages' mean genes) |
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
| `GET /api/push/key` | | `{enabled, public_key?, kinds}`: whether the server sends push notifications, its VAPID public key (base64url, the browser's `applicationServerKey`) and the push kinds |
| `POST /api/push/subscribe` | Bearer | A browser's `PushSubscription.toJSON()` (`{endpoint, keys: {p256dh, auth}}`) plus an optional `label` (default: the user agent): stores it for the account, replacing one with the same endpoint. `{endpoint, created_unix, label?}`; `400 bad_subscription` (endpoint not a known push service, keys not a P-256 point and a 16-byte secret); `503 push_disabled` |
| `POST /api/push/unsubscribe` `{endpoint}` | Bearer | Forget one of the caller's devices: `{removed}` |
| `GET /api/push/subscriptions` | Bearer | The caller's devices, oldest first: `{subscriptions: [{endpoint, created_unix, label?}]}` (keys stay on the server) |
| `GET /api/push/prefs` | Bearer | The push kinds the caller switched off, `{off: [kind]}` (`mating`, `death`, `market`, `species`, `season`, `catastrophe`); `{off: []}` until saved |
| `POST /api/push/prefs` | Bearer | Replace them; the reply is normalized (sorted, no duplicates); an unknown kind is refused (422) |
| `GET /api/map.png?scale=4` | | PNG of the world (`render_rgb`, 1–8 px per tile, lowered to keep a side within 2048 px), for debugging |
| `GET /api/metrics` | | Server health (`metrics.rs`): world-thread timings over the last 256 samples as `{n, p50, p99, max}` ms (`tick_ms`: step and bookkeeping; `states_ms`: one state push to every connection; `substep_ms`; `apply_ms`), `ticks`, `conns`, `state_pushes`, `applied`, `max_queued`, `dropped_conns`, `skipped_states`, the last save (`saves`, `save_capture_ms`, `save_encode_ms`, `save_write_ms`, `save_bytes`, `snapshots_pending`), WebSocket traffic (`bytes_out`, `msgs_out`, `state_bytes`, `states_out`, `ws_opened`, `ws_closed`; `state_encode_ns`, the writers' total time encoding states; protocol 2's `delta_conns`, `keyframes_out` and `resyncs`), the process's `cpu_seconds` and `rss_bytes` (Linux), and password hashing over the whole process, `hashing`: `concurrency` and `queue` (the bound), `in_flight` and `waiting` now, `peak_in_flight` (never above `concurrency`), `hashes` done, `rejected` (refused as busy) and `mean_ms` per hash |
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

Server → client:

| `type` | When |
| --- | --- |
| `welcome` `{player, name, lineage, proto?}` | On connect; `proto: 2` when the connection speaks protocol 2 (absent for protocol 1) |
| `state` | On connect, then every state interval (default 1 s): `clock` (tick, day, year, hour, season, daytime, era, era_days), `weather` (mean wind, storm, drought and flood shares, `catastrophe` while one is under way), `lineage`, `slots_left`, `population` (lineage size, cohort members included), `migration` (the main group's), `groups` (yours by id, the main group always: `{id, members, adults, migration?, territory?: {x, y, radius, claimed_tick}, max_radius, claim_ready_tick?}`), `view`, `organisms` (yours: id, lineage, position, action, sex, age, growth, health, needs, possessed, pregnant, generation, `look`, `group` when not 0; when possessed also `speed`, tiles per tick toward an order on open ground (`World::move_speed`), and `order` `[x, y]` while it has one), `nearby` (others in the window, also with `look`), `leaderboard` (cut to the top 50 rows plus yours and those of the owners in `nearby` when longer), `offers` (your open offers, with `listing?` and `escrow` when bought from a listing or priced), `balance` and `escrow` (your renown, and how much of it is held on open agreements) |
| `result` `{id, command, ok, error?: {code, message, detail?}, data?}` | After each command. Codes include the sim's `not_found`, `not_owned`, `no_consent`, `not_mature`, `same_sex`, `already_pregnant`, `too_far`, `no_slots_left`, `possessed_by_other`, the group commands' `no_such_group`, `group_limit`, `group_too_small`, `not_in_group`, `same_group`, `no_territory`, `claim_cooldown` (`detail.ticks_left`), `too_many_organisms`, and `incompatible_body_plans` (`detail.genes`: the differing body-plan genes), `genetically_incompatible`, `died`, `bad_message`, `bad_coordinates`, `unknown_offer`, `offer_closed`, `too_many_offers`, `busy`, `rate_limited`; the marketplace's `insufficient_funds` (`detail`: `{balance, needed}`), `bad_price`, `bad_expiry`, `already_listed`, `too_many_listings`, `unknown_listing`, `own_listing`, `own_organism`, `listing_busy`, `organism_busy`, `auction_listing`, `not_auction`, `bid_too_low`, `auction_closed`, `auction_has_bids`, `bad_duration`, `bad_cursor` |
| `mating_request` `{offer}` | Someone asks to mate with your organism |
| `mating_update` `{offer, outcome?, error?}` | An offer changed status (to both sides) |
| `alert` `{alert: {tick, day, kind, text, organism?}}` | `kind`: `daily_summary`, `birth` (first offspring, new generation), `death` (founder or possessed organism), `milestone`, `need` (possessed organism parched or starving), `world` (a catastrophe began or ended, an era ended with your rank), `achievement` (species founded, longevity, niche dominance, survived a catastrophe), `market` (someone bought a mating with your listed organism, a sale paid to you, a price paid or refunded from your escrow, a listing closed by expiry or death, an achievement's renown bonus, a bid on your auction, outbid and refunded, an auction won, sold, unsold or failed) |
| `session_ended` `{reason}` | The session expired; the socket closes next |

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

`look` (`appearance.rs`) is the visible body plan of an adult, computed from
the phenotype with a few comparisons per organism per push: `size` (0 tiny
< 1 kg, 1 small < 8 kg, 2 medium < 40 kg, 3 large), `diet` (herbivore /
omnivore / carnivore by carnivory), `warm` (endothermy), `locomotion` (walk,
swim at swimming ≥ 0.6, glide, fly), `cover` (skin, fur, scales, armor: the
strongest expressed ≥ 0.5) and `coloring` (drab, camo, bright display
colors, or warning = bright and toxic). Only what anyone could see, and
coarse so the client's sprite cache stays small. The API answers
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
marketplace auctions', 8, `0008_push.sql`, push notifications').

| Persisted | File | PostgreSQL table |
| --- | --- | --- |
| Accounts (name, player id, argon2 hash) | `accounts.json` | `accounts` (name PK, player unique) |
| Founder designs (one per player per world) | `worlds/<id>/designs.json` | `designs` (PK `(shard, player)`, design `jsonb`, upsert) |
| One record per simulated day and world: legacy shares (raw and weighted), every lineage's size, births and deaths by cause that day, player lineages' mean gene values (the new fields default when reading older lines) | `worlds/<id>/history.jsonl` | `daily_history` (PK `(shard, world_id, day)`, upsert; legacy and lineages `jsonb`; `seq` keeps write order) |
| Finished directed-mating agreements with price and outcome, the listing they were bought from and how the escrow settled (`paid`/`refunded`) | `worlds/<id>/trades.jsonl` | `trades` (`bigserial` id, `shard`; `listing`, `settlement` from migration 2) |
| How each auction ended (seller, organism, reserve, deadline, bids, highest bid, winner and their organism, outcome `sold`/`unsold`/`failed`/`ended`/`withdrawn`, the agreement it became, day); a world replaying a day replaces its record | `worlds/<id>/auctions.jsonl` | `auctions` (PK `(shard, world_id, listing)`, upsert; `id` keeps write order; migration 6, `shard` from 7) |
| Renown balances, upserted on every change | `worlds/<id>/balances.json` | `balances` (PK `(shard, player)`, balance, day, updated_at) |
| Alert preferences (hidden and toasted alert kinds), upserted when a player saves them | `alert_prefs.json` | `alert_prefs` (player PK, hidden and toast `text[]` of kind names, updated_at; migration 3) |
| Web Push subscriptions (player, endpoint, `p256dh` and `auth` keys, label, created), one per endpoint | `push_subscriptions.json` | `push_subscriptions` (endpoint PK, indexed by player; migration 8) |
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
and keeps the last line per `(world_id, day)` when reading), so the history
has one record per day. A `--fresh` world has a new id.

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
URL then stops the server with an error. The persistence tests
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
terrain's content hash) and the `World` in `bincode`, LZ4-compressed.
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
claimed territories and claim times).
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

**Saved games** (`snapshot.rs` in the server): magic, the game-side state
as JSON (player → lineage, open offers with their escrow, migration orders,
alerts, watched organisms, milestone progress, chronicle entries already
announced, the birth and death counters at the last day boundary, and the
economy: balances, recent ledger, listings with open auctions and their highest bids, who had the founding grant)
and the world snapshot. The game JSON carries a layout `version`
(`SAVED_GAME_VERSION`, 2 since the economy, 3 since ranked seasons: the
season under way and the last finished record, 4 since auctions: open
auctions and their highest bids among the listings, none in versions 2
and 3); a version-1 saved game still
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
(bincode + LZ4, the expensive part) and passes the bytes to the store task,
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
- **Left as is:** names are case-sensitive (`Ann` and `ann` are different
  accounts); a reverse proxy makes every client one IP for the limits;
  `GET /api/leaderboard/seasons` reads up to 100 000 daily records per call
  (rate-limited now; a cache would be the next step); `accounts.json` is
  written with the process umask; after a crash and a restart from an older
  saved game, the store's balances and ledger can hold entries from the
  abandoned timeline until they are overwritten, which a later `--fresh`
  refund (that trusts the store after the newest save) inherits.

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
npm run e2e          # Playwright smoke, phone-emulation and onboarding tests (needs `npm run build` and `cargo build --release -p evo-server`)
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
- **World** (`ui/game.ts`, `world/worldView.ts`, `world/terrain.ts`): the
  tile map from `GET /api/terrain`, painted one pixel per tile into an RGBA
  buffer shown as a nearest-filtered texture (one world unit per tile, tile
  edges outlined from 18 px per tile, dimmed at night). The dynamic layers
  are refetched every 4 s from `GET /api/terrain/dynamic` and repaint the
  same texture. Colors: biome, darkened where grazed out; grass, forest,
  marsh and tundra tinted by season (fresh spring green, summer yellow-green,
  autumn brown, dull winter); land in drought yellows toward straw; lakes
  darken with depth, rivers are bright blue (a dry bed is sand, a dried marsh
  or lake shore cracked mud); flooded land is muddy blue; ice is pale blue;
  snow blends to white; hill shading from the north-west. A legend over the
  map gives the drought, flood, snow and ice shares. Organisms from `state`
  are drawn as **creature sprites by body plan** (`world/creature.ts`,
  `world/sprites.ts`) from the server's `look`: body radius by size class
  (juveniles at 65 % with a bigger head); herbivores a round muzzle (horns
  when large), omnivores a short snout, carnivores a long snout with fangs
  and a brow; warm-blooded bodies round with ears and a stubby tail,
  cold-blooded ones long and low with a tapering tail; a fur fringe, scale
  dots, armor plates and a heavy outline, or a pale belly on bare skin; drab
  browns, camouflage blotches, a bright display crest or yellow-and-black
  warning bands; legs, or fins and a tail fin for swimmers, a membrane for
  gliders, a raised wing for fliers. The owner is an ellipse at the feet
  (yours yellow and stronger, wild grey, other players a stable hue each); a
  ring round the body when possessed (red) or pregnant (green); and an icon
  for what it is doing (z resting or dormant, ! fleeing, crosshair hunting,
  heart seeking a mate). Sprites face the way they last moved. Each distinct
  class is drawn once into a texture (`TextureCache`, supersampled so it
  stays crisp as markers grow with zoom); a marker is four sprites sharing
  those textures, so hundreds of organisms cost no per-frame drawing.
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
  **Migrate…** then a click sends the group command; **Pin view here** sends
  `set_view` at the camera center. Group commands: the toolbar's group
  menu picks which group **Migrate…**, **Clear migration**, **Territory…**
  and **Split** act on ("All" reaches every group; the territory and split
  tools want one group, the only one if there is just one). **Territory…**
  is two taps: the center (the largest circle the group may hold is
  previewed), then the edge, which sends `claim_territory` with that radius;
  **Split** sends `split_group` and picks the new group. Each group's
  territory is drawn as a tinted disc in its color (the picked group's
  outline bolder) and its migration target as a ringed cross; the
  inspector names an organism's group. On phones the same tools sit in the
  scrolling toolbar and both taps work by touch. The logic (names, colors,
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
  `seasons.test.ts`), from `/api/seasons/current`, `/api/seasons` and
  `/api/seasons/player/:me` every 5 s while open: the ranked season under way
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
clock, connection and log out; the toolbar scrolls sideways; the legends
hide behind a **Legend** button; only the two newest toasts show.

**Mobile apps (Capacitor).** `capacitor.config.ts` wraps `dist/` as
`org.evolutionmmo.client`; the native projects are generated locally and
not committed (`client/android/`, `client/ios/` are git-ignored):

```sh
cd client
npm run build
npx cap add android        # once; needs Android Studio / SDK to build
npx cap add ios            # once; needs macOS with Xcode (CocoaPods or SPM)
npm run cap:sync           # build + copy dist/ and plugins into both projects (after every client change)
npx cap open android       # or: npx cap run android / npx cap open ios
```

The app's page is `https://localhost` (Android) or `capacitor://localhost`
(iOS), so it talks to a remote game server. The origin comes from, in
order: `?server=https://host:port` on the page URL (remembered), the
**Server** field on the login screen (remembered; a native app opens it
until set), or a build default `VITE_EVO_SERVER=https://game.example.org
npm run build`; otherwise the page's own origin (`setApiBase()` in
`api.ts`, `serverOrigin.ts`). The WebSocket follows the same origin
(`wss://` for https). For a plain-http LAN server during development, run
`EVO_CAP_CLEARTEXT=1 npm run cap:sync` (cleartext and mixed content on
Android; iOS also needs an App Transport Security exception in
`ios/App/App/Info.plist`); `EVO_CAP_LIVE_URL=http://192.168.1.20:5173`
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
             asks for protocol 2 and rebuilds its binary delta states into whole `state`s
wire.ts      protocol 2 decoder (mirror of crates/server/src/wire.rs; wire.fixture.json pins them)
model.ts     GameModel: latest state, offer table (requests, updates, outcomes), alerts feed, toasts; pub/sub
alertPrefs.ts  alert preferences: normalize, per-kind show/toast switches, feed filter
push.ts      push notifications: device status, per-kind switches, subscribe/unsubscribe (browser APIs behind PushDeps)
sw.ts        the service worker (built to dist/sw.js): shows pushes, focuses or opens the game on a click
swLogic.ts   the service worker's decisions (payload check, notification, target URL, which window), unit-tested
cost.ts      design point cost and validation (mirror of crates/sim/src/design.rs)
ui/          DOM screens and panels; panels re-render at most 4×/s and rebuild buttons only
             when their content changes, so they stay clickable under 10 Hz state pushes
world/       PixiJS view: camera, tile-map texture (terrain.ts decodes and colors the grid), overlays,
             markers, hit testing, mouse/touch gestures (tap, long press, drag, pinch);
             creature.ts: body plan → shapes (pure, tested); sprites.ts: shapes → cached textures
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
plan in words), the texture cache (one make per key, disposal, 600 markers
from five plans → five textures) and the marker scale; the server origin
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
placement; and the protocol 2 decoder against the Rust fixture
(`wire.test.ts`, above). `e2e/smoke.mjs` (Playwright, pre-installed Chromium) starts
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
toggles; **Split** in the scrolling toolbar makes a second group and two
taps claim a territory for it, with no sideways scroll. Then a 1280×800 desktop: sidebar on the right with tabs on top,
phone controls hidden, sprite cache stats (textures shared per class), and
a gallery of 20 body plans drawn with the game's sprite code. Screenshots:
`iphone13-*.png`, `pixel7-*.png`, `desktop-world.png`,
`desktop-sprite-gallery.png`.
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
The server side of delivery is tested in Rust (`crates/server/tests/push.rs`)
against a mock push service.

**Not yet:** native builds tested on devices (the Capacitor projects are
generated but not built here), push notifications in the native apps (their
WebViews have no Web Push: that needs FCM/APNs through a Capacitor plugin;
the card says push is not available there; Safari on iOS has Web Push only
for a site added to the home screen, `manifest.webmanifest` is there for it,
untested on a device), animated sprites, alert preferences applied
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
6.7 → 4.4 ms, a possession sub-step 7.4 → 5.1 ms. Not done: reusing
sub-step states (commands arrive between sub-steps from most players, so
any cache would need per-player invalidation for offers, market moves and
possession), and the `StateIndex` pass over every organism at each sub-step,
now the largest state cost.

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
```

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

Next on the world thread is the sim tick itself, then the sub-step states
(it still builds a whole `StateView` for every steering player three times
a tick, though only the possessed organism's delta goes out).

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
  4–6 cores per world above with about 2; add worlds to a process while its
  cores have headroom, and past that run more processes (each with its own
  world list) behind the same accounts store.

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
  per world** and **about 2 cores per world**: two 500-player worlds held
  the clock on 4 shared cores with command p99 under 0.5 s, so 1 world per
  2 cores is measured with margin. The mean use (0.3 cores per world)
  suggests a dedicated box holds more, up to about 1 world per core, but
  that is extrapolated: measure before going past 2 worlds per 4 cores.
  Budget 0.5 GB of memory and 1.4 MB/s (11 Mbit/s) of egress per
  500-player world with protocol 2 clients. Login storms no longer stall
  the worlds (next section); the default hash bound (2 at once on 4 cores,
  ~45 hashes a second) admits about 22 new players a second (a registration
  and a login each) and makes the rest wait a few seconds.

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
  survival and mean alive, and a verdict, including roughly how many seeds a
  difference of the observed size would need to resolve. Adopt a change only
  when the paired CI excludes 0 on seeds no tuning run saw (31 onward).

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
  the predator side (embryo resorption) suppressed the hunters too much. Hybrids between antelope and herd grazers
  still occur, as do fox × pack hunter hybrids, since those pairs share a
  body plan.
- The prey-side changes below lean the balance toward prey: voles, newts,
  frogs and burrowers hold significantly more members and predators lose a
  seed or two more often (pack hunters 92% vs 100% on seeds 101–124). The
  score difference is not significant, but its sign is negative.
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
  cold-blooded eggs take to incubate.

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
checks that in autumn the herd is on the move south with most migrants
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
thermal risk (Balance section), but incubated eggs still win tournaments
most often. Doc counters that already work: warm-blooded designs with
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

## Suggested next steps

In priority order; each item is open (done work is described in the
sections above).

1. **Mobile apps on devices.** The Capacitor projects are generated but
   have never been built or run on a phone. Build both, play a session on
   real iOS and Android devices (login with a remote server, touch
   gestures, the bottom sheet, the onboarding tour, reconnects when the app
   is backgrounded), and add the result to CI if it can run headless.
2. **Native push notifications.** Web Push is implemented for the browser
   (see [Push notifications](#push-notifications)). What remains is native
   push in the Capacitor apps through FCM (Android) and APNs (iOS), and
   trying Web Push from an iOS home-screen web app on a real device.
3. **Capacity with several worlds.** Measured up to two 512² worlds of
   500 players in one process on a shared 4-core box (see [Several worlds:
   capacity](#several-worlds-capacity)): a second world costs what the
   first did, nothing serializes them, and about 2 cores per 500-player
   world holds the clock. Still open: where one process stops scaling (3+
   worlds, on dedicated cores, where the measured 0.3 cores per world
   suggests up to one world per core).
4. **Incubated eggs still win tournaments**: after the final balance pass
   and egg thermal risk they won 29.1% of entries pooled over 144 worlds
   (a fair share is 25%), abandoned eggs 16.8%; see [Balance](#balance). Next: find what
   incubators gain (the paired-rule method of the dominant-design work)
   and judge any fix with `compare` and fresh tournament seeds.
5. **Slow breeders under predation.** Antelope now survive the held-out
   sets, but seeds that lose them are those where pack hunters boom on
   other prey (apparent competition). Try a predator-side numerical
   response (pack hunters starving faster when prey thins, or prey
   switching), measured with `compare` on 48+ seeds, and check whether the
   prey-side rules cost predators seeds systematically (the sign was
   negative, not significant).
6. **World-thread cost.** At high density decide is still most of a tick
   and is now bound by memory (neighbor genes and positions; see [High
   density](#high-density-decide-act-sweep-sub-step-states)); going further
   means changing results (cap or subsample the neighbors scanned, judged
   with `compare`) or packing the grid's genes smaller (bounds widened to
   match). The sequential act sweep is half what it was; splitting the rest
   by region is next if the apply becomes the bottleneck. Sub-steps still
   build `StateIndex` (a pass over every organism) and a whole `StateView`
   for every steering player three times a tick; reusing them needs
   per-player invalidation for commands applied between sub-steps.
7. **Bandwidth and saves.** permessage-deflate would take another 2.8× off
   protocol 2 states, but the WebSocket stack (axum 0.7, tungstenite 0.24)
   doesn't offer it; revisit when it does (states are already ~1 KB/s a
   client, so this is low priority). For save size, store the map's
   changing tiles (about 6 MB of an 18 MB save at 26k agents) as a delta
   against the previous save.
8. **Design-doc features not built.** Offline protection (home ranges or
   surviving founder groups; a territory could become one), group commands
   as genes (the doc's open question; wanderlust and group tolerance
   already shape how a group moves), and the doc's open question on
   duplicate mutations; `GET /api/leaderboard/seasons` would also want a
   cache instead of reading the whole history per call.
9. **Tooling.** An adaptive `compare` that adds seeds until the CI
   resolves, and a check of how well 40-day screen ranks predict 120-day
   ranks; what migrating buys a design (the caribou with and without the
   gene, paired) and a reason for winter ranges to exist (better
   cold-season pasture than the summer range).
