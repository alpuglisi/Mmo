# The starting design

The design editor opens on a starting design that a new player can found
as it is. It used to be every gene at its default: a small, bold, loosely
grouped herbivore. In play-throughs that lineage often died out within a
few weeks, and the player was left with nothing to do.

The starting design is now a sturdy, social, cautious herbivore that is
ready for its first winter:

| Gene | Default | Starter | Points |
| --- | --- | --- | --- |
| body size | 0.3 | 0.5 | free |
| group tolerance | 0.5 | 0.9 | free |
| boldness | 0.5 | 0.3 | free |
| fur | 0 | 0.6 | 1.8 |
| fat stores | 0 | 0.6 | 1.2 |
| hibernation | 0 | 0.5 | 4.0 |

Everything else stays at the defaults: warm-blooded, live birth, 2 young
per birth, 20 founders. The whole design costs 9.5 of the 100 points
(it was 2.5), so 90.5 are left for the player. No balance tunables or
server gene defaults changed; the client applies the starter
(`client/src/designMorph.ts` `STARTER_TRAITS`), and "Reset traits" goes
back to it.

## How it was measured

`crates/server/examples/starter_survival.rs` founds a design through the
server's own path (`DesignSpec::to_design`, `Game::found`) into fresh
standard worlds. The worlds have the wild ecosystem and catastrophes on,
as in the game. Each world first runs on its own for a warm-up, the way a
newcomer finds a world already going. The example then runs 120 more game
days and counts the lineages still alive.

```
cargo run --release -p evo-server --example starter_survival -- \
  [--design FILE.json] [--seeds N] [--first-seed N] [--size N] \
  [--warmup DAYS] [--days DAYS] [--days-per-year N]
```

Without `--design` it founds the old default design. A design file is
the client's `POST /api/lineage` body.

## Results

Each cell is lineages alive after 120 days, with the median population at
the end in brackets. A is the first, free-only starter (body size,
group tolerance and boldness); H is the chosen starter.

| World (24 seeds each) | Old default | A (2.5 points) | H, the starter (9.5 points) |
| --- | --- | --- | --- |
| 128×128 (the server's default), 120-day years, joined on day 30, seeds 1–24 | 22/24, 92% (129) | 24/24, 100% (193) | 24/24, 100% (215) |
| 64×64, 480-day years (the dev and e2e setup), joined on day 30, seeds 1–24 | 16/24, 67% (36) | 24/24, 100% (258) | 24/24, 100% (242) |
| 64×64, 480-day years, joined on day 30, seeds 25–48 | 15/24, 62% (9) | 24/24, 100% (303) | not run |
| 64×64, 480-day years, joined on day 200, seeds 49–72 | 9/24, 38% (0) | 24/24, 100% (173) | not run |
| 128×128, 120-day years, joined on day 200, seeds 73–96 | 3/24, 12% (0) | 14/24, 58% (9) | 18/24, 75% (21) |

The hardest row is a newcomer joining a default-size world in late autumn
of year 2, with catastrophes on. Free genes alone left that newcomer at
58%. Deaths there were mostly starvation and cold exposure, with predation
far behind, and some lineages starved within days of arriving as winter
set in. Screening on seeds 73–84 (12 worlds each) tried these:

| Variant on top of A | Points | Alive |
| --- | --- | --- |
| A itself | 2.5 | 6/12 |
| fur 0.6, fat stores 0.6 | 5.5 | 7/12 |
| the same, body size 0.4 (lighter) | 5.5 | 5/12 (much more predation) |
| fur 0.6, fat stores 0.6, hibernation 0.5 (H) | 9.5 | 9/12 |
| pickiness 0.2, foraging range 0.8 (free) | 2.5 | 5/12 |
| H plus pickiness 0.2 and foraging range 0.8 | 9.5 | 8/12 |
| H with 40 founders | 23.5 | 7/12 |

Placing founders by the food actually present rather than the ground's
potential (a change to the sim's `pick_home`) made no difference at all
on those seeds, so it was dropped.

The result: the starter keeps at least three in four lineages alive in
every setting measured, and every one in the settings most new players
meet. The hardest case (joining just before a year-2 winter) still loses
about one in four. A design alone gets no further without spending most
of the budget. A lineage that dies out can found again at once
(re-founding, below). Whether newcomers should get help when they join
just before winter, such as starting fed or being warned about the
season, is a game-design question left open here.

## Re-founding

A player whose lineage has died out can found a new one straight away.
Died out means no living organisms and no eggs waiting to hatch.

- The player keeps their renown, ledger, records and titles. There is no
  second founding grant.
- The old lineage stays in the history as extinct.
- A lineage that is still alive refuses a second founding (409
  `already_founded`).

Server: `Game::is_extinct` and `Game::found`, tested by
`refounding_only_after_extinction`. Client: the Lineage tab and the
inspector offer "Start a new lineage". The e2e test is
`client/e2e/refound.mjs`. It uses the test-only route
`POST /api/e2e/wipe-out`, which the server mounts only when it runs with
`EVO_E2E_HOOKS=1`.
