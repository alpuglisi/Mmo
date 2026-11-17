# The starting design

The design editor opens on a starting design that a new player can found
as it is. It used to be every gene at its default: a small, bold, loosely
grouped herbivore. In play-throughs that lineage often died out within a
few weeks, and the player was left with nothing to do.

The starting design is now a sturdy, social, cautious herbivore. It
changes three genes away from their defaults, and all three are free:

| Gene | Default | Starter |
| --- | --- | --- |
| body size | 0.3 | 0.5 |
| group tolerance | 0.5 | 0.9 |
| boldness | 0.5 | 0.3 |

Everything else stays at the defaults: warm-blooded, live birth, 2 young
per birth, 20 founders. It costs 2.5 of the 100 points, the same as
before, so the whole budget is still the player's to spend. No balance
tunables or server gene defaults changed; the client applies the starter
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

Each row is 24 seeds. The share is lineages alive after 120 days, with
the median population at the end in brackets.

| World | Old default | Starter |
| --- | --- | --- |
| 128×128 (the server's default), 120-day years, joined on day 30, seeds 1–24 | 22/24, 92% (129) | 24/24, 100% (193) |
| 64×64, 480-day years (the dev and e2e setup), joined on day 30, seeds 1–24 | 16/24, 67% (36) | 24/24, 100% (258) |
| 64×64, 480-day years, joined on day 30, seeds 25–48 | 15/24, 62% (9) | 24/24, 100% (303) |
| 64×64, 480-day years, joined on day 200, seeds 49–72 | 9/24, 38% (0) | 24/24, 100% (173) |
| 128×128, 120-day years, joined on day 200, seeds 73–96 | ROW_DEFAULT | ROW_STARTER |

Two variants that also spent points did no better on the 64×64 seeds 1–24:
sentry instinct 0.5, large ears 0.5 and fur 0.4 on top of the starter
(7.7 points), and the same with 30 founders (14.7 points). Both kept
24/24 alive. A design that costs nothing extra does the job, so the
starter leaves the points for the player.

The weakest starter run was one seed in the day-200 join on the small map,
which ended with a single organism. "Large majority" holds; "every world"
is not promised. A lineage that does die out can now found again
(re-founding, below).

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
