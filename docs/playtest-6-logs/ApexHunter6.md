# ApexHunter6 log (predator persona)

Start 21:05 UTC, world day 11 (spring year 0), fresh world (914 wild organisms).

## 21:06-21:07 (day 25-33) Register, design, found (UI via Playwright)
- Registered via UI (player 5). No console errors / failed requests during register+design+found.
- Design "Cinder Wolves": warm, live birth litter 3, pure carnivore (carnivory 1.0), pack 1.0, group tol .85, body .5, muscle .8, fangs .9, nose .9, claws .6, legs .45, fur .5, fat .7, vigilance .4, aggression .8, hibernation .5, caching .5, obedience .85, boldness .7, eyes .4, swim .2; 45 founders, 68.7/100 pts.
- Designer, 70 founders: NEW carnivore-aware warning "70 founders (about 360 kg of animals) land together and eat the prey around them within days: large foundings tend to lose most of their founders to hunger..." Good. At 45 it says only generic text ("Food layer shows where grazing herds (and so prey) grow").
- Founded day 32-33, lineage #5, grace 58.9 days (~290 real s). Lineage tab explains grace clearly ("other players can't order attacks on your animals ... wild predators still hunt them"); also "Season weight 20%... founding ramp". Grace ends ~day 92; winter starts day ~90 (days_to_winter 79 on day 11) -> founding at day 32 gives a grace that ends just after winter's start. Watch for a first-winter extension.
- My mistake: I never clicked the Birth radio; the default was eggs (incubated), not live birth. Noted only because the Lineage tab says "Eggs: 3 about to lay". (Low UX: the designer's birth choice is easy to overlook; the default is eggs for a pure carnivore pack.)
- 45 founders > GRACE_WINTER_FOUNDERS (40) so, per code, no first-winter grace extension for me; nothing in the designer/Lineage tab tells a player that 40 or fewer founders would extend their grace through the first winter (Low/Medium UX, check whether any text says so).

## 21:08-21:10 (day 39-53) Founding weeks
- Day 39: hunger mean 59, 8 of 45 above 80 (one at 100) six days after founding; pack already spread x 73-120, y 0-67 (pack instinct 1.0 doesn't keep a pack). Day 52: hunger 52, 54 alive (11 young), 0 starved; 2 founders died of thirst (days 45, 48). Founding crash much milder than pt5 (45->40 then). Eggs chill 2-5/day while parents hunt.
- Started WS watcher (ApexHunter6-watch.mjs) and helper (ApexHunter6-hunt.mjs: 1 attack order per 15 s, nearest non-poisonous target within 10 tiles).
- Attack on a vole marked not burrowed in my 2 s old state -> "sheltered: down in its burrow" (fine, it dived). Second order (wild fox at 4.9 tiles) accepted. Possess-second notice "Released #2103: you possess one animal at a time." still works.

## 21:11-21:14 (day 75-109) Grace end at winter, winter starvation, territory prey check
- Founding alert (from /api/lineage/stats alerts): "Newcomer grace: 60 days (about 5 minutes of real time). Other players can't order attacks on your animals, nor yours on theirs, until it ends." -> **pt5 "grace explained at founding": FIXED** (clear). It says nothing about the first-winter extension, which by code (`grace_end`, GRACE_WINTER_FOUNDERS=40) applies only to foundings of <=40 founders. My 45-founder lineage got none: grace ended day 91, winter began day 90 -> **pt5 #12 "starter grace ends exactly at winter's start": partly fixed** (only for <=40 founders, and the 40-founder threshold is not shown anywhere a player can see before founding). Medium-Low.
- Grace end alert day 91 exactly ("Your newcomer grace has ended: other players' animals can now attack yours, and yours theirs"). OK.
- set_view to a far spot without my animals -> refused "out_of_sight: you can only look where your organisms are". Good (no radar).
- Winter: 23 of 50 dormant; awake ones starving: deaths days 88-106 all "starved" (1-3/day), pop 54 (day 72) -> 39 (day 105). 98 eggs chilled since founding vs ~11 births (my default-eggs mistake, but no lineage warning about it either).
- `poisonous` became [4] (Herdmaster6's warning-coloured herd) on ~day 100 without any toxin death alert to me.
- **Territory prey check (pt5 #9)**: claim_territory (95,77) r12 for my 39 pure carnivores -> ok, `prey_nearby: 178`, no warnings. Code (`prey_check`, game.rs ~5139): warns only if prey*2 < members, i.e. under 0.5 prey per hunter, and counts EVERY other-owner animal within reach: burrowed voles, graced rivals, learned-poisonous herds, other carnivores. pt5's repro (46 voles/newts for 60 hunters, half burrowed) still would NOT warn. -> **partly fixed** (warning exists, threshold too lenient and prey count unfiltered). Medium.
- Same claim: 26.8 tiles from the group while ~50% of it is dormant: no dormant-share warning (travel warnings only apply to claims beyond 30 tiles). Low-Medium.

## 21:14-21:16 (day 119-140) Territory backfires, renown, hunting party
- Day 120 alert: "5 of the 10 animals holding territories (Group A (main)) are starving or parched: they obey the claim and stay inside while the ground runs out. Move them, or abandon the claim." Good alert, but the claim reply had said nothing (178 "prey", mostly burrowed voles). Pop fell to 28 by day 123; abandoned the claim day 125.
- Season 2 ended alert: "#5 of 5 in legacy". 
- Renown shop: reinforce 150 (+10 adults) and provision 40 (fed 20): both worked. Reinforcement clones are generation 2, ages 30-37 days (pt5 Low "clones keep generation 0": FIXED). Reinforce reply has `"group": 8` while I had only group 0 -> pt5 Low "reinforce reply's group is a count/not an id": still present (8 is neither my group id nor obvious). Low. Reinforce price doubled to 300 after the first (fine, shown).
- Split 12 healthiest adults into group 1 (ok, `{"group":1,"kept":21,"moved":12}`), migrated 61 tiles north to look for rival herds: reply includes brooding note "3 brooding parents stay on their nests until the eggs hatch, then follow." (pt5 #10 brooding parents pulled away: FIXED / explained), destination `prey_nearby 185`, `drinkable_tiles 22`, `dryness 0.64`, warning "61 tiles from the group". Good.
- UX (predator): rivals are invisible until stumbled on (set_view refused out of sight, spectate hidden). Fair, but a predator's only option is blind migration. Low.

## 21:17-21:21 (day 149-201) Group B finds Herdmaster6; toxic prey
- Group B "reached its migration target (58, 22)" 61 tiles in ~9 days (alert clear). set_view there ok (radius clamped to 8.04 though I asked 16; pt5 Low still present).
- 68 of Herdmaster6's warning-coloured toxic herbivores (lineage 4, learned poisonous) in view. Attack order -> ok with warning "#10541 looks poisonous to your kind ... (on their own they pass it up 85% of the time)" -> **pt5 "avoidance capped at 85%": CONFIRMED** (warning colours at cap; code `avoid_cap 0.85`). The hunter killed and ate it at once: health 100 after (tolerance 70%).
- Helper switched to attack learned-toxic rivals: 18 orders on lineage 4 in ~3 min of wall time, ~6 kills, **1 toxin death** (day 188) in that period. Pack 33 -> 49 (day 188) -> 45 (day 201). Herdmaster6's log confirms the death alerts name my wolves ("by ApexHunter6's Cinder Wolves (5)") and a lineage-level "Your lineage is being hunted" warning -> **pt5 #11 death notices naming hunters + predation warning: FIXED** (cross-checked from the prey's side).
- `poisonous` grew to [1,4,8] by day 201: **lineage 1 = the wild Steppe antelope**, whose preset has no toxins (crates/sim/src/presets.rs). /api/species still shows Steppe antelope 100% wild. Presumably Herdmaster6 x antelope hybrids (both warm size-1 walking herbivores) born to antelope mothers carry lineage 1 and poisoned my wolves, so one player's toxin now protects a wild species from all player predators. **pt5 #7 (player toxins marking wild species poisonous): still present.** Medium.
- Leaderboard day 201: Herdmaster6 32.6% (1,406 alive, a warm walking warning-coloured toxic herbivore), GeneBroker6 6.5% (488, camo, also learned toxic: lineage 8), Rulebender6 6.3% (311, cold toxic warning flier), me 1.1% (45), PocketNomad6 0.9%.

## 21:21-21:23 (day 211-228) Second winter; UI attack flow
- 2nd reinforcement (300) landed at (62,17) beside group B. Provision #2 (40, fed 20).
- UI attack flow (Playwright, real clicks): Lineage tab list -> Animal panel shows "Possess (releases #14588)" (explicit, nice) -> possession bar "Possessing #19763: click to walk ... Attack / Inspect / Release" -> Attack -> hint "#19763: click the animal to attack." -> clicked a GeneBroker6 Broker Buck -> toasts "Released #14588: you possess one animal at a time." and "Attack: Organism 16572 is down in its burrow, out of reach: wait for it to come out". No console errors / failed requests. Shots: shots/ApexHunter6-atk-*.png.
- Note: clicking an animal in the Lineage list only selects it (bar kept showing the earlier possessed #14588); fine but I first thought I had possessed the new one. Low.
- Genes tab "Births and deaths" chart lists "poisoned (toxic prey)" as a cause, and the "Why are my animals dying?" link is there. Good.
- Day 228 leaderboard: Herdmaster6 44.7% (1,140), wild 37.6%, GeneBroker6 7.4%, Rulebender6 6.3%, PocketNomad6 2.4%, me 1.6% (41) last.

## 21:24-21:29 (day 240-302) Pure carnivore stalls; retire and re-found (live birth)
- Severe winter (days 210-240): alert at day 210 says "(days 211–240)" while /api/catastrophes says start_day 210 (pt5 Low off-by-one: still present). Survived 45 -> 36 with 2 provisions; "Survivor" achievement.
- Spring: whole pack (merged) to (58,25): claim refused "overlaps another player's territory; claim ground of your own" (no owner named: pt5 radar note FIXED). Rally ok: prey_nearby 163 for 32 hunters, no warning.
- Pure carnivore after 260 days: 45 founders + 20 reinforcements + 2 provisions -> 31 alive, 1.1% share, last of 6. Eggs: 250 lost since founding (237 chilled, 13 raided) vs ~25 births: incubated eggs for a roaming hunter are a trap (my mis-click, but the designer gave no warning for a pure carnivore pack on incubated eggs; the Lineage tab shows the numbers).
- Herdmaster6 (warm walking warning-coloured toxic egg-laying herbivore) hit 2,098 (hard cap) at 35-44% share. **pt5 #1: the cold toxic *flier* no longer dominates (Rulebender6 cold flier 4-6%), but a warning-coloured toxic herbivore still does.** For a predator, the leader's herd is "poisonous" (85% avoidance on instinct) so it can only be eaten by explicit orders, one at a time.
- Retire via UI: now an in-page dialog (#retire-confirm), not native confirm() -> pt5 Low FIXED. Its text: "Founding again within 375 game days (about 30 real minutes) of retiring founds half the founders you design." But the designer and the re-founding alert describe a different rule: "founders cost double points (you get as many as the budget then buys, at least half)" and I got 40 of 40. **Retire dialog wording is stale/contradicts the designer.** Low-Medium.
- Designer remembered my last design ("Starts from the design you founded last") -> pt5 Low FIXED (name left empty, fine). Birth radio still on incubated eggs from last time.
- Designer budget: "78.3 / 100 points · founding now: 40 of 40 founders" but "35.7 points left to spend" (100-78.3 = 21.7). pt5 Low "Budget vs costing" mismatch: still present in a new form. Low.
- Rulebender6's nameplate on the leaderboard reads **«Head G.M.»** -> pt5 #5 "G.M." passes the title filter: STILL PRESENT. Medium (staff impersonation).
- Re-founded "Ember Stalkers": pure carnivore, LIVE birth x3, body .4, 40 founders (to test the <=40 winter-grace rule), 78.3 pts at double cost. Alerts: "Re-founded: 40 of 40 founders (...)" and "Newcomer grace: 61 days (about 5 minutes of real time)..." (fresh grace; 61 = extended to the year's end at day 360). Landed (95,84).

## 21:29-21:31 (day 302-330) Live-birth pure carnivore, reach
- Live birth: 40 -> 52 by day 318 (15 young) despite 2 founder starvations; much better start than the egg-laying wolves.
- Renown shop counters reset after the re-founding (reinforcements_bought 0, price back to 150): bought 2 more (+20). With 529+ renown banked a player can retire/re-found to refresh the 2-per-lineage reinforcement limit (minor; costs the old lineage). Low.
- Attack reach: orders accepted at 11.7 and 14.0 tiles (distances from my <=2 s old view; the 14.0 target likely closed in). The out-of-reach refusal text in code now reads "#X is N tiles from your #Y (orders reach R tiles): get closer first" (pt5 Low "it moved since your view updated" wording: FIXED in code). set_view radius 16 -> clamped to 11.3 (varies with my spread; pt5 Low still present).

## 21:32-21:34 (day 343-360) Winter on live birth; grace through winter; era end
- Live-birth pure carnivore grew through winter: 52 (day 318) -> 79 (day 343, hunger 30) -> 94 (day 358), only founders starving (1-2/day). With incubated eggs the same pack shrank every winter: **birth mode, not diet, was most of my first lineage's failure.**
- Flood alert: "Flood: 28% of the rivers are over their banks... 12 of your animals stand on floodplains or lake margins (0 asleep). Move groups to high..." -> **pt5 #4 no flood alert: FIXED** (counts mine at risk).
- Grace (40 founders, founded day 299) ended exactly day 360 = end of winter -> **first-winter extension works** for <=40 founders. Alert at founding said "61 days" without mentioning that it was extended for winter or why (Low).
- "Era 1 has ended: you finished #5 with a mean 0.9% of breeding alleles over the era (1.5% at its end)." -> **pt5 #12 era ranks from one snapshot: FIXED (mean)**.
- Season 6: #5 of 6.

## 21:34-21:39 (day 360-410) Hunting toxic rivals after grace
- Helper on rivals: 62 orders total on learned-poisonous lineages (mostly Herdmaster6's lineage 4) this session; each order warned "pass it up 85%". Kills land (e.g. "your #29115 killed #22565"), but the new lineage pays: since re-founding (day 299-409): 34 births; deaths 23 starved, **8 toxin**, 5 heat, 3 eaten, 2 thirst. Toxin harm tolerance (70% x lesson) doesn't make the leader's herd a safe food source; a pure carnivore that hunts the warning-coloured leader still loses ~1 hunter per 8 orders.
- Pack plateaued: 98 peak (day ~385) -> 82 (day 409); share 2.0-2.2%, 4th-5th of 7 (ahead only of players whose lineages crashed). **pt5 "can a pure carnivore compete (not just survive)": still NO** - it survives (live birth: 40 -> ~90 and holds), but tops out near 100 while the warning-coloured toxic herbivore sits at the 2,100 cap (36-44%).
- `poisonous` now [1,4,8,9,11]: every player herbivore lineage (4 Herdmaster6, 8 GeneBroker6 camo, 9/11 PocketNomad6) plus the wild antelope. Every rival chose toxins again; the only safe meat is wild voles/newts/foxes (and the leader's toxin hybrids have spoiled the antelope).
- Death causes now list "heat" separately (pt5 #11/Low "cold, heat or winter sleep" lumped label: FIXED).
- Rally probes for the prey warning: prey_nearby 395-592 at random land spots (counts all animals), never a warning; rally to two water spots refused "mostly_water ... choose ground with some land" (good).
- Ranks: Rulebender6 «Head G.M.» shown in Legacy, Largest lineage and Hall of fame lists (pt5 #5 still present). "Survival 1st of 5 (tied, no title)" for season 4 in my record — fine.

## 21:39-21:41 (day 410-430) Misc
- **New (Low-Medium): a hunter killed by prey that fought back is reported as eaten by a herbivore.** Alert: "Founder #27526 was eaten by Herdmaster6's Rolling Thunder Herd." (day 385) - that lineage is a pure herbivore. Code: `World::kill_by` (crates/sim/src/world/mod.rs ~1677) files "a hunter killed by prey that fought back" as DeathCause::Predation, so the text says "eaten" and daily summaries count it under "eaten". Expected: "was killed defending itself / gored by ..." and a separate cause. Repro: hunt a herd (group defence) with a pure carnivore; watch death alerts.
- 3rd reinforcement accepted after day 420 (season 7 started day 421?) - reply `"group":30` (pt5 Low: reply's group field is not a group id; still present).
- Pack 82 -> 75 (day 424) with helper on non-toxic prey only; provision bought.

## 21:41-21:45 (day 447-494) Third winter
- Winter day 461: hunger mean 63, 35 of 84 above 80. Bought 3 provisions (3rd refused: "at most 3 provisions within a ranked season's length (60 days)" - clear). 84 -> 57 by day 491 (spring), mostly starved.
- My own API slip: a set_view I sent at day 326 pinned the view at (63,92) until clear_view (day 494); possessions didn't move it, so my helper saw only Herdmaster6's poisonous herd and gave no orders for ~45 days. The web client's "Pin view" toggle makes this visible; over the API nothing in the state says the view is pinned vs following. Low (API UX).
- Herdmaster6 51.9% (1,856) at day 477; wild down to 29%.
- Day 506: attack on wild Steppe antelope #43077 (lineage 1) whose look is **camo** (the preset antelope is drab skin) -> warning "looks poisonous to your kind ... pass it up 70%". Confirms player genes (camo + toxins, probably GeneBroker6's Broker Bucks or Herdmaster6) inside the wild antelope lineage, which now counts as poisonous for my wolves. The hunter killed it and stayed at health 100. Steppe antelope alive: 490 (day 201) -> 59 (day 507).
- Spring recovery: 57 (day 491) -> 76 (day 507).

## 21:45-22:00 (day 494-672) Fourth year; wrap-up
- Pack oscillates 57-97: winter dips (84 -> 57 in winter 3 even with 3 provisions; 67 -> 72 in winter 4 with 3 provisions), spring/summer recovery, renown reinforcements each season (+10 each, 150/300). Kills by orders: 69 over the session (185 orders); toxin deaths ~10 over both lineages, all on learned-poisonous prey.
- Non-founder deaths to predators are only "1 eaten" in daily summaries (no hunter named); only founder deaths and the lineage-level "being hunted" warning name the hunter. Low.
- Late in the game the **cold, warning-coloured toxic fliers surge again**: LateComer6 "Late Skylarks Six" (cold flier, warning) 1,625 alive / 18% at day 670 after a late founding; PocketNomad6 "Pocket Gliders Again" (cold flier, warning) 488. Herdmaster6's warm toxic walker holds the cap (2,100, 41%). -> pt5 #1: **partly fixed** - the flier no longer wins from the start, but toxic + warning colours (walker or flier) are the top two strategies; every top lineage is toxic.
- Rulebender6 has a (dead) lineage named **"Meadow vole"** (/api/spectate lineage 12), the wild species' name: name filter lets a player lineage impersonate a wild species. Medium (pt5 #5 family).
- 22:00 stopped my watcher and helper (kill by PID). Final day 672: Herdmaster6 40.8% (2,100), LateComer6 18.1% (1,573), PocketNomad6 6.1%, GeneBroker6 2.5%, Rulebender6 1.9%, **ApexHunter6 1.3% (96 alive), 6th of 6 players**.
