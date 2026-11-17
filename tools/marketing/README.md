# Marketing content from the real game

Tooling that turns the running game into shareable media: cards players
share from the game, a spectator link for streams, and a capture script that
records scripted scenes as short videos. Everything is drawn by the game
itself (the creature art library, the painted map, the real simulation);
nothing is mocked.

Generated media goes to `tools/marketing/out/` (git-ignored). A few small
samples live in `docs/marketing/samples/`.

## 1. Shareable cards (in the game)

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

## 2. Spectator mode (read-only watch links)

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

## 3. Capture script

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

**Output:** `out/<scene>-<format>.mp4` and `.gif`, the cards
(`out/<scene>-portrait.png`, `out/evolution-family-tree.png`) and
`out/manifest.json`:

```json
{
  "generated": "2026-11-17T14:00:00.000Z", "ffmpeg": true, "fps": 12,
  "items": [{
    "id": "evolution",
    "title": "A species evolving over the years",
    "caption": "Forty founders, a few game years: …",
    "hashtags": ["#EvolutionMMO", "#indiegame", "#gamedev", "#evolution", "#simulation", "#ArtificialLife", "#procedural"],
    "duration_s": 12, "fps": 12,
    "files": { "vertical": { "mp4": "evolution-vertical.mp4", "gif": "evolution-vertical.gif" }, "square": { … } },
    "cards": { "family-tree": "evolution-family-tree.png", "portrait": "evolution-portrait.png" }
  }]
}
```

A full run (four scenes × two formats, 12 s at 12 fps) takes about 5 minutes
on 4 cores.

## Tests

- **Server:** `game::spectate::tests` (counts add up, coarse cells, nothing
  private in the JSON); `worlds::spectators_watch_a_world_without_logging_in`
  (no login, per world, a player's lineage by its public name, one copy per
  simulated day).
- **Client unit tests:** `client/src/spectate.test.ts` (watch link, followed
  lineage, density fades, history, captions) and
  `client/src/share/*.test.ts` (text wrap, tree layout, share method and
  fallbacks, file names, watch URLs).
- **End-to-end:** `client/e2e/spectate.mjs` (part of `npm run e2e`): a
  logged-out watch link shows the lineage and the map; Share card downloads a
  1080 × 1350 PNG; the Family tab's Share bar downloads both cards.
