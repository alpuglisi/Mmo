# PocketNomad9 log (phone newcomer, iPhone 13 emulation, 5 fps, DPR 1)

## 08:38–08:50 UTC (game days 12–30)
- Registered via phone UI (login screen fine). Designer opened with empty name (fresh account, expected), starter: warm, incubated eggs, litter 3, 20 founders, 14.0/100 points.
- PT8 #4/#8 large-clutch warning: FIXED. Litter 8 → "A brooding parent warms 4 eggs at most: of each clutch of 8, the other 4 chill whenever the nest is cool, so about 4 hatch. The extra eggs still cost points..." (shot 03-litter8-warn).
- PT8 egg-chill warning: FIXED (shown in spring for incubated eggs: "Incubated eggs still chill on cold nights...").
- PT8 #8 estivation advice: FIXED - drought warning now says "Estivation (0.6 or more) ... but it has costs: a sleeping lineage doesn't breed, so a dry spring can cost a season's young, and it wakes needing food."
- Drought was rising fast in spring: 48% (day ~13) → 58% → 64% at founding (day 24).
- Founded "Pocket grazers" (starter) day 24. Feed: grace 126 days (extended through first winter, explained), drought provision (64% dry, start watered), landing text "Your founders landed around (93, 29): no other player's animals or territories on the map yet." (landing distance text present; no rivals yet to measure distance against).
- Founders land young: FIXED. Ages at day 30: 32–58 days (26–52 at landing; maturity ~25 d, lifespan ~264 d).
- Tour: 9 steps, phone layout fits; finished with Finish. (Will check it doesn't return.)
- Low: the drought banner + tour card cover ~2/3 of the phone map right after founding (shot 05-after-found) - you can't see your founders until you dismiss both.

## 08:40–08:45 UTC (days 30–60)
- Tour only once: FIXED. Finished it, reloaded (new browser context with saved storage) - no tour.
- View group reason: FIXED. With one group the button is greyed but tappable; tap → toast "View group needs two or more groups: split your lineage (Split) to look around each in turn." Low: the toast appears behind the open Tools menu (half hidden, shot 09-viewgroup-toast); the menu stays open after the tap.
- (Automation note: Playwright refuses to tap aria-disabled buttons; a human can tap them, fine.)
- Market: "Taproot grazers" (GeneBroker9) is now an allowed name (PT8 false positive fixed). Listing copy now "refunded if the two never get together before the listing closes" (PT8 wording fixed).
- Market naming picker: FIXED. "Name an animal" uses a searchable picker ("Choose one of your animals…", "#1461 female", "#5245 male (young)", "50 of 76 shown, nearest first: search by id to find the others"). Confirm "Name #1461 "Pip" for 50 renown?" → toast "#1461 is now called Pip." (shots 12, 13)
- Survey price rule: FIXED. Shop text: "The price is 30 renown plus 1 for every 10 of your animals alive, so it rises as your lineage grows." 76 alive → 37 renown, matches.
- Day 57: 63 alive (22 adults, 41 juveniles); eggs lost chilled 10 (days 31–32, early summer?) and raided 5. Leaderboard: wild 82.6%, Herdmaster9 7.8% (179), GeneBroker9 4.1% (194), Rulebender9 2.3%, ApexHunter9 1.9%, me 1.4%.
- Renown rises quickly for a newcomer: 53 at founding → 134 by day 53.

## 08:48–08:56 UTC (days 145–170)
- "While you were away": FIXED. After ~6 min away: "While you were away: 15 warnings, 2 death alerts, 2 world events, 1 birth alert. The Genes tab's feed lists them." Re-entering ~1 min later: no summary.
- Zoomed-out map: MOSTLY FIXED. Max zoom-out now fits the whole map width on the phone (shot 16-zoomed-out-clean); black bands above/below the square map remain (Low, cosmetic). Animal sprites stay large at that zoom, so a herd covers a big blob.
- Year 1 summary (days 76–150): eggs chilled in autumn (6–8 per day), founders starved/thirst in late drought (75→54), survived winter under grace, then spring egg raiders took 3–12 eggs a day for 20+ days (days 129–150). Egg failure alerts are clear and specific (chilled vs raiders, with advice).
- MEDIUM (fairness): grace ended day 150 and on day 151 ApexHunter9's Ironjaw Pack ate 15 of my 74 (and 5, 7, 4, 3 the following days: 74 → 50 in 4 days). The pack was clearly sitting on my herd waiting out the grace. "Your newcomer grace has ended" arrived in the same tick as a megadrought starting (days 151–195). For a newcomer this is a cliff: no warning beforehand ("grace ends in N days, a pack is near your herd").
- LOW: the animal I paid 50 renown to name (#1461 "Pip") was eaten on day 150, a few minutes after naming. No refund (fine), but nothing in the naming dialog says names are lost when the animal dies.
- PT8 "one death alert per founder": PARTLY FIXED - same-day founders are batched ("2 founders died: 2 eaten (#1456, #1466)") but day 150 still produced 4 separate founder-death alerts plus a daily summary plus a "being hunted" alert.
- Renown piles up for a newcomer too: 529 renown at day 159 with only ~1.5% share (achievements + season places).
- Migrated (Tools → Migrate… → tap) to (88,12), the lasting lake shore away from the pack. Server accepted (state.migration=[88,12.9]). LOW: the success toast "Adults migrating toward…" never showed on screen - only "1 brooding parent stays on its nest…" and a hunted alert were visible 2.5 s later (shot 19). A newcomer can't tell the order went through.
- LOW (phone): megadrought banner + 2 toasts cover the top ~55% of the map (shot 19).

## 08:55–09:02 UTC (days 170–240)
- Migration to (88,13) failed: "Group A (main) didn't gather at its migration target (88, 13) (half the group is still more than 19 tiles away); ... got about 24 tiles closer, then stopped" and "Group A is starving on its way to (88,13): 5 of its 12". The pack kept eating 2–7 a day through the move (days 160–197). Lineage 77 (day 149) → 4 (day 201) → 2 (day 214). Mostly eaten by ApexHunter9's Ironjaw Pack, a few starved/old age.
- LOW/confusing: day 195 two world alerts in the same tick: "The megadrought is over: 719 fewer organisms..." and "Drought: 96% of the land is dry." A newcomer reads "over" then "96% dry".
- Wall-clock note: one year (~10 min) of grace, then the pack wiped a 77-animal starter in ~4 real minutes. As a phone newcomer I had no real counter beyond Migrate (which didn't outrun them).
- Retired via Lineage tab (details "Retire this lineage" → rule text → confirm dialog with server rule). Toast "Lineage #4 retired: 2 animals left the world." LOW: the toast is drawn behind the red "You retired lineage #4" banner (shot 21).
- PT8 server's last design preferred: FIXED. I first overwrote this browser's localStorage design with a stale copy ("Stale local copy", cold, litter 2, 12 founders); the re-found designer still prefilled the server's "Pocket grazers" (warm, litter 3, 20) with "Starts from the design you founded last".
- PT8 saved last design name: FIXED (name prefilled "Pocket grazers").
- PT8 re-found fee display: FIXED for the case I could see: "Re-founding now: 10 of your 20 founders land, at the re-founding price of 14.0 points and a 100-renown fee." plus a "Re-founding soon" card with the full rule. I had 662 renown, so the "(N now, M owed)" variant never applied; could not test it without being broke.
- Re-founded "Pocket emberkin" day 239 (strongest-looking: warm, incubated eggs, litter 4, body 0.25, toxicity 0.6, vivid colour, sentry 0.5; 36.3 pts): 10 founders, 100 renown. Feed: "Re-founded: 10 of 20 founders, 100 renown (...)".
- Founders young again: ages 23–38 at landing. FIXED.
- Landing distance text: FIXED (present): "Your founders landed around (96, 68): the nearest rival animal (GeneBroker9's) is 9 tiles away; Herdmaster9's territory is 81 tiles away." But PT8 #11 PARTLY: landing 9 tiles from a rival.
- MEDIUM (newcomer fairness): re-founding gives NO grace: "No newcomer grace: it continues the grace of your earlier lineage (from day 25), which has run out." So a newcomer whose first lineage is eaten by a pack right after grace re-founds 10 animals in winter with zero protection. The rule is stated clearly (retire panel + designer), but combined with the 100-renown fee and half founders it makes recovering from a predator wipe very hard.
- Tour did not reappear after re-founding: FIXED (tour once).

## 08:57–09:00 UTC (days 240–250)
- Split with 21 animals: works ("Group A (main) split: 10 went to Group B, 11 stayed"). Then View group: FIXED/works - taps cycle "Viewing A" → "Viewing B" → back to whole lineage, toasts say so, the view window pins on each group.
- (Split-unavailable reason: verified in code/aria - "Too few animals in this group to split." / "You have 4 groups..." via the same soft-disable+toast mechanism as View group; I did not have a too-small group to tap.)
- LOW bug: the new lineage's Group A carries the RETIRED lineage's last outcome: state.groups[0].last_outcome = "migration to (88, 13) ended: stalled after 24 tiles" right after re-founding (an order the new lineage never had).
- Possession on phone: tap an own animal → FAB "Possess #20558" → bar "Possessing #20558: tap to walk (even onto other animals); long-press one to select it." Inspect/Release. Smooth.
- Re-founded lineage bred at once: 10 founders → 28 (18 hatchlings) by ~day 245 (early spring).

## 09:00–09:06 UTC (days 250–300)
- The toxic+bright small egg-layer re-found grows fast: 10 → 25 (d249) → 50 (d274) → 100 (d290) → 139 (d300), even with no grace; ApexHunter9's pack took only 2–3 at a time (vs 15/day from the drab starter). Strong evidence the "strongest" archetype is still warm small incubated-egg toxic bright; the starter design is a trap for newcomers when a pack is around (MEDIUM balance/newcomer: the default design is the one a pack eats).
- Ranks tab (phone) readable. Lineage tab confirms the stale-outcome bug in the UI: new lineage #10's "Group A (main) ... last order: migration to (88, 13) ended: stalled after 24 tiles" (that order belonged to retired lineage #4). LOW.
- Season standings: season 5 legacy 5th (0.4% mean) while the live share is 1.2%. Wild holds 86%; Herdmaster9 leads players at 6.0% / 662 (no runaway this time so far).
- Territory claim via Lineage tab "Claim territory…" → hints "Tap the center…", "Tap the edge… (radius up to 15 tiles)" → "Group A (main) holds a territory of radius 9.2 at (97, 64)". Clear on phone. Home range protection shows in state (72 min budget).

## 09:02–09:05 UTC (days 300–315)
- Survey: button "Survey nearby rivals: 44 renown" (140 alive → 30+14 = 44, matches the stated rule); confirm "Survey for 44 renown?" same fresh price (PT8 stale price FIXED). Result dated: "Survey taken on day 311: GeneBroker9: about 20 animals near 108, 52 / Rulebender9: about 410 animals near 100, 108 · territory at 105, 105 (radius 20) / ApexHunter9: about 160 animals near 60, 60".
- LOW: just after claiming a 9.2-tile territory: "Group A (main) has outgrown its territory at (97, 64): it feeds about 63 of the group's 64 animals now." An alarm for 63/64 feels premature (and the claim reply itself gave no such warning).
- Reinforcements: "Buy reinforcements for 150 renown? 10 young adults copied..." → "10 adults joined your lineage at (90, 64), beside 77 of your animals." Clear.

## 09:05–09:09 UTC (days 315–348)
- Peak 239 alive / 4.8% live share (day 343, 4th of players by share, ahead of GeneBroker9). Then ApexHunter9's pack (34 hunters visible) moved into the herd at (91,71) with 24 wild foxes: 7/day, then 23 eaten on day 346. 239 → 217.
- MEDIUM (balance, predator side): toxic + vivid prey is NOT protected once a pack settles in: 21 of my toxic bright animals eaten by the pack in one day (day 346). Honest signals in, so hungry hunters take bright toxic prey; good for predators, but it means the "strongest" herbivore class now has no defence a phone newcomer can use (Migrate doesn't outrun a pack).
- Genes tab on phone: clear. "Last 30 days: 155 born, 113 died", cause legend, "18 alerts from before this lineage (day 240) hidden. Show them" (nice).

## 09:09–09:13 UTC (days 348–412)
- Listed a stud on the Market (picker "#28919 male · gen 3", fee 12) → toast "#28919 listed for 12 renown." Easy on phone.
- Day 400: 196 alive, 5.5% live share - 3rd of 6 players (Herdmaster9 18.5% / 701, ApexHunter9 7.2%, me 5.5%, Rulebender9 3.3%, LateComer9 2.0%, GeneBroker9 0.3%). Pack takes 4–14 a day, egg raiders 3–16 eggs a day; births roughly keep pace.
- MEDIUM (PT8 #13 still present): renown piles up even for a small player: balance 1,280 renown at day 403 with ~5% share, after spending ~400. Every sink I can use as a newcomer (survey 44, provision 40, reinforcements 150 x2/season, naming 50) is tiny next to the income. Because of this I could not reach the "(N now, M owed)" re-found fee display at all - a newcomer never owes anything.
- 2nd reinforcement bought on day ~405 at 150 again (first was day ~311, outside the 60-day window - consistent with the rolling-window rule). Provision: "Buy a provision for 40 renown? Your lineage's 20 hungriest or thirstiest adults get 50 off..." → "20 adults fed and watered."
- "While you were away" correctly absent on short re-entries (<5 min).

## 09:13–09:16 UTC (days 412–463)
- Season 7 ended (day 420): I placed #3 of 6 in legacy, #2 of 6 in lineage - best result so far.
- Migrated all (Tools → Migrate…) to the lake at (64, 99), ~38 tiles away from the pack at (93,72). This time the success toast showed: "Adults migrating toward (64, 99): the group keeps its territory at (97, 64) while away: ...". Group B (199) arrived day 433. The pack followed the whole way: by day 463 29 pack hunters sit at (60,96) beside my 101 at (63,95). Fleeing a pack with Migrate does nothing; the journey also cost 1–3 starved/thirst a day. 239 → 113 → 101.
- LOW: "Group A (main)" ended with 0 members (2, then 0) while "Group B" holds all 101; the group called "(main)" is empty. Its migration alert: "never got going: the way may be blocked..." for a 2-animal group.
- PT8 #14 survey with empty group 0: FIXED - survey works with Group A empty ("Survey for 39 renown?", ~93 alive → 30+9, rule holds).
- Balance: Herdmaster9 jumped to 32% share (795) by day 455 while wild fell to 46%: the herd leader is running away again in year 4 (cf. PT8 #1). ApexHunter9 7.6% (237) - the best predator so far, mostly on my animals.

## 09:16–09:21 UTC (days 463–526)
- Lineage #10 kept falling under the pack + egg raiders (7–17 eggs raided/day, 0 births for days): 101 → 65 (day 518) → 48. Retired it on day 525 to test a second re-found.
- Retire confirm still only quotes the general rule ("costs 100 renown after a retirement ... doubling"); it doesn't tell me MY next fee will be 200. LOW (the designer does say it).
- Designer after 2nd retire: prefilled the server's last design "Pocket emberkin" (litter 4, toxicity 0.6, colour 1, body 0.25) - FIXED/consistent. Fee doubling shown correctly: "Re-founding now: 10 of your 20 founders land, at the re-founding price of 36.3 points and a 200-renown fee."
- Estivation 0.7 in the designer during a 65% drought: the estivation advice disappears (correct) and the stronger drought text shows.
- Re-founded "Pocket diggers" (emberkin + burrowing 0.6, 41.1 pts) day 526 for 200 renown, 10 founders aged 20–33 (young - FIXED).
- HIGH (fairness, PT8 #11 still present): "Your founders landed around (108, 57): the nearest rival animal (ApexHunter9's) is 1 tiles away; LateComer9's territory is 19 tiles away." My graceless re-found landed ONE tile from the pack that had just eaten two of my lineages. The landing text is honest, but the founding search should keep graceless founders away from a predator pack. (LOW wording: "1 tiles".)
- LOW (again): the new Group A shows the retired lineage's outcome "migration to (64, 99) ended: never got going".
- Day 554: Pocket diggers 10 → 25 → 23; the pack (16 visible at (102,62), 6 tiles off) started picking off founders on day 550. Bought reinforcements (150): "10 adults joined your lineage at (109, 54), beside 18 of your animals." Season 9 (day 540): 5th of 6 legacy, 4th of 6 lineage.

## 09:22–09:32 UTC (days 556–681)
- "While you were away" after ~5.5 min away: "While you were away: 4 world events, 2 warnings, 1 achievement, 1 death alert, 1 market alert, 1 milestone." Correct again.
- Season 10 (day 600): 6th of 6 legacy, 5th of 6 lineage. Flood year days 601–623; "Survivor" achievement (+25) for coming through it - my lineage grew 28 → 76 during it, so it was automatic (PT8 #13 survivor achievements near-automatic: still present, Low).
- Pocket diggers (toxic, bright, burrowing) recovered: 80 (day 630) → 301 (day 681), 3.9% live share, tied with ApexHunter9. The pack hovered 9 tiles off but took only 1–3 a day once burrowing was in.
- My stud listing (#28919, 12 renown) never sold; the market stays thin (PT8 #7).
- Balance: Herdmaster9 35.1% / 1,680 at day 681 (wild 42.7%); runaway leader again (PT8 #1 still present in year 5).

## 09:35 UTC (day 725) - final
- Pocket diggers 535 alive, 9.8% live share, 2nd of 6 players (Herdmaster9 22.2% / 710 dropped from 1,680; LateComer9 8.2%; ApexHunter9 7.1%; Rulebender9 3.3%; GeneBroker9 0.5%; wild 48.8%).
- No page errors, console errors or failed requests were logged by any of my Playwright sessions (lib.mjs prints ERRS on close; none printed). Server up throughout.
