# ApexHunter11 log (predator persona, playtest 11)

Start 16:09 UTC, world day ~37 (summer y0, drought 66-91%). Server 8103. Scripts and raw logs in playtest11/apex11/.

## 16:10-16:13 (day 37-44) Register, design, found (UI via Playwright, 5 fps cap, DSF 1)
- Registered in UI as ApexHunter11 (player 5). No page errors, console errors or failed requests.
- Design "Duskmaw Pack": warm, live, litter 3, 20 founders, carnivory 1, pack_instinct 1, group_tolerance .9, body .45, muscle .9, fangs .9, claws .6, keen_nose .8, legs .45, fat .6, fur .5, food_caching .5, large_eyes .3, aggression .85, obedience .9, boldness .7, seasonal_coat .4, hibernation 0. 43.7/100 pts.
- Designer food hint (carnivore-aware): suggests ~12 founders; drought warning 79-91%.
- Founded day ~43 as lineage 8. Landing toast: "Your founders landed around (93, 12): the nearest rival animal (GeneBroker11's) is 10 tiles away; Herdmaster11's territory is 32 tiles away." plus the drought provision text.
  - Low: the landing toast text appears twice in the toast stack (same sentence, once alone, once prefixed to the drought-provision text).
  - Observation: a predator lands 10 tiles from a graced herd (GeneBroker11, protected=true). Fine for me, but GeneBroker11 will feel it once grace ends.
- Watcher apex11/ApexHunter11-watch.mjs (PID 17550), scan loop apex11/ApexHunter11-loop.sh (PID 17551).

## 16:13-16:19 (day 44-110) Scatter, starving inside my own grace, end grace
- **Pack scatter (pt10 #7, pack_cohesion off): STILL PRESENT.** 20 founders landed "around (93,12)"; by day 54 (11 days) the 19 animals span x70-118, y0-45. Rally (112,30) pulls them in only while it lasts.
- Rally replies: "prey ... feeds about 0 of the group's 25/29" while I had 19 alive (the group count seems to include something else - young/unborn? Low, unclear). Float noise in rally `radius` 7.385164737701416 (pt10 Low float noise: still present).
- Migrate preview to the wild prey in the south (66,70): honest 61-tile dry-stretch warning; I stayed north.
- **New, Medium (balance/UX): my own newcomer grace starved my pack beside unprotected rival herds.** Grace is two-sided for hunters (an attack order says `own_grace`: "your lineage is still under its newcomer grace; it can't attack other players for another 62 days"), and the pack's own hunting skips rival animals too. From day ~80 the rivals' grace was over (GeneBroker11 77 seen, Herdmaster11 96 seen, none `protected`, all within 10 tiles of my hunters), yet my pack starved 24 -> 16 (lineage_stats: starved every 1-2 days, days 81-100) while the wild nearby (refuge, ramping, burrowing voles) fed "0 of the group". I ended grace (`end_grace`) on day ~101: starvation stopped at once (0 deaths days 101-108) and helper kills began ("Attack over: your #5141 killed #7366"). None of the hunger alerts ("dying off slowly ... Move the pack to where prey is, split it, or fatten it before the cold") mention that the grace is what keeps the pack off the herds around it, nor that "End my grace now" exists. A carnivore founder's grace is a trap, not a protection. Suggest: for a carnivore, the hunger alert should say "your grace keeps your hunters off rival herds (N within reach); end it to hunt them", or make grace one-sided for pure carnivores.
- **Own-pack cannibalism (Medium, maybe by design):** day 90 "Founder #3042 was eaten by your own Duskmaw Pack." A pack of 19 eating its own member while starving; no explanation in the alert.
- Low wording: every hunger alert ends "Day by day it looks small." - reads oddly; it seems to mean "each day's loss looks small".
- Low: sex ratio at founding/early life skewed: Lineage tab shows ~12 males vs 4 females at day 109.
- Lineage tab (UI, day 109): legacy board Herdmaster11 7.1% (403), Rulebender11 5.7% (190), GeneBroker11 1.2% (41, down from 86 peak), PocketNomad11 1.2%, me 0.4% (16). Group A card shows rally lapse "19 days (≈ 2 min)". No page errors.
- Protected flags (pt10 shield fix): rival animals under grace showed `protected: true` in my view (scan day 45: GeneBroker11 51/51 protected); after grace none. Attack refusal for own grace is explicit. Still to test: market-agreement protection (`protected` code).

## 16:19-16:23 (day 110-125) Reinforcement, survey, market
- Renown 96 -> 222 by day 116 (prizes/income). Reinforce (150) day ~117: landed at (111.45,32.18) "group 0, nearby_count 15" - with my group, no rival territory near (survey: Herdmaster11's territory (114,55) r16, 23 tiles south). No note needed. Float noise in `at` (Low). Next price 300.
- Survey (32): GeneBroker11 20 at (108,36) right inside my pack; Rulebender11 190 at (60,20) (+10 at (52,20)); Herdmaster11 390 at (116,52) with territory r16.
- Market: one listing (GeneBroker11 dam #1036, fee 12) shown at "x 80, y 16". GeneBroker11's log: "Listing #1 closed: #1035 was eaten by ApexHunter11's Duskmaw Pack" (day 101, right after my grace ended): stud listings are not shielded (by the new rule only agreements are), and the alert names the killer.
- **Nest raids ignore grace (code + circumstantial live evidence), High (agrees with Rulebender11's code read).** `World::scent_clutch` (decide.rs ~1778) and `apply::raid_clutch` (apply.rs 1133) skip only the hunter's own owner's clutches and given-up species; neither checks `grace_spares`/the market shield/offline protection, and raid_clutch runs for any animal with carnivory >= 0.3 standing on a clutch tile. Live: my pack landed 10 tiles from GeneBroker11's graced egg-laying starter herd on day 43 and GeneBroker11 logged "Your eggs are failing: 8/8/21 eggs eaten by raiders" on days 45-55 (both of us under grace; wild foxes could share the blame - I can't see raids from my side). A graced pack that may not hunt players still eats their eggs, and a graced egg herd's eggs aren't protected.
- 16:20 (day ~128) UI attack (Playwright: possess from map, Attack button, click a GeneBroker11 marker): toasts "#3051 is hunting #1043; hungry pack-mates nearby join in" then "Attack over: your #3051 killed #1043." and a folded "Attack over: your #3039 killed #5926. (and 1 more like it)" - folding works (pt10 alert flood, attack side). No page errors. Screenshot shots/ApexHunter11-uiattack2.png.

## 16:21-16:24 (day 128-165) Spring: pack grows on the herds
- Pack 27 -> 45 by day 158 (hunger mostly < 40) feeding on Herdmaster11's (toxic, warning-coloured) fliers and GeneBroker11's herd; helper kills ~1 per 3 game days plus the pack's own hunting. Poison deaths occasional ("#3039 ... was poisoned").
- GeneBroker11 86 (peak) -> 41 (day 109) -> 15 (124) -> 8 (158): my pack sat inside its small starter herd from day 101. Herdmaster11 meanwhile 390 -> 925. So the pack is strong against a small drab herd next to it, weak against a big toxic one.
- **Pack scatter, still present:** when the rally lapsed (day ~150), spread went x105-118 (day 124) -> x35-118, y17-75 (day 158).
- **pt10 Low "rally into a rival territory has no warning": FIXED.** Rally (114,52) said "That ground lies inside Herdmaster11's territory: its holders harass intruders, and your animals there are the first they meet." plus prey "637 animals ... feeds about 35 of the group's 43".

## 16:24-16:26 (day 165-200) Plateau inside the big toxic herd
- Day 197: pack 42 (hunger ~45 median), all inside Herdmaster11's territory with 254 of its fliers in view; Herdmaster11 925 -> 1,287 (10.4%), Rulebender11 461. The pack holds level on a toxic herd but can't grow on it: the pt10 "toxic herds can't feed a pack" is only partly eased (kills every ~3 days by order; poison and "killed by its prey fighting back" losses).
- Season 3: #3 of 5 legacy, #4 of 5 lineage.
- **Alert repeats (pt10 #12, my side): partly fixed.** No territory/flood swings for me, attack results fold. But "Your lineage is dying off slowly" came on days 85, 87, 90, 98, 101, 104 (every 2-8 days), and day 104's text was word-for-word day 101's ("20 → 16 alive ... 5 died ... (25% of the 20)") - stale figures resent (Low; also seen in pt10).

## 16:27 (day ~205) Reinforcements from inside a rival territory
- **pt10 #9 "reinforcements land inside a rival territory": FIXED.** With my whole pack rallied inside Herdmaster11's territory (survey: territories (116,56) r22 and (95,58) r22; my group at (108,52)), `reinforce` replied: at (102.45,29.11), nearby_count 0, note "your group stands in or beside another player's territory, so they landed on the nearest clear ground, 24 tiles from it". The landing point is ~8 tiles outside both circles.
  - Low wording: "24 tiles from it" - 24 is the distance from my group (108,52), not from the territory (~30 tiles from either centre, ~8 from either edge); "it" reads as the territory.
  - Low: the ten land alone (nearby_count 0) 24 tiles from the pack - fine for a herd, for a pack it means a 24-tile walk through open ground; maybe say "they will walk to the group".
- Survey day ~205: Herdmaster11 540 + 960 in two territories, Rulebender11 210. Herdmaster11 ~1,500 vs my pack 42.

## 16:28-16:33 (day 220-285) Growth to 76; split; heading for the egg herd
- Pack 42 -> 67 (day 248) -> 76 (day 277), inside Herdmaster11's territories (Herdmaster11 1,021-1,697, 18-23%). Many of my animals show `flee` (15 of 67) - the territory holders harassing intruders, as the rally warning said.
- Low, STILL PRESENT (pt9/pt10): "Attack over: your #19534 was poisoned before it reached #18121." (odd: poisoned by what, before reaching?).
- "Your hunters were poisoned biting Rulebender11's Thornback Tumble ... have learned they are poisonous" arrived on day 254, right after my group B first walked near Rulebender11's herd - consistent with a real bite.
- Day ~250 split (65 -> 33 + 32) and migrated group B to (40,40): migrate warned the destination's forage "reaches into Rulebender11's territory at (54, 24) (radius 20...)" - good. Arrival alert + carnivore prey alert: "The prey Group B can take in or near its home ground at (40, 40) (259 animals) feeds about 5 of its 32 hunters for good" (toxic Rulebender11 herd).
- pt10 Low "about about N days": FIXED ("about 19 tiles (about 2 days)").
- Day ~280: PocketNomad11's warm EGG herd "Pocket Emberkin" (toxins 0.6, burrowing 0.6, per their log) is at 661 alive around (43,121). To test "egg herds now feed a pack", I sent group B (43) 64 tiles south to (42,110).

## 16:33-16:38 (day 288-352) Group B on the egg herd: nest raids verified
- Group B reached (42,110) day 288: "The prey Group B can take in or near its home ground at (42, 110) (838 animals) feeds about 30 of its 45 hunters". In view: PocketNomad11's lineage 9 (236-244 animals).
- **pt10 #7 / v89 nest raids - "egg herds now feed a pack": VERIFIED (FIXED).** Group B 43 -> 53 (day 316) -> 66 (day 350), hunger median ~48, with 18-42 of my animals in `scavenge` at any time (the action scent_clutch sets) amid a toxic herd my hunters mostly pass up. PocketNomad11's log for the same days: "Duskmaw Pack reached my herd and eats 4-8 a day; egg raiders eat 7-38 eggs a day". Pack total 76 -> 113 (day 350), past 100 for the first time (pt10 peak 112).
- **"Pack too strong / wipes out egg herds": NO.** PocketNomad11's egg herd kept growing with my pack inside it: 651 -> 831 -> 1,000 (day 337), 993 alive / 13.0% on day 350. The pack takes <1% a day. Balance looks right from my side; if anything the pack is still small (1.5% share vs Herdmaster11 26.8%).
- Medium (corroborating PocketNomad11): their side gets "being hunted" and "eggs are failing" alerts alternating every ~5 s real time for <1% daily losses - alert flood for the prey, pt10 #12 family still present on the prey side.
- Reinforce (150) day ~351: landed at (43.99,115.70), "group 1, nearby_count 9" - with group B, i.e. 10 fresh hunters dropped ~5 tiles from the centre of PocketNomad11's herd. Allowed because the herd holds no territory; legal by the rule, but a predator can buy 20 hunters straight into a rival herd that hasn't claimed ground (Low/Medium balance note). Float noise in `at` (Low).

## 16:38-16:41 (day 352-395) Era 1 ends; pack peaks
- Era 1 (UI World tab): me #5, era mean 0.7%, final 1.9% (GeneBroker11 #4 with the same 0.7% mean and 0.4% final - tie shown with equal rounded numbers, Low). Ranks season 7: Legacy 4th of 6 (1.9%), Largest lineage 4th (134). No page errors.
- Pack 113 (day 350) -> 136 (day 393): above playtest 10's peak (112), still far below playtest 9's 261. Group B (87) inside PocketNomad11's egg herd; 87 of 136 in `scavenge` in early winter; hunger rising (median ~50, a few at 100) as clutches stop in winter.
- PocketNomad11 993 -> 836 (winter + pack). Herdmaster11 1,902 (27.8%).
- Species list shows "Skyglass Swarn" (Rulebender11) and "Skyglass Swarq" (GeneBroker11): one-letter variants of Herdmaster11's "Skyglass Swarm" pass as species names (pt10 #4 near-copy names, still present; others' tests).
- LateComer11 founded day 362 "2 tiles" from Rulebender11 (their log); far from my pack (no new lineage within my view). Pack-clearance for foundings not testable from my side this time.
- Possess-attack on a vole: `sheltered` with the lag explanation (fine).

## 16:41-16:48 (day 395-480) Summer of year 4: pack 205
- Heat deaths in summer ("Heat is killing your animals: 3/5/3 died of heat on day 400/403/408") - fur .5, fat .6; fair warning text.
- "poisoned biting LateComer11's Latewing Drift" on day 424, just after LateComer11's grace ended (day 421.6): first-bite lessons line up with grace again. Same for GeneBroker11's Burrow Brokers (day 442).
- Reinforce day ~435 landed with group B (57.2,106.2) "nearby 29". Survey: Herdmaster11 three territories (116,56) (95,58) (70,68) r22 with 560/710/840; PocketNomad11 1,050 at (36,92); Rulebender11 410 + 690; LateComer11 510 with territory (17,31) r14; GeneBroker11 140.
- **Pack size: 125 (day 433) -> 205 (day 477) -> 209 (day 480)**, gen 9. That's nearly double playtest 10's peak (112) and close to playtest 9's 240-261. Group B (100) lives off PocketNomad11's egg herd, group A (105) roams. Hunger mostly 30-60.
- **Not too strong:** PocketNomad11 grew 985 -> 1,691 (18.8%) over the same days with 100 of my hunters inside it; Herdmaster11 1,775 (25.7%). The pack is 2.4% share.
- **Pack scatter still present:** spread x2-125, y6-123 (the whole map) on day 477 with no rallies.
- Watcher restarted 16:44 (PID in apex11/watch.pid) with a PROTECTED logger (first sighting of any rival animal flagged protected) and the hunt helper skipping protected targets. No protected animal seen after the grace periods so far: no agreement walkers have crossed my view.

## 16:49-16:55 (day 485-510) UI check; merge test
- Lineage tab (UI, day 499): "Alive 219 · crowding 181 / 1,372 (world share)" - **pt10 Low "crowding header 653 / 1,500 while the world-share cap binds": FIXED** (header names the world-share figure). Groups: A 117 roaming, B 102 "at home near 42, 110, where its migration arrived". Year 5 spring table: me #5 (2.6%, 221). No page errors.
- Ranks/legacy list shows Rulebender11's nameplate «Herdmatser» (letter swap of Herdmaster11) - pt10 #4 near-copy names STILL PRESENT for transpositions (Medium; Rulebender11's own test).
- Market: "wild Red fox: sterile (diet too different to breed, even in the closest pair) · sterile now; drift could make them fertile in time" - **pt10 Low "sterile ... drift risk high": FIXED** (reworded).
- **pt10 #10 "merging a group gives a false 'Group X has died out'": FIXED** for me: merge_groups B -> A (105 moved) on day ~500; no group alert at all in the next ~8 game days. Re-split at once (108/107: west half around the egg herd, east half).

## 16:55-16:58 (day 510-567) Pack 250, 5% share; the wild collapses
- Pack 215 -> 250 (day 562): "Your lineage reached 250 living organisms", "Your legacy share passed 5%". Leaderboard day 564: Herdmaster11 1,308 (30.9%), PocketNomad11 1,058 (24.3%), wild 1,007 (20.1%), Rulebender11 486 (10.8%), LateComer11 529 (8.1%), **me 250 (5.3%)**, GeneBroker11 21.
- Still not wiping the egg herd: PocketNomad11 fell 2,017 -> 1,009 (day 499 -> 567) but so did everyone in this dry stretch (drought 66%, Herdmaster11 1,880 -> 1,258), and the wild fell hardest.
- **Observation, Medium (balance): the wild collapsed in year 5 with no catastrophe** (`/api/catastrophes` history empty): /api/spectate day 567 - voles 566 (1,342 on day 60, ~1,200 adults at day 376), antelope 108 (~755 adults at day 376), newts 139 (~1,024 on day 60), foxes 208. Wild share 34.6% (day 499) -> 20.1% (day 564). Players' herds now outnumber the wild 3:1; the refuge (full strength since day 120) doesn't stop it, so it's probably grazing competition + drought rather than predation. For a predator this leaves only player herds (toxic) and eggs to eat.
- Still present (Low): "Attack over: your #53896 was poisoned before it reached #53362."
- No PROTECTED sighting yet (agreement walkers never came into my view).

## 16:58-17:02 (day 567-625) Winter of year 5: poisoned down to 180
- Pack 250 (day 562) -> 184 (day 620) -> 179 (day 625). Daily summaries days 596-625: 2-7 deaths a day, mostly **toxin** (e.g. "Day 615: 0 born, 7 died (6 toxin, 1 starved)"), some starved/old age. The wild is gone (antelope ~100), Herdmaster11 crashed 1,308 -> 501, so the hungry pack eats toxic herds and dies of it. My hunt helper's orders account for 16 poisonings of ~100+ toxin deaths (helper switched off at 17:01).
- **New, Medium (warning gap):** a 28% fall in ~60 days brought only one alert ("going hungry ... on day 586 5 starved", day 585). The "dying off slowly" alert counts only "hunger, thirst or cold", so the toxin deaths that drove this decline never trigger a decline alert - while early on a 20 -> 16 dip fired that alert six times. Suggest: count toxin (and say "your hunters are poisoning themselves on X's herd; their lesson fades when they starve").
- 3 provisions (40 each, "fed 20"); 4th refused "at most 3 provisions within a ranked season's length (60 days)" - clear.
- Renown 1,476 unspent at day 620 (renown still piles up, pt10 Low).
- Leaderboard day 620: wild 40.3% (1,748 - recovering), PocketNomad11 752 (23.7%), Herdmaster11 501 (12.5%), Rulebender11 315, LateComer11 298, **me 184 (6.4%, 6th of 7 rows / 5th of 6 players by share)**, GeneBroker11 7.

## 17:03 (day 672) End
- Pack steady at ~178 after the helper went off (toxin deaths down to 0-2 a day). Final leaderboard day 671: wild 39.1%, PocketNomad11 1,214 (23.4%), Rulebender11 899 (16.2%), LateComer11 877 (11.5%), Herdmaster11 452 (6.1%), **ApexHunter11 178 (3.6%)**, GeneBroker11 12. Season 11: #5 of 6 legacy and lineage. Peak 250 alive / 5.3% (day 562). Era 1: #5 (0.7% mean, 1.9% final).
- No PROTECTED (agreement walker) animal ever came into my view; the protected flag is confirmed only for graced lineages (live) and for agreements by code (game.rs mark_protected includes market_shielded; possess_attack refuses `protected`).
- Watcher and scan loop stopped by PID; nothing possessed; hunt helper off. No page errors, console errors or failed requests in any of 5 UI runs.
