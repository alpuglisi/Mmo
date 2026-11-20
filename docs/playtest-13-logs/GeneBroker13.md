# GeneBroker13 playtest log (breeder/trader)

Started 2026-11-20 00:09 UTC (game day ~10, year 0). Server http://127.0.0.1:8107.

## 00:10 (day ~20) Registered + founded via web UI
- Registered GeneBroker13 in the UI; founded "Broker Clover" with the default starter design (37/100 pts, 30 founders). No page/console errors, no failed requests.
- Landing toast: 0 `.toast` elements seen during the 6 s after the map appeared (p12 #12 "landing toast shown 0 times" — looks STILL PRESENT from my run; I only sampled once at +6 s, so it may have flashed earlier). Low.
- Compatibility: Pocket Wanderers (PocketNomad13) fully fertile (shared starter); Glintwing Frost (Rulebender13) mean 0.1, 70% of pairs; `limiting: body_plan`.
- Shop JSON now has `listing_fee_percent: 0` next to `market_fee_percent: 10`. Ledger totals now include `escrow_paid` (p12 Low: escrow payout line — data field present; to verify live after a payout).

## 00:12 (day ~45) First listings + offers
- Mates tab → My adult picker → **"List it for 10"**: a confirm dialog "List #1199 on the market for 10 renown? Anyone may buy one mating with it; a listing's sale pays no market fee…" then toast "#1199 listed for 10 renown (no market fee on a listing's sale)." Works, one tap + confirm. Screenshots GeneBroker13-0012-listit-*.png.
- Listed dam #1212 for 10 via API (listing 2).
- browse_market: listing positions are given as x:16,y:16 for an animal at ~(27,15) → coarse 32-tile region centre. p12 Low (listing streams seller animal's position): consistent with FIXED (to re-check on a purchase/agreement view).
- offer_to_lineage → Pocket Wanderers (#1207 M, 8): `apart: 96`, `mine_age {74.1 of 263}` → **offers show ages: FIXED**. → Glintwing Frost (#1226 F, 5): apart 32.
- Founder #1224 died of thirst day 45 (was 91% health, thirst 46 a minute earlier; old-ish founder).

## 00:13 (days 53–62) Unfit refusals; two trades completed
- **Unfit refusal VERIFIED (hunger)**: offer_to_lineage with #1205 (hunger 100) → `unfit` "#1205 can't be offered now: it is starving or parched: let it eat and drink first". Same for #1216. Clear reason.
- restore_pasture preview: 200 renown for 243 food at (28,15), no hint (0.82/food). Couldn't afford yet.
- Provision 40 → 20 adults fed. OK.
- Offer #2 (my #1226 F → Rulebender13's Glintwing, 5) accepted day 55, partner #1019 16 tiles away, met at (28,36) on my side, **completed day 55, conceived at fertility 0.15** (a hybrid!).
- Offer #1 (my #1207 M → PocketNomad13, 8) accepted day 56: partner 68 tiles away, meet (68,20) = midpoint (halfway, correct), meet_note "predators are near every point between the two: the usual point is kept" (no placement given: p12 Low meet_note placement → FIXED, though it still tells me predators exist somewhere out of sight), walk_warning "#1207 walks about 28 tiles alone…". I possessed it and ordered it to the meeting point. Completed day 62, conceived (fertility 1.0). 19 mating_updates total in ~40 s across two offers (all real coarse changes).
- **Ledger escrow payout line VERIFIED**: "Escrow paid out: 8 renown to PocketNomad13 for offer #1 (the mating ran)" (delta 0, shows as its own line); totals now {escrow:13, escrow_paid:13} (p12 "totals.escrow stuck": FIXED).
- Mates tab: heading **"Open offers and agreements"** (FIXED); outgoing lineage offers show "About 96 tiles from their nearest suitable animal." (**outgoing distance FIXED**).
- Shop text: "Fixed listings' sales pay no market fee; offers and auctions pay out less a 10% market fee" (**FIXED**). Market listings show "somewhere around 16, 16" (32-tile region).
- Autumn day 66: 37 of 64 adults already dormant; dying of thirst (14 thirst + 7 hunger in 10 days).

## 00:14–00:17 (days 69–85) A listing sale agreed; asleep refusal; names
- **Rulebender13 BOUGHT my dam listing #2 (#1212, 10 renown)** day 69 — alert "Rulebender13 bought a mating with your dam #1212 for 10 renown (held in escrow until #1007 meets it)." Agreement positions: mine (16,16), theirs (16,48), meet (16,48) — all 32-tile region centres (**listing deals give positions only as 32-tile regions: VERIFIED**). meet_note "moved to keep clear of a predator near the usual point" — **no distance/placement (VERIFIED)**. My dam was already dormant ("One of them is asleep: the meeting resumes when it wakes (the deadline waits)") — clear Mates-tab text.
  - Observation: create_listing accepts a dormant/starving animal (no fitness check on listings; the listing shows `dormant: true` to buyers so it's informed). buy_listing also doesn't run the unfit check on the BUYER's animal (code read: check_fit_to_offer only in request_mating/offer_to_lineage). To try live.
- **Asleep refusal VERIFIED**: offer_to_lineage with dormant #1203/#1204 → `unfit` "…it is asleep (winter or summer sleep) and won't walk to a meeting until it wakes".
- Day ~77: no awake, fit, non-pregnant adult at all (37/64 dormant in early autumn; the rest hungry/thirsty ≥70) → no offers possible for a while.
- **Names** (name_animal on #1212):
  - REFUSED (good): "Master Herd", "Bender Rule", "Nomad Pocket", "Hunter Apex", "Herd Master", "Rule Bender", "Herd-Master", "Master Herds", "Herdmaster 13", "Thirteen Herdmaster" (bad_name); two-swap "Hedrmatser", "Rluebendre", "PcoketNomda" (bad_name); blends "Glintwing Wanderers", "Pocket Frost", "Ember Pack", "Duskmaw Flock", "Wanderers Pocket", "Frost Glintwing", "Apex Pack", "Wanderers of Pocket", "Frostglint Wing" (name_taken). Ordinary "Bramble" PASSED.
  - **NEW holes (Medium)**: "Masterherd" (Herdmaster12-style reversal run together, one word), "The Master Herd" (reversal + leading article) and "Apex the Hunter" (ApexHunter13 with a filler word) were all ACCEPTED (50 renown each) and would show on my dam's market listing. Repro: name_animal {organism, name:"Masterherd"} with player Herdmaster13 present → ok. Renamed the dam "Clover" afterwards.

## 00:16–00:19 (days 88–123) Second listing sale; first rival listing; I buy it
- Rulebender13 also bought my stud listing #1 (#1199, 10) day 88 (both my animals asleep at purchase; deadlines wait).
- **Rulebender13 listed dam #1010 for 15 (listing 3)** — the first non-me listing seen in two playtests. Browse shows it at "16, 48" (region).
- preview_mating my #1207 × #1010: fertility 0.3, `distance: 96`.
- **I BOUGHT listing 3 with my #1207 while #1207 was DORMANT** (winter sleep, possessed and stranded at ~(70,20) after the earlier Pocket deal): accepted at once, escrow 15, walk_warning "#1207 walks about 56 tiles alone…", meet_note "predators are near every point…". **Inconsistency (Low/Medium)**: offer_to_lineage/request_mating refuse an asleep (or starving/old) animal as `unfit`, but buy_listing takes the same animal as the buyer's half with no check — the very "offered animal dies/sleeps before the meeting" problem p12 #8 was about, now through the listing route. Repro: buy_listing {listing, mine: <dormant adult>} → ok:true. (create_listing also accepts dormant/starving animals; buyers at least see `dormant: true`.)
- Offer #3 (Rulebender bought my dam #1212): dam woke around day 120, now 16 tiles from the partner.
- Possession note: #1207 stayed possessed after the Pocket mating completed and then slept where it stood, ~45 tiles from my herd; nothing told me it was still possessed and far away. Low UX.

## 00:19–00:20 (days 125–142) Two listing sales PAID in full; restore matches quote
- Offer #3 (Rulebender bought dam #1212) completed day 125, conceived (0.31). Offer #4 (Rulebender bought stud #1199) completed day 127, conceived (0.10). Ledger: "Sale: 10 renown from Rulebender13 for listing #2 (your #1212 with #1007)" and "…listing #1 (your #1199 with #1006)" — +10 each, **fee-free full price VERIFIED**. totals.sales 20.
- Dam #1212 died of thirst 6 days after its sale ("Listing #2 closed: #1212 died (thirst)").
- **restore_pasture food VERIFIED**: preview 200 renown / 1,504 food → bought 2 s later: 200 / **1,561 food** (≥ quote; p12 #9 "14.5% less than quoted": FIXED).
- 10 s after that full restore, a new preview offered 471 more food in the same circle (price 400). Odd that a just-restored circle has 471 food of headroom two game days later (grazed that fast, or restore isn't to full?). Low / question.
- Buying the second restore with 95 renown → `insufficient_funds "400 renown needed, you have 95"` (clear).
- ranked-season rollover reset provisions_bought to 0 (fine).

## 00:21 (days 150–156) Spring offers; duplicate pending offers allowed
- (My mistake: a stale second copy of my bot replayed my commands.) Result worth noting: **the server accepted the SAME animal in two pending offers to the same lineage** (#1703 → Pocket in offers #6 and #9; #1864 → Rulebender in #8 and #10), escrowing the price twice. Pending offers hold nothing, so a double-click/replay makes duplicate offers and the owner sees two identical cards; only one can ever run. Low/Medium. Repro: send offer_to_lineage {mine:X, lineage:L} twice → two `pending` offers. I cancelled #9/#10 (refunds immediate, clear alerts).
- #1791 refused `unfit` (starving) — consistent.
- Offer #6 (#1703 → Pocket) accepted within ~15 s: partner 68 tiles away; meet (28,12) right at my animal's cell rather than halfway; meet_note "moved to keep clear of a predator near the usual point" (no distance stated — the note no longer says how far; for the seller this hides that the point moved ~30 tiles onto my side. Neutral).
- Anti-spam: 3rd offer to the same player in a short time → `too_many_requests` ("try again later", ticks_left 35998 ≈ 750 days?!). ticks_left 35,998 at 48 ticks/day = 750 game days ≈ 62 real minutes: that's effectively the rest of the playtest. Medium? (Check: maybe the window counts from the first request.)
- Code check of the limit: REQUESTS_PER_TARGET = 3 per REQUEST_WINDOW_MINUTES = 60 real minutes, counting every request sent (cancelled and completed ones too). **In a ~60-minute session a breeder can make only 3 offers to each rival in total** — with only 2–3 fertile rival lineages that is ~6–9 offers per hour, and a cancelled or failed one still uses a slot. I'm already capped with PocketNomad13 and Rulebender13 at 00:21 (25 min in). **Medium** (throttles the core breeder loop; the error says only "try again later" — the ~60-minute wait is only in `detail.ticks_left` in ticks).
- 00:22 Listed stud #1730 via the Mates tab "List it for 10" (confirm dialog + toast, works again).

## 00:22 (day 168) NEW: the "old" gate is mis-calibrated — offered animal died of old age the same second
- offer_to_lineage #1221 (male, `mine_age {175.8 of 275}`, no `aging` flag) → Herdmaster13's Emberdown Flock (lineage 7): accepted as fit, and **failed the same second: "organism 1221 died of old age before the mating"** (refund OK).
- Why (code read): check_fit_to_offer refuses only at ≥ 90% of `lifespan_days`, but with Balance::age_spread = 0.12 the old-age hazard (apply.rs old_age_hazard, Rayleigh) **starts at ~62% of the lifespan and the mean age at death is 85%** — so most animals die of old age *before* the 90% gate ever triggers, and the client's "(old: it may die before they meet)" (`aging`) didn't show either. #1221 was at 64%. **Medium** (p12 #8 "old" part only PARTLY fixed: the gate exists and gives a reason, but at a threshold most animals never reach). Suggest gating/flagging from the hazard onset (~0.62) or warning from ~70%.
- Lineage ids: Herdmaster13's Emberdown Flock is lineage 7 (my offer to "3" was the wild fox: `unknown_lineage`, fine). Offer #11 (#1732 F → Emberdown, 6) pending, apart 96.

## 00:21–00:23 (days 159–176) Three more trades done; my purchase lapsed
- **My purchase of Rulebender13's listing #3 LAPSED** day 159: "#1207 never came to #1010 within the deadline: the agreement lapsed; 5 renown of the fee is forfeit to Rulebender13" (refund 10 of 15). #1207 was asleep at purchase and ~56 tiles away; after waking it was parched and wandered. This is exactly the case the unfit gate is meant to stop — but buy_listing lets an asleep buyer's animal through (see 00:19). Confirms the gap costs real renown. **Medium**.
- Offer #7 (#1732 F → Pocket, 6) completed day 160, conceived. Offer #6 (#1703 M → Pocket, 6) completed day 170, conceived (0.7). Offer #8 (#1864 M → Rulebender, 6) accepted day 176 (partner 16 tiles off, met at (36,28)), completed the same day, conceived (0.10). "Mating done: … 6 renown paid from escrow." alerts each time.
- Tally so far: **7 cross-player matings completed** (2 listing sales to Rulebender13, 5 offers: 3× Pocket, 2× Rulebender), 1 purchase of a rival listing (lapsed), 1 offer failed (old age).
- Lineage 287 alive (day 179), 3.2% share, 3rd of 5 players. Rulebender13 leads 12.7% (1,106).

## 00:24 (days 187–190) Herdmaster accepts; meeting slid 34 tiles with no figure; I buy listing #3 again
- Offer #11 (#1732 F → Herdmaster13, 6) accepted day 187: partner #14132 at ~(56,96), mine at ~(68,20); meet (60,92) = **right at their animal**; meet_note "moved to keep clear of a predator near the usual point"; walk_warning "#1732 walks about 68 tiles alone to the meeting point and is already thirsty…". The note no longer says how far the point moved (p12 said "moved 12 tiles toward the partner"); here it moved ~34 tiles off the halfway point onto the accepter's side and my animal walks the whole way. The walk_warning carries the length, so it's discoverable, but the note alone undersells it. Low/Medium (fairness of lineage offers: the halfway promise can silently become ~100% my walk).
- Rulebender13 relisted dam #1010 (15) and opened an auction for stud #1017 (reserve 10; sterile with my lineage per preview: birth_mode gap 0.4).
- **Bought listing #3 again** with #12143 (awake, fit, hunger 22, 49 of 260 days): offer #13, escrow 15, preview fertility 0.2, distance 16–19. The dam is asleep (deadline waits).
- Taper wording (code read, game.rs:4397): still " (N, reduced by the taper on a balance over 1,000)" — the "balance plus renown held in offers" wording is **NOT present** in client or server source. STILL PRESENT (can't reach 1,000 live yet; balance ~250).
- Update flapping (Low): while offer #11 is paused asleep (`asleep: true`, my animal not moving: mine cell (68,20) constant), mating_updates still arrive every ~5 s with `distance` alternating 68 ↔ 72 (and expires_tick advancing): 9 updates in 90 s that carry no news. The 4-tile change threshold has no hysteresis, so an animal jittering on a boundary re-triggers it. The client only toasts status changes, so it's bandwidth/feed noise, not toast spam.
- **UI wording bug (Low)**, Mates tab agreement card: "Meeting point predators are near every point between the two: the usual point is kept." — the client prefixes "Meeting point " to every meet_note, which reads fine for "moved to keep clear…" but not for the "predators are near every point…" note. Screenshot GeneBroker13-0025-tab-matings.png. Repro: any agreement whose meet_note is the "usual point is kept" one.
- Market tab shows Rulebender's dam #1010 with "asleep: walks to no meeting until it wakes (the deadline waits)", "232 of about 360 days", "2 sold · promised", "Can't buy: already promised" — informative.

## 00:26 (days 215–218) Winter; second purchase failed — the listed dam died of old age at 64%
- Offer #13 (my purchase of Rulebender13's listing #3) failed day 215: "organism 1010 died of old age before the mating" — the dam was 232 of about 360 days (64%) when I bought, shown as a normal listing. Full refund of 15 (fine). Second confirmation that **old-age deaths start well below the 90% gate**, and listings themselves have no age gate or warning (the Market card just says "232 of about 360 days"). Medium (same root as 00:22).
- Winter day 218: 225 of 232 adults dormant → no offers or listings possible for a while. Unfit-asleep refusal again on #13831.
- Standing: **#2 player, 5.0% share, 234 alive** (Rulebender13 22.7%, 1,506). Balance 499.
- Income totals: earned 1,011 since founding; shop 490; sales 20; escrow 85 / paid 31; refunds 43.

## 00:28 (days 240–247) Agreement lapsed: old-age death mid-agreement, 5 of 6 forfeited
- Season 4 ended: **#2 of 5 legacy, #2 of 5 lineage**.
- #1732 (offered at 159 of 260 days = 61%, no "old" warning anywhere) died of old age day 241 at ~79% while asleep on the way to Herdmaster13's #14132. Alert: "send another animal for agreement #11 within about a real minute, or cancel (else the hold fee is forfeit at the deadline)". In winter 97% of my adults were dormant, and substitutes must pass the same checks, so the substitute route was impossible. Day 247: lapsed, **5 of the 6 renown forfeit to Herdmaster13**, 1 refunded. A natural old-age death the server judged "fit" costs the offerer the hold fee. Medium (old-age gate threshold again; also the "about a real minute" substitute window vs winter sleep).
- Market empty: Rulebender's listing #3 closed with its dam's death; no one else listed.

## 00:29–00:30 (days 250–264) Old-age refusal seen; CORRECTION on the request limit
- **Old refusal VERIFIED**: offer_to_lineage #1792 → `unfit` "#1792 can't be offered now: it is old (239 of about 262 days) and may die before the two meet" (91%).
- But #1731 at 225.5 of 258 days (87%, no `aging` flag) was accepted at 00:29:18 and **died of old age 49 s later** (offer #14 failed, refunded). #1898 at 238 of 277 (86%) accepted too (offer #15 → Pocket). With the Rayleigh old-age curve (mean 85%), an 86–87% animal is already past the average death age, yet the gate and the client's "(old…)" flag (game.rs:4135/5134, both `>= 0.9`) say nothing. Reinforces the Medium above.
- **CORRECTION to 00:21 "3 offers per hour" (downgrade to Low)**: code (game.rs:4479, 5030) drops accepted/completed/cancelled/failed offers from the per-target log; only pending and declined ones count. I hit `too_many_requests` because three were pending at once. Fair design. Remaining nit (Low): the refusal says "you have sent PocketNomad13 3 requests recently; try again later" with `ticks_left: 35998` (≈ 60 min), although the slot frees as soon as any of them is answered — the message should say "3 unanswered requests are open; wait for an answer or cancel one".
- Achievement "Lineage longevity: 2 years" +20 renown.

## 00:31–00:32 (days 270–292) Spring trading round 2
- New offers: #16 (#12141 M → Rulebender, 6, apart 8), #17 (#11968 F → Herdmaster, 6, apart 96), #18 (#10854 M → Pocket, 6, apart 8) — **#18 completed within seconds, conceived** (trade #8). #12280 refused unfit (starving) again.
- Listed stud #12142 (API) and dam #12243 via the Mates tab "List it for 10" (works; toast; listings 6 and 7). Old listing #5 had closed: "#1730 died of old age" (listed at 122 days).
- Agreement #15: my #1898 died of thirst on the way ("send another animal for agreement #15 within about 2 real minutes, or cancel"). substitute_mating with #11331 → `already_pregnant` (state said not pregnant; mine was 4 s stale); with #10816 → accepted, new walk_warning "walks about 32 tiles alone". **Substitution works**.
- Second restore: preview 294 / 5,139 food (feeds_others: true) → bought: 294 / **5,519 food** (+7%). Matches quote again.
- Lineage "dying off slowly" (hunger 26, thirst 13, hunted 4 in 10 days); balance had reached 877 before the restore.
- **Substitute lapsed — whole fee forfeit (Medium)**: the substitute #10816 (accepted 00:32:25, day 292) kept the original deadline (expires_tick 14464 ≈ 9 game days ≈ 47 real s away) though the pair was 64 tiles apart and the walk_warning said 32 tiles alone. Day 301: "#10816 never came to #15300 within the deadline: the agreement lapsed; **5 renown of the fee is forfeit** to PocketNomad13" (all 5 of 5). The server accepts a substitute that cannot physically arrive in time and charges the full forfeit; it should extend the deadline for the new walk or refuse with "too far to arrive before the deadline". Repro: let the offered animal die mid-agreement late in the deadline, substitute one ~60 tiles away.
- Season 5 ended: #3 of 5 legacy and lineage.

## 00:33–00:36 (days 309–340) Ninth trade; sleep dominates
- Offer #16 (#12141 M → Rulebender13, 6) completed day 333, conceived (fertility 0.08). **9 cross-player matings completed so far.**
- Listing #7 closed: dam #12243 starved (listed 2 game-weeks earlier, healthy-ish hunger 57).
- Day 309 (autumn): 136/165 adults dormant; day 318 "95% asleep" alert. The trading window each year is roughly spring + early summer (~60 game days ≈ 5 real minutes of every 10). p12 Low "~80% asleep by mid-summer": still present (by design of the starter's hibernation, but it starves the market).
- Phone (390×844) Mates tab: form, "List it for 10" button and Recent outcomes render fine (GeneBroker13-phone-0034-tab-matings.png).
- Balance 826 (+6 escrow) at day 340, saving for the taper check.
- Winter restore: preview → price 0, restored 0, hint "nothing would regrow there now (winter, a drought, or the plants are already as high as the season allows)"; buying → `nothing_to_restore` with the same reason, no charge. **Refusal VERIFIED for nothing**; the <50-food cut-off (economy::RESTORE_MIN_FOOD = 50, shop.rs) is in the code — I didn't hit a 1–49 food case live.

## 00:37–00:38 (days 353–370) Era 1 over; taper seen live
- Season 6: #2 of 5 legacy and lineage. **Era 1: I finished #2** (mean 3.2%, 3.7% at its end); niche prize "over era 1, on average 28% of the breeding alleles in the tundra were yours" (+40).
- Listing #6 closed: stud #12142 died of old age (82% of its lifespan) — every listing I made this game ended by death (2 sold first).
- **Taper wording STILL PRESENT (Low)**: ledger "Day 367 income: legacy share 3.4%, weighted 2.8% (5, reduced by the taper on a balance over 1,000)" posted when my **balance was 998** (+6 in escrow = 1,004). Escrow is counted (fix holds), but the text still says "a balance over 1,000", not "balance plus renown held in offers"; a player at 998 reads it as a bug. Repro: hold a small priced offer while the balance sits just under 1,000.

## 00:38–00:40 (days 370–386) First INCOMING lineage offer
- 00:38:48 `mating_request` #19: Rulebender13 offers its #17131 (198 of 360 days) to my lineage, free, apart 16.
- Mates-tab incoming card: "Expires unanswered in 120 days (≈ 9 min). **Your nearest suitable animal is about 16 tiles away: they would meet halfway.** Their #17131 is 202 of about 360 days old." → **incoming lineage offers say "halfway": FIXED**; ages shown on incoming cards too.
- The card's "which of your animals" select lists 24 candidates (= `lineage_candidates`), but **it includes animals the offerer side would refuse as unfit**: #13638 (hunger 100, thirst 71, 228 days), #30615 (hunger 100), #25487 (hunger 96, thirst 77), #25993 (hunger 89), #25994 (thirst 83). respond_mating has no fitness check on the accepter's pick (code read: check_fit_to_offer only in request_mating/offer_to_lineage), and the default "nearest free one" could be one of them. **Low/Medium**: the unfit gate is one-sided (requester, not accepter/buyer). Suggest filtering candidates and checking the picked partner.
- I accepted through the UI picking #29875 (hunger 8, thirst 5, 91 days): toast "Mating offer 19: accepted"; card "Walking to meet: the partner is about 40 tiles S of yours. They meet near 20, 44 (S of yours). One is possessed: the other only walks to the meeting point and waits there."
- New player LateComer13b ("Ochre Lateglow", 0% mean, 20% of pairs fertile with mine).
- Spring offers: #20 (#25220 F → Pocket, 8, apart 64), #21 (#25224 M → Rulebender, 8, apart 16), #22 (#25381 M → LateComer13b, 8, apart 96). Two picks had already died (`not_found`; my state file was a few seconds old). Listed stud #24169 for 12 (listing 9). Someone created and closed listing 8 between my looks.

## 00:40–00:41 (days 391–400) Drought crash kills the incoming deal
- Incoming deal #19 failed day 392: "organism 29875 starved before the mating" — my pick had hunger 8 when I accepted ~18 game days earlier, then walked toward a partner 40 tiles S in a drought (0.58). Listing #9 closed: stud #24169 died of thirst. No money involved (free offer).
- Lineage crashing 140 → 92 in 10 days (27 hunger, 26 thirst, 7 hunted). Crash alerts are informative.
- claim_territory at the lake shore (29,14) r12 → `overlaps` with a clear explanation. Migrated group A to (30,8) by the big lake: reply gives journey (1.1 days, 9.8 tiles), destination water share 39%, food for ~10 days, and that the forage circle reaches into Rulebender13's territory at (46,22) r18.
- **One-sided gate confirmed live (Medium)**: PocketNomad13 accepted my offer #20 day 403 with partner #14675 whose `partner_age` was **{244.7 of 265, aging: true}** (92%, past the very gate that refuses MY animals) — 64 tiles apart. It died of old age day 409 ("organism 14675 died of old age before the mating"); refund to me OK. respond_mating (and the server's default "nearest available" pick — I can't tell which Pocket used) lets an `aging`/starving/asleep partner be picked. The offerer gets the refund, but the deal and its walk are wasted. Repro: accept a lineage offer naming an animal ≥90% of its lifespan → ok.
- Offer #21 failed: my #25224 starved before Rulebender answered (pending, drought).

## 00:42 (day ~420) Rescue spending; third restore 9% under the quote
- Lineage down to 83 (1.2%, 6th of 7). Spent: provision 36 (20 fed), **reinforce 150 → 10 adults, "at": [22.57, 10.08]** (p12 Low float noise in reinforce `at`: FIXED), restore.
- Third restore: preview 200 / **629 food** → bought 7 s later: 200 / **573 food** (−8.9%). Within the server's 20% `quote_changed` tolerance, so it charged the full quoted price. Earlier two buys gave +3.8% and +7%. So p12 #9 is fixed in the sense of "same circle, no systematic −14.5%", but in a drought the delivery can still fall short of the quote by up to 20% at the quoted price. Low.
- create_auction (stud #29049, reserve 8, days 5) → ok with `days: 62.5` and **note "lengthened from 5 to 62.5 game days: every auction runs at least 5 real minutes"** (p12 Low "auction days 20 → 62.5 silently": FIXED — now explained). Listing 10, dormant stud.
- Group alert day 427: "Group A has outgrown its home ground at (30, 8): it feeds about 85 of the group's 95 animals now. Most of the group is asleep…" (clear).
- Day 446: "Group A (main)'s home ground at (30, 8) has no drinking water left" — the point is on the shore of the map's biggest lake (deep water from x≈32). Probably ice at the turn of winter (tundra), but the alert doesn't say frozen; reads as wrong. Low / question.

## 00:46–00:47 (days 470–480)
- Offer #22 (→ LateComer13b) failed: "#25381 died (winter sleep) before the mating" (refund). Season 8: #5 of 6. Lineage 73, all asleep at the start of spring.

## 00:47–00:49 (days 485–495) Second incoming offer; spring round 3
- 00:47:46 incoming #23 from Rulebender13 (#42405, 81 of 383 days, apart 16, free). lineage_candidates gave 8 males, **again incl. #39519 (hunger 100), #42522 (hunger 100), #44183 (hunger 100, thirst 92)** and **#29049, my stud currently up for auction (listing 10)**. I accepted with #29049 (respond_mating partner 29049) → ok: the auction didn't lock it (no bids yet). An animal on auction can be promised in a separate agreement at the same time. Low (check what happens if a bid lands).
- **`apart` ≠ "nearest suitable" (Low/Medium)**: the card said "Your nearest suitable animal is about 16 tiles away: they would meet halfway", but the agreement then showed distance 72 (their #42405 at region (68,76), my candidates around (25,12)). Code (game.rs ~5166): for a lineage offer `apart` is the nearest adult of the other sex in the lineage — any, incl. pregnant, promised, asleep, starving ones that lineage_candidates excludes — so the "nearest suitable" figure can be far below the real walk. Same with #19 (apart 16, real 40).
- My offers: #24 (#38129 F → Pocket, 8, apart 96), #25 (#38727 F → Herdmaster, 8), #26 (#42523 F → Rulebender, 8, apart 8), #27 (#39520 M → LateComer13b, 8).
- Fourth restore (home grazed bare, 2 starved): preview 200 / 2,720 food.
- Fourth restore bought 11 s after the preview: **2,465 food vs 2,720 quoted (−9.4%)**. With the third (−8.9%), the last two buys both came in ~9% short at the full price (the first two were +3.8%/+7%). Restore food vs quote: **PARTLY fixed** (circle kept, but ~9% short is common; the quote is only checked against a 20% drop). Low/Medium. Suggest pricing by the food actually delivered, or re-quote if > 5% short.
- Incoming deal #23 failed day 498, 29 s after I accepted: "organism 29049 died of old age before the mating" (208 of 259 days = 80%, no `aging` flag, offered in the candidate list). The same animal's auction (listing 10) ended with it. Old-age deaths at 61–80% have now failed **six** of my deals (#12, #11, #13, #14, #20 partner, #23).

## 00:50–00:51 (days 519–526)
- Offer #26 (#42523 F → Rulebender13, 8) completed day 519, conceived (0.32). **10 cross-player matings completed.**
- Lineage still starving at the lake home (13 hunger, 8 thirst in 10 days); "outgrown its home ground" alerts.

## 00:52–00:53 (days 537–552) Collapse to 21
- Drought (48% dry) + summer sleep: lineage 81 → 21 in ~15 days (hunger and thirst beside the big lake). #38129 died of thirst on the way to Pocket's #45158 (agreement #24; substitute window "about a real minute", nobody awake to send).
- Alerts while asleep: "While most of your lineage sleeps (86% asleep): Your lineage is crashing. **Sleepers can't be moved or fed**…" — yet a Provision bought seconds later reported `fed: 20` with all 21 animals dormant. Either sleepers can be fed (then the alert is wrong) or provision fed nobody useful. Low (wording/consistency).
- Season 9: #5 of 6.
- Agreement #24 lapsed day 579 (my animal died of thirst; nobody substituted): 2 of 8 forfeit, 6 refunded. Severe-winter catastrophe days 571–600. 20 alive, 0.5% share, 7th of 7.

## 00:57–00:58 (days 597–610) Spring again with 15 animals
- Offers #25 (Herdmaster) and #27 (LateComer13b) failed at day 600/604: **both animals died of old age** while pending (offered at 110–115 days; died at ~82% and ~88% of lifespan). Pre-expiry reminders ("expires in about 2 minutes without an answer") arrived first. Season 10: #6 of 6 legacy, **#1 of 5 survival**; "Survivor" +25.
- New offers (10 each): #28 (#49296 F → Rulebender, apart 8), #29 (#49398 M → Herdmaster), #30 (#49745 F → Pocket), #31 (#50331 M → LateComer13b, apart 64).
- Restore in early spring: preview 1,092 food → buy 1 s later refused **`quote_changed` "the pasture changed since the quote: about 861 food now, not 1092; preview again"** (the >20% guard works live). Re-preview 596 → bought 571 (−4%). The restorable amount halved in ~12 s (natural spring regrowth), at a constant 200 price — explains the earlier ~9% shortfalls. Low.
- Second reinforce (150): 10 adults "at": [17.05, 22.11], `nearby_count: 0` — landed ~18 tiles from my herd at (30,8). Fine.

## ~01:00 End
- The container restarted at about 01:00 UTC and the server went away (coordinator notice); my last command died with exit 137 at ~00:59. Final state seen (day ~620): lineage Broker Clover ~25 alive (15 + 10 reinforcements), 0.5% share, 7th of 7; balance ~760; offers #28–#31 and listing #11 still open. Ended 2026-11-20 ~01:00 UTC (started 00:09; ~51 minutes of play).
