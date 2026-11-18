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

## Decided and implemented (follow-up)

The coordinator decided on four items this review first only reported. Each now has a test.

| Item | Decision | Implementation | Test |
| --- | --- | --- | --- |
| Offline protection renewed by reconnecting | **A rolling budget:** at most 12 hours of protection in any 24, however often the player reconnects. The client shows how much is left. | Each spell starts with a `credit`: the protection already used in the window (twice the budget), counted from past spells (`OfflineTracker::past`, saved with the game) and capped at the budget. The surviving core's cap and the home range's onset, full strength and fade all run from that clock. A single long night behaves exactly as before. The home-range group in the state carries `protection_left_min`, and the client's home-range note shows it ("Protection left: 7 h 30 min (at most 12 hours in any 24)"). | `reconnecting_does_not_renew_protection`, `the_protection_budget_survives_a_restart`, `the_home_range_shows_the_protection_left`; client `groups.test.ts` |
| Auctioned dam promised elsewhere | **A dam in an open auction is locked.** | `request_mating` (either side, own pairs included), accepting a pending offer, buying a listing with her, and bidding with her are refused with `in_auction` ("#N is up for auction (#L) until it closes or is withdrawn"; `detail.listing`). The auction's own settlement is unaffected (the listing is closed first). A stud in an auction isn't locked, since he may serve several. | `a_dam_in_an_open_auction_is_locked` |
| Wash trading | **Diminishing returns per counterparty, per season.** | `SeasonTracker::add_sale(seller, buyer, renown)`: the first sale to a buyer counts in full, each further sale to that buyer half the one before, and a new season starts over. A sale to oneself counts nothing; the commands already refuse it (`own_listing`, `own_organism`). | `repeated_sales_to_one_buyer_count_less` |
| Group-command rate per connection | **Per player.** | `Limiters::group_commands`, keyed by player across all their connections, replaces the per-connection bucket. | integration `the_group_command_rate_is_per_player` |

## Still reported

| Severity | Finding | Why not fixed |
| --- | --- | --- |
| Info | A seller can directed-mate their own listed dam (directed matings ignore breeding holds). A buyer's pending agreement then fails and is refunded. | The seller's own loss. |
| Info | ~~`command_migrate_group` accepts a group id with no members.~~ | Fixed in round 3: `no_such_group`. |

## Checked and found sound

- **Ownership on every command.** `possess`, `release` and `order_move` check ownership or possession. A split names only the caller's organisms in that group, and merges and orders act only on the caller's groups. Only an offer's recipient answers it, only its parties complete or cancel it, and only the owner withdraws a listing. Bids and purchases check the buyer's organism and refuse the buyer's own listing.
- **Numbers.** Coordinates must be finite and are clamped to the map. View radius, territory radius and split size are clamped or capped. Prices are `u64`, capped at 1 000 000 before any check. Escrow debits are checked, and credits saturate. A raised bid pays only the difference over the bidder's own high bid, so it can't underflow.
- **Founding.** Gene values must be in [0, 1] (NaN fails the range check). Reserved genes are refused, and litter size and founder counts are bounded. The point cost (traits, litter, founders, small body, cold blood, morphology) is checked against the budget, and oversized founder counts are priced over it. A player founds once per world. The founding grant is paid once per economy (`granted`, rebuilt from stored balances).
- **Escrow and refunds.** An offer is opened only if its escrow is taken. Closing settles once and removes the offer. An outbid bidder is refunded at once. A settled auction releases the bid and escrows it again on the new agreement. A dead lot refunds the high bid. At a season change every open offer expires with a refund. The `--fresh` refund counts open escrow and bids from the saved game plus later store entries, skips settled ones, and marks its own refunds so a second fresh start doesn't pay twice.
- **Income and bonuses.** Paid once per legacy day (`last_legacy_day` restored from the world). Achievement bonuses are paid once (`seen_achievements` is saved).
- **Worlds.** Balances and ledgers are per world (`shard`), so renown can't be spent twice across worlds.
- **Replays and races.** Every command runs on the world thread one at a time and needs a live session. Replaying a message repeats a legitimate command, which is subject to the same checks and the per-connection and per-address rate limits.

## Playtest 1 fixes (B): griefing, exploits, information leaks, protocol

These come from `docs/playtest-1.md` (findings 11, 12, 18, 20, 22 and 27, and the code note on names). Each has a test.

| Finding | Fix | Test |
| --- | --- | --- |
| A pending mating request stripped offline protection from both animals (no slot, no consent, up to 8 at once). | Only an **accepted** agreement exempts its organisms. | `organisms_in_an_agreement_are_exempt` |
| Buying a rival's listed dam and never bringing the male held her until the season ended. | A bought listing or won auction **lapses** if the two haven't met within `GameConfig::purchase_deadline_ticks` (30 minutes of real time). The dam is freed. The seller keeps 10% of the fee (at least 1) and the rest is refunded; if nothing was paid, the buyer forfeits a mating slot (`World::forfeit_mating_slot`). Both parties get a `mating_update` with error `lapsed`. | `a_bought_listing_lapses_when_the_buyer_never_comes` |
| Mating-request spam, with no way to block it. | New `mating_prefs` command, saved with the game: block or unblock a player, or auto-decline every request (`requests_refused`). Also a per-target limit: at most 3 requests to one player within `request_window_ticks` (an hour), else `too_many_requests`. Requests to juveniles already answered `not_mature`; the test now pins that down. | `mating_requests_can_be_blocked_declined_and_are_limited_per_target` |
| Case-sensitive names allowed look-alike accounts. | A name's skeleton (lower case, `0`/`o`, `1`/`l`/`i`, `_`/`-`, `rn`/`m`, `vv`/`w`) must be unique. Names are ASCII only, so there are no other confusables. A login differing only in case finds its account. | `look_alike_names_are_refused` |
| `/api/map.png` needed no login and showed everyone's animals. | Login required. The image shows the terrain, the caller's organisms, and others only within their view radius. | `rivals_hidden_state_and_positions_do_not_leak`, integration `server.rs` |
| `set_view` could be pointed anywhere (16 calls mapped every player). | Allowed only within the view radius of one of your organisms or your lineage's center (`out_of_sight`). | same |
| Listings exposed live positions. | Listings give the centre of the animal's 16-tile cell. A preview's distance to a partner you can't see is rounded up to 16 tiles, so it can't be used to triangulate. | same |
| `already_pregnant` told a rival about hidden state. | A rival's pregnant female gets a generic `partner_unavailable`; your own still says `already_pregnant`. | same |
| "Species founded" was farmed: split, merge back, re-detect, +100 each time. | The bonus and the season count are paid only if the lineage hasn't merged back from an earlier species of the same player, and at most once a game year per lineage (`species_bonus_due`, saved with the game). | `species_founded_bonus_is_not_paid_twice_for_one_lineage` |
| `bad_message` replies had no request id (the client timed out), and showed the parser's text listing every command. | The reply carries the request's `id` and the command name when they can be read from the JSON, with a generic message. The client toasts friendly text (`client/src/errors.ts`: interface names, no raw codes or parser text). | integration `server.rs`, client `errors.test.ts` |
| A lineage counted in two species at once; an extinct species still showing adults; circular "merged into" labels. | The species tracker zeroes the counts of a species that merges or is missed (its lineages stay listed, as history). When a species merges, any species that had merged into it is re-pointed to the new survivor. | `merged_species_count_nothing_and_merge_chains_point_at_the_survivor` |
| `/api/lineage/stats` vitals repeated the current day. | Today's partial counts merge into the last record when that record already carries today's day. | `lineage_stats_follow_the_daily_history` |
| Ledger day labels: mixed 0- and 1-based numbering. | Convention, now in the README: `day` fields count from 0, player-facing text from 1, and the client shows `day + 1`. The server texts already followed it, and a test now guards it (the playtest compared the raw 0-based field with the 1-based text). | `ledger_texts_name_their_entry_day` |

Explained, not changed: "founded by X" with "Y 100% of alleles" on an extinct species. The founder is the player credited when the species split off, and the share is its composition at its last detection. A species can continue through a cluster that is by then another player's animals, so the two can differ without either being wrong. Labelling the two fields clearly is client display work (session D).

## Remediation round 3: leaks, newcomer grace, alert spam, validation

From `docs/playtest-2.md` (findings 7, 12, 13 and 14, and the smaller items on `mating_prefs` and `COMMAND_NAMES`). Each has a test. The standard sweep is unchanged: 11.76 before and after on the merged base. The world's first players (founded at tick 0) never have a grace, and the sweep founds every lineage then.

| Finding | Fix | Test |
| --- | --- | --- |
| `/api/leaderboard/history?genes=true` gave every lineage's full mean genome without a login (a design recipe). | Gene means need a login: `genes=true` without one is 401, and the default is genes only with a login. A player gets every gene of their own lineages; of other players' lineages only `body_size`, `leg_length` and `fur`, rounded to quarters (what their animals show anyway). | `rivals_gene_means_are_coarse_looks_only` (unit); integration `a_long_running_worlds_history_stays_bounded` (ignored, long) |
| `set_view` (32-tile reach plus a 32-tile radius) still mapped most of the world for a spread-out lineage. | The whole window must fit within one of the player's animals' sight plus `VIEW_MARGIN` (16 tiles; 16 around the lineage's center): `Game::view_reach`. A wider request is narrowed (the reply's `data` is the window granted), and one where nothing fits is `out_of_sight`. Sight is the sim's unalarmed perception radius (2.7 to 7 tiles). `map.png` marks the same squares. | `rivals_hidden_state_and_positions_do_not_leak` |
| `/api/spectate` located every lineage, newcomers included, for camping during their grace. | A player lineage under its grace has no cells (`hidden: true`): it is counted, but not placed. Others stay at 16-tile cells. | `a_newcomer_under_its_grace_is_not_placed` |
| `preview_mating` answered for any organism anywhere (a probe for distance and existence). | Only for a partner in the player's sight (`Game::in_sight`, the same reach), on the market, or in one of their open offers; otherwise `out_of_sight`. A partner out of sight still gets a distance rounded up to 16 tiles. | same as `set_view` |
| Newcomer grace was one-sided: a graced lineage could still attack others. | Two-sided: `World::grace_spares` also spares other players' animals from a graced hunter (wild prey stays fair game), and `possess_attack` refuses a graced attacker with `own_grace` (`detail.days_left`). | `the_grace_is_two_sided_lasts_in_real_time_and_survives_refounding` (sim); `the_newcomer_grace_is_in_the_state_and_lasts_real_minutes` (server) |
| The grace restarted on every re-founding. | It runs from the first founding in an unbroken run of the owner's foundings, each within twice the grace of the one before (`World::grace_start`). This is derived from the saved `lineage_founded` ticks, so the snapshot format is unchanged. The starts are cached (not saved) and rebuilt on founding and on restore. | same (sim), including a snapshot round trip |
| 20 game days passed in 100 s at playtest speed. | At least `GameConfig::grace_min_minutes` (30) of real time, set by the game from `tick_ms` (`World::set_attack_grace_min_ticks`, not saved). | same (server) |
| The grace was invisible. | `StateView::grace` = `{days_left, minutes_left}` while it lasts (a protocol 2 meta field). The client indicator is session D's. | same (server); wire fixture regenerated |
| Victims got one alert per attack order, with no limit. | At most one alert per (attacker lineage, victim) every `GameConfig::attack_alert_minutes` (5) of real time, and repeating a standing order alerts nobody. | `an_attack_order_alerts_the_targets_owner` (extended) |
| `COMMAND_NAMES` lacked the newer commands. | Added `retire_lineage`, `renown_shop`, `buy_mating_slot`, `reinforce` and `compatibility` (`possess_attack` and `mating_prefs` were already there), and labels for them in `client/src/errors.ts`. A test reads the parser's own list of variants and checks both. | `every_command_has_a_name` |
| `mating_prefs` accepted player 0 and players who don't exist. | `no_such_player` (`detail.player`). | `mating_requests_can_be_blocked_declined_and_are_limited_per_target` |
| `migrate` accepted a group without members (and `clear_migration` too). | `no_such_group` for any group but the main one without members. `clear_migration` still clears an order that outlived its group. | `orders_tests` (sim), `group_commands_split_claim_and_merge` (server) |
| A string `id` or text that isn't valid JSON lost its request id. | Best effort: a numeric string id counts, and text that isn't JSON is scanned for `"id": <number>` and `"type"`. Such messages always got a `bad_message` reply, and the connection carries on; the test now pins that down. | `a_malformed_message_gets_its_id_when_it_can_be_read` (unit); integration `server.rs` |
