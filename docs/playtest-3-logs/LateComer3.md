# LateComer3 log
Started 07:12:04. Joining at 07:37.

## Wait phase (07:12-07:37): reading only, no server contact

Read: BRIEF, docs/playtest-2.md, README (founding ramp, playtest-2 changes, "Playtest 2: competitive balance"), game.rs (found / winter provision), client lineageInfo.ts, seasonNotice.ts, design.ts, market.ts (shop), e2e smoke/refound/frames.

What changed, per the code/docs (to verify live):
- egg_hybrid 0.2: birth mode counts 0.8 in the fertility gap -> egg layer ~10% fertile with a close live-bearer (was sterile).
- crowding_ceiling 2 -> 1.4 (hard 2,100 on 1,500 soft), lineage_crowding 4 -> 6, crowding_hunger 0.3 -> 0.6.
- body size free only to 0.5 (3.6 kg), then 40 pts/unit; hibernation 8->12/unit, burrowing 6->8.
- dormancy_toll 0.08/season of sleep (x2 first year, up to x2 again as fat runs out); emergence_lag 6 days no breeding after waking.
- reinforcements: max 2 per player per ranked season (retired lineages count), only while <= 1/5 of soft cap alive (300), 150 renown doubling. Founding grant is only 50 renown, so a late joiner can't buy one at once.
- grace: min 30 wall-clock minutes (grace_min_minutes), shown in Lineage tab ("Newcomer grace") and inspector "protected newcomer"; Help topic explains it.
- founding ramp: Lineage tab "Season weight" row (rampText) + LEGACY_EXPLAIN sentence.
- winter provision: founding <=30 days before winter -> founders hunger 0 and fat = capacity; design-screen notice says so and refreshes.

Code-read observations:
- (Low, doc) README's scripted replay ("dominance" example) itself says the egg design "still takes the most titles" (16 of 40) and is "still the largest at the end" on every seed under the new rules - the developers' own sim says it still leads, just by less. Will check live.
- (Low) Session F (predators learning that drab toxic prey is poisonous) is referenced as not done in this branch ("belong to ... session F"); grep finds no aposematic-learning change for drab prey, so toxic-but-drab is likely still strong.
- (Question) Winter provision: founders are fed + max fat, but the dormancy toll is x2 in the first year: founders hibernating their first winter right after founding pay 0.16/season. Provision's full fat should keep the fat multiplier at 1.

Plan: found the playtest-2 design ("eggrunner", ~99.9 pts) nearly unchanged to test whether it still dominates, compare with others' strategies on the leaderboard.
- (Medium, UX/balance info) The Hibernation trait text (client/src/ui/traitText.ts:429-433) still says "deep hibernator: very cheap winter sleep" and never mentions the new dormancy toll (exposure deaths, x2 in the first year) or the 6-day emergence lag. A player pays 12/unit with no hint of the new downside.
- (Medium, balance) Help > "Toxins and colours" literally tells every player that "toxic but drab prey is not recognised, so it poisons more hunters" - documenting the playtest-2 dominant trick as advice, while the counter (session F) isn't in.
- (Low) No Help topic about the founding ramp or the winter provision; the ramp is only explained after founding (Lineage tab "Season weight", LEGACY_EXPLAIN). The design screen (design.ts) has no ramp text - a late joiner can't learn before founding that the first 10 days count partially. To verify live.
- (Potential HIGH exploit, to verify with /api/design/compatibility) The egg_hybrid fix only re-weights birth mode, but the body-plan gap is a MAX over parts plus the morph term BODY_PLAN_STEP(0.1) x eye/leg-count steps (crates/sim/src/morph.rs:779-784, mating.rs:106-110). Eye count 2 -> 5 is 3 steps = gap 0.30 = sterile with every 2-eyed animal, for 3 points (eye_count costs 1/step). So reproductive isolation (the playtest-2 dominance ingredient) can be bought back for 3-4 points regardless of birth mode.

## Joining (07:37, game day 321-332)

- 07:37 day 321 (autumn, 9 days to winter). Before registering: other players' lineages (players 1-4) have 230-310 days of grace left, Herdmaster3 89: grace min is 30 wall-clock min = ~360 game days at 5 s/day. Herdmaster3 811 alive (12.7%), all others 13-113 alive (they re-founded recently).
- Register via web client: smooth. Design screen notice: "Winter starts in 7 days. Fur, fat stores and hibernation help... Founders arriving now start fed, with full fat reserves." Clear, and it updated live to "It is winter now" on the next visit. OK.
- Playtest-2 #14 check: GET /api/leaderboard/history?genes=true without login -> 401 unauthorized. FIXED.
- Design compatibility (POST /api/design/compatibility) for the exact playtest-2 egg design: Herdmaster3 83% fertile (gap 0.005), Rulebender3 61%, GeneBroker3 56%, wild Steppe antelope 18% mean (68% of pairs fertile). => **every rival copied the egg-layer body plan** (gaps ~0.01 with each), so egg-laying no longer isolates anyone; egg_hybrid works vs live-bearers (antelope 18%).
- **HIGH (exploit) CONFIRMED: 5 eyes buys back full reproductive isolation for 3 points.** Same design with eye_count 5 (camouflage .4->0, nose .3->.1, ears .4->.35 to pay the 3 points): /api/design/compatibility -> 0 fertile with every lineage (Herdmaster3, Rulebender3, GeneBroker3, ApexHunter3, voles, antelope), best pair 0, limiting "morphology", gap 0.30. Cause: morph term 0.1 x eye-count steps (morph.rs:779), 3 steps = sterile, eyes cost 1 pt/step. It also gives "Sees all the way round, 120 deg depth, 0 deg blind" (View 100%), i.e. it is a pure upside. Steps: design screen > Builder > eye + three times > Interbreeding / compatibility. Suggest: weight the morph gap like egg_hybrid, or price eyes above 2 much higher, or make extra eyes cost upkeep.
- (Low) Builder interbreeding card for the 5-eye design says "Just out of range on average, ... some pairs may still be fertile: Herdmaster3 and Rulebender3's Steppe antelope" while the server's check says best pair 0 fertility. Card and server disagree (card ignores that the morph step is fixed, not varying).
- (Low) Design card "extra founders 25.6" for 55 founders: README says 20 free then 0.7 (=24.5+1.05). Fine, matches. Starter costs 11.5. OK.
- 07:37:40 founded "Late Eggrunners III" via the UI on day 332 (winter). 55 founders, winter provision applied.
- Lineage tab right after founding: "Newcomer grace 373 days (~30 min) left: ..." and "Season weight 20% of your share counts... rises to 100% over the 10 days after founding (day 332)". **Grace indicator: FIXED** (visible, game+real time). **Ramp explanation: FIXED after founding** (clear sentence); still nothing on the design screen before founding (Low).
- (Medium, balance) Grace now lasts 30 wall minutes = ~370 game days = 3 game years at this tick rate. Almost the whole server is permanently graced (5 of 6 lineages at join time), so attack orders are basically unusable for a player predator for much of a 70-min session.
- **MEDIUM bug: founding clutches vanish silently.** Right after founding the Lineage tab said "Eggs: 6 clutches (18 eggs) · 6 brooded · 4 brooding now"; ~9 game days later stats.eggs = {clutches 0, eggs 0}, births 0, and no alert/vitals entry mentions lost/chilled eggs. Founders of an egg-laying design founded in winter lay at once and then go dormant, so the eggs die. Expected: either no laying while dormant, or an alert "N eggs chilled". 
- Winter provision + dormancy toll: days 332-341, 5 of 55 founders died of "cold or heat" (exposure) while dormant with full fat (~9% in 9 days; the toll is documented as 0.16/season in year 1, ~5% for 9 days). Toll works and is visible as "cold or heat" deaths, but it is heavier than documented for provisioned founders. Medium-low.

## Spring of year 4 (07:40-07:43, days 361-372)

- **HIGH (late-joiner fairness): the founding home was in the snowfield at the north map edge.** Founded on day 332 (winter): founders placed at about (38, 4) on the 128-tile map, the white snow band of /api/map.png. Other lineages were around the big lake. The home search (World::pick_home) seems to avoid crowds and can't see food in winter, so a winter founder gets the worst ground.
- **HIGH: the winter provision doesn't save a winter founding; my lineage halved at the spring thaw.** Days 332-360: 9 founders died of exposure while dormant (dormancy toll). Days 366-369, at waking: 17 starved in 4 days (13 on day 367 alone), down from 55 to 25. The provision gives full fat at founding, but after ~30 days of sleep plus the emergence the founders woke empty in snow with no plants. So the provision now covers less than it promises ("start fed, with full fat reserves") because of the dormancy toll plus snowbound homes. Compare playtest 2: the same design founded in winter thrived.
- Spectate (/api/spectate, no login) still shows every graced ("hidden: true") lineage's alive/adults/look/share; only the cells are hidden. Fine by design, noted (Low).
- Spectate shows all four other players' current lineages are drab, furred, 2-eyed, 4-legged herbivores of size 1 (egg layers per the compatibility check). PocketNomad3 alone differs (cold-blooded warning-coloured 4-eyed flier). **The egg-layer herbivore is still the consensus strategy**: every experienced player picked it.
- 07:41 migrate group 0 to (41,26) (green land north of the lake): ok, route drawn in the result. Half the group had already walked south by itself.
- 07:42 Market > Renown shop > "Reinforcements (10 adults): 150 renown": one click, no confirmation, toast "10 adults joined your lineage." Price then 300, "(1 bought)". Works. (Low) 150 renown is spent with no confirm step (the nameplate etc. too?).
- Renown: 65 at founding (grant 50 + ~15), 170 by day 366: ~3/day. A late joiner affords the 1st reinforcement ~35 days (~3 min) after founding and the 2nd (300) not within a season. **Reinforcements limit: FIXED** (max 2/season, the price doubles, shown in the UI). The price still resets every ranked season (60 days = 5 min).
- 07:43 day 402: claim_territory (41,27) r12 -> ok, water_share 0, drinkable_tiles 39 (the water check now reports). "home range" badge with offline protection left shown in the Lineage tab: clear.
- Day 390-435: **megadrought catastrophe** (drought 94-100%). The red catastrophe banner over the map shows "29 days left (through day 435)" plus the effect. OK (playtest-2 #10 drought warning: FIXED for catastrophes; the amber drought notice exists in code for ordinary droughts).
- Recovery: day 389 67 alive -> day 407 102 alive (40 adults, 62 young, 30 eggs in 10 clutches). The Lineage tab "Eggs" row (clutches, brooded, about to lay) is clear.
- (Low, still present) The top bar says "Year 4 · day 407" while /api/world says year 3 (0- vs 1-based years, playtest-2 smaller item).
- 07:44-07:46 days 414-436: megadrought; Herdmaster3 1,898 -> 1,251 alive but share 25.8 -> 27.5% (wild crashed more). Mine 127 -> 154 (my small territory with ponds held; 1 thirst death). Rank 4/6, share 2.2%.
- Season 7 (days 360-419) won by Herdmaster3 (legacy 14.4%, largest 1,919). Season 6 too.
- 07:46 day 436 (season 8): reinforcement price back to 150, "0 bought": bought the 2nd via API (ok, balance 96). So a late joiner gets 1 per season in practice (income ~3-4/day); the 2-per-season cap mostly binds the rich.
- Exposure deaths in late summer/autumn (days 431-435, 1-2 a day) at drought 96%: heat? The cause label "cold or heat" doesn't say which (Low).

## Second winter (07:47-, days 451-480)

- Day 452 -> 468: alive 140 -> 119, all dormant; deaths are almost all "exposure" (1-3 a day; the 5 juveniles all died). That is ~14% in 17 days, a pace of ~25-30% per winter, against the documented dormancy_toll 0.08 per season (x2 in the first year, up to x2 again as fat runs out). The hungriest sleepers show hunger 46-63 mid-winter. **Medium (balance/doc): the toll is 2-3x what the README implies**; it plus the 6-day lag makes hibernation an expensive trait now (12 pts/unit). The playtest-2 "hibernation wins winters" part of the dominant strategy is clearly nerfed, maybe overcorrected for young/first-year animals.
- Board at day 464: Herdmaster3 1,251 -> 341 alive (megadrought + winter), share 9.1%; PocketNomad3 (cold-blooded flier) 7.5%, rising; Rulebender3 4.8%; me 3.5% (126); GeneBroker3 0.6%; ApexHunter3 extinct (0). Big swings: no single strategy holds the board through drought + winter.
- Playtest-2 spot checks (07:49): migrate to group 2 that doesn't exist -> no_such_group (FIXED); migrate off map -> off_map; set_view at (100,100) far from my animals -> out_of_sight (FIXED: the view can't reach the whole map now); mating_prefs with blocked [0, 999] -> ok, blocked [] (invalid players dropped, FIXED-ish, though it answers ok without saying they were dropped: Low).

## Spring year 5 (07:50, day 487)

- Woke with 114 -> 103; starvation again at the thaw (3+3 starved on days 486-487), hunger spread 10-100. /api/map.png shows nearly every lineage (white dots) packed on the north shore of the big lake where my territory is; the east side is empty green. Migrated group 0 to (70,42) (route ok).
- Pattern worth a note (Medium, balance): with hibernation everyone wakes at once on the same pastures; the emergence starvation plus the toll makes the thaw the deadliest time for hibernators now.
- 07:52 day 516: alert "Group A (main) reached its migration target (70, 42); the order is done." but at that moment only 3 of 61 adults were within 8 tiles of (70,42); the median adult was ~29 tiles away (most stopped around (55,36) to graze while starving). **Playtest-2 #13 (migrations end early): still present / partly fixed** (Medium). Steps: order a hungry group ~35 tiles; read distances when the "reached" alert lands.
- (Low/Medium, exploit-ish) Alerts "While you were away, your home range kept hunters off the members inside it for 13 simulated days" arrived while I was actively playing through short HTTP/WS calls: a power user who never holds a socket open is "offline" all the time and gets offline protection while giving orders.
- Season 8 ended: me #4 legacy, #4 lineage, **#1 survival** (alert text clear).
- Thaw recovery: 77 (day 502) -> 135 (day 513): ~10 born a day once fed. Claimed territory (60,38) r18: drinkable_tiles 257.
- 07:53 day ~518 (season 9): renown had jumped to 523 (season 8 survival title?). Bought 2 reinforcements (150 + 300); a 3rd -> limit_reached "at most 2 reinforcements a ranked season (retired lineages count)". Cap works.

## Summer-autumn year 5 (07:53-07:56, days 527-555)

- Peak 219 alive (day 539), then drought 73-81%: last 30 days 77 thirst deaths (of 38 born... most were juveniles), 6 exposure, 5 starved. My territory (claimed with "drinkable_tiles 257") was on streams that dried up. (Medium) The claim's water check counts water at claim time only; no warning when a territory's water later dries (a drought + territory still kills herds: playtest-2 #10 partly fixed - the claim reports water now, but not over time).
- Day 553: all 151 already dormant ~17 days before winter (they fattened and bedded down early; the dormancy toll then runs longer).
- Herdmaster3 rebounded from 273 (day 477) to 1,685 (day 539) in one spring/summer: litters at full size until 1,500 are huge. PocketNomad3 (cold-blooded, warning-coloured, 4-eyed flier) grew to 1,121 and 11.3%.

## Machine restart (~07:57) and restore

- ~07:57 the machine restarted (coordinator notice): my browser/scripts/timers died; server back on 8092 with the world restored from its save (day 566). My last watcher line before it was 07:56:18 day 564, so ~2 game days were lost at most.
- After restart my saved token -> 401 "missing or unknown session token": sessions made since the last graceful stop are lost on a crash (documented in README). A web player would be bounced to the login screen; not data loss, but (Low) the message could say "the server restarted, please log in again".
- Restore check (07:58, day 573): lineage alive 138 (145 before at day 564, minus winter exposure deaths: consistent), territory (60,38) r18 kept, home range kept, grace 133.6 days left (consistent with ~day 707 end before the restart), ramp full, renown 306, alerts (50) and legacy history (244 points from day 332) intact, seasons 7-9 results intact, achievements still firing ("Lineage longevity ... 2 years", +20 renown).
- (Low) /api/lineage/stats `vitals` (births/deaths per day) restarted: only days 571-573 after the restore. The Genes tab's "Births and deaths, last 30 days" chart loses its history on every restart.
- (Low, wording) Season 8 end alert said "you placed ... #1 of 6 in survival", but /api/seasons lists only legacy and lineage winners for season 8 (no survival title awarded). The alert implies a title I didn't get.
- (Low) Ranks tab: "Your record: 4 seasons ranked · 0 legacy crowns · 1 title · Survival 1st (season 8)", but the Hall of fame "Most titles" line (Herdmaster3 13, Rulebender3 7, GeneBroker3 1) leaves me out and /api/seasons has no season-8 survival winner. Title counting is inconsistent between "Your record" and the hall of fame. No console errors in the Help/Ranks pass after the restart.

## Year 6 spring (08:00, day 603)

- Winter 3: 145 -> 108 (exposure ~1/day again, plus old age). PocketNomad3's cold-blooded warning-coloured flier went through the same winter flat (1,136 -> 1,134) and leads at 22.3%; Herdmaster3 754 -> 632 (11.7%).
- **Balance (Medium): the new dominant candidate is the cold-blooded brumator.** The dormancy toll is only for *warm* hibernators (README: "A warm hibernator asleep in winter dies..."), and brumation costs 6/unit against hibernation's 12. Playtest 2's warm-hibernator dominance is gone, but the cost has moved onto one branch only: cold-blooded designs now winter for free. Worth checking whether brumators should pay a comparable toll.
- 08:00 claim_territory (55,46) r16 on the big lake's NE shore: drinkable_tiles 597 (permanent lake water). 63 of 103 animals immediately "defend" (chasing strangers out of the crowded shore territory): territory defence wastes a herd's time where every lineage packs together (Low/Medium).
- Season 11: reinforcement 1 bought (150); 2nd refused insufficient_funds (296/300) with a clear message.
- 08:02 day 623: 2nd reinforcement of season 11 bought; 234 alive, 9-26 born a day.
- **Bug (Low/Medium):** preview_mating of my 5-eyed male with a Herdmaster3 female (81813) -> `problem: {code: incompatible_body_plans, detail: {genes: []}, message: "different body plans:  differ too much to conceive"}`: an empty gene list and a double space where the trait name should be, because the gap comes from eye count (morphology), not a body-plan gene. `body_plan_gaps: []` too. The Market's lineage list does say "eye or leg count 0.30 apart", so only the preview misses it. Steps: found a 5-eye design, then preview_mating with any 2-eyed animal.
- possess_attack on rival/wild animals 10+ tiles away -> target_out_of_sight (clear message).

## Summer year 6 (08:03-08:06, days 635-662)

- Peak 282 alive (day 635, 179 young), then a drought (71-74%) crash to 113 by day 662: in 15 days 61 thirst, 23 starved, 7 eaten, inside a lakeshore territory that had 597 drinkable tiles at claim time. /api/map.png shows both lakes have receded and every lineage is packed on the strip between them.
- **Playtest-2 smaller item "parched herbivores scavenge instead of drinking": STILL PRESENT (Medium).** At 08:06 an adult at thirst 93 and a juvenile at thirst 99, both at (55,46), had action "scavenge"; most of the thirst deaths are juveniles. Steps: GET state in a drought, sort my organisms by needs.thirst, look at the actions.
- Share stays ~1.8-2.0%; I've held rank 4/6 since day ~414.

## Year 7 (08:08-08:13, days 698-758) and wrap-up

- Day 706: my newcomer grace ran out (it lasted ~374 days, ~31 wall minutes). The "Newcomer grace" row simply disappears from the Lineage tab; (Low) no alert says the protection has ended.
- Season 10 legacy PocketNomad3 (lineage Herdmaster3); season 11 both PocketNomad3.
- Winter 4: 116 -> 96. Spring: 2 reinforcements in season 13 (days 721+), 295 alive at day 758 (178 young).
- Final board, day 758 (08:13): wild 61.1%, **PocketNomad3 23.0% (2,099 alive = the 1.4x hard cap)**, Herdmaster3 9.2% (1,146), Rulebender3 3.8% (1,675), **LateComer3 1.7% (295), 4th of 6**, GeneBroker3 1.1%, ApexHunter3 extinct.

### Verdict on dominance
- The playtest-2 egg/toxic-drab/warm-hibernating burrower **no longer dominates**: same design (plus 5 eyes for isolation) founded late in winter peaked at 282, never above 3.6% share, rank 4/6. Causes seen: snowbound winter home, the thaw starvation, the dormancy toll (~25-30% a winter, all ages), the 6-day lag, droughts. The 1.4x ceiling was reached only by PocketNomad3.
- Every player except PocketNomad3 still copied the egg-laying drab herbivore body plan, so egg-laying no longer isolates anyone (egg_hybrid works: 18% with antelope, 56-83% with rivals).
- The new leader is a **cold-blooded, warning-coloured, 4-eyed flier** (PocketNomad3): flat through winters (no dormancy toll for cold-blooded sleepers), at the hard cap by day 758, legacy winner of seasons 10-11. Watch it: it may be the next single dominant strategy, since the toll only taxes warm hibernators.
- The 5-eye isolation trick works (3 points -> sterile with everyone) but isolation alone didn't win here.

### Late-join experience
- Founding ramp: explained clearly after founding (Lineage "Season weight", 20% -> full over 10 days); nothing on the design screen beforehand (Low).
- Grace indicator: clear (days + real minutes, inspector pill, Help topic); it lasts ~31 wall minutes = ~370 game days, too long at this tick rate (Medium); no "grace ended" alert (Low).
- Winter provision: the notice is accurate and refreshes, but the provision didn't save the founders: home placed in the snowfield, 9/55 exposure deaths in the sleep, 17 starved at the thaw, 55 -> 25 (High for late joiners founding in winter). Founding clutches laid in winter vanished silently (Medium).
