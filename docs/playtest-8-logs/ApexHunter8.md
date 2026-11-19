# ApexHunter8 log (predator persona, playtest 8)

Start 04:53 UTC, world day 11 (spring y0), fresh world (889 organisms).

## 04:53-04:57 (day 11-40) Register, design, found (UI via Playwright, 5 fps cap, DSF 1)
- Registered via UI as ApexHunter8 (player 4). No page errors / console errors / failed requests during register, design, found.
- Design "Ninefold Fang Pack": warm, live birth, litter 3, 40 founders, pure carnivore (carnivory 1), pack 1, group tol .9, body .45, muscle .9, fangs .9, claws .6, nose .8, legs .45, fat .6, fur .5, hibernation .55, caching .4, eyes .3, aggression .85, obedience .9, boldness .7; 63/100 pts (37 left).
- Designer food hint is carnivore aware: "40 founders ... eat the prey around them within days ... Around 24 founders spread out" (but on my first dry-run page load with the same sliders it showed the generic herbivore text "The map's Food layer shows where grazing herds (and so prey) grow" - warnings lag one update? pt7 #11 "warnings lag one check" possibly still present, Low).
- Grace hint text now says "keeps it through that winter and the spring after it" - pt7 Low "until that winter ends" text: FIXED.
- Founded ~day 37 (lineage 4). Herdmaster8 (lineage 5, 36 animals) within 16 tiles of my founding spot.
- localStorage evo.lastDesign has "name":"" though the lineage was named (Low? the name may be deliberately not remembered).
- Founders starve 1-2/day from day 38 (hunger mean 51, 7 > 80 by day 40).
- Watcher apex8/ApexHunter8-watch.mjs (PID in apex8/watch.pid): logs alerts, replies, state summary; hunt helper = 1 attack order per 15 s, hungriest-nearest adult -> nearest non-burrowed non-carnivore within 10 tiles, preferring players' animals.

## 04:57-05:00 (day 48-83) Founding weeks
- Hunt helper first picked Herdmaster8 animals (my neighbour, lineage 5, ~190 warning-coloured herbivores within 16 tiles): refused `newcomer_grace` with days and real seconds - clear. Switched helper to wild prey only.
- Founders starve 1-3/day (days 38-75), births offset: 40 -> 58 (day 82), hunger mean ~50, 10 > 80.
- **pt7 Low "Possessed #X is starving ... foraging on its own until you give it an order" in the same second as an order: STILL PRESENT** (helper possesses then orders 0.3 s later; the need alert arrives in between). Low.
- **pt7 #3 / pt6 #9 honest prey count: FIXED.** Rally (day 82) at my pack's middle (76,80): reply `prey_nearby 16, prey_feeds 0` and warning "The prey your hunters can take in or near this circle (16 animals: no burrowers, poisonous kinds or other hunters) feeds about 0 of the group's 58 for good; expect hunger." Pinned view of the circle (r 10.6): 29 burrowed voles, 18 Herdmaster8 (graced), 6 open voles, 2 omnivores -> the 16 matches what is catchable (burrowed and graced excluded; code `prey_check` now filters sheltered, too heavy, grace-spared and toxic-avoided). Also a "feeds N" figure - very useful.
- Pack instinct 1.0 still doesn't keep the pack together: spread x 34-113, y 15-128 by day 74 (Low-Medium, as in pt7).

## 05:00-05:03 (day 90-113) First winter; slow die-off warning; View group
- **pt7 #4 slow die-off warning: FIXED.** Day 94: "Your lineage is dying off slowly: 60 → 45 alive over the last 10 days, 16 of them of hunger, thirst or cold (27% of the lineage). Day by day it looks small; ..." plus "Your lineage is going hungry: on day 94 5 starved ... (10% of it) ... 51% of your animals are asleep ...". Re-sent on day 96 with updated numbers (2 days later; mild fatigue risk, watch).
- **Bug (Low-Medium): attack order accepted for a hibernating possessed animal.** Day 95: possessed #4955 (dormant), alert "has bedded down for the winter ... takes no orders until it wakes", but `possess_attack` on it replied ok=true `{"organism":4955,"target":2331}` - no refusal (expected `asleep`/dormant refusal like the alert says).
- Hibernation .55 on a pure carnivore: on day 100, 28 of 43 dormant; awake ones keep starving. 60 -> 37 by day 109 (first winter inside grace, ~40% lost; pt7 had 60%). **pt7 first-winter larder: partly better** (still heavy losses inside grace).
- Split (API) day 102: Group A 21 / Group B 21.
- **pt7 Low "second hunting party blind unless view pinned": FIXED-ish with a new 'View group' map tool** that cycles Viewing A -> Viewing B -> whole lineage (pinned view + hint "Viewing Group A (main): the view window stays on it as it moves"). Group B worked: view (76.8,78.7) r 12, 113 animals in sight.
- **New bug (Medium): View group on a scattered group pins the view on empty ground with a tiny radius.** "Viewing A" pinned at (72.6,50.1) radius 3.5 - the mean of Group A's spread-out (mostly hibernating) members, in the middle of a lake: 0 animals in sight, the map shows the view bracket over water (shots/ApexHunter8-viewgroup-0.png). Expected: centre on where members actually are (e.g. the densest cluster / the possessed one), or a radius that covers them. Repro: split a pack that is spread across 40+ tiles; click View group.
- Pack-mate kill credit: "Attack over: your #3852 killed #2391." seen (needs more data).

## 05:03-05:07 (day 119-175) Shop, end of grace, hunting Herdmaster8's bright toxic herd
- Day 121: 3 provisions (40 each, "fed 20") + 1 reinforcement (150; +10 landed with Group B). The 10 reinforcements were starving within ~15 days (#7568, #7563 starved d141/d144): reinforcements land into the same empty ground.
- Pack 60 (day 82) -> 22 (day 152) over the first winter and early spring (wild prey mostly burrowed voles). Slow die-off alert re-sent every ~3 days with fresh numbers (days 136, 139, 142, 148, 151) - informative, borderline repetitive (Low).
- Day 149: grace ended. **pt7 Low "learning a new poisonous prey isn't announced": FIXED** - "Your hunters have learned that Herdmaster8's Sunbright Drove are poisonous: on their own they pass them up most of the time (less so when starving), and an attack order on one warns you." (and the same for Rulebender8's Rule Rovers on day 152).
- **pt7 Low "pack-mates' kills read as failures": FIXED** - "Attack over: your #1560's pack killed #11460." / "your #1589's pack killed #7251" alongside "your #5782 killed #7727".
- Orders on Herdmaster8's warning-coloured herd (days 152-165): 5 orders, 5 kills (2 by the possessed animal, 2 by its pack). 2 toxin deaths of mine in the period. Orders are very effective now.
- **pt7 Low "attack-order warning always says 85% ignoring hunger": STILL PRESENT** - order by #1560 at hunger 85: warning "(on their own they pass it up 85% of the time)".

## 05:07-05:11 (day 168-225) Summer self-feeding; second autumn
- Helper OFF days 168-203 (sampler apex8/ApexHunter8-sampler.py logs per-animal hunger drops >25 = a meal, with distance to nearest learned-toxic and nearest safe prey). The pack fed itself well in summer: 3-9 meals per 2 game days, no starvation, 27 -> 39 by day 204. pt7's ~2-3 starving per day beside a toxic herd did not happen this summer (prey-scaled larder / summer wild prey). **pt7 #3 "predators can't compete": partly better** - survives and grows in summer, but Herdmaster8 went 186 -> 1,341 (28%) while I went 58 -> 42 (0.7%).
- Hungry-hunter test inconclusive in summer: hunger stayed < 80 (0-1 hungry hunters near toxic prey at any time).
- Claim for Group B at (59,77) r10 refused `overlaps` (Herdmaster8's territory (58,76) r22). New text: "A group ten times the size of a small holder (at most 12) may claim over its ground" (pt7 #13 squatting: addressed).
- Rally there instead: `prey_nearby 24, prey_feeds 1` of 38 + warning "53% of the group is dormant (hibernating) and won't walk until it wakes; the order may run out before then." Honest (Herdmaster8's 57 toxic animals in the circle excluded). Good dormancy warning.
- Helper orders on Herdmaster8 days 203-222: 13 kills of ~16 resolved orders; mix of "your #X killed" and "your #X's pack killed".
- `poisonous` list dropped Rulebender8 (learned day 152) by day 222 - lessons fade (fine, undocumented).
- Leaderboard day 222: Herdmaster8 28.2% (1,341), GeneBroker8 9.4%, me 0.7% (42), Rulebender8 0.5%, PocketNomad8 0.2%.
- Day 225: 3 provisions again (60-day window reset), helper OFF again for a winter hungry-hunter test (days 222-250).

## 05:11-05:16 (day 225-265) Second winter held; Ranks
- Winter 2 (days 210-240), helper OFF days 222-251: 42 -> 38 (deaths: old age, winter sleep, 1 thirst; 0 starved, 0 toxin). vs first winter 60 -> 37. **pt7 first-winter larder / winter losses: better in winter 2** (with 3 provisions).
- Hungry-hunter vs bright-toxic (helper OFF, winter): sampler shows hungry hunters (hunger 85-100) eating while the nearest Herdmaster8 (learned toxic, warning-coloured) animal was 0.2-1.7 tiles away and the nearest safe prey 6-10 tiles away (e.g. "#16549 ate 100->59 dtox 1.0 dsafe 8.1", "#16724 ate 87->51 dtox 0.5 dsafe 8.4"), yet 0 toxin deaths in that window - can't tell from a player's view whether they ate the toxic herd (no death-by-me feed for the victim's lineage). Not conclusive; the brief says the bright_aversion fix is not in this build. Herdmaster8 grew 757 -> 2,100 (hard cap) beside my pack.
- Day 256: reinforcement #2 (150) landed with Group B ("group":1). Pack 48 by day 261.
- Ranks tab (UI) day 261: Season 5 legacy 3rd of 5 (Herdmaster8 29.1%, GeneBroker8 6.8%, me 0.9%); "Largest lineage (peak)": Herdmaster8 2,100 (the hard cap). Herdmaster8 4 titles. No console errors (shots/ApexHunter8-d260-ranks.png).
- Balance (High, not mine to verify but visible): one warning-coloured toxic herbivore lineage hit the 2,100 hard cap by day ~260 and holds ~30% share, the same class that topped pt5-7.

## 05:16-05:19 (day 268-300) Megadrought; UI attack
- Catastrophe: megadrought days 271-315. Pack holds ~50 (few thirst/starve deaths), helper kills: 22 kills of ~32 resolved orders so far.
- Survey (35 renown, price crept up from 33): Herdmaster8 in 4 groups (610 / 870 / 300 / 10) with 3 territories r22-24; GeneBroker8 300 at (92,20); Rulebender8 30.
- UI attack via real clicks (possess -> Attack -> click a Herdmaster8 Sunbright Drove): toasts "#21477 is hunting #28438; hungry pack-mates nearby join in. #28438 looks poisonous to your kind ... (on their own they pass it up 85% of the time)" then "Attack over: your #21477's pack killed #28438." (shots/ApexHunter8-d300-attack.png). No console errors. Hunter at hunger 79-80 still quoted 85%.
- (Spectate 65 vs my 50 was just my stale summary: population 68 by day 306. Not a bug.)

## 05:19-05:22 (day 306-322) Second hunting party
- Split 12 adults nearest Herdmaster8's south territory into Group C (API `split_group` with organisms: reply groups A 2 / B 54 / C 12) and migrated it to (90,104): reply journey 3.1 days / 35 tiles, destination `prey_nearby 48, prey_feeds 1` + warning. Arrived day 316: "Group C reached its migration target ... keeps it as its home ground" then "The prey Group C can take in or near its home ground at (90, 104) (36 animals) feeds about 0 of its 12 hunters for good: move the pack to where prey is, or split it." Clear, honest (Herdmaster8's 120 toxic animals there excluded).
- **View group (UI) with three groups: works** - Viewing A (r 12, 205 in sight) -> Viewing B (r 8.4, 134) -> Viewing C at (86.3,105.3) r 11.7 with 122 Herdmaster8 + 18 wild in sight -> whole lineage. pt7 Low "a second hunting party is blind unless the view is pinned on it": FIXED (one button; still a pin, and only one party visible at a time).
- Megadrought (days 271-315) survived 50 -> 67 ("Survivor" achievement, +25). "The megadrought is over: 3363 fewer organisms in the world than at its start."
- Odd but plausible: "#23583 killed #21611" and "#23583 ... starved" in the same second (a kill doesn't feed the killer in time). Low.
- Day 321: 69 alive, 1.7% share, #3 of 5 players (Herdmaster8 35.4%/1,209, GeneBroker8 13.0%).

## 05:22-05:25 (day 322-364) Third winter; era 1 ends
- Group C hunting at Herdmaster8's south territory with the view pinned on it: kills continue (35 kills of ~48 resolved helper orders overall; new outcome "escaped; your #24072 lost sight of it (12 tiles away)" x3).
- Third winter (days 330-360): 69 -> 71 -> 88 by day 364 with 3 provisions + 1 reinforcement (day 352-353, reinforce now 300... balance math: provisions show price 40 but balance fell 28-34 per buy because renown trickles in between, fine).
- "Era 1 has ended: you finished #3 with a mean 1.1% of breeding alleles over the era (2.5% at its end)." Seasons 5 and 6: #3 in legacy and lineage (#1 of 5 in survival in season 5).
- **88 alive on day 364 (gen 6), 2.5% share, #2 of the 6 players by alive count after Herdmaster8** - the best predator result of these playtests (pt7 peak 84, last place). pt7 #3 "predators can't compete": **partly fixed** - a pack now grows through winters and drought, but stays ~1/17 of the leading herd (1,483, 40.3%).
- LateComer8 appeared (61 alive by day 364, era 2 start).
- **New bug (Medium-Low): Survey refused while 88 of my animals were alive** - day 366 `survey` -> `no_lineage` "your main group has no animals to survey from". The main group (A) had emptied (its 2 members died; my animals were in Groups B 71 and C 19). Expected: survey from any group (largest / possessed), or the shop should say "needs animals in the main group" before you click. Worked after `merge_groups` B into A. Repro: split so all animals are outside group 0 (or let group 0 die out); buy a survey. (Also the client's "View group"/groups list shows "Group A (main) · 0".)
- Day 371 leaderboard: Herdmaster8 47.1% (2,099 = hard cap, now above the wild's 46.7%), **me 2.8% (91) - #2 of the players**, LateComer8 2.3% (116), Rulebender8 1.0%, PocketNomad8 1 animal, GeneBroker8 0.

## 05:25-05:28 (day 371-435) Pack passes 100; epidemic; a claim that starves
- Pack 91 (day 371) -> 136 (day 401, gen 7) in spring/summer with the helper hunting Herdmaster8; 2nd reinforcement of the window (300) day 403. Larder now scales with prey, so a pack can pass the old 60-100 band limit. **pt7 #3 prey-scaled larder: works** (136 > pt7's 84 cap).
- Catastrophe: epidemic days 394-408 (1 disease death seen).
- Season 7 (ended day 419): "#2 of 6 in legacy, #4 of 6 in lineage, #1 of 5 in survival".
- Territory claim for the main group (119) at (55,44) r8 away from Herdmaster8: reply `prey_nearby 50, prey_feeds 0` + warning "feeds about 0 of the group's 119 for good; expect hunger" - and it was right: within 6 days "12 of the 38 animals holding territories (Group A (main)) are starving or parched ..." and "Your lineage is going hungry: on day 425 6 starved (4%)" (x2). Abandoned day 435. **pt7 #5 food estimates too high: for a pack, now honest/conservative.** But note: every rally/claim/migrate check I made all game said "feeds 0-1" (also beside Herdmaster8's herd, where the pack actually grew 27 -> 136), because the toxic herd it lives on (by orders) is excluded - so the warning fires everywhere and loses meaning for a pack that hunts toxic prey by order (Low).
- 0 "killed by their prey fighting back" deaths in ~435 days, so the new fought-back wording could not be observed in play (code: game.rs DeathCause::FoughtBack "was killed by its prey fighting back").
- No cannibal "eaten by ApexHunter8" summaries and no "drifted from its home" repeats this time.

## 05:28-05:31 (day 436-462) Fourth winter; fought back seen
- **pt7 #14 / round-7 "fought back" cause: FIXED.** Day 457 order on a Herdmaster8 Sunbright Drove: "#33690, the animal you were possessing, was killed by its prey fighting back." / "Attack over: your #33690 was killed by its prey fighting back before it reached #32822." Daily summary: "Day 458: 1 born, 1 died (1 killed by prey)" - no longer "eaten by Herdmaster8's ..." for a plant eater. (Low wording: "fighting back before it reached #32822" - if it never reached the target, which prey fought back? Probably a herd-mate; say "killed by #X's herd fighting back".)
- Winter 4: 125 -> 102 (2-4 starved/day days 441-456; the slow die-off / hunger alerts fired). 3 provisions day 456.
- Helper OFF days 462-482 for another winter hungry-hunter check (17 hunters > 80 hunger beside 250 Herdmaster8 animals).

## 05:31-05:39 (day 462-570) Year 5; third hunting party on LateComer8
- Helper OFF days 462-482 (winter 4): pack ~100 -> 90, 1 starved every few days, no toxin deaths; at most 0-2 hunters > 80 hunger near the toxic herd at any sample; hungry hunters eat but I cannot see what. Hungry-hunter vs bright-toxic remains **unverifiable from a player's seat**; not fixed per the brief (bright_aversion fix absent).
- Genes tab "Births and deaths" chart has a "killed by prey fighting back" legend entry (shots/ApexHunter8-d490-genes.png). Low: "born" and "old age" bars use near-identical greens.
- Day 509-527: pack 94 -> 112 (gen 8), renown piling up to 733 for the first time (shop caps: 3 provisions + 2 reinforcements per 60 days) - pt7 #16 renown piling up now applies to a predator too (Low).
- Survey day 529 (42 renown; price climbs 33 -> 35 -> 39 -> 42 per purchase, never stated) shows LateComer8 in two groups (170 with territory, 190 at (84,36)); Herdmaster8 4 groups summing 1,930 = its alive count (earlier mismatch was spring births).
- Split 20 into a new Group B and migrated 37 tiles to LateComer8's flier herd (84,36): journey 3.4 days, arrived on time (day 535) with the home-ground and prey-short alerts. Destination check said `prey_nearby 293, feeds 2 of 20`; rally there `prey_nearby 86` while my pinned view (r 10.4) showed 15 open non-toxic prey + 83 LateComer8 fliers (learned poisonous, lineage 11 in `poisonous`) - possibly the toxic fliers are still counted when the learned lesson is weak (<0.5); can't confirm from the client (Low).
- Orders on LateComer8's warning-coloured fliers: 8 orders, 8 kills (walkers kill fliers fine). Each warned "85%" whatever the hunter's hunger (87-100).
- Day 570: 118 alive, 2.3% share (#4 of 6 players by share: Herdmaster8 36.3%, LateComer8 6.7%, PocketNomad8 2.6%).

## 05:40-05:47 (day 572-660) Wrap-up
- Winter 5 held (118); summer: 115 -> 149 (day 653), 3.6% share at peak. Herdmaster8 fell 2,100 -> 1,730 meanwhile; LateComer8 (era 2 late joiner, cold toxic flier class) rose to 11-12%.
- Ranks tab (UI, day 657, shots/ApexHunter8-final-ranks.png): Season 11 legacy 4th of 6 (Herdmaster8 39.8%, LateComer8 12.0%, PocketNomad8 2.6%, me 2.2%); peak lineage 149 (4th). Record: 10 seasons, best Legacy 2nd (seasons 1, 7, 8), Survival 1st (season 5, tied, no title). Era 1: #3 of players (mean 1.1%).
- Helper totals: 145 orders, 258 ok replies, refused only 2 sheltered / 5 not_found; outcomes: 103 kills ("your #X killed" or "your #X's pack killed"), 11 escaped (lost sight 12-13 tiles), 6 went to ground, 5 lost track, 3 starved first, 3 other (poisoned / fought back). ~75% of orders end in a kill and are credited properly.
- 05:47 stopped watcher and sampler by PID (apex8/watch.pid, apex8/sampler.pid), helper off, view cleared. No page errors / console errors / failed requests in any of 8 UI sessions.
