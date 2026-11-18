# Rulebender — playtest 2 log

Persona: competitive rules-prober (API + normal client). Server http://127.0.0.1:8091. Player id 2.
Tools: a Node WS daemon (scratchpad/playtest2/rb/daemon.mjs) on protocol 1 JSON + curl for HTTP.

## Timeline
- 23:21 Registered "Rulebender" (player 2). World was at day 18, only wild (1068 animals), nobody founded yet.
- 23:21 Founded "Loophole Boars" (lineage 5) at day ~31 (summer, drought 0.36): warm, live, litter 3, omnivore (carnivory .4), body .65, fur/fat .6, hib .5, swim .3, vigilance .4, claws/fangs .3, pack .85, 80 founders. Cost check: 80 founders + traits = ~94 pts (trait cost 19.5; founder_cost(80)=74.6). 200 founders was refused (938 pts) — founder price curve works.
- Founders landed around (82,10), NE edge. 79 alive at day 33; many pregnant.
- NOTE on pacing: I lost ~30 min of wall clock between founding and the first probe (day 33 -> day 401). The lineage ran unattended: won seasons 1-2 (legacy+lineage), then fell to #3, #5 as GeneBroker2 grew; era 1 finished #4 with 2.3%. At 23:52: 94 alive, 1.6% share, last of 6 players. Balance 1768 renown.
- From ~day 247 on: a stream of `attack` alerts "A Loophole Boars of yours (#...) is being hunted by a player's Apex Raptors (#...)" (ApexHunter2), ~1 every 2 game days.

## Probe results (23:52 - )
### Look-alike names — FIXED
- Registering `RuleBender` (case only vs my own name) -> 409 name_taken. `Ru1eBender` (1/l) -> 409 name_taken. Login as `RULEBENDER` finds my account (player 2). No extra account was created (I logged that extra login session out at once).
- Trying the same against another player's name (P0cketnomad2) was blocked by my own permission guard, so I only tested against my own name. Code: accounts.rs `name_skeleton` folds case, 0/o, 1/i/l, _/-, rn/m, vv/w.
### Malformed commands — FIXED (minor gaps)
- `{"type":"possess","id":901}` (missing field), wrong field type, unknown type, missing type, NaN string, negative amount: all answer `result` with the `id`, `command` named after the type (or `message` when unknown), code bad_message, generic friendly text, no parser text or command list.
- Gaps (low): a non-numeric id (`"id":"strid"`) gets a result with no id; a message that is not valid JSON at all (e.g. a number literal 1e309, which serde refuses) loses the id too. The web client always uses numeric ids, so only API users notice.
### /api/map.png — FIXED (with a caveat)
- No token -> 401; `?token=` query -> 401 (only Bearer). With Bearer: terrain + my animals + others near mine.
- Caveat: "near" is a 65x65-tile SQUARE around each of my animals (max_view_radius 32, Chebyshev). On a 128x128 map a spread-out lineage (mine spans x 14..124) sees most of the world.
### set_view — PARTIALLY FIXED (range is very wide)
- Far point (20,110) with no animals near -> out_of_sight. Good.
- But the view centre may be up to 32 tiles (square) from any of my animals AND the window radius is clamped to 32, so the window reaches ~64 tiles (90 diagonally) from my nearest animal. Measured: nearby organisms up to 43 tiles from my nearest animal (list capped at 256). Scanning set_view over cells where I have animals (~15 calls) listed 1,000+ rival animals with exact positions, sex, maturity: 345 of GeneBroker2's ~1,092, all of Herdmaster2's and ApexHunter2's seen, PocketNomad2's and LateComer's. Severity: medium (scouting is effectively free; perception is 8 tiles).
### Listings — positions FIXED, genes still exact
- browse_market: the only listing (GeneBroker2 stud #27035) shows x=104,y=40 = a 16-tile cell centre. Still 14 exact trait values per listing (by design for a market?).
- preview_mating gives the exact distance (60.76) to that stud — fine because it was within my (wide) sight; the 16-tile rounding only applies to unseen partners.
### already_pregnant — FIXED (residual oracle, low)
- request_mating to rivals' pregnant females -> `partner_unavailable` "#52841 can't mate now" (generic). But preview_mating for the same pair said compatible, no problem, so "preview OK + request refused" is still a pregnancy oracle for a mature rival female (only reason left besides being held). Low.
### Mating-request spam — FIXED
- Male #34899 -> ApexHunter2 females: requests 1-3 accepted as pending (offers 11, 12, 13), 4th and 5th -> `too_many_requests` "you have sent ApexHunter2 3 requests recently" (ticks_left 3512 = ~6 min wall clock at 100 ms ticks). Cancelling one (offer 11) does NOT free a slot in the window (still too_many_requests): good.
- Refused requests (partner_unavailable) do not count against the window.
- Pending requests cost no slot (slots_left stayed 6), so one player can still hold 3 pending per rival x 5 rivals = 15 notifications per ~6 min. Acceptable now that block/auto-decline exist.
### Pending offers vs offline protection — FIXED (code + behaviour)
- offline.rs only exempts organisms in an `Accepted` offer (plus listed/bidding organisms, which are the owner's own choice). Test `organisms_in_an_agreement_are_exempt` covers it.
### Species-founded farming — PARTIALLY FIXED
- My own lineage did it without trying: Loophole Boars hybridised with wild Red fox and was re-detected as a new species 4 times (species 19 day 230, 21 day 290, 22 day 330, 24 day 340; each earlier one "merged into" Red fox 3).
- Renown: only ONE +100 "Species founded" bonus (day 230); days 290/330/340 paid nothing. Season title: only season 4 lists me as species winner; seasons 5 and 6 (which had the later re-detections) have no species winner. So the renown/season farm is closed.
- Still open: /api/achievements shows `species_founded: 4` for me and /api/species `founded` count 4; achievement alerts "Species founded: Loophole Boars (3/4/5) split off as a species of its own" still fire for each re-detection. Public counters are inflated (low).
### Attack orders (23:55-23:58)
- possess + `possess_attack` on a GeneBroker2 animal: accepted. Own animal -> `own_target` with a clear sentence. A target 10 tiles off -> `target_out_of_sight` "get closer first". A 22 kg Broker vole -> `too_big` "(22 kg) is too big to take on; this one can tackle up to 4.4 kg (more with pack-mates around)" with detail. A non-possessed attacker -> `not_possessed`. Good messages.
- BUG (low): an attack order was accepted for a DORMANT (hibernating) attacker (#37324, winter); the order then silently vanished (state `order` None, action stays dormant). No feedback to the player.
- Attack order silently gives way to hunger: my founder's order disappeared within one state push (hunger 69 -> 100) and it went back to grazing; nothing tells the player why the order ended (no result/alert). Low/UX.
- GRIEFING (low-medium): every `possess_attack` call sends the target's owner an `attack` alert (toasted by default), with no de-duplication: re-issuing the identical order (same attacker, same target) twice returned ok twice -> two alerts. A griefer can toast-spam a rival at the command rate limit, even with orders that never connect. game.rs `alert_attacked` is called on every accepted order. Suggest: one alert per (attacker,target) per N minutes, or only when the strike lands.
- On the receiving end, ApexHunter2 sent me ~25 attack alerts over ~70 game days; they're informative but one per order is a lot.
### Newcomer grace is one-sided (new, medium)
- After re-founding (day 471, so my new lineage is under the 20-day grace), my founder #65331 got `possess_attack` accepted against GeneBroker2's animals #51306 and #61385. So a lineage under grace can hunt established players' animals while their animals are forbidden to hunt it (`grace_spares`) and their orders against it are refused. Combined with retire -> re-found (which resets the grace: grace follows the lineage's founding tick), a predator player can repeatedly be immune for 20 days. Costly (losing the lineage), so medium-low, but the asymmetry is surely unintended: suggest the grace also stops the newcomer's own orders/hunts against players.
### Retire + share ramp (23:57) — FIXED
- Season 8 was 12 days from its end when I retired lineage 5 (`retire_lineage` -> removed 121, no deaths/alerts) and re-founded at once with 85 founders (16 pregnant). Founding allowed immediately.
- Season 8 legacy score: 0.0229 just before -> 0.0201 at the end (it fell; no spike). Era/season mean-of-days + 10-day ramp + weighted half work.
- Raw daily share did not spike either, because the winter founding starved: 85 founders -> 26 alive in 11 days (drowned, thirst, starved; hunger 65 / thirst 58 average). Playtest-1 finding 4 (large foundings starve; founders land on poor ground) still holds in winter.
- Oddity (low): Season 8 result still put me "#1 of 6 in survival" although I retired the lineage that had survived the season; retiring doesn't count as an extinction for the survival category. Every player is tied #1 in survival so the category says little.
### Reinforcements (renown shop) after a re-found
- Right after re-founding, with 2,101 renown banked from the old lineage, I bought 3 reinforcements (150+300+600 renown) = 30 copied adults at hunger ~26 (fresher than my starving founders; one arrived pregnant). Fourth refused (1200 needed). This is the intended catch-up sink, but note it makes retire->re-found stronger: renown earned by the old lineage buys a third of a new founding at once. Price doubles within a ranked season; a new season (season 9 had just started) resets it.
### Market: buying a rival's dam without bringing a male — STILL OPEN in practice (medium)
- No rival dam was listed all session (the only other listing was one GeneBroker2 stud, sterile with me), so I couldn't run it live. From the code and the server's clock:
  - `purchase_deadline_ticks` = 30 min of wall clock (main.rs scales it by tick_ms: 18,000 ticks here).
  - But `after_tick` expires EVERY open offer at each weather-season change (30 game days = 1,440 ticks: 24 min at the default 1 s tick, 2.4 min on this 100 ms server), with a full refund via `settle`.
  - So the 30-minute lapse can never fire before the season change does, at either speed: a bought dam is still held (`hold_consented_females`) until the weather season ends, and the buyer pays nothing for it (season expiry refunds the whole escrow; the 10% lapse fee and the slot forfeit never apply). That is exactly playtest 1's complaint. Fix: make the deadline shorter than a weather season in ticks (e.g. min(30 min, a few game days)), or charge the lapse fee on season expiry of an un-met purchase too.
- Market validation held: fee 5,000,000 -> bad_price (cap 1,000,000); expires_days 0 -> bad_expiry; negative -> bad_message with id; auction ticks 0 or ticks+days -> bad_duration; second listing of the same animal -> already_listed; bid on own auction -> own_listing. A 1-tick auction (0.1 s) is still allowed (ticks 1..1440): legal but meaningless (playtest-1 #16 "auctions close in seconds").
- Listed my own dam (#22, fee 15) and a stud auction (#23, reserve 5, 10 days) to invite trades.
### Per-target request window is not scaled to the tick (low)
- `request_window_ticks` is a fixed 3,600 ticks ("an hour" at 1 s ticks) and main.rs doesn't rescale it like the purchase deadline and offline protection: on this 100 ms server the window is 6 minutes (I saw ticks_left 3512). Harmless here; inconsistent with the README's "an hour".
### Spawn camping vs grace — FIXED as far as I could see
- My re-founded lineage (day 471) lost 65 founders in its first 20 days, ALL to starvation/thirst/drowning (winter founding), none eaten, though ApexHunter2's raptors had been hunting my previous lineage every 2-3 days. No `attack` alerts against the new lineage during the grace.
### More info-leak checks (00:05)
- Own pregnant female -> `already_pregnant` (own) as specified; rival -> generic `partner_unavailable`. OK.
- preview_mating on a rival animal out of sight: distance rounded up to a multiple of 16 (64, 80, 96 seen). In sight (within the 32-tile square) it's exact, e.g. 54.66 tiles. OK as designed.
- preview_mating still works on ANY organism id on the map: `not_found` vs a reply is a liveness oracle for any id, and `body_plan_gaps` gives exact carnivory/body-size gaps (so the rival's exact genes, given my own) for animals I can't see. Low (listings show exact genes anyway).
- `/api/spectate` (no login) lists every player lineage's members per 16-tile cell, refreshed every game day. Coarse, but enough to find a newcomer's founding cell for camping without logging in or having sight. Low.
### mating_prefs (00:04)
- Read, block 4, unblock 4 + auto_decline true, auto_decline false all work and echo the state. Blocking myself is silently ignored (fine). Blocking a non-existent player (999) or 0 (the wild) is ACCEPTED and stored (low: no validation; MAX_BLOCKED 200 caps it). I unblocked them again.
- The refusal text "X isn't taking mating requests from you" tells a requester they're blocked (acceptable). Could not test receiving: nobody sent me a request all session.
- Client errors.ts has friendly text for too_many_requests, requests_refused, partner_unavailable, out_of_sight, bad_message. Missing interface labels for retire_lineage, renown_shop, buy_mating_slot, reinforce, compatibility (they toast as "Command: ..."). Low.
### bad_message command names incomplete (low, new)
- `{"type":"retire_lineage","lineage":-1,"id":N}` -> bad_message with the id, but `command: "message"` instead of "retire_lineage". protocol.rs `COMMAND_NAMES` lacks `retire_lineage`, `renown_shop`, `buy_mating_slot`, `reinforce` and `compatibility` (all newer commands), so malformed ones of those toast as "Command: ..." in the client. Nothing checks COMMAND_NAMES against `ClientMsg::name`.
- retire_lineage with a rival's lineage (8) or my dead one (5) -> `no_lineage` "lineage 8 is not your living lineage (yours is 15)". OK.
### Attack orders, second round (00:04)
- Herdmaster2's lineage 16 (not a newcomer): order on an 18 kg adult -> too_big (limit 9.7 kg), on a juvenile (18 kg?!) -> too_big (limit 15 kg), on another juvenile -> accepted; my hunter got within 10 tiles, the target outran it to 16.6 tiles and the order ended silently (as documented: >12 tiles ends it). A juvenile reported as 18 kg, heavier than an adult of the same lineage (17.7 kg), looks odd.
- `target_out_of_sight` came back for a target 3.7 tiles away by state positions (positions were ~1 s old; could also be the vision cone / directional vision). The text says "get closer first", which is confusing when it is already close. Low.
- (00:07) Species re-detection #5: "Loophole Boars (6)" split on day 460 (first in game year 3), alert fired, again no renown (whole session: exactly one "+100 renown ... (Species founded)" alert in my event log, five "Species founded" achievement alerts). /api/achievements now `species_founded: 5`, /api/species `founded` count 5. The re-founded lineage got its own "founding" species (27) without any bonus. So: renown and season title closed; public counter and alerts still farmable.
- (00:06) My re-founded herd lost 67 -> 40 in 16 days in the territory I claimed (autumn drought; thirst and starvation). Migrated to (55,47) near a lake.
### Public design leak (low / design question)
- `GET /api/leaderboard/history?genes=true` (no login) gives every player lineage's daily mean of every gene (birth_mode, endothermy, carnivory, body_size, all traits) plus deaths by cause. That's a complete recipe for copying a rival's design (PocketNomad's playtest-1 "copy the leader's body plan" trick) without any sight of their animals. Documented in the README, but it undercuts the set_view/listing hardening; consider limiting genes to the caller's own lineage.
### Migration into a flooding floodplain (playtest-1 #8, observed again, medium)
- Day 552 I ordered migrate to (55,47), which was dry by /api/terrain/dynamic when I chose it (water <= 5 cm). By day 570 the autumn rise put 38 cm on the target and 9-23 cm along the route's last third; the herd went 40 -> 14 in 20 days with 8 drowned (rest thirst/starvation). The order was still standing ("migration": [55,47]) with the target under water: nothing re-checks or warns once water rises after the order was accepted. Re-targeted to dry ground at (48,58).
- Possessed animals that get hungry now forage on their own with a `need` alert ("Possessed #69036 is parched (thirst 90): it is foraging on its own until you give it an order") — the playtest-1 "possessed animals stand until they starve" part looks fixed.
- (00:06) Reinforcement price reset with ranked season 10 (day 540): 150 again, then 300, 600 -> 30 more adults for 1,050 renown. At this server's speed a ranked season is 5 minutes, so the "doubling per ranked season" cap resets often; at 1 s ticks it'd be 48 min. Renown income for a tiny lineage is ~3/day, so this is mostly spending the old lineage's bank.
- (00:08) Evidence for the season-expiry point: my pending offers 12 and 13 (created ticks 21022/21093) were closed `expired` at tick ~21600, the weather-season change, about 50 s of wall clock after I sent them. A season here is 1,440 ticks; the purchase deadline is 18,000 ticks.
- (00:08) The 30 reinforcements I bought on day 580 were down to 13 animals total (lineage) by day 593: starved/drowned on late-autumn ground. Reinforcements are placed next to an existing (starving) member, so they inherit its bad spot.
- (00:12) Leaderboard day 622: wild 65.6%, LateComer 26.8% (1,966 alive), GeneBroker2 4.0% (99 alive, down from 1,286 at day 566), Herdmaster2 1.6%, ApexHunter2 1.2%, PocketNomad2 0.4%, Rulebender 0.31% (19 alive). Era 0 final: I was #4 of players with 2.29%.
- (00:20) No rival ever listed a dam (market watcher 00:03-00:19 saw only my own listing #24), so the 30-min deadline could not be exercised live; verdict rests on code + timing above.
- (00:20) FINAL: day 744. Loophole Boars II died out over the winter (0 alive; 19 at day 626). Leaderboard: wild 54.1%, LateComer 34.0% (2,650), GeneBroker2 7.1%, Herdmaster2 4.4%, ApexHunter2 0.3%, PocketNomad2 0.03%, Rulebender 0.0%. Season 13 legacy: last. Earlier: won seasons 1-2 (legacy + lineage), era 0 #4 of players (2.29%). Stopped my WS daemon and market watcher.

## Verification table (playtest-1 exploits)
| Item | Verdict | Evidence |
| --- | --- | --- |
| Spawn camping vs 20-day grace | Fixed (one-sided) | Re-founded lineage lost 0 to predation in its 20 days while raptors had hunted my previous one every 2-3 days; but a lineage under grace can itself issue attack orders on established players |
| Mating-request spam | Fixed | 4th request to one player -> too_many_requests; cancel doesn't free the window; block/auto_decline work (unvalidated ids accepted) |
| Pending offers strip offline protection | Fixed | offline.rs exempts only Accepted offers |
| Buy dam, never bring male | Still open in practice | 30-min deadline (18,000 ticks) > weather season (1,440 ticks here, 24 min at 1 s ticks); season expiry frees her first with a full refund, so the lapse fee never applies |
| Species-founded farming | Partially | Only one +100 over 5 re-detections; no extra season titles; but /api/achievements and /api/species counts (5) and alerts still inflate |
| Forced-extinction share spike | Fixed | retire_lineage works; re-founding 12 days before season end lowered my season score (0.0229 -> 0.0201) |
| /api/map.png | Fixed (wide) | 401 without Bearer; shows a 65x65 square around each own animal |
| set_view range | Partially | Centre within 32 tiles (square) + radius 32 -> saw rivals 43+ tiles out; ~15 calls listed 1,000+ rival animals |
| Listing positions | Fixed | 16-tile cell centres; genes still exact |
| already_pregnant | Fixed | rival -> partner_unavailable; preview OK + request refused remains a weak oracle |
| Look-alike names | Fixed | RuleBender / Ru1eBender -> name_taken; case-insensitive login |
| Malformed commands | Fixed (gaps) | id + friendly text; COMMAND_NAMES lacks 5 newer commands; non-numeric id lost |
