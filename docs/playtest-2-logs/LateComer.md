# LateComer playtest 2 log
Start 23:20:38, joining at 23:45

## Wait period (23:20-23:45): reading docs/code, server untouched
- Read docs/playtest-1.md (29 findings), game-design.md, starter-design.md, README sections: founders/soft cap/market, predators vs players (founding grace 20 d), founding ramp (10 d), season titles (mean raw+weighted), winter provision (founding <=30 d before winter: founders fed, full fat), retire_lineage, reinforcement shop.
- Key numbers from code: budget 100; founders free to 20, 0.7 each to 50, then +0.07 each step (60=31.9, 70=49.7, 80=74.6, max 88). Founding hint warns at >=60 founders or >=900 kg. Grant 50 renown, stipend 2/day + 1/pct share. Reinforce: 10 adults for 150 renown (doubles each), only while alive <= 20% of soft cap. Season = 60 game days (~5 min wall).
- Calibration: leg length 25 pts/unit and hooves 16 are "valuable in every design"; armor/fat lose fitness; founders best buy (+16 founders = +0.30 log share). Warm live-birth wins most tournaments. Herdmaster (pt1 winner) used big (0.75 free) social hibernating grazer with some swimming.
- Code note: Game::found_at_any_price is #[doc(hidden)] (offline tools only) - fine.
- Doc inconsistency spotted: game-design.md "Point costs ... 0.7 points each up to 50" vs README "0.7 each above 20 up to 50" (20 free). Minor.

### Plan
1. Register via web client, check the season notice / founding hint / founder pricing display.
2. Design: warm, live birth, herbivore (sterile vs carnivore/omnivore players -> no dilution), body ~0.6-0.7 (free; herd defence, fewer predators), starter's fur/fat/hibernation, legs up, sociability high, ~50-55 founders (flat price), litter 3. Spend rest on leg length/hooves.
3. Use grace (20 d) to grow; claim territory on good food; avoid migrate into water.
4. Save renown for a reinforcement (150) while small.
5. Poll /api/leaderboard, /api/lineage/stats, /api/seasons/current via API between browser check-ins.
- Code reading (23:23): client has NO text for the founding grace (only an attacker's `newcomer_grace` error) and none for the 10-day founding ramp / half-weight fresh founding in season titles -> newcomer can't see they're protected or why their season score lags. To verify live.
- Code reading: design screen fetches world summary ONCE when opened; the season notice ("Winter starts in N days") and founding hint (summer/drought) don't refresh while the player edits (a day = 5 s, so 2 min in the editor = 24 days stale). To verify live.
- Interbreeding rule: body-plan gap = max over thermo/birth/size/diet/legs/eyes; sterile at >=0.30. Live (0.85) vs incubated eggs (0.5) = 0.35 -> an egg-layer is sterile with every live bearer. Mixed young "score for the wild".
- README: group orders (rally/territory) measurably cost a herd near carrying capacity; leaving herd alone is best default.

## Joined (23:45 wall, world day 321, year 2 (UI "Year 3") autumn, 9 days to winter)
- World at join: 5,772 organisms, wild 80% share; players small: GeneBroker2 9.3% (961 alive), PocketNomad2 3.7%, ApexHunter2 2.9%, Herdmaster2 2.6%, Rulebender 1.6%. => established players are NOT dominant; a late joiner can compete on share immediately. Ranked season 6 (days 300-359), 24 days left; era ends day 360.
- 23:45:30 registered via web client (smooth). Design screen: season notice shown ("Winter starts in 2 days ... Founders arriving now start fed, with full fat reserves.") - good, clear.
- BUG (low-med) 23:45:48: Interbreeding card says "a wild species: Broker voles (1296 adults...)" - Broker voles is GeneBroker2's player species name (it absorbed the wild Steppe antelope). Called "wild", and the same card says "no other player's lineage is within breeding range" though Broker voles lineage (owner GeneBroker2) is in that species.
- /api/species is cluttered: ~15 extinct/empty entries ("Steppe antelope (2)".."(7)", "Loophole Boars (2/3)", "Pocket grazers (2)") with 0 adults; species "Broker voles" (body 0.66) lists Steppe antelope + Broker voles + Thunder Bison lineages; "Steppe antelope" species also listed separately with 0 adults.
- Founder pricing label is clear: "Founders (20 included, 0.7 pts each up to 50, then each 0.07 pts dearer than the last)". Founding hint at 55 founders: "55 founders, about 250 kg... can grow to the population cap of 1,500" (ok-level; warns only at 60+).
- STALE notice confirmed (low): season notice still said "Winter starts in 2 days" at 23:46 when the world was already in winter (day 331, days_to_winter 0). Editor fetches world summary once.
- Design "Late Eggrunners" (96.8 pts): warm, INCUBATED EGGS (birth gap 0.35 => sterile with every live bearer; card confirmed "No wild or rival species can interbreed with this design"), litter 3, 55 founders, body 0.5, legs .7, hooves .5, toxins .7 + coloration .4 (warning colouration), fur/fat .6, hibernation .6, burrowing .4, vigilance .5, ears .4, vocal .4, camo .4, swim .3, caching .4, muscle .3, nose .3, eyes .3; sociability .9, boldness .3, pickiness .2, range .7, aggression .3.
- 23:46:06 founded on day 331-332 (winter, provision on).
- BUG (HIGH) day 332: "7 founders died: 7 drowned" within ~30 ticks of founding. Founding site is a coastal peninsula around (77-92, 100-116) next to/among water; 18 of 67 members stood on water tiles at day 342. Founding into winter drowns founders immediately (and more risk at the melt).
- BUG (med): Lineage panel still says "Each player may have at most 1,500 living organisms on this map, young included. At the cap births pause until some of yours die." - that's the OLD hard-cap rule; README says soft cap (litters shrink, juveniles count 1/4, hard ceiling 2x). Wrong information (pt1 #5 text not updated).
- Minor: milestone "Your lineage reached 50 living organisms" fires at founding with 55 founders; "Your legacy share passed 1%" at founding (raw share counts at once; ramp is season-only, not shown).
- Off-by-one: daily_summary alert at tick 15985 (day 333) says "Day 334: 0 born, 7 died" (pt1 #20 day-label still off?).
- After founding: raw share 1.17% instantly (rank 7/7), weighted 0.0%. Leaderboard shows weighted column; nothing explains ramp/half-weight, and nothing says I have a 20-day grace.
- 23:47 day 349: 85 alive (48 adults, 37 young), share 1.36%. World population fell 5772 -> 4650 in winter. Births in winter despite hibernation 0.6.
- Ranks tab: "Season 6 · days 301–360 · 8 days left ... You: Legacy 6th of 6" with 0.2% (raw 1.36%): ramp + half weight + missing days count 0 — none of it explained in the tab. "Largest lineage" Herdmaster2 587 vs leaderboard alive 216 (it's a season peak; label doesn't say "peak").
- 23:48 day ~357: 108 alive. Animals graze / seek mate in mid-winter despite hibernation .6 (with provisioned full fat) - none dormant. Inspector body plan says "small warm-blooded herbivore · drab" for body .5 / coloration .4.
- Genes tab gene drift lists "Offspring per birth 0.18 (-0.004)" - raw gene value, not the litter count (3) the player chose: confusing.
- BUG (med) 23:49: Market "Who your lineage can breed with" says wild Steppe antelope 22% fertile (birth mode 0.26 apart), Broker voles 18%, Meadow vole 10%, Herdmaster2's Drought Bison 33% (size 0.24) — but the design screen promised "No wild or rival species can interbreed with this design. Your genes stay your own". Designer's body-plan model (incubated = 0.50) doesn't match the founders' expressed birth mode (~0.60). Misleading promise.
- 23:49 (day 375) Renown was 189 (grant 50 + stipend + milestones?). Bought Reinforcements (10 adults) for 150 via Market > renown shop: toast "10 adults joined your lineage." Works. First click attempt via Playwright locator failed "Element is not attached to the DOM" - the Market panel is rebuilt under the cursor (pt1 #13 class of problem still present in Market).
- Late-joiner lever: reinforcement price doubles per ranked season, resets next season (day 361) -> I plan to buy again then.
- Correction: the reinforcement was bought on day 375, i.e. already in season 7 (renown 189 = 50 grant + ~3/day). Next one this season costs 300.
- Ledger income labels still a day off (pt1 #20 NOT fixed): entry at day 378 reads "Day 379 income: legacy share 3.1%".
- 23:49 day 369: 163 alive, share 2.05% (5th of 7 rows, ahead of Rulebender and Herdmaster2). Herdmaster2 now 0 alive (extinct). Origins 99.4% mine, 0.4% wild, 0.2% GeneBroker2 -> some hybridisation is happening despite the designer's "no species can interbreed".
- 23:50 day ~382 season 7 standings: legacy GeneBroker2 12.2%, PocketNomad2 5.1%, ApexHunter2 2.3%, LateComer 1.8% (4th), Rulebender 1.4%, Herdmaster2 1.2%. Largest lineage: me 2nd (237). Season 6 record: GeneBroker2 won (12.3%), I was last (0.2%) having joined with 28 days left - expected.
- World tab: Era 1 board lists me "#6, 1.5% final, weighted 0.0%" for an era I joined 29 days before its end - fine but harsh-looking.
- POSSIBLE EXPLOIT (pt1 #12 still present?): "Species founded: PocketNomad2 2 · Rulebender 4". Rulebender's Loophole Boars split off from Red fox as "(2)", "(3)", "(4)", "(5)" and each merged back into Red fox; Rulebender credited 4 species (each +renown). Steppe antelope (2)..(7) likewise split/merged repeatedly. Species list is long with repeated "merged into" rows.
- Broker voles species shows "wild 57% of alleles" - GeneBroker2's lineage merged with wild antelope.
- 23:50 day 388: alert "A Late Eggrunners of yours (#45976) is being hunted by a player's Apex Raptors (#44913)" - ApexHunter2 attacking me 36 days after founding. Attack alerts work and toast.
- FAIRNESS: the 20-day founding grace = 100 s of wall clock at this speed (day ≈ 5 s). For a human who spends 2 minutes reading the tour, the grace is over before they've looked at the map. It should be measured in wall time too (like the rally floor of 5 min), or follow head count.
- Death alerts: "Founder #45985 was eaten." - cause yes, but not by whom (player or wild).
- Toast "Mating offer 6: expired" - I never made or received an offer I saw; unclear what offer 6 was (screenshot LateComer-07-under-attack.png).
- Map chunk seams still visible (pt1 #15): rectangular light/dark blocks on terrain in the same screenshot.
- Day 393: summer, drought 48%; deaths now eaten + thirst + starved. 290 alive, share 3.3%, ahead of ApexHunter2 and Rulebender.
- 23:52 day ~396: Mates tab: incoming request #7 from GeneBroker2 (his #48373 x my #45968, price 50) — the market is alive this time (pt1 #1: zero trades). Earlier #6 (price 40) expired unseen; the only trace was the bare toast "Mating offer 6: expired". "Lapses when the season changes, in 21 days (≈ 2 min)" - good wall-time hint (pt1 #16 improved).
- Accepted #7 (to test the market; renown has little use to me now that I'm near the 300-alive reinforcement limit). Note: my #45968 is PREGNANT (list said "female (pregnant)") and the accept still went through ("organisms are 18.9 tiles apart; they must be within 1.5"). Will see what happens when they meet. Also the pair is only ~18% fertile per Market tab, but the request showed no fertility warning to me.
- 23:52 day ~412: Escorted #45968 via "Escort" (possessed; toast "The partner #48373 is out of sight: click the map to walk" though it was 3.3 tiles away), then "Complete now" -> "#7 completed · Conceived · fertility 20% · failure chance 2%". She had been listed "(pregnant)" when I accepted; by completion pregnant=false (eggs laid?). No stud-fee ledger entry seen yet (check later). Hybrid offspring incoming (dilution accepted for test).
- Summer drought: share slipped 3.5% -> 2.9%; deaths starved/thirst/cold-heat/eaten. Herd at (89,103) sits on poor ground (veg sum 6k in a 17x17 box vs ~57k at (110,68)). The founding placed me on a poor coastal peninsula.
- /api/map.png now 401 without login (pt1 #27 fixed).
- Migrated main group to (108,72) via API: reply carries an 18-point land route (pt1 #8 route now planned and sent).
- 23:53 day 394-420: summer drought (47-54%) die-off: 316 -> 212 alive (30-day vitals: 181 born, 279 died: eaten, thirst, exposure, starved). Numbers reconcile (I first suspected missing deaths; they do add up).
- SEASON 7 RESULT (days 361-420): legacy GeneBroker2 10.5%, PocketNomad2 3.4%, **LateComer 3rd 2.7%**, ApexHunter2 1.8%, Herdmaster2 1.65%, Rulebender 1.4%. Largest lineage: LateComer 2nd (319). **Stud market title: LateComer** — won by accepting ONE 45-renown offer (no seller competition at all; the title is trivially farmable by two players trading with each other).
- Ledger: "Sale: 45 renown from GeneBroker2 for offer #7, less a 5 renown market fee" - clear.
- 23:54 day ~421 (season 8 start): reinforcement price reset to 150; bought 10 more adults (alive 212 <= 300). Late-joiner catch-up lever works and is cheap: renown 330 accumulated at ~5/day.
- 23:54 day 432 autumn: Food layer toggle (top bar "Food") works and is readable (pt1 #4 fixed): most of the map centre is bare (red/orange); green at west/east edges. Share 2.4% (4th), GeneBroker2 11.2%.
- The "A pregnancy" hint card sat over the bottom-left of the map for ~10 game-minutes until dismissed.
- 23:56 day 432: alert "Group A (main) reached its migration target (108, 72); the order is done." (pt1 #8 never-ending migration: FIXED).
- day 436: Catastrophe: epidemic (through day 451). Prominent red banner over the map with days left + top-bar pill "Epidemic" (pt1 #10: FIXED, very visible). Screenshot LateComer-10-epidemic.png.
- Lineage shrinking: 166 alive day 436 (100 adults), starvation + predation (ApexHunter2's Apex Raptors attack orders on days 411/413/418). Onboarding hint cards keep popping over the map ("A death in your lineage").
- 23:55 day 452: second reinforcement this season at 300 renown (now 600 next). day 453 winter: 230 alive (147 adults), share 3.30% -> **2nd among players** (GeneBroker2 14.3% with 1,435 alive near the soft cap; Herdmaster2 2.8%, Rulebender 2.35%, ApexHunter2 2.1%, PocketNomad2 1.4%). Wild 73.7%.
- 23:59 day 485 (spring, year 5 in UI): 303 alive (204 adults), share 5.35%, 2nd of players. Winter went well (hibernators); GeneBroker2 19.9%. Herdmaster2 extinct again (0 alive, still 1.9% share via alleles elsewhere); Rulebender down to 29.
- SEASON 8 RESULT (days 421-480): legacy GeneBroker2 14.3%, **LateComer 2nd 3.0%**, Herdmaster2 2.6%, Rulebender 2.0%, ApexHunter2 1.6%, PocketNomad2 1.1%. Largest lineage: me 2nd (267). **Species founded title: LateComer** (my own lineage was detected as "Late Eggrunners (2)" splitting off from itself: +100 renown). Survival: all six tied #1 (one epidemic).
- Species farming still present (pt1 #12 not fixed): Rulebender now 5 "species founded" (Loophole Boars (2)..(6)), each +100 renown; I got one for my own lineage drifting into "(2)" without doing anything.
- Wording: "The epidemic is over: 0 fewer organisms in the world than at its start." while world population rose 4,650 -> 6,073 - misleading "0 fewer" (clamped at 0?).
- Achievements renown: Survivor +25, Longevity (1 year = 120 days) +20, Species founded +100.
- Reinforcement now blocked: alive 316 > 300 cap for reinforcement.
- 00:03 day 521 (summer): baby boom: 764 alive (349 adults, 415 young), share 8.44%, weighted 10.44%; GeneBroker2 11.44% / 13.61% (1,661 alive, past the soft cap). **A late joiner, 190 days (~16 min) after joining, is within 3 points of the leader who had 320 days head start.** The egg-laying isolated design + hibernation + two cheap reinforcements worked.
- Note: client state organisms for my lineage (461 at day ~490) vs /api/lineage/stats alive (316 a minute before) — fast growth, not a bug as far as I can tell.
- 00:05 day 550 (autumn, drought 52%): **LateComer #1 among players: 10.33% share (1,015 alive) vs GeneBroker2 9.17% (1,340)**. Season 10 (days 541-600) live legacy: LateComer 10.3%, GeneBroker2 9.4%.
- Thirst die-off in drought: 20 -> 29 -> 40 thirst deaths/day. Of 51 animals at thirst >70: 19 drinking, but 10 "graze", 9 "scavenge" (a pure herbivore, carnivory 0, scavenging?), 8 "rest", 5 flee; several at thirst 100 grazing/resting (pt1 #24-like: parched animals not seeking water).
- 00:04 day 556: Lineage panel still claims 'at most 1,500 living organisms ... births pause' while GeneBroker2 peaked at 1,538 (soft cap) - definitely wrong text.
- 00:09 day 582 (winter): **share 24.65% (weighted 26.1%), 1,001 alive (854 adults)**; GeneBroker2 9.5% (752, halved by winter), wild 59.8%. World 7,092 -> 4,524 in the winter crash. My hibernating egg-layers came through with ~1% loss. BALANCE: one winter swings the board massively; hibernation is decisive (pt1 also saw "GeneBroker halved every winter").
- **SEASON 10 RESULT (days 541-600): LateComer won the legacy crown with 18.4% mean share** (GeneBroker2 8.8%). Largest lineage title GeneBroker2 (1,538 peak) vs me 1,159.
- ~00:09 day 611 (spring, flood-year catastrophe days 600-623): share 30.07% (weighted 33%), 1,645 alive (1,002 adults; soft cap shows litter x0.73). GeneBroker2 now 0 alive (extinct or retired), ApexHunter2 1.7%, PocketNomad2 0.6%, Rulebender 0.3%. The late joiner now dominates; the established players got crushed by winter/catastrophes, not the reverse.
- /api/catastrophes confirms the epidemic's population went UP (6,399 -> 6,492): the alert text "0 fewer organisms" is a clamped number.
- Births spike: 268 births on day 610 (spring hatch).
- ~00:10 API probes: retire_lineage with wrong id -> clear "lineage 99 is not your living lineage (yours is 13)"; rally off map -> off_map refused (pt1 #25 fixed); order_move on own unpossessed -> not_possessed "must be possessed first" (fixed); unknown command -> bad_message with request id and a friendly sentence, no command list leak (pt1 #18 fixed).
- BUG (low): `migrate {group:3}` for a group that doesn't exist returns ok:true with route [[2,2]] (target (2,2) is open sea in the NW corner; no snap, no `unreachable`); clear_migration on group 3 also ok. Should refuse unknown groups.
- possess_attack from my herbivore (no fangs) on PocketNomad2's carnivore glider 3.4 tiles away: accepted (released at once). A plant-eater can be ordered to attack a predator; not necessarily wrong, but surprising.
- (Note: wall-clock stamps from 23:48 to 00:10 are approximate, +-3 min; game days are exact.)
- 00:12 day 650 (summer): 2,247 alive (1,391 adults), crowding count 1,605 > soft cap 1,500, litter multiplier 0.43 - the soft cap works as documented (no hard wall; growth slows, starvation up: 21 starved on day 648). Share 28.7% (weighted 32%). GeneBroker2 re-founded (375 alive, 4.0%), Herdmaster2 3.0%.
- 00:12 day 654: renown 3,856 and climbing ~30/day (stipend + 1/share point) with nothing to spend it on (reinforcement blocked above 300 alive, extra mating slots useless to an isolated lineage) - leader's renown piles up (pt1 #1 sink problem persists for a leader).
- GeneBroker2 has sent ~19 mating offers (price 50); most expired unseen ("#19 ... expired", "#16 ... expired"); the "A mating offer" hint card appeared after the offer had already lapsed. Offers that lapse in ~1-2 min of wall time are easy to miss unless watching the Mates tab.
- 00:16 day ~690: 2,970-2,976 alive (crowding 2,661, litter x0.09) = at the hard ceiling (2x soft cap). Share 33.5%, weighted 35.4%. One late player now holds a third of all breeding alleles and ~1/3 of all organisms on the map. BALANCE: the soft cap's 2x hard ceiling lets one lineage take ~3,000 slots in a ~9,000-organism world.
- BUG (med, confirmed with screenshot LateComer-13-cap-text-wrong.png): Lineage panel at 2,976 alive says "Each player may have at most 1,500 living organisms ... You are at the cap: no young are born until some of yours die (pregnancies wait). A bigger number no longer raises your share". All three statements are false under the soft cap (I grew 1,500 -> 2,976 and share rose 24% -> 33%; 112 pregnant).
- pt1 #13 FIXED: Mates "My adult" select kept my choice (#67402) over 8 s of state pushes (2,690 options!). The select lists every adult: 2,690 options is unusable as a dropdown.
- pt1 #23 FIXED: "Migration: Group A (main) toward 93, 64" shown while migrating.
- 00:16 SEASON 11 (days 601-660): LateComer won legacy (29.8% mean) AND largest lineage (2,598). Season 12 live (12 days left): LateComer 34.8%, Herdmaster2 5.7%, GeneBroker2 4.2%.
- 00:19 **ERA 2 (days 361-720) final: LateComer 40.0% (weighted 42.5%)** vs wild 45.7%, Herdmaster2 6.8%, GeneBroker2 6.4%, ApexHunter2 1.0%. Niche leader: forest (53%), marsh (64%). **SEASON 12 (days 661-720): LateComer won legacy (36.1%) and largest lineage (2,990).**
- Numbering: /api/eras calls it era 1 (0-based) while the World tab says "Era 2"; /api/world clock year 2 vs UI "Year 3" (consistent offset, but API consumers will trip).
- Day 745 (spring, year 7 UI): 2,663 alive, share 33.7%. Deaths now include old_age and "recessive" (1). Rulebender extinct, PocketNomad2 2 alive.
- 00:24 day 806: 2,887 alive, share 35.2% (home 33.3%, ~1.9 pts of my alleles now live in other lineages - Herdmaster2 is absorbing some), renown 9,781 unspendable. Herdmaster2 8.5% (885), GeneBroker2 4.0%.
- Season 9 (481-540): GeneBroker2 won (15.4%); I was 2nd. Season 13 (721-780): LateComer won (34.2%, largest lineage 2,962).

## Strategy summary
- Read code during the wait; picked an isolated design (warm, incubated eggs -> body-plan gap 0.35 vs every live bearer), hibernation + fur + fat (the starter's winter kit), litter 3, 55 founders (just past the flat 50), warning colouration (toxins .7), legs/hooves.
- Founded 2 days into winter -> winter provision (fed, full fat). Grew 55 -> 300 by the first summer; drought cost a third.
- Bought reinforcements three times (150, 150 next season, 300) while <= 300 alive - cheap catch-up.
- One migration off a barren peninsula toward vegetation (route sent, ended with an alert).
- Hibernators came through winter almost untouched while rivals halved -> from 2nd to 1st in one winter, then soft cap to the 2x ceiling.
- Titles: season 7 stud market (by one trade), season 8 species founded (automatic), seasons 10, 11, 12, 13 legacy (and 11-13 largest lineage), era 2 #1 player (40.0%).
- 00:30 day 875 (summer, drought 49%): adults crashed 2,641 -> 1,045 in ~70 days (old_age 9-15/day as the boom cohort ages out together, plus drought). 2,109 alive (1,064 young). Share 26.4% (weighted 18.7%), still #1; GeneBroker2 9.4%, Herdmaster2 8.7% (1,407). Herdmaster2's alleles now 1.7% of my lineage (hybridising with the Drought Bison the designer said couldn't). Season 15 live: LateComer 29.2%, Herdmaster2 9.9%, GeneBroker2 7.9%.
- 00:30 closed browser and logger. Session over.

## Final prioritised problem list (see report)
1. HIGH Founders drown on founding (7 of 55 within 30 ticks; site on a coastal peninsula among water tiles), founding in winter.
2. HIGH (fairness) Founding grace = 20 game days = 100 s wall time; no UI says it exists or when it ends; attacks came right after.
3. MED Lineage panel cap text describes the old hard cap ("at most 1,500 ... births pause ... bigger number no longer raises share") - false at 2,976 alive.
4. MED Designer's interbreeding card promised "no species can interbreed" but Market shows 10-33% fertility and origins later showed 1.7% Herdmaster2 alleles; card also calls GeneBroker2's "Broker voles" a wild species.
5. MED Species-founded farming still happens (Rulebender 5 species via Loophole Boars (2)-(6); my own lineage credited automatically).
6. MED Leader's renown piles up (9,781) with no sink; stud-market title won with a single 45-renown trade.
7. MED Balance: winter decides the board (hibernators survive, others halve); soft cap's 2x ceiling lets one player hold ~3,000 organisms / 1/3 of the map; boom cohorts then die of old age together.
8. LOW Season notice/founding hint computed once (said "winter in 2 days" during winter).
9. LOW Ledger and daily-summary day labels one day ahead (pt1 #20 not fixed).
10. LOW Epidemic end alert "0 fewer organisms" when population rose.
11. LOW migrate/clear_migration accepted for a nonexistent group (route to open sea).
12. LOW Parched animals (thirst 100) grazing/resting/"scavenging" (herbivore) instead of drinking.
13. LOW Mating offers lapse in ~1-2 min wall time and mostly expire unseen; the hint card arrives after expiry; "Mating offer 6: expired" toast with no context.
14. LOW Map chunk seams still visible; onboarding hint cards cover the map for long periods; Mates "My adult" dropdown with 2,690 entries; gene drift shows litter as 0.18.
15. LOW Species list cluttered with empty "(2)..(7)" entries; numbering 0-based in API vs 1-based in UI (era/year).
