# Marketing content tools

Tools that turn the simulation into posts, clips and devlog images. Nothing
they generate belongs in git: output goes to `film-out/` or
`tools/marketing/out/` (both ignored), and `*.png`, `*.mp4` are ignored
repo-wide.

| Tool | Runs | Makes |
|---|---|---|
| `evo-sim film` (this page) | headless, Rust only (ffmpeg optional) | map time-lapses in 9:16 and 1:1, story clips, `stories.json` + `manifest.json`, charts |
| `bin/evo-marketing.mjs` ([PUBLISHING.md](PUBLISHING.md)) | Node 20+ | drafts per platform from those two files, approval queue, calendar, publishing |
| `tools/marketing/capture.mjs` (below) | the browser client via Playwright, plus ffmpeg (optional) | in-game scenes in 9:16 and 1:1, share cards, `stories.json` + `manifest.json`; also the in-game share cards and spectator watch links |

## Headless film, stories and charts: `evo-sim film`

Runs a world for years with no browser, records a per-day timeline, renders
map frames in the client's art style, and mines the run for stories.

```sh
cargo build --release
# 4 years on the default 96×96 map, carnivore rules and random catastrophes on:
./target/release/evo-sim film --years 4 --seed 31 --meat --catastrophes --out film-out/seed31
```

About 2–4 minutes per simulated year at 96×96 with a frame every day (the
frames, not the simulation, are most of it; `--frame-days 2` halves that).

### What it writes

| File | What |
|---|---|
| `film-9x16.mp4`, `film-1x1.mp4` | H.264, yuv420p, 24 fps, `+faststart` (ready for Reels/Shorts/TikTok and feeds) |
| `film-9x16.gif`, `film-1x1.gif` | 480 px wide, ≤15 fps, 128-colour palette, for READMEs and chats |
| `frames-9x16/00001.png ..` | 1080×1920: title + date band, 1080² map, one sparkline row per big lineage |
| `frames-1x1/00001.png ..` | 1080×1080: map, date/season pill top-left, translucent sparkline band |
| `timeline.json` | per day: population and mean of ten genes per lineage; species (origin, parent, founded/extinct day), catastrophes |
| `stories.json` | notable events, best first, in the publisher's format (below) |
| `manifest.json` | every video, GIF, clip and chart with size, length, story and alt text (below) |
| `clips/<story id>-9x16.mp4` | a 6–30 s clip of each top story (`--clips N`, default 6) |
| `population.svg/.png` | population by lineage, catastrophes shaded, direct labels |
| `genes.svg/.png` | gene drift: small multiples of ten genes, lineage means over time |

The frames are kept only when ffmpeg is missing, `--no-video` is given, or
`--keep-frames` is given. To encode kept frames by hand:

```sh
ffmpeg -framerate 24 -i film-out/frames-9x16/%05d.png -c:v libx264 -pix_fmt yuv420p -crf 20 film-9x16.mp4
```

### Options

`evo-sim film --help` lists them all. Film options: `--years N` (default 3),
`--out DIR`, `--frame-days N` (default 1), `--fps N` (default 24),
`--formats 9x16,1x1`, `--title TEXT`, `--clips N` (default 6), `--no-frames` (timeline, stories and
charts only, fast), `--no-video`, `--keep-frames`. Every world option of
`evo-sim run` also works: `--seed`, `--size`, `--meat`, `--food`,
`--catastrophes`, `--catastrophe KIND@DAY` (e.g. `epidemic@200`),
`--migrants`, `--niches`, `--founding`, `--design`, `--set NAME=V`,
`--threads`.

To re-mine stories or redraw charts from a finished run without simulating
again (after changing the detectors, say). It lists the films and charts in
`manifest.json` again but not the story clips, since story ids can change
with the detectors:

```sh
./target/release/evo-sim film --from film-out/seed31/timeline.json --out film-out/seed31
```

### The art

The renderer (`crates/sim-cli/src/film/art.rs`, `raster.rs`) uses the
client's tokens from `client/src/world/palette.ts` and `colors.ts` (see
`client/src/world/STYLE.md`):

- **Ground.** The biome ground colours, with the season's green blended into
  plant cover. Grazed ground fades toward bare.
- **Water.** Shore, shallow, deep and abyss colours by depth, plus river,
  ice and snow.
- **Light and texture.** Light comes from the top-left on the relief, with a
  soft warp so tile edges read as painted, not gridded.
- **Creatures.** Flat-shaded discs sized by body mass, each with:
  - one top-left highlight;
  - the uniform dark outline `#15181c`;
  - a contact shadow offset down-right (`#0b0e12` at 32%).
- **Lineage colours.** Each lineage gets the client's golden-angle owner hue,
  and the charts use the same colour for that lineage.

Text uses a built-in 5×7 pixel font, so no font files or system fonts are
needed.

### Stories (`stories.json`) and `manifest.json`

Both files use the format the publishing tool reads (see "Inputs" in
`PUBLISHING.md`), so a run feeds it directly:

```sh
cd tools/marketing
node bin/evo-marketing.mjs draft --stories ../../film-out/seed31/stories.json --manifest ../../film-out/seed31/manifest.json
```

`stories.json` is `{ generated_at, world, seed, stories: [...] }`, best first:

```json
{
  "id": "s31-comeback-marsh-newt-d140",
  "kind": "comeback",
  "title": "Back from the brink: the Marsh newt",
  "summary": "The Marsh newt fell from 120 to just 9 in year 2 (summer), then recovered to 95 by year 3 (spring).",
  "facts": { "lineage": "Marsh newt", "peak": 120, "low": 9, "recovered": 95, "year": 2, "season": "spring", "start_day": 140, "end_day": 262 },
  "media": ["clip-s31-comeback-marsh-newt-d140-9x16", "chart-population"],
  "players": [],
  "score": 0.93,
  "start_day": 140, "end_day": 262,
  "when": "year 2, spring (day 140) – year 3, spring (day 262)",
  "lineages": ["Marsh newt"],
  "series": [[140, 120], [143, 112], "..."]
}
```

The publisher's fields:

- `id`: stable for a seed and the detectors (`s{seed}-{kind}-{subject}-d{start_day}`).
- `title`: a one-line headline, true as stated.
- `summary`: one or two sentences.
- `facts`: flat scalars, the only numbers copy may cite. Every story also gets
  `year`, `season`, `start_day` and `end_day`.
- `media`: manifest ids. That is the story's clip when one was cut, plus the
  population chart (the gene chart for `trait_shift`).
- `players`: always empty. The sample lineages belong to no real player, so
  copy never names a player.

The extra fields are for people and charts:

- `start_day`/`end_day`: days start at 0, and a year is 120 days (four
  30-day seasons).
- `when`: the time range in words.
- `lineages`: the lineages involved.
- `series`: `[day, value]` pairs, thinned to about 40 points.

Each further story of a kind already in the list scores 8% less than it
would alone, so one busy kind doesn't crowd the top.

`manifest.json` is `{ generated_at, world, items: [...] }`. Each item has
`id`, `path` (relative to the manifest), `type`, `duration_s`, `width`,
`height`, `story_id` and `alt`. The items are:

- `film-9x16` and `film-1x1`: the films, as video.
- `gif-9x16` and `gif-1x1`: the GIFs, typed as images.
- `chart-population` and `chart-genes`: the chart PNGs.
- `clip-<story id>-9x16`: one per story clip.

Story clips are cut from the frames for the top `--clips N` stories (default
6), using the vertical frames when both shapes are made. Each covers the
story's days plus 15 on each side, slowed or sped up to last 6–30 seconds.
They need ffmpeg.

When a film was made, a `timelapse` story for the whole run is added, with
both films as media.

| kind | Detected when | Score |
|---|---|---|
| `speciation` | the chronicle records a species splitting off another and it holds at least 20 days (shorter ones are the species clusterer flickering) | 0.6–0.95 (longer-lived, bigger, surviving species score higher) |
| `species_extinction` | a species that held 30+ days and reached 20 adults dies out; the summary names the daughter species its line lives on in, if any. Its range is its last half year (for the clip) | 0.45–0.75 |
| `comeback` | a lineage drops to ≤15% of an earlier peak (and ≤30), then climbs back to ≥50% of that peak and ≥3× its low | 0.7–0.95 by depth |
| `predator_boom`, `population_boom` | the 5-day-smoothed population at least doubles to a peak (≥20) within a year, then falls to ≤40% of it within a year; predator when the lineage's mean carnivory is above 0.5 | 0.55–0.9 |
| `catastrophe` | a severe winter, megadrought, epidemic or flood year happened | 0.5–1.0 by the share of life lost and severity |
| `trait_shift` | the 30-day-smoothed mean of a gene moves ≥0.15 within two years, in a lineage of 10 or more (e.g. "The Pack hunters evolved pack hunting") | 0.4–0.95 by size of the move |
| `extinction` | a lineage that reached at least 5 dies out | 0.45–0.75 by its peak |
| `timelapse` | always, when a film was made | 0.5 |

The publisher picks hashtags by kind (`src/hashtags.mjs`): `boom`,
`comeback` and `trait` are listed there for these kinds.

The followed genes are carnivory, body size, pack instinct, group tolerance,
aggression, boldness, litter size, fat stores, pickiness and nose. They are
listed in `timeline.json`'s `genes` and set in
`crates/sim-cli/src/film/timeline.rs`.

Seeds differ a lot. For a story-rich run, use `--meat --catastrophes` and
4–8 years, and try a few seeds with `--no-frames` (fast). Then film the seed
whose `stories.json` reads best:

```sh
for s in 1 2 3 4 5 6; do ./target/release/evo-sim film --no-frames --years 6 --seed $s --meat --catastrophes --out film-out/scan$s; done
head -c 600 film-out/scan*/stories.json
```

### Charts

`population.svg` and `genes.svg` are hand-written SVG and stay editable. They
have a white ground, hairline grid, direct labels instead of legends,
catastrophes as faint red bands, and `<title>` tooltips on every line. The
PNGs are drawn by the same rasterizer as the film, for places that don't
take SVG. Both charts show the biggest lineages only (8 in the population
chart, 6 per gene panel).

## In-game capture, share cards and spectator mode: `capture.mjs`

These are made by the real client in a browser: the creature art library, the
painted map and the live simulation. Small samples are in
`docs/marketing/samples/`, with a `manifest.json` and `stories.json` from a
short run (their paths name that run's files, not the samples).

### Shareable cards (in the game)

- **Species portrait card** (1080 × 1350, 4:5 for feeds): the lineage's
  average animal today, drawn with the art library on a canvas, its name,
  species, a caption ("small warm-blooded omnivore · scales · …") and its
  numbers.
- **Family-tree card** (1080 × 1920, 9:16 for stories): the founding, today
  and up to four stages in between, each with its portrait and what changed
  ("Ear size ↑", "Six legs").
- **Where:** the Family tab's Share bar (`Share portrait`, `Share tree`,
  `Copy watch link`) and the spectator page's `Share card`.
- **How it shares:** the Web Share API with the PNG attached where the browser
  can share files (phones, the apps' WebViews). Otherwise it shares the link
  (`navigator.share` without files). Otherwise it downloads the PNG and copies
  the lineage's watch link to the clipboard.
- **Code:** `client/src/share/card.ts` draws (pure layout helpers, unit-tested);
  `share.ts` shares; `familyShare.ts` is the Family tab's bar.
- **Server-side rendering:** the art library is TypeScript, so cards are not
  drawn by the Rust server. Headless rendering (batch cards for a press kit,
  link previews) goes through the capture script below. It drives the real
  client in headless Chromium and reads the PNGs from `window.evoCards` (a
  logged-in player's own lineage) and `window.evoSpectator.portraitCard()`
  (any lineage, from public data).

### Spectator mode (read-only watch links)

`https://<server>/?watch=<world>[&lineage=<n>][&stream=1]` shows a world with
no login:

- the painted map, with every lineage as soft coloured discs;
- the followed lineage's portrait, owner, head count and gene share;
- the largest lineages (click one to follow it) and a population chart;
- the clock, and a banner and sweeping tint while a catastrophe is under way.

Without `lineage` it follows the largest player lineage. `stream=1` hides the
buttons and enlarges the type for streams. A portrait window stacks the panel
under the map; a landscape one puts it beside.

**Data:** `GET /api/spectate` (server `game/spectate.rs`), public and
rate-limited like the other reads. It follows the info-leak rules (playtest 1,
finding 27):

- **No animals:** no ids, exact positions, sex, needs, actions, orders,
  pregnancies or groups. Each lineage's members are counted on 8-tile cells.
- **No genes:** a lineage shows its most common adult look, the coarse classes
  every player already sees on the map.
- **Not live:** the server answers from a copy taken at most once per simulated
  day, so nobody can follow animals tick by tick.
- **Public names only:** player names, head counts and shares are the public
  leaderboard's; catastrophes are the public chronicle's.

Players get their lineage's watch link from the Family tab (`Copy watch link`).

### The capture script

```sh
cargo build --release -p evo-server
(cd client && npm ci && npm run build)
node tools/marketing/capture.mjs                       # every scene, vertical and square, 12 s at 12 fps
node tools/marketing/capture.mjs --scenes builder --formats square --seconds 8
node tools/marketing/capture.mjs --no-ffmpeg           # PNG frames + a GIF from gif.mjs
```

Options: `--scenes` (comma list), `--formats vertical,square`, `--seconds`,
`--fps`, `--out DIR` (default `tools/marketing/out`), `--no-ffmpeg`,
`--keep-frames`. Playwright and Chromium must be installed, as for the client's
e2e tests; `EVO_SERVER_BIN` overrides the server binary.

**How it records.** Each scene and format gets a fresh in-memory server with a
fast clock (`--tick-ms`) and a fresh headless Chromium. The page is
screenshotted frame by frame, at 1080 × 1920 for vertical (a 540 × 960
viewport at 2×) and 1080 × 1080 for square. Frames are as far apart in game
time as the world moved while one was taken, so every video is a time-lapse.
With `ffmpeg` on the PATH, the frames become an H.264 MP4 (`yuv420p`,
faststart: it plays everywhere) and a palette-optimised GIF. Without ffmpeg,
they stay as a PNG sequence under `out/frames/` and the GIF comes from
`gif.mjs`, a dependency-free encoder (fixed 252-colour palette, ordered dither).

**Scenes:**

| Id | What it shows |
| --- | --- |
| `evolution` | A weird three-eyed, six-legged design founded on a fast-clock world, followed on the spectator view for years (map, portrait, population), ending on its family-tree card from the Family tab. Also writes its portrait and family-tree cards. |
| `boom-bust` | The wild red fox followed against its prey: the population chart rises and falls. Also writes the fox's portrait card. |
| `builder` | The body builder (`?builder=strider`) turned into a weird creature step by step: three eyes, swept back, six long legs, four fan ears, a club tail, horns, stripes, a long snout and fangs. |
| `catastrophe` | A world with a player lineage; a severe winter is started (the test-only `POST /api/e2e/catastrophe`, with `EVO_E2E_HOOKS=1`) and sweeps over the map with its banner. |

**Output** (in the publishing tool's format, PUBLISHING.md "Inputs", so
`node bin/evo-marketing.mjs draft --stories out/stories.json --manifest
out/manifest.json` takes it as is; `capture-manifest.mjs` builds both files):

- `out/<scene>-<format>.mp4` and `.gif`, the cards (`out/<scene>-portrait.png`,
  `out/evolution-family-tree.png`).
- `out/manifest.json`, `{ generated_at, source, fps, ffmpeg, items }`: one item
  per file, `{ id, path, type, duration_s, width, height, story_id, alt }`.
  Videos are `<scene>-<format>` (`video`), GIFs `<scene>-<format>-gif`
  (`image`), cards `<scene>-portrait` and `<scene>-family-tree` (`card`);
  `story_id` is the scene id.
- `out/stories.json`, `{ generated_at, source, fps, ffmpeg, stories }`: one
  story per scene, `{ id, kind, title, summary, facts, media, players,
  hashtags }`. The title, summary and facts come from what the run measured,
  so every number in the copy is in `facts`:

  | Scene | `kind` | Facts |
  | --- | --- | --- |
  | `evolution` | `lineage` | `lineage`, `founders`, `alive`, `years`, `day`, `share_pct` |
  | `boom-bust` | `predator_boom` | `lineage`, `start`, `peak`, `peak_year`, `low_after_peak`, `low_year`, `end`, `years`. It is told as "Boom and bust" only when the population fell to 60% of its peak or less after the peak |
  | `builder` | `devlog` | `eyes`, `legs`, `ears`, `ear_shape`, `tail`, `pattern`, `view_pct` |
  | `catastrophe` | `catastrophe` | `catastrophe`, `year`, `start_day`, `days`, `animals_before`/`after`, `lineages_before`/`after` |

  The facts are those of the first format's run (each format is its own run of
  the scene, so the numbers differ a little between them). `players` is
  always empty: the scenes' lineages are scripted, so copy names no player.
  `hashtags` holds the script's own ideas for people; the publisher picks its
  own by `kind`.

A full run (four scenes × two formats, 12 s at 12 fps) takes about 5 minutes
on 4 cores.

### Tests

- **Server:** `game::spectate::tests` (counts add up, coarse cells, nothing
  private in the JSON); `worlds::spectators_watch_a_world_without_logging_in`
  (no login, per world, a player's lineage by its public name, one copy per
  simulated day).
- **Client unit tests:** `client/src/spectate.test.ts` (watch link, followed
  lineage, density fades, history, captions) and
  `client/src/share/*.test.ts` (text wrap, tree layout, share method and
  fallbacks, file names, watch URLs).
- **Publishing format:** `tools/marketing/test/capture.test.mjs` (part of
  `npm test` here) reads `capture.mjs`'s files back with the publisher's
  loaders, and checks every number in a story's copy is one of its facts.
- **End-to-end:** `client/e2e/spectate.mjs` (part of `npm run e2e`): a
  logged-out watch link shows the lineage and the map; Share card downloads a
  1080 × 1350 PNG; the Family tab's Share bar downloads both cards.
