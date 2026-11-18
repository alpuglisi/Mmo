# GeneBroker6 playtest log (breeder/trader)

Start: 2026-11-18 21:05 UTC, server day 14 (fresh world, only wild on the leaderboard).

## 21:05–21:20 (days 14–95) Registration, designer, founding

- Registered GeneBroker6 via the UI (player 1). Founded "Broker Bucks" (lineage #8) via UI at day 60: warm, live birth, litter 3, 40 founders, vigilance/camouflage/burrowing/toxins 0.5/seasonal coat etc. 70.4/100 pts.
- **Warm + abandoned eggs warning (P5 #3): FIXED.** Selecting Abandoned eggs on a warm design shows in the designer warnings: "Warm-blooded eggs left unattended chill and die unless the weather is hot: this lineage would hardly ever hatch young. Choose incubated eggs ... live birth, or make it cold-blooded." (shot GeneBroker6-02-warm-abandoned.png)
- **Name filter (P5 #5): PARTLY FIXED.** Server (POST /api/design/compatibility) now rejects wild, Wild, "w i l d", W1ld, wiId, "the wild", "wild.", "(wild)", "Wild 2", GM, "G M", G-M, G_M, GMs, GM6, G.M., Game.Master, "Mod herd", M.o.d, GameMod, TheOwner, SupportTeam, Root, Anthropic, Admin, A.d.m.i.n, S.y.s.t.e.m, Server.
  - STILL PASSES (Medium): "G.M. herd", "The G.M. herd", "G.M.herd", "herd G.M.", "M.o.d. herd" (dotted staff abbreviations inside a longer name), while "GM herd" is rejected — inconsistent. Rival player names as lineage names pass: "Rulebender6", "Herdmaster6", "GeneBroker6" (exact player names!), and "Rulebender"/"Herdmaster"/"PocketNomad". "Wild herd", "Wild Ones", "Wild species", "wildlings" pass ("Wild species" on a leaderboard reads like the wild ecosystem) — Low/Medium.
  - UX (Medium): the designer shows NO error while typing a bad name; the live compatibility request returns 400 bad_name (console errors "Failed to load resource: 400" for every keystroke-check), the Found button stays enabled and #design-problem stays empty. The user only learns at submit (not verified) — the bad_name answer is already in hand and should be shown inline.
- Designer interbreeding card: my default-size warm herbivore is 66–68% fertile with wild Steppe antelope (measured) — big dilution risk, clearly warned. Market section names Herdmaster6, Rulebender6, PocketNomad6 as breedable.
- Compatibility (WS `compatibility`) after founding: Rulebender6 Loophole Fliers (cold) limiting "endothermy", mean 10%, best pair 32%, 66% of pairs fertile → **cold/warm partial fertility (P5 #1/#2): FIXED** (no longer sterile; best pairs ~30%). PocketNomad6 16% mean (best 75%), Herdmaster6 8% (best 41%), ApexHunter6 (carnivore wolves) 0.
- **Listing minimum 5 for all (P5 #6 stud-lock): FIXED.** create_listing stud fee 0/1/4 → bad_price "a listing is at least 5 renown", dam fee 0 same, create_auction reserve 0 same. Listed stud #1 (5415, fee 12) and dam #2 (5414, pregnant, fee 10; shows busy:true).
- Renown shop: provision_price 40 for 20 (small lineage), reinforce 150, slot 30. Market fee 10%.
(Correction: founding was at ~21:09 wall clock; first market events 21:12.)

## 21:10–21:16 (days 77–120) First trades — market is OPEN
- 21:10 d77: offer_to_lineage to Rulebender6 (cold fliers) and PocketNomad6, price 5 each (escrow 5 each).
- 21:12 d97: PocketNomad6 bought my dam listing #2 (fee 10). 21:13 d103 their #3335 starved en route → alert "may send another animal before the deadline"; 21:13 d107 they sent #3327 ("sends #3327 instead of #3335") → completed d108, conceived (fertility 19.6%). **substitute_mating after buyer's animal dies: FIXED/works.** Ledger: "Sale: 9 renown from PocketNomad6 for listing #2, less a 1 renown market fee". **Sales complete: FIXED** (P5 had 0).
- 21:13 d103: my offer #2 accepted by PocketNomad6 (their #6512) → completed, conceived (fertility 41%), 5 paid from escrow. Second completed cross-player trade.
- 21:12 d97 Rulebender6 sent a free request (#4) for my listed stud 5415; 21:13 d104 Rulebender6 then BOUGHT listing #1 (fee 12) on the same stud while request #4 was still pending → **pending requests don't block listings: FIXED.** I declined #4.
- Winter (d90–119): bought agreement #5 (Rulebender6's #3599 → my stud 5415, which was dormant/hibernating): expires_tick moved 6432 → 7246 while my side slept → deadline pauses with one side asleep (Rulebender's animal state not visible to me, so "either" vs "both" not 100% proven; it's at least pausing). Partly verified.
- Listings remain after a sale (sales:1, busy:true) — fine. Listing a pregnant dam is allowed; UI listing card does not say "pregnant" (Low).
- Auction: asked days:8, got deadline 3000 ticks (62.5 game days) because of a wall-clock minimum (min_auction_ticks); the reply doesn't explain it (Low; check UI).
- Season 2 ended d119: market category had 0 rows — expected (each seller had just 1 sale; 2 needed). Season 3 is the chance for the market title.
- Concern (Medium): founders starving/hibernating in winter with hunger 50–82 (auctioned 5423 dormant at hunger 82); 1 death labelled "winter sleep" on day 81 (late summer, before winter at d90) — P5 Low "cold, heat or winter sleep" label wording area.

## 21:14–21:18 (days 117–163)
- d117 Flood alert now exists: "Flood: 25% of the rivers are over their banks ... 4 of your animals stand on floodplains or lake margins (0 asleep). Move groups to high, dry ground." (P5 #4 alert part: FIXED).
- Winter starvation: ~10 founders starved d106–129 (hibernating design, fat 0.8) + "going hungry" alert d119. My design simply under-fed; noted as balance info, not a bug.
- d130 Provision (small lineage, 120 alive): 40 renown, fed 20, "2 renown each". OK.
- d131 stud sale #1 (Rulebender6, fee 12) completed: ledger "Sale: 11 renown ... less a 1 renown market fee" (1.2 → 1, nearest). Outcome cold×warm: fertility 24.5%, conceived. UI "Recent outcomes" reads "Conceived (25% chance to conceive) · 54% risk the override failed" — 'override' wording confusing for a market sale that succeeded (Low, P5 Low item still present in new words).
- d140 My offer #1 to Rulebender6 (pending ~65 days) accepted & completed: did not conceive (22.8%), 5 paid.
- d156 Rulebender6 bought my dam #5416 listing (fee 25) — pending meet (their animal possessed, 36 tiles away).
- d160 Bought Rulebender6's stud listing #6 (fee 5) for my female 5414 — she is ALSO my own listed dam (#2). Allowed; completed in 1 day; not conceived (17.4%). preview_mating cold×warm pairs show 29–32% fertility on the best pairs → partial fertility ~30% confirmed.
  - Minor: buy_listing for 4 females in a row answered already_pregnant (my 20-s-old state); fine, but the Mates/Market UI "For my adult" picker should hide pregnant females (P5 Low "incoming picker lists juveniles" related) — check.

## 21:18–21:24 (days 165–222)
- d166 Rulebender6 cancelled their accepted purchase of my dam #5416 (fee 25): "6 renown of the fee is forfeit to GeneBroker6", 19 refunded → **cancel forfeit 25% works; rounding 6.25→6.** Ledger: "Hold fee: 6 renown from Rulebender6 for listing #3 (the buyer cancelled)" — logged as kind "sale" but NO 10% market fee taken (design: "10% of every payout is destroyed"); 6×10% = 0.6 → should round to 1 → 5 expected. **Fee not applied to forfeits (Low/Medium, inconsistent).**
- Season 3 (d121–180) result: "Stud market: Rulebender6 (6 renown)" → **market title winnable: FIXED** (first award). BUT Rulebender won with two 5-renown trades both bought by me (their stud listing #6 + my offer_to_lineage #1). MARKET_TITLE_BUYERS = 1 → two colluding players can farm the title with 2×5 renown per season (Medium exploit/fairness). My season-3 tally: one sale (11) + one hold fee (not counted) → unranked; fine.
- d177 my auction #5 closed without a bid → "the lot is yours again". d184 "Listing #1 closed: #5415 died of old age." (good alert).
- d203 provision on a 443-alive lineage: shop shows provision_price 160 / provision_size 40 (4 each), fed 40 → **provisions for big lineages: FIXED** (reinforce correctly blocked: "for lineages of at most 300 (yours has 443)"). Provision count reset after 60+ days (rolling window) as documented.
- Population crash: peak 489 (d201) → 224 (d221), 11–27 starved/day; my herd sat at (40,27) where 16×16-block veg ≈ 2/255 (barren). Catastrophe: severe winter d211–240.
  - Hunger warning now accounts for sleepers: "73% of your animals are asleep: sleepers can't be moved, and those that starve are the ones out of fat or awake. Fatten them..." → P5 #11 sleeping-herd advice: FIXED.
  - migrate to (84,22) (42 tiles): reply warned dormant share (52%) and destination food (feeds 52 of 366). At d219: "Group A didn't gather at its migration target ... the order has ended: 73% of the group is dormant ... migrate again once it wakes." → P5 #8 quiet failure: FIXED for this case (clear alert).
  - split_group with an explicit organisms list fails entirely if one listed animal died since ("organism 10306 not found") — could skip the dead (Low). Split 41 awake into group 1 and migrated them.
- Listed: stud #7 (11919, fee 5), dam #8 (12216, fee 7), auction #9 (17304, reserve 5).

## 21:24–21:27 (days 222–240) Nameplate filter, provisions
- Lineage still dying in the severe winter; all my listed dams starved (alerts "Listing #N closed: #id starved" — good). Bought 2 more provisions (40 each, 20 fed); 4th refused "at most 3 provisions within a ranked season's length (60 days)" (clear).
- Nameplate title filter (P5 #5): rejected "G.M.", "wild", "M.o.d", "Server G.M. team", "Official", "Wild species"; "Herdmaster6" and "Rulebender" → "looks like another player's name" (good — titles check rival names, lineage names do NOT).
  - **STILL PASSES (Medium): "The G.M. herd" and "herd G.M."** — bought both (500 + 100 renown) — dotted staff abbreviation inside a longer title. Changed to "Gene Broker" afterwards (100).

## 21:27–21:31 (days 240–256) Spring, season 4 result
- Season 4 (d181–240): I placed **2nd legacy (7.4%)** and 2nd lineage (peak 489); Herdmaster6 33.6% mean, 2,099 organisms (hard cap). Market: nobody (no second sale). Survival: all five players value 1.0 (severe winter), summary names no survival winner (consistent with "a title nobody failed goes to no one").
- Group B (41 awake split off) migration ended d238 "stopped getting closer" (only 13 left alive) — clear alert.
- d248 merged groups, migrated main to (40,54): reply predicted food for 145 of 152. d252 "reached its migration target"; d253 (one day later!) "Group A has outgrown its home ground at (40,54): it feeds about 89 of the group's 152" → **territory/home re-check as herds grow (P5 #9): FIXED**, but the migrate estimate (145) vs one-day-later (89) disagree a lot (Low).
- claim_territory (42,58,r20) → "overlaps another player's territory; claim ground of your own" (no owner named → P5 radar naming fixed). Immediately after, claim (36,54,r12) → claim_cooldown "This group ... claimed a territory recently; it can claim again in about 1.9 min (23.5 game days)". I never had a successful claim: the REFUSED claim started the cooldown (probably the anti-radar fix), but the message says "claimed a territory recently" — misleading (Low/Medium: a player who misjudges one overlap is locked out 23 days, and the wording blames a claim that never happened).
- Listings: #7 stud died (eaten) → alert. New dams #10 (fee 5) #11 (fee 15); withdrew #4.

## 21:31–21:34 (days 256–290)
- d260 Rulebender6 offered to my lineage (offer #10, price 5); I accepted d289 (server picked my nearest free female 15643), completed d290 (fertility 10.8%, not conceived). Ledger: "Sale: 4 renown from Rulebender6 for offer #10, less a 1 renown market fee" → **fee rounding: half rounds up** (code `(amount*10+50)/100`): 5→1, 10→1, 12→1, 25 would be 3. Consistent with "nearest", but at the 5-renown minimum price the fee is effectively 20% (Low; worth a sentence in Help).
- **Fee not applied to hold fees (code-confirmed):** game.rs forfeit_hold posts `fee` straight to the seller as LedgerKind::Sale without economy::market_fee (my 6-renown hold fee arrived whole). Low/Medium inconsistency with "10% of every payout is destroyed".
- forfeit_hold also excuses the buyer entirely when the partner (seller's animal) slept through part of the hold ("nothing is forfeit: ...slept through part of it") — a seller whose listed animal hibernates can't collect any hold fee; buyers can hold a hibernating dam free. Low/Medium (from code).
- d273 "Group A's home ground at (40,54) is grazed bare (food for about 3 days)" alert — good.
- Recovery: 149 (d255) → 475 alive (d290) in spring.

(Wall-clock note: the times in section headers after 21:16 ran ahead; real time at day 297 was 21:29. Events in gb6/events.jsonl carry the true times.)

## 21:29 (day 300) Species / offers
- /api/species: every player lineage has its own species (Broker Bucks, Loophole Fliers, Rolling Thunder Herd, Pocket Gliders Six, Ember Stalkers) — no small player lineage filed into a wild species this game (P5 #7 part: looks FIXED; copies-of-a-design case not testable by me — needs two players founding one design).
- Oddity (Low/Medium): extinct species 6 "Pocket Nomads" (PocketNomad6's dead first lineage) now lists dominant GeneBroker6 51%, lineages [Broker Bucks (GeneBroker6), 0 adults], namer cleared. A dead species with 0 adults flips its credit to me (presumably via old hybrids) — credit flipping still happens for extinct species.
- d300 offers: #11 to Herdmaster6's lineage (my old female 8134, health 55, price 5), #12 to Rulebender6's (12457, price 6) — hoping to observe substitute/death handling from the buyer side.
- d309 offer #11: my 8134 died of old age while the offer was still pending → "failed", full refund 5 (pending, no hold) — fine.
- Season 5 (d241–300): 2nd legacy, 2nd lineage. My one season-5 sale (offer #10) → still no market title (need 2).
- name_species: same name → refused "same_name" (P5 Low "charged 300 for own name": FIXED); extinct species 6 → "species_gone"; "wild" → bad_name; **"Wild herd" accepted as a species name (300 renown)** — a species called "Wild herd" in the species list reads like the wild ecosystem (Low/Medium; P5 #5 partly fixed). Renamed back to "Broker Bucks" (another 300).
  - CORRECTION: rename back refused: "already_named: GeneBroker6 has already named this species" → species names are one-shot and permanent, so my species is now stuck as "Wild herd" (no renown charged for the refusal). Makes the "Wild herd" filter gap Medium: a permanent, misleading species name.
- d316: lineage reached 1,015 alive (34 born/day).

## 21:29–21:34 (days 309–354)
- d316 big-lineage provision (1,015 alive): shop provision_price 320 / provision_size 80 (4 each) → fed 80. Scales as documented (P5 #13 provisions: FIXED).
- d309 Rulebender6 bought my dam listing #10 (fee 5) with a possessed animal 36 tiles away; my dam 7779 went dormant for winter. expires_tick 16370 → 17810 by d354 while my side sleeps → deadline pauses when (at least) the seller's animal sleeps (P5 winter pause "only when both sleep": appears FIXED; can't see whether the buyer's animal also slept).
- Flood alerts d332/d346 with counts on floodplains (65, 12; 0 asleep) — good.
- d320 "Your legacy share passed 10%." Listings keep closing from old age / winter sleep deaths — alerts clear.

## 21:34–21:37 (days 355–366) Era 1 end, hall of fame, species
- d355 Rulebender6's buyer animal #4902 died of old age en route to my dam → "may send another animal before the deadline"; d356 my dam 7779 also died of old age → offer failed. Fine.
- substitute_mating on my own *pending* offer #12 → "unknown_offer: no offer 12 involving you" — wrong message (the offer exists and is mine; it just isn't an accepted agreement with a dead animal). Low.
- **Era 1 ended (d360): "you finished #2 with a mean 5.5% of breeding alleles over the era (18.0% at its end)"** — era ranks now use the mean (LateComer6, joined ~d330, ranked last with mean 0.0% despite 1.5% final) → P5 #12 era snapshot: FIXED.
- **HIGH/MEDIUM — staff-impersonating nameplate live in the Hall of fame:** Ranks tab shows "Rulebender6 «Head G.M.» 0♛ 1" (shot GeneBroker6-e-ranks.png). Same gap I reproduced ("The G.M. herd", "herd G.M." accepted as titles). P5 #5 "G.M." in titles: STILL PRESENT when embedded in a phrase.
- My species is publicly listed as "Wild herd · herbivore · ~748 adults · GeneBroker6 98%" (World tab) — permanent, see above.
- **MEDIUM — species-founding title for a copied design:** LateComer6 founded ~d330 a lineage with the same body plan as PocketNomad6's Pocket Gliders Six (cold, birth 0.75, size 0.5, herbivore). /api/species files it as origin "split", parent 9, founder LateComer6, and Season 6's summary awards "Species founded: LateComer6 (1 species)" + the "Species founded" achievement after ~5 days of existence. A fresh founding near a rival's design is being credited as speciation → the "New species" title can be farmed by founding a near-copy. (P5 #7 copies: no longer merged into the first player's species, but now earns a speciation title instead.)
- Season 6 (d301–360): I'm 2nd legacy (12.8% mean), 2nd lineage (1,080 peak). Market: nobody.

## 21:37–21:41 (days 370–410)
- Rulebender6's lineage gone at d362 (offer #12 failed "lineage_gone", refunded) — they seem to have retired/re-founded. My offer #14 failed: my animal starved; refunded (pending offer).
- Predation warnings now name the hunter each day: "being hunted: 11 of your animals were eaten on day 397, by ApexHunter6's Ember Stalkers (11)..." → P5 #11 predation warning: FIXED. 5–11 eaten/day by Ember Stalkers.
- Summer starvation again 28–31/day at (40,54). Migrate previews: destination water warnings present ("Only 14 tiles of drinking water for 402 animals in a drought (97% dry here)") → P5 #8 destination water warning: FIXED. Each preview *is* a real order (no dry-run): to compare 6 spots I had to issue 6 migrations (Low UX: a "preview destination" would help).
- Split main group (173/172): group 0 → (60,40) food 219; group 1 → (88,44).
- d421 Group A (173, to (60,40), ~25 tiles): "didn't gather ... it never got going: the way may be blocked ... or the group too busy with hunger and thirst." d425 Group B (172, to (88,44), 44 tiles): "didn't gather ... 64% of the group was basking or resting (cold-blooded animals warm up before they travel)".
  - **BUG (Medium): my lineage is warm-blooded** (designed warm; species endothermy 1.0) — the failure reason blames cold-blooded basking. Wrong cause text.
  - Migrations of ~170-animal groups over 25–44 tiles in summer still fail (P5 #8: no longer silent, but still don't work for big hungry herds). Partly fixed.
- Season 7 (d361–420): 2nd legacy, 2nd lineage again. No market title (no buyers this season; Rulebender6, the only active buyer, retired).
- 21:42 UI listing flow (Market → List one of my adults): picker lists adults; fee 3 → inline "Listing: A listing is at least 5 renown..." (good, P5 min-5 also enforced in UI messaging); then fee 5 → "#38402 listed for 5 renown." but the old min-5 error stays visible next to the success (stale error, Low). Fee input has min=0 (could be 5) (Low). Shots GeneBroker6-f-list-fee3/5.png.
- Market liquidity: by d440 no other player has any listing; only Rulebender6 ever listed (once). Trade happened via offers/buying my listings. Market is open but thin.

## 21:43–21:46 (days 480–495) Collapse, retire, re-found (via UI)
- Season 8 (d421–480): I fell to 3rd legacy (4.6%) as my lineage dropped ~700 → 277 (starvation + 5–11/day eaten by Ember Stalkers). LateComer6 2nd. Herdmaster6 52–56% share at the hard cap (2,100) — **balance (High): one warm small-bodied omnivore (Rolling Thunder Herd: endothermy 1, carnivory 0.25, size 0.25) has held the cap since ~d200 and won every season 1–8 legacy + lineage crowns**; P5's cold-flier class is gone but a new single dominant design took its place.
- 21:44 d490 Retired lineage #8 from the Lineage tab (app dialog). Confirm text: "Founding again within 375 game days (about 30 real minutes) of retiring **founds half the founders you design**."
- Designer after retiring: "Re-founding soon ⚠ Founding now gives 40 of your 40 founders, costing 84.4 points. ... founders cost double points (you get as many as the budget then buys, at least half). The full design founds again from day 865". Budget 84.4/100.
  - **Inconsistent rule text (Medium/Low):** the retire dialog still states the old "half the founders" rule; the designer and the actual founding use "double points".
  - **Re-found pricing (P5 #13): FIXED for the half-for-full-price bug** (it now charges for what it founds), BUT only the 20 extra founders' points doubled (14 → 28); with 29.6 unspent points my re-found got all 40 founders at no real penalty. The penalty only bites designs that spend the whole budget (Medium balance note).
- Founded "Broker Bucks II" (lineage #16) at d491: 40 adults, 20 pregnant, fresh newcomer grace 58 days (my previous founding was d59 → fresh grace is correct per rules).
  - Low: the new lineage's Group A shows "last order: migration to (60, 40) ended: never got going" — the retired lineage's group history carried over.
- Leaderboard row shows "Rulebender6 «Head G.M.»" nameplate too.
- d500 /api/species — **species credit for copies (P5 #7): STILL PRESENT (Medium).** Species 9 "Pocket Gliders Six" (founded by PocketNomad6, d185) now lists dominant **LateComer6** with only LateComer6's "Late Skylarks Six" (244 adults); species 11 "Late Skylarks Six" (the 'split' that won LateComer6 the season-6 species title) now has 0 adults. So the copy first earned a speciation title, then got re-filed into the original player's species and took over its credit. Also new "Rolling Thunder Herd (2)" split (Herdmaster6, 29 adults). My retired "Wild herd" species no longer listed; my new lineage not yet in any species at d500 (40 adults).
- d489 "Listing #18 closed: #38402 died." — the animal was removed by my retirement, not killed (P5 Low "listing alerts after a retire say 'no longer alive'": reworded, still wrong — Low).
- d492 re-found milestone alert states the double-points rule and "40 of 40 founders" — matches designer (dialog is the odd one out).
- d530 species: my re-found copy "Broker Bucks II" filed into my old species 8 ("Wild herd", named, dominant me) — no split title for copying my own design (good). PocketNomad6's "Pocket Thunder Two" (body plan identical to Herdmaster6's Rolling Thunder Herd: carn .25, size .25, warm, birth .5) got its own *founding* species, while LateComer6's copy of Pocket Gliders got a *split* — copies are filed inconsistently.
- Renown: 2,595 unspent at d533 with little to buy (provisions capped 3/season, reinforcements blocked > 300, nameplate/species cosmetic) → P5 #13 "renown has no use" partly still present (Low/Medium).

## Wrap-up 21:50 (day ~556)
Final: Broker Bucks II (re-founded d491) ~400 alive, 4.5% share, 4th of 7 rows (3rd player behind Herdmaster6 49.9%, LateComer6 9.9%). First lineage Broker Bucks: peak 1,080 alive, era 1 #2 (mean 5.5%, 18% at end), season legacy ranks 3,3,2,2,2,2,3,3 (seasons 2–9). Renown 2,742. Market: 4 sales completed as seller (9, 11, 4 net + 6 hold fee), 4 purchases completed (5 each). No market title (never 2 sales in one season). Watcher (PID 22116) stopped.
