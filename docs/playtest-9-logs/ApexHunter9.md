# ApexHunter9 log (predator persona, playtest 9)

Start 08:38 UTC, world day 13 (spring y0), fresh world (968 organisms, 4 lineages incl. wild).

## 08:38-08:45 (day 13-43) Register, design, found (UI via Playwright, 5 fps cap, DSF 1)
- Registered in UI as ApexHunter9 (player 4). No page errors, console errors or failed requests (only SwiftShader GL perf warnings).
- Design "Ironjaw Pack": warm, live birth, litter 3, 36 founders, carnivory 1, pack 1, group tol .9, body .45, muscle .9, fangs .9, claws .6, nose .8, legs .45, fat .6, fur .5, caching .5, eyes .3, aggression .85, obedience .9, boldness .7, seasonal coat .4; hibernation 0.5 came from the starting design (I did not set it and kept it). 60.9/100 pts.
- Designer food hint is carnivore-aware on first fill this time ("36 founders ... eat the prey around them within days ... Around 16 founders spread out"). pt8 Low "designer first load lags one check": not seen this time (fixed or not reproduced).
- Drought warning in designer (63% dry) includes the estivation costs now ("a sleeping lineage doesn't breed, so a dry spring can cost a season's young, and it wakes needing food") - pt8 Medium 8 estivation advice: wording FIXED (cost stated).
- Founded day 25. **Landing text: "Your founders landed around (61, 49): the nearest rival animal (PocketNomad9's) is 32 tiles away."** pt8 #11 "nothing tells a founder how far the nearest rival is": FIXED. It names only the nearest rival's animals, not territory (none existed). Drought provision alert also shown.
- The founding reply's landing text is in the alerts feed (Genes tab) but I could not catch a dialog with it after clicking Found (the page just goes to the map); fine.
- Founders spread x37-100 y24-89 by day 33 (36 founders landing "around (61,49)" yet spanning 63 tiles in 8 days) - pack instinct 1.0 still doesn't keep a pack together (pt7/pt8 Low-Medium: STILL PRESENT).
- 3 founders starved days 31-35; births from day 34: 46 alive by day 43.
- Name "Taproot grazers" (GeneBroker9) is live in /api/spectate: pt8 false positive "Taproot" fixed.
- Watcher apex9/ApexHunter9-watch.mjs (PID in apex9/watch.pid), plain JSON WebSocket; logs alerts/results/summaries; meal sampler (hunger drop >20 with distance to nearest warning-coloured vs other prey) and "vanished prey next to my animal" log.

## 08:41-08:43 (day 45-67) Scouting, split, rallies, prey warning
- Pack spread x8-107 y5-101 at day 45 with 48 animals: the "pack" covers almost the whole 128-tile map. **Pack instinct 1.0 doesn't hold a pack (Medium, STILL PRESENT since pt7).**
- Scan (set_view at each cluster): GeneBroker9 156 drab herbivores NE (~104,33); PocketNomad9 38 drab NE (~97,27); Herdmaster9 warning-coloured herd NW (22-26,18-29) plus a territory (13,22) r14; Rulebender9 warning-coloured herd NW (22-41,7-27). All graced (protected flag).
- Split day 66 by position (x<60 -> group B, 17; A 36). Rally B at (31,20) next to the two warning herds: `prey_nearby 0, prey_feeds 0`, warning "The prey your hunters can take in or near this circle (0 animals: no burrowers or other hunters, poisonous kinds only as often as hungry hunters take them) feeds about 0 of the group's 17 for good; expect hunger. It is 39 tiles from the group..." New honest-signal wording ("poisonous kinds only as often as hungry hunters take them") - consistent with balance round 8.
  - Low: the text lists what's excluded (burrowers, hunters, poisonous) but not graced animals; ~56 graced herbivores sit in that circle and the count says 0 with no reason a player can see. Suggest "(... animals under a newcomer's grace not counted)".
- Rally A at (100,32) near GeneBroker9's herd: `prey_nearby 36, prey_feeds 0` of 40 - 36 catchable animals feed "about 0 for good". Hard to read: if 36 animals are catchable, "feeds 0" sounds wrong; probably a sustainable-yield figure. (Low, wording)
- Rally at (31,20) in a 64% drought with `water_share 0` (drinkable_tiles 68) gave no water warning. (Watch whether thirst follows.)

## 08:44-08:45 (day 78-96) Early winter
- Day 78: 35 of 72 dormant (hibernation 0.5 from the starting design); winter starts ~day 90 (bedding down ~2 weeks early, documented).
- **pt8 #14 "attack orders accepted for a hibernating animal": FIXED.** Day 96 possessed dormant #5358, `possess_attack` on a wild vole 1.3 tiles away -> refused `asleep`: "organism 5358 is in its deep sleep (hibernating, brumating or estivating): it can't attack until it wakes". The possess itself is accepted silently, then an alert says it has bedded down (fine).
- Drought eased day 86 (29% dry).
- Leaderboard day 90: Herdmaster9 165 (6.2%), GeneBroker9 131, Rulebender9 77, me 72 (2.3%), PocketNomad9 58.
- Day 107-111 alerts: "Your lineage is going hungry: on day 108 7 starved ... 60% of your animals are asleep ..." and "Your lineage is dying off slowly: 74 → 58 alive over the last 10 days, 17 of them of hunger, thirst or cold (23% of the lineage). Day by day it looks small. 60% ... asleep ..." re-sent 3 days later with fresh numbers (74 → 53). **pt8 slow die-off warning: FIXED/working** (numbers right, order correct: 74 → 53 not the pt8 "954 → 1251").
  - Low: the advice is herbivore-worded for a pure carnivore pack ("fatten the herd on rich ground ... the Food layer shows it"); for hunters, rich ground means prey, not plants.
- Day 115: bought 3 provisions (40 each, "fed 20"); 4th refused with a clear reason ("at most 3 provisions within a ranked season's length (60 days)").
- Day 126: 54 alive (from 74 on day 98). Group A (37) at (99,33) hunger median ~57, 7 at >=90; Group B (17) at (31,21) beside the warning herds, hunger 26-70 (better fed; GeneBroker9's drab animals wander there unprotected).
- Day 131 view at (31,21): 44 Rulebender9 + 32 Herdmaster9 warning-coloured herbivores (protected), 5 GeneBroker9 drab (unprotected), 3 wild.

## 08:49-08:53 (day 149-196) Grace ends, megadrought, hunters vs bright toxic herds
- Day 149 grace ended; day 150 megadrought (days 151-195). Day 150 (1 s after grace ended): "Your hunters have learned that Herdmaster9's Emberwool Drove are poisonous ..." and the same for Rulebender9's Sorrel Hoppers - **before any of my animals had bitten one** (first poisoning day 169). Low: "learned" with no lesson behind it; maybe the warning colours are innate aversion, but then the text should say "your hunters see that ... look poisonous".
- **Hungry hunters vs bright toxic prey (honest signals): NOW WORKING.** Group B (17 at day 131) lived beside Herdmaster9's warning-coloured herd with no other prey nearby (nearest non-warning prey ~20-60 tiles in the sampler; meals logged with the nearest warning animal 0.3-1.3 tiles away) and grew to 30 by day 170; daily summaries show toxin deaths about 1 every 2-3 days (days 161, 169, 170, 171; "Founder #1501 was poisoned", "#1484 was poisoned"). So the pack eats the toxic herd and pays for it. (pt7/pt8 "hungry hunters never take warning-coloured prey": FIXED.)
- **Attack-order warning now hunger-aware** (pt8 Low "always 85%": FIXED): order by #13757 at hunger 94.5 -> "(on their own they pass it up 87% of the time)"; by #9285 at hunger 26 -> "95% of the time". The latter killed its target ("Attack over: your #9285 killed #12177.").
- Low, STILL PRESENT: "Possessed #13757 is parched (thirst 76): it is foraging on its own until you give it an order." 5 s after I gave it an attack order (same for #9285 2 s after its kill - that one is fine).
- Day 195: "The megadrought is over: 719 fewer organisms ..." in the same second as "Drought: 96% of the land is dry ..." - contradictory pair (Low).
- Day 195: "Survivor: your lineage came through a megadrought with at least a quarter of its animals." (I went 76 -> 118 through it.)
- Hunger alert advice is herbivore-worded for a pack ("Move groups to better ground (the Food layer shows it), split big herds") (Low).
- **Prey warning near a herd (pt8 Low): STILL PRESENT, partly.** Rally B at (24,17) inside Herdmaster9's herd: "242 animals ... feeds about 4 of the group's 29 for good; expect hunger", though group B had grown 17 -> 30 eating that herd and its hunger median was 34. Rally A at (62,36) by Rulebender9's herd: "293 animals ... feeds about 6 of the group's 82". The discount for poisonous prey is stated, but "expect hunger" contradicts what the pack is living on.
- Rally B also warned "76% of the group will bed down within about 7 days" (autumn, hibernation): good dormancy lookahead.
- Leaderboard day 181: Herdmaster9 567 (10.1%), Rulebender9 176 (3.6%), me 111 (2.1%), PocketNomad9 7, GeneBroker9 0 (retired).

## 08:53-08:59 (day 196-255) Hunt helper, second winter, spring shop, UI tabs
- Hunt helper (in the watcher, toggled by apex9/hunt.on): every 15 s one attack order, alternating groups, hungriest awake adult -> nearest unprotected prey within 6 tiles (group A prefers Rulebender9's, B Herdmaster9's).
- Results days 210-242: kills ("your #17096 killed #19232", "your #9285's pack killed #14344"), "lost track of #15072 (it died or left the area)", "#18068 starved before it reached #6339" (honest outcome texts).
- Low: 3 of ~8 orders on wild voles refused `sheltered` ("down in its burrow") although the target had no `burrowed` flag in the state I had just received (it may have dived in the 0.4 s since; a client using the flag will still offer Attack on them).
- Second winter 125 -> 66 (days 192-239; starving + winter sleep + old age + 1-2 toxin). Day 202: "dying off slowly: 125 → 94 alive over the last 10 days, 26 of them of hunger, thirst or cold (21%)" - correct.
- Herdmaster9 crashed 567 -> 160 through the megadrought and winter; Rulebender9 hit 0 and re-founded; GeneBroker9 retired. My pack (71 -> 103 by day 255) is now 2nd in legacy share.
- Spring day 247-251 shop: reinforce 150 (+10, landed at (53.7,20.9) "group 0"), second reinforce priced 300 (doubling, shown in the shop), landed at (41.7,2.9) - **two reinforcements for group A landed 20+ tiles apart and neither near group A's centre (~69,43)** (Low-Medium: where do reinforcements land? the reply says `group 0` and `nearby_count 16/13`, i.e. near a few members, but not near the group).
- UI Lineage tab: groups A 73 / B 21, legacy table (GeneBroker9 with nameplate «Taproot stud farm»). Ranks tab: season 5, me Legacy 2nd of 5 (1.9%) behind Herdmaster9 3.2%; my record "Survival 1st (season 3)". Ranked tables list 5 players although 6 accounts exist (LateComer9 not registered yet - fine).
- Market tab: compatibility card says every lineage is sterile with mine (pure carnivore) - honest. **Stud listings show the animal's exact position ("at 104, 56")**: for a predator this is a free locator for a rival's prize animals (Low-Medium; GeneBroker9's 5-renown dams/studs are protected only by distance).
- No page errors or failed requests in any UI run so far.

## 08:59-09:02 (day 263-290) Founding placement, View group
- PocketNomad9's re-founded "Pocket emberkin" (lineage 10, warning-coloured) at ~(90.7,60.6) on day ~263: my nearest hunter 7.8 tiles away, 4 of mine within 10 tiles, 11 within 16. My group A is scattered over x23-96, y0-90, so it is hard to land "away from" it; group-centre avoidance would miss a scattered pack's members. **pt8 #11 placement beside a pack: inconclusive / possibly still present** (I can't see their landing text; check PocketNomad9's report).
- Again "Your hunters have learned that PocketNomad9's Pocket emberkin are poisonous" on day 264, with no bite recorded - same "learned without a lesson" pattern.
- **View group (pt8 #14): FIXED for groups.** On my scattered pack, "Viewing A" pinned at (75.4,63.7) r10.6 with 41 of my animals in the circle (densest cluster), "Viewing B" at (16.9,19.9) r11.5 with 31 in it. The third click ("follows your whole lineage again") puts the window at the lineage mean (54.7,44.9): only 2 of my 140 animals in sight there - the same empty-centre problem for the unpinned whole-lineage view (Low).
- "While you were away: 15 attack alerts, ..." toast on a page load 2 real minutes after my previous UI visit - but my harness re-seeds localStorage from the founding-time copy (evo.seen tick 1180), so this is my artifact, not a bug (the client keys "away" on its own localStorage).
- A helper-ordered hunter (#10478) whose pack killed a toxic target was then poisoned ("#10478, the animal you were possessing, was poisoned").

## 09:02-09:05 (day 300-341) Growth to 220
- Pack 140 (day 282) -> 224 (day 337); leaderboard day 338: Herdmaster9 475 (12.1%), Rulebender9 285 (5.6%), **me 221 (5.1%)**, PocketNomad9 264 (4.2%). **pt8 #12 "predators trail the herds": much better** - my pack is ~1/2 of the leading herd by head count (pt8: 1/14), and passed two herbivore players at times. Wild population fell 4,995 -> 3,345 in ~50 days.
- Spread x1-125, y2-126: the 198-strong group A covers the entire map.
- Third winter: toxin deaths now 1-2 a day (days 338-341: "2 toxin, 1 starved", "1 starved, 1 toxin") - starving hunters taking the bright toxic herd as designed; costs real animals but the pack is alive. Season 5 ended: me #2 of 5 legacy.

## 09:05-09:09 (day 347-380) Era 1 ends, LateComer9 joins
- Claim for group B over Herdmaster9's ground refused `overlaps`: "... A group ten times the size of a small holder (at most 12 adults; young don't count) may claim over its ground, and a group twice the size of one holding 4 times the food it needs" - squatting rule now excludes young (pt8 #10 addressed); the last clause has no verb/period and is hard to parse (Low).
- Rally B in winter (day 350): "81% of the group is dormant ... Little or no drinking water in or near this circle ... 195 animals ... feeds about 3 of the group's 16" - water and dormancy warnings work.
- **Era 1 (days 0-359): I finished #2** (mean 2.3%, 7.1% at the end) behind Herdmaster9 (mean 5.9%, 11.4% at end); Rulebender9 mean 2.25% just behind me. Niche prizes: carnivore (45% of carnivore alleles), plus **biome:desert with a 0.96% share (127 adults), mountain 2.56%, grassland 3.13%** - biome niche titles go to whoever leads among players even at <1% share (Low balance: a title at 0.96% feels cheap).
- Pack-vs-herd at era end: 240 vs 324 (Herdmaster9) by head count; 7.1% vs 11.4% share. Best predator showing in any playtest; pt8 #12 largely FIXED.
- LateComer9 founded ~day 360 ("Ember Kites", 60-72 warning-coloured fliers). My full scan at day 378 (25 view windows over my 240 animals) didn't see a single one, so they landed away from my pack (which spans the whole map, though thin in the NE): **founding placement away from packs: looks FIXED** (pending LateComer9's own landing text).
- Rulebender9 re-founded twice more (Burrow Sorrels, drab burrowers), GeneBroker9 now "Beetroot burrowers" - both burrowers: hard prey for me.

## 09:09-09:12 (day 380-437) Group B dies out silently; Herdmaster9 runs away again
- **Group B wasted away 29 (day 282) -> 22 -> 17 -> 14 -> 10 -> 7 -> 5 -> 2 -> gone (day ~415) living beside Herdmaster9's toxic herd, and the group simply vanished from `groups` with no group-level alert** ("Group B is starving / down to 2 / gone"). The only alerts were lineage-wide "going hungry" ones whose advice ("Move groups to better ground (the Food layer shows it), split big herds") never names the group in trouble. Medium (a player with a big main group never notices a satellite group dying).
- Day 421 leaderboard: **Herdmaster9 1,240 (26.0%)** (324 at day 360: x3.8 in 60 days), me 203 (6.2%), PocketNomad9 213, LateComer9 202, Rulebender9 53, GeneBroker9 14; wild down to 2,179. The warm toxic herd runs away again in era 2 while my pack can't dent it: a predator beside it slowly dies of toxin + hunger. (pt8 High #1, balance: still present in a milder form; Herdmaster9's own report will say more.)
- Day 420: "learned that LateComer9's Ember Kites are poisonous" (again with no bite).
- Day 436 split 50 hunters nearest to Herdmaster9's herd as a new group B, rally at (14,20): "821 animals ... feeds about 18 of the group's 50 for good; expect hunger" (the count includes the toxic herd, the feed figure discounts it).

## 09:12-09:16 (day 437-481) The second toxic-herd group starves too
- New group B (50 at day 436, rallied into Herdmaster9's herd): 49 -> 35 -> 16 by day 471 (winter), mostly starved, ~1 toxin death; group A lost far fewer. Helper orders on the herd still killed (5 of 6: "your #41661 killed #39528", "#36764's pack killed #37294").
- So the rally warning "821 animals ... feeds about 18 of the group's 50; expect hunger" was **right**: over a year a pack can't live on a bright toxic herd (honest signals let it eat some, toxins and lessons cap it). I revise my earlier note: **the prey warning near a toxic herd is honest (pt8 Low: FIXED)**; group B's summer growth was the summer's wild prey.
- Still no group-level alert while group B lost 2/3 of its members in 30 days (Medium, as above).
- Herdmaster9 fell back 1,240 -> 491 in winter (era-2 day 120). Leaderboard day 480: Herdmaster9 491 (23.3%), **me 191 (8.0%)**, LateComer9 171 (7.8%), PocketNomad9 79, GeneBroker9 17, Rulebender9 12. Season 8: me #2 of 6 legacy and #2 of 6 lineage.
- Merged B back into A (day ~482).

## 09:16-09:18 (day 486-497) Spring shop, survey, reinforcements into a rival territory
- Day 488: 2 reinforcements (150 + 300) and 3 provisions, survey (49 renown; was 35 earlier: price rise unexplained, pt8 Low STILL PRESENT).
- **Both reinforcements landed at (53.7,27.1) and (53.6,28.8), 4-5 tiles from the centre of LateComer9's territory (58,29) r12, on top of its 70-member group** (LateComer9's grace was over: no protected flags). 20 hunters dropped into a rival's claimed ground; one of them (#43886, minutes old) was ordered on an Ember Kite 1.5 tiles away and "your #43886's pack killed #40626" a second later. Reinforcement placement ignores rival territories (Medium: fairness - a predator can buy a strike squad into a newcomer's territory; foundings avoid rival territories, reinforcements don't).
- Shop text: "reinforce_blocked: ... the next is possible on day 549 (about 5 real minutes)" while `reinforce_next_day` is 548 - pt8 Low "reinforcement next day off by one": STILL PRESENT. After 2 bought, the shop shows `reinforce_price` 150 though the next would be... (unclear; low).
- Attack warning on LateComer9's kite: "#40626 looks poisonous to your kind: **your animals have been poisoned eating its kind** (on their own they pass it up 89% of the time)" - none of mine has ever been poisoned by LateComer9's kites (no toxin death since they appeared besides those beside Herdmaster9's herd). Wrong claim; same root as "learned ... poisonous" alerts firing at grace end with no bite (Low-Medium: misinformation).
- Survey reply: groups with members rounded (20/80/130), Herdmaster9 "130" at (36,20) while it has 491 (only groups within radius 64 of my centre; fine but not said).

## 09:18-09:26 (day 509-608) Herdmaster9 at 37%; a winter migration and a crash
- Day 515 thirst alert: "Your lineage is dying of thirst: on day 515 2 starved and 3 died of thirst ... Move groups beside lasting water" (good, thirst told as thirst).
- Day 527: "learned that PocketNomad9's Pocket diggers are poisonous" - again on the newcomer's grace end, no bite (the pattern holds for every new rival lineage: 5 cases).
- Leaderboard day 566: Herdmaster9 712 (34.6%), **me 261 (11.5%)**, LateComer9 304 (9.1%), Rulebender9 18, PocketNomad9 30, GeneBroker9 18; wild 1,085. UI Lineage tab showed me 11.4% (weighted 15.2%).
- Possession survives closing the browser (a UI run's possessed animal was still mine-possessed in the next run: "Released #48211: you possess one animal at a time.") - fine, but nothing tells a returning player.
- Tried to drive Attack in the UI by clicking a kite marker in attack mode: no toast appeared (likely my harness's click missing the marker at the default zoom; not counted as a bug). Screenshot shots/ApexHunter9-d555-attack.png.
- **Migrate into Herdmaster9's territory (30,20), day 567 (1 day before winter, 51 of 260 already dormant, hibernation 0.5): warnings "Destination: it is inside Herdmaster9's territory ... its holders defend it" (pt8 #9 rival-territory warning: FIXED), water share, prey "624 animals ... feeds about 13 of 264". But no warning that winter starts tomorrow and a hibernating lineage beds down en route (the rally had "76% of the group will bed down within about 7 days" in a similar case); journey "13.6 days, food_share 1".** Result: "migration to (30, 20) ended: stalled after 35 tiles", and the pack fell 261 -> 108 by day 608 (also winter itself: earlier winters cost ~45%). Medium: missing bed-down lookahead on migrate (pt8 #9 dormant warning: partly fixed - rally yes, migrate no).
- **"Your lineage is dying off slowly: 261 → 143 alive over the last 10 days, 88 of them of hunger, thirst or cold (34%)"** - 45% gone in 10 days is a crash, not "slowly" (Low-Medium wording: the slow die-off alert should switch to a "crashing" text above some rate). Also a 34% figure next to 118 lost: the 34% counts only hunger/thirst/cold deaths (88/261), confusing beside the totals.
- Day 599: Season 10 ended, me #3 of 6 legacy; flood year days 601-623.

## 09:26-09:32 (day 608-674) End
- Season 11 ended: me #3 of 6 legacy, #3 lineage, **#1 of 6 survival** (flood year days 601-623).
- Day 674 leaderboard: Herdmaster9 1,736 (34.4%), LateComer9 733 (9.2%), Rulebender9 31 (4.7%), **me 208 (3.9%)**, PocketNomad9 249 (3.4%), GeneBroker9 19; wild 2,414. Herdmaster9 went 333 -> 1,736 in ~60 days of spring/summer again: the warm toxic herd still swings to a third of the world every summer.
- Orders over the game (helper + manual): 55 "killed", 13 "pack killed", 5 went to ground, 5 escaped, 2 killed by prey fighting back, 2 starved en route, 2 lost track, 1 "was poisoned before it reached" (wording odd: poisoned by what, on the way?). Deaths from daily summaries: 576 starved, 168 toxin.
- Watcher stopped (killed by PID). Possessed animal released earlier; helper off.

## Final standing
- Day 674: 208 alive, 3.9% share, 5th of 7 on the live leaderboard (peaked 2nd: 11.5% / 261 on day 566); era 1 #2 (mean 2.3%), carnivore niche + 3 biome niches; seasons 4/5/8/9 legacy #2; survival #1 in seasons 3 and 11.
