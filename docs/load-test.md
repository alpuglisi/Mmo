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
commands). Per player it now goes mostly to `browse_market` and to
founding, which both scan the whole world or market (see *What is left*).

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

## What is left

- **`browse_market` is O(listings) per call.** Listings grow with
  players, so its total cost grows with N². At 4000 players (8000
  listings) it averages 0.7 ms per call and is now the largest
  steady-state cost on the world thread. A sorted listing index per sort
  order, or answering market reads off the world thread from a published
  snapshot, would make it O(page).
- **Founding costs about 16–20 ms** of world-thread time, for the
  sim-side home search over the whole world. In steady state that does
  not matter, but a burst of 10 new players a second takes 20% of the
  tick budget.
- **The floor on command latency** is the sub-step cadence, not load.
- **Beyond about 4000 players per world**, the next limits are state
  builds and applying commands on the world thread (about 150 ms p99 to
  apply at 4000) and, on this box, CPU shared with the load generator.
  For more players per process, more worlds (`--world`) scale better,
  since each runs on its own thread.
