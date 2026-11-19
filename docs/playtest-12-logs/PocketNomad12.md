# PocketNomad12 playtest log (iPhone 13 emulation, touch, 5 fps, DPR 1)

Start: 2026-11-19 20:30 UTC, server at game day 5.

## 20:30–20:34 (days 5–48) Registration and founding
- Designer on 390 px phone: starter = warm, incubated eggs, litter 3, 30 founders, toxicity 0.50, colour 0.60, fur 0.60, hibernation 0.50, body fat 0.60; 33/100 points. Text "The starting design is a safe first lineage (mildly toxic and brightly coloured...; 30 founders), not the strongest" shown. Founded unchanged as "Pocket Drifters" on day ~17.
- No designer heat warning shown in spring for this furred design (nothing to warn about yet; landing spot unknown before founding). Will revisit at re-found.
- Log out in top bar: whole, not clipped (shot 06-landed). **Fixed.**
- **Landing toast shown ZERO times** (Low/Medium): polled `#toasts .toast` every 1 s for 16 s after the map opened: no toast at all. The landing text ("Your founders landed around (103, 27): the nearest rival animal (GeneBroker12's) is 68 tiles away.") is only in the feed. Likely cause from reading client: the server's alert toasts into model.toasts while the design screen is still mounted (no toast renderer), then main.ts's 1.5 s fallback sees the same text in model.toasts and skips it, and the game screen never renders toasts already in the model. Playtest-11 fix (2-3 copies) overcorrected to 0 copies. The tour card was up at the time. Steps: register on phone, found starter, watch for toasts.
- Crowded founding text: "68 tiles away" from GeneBroker12; map not crowded yet (day 17), so can't verify the crowded case.
- Lineage tab share-during-boom note present: "Most of yours are young (76 of 106): the legacy share counts breeding adults only, so it can fall during a boom..." **Fixed.**
- Low: Lineage tab "Season weight: founded on day 18" while the landing alert is stamped day 17.
- Starter growth: 30 founders -> 106 by day 41 (first clutch day 25-27, 37 young). Leaderboard day 31: Herdmaster 4.3%, me 3.2%, GeneBroker 3.2%, Apex 2.1%.

## 20:35–20:37 (days 50–81)
- Food layer zoomed out: animals drawn ~half size and see-through, Food colours readable under the herd (shots food-1-on-default vs food-2-on-zoomedout). **Fixed.** At default zoom the sprites still cover much of the herd's ground (by design).
- Incoming lineage offer from GeneBroker12 (#995 x my pick, price 8): card says "About 96 tiles apart: they meet three quarters of the way to your animal, which walks the last quarter." Distance shown. **Fixed** (pt11 #10). Accepted with "nearest free one" -> #1611; toast "Mating offer 1: accepted" (one line). Accepted card: "Lapses if they haven't met in 30 days... One of them is asleep: the meeting resumes when it wakes (the deadline waits)." 
- Low: "Meeting point predators are near every point between the two: the usual point is kept." reads garbled (missing colon/word: "Meeting point: predators are near...").
- Low: pick-own select lists 55 males including gen-0 founders; fine, but it's a native <select> of 55 items on a phone.
- Market compatibility: GeneBroker fully fertile, Rulebender 40%, Herdmaster 20%, Apex sterile. One stud listed (GeneBroker #996 for 12).
- Grace-ended toast: 2 lines (38 px). Good.

## 20:37–20:40 (days 87–127)
- Mates tab partner picker ("Choose a partner…") opens with its search and first rows inside the bottom sheet, visible (rows at y 369–481 of 664). **Fixed** (shot picker-open).
- Claimed a territory around the herd in early winter via Tools > Territory… (tap centre, tap edge). Reply: "Claimed, but: Little or no drinking water ... plants here feed about 3 of the group's 173 animals ...". Followed at once by two alerts ("has no drinking water left", "grazed bare"). Phone toasts: 2 lines max with ellipsis, tap expands. **Fixed.**
  - Low/Medium: the claim's own reply (the longest, most important text) rendered with clientHeight 0 (the oldest of 3 toasts) while the two follow-up alerts that repeat it were visible: on a phone you see the consequences, not the answer to your action. (shot claim-2)
  - UX: it let me claim a territory that feeds 3 of 173 with no water at all; a newcomer might expect a refusal or a confirm step. The warning is clear though.
- Cleared it (Tools > Clear): "Cleared: Group A (main)'s territory" + "While most of your lineage sleeps (82% asleep): Your lineage is going hungry..." Fine.
- Starter: 30 -> ~180 alive by day 81 (140 adults), share 4.1%, #2 of 5. Losses so far small (eaten 7, winter_sleep 2).

## 20:40–20:42 (days 127–138) First winter and spring
- First winter cost the starter ~40%: ~180 -> 104 (days 100–129: starved 48, winter_sleep 8, attrition 3, eaten 1, thirst 1; 2 births). Most starvation happened in hibernation. Herdmaster12 meanwhile 398. **Balance (Medium):** the starter's first winter is a big drop for a newcomer who did nothing wrong (the herd was asleep, "Sleepers can't be moved or fed"). Share 4.1% -> 3.4%.
- Eggs "chilled: no parent was brooding them, the parents were asleep" on days 122–123: the hibernating starter lays eggs then sleeps on them? Confusing for a newcomer (Low).
- Low: daily summaries are stamped one day before the day they describe in the feed ("d119 daily_summary: Day 120: ..."; "d25 ... Day 26").
- Spring Food layer: herd standing on red/bare ground, green to the south. Migrated via Tools > Migrate… to (98, 49): "Order given... But: The way runs about 25 tiles (about 3 days) with no drinking water that lasts the drought, and 19% of the group is thirsty already... 26 brooding parents stay on their nests". Clear and useful. (World drought 0.22 at the time; no drought banner visible, so "the drought" is a slightly surprising word, Low.)

## 20:42–20:45 (days 143–185)
- Lineage tab "My organisms" list: tapped 6 rows at different moments (list re-sorts as actions change); each tap selected exactly the tapped animal and switched to Animal tab (6/6). **Fixed.**
- First accepted mating (#1) failed: "#1611 was eaten by ApexHunter12's Gloamfang Pack before the mating" (my male, while walking? it was accepted; pt11 says accepted walkers are protected from other players' animals; it was eaten on day ~81 right after my grace ended — the shield maybe did not apply because the seller's animal was asleep: "awake-only shield"). Medium-ish if the shield should have held; the feed gave no alert of the loss, I only saw it in matingLog/Recent outcomes.
- Second offer #4 from GeneBroker12 (price 8): accepted; "nearest free one" picked #5800 which was ~52,28, far W of the main herd (~98,23). Completed day 160: ledger "Sale: 8 renown from GeneBroker12 for offer #4". Pending card said "they meet three quarters of the way to your animal"; accepted card said "partner is about 64 tiles W of yours. They meet near 52, 28 (W of yours)" then later "(at your animal)". OK.
- Mating toasts: during these offers I saw only "Mating offer 1: accepted"; no repeated toasts on deadline moves. **Appears fixed** (no spam observed; limited sample).
- Migration to (98,49) silently "ended: stalled after 4 tiles" (shown only in the group card's "last order"); no toast or alert seen. Medium UX: newcomer thinks the herd went to the green.
- Eggs: 314 lost since founding (294 chilled, 20 raided) by day 182 vs ~330 alive. Warm incubated eggs chill constantly (spring, early autumn, and while parents migrate). **Balance (Medium):** the starter's egg mode wastes most clutches; "Your eggs are failing" alerts come every few days.
- Bought Reinforcements (10 adults, 150 renown) via Market: confirm dialog clear; "10 adults joined your lineage at (101, 17), beside 159 of your animals." Good.
- Drought banner (56–61% dry) folded under the toolbar with "More": readable, not covering tools.
- Starter now 329 alive (206 adults), gen 4. Share ~3.6%, rank 3/5 behind Herdmaster12 (~800) and GeneBroker12 (~500).

## 20:45–20:48 (days 186–205) Autumn
- Drought: thirst deaths 1–4/day at the old spot (water there but grazed bare). Migrated to (87,16) (veg + water, 13 tiles). The order text warned well: thirst, 7% food on the way, 46% dormant "the order may run out before then". It then ended: "didn't gather ... 67% of the group is dormant ... migrate again once it wakes." So: second migration in a row wasted; in autumn a hibernating herd effectively can't be moved. Fine, but a newcomer only learns by failing (Low/Medium).
- Phone toasts: real long toast showed 2 lines (38 px) with ellipsis; tap expanded it to 68 px. **Fixed** (shot toast-expanded).
- Incoming GeneBroker12 offer #7 (#1001 x one of mine) failed on its own within ~30 s: "organism 1001 died of old age before the mating". Low: the offer let a rival offer an animal at the end of its life; the "listings show age vs lifespan" info is not shown on incoming offers.
- Toasts seen in 70 s: 5, all distinct, no repeats. "While most of your lineage sleeps (70% asleep): ... said again in 3 days at most" fold works.

## 20:49 (day 217) Names
- **Medium (names / impersonation):** GeneBroker12 named its species **"Nomad Pocket"** (`/api/species`: id 4, `named: true`, namer 2), a word-swap of my player name PocketNomad12 (and close to my lineage "Pocket Drifters"). Playtest-11 #7 "blends" were supposed to be refused; a reversed two-word split of a rival player's name passes for species names. Steps: name a species "<second half> <first half>" of a rival's CamelCase player name.
- Low: my founding species "Pocket Drifters" shows `"parent": 4` (GeneBroker12's species) in /api/species although it is an independent founding (both are the default starter design, so they are kin by genes). A newcomer may read it as "split from GeneBroker's species". Two players on the same starter are fully fertile and counted as one gene pool, which also means starter players' species detection merges.

## 20:49–20:51 (days 217–250) Second winter
- **Second winter crash: 329 (day ~180) -> 124 (day 247), -62%**; mostly starved/thirst while hibernating, plus old age. Pattern: the starter booms to ~300 each summer and loses half or more each winter. Still rank 2/5 by share (4.5%) because GeneBroker12 (also on a starter-like "Brokers Kin") crashed harder (614 -> 66). Herdmaster12 (Frostwing Drift) 460, 15.3%.
- World tab lists Rulebender12's "Gale Wardens II" (a re-found) but no line for its retired first lineage (neither "retired day N" nor "extinct"); can't judge the wording until I retire myself.
- Spring: migrated to (106,45) (22k veg, water). Order warnings again informative ("44% of this circle is water the group can't stand in").
- Accepted GeneBroker12 offer #8 (#9057, price 60). Card: "partner is about 40 tiles W of yours. They meet near 36, 28". Offer #4 recorded "completed · Conceived (100% chance to conceive)".

## 20:51–20:55 (days 250–300) Year 3 summer
- Bought a second Reinforcements (still 150: new ranked season). Herd boomed again 124 -> 420 by day 297.
- **Eggs chill in mid-summer** (Low/Medium): "on day 293 19 eggs were lost (19 chilled) and 8 young were born" (day 293 = year-3 day 53, summer; no orders active). The alert's advice ("keep brooding parents at their nests... nest in shelter") doesn't explain a summer chill.
- The spring migration to (106,45) did not stick: by day 297 the herd was back on its bare home range at (98,22) (veg 635, water 7) starving 2–4/day and thirsting. Group card showed no order. UX (Medium): migrations end and the herd drifts home; a newcomer has to discover that only a territory holds them.
- Claimed a territory at (74,22) r=8 (28k veg, lots of water). Reply "Claimed, but: ... plants here feed about 41 of the group's 410 animals; ... whole group for about 3 days." was again the hidden (clientHeight 0) third toast; visible were "2 brooding parents stay on their nests..." and an immediate alert "territory at (74, 22) is grazed bare (food for about 3 days): abandon it or move the group." **Low/Medium:** "grazed bare" for a lush spot the group hasn't reached yet is misleading; it's really "too small for 410".
- Thirst alert expanded on tap from 38 to 83 px. Good.

## 20:55–20:58 (days 300–320) Split
- Split via Lineage tab: "Group A (main) split: 185 went to Group B, 186 stayed". No confirm. Lineage tab marked Group B "Picked" in that session; in a fresh page load the Tools > Territory… said "Pick a group in the group menu first" although nothing in the visible UI told me no group was picked (the Lineage tab then showed both as "Pick"). Low.
- The pick moved me through a "claim wait": "This group (or the group it was split from) claimed or tried to claim a territory recently; it can claim again in under a minute (1.5 game days)". Fine.
- Alerts for Group A "territory at (74, 22) is still grazed bare: 3 of the group starved yesterday. Most of the group is asleep..." while the group is asleep around (89,20) and never reached the territory (it hibernated before arriving). The alert blames the territory's grass for starvation that is really winter-sleep hunger at the old range. **Medium (confusing/wrong cause)**. Also "territory no longer feeds it (16% of it is starving): the hold is loosened".
- Hibernation starts very early: 81–83% asleep on days ~305–315 (year-3 day 65–75, early autumn). So the starter's active season is ~days 0–65 of each 120-day year; any order given after mid-summer is wasted.

## 20:58–21:02 (days 320–385) Third winter, era 1 result
- **Era 1 (days 1–360): PocketNomad12 #3 of 5, mean 4.1% (5.5% at end)**. Herdmaster12 #1 (8.1% mean, 27.4% final), GeneBroker12 #2.
- Third winter: ~420 -> 144 (day 375). Group A (41 left at the (74,22) territory) was being eaten by ApexHunter12's pack: "24 eaten in 24 days" while 97% asleep, then 5–6/day in spring. **Balance (Medium):** the starter's toxicity 0.5 + bright colour did not teach the pack to leave it alone over 300 days; sleeping toxic animals are eaten freely.
- Runaway again: Herdmaster12 31.2% share / 1,413 alive on day 370 (pt11 High #2 "share runs away"; still looks present, though it may just be a strong lineage; wild 3,569).
- Survey nearby rivals (47 renown; needed a second tap on a "Survey" confirm): "ApexHunter12: about 60 animals near 68, 20" — 6 tiles from the territory I had claimed for Group A. Nothing at claim time warned that the territory sits in a pack's range (pt11 said far claims warn of hunters; this claim was only ~20 tiles from the group, so no warning). Medium.
- Migrated Group A to (104,44) next to Group B. Low wording: toast says "group a (main) adults migrating" (lower-case group name mid-sentence).
- Low/Medium (phone): Market 'Reinforcements (10 adults): not now' is disabled with its reason only in the button's title tooltip ('reinforcements are for lineages of at most 300 alive at any time in the last 60 days (you had 324 at the most)'); on a touch phone there's no hover, so the reason is invisible.

## 21:02–21:06 (days 385–435) Year 4: starvation crash
- Year-4 spring/summer: herd climbed to ~278 then crashed to ~93 by day 430: "152 died (109 of hunger, 42 of thirst, 1 eaten)". Cause: Group B's territory at (106,45) (lush in spring, claimed in winter) "outgrown": "feeds about 91 of 158" while Group A arrived on top of it. Rank fell to 5/6 (1.7%). Season 7: #3 of 6.
- **Alert flood (Medium, pt11 #4 partly fixed):** within 14 s: "Your lineage is crashing: 278 → 164 alive over the last 10 days ... 152 died" then "Your lineage is crashing: 247 → 131 alive over the last 10 days ... 149 died", plus "going hungry", "Group B has outgrown", "15 of the 48 animals holding territories are starving", "Group B has fallen from 120 to 59 adults" — 6 decline messages in ~20 s. Also the "crashing" numbers are inconsistent with each other and with the daily summaries (lineage was ~150–180 per /api/lineage/stats at the time, not 278).
- Phone toasts: the CSS shows only the newest 2 (the third is display:none). That's why the claim reply was hidden earlier: the reply came first and two alerts it triggered pushed it out. Suggest: keep the reply to the player's own action pinned over automatic alerts.
- Tools > Clear with no group visibly picked cleared only Group B ("Cleared: Group B's territory"), not all groups as the tooltip says ("the group picked in the group menu, or all"). A pick from an earlier step persisted invisibly. Low/Medium.
- A migrate tap (Tools > Migrate…, tap map) produced no toast and no order once (group card unchanged); repeating after merging worked. Merge ("Merge into A") gives no confirmation toast. Low.
- Incoming offers #11 and #13 (GeneBroker12) failed: their animal "was eaten by ApexHunter12's Gloamfang Pack before the mating" while pending (by design, pending offers are not shielded). GeneBroker12 has now sent me 7 offers; 3 completed (8+8+60 renown... #4, #8), the rest died before meeting.
- Hibernation again by day ~430 (93% dormant): migrate order "may run out before then".

## 21:06–21:12 (days 435–536) Starter verdict, retire, re-found
- Year-5 spring (day 488): starter down to 65 alive (1.4%, rank 4/6). Herdmaster12 34.4% / 1,603 (runaway; pt11 High #2 looks still present).
- **Starter verdict:** with ordinary newcomer play (migrate to Food-layer green, territory, split, reinforcements x2, accept every offer) the 30-founder starter boomed each summer (330, 420, ~280) and lost 55–65% each winter (hibernation starvation + pack predation on sleepers + chilled eggs), trending down: peak 420 -> 65 by year 5. It held #2–#3 for the first ~4 years (era 1 #3, mean 4.1%), so it's survivable and far better than pt11's "stalled at ~40", but it does not compound; a newcomer ends up retiring around year 4–5.
- Retired via Lineage tab > "Retire this lineage" (a <details>, then "Retire lineage" + confirm). Panel states the rule: "re-founding within 375 days (about 30 real minutes) founds half your founders (rounded up, at most 10) and costs 100 renown...". 
- After retiring (day 506), reloading lands on the map with "retired lineage #5. Its animals have left the world. Start a new lineage" — not "died out". Good.
- **Retired lineage "retired day N": not verifiable / partly** — the client has `retired day N` wording, but `/api/species` drops retired species entirely (its `founded` list is empty) and the World tab species list just omits "Pocket Drifters" (and Rulebender12's first "Gale Wardens"). No wrong "extinct day" label any more; also no history line at all. Low.
- Designer after retiring: "Starts from the design you founded last"; Budget: "Re-founding now: 10 of your 30 founders land, at the re-founding price of 26.0 points and a 100-renown fee"; "Re-founding soon" box explains it. Grace line: "No newcomer grace: it continues the grace of your earlier lineage (from day 18), which has run out." Clear.
- **Heat warning in designer: Fixed.** Setting fur 1.0 + body fat 1.0 shows "Heat: in summer about 32% of the land gets hotter than this design can bear (about 30 °C...)... less fur and fat, a smaller body, burrowing ... or estivation ... do." (shot designer-heat). Disappears at the starter's fur 0.6/fat 0.6. Note via API: body size 0.5 vs 0.9 gives the same "32%" (size doesn't change the figure; Low).
- Re-founded the starter as "Pocket Drifters Again": result dialog "Founded 10 of 30 founders. It cost 100 renown..." **Re-found lands at most 10: Fixed** (10 founders; 25 alive by day 536).
- Landing: feed "Your founders landed around (30, 16): the nearest rival animal (GeneBroker12's) is 10 tiles away; Herdmaster12's territory is 68 tiles away." (8-tile rule respected; crowded-map wording fine.) Drought provision line too. Landing toast: the post-found result dialog covered the first seconds; on opening the map no landing toast was shown (0 again), only feed lines. **Landing toast: over-fixed, shown 0 times** (both foundings).

## 21:12–21:21 (days 536–615) Re-found play
- Claimed a lakeside territory (33,24) r=6; bought a 3rd Reinforcements (10 adults, 150; "beside 14 of your animals"). 37 alive by day 574.
- Winter again: 37 -> 17 by day 606: **19 starved, 0 other deaths, 0 births** (days 579–608). The re-found landed mid-summer (day 524), so its young had one short season to fatten. **Balance (High for the newcomer question):** the starter's hibernation 0.5 + body fat 0.6 starves ~50–60% of the lineage every winter regardless of size (180->104, 329->124, 420->144, 278->65, 37->17). That, not predators, is why the starter never compounds. Either the starter needs more fat / no hibernation, or winter-sleep hunger needs tuning; the designer calls it "a safe first lineage".
- Incoming offer #15 failed: "organism 52892 died (winter sleep) before the mating".
- Ranks tab on 390 px: GeneBroker12's title renders as three lines "GeneBroker12 / « / Stud Farmer / »" in the narrow name column (Low, cosmetic).
- Season 11 standings (day ~610): Herdmaster12 35.9%, LateComer12 13.6%, GeneBroker12 2.1%, ApexHunter12 1.7%, PocketNomad12 0.4% (#5 of 6), Rulebender12 0.4%. Herdmaster12 at 32–36% share for 250+ days: runaway (pt11 High #2) still present.

## 21:21–21:25 (days 628–660) Final watch
- Moved the territory to (33,29): reply "Claimed, but: ... feed about 27 of the group's 29 animals ... about 7 days" (again the hidden third toast), then 7 s later "territory at (33, 29) is grazed bare (food for about 3 days): abandon it". The two estimates contradict each other within seconds (Low/Medium).
- **Territory swing still present (pt11 #4, Medium):** in 150 s: "no longer feeds it (18% starving)" (+35 s) -> "has recovered: it keeps inside its territory again" (+54 s) -> "still grazed bare" (+58 s) -> "no longer feeds it (15%)" (+96 s): about every 4 game days.
- "Your eggs are failing: ... 6 eaten by raiders" twice in 100 s for a 31-animal lineage.
- Season 11 ended: #5 of 6 legacy, #5 of 6 lineage.
- No page errors, console errors or failed requests in any session (all `errors` arrays empty). Server stayed up throughout.
