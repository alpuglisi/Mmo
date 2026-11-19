# Rulebender8 log
Started 04:54 UTC, world day 12 (fresh world).

## 04:54-04:58 UTC (game day ~12-45) Names probe via POST /api/design/compatibility (no side effects; same check as founding)
Tool: rb8/Rulebender8-names.sh. Server's reply = `bad_name` (check) or `name_warning` (look-alike/wild/rival) else OK.
- FIXED: "Claude2 fan", "GM2 herd", "Head GM2", "claudebot", "GMbob", "WildHerd", "Wild2 herd" refused. "Herdmaster", "Herd master8", "Herdrnaster" warned as a player look-alike. "Meadow vole", "The Meadow vole 2", "Meadow voles", "MeadowVole", "Meadovv vole", "Red fox" warned as wild names. "Sunbright Drove" / "Sunbright Droves" / "The Sunbright Drove 3" / "Sunbright-Drove" warned as rival copies.
- STILL OPEN [Medium] staff word INSIDE a run-together lowercase word passes (only prefix/suffix checked): "realclaudebot", "theownerbob", "mygmherd". Digits on BOTH sides in lowercase pass: "2claude2 fan", "7gm7 herd". Split letters pass: "C laude fan", "W ild herd", "Sup port Bob".
- STILL OPEN [Medium] wild prefix after a digit, lowercase: "2wild herd", "x2wild herd" OK (wild check is starts_with on the folded word; "2Wild" is caught only via CamelCase split).
- STILL OPEN [Medium] wild species / rival lineage copies with extras: "Meadow vole II", "Meadow vole herd", "Big Red fox", "Red Foxes" (-es plural), "Steppe antelope III", "Sunbright Drove II", "Sunbright Drove Two", "New Sunbright Drove", "Ninefold Fang Pack II", "Sunbrite Drove" all accepted. Roman numerals are the obvious copy ("II") - name_key strips digits only.
- STILL OPEN [Low] player look-alike only exact/trailing digits: "Herdmaster8x", "Herdmaster8 fans" accepted as a lineage (titles do check containment).
- NEW [Medium] FALSE POSITIVES on ordinary names (refused "looks like a staff or system name"): "Taproot grazers", "Beetroot voles", "Arrowroot herd" (ends with root), "Ecosystem grazers", "Systematic grazers" (system anywhere), "Observer pack" (server anywhere), "Badminton hares" (admin), "Distaff herd" (staff), "Cooperator ants" (operator suffix), "Modesty voles", "Modena runners", "Mythmods", "Claudette herd", "Wildly happy herd", "Wildrose grazers", "Wilder herd", "Oscar Wilde voles", "Supports", "The Help", "Gmelina moths". (Hotrod, Commodore pass.) Herbivore names like Taproot/Ecosystem are plausible.
- Account look-alikes (code reading only, since registering a test name would create an account): `name_in_use` uses names::impersonates: equal fold or equal without TRAILING digits. "Rulebender8a", "8Rulebender", "Rulebender8-x" are not look-alikes and would register. Also accounts may use "wild": Kind::Account skips the wild-word check, so an account "Wild Life"/"WildHerd"/"Wildkeeper" is allowed and shows as a lineage owner next to the real "wild" owner. [Low-Medium]
- /api/metrics: FIXED for logged-out (conns/players_online zeroed). STILL LEAKS [Low]: `/api/worlds` and `/api/assign` (both public, no login) return `players_online`; any logged-in account still sees full metrics. `/api/assign?name=X` returns `has_account` true/false -> public account enumeration [Low].

## 04:59 UTC day 74-78: founded "Rule Rovers" through the web client
45 founders, warm live-bearing herbivore, toxins 0.8, colour 0.8, estivation 0.6 (drought 34%), 67.7/100 pts. Designer warned 45 is big; grace 60 days (not extended since >40). Founding grant 50 + 3/day income.
- [Medium] Playtest-7 #10 (late joiners land crowded) STILL PRESENT: my founders landed at (90,30) with PocketNomad8's whole herd (60 animals within 16 tiles), ApexHunter8's carnivore pack (7 animals in view, 1563 at 85,28) and GeneBroker8. Founding search still ignores rivals.
- Screenshot shots/Rulebender8-designer.png, -design-filled.png, -founded.png. No console errors.

## 05:00 day 78-102: preview_mating precision (playtest-7 #8)
- PARTLY FIXED: rival partner -> fertility, body_plan_gaps, mismatch, failure_chance rounded to tenths. Own pair exact.
- STILL LEAKS [Medium]: `cost` is sent unrounded (game.rs:3872 `cost: p.cost`). cost = k*(mismatch-1)^2 (k=override_k, ~1 here), so sqrt(cost)+1 gives the EXACT mismatch: e.g. rival #2415: mismatch "3.6", cost 6.51787 -> exact 3.5530; #1563: "2.7", cost 2.82720 -> 2.6814. mismatch = |display - preference| / tolerance on coloration and BODY SIZE, so with own animals of known genes the rival's exact body_size (a body-plan gene) or preference genes can be solved, the same class as the playtest-7 leak.
- Also Market listings carry `body_plan_fertility` at full precision (0.818606436...) for my sample vs each listed rival animal (annotate_compatibility), and listed animals' traits at full precision (owner opted in by listing, so Low).

## 05:01 day 102-105: territory at clamped radius (playtest-7 #5)
- FIXED: claim radius 300 at my herd -> "radius cut to the group's limit, 17.4 tiles", food_supports 31 of 45, standing_days 20.7 computed at 17.4 (not at 300). Warned "expect hunger".
- Odd [Low]: `drinkable_tiles: 912` inside a 17.4-tile circle (area ~950) with water_share 0.25 - looks like a tile count, not drinking spots.

## 05:02 Squatting (playtest-7 #13) - code + live
- PARTLY FIXED: a rival territory held by <=12 animals (SQUAT_MEMBERS) can be claimed over by a group 10x its size (groups.rs claim_squats).
- STILL OPEN [Medium]: a 13-animal splinter is never a squat: `theirs > SQUAT_MEMBERS` -> Overlaps for any claimer, whatever its size. A 45-founder lineage can split 13 onto a key water spot and deny it to everyone indefinitely (radius min). And one player can run several such splinters.

## 05:03 Market (playtest-7 #7) - live + code
- Listing fee 4 refused (min 5); fee 12 listing created; auction asked for `ticks:1` ran the minimum 3,000 ticks (ok, documented).
- FIXED: predation no longer excused (excused only disease/recessive/attrition/winter-sleep, and never once steered: possessed, ordered, migrating or rallying).
- FIXED-ish: seller retire now refunds buyer + penalty. STILL OPEN [Medium]: the penalty is `.min(self.balance(player))` (game.rs settle_agreements_on_retire) - a seller who spends renown first (shop, slots, escrow bids) then retires pays 0 compensation; same hole as the old re-found fee, which now uses debt.
- FIXED: re-found fee beyond balance becomes debt taken from later income (post_on). Side-effect [Low]: debt is taken from ANY positive ledger post, including refunds of one's own escrow (outbid refunds, cancelled offers) - a player's own money held back.
- NEW EXPLOIT [High]: seller's own pack kills the buyer's animal and the BUYER pays. safe_meeting_point only avoids predators of third parties (`x.owner != from && x.owner != to`), the point is 75% toward the seller, and buyer_lost excuses no predation at all - it never checks `killer == seller's lineage` (only the reverse case, buyer killing the dam, is handled at game.rs:4522). A carnivore seller lists a stud, the buyer's animal walks into the seller's range, the seller possess_attacks it (after grace) -> buyer's animal lost, not excused, hold fee forfeit TO THE SELLER, seller also eats it.
- NEW [Medium]: attrition (old age) deaths are excused. Buying a rival's listed DAM with a very old male of one's own holds her from breeding (hold_consented_females) until the deadline; when he dies of old age the loss is excused -> no forfeit. A free dam-lock grief using elderly animals.
- Steering covers possession/order/migration/rally but not claim_territory (disease deaths after a territory move are still excused) [Low].

## 05:04-05:08 UTC day 129-196: play, survey, retire + re-found (debt test)
- Spring day 129: herd 47, moved by a far claim to (64,50) (food_supports 139, standing 37.6 days); refused at (80,56) as overlapping (survey showed Herdmaster8's 740-strong territory at (87,90) r24 - legit). Overlap refusal and cooldown text fine ("under a minute (1.8 game days)").
- Survey (37 renown) worked; price rises with my animals incl. young.
- I mistakenly founded incubated eggs (the designer default birth radio; I didn't click Live) - my error, not a bug. Lineage grew 45 -> 127 by day 173.
- Retired via Lineage tab (#lineage-retire): panel + confirm clearly state the re-found rule and the 25% gene transfer. Then bought a 3rd mating slot (120 renown) with NO living lineage to get my balance to 17 [Low: the shop sells mating slots with no lineage, 5 days before the season reset them].
- Re-found "Rule Rovers II": 23 of 45 founders. Designer said "re-founding price of 52.3 points and 17 renown"; the after-founding notice said "It cost 100 renown" and neither said 83 is now OWED [Low wording]. `design/compatibility` `refound.fee` = 17 (capped at balance), also hides the debt.
- FIXED (playtest-7 #7 re-found fee capped at balance): ledger now shows "Day 189 income ... (less 2 toward the re-founding fee owed)" each day -> debt collected from income.
- Re-founded group landed at (2,56) on the map edge with no rivals in view this time. No fresh grace (continued expired grace), as documented.

## 05:09-05:10 UTC day 196-215: edge-value and public-endpoint probes
- Robust: claim radius 1e39 -> bad_coordinates; migrate x=-1e30 -> off_map; set_view radius -5 refused, 1e9 clamped to 11.7; price u64::MAX -> "prices go up to 1000000"; split with [] refused. Rate limiter answers "too many commands; retry in 850 ms" for 8 quick commands (fine).
- /api/spectate (public): player lineages' cells are empty (only wild cells) - FIXED no position leak. /api/map.png -> 401 when logged out while /api/terrain stays public (playtest-7 Low: still present, cosmetic).
- Claim cooldown is inherited by split groups and survives merge+re-split (good).
- Two claims in the same second on group 0 both succeeded (2nd: "this replaces the group's territory") because the first territory fed 21 < 23 (outgrown waiver). Testing whether a deliberately undersized claim keeps the waiver open forever (cooldown bypass / overlap radar).
- Result: not reproduced. A radius-1 claim was raised to the 3-tile minimum, fed 13 of 12 with 3.1 standing days (just above the waiver thresholds), and the next claims hit the cooldown. The waiver works as designed here; a bigger group on a bare patch might still chain claims (untested).

## 05:11-05:13 UTC day 215-255: moving the re-founded herd + more market code review
- Claims refused as `mostly_water` (59%, 56%) cost no cooldown (good). One overlap refusal at (48,52), then a claim 2 s later at (34,44) succeeded: the overlap refusal's "short wait" is under 2 s. Rival outlines are public in state.territories, so this gives no radar (ok).
- The far claim (32 tiles) became a migration with a route, food_supports 37, dryness 0.73, and a note that 5 brooding parents stay on their nests.
- NEW EXPLOIT [Medium-High]: a SELLER can make the buyer forfeit by dodging the meeting. `lapse()` always blames the buyer ("the buyer never came") and forfeits 25% (min 5) of the escrow to the seller. Only the requester's animal is tracked as `steered`; the partner's owner can possess or order_move the partner away, migrate or claim its group far away, or (with offer_to_lineage, respond_mating `partner`) pick the farthest compatible animal (pick_partner checks only compatibility and that it is free). The pair never meets, the deadline passes, and the buyer pays the hold fee to the seller. Repeatable on every sale. Combined with the seller's-own-predators hole above, the meeting point at 75% toward the seller is entirely the seller's ground.
- [Low] pick_partner lets the accepter choose the LEAST fertile compatible animal (fertility only has to be > 0) and still collect the full price when the mating "runs".

## 05:14-05:16 UTC day 265-300: megadrought (catastrophe days 271-315)
- Migration to (34,44) ended "dormant" (herd estivated in spring drought); Genes tab alert said the group "didn't gather ... the order has ended" (clear).
- state.territories shows a rival circle at (7,60) r10 next to my herd, but a Survey bought right after lists no group with a territory there (only Herdmaster8's (58,76) r22; its 10-strong splinter at (28,52) shows no territory). Survey seems to omit territories kept while their group is away, so I couldn't tell whose it was or whether it is a takeover-able squat [Low-Medium: survey vs map disagree].
- Claims in the drought: (22,46) r8 -> water_share 0, standing 0.7 days, feeds 12 of 27; next claim allowed at once by the bare-territory waiver (good) -> (25,48) feeds 12 of 23, 9 drinkable tiles. Herd fell 40 -> 23 within ~30 game days of the megadrought starting (thirst). Bought a provision (40) for 20 adults.
- Mates tab "Recent outcomes" shows GeneBroker8's offer #3 to my old lineage "failed: the lineage has no living animals left" after my retire: clear wording, refund presumably theirs.

## 05:17-05:19 UTC day 303-314: more code-backed holes
- [Low-Medium] /api/metrics for ANY logged-in player shows conns 4, players_online 3 and world-wide per-command counts (possess_attack 60, buy_listing 2, offer_to_lineage 4, retire_lineage 2 ...). A rival can watch these between polls (a jump in possess_attack means a hunt is on) and see who is online, which the playtest-7 fix meant to hide. /api/worlds (public) still says players_online 3.
- [Medium] Squat rule counts YOUNG: claim_squats uses group_counts().0 (all members, juveniles included). A 2-adult squat that breeds past 12 members (an egg layer does this in one clutch) can never be taken over, and the claimer's own young pad its "10x" count too.
- [Medium design/exploit] "Stud raid": natural mating has no owner check (decide.rs mate scoring, apply.rs try_mate). A player can possess a male, walk him into a fertile rival herd and release him; he sires the rival's young without consent or a market fee, spreading his alleles abroad (counted in the legacy share) and tying up the rival's females with pregnancies. Mating blocks and auto_decline don't apply. (My first founding landed inside PocketNomad8's herd, 80% fertile, so this already happens by accident.) Tried live below.
- Herd status day 314: 19 adults (12 m / 7 f) estivating, hunger 50-100, thirst 40-100; the megadrought ends day 315.

## 05:19-05:22 UTC day 335-350: winter, Survivor, stud-raid attempt
- Winter came right after the megadrought; wild red foxes (33 in view) around the herd, 9 animals on "defend". Herd 17 (15 adults, 2 young). Egg losses so far: 19 chilled, 3 raided.
- Day 315: "Survivor: achievement bonus" +25 renown. My lineage held about 48% of its size from the catastrophe's start (40 -> 19), and the rule needs 25% (SURVIVAL_KEEP). Fair. Playtest-7 #15 (Survivor after ~99% loss): FIXED by the 25% rule.
- Stud raid: possessed male #21592 (survey: PocketNomad8's 20 at (44,12), Herdmaster8's two territories (80,70) r24 and (58,76) r22). The (7,60) r10 rival territory still shows in state but not in the survey. Ordered him to (44,14). Possession survives my socket closing (the UI shows "Possessing #21592" on the next login).
- UI [Low]: the possession bar offers "Attack" for a plant eater, although the server refuses attack orders from plant eaters. Screenshot shots/Rulebender8-map-possess.png.

## 05:22-05:26 UTC day 358-370: era 1 ended; retire hand-over observed
- Stud raid failed: possessed #21592 died (probably wild foxes) within about 10 days of walking off; I got no visible word of it in the API. The stud-raid idea is untested live.
- Species-name filter (name_species answers insufficient_funds only after the name checks pass): "Meadow vole II", "Sunbright Drove II", "realclaudebot", "Herdmaster8 herd" all PASS the checks. "Sunbright Drove" -> name_taken. Same holes as lineage names.
- Reinforce (150): 10 adults joined at (23,57) beside 6 of mine. Lineage 16 -> 29.
- Era 1 results (/api/eras): every niche went to the wild again (biome:desert/forest/grassland/marsh...). Playtest-7 Low "every niche prize went to the wild": STILL PRESENT.
- NEW [High, balance/collusion]: the RETIRE HAND-OVER (RETIRE_TRANSFER_SHARE 0.25) passed GeneBroker8's lineage to Herdmaster8. GeneBroker8's Studbook Reeds (a split from Herdmaster8's Sunbright Drove, 440-475 alive) went to 0 on day 367-368. On the same day Herdmaster8's legacy share jumped 0.409 -> 0.460 and its alive count reached 1,992, past the 1,500 soft cap and near the 2,100 hard cap. Any lineage bred from a rival's studs is one click away from being gifted whole to that rival. A "feeder" account can raise a lineage off the leader's studs (paying the leader royalties and sales on the way) and retire to hand it over, and the receiver gets hundreds of fed adults with no reinforcement cap or price. Since the receiver is told only after the fact, it also cannot refuse animals that push it over the soft cap.
- [Low] At day 367 /api/leaderboard showed GeneBroker8 "alive 0, share 0.105" (daily share snapshot next to live alive count); /api/species still listed Studbook Reeds with 317 adults for GeneBroker8 after the retire; adult counts with float noise (1194.0000000000002, 316.9999999999999): playtest-7 Low STILL PRESENT.
- A 7th player, LateComer8, joined (83 alive at day 367).

## 05:23-05:28 UTC day 383-425
- Claimed (10,31) r12 (food_supports 75, 81 drinkable tiles, standing 26 days). Herd 54 -> 71 (59 adults) by day 420, mostly estivating (drought 58%).
- Market still thin: my listing #12 (fee 10) is the only listing; offer ids reach only #5 by day 425. Playtest-7 "market stayed thin": STILL PRESENT. Sent offer_to_lineage #5 to Herdmaster8 (price 15) to watch where the meeting point lands if it is accepted.
- Reinforce price shows 300 after one purchase (doubling, correct; playtest-7 Low "blocked reinforcement price shows 600": not seen).
- Code check: retire_player_sparing retags the retiree's alleles as wild ("no legacy gifted"), so the hand-over is by design to keep the receiver's alleles alive. The head gift still has no hard-cap check: Herdmaster8 sits at 2,098 of 2,100 at day 420, so its births have stopped.
- Leaderboard day 420: wild 45.3%, Herdmaster8 45.2% (2,098), ApexHunter8 3.2%, Rulebender8 1.8% (71), PocketNomad8 1.8%, LateComer8 1.8%, GeneBroker8 0.9% (re-founded, 34).

## 05:28-05:33 UTC day 425-495: crash and wrap-up
- Titles (set_nameplate answers insufficient_funds only once the title checks pass): "realclaudebot", "2wild herd", "Meadow vole II" pass. "Taproot fan" refused (false positive). "Herd Master" refused as a look-alike of Herdmaster8 (good).
- Bought a 2nd reinforcement on day 431. The shop on day ~425 said reinforce_price 300 and reinforcements_bought 1, but the ledger charged 150 (the count had reset with the new season). A 3rd on day 475 cost 300, matching the shop. [Low] The shop's price and count lag the season rollover. Playtest-7 Low (blocked reinforcement price shown wrong): still partly present.
- Territory (10,31) was claimed on day 383 as feeding 75 with 26 standing days. By day 399 it "no longer feeds it (9% starving, 16% parched)", and the drought rose to 58% after the claim. Lineage 81 -> 25 by day 465, deaths "starved"/"thirst" 1-3 a day. The slow-decline warning fired ("41 -> 22 alive over the last 10 days ... 46%"), good (playtest-7 #4 looks FIXED). The hold loosened as designed. The estimate is still blind to a drought rising after the claim (playtest-7 #5 tail: STILL PRESENT).
- My pending offer #5 to Herdmaster8 failed "organism 20706 starved before the mating", refunded 15. Achievement bonuses: Survivor x2 (megadrought, epidemic 394-408), Lineage longevity +20.
- Final (day 495): wild 48.7%, Herdmaster8 43.1% (2,098, at the hard cap), LateComer8 4.4% (222), ApexHunter8 2.3%, PocketNomad8 0.9%, GeneBroker8 0.4%, Rulebender8 0.3% (13 alive) -> 7th of 7 players by alive, 6th of 6 in season-9 legacy so far. One player at 43% of all breeding alleles is a runaway (playtest-7 #1/#2 class, not my focus).

## 05:34-05:36 UTC day 504-507: second retire + re-found (lineage dying, 15 left)
- Retired 15 (transferred 0). Re-founded "Rule Rovers III" via POST /api/lineage: 23 of 45, `refound_fee: 200` (doubling: FIXED), ledger "Re-founding fee (re-founding 2 within the window)" -64 (all I had), then "(less 2 toward the re-founding fee owed)" each day: the 136 debt is collected. design/compatibility `refound.fee` showed 64 (capped at balance) and never mentioned the 200 or the 136 owed [Low].
- NEW [Medium] GRACE FARMING: although this re-found is inside the 375-day re-found window, it got a FRESH "Newcomer grace: 124 days (about 10 minutes)", extended through winter because it is <=40 founders, since my last FOUNDING (day 186) was more than 120 days ago. The retire panel says "A quick re-found also continues your last grace instead of starting a new one", which is true only within 120 days. A player under attack can retire and re-found every ~121 days and stay immune to attack orders, paying 100, then 200, 400... per window, with half founders. The 120-day grace window is shorter than the 375-day re-found window, so the two rules disagree.
- [Medium] Late-joiner placement (playtest-7 #10) STILL PRESENT: Rule Rovers III landed at (91,27) with LateComer8's 23 animals and a GeneBroker8 animal in view.
- Code: grace_cooldown = 2 x grace_length (60 -> 120 days), "so a re-founding never starts afresh a grace that would still be running". A winter-extended grace (<=40 founders) runs 124-127 days, which is longer than the 120-day cooldown. So a re-found on day 121 restarts a grace that is still running, and chaining small late-autumn foundings gives near-continuous immunity. Even without the extension it covers 60 of every ~121 days.
- Claimed (88,28) r10 for III: food_supports 32 (23 founders), 269 drinkable tiles, standing 22 days, dryness 0.53.
- Survey code: it lists only groups whose middle is within 64 tiles, with `g.territory`. Territories kept while their group is away, or whose group is farther than 64 tiles, are left out, which explains the (7,60) mismatch [Low].

## Final 05:35 UTC, day 530
wild 57.5%, Herdmaster8 32.6% (1,938), LateComer8 4.6% (356), ApexHunter8 2.6%, PocketNomad8 1.6%, Rulebender8 0.5% (26, Rule Rovers III), GeneBroker8 0.5%. I'm 5th or 6th of 6 players by share, tied with GeneBroker8, and carry a re-found debt of ~130.
