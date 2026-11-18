# PocketNomad5 playtest log (phone newcomer, iPhone 13 emulation, 5 fps, DPR 1)

Started 16:46 UTC (wall). Server day 12 year 0 at start.

## 16:46 (day ~22, spring y0) Register + designer, starter design
- Registered "PocketNomad5" via phone UI, fine. Tour? (see below)
- Builder fired POST /api/design/compatibility with name "Unnamed design" on load -> 200 (pt4 Low "empty name 400 on load": FIXED).
- [Low] Preview line says "small (adult ≈ 4.2 kg)" while the Body size slider line says "medium ... adult ≈ 4.2 kg". pt4 Low "starter preview small for 4.2 kg": STILL PRESENT.
- [Medium] The default starter design shows "⚠ Interbreeds with Steppe antelope" right away: server-measured 20-22% fertile (16 sampled), advice "move birth mode at least 0.11-0.12 further". docs say the starter "is its own species, only somewhat fertile" — a newcomer who keeps the default is warned their legacy may go wild. Will verify whether the lineage stays its own species over time.
- [Low] The interbreeding box shows two different percentages for the same species in one card: "Measured by the server ... Steppe antelope (wild) 20% fertile" and then in the bullet "Steppe antelope (70 adults, 23% fertile)". Both re-roll on every preview (22/17, then 20/23). A newcomer cannot tell which is the real one. Server-measured fertility IS shown (pt4 builder-vs-server disagreement: partly fixed - the server figure is there, but the bullet's estimate still differs).

## 16:47-16:50 (day 30-55) Founded starter "Pocket Grazers" (20 founders, server-chosen spot)
- Founding spot: herd landed on a peninsula/lakeshore in the east, beside water (drought 47-65% at the time). No deep-water founders seen. 2 drowned on day 47 (starter is a non-swimmer living on a lakeshore) - Low/Medium, a newcomer can't do much about it.
- Designer showed drought warning ("Keep the founders near water") once named. Good.
- Tour: 9 steps, fine on phone; last step's Finish card sits over the toolbar/top notice (ok).
- Top bar "Y1 d30" vs API year 0 (pt4 Low day numbering: still present, cosmetic).
- [Medium] Eggs: lots of losses "eaten by egg raiders" (2-5/day, days 31-49) and "chilled" from day 31. "Why are my animals dying?" help (NEW) exists in Help > How the world works, and covers thirst/hunger/winter sleep/predators/poisoned/drowned/chilled eggs, but does NOT mention "eaten by egg raiders" (the single biggest loss for the starter) nor "attrition (disease, accidents)" / "exhaustion" which appear in my charts/alerts.
- [Low] Same death labelled two ways: alert "Founder #1730 died of exhaustion" vs daily summary "1 died (1 attrition)" and chart "attrition (disease, accidents)".
- [Low/Medium] Alert text bug: "GeneBroker5 asks to mate their #2078 with your #0#0" - offer with accepter's pick shows "#0" and the id link duplicates it ("#0#0").
- [Low] Incoming request "price 3" doesn't say who pays whom; no fertility/compatibility shown for their animal on the request card. Accepting gave +2 renown (fee?) without explanation.
- Accepted GeneBroker5's #2080: "partner is about 75 tiles W of yours", lapses in 30 days (≈2 min). 75 tiles in 2 minutes looks impossible without Complete now; watching.

## 16:50-16:53 (day 55-72) phone checks, species
- pt4 #15 bottom map edge: FIXED. Panned to the bottom edge: map ends at y 521, above the Possess FAB (527) and hint (568); edgePad.bottom 93. Shot 12.
- pt4 #15 chooser width: FIXED. Tap in a herd of 24 at zoom 7: "Here:" chooser is 374 px wide at x 8-382 (scrolls inside, scrollWidth 746), × visible at right. Shot 13.
- Two notices: drought notice (expanded, ~100 px) + "Season 1 has ended: you placed #4 of 5..." line shown together; with both the top 225 px of a 664 px screen is covered (toolbar+notices). Will test with a catastrophe.
- [Medium] pt4 #6 starter species: PARTLY FIXED. My starter is no longer filed under the wild Steppe antelope (birth mode 0.5 vs antelope 0.835), but /api/species files "Pocket Grazers" under Herdmaster5's "Thundering Plains" (species 4, founded day 25, namer Herdmaster5) because Herdmaster5 also founded the starter design 4 days earlier. Every newcomer who keeps the default design joins whoever founded the default first: no founding credit, no naming. The designer did say "Players' species in range: Herdmaster5's Thundering Plains (100% fertile). Chance meetings mix your genes with theirs both ways" but not that I would join their species.
- [Low] Designer said starter interbreeds with Steppe antelope ~14-22% "move birth mode at least 0.11 further" though antelope's species birth_mode is 0.835 vs starter 0.5 (gap 0.335 > 0.30, which the same card says is sterile). Contradictory advice.
- Mating with GeneBroker5 completed within ~1 min (their side used Complete now?): "Conceived · fertility 8% · failure chance 0%" - [Low] the two numbers read as contradictory to a newcomer.
- [Low] Incoming-request animal picker lists juveniles (#4123 female gen 1 is a juv) among "my pick" options.
- Lineage tab: grace "20 days (≈ 2 min) left", clear. Eggs line now splits "now 7 clutches (17 eggs) · ... · 33 lost since founding (6 chilled, 27 raided)" (pt4 Low "Eggs line mixes current and lifetime": FIXED).

## 16:53-16:56 (day 89-112) Grace ends, wolves arrive, first winter
- Grace ended day 89 (alert shown). From day 90 to 101: 80 -> 30 alive, 44 "eaten", 1 starved (founder #1748 day 98 - founders' "free first winter" covers winter sleep only, fine), 1 winter sleep (day 83/92 - founder in its first winter? ids not given), 2 attrition/exhaustion. All juveniles gone (35 -> 0), gen 2 wiped, max_generation dropped 2 -> 1.
- Six of ApexHunter5's "Ash Wolves" (lineage 6) were among my herd (inspector: #1923 · ApexHunter5, scavenge). No attack-order alerts came, so this is instinct hunting by a player predator - pt4 "player predators starve": the predator now clearly eats (FIXED from the prey side).
- [Medium] Newcomer UX: alerts only say "Founder #1740 was eaten. (and 5 more like it)" - never by whom (wild fox or a player's wolf), and nothing suggests it's a rival player or what to do. The death chart says "eaten". "Why are my animals dying?" says "Predators: stay in groups, add speed, armor, toxins or camouflage" - the last four need a new design; the herd already was in one group. A note "eaten by ApexHunter5's Ash Wolves - move away (Migrate) or ..." would help.
- [Medium/balance] Starter herbivore lost 63% in 12 days right as grace ended + winter started (the two coincide for a lineage founded ~day 30: grace of 60 days ends right at winter). Brutal first experience.
- [Low] Tip "A mating offer. Another player wants to mate..." popped again at day 112 though I'd already handled two offers on day ~55.

## 16:54-16:58 (day 120-154) Starter collapses, retire, re-design
- Starter: 80 -> 13 by day 125 (spring y2). Eggs: 67 lost since founding (40 chilled, 27 raided); only 1 generation survived. Starter is a poor newcomer experience in this world (predator wolves + chilled eggs).
- Help "Why are my animals dying?" (NEW): present, readable on phone (Help "?" > How the world works > collapsible topic). Missing causes I actually saw: "eaten by egg raiders", "attrition (disease, accidents)" / "exhaustion"; uses "Hunger" while charts say "starved". Discoverability: only via "?" in the top bar; nothing in the Genes "Births and deaths" chart links to it. [Low/Medium]
- [Infra, mine] My Playwright driver crashed when two dialog handlers both accepted the retire confirm; relaunched, logged in again in a fresh context: the tour did NOT reappear (pt4 Low "tour reappears in every new browser context": FIXED).
- Retire via Lineage tab: confirm says "re-founding within a ranked season's length founds half the founders" (a "season" is 60 days, but the designer says the window is 375 days ≈ 30 real min - [Low] inconsistent wording). After retire: "Mating offer 5: failed (the lineage has no living animals left)" fine.
- Half-founders (pt4 #9 "not shown"): FIXED in the designer: "Re-founding soon ⚠ Founding now gives 10 of your 20 founders, costing 14.0 points. Because your last lineage retired on day 138: re-founding within 375 days (about 30 real minutes) founds half the founders. The full design founds again from day 513". [Low/Medium] It charges the full point cost for half the founders (71.4 pts for 10 of 20 fliers) - and the budget/total line still says "20 founders, about 63 kg of animals".
- [Low] Designer interbreeding names species inconsistently in one card: measured "Loophole Grazers (Rulebender5) 20% fertile · Steppe antelope (wild) 19%", bullet "Rulebender5's Steppe antelope (339 adults, 15% fertile)" and "Herdmaster5 and GeneBroker5's Thundering Plains" vs measured "Brokerline (GeneBroker5) 90%".
- Flier design (cold, live birth, wings .8, toxins .8, colour .8, brumation .6, scales .4, body .45 = 3.1 kg): 71.4 pts incl. a new "cold blood 20.0" line. Server measured: no species fertile with it. Steppe/water warnings: none shown (no steep-bank case to test); compatibility request fires with no name (so the water warning could show before naming: code path FIXED, not observed live).

## 16:59-17:03 (day 165-210) Winter advice, flier founded, reinforcements
- pt4 #18 winter advice by thermoregulation: FIXED. With days_to_winter ≤30 the designer's season notice reads, for Cold-blooded: "Winter starts in 29 days. Brumation and fat stores help a cold-blooded lineage through it (not hibernation: ...); cold-blooded animals don't breed in winter. Founders arriving now start fed, with full fat reserves. Founders sleep their first winter free..." and for Warm-blooded: "Fur, fat stores and hibernation help ...". Toggling thermo kept my other sliders. Shot 29.
- Founded "Pocket Gliders" day 185 (cold flier): got 10 of 20 founders as the designer said (half-founders shown: FIXED). Landed in a dry area among a big wild/herdmaster herd (drought 63-70%). New grace 49 days. All 10 founders "dormant" by day 201 (autumn, 9 days before winter) with thirst ~24 rising; no births.
- [Low] Right after founding, tip "A death in your lineage" popped (leftover from the retired lineage) over the map; the "A mating offer" tip also keeps reappearing after being answered.
- Market tab on phone: readable; compatibility list says every species sterile with the flier (as the designer said: consistent - builder vs market FIXED for this design).
- Bought Reinforcements (10 adults) 150 renown via the shop; confirm dialog clear ("10 young adults copied from your own"), next price shows 300, "1 bought". Lineage 10 -> 20 immediately. Will watch whether they survive (pt4 #17 unprotected reinforcements).

## 17:03-17:05 (day 210-232) Severe winter catastrophe, possession
- Catastrophe "Severe winter · 20 days left (through day 240)" + "Dry winter: 36% of the land has no open water". pt4 #15 two notices: FIXED/partly - the second notice starts folded ("More"); together they cover y 94-234 = 140 px of the 576 px map (~24%, was 36%), plus any toast below them.
- Possession on phone via Possess FAB: works. Possessing a brumating flier says "Possessed #21158 has bedded down for the winter with its kind: it sleeps where it is and takes no orders until it wakes." (pt4 #16 possession blocking hibernation: looks FIXED - the animal sleeps).
- [Medium] Phone overlap: after tapping into the herd and possessing, the "Here:" chooser (y≈470-510) lies on top of the possession bar (y 440-514) and hides its Attack / Inspect / Release buttons until the chooser's × is pressed. Shot 34 vs 35.
- [Low] While possessing, "Release #21158" appears twice (bar button + big red FAB); "Attack" is offered for a sleeping possessed animal.
- Flier through the severe winter: 20 -> 20 (zero deaths days 210-230) incl. the 10 reinforcements; thirst 29 while dormant, health 100. Meanwhile Herdmaster5 293 -> 127, Rulebender5 and GeneBroker5 -> 0. Cold brumating flier is again the "flat through winter" design (pt3/pt4 #1 dominance - see later whether it booms).
- pt4 #20 /api/spectate without login: FIXED (200, but every player lineage's cells are empty; only wild herds shown).
- [Medium] Name filter gap: /api/spectate lists a lineage named "wild" owned by GeneBroker5 (0 alive by day 249). A player lineage called "wild" reads like the wild ecosystem in leaderboards ("wild" is also the leaderboard row name for the ecosystem). pt4 #12 listed "wild" as accepted for nameplates; for lineage names it is still accepted.

## 17:07-17:10 (day 250-290) Flier wakes; species naming
- Fliers stayed dormant ~day 201-255, woke by day ~265 (bask/drink), first pregnancy day 268. 20 alive, 0 deaths through winter + severe-winter catastrophe.
- "Pocket Gliders" is its own species (id 8, namer me) - a non-starter design does get its own species and the lineage name (as designed).
- [Medium/High] Species-name filter gap: World tab > "Name it…" opens a native prompt "A name for "Pocket Gliders" (1 to 40 characters; costs 300 renown)". I typed "wild": accepted, 300 renown charged (423 -> ~123), and /api/species + World tab now list species 8 as "wild · herbivore · PocketNomad5 100% of alleles". A player species can be named exactly like the wild ecosystem ("wild" is how leaderboards name it). Steps: found any lineage that forms its own species, World tab, Name it…, enter "wild".
- [Low] With the native prompt, an accidental OK on an empty field silently does nothing (fine) but there's no in-page form; on phone a native prompt is OK.
- pt4 Low "named species still offers Name it…": FIXED (button gone after naming).
- Severe winter summary in World tab: "population 5193 → 2548 · 3 of 4 player lineages survived".

## 17:08-17:10 (day 292-311) Migration, growth
- Tools > Migrate… on phone: "Tap the map to set the migration target." then tap -> "Adults migrating toward (93, 53)" with a drawn route. Clear. No water warning for the flier (pt4 Low "migrate water warning for fliers": not reproduced).
- Flier boom after waking: 20 -> 74 (52 young) by day 309; 1 "winter sleep" death on day 306 (autumn, early brumation).
- [Low] Family tab: "Species: wild" (result of the name-filter gap above).
- Leaderboard day 311: wild 94.3%, Herdmaster5 2.3% (191), GeneBroker5 1.5%, ApexHunter5 0.8%, me 0.7% (76), Rulebender5 0.5%. No player design dominates so far; the wild ecosystem holds 94%.

## 17:11-17:14 (day 326-365) Second winter, era 1 ends
- Bought Provision (40 renown): confirm "Your lineage's 20 hungriest or thirstiest adults get 50 off their hunger and thirst, once", toast "20 adults fed and watered." Clear.
- Second winter (day 330-360): fliers 106 -> 104, 2 winter-sleep deaths. Flat through winter again.
- Era 1 final: wild 88.3%; 1 Herdmaster5 3.2%, 2 LateComer5 2.7%, 3 me 2.5%, 4 GeneBroker5 2.1%, 5 Rulebender5 0.6%, 6 ApexHunter5 0.5%. Niche leaders all wild. No player design dominates.
- [Medium, fairness] Species takeover: LateComer5 founded a near-copy cold flier ("Late Skylarks Five", founded ~day 300) and it joined MY species 8 (which I founded on day 186 and named - for 300 renown). By day 361 /api/species shows species 8 with namer 6 (LateComer5) and dominant LateComer5 52% (117 vs my 104 adults). Founding credit/naming credit moved from me to a later copier with a few more animals. World tab: "wild · herbivore · ~218 adults · LateComer5 52% of alleles". The same thing that put my starter under Herdmaster5.
- [Low] Hint strip "Tip: A pregnancy. One of your organisms is pregnant..." stayed on screen for 5+ minutes (days 295-365) under the map; it doesn't go away on its own.

## 17:15-17:18 (day 365-385) Market on phone
- Market compatibility: LateComer5's Late Skylarks Five "fully fertile · drift risk high", Rulebender5's Loophole Fliers 20% (81% of pairs can conceive); all else sterile. Listings of GeneBroker5's sterile studs still show a "Buy for 2" button next to "Sterile with your lineage" + "Can't buy: pick your adult" [Low: a sterile listing shouldn't offer Buy].
- Listing on phone: "Choose an adult to list…" opens a searchable list of buttons; picked a male, fee 3, List -> appears under My listings. Works.
- [Low/Medium] pt4 #17 still present: my listed stud #25084 is one of the bought reinforcements (ids 25079+ arrived day 209) but shows "gen 0, a founder · 224 days" - reinforcement copies are labelled founders, generation 0, with an inherited age. The reinforcements themselves all survived (no losses in the 10 days after arrival, inside my grace).
- Directed mating within my lineage from the inspector ("Mate this one…"): partner list shows distance only, picking one shows "Compatible: fertility 100% · within both preferences · 10 tiles apart ... walk one to the other first". Send request -> "They are 10.1 tiles apart..."; "Walk to the partner" possesses the female and walks her; she was pregnant on arrival. Works. [Low] the hint says "possess one and click the map" on a touch phone (should be tap).
- Territory on phone (Tools > Territory…): "Tap the center…", "Tap the edge… (radius up to 24 tiles)", then "Group A (main) holds a territory of radius 12.1 at (87, 53)" + "Territory: Only 56 tiles of drinking water for 475 animals in a drought (59% dry here): it may not last." Good feedback.

## 17:18 (day 411) Flier boom
- 104 adults after winter -> 477 alive (198 adults, 279 young, 56 pregnant) by day 411, ZERO deaths of any cause since day 344. Leaderboard: me 4.86% (#1 player), GeneBroker5 4.75%, LateComer5 3.37% (the other flier, same species), Herdmaster5 2.26%. The cold toxic bright flier again tops the players within ~1 year of play (watching whether it runs away like pt3/pt4).
- 17:21 (day 441): 1,062 alive (from 104 adults at day 365 - 10x in 75 game days, ~6 real min). Only deaths: 3 old age. Renown 861 (+~10/day). Leaderboard day 432: me 7.7%, GeneBroker5 6.2%, LateComer5 4.7% (also flier), Rulebender5 2.9%, Herdmaster5 2.8%, ApexHunter5 0.9%. [High, balance] pt3/pt4 #1 "cold brumating toxic flier dominates": STILL PRESENT in effect - a newcomer copying the flier from the docs goes from 10 founders to the top player within ~25 real minutes, with no predation and no thirst/hunger deaths. Phone rendering of 1,000 animals at 5 fps: fine, no page errors.

## 17:21-17:24 (day 450-474) Winter 3, nameplates
- Winter 3 toll: ~1-7 winter-sleep deaths/day on ~1,090 (≈ 8-9% over the winter, as documented). Share passed 10% (day 454). Leaderboard day 459: me 13.1%, GeneBroker5 7.3%, LateComer5 6.9%, Rulebender5 5.5%, Herdmaster5 2.3%, ApexHunter5 0.9%; wild 64%.
- Renown pill opens the Market sheet (pt3 fix still works).
- Nameplate: confirm dialog now shown ("Buy your nameplate for 500 renown? amber, "Game Master": it is cosmetic...") (pt4 Low no-confirmation: FIXED). "Game Master", "GM", "wild" all refused: "That title can't be used: that looks like a staff or system name" (pt4 #12: FIXED for nameplates). So the species-name filter (which accepted "wild") is weaker than the nameplate filter - inconsistent.
- [Low] The refusal is a toast that stacks ("(and 2 more like it)"); nothing inline next to the title field.
- Data: "Listing #17 closed: #25084 is no longer alive" - the reinforcement stud I listed died ~day 443 (likely old age: it was created with an inherited age of ~224 days).
- [Low] Shop "Reinforcements ... For lineages of at most 300 (yours has 1028)" fine; but my renown keeps climbing (1,387) with nothing useful left to buy at this size - pt4 #25 "no renown sink" for big players too.

## 17:26 (day 496) Fliers lead the server
- Leaderboard day 496: me 18.8% (1,368), Rulebender5 10.8% (1,065, "Loophole Fliers": cold, size .25), LateComer5 10.4% (736, cold flier in my species), Herdmaster5 1.8%, ApexHunter5 1.0%, GeneBroker5 0.4%; wild 56.9%. The top three players are all cold-blooded fliers; every warm design is below 2%. [High, balance] cold flier dominance: STILL PRESENT (pt4 #1).
- Spring births day 494-496: 120/125/112 a day; first starvation deaths (1-2/day) appear only now.
- [Medium] Because of the name-filter gap, the biggest species on the server (1,589 adults, mine + LateComer5 + GeneBroker5's "Broker Wings") is listed everywhere as "wild".
- [Low, wild side] /api/species groups lineage "Red fox/wild" (an omnivore) inside the Meadow vole species.
- 17:27 (day 520): 1,832 alive (789 adults, 1,043 young), "breeding slowing: litters ×0.9", home-range territory explained well. Season 8 legacy crown won (10.9%); season 9 leading at 18.7%. Ranks show "6 ranked" but list 5 rows (ApexHunter5 missing) [Low].
- [Medium] Nameplate filter gap still exists in another spelling: Rulebender5 shows « G.M. » on the leaderboard and Ranks tab (my "GM" and "Game Master" were refused).

## 17:28-17:31 (day 520-548) Territory famine, recovery
- [Medium] My territory (radius 12, claimed day ~300 when I had 475) held 1,800+ animals: days 526-536 starved 20-78 a day, 1,941 peak -> 861 (-55%). Alerts were good and specific: "Your lineage is going hungry: on day 527 67 starved ... their ground can't feed them. Move groups ..." and "253 of the 967 animals holding territories (Group A) are starving or parched: they obey the claim and stay inside while the ground runs out." But a newcomer who claims a territory once never gets asked again as the herd grows 4x; the claim keeps them penned in. The territory food check happens at claim time only (pt4 #22-ish).
- Attack alert names the hunter now: "#50859 of your Pocket Gliders is being hunted by a player's Ember Jackals (#45326)" (good; only for attack orders, instinct kills still say just "eaten").
- Abandon territory -> "Group A (main) gave up its territory"; the group then shows "at home near 93, 53, where its migration arrived" - a migration target from day 295 (250 days earlier) still holds it (pt4 Low "held-at-home ... never expires": STILL PRESENT).
- Split on phone: "Group A (main) split: 556 went to Group B, 557 stayed"; Tools menu lists both groups; Migrate… B onto a lake: "that is water or impassable; the group goes to the nearest land". Clear.
- Season 9 ended: #1 of 6 in legacy, #2 in lineage.
- 17:30 (day 568): Rulebender5 24.0% at the hard cap (2,100, cold "Loophole Fliers"), LateComer5 13.3% (2,070, cold flier), me 10.5% (1,722 after my famine), every warm design ≤1.9%. Wild 48%.
- No page errors, console errors or failed (4xx/5xx) requests in the second browser session (since 16:56).

## 17:33 (day 592)
- [Medium] Group B's migration (Migrate… day ~548 to (70, 86), moved to nearest land) ended "last order: migration to (70, 86) ended: stalled" - the flier group of ~560 never got there (pt4 "fliers stalled after 25 of 70 tiles": STILL PRESENT). No alert told me it stalled; I only saw it in the Lineage tab's group line.
- [Low] pt4 Low "Litters are still full size at ×0.9": STILL PRESENT - the Lineage tab said "litters ×0.9" on day 520 while the hunger alerts of days 524-527 say "Litters are still full size, so the population cap isn't the cause".
- Standings day 592: Rulebender5 22.0% (2,027), LateComer5 18.2% (1,912), me 17.4% (1,654, litters ×0.5 at crowding 1,451/1,500). Three cold fliers hold 58% of all alleles between them; the three warm designs ~1.3-1.5% each.

## 17:38 (day 666) Wrap-up
- Final standing: #1 player, legacy share 22.6% (weighted 25.4%), 2,090 alive (hard cap 2,100). Rulebender5 16.1% (2,100), LateComer5 16.1% (2,100), GeneBroker5 5.6% (1,370, "Broker Wings" - also winged), Herdmaster5 1.6%, ApexHunter5 1.4%; wild 36.6%. Season 11 at day ~640: 2nd in legacy, 1st (tied) largest lineage. Record: 10 seasons ranked, 2 legacy crowns (seasons 8, 9), 5 titles. Era 1: #3 (2.5%).
- Closed browser and watcher (own PIDs only).
