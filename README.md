# Evolution MMO

[![CI](https://github.com/alpuglisi/mmo/actions/workflows/ci.yml/badge.svg)](https://github.com/alpuglisi/mmo/actions/workflows/ci.yml)

A persistent multiplayer world where each player designs a founding organism
and steers its lineage through breeding rather than direct editing. The full
design is in [`docs/game-design.md`](docs/game-design.md).

Build step 1 of that design (prototype the simulation rules headless, with
no players attached, and measure per-organism tick cost) is done and still
being tuned. **Build step 2 has started:** `evo-server`, the authoritative
Rust server around the simulation (accounts, founding, WebSocket play,
directed-mating consent, persistence behind a trait; see [Server](#server)).
**Build step 3 has started:** a TypeScript web client (PixiJS, Vite) with
login, the founder design editor, the world view, matings and a dashboard;
see [Client](#client).

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
| `terrain` | Seasons and biomes, Movement and terrain | Noise-generated biomes, latitude/altitude climate, four seasons, day/night. Rivers carved by flow accumulation down a pit-free drainage network to lakes, sea or map edge. Regional weather on a 16-tile grid: fronts drifting east bring wind, storms (chill, costly open ground, grounded flyers) and rain or snow; slow dry and wet spells and the seasons drive a per-cell soil wetness that fills and drains shallow lakes, marsh and rivers (droughts dry them out, crowding organisms at the rest) and paces plant regrowth; spring snowmelt swells rivers so fast deep water blocks weak swimmers, and rivers freeze in winter. Canopy browse needs height or climbing, and climbers in canopy are safe from ground hunters. Tiles update chunk by chunk, each chunk once per simulated hour. Hooks: `Map::weather_at`, `Tile::wetness`/`Map::dryness_at`, `Map::drought`/`flood`, `Map::terrain_version`/`chunk_version`; `Map::climate_temp`/`climate_wet` are climate anomalies (0 normally) that catastrophes set. `traverse()` returns Blocked / Passable{speed, energy} from body vs. tile. |
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

## Running

Needs a recent stable Rust.

```sh
cargo test --release                                  # unit and world tests
cargo run --release --bin evo-sim -- designs          # sample designs and point costs
cargo run --release --bin evo-sim -- run --days 120 --report 10 --snapshot out/world
cargo run --release --bin evo-sim -- bench --scale 4 --days 3
cargo run --release --bin evo-sim -- bench --size 256 --scale 16 --days 12 --threads 1,2,4 --compare-apply
cargo run --release --bin evo-sim -- sweep --trials 16 --seeds 3 --rounds 2 --out sweep.csv
cargo run --release --bin evo-sim -- run --set sprint=2.0 --set crowding=0.05   # try a balance tweak
cargo run --release --example trace -- 3 200          # follow one fox (lineage 3)
cargo run --release --example stuck -- 120 1 2        # how often organisms get stuck
cargo run --release --example calibrate -- tournament --worlds 20   # point-cost check (CSV)
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
| Rust | `cargo build --release --workspace`, `cargo test --release --workspace`, `cargo clippy --release --all-targets -- -D warnings` (stable toolchain, cargo cache) |
| Balance smoke check | `scripts/balance-smoke.sh`: `evo-sim sweep --size 64 --trials 1 --seeds 3 --rounds 1 --days 60` with the default `Balance`, failing if fewer than 14 of the 24 lineage-worlds (8 lineages × 3 seeds) still hold 4 members with 2 adults, or the score drops below 7 of 12. The current rules make 21 of 24 and 10.4, so only a rule change that wrecks coexistence trips it; thresholds are env-overridable (`MIN_SURVIVORS`, `MIN_SCORE`, `SEEDS`, `DAYS`, `SIZE`). |
| Client | `npm ci`, `npm run typecheck`, `npm test`, `npm run build` |
| End-to-end | builds `evo-server` and the client, installs Playwright and Chromium (CI only; developer machines have them pre-installed), runs `npm run e2e` and uploads the screenshots as an artifact when it fails |

Run the same checks locally before pushing:

```sh
cargo build --release && cargo test --release --workspace
cargo clippy --release --all-targets -- -D warnings
scripts/balance-smoke.sh
(cd client && npm ci && npm run typecheck && npm test && npm run build && npm run e2e)
```

Formatting isn't checked yet: the code is not `cargo fmt`-clean and no
`rustfmt.toml` matches its hand-wrapped wide style, so enforcing it needs
one mechanical whole-repo `cargo fmt` commit first (best done when no other
branches are in flight), then a `cargo fmt --all --check` step.

**Determinism in tests.** The server integration tests that need exact
timing use a manual world clock: `ServerConfig::manual()` makes the world
thread tick only on `RunningServer::step(n)` (which pushes state once at the
end) and apply commands as they arrive, so nothing races the 20 ms fast-mode
ticker (`directed_mating_consent_round_trip` runs this way). The browser
smoke test waits on explicit state conditions with generous timeouts rather
than fixed sleeps, and retries the steps the living world can legitimately
interrupt (a click landing on an organism that just moved there, a
possessed organism breaking off its order to drink or flee, a picked mate
dying or conceiving first).

## Server

`crates/server` (binary `evo-server`) is build step 2: the authoritative
server. Clients only display state and send commands.

```sh
cargo run --release --bin evo-server                          # 128×128 world, 1 tick/s, data in ./data
cargo run --release --bin evo-server -- --fast --memory --size 64   # 20 ms ticks, nothing saved
cargo run --release --bin evo-server -- --bind 0.0.0.0:8080 --data /var/lib/evo --seed 7 --tick-ms 500
cargo test -p evo-server                                      # unit + end-to-end tests (64×64 worlds)
```

Other flags: `--no-wild` (no wild ecosystem), `--days-per-year N` (short
years for demos), `--state-ms` (state push interval), `--static DIR` (serve
the built web client from `DIR`, e.g. `client/dist`, on every path the API
doesn't use), `--session-idle-min N` / `--session-max-hours N` (session
expiry, default 60 min idle and 24 h), `--no-rate-limits`, `--lod` (level
of detail, below), `--no-catastrophes` (catastrophes are on by default in
the game, never in the first year). `RUST_LOG=debug` for more logging.

### Architecture

```
HTTP / WS handlers (Tokio, Axum) ──Request──▶ world thread (owns Game + World)
        ▲                       bounded queue        │ tick loop, fixed rate
        └──── per-connection queue ◀── results, events, state pushes
                                    ◀── watch: world summary + leaderboard
                       store task ◀── daily records, finished trades
```

- **One world thread** owns the `World` exclusively (no locks around it).
  Handlers send `Request`s over a bounded channel (a full queue answers
  `busy`). Each tick it waits for the deadline (default 1 s, `--tick-ms`),
  answering reads at once (connect, map snapshot) and queueing world changes
  (commands, foundings); then applies the queue **in arrival order**, calls
  `World::step`, runs the game bookkeeping, publishes a summary and pushes
  state. A run is reproducible given the same commands in the same ticks.
- **`Game`** (`game.rs`) is the pure logic around the world: one lineage per
  player, the mating-offer table and consent flow, migration orders, views and
  interest management. It returns messages instead of sending them, so it is
  unit-tested without sockets.
- **Interest management:** a state message carries the player's own organisms
  in full and other organisms only inside a square view window (default
  half-side 16 tiles, at most 32; at most 256 foreign organisms, nearest
  first). The window follows the possessed organism, else the lineage's
  centroid, unless the client pins it with `set_view`. Foreign organisms show
  only what can be seen (position, action, sex, maturity), not needs.
- **Slow clients:** each connection has a 256-message queue. A full queue
  drops state frames (the next replaces them); a connection that can't take
  replies or events is closed.
- **Accounts:** argon2id password hashes, random 256-bit bearer tokens held in
  memory. Each account owns a `PlayerId` (1, 2, …; 0 is the wild ecosystem).
  A session ends after 60 minutes without use (an authenticated request or a
  WebSocket command) or 24 hours after login (`SessionPolicy`); an open
  WebSocket checks every `min(idle, max_age)/4` (at most 30 s), sends
  `session_ended` and closes.
- **Rate limits** (`limits.rs`, token buckets, `Limits` in `ServerConfig`):
  logins and registrations per client IP (burst 10, then one per 6 s; checked
  before any password hashing, `429 rate_limited` with `retry_after_ms` and
  `Retry-After`), WebSocket commands per connection (20/s, burst 40) and per
  IP over all its connections (60/s, burst 120); a refused command gets a
  `result` with `rate_limited` and never reaches the world thread. The IP is
  the TCP peer (behind a reverse proxy that would be the proxy: trusting
  `X-Forwarded-For` is not implemented).
- **Alerts and history** (`game.rs`): the world thread keeps the daily records
  since start (up to 365) with each lineage's births, deaths by cause and
  mean gene values, and raises per-player alerts: deaths of founders and
  possessed organisms (with the cause), the day's births and deaths,
  population (25, 50, 100, …) and legacy-share (1, 5, 10, 25, 50 %)
  milestones, a new generation, extinction, a possessed organism that is
  parched or starving, catastrophes beginning and ending, era ends (with the
  player's rank) and achievements. The last 50 per player are kept for the dashboard.
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
| `POST /api/register` `{name, password}` | | Create an account (name 3–24 of `[A-Za-z0-9_-]`, password ≥ 8). `201 {token, player, name}`; `409 name_taken` |
| `POST /api/login` `{name, password}` | | `{token, player, name}`; `401 bad_credentials` |
| `POST /api/logout` | Bearer | End the session |
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
| `GET /api/achievements` | | `{players: [{player, name, species_founded, longevity_days, niche_dominance, catastrophes_survived}], recent: [{day, player, name, kind, text}]}` |
| `GET /api/lineage/stats` | Bearer | The genetics dashboard: counts (alive, adults, young, pregnant, newest generation), `age_bands` (up to six equal bands by sex), `vitals` (births and deaths by cause per recorded day, the last entry today so far, at most 30), `genes` (every gene's mean, spread and founding mean with the daily means, largest drift first), `legacy` (share per recorded day) and recent `alerts` |
| `GET /api/map.png?scale=4` | | PNG of the world (`render_rgb`, 1–8 px per tile), for debugging |
| `GET /ws?token=…` | token | The game WebSocket (or `Authorization: Bearer`) |

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
| `request_mating` | `mine, partner, price?` | Directed mating. Your own pair runs at once; another player's organism sends them a `mating_request` |
| `respond_mating` | `request, accept` | The partner's owner answers |
| `complete_mating` | `request` | Retry an accepted agreement now |
| `cancel_mating` | `request` | Either side withdraws an open offer |
| `migrate` / `clear_migration` | `x, y` / – | Group command for your adults |
| `set_view` / `clear_view` | `x, y, radius` / – | Pin or release the view window |

Server → client:

| `type` | When |
| --- | --- |
| `welcome` `{player, name, lineage}` | On connect |
| `state` | On connect, then every state interval (default 1 s): `clock` (tick, day, year, hour, season, daytime, era, era_days), `weather` (mean wind, storm, drought and flood shares, `catastrophe` while one is under way), `lineage`, `slots_left`, `population` (lineage size, cohort members included), `migration`, `view`, `organisms` (yours: id, lineage, position, action, sex, age, growth, health, needs, possessed, pregnant, generation), `nearby` (others in the window), `leaderboard`, `offers` (your open offers) |
| `result` `{id, command, ok, error?: {code, message}, data?}` | After each command. Codes include the sim's `not_found`, `not_owned`, `no_consent`, `not_mature`, `same_sex`, `already_pregnant`, `too_far`, `no_slots_left`, `possessed_by_other`, and `bad_message`, `bad_coordinates`, `unknown_offer`, `offer_closed`, `too_many_offers`, `busy`, `rate_limited` |
| `mating_request` `{offer}` | Someone asks to mate with your organism |
| `mating_update` `{offer, outcome?, error?}` | An offer changed status (to both sides) |
| `alert` `{alert: {tick, day, kind, text, organism?}}` | `kind`: `daily_summary`, `birth` (first offspring, new generation), `death` (founder or possessed organism), `milestone`, `need` (possessed organism parched or starving), `world` (a catastrophe began or ended, an era ended with your rank), `achievement` (species founded, longevity, niche dominance, survived a catastrophe) |
| `session_ended` `{reason}` | The session expired; the socket closes next |

**Directed-mating consent:** `request_mating` checks ownership, maturity, sex,
pregnancy and slots, then creates a `pending` offer (price recorded; no
economy yet) and asks the partner's owner. Wild organisms can't consent. On
`accept` the server tries the mating at once with `partner_consent`; if the
two are farther apart than the meeting distance the agreement stays
`accepted` (both get `too_far`) and runs on the first tick they meet, so
escorting a possessed organism to the meeting is the trip the design
describes. Outcomes: `completed` (with mismatch, override cost, failure
chance, fertility, conceived), `declined`, `cancelled`, `failed` (the rules
refused it, e.g. an organism died) or `expired` (season change, when slots
reset). The requester's seasonal slot is spent.

### Persistence

`Store` (`store.rs`) is an async trait with a JSON-file implementation
(`FileStore`, the default, in `--data DIR`) and an in-memory one (`--memory`,
tests). A PostgreSQL/sqlx `PgStore` would implement the same methods, one
table per record.

| Persisted | File |
| --- | --- |
| Accounts (name, player id, argon2 hash) | `accounts.json` |
| Founder designs | `designs.json` |
| One record per simulated day: legacy shares (raw and weighted), every lineage's size, births and deaths by cause that day, player lineages' mean gene values (the new fields default when reading older lines) | `history.jsonl` |
| Finished directed-mating agreements with price and outcome | `trades.jsonl` |

**Not persisted yet:** the `World` itself, login sessions and alerts (the
dashboard's history starts when the server does; the seasonal leaderboards
read the whole stored history). A restart
generates a fresh world from the seed and re-founds every stored design in
player order (a design that no longer fits the budget is skipped with a
warning); players log in again. World snapshots need serde on the sim types
(organisms, genomes, map, weather, counters, the world's private fields);
the RNG is derived from `(seed, stream, tick, organism)`, so a snapshot is
just data plus the tick.

## Client

`client/` is build step 3: the browser client, TypeScript with PixiJS 8 for
the map and plain DOM/CSS for panels (no UI framework), bundled by Vite into
static files that `evo-server` can serve and Capacitor can wrap later. It
only displays state and sends commands; the server stays authoritative.

```sh
cd client
npm install
npm run dev          # Vite on :5173, proxies /api and /ws to 127.0.0.1:8080 (EVO_SERVER=http://host:port to change)
npm run build        # typecheck + static bundle in client/dist
npm run typecheck
npm test             # Vitest unit tests
npm run e2e          # Playwright smoke test (needs `npm run build` and `cargo build --release -p evo-server`)
```

Serve the built client from the game server (one origin, no proxy):

```sh
(cd client && npm run build)
cargo run --release --bin evo-server -- --fast --memory --static client/dist   # open http://127.0.0.1:8080
```

**Screens.**

- **Login / register** (`ui/login.ts`). The token is kept in
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
  map gives the drought, flood, snow and ice shares. Organism markers from
  `state` sit on top: own organisms yellow (larger when
  adult; red ring when possessed, green when pregnant), wild grey, other
  players a stable hue each. Markers live in screen space so they stay
  readable at any zoom, and glide between state pushes. Drag to pan, wheel or
  +/− to zoom; **Follow** keeps the camera on the server's view window (your
  possessed organism or your lineage's centroid), drawn as a blue square.
  Click an organism to inspect it: yours show action, sex, age, stage,
  health, lineage and generation, and the eight needs as bars; others show
  only what the protocol reveals. **Possess** / **Release**; while
  possessing, clicking empty map sends `order_move` (target line drawn).
  **Migrate…** then a click sends the group command; **Pin view here** sends
  `set_view` at the camera center.
- **Matings** tab: request a directed mating (one of your adults, a partner
  id filled from the inspector's "Request mating…", optional price);
  incoming requests with Accept / Decline; your open offers with Cancel and,
  once accepted, **Complete now** and **Escort** (possess your organism and
  walk it to the partner's last seen position, the trip to the meeting);
  recent outcomes with conception, mismatch, fertility and failure chance.
- **Genetics** tab (`ui/dashboard.ts`, `ui/charts.ts`), from
  `GET /api/lineage/stats` every 5 s while open: count tiles; a population
  pyramid by age band and sex; births (up) and deaths stacked by cause (down)
  per day; legacy share and weighted share over the recorded days; gene
  drift sparklines for the six genes that moved most since founding (dashed
  founding value); and a table of every gene with mean ± spread on a 0–1
  track. Hand-rolled SVG (no chart library), fixed categorical colors, a
  `<title>` tooltip on every mark. Below it the **alerts feed**: server
  alerts (births, deaths of founders and possessed organisms, milestones,
  needs, daily summaries) merged with mating offers and their outcomes,
  newest first, organism ids linking to the inspector; an unread badge on
  the tab.
- **Lineage** tab: lineage size (with level of detail on, also how many of
  them are agents on the map; the Genetics tab adds an "On the map" tile),
  adults and young, pregnancies, newest
  generation, mating slots left this season, migration target, the legacy
  leaderboard (share and weighted share per player, yours highlighted), the
  **seasonal leaderboards** (`GET /api/leaderboard/seasons`, the current
  season open) and a clickable list of your organisms. The top bar shows the
  clock, season, day/night, weather, a red catastrophe banner while one is
  under way (from `state.weather.catastrophe`) and the connection state.
- **World** tab (`ui/chronicle.ts`), from `/api/species`, `/api/eras`,
  `/api/catastrophes` and `/api/achievements` every 10 s while open: the
  active catastrophe and what it does, the era clock, finished eras'
  leaderboards (your rank, niche leaders), the species tree (daughter
  species under their parents; diet, estimated adults or extinction, dominant
  allele owner and share, founder), the catastrophes with population before
  and after and how many player lineages survived, and achievement tallies
  per player with the latest achievements.

**Architecture.**

```
main.ts      login → design (no lineage) → game; one GameSocket from login to logout
protocol.ts  wire types written by hand from crates/server/src/protocol.rs (keep in sync) + parseServerMsg
api.ts       fetch wrapper for the HTTP API (ApiError carries code and detail)
socket.ts    GameSocket: ids paired with `result`, 10 s reply timeout, reconnect with
             exponential backoff (0.5 s doubling to 15 s, ±20 % jitter), connection state
model.ts     GameModel: latest state, offer table (requests, updates, outcomes), alerts feed, toasts; pub/sub
cost.ts      design point cost and validation (mirror of crates/sim/src/design.rs)
ui/          DOM screens and panels; panels re-render at most 4×/s and rebuild buttons only
             when their content changes, so they stay clickable under 10 Hz state pushes
world/       PixiJS view: camera, tile-map texture (terrain.ts decodes and colors the grid), overlays,
             markers, hit testing
```

Every failed `result` (server error codes such as `not_owned`,
`too_far`, `no_slots_left`, or local `not_connected` / `timeout`) shows as
a toast. `window.evo` exposes the model, socket and view for debugging and
the end-to-end test.

**Tests.** Vitest (38 tests): the World tab's data prep (species tree
order and depth, statuses, bad parent links, the catastrophe banner and
labels, niche labels, the player's era ranks); protocol parsing against messages shaped like
serde's output, including malformed and unknown ones and the `alert` and
`session_ended` messages; terrain decoding (layer sizes, refusing a wrong
size, newer-only dynamic updates) and tile colors (snow, ice, drought,
floods, dry river beds, dried lakes, the season tint) and painting; chart
data prep (scales, axis maxima, gap-aware line paths, deaths stacked by
cause with rare causes folded into "other", the age pyramid, gene drift
ranking, legacy percentages) and the alerts feed merge; the point cost and
validation against the server's own examples (the README design costs
15.1, exactly-at-budget passes, one more founder fails); socket id pairing,
backoff, reconnect and give-up; the offer table through request, accept
and outcome. `e2e/smoke.mjs` (Playwright, pre-installed Chromium) starts
`evo-server --fast --memory --static client/dist` on a free port, registers,
checks the live cost against the prices in `/api/genes` (and that the
server's founding response agrees) and the client-side over-budget refusal, founds a lineage, waits for markers and
the tile map (64×64 texture, at least 3 biomes and 20 tile colors, a dynamic
refresh, the terrain legend), registers a second player and runs a directed
mating through both players' panels while both lineages are young (request
with price → incoming → accept → both see the update → escort, then
release), clicks one of its own organisms (the one with
the most room around its marker, checked with the view's hit test, retried
if the selection isn't its own), possesses it, clicks a dry-land spot clear
of markers and checks it walks toward the target (closest approach tracked,
an arrival counts, the order re-issued if a click misses or the organism
breaks off for a need), migrates, sees an error toast, checks the seasonal
leaderboard and the genetics dashboard (at least four charts with drawn
lines, drift sparklines, every gene in the table, alerts in the feed) and
the World tab (species listed, the era clock, an achievements row). Every
step waits on explicit state with generous timeouts. Screenshots go to `SCREENSHOT_DIR`. `e2e/winter_shot.mjs`
is a screenshot helper (not a test): a 16-day-year world played into winter.

**Not yet:** organism sprites by body plan, touch pinch-zoom, a stud
marketplace and economy UI, allele-origin shares within the lineage (whose
genes your organisms carry), per-gene distributions (histograms rather than
mean ± spread), alert preferences, and the Capacitor shells.

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
  perception); the apply's sequential act sweep is the next part to split by
  region if the apply phase becomes the bottleneck.

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
| **Current: seasonal strategies (12 unseen seeds)** | **10.94** | 100% | 58% | 92% | 100% | 100% | 92% | 100% | 100% |

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
- Antelope and herd grazers are the weakest lineages now that every predator
  is viable (mean alive ~20 and ~40). Hybrids between those two
  near-identical designs still occur, as do fox × pack hunter hybrids, since
  those pairs share a body plan.
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

All the doc's seasonal add-ons are now in. Not yet implemented on the
environment side: rivers do not overflow onto
their floodplain (flooding only deepens the channel), wind has no direction
and does not interact with wing loading or gliding, and lakes do not form in
filled depressions.

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
are unchanged (100%, 448). Not addressed:
incubated eggs still win 38% and abandoned eggs 14% (birth mode, not diet or
metabolism). Doc counters that already work: warm-blooded designs with
abandoned eggs survive only 62% of the time (thermoregulation mismatch), and
herd grazers lose herd defense when shrunk below 0.45.


**Seasonal strategies** (`world::seasonal`, tunables `dormancy_temp` …
`cache_capacity` in `Balance`): the sample designs use them where they
clearly fit: voles cache, newts and toxic frogs brumate, burrowing omnivores
hibernate. Antelope were tried as migrators, but on both 12-seed sets
migration cost them survival (67% vs 75% and 50% vs 67%), mostly to
predators met on the autumn trek, so they stay year-round. Seasonal genes
are left out of the genetic distance (`genome::IN_DISTANCE`): counted, they
diluted it enough to make foxes and pack hunters kin. Appending genes still
reshuffles founder and mutation randomness, which alone moved a 12-seed score by up to
0.4 (seeds 43–54: 11.13 before, 10.70 with the new genes and no strategies
in use, 10.71 with the presets' strategies).

## Suggested next steps

1. **Slow breeders under predation:** antelope and herd grazers are now the
   weakest lineages (sub-adults and orphans are the main losses).
2. **Sweep robustness:** score trials on more seeds (single worlds are
   chaotic), and screen trials on short runs before giving the best full runs.
3. **Decision cost at density:** decide is three quarters of a tick at the
   prototype target and its per-organism cost doubles from scale 4 to 16
   (perception scans more neighbors in a crowd); cap or subsample the
   neighbors scanned. Then the apply phase's sequential act sweep (a quarter
   of apply) by region. See [Benchmark](#benchmark).
4. A migrating sample design to exercise group navigation in play (none
   migrates yet: migrating antelope lost survival again even with group
   navigation).
5. **Server (step 2, started):** world snapshots (serde on the sim types,
   then a snapshot every simulated day and on shutdown); a `PgStore` on
   sqlx; possessed organisms ticking faster than the world clock; interest
   management through the sim's spatial grid instead of a scan per
   connection; session expiry and rate limits; a stud marketplace and an
   economy behind the recorded prices; seasonal leaderboard aggregation;
   several worlds (shards) per process.
6. **Client (step 3, started):** a terrain endpoint (tile types and heights
   instead of the debug PNG) and tile rendering with seasons and weather;
   a genetics dashboard (trait distributions and allele shares from a new
   lineage-stats endpoint) and alerts (deaths, births, offers); sprites by
   body plan; touch gestures; Capacitor shells for iOS and Android.
