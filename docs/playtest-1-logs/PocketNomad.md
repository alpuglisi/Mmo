# PocketNomad playtest log (iPhone 13 emulation)

Started 11:53 UTC.

## Onboarding (11:52–11:56 UTC, game days 22–47)

- 11:53 d22 Login screen fits iPhone 13 fine. Register worked first try.
- 11:53 d22 **Comprehension**: design page is ~4,900 px tall on the phone (Found button at y≈4825). A newcomer has to scroll through ~45 sliders to reach "Found lineage". The footer text says "90.5 points still unspent. The starting design is a sturdy herbivore..." — it is unclear whether leaving 90% of the budget unspent is a mistake. As instructed I founded with the starting design (9.5/100 pts).
- 11:53 d22 Hint "Name your lineage (Basics, at the top) to found it" — good, but only visible at the very bottom of the page, far from the field.
- 11:54 d25 Founded "Pocket Nomads". Tour appeared (9 steps), fits the screen. Screens: shots/PocketNomad-05-tour-*.png
- **Tour/labels mismatch (minor, confusing)**: tour text says "Seasons tab" (step 7) but the phone tab is labelled "Ranks" (id tab-seasons); "Matings tab" vs label "Mates"; "Genetics tab" vs label "Genes"; "Animal" tab is the inspector. Step 3 says "The Lineage tab ... ranks every player by legacy share" while there is also a separate "Ranks" tab — a newcomer can't tell which to use for standings.
- 11:54 Top bar date is truncated on iPhone 13: "Year 1 · day 25 · 2..." — the hour/season is cut off by the renown pill (see shots/PocketNomad-04-after-found.png).
- 11:55 Renown climbs steadily (54 → 120 in ~3 min) with no explanation on the pill of why/what for (tour mentions Market only).
- 11:56 First-time hints ("A pregnancy", "A death in your lineage") pop up over the bottom sheet area and cover the map near my herd; they appear back-to-back. OK but cover the lineage sheet.
- 11:57 Lineage tab: tapping an already-"active" tab closes the sheet (toggle). After the tour ends, Lineage is highlighted as active although no sheet is open — first tap therefore appears to do nothing visible... actually opens; minor.
- Console: only WebGL "GPU stall due to ReadPixels" warnings (headless SwiftShader), no JS errors so far.
- d44 leaderboard: wild 85%, Herdmaster 8.5%, GeneBroker 5.2%, me 1.3% (30 alive), ApexHunter 0%. "Weighted" column is 0.0% for every player and 100% for wild — confusing for a newcomer (what is weighted? why 0?).

## Early play (11:56–12:01 UTC, days 50–106)

- 11:57 d61 Selecting an organism on the phone does NOT open the inspector; only a floating "Possess #id" button appears. You must know to open the "Animal" tab. The tour told me "tap any organism to inspect it", so I expected the inspector to pop up. **Comprehension**.
- 11:58 d73 Opened Animal tab ~10 s later: "#2356 is no longer in view: it may have died or wandered out of sight". Fast world (1 day ≈ 5 s) makes the inspector go stale before a phone user can read it.
- 11:58 d76 Possessed #1398 via FAB, toast "Possessing #1398. Tap the map to walk." Tapped a spot ~150 px away; within 3 s it was eaten (walked straight into a group of predators). No warning, the "Release" FAB just vanished, no "your possessed organism died" message beyond the generic death toast. **Newcomer trap**: predators (GeneBroker's omnivore "Brokers") look like every other brown quadruped at this zoom; threat is not readable on the phone.
- 11:59 d79-88 Starter design (herbivore, fur, hibernation, 20 founders) is being eaten steadily: all deaths are "eaten"; 20 founders → peak 41 → 21 by d88 → 10 by d106. **Balance/onboarding**: the recommended starting design ("a sturdy herbivore ready for its first winter") is defenseless in a world where another player runs a pack of omnivore hunters next to the spawn. New players spawn right in the middle of other lineages (my founders spawned inside Herdmaster's herd and GeneBroker's hunting ground).
- 11:59 World tab says "Pocket Nomads · herbivore · merged into Steppe antelope", while the Family tab says "Species: Pocket Nomads" with a branch "Herdmaster Grazers ... (13 yours)". Inconsistent species naming between tabs.
- 11:59 Seasons ("Ranks") tab legacy shares (Herdmaster 3.4%) differ from the Lineage leaderboard (Herdmaster 8.5%) at the same moment — one is a season mean, the other the snapshot, but neither table says so clearly. Confusing.
- Market: 263 renown accumulated but no listings exist; nothing to spend it on yet.

### Strategy switch (12:00, ~day 95 — early, because the starter lineage was collapsing) — going competitive
Reading the rules: score = share of *alleles* tagged to me among all breeding adults, wherever they live (wild animals included). Wild voles (~740 adults) dominate the allele pool. Mates are fertile when body-plan genes (thermo, birth type, size, diet) are within 0.12 and genome distance < 0.08. Plan: re-found as a **vole mimic** (warm, live birth, size 0.12, herbivore, burrowing/claws/camouflage/caching like voles, litter 6, very easygoing mate choice, ~90 extra founders) so my alleles introgress into the biggest wild population.
- **Exploit/griefing note**: there is no "abandon lineage" button and re-founding requires extinction, but a player can trivially wipe out their own lineage on purpose (Migrate group command into a predator pack, or possess-and-walk into predators) to re-roll a design. I did exactly that (12:01 d106: Migrate all → GeneBroker's hunting ground at 112,56).

## 12:01–12:06 UTC (days 106–170): trying to re-roll, API probing
- Possession is one-at-a-time (possessing another releases the first), and a walk order into deep water is silently not followed (organism just flees/wanders, no "can't go there" feedback on the phone). So deliberately killing off a lineage to re-roll is slow: Migrate into the GeneBroker hunting ground for 20 game days killed almost nothing — my animals even bred there. Migrating toward a mountain (55,59) with no water: still 8–13 alive after 50 days. (Good from an anti-reroll standpoint, but there is no honest "retire lineage" button either — a stuck newcomer with a doomed starter lineage cannot start over.)
- 12:03 FAB label was stale once: tapping organism #14164 left the FAB reading "Possess #6963" (overlapping markers; tap hit-test picked a neighbour). On a crowded phone screen it is easy to possess the wrong animal.
- WS API probing (own token): possess/order_move on another player's organism → `not_owned` ✔; request_mating price 1e11 → `bad_price` (max 1,000,000) ✔; migrate to (-500, 1e30) → clamped to map edge ✔; claim_territory radius 1000 → clamped to 8.9 ✔. Server-side validation looks solid.
- **Impersonation (from code, accounts.rs `name_in_use`)**: account names are compared case-sensitively (`by_name.contains_key(name)`), so "herdmaster" / "HerdMaster" can be registered next to "Herdmaster" and appear on leaderboards and in mating offers as a look-alike. (Did not create such an account; read from source.) Severity medium (griefing/scam via stud market).
- **Shared-IP rate limits**: limits.rs keys login/read/command buckets by client IP. All players behind one NAT (or, here, all on 127.0.0.1) share a bucket, so one aggressive client can throttle everyone else's commands. Concern, not tested (no flooding).
- d165: wild 86.1%, GeneBroker 11.1% (823 alive), Herdmaster 2.7% (317), me 0.08% (8 alive), ApexHunter extinct (0).
- Top bar shows "Year 2 · day 166" — day is the absolute day, not day-of-year; with 120-day years this reads oddly next to "Year 2". Season table on the Lineage tab says "Year 1 summer · days 31–43" so a year is 120 days, while the World tab era is "Days 1–360" — three different time units with no explanation.

### New plan: predator mimic
preview_mating against GeneBroker organisms says the only body-plan gap is diet (carnivory 0.34–0.36), fertility otherwise ~0.72, and their mate preferences already accept my look (mismatch 0.18). Organisms that are "kin" are never hunted. So a design = starter body + carnivory ≈0.35 should (a) not be hunted by GeneBroker's 800 omnivores, (b) interbreed into the biggest player population so my alleles ride their success, and (c) spend the ~88 unused points on ~125 extra founders (founders are breeding adults → immediate share). **Balance concern**: copying the leader's body plan to become un-huntable kin and parasitise its gene pool is cheap and probably dominant.
- 12:06 d186 Help dialog (? button) also says "Seasons ranks players; alerts badge the Genetics tab" — the phone tabs are "Ranks" and "Genes". Same label mismatch as the tour.
- 12:06 Tapping a GeneBroker organism (#2066) on a zoomed-in crowded map selected wild #17182 instead; hit-testing among overlapping sprites picks the wrong animal (second occurrence).
- 12:07 Market: GeneBroker listed stud #2040 for 8 renown. "For my adult" select reads "(none: no compatibility)" as its first option; Buy button disabled with tooltip "pick one of your adults" (tooltips don't exist on touch — on a phone there's no explanation why Buy is greyed out). Correctly prevents buying an incompatible mating.
- 12:08 Territory on phone: Tools → Territory… → "Tap the center…" — if a bottom sheet (e.g. Market) is open, the tap lands on the sheet and nothing happens; the hint doesn't say to close the sheet. After closing it: tap centre, tap edge → "Group A (main) holds a territory of radius 6.0 at (55, 58)". Works.
- Server "nearby" view stays centred on my lineage (radius 16), not on my camera — panning the phone camera elsewhere shows terrain but no other animals unless "Pin view" is used. Not explained anywhere.
- d220: wild 73.9%, GeneBroker 20.5% (987 alive), Herdmaster 4.6%, ApexHunter re-founded 0.9%, me 0.04% (6 alive).

## 12:11–12:21 UTC (days 240–335): the lineage refuses to die; switching to growth
- My 6 survivors rebounded to 26–28 in spring once GeneBroker's horde moved off (they were dormant through winter). The re-roll plan is dead: there is effectively no way to retire a lineage. **Newcomer problem**: a player who regrets the starter design is locked into it for as long as a handful of animals survive (could be hours of real time).
- 12:13 Long-press on own organism (CDP touch, 900 ms) possesses it ✔ — matches the tour.
- 12:18 Mates tab: inspector "Mate this one…" pre-selects the organism even if it is already pregnant. Typing a partner id shows a green "Compatible: fertility 100% · within both preferences · 3 tiles apart." even though the female is pregnant; Send then fails with a red toast in developer wording: "request_mating: the female is already pregnant (already_pregnant) (and 1 more like it)". Expected: preview to say "already pregnant" and the select to mark/disable pregnant females. (shots/PocketNomad-38-mating2.png). Also, on a phone you must type the partner's numeric id — there's no picker from the map in this form.
- 12:18 Sex ratio drifted to 13 males : 5 females among adults; every female becomes pregnant on her own, so directed matings within my lineage add nothing.
- 12:19 Listed three studs (#26459, #28891, #33603) at 2 renown, 60 days: if other players buy, my alleles enter their lineages (legacy counts genes anywhere). Listing UI on phone works.
- 12:20 Claimed max territory radius 12 at (55,56) (API; UI flow tested earlier).
- 12:20 Lineage panel shows "home range: true, protection_left_min 720" on my group even while I'm connected — state is ambiguous (is it protecting now or not?).
- d318: wild 78.1%, GeneBroker 16.6% (731), Herdmaster 4.7% (252), ApexHunter 0.38% (41), me 0.25% (28).

## 12:20–12:25 UTC (days 330–400): catastrophe wipe-out, re-found
- **Missed catastrophe**: a "severe winter" catastrophe ran days 330–360 and starved my whole lineage (28 → 0; 5 starved, 2 thirst per day). I had stopped watching for ~90 s of wall clock (≈20 game days). The alert feed shows NO alert when the catastrophe *began* — only "The a severe winter is over: 2864 fewer organisms…" after it ended (day 361). Expected: a prominent warning (toast + push) at the start, ideally a few days' notice. With 1 day ≈ 5 s, a phone player who glances away for two minutes loses everything.
- **Text bug**: alert reads "The a severe winter is over: …" (double article). See Genes tab alert feed, day 361.
- Era 1 ended the same day: "Era 1 has ended: you finished #4 with 0.0% of breeding alleles."
- Extinction notice works: Lineage tab shows "Your lineage has died out… Start a new lineage. Your renown, records and titles stay." After extinction the map view still draws my old territory circle (green line) with no animals.
- Re-found design screen still says "The starting design is a sturdy herbivore ready for its first winter" even after I changed diet to 0.35 (omnivore) — static text.
- 12:22 d378 Re-founded "Nomad Brokers": starter body + carnivory 0.35 (GeneBroker's body plan), fat 0.8, hibernation 0.8, caching 0.5, pickiness 0.1, tolerances 0.9, sociability 0.9, **141 founders** (99.5/100 pts). preview_mating vs a GeneBroker male: compatible, body-plan gap none, fertility 0.71 (genome distance ≈0.14 > species_distance 0.12, so not "kin" — they can still hunt me).
- **Balance: extra founders are the best buy in the game.** 141 founders → 180 alive and **5.08% legacy share within 20 game days** (3rd place, ahead of ApexHunter, just behind Herdmaster 6.35%), vs 0.5–1% for the default 20-founder start. Founders are instant breeding adults; 0.7 pts each is far cheaper than any trait. The starter design leaves 90 points unspent and the UI never suggests founders.
- 12:23 Founders started starving within ~15 days (crowding: 141 animals on one spot). Spawned at (7,32) on the far west edge.
- 12:24 Split via Tools → Split: message "Group A (main) split: 106 went to Group B, 106 stayed" but the group list then showed 106 and 112 members (totals don't add up to the 218 alive). Migrated Group B to (38,62).
- **Scouting**: `set_view` accepts any point with radius up to 32, so 16 calls map every player's animals across the whole map (positions, diet, actions). The phone UI's Pin view offers the same. Plus market listings expose exact gene values and live x,y of listed animals → a predator player can locate and hunt them (listed animals are also exempt from offline protection).

## 12:25–12:32 UTC (days 410–495): growth, then a flood year
- 12:24 Group B's first migration target (38,62) was on the mountain — on the phone at low zoom the grey mountain is not obviously impassable and Migrate accepts any point. Drowned deaths appeared during migration (route across water). Re-targeted B to (46,30).
- 12:28–12:29 d454–470 winter: 311 adults all hibernating, share 7.6% → **8.68% (3rd)**. Peak so far.
- 12:29 Listed 4 more studs at 1 renown/120 days (spread alleles to GeneBroker, whose body plan matches). Renown 1143 → 1690 in 5 min: renown accrues much faster than there is anything to spend it on (no listings by others except one).
- 12:30 d480 **Flood year catastrophe** (480–503). Again **no alert in the alert feed when it started**. On the phone, the only sign is a red pill in the top bar: "Flood year: relentless rai…" — it *replaces* the date/season text, truncates, and pushes the renown pill and "Log out" onto two lines (header overflow, shots/PocketNomad-50-flood.png). Full text ("…deep water that weak swimmers cannot cross · 9 days left") is unreadable on the phone; no tap-to-expand.
- The hidden weather line simultaneously says "flood 0%" while the flood-year catastrophe is active — contradictory numbers (`weather.flood: 0` in state as well).
- Flood deaths are mostly **thirst** and starvation (e.g. day 488: 15 thirst, 8 starved), not drowning — counter-intuitive in a flood; probably swollen water cuts animals off from drinkable shores. No guidance on what a player should do.
- d488: 311 → 228 alive; share 6.5% (4th by a hair behind GeneBroker 16.2% / Herdmaster 18.2%). Herdmaster is overtaking GeneBroker.
- Death-toast spam: during the catastrophe the map shows a constant stream of "Founder #… died (thirst)" and "Listing #20 closed: #51161 is no longer alive" toasts stacked over the top of the map.

## 12:32–12:35 UTC (days 503–530)
- 12:32 Claimed territories for both groups (r 20/21, accepted). Founder cohort now dying of **old age** together (all 141 founders were spawned at similar ages → synchronized die-off ~day 500+). Balance note: the big-founder strategy front-loads share, then crashes as founders age out; litter 2 can't replace them.
- 12:33 Herdmaster sits at exactly **1500 alive** = `lineage_cap` default (1500) — the cap is binding for the leader; config comment suggests it should scale with map area (128² map → 750?) but the leader reaches 1500.
- 12:34 World tab bugs/oddities (shots/PocketNomad-51-world.png):
  - Era 1 header "you #4" but the table lists PocketNomad in row **5** (wild counted as #1 in the table, excluded in the header).
  - Species "Herdmaster Grazers (2) · founded by Herdmaster" shows "**PocketNomad 100% of alleles**" — founder attribution contradicts allele ownership.
  - "Nomad Brokers · merged into Brokers (2)" — confirms the body-plan mimic merged my new lineage into GeneBroker's species within ~130 days; "Nomad Brokers (2) … founded by PocketNomad · since day 511" earned me "Species founded: 1".
  - Catastrophe list: "Flood year · days 481–503 · population 4990 → 6530" — a catastrophe whose population *rose* 30%; and the API says start_day 480 vs UI 481 (off-by-one between 0- and 1-based days). Top bar "Year 5 · day 495" vs API year 4 (0-based year in API, 1-based in UI — fine, but the API/UI mix is confusing in debugging).
- d523: wild 57.7%, Herdmaster 21.4% (1500, capped), GeneBroker 15.2% (1099), me 3.8% (260), ApexHunter 2.0% (664).
- 12:35 Rally works on phone: "Group A (main) gathering at (18, 30), within 11.7 tiles". (Rally silently replaced Group A's territory claim — the group's territory became null; not stated in the toast.)
- 12:36 d530→551: crash **260 → 79**, almost all **thirst** (8/day) in early autumn with drought 0.34 and no catastrophe. Most survivors are already "dormant" (hibernating) in early autumn with thirst 20–50 and some awake ones at thirst 80–100. **Possible balance bug**: hibernation (0.8) sends animals dormant ~20+ days before winter while thirst keeps rising; dormant animals die of thirst. Design doc says hibernators need "almost no winter food" but nothing about water.
- d551: Herdmaster 20.9% (1499), GeneBroker 15.4% (1170), ApexHunter 2.3% (917), me 1.49% (80). Dropped to 5th... (4th of players).

## 12:37–12:40 UTC (days 568–580)
- 12:37 Reload: map back in ~1.1 s, tour does not reappear, no console errors ✔. Logout → login with wrong password → "Wrong name or password." (plus expected 401 in console) → correct password logs in and resumes the game ✔.
- 12:39 **Correction / root cause for "no catastrophe warning"**: the server does emit "Catastrophe: … has begun (until about day N)" (game.rs `chronicle_alerts`), but (a) Alert preferences default the *world* kind to **Show on / Toast off**, so a catastrophe start never toasts on the map, while per-founder death alerts *do* toast; and (b) the alert feed is capped, and during a die-off dozens of day/death alerts push the "has begun" entry out of the feed within a few game days. Net effect for a phone player: the catastrophe start is effectively invisible. Recommend: world/catastrophe toasts on by default, pin active catastrophes at the top of the feed, and collapse per-founder deaths.
- Same function: the end text is `format!("The {what} is over…")` with `what = "a severe winter"` → "**The a severe winter is over**" (grammar bug, also likely "Catastrophe: a severe winter has begun").
- Old session token is replaced on login (had to refresh my API token) ✔.

## 12:40–12:45 UTC (days 600–655): hitchhiking on GeneBroker's gene pool
- 12:40 d600 Since World tab lists my lineage as merged into GeneBroker's species "Brokers (2)", I migrated Group B (31 animals) from (44,32) into GeneBroker's range (84,30) to interbreed. By d626 the Genes tab "Whose genes" shows 6.8% GeneBroker alleles in my animals → gene flow both ways works. **Exploit/balance**: a small player can parasitise the leader's population (offspring of mixed matings born to GeneBroker females count against *their* lineage cap but carry my tagged alleles). The design doc's countermeasures (outbreeding depression) don't bite because the mimic's body plan is identical.
- 12:43 Migration through the east caused **drowned** deaths (1–2/day, d647–649): group migration paths cross deep water for non-swimmers even though the design doc says pathfinding avoids impassable tiles. Many of group B were "flee"ing on arrival.
- 12:44 Claimed territory r10 at (80,29) for group B; group A (22) stays west.
- Server "nearby" stays on group A, so the phone shows nothing around group B unless I pan + Pin view; with two distant groups the client can only watch one.
- d649: Herdmaster 24.4% (1500, cap), GeneBroker 11.1%, ApexHunter 2.1% (915), me 1.03% (46).
- 12:47 **Griefing vector — stripping offline protection with mating offers**: a pending directed-mating offer (either side) makes both organisms *exempt from offline protection* (server/src/game/offline.rs doc: "Organisms in an open directed-mating agreement (either side, pending or accepted) … are exempt"). Sending a request costs **no mating slot** while pending (slots stayed 6/6 after two offers) and needs no consent; up to `max_open_offers` = 8 at once. So a rival can target up to 8 of a logged-out player's last survivors, removing the 1/4-kill-chance shield exactly when it matters, and re-send when declined. Tested: two offers to ApexHunter females accepted as "pending" (then I cancelled them).
- 12:47 **Info leak**: request_mating against another player's female returns `already_pregnant` — reveals hidden state (UI says needs/health of other players' organisms are hidden). preview_mating also leaks exact body-plan gene gaps.

## 12:48–12:52 UTC (days 700–745)
- Era 2 (days 360–719) final: wild 62.5%, Herdmaster 20.8%, GeneBroker 12.8%, ApexHunter 2.6%, **PocketNomad 1.35% (4th of 4 players)**. Era 1: 0.02% (5th row / 4th player).
- Winter again killed animals by **thirst** while dormant (d724–726: thirst 3,2,1 + starved 3) — repeat of the hibernation/thirst pattern.
- Spring d744: 50 alive (30 adults, 20 young), 0.94%.
- Could not test flier/swimmer/toxic/horned designs: the API/UI only allow a new design after extinction, and my lineage never fully died again. (A design sandbox/preview vs. the live world would help experimentation.)

## Wrap-up (12:59 UTC, day 835)
- 12:53 Ranks tab: "Largest lineage: 1 ApexHunter 1500 · 1 Herdmaster 1500" — two lineages pinned at the hard cap.
- 12:59 Headless browser driver process exited (code 144, externally) at the end of the session; no client-side error had been logged before. Only console errors over the whole session: the expected 401 from a deliberately wrong password, and SwiftShader "GPU stall due to ReadPixels" warnings.
- **Final**: day 835 · 42 alive (all adults, wintering) · legacy 1.51% (weighted 1.83%) · 4th of 4 players (wild 66.0%, Herdmaster 17.3%, GeneBroker 12.2%, ApexHunter 2.9%). Peak 8.68% (3rd) at day 470 with the 141-founder omnivore mimic. Titles: Species founded 1st (season 9), Survival 1st (season 9).
