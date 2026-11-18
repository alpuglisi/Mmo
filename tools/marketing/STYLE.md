# Evolution MMO: voice and style

One page, for anyone (human or Claude) writing posts. The machine-readable
copy of these rules is `src/style.mjs`; keep the two in step.

## Who we sound like

A field naturalist narrating what the simulation did. Curious, concrete,
warm. The event is the hook; the game is the context.

- **Concrete.** Name the species, the season, the year, the number:
  "The herd halved from 1,240 to 610 in the year-3 winter", not
  "a massive die-off".
- **True.** Every claim about an event comes from the story data
  (`stories.json`). No invented numbers, players, dates, prices, release
  dates or platforms. The drafter checks this: any number not in the story
  fails the draft.
- **Emergent, not scripted.** Say so when it matters: "Nothing here was
  scripted."
- **Plain words, short sentences.** No hype words (revolutionary, epic,
  insane, ultimate, game-changer...), no superlatives about the game itself.
- **Respectful of players.** Name a player only when the story marks them
  as opted in to being featured.
- **Light on decoration.** At most one emoji per post, none in titles;
  one or two hashtags on X and Bluesky, up to four on Mastodon, a few on
  video captions, none on Reddit.
- **No engagement bait.** Never ask people to like, upvote, share, follow
  or subscribe.

## Claims to avoid (checked against playtest 3)

The drafter rejects copy that makes these claims (`UNSUPPORTED_CLAIMS` in
`src/style.mjs`); update the list when the game changes.

- **Hunting other players.** Predators can attack rival lineages only in
  narrow cases (size limits, a founding grace, burrows), and in playtest 3
  no player predator lasted long enough to use it. Show wild predator–prey
  stories instead.
- **Trading as the core loop.** The core is designing and breeding your own
  lineage; the stud market is a side system, and cross-player matings rarely
  completed in playtest 3.
- **Shipped mobile apps.** The iOS and Android wrappers build, but have not
  shipped.
- **"Balanced" or "no dominant strategy."** Each playtest found a new
  dominant build; say the balance is being tuned.
- **Player counts** we can't back.

Staged footage says so: a catastrophe triggered for a capture is described
as "triggered for this capture in a test world".

## Per channel

| Channel | Shape |
| --- | --- |
| X, Bluesky, Mastodon | One or two sentences about the event, then the tracked link and tags. |
| TikTok, Shorts, Reels | A caption that reads well under the clip; "link in bio" (links in captions aren't clickable). |
| Reddit | Written as the developer, openly. Plain title stating the event, no clickbait. Body explains what happened and invites questions. |
| Discord | Bold title line, one or two lines of detail, clip attached. No @everyone unless configured. |
| Devlog | Markdown: heading, a few paragraphs, embedded media, "By the numbers", a short "About the game". |
| Newsletter | Plain text, friendly, short; why you're receiving it and how to unsubscribe. |

## Facts we may always state

- Each player designs a founding organism on a 100-point budget.
- Players steer their lineage through breeding rather than direct editing.
- The world is persistent and shared: player lineages live alongside a wild ecosystem.
- The score is genetic legacy: your alleles' share of the breeding adults alive.
- Seasons, weather, droughts, floods and catastrophes shape who survives.
- It runs in a web browser; mobile apps are in development.

Anything else (release date, price, player numbers) needs a source and goes
into the config (`pressKit`) or the story first.
