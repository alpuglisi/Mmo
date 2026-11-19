# GeneBroker12 playtest log (breeder/trader)

Started 2026-11-19 20:30 UTC (game day 7, year 0)

## 20:31 (day ~16) Registered + founded via web UI
- Registered GeneBroker12, design "Brokers Kin" (near-default grazer, fur .35, ears .4, vigilance .5): 35.9/100 pts, 30 founders. No console errors.
- Lineage tab: "Season weight ... rises to 100% over the 10 days after founding (day 15)" — "(day 15)" reads as end day but is the founding day? ambiguous. Low.
- Compatibility: Pocket Drifters (PocketNomad12) fully fertile; Frostwing Drift (Herdmaster12) 20% mean, "fertile_share 0.8" vs UI "70% of pairs can conceive" (UI 70% vs API 0.8 different?). Pocket row with fertility 1.0 and gap 0 still says `limiting: body_plan` (a limiter at full fertility). Low.
- Shop JSON `market_fee_percent: 10` with no hint that fixed listings are fee-free (the Market panel text does say "A fixed listing's sale pays no market fee"). Low.

## 20:35 (day ~30) Market
- Listed #996 (named "Bramble") as a stud, fixed fee 12, via the Market UI: toast "#996 listed for 12 renown." OK.
- offer_to_lineage → Pocket Drifters (#995 F, 8 renown) and → Frostwing Drift (#998 M, 5 renown): both return `apart: 96`. Same number for two different lineages: likely a cap/coarse value; check.
- Names (name_animal): REFUSED as expected: "Herdmatser" (swap), "Herd Nomad" (blend), "Pocket Master" (blend), "ApexHuntre" (swap) -> bad_name; species near-copies "Pocket Drifterz", "Pokcet Drifters", "Frostwing Drfit", "Gloamfang Pakc" -> name_taken. Ordinary "Bramble" PASSED (50 renown). P11 #7: FIXED for these cases.

## 20:37 (day ~69) restore_pasture per-group quotes
- Split into groups 0 (101) and 1 (100). Previews: group 0 → 201 renown / ~1280 food; group 1 → 211 / ~1255 food (feeds_others true). Then bought group 0 (no preview flag): charged 201 (group 0's quote), restored 1289 food. P11 #14 "one quote per player (preview A, preview B, buy A → charged B's)": FIXED.
- Food delivered (1289) ≥ quoted (1280) here; couldn't force a >20% drop within 30 s to see `quote_changed` live (code path exists in game/shop.rs). Not verified live.
- Restore price tracked my own living count (201→211 as births came) — I could not derive rival counts from it (sight filter appears to hold). P11 #8 restore leak: looks fixed (not exhaustively).
- Income ~6/day at 3.5% share; taper starts at balance 1000 (balance + escrow per game.rs:4274). Can't reach 1000 live at this income → taper-with-escrow not testable live; code counts escrow.

## 20:36–20:38 (days 74–97) First agreements
- Day 71 alert: "25 of ApexHunter12's hunters are within 30 tiles of your animals" — exact rival hunter count still given (grace-ending warning). P11 #8 (exact hunter counts): STILL PRESENT in this alert. Low/Medium (info leak).
- Offer #1 (my #995 → Pocket Drifters, 8 renown) accepted day 77: Pocket picked #1611 80 tiles away (partner at cell 92,28; mine 12,36). meet at 52,28 (halfway). meet_note: "predators are near every point between the two: the usual point is kept" (explains why). walk_warning: "#995 walks about 40 tiles alone to the meeting point; possess it to lead it past water and danger". P11 #10 meet_note + walk warning: present. (Warning mentions water/danger but not thirst/drinking explicitly.)
  - Pocket's partner #1611 was eaten by ApexHunter12's pack day 81 during the agreement (meet.asleep was true → consistent with "sleeping walkers aren't protected"). Offer failed, my 8 renown refunded with a clear alert. Only ONE mating_update before failure (no spam).
  - Concern (Medium): the accepter picked an animal 80 tiles away (LINEAGE_PICK_SLACK lets them pick up to 24 tiles farther than the nearest free one; maybe the nearest was already far). An 8-renown deal asks my animal to walk 40 tiles.
- Rulebender12 BOUGHT my "Bramble" (#996) stud listing (fee 12, offer #3, listing 1) at day 96. meet_note: "moved 12 tiles toward the partner to keep clear of a predator near the usual point" — explains AND says how far. walk_warning to buyer: "#2992 walks about 64 tiles alone". Buyer's animal is dormant (asleep:true) → deadline pausing (expires_tick 6076→6096→6120→6165 within seconds). Watching for: cap 30 days (≤1440 ticks), no update spam.
- Listing browse shows my listing `busy: true` once bought — fine.

## 20:39–20:41 (days 116–135) LISTING SALE COMPLETED
- Offer #3 (Rulebender12 bought my stud "Bramble" #996, 12 renown): buyer's #2992 slept from purchase (day 96) to ~day 116; deadline moved 6076 → 7008 = 932 ticks ≈ 19.4 days of pause (under the 30-day cap; cap itself not reached live). During the ~20 sleeping days the server sent ZERO mating_updates (only 3 at purchase while Bramble walked to the meeting cell). After waking, updates came only when a coarse cell / 4-tile distance changed (9 in 36 s while walking) and the client toasts only status changes (offerKey). P11 #3 spam: FIXED. P11 #3 open-ended pause: consistent with the fix (paused < 30 d).
- Completed day 135, conceived (fertility 0.35). Ledger: "Sale: 12 renown from Rulebender12 for listing #1" — +12, the full price: fixed listings fee-free CONFIRMED. P11 low "net shown as gross": fixed.
- Seller-side Mates card text was clear: "Lapses if they haven't met in 14 days (≈ 64 s). Walking to meet: the partner is about 32 tiles SE of yours. They meet near 28, 52 (at your animal)... Meeting point moved 12 tiles toward the partner to keep clear of a predator near the usual point." Good.
- Minor UI: an agreement bought FROM me appears under "My open offers" (it's the buyer's offer). Low.
- Stud listing stays open after a sale (listing 1 back to busy:false) — good for a stud farm; the Market tab "My listings" showed only the dam right after the sale (stud reappears later?). Low/check.
- Renown: income ~7/day at ~4% share; 540 by day 135 (no big unexplained jumps).

## 20:43 (day ~155) Names, round 2 — NEW BUG: reversed-word blends pass
- name_species (species 4): "Frostwing Drfit", "Pocket Drifter", "Gloamfang Pakc" → name_taken; "Herd Nomad" → bad_name. Good.
- BUT "Nomad Pocket" (PocketNomad12's name, words swapped) was ACCEPTED as my species name (300 renown spent; it can't be renamed: `already_named`). Animal names "Master Herd" (Herdmaster12), "Hunter Apex" (ApexHunter12), "Bender Rule" (Rulebender12) were all ACCEPTED (50 each) and showed on my dam's market listing. **Medium** (impersonation on the market / leaderboard; P11 #7 partly fixed: swaps, blends, species near-copies refused, word-order reversal not).
  Repro: name_animal {organism, name:"Master Herd"} while a player "Herdmaster12" exists → ok:true.
- Renamed the dam to "Clover" afterwards so the listing doesn't impersonate anyone; the species name is stuck as "Nomad Pocket" (sorry — test artefact).
- Order of checks: once named, name_species answers `already_named` before the name check (no free oracle there).
- `apart` on my pending lineage offers is live and coarse (96 → 64 as herds moved); 96 looks like a cap.
- Bramble (#996) survived the 2 days after mating (post-mating shield; not provable from my side, no attack seen).

## 20:42–20:46 (days 155–190) Second trade completed; auction
- Offer #4 (my #995 → Pocket Drifters, 8 renown; sent day 96 while #995 slept) accepted day 155 (after ~59 days pending), partner #5800 76 tiles away; meet at 52,28 halfway; meet_note "predators are near every point between the two: the usual point is kept"; walk_warning shown while ≥24 tiles and dropped below. Completed day 159, conceived (fertility 1.0), "Mating done: ... 8 renown paid from escrow." 11 mating_updates in 22 s, each a real coarse change. #995 alive and grazing afterwards (shield on the way back: no loss).
- Offer #2 to Herdmaster12 expired unanswered (125 days ≈ 10 min); refund alert OK. The 2-minute pre-expiry reminder came. Good.
- Stud "Bramble" #996 starved day 158 (drought, 23 days after its sale): "Listing #1 closed: #996 starved." OK.
- Created auction for gen-4 stud #17095, reserve 6, `days: 20` → deadline_tick − created_tick = 3000 ticks = 62.5 game days (48 ticks/day). P11 Low "auction days: 20 silently ~62": STILL PRESENT. Low.
- Standing day 189: I'm #1 player, 10.0% share, 576 alive; Herdmaster 6.4%, Pocket 5.7%.
- No other player has listed anything all game (supply side still empty except me). P11 #10 "no player ever listed": partly (I list; rivals don't).

## 20:47–20:50 (days 195–206)
- Sent 3 more lineage offers (6 renown each): #5 → Herdmaster (apart 96), #6 → Rulebender (apart 40), #7 → Pocket (apart 64). #5 failed at once-ish: my #1010 (a founder I picked by script) died of old age; refund fine.
- Market tab "My listings" cards render clean (the pregnant/old/asleep hint spans exist hidden in the DOM — fine). Auction card: "closes in 54.4 days (≈ 4 min)".
- Renown shop text still says "Sales pay out less a 10% market fee" — contradicts the fee-free fixed listings stated just above in the same tab. **Low** (wording, but misleading for sellers choosing fixed vs auction).
- restore_pasture preview, autumn day ~205: group 1 quoted **price 366 for `restored: 2` food, with no hint**; seconds later both groups quoted price 0 / restored 0 with hint "nothing would regrow there now (winter, a drought...)". P11 Low "restore preview quotes a price for 0 food with no reason": PARTLY fixed — at exactly 0 it's price 0 + hint, but a near-zero yield still gets the full price and no warning. **Low/Medium** (a player can pay 366 for 2 food). Repro: preview restore late autumn / drought on a nearly-bare circle.
- Lineage crashing 680 → 428 (hunger + ApexHunter pack inside my herd: 25 of their animals were in my view).

## (days 206–259) Winter crash; taper with escrow
- Lineage crashed over the winter: 576 (day 189) → 91 (day 232): starvation in late autumn + ApexHunter12's pack (25 animals living inside my herd) + hibernation. Group B alerts "fallen from 193 to 96", "96 to 44", "45 to 21" adults.
- All my listings died: auction stud #17095 starved day 203 ("Listing #3 closed: #17095 starved"), dam "Clover" #1482 eaten by ApexHunter12 day 236 (listed animals aren't shielded — as documented). Auction never got a bid.
- Offer #6 failed: my #995 eaten by ApexHunter12 (pending offer, unprotected: expected). #7 failed: #1001 died of old age (pending). Refunds fine.
- **Income taper counts escrow — VERIFIED live** (P11 #9 FIXED): balance crossed 1,000 at day ~250 → "Day 251 income: ... (4, reduced by the taper on a balance over 1,000)" = 3. Then parked 60 in escrow (offer #8, 60 renown → Pocket): balance 992–998 (< 1,000) + escrow 60 → still "(4, reduced by the taper on a balance over 1,000)". Wording nit: it says "a balance over 1,000" while the shown balance is 992 — should say "balance plus escrow". Low.
- Zero incoming mating requests all game (0 mating_request messages): nobody sent me a lineage offer, so the incoming-offer distance and the lineage-offer picker (lineage_candidates) could not be tested from the receiving side. From the sending side, pending lineage offers now carry `apart` (96/64/48/40) before acceptance — P11 "no apart without a named partner": fixed in the data.
- (day ~266) Spring: restore group 0 (200 → 1367 food; group 1 preview correctly hinted 'poor value now: about 1.1 renown per food'), reinforce (150, 10 adults), merged B into A. reinforce result still has float noise: "at": [18.90999984741211, 21.09000015258789] (P11 Low float noise: STILL PRESENT).

## 20:51–20:54 (days 261–295) Third trade
- Offer #8 (60 renown, my #9057 → Pocket Drifters) accepted day 261: partner 40 tiles away, meet at the coarse midpoint (36,28), no meet_note (not moved: correct, no note needed), no walk_warning (my walk ≈ 18 tiles < 24). Completed day 265, conceived (fertility 1.0). 10 updates in 18 s, all real changes.
- Relisted: stud #20721 (listing 4) and dam #19437 (listing 5), 10 each. New offers #9 → Rulebender (apart 56), #10 → Herdmaster (apart 96).
- Listing ids 1–5 are ALL mine: no other player has created a single listing this game, so "buy another player's listing" is impossible so far. (Supply side still empty — P11 #10 tail.)

## 20:55–20:57 (days 315–325) Pre-winter
- Recovered to ~399 (rank 3 by share: Herdmaster 11.4%, Pocket 5.7%, me 5.6%). Season 5: #3 legacy, #3 lineage, #1 of 2 survival.
- restore preview day 318: 788 (doubled) for 237 food with hint "poor value now: about 3.3 renown per food" — good hint.
- Migrate preview is informative (journey days/tiles, food share, "75% of the group is dormant ... the order may run out before then"). Cancelled: 75% of my herd was already hibernating in mid-autumn (day 324, winter starts ~330).

## 21:00–21:03 (days 360–395) Era 1 over; nameplates
- Era 1: I finished #2 (mean 4.8%, 7.4% at end); Herdmaster12 #1 (8.1% mean, 27.4% final). Day 382: Herdmaster 28.3% share (1,258 alive) vs me 6.7% — a runaway leader again (P11 #2 share run-away: looks STILL PRESENT; outside my focus).
- World tab lists my species publicly as "Nomad Pocket · herbivore · ~235 adults · GeneBroker12" — the reversed-name impersonation is visible to everyone.
- Nameplate titles: "Herdmatser", "Pocket Nomad", "LateCmoer", "Herd Nomad" → bad_title (good). But "Master Herd" (Herdmaster12) and "Comer Late" (LateComer12) ACCEPTED (500 + 100 renown). Same word-order-reversal hole as animal/species names. **Medium**. Then set "Stud Farmer" (ordinary, accepted).
- Renown piles up: 1,500 by day 395 with income tapered to ~4–5/day; nothing worth buying (reinforce blocked over 300 alive, restore doubled to 788 for poor value). P11 Low "renown still piles up": still present.
- LateComer12 joined around day 380.

## 21:04–21:10 (days 420–470) Offers keep dying; UI offer; code read of the incoming card
- Season 7: #2 of 6 legacy, #2 lineage. Offers #9 (starved), #11 and #13 (eaten by ApexHunter12 while pending), #12 (starved) all failed before any answer — the Gloamfang pack (90 animals per survey) lives next to my herd and keeps eating my offered animals. Pending offers aren't shielded (by design), but as a breeder ~half my offers die before the owner even answers. Medium (market frustration).
- Survey (36 renown): only Rulebender (20) and ApexHunter (90) within 64 tiles; Pocket — my only fully fertile partner — is > 64 tiles away, so every Pocket deal is a 30–40 tile walk.
- Sent offer #14 through the Market UI ("Offer a mating" on the Pocket row, price 15): toast "Offered #33751 to PocketNomad12's Pocket Drifters for 15 renown: they pick the partner." The Mates card for my OUTGOING lineage offer shows no distance (server sends `apart` 64 but the client only prints it on incoming cards). Low.
- Code read (client/src/ui/game.ts ~1924), incoming offers: the distance line always says "About N tiles apart: they meet three quarters of the way to your animal, which walks the last quarter." For an offer to a lineage the server meets HALFWAY (MEET_HALFWAY = 0.5; observed live: offers #1, #4, #8 met at the midpoint), so the text is wrong for lineage offers. **Low/Medium** (misleads the accepter about whose animal walks).
- Code read, lineage-offer picker: lists only `lineage_candidates` ids once the reply arrives, but before the first reply or if that call fails (`ids = null`) it falls back to every non-pregnant adult. Small window; Low. Could not see it live (no incoming offers all game). `lineage_candidates` on my own outgoing offer → `not_a_lineage_offer` (fine).
- Still no listing by any other player (listing ids 1–6 all mine).

## 21:10–21:20 (days 480–600) Late game
- PocketNomad12 retired (day 505): my pending offer #14 ended with a clear reason "PocketNomad12 retired their lineage, so the offer ended" + refund. Good. Pocket re-founded "Pocket Drifters Again" (fully fertile with me).
- My lineage fell to 151 (1.9%, rank 4) through the second winter: listed stud #20721 "died (winter sleep)", stud #40230 eaten by ApexHunter12, offer #15's #52892 "died (winter sleep) before the mating". Hibernation deaths + a resident rival pack.
- Reinforced again (10 adults; float noise in "at" again: 14.829999923706055). All 10 reinforcements arrived DORMANT (it was winter) — fine, but they can't be used for trades until spring.
- Offer #16 from a sleeping female (#60069) to Pocket Drifters Again (apart 8): unanswered after 4+ min, so the 30-day pause CAP could not be observed hitting its limit live (offer #3 paused 19.4 days < cap).
- Restore preview day ~547: 200 for 254 food (0.79/food), no hint; earlier hint threshold was ~1.1/food. Fine.

## 21:20–21:26 (days 633–680) End
- Drought collapse: 198 → 29 between days 640 and 651 (thirst + hunger; I didn't move the herd). Crash alerts within 6 days quoted three different baselines: "112 → 30 over the last 10 days (73% fewer)", "65 → 30 (54% fewer)", "still crashing: 155 → 32 over the last 20 days (79%; 9% in the last 10)". Hard to read which number is the real loss. Low.
- Offer #16 (sleeping female → Pocket Drifters Again) never answered by the end. No rival listing ever appeared (listing ids 1–7 all mine); 0 incoming mating requests all game.
- FINAL (day ~680, 21:25): GeneBroker12 29 alive, 0.6% share — 6th of 7 by share at the very end (was #1 player at day 59–135 and 189, #2 era 1). Renown 1,464 unspent. Herdmaster12 20.9% (1,677), LateComer12 7.4%.
- Trades: 4 completed cross-player matings — sold my stud listing to Rulebender12 (12, fee-free), and 3 offers to Pocket Drifters (8, 8→conceived, 60→conceived; first one failed when their partner was eaten). Could NOT buy another player's listing: nobody else ever listed.
