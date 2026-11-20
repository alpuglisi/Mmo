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
| The grace was invisible. | `StateView::grace` = `{days_left, until_tick, real_seconds_left}` while it lasts (a protocol 2 meta field), and `NearbyOrganism::protected` on a graced lineage's animals (a neighbour flag bit in protocol 2): the names session D asked for. The client indicator is session D's. | same (server); wire fixture regenerated |
| Victims got one alert per attack order, with no limit. | At most one alert per (attacker lineage, victim) every `GameConfig::attack_alert_minutes` (5) of real time, and repeating a standing order alerts nobody. | `an_attack_order_alerts_the_targets_owner` (extended) |
| `COMMAND_NAMES` lacked the newer commands. | Added `retire_lineage`, `renown_shop`, `buy_mating_slot`, `reinforce` and `compatibility` (`possess_attack` and `mating_prefs` were already there), and labels for them in `client/src/errors.ts`. A test reads the parser's own list of variants and checks both. | `every_command_has_a_name` |
| `mating_prefs` accepted player 0 and players who don't exist. | `no_such_player` (`detail.player`). | `mating_requests_can_be_blocked_declined_and_are_limited_per_target` |
| `migrate` accepted a group without members (and `clear_migration` too). | `no_such_group` for any group but the main one without members. `clear_migration` still clears an order that outlived its group. | `orders_tests` (sim), `group_commands_split_claim_and_merge` (server) |
| A string `id` or text that isn't valid JSON lost its request id. | Best effort: a numeric string id counts, and text that isn't JSON is scanned for `"id": <number>` and `"type"`. Such messages always got a `bad_message` reply, and the connection carries on; the test now pins that down. | `a_malformed_message_gets_its_id_when_it_can_be_read` (unit); integration `server.rs` |

## Round 4: playtest 3 (grace, security, fairness)

From `docs/playtest-3.md`. Each fix has a test. No snapshot format change: the grace waiver, the request log and the attack-alert clocks are saved with the game (`SavedGame`), and grace starts stay derived from `lineage_founded`.

| Finding | Fix | Test |
| --- | --- | --- |
| 1. Grace restarted on re-founding: the chaining window was 2 × `attack_grace` game days (~3 real minutes at playtest speed) and ignored the 30-minute real-time floor. | The window is twice the grace's whole length (`World::grace_length`, real-time minimum included), so a re-founding never starts over a grace that would still be running; the time left carries on. The help text says so. | `refounding_chains_on_the_whole_grace_and_an_ended_grace_stays_ended` (sim) |
| 2 and 18. Predators never left grace (30 real minutes ≈ 370 game days at fast ticks; 5 of 6 lineages graced at once). | The real-time minimum is capped at `GameConfig::grace_max_days` (60 game days). New `end_grace` command and a Lineage-tab control ("End my grace now"): the owner of a predator lineage can opt out. The opt-out holds for re-foundings in the window and is saved. A grace that ends (runs out, or is ended) is announced with an `attack` alert. `own_grace` and `newcomer_grace` refusals give the real seconds left too. The help and inspector text now say instinct hunts are blocked too. | `the_newcomer_grace_is_in_the_state_and_lasts_real_minutes`, `a_grace_running_out_is_announced` (server); sim test above |
| 9. Free dam-lock: a buyer could cancel an accepted purchase before the deadline for a full refund. | A requester cancelling an accepted agreement that holds the partner's dam forfeits the hold fee as a lapse would (`Game::forfeit_hold`: 10% of the escrow, at least 1, to the seller; a mating slot when nothing was paid). The seller backing out refunds the buyer in full. | `a_buyer_cancelling_an_accepted_purchase_pays_for_the_hold` |
| 16. A failed pending offer told the other party "already pregnant". | `close` gives `already_pregnant` only to the pregnant female's owner; the other party gets `partner_unavailable`. | `rivals_hidden_state_and_positions_do_not_leak` |
| 16. `request_mating` had no sight check; `preview_mating` answered `not_found` and `same_sex` before checking sight. | Both check the partner first (`Game::check_partner_visible`): one of yours, within your animals' sight plus `NAME_MARGIN` (16), on the market, or in one of your open offers. Otherwise `out_of_sight`, also for ids that don't exist. | same |
| 16. `/api/species` and `/api/design/compatibility` gave rivals' exact body plans. | A player species' carnivory, body size, endothermy and birth mode are given in quarters (wild species exactly). Rivals' rows of the design check are given in tenths, with `limiting: "body_plan"` instead of the gene (your own lineage's row stays exact). | `catastrophes_achievements_and_eras_are_announced_and_exposed`, `design_compatibility_samples_individuals` |
| 16. `/api/leaderboard/history?limit=huge` returned the whole history without a login. | At most 60 days logged out (`history_limit`). | `the_history_limit_is_capped_tighter_without_a_login`; long integration test |
| 16. `set_view` still listed hundreds of rival animals 3 to 5 times as far as anything perceives. | `VIEW_MARGIN` is 8 (was 16). Exact preview distances follow it. Naming a partner keeps 16 (`NAME_MARGIN`). | `rivals_hidden_state_and_positions_do_not_leak` |
| 15. Titles and species names accepted bidi overrides, zero-width characters, markup and "Server Admin". | `shop::display_text`: ASCII letters, digits, single spaces and `. , ' ! ? - : + # ( )` only. No reserved words (admin, moderator, server, staff, system, official, developer), matched through look-alike folding (`0`/`o`, `1`/`l`/`i`, `3`/`e`, …). Nothing that looks like another player's name. Species names must also differ from other species' names after folding. | `nameplates_and_species_names_are_cosmetic_sinks` |
| 17. A restart reset `request_log` and `attack_alerts`. | Both are saved with the game. | `mating_requests_can_be_blocked_declined_and_are_limited_per_target` (JSON round trip) |
| 25. Retire transfer abuse: animals 75% the retiree's went to a rival holding 25%, then paid the retiree royalties. | An animal moves only if the rival holds more of its alleles than the retiree. The retiree's alleles in it are re-tagged as the receiver's (`Genome::retag_origin`). | `a_retirement_hands_over_only_animals_mostly_a_rivals` (sim) |
| 28. API players without a socket counted as offline (protected while giving orders; "While you were away" each time). | A player's offline spell starts only after `GameConfig::offline_idle_ticks` (10 real minutes) with no connection. Coming back sooner ends nothing and says nothing. | `short_absences_are_not_away` |
| Low: COMMAND_NAMES listed 5 names twice. | Removed; the test now checks for duplicates. | `every_command_has_a_name` |
| Low: self-block was silently accepted; invalid players were dropped without saying so. | `self_block`. `mating_prefs` also takes `blocked` (the whole list); every unknown id is named in `no_such_player` (`detail.players`). | `mating_requests_can_be_blocked_declined_and_are_limited_per_target` |

## Round 5: playtest 4 (names, leaks, collusion)

From `docs/playtest-4.md`. Each has a test. No snapshot change.

| Finding | Fix | Test |
| --- | --- | --- |
| H2. Lineage names were unfiltered ("Server Admin" with a bidi override, public in the history, market, species and achievements, and the founding species' name). | One filter for every chosen name (`crate::names::check`): lineage and design names (`DesignSpec::to_design` and `Game::found`, which also refuses a look-alike of another player's name), species names, nameplate titles, and new account names (reserved words). Names saved before it are cleaned when a game is restored (`Game::sanitize_names`: lineages, species, nameplates; bidi, zero-width and markup characters dropped, a plain fallback for what's left empty or passing for staff), and lineage names stored in the daily history are cleaned as they go out. | `lineage_names_go_through_the_name_filter_and_old_ones_are_cleaned`, `names::tests`, accounts `register_login_and_sessions`, integration `server.rs` |
| M12. Nameplate gaps ("Game Master", "GM", "Mod", "Owner", "Support", "Sysop", "wild", "Herdmaster4 (alt)"). | Reserved fragments anywhere (admin, moderator, gamemaster, sysop, server, staff, system, official, developer) and reserved whole words (gm, mod(s), dev(s), owner, support, helpdesk; "wild" for titles), all matched through look-alike folding. A title containing another player's name counts as a look-alike. Plain errors: "that title can't be used: that looks like a staff or system name" / "1 to 24 characters". | `nameplates_and_species_names_are_cosmetic_sinks`, `names::tests` |
| M20. `/api/spectate` without a login was a radar of every player herd (16-tile cells). | Players' lineages have no cells (`hidden: true`): counts, look and share only. Wild species keep their 8-tile cells. | `players_herds_are_not_placed` |
| M21. A retirement re-tagged the retiree's alleles as the receiver's: colluding players could gift legacy share. | They become the wild's (`Genome::retag_origin(player, WILD)`). The receiver gets the animal, but only the share it bred in. | `a_retirement_hands_over_only_animals_mostly_a_rivals` |
| Low: a `u64::MAX` fee (JavaScript `2 ** 64`) gave a generic `bad_message`. | Price fields (`fee`, `reserve`, `amount`, `price`) take any whole number, saturating past `u64::MAX`, so the range check answers `bad_price`. | `huge_prices_reach_the_range_check` |
| Low: a NaN in a design gave the JSON parser's text. | `bad_design` with a plain description of the body. | integration `server.rs` |
| Low: `preview_mating` gave exact body-plan gaps for a partner just inside the naming margin. | Gaps are given in tenths unless the partner is in plain sight. | `rivals_hidden_state_and_positions_do_not_leak` |

## Round 6: playtest 5 (names, inputs, fairness)

From `docs/playtest-5.md` (M5, M12 in part, and the input checks in `Rulebender5.md`). Each has a test. No snapshot change.

| Finding | Fix | Test |
| --- | --- | --- |
| "wild" passed lineage and species names (the largest species was named "wild"); "G.M." passed because reserved words were checked per word. | `names::reserved` also reads the whole name run together (spaces and punctuation dropped, a leading "the", trailing digits and a team word such as "team"/"crew"/"s" taken off) against whole forms: gm, mod, dev, owner, support, help(desk), claude, wild, root, admin, op(s). "gamemod" and "anthropic" are refused anywhere, "claude" as a word. "Wild horses", "Root voles", "Helpful herd" still pass. Names saved before (a species called "wild") are renamed on load by `sanitize`. | `names::tests` |
| A rival's name minus its digit ("Herdmaster" against "Herdmaster5") passed. | `names::impersonates` also compares with trailing digits dropped (at least 4 characters). | `names::tests` |
| The designer couldn't warn of a look-alike name before Found. | `/api/design/compatibility` gives `name_warning`. | `design_compatibility_samples_individuals` |
| A 49 KB WS message closed the socket silently and dropped a queued reply. | The socket reads up to 256 KB (`MAX_WS_READ`); a command over 16 KB is answered `too_large` (with its id and command when readable) and the connection carries on. Past 256 KB the connection closes as before. | integration `security.rs` |
| A 1.5 MB design body read as a malformed design. | 413 `too_large` ("a request body goes up to 64 KB"). | — (the body limit is axum's; the mapping is in `design_body`) |
| A bare `NaN` in a WS command gave a generic `bad_message`. | `bad_coordinates` ("numbers must be finite"), spotted outside strings (`non_finite_number`). | `bare_nan_and_infinity_are_spotted_outside_strings` |
| `set_view` with a radius ≤ 0 pinned a 1-tile view. | Refused, `bad_coordinates`. | `rivals_hidden_state_and_positions_do_not_leak` |
| possess/split (and other own-animal commands) answered `not_owned` for a living rival animal anywhere: an alive/dead oracle. | `not_owned` for a rival animal out of the player's naming reach reads as `not_found` (`Game::hide_unseen`), like a dead or unknown id. | same |
| Gene keys were normalised silently ("fur " and "FUR" overwrote each other). | Two keys naming one gene: `duplicate_gene`. | `gene_names_parse` |
| A juvenile got `same_sex` before `not_mature`. | Maturity first in `preview_mating` and in requests within one's lineage (cross-player checks already did). | `maturity_is_checked_before_sex` |
| M12: the small-lineage offline shield seemed to start on disconnect. | Already delayed since round 4: no spell (and so no shield of any kind) starts before `offline_idle_ticks` (10 real minutes) without a connection. The test now checks the world's protection itself during the idle time. | `short_absences_are_not_away` |

## Round 7: playtest 6 (names, leaks, offline)

From `docs/playtest-6.md` (M5, M12, M15) and `Rulebender6.md`. Each has a test. No snapshot or sim change.

| Finding | Fix | Test |
| --- | --- | --- |
| A dotted or run-together staff word next to another word passed ("Head G.M.", "Real G.M.", "GMBob", "ClaudeBot", "OwnerBob", "Operator", "Moderater", "Gamemastr", "Root user"). | `names::words`: runs of single letters are joined ("G.M." reads "gm") and CamelCase words are split into their parts ("GMBob": "gm", "bob"); the reserved words gained gms, owners, operator(s); "moderater", "gamemastr", "rootuser" are refused anywhere. | `names::tests` |
| "Wild herd" passed as a species name. | A lineage or species name may not start with "Wild"/"Wilds" (it would read as a wild species); a title may not use the word at all; "Wildebeest", "Rewilded" pass. | `names::tests` |
| A lineage could take a wild species' name ("Meadow vole"). | `Game::wild_name` (a wild lineage's or wild-dominated species' name, folded, plural either way) refuses lineage names at founding, species names and titles; the design check's `name_warning` says so first. | `wild_species_names_are_refused` |
| Account names used a weaker check. | New accounts go through `names::check` like every other name. | accounts `register_login_and_sessions` |
| "Roots" was refused (root + plural). | No plural `s` for plain words among the whole forms (root, help). | `names::tests` |
| Saved offenders ("Head G.M." title, "Wild herd" species). | Cleaned on load by the existing `Game::sanitize_names`, now with the stricter check. | — |
| `/api/species` showed every player species' body plan (coarse). | Another player's species shows no thermoregulation, birth mode, leg or eye count; the caller's own (logged in) and the wild keep theirs; diet and size stay in quarters for the labels. | `rivals_species_show_no_body_plan` |
| The WS `compatibility` command gave rivals' numbers exactly, with the limiting gene. | Same coarsening as the HTTP check (`coarsen_rivals`). | `compatibility_shows_the_diet_split` |
| Error replies gave gene gaps to 17 digits. | `incompatible_body_plans` gaps in tenths (detail and message). | `different_body_plans_are_refused_at_request_time_with_the_genes` |
| `too_big` gave a rival's exact mass. | About: whole kg under 10, then 5 kg, then 10 kg steps (`protocol::coarse_kg`). | `a_too_big_target_gives_its_mass_roughly` |
| M12: home-range protection lasted 12 real hours, thousands of game days at a fast tick. | All offline protection (core cap, home range, fade) is also capped in game days: `GameConfig::offline_max_days`, 900 by default: about 12 real hours at the normal 1 s tick (normal play unchanged), about 75 real minutes at the 100 ms playtest speed. | `offline_protection_is_capped_in_game_days_and_dated_from_the_leaving` |
| The home range started ~20 minutes after leaving, the Lineage tab says 10. | The spell is dated from the leaving (it still starts only after the idle wait), so the onset counts from then. The home-range note mentions the game-day cap. | same |
| `too_large` had no id when the id came after the long field. | The whole message is scanned. | integration `security.rs` |
| A literal duplicate JSON key in a design was accepted. | Refused: `duplicate_gene` ("fur is given twice"). | `gene_names_parse` |
| A `"NaN"` string value gave `bad_message`. | `bad_coordinates`, like a bare NaN. | `bare_nan_and_infinity_are_spotted_outside_strings` |
| `substitute_mating` told a living rival animal from a dead one. | Its `mine` (and `respond_mating`'s `partner`) go through `Game::hide_unseen`. | — |

## Round 8: playtest 7 (names, leaks, Lows)

From `docs/playtest-7.md` (M6, M8 and the Lows in `Rulebender7.md`). Each has a test. No snapshot or sim change.

| Finding | Fix | Test |
| --- | --- | --- |
| Digits beside staff words passed ("Claude2 fan", "GM2 herd", "Mod2 squad", "Head GM2", "Owner2 herd", "Moder8or"). | `names::digit_forms`: each word is also read with its leftover digits dropped and with `8` as "ate". | `staff_words_with_digits_or_run_together_are_refused_but_plain_names_pass` |
| Run-together lower case passed ("claudebot", "GMbob", "ownerbob", "Supportbot"). | Staff words (gm, mod(s), owner(s), claude, support, operator, sysop, root) are refused as the start or end of any word, whatever its case, except common words in `AFFIX_ALLOWED` (model, modest, modern, paradigm, landowner, rootstock, supportive, ownerless, …). A list of 51 ordinary names is checked against false positives. | same |
| "WildHerd", "Wildherd", "A Wild herd", "Wild1 herd" passed. | For lineages, species and titles, no word may start with "wild", except common words in `WILD_ALLOWED` (wildebeest, wildflower, wilderness, wildfire, wildwood, wildcat). | same |
| "Meadow vole 2" and "The Meadow vole" passed as wild species' names. | Wild names are compared by `names::name_key` (articles and digits dropped, plural either way, at least 4 letters). | `wild_species_names_are_refused` |
| An exact copy of a rival's lineage name ("Ash Fang Pack"). | `Game::rival_name` refuses a lineage (at founding) or species name equal by `name_key` to another player's living lineage or species; the design check warns. | same |
| Account look-alikes kept digits and separators ("LateComer_7", "LateComer8", "LateComer" beside "LateComer7"). | Registration also refuses `names::impersonates` look-alikes of any account. (So "user1" and "user2" can't both exist.) | `account_look_alikes_with_digits_or_separators_are_taken` |
| Saved offenders ("Claude2 fan"). | Cleaned on load by `Game::sanitize_names` with the stricter check. | — |
| M8: `preview_mating` gave body-plan gaps, fertilities, mismatch and failure chance exactly for a rival's partner (with one's own animals' genes known, that solved the rival's body plan). | All in tenths unless the partner is the player's own. | `rivals_hidden_state_and_positions_do_not_leak` |
| `/api/metrics` showed `players_online` and per-command counts without a login. | Logged out, `conns` and `players_online` read 0 and `apply_by_command` is empty; timings stay public. (`/api/worlds` still shows players online for the start screen.) | integration `server.rs`, `worlds.rs` |
| Listing positions looked like an 8-tile grid. | Checked: they are the centres of 16-tile cells (8 + 16k), as before; the paid survey gives 8-tile cells. No change. | — |
| Fractional auction `days` gave `bad_message`. | Rounded up to whole days (1.5 → 2); 0 or less gives `bad_duration`. | `fractional_auction_days_round_up` |
| `/api/map.png` needs a login while `/api/terrain` is public. | As intended (terrain has no animals); now in the README. | — |
| Alert ticks seemed to change between reads. | Stored alerts never change; the watcher saw two alerts with the same text (the older later evicted from the capped feed). Alerts now carry a stable per-player `id` (kept over restarts) to tell them apart. | `alerts_have_stable_ids` |

### Follow-up: random e2e account names

| Finding | Fix | Test |
|---|---|---|
| e2e context-loss once failed at registration: base36 ids like `ctx_mi8k3gm2` read as ending in "gm" once all digits were dropped. | Digits now only hide a staff word that comes before the first digit of a word ("Claude2", "GM2", "Wild2", "Moder8or" stay refused). Dropping every digit, which merged random letters ("k3gm2x" → "kgmx"), is gone. The e2e scripts share `client/e2e/names.mjs`, whose `stamp()` gives letter-only suffixes from consonants that spell no word. | `staff_words_with_digits_or_run_together_are_refused_but_plain_names_pass` (random ids pass) |

## Round 9: the name filter (playtest 8, Medium 5)

The staff-word check now works per word instead of on substrings:

- **Strong staff words** (gm, mod, admin, owner, claude, support, operator, sysop, official) are refused as a whole word. They are also refused at either end of a word, or inside a word when a lead-in comes before them (the, my, real, true, head, ...) or a role word comes after them (bot, team, crew, official, user, ...). A word on a small allowlist of common English words passes when it covers the staff word: modest, landowner, cooperator, claudette, gmelina, ... Otherwise a staff word inside a word passes: dogma, badminton, commodore.
- **Soft staff words** (server, system, staff, root, dev, help) are inside common words, so they only count run together with a lead-in or a role word and nothing else: "serverbot", "realstaff", "rootuser".
- **Long fragments** (moderator, administrator, gamemaster, sysop, developer, anthropic, rootuser) are still refused anywhere in the name.
- **Wild:** a word reads as the wild only when "wild" is followed by a tail meaning a group (herd, pack, lings, born, ...) or comes after a lead-in ("TheWild").
- **Digits and split letters:** digits at both ends of a word are taken off ("2claude2", "2wild"). A single letter set apart is read with the word next to it ("C laude").

| Finding | Fix | Test |
|---|---|---|
| False positives: "Taproot/Beetroot/Arrowroot", "Ecosystem/Systematic", "Observer", "Badminton", "Distaff", "Cooperator", "Modesty", "Modena", "Claudette", "Wildly/Wildrose/Wilder/Oscar Wilde", "Supports", "The Help", "Gmelina", "Staffpick". | Per-word matching as above. "Supports" is a plain plural. "Help" is no longer a reserved whole name ("Help desk" still is). "RootGrazers" now passes, like "Root grazers". | `staff_words_are_matched_per_word_with_common_words_allowed` |
| Bypasses: "realclaudebot", "theownerbob", "mygmherd", "2claude2 fan", "C laude fan", "2wild herd". | Lead-ins and role words, trimmed digits, joined single letters. "Moder8or" is caught by reading 8 as "at" ("moderator"). | same |
| Copies with extras of a wild or rival name ("Meadow vole II", "Red Foxes", "New …", "Sunbright Drove II"). | `names::copies` compares word by word, setting aside articles, digits, roman numerals and plurals ("foxes" → "fox"). The copied words may be run together, with only decorating words added (new, old, real, second, ...). "Meadow vole hunters" still passes. | `copies_with_decorations_are_copies`, `wild_species_names_are_refused` |
| Animal names and nameplates skipped the wild-name and rival-name checks. | `name_animal` checks both (`bad_name` / `name_taken`). `set_nameplate` also checks rival lineage names (`bad_title`). | `wild_species_names_are_refused` |
| Account look-alikes with a prefix or suffix ("8Rulebender", "Herdmaster8x"). | Names that match once every digit is dropped are look-alikes, for any kind of name. A new account that holds another account's letters with at most 2 more (at least 5 letters) is also a look-alike. | `titles_naming_another_player_are_look_alikes` |

A random id with digits around a staff word ("8mod2") is now refused, the same as "2claude2". The e2e names have been letter-only since the previous follow-up.

## Round 10 (playtest 9)

| Finding | Fix | Test |
|---|---|---|
| High 5: a pinned view kept watching after the animals left or died (`set_view` was checked only when pinned). The default view centred on the mean of all animals, which could fall between groups. | `Game::recheck_views` runs every tick (`after_tick`). A pinned view narrows to what the player can still see, and is dropped once none of their animals (nor their largest group) sees its centre. The lineage centre (the default view and its 8-tile sight margin) is now the largest group's: the mean of the animals in the 3×3 block of 16-tile cells holding the most of them. | `a_pinned_view_is_dropped_once_the_animals_leave`; `rival_outlines_follow_each_group…` now moves 2 of 6 founders away (3 of 6 tied the two groups) |
| Medium 8: lineage, species and nameplate names holding a player's name ("Herdmaster9x", "The Herdmaster9", "Herdmaster9 herd", "Herdmaster9 II", "Real Herdmaster"). Accounts "TheHerdmaster9" and "Herdmaster9fan". | `names::impersonates` refuses, for every kind, a name holding another player's account letters (at least 5, digits dropped). | look-alike table in `titles_naming_another_player_are_look_alikes` |
| Near copies of rival and wild names ("Ironjaws", "Emberwool Drovers", "Steppe Antelope herd", "Red fox pack"). | `names::copies` adds group words (herd, pack, drove, flock, …, of) to the decorations, and reads an agent "-er" as the word ("drovers" → "drove"). | `copies_with_decorations_are_copies` |
| Name checks ran before "already founded": a free probe of rivals' and wild names. | `found` answers `already_founded` first. | `wild_species_names_are_refused` |
| Medium 14: `/api/metrics` showed anonymous callers connections opened and closed, login hash counts and world-wide `applied`. | Without a login, every count of players or their activity reads 0: sockets, commands, pushes, messages, bytes and hashes. Timings and saves stay public. loadgen reads metrics as one of its players. | integration `server.rs`, `security.rs` |
| `/api/worlds` showed players online without a login. | `players_online` is left out for anonymous callers (optional in the protocol; the world picker omits it). `/api/assign` is unchanged, for the lobby. | integration `worlds.rs`, `push.rs`; vitest `worldChoice` |
| `same_sex` told a rival animal's sex when it was named beyond the view, within the naming margin. | `same_sex` only where the partner's sex is already shown (own, in sight, or listed); otherwise `partner_unavailable`. Ownership and visibility are still checked first. | `same_sex_is_told_only_where_the_sex_is_shown` |
| Listings showed a herd's position to a 16-tile cell. | 32-tile regions. The distance to an unseen listed partner is rounded to 32 tiles. Offers between the two parties keep their 8-tile cells. | info-leak test |
| The migrate reply's `others_on_the_way` worked as a radar. | A band: "few" (< 20), "some" (< 100), "many". | — |

## Round 11 (playtest 10)

| Finding | Fix | Test |
|---|---|---|
| Medium 4: one-letter deletions of a player's name passed in nameplates and animal names ("LateComr10", "Herdmastr10", "Hrdmaster10", "GeneBrokr", "ApexHuntr", "PocketNomd", "Herdmaste"). | `names::impersonates` also refuses, for every kind, a stretch of the name within one edit (a letter added, dropped or changed) of another player's name of at least 6 letters (`within_one`, `near_inside`). Look-alike folding still applies, so "rn" reads as "m". | look-alike table, `one_edit_apart` |
| Rival lineage names passed in titles and animal names ("Ironfang", "Ironfangs", "Ironfang chief", "Cinderfleece"). | `names::copies` compares both names without their decorations ("Cinderfleece" = "Cinderfleece Drove"), and allows one edit at 6+ letters. Titles and animal names may not hold a living rival lineage's name, decorations dropped, anywhere (`names::holds_name`, `Game::holds_rival_name`). The name checks already came before the funds check. | `copies_with_decorations_are_copies`, `one_edit_apart` |
| "Nomad Hunter", built from two players' names. | Allowed: generic words, and neither name (at least 5 letters) is reproduced. | look-alike table (`false`) |
| Names were checked only against players who existed at naming time ("LateComer10s pet" before LateComer10 joined). | Each world publishes its public names (`Published::public_names`: nameplate titles, animal names, players' current lineage names, player-named species), refreshed after commands and ticks. A new account name that one of them reads as is `name_taken`. | integration `server.rs` ("Latecomer herd" → account "Latecomer" taken) |
| e2e names in a row could be one letter apart. | `stamp()` scrambles its counter (a multiply modulo 13^10), so consecutive names differ in most letters. | e2e |
| Medium 13: logged-in players saw `/api/metrics` `players_online`, connection and login counts, and `apply_ms.n`. | These counts now need the operator token (`ServerConfig::admin_token`, `EVO_ADMIN_TOKEN` by default), logged in or not; `apply_ms.n` reads 0 without it. loadgen starts its servers with a token and sends it; the iOS smoke script does the same. | integration `server.rs`, `security.rs`, `worlds.rs` (a shared `operator_get`) |
| Migrate bands had no cooldown (8 a second mapped the crowds). The `restore_pasture` preview gave an exact census of rivals. Claim and rally circles counted prey anywhere. | Only animals the player can see count: the journey's `others_on_the_way` (and the route food share it feeds), and the restore price's count of other players' animals in the circle (main dropped the preview's `also_feeds` list). A claim or rally circle gives `prey_nearby`/`prey_feeds` only when the player sees its centre; otherwise a warning says the prey isn't counted. | `circle_prey_is_counted_only_in_sight` |
| Listings looked like 16-tile cells. | Checked: since round 10 they are the centres of 32-tile regions (16, 48, 80, 112, …), which the report read as 16-tile cell corners. The Market says "somewhere around x, y", not "near x, y" (worded so on main too). | — |

## Round 12 (playtest 11)

| Finding | Fix | Test |
|---|---|---|
| Medium 7: neighbour swaps passed ("Herdmatser", "Rulebedner", "LateCmoer", "GeneBorker"). | `within_one` also counts a swap of two neighbours (Damerau distance). | `one_edit_apart`, look-alike table |
| Blends of two players' names ("Herd Nomad", "Pocket Master", "Apex Nomad"). | Rule: a name whose letters are the start of one player's name and the end of another's (at least 4 letters each, two different players) is a look-alike (`names::blends`, in `impersonates`, so every kind and new accounts). Two ends ("Nomad Hunter") or two starts ("Herd Pocket") are generic enough to pass. | `blends_of_two_names_are_look_alikes` |
| Species names near rival lineage names ("Skyglass Swarn" beside "Skyglass Swarm"); "Thornbak". | `name_species` also checks `holds_rival_name`. `holds_name` also matches one longer word (7+ letters) of a rival's name a letter off. | `one_edit_apart` (`holds_name` cases) |
| Name checks before the price were a free oracle (`bad_name` vs `insufficient_funds`). | `set_nameplate`, `name_animal` and `name_species` check the colour, ownership, the species' eligibility ("already named", "not yours") and the price first, then the names. Only the plain charset/staff check (which says nothing about other players) still comes first. | `name_checks_come_after_the_price` |
| Existing names that now fail. | Left as they are (no startup scan): the checks apply from the next naming on. | — |
| Medium 8: restore price against the shop's own was an exact count of rivals in the circle. | Main counts rivals in the circle (economy round 11) and only those the player sees (review round 11). That count is now rounded up to tens. | — |
| Way warnings gave exact rival hunter counts along any drawn way; rally then cancel was a free radar. | Only hunters the player sees are named, as "a few", "some" or "many". Rally and claim replies already gave prey only where seen (round 11), so drawing a circle anywhere now reveals nothing beyond what the player sees. | `way_warnings_name_only_hunters_in_sight_in_bands` |
| `players_online` in `/api/assign` (anonymous) and `/api/worlds` (logged in). | Both only for the operator token (`/api/assign` reads 0, `/api/worlds` leaves it out). `evo-lobby` sends the token. The mobile e2e runs its servers with one. | integration `worlds.rs`, `push.rs` |
| Listed animals can be requested from anywhere; a pending request's `apart` (8-tile steps, refreshed every push) narrowed one below its 32-tile region. | `apart` is in 32-tile steps while the requester doesn't see the partner. | — |
| Anonymous `/api/metrics` command and save timings. | `apply_ms` (all of it), the save timings and `save_bytes` read 0 without the operator token. Tick and state timings stay public. | integration `server.rs` |

## Round 13 (playtest 12)

| Finding | Fix | Test |
|---|---|---|
| Medium 4: a player's name with its words reversed passed everywhere ("Master Herd", "Hunter Apex", "Bender Rule", "Comer Late", species "Nomad Pocket"). | `names::reordered`: a name whose words (2 to 5, at least 3 letters each), put in another order, spell a player's name (6+ letters, a letter off at most) is a look-alike, for every kind. | `blends_of_two_names_are_look_alikes` (reordered cases) |
| Two neighbour swaps ("Hedrmatser"). | Names of 9+ letters: within two edits (optimal string alignment, `within_two`) of the whole name or one of its words. Not any stretch: "Herd of masters" passes. | same |
| Blends with a rival lineage name ("Apex Drifter"). | `Game::blends_rivals`: `names::blends` over other players' names and their living lineage names (with and without a plural s). Applied with `holds_rival_name` (titles, animal and species names) and at founding. | `blends_of_two_names_are_look_alikes` (lineage cores) |
| Medium 5: migrate, claim and rally warnings named rival territories (with coordinates) anywhere on the map. The cooldown was per group. | `way_warnings` (territory at the destination, crossed on the way) counts only territories the player sees, by the state's outline rule (within 48 tiles of one of their groups). Unseen territories are not mentioned at all. The destination warning (`rival_ground_at`) already used that rule. The migrate/rally cooldown is now per player: one order a second across all their groups. | `move_orders_have_a_short_cooldown` |
| Low: anonymous `/api/world` gave exact grace days for every graced player, a retired lineage included. | `protected` lists living lineages only, with `days_left` rounded up to whole days. | — |
| Anonymous `/api/metrics` `state_encode_ns` grew with connections. | Zero without the operator token. | — |
| A listing's purchase streamed the seller's animal's position; `meet_note` placed out-of-sight predators. | An agreement from a listing gives both animals and the meeting point as 32-tile region centres, with the distance in 16-tile steps (other agreements keep 8-tile cells and 4-tile steps). The note says the point moved to keep clear of a predator, without how far or which way. | — |
| `others_on_the_way` not filtered by sight. | Already counts only grazers the player sees, since round 11; the playtest build predated it. | — |

## Round 14 (playtest 13)

| Finding | Fix | Test |
|---|---|---|
| Medium 8: rearrangements with fillers or run together passed ("Nomad Pocket II", "Nomadpocket", "Comer Late II", "Master Herd Jr", "Masterherd", "The Master Herd", "Apex the Hunter"), and two swaps split across words ("Pokcet Nomda II"). | `names::rearranged` replaces `reordered`. It first normalises the text (`normalised`): drops fillers (`FILLERS`: the, of, jr, new, real, fan, …), roman numerals, digits and words of one or two letters, then joins the rest. It then compares the result with every rotation of each player's name ("masterherd" for "Herdmaster") and every order of the name's CamelCase parts: a letter off anywhere, or two off as a whole for 9+ letters. As a consequence, "Masters of the herd" and "Herd of masters" now read as Herdmaster too. | `blends_of_two_names_are_look_alikes` (playtest 13 cases) |
| "Nomad Pocket II" stood as a species name for good. | `Game::rename_look_alikes`, run on every restore once the accounts' names are set (`lib.rs`), renames names that now read as another player's: a player-named species becomes "Species N", a nameplate "Breeder", and an animal's name is dropped. The count is logged. | `old_look_alike_names_are_renamed` |
| Low: grace days (rounded) still public in anonymous `/api/world`. | `protected` lists who is under grace without `days_left` (optional in the protocol). The client shows "newcomer grace" without a duration for others. Owners read their own days in `lineage_stats` and the state. | `orders_tests` (days_left none) |
| A pending lineage offer's `apart` was recomputed live (possessing the offered animal ranged the other side's). | `Offer::apart` is frozen when the offer is made (and when it becomes a lineage offer). An offer saved before this is computed as before. | — |
| The `too_far` refusal gave the exact distance. | "about N tiles apart", in 8-tile steps. | — |
| `/api/leaderboard/history` gave rival lineages' `morph` to 3 decimals. | Other players' lineages: eye azimuth to 15°, snout, horns and wings to quarters (`public_morph`), like `public_genes`. | — |
| Mating updates of a pair asleep flapped 68 ↔ 72. | An update is news only when the meeting point, the asleep or possessed flags, or the distance in 16-tile steps change (`meet_key`). | — |
