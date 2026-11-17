# Launch marketing: drafting, approval, scheduling, publishing

`tools/marketing/` is a separate Node package (Node 20+, no required
dependencies) that turns the generated media and simulation stories into
platform-ready drafts, puts them in a local approval queue, schedules the
approved ones on a content calendar, and publishes through each platform's
official API, on the owner's own accounts only. Dry-run is the default
everywhere; posting needs `--live`.

```
stories.json + manifest.json ──draft──▶ queue (pending)
                                           │ human: approve / edit / reject   (CLI or local web page)
                                           ▼
                                        approved ──schedule──▶ scheduled ──publish --live──▶ posted
                                                                   └─ TikTok/Instagram without app approval, devlog: manual export folder
```

## Hard boundaries

These are design rules of the tool and rules for whoever operates it.

1. **No fake or sock-puppet accounts.** Every adapter posts as the owner's
   one account per platform. There is no multi-account support, by design.
2. **No automated engagement.** Nothing here likes, upvotes, reposts,
   follows, replies or comments. Adapters only create the owner's own posts
   and read their public metrics.
3. **No mass unsolicited messages.** No DMs at all. The newsletter goes only
   to people who subscribed (Buttondown's list, or SMTP to a list where each
   address has `optedIn: true`), with an unsubscribe link and
   `List-Unsubscribe` header. Creator outreach produces *drafts* that a
   human personalises and sends one by one; each needs a specific reason
   for that creator, and a run drafts at most 15.
4. **No posting into third-party communities without a human approving that
   post.** Reddit posts go to the owner's own subreddits or profile by
   default; another subreddit needs `approve --target r/Name
   --confirm-third-party` on that one draft (after reading its rules), and
   the adapter re-checks that at post time. Discord posts only to the owner's
   own webhook. Community suggestions (`promote`) are a reading list, not
   an action.
5. **No evading detection or rate limits.** Requests identify the tool
   (Reddit's required User-Agent, no browser spoofing), a minimum interval
   separates requests per platform, daily caps sit well below platform
   limits, and a 429 is waited out once as the platform asks (if the wait is
   two minutes or less) or the run stops for that platform. No proxies, no
   identity rotation, no scraping, no unofficial APIs.
6. **Nothing posts without approval.** Drafts start `pending`. Editing an
   approved draft sends it back to `pending`, and the publisher refuses any
   item whose content changed since approval.
7. **Claims stay factual.** Copy may only state numbers that appear in the
   story data (checked automatically), names players only if the story marks
   them `optedIn`, and never invents release dates, prices or platforms.
8. **Credentials never enter the repository.** Secrets come from environment
   variables; `marketing.config.json`, `.env*`, `data/` and `out/` are
   gitignored, and the config loader refuses a `secrets` block in the file.

## Quick start

```bash
cd tools/marketing
npm install            # optional: @anthropic-ai/sdk (Claude copy), nodemailer (SMTP)
cp marketing.config.example.json marketing.config.json   # edit: timezone, site URL, own subreddit...
node bin/evo-marketing.mjs draft --stories ../../out/stories.json --manifest ../../out/capture/manifest.json
node bin/evo-marketing.mjs review          # http://127.0.0.1:8787/ : edit, approve, reject
node bin/evo-marketing.mjs schedule
node bin/evo-marketing.mjs calendar
node bin/evo-marketing.mjs publish         # dry run: shows every request it would make
node bin/evo-marketing.mjs publish --live  # posts what is due
npm test
```

A typical routine: draft whenever new footage or stories arrive, review once
a day, and run `publish --live` from a cron job or by hand every 30–60
minutes; it only posts items whose slot has come.

## Inputs

The generator branches own these formats; the loaders accept a few wrapper
shapes and field-name variants so they keep working while those settle
(`src/stories.mjs`). Examples: `examples/stories.example.json`,
`examples/manifest.example.json`.

**stories.json** (perf branch): `[Story]` or `{ "stories": [...] }`.

| Field | Meaning |
| --- | --- |
| `id` | Stable id; becomes `utm_content` and the duplicate key. |
| `kind` | e.g. `catastrophe`, `extinction`, `speciation`, `lineage`, `era`, `timelapse`; picks hashtags and communities. |
| `title` | One-line headline, true as stated. |
| `summary` | One to three sentences. |
| `facts` | `{ name: value }`: the only numbers copy may cite. |
| `media` | Manifest ids (or file names) for this story. |
| `players` | `[{ name, optedIn }]`; only `optedIn: true` names are ever used. |

**manifest.json** (shard branch): `[Item]` or `{ "items": [...] }`, each
`{ id, path, type: video|image|card|chart, duration_s, width, height,
story_id, alt }`. Paths are relative to the manifest. Vertical 9:16 video is
preferred for TikTok, Shorts and Reels; landscape gets a warning.

## Drafting

`draft` writes one draft per story per platform: X, Bluesky, Mastodon,
TikTok, YouTube Shorts, Instagram Reels, Reddit, Discord, devlog (Markdown)
and newsletter. Video-only platforms are skipped for stories without video.

- **With `ANTHROPIC_API_KEY`** (and `npm install`): one Claude request per
  story returns copy for every platform as JSON (structured outputs), with
  the style guide and fact rules in the system prompt and each field's
  character budget (limit minus the link and hashtags the tool appends) in
  the request. The model is `claude-opus-5-5` at `effort: medium`;
  change it with `claude.model` / `claude.effort` in the config or
  `MARKETING_CLAUDE_MODEL`. Requests opt into the API's server-side
  refusal fallback (`fallbacks: "default"`). Copy that fails a check is sent
  back once with the errors; if it still fails, or the API errors or
  declines, that platform gets the template draft and the reviewer sees why.
- **Without a key** (or `--no-claude`): deterministic templates built only
  from the story.

Every draft is then checked (`src/drafting/validate.mjs`): the length limits
below with each platform's own counting, hashtag count, banned phrases,
emoji count, YouTube's ban on `<` `>`, required video, video length and
orientation, alt text, and the fact check (every number must appear in the
story). Over-long copy is cut at a sentence or word boundary, never inside a
link.

| Platform | Field limits (counting) | Tags |
| --- | --- | --- |
| X | text 280 (weighted: URL = 23, CJK/emoji = 2) | ≤ 2 |
| Bluesky | text 300 graphemes | ≤ 2 |
| Mastodon | text 500 (URL = 23; `mastodon.maxChars` for your instance) | ≤ 4 |
| TikTok | caption 2,200 | ≤ 5 |
| YouTube Shorts | title 100, description 5,000 bytes; video ≤ 3 min, vertical | ≤ 3 |
| Instagram Reels | caption 2,200; video ≤ 90 s | ≤ 5 |
| Reddit | title 300, body 40,000 | 0 |
| Discord | content 2,000; attachments ≤ 10 MB | 0 |
| Devlog | title 120, Markdown body | 0 |
| Newsletter | subject 78, plain-text body | 0 |

## Review and the queue

The queue is `data/queue.json` (local, gitignored). Each item keeps its full
history (created, edited with the previous text, approved by whom, scheduled,
posted or failed).

```bash
node bin/evo-marketing.mjs queue --status pending
node bin/evo-marketing.mjs show 3f9a
node bin/evo-marketing.mjs edit 3f9a --field text --value "New copy..."   # or: edit 3f9a --editor
node bin/evo-marketing.mjs approve 3f9a
node bin/evo-marketing.mjs approve 77c1 --target r/IndieGaming --confirm-third-party
node bin/evo-marketing.mjs reject 3f9a --reason "too dry"
node bin/evo-marketing.mjs review --port 8787
```

The review page binds to 127.0.0.1 only, shows each draft with its media,
checks and editable fields, and requires a per-run token for every change.

Duplicates: a draft is not queued when the same story is already queued for
that platform, or when identical text (ignoring UTM parameters) is; rejected
items don't count. At publish time an item identical to one already posted on
that platform is skipped.

## Content calendar

`schedule` gives each approved draft the next free slot on its channel, in
approval order, in `timezone` (IANA name). Defaults (`src/config.mjs`,
override per channel under `calendar.channels`):

| Channel | Posts/week | Windows (local) |
| --- | --- | --- |
| X | 5 | Mon–Fri 09–11, 17–19 |
| Bluesky | 5 | Mon–Fri 09–12 |
| Mastodon | 4 | Mon–Fri 10–13 |
| TikTok | 4 | Tue–Sat 18–21 |
| YouTube Shorts | 3 | Thu–Sat 15–18 |
| Instagram | 3 | Mon, Wed, Fri 11–13 |
| Reddit | 1 (96 h apart) | Sat–Sun 14–17 |
| Discord | 2 | Tue, Fri 18–20 |
| Devlog | 1 | Thu 15–17 |
| Newsletter | 1 | Tue 15–16 |

Every channel also has at most one post a day and 20 hours between posts
unless overridden. These windows are reasonable starting points, not
measured; adjust them from `report` after a few weeks.

## Publishing

`publish` takes every scheduled item whose time has come (or `--id a,b`).
Without `--live` it runs each adapter end to end against recorded fake
responses: no network, placeholder credentials if none are set, queue
untouched. `--verbose` prints each request with secrets redacted. With
`--live` it posts, then marks the item `posted` (with the post URL and id)
or `failed` (with the error; approve again to retry).

TikTok and Instagram (until their apps are approved and enabled), the devlog,
and `email.provider: "manual"` go to `out/manual/<platform>/<date>-<id>/`:
the media, one text file per field, and a README. After posting by hand,
record it: `node bin/evo-marketing.mjs posted <id> --url <post url>`.
`export` writes these folders for any approved draft on demand.

`check` lists which channels are API-ready and which variables are set.

## Platform setup (owner steps)

Set the variables in your shell, a secrets manager, or a `.env` file you
source (never committed). Scopes listed are the minimum the adapters use.

### Claude (drafting copy)

1. Create an API key in the Claude Console (console.anthropic.com → API keys).
2. `export ANTHROPIC_API_KEY=...` and `npm install` in `tools/marketing`.

### Bluesky

1. Bluesky app → Settings → Privacy and security → App passwords → Add (no DM access needed).
2. `BLUESKY_HANDLE=yourname.bsky.social`, `BLUESKY_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx`.
3. Self-hosted PDS: set `bluesky.service`.

Posts use `com.atproto.repo.createRecord` with link and hashtag facets;
images and video go through `com.atproto.repo.uploadBlob`. Before the first
live video post, try one by hand: if Bluesky doesn't process the uploaded
video, switch that step to the video service (`app.bsky.video.uploadVideo`,
which needs a service-auth token), a small change in `src/adapters/bluesky.mjs`.

### Mastodon

1. On your instance: Preferences → Development → New application.
2. Scopes: `write:statuses`, `write:media`, `read:statuses`.
3. `MASTODON_INSTANCE=mastodon.social`, `MASTODON_ACCESS_TOKEN=<your access token>`.
4. Many instances require bot-like accounts to be marked as automated:
   Preferences → Profile → "This is an automated account" if you post
   mostly through the tool, and check your instance's rules.

### X (API v2)

1. developer.x.com → create a Project and App on a plan that allows posting
   (the free tier's monthly post cap is small; check the current limits).
2. User authentication settings: OAuth 2.0, type "Web App / Automated App",
   a callback URL you control; scopes `tweet.read tweet.write users.read media.write offline.access`.
3. Do the OAuth 2.0 PKCE authorization once as the owner (any OAuth tool, or
   X's docs example) to get a refresh token.
4. `X_CLIENT_ID`, `X_REFRESH_TOKEN` (and `X_CLIENT_SECRET` for a confidential
   client). The adapter refreshes the 2-hour access token and stores the
   rotated refresh token in `data/secrets/x-token.json` (mode 600). For a
   quick test, `X_USER_ACCESS_TOKEN` alone also works until it expires.
5. Uses `POST /2/tweets` and the v2 chunked media upload
   (`/2/media/upload/initialize`, `/append`, `/finalize`). X has changed its
   media endpoints before: confirm them against the current docs before the
   first live post.
6. Mark the account as automated in X settings (Account information →
   Automation) as X's automation rules ask.

### YouTube Shorts (Data API v3)

1. console.cloud.google.com → new project → enable "YouTube Data API v3".
2. OAuth consent screen (External; add yourself as a test user), then
   Credentials → OAuth client ID (Desktop app).
3. Get a refresh token once for scopes
   `https://www.googleapis.com/auth/youtube.upload` and
   `https://www.googleapis.com/auth/youtube.readonly` (e.g. OAuth Playground
   with your own client ID, or any installed-app OAuth helper).
4. `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN`.
5. Videos from unverified projects are locked private: request the API
   audit (YouTube API Services compliance) before launch. Uploads cost a
   large share of the default daily quota; check Google's quota calculator.
6. A vertical video of up to three minutes becomes a Short; the adapter adds
   `#Shorts` to the title when it fits.

### Reddit

1. reddit.com/prefs/apps → "create another app" → type **script**,
   redirect URI `http://localhost:8080` (unused).
2. `REDDIT_CLIENT_ID` (under the app name), `REDDIT_CLIENT_SECRET`,
   `REDDIT_USERNAME`, `REDDIT_PASSWORD` (the owner's account; script apps
   don't work with 2FA-only logins, so use an app password flow or disable
   2FA for this account knowingly).
3. Config: `reddit.ownSubreddits: ["YourSub"]`, `reddit.defaultTarget`.
4. Read Reddit's Responsible Builder Policy and API terms; register for API
   access if Reddit asks. Reddit's published rate limit is per OAuth client (100 queries a minute at the time of writing).
5. Self-promotion: Reddit expects most of an account's activity to be
   genuine participation, not links to your own project. Use the tool for
   your own subreddit; participate in other communities yourself.

### Discord

1. Your server → Server Settings → Integrations → Webhooks → New Webhook,
   pick the announcements channel, copy the URL.
2. `DISCORD_WEBHOOK_URL=https://discord.com/api/webhooks/<id>/<token>`.
3. Optional: `discord.username`; `discord.allowEveryone: true` lets
   `@everyone` ping (off by default).

### Newsletter

- **Buttondown** (recommended): `email.provider: "buttondown"`,
  `BUTTONDOWN_API_KEY` from Settings → API. Creates a draft in Buttondown;
  set `email.sendNow: true` to send immediately.
- **SMTP**: `npm install`, `email.provider: "smtp"`, `email.from`,
  `email.listFile` (JSON `[{ "email", "optedIn": true, "optedInAt" }]`),
  `email.unsubscribeUrl` (a page you run), `SMTP_URL=smtps://user:pass@smtp.example.com:465`.
  Sends one message per second; addresses without `optedIn: true` are skipped.

### TikTok (needs an approved app)

1. developers.tiktok.com → create an app, add the **Content Posting API**
   product, scopes `video.upload` (drafts to the inbox) and, for direct
   posting, `video.publish`. Submit for review.
2. Authorize the owner's account (Login Kit, OAuth) to get a user access
   token: `TIKTOK_ACCESS_TOKEN` (refresh it per TikTok's docs; tokens last 24 h).
3. Config: `tiktok.enabled: true`, `tiktok.appApproved: true`,
   `tiktok.mode: "inbox"` (default: the video lands in TikTok's inbox and
   you finish it in the app) or `"direct"` with `tiktok.privacyLevel` from the
   creator's options. Until TikTok audits the app, direct posts can only be
   private (`SELF_ONLY`).
4. Until then: manual export folders.

### Instagram Reels (needs an approved app)

1. A Business or Creator Instagram account linked to a Facebook Page (or use
   Instagram Login).
2. developers.facebook.com → app → Instagram API; permissions
   `instagram_basic` + `instagram_content_publish` (Facebook Login) or
   `instagram_business_basic` + `instagram_business_content_publish`
   (Instagram Login; set `instagram.graphHost: "graph.instagram.com"`).
   App Review is required for production.
3. Long-lived token: `INSTAGRAM_ACCESS_TOKEN`; account id: `INSTAGRAM_USER_ID`.
4. The API downloads the video from a public URL: upload the exported file
   to your site/CDN and set `instagram.publicMediaBaseUrl`.
5. Config: `instagram.enabled: true`, `instagram.appApproved: true`. The API
   caps published posts per 24 hours (query `content_publishing_limit`);
   the tool caps itself at 2.

## Rate limits and automation rules

| Platform | What the tool does |
| --- | --- |
| All | ≥ 1–5 s between requests; daily caps in `dailyCaps`; one wait on 429 if ≤ 120 s, else stop. |
| X | Respects `x-rate-limit-reset`; account labelled automated; no replies, likes, follows or DMs. |
| Bluesky | Own repo only; `createRecord` only. |
| Mastodon | `Idempotency-Key` so a retry never double-posts; reads `X-RateLimit-Reset`. |
| Reddit | Descriptive User-Agent; ≤ 1 post/day; own subreddit unless approved; no voting or crossposting automation. |
| YouTube | Within the quota; uploads only. |
| TikTok / Instagram | Official content-posting APIs only, behind app approval and config flags. |

## Promotion

```bash
node bin/evo-marketing.mjs promote --stories stories.json     # hashtags per platform + communities to consider
node bin/evo-marketing.mjs press-kit --stories stories.json --manifest manifest.json   # out/press-kit/
node bin/evo-marketing.mjs outreach --creators creators.json --stories stories.json    # out/outreach/*.md drafts
node bin/evo-marketing.mjs links --story y3-severe-winter     # UTM links per channel
```

- **Communities** (`src/promote.mjs`) are suggestions with a note each; rules
  change, so read each community's rules before posting there.
- **Press kit**: fact sheet (unknown facts show as TBD until you fill
  `pressKit` in the config), description, notable moments, media table.
- **Outreach**: `creators.json` is a list you compile by hand:
  `[{ name, channel, email, why, recentWork }]`. `why` must say why this
  creator specifically (20+ characters) or no draft is written. Send each
  from your own mail client after editing; one follow-up at most.

**UTM scheme**: `utm_source` = platform (`x`, `bluesky`, `reddit`, `press`,
`outreach`...), `utm_medium` = `social` | `video` | `community` | `email` |
`devlog` | `press` | `outreach`, `utm_campaign` = `site.campaign`,
`utm_content` = story id.

## Analytics

```bash
node bin/evo-marketing.mjs metrics --live   # snapshot public metrics into data/metrics.json
node bin/evo-marketing.mjs report           # Markdown summary: per channel, top posts
```

Metrics come from the same official APIs: Bluesky (`getPosts`, public),
Mastodon (status counts), X (`public_metrics`), YouTube (`statistics`),
Reddit (score and comments), Instagram (media insights). Discord, email and
TikTok inbox uploads have none here. Clicks come from your site analytics:
filter by `utm_campaign`, group by `utm_source` and `utm_content`.

## Configuration

`marketing.config.json` (copy the example; gitignored) or
`EVO_MARKETING_CONFIG=path`. Keys: `timezone`, `site.url`, `site.campaign`,
`claude`, `calendar.defaults` / `calendar.channels.<p>` (`postsPerWeek`,
`maxPerDay`, `minGapHours`, `windows: [{ days: [0-6, Sunday = 0], start, end }]`),
`dailyCaps`, and one block per platform as above. Environment variables:

| Variable | For |
| --- | --- |
| `ANTHROPIC_API_KEY`, `MARKETING_CLAUDE_MODEL` | Claude drafting |
| `BLUESKY_HANDLE`, `BLUESKY_APP_PASSWORD` | Bluesky |
| `MASTODON_INSTANCE`, `MASTODON_ACCESS_TOKEN` | Mastodon |
| `X_CLIENT_ID`, `X_CLIENT_SECRET`, `X_REFRESH_TOKEN` or `X_USER_ACCESS_TOKEN` | X |
| `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN` | YouTube |
| `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USERNAME`, `REDDIT_PASSWORD` | Reddit |
| `DISCORD_WEBHOOK_URL` | Discord |
| `BUTTONDOWN_API_KEY` or `SMTP_URL` | Newsletter |
| `TIKTOK_ACCESS_TOKEN` | TikTok |
| `INSTAGRAM_USER_ID`, `INSTAGRAM_ACCESS_TOKEN` | Instagram |

## Tests

`npm test` (Node's built-in runner, no network): text measurement per
platform, drafting within limits for every platform, the fact check, the
Claude path against a fake client (structured-output request, retry with
feedback, fallback to templates on bad copy, refusal or error), queue states,
duplicates, approval and Reddit third-party gating, the calendar (time
zones, DST, weekly/daily caps, gaps), every adapter's requests against a
mocked `fetch`, 429 handling, dry-run redaction, manual export, metrics, the
outreach and press-kit guards, and the review page's token check and
escaping.

None of the adapters has posted to a real account yet: they were built from
the platforms' documentation and verified against mocks. Do one live post per
platform by hand (`publish --live --id <one item>`) and check it before
leaving `publish --live` on a schedule.

## Layout

```
bin/evo-marketing.mjs     CLI
src/platforms.mjs         per-platform limits
src/text.mjs              length counting per platform, truncation
src/style.mjs             brand voice (mirrors STYLE.md)
src/stories.mjs           input loaders
src/drafting/             templates, Claude copy, assembly, validation
src/queue.mjs             approval queue
src/calendar.mjs          content calendar
src/http.mjs              dry-run/live HTTP, rate limits, redaction
src/adapters/             one file per platform + manual export
src/publish.mjs           publisher
src/promote.mjs           communities, press kit, outreach
src/analytics.mjs         metrics and summary
src/review-server.mjs     local review page
```
