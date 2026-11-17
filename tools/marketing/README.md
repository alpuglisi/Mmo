# Marketing content tools

Tools that turn the simulation into posts, clips and devlog images. Nothing
they generate belongs in git: output goes to `film-out/` or
`tools/marketing/out/` (both ignored), and `*.png`, `*.mp4` are ignored
repo-wide.

| Tool | Runs | Makes |
|---|---|---|
| `evo-sim film` (this page) | headless, Rust only (ffmpeg optional) | map time-lapses in 9:16 and 1:1, `stories.json`, charts |
| `tools/marketing/capture.mjs` | in the browser client via Playwright | in-game captures (see its section, maintained with that script) |

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
| `stories.json` | notable events, best first (below) |
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
`--formats 9x16,1x1`, `--title TEXT`, `--no-frames` (timeline, stories and
charts only, fast), `--no-video`, `--keep-frames`. Every world option of
`evo-sim run` also works: `--seed`, `--size`, `--meat`, `--food`,
`--catastrophes`, `--catastrophe KIND@DAY` (e.g. `epidemic@200`),
`--migrants`, `--niches`, `--founding`, `--design`, `--set NAME=V`,
`--threads`.

To re-mine stories or redraw charts from a finished run without simulating
again (after changing the detectors, say):

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

### Stories (`stories.json`)

An array, best first. Every story looks like this:

```json
{
  "kind": "comeback",
  "score": 0.93,
  "start_day": 140, "end_day": 262,
  "when": "year 2, spring (day 140) – year 3, spring (day 262)",
  "hook": "Back from the brink: the Marsh newt fell from 120 to just 9 — and recovered to 95.",
  "lineages": ["Marsh newt"],
  "data": { "peak": 120, "low": 9, "low_day": 171, "recovered": 95, "population": [[140, 120], ...] }
}
```

`hook` is a ready caption line. Each further story of a kind already in the list
scores 8% less than it would alone, so one busy kind doesn't crowd the top.
`data` holds the numbers behind it (series are
`[day, value]` pairs, thinned to about 40 points) so the posting pipeline can
write its own text or chart. Days start at 0, and a year is 120 days (four
30-day seasons). Use `start_day`/`end_day` to cut the matching stretch out of
the film: frame `n` shows day `(n-1) × frame-days`.

| kind | Detected when | Score |
|---|---|---|
| `speciation` | the chronicle records a species splitting off another and it holds at least 20 days (shorter ones are the species clusterer flickering) | 0.6–0.95 (longer-lived, bigger, surviving species score higher) |
| `species_end` | a species that held 30+ days and reached 20 adults goes extinct; the hook names the daughter species its line lives on in, if any | 0.45–0.75 |
| `comeback` | a lineage drops to ≤15% of an earlier peak (and ≤30), then climbs back to ≥50% of that peak and ≥3× its low | 0.7–0.95 by depth |
| `boom_crash` | the 5-day-smoothed population at least doubles to a peak (≥20) within a year, then falls to ≤40% of it within a year; "Predator boom and bust" if the lineage's mean carnivory is above 0.5 | 0.55–0.9 |
| `catastrophe` | a severe winter, megadrought, epidemic or flood year happened | 0.5–1.0 by the share of life lost and severity |
| `trait_shift` | the 30-day-smoothed mean of a gene moves ≥0.15 within two years, in a lineage of 10 or more (e.g. "The Pack hunters evolved pack hunting") | 0.4–0.95 by size of the move |
| `extinction` | a lineage that reached at least 5 dies out | 0.45–0.75 by its peak |

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
