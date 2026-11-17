# Load test: how many players one evo-server holds

**Short answer:** on a 4-core container that also runs the load generator
(up to 1 core of it), one world on one `evo-server` holds **4000 connected
heavy players at the 1 s tick** after the fixes below: 1.0 ticks per
second, command → result p99 under 0.5 s, no rejected commands, no dropped
connections. Before the fixes the world thread saturated at about **3000**:
at 4000 it ran 0.3–0.8 ticks per second, commands took up to 3.9 s and 7222
were turned away as `busy`. Up to 2000 players both builds keep the 1 s
clock with room to spare (server under 1 core).

The limit is the single world thread (sim, state builds, applying
commands). Past one world's size you add worlds (one thread each) and
processes. The later sections cover that and the follow-up work:

- *The market index:* `browse_market` costs its page now, not the whole
  market.
- *Reconnect storms:* a restart no longer logs everyone out. At 4000
  players everyone was back 9.4 s after the server answered again,
  instead of 70.9 s.
- *Beyond one box:* world capacities, `GET /api/assign`, the `evo-lobby`
  front door the web client now uses, accounts shared safely across
  servers, and the capacity and cost plan.
- *Compression:* the game WebSocket negotiates permessage-deflate. That
  cuts traffic 2.6× for 12–15% more server CPU.

## Method

`crates/server/examples/loadgen.rs` is the load generator, a Rust example
in the server crate. It already existed; this round added metrics to it.
Each simulated player speaks the real protocol. It registers and logs in
over HTTP, founds a lineage of 12 founders and opens the game WebSocket
with protocol 2: binary delta states, decoded with `wire::DeltaDecoder`
as the client does, with `resync` on a bad frame. About 30 times a
minute it does what a player does:

- pans and zooms the view (`set_view`, `clear_view`);
- possesses and steers organisms (`possess`, `order_move`, `release`).
  This is about 30% of actions, so most players also get the possession
  sub-step states;
- requests, previews and answers matings;
- browses the market, lists, buys and bids;
- reads the ledger and sets migrations.

Every command carries an id, and its latency is the time to its `result`.

```sh
cargo build --release -p evo-server --bins --examples
# Grow a 512² world to 20k organisms once, then copy that save for every run.
target/release/examples/loadgen --players 1 --duration 5 --size 512 --warm-until 20000 --data WARM
target/release/examples/loadgen --players 2000 --duration 90 --ramp 200 --size 512 --data COPY_OF_WARM --json out.json
```

Run settings:

- Every run started from a copy of the same warmed save, with 21.4k
  organisms. Populations then grew with the players' lineages, to 23k with
  10 players and 75k with 4000.
- 1 s ticks, 1 s state interval and 4 possession sub-steps per tick.
- `--no-rate-limits`, because every player comes from 127.0.0.1.
- A ramp of max(10 s, N/10 s), then 90 s measured. The tables give
  medians over the 10 s reports after the ramp.
- The server and the load generator share the 4 cores.

Timings come from the server's own `GET /api/metrics`: world-thread tick,
push, sub-step and apply windows, queue depth, bytes out, CPU and RSS.
This round added two fields: `states_build_ms`, the build part of a push,
and `apply_by_command`, world-thread time per kind of command. The load
generator prints both in its summary (`states_build_ms_p50`,
`apply_us_by_command`).

## Results before the fixes

| | 10 | 50 | 100 | 250 | 500 | 1000 | 2000 | 4000 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Players connected (end of ramp) | 10 | 50 | 100 | 250 | 500 | 1000 | 2000 | 3896 |
| Ticks/s (target 1.0), median / worst 10 s | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 0.90 | 1.00 / 0.90 | 0.50 / 0.00 |
| Tick (sim + bookkeeping) p50 / p99 ms | 21 / 33 | 22 / 32 | 24 / 30 | 25 / 35 | 29 / 44 | 39 / 110 | 60 / 101 | 74 / 226 |
| State push to all p50 / p99 ms | 0.9 / 2.1 | 1.4 / 2.3 | 2.4 / 5.3 | 4.3 / 6.4 | 9.1 / 15.3 | 20.1 / 37.2 | 42.8 / 115.5 | 79.0 / 315.0 |
| Sub-step p99 ms | 2.5 | 3.4 | 4.9 | 6.8 | 12.7 | 31.6 | 84.4 | 199.5 |
| Command → result p50 / p99 ms | 123 / 250 | 133 / 250 | 131 / 250 | 133 / 250 | 135 / 249 | 142 / 250 | 151 / 264 | 432 / 3891 |
| Commands/s | 5 | 24 | 49 | 123 | 244 | 482 | 942 | 1828 |
| Most commands queued at one boundary | 4 | 14 | 23 | 50 | 90 | 149 | 351 | 5491 |
| State frame p50 / p99 bytes | 32 / 1779 | 67 / 5209 | 60 / 5473 | 67 / 6071 | 77 / 6775 | 92 / 7427 | 103 / 8750 | 114 / 11905 |
| State encode (writer task) µs | 16 | 17 | 17 | 18 | 27 | 44 | 72 | 127 |
| States per client per s | 2.77 | 2.76 | 2.77 | 2.83 | 2.79 | 2.76 | 2.73 | 1.48 |
| Outbound per client KB/s | 1.05 | 1.90 | 2.20 | 2.36 | 2.74 | 3.08 | 3.38 | 3.17 |
| Outbound total MB/s | 0.01 | 0.10 | 0.22 | 0.59 | 1.37 | 3.08 | 6.77 | 12.66 |
| Server CPU cores (steady) | 0.07 | 0.10 | 0.10 | 0.14 | 0.19 | 0.36 | 0.79 | 1.38 |
| Server RSS max MB | 179 | 186 | 201 | 222 | 271 | 409 | 682 | 1200 |
| Load generator CPU cores | 0.00 | 0.01 | 0.01 | 0.04 | 0.08 | 0.19 | 0.45 | 0.66 |
| Dropped conns / skipped states / unexpected closes | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 104 |
| `busy` rejections | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 7222 |

The old build had no per-command apply metric. Sampled live at about 3200
connections, applying the queued commands took 175 ms p50 and 680 ms p99
per boundary, with up to 1900 commands queued. That is what pulled the
world under 1 tick per second.

## Results after the fixes

| | 10 | 50 | 100 | 250 | 500 | 1000 | 2000 | 4000 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Players connected (end of ramp) | 10 | 50 | 100 | 250 | 500 | 1000 | 2000 | 4000 |
| Ticks/s (target 1.0), median / worst 10 s | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 1.00 | 1.00 / 0.90 | 1.00 / 0.91 | 1.00 / 0.92 | 1.00 / 0.68 |
| Tick (sim + bookkeeping) p50 / p99 ms | 23 / 29 | 23 / 31 | 23 / 30 | 25 / 35 | 29 / 38 | 36 / 65 | 42 / 69 | 80 / 185 |
| State push to all p50 / p99 ms | 1.0 / 1.4 | 1.6 / 2.6 | 2.1 / 3.1 | 3.7 / 7.9 | 6.4 / 10.8 | 12.2 / 22.6 | 21.6 / 37.7 | 42.5 / 113.1 |
| Sub-step p99 ms | 2.6 | 3.0 | 4.1 | 6.7 | 10.1 | 17.8 | 32.8 | 149.8 |
| Apply queued commands p50 / p99 ms | 0.1 / 0.6 | 0.4 / 1.2 | 0.6 / 3.1 | 1.6 / 4.3 | 3.7 / 7.6 | 5.2 / 11.3 | 13.2 / 28.0 | 63.0 / 152.3 |
| Command → result p50 / p99 ms | 120 / 250 | 129 / 250 | 129 / 250 | 133 / 250 | 136 / 250 | 140 / 249 | 146 / 252 | 186 / 484 |
| Commands/s | 5 | 24 | 49 | 124 | 247 | 480 | 944 | 1841 |
| Most commands queued at one boundary | 4 | 14 | 22 | 44 | 82 | 150 | 275 | 1307 |
| State frame p50 / p99 bytes | 32 / 1779 | 67 / 5128 | 66 / 5499 | 68 / 6067 | 78 / 6740 | 92 / 7510 | 103 / 8456 | 109 / 10300 |
| State encode (writer task) µs | 17 | 15 | 17 | 21 | 32 | 48 | 56 | 82 |
| States per client per s | 2.77 | 2.71 | 2.72 | 2.84 | 2.77 | 2.74 | 2.73 | 2.67 |
| Outbound per client KB/s | 1.05 | 2.04 | 2.06 | 2.43 | 2.73 | 3.05 | 3.34 | 3.89 |
| Outbound total MB/s | 0.01 | 0.10 | 0.21 | 0.61 | 1.36 | 3.05 | 6.68 | 15.57 |
| Server CPU cores (steady) | 0.07 | 0.08 | 0.10 | 0.15 | 0.23 | 0.40 | 0.72 | 1.89 |
| Server RSS max MB | 180 | 193 | 200 | 227 | 283 | 400 | 690 | 1431 |
| Load generator CPU cores | 0.00 | 0.01 | 0.02 | 0.04 | 0.09 | 0.20 | 0.41 | 1.02 |
| Dropped conns / skipped states / unexpected closes | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 280 / 0 |
| `busy` rejections | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

The 10–500 columns come from an intermediate build. It differed from the
final one only in `browse_market`, which costs little at that size. The
1000–4000 columns come from the final build.

Notes:

- **Command latency** cannot go below about half a sub-step, roughly
  125 ms at the 1 s clock with 4 sub-steps. Commands are applied at the
  next sub-step boundary by design, so a p99 of about 250 ms is one whole
  sub-step.
- **Bandwidth** per client is small, 1–4 KB/s. About half of it is states:
  deltas are about 100 bytes at p50, and keyframes and crowded views reach
  10 KB. The other half is `browse_market` results. 4000 players send
  about 16 MB/s in total.
- **Memory** grows by about 0.3 MB per player. Most of it is the world's
  extra organisms from their lineages, plus each connection's delta
  encoder and queues.
- **Dropped ticks:** with 1 s ticks, neither build dropped any up to 2000
  players. At 4000 the old build ran 0.3–0.8 ticks/s for minutes. The new
  one runs at 1.0 in steady state. Its one 10 s window at 0.68 came at the
  end of the ramp, while 10 players a second were still founding.
- **Skipped states** at 4000 (280) are sub-step states for a few slow
  readers that newer ones replaced (`crate::push`). No events were lost.
- **Simulation results are unchanged.** The 10-player runs of both builds
  end with exactly the same organism count (22 882) and byte totals. The
  state equivalence test compares every frame byte for byte.
- **State encode µs** is wall time measured in the writer task. When the
  box is saturated it includes time the task waited for a CPU.

## Where the world thread's time went (2000 players)

World-thread time per command kind, before and after (`apply_by_command`):

| command | before µs | after µs | why it cost that much |
| --- | --- | --- | --- |
| `found` | 16 300 | 15 800 | Sim side: the home search scans the world (`World::found_lineage`, not touched) |
| `browse_market` | 410 | 323 | The rows were reallocated with a big inline preview, then fully sorted |
| `migrate` | 423 | 188 | `World::command_migrate` scans every organism |
| `respond_mating` | 259 | < 10 | `hold_consented_females` rescanned all listings and offers |
| `create_listing` / `create_auction` | 256 / 229 | < 10 | Same |
| `buy_listing` | 101 | 28 | Same |
| `request_mating` | 71 | 11 | Same |

`migrate`'s average still includes the ramp. While players are founding,
the state index is stale and `migrate` falls back to the scan.

State builds on the world thread, at 1500 players: building a push went
from 30–43 ms to about 10 ms. Most of the old push time was one serial
loop that did about 400k random reads into the 5 MB cache of neighbour
views.

## The fixes

All changes are in `crates/server`, and the wire format is unchanged.

1. **Parallel state builds** (`game/states.rs`, `Game::build`). A push or
   sub-step only reads the world, the index and the caches, so it now runs
   on rayon's pool. The world thread waits for it, as it does for the
   sim's own parallel phases. A build works in three steps:
   1. each connection's view window and neighbours;
   2. the neighbour views missing from the cache, each made once. Builds
      claim them through an atomic bitset instead of checking every pick
      in a serial loop;
   3. the messages, in connection order.

   The output is the same bytes as before.
2. **A new connection's first state** reuses the kept index
   (`Game::state_now`). Before, every connect indexed the whole world on
   the world thread, so a reconnect storm after a restart would have cost
   one full index pass per player.
3. **Incremental breeding holds** (`Game::hold_added`). Every market and
   mating command used to rescan every listing and open offer to hold
   consenting females. Once the tick's full hold has run, a command now
   holds only the females of the offer or listing it opened or changed.
   Within a tick, holds only ever extend (`max`) to the same deadline, so
   the result is identical. When no full hold has run this tick (for
   example right after a restore), it falls back to the full scan.
4. **`migrate`** finds the player's groups from the state index's owner
   list when that list is this tick's (`Game::migrate_all`). Otherwise it
   falls back to the sim's scan.
5. **`browse_market`** pre-sizes its rows and boxes the per-row preview,
   which is usually absent. It then selects the page with
   `select_nth_unstable` instead of sorting every match. That is 26% fewer
   instructions at 1200 listings (callgrind). Computing the page's previews
   in parallel was also tried, and was slower.
6. **Fan-out** (`runner.rs`). An index from player to connections now
   serves player-targeted messages and push notices; before, each message
   scanned all connections. Dispatch no longer allocates a `Vec` per
   message, and takes the metrics lock once per dispatch.

Checks:

- `the_client_fixture_is_current` passes unchanged.
- The existing `cached_substep_states_match_the_old_build` test now also
  runs the old game in a *reference* mode: full hold scans, the sim's
  migrate, and a fresh index per connection. Every tick it compares
  breeding holds and migration orders between the two games, and it
  exercises the parallel build path.

## The market index

`browse_market` scanned and filtered every listing on every call, and
listings grow with players (2 per player in the load test), so the market
cost grew with N². `game/market.rs` now keeps an index of the listings.
For each page order it holds the listings' sort keys, the same keys the
scan sorts by. It also counts listings by kind and role, overall and per
owner, and keeps each owner's listings, the listing of each organism and
the renown each bidder holds on auctions.

Creating, withdrawing, bidding and maintenance update the index, and a
restored or seeded economy is indexed again. A page walks one order from
its cursor, testing each listing against the scan's filters, until it has
a page and one more. The total comes from the counts. The counts are exact
once the tick's maintenance has dropped listings whose organism died;
organisms only die in a tick.

Some queries still use the scan: those with `max_price`, `lineage` or
`species`, and those whose order or filter needs every listing's preview
(`compatible`, or `fertility` with `mine`). In tests, every indexed page
is compared with the scan, and the index with a freshly built one. A
randomized test drives listings, auctions, bids and withdrawals through
300 rounds of paged queries.

| 4000 listings (`market_cost` bench) | scan | index |
| --- | --- | --- |
| `browse_market`, median per page | 109 µs | 12 µs |

At 4000 live players a page now averages about 360 µs, down from 713 µs.
Nearly all of that is the 20 mating previews the page carries (the load
generator always sends `mine`), so it no longer grows with the market.

## Reconnect storms

`loadgen --restart-at SECS` stops the server mid-run, gracefully (it
saves), and starts it again from its save on the same port. Every player
then reconnects as the web client does (`client/src/socket.ts`): a
doubling backoff from 0.5 s to 15 s with ±20% jitter. After two refused
attempts while the HTTP API answers, the session is gone. The web client
then shows the login screen; the load generator logs in again at once,
which is the worst case for the server. Each player's outage runs from
losing the socket to its first state after reconnecting.

**What hurt.**

- Sessions lived in memory only, so a restart logged every player out.
  The whole player base then had to log in again at once: one argon2 hash
  of about 25 ms each, two at a time (`HashLimiter`). Most logins were
  refused with `busy` and retried.
- Every reconnection also cost the world thread 0.8–3.8 ms:
  - The connecting player's name was "changed", which rebuilt the whole
    leaderboard of the kept state index, and after a restart every row
    also formatted a fallback name. That is O(players) per connect, so
    O(N²) for a storm.
  - The offline-protection update walked every offline spell, open offer
    and listing, once per login.
  - The first state rebuilt every neighbour view and followed every
    steering organism.

**Fixes.**

1. **Sessions survive a graceful restart.** The stop saves them, as the
   SHA-256 of each token (never the token) with its age and idle time.
   The start restores them, keeping the idle and maximum-age limits. The
   file store writes `sessions.json` (mode 0600); PostgreSQL uses a
   `sessions` table (migration 0011). A logout deletes the saved session,
   so it stays revoked across restarts. A crash loses only the sessions
   made since the last graceful stop: those players log in again, as
   before.
2. **A rename patches its leaderboard row** instead of rebuilding the
   board. Names don't affect the order, and nothing else on the board
   changes between ticks.
3. **Offline-protection updates are deferred inside the runner** until
   the next world step or save, the only times the world reads them. A
   test runs 300 random login/logout ticks on a world with hunters and
   checks that the deferred and immediate games stay identical.
4. **A new connection's first state uses the kept neighbour views.** The
   kept index also skips the steering organisms when no sub-step has moved
   them since it was last brought up to date. The states equivalence test
   now opens connections mid-tick, right after sub-steps and back to back.

| Restart with N connected | 1000, before | 1000, after | 4000, before | 4000, after |
| --- | --- | --- | --- | --- |
| Stop (with save) / start until the API answers | 0.4 / 0.8 s | 0.4 / 0.6 s | 2.9 / 5.4 s | 4.0 / 6.0 s |
| Logins needed / refused `busy` | 1000 / 3103 | 0 / 0 | 4000 / 20 032 | 0 / 0 |
| Everyone back, after the server answers | 21.2 s | 1.2 s | 70.9 s | 9.4 s |
| Outage per player p50 / p99 (from losing the socket) | 6.8 / 20.7 s | 1.5 / 1.8 s | 31.6 / 69.1 s | 16.1 / 18.3 s |
| World-thread time per connect | 772 µs | 482 µs\* | 3.83 ms | 1.18 ms |
| Lowest ticks/s after the restart | 0.92 | 0.90 | 0.90 | 0.50 |

\* The 1000-player "after" run had only fix 1. The 4000-player "after"
run had all four.

At 4000 players, the outage after the fixes is the server being down
(about 10 s to save and load a 75k-organism world) plus the backoff the
clients have grown by then; 99% are back within 18 s. One 10 s window
ran at 0.5 ticks per second while 4000 states were built in a burst.
Before the fixes, players waited up to 78 s, behind 20 000 refused logins.
The web client would also have sent every one of them to the login
screen.

Not done here: a client that cuts its backoff when the server comes back
(it could poll `/api/world`), and making the save and the load faster.
Those two are now the larger part of a restart.

## Beyond one box

### Several worlds per process

A process already hosts several worlds (`--world`, one thread each,
staggered ticks; see the README). The same 4-core box, shared with the
load generator, at the same per-player load:

| | 1 world × 4000 | 2 worlds × 2000 | 3 worlds × 1500 (+ deflate estimate) |
| --- | --- | --- | --- |
| Ticks/s per world (median) | 1.00 | 1.00 / 1.00 | 1.00 / 1.00 / 1.00 |
| Tick p99 per world | 185 ms | 156 / 144 ms | 452 / 419 / 482 ms |
| Command → result p50 / p99 | 186 / 484 ms | 138 / 254 ms | 189 / 984 ms |
| Server CPU / RSS | 1.89 cores / 1431 MB | 1.63 cores / 1390 MB | 1.82 cores / 1736 MB |
| Load generator CPU | 1.0 core | 0.9 core | 2.0 cores |
| Outbound per player | 3.9 KB/s | 3.6 KB/s | 3.5 KB/s |

Splitting 4000 players over two worlds halves the command p99 and uses
less CPU: two world threads share the work, and each world's
O(players-in-view) costs are smaller. The 3 × 1500 run saturated the box.
The load generator's own deflate estimate took two cores, so its tails are
the box's, not the server's. That run also measured what permessage-deflate
with context takeover would save on state frames: **2.6×** (state frames
are half the traffic; the `browse_market` JSON, the other half, compresses
better).

### Assigning players: `GET /api/assign` and `evo-lobby`

- **World capacity.** Each world has a capacity: `--world id,capacity=N`,
  default 3000, which leaves headroom under the 4000 measured above.
- **`GET /api/assign`** (with a token, or `?name=`) returns the world a
  player should use (`worlds::assign`):
  1. the world holding the player's lineage, even when it is full
     (lineages don't move);
  2. else the busiest world still under 80% of its capacity, so new
     players meet others;
  3. else the world with the most room for its size;
  4. 503 `full` when every world is at capacity.

  It answers from the world's published state (players online,
  leaderboard). It does not touch the world thread.
- **`evo-lobby`** (`crates/server/src/bin/evo-lobby.rs`) is a stateless
  front door over several processes:
  - `evo-lobby --backend http://a:8080 --backend http://b:8080` serves
    `GET /lobby/assign?name=`.
  - That asks every backend's `/api/assign` at once, with a 1 s timeout,
    and applies the same rule across all of them: a backend reporting the
    player's lineage wins, then the busiest world under its fill target,
    then the most room.
  - It answers with the `server` and `world`. The client then logs in on
    that server and opens `/ws?world=` there.
  - `GET /lobby/servers` lists each backend's worlds and load.
  - Tested against two live processes (three worlds): a new name went to
    the first world with room; a player with a lineage on the second
    process was sent there.

Limits of the prototype:

- **Accounts.** They must live in one shared PostgreSQL database for an
  account to work on every backend; separate file stores have separate
  accounts. With one database, several servers now register safely; see
  *Accounts across servers* below.
- **Login sessions.** They are per process: a player logs in on the
  server the lobby picked. The web client does that.

### Accounts across servers

Each server used to hand out player ids from its own in-memory counter,
seeded with the highest id it loaded at start. Two servers on one
database would give two new accounts the same id, and the second insert
would fail.

- **Id allocation.** Ids now come from the store (`Store::allocate_player`).
  PostgreSQL uses a `player_ids` sequence (migration 0012), seeded past
  the highest id in use and never handing out an id at or below it. The
  file and memory stores, which serve one process each, use a counter
  under their lock.
- **Races.** The database's unique `player` column is the real guard: a
  registration that loses a race retries with a new id. A name registered
  on two servers at once ends as one account, and the other server
  answers `name_taken`.
- **Logins.** A login, and the lobby's `?name=` lookup, for an account
  another server registered after this one started now reads it from the
  store (`Store::find_account`) and keeps it.

Tested with two servers on one PostgreSQL database (two connection
pools, as two processes would have) and on one shared in-memory store:

- 24 concurrent registrations, 12 on each server, got distinct ids;
- a name raced on both servers gave one account and one `name_taken`;
- an account made on one server logged in on the other with the same
  player id.

The PostgreSQL run used a local PostgreSQL 16 in this container; the
whole integration suite also passes against it (`DATABASE_URL`).

### The client and the lobby

Before logging in, the web client asks where to play (`client/src/assign.ts`):

- **Who it asks.** It asks the lobby when one is configured
  (`?lobby=https://lobby.example.org` on the page URL, which it remembers,
  or the build-time `VITE_EVO_LOBBY`, e.g. for the store apps), else the
  game server's own `/api/assign`. The answer comes within 2.5 s or not
  at all.
- **Another server.** If the lobby names one, the client logs in there.
  It remembers that server, because the session belongs to it.
- **Which world.** The player's own pick in the world list wins, then the
  world holding their lineage, then the world remembered for them, then
  the world the server would fill, then the server's default.
- **Fallback.** No answer (an older server, a lobby that is down, a slow
  reply, every world full) leaves the login exactly as before.
- **CORS.** `evo-lobby` sends CORS headers, because the page usually
  comes from another origin.

`e2e/lobby.mjs` checks this in Chromium with two servers and a lobby:

- a new player opening server A's page with `?lobby=` is sent to A's
  first world with room;
- a player whose lineage lives on server B, opening A's page, ends up
  playing on B;
- without a lobby, A's own `/api/assign` keeps a player with their
  lineage in their world.

## Compression: permessage-deflate

axum's WebSocket (tungstenite 0.24, and still in 0.30) has no
permessage-deflate. It also rejects compressed frames, which browsers
send once the extension is agreed. The game WebSocket now runs on
`yawc` (RFC 6455 and RFC 7692 over hyper 1, Autobahn-tested, MPL-2.0)
and negotiates compression whenever the client offers it
(`--ws-compression`):

| Mode | What it does |
| --- | --- |
| `context` (default) | Each connection keeps its deflate stream, so a message can refer back to earlier ones. |
| `message` | Each message is compressed on its own, with deflate state shared by all connections; the client is asked to do the same. |
| `off` | Plain frames. |

In every mode, messages under 64 bytes go out plain. Client messages
stay bounded at 16 KiB, before and after inflating.

**Who negotiates it.**

- Every browser offers `permessage-deflate; client_max_window_bits`.
  Chromium accepting it from this server is checked in `e2e/lobby.mjs`,
  from the DevTools protocol's handshake events, and smoke, mobile and
  android-desktop run over compressed sockets.
- The Capacitor apps use their web view's WebSocket: Chromium on Android,
  as tested. iOS (WKWebView) could not be checked here. If a client
  doesn't offer the extension, the server sends plain frames, so nothing
  breaks.
- Clients that don't offer it get plain frames: the load generator with
  `--no-ws-deflate`, and the tests' tokio-tungstenite clients.

**Cost against saving.** The load generator now negotiates compression
as a browser does, and counts the bytes it reads off each socket.

| | 1000 off | 1000 `message` | 1000 `context` | 4000 off | 4000 `message` | 4000 `context` |
| --- | --- | --- | --- | --- | --- | --- |
| Wire bytes per player | 2.26 KB/s | 1.04 KB/s (2.2×) | **0.85 KB/s (2.6×)** | 2.23 KB/s | 1.03 KB/s (2.2×) | **0.87 KB/s (2.6×)** |
| Server CPU | 0.36 | 0.45 (+25%) | 0.41 (+15%) | 1.82 | 2.13 (+17%) | 2.04 (+12%) |
| Server RSS | 390 MB | 398 MB | 590 MB | 1244 MB | 1348 MB | 2065 MB |
| Command → result p99 | 249 ms | 251 ms | 250 ms | 391 ms | 591 ms | 512 ms |
| Ticks/s | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |

At 4000 players the box was close to full: server and load generator
used about 3.2 of 4 cores, and both now spend CPU on deflate. That is why
the latency tails grew there. More sub-step states were also replaced by
newer ones before slow writers sent them: 0.2% of states in compressed
mode, against almost none uncompressed. Nothing was dropped and no
decode failed.

`context` takes the most off for the least CPU. It costs about 200 KB of
deflate state per connection: 0.8 GB at 4000 players, which is more RAM
than CPU per player. `message` costs almost no memory but more CPU, and
saves less, because small deltas barely shrink on their own. For servers
short of memory, `--ws-compression message` is the choice.

The traffic is now about half quantised delta states and half
`browse_market` JSON:

- State frames were already quantised binary deltas: positions ×64,
  needs ×10, per-record change masks, and palettes for looks, actions and
  eyes (`wire.rs`).
- Keyframes go out once a minute.
- Nothing cheaper is left in them without changing the protocol.

The market's 20-listing pages, each listing with its traits and a mating
preview, compress well. Making them smaller (fewer fields, or previews
on demand) would change what the client receives, so it is not done
here.

### Capacity plan

These are measured with heavy players: 30 commands a minute, mostly
steering, so about 2.7 states a second each. That is several times what
a typical player sends. The box also ran the load generator, so the
numbers are pessimistic.

| Per 1000 concurrent heavy players | Measured | Plan with |
| --- | --- | --- |
| Server CPU | 0.40 cores (1 world × 1000); 0.41 per 1000 at 2 × 2000 | 1 vCPU (2.5× headroom for bursts, saves and restarts) |
| Memory (RSS) | +0.3 MB per player over a ~180 MB world (512², 20k organisms), +0.2 MB per connection for deflate contexts | 0.75 GB, plus 0.25 GB per world |
| Outbound bandwidth | 2.2–3.9 KB/s per player uncompressed, depending on the build (2.2 with the current food rules); **0.85–0.87 KB/s with permessage-deflate** (measured, `context`): about 7 Mbit/s, 2.3 TB a month at full occupancy | 2.5 TB a month (6 TB uncompressed) |
| Players per world | 4000 held at 1 tick/s; 3000 default capacity | ≤ 3000 per world, one world per 1–2 vCPU |

**Rough cost per 1000 concurrent heavy players a month.** These are
approximate on-demand list prices; check current pricing.

| Provider (instance) | Compute | Traffic (2.5 TB deflated; 6 TB uncompressed) | Total |
| --- | --- | --- | --- |
| AWS c7g.xlarge (4 vCPU, 8 GB, ≈ $106/mo; 4000 CCU per box at 1 vCPU each) | ≈ $27 | ≈ $225 ($540) at $0.09 per GB | ≈ $250 |
| GCP c3-standard-4 (4 vCPU, 16 GB, ≈ $150/mo) | ≈ $38 | ≈ $210 ($510) at ≈ $0.085 per GB | ≈ $250 |
| Hetzner CCX23 (4 dedicated vCPU, 16 GB, ≈ €30/mo, 20 TB included) | ≈ €8 | included (20 TB per box covers its 4000 CCU) | ≈ €8 |

Compute is cheap. **Outbound traffic still dominates on the big clouds**,
about 8× the compute at AWS and GCP egress prices, even after
permessage-deflate cut it 2.6× (now on by default). The remaining levers:

1. Send `browse_market` results less often or smaller: they are about
   half the bytes.
2. Host where traffic is included.

## What is left

- **Founding costs about 16–20 ms** of world-thread time, for the
  sim-side home search over the whole world (`crates/sim/src/world`, not
  touched). In steady state that does not matter, but a burst of 10 new
  players a second takes 20% of the tick budget.
- **The floor on command latency** is the sub-step cadence, not load.
- **Restarts** now cost the save and load (about 10 s at 75k organisms)
  plus the client's backoff.
- **Past about 3000–4000 players per world**, the world thread's state
  builds and command application are the limit: use more worlds. Past
  one box, use the lobby, with a shared PostgreSQL database.
- **`browse_market` with `max_price`, `lineage`, `species` or every
  candidate's preview** still scans the market.
