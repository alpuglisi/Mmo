# Server game rules: correctness review

This review covers the game rules a client can reach through `crates/server`:

- player commands (migrate, territory, rally, split and merge, possession);
- the market and agreements (offers, fixed listings, auctions, escrow, refunds, the `--fresh` refund), renown income and bonuses;
- founding and design validation;
- offline protection and the level-of-detail (LOD) handover;
- saving and resuming;
- sessions and rate limits;
- what a hostile or buggy client could send.

The README's *Security notes* already record two earlier reviews (HTTP, auth, tokens, rate-limit keys, slow connections, SQL, push, numbers on the wire). This one looked for logic holes in the game rules. The method was the same as `docs/predator-review.md`: each bug got a test that failed first, then the fix.

None of the fixes touches the simulation's own rules. They change only what commands and the game layer do, so the standard sweep is unchanged: 11.50 on the current base. No snapshot format change is needed: the claim ticks were already saved, and LOD focus is set by the game every hour.

## Fixed

| # | Severity | Finding | Fix | Test |
| --- | --- | --- | --- | --- |
| 1 | Medium | **Territory claim cooldown reset by split and merge.** The cooldown (`CLAIM_COOLDOWN_DAYS`) was stored per group id. `split_group` removed the new group's claim tick, so split → claim → merge back → split cleared it every round. A player could claim a new territory as often as the group-command rate (1 a second per connection) allowed. Territories steer the group and choose the offline home range, so the cooldown is what stops territory hopping. | A split's new half inherits the parent group's claim tick. A merge gives the merged group the later of the two ticks. | `splits_and_merges_keep_the_claim_cooldown` (sim, `world/groups_tests.rs`) |
| 2 | Medium | **Unlimited possession.** `possess` took any number of a player's organisms, and possession outlives a disconnect. Possessing the whole lineage gave every member the possession bonuses (10% speed in `move_speed`, 20% perception in `decide`) and kept all of their regions as full agents under LOD. The README and the client treat possession as one organism ("Take control of one of your organisms"; the client shows the first possessed one). | Possessing an organism releases the player's other possessed organisms and drops their move orders. | `a_player_possesses_one_organism_at_a_time` (sim, `world/commands.rs`) |
| 3 | Medium (with `--lod`) | **LOD folded traded organisms into cohorts, and the game treated them as dead.** Organisms in open agreements, on the market, or offered as an auction's highest bid are kept as agents only if someone's view or a possessed organism is near them. Once their region turned abstract, `World::organism` no longer found them. `after_tick` then failed the agreement as "died before the mating", `maintain_listings` closed the listing as "no longer alive", and a high bid was refunded as if the lot had died. | `sync_focus` adds each traded organism's position to its owner's focus points. It also runs when an offer, listing, auction or bid opens, so the next hourly LOD update keeps them as agents. | `level_of_detail_keeps_traded_organisms_as_agents` (server, `game.rs`) |

## Not fixed (reported)

| Severity | Finding | Why not fixed |
| --- | --- | --- |
| Medium (design) | **The offline protection cap can be renewed indefinitely.** A spell starts at the last disconnect and protects a small lineage for its first 12 hours. Any login ends the spell, and the next logout starts a fresh one. A script that opens and closes a WebSocket every 11 hours keeps a lineage protected forever, and the home range likewise. | Real players also start a fresh spell each day, so the fix is a rule choice. Options: a login shorter than N minutes doesn't end the spell; or a cap per day of real time. |
| Low | **An auctioned dam can be promised after the auction opens.** `create_auction` refuses a dam that's already in an open agreement. But nothing stops her owner from making a cross-player request with her afterwards (`check_cross_pair` doesn't look at listings), and `check_settlement` checks only the winner's organism. The winner's paid agreement can then fail when she's pregnant by the other. | The winner is refunded, so no renown is lost. A fix would refuse requests for listed dams, or check the lot at settlement. |
| Low | **Wash trading.** A player with a second account can buy their own listings to count sales toward the season's sales standing (`SeasonTracker::add_sale`). It costs the second account the fee, which flows back. | Needs account-level linkage, or a rule such as counting sales only between unrelated players. |
| Low | **Group-command rate is per connection.** The 1 per second (burst 6) bucket is per WebSocket, and a player may hold 8. | With fix 1 the claim cooldown is the real guard, and the other group commands only rewrite standing orders. A per-player bucket would be stricter. |
| Info | A seller can directed-mate their own listed dam (directed matings ignore breeding holds). A buyer's pending agreement then fails and is refunded. | The seller's own loss. |
| Info | `command_migrate_group` accepts a group id with no members, and the order waits for that id. | Harmless: a split clears the new group's orders. |

## Checked and found sound

- **Ownership on every command.** `possess`, `release` and `order_move` check ownership or possession. A split names only the caller's organisms in that group, and merges and orders act only on the caller's groups. Only an offer's recipient answers it, only its parties complete or cancel it, and only the owner withdraws a listing. Bids and purchases check the buyer's organism and refuse the buyer's own listing.
- **Numbers.** Coordinates must be finite and are clamped to the map. View radius, territory radius and split size are clamped or capped. Prices are `u64`, capped at 1 000 000 before any check. Escrow debits are checked, and credits saturate. A raised bid pays only the difference over the bidder's own high bid, so it can't underflow.
- **Founding.** Gene values must be in [0, 1] (NaN fails the range check). Reserved genes are refused, and litter size and founder counts are bounded. The point cost (traits, litter, founders, small body, cold blood, morphology) is checked against the budget, and oversized founder counts are priced over it. A player founds once per world. The founding grant is paid once per economy (`granted`, rebuilt from stored balances).
- **Escrow and refunds.** An offer is opened only if its escrow is taken. Closing settles once and removes the offer. An outbid bidder is refunded at once. A settled auction releases the bid and escrows it again on the new agreement. A dead lot refunds the high bid. At a season change every open offer expires with a refund. The `--fresh` refund counts open escrow and bids from the saved game plus later store entries, skips settled ones, and marks its own refunds so a second fresh start doesn't pay twice.
- **Income and bonuses.** Paid once per legacy day (`last_legacy_day` restored from the world). Achievement bonuses are paid once (`seen_achievements` is saved).
- **Worlds.** Balances and ledgers are per world (`shard`), so renown can't be spent twice across worlds.
- **Replays and races.** Every command runs on the world thread one at a time and needs a live session. Replaying a message repeats a legitimate command, which is subject to the same checks and the per-connection and per-address rate limits.
