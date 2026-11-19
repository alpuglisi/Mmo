# ApexHunter12 log (predator persona, playtest 12)

Start 20:30 UTC, world day ~4 (spring y0). Server 8105, main at 078e247. Scripts and raw logs in playtest12/apex12/ (watch.log = alerts + 30 s summaries, cmds.log = commands).

## 20:31-20:33 (day ~22-34) Register, design, found (UI via Playwright, 5 fps cap, DSF 1)
- Registered in UI as ApexHunter12 (player 5). No page errors, console errors or failed requests.
- Design "Gloamfang Pack": warm, live, litter 3, 20 founders, carnivory 1, pack_instinct 1, group_tolerance .9, body .45, muscle .9, fangs .9, claws .6, keen_nose .8, legs .45, fat .5, fur .3, food_caching .5, large_eyes .3, aggression .85, obedience .9, boldness .7, wanderlust .3, foraging .6, no toxins. 41.3/100 pts. Founded as lineage 7 on day ~22.
- **Low (wording, stale after graced_hunting):** designer grace line: "Newcomer grace: 128 days ... Other players can't order attacks on your animals, nor yours on theirs, until it ends." With round 12's `graced_hunting` a graced lineage CAN attack (first attack ends its grace), so "nor yours on theirs" is now wrong. Same stale comment on `StateView::grace` in protocol.rs ("and it can't attack theirs").
- **Low:** budget box says "A hunter needs speed, strength or bite to catch prey: raise the traits that suit it, or try a preset" with muscle .9, fangs .9, claws .6, legs .45 already raised (hint seems unconditional or threshold too high).
- Toast stack was empty 6 s after founding (no landing toast visible in my capture; may have been dismissed earlier/timed).
- Survey day 34: PocketNomad12 70 at (100,28), GeneBroker12 70 at (28,36); my pack centred ~(60,20). Herdmaster12 outside 64 tiles.

## 20:33-20:36 (day 36-77) Cohesion, and my grace ended by my own pack
- **Pack cohesion 0.3 (pt11 #17 "pack scatter"): STILL PRESENT for an unrallied pack (Medium).** Single-link clusters (8 tiles) of my animals, no rally/orders given: day 36 (11 days after landing) 28 animals in [15,10,+3 singles], half within 12 tiles of the median; day 74: 52 animals spread x8-103, y1-116 (nearly the whole map), only 5 within 10 tiles of the median, largest cluster 16, then 7,6,5,4,4,3,2. Cohesion 0.3 slows it a little at best; a pack still scatters over the map in ~50 game days.
- **pt11 #6 "carnivore's own grace starves it": FIXED (mechanically), but with a new UX gap (Medium).** Day 74, with 75.9 grace days left (state.grace), I got "Your newcomer grace has ended: other players' animals can now attack yours, and yours theirs." then 2 s later "Your hunters were poisoned biting GeneBroker12's Brokers Kin ... have learned they are poisonous". I gave no order: a scattered hunter of mine bit an (apparently unprotected) GeneBroker12 animal on its own and that waived my grace 76 days early (apply.rs `grace_waived`). Expected per README. Problem: the end-of-grace alert is the same text as a natural expiry and doesn't say WHY it ended ("your hunter #X attacked GeneBroker12's #Y; a graced lineage's first attack on a rival ends its grace"). A player who never pressed attack loses 76 days of protection silently; an unrallied, scattered pack decides it for you. Suggest: name the cause in the alert, and maybe only count player-ordered attacks or warn in the designer/grace line that your pack's own hunting ends grace.
- No grace-ending warning 3 days before (M15 warning) came either, since the waiver was instant (fine, but adds to the surprise).
- GeneBroker12 animals showed `protected` on day ~52 (first sighting), so they were graced then; by day 74 my hunter could bite one, so their grace must have ended early (they ended it, or bigger founding). Not verifiable from my side.
- Founder #1461 died of thirst day 68 (landing area dry?), founder starved day 70/77.

## 20:37-20:42 (day 84-135) Rally works; split to test raids on a graced egg herd
- Rally (62,76) day 84 (52 tiles away): honest "None of your animals sees this circle" + distance warning. By day 118 the pack was one cluster (59 of 60 within 8-link, p90 11 tiles from centre), hunger median ~66-70, 256 wild in view. Rally gathers well; without it, scatter (above).
- Low: rally reply says "None of your animals sees this circle" though one of my 58 animals stood within 20 tiles of it (scatter): may be correct for sight radius; just noting.
- Other logs confirm my pack's bites lined up with GeneBroker12/PocketNomad12's grace ends (~day 74), so my grace waiver was legit. GeneBroker12 logs that a pending-agreement partner (#1611, asleep) of PocketNomad12 was eaten by my pack on day 81 ("sleeping walkers aren't protected") - pt11 #3 fix (sleep no longer an open-ended shield) works from the predator's side.
- Info: anonymous /api/world lists `protected: [{player, lineage, days_left}]` (Rulebender12 noted): a pack knows to the day when each newcomer becomes huntable. Low/leak, probably intended.
- Day 131 survey: GeneBroker12 130 at (20,44), Rulebender12 60 at (92,92) (graced, incubated eggs, per their log), Herdmaster12 400 at (28,108) with territory r14 (graced 22 more days at day ~128). Split (31/31) and rallied group B to (90,90) beside Rulebender12's graced egg herd to test the clutch-grace rule.

## 20:42-20:46 (day 135-172) Herdmaster12's herd feeds group A; reinforcement clearance
- Rulebender12 re-founded (10 founders, no grace) and wrapped up, so the graced-egg-herd raid test at (90,90) had no subject; group B found no rivals there. /api/world `protected` empty from day ~160 (all graces over). Raid-vs-grace can't be tested live by me unless a later founder (LateComer12) lays eggs.
- Group A (48) wandered into Herdmaster12's flier herd (138 unprotected in view, Herdmaster's territory (45,100) r20): hunger median fell 66 -> 23 by day 162-168. "poisoned biting Herdmaster12's Frostwing Drift" day 151, right as Herdmaster12's grace ended (lines up).
- **pt11 #17 "reinforcements kept 8 tiles from rival animals": FIXED (as far as I can see).** Day ~168, group A at (37,86) amid Herdmaster12's animals: `reinforce` -> at (21.88,83.36), nearby_count 1, note "your group stands in or beside another player's territory or animals, so they landed on the nearest clear ground, 16 tiles from it". Snapshot ~4 days later: 9 of 10 newcomers 11-13 tiles from the nearest rival animal (one had wandered to 3 tiles). Price 150 -> next 300.
  - Low (still present from pt11): float noise in `at` (21.8799991607666, 83.36000061035156); "16 tiles from it": "it" reads as the territory/animals but is the distance from my group.
- Group B (39, hunger 61) sent by migrate from (93,92) to GeneBroker12's ~680-animal egg-laying starter herd at (30,42): honest 47-tile dry-stretch warning (drought 49%), journey 9.5 days.
- Survey prices rise: 32 -> 35? -> 39 (now 39 after use).

## 20:46-20:51 (day 172-245) Nest raids on GeneBroker12's eggs; winter crash; UI check-in
- Group B reached GeneBroker12's egg herd (30,42) day 174: "prey ... (662 animals) feeds about 33 of its 41 hunters". Group B 41 -> 61 by day 213, 12-17 of them in `scavenge` (the clutch-raid action) at any time. Raids on an UNprotected rival work as in pt11.
- **Cohesion with a home ground is fine; without one it scatters (Medium, persona).** Day 213: group B (home ground from its migration) one cluster of 60/61; group A (no order, rally expired) x2-72, y43-119, clusters 23/11/9/7/5/3. So `pack_cohesion` 0.3 alone isn't what holds a pack; home ground/rally is.
- Winter year 2 crash: 138 (day 220) -> 87 (day 236). Group B "has fallen from 51 to 25 adults" (GeneBroker12's herd asleep/no clutches in winter).
- **pt11 #16 "toxin deaths never trigger a decline alert": FIXED.** Day 230: "Your lineage is dying off slowly: 138 → 98 alive over the last 10 days (29% fewer); 39 died (32 of hunger, 7 of toxins (poisonous prey); 28% of the 138)." 
  - Low: the follow-up "Your lineage is still crashing: 138 → 92 ... 28 died of hunger, thirst or cold in those 10 days" (days 233, 236) drops the toxin count again (inconsistent with the first alert).
  - Low (still present): "dying off slowly" for a 29% fall in 10 days; "Day by day it looks small." wording; the 3 alerts within ~6 game days.
- Hunger alerts (ungraced now) correctly carry no grace sentence. I could not see the new grace sentence in a hunger alert live because my pack's own bite ended my grace on day 74, before any hunger alert. (Code: game.rs ~6436 has it.)
- UI check-in (Playwright, 20:48-20:50, 2 runs): Lineage tab: "Alive 118 · crowding 95 / 614 (world share)", Groups A/B with homes; legacy 3.0% (#4 of 5 players); Genes tab death causes (prey fighting back, starved, poisoned, old age); Alerts list shows the hunger and group alerts above. Ranks: Season 4 legacy 4th. World: species list. No page errors, console errors, failed requests or HTTP >= 400 in either run. Screenshots shots/ApexHunter12-ui1-*.png, ui2-*.png.
- UI attack (possess #14746, Attack button, click a Herdmaster12 marker 1.6 tiles away): toast "#14746 is hunting #21205; hungry pack-mates nearby join in. #21205 looks poisonous to your kind: ... pass it up 87% of the time" then alert "Attack over: your #14746 killed #21205." Works. Screenshot shots/ApexHunter12-ui2-attack.png.
- Bought 3 provisions (40 each, fed 20 each). Migrated both groups to the wild antelope/vole country in the SE ((84,98), (78,90)); both warn they lie inside Herdmaster12's territory (70,102) r20 - honest.
- Ranks: Season 4 legacy GeneBroker12 7.1%, Herdmaster12 6.8%, PocketNomad12 5.4%, me 3.0%.

## 20:51-20:55 (day 245-295) Spring recovery in the SE
- Both groups reached the SE wild country; "flood year" catastrophe days 241-263 (+1,239 organisms). Hunger median 29 by day 288; pack 86 -> 95 -> 109 (with 10 reinforcements).
- Low (alert repeat): group B's "The prey Group B can take ... feeds about 25 of its 29 hunters" came on days 263, 277, 281 with nearly identical figures - the arrival-type prey note keeps repeating while nothing changed.
- 2nd reinforce (300) day ~289 with group A inside Herdmaster12's herd (194 rival animals in view): at (90.91,82.52), nearby_count 2, "landed on the nearest clear ground, 20 tiles from it". Snapshot a few seconds later: newcomers 2.7-9.0 tiles from the nearest Herdmaster12 flier (median ~7). Landing point likely 8+ clear, but scatter (up to 3 tiles) plus moving fliers put several within 8 within a day. Fine for gameplay; Low note that the "8 tiles" is only at the landing instant.
- Merged B into A (34 moved, no false "died out" alert in the following minutes).
- Started a hunt helper (ApexHunter12-hunt.mjs, PID 18249): every 25 s one hungry adult possesses and attacks the nearest WILD antelope (non-toxic) within 6 tiles, released next round.

## 20:56-21:02 (day 326-385) Era 1 ends; Herdmaster12 runs away; offline/grace targets gone
- Hunt helper: 7 orders on wild antelope, 7 kills ("Attack over: your #20873's pack killed #33502"). Low: "Possessed #20873 is parched (thirst 75): it is foraging on its own until you give it an order." after the helper's possess - fine.
- Era 1 (day 360): "you finished #4 with a mean 1.8% ... (2.2% at its end)"; niche prize "over era 1, on average 50% of the breeding alleles among carnivores were yours" (+40 renown).
- **Balance (pt11 #2 share runaway): looks present again.** Day 370 leaderboard: Herdmaster12 1,414 alive = 31.2% (11.8% on day 326) while the wild fell 70.4% -> 55.0% in ~45 days; Herdmaster12's alive count was flat (1,456 -> 1,414). Share rose 2.6x with no head-count growth = attrition of the wild, not growth. (Watching: share_memory smoothing should damp this.)
- Split 52 to the north (68,22) to test offline protection on Rulebender12's (offline since ~20:42) herd - but Rulebender12 is down to 6 animals; no test possible. Group B redirected to GeneBroker12's egg herd (26,34): migrate warns "The way crosses PocketNomad12's territory: its holders harass intruders." (good).
- LateComer12 founded at era 2 start (graced, 44.8 days left on /api/world at day ~375), a live-bearing flier per their plan: so no graced clutches exist on the map to test the raid-vs-grace rule live. Pending: an attack order on a graced LateComer12 animal (should be refused) if they come into view.
- Market: GeneBroker12 lists a dam and a stud at (16,16) (fee 10, dormant). Protected-flag logger (ApexHunter12-prot.mjs, PID 19529) running for agreement walkers.

## 21:02-21:09 (day 385-470) Plateau at ~105-124; the pack ate GeneBroker12's herd
- Pack flat at 105-114 for ~100 game days (days 330-430) with low hunger (median 24-29): births only ~2/day. **Sex ratio skew (Medium for a pack; pt11 Low "early sex ratio skew" still present and persistent):** day 428: 89 adults, only 31 female (35%), 7 pregnant. A live-bearing litter-3 pack can't grow on a third of its adults.
- Low: daily summary lists "1 attrition" with no explanation (the Genes chart legend says "attrition (disease, accidents)", the summary doesn't).
- Herdmaster12 share swings: 11.8% (d326) -> 31.2% (d370) -> 11.4% (d421) -> 25.8% (d465) with alive 1,379-1,818. The wild's share swings 55-78% in 50-day steps. Share is volatile, driven by wild crashes/rebounds rather than by players (pt11 #2 partly).
- GeneBroker12 (egg herd) 278 -> 69 and its log blames winter + "ApexHunter pack inside my herd"; its listed dam "Clover" and a pending-offer animal were eaten by my pack (unshielded, as documented). Group B (84) had nothing left there by day 465 (hunger 61); moved it to wild prey at (10,74).
- Hunt helper stopped at 21:08 (19 kills of 22 orders, all on wild antelope). Low (UX): each possession for an order produces "Possessed #X is starving (hunger 77): it is foraging on its own until you give it an order." - the attack order IS an order; the alert fires right after a successful attack ends.
- "poisoned biting Rulebender12's Gale Wardens II" day 465: the offline Rulebender12 (11 animals) - my group B passed its herd. No way to see offline softening from my side.
- No own-kind kill notice so far (no "eaten by your own" deaths in any summary): pt11's cannibalism not reproduced this time, so the new explanation is unverified live.
- No market-shielded animal (protected after grace) ever came into view (prot.log empty since start).

## 21:09-21:12 (day 470-485) Starvation dip; UI check 3
- Pack 124 -> 96 (days 465-482): starvation (group B's egg herd gone) + toxins. "Dying off slowly" alerts on days 472, 475, 478 (every 3 days): each counts toxins now ("21 of hunger, 3 of toxins (poisonous prey), 1 eaten by hunters") - pt11 #16 FIXED. Low: "6% fewer" alongside "25 died ... 21% of the 121" in one sentence is confusing (net vs gross); still "Day by day it looks small".
- "poisoned biting LateComer12's Latefeather Omni" on day 473: LateComer12's grace had ended (~day 420; /api/world protected empty by then). Consistent: no bites on LateComer12 while graced (none reported before).
- UI check 3 (21:10): Lineage tab "Alive 96 · crowding 78 / 932 (world share)", groups A 36 at (84,98), B 60 at (10,74). Legacy table day 482: wild 56.5%, Herdmaster12 32.7% (1,637), LateComer12 6.6%, me 1.6% (96), PocketNomad12 1.5%, GeneBroker12 (shown with its «Stud Farmer» nameplate). No rival animal within 6 tiles of a matured hunter in the page's view for an attack test. No page errors, console errors, failed requests or HTTP errors. Shots ApexHunter12-ui3-*.png.

## 21:12-21:16 (day 485-530)
- Low: "#34989, the animal you were possessing, died of old age" on day 506 - my hunt helper had been killed while holding it possessed; possession outlives the socket/session that took it (a closed tab can leave an animal possessed indefinitely, foraging "on its own"). Probably intended; noting.
- Group B (71) now inside LateComer12's flier herd at (10,74): 205 LateComer12 animals in view, none protected; hunger 39. Pack 96 -> 99.

## 21:17-21:24 (day 590-661) End
- API attack test day 598: `possess_attack` on my own animal -> `own_target` "organism 40496 is yours; you can't attack your own animals"; on an (ungraced) LateComer12 animal -> ok with warning "#52034 looks poisonous to your kind ... (pass it up 95% of the time)", then "Attack over: your #61742 killed #52034". Fine.
- Final leaderboard day 660: wild 59.8%, Herdmaster12 1,358 (21.8%), LateComer12 589 (6.6%), **ApexHunter12 96 (1.8%)**, PocketNomad12 0.7%, GeneBroker12 0.6%, Rulebender12 0.3%. Season 11: #3 of 6 legacy, #4 lineage (seasons 9 and 11 #3 legacy). Era 1: #4 (mean 1.8%). Niche prize: carnivores era 1. Peak 136 alive (day 219). Renown 1,700 unspent (piles up, pt11 Low still).
- Pack never passed 136 (pt11 peak 250): ~100 for the last 400 game days, held back by a 1:2 female:male adult ratio, toxic prey (every player herd is toxic), and the egg herd I fed on collapsing.
- Watcher, protected logger and hunt helper stopped by PID; no browser open.
