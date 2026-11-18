# Rulebender3 — playtest 3 log

Persona: competitive rules prober (normal client + API). Server http://127.0.0.1:8092. Player id 1.
Tools: Node WS daemon (rb3/Rulebender3-daemon.mjs, protocol 1 JSON) + curl.

## Timeline
- 07:12 (day 19) Registered Rulebender3 (player 1). World: only wild (1090).
- 07:15 (day ~25) Founded "Clause Moles" (lineage 4) by API: warm, incubated_eggs, litter 3, herbivore, body .5, fur/fat .5, hib .45, burrow .5, estivation .3, vigilance .4, 60 founders, cost 52.15/100.
- 07:13 (day 29) State carries `grace: {days_left 370.8, until_tick 19229, real_seconds_left 1780}` -> grace indicator data present in protocol-1 state too.

## Probes, round 1 (07:13-07:15, days 29-50)
### Validation — FIXED
- Bad messages for set_nameplate / name_species / offer_to_lineage / retire_lineage / watch_season / possess_attack: reply names the command, generic text. COMMAND_NAMES fixed. (Low, code: protocol.rs COMMAND_NAMES lists retire_lineage, renown_shop, buy_mating_slot, reinforce, compatibility TWICE — harmless duplicate.)
- id "771" (numeric string) -> reply id 771. FIXED. id "strid" -> no id (expected). `{"type":"possess","id":772,"organism":1e309}` -> id kept. Non-JSON `garbage "id": 773 "type":"migrate"` -> id 773 + command migrate. Truncated JSON -> id kept. FIXED.
- mating_prefs block 0 / 999 -> no_such_player. FIXED. (block = own player id 1 -> ok, silently no-op; unblock 0 -> ok no-op. Low/cosmetic.)
- migrate / clear_migration / claim / rally / cancel_rally / abandon / split / merge on group 200 -> no_such_group. FIXED. merge 0 into 0 -> same_group.
### Leaks
- /api/leaderboard/history?genes=true without login -> 401; default without login has no genes. With login: own lineage all genes, rivals only body_size/fur/leg_length rounded to .25 (+ exact morph like eye_azimuth/snout_length, which are visible anyway). FIXED.
- /api/spectate: every player lineage (all 5, founded after tick 0) is `hidden:true`, 0 cells. FIXED.
- set_view: far point -> out_of_sight; radius 64 at my animal -> narrowed to ~18. Scan with one set_view per 16-tile cell of mine (7 calls) listed 471 non-own animals (118 Herdmaster3, 21 ApexHunter3, 4 PocketNomad3); farthest 24.3 tiles from my nearest animal (was 43+, 1,000+ animals in 15 calls). PARTLY FIXED by design (16-tile margin over 3-7 tile sight is still 3-5x what an animal perceives). Low.
- preview_mating: an animal out of sight -> out_of_sight. BUT the checks run in the wrong order: an out-of-sight id answers `not_found` (doesn't exist) or `same_sex` (#50, out of sight, wild) before the sight check. Existence + sex oracle for any id. Low-Medium. Steps: preview_mating mine=<my male> partner=50 -> same_sex; partner=400 -> not_found; partner=900 -> out_of_sight.
- NEW: request_mating has no sight check at all: for any organism id anywhere it answers not_found / no_consent ("wild organisms have no owner") / not_mature / (for rivals) creates an offer. Ownership + maturity oracle for any id, and offers can be sent to rival animals you have never seen (ids are sequential). Low-Medium.
- /api/species gives each species' exact mean carnivory, body_size, endothermy, birth_mode (player species too). Coarse-genes fix bypassed for 4 genes. Low.
### Grace
- Grace = 30 real minutes = 370 game days (~3 game years) at this server's 100 ms tick. At 1 s ticks it would be 37 days. At playtest speed a newcomer can't be hunted by players for three winters. Balance note, Medium (only at fast tick; consider max(days, minutes) -> min?).
### Economy constants seen
- Shop: nameplate 500, species name 300, reinforce 150 (max 2 per ranked season, only <=300 alive), slot 30, royalty 5/pt.
- Per-target request window is now 60 real minutes (36,000 ticks = 750 game days here): at most 3 requests to one player per hour. Pending offer lives 10 min.
- economy.rs doc contradiction: module doc says royalty pays for alleles "in rivals' animals or the wild"; the constant's doc says "alleles in the wild don't pay". Low (doc).

## Probes, round 2 (07:15-07:17, days 62-92)
- set_view reveals graced lineages: nearby list gives exact positions (with `protected: true`) of PocketNomad3's and Herdmaster3's animals while /api/spectate hides them. Spectate fix is cosmetic for anyone whose animals are spread. Low.
- offer_to_lineage: own lineage -> own_organism; wild 0 / 99 / 3(wild) -> unknown_lineage; price 1000001 -> bad_price; 99999 -> insufficient_funds. Same male in 3 pending offers at once allowed (male serves several: fine). OK.
- 07:15 (day 64-66) opened: offer 1 (request_mating #1456 -> PocketNomad3 #1713, 5 renown), offer 2 (offer_to_lineage Herdmaster3 lineage 7, 3 renown), offer 3 (ApexHunter3 lineage 8, 2 renown).
- Offer 2 accepted by Herdmaster3 ~day 68: server picked #2580 (20.9 tiles away); `expires_tick` became accepted+18000 (=30 real min).
- 07:17 day 90->91 weather season change autumn->winter: offer 3 (pending) and offer 2 (accepted) both SURVIVED. FIXED: offers no longer expire at weather-season changes.
- NEW (Low-Medium leak): offer 1 was never accepted, yet it closed `failed` with `already_pregnant` "the female is already pregnant" on day 89 (+refund alert). The pending-offer failure path tells the sender a rival female's hidden pregnancy, which `request_mating` hides as `partner_unavailable`.
- Dam lock, new shape (Medium): an accepted agreement holds the dam from breeding (`hold_breeding`) until the 30-REAL-minute deadline. At 100 ms ticks that is ~375 game days (~3 game years, most of a female's breeding life) for a 3-renown offer, and the lapse costs the buyer 10% (min 1 renown). Mitigation: the dam's owner can cancel_mating, but only if they notice. Deadline should scale with game time (e.g. min(30 real min, N game days)).
- possess_attack ordering: checks sight, burrow, size, target grace, THEN own grace. All 6 players are graced (none founded at tick 0), so own_grace is unreachable live: every player target answers newcomer_grace first. Code (sim world/commands.rs possess_attack) is right.

## Shop / nameplates / UI check-in (07:18-07:20, days 100-118)
- 07:19 set_nameplate validation: bad colour -> bad_color; "", "   ", 25 chars, "line\nbreak" -> bad_title. BUT zero-width space (U+200B) and RTL override (U+202E) are ACCEPTED ("zero​width", "rtl‮evil" bought, 500 + 100 renown). clean_text only rejects Cc controls; Cf format characters pass (also for name_species). Player names are ASCII-only, titles/species names are not: invisible chars, bidi spoofing, look-alikes. Medium-Low (cosmetic spoofing; client never shows titles anyway, see next).
- NEW (Medium): nameplates are never shown. The client sends `set_nameplate` from the Market tab ("Cosmetic: shown with your name") but no client code requests `nameplates` or renders anyone's title/colour (grep client/src: only protocol.ts, errors.ts, market.ts). /api/leaderboard rows carry no nameplate. Players pay 500 renown for something nobody (not even themselves on the ranks) sees. Steps: Market -> Renown shop -> pick colour + title -> buy; look at Ranks/Lineage/leaderboard: no title anywhere.
- NEW (Medium): no UI for name_species at all (the shop note just says naming costs 300). API only.
- name_species code (crates/server/src/game/shop.rs): uniqueness only case-insensitive ASCII against other species names; no skeleton/look-alike check (player names have one), no check against lineage or player names, Unicode allowed. Low.
- UI: Lineage tab shows "Newcomer grace 283 days (≈ 23 min) left: …" with a clear explanation. Soft cap explained (crowding 192/1,500, hard cap 2,100). FIXED (playtest-2 #7 indicator, #8 soft cap).
- UI Market "Who your lineage can breed with": percentages are the mean pair fertility (API `fertility`), the footnote says "From the best pair in a sample of adults" — wrong now that it's sampled means. Low.
- UI Market: picker "For my adult" none-option reads "(none: no compatibility)", which looks like "you have no compatible adult". Low wording.
- Ledger summary now lists royalties and shop spending: "earned 456 (royalties 0), paid into escrow 0, sales 0, refunds 10, spent in the shop 600". FIXED (playtest-2 small item).

NOTE: wall-clock stamps above after 07:16 were my estimates and run fast; real clock: founding 07:13, offers 07:15, season-change check 07:17, nameplate 07:19, UI check-in 07:20. From here on stamps are from `date`.

## 07:21 (day 131) new lapse test
- Male #1456 (the one in offers 2 and 3) died of thirst ~day 95, so both closed `failed` "died (thirst) before the mating". (Low: the failed view of offer 2 shows expires_tick 9347, its old pending expiry, instead of the agreement's 21482.)
- Bought GeneBroker3's stud listing #1 (#12659, 10 renown) for my dam #1471, 43 tiles away: offer 7 accepted, expires_tick 24296 (=+18000 = 30 min, ~07:51). I will NOT bring her. Expect: lapse, seller keeps 1, 9 refunded.
- Offers 8 (Herdmaster3 lineage 7) and 9 (GeneBroker3 lineage 10) via offer_to_lineage from male #1470, 4 renown each.

## 07:22-07:25 (days 141-176)
- Auctions: `ticks:1` -> deadline +3000 ticks (5-min minimum). FIXED (1-tick auctions). Error text "1 to 1440 ticks" doesn't mention the 5-min floor (Low). days:0 -> bad_duration; expires_days 0 -> bad_expiry; juvenile -> not_mature.
- Listing a PREGNANT dam is allowed and the listing says `busy:false` (Low: a buyer pays into escrow for a dam that can't conceive; it fails later and refunds).
- Ranked season = 60 game days = 5 real minutes here. Reinforcements: max 2 per ranked season and a lineage <=300 alive -> a small lineage can buy +20 adults every 5 real minutes for 450 renown, about a small lineage's whole income. Not a loop exploit any more (counts survive retire, see below), but at this tick speed reinforcement is a steady 'renown -> adults' conversion. Medium-Low balance (fine at 1 s ticks: 50 min seasons).
- Preview distances: an accepted-offer partner (#14880, GeneBroker3, graced) and a market stud (#12659) answered EXACT distances; trilateration from 4 of my animals located them to within ~0.5 tile (resid 0.08), 9.2 tiles from my nearest animal. Consistent with the set_view reach (sight + 16), so not a new leak, but the market's 16-tile coarse cell is defeated by preview trilateration whenever the lot is within ~23 tiles of any of my animals. Low.
- Offers 7 (stud bought) and 9 failed: the partners died of thirst. Only offer 8 (Herdmaster3 dam #16374, accepted ~day 163, expires_tick 25833 ≈ 07:55) is left for the lapse test. Matings tab shows "Lapses if they haven't met in 372 days (≈ 30 min)". Good UX.
- GeneBroker3 sent me offers 4, 10, 11 (I'm API-only so never answered; they failed when animals died).
- Ranks tab: I lead season 3 legacy (5.4%) and largest lineage; won seasons 1-2.
- 07:25 claimed territory (26,47) r12 for water (166 drinkable tiles, 10%): thirst is killing 5-12 a day in the summer drought.

## 07:25-07:28 (days 176-210) territory die-off, reinforcements, retire + re-found
- NEW (High): one territory claim wiped out the lineage. 07:25 day 176: 420 alive, deaths 3-7/day (thirst) in the summer drought. Claimed territory (26,47) r12 for group 0 (all 420, spread x 10..106): reply `drinkable_tiles 166, water_share 0.10`, no warning. Days 186-197: 13, 26, 21, 23, 22, 31, 28, 30, 34, 13, 30 deaths/day (thirst + starvation), 0-2 births. Day 204: 67 alive (-84% in ~2 real minutes). The claim pulls a whole spread-out lineage into a 12-tile circle: food runs out and the shallow drinking tiles crowd. The water check exists (playtest-2 #10 partly fixed) but nothing checks food/capacity vs. members, and no alert says "your territory can't feed N animals". Also the founders were ~180 days old and dying of old age at once (cohort, playtest-2 #1).
- The territory group showed `home_range: true, protection_left_min: 720` while I was online (daemon connected). Confusing (Low).
- Offer 8 (the last lapse test) failed on day 186: my male #1470 died of thirst. Lapse not observable live (3 attempts, all ended by deaths first).
- PocketNomad3 retired: alert "8 of its animals, mostly of your genes, joined yours" (retire transfer works).
- 07:27 day 209: 62 alive (<300) -> reinforce: #1 ok (+10, 150), #2 ok (+10, 300), #3 -> limit_reached "at most 2 reinforcements a ranked season (retired lineages count)".
- 07:28 day 210: retire_lineage 4 -> removed 77, transferred 2. Re-founded "Clause Moles II" (lineage 14, 70 founders). reinforce -> limit_reached (retired lineages count). FIXED (reinforcement loop).
- NOT FIXED (High): grace RESTARTED on re-founding. Before retiring: grace real_seconds_left 930 (until_tick 19229). New lineage 14: `grace {days_left 374.9, until_tick 28118, real_seconds_left 1800}` - a fresh 30 minutes. Cause (crates/sim/src/world/commands.rs `grace_cooldown`): the "unbroken run" window is 2 x `Balance::attack_grace` DAYS (40 days = 1,920 ticks = 3.2 real min at 100 ms), ignoring `grace_min_ticks` (30 real min = 18,000 ticks) that sets the grace's actual length. Any re-founding more than 40 game days after the previous one starts a new run. At this speed a player can re-found every ~25 min and never leave grace (never huntable by players); at 1 s ticks the window is 32 min vs a 30-min grace, i.e. ~1x not 2x. Fix: cooldown = 2 x the grace length actually used (max of days and min ticks).
- 07:28 every other player has also retired/re-founded at least once (lineages 13 Apex Foxhounds, 15 Horned Skywatchers, 16 Mendel and Sons): with the grace bug each re-founding after 40 game days gives them a fresh 30-min grace.
- 07:28 lapse test #4: bought GeneBroker3 stud listing 7 (#27122, 5 renown) for my fresh founder dam #26902 (distance reported "80" = rounded, out of reach: rounding works). Offer 15 accepted, expires_tick 28608 (~07:58:50). Also bid 2 on auction 8 (#27124, reserve 1) with #26904; closes tick 13465.

## 07:29-07:31 (days 215-240)
- Nameplate change (100): title "<b>Server Admin</b>" in jet accepted. No markup/impersonation filter (Low while titles are not displayed; becomes Medium the day they are).
- /api/design/compatibility (login required) answers, for ANY design, each lineage's closest-pair gap + limiting body-plan gene: varying one gene at a time solves a rival's body-plan genes (birth mode, diet, warm blood, size). Same 4 genes /api/species already gives as exact means. Low.
- Attack feedback: after my possess_attack (#19055 -> wild #21384) an alert came at once: "Attack over: #21384 went to ground (a burrow or shelter) and escaped your #19055." FIXED (playtest-2 #12 no feedback).
- Species tracker: my extinct species 4 "Clause Moles" (merged into antelope day 140) was REVIVED on day ~223 as Herdmaster3's species (dominant Herdmaster3, 397 adults), still named after my old lineage. Hysteresis re-uses the identity; the name now misleads (Low).
- Incoming offer 14 (GeneBroker3 offer_to_lineage, 10 renown). respond_mating partner checks: rival's animal -> not_owned; my dam busy in offer 15 -> partner_unavailable; my male -> partner_unavailable; my juvenile -> partner_unavailable; id 99999999 -> not_owned; accepting someone else's offer 15 -> unknown_offer. All good. Accepted with no partner -> server picked my #26948, expires_tick 29371 (+30 min). Low: the `respond_mating` result has no data (the picked partner only arrives in a mating_update, which carries error `too_far` - shown to the accepter like an error).
- Listing: a dam already promised in an accepted agreement (#26948 in offer 14, #26902 in offer 15) can be put up as a FIXED listing (shows busy:true), but create_auction refuses (`organism_busy`). Inconsistent, Low.
- 07:31 bought 1 extra mating slot (30): slots 7, next price 60. OK.
- NEW / dam-lock still open (High): the buyer can dodge the lapse fee. 07:31 day 250 I cancelled my accepted bought-listing agreement (offer 15, 5 renown) with `cancel_mating`: status cancelled, "Refund: 5 renown back" - the FULL price, no 10% lapse fee. So: buy a rival's listed dam (the listing is consent), never bring the male, cancel at 29 min -> she was held from breeding (`hold_breeding`) for 30 real minutes (~375 game days here) for free. The deadline only bites a buyer who forgets to cancel. Fix idea: a buyer's cancel of an accepted agreement after N minutes pays the same lapse fee (or a fee rising with time held).

## 07:32-07:34 (days 256-286)
- Offer 14 failed day 271: GeneBroker3's stud #27118 died of thirst. My dam #26948 died of thirst day 277. Auction 8 closed day 280: "Auction #8 won with 2 renown but it can't run (organism 26904 died before the mating): refunded." Auction settlement + refund work.
- Lapse attempts #1-#5 ALL ended by a death (thirst/starvation) long before the 30-min deadline. Finding (Medium): the 30-REAL-minute deadline = ~375 game days at this speed, longer than most animals live in this harsh world, so the lapse (and its 10% fee) practically never fires; every outcome is "died -> full refund" or "buyer cancels -> full refund". Verdict on the purchase deadline: the code path now exists and offers no longer expire at season changes, but I could not see it fire in 4 tries over 20 min, and it is trivially dodged by cancelling.
- Founders of my re-founded lineage (landed SE, ~92,112) dying of thirst/starvation days 269-283 in a dry spring. 07:34 migrate (95,100) toward a small lake (route returned, 11 waypoints). Avoided territory this time.
- 07:35 UI Lineage tab after the re-found: "Newcomer grace 288 days (≈ 23 min) left" - confirms the restarted grace in the client (screenshot shots/Rulebender3-c-lineage.png). The help text (client/src/lineageInfo.ts:120 and onboarding.ts:282) says "Wild predators, and animals hunting on instinct, still can" - but the sim's instinct hunting (decide.rs:1226) and strikes (apply.rs:774) both use grace_spares, so other PLAYERS' animals can't hunt a graced lineage on instinct either. Wording wrong (Low). onboarding.ts also promises "Founding again soon after does not start it over" - false at this tick (see grace bug).
- watch_season limit 100000 -> capped at 200; ledger limit 100000 -> 100 entries. OK.
- Code note (Medium-Low, theoretical): retire transfers each of my animals with >=25% of a rival's alleles into that rival's lineage (crates/sim/src/world/hosts.rs retire_player_sparing), even if 75% of its alleles are mine. Two abuses: (1) royalty farming - after re-founding, my alleles in those transferred animals live "in other players' lineages" and pay me 5 renown/day per 1% of all alleles (traded_share); (2) dumping - a player can push a rival's crowding count up (litters shrink toward the soft cap) with animals that score mostly for someone else. Suggest transferring only animals whose majority of alleles is the rival's, and excluding transferred animals' alleles from the retiree's royalty.
- 07:35 day 307 /api/world `protected` (public): me lineage 14 278.6 days left, GeneBroker3 lineage 16 248.4, PocketNomad3 lineage 18 323.6, ApexHunter3 lineage 17 310.9, Herdmaster3 lineage 7 103.6 (the only one never re-founded; first founded ~day 37). Four of five players are on a restarted grace after re-founding: the grace-reset bug is hit in normal play, not just by me.

## 07:36-07:45 (days 310-415)
- /api/leaderboard/history?limit=999999999 without login returns the full history from day 0: 474 KB (313 days) per unauthenticated request vs 70 KB by default. Bounded by the history cap, but an easy bandwidth amplifier. Low.
- /api/seasons/<huge>, /-1 -> bad_season; /seasons/player/abc -> bad_player. OK.
- 07:44 day 411 Herdmaster3's grace (founded ~day 37, never re-founded) ended: /api/spectate now shows lineage 7 (1,893 alive, 17 cells); all re-founded lineages still hidden. LateComer3 joined (lineage 19, 295 days). ApexHunter3 is on lineage 20 with a FRESH 345 days (re-founded again).
- 07:44 two-sided grace: my possessed (graced) animals vs Herdmaster3 (no longer graced) -> `own_grace` "your lineage is still under its newcomer grace; it can't attack other players for another 170 days" (detail.days_left). FIXED. (Low: the text gives game days only, not the ~14 real minutes the Lineage tab shows.)
- Attack-alert rate limit: not testable live (every player I could attack, or who could attack me, was graced). Code check (game.rs alert_attacked): one alert per (attacker lineage, victim) per 5 real min; keyed on the attacker's lineage, so re-founding (cheap, see grace bug) opens a new key. Looks right.

## 07:55 offer 20; ~07:56 MACHINE RESTART; 07:58-08:00 what survived (days 566-591)
- 07:55 accepted GeneBroker3's offer 20 (15 renown, server picked my #66597, expires_tick 44699).
- Restart: server restored from the day-566 save (my last event before it was day 567, so ~1 day rolled back).
- Sessions: my token -> 401 after the restart; had to log in again (sessions are not saved). Low (a web player gets logged out).
- Grace: SURVIVED correctly. Before: until_tick 28118; after: /api/world protected lineage 14 days_left 14.2 at tick 27435 and state grace until_tick 28118, real_seconds_left 60. Other players' graces continued too (GeneBroker3's lineage 16 ended and became visible in spectate). grace_min_ticks is re-applied from config at start; no reset on restart.
- Shop counters: reinforcements_bought 0 (correct: my 2 were in ranked season 4, now season 10), slots_bought 0 (correct: winter began day 570). Nameplate kept. mating_prefs kept. Balance 2338 kept.
- Offer 20 (accepted before the save) was gone after the restart with no mating_update, alert or ledger line for me; GeneBroker3's stud #61473 is not_found (dead), so it probably failed by death while I was disconnected. The non-buying party never learns an agreement closed while offline (Low; Matings tab history may show it).
- NEW (Medium): per-target request window resets on restart. Before the restart I had sent Herdmaster3 2 requests in the 60-min window (07:15, 07:21). After it: 3 offer_to_lineage to lineage 7 accepted (offers 22-24), the 4th -> too_many_requests. game.rs: `request_log` "not saved". Likewise `attack_alerts` (attack-alert rate limit) is "not saved": a restart re-opens both windows. Not player-triggerable, so Medium-Low, but any crash/deploy lets a spammer start over. (Cancelled 23 and 24.)
- Royalty WORKS: ledger after restart shows daily "royalty +1: 0.15% of all alleles are yours and live in other players' lineages" (my genes reached rivals via their offers/retire transfers).
- 07:59 tick 28130 my (non-reset) grace ended: spectate now places lineage 14 (33 cells). possess_attack on Herdmaster3's #62335 -> ok; the identical order again -> ok (alert suppressed by design; can't observe the victim's side). Alert: "Attack over: your #71451 killed #62335." Kill feedback works.
- 08:00 day 604: Genes tab "Only 27% of your animals' genes are yours" (API: wild 62%, mine 30%, Herdmaster3 8%). 896 alive but legacy share only 3.8%. My incubated-egg design is no longer sterile with live-bearers (Balance::egg_hybrid): compatibility at founding showed antelope 18% fertile / drift risk high, but no alert ever warned as the lineage went from 96% mine (day 169) to 27% mine. Playtest-2 #3 (silent dilution) PARTLY fixed: the info is in the Genes tab and Market card, but there is no notification. Medium.
- Era 1 ended day 360: I finished #2 with 2.9%.
- Leaderboard day 598: wild 59.7%, PocketNomad3 22.3% (1,132), Herdmaster3 11.7% (631), Rulebender3 3.7% (826), LateComer3 2.1%, GeneBroker3 0.3%, ApexHunter3 0.

## 08:01-08:09 (days 604-709) wrap-up
- Offered 2 studs to GeneBroker3 (offers 25, 26) and listed a stud (#14). 25 accepted then failed (died), 26 failed (died), listing closed (stud died), 22 failed (died). Every one of ~12 trades this session ended with a death before the animals met. In this world nothing walks the two together, and animals die fast, so trades almost never complete (Medium: trading still marginal; playtest-2 #5).
- Lineage peaked at 1,500 (day 648), 873 alive day 709; 4-year longevity bonus. Balance 3,267 renown unused (nothing worth buying: nameplate invisible, reinforcement blocked above 300, species naming impossible - no split species).
- FINAL day 709: wild 57.3%, PocketNomad3 29.3% (1,624), Herdmaster3 7.6%, Rulebender3 2.9% (869 alive; 3rd of players), LateComer3 1.8%, GeneBroker3 0.7%, ApexHunter3 0. Season titles: won seasons 1-3 legacy+lineage; era 1 #2 (2.9%). Stopped daemon and watcher.
