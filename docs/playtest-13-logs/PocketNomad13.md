# PocketNomad13 playtest log (iPhone 13 emulation in Chromium, touch, 5 fps cap, DPR 1)

Start: 2026-11-20 00:09 UTC (server at game day ~10). Plan: default starter, never re-found, accept all mating offers.
Driver: one headless browser (pid in scratch pn13/driver.pid), controlled via a local eval port; toasts/console/failed requests logged to pn13/events.log.

## 00:10 (day ~16) Register + design + found
- Designer on phone: starter = warm, incubated eggs, litter 3, 30 founders, toxicity 0.50, colour 0.60, fur 0.60, **hibernation 0.80, body fat 0.80** (new). 37/100 points. Text: "safe first lineage (... it sleeps through winter on ample fat; 30 founders)". Hibernation caption "~1 in 10 die asleep".
- **Grace wording: FIXED** in designer: "Yours may hunt rivals' unprotected animals, but your lineage's first attack on a rival's animal (even one your animals start on their own) ends your grace."
- Designer interbreeding: "No wild or rival species is in breeding range of this design... no other player's lineage would be fertile with these founders" (measured, 16 sampled). Note: GeneBroker13 also has exactly 30 alive, so probably also a starter-ish design, yet sterile? (watch).
- Founded unchanged as "Pocket Wanderers" (day ~16-24).
- **Landing toast: FIXED (shown once)**: "Your founders landed around (113, 15): the nearest rival animal (Rulebender13's) is 89 tiles away." + "Drought provision: 16% of the land is dry..." both visible ~6 s, 2 lines (38 px). Polled every 1 s for 25 s: one visible copy. (DOM: my MutationObserver saw the two toast elements re-created once at +5 s while still visible, no visual duplicate.)
- Founding alert grace wording also FIXED ("Yours may hunt rivals' unprotected animals, but your lineage's first attack ... ends your grace"; "60 days ... ends on day 79").
- Tour: 9 cards, all fit the 390 px phone. Tip cards ("A pregnancy") fine.
- Low (still present): daily summaries stamped a day early in model feed: "d25 daily_summary: Day 26: ...".
- World tab species diet labels: all players' species carnivory 0.0 "herbivore"; Red fox carnivory 0.61 "omnivore". No 0.3–0.4 species to check the pt12 case yet.

## 00:12–00:14 (days 44–56) First offer
- Starter: 30 -> 111 by day 53, 0 deaths. #1 of 5 players by share (3.4%).
- Incoming offer from GeneBroker13 (#1207, price 8). Card: "Your nearest suitable animal is about 96 tiles away: they would meet halfway. Their #1207 is 74 of about 263 days old." **FIXED**: halfway wording (pt12 said "three quarters"), animal age shown.
- Accepted ("nearest free one" -> #1162). Toast "Mating offer 1: accepted" (one line). Accepted card: "the partner is about 44 tiles W of yours. They meet near 68, 20" (Low: the pending card said 96 tiles apart a second earlier; 44 vs 96 unexplained).
- Low (still present): "Meeting point predators are near every point between the two: the usual point is kept." (missing colon, reads garbled).
- Question: the designer said "no other player's lineage would be fertile with these founders" — so is this paid mating fertile at all? (check Market compatibility).
- Offer #1 completed ~day 63: "Mating offer 1: completed, conceived" + "Sale: 8 renown from GeneBroker13 for offer #1" (two short toasts).
- **Medium (new): designer interbreeding check contradicts the Market.** Designer at founding (day 16): "measured, no other player's lineage would be fertile with these founders; their studs would be sterile with yours." Market on day 64: GeneBroker13's Broker Clover "fully fertile", Herdmaster13 50%, Rulebender13 10% (and the GeneBroker mating conceived). A newcomer reading the designer would think trading is pointless. (Maybe the designer sampled only wild? The other lineages had founded by day 16.)
- Shop wording FIXED: "Fixed listings' sales pay no market fee; offers and auctions pay out less a 10% market fee".

## 00:16–00:18 (days 64–75) Autumn: early sleep
- Day 66 (year-1 autumn, ~24 days before winter): 89 of 126 already "dormant", hunger avg 21. Hibernation 0.8 starts sleeping very early (pt12 Low "~80% asleep by mid-summer" - here it's early autumn).
- Ranks: Season 1 legacy crown = me (1.7%). Season 2 #2 behind Rulebender13.
- Bought Reinforcements (150) via Market: in-app confirm dialog clear; toast "10 adults joined your lineage at (113, 19), beside 76 of your animals." Good.
- Grace: "ends in about 3 days... 6 of ApexHunter13's hunters are within 30 tiles of your animals" warning (useful), then "Your newcomer grace has ended". Each toast logged once visible (DOM re-created once, no visual repeat).

## 00:17–00:20 (days 76–133) FIRST WINTER — starter verdict part 1
- Whole herd dormant from ~day 76 (128/128) to ~day 125.
- **Winter-sleep starvation: LARGELY FIXED.** Peak ~136 (incl. 10 reinforcements) -> 118 alive on day 133 = -13% (pt12: -40% first winter). Deaths: ~10 "winter sleep" (~1-2 every few days, matching the designer's "~1 in 10 die asleep"), 2 starved, 1 thirst. No predation while asleep even though ApexHunter13's hunters were within 30 tiles.
- Season 2 ended: #2 of 5 legacy and lineage (alert in feed).
- Feed message "Founder #1162 starved. Further founders lost the same way are counted in the daily summary" - fine.
- Rulebender13 meanwhile 495 alive / 12.6% (others 1-4%).

## 00:20–00:23 (days 133–163) Spring/summer year 2: boom, territory, offers
- Spring: whole herd awake by day 133, breeding at once: 118 -> 284 by day 152.
- Claimed a territory via Lineage tab > Claim territory… (tap centre, tap edge), r=8 at (100,21) (I aimed at (100,30); Follow was on, camera likely moved between my coordinate read and taps — my error probably).
  - Reply "Claimed, but: At this season's growth the plants here feed about 125 of the group's 242 animals; expect hunger. ... about 8 days." + "33 brooding parents stay on their nests until the eggs hatch, then follow."
  - **Reply on top: PARTLY FIXED.** The reply's brooding line stayed pinned on top for 10+ s, but the main "Claimed, but:" line was replaced after ~4 s by an alert it triggered: "Group A (main) has outgrown its territory at (100, 21): it feeds about 117 of the group's 244 animals now..." then "Founder #1154 died of attrition", "Founder #1176 was eaten by ApexHunter13's Duskmaw Pack". So the less important half of my reply is pinned while the warning half is pushed out (Medium on phone). Also the "outgrown" alert 4 s after the claim just repeats the claim's own warning (Low).
- 3 offers from GeneBroker13 at once (#6, #7, #9), **#6 and #9 for the same animal #1703**; toasts folded as "GeneBroker13 asks...#1703", "(×2) ... #1732", "(×3) ... #1703" — the ×N counter counts different offers as repeats (Low). Accepting #6 auto-cancelled #9 ("Mating offer 9: cancelled") — fine but the rival could double-book an animal.
- Accepted card #6: "the partner is about 56 tiles W of yours. They meet near 28, 12 (W of yours)" — meeting point ~85 tiles W of my animal, i.e. beyond the partner itself ("Meeting point moved to keep clear of a predator near the usual point"). Low/Medium: walking past the partner is odd.
- Hunger in the territory: "63 of the 150 animals holding territories (Group A (main)) are starving or parched..." and "Your lineage is going hungry: on day 157 6 starved and 2 died of thirst (3% of it)..."
- **Clear naming its group: FIXED** — Tools menu button reads "Clear A", tooltip "Cancel the orders of Group A (main)...". Reply "Cleared: Group A (main)'s territory" stayed pinned on top while other alerts cycled (good).
- Sale: 6 renown for offer #7 (completed quickly).

## 00:21–00:24 (days 154–190) Need-alert budget, epidemic, autumn crash
- Toasts in the 2.5 real minutes after the bad claim (days 154–182), distinct need alerts: territory no longer feeds (1), "N of the M animals holding territories are starving" (2), "going hungry" (4: days 157, 160, 178, 182), "dying off slowly" (1), **"Your eggs are failing" 6 times (days 154, 162, 166, 169, 172, 175 — every 3–4 days)**. **Need-alert budget: PARTLY FIXED** — no more one-per-day 4-way flood, but the egg alert alone fires every 3–4 days for weeks (Medium: noise; a newcomer can't act on it, and in summer the advice "nest in shelter" doesn't help).
- **"dying off slowly" still used for big falls (Low/Medium, pt12 #10 still present):** "Your lineage is dying off slowly: 293 → 260 alive ... 81 died (58 of hunger, 13 of thirst, 10 eaten): 28% of the 293" and "dying off slowly: 258 → 196 alive (24% fewer) ... 66 died (36 of thirst, 25 of hunger, 5 eaten): 26%". 24–28% in 10 days is not "slowly".
- Catastrophe: epidemic days 163–177 ("The epidemic is over: 850 more organisms in the world than at its start" — odd but fine).
- Season 3 ended: #2 of 5 legacy & lineage.
- Autumn (day ~186): 250 -> ~200 by day 190, mostly thirst + hunger on the bare home range (drought). Migrated via Tools > Migrate… (west, to green): reply "Order given: Group A (main) adults migrating toward (85, 19). But: The way runs about 15 tiles ... no drinking water ... 44% thirsty ... 46% of the group is dormant (hibernating) and won't walk until it wakes; the order may run out before then. Winter sleepers bed down early..." — informative; reply stayed on top while "While most of your lineage sleeps (59% asleep): ... dying of thirst; ... crashing" fold appeared under it. Good.
  (My first taps were offset by the canvas's top offset — my scripting, not a bug.)
- Group names now capitalised: "Group A (main) adults migrating" — pt12 lower-case Low FIXED.

## 00:24–00:26 (days 190–206) SECOND autumn/winter: crash
- **Migration end alert: FIXED.** Feed/toast: "Group A (main) didn't gather at its migration target (85, 19) (half the group is still more than 29 tiles away); the order has ended: 86% of the group is dormant ... migrate again once it wakes." Group card last_outcome "migration to (85, 19) ended: dormant".
- **Crash: 306 alive (day 156) -> 94 (day 206), -69%.** From daily summaries: days 155–188 (late summer/autumn, awake, drought ~?) ~150 died mostly starved + thirst on the grazed-bare home range (my territory r=8 made it worse days 152–160); days 189–198 ~80 more died of thirst/starvation as they bedded down (94–100% asleep by day 195–204); then **ApexHunter13's Duskmaw Pack eats the sleepers: 1, 1, 1, 2, 3, 5 a day (days 195–206)**, "your lineage is being hunted by ApexHunter13's Duskmaw Pack (6): 6 eaten in 6 days" while 100% asleep. Toxicity 0.5 + warning colours give sleepers no protection (pt12 #3 sub-point "toxicity didn't stop the pack eating 24 sleepers" — STILL PRESENT, Medium/High for a newcomer since nothing can be done).
- Winter-sleep deaths proper ("winter sleep") are only ~1/day — the hibernation itself is fine; the losses are pre-sleep drought hunger/thirst and pack predation on sleepers.
- The fold "While most of your lineage sleeps (N% asleep): ... said again in 3 days at most" works: one toast per ~3 days during sleep. Good.
- Leaderboard day 206: Rulebender13 17.8% (1592), GeneBroker13 5.2%, Herdmaster13 2.2%, me 2.0% (92), Apex 1.5%. Rank 4/5.

## 00:26–00:29 (days 207–225) Shop, survey, retire dialog, World tab
- **"not now" reasons on tap/phone: FIXED** — the Market shows the reason as visible text under the button: "Reinforcements (10 adults): not now — reinforcements are for lineages of at most 300 alive at any time in the last 60 days (you had 306 at the most)".
  - Balance (Medium): the starter peaked at 306 for one day and crashed to 69 within 60 days; the 300-peak rule locks a crashing newcomer out of the only "rescue" item exactly when it needs it. Use current size, or the peak in a shorter window.
- Provision (40): in-app confirm, toast "20 adults fed and watered." (while 100% asleep). Low: the sleep fold alert says in the same minute "Sleepers can't be moved or fed" — but Provision does feed them; the advice should mention Provision.
- Survey (37): confirm dialog "You see where rivals' groups and territories near your main group are now (to the nearest 8 tiles)". Result only appears inside the Market panel ("Survey taken on day 214: ApexHunter13: about 90 animals near 108, 44 · territory at 103, 48 (radius 14)"), no toast — easy to miss on phone (Low). The Survey button stays enabled right after, so my second tap bought a second survey (Low).
- **Retire dialog count: FIXED** — "Retire lineage #5? Its 69 animals and their eggs leave the world..." with population 69. (Cancelled; not retiring per plan.)
- **Names (Medium, pt12 #4 STILL PRESENT):** Rulebender13 named its species (the Glintwing Frost lineage) **"Nomad Pocket II"** (/api/species id 4, named:true, namer 1) — a word-reversal of my player name PocketNomad13, shown at #2 in the World tab species list above my own "Pocket Wanderers". (The " II" suggests a duplicate check fired on "Nomad Pocket" but the reversal of a rival's name is still allowed.)
- **Low/Medium (new): catastrophe "survivors" wrong.** World tab: "Epidemic · days 163–177 · population 5442 → 6292 · 0 of 5 player lineages survived" (/api/catastrophes survivors: []), but all five player lineages are alive.
- World tab species diet labels all fine for current species (carnivores "carnivore" 1.0, Red fox 0.6 "omnivore"). No lineage has ended yet, so the ended-lineage history line can't be checked yet.
- My species shows "PocketNomad13 99% of alleles" (GeneBroker matings), no `parent` field now (pt12 Low "parent points at another player's species" — not reproduced, FIXED or n/a).

## 00:28–00:32 (days 240–275) Year-3 spring: recovery attempt
- Winter 2 tail: 94 (day 206) -> 60 (day 241). Pack ate ~22 more sleepers (days 207–226; 8 on day 210 alone), winter sleep ~9, old age 4. **Year-2 total: peak 306 -> 60 = -80%.** Season 4: #3 of 5.
- Rulebender13 at 25–30% share (1,269–1,334 alive): leader runaway again (pt12 High #1 — looks present; not my focus).
- Spring: home range bare (veg sum 6.3k in r8) — found a lush spot with water at (82, 38) (37k) 25 tiles SW, away from the Apex pack. Tools > Migrate… reply: "Order given ... But: Destination: 26% of this circle is water the group can't stand in. 3 brooding parents stay..." 
- **Migration reached alert: present** — "Group A (main) reached its migration target (82, 38); the order is done, and the group keeps it as its home ground until its next order (claim a territory to hold it firmly)." Nice: the group card shows `home` (pt12 "herd drifts home after a migration" looks addressed).
- Claimed r=9 at (82, 37) via Tools > Territory…: reply "Group A (main) holds a territory of radius 9.0 at (82, 37)" + "14 brooding parents stay on their nests...". **Again the main line was replaced after ~5 s by an incoming offer toast, while the secondary "brooding parents" line stayed pinned** — consistent behaviour: the pinned toast is the first/side line of the reply, not the reply proper (Medium on phone).
- Offers #15 and #18 from GeneBroker13 accepted. #15 pending card: "nearest suitable animal is about 64 tiles away: they would meet halfway"; accepted card: "the partner is about 52 tiles W of yours. They meet near 44, 28" — my animal at ~x83, so it walks ~39 of 52 tiles (3/4 of the way), not halfway (Low/Medium: wording vs behaviour; pt12 had the opposite complaint). #18 completed at once ("8 tiles away"): "completed, conceived" + "Sale: 6 renown".
- Day 293: 186 alive (60 -> 186 in 50 days), 2.3% share, #4 of 5 by share. Offer #15's partner died of thirst en route, substitute sent ("GeneBroker13 sends #10816 instead"), later "expired ... 5 renown of the fee is forfeit to PocketNomad13". Low: "Mating offer 15: accepted" toast/feed line repeated 3 times (days 271, 282, 292) for one offer as the partner was substituted.
- Split (Lineage tab): toast "Group A (main) split: 97 went to Group B, 98 stayed" (fine, no confirm). Group B auto-"Picked".
- Group B claim at (94, 20) r9 (its old, now regrown home): reply "Claimed, but: The straight way there (18 tiles) passes a few of ApexHunter13's hunters within 10 tiles... plants here feed about 89 of the group's 112 animals" — visible <1 s, then replaced by "Group B has outgrown its territory at (94, 20): it feeds about 88 of the group's 113 animals now" (redundant restatement 0 s later). Same pinning problem.

## 00:33–00:36 (days 300–327) Autumn year 3, nameplate, Ranks
- Season 5: #5 of 5 legacy, #4 lineage. Day 319: 174 alive (Group A 63 at (82,37), Group B 111 at (94,20)).
- Pre-winter: pack predation again from day ~294: ApexHunter13's Duskmaw Pack eats 2–4 a day, continuing while 71→98% asleep ("6 eaten in 3 days", "4 eaten in 3 days"). Starved 1/day before sleeping.
- Low: "Group A (main) has outgrown its territory at (82, 37): it feeds about 68 of the group's 69 animals now" — 68/69 is not "outgrown" (alarmist threshold), and it comes while the group is asleep.
- Bought a nameplate (500) with title "Wandering Pocket Keeper": in-app confirm, toast "Nameplate set: it shows next to your name in Ranks, the hall of fame, the leaderboard and on your animals."
- **Ranks titles on one line: FIXED** (each row one line, 25 px) — but on the 390 px phone both names and titles are ellipsized: "Rulebender… [Rule Pro…]", "PocketNom… [Wanderi…]" (Low: you can no longer read who's who at a glance; my own name truncated). Hall of fame line shows full: "Most titles: Rulebender13 «Rule Prober» 4♛ 9 · PocketNomad13 «Wandering Pocket Keeper» 1♛ 1".

## 00:36–00:37 (days 320–348) THIRD winter: pack predation is now the killer
- 219 alive (day 300 peak) -> 112 (day 348). Days 320–348: ~70 eaten by ApexHunter13's Duskmaw Pack (2–5 every day, steady), ~15 "winter sleep", a few old age/thirst. Group A (63, closer to the pack) fell to 22; Group B (111 -> 90).
- **Balance (High for the newcomer question):** the deeper hibernation fixed winter-sleep starvation, but a sleeping starter herd is now a larder for any pack within ~25 tiles: 100% asleep for ~50 days, can't be moved, toxicity 0.5 + warning colours don't deter, territory "home range" protection only applies while offline. The Apex pack (~120) seems to live off my sleepers each winter (year 2: ~30 eaten; year 3: ~95 eaten since day 294).
- Alerts during sleep: one folded alert every 3 days ("While most of your lineage sleeps (100% asleep): your lineage is being hunted by ApexHunter13's Duskmaw Pack (8): 8 eaten in 4 days...") — budget OK.
- Low: "Group B's territory at (94, 20) has no drinking water left" in mid-winter while the group is asleep, then "is grazed bare (food for about 3 days)" — both irrelevant to sleepers (pt12 "grazed-bare blamed for winter-sleep starvation" — the wording now adds "most of the group is asleep and moves only when it wakes; plan where it goes then", which is OK).

## 00:38–00:41 (days 360–380) Era 1 result, year-4 spring
- "Era 1 has ended: you finished #4 with a mean 2.1% of breeding alleles over the era (1.7% at its end)." Season 6: #4 of 5 legacy, #3 lineage. Leader Rulebender13 ~29%.
- **Winter 3: 219 (day 300) -> 82 (day 372) = -63%**, almost all predation by ApexHunter13's pack on sleepers + ~1/day winter sleep. Group A (near the pack) 63 -> 7.
- **Merge confirmation: FIXED** — Lineage tab "Merge into A": toast "Group B merged into Group A (main): 75 animals joined it."
- Survey day 378: Rulebender13 ~480 near (44,20) territory r18; GeneBroker13 ~140; ApexHunter13 ~130 near (52,44); LateComer13b ~140 near (116,28) (a late joiner). Kept the lush (82,37) territory (veg 51k + water) since the pack has moved 30 tiles off.

## 00:39–00:42 (days 383–408) Year-4 summer: the pack finishes the herd
- Day 399: 78; day 407: **27 alive**. ApexHunter13's pack (~31 visible, centred ~15 tiles S of my territory) took 4–9 a day while awake, plus drought thirst (territory's water dried: "has no drinking water left: abandon it or move the group. 3 of the group died of thirst yesterday").
- **Crash-alert flood (Medium, pt12 #10 STILL PRESENT for crash alerts):** four "Your lineage is crashing" toasts in 48 s (≈10 game days) with shifting baselines: 00:41:07 "102 → 69 alive over the last 10 days"; 00:41:21 "89 → 39"; 00:41:38 "85 → 29"; 00:41:55 "76 → 23". Each repeats the same advice. Plus "Group A (main) has recovered: it keeps inside its territory again" at 00:41:38 in the middle of a 70% crash (contradictory tone).
- Also earlier same window: "being hunted" alerts on days 388, 391, 395 + "dying off slowly" days 393, 397 ("dying off slowly ... 38 died ... 36% of the 107") + "starving or parched" days 394, 398: ~10 need alerts in 10 game days.
- Escaped: Migrate order to (97, 10) (north, lake, ~45 tiles from the pack): "Order given: adults migrating toward (97, 10): the group keeps its territory at (82, 37) while away: no rival may claim over it..." (clear).
  - Low (UI): with the Tools menu left open, the Territory tool still shows "active" and the hint "Tap the center of Group A (main)'s territory" — my map tap landed on the open menu's Migrate… item instead and switched modes. A newcomer could hit the same.
- Accepted GeneBroker13 offer #20 (price 8): "partner is about 64 tiles NW of yours. They meet near 52, 20".
- 5th crash alert at 00:42:04: "52 → 23 alive over the last 10 days" — five crash alerts in 57 s, baselines 102/89/85/76/52.
- Reached (97,10): "Group A (main) reached its migration target (97, 10)...". Low/Medium: the migrate reply had promised "the group keeps its territory at (82, 37) while away ... it is held again if you clear the order", but after arrival the group has no territory (only a home ground at the target) — the old territory silently lapsed.
- Season 7: #6 of 6. Claimed r7 at (97,11): "Group A (main) holds a territory of radius 7.0 at (97, 11)". Bought Reinforcements (allowed again: peak under 300 in last 60 days): "10 adults joined your lineage at (96, 11), beside 23 of your animals." then instantly "Group A (main) has outgrown its territory at (97, 11): it feeds about 23 of the group's 26 animals now" (Low, alarmist).
- World tab: Era 1 table — I finished #4 (mean 2.1%). Niche prizes to ApexHunter13 (carnivores) and GeneBroker13 (tundra). **No ended lineages exist on this server** (all 5 original + LateComer13b alive), so the "ended-lineage history line" can't be verified.

## 00:43–00:49 (days 425–495) FOURTH winter (no pack), year-5 spring
- Away from the pack, winter 4: 34 (day 428, incl. reinforcements) -> 23 (day 484) = -32%: 7 "winter sleep", 4 old age, 1 disease, **0 eaten, 0 starved**. For a small herd the winter-sleep death rate (~20%) is double the designer's "~1 in 10 die asleep" (founders are old, which may explain part).
- Low: territory alerts not folded into the sleep fold: "Group A (main)'s territory at (97, 11) has no drinking water left: most of the group is asleep..." 5 times (days 441, 451, 460, 467 + a grazed-bare one day 448) during the sleep — every ~8 days, for a group that can't act.
- Season 8: #6 of 6. Day 484 leaderboard: Rulebender13 30.3% (885), LateComer13b 10.3% (late joiner, 300), Herdmaster13 5.0%, GeneBroker13 2.5%, ApexHunter13 2.1%, me 0.8% (23).
- Restore pasture (200): dialog "About 3082 food of plants regrow within 12 tiles of Group A (main)"; reply "Pasture restored around Group A (main) (about 4246 food regrown) for 200 renown. The next costs 400." **pt12 #9 (delivers 14.5% less than quoted): changed — now the reply says 38% MORE than the quote** (Low: quote and result still disagree; a newcomer can't tell which number is real). Raw veg in the r12 circle doubled (35.6k -> 71.2k bytes).

## 00:49–00:52 (days 495–525) Year-5 summer
- Starter regrows: 22 -> 67 by day 518 (0 deaths in 20 days). Drought again (48% dry): no water within ~15 tiles except LateComer13b's lake.
- Offer #24 (GeneBroker13): accepted. Low: incoming card says "Their #38129 is 118 of about 265 days old" while the offer data says mine_age 138.3 days (20 days off).
- Tried to claim r12 toward the lake: refused "Territory: That circle overlaps another player's territory (rival territories near your groups show on the map); claim ground of your own (the attempt costs a short wait). Two exceptions: ..." — refusal reply stayed pinned on top (good).
- Low: "Group A (main)'s territory at (97, 11) is grazed bare (food for about 3 days)" ~35 days after I restored pasture there (raw veg in r12 ~73k) — for 67 animals in r7. Plausible but the "grazed bare" word vs the map's green is confusing.
- Cleared the territory so the herd can roam to water ("Cleared: Group A (main)'s territory", pinned).
- **Daily-summary day stamp: FIXED in the UI** — Genes tab feed shows "Day 549: 0 born, 1 died (1 winter sleep) · 63 alive · day 549" (only the raw model field is a day lower).
- Year-5 summer was short: growth 22 -> 74 (day 529), then starvation/thirst ~1/day in drought (no water reachable) and sleep from ~day 545. Offer #24's partner died of thirst en route. Season 9: #6 of 6.
- Genes drift: Hibernation mean 0.76 (-0.02) — selection is pushing the starter's hibernation down.

## 00:53–00:58 (days 550–605) FIFTH winter (severe-winter catastrophe), year-6 spring
- Winter 5: 63 (day 550) -> 44 (day 602) = -30%: ~13 winter sleep, 4 old age, 1 attrition; 0 eaten (pack far away), 0 starved — even through "a severe winter (days 571–600)" which cut the world from 5,360 to 3,027. Achievement "Survivor" (+25); Season 10: #5 of 6 legacy, **#1 of 5 in survival**.
- Offer #24 expired: "#38129 died (thirst) and no other animal came ... 2 renown of the fee is forfeit to PocketNomad13 (PocketNomad13's #45158 slept through 56% of the hold)" — clear.
- /api/catastrophes: severe winter survivors [1,2,3,4,6] (sensible), but the epidemic's survivors [] although every lineage grew — the World tab's "0 of 5 player lineages survived" for the epidemic is wrong (Low/Medium).
- Flood alert at spring start: "animals that can't swim drown there, sleepers most of all" (fine).
- Restore pasture #2: quote "About 3078 food", reply "about 2779 food regrown" (-10%); #1 was +38%. Quote vs delivery still off in both directions (Low). Also #1's reply said "The next costs 400" but the next cost 200 (a new ranked season had started — the reply could say "400 until day 600").
- LateComer13b's ~211 animals now graze around (94,19), right beside my home ground (competition for the same patch).

## 00:58–01:01 (days 605–640) Year-6 summer
- 41 -> 74 (day 624) then thirst/hunger in the third drought: "Your lineage is dying off slowly: 74 → 57 alive over the last 10 days (23% fewer ...)" (again "slowly" for 23%).
- Offer #30 (GeneBroker13, price 10) accepted — GeneBroker13 sent me ~10 offers this session; I accepted all; 4 completed (8+6+6+6 renown), others lapsed when its animals died of thirst.
- Migrate toward water at (92, 33): reply warned "The way runs about 24 tiles ... no drinking water ... 53% thirsty ... Destination: it is inside LateComer13b's territory at (96, 28) (radius 14): its holders defend it ... It is 91% dry" — good warning, so I cancelled: "Cleared: migration" (Low: unlike the territory clear, this reply doesn't name the group).

## 01:01 End
- The container restarted at ~01:00 UTC and the server went away (my last check-in died with exit 137). Coordinator asked me to stop. Driver process (pid 8379) is already gone. End: 01:02 UTC, game day ~640. Final: ~60 alive, share 1.1%, #5 of 6. Starter kept the whole session, never re-founded.
