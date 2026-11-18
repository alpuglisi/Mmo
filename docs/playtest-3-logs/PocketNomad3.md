# PocketNomad3 playtest log (phone newcomer, iPhone 13 emulation, touch)

Driver: per-check-in Playwright scripts (one capped 5 fps headless Chromium, iPhone 13 descriptor, DPR 1, storageState kept between runs).

## 07:12 UTC, day ~15
- Login screen fine on phone. Registered "PocketNomad3".
- Design screen fits 390 px, sticky footer "11.5 / 100 points · founding with points left is fine / Review & found".
- Soft-cap text on design screen: "They can grow past 1,500, where breeding slows to half pace; births stop at 2,100." -> PT2 #8 (design side) FIXED.
- [Medium] The STARTING design (untouched defaults) is flagged "Interbreeds with Steppe antelope (70 adults, 100% fertile)". A newcomer who follows the onboarding with the starting design walks straight into hybrid dilution (PT2 #3 consequence). The default should sit outside every wild species' range, or the onboarding should nudge size.

## 07:12-07:15, days 30-55 (onboarding with starting design "Pocket Grazers")
- Tour: 9 steps, all fit 390x664, Back/Next/Skip/× work by tap. Texts clear. [Low] Step 2 says "Migrate sends your adults somewhere" but no Migrate button is visible in the phone toolbar (it's under Tools / Lineage groups), a newcomer can't find it from the highlight.
- Drought notice: shown at top of map ("Drought: 67% of the land is dry ... Keep herds near water; territories and rallies away from water are risky." + Less). PT2 #10 (no drought warning / hidden on phones) FIXED for the warning. [Low] it takes ~90 px (15% of the map) until you tap Less.
- Founders landed on a lakeshore in a 67% drought: good spot, no drowning seen.
- First hint "A pregnancy" appeared right after the tour; hint card sits above the floating "Possess #1713" button, does not cover it; "Got it", ×, "No more tips", "Show me" present. Dismissible: yes.
- Top bar "62 renown" pill: readable. [Low] tapping it does nothing (no explanation of what renown is or link to Market).
- Lineage tab: "Alive 37 · crowding 24 / 1,500", full soft-cap paragraph ("litters shrink toward the soft cap of 1,500 (half size at it...) ... Births stop only at the hard cap of 2,100"). PT2 #8 FIXED.
- Newcomer grace: "365 days (≈ 29 min) left: other players can't order attacks on your animals, and yours can't be ordered to attack theirs. Wild predators, and animals hunting on instinct, still can." PT2 #7 FIXED (visible, longer, two-sided). Help also explains it.
- "Season weight: full: founded on day 29, more than 10 days ago" - ramp now explained. PT2 smaller item FIXED.
- Tap accuracy on moving animals (real CDP touch taps at marker position after ~250 ms): Follow OFF 12/12 hits (herd taps open the "Here:" chooser with own first, target first). Follow ON (the default after founding) + zoom-in buttons: 8/12, the 4 misses selected a neighbour with no chooser. [Low/Medium, uncertain] With Follow on, the camera recentres continuously so a moving animal slides away under the finger; a newcomer taps a neighbour. The chooser also stays open 8 s and may catch the next tap.

## 07:15-07:18, days 60-93
- Possess via the floating "Possess #1713" button works; possession bar at y 440-514 ("Possessing #1713: tap to walk... Attack / Inspect / Release") plus a FAB "Release #1713". Possession survives a page reload.
  - [Low] Two messages say the same thing while possessing (a banner "Possessing #1713. Tap the map to walk." and the bar's text).
  - [Low] "Attack" button offered for a pure herbivore (diet 0); unclear what it would do.
- Hint cards vs possession bar: with tips reset (Help > Show tips again) and possessing, a hint ("A mating offer") appeared at the TOP (y 90-214), the bar at y 440-514: no overlap. PT2 "hint cards cover the possession bar" FIXED. [Low] The hint card now completely covers the drought notice.
- Help dialog has "How the world works" sections (Attacks, Newcomer grace, Territories and rallies, Toxins and colours, Droughts and catastrophes, Growth and the soft cap). Good for newcomers.
- Mates picker ("My adult"): search box, chips (females/males/free now/not pregnant), list with "gen", "pregnant", "promised, possessed". Fits 390 px, no sideways scroll. PT2 "huge My adult dropdowns" FIXED.
- Block player / "Who may send me requests" / "Decline every request automatically" now in the Mates UI. PT2 #15 (block UI) FIXED.
- Incoming offer from Rulebender3 (#1456 x my #1713, price 5): shown "Expires unanswered in 107 days (≈ 10 min)" (PT2 #4 offers expiring at each weather-season change: looks FIXED, the expiry is now explicit and long).
  - [Low] Right after a page load the Mates panel listed "Incoming requests: None." although model.offers had the offer; it appeared a moment later. A newcomer tapping "Show me" on the offer tip may see "None".
  - [Medium] I tapped Accept on day 90; it failed at once: "the female is already pregnant". The female was "promised" in the pending offer but kept mating naturally with my own males, and the incoming card gave no sign she was pregnant before I tapped Accept. Rival-female-pregnant problem from PT2 #5 persists: promised females should not mate naturally, or the card should say "now pregnant" and disable Accept.
  - Drought notice disappeared when the drought ended (good).

## 07:18-07:22, days 93-133 (first winter, year 2 begins)
- Winter on the starting design (hibernation 0.5): herd went 75 -> 80 through winter, no die-off. The starting design does survive its first winter as promised.
- Tabs on phone (Market, Family, Ranks, World): no sideways overflow (scrollWidth 390).
- Family tab: "Species: Steppe antelope" - my lineage was absorbed into the wild Steppe antelope species (World tab: Steppe antelope "wild 72% of alleles"); Genes tab "Only 98% of your animals' genes are yours ... 2% came from wild animals". Confirms the starting-design trap above. [Medium]
- Market "Who your lineage can breed with": wild Steppe antelope 81% fertile (birth mode 0.01 apart), Herdmaster3 19%, GeneBroker3 18%, Rulebender3 13%. Explains limiting trait. Good (PT2 #3 compares real animals now). [Low] "For my adult: (none: no compatibility)" in the stud marketplace although three player lineages are listed as fertile just above.
- Ranks: "Most titles: Rulebender3 1♛ 2" is cryptic. [Low]
- Year label: top bar "Y2 d126" while /api/world says year 1 (0-based) - API vs UI only, fine.
- Herd chooser ("Here:" chips): [Medium] on the phone it is placed at bottom:160px over the map and with 6 chips wraps to 2-3 rows (roughly y 300-450 of the canvas). It stays 8 s. A second tap/long-press on an animal under it hits a chip instead (my long-press on #1716 selected #2746 from the chooser). Also: tapping exactly on a wild animal (#222, hitTest = 222) selected my own #14684 instead (own-first ordering), so inspecting a wild/rival animal in a mixed herd needs the chooser every time. [Low]
- Long press (900 ms) on own #1724 at zoom 24 (chooser closed) possessed #4510, a neighbour. [Low, uncertain: animals moving]
- Inspector, juvenile: "Body plan: small warm-blooded herbivore · skin · drab" while the design (and adults) have fur. [Low] juveniles reported as "skin" (maybe intended).
- Founders started dying of old age at day ~125 (one year), alerts "Founder #1728 died of old age."
- 07:23 API checks: `/api/leaderboard/history?genes=true` now 401 without login; logged in, rivals' entries carry only body_size/fur/leg_length plus morph, not the full genome. `/api/spectate` gives player lineages 0 cells (only wild are located). PT2 #14 (history genes, spectate) FIXED for these two (set_view/preview_mating not tested).

## 07:23-07:25, days 150-180 (second drought, 85-95% dry)
- [Medium] BUG: on the phone the expanded drought notice (z-index 6, top 52-56 px) draws OVER the open Tools menu: it hides the group selector ("All") and "Migrate…", and taps on "Migrate…" land on the notice (Playwright: "<span class=cat-effect> from <div id=map-notices> intercepts pointer events"; elementFromPoint at Migrate = notice). In a drought, exactly when you need to move the herd to water, Migrate is unreachable until you find and tap "Less". Screenshot shots/PocketNomad3-29-tools-under-notice.png. Same will apply to the catastrophe notice (same container).
- After folding ("Less" -> one line "Drought: 95% of the land is dry · More"), Tools > Migrate… > tap the lake: toast "Adults migrating toward (41, 46): that is water or impassable; the group goes to the nearest land"; Lineage tab shows "Migration: Group A (main) toward 41, 46". Clear feedback. Good.
- Grace counter now "224 days (≈ 18 min)" after ~13 min wall time: real-time estimate consistent.
- Day 183: alert "Group A (main) reached its migration target (40, 46); the order is done." PT2 #13 (migrations never end) FIXED for a compact herd.
- Drought deaths are itemised per day ("1 starved, 1 thirst", "Founder #1719 died of thirst"); Genes chart legend born/eaten/starved/thirst/old age. Good.
- [Low] The drought notice's "Less" fold is not remembered: every reload / new session shows it expanded again (90 px).
- Season 3 ended: "#3 of 5 in legacy". Starting design: ~80-90 animals, 1.1-1.9% share after 180 days; Herdmaster3 ~430, Rulebender3 ~400. Starting design is safe but slow.

## 07:27-07:31, days 193-216: Retire and re-found with an unusual design
- Retire (Lineage tab > "Retire this lineage" > Retire lineage > confirm): clear text ("removed, not killed and not left wild... renown, records and titles stay"). [Low] confirm said "All 93 of its animals leave the world", the toast then "85 animals left the world" (population count includes something else, or stale).
- After retiring, the map shows "You retired lineage #5... Start a new lineage" which opens the design screen ("Back to the map" available). Season notice on design: "Winter starts in 15 days ... Founders arriving now start fed, with full fat reserves." Good.
- Builder mode on the phone: Eyes stepper (+ to 3 eyes -> "Depth 120° ↑100°" in the preview readout), Horns None/Curved/Straight/Branched choice sets horn size 0.4, coat pattern, wing-shape slider, presets. All tappable. [Low] The sticky Preview header covers the top ~170 px of the builder while scrolling (Legs row half hidden under it).
- Design "Horned Skywatchers": warm, incubated eggs, litter 3, 8 founders, body size 1.0 (needed for 0% fertility with every wild/rival species; the compatibility card said "No wild or rival species is in breeding range"), wings 0.6, swimming 0.5, toxins 0.5, coloration 0.6 (honest warning colours), large eyes 0.4, 3 eyes, branched horns. 67.6/100 pts.
  - [Low] Budget line labels the large-body surcharge "cold blood / small body 20.0" although this design is warm-blooded and LARGE.
  - Founding hint for 20 founders: "big animals (about 75 kg each)... around 8 founders spread out" - useful; I followed it.
  - [Low] Wings 0.6 on a 75 kg body give nothing (inspector: "walks"); the slider caption says too heavy, but the budget card / summary don't flag 14 wasted points. Easy to miss on a phone where the caption is off-screen when tapping.
- Founded day 212 in early winter at (7.5,118.5), all 8 drinking, health 100.
- [HIGH] Newcomer grace RESET on re-founding. Before retiring (day 193) the Lineage tab said "Newcomer grace 211 days (≈ 17 min) left"; after re-founding (day 215) it says "372 days (≈ 30 min) left". Help says "Founding again soon after does not start it over." Cause (crates/sim/src/world/commands.rs grace_cooldown): the "unbroken run" window is 2 x Balance::attack_grace = 40 game days, but the actual grace length is max(attack_grace, server grace_min_minutes=30 real minutes ≈ 370 days at this speed). Any re-founding more than 40 game days (~3.3 real minutes) after the previous founding gets a brand-new 30-minute grace while the old one is still running. A player can keep themselves permanently unattackable (by order) by retire/re-found every ~25 min, and the grace also blocks them from attacking... but they can choose. PT2 #7 "resets on re-founding" STILL PRESENT at playtest speed. Fix: base the cooldown on the effective grace length (ticks), not on attack_grace days.
- Season weight: "30% of your share counts toward the ranked season now; it rises to 100% over the 10 days after founding (day 212)". Clear.

## 07:29-07:32, days 216-240: Horned Skywatchers wiped out
- All 8 founders died within 18 game days of founding: thirst 6, starved 1 (lineage stats vitals days 226-230), 0 eggs laid. Founding spot (7.5,118.5) is a desert tile ('5') between mountains and more desert (biome map rows 116-124), just after a 97% drought and in winter. At founding all 8 were in action "drink" with thirst 76, i.e. already searching for water. The map shows the herd on a sand peninsula with blue all round, so a player would assume water is right there.
  - [High] The server's founding spot (pick_home, water_reachable checks can_drink_at at that instant) gave a large-bodied lineage a home where it could not drink; a newcomer re-founding gets a dead lineage within ~1.5 real minutes with no warning beyond the generic death alerts. Possibly frozen / receding water after the drought. Founding should require water that stays drinkable (not ice / not drought-edge), or warn. Also no "your founders are thirsty" alert/notice before the deaths (only per-death toasts "Founder #27068 died of thirst.").
  - Retiring is not needed after extinction: the "Start a new lineage" path appears automatically (good).

## 07:31-07:33, days 253-257: catastrophe + 3rd lineage "Glimmer Gliders"
- Catastrophe notice (red, top of map): "⚠ Catastrophe: Flood year · 11 days left (through day 263) / Relentless rain: soaked soil, swollen lakes and marsh, deep water that weak swimmers cannot cross. / Less". Clear, with countdown and end day. Also shown on top of the Lineage sheet. Good.
  - [Low/Medium] The design screen (founding during the flood) said nothing about the running catastrophe: the season note was empty and no flood/drowning warning, although founding in a flood year is exactly when a non-swimmer design could drown on landing (PT2 #9).
- Design via phone UI: cold-blooded, incubated eggs, litter 3, 20 founders, body 0.5 (≈4 kg), wings 0.8 ("flies over water, cliffs and predators"), swimming 0.5, toxins 0.6 ("very toxic: most attacks fail"), coloration 0.6 (honest warning), brumation 0.7, 4 eyes (builder stepper), curved horns. 77.8/100. Compatibility card: "No wild or rival species is in breeding range". Same "cold blood / small body 20.0" label (here correct: cold).
- Founded day 257: 20 adults basking, "Eggs: no clutch yet · 8 about to lay". Nice detail.
- [HIGH, confirmed again] Grace reset a second time: "374 days (≈ 30 min) left" (previous founding day 212, this one day 257 = 45 days later, beyond the 40-day cooldown).

## 07:33-07:36, days 260-287: Gliders growing
- Day 275: 48 alive (20 adults + 28 young), 21 eggs. Inspector: "small cold-blooded herbivore · fur · warning colors · flies", 4 eyes give "View 100% (360°) ... Sees all the way round". Flier confirmed.
  - [Low] the starting design's fur 0.6 stays in when switching to cold-blooded; nothing on the design screen says fur is pointless/harmful for a cold-blooded body (or it is fine - but a newcomer can't tell).
- [Medium] Phone camera vs world edge: the gliders were founded at y≈6 (north edge). With Follow on, the camera clamps at the map edge, so the whole herd sits at canvas y 60-77, UNDER the floating toolbar (Tools / Food buttons) and under where map notices appear (shots/PocketNomad3-44-gliders-edge.png). Taps there hit the toolbar. The camera should allow over-scroll / padding at the world edge on phones so animals can be brought below the toolbar.
- Tap test on gliders with Follow on: 8/10 direct hits, 2/10 a neighbour selected but the target in the "Here:" chooser. 0 outright misses.
- Possessed flier #31840 walked south on orders ("ordered"); thirst rose 61->89 in ~10 s, then it showed "flee" (reflex while possessed) and thirst dropped to 33 (drank). PT2 #12 "possessed animals can't drink and die unnoticed": a possessed animal did drink here (partly fixed or a reflex); no thirst warning on the possession bar while thirst was 89. [Low]
  - Update: the Genes alert feed has "Possessed #31840 is parched (thirst 76): it keeps to your order until the need reaches 85, then stops to drink; walk it to water or food, or release it." PT2 #12 possessed-drinking FIXED (with an alert). [Low] the same alert came twice (day 295 and 296).

## 07:37-07:41, days 315-356: first winter of the gliders
- Gliders: 100 alive at day 320, 0 births from day 312 on (autumn), all 100 "dormant" in winter (brumation 0.7), 0 deaths through winter. Cold-blooded brumating flier is a safe winterer.
- Ranks, season 6 (days 301-360): "You: Legacy 2nd of 6" with 1.7% (Herdmaster3 12.6%, 909 organisms). [Low] heading says "6 ranked" but lists 5 rows.
- Market: ledger shows daily income +3..+4 at 1.1-1.5% share. Reinforcements (10 adults copied from your own) 150 renown -> bought by one tap: [Low] no confirmation for a 150-renown purchase; price then doubled to 300 (PT2 #6: price escalates now; whether it resets each season / survives retire not yet checked). Toast "10 adults joined your lineage." 100 -> 110 alive.

## 07:41-07:43, days 360-375: era 1 ends, season 7
- Era 1 final: Herdmaster3 13.9%, Rulebender3 2.9%, me 2.7% (#3, weighted 3.6% - weighted would rank me 2nd). Niche leaders: "tundra PocketNomad3 47%". Renown jumped back to ~900 (era/niche prize).
- World tab: Steppe antelope now ~1460 adults, "wild 51% of alleles" - a wild species half made of a player's genes still shows only "wild 51%" (whose is the rest?). [Low] Pocket Grazers "merged into Steppe antelope".
- [Medium, PT2 #6 still present] Reinforcement price reset from 300 back to 150 at the season change (day 361). I bought two at once (150 + 300 = 450 renown) -> +20 adults; price now 600. With ~4 renown/day income plus season/era prizes, buying the cheap ones every 60-day ranked season (~5 real minutes) is a steady head-count pump that pure breeding can't match for a small lineage.
- Spring: a founder died of thirst (day ~374).

## 07:43-07:45, days 390-399: spring boom + megadrought
- Gliders boomed after winter: day 395 alone 92 hatched; 285 alive (125 adults + 160 young), 28 more eggs.
- Catastrophe "Megadrought · 37 days left (through day 435) / The rain has failed: soil dries out, lakes, marsh and streams shrink, plants stop growing." Notice OK. The drought notice is replaced by the catastrophe notice (not both stacked) - good.
- [Medium, same bug] The catastrophe notice also covers the Tools menu's group selector and "Migrate…" (shots/PocketNomad3-50-tools-two-notices.png) - in a megadrought, the moment to move herds to water.
- Gliders mean thirst 26, most basking: fliers near water ride out the drought fine so far.

## 07:44-07:47, days 408-419: territory on the phone
- Tools > Territory… > tap centre > tap edge, zoomed out (zoom 5): accepted at once, toast "Group A (main) holds a territory of radius 6.0 at (70, 82)". 
  - [Medium] PT2 #10 still present: the centre tile (70,82) is shallow water (biome '1'), at the edge of a lake, during a MEGADROUGHT, and ~85 tiles from the herd (median position 108,13) - no warning about water, distance, or that all 309 animals would trek across the map in a drought. A mis-tap while zoomed out on a phone commits the whole lineage.
  - [Low] The toolbar "Clear" cancels only migrations; it silently did nothing to the territory. Abandoning needs Lineage tab > group card > "Abandon territory" - and that gave no toast either.
- 409 alive on day 419 (121 adults), still in the north-east, mean thirst fine.

## 07:45-07:46, days 420-436: megadrought losses
- Gliders lost 409 -> 283 in the megadrought (8-19 deaths a day, nearly all thirst), even as fliers. Herdmaster3 crashed harder: 1901 -> 496. Result: I'm now 2nd on the legacy board (5.4%, Herdmaster3 13.8%). Alert "Your legacy share passed 5%" arrived on day 432 while my lineage was dying of thirst (relative share).
- [Medium] No lineage-level warning during the thirst die-off: only the daily tally lines ("Day 424: 0 born, 19 died (18 thirst, 1 starved)") and per-founder death toasts. A phone player who glances at the map sees nothing telling them "a third of your herd is parched; nearest open water is at X". The drought tip in Help is generic.
- Season 7 ended (day 421): #3 of 6 in legacy. Bought 2 reinforcements again right after the price reset (150 + 300), price now 600.

## 07:46-07:51, days 442-501: 5-minute continuous phone session through winter into spring
- One page open 5 min at 5 fps: stayed "connected", no page errors (console only GL performance warnings), no visible chunk seams or stale snow after the thaw (shots/PocketNomad3-54-long-*.png). PT2 "seams / stale snow after long sessions": not reproduced in 5 min.
- [Low/Medium] Winter, map fully snow-covered, yet the notice says "Drought: 72% of the land is dry ... Animals far from lakes and rivers go thirsty" (shots/PocketNomad3-54-long-03-d458.png). Reads as a contradiction; if frozen water is what makes it "dry", say so ("water frozen or dried up").
- Gliders wintered with only old-age founder deaths (277 -> 270), then hatching resumed in spring (305 by day 501). Bought reinforcements again at the season-9 reset (day 482: 150, then 300).

## 07:52-07:55, days 513-541: boom to 1,000
- Day 535: 971 alive (301 adults, 670 young), 30-48 hatchlings a day. Day 541: 1,012, "crowding 484 / 1,500 ... Now: litters are full size". Eggs line: "91 clutches (205 eggs) · 91 brooded · 51 brooding now · 15 about to lay" - good detail.
- Season 9 ended: "#2 of 6 in legacy, #2 of 6 in lineage". Ranks season 10: Legacy 2nd (5.0% vs Herdmaster3 17.6%), Largest 2nd (1,013 vs 1,709). Record: "1 title".
- Phone client with 1,154 markers (1,005 own) stays responsive at 5 fps; Lineage list caps at ~50 then "…and 952 more" (good).
- [Low] Lineage list shows "#46493 F preg · bask" for an egg-laying (incubated eggs) species; "preg" is odd for an egg layer ("gravid"/"carrying eggs").
- Grace counter "90 days (≈ 7 min) left" counts down consistently.

## ~07:56-07:58: machine restart
- The machine restarted (my scripts and watcher died; server restored from its save at day 566). Phone client on reload: login screen with "Your session has ended (it expired, or the server restarted). Please log in again." Clear. Console: one WebSocket error "HTTP Authentication failed" for the stale token (expected). Re-login went straight to the map with the lineage intact (1,140 alive, 1,291 renown, day 580). No design screen, no tour. Good.
- [Low] A crash loses all sessions, so every player must re-log in; the old API token also became invalid (401). Acceptable, message is clear.
- Day 581 after restart: grace 48.9 days left (consistent with day 541's 90). Leaderboard: I'm 1st, 18.7% (1,139) vs Herdmaster3 13.5% (820): the cold-blooded brumating flier wins winters.

## 07:59-08:02, days 581-620
- Season 10 won: "Legacy 1st of 6 · 15.6%" - 1 legacy crown, 2 titles (Survival 1st season 8). Season 11 now: Legacy 1st 23.5% (Herdmaster3 9.1%), Largest 2nd (Rulebender3 1,168 vs my 1,132). Achievement "+20 renown (Lineage longevity 3 years)".
- Reinforcements now disabled for big lineages: button title "reinforcements are for lineages of at most 300 (yours has 1127)". PT2 #6 PARTLY FIXED: the pump only helps lineages <= 300 (still resets each season for them). [Low] the reason is only in a hover title (invisible on a phone); the button just looks greyed out.
- Spring: 0 hatchings days 600-619 while 289 eggs incubate (150 clutches); litter factor 0.86 at crowding 1,101/1,500. Lineage tab explains it.

## 08:02-08:06, days 620-662: overshoot and starvation, split + migrate
- Hatch boom to 1,627 (day 645) then mass STARVATION: adults fell 1,101 -> 663; deaths/day reached "starved 86, thirst 17". The soft cap readout said "crowding 904 / 1,500, litters 95%" - i.e. the soft cap never kicked in while the land was eaten bare (summer drought 0.79). [Medium] The soft cap counts young at 0.25, so a big egg cohort pushes the real population far past what the land can feed while the UI says "litters are full size"; the crash kills breeding adults (my share fell 20.9% -> 14.1%). No "your herd is starving / out of food" warning beyond daily tallies.
- Food layer on the phone (leaf button) shows red/grazed vs green clearly - useful. 
- Tools > Split: toast "Group A (main) split: 747 went to Group B, 747 stayed". Tools group selector then offers "All groups (1496) / Group A (main) · 750 / Group B · 746". Migrate Group B to green land at (19,96): "Group B adults migrating toward (19, 96)". Lineage tab shows both groups with "Merge into A". Works on the phone (after folding the drought notice).
- Season 11 ended: "you placed #1 of 6 in legacy, #1 of 6 in lineage." Renown ~3,080 (no sink for a leader: reinforcements disabled >300, slots 30, nameplate 500 - PT2 #5 "no renown sink for leaders" still present). [Medium]
- Grace ended (~day 630); no attack alerts so far.

## 08:07-08:09, days 688-695
- Recovered to 1,637 (1,422 adults); share passed 25% (day 693); leaderboard 24.2% vs Herdmaster3 8.7%, Rulebender3 3.4% (1,229 alive).
- Lineage tab near the cap: "1,637 · crowding 1,476 / 1,500 — breeding slowing: litters ×0.5". Good, explicit.
- [Medium] PT2 #13 partly present: Group B (746, migrating ~95 tiles from (111,20) to (19,96)) ended with "Group B didn't gather at its migration target (19, 96) in time (half the group is still more than 109 tiles away); the order has ended." More than 109 tiles away is FARTHER than the start distance, so half the group never really set off (juveniles / "adults migrating" only?). The message is clear and actionable, but long migrations of a big group just fizzle; the UI accepted the far target without saying it was beyond what can be reached in time.
- New alert "The drought has eased: 27% of the land is dry now." Nice.
- No attack alerts after grace ended (~day 630).

## 08:09-08:11, days 700-709: market listing on the phone
- Market > "List one of my adults": picker opens (nearest first), chose #80625; tapped List -> "#80625 listed for 10 renown." The fee I meant to type went nowhere (my script used the wrong id; the field is #market-fee and empty means the default 10) - [Low] an empty fee silently lists at 10 with no confirmation of the price before listing.
- Listing card: "Dam #80625 · herbivore, winged, toxic, furred, brightly colored · gen 3 · at 104, 24" (position rounded to 8 tiles - coarse, PT2 fix holds).
- Income now +31 renown/day at 29% share; balance 4,198 with nothing to spend it on (reinforcements disabled >300, nameplate 500, slot 30). PT2 #5 "no renown sink for leaders" STILL PRESENT. [Medium]

## 08:15, day 783: final standing
- Leaderboard: PocketNomad3 1st, 22.1% (weighted 17.2%), 2,100 alive (at the hard cap). Herdmaster3 11.0% / 1,180, Rulebender3 4.4% / 2,098, LateComer3 3.8%, GeneBroker3 1.3%, ApexHunter3 0 (extinct).
- Season 14 (days 780-839): legacy 1st 19.3%; largest lineage tied 1st with Rulebender3 at 2,100. Won seasons 10 and 11 legacy crowns plus a survival title (season 8).
- [Low] two lineages both pinned at exactly 2,100 are "rank 1" together in largest lineage; with the hard cap, that category stops separating leaders.
- Winning design: cold-blooded, incubated eggs, body 0.5, wings 0.8 (flies), swim 0.5, toxins 0.6 with honest bright colours, brumation 0.7, 4 eyes, curved horns; sterile with every other species. It wins winters (dormant, 0 deaths) and catastrophes; the danger is summer overshoot/starvation.
- Watcher stopped; browser closed.
