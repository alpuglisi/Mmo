# ApexHunter10 log (predator persona, playtest 10)

Start 12:37 UTC, world day ~14 (spring y0). Fresh world (962 organisms, wild only + early players).

## 12:37-12:41 (day 14-49) Register, design, found (UI via Playwright, 5 fps cap, DSF 1)
- Registered in UI as ApexHunter10 (player 4). No page errors, console errors or failed requests.
- Design "Ironfang Pack": warm, live, litter 3, 36 founders, carnivory 1, pack_instinct 1, group_tolerance .9, body .45, muscle .9, fangs .9, claws .6, keen_nose .8, legs .45, fat .6, fur .5, food_caching .5, large_eyes .3, aggression .85, obedience .9, boldness .7, seasonal_coat .4, hibernation 0 (set to 0 explicitly this time). 54.9/100 pts.
- Designer food hint: "36 founders (about 140 kg of animals) land together and eat the prey around them within days ... Around 24 founders spread out" (carnivore-aware). Drought warning 30%.
- Founded day ~25, lineage #4. Lineage tab: newcomer grace 125 days, season weight ramp text. No founding dialog seen (page went straight to the map, Genes badge 3).
- Watcher apex10/ApexHunter10-watch.mjs (PID in playtest10/apex10/watch.pid): plain JSON WS; logs alerts/results; command file; hunt helper (toggled by apex10/hunt.on).
- Scanner apex10/ApexHunter10-scan.mjs: set_view on each cluster of my animals (set_view refused away from my animals, `out_of_sight` - as designed).
- **Pack scatter (pt9 #16, pack_cohesion now off by default): STILL PRESENT.** 36 founders landed together; by day 37 (12 days) the 41 animals span x97-127, y21-80; day 45 x89-127 y0-92; day 49 x75-126 y3-85. Pack instinct 1.0 does not keep a pack.
- Scans days 45/49: no other player's animal within my view windows (my own placement: away from rivals, good).

## 12:41-12:43 (day 59-77)
- Season 1 ended day 59: me #3 of 5 legacy, #5 of 5 lineage.
- (Retracted: not a bug - my two overlapping log prints showed the same lines twice; no duplicate alert ids in the watcher log.) ~~my watcher received alerts id 26 and 27 ("reached 50 living organisms", "Generation 2 reached") twice each in the same second (day 66). Possibly a resend on another connection opening (my scanner opens a second socket). Will see if it repeats.~~
- Day 67 scan: GeneBroker10's herd at ~(28,59), graced; my nearest hunter 8.8 tiles (my pack drifted to it: spread x31-128!). Pack spread over the whole width by day 67.
- Day 75 split 4 far-west strays into group B (satellite test). Rally B at (30,58) next to GeneBroker10's graced herd: "107 animals: no burrowers, other hunters or animals under a newcomer's grace, poisonous kinds only as often as hungry hunters take them) feeds about 1 of the group's 4". **pt9 Low "prey count doesn't mention graced animals": FIXED.**
- Day 76 flood warning names my animals on floodplains (7) - good.

## 12:43-12:47 (day 84-132) Autumn crash; carnivore advice; refuge rule
- Pack fell 64 -> 33 (days 95-118), mostly starved, though the wild is booming (voles 979 -> 1,948, antelope 150 -> 475 by day 124).
- **New "crashing" wording (pt9 Low "dying off slowly for a 45% crash"): FIXED in part**: day 105 "Your lineage is crashing: 64 → 40 alive over the last 10 days (38% fewer)". But day 108 "dying off slowly: 57 → 41 ... (28% fewer)" and 111 "50 → 37 (26% fewer)" - 26-28% in 10 days is still called "slowly" (Low).
- **Carnivore advice (pt9 Low): FIXED.** Advice now "Move the pack to where prey is, split it, or fatten it before the cold." for my pure carnivore.
- Low: the "dying off slowly 40 → 34" alert was repeated word for word on day 116 and day 119 (same numbers, 3 days apart) - stale figures re-sent.
- Possible balance concern (Medium, hypothesis): the wild refuge (`wild_refuge` 0.3 of each wild species' cap) appears to bite a pack founded early in a FRESH world: at day 14 the antelope were 150 (cap ~1,250 if split evenly -> r≈0.4, skip chance ~97%) and voles burrow, so my hunters had little legal prey in spring; with 36 founders the pack starved from ~64 to 33 in autumn while total wild was 3,200+. The refuge is meant to protect a hunted-down species, but in a fresh world every species starts below its cap.
- Day 117-119: GeneBroker10's herd (lin 5) graced, at ~(23,60), 120 of its 132 seen animals within 10 tiles of my hunters (my strays wandered into it; fine, they founded first). Rulebender10's lin 8 at (41,72) shows no protected flag by day 119 (grace ended or ended early).
- Rally into the antelope ground (22,90)/(26,94): prey_feeds 10 of group B's 9, 13 of group A's 31.
- Hunt helper on (prefers player-owned unprotected prey within 6 tiles, else wild).

## 12:47-12:51 (day 140-184) Grace ends, poison lessons, hunt helper, reinforcement
- After the rally the pack gathered: day 146 spread x16-32 y84-101 (rallies do hold a pack; its own instinct doesn't).
- Day 147: "Your newcomer grace ends in about 3 days (about 1 real minutes): then other players' animals can attack yours." No hunter count because none is near (as designed). "1 real minutes" - plural with 1 (Low, same family as pt9 "1 tiles").
- Day 151: "Your hunters were poisoned biting Rulebender10's Quillback Drift (not always fatally) and have learned they are poisonous ..." and day 152 the same for GeneBroker10's herd; toxin deaths day 154 and 170. **pt9 "learned they are poisonous fires at grace end before any bite": looks FIXED** - wording now says "were poisoned biting", and it came 1-2 days after MY grace ended (my hunters couldn't bite rivals before), followed by real toxin deaths. (I can't see non-fatal bites, so not fully proven.)
- Day 146 scan: Rulebender10's Quillback Drift (28 seen, unprotected) at ~(22,97) right where my pack rallied (nearest hunter 1.1 tiles). GeneBroker10's herd (6 seen, still protected on day 146) at (25,77).
- Hunt helper bug was mine (must possess before possess_attack). Fixed: possess then attack. Attack warning: "#8670 looks poisonous to your kind: your animals have been poisoned eating its kind (on their own they pass it up 95% of the time)".
- Market empty on day 181 (0 listings) - can't test the market shield yet.
- Day 184 reinforce (150): landed at (17.5,92.5) "group 0, nearby_count 19" - beside my group A (centre ~22,93), 13 tiles outside the only rival territory (46,110) r20. **pt9 M12 reinforcement placement: looks FIXED for me** (lands with the group, outside the rival territory).
- Low: survey price 35 -> 36 between two shop reads with no explanation (pt9 Low "shop price drift": still present, minor).

## 12:51-12:56 (day 184-235) Eating Rulebender10's herd; satellite alert; shield check
- Hunt helper kills: "your #12151's pack killed #17053" (Rulebender10's, warning "pass it up 40% of the time" at hunger 90), "#12150's pack killed #16409"; some escapes ("#17254 escaped; your #19675 lost sight of it (12 tiles away)"); several voles `sheltered` ("down in its burrow ... it went down since your last view if it showed above ground" - wording now explains the lag: pt9 Low FIXED).
- Rulebender10's herd beside my rally: 117 seen at day 172 -> 94 (day 186) -> 5 (day 224) in my view windows (moved and/or eaten; Rulebender10's own log says my pack eats 1-4 a day).
- **pt9 M15 "a satellite group dies with no group alert": FIXED.** Day 200: "Group B has fallen from 12 to 6 animals. Merge it into another group, or move it to better ground."
- Low, STILL PRESENT: "Possessed #12151 is starving (hunger 90): it is foraging on its own until you give it an order." 3 s after its order ended in a kill; similar for most helper-ordered hunters ~4 s after the order (when the order ended quickly it's fair).
- PocketNomad10 re-founded (lineage 9) around day 185; not seen in my view windows (my pack is all in the SW x2-41 y72-126).
- **Market shield (pt9 #10 follow-up), code reading** (crates/sim/src/world/commands.rs `possess_attack` ~l.590-620 and apply.rs `attack` ~l.855): an animal in an open offer is spared in `grace_spares`, so neither my hunters' own hunting nor a strike lands. BUT `possess_attack` only checks `attack_grace_left` (lineage grace), not the market shield, and `mark_protected` (game.rs ~1534) marks only graced lineages - so a shielded animal shows no `protected` flag, an attack order on it is ACCEPTED with no warning, and the hunter then chases it without ever striking (silent no-op). Medium (confusing for a predator: the order "works" but never can). Combined with Rulebender10's finding (any player can shield up to 8 animals free for 125 days by unpriced offer_to_lineage) it's a cheap anti-predator shield the predator can't even see. Trying to observe it live with an attack-distance tracker in my watcher.

## 12:56-13:01 (day 235-300) Summer; merge; shop; survey
- Day 255 "Attack over: your #19819 was poisoned before it reached #16721." - pt9 Low (odd "poisoned before it reached") STILL PRESENT.
- Day 263 a second "Your hunters were poisoned biting GeneBroker10's ... have learned they are poisonous" (same lineage as day 152; repeat after the lesson faded? fine but repetitive) (Low).
- Attack tracker: 8 tracked helper orders on Rulebender10's animals, all resolved (kills, escapes, poisoned) - no shielded target seen in practice yet.
- **New bug (Low-Medium): merging a satellite group fires a false death alert.** Day 282 merge_groups B -> A (5 animals moved, ok). Next day: "Group B has died out (it had 5 animals)." The group-decline check treats a group gone by merge as died out (game.rs group_decline_alerts: "Groups gone from the list since"). Repro: split a few animals into B, merge_groups {from:1, into:0}, wait a day.
- MY harness bug, not the game's: restarting my watcher replayed my command file, re-sending an old reinforce (day 243, 300 renown) and old rallies/split. Ledger confirms "day 243 purchase -300 Reinforcements: 10 adults joined your lineage at (20, 108), beside 6 of its animals". The shop's "next is possible in N days" (next day 303 = 243+60) is therefore right; pt9 Low "reinforcement next day off by one": FIXED (text now gives days, matching `reinforce_next_day`).
  - That replayed reinforcement landed at (20,108), 26 tiles from the centre of Rulebender10's territory (46,108) r24 = 2 tiles outside its edge; "beside 6 of its animals". Rule says not "in or at the edge"; 2 tiles is borderline (Low).
  - Day 282 reinforce (300): landed (15,97) "beside 25", with the group. Good.
- Survey (37 renown) day 293, radius 64 from (20,92): GeneBroker10 230 at (12,52); Rulebender10 220 at (44,108) with territory (46,108) r24 and 120 at (52,116); Herdmaster10 60 at (12,36). Herdmaster10 is 753 alive / 12.5% (day 280) - most of it outside my survey radius.
- Leaderboard day 280: Herdmaster10 753 (12.5%), Rulebender10 309, PocketNomad10 133, GeneBroker10 152, **me 62 (1.7%)**. The pack is nowhere near pt9's (240 vs 324 at era end). **"Pack out-eats herds": NO this time** - the pack is ~1/12 of the leading herd.
- Rally into Rulebender10's territory (38,104) (8.9 tiles from its centre, radius 24): accepted with only the prey warning ("264 animals ... feeds about 6 of the group's 71"); **no "inside Rulebender10's territory" warning on a rally** (migrate has one) (Low).

## 13:01-13:06 (day 300-370) Inside Rulebender10's herd; severe winter; era 1 ends
- The pack lives inside Rulebender10's territory/herd (lin 8: 99-129 seen, 74-128 within 10 tiles of my hunters). Helper kills ~1 per 10 days of orders plus the pack's own hunting; toxin deaths ~1 per 5-10 days. Pack 71 -> 75 through a severe winter catastrophe (days 331-360: "2996 fewer organisms in the world").
- Era 1 ended day 359: "you finished #5 with a mean 1.2% (3.0% at its end)"; carnivore niche (35% of carnivore alleles, +40 renown). Season 6: #5 of 5 legacy.
- UI day 357 (Lineage tab leaderboard): wild 63.3%, Herdmaster10 12.3% (273 alive, down from 753 in the winter), GeneBroker10 11.8% (270), Rulebender10 7.9% (178), me 2.9% (70), PocketNomad10 1.8%. Ranks: season 7, me Legacy 4th of 6. No page errors / failed requests.
  - Low/Medium (names): GeneBroker10's nameplate is «Nomad Hunter» - made of fragments of two other players' names (PocketNomad10, ApexHunter10); the name filter blocks whole names but this reads like impersonation of either.
  - Market tab: "wild Red fox: sterile (diet too different to breed, even in the closest pair) · drift risk high" - sterile yet "drift risk high" (Low; also seen by Rulebender10). Toast stack at the bottom-right sat over the possession bar's right end (screenshot shots/ApexHunter10-d340-Market.png): two toasts plus the bar fill the lower map; readable at 1280x800.
- LateComer10 founded lineage 10 near day 360; not in my view windows (pack at x14-59, y94-126) by day 367.

## 13:06-13:10 (day 370-425) Foundings vs my pack; move into the big herds
- **Foundings kept clear of my hunters (pt9 H2): looks FIXED.** PocketNomad10's re-found (day 167) landed "around (120, 69): the nearest rival animal (Rulebender10's) is 38 tiles away" (their log) while my pack was in the SW; LateComer10's founding (day 360) "around (64,53): nearest rival animal (GeneBroker10's) 28 tiles away" - my nearest hunter was ~41 tiles away (pack x14-59, y94-126). PocketNomad10's third lineage (#11, ~day 385) and Rulebender10's #12 (~day 420) never appeared in my view windows.
- Pack scatter again once the rally lapsed: x14-59 (day 367) -> x2-111 (day 382) -> x3-102 (day 391). (pt9 #16 STILL PRESENT; pack_cohesion off by default.)
- Rally to (14,54) by GeneBroker10/Herdmaster10: "626 animals ... feeds about 32 of the group's 87" (vs 6 of 71 inside Rulebender10's ground). Pack walked 63 tiles and gathered (x3-29, y41-69 by day 423) inside GeneBroker10's (80 seen) and Herdmaster10's (336 seen, 279 within 10 tiles) herds.
- Day 394/395: "poisoned biting GeneBroker10's ..." and "poisoned biting Herdmaster10's Cinderfleece Drove" - Herdmaster's arrived the day my pack first reached its herd (consistent with a real first bite).
- Helper kills now nearly every order (8 kills in 25 days); pack 87 -> 104 by day 425. Season 7: #5 of 6 legacy, #6 of 6 lineage.

## 13:10-13:17 (day 425-485) Hunger amid toxic herds; GeneBroker10's listings eaten; UI attack; reinforcements inside rival territory
- Day 459-462: "Your lineage is going hungry: ... Their prey has run short: move the pack to where prey is (the herds on the map; a group's orders say how much it can t[ake])" - carnivore-worded (pt9 Low FIXED). The pack starves beside Herdmaster10's 329 toxic animals (2 toxin + 5 starved a day at worst): the honest-signals/lesson balance again makes a toxic herd a poor larder.
- Leaderboard day 463: Herdmaster10 730 (18.3%), LateComer10 506 (12.4%), **me 92 (2.0%)**, PocketNomad10 73, Rulebender10 29 (re-founded), GeneBroker10 9.
- **Market shield vs listings (from the predator side):** GeneBroker10's log: "Listing #8 closed: #35129 was eaten by ApexHunter10's Ironfang Pack" and #7 likewise (days 398/408) - my pack ate two listed studs on its own. The shield covers offers/agreements only, not stud listings (game.rs builds it from `self.offers`). Either intended or a gap; the Market text should say listed animals aren't protected (Medium, agrees with GeneBroker10). Shielded-offer animals carry no `protected` flag and attack orders on them are accepted (see 12:51 code note).
- UI attack (possession bar, Attack, click a Herdmaster10 marker, day 478): toast "#36620 is hunting #37098; hungry pack-mates nearby join in. #37098 looks poisonous to your kind: your animals have been poisoned eating its kind (on their own they pass it up 95% of the time)" then "Attack over: your #36620 killed #37098." Works. Screenshot shots/ApexHunter10-uiattack2.png.
  - Low: the toast stack covers the possession bar's Release button (the bar's right end) at 1280x800 (pt9 M15 toast-overlap family, still present in this spot).
  - First try possessed a hunger-100 hunter that starved within a second: bar then said "#21882 died while you were possessing it (the alert says how)". Fine.
- 3 provisions (40 each, "fed 20"), 4th refused with a clear limit text.
- **Reinforcements landed inside a rival's territory (pt9 M12): STILL PRESENT when my pack lives in rival ground.** Day 484, two reinforce: at (13.1,17.3) and (11.3,17.4), "group 0, nearby 40/49". Survey the same second: Herdmaster10 territories (18,32) r24 and (28,14) r22 - both landing points are ~8 and ~7 tiles inside both circles. 11 s later the 20 newcomers stood at y≈6 (inside (28,14) by 4-5 tiles, and within 6 tiles of the map edge). Presumably the fallback when nothing near the group is clear - but the rule "never in another player's territory" isn't kept and nothing in the reply says so. Medium (a predator can still drop 20 hunters into a herd's claimed ground). Repro: live inside a rival's territories, buy `reinforce`, compare `at` with `survey`/state `territories`.
- Renown 974 at day 478 (era/niche prizes and income): renown piles up for me too.

## 13:17-13:20 (day 485-529) The herds collapse; the pack doesn't
- Day 526 leaderboard: Rulebender10 185 (4.9%), LateComer10 214 (4.2%), PocketNomad10 192 (4.0%), **me 111 (3.7%)**, Herdmaster10 80 (2.7%), GeneBroker10 34. Herdmaster10 went 1,372 (day 416) -> 730 (463) -> 57 (529) while my pack (77 -> 112) wintered inside its territories with the hunt helper ordering a kill every ~15 s of wall clock. I can't separate my kills from winter/crowding deaths (my summaries only count my own deaths); helper orders on its animals killed 6 of 9 in days 496-524.
- GeneBroker10 founded lineages 13-16 in a few days (retire/re-found loop); none appeared in my view windows (foundings away from my hunters).
- Day 516: "poisoned biting Rulebender10's Thornback Drift" (the re-found) - again a real-contact lesson.

## 13:20-13:23 (day 529-545) Looking for prey
- Migrate preview to LateComer10's ground (56,50): warning text "The way runs about 15 tiles (**about about 2 days**) with no drinking water ..." - doubled word (Low, new). Journey estimate now counts the group's thirst ("35% of the group is thirsty already: expect thirst on the way") - pt9 M11 partly FIXED. Float noise still in `dry_stretch_tiles` 15.399999618530273 and `thirsty_share` 0.3499999940395355 (pt9 Low still present). Rival-territory reach warning present on migrate.
- Rally previews: around my pack (20,28) "3 animals ... feeds 0" - Herdmaster10's ground is empty now; (40,80) 67/2; (70,100) 380/8. (100,60) refused `mostly_water` with water_share 0.747826099395752 (float noise in an error detail, Low). Rallied to (70,100).

## 13:23-13:27 (day 545-600) Second severe winter; Herdmaster10's herd extinct
- Pack 112 -> 51 (days 530-575) in a second severe winter (days 571-600) with prey gone from Herdmaster10's ground. Alerts again "dying off slowly: 68 → 51 alive over the last 10 days (25% fewer)" - 21-25% in 10 days still "slowly" (Low; threshold for "crashing" seems ~35%+).
- Day 580 reinforce x2: landed (73.5,102.9) and (73.6,101.5) with the group ("nearby 24/34"), well clear of the two rival territories (23,122) r15 and (76,45) r10. 3 more provisions.
- World tab (day 591): **Cinderfleece Drove (Herdmaster10) extinct day 551** - the pt9-class runaway herd (1,372 at day 416) was wiped out in the autumn/winter of era 2 while my pack wintered inside it; Herdmaster10 re-founded "Ashmeadow Grazers" (day 566). Era 1 table: me #5 (1.2% mean, 3.0% final), carnivore niche 35%. Achievements table shows Survived 0 for everyone but Rulebender10, yet the recent list still shows "Herdmaster10 Survivor: ... day 361" (public survivor feed vs keep-half rule; pt9 M7 partly present, as LateComer10 also found).
- Ranks, season 10 (2 days left): me Legacy 5th of 6 (2.0%), Largest lineage 4th of 6 (102). Nameplates «LateComr10» (Rulebender10, near-copy of a player name) and «Nomad Hunter» shown in the Ranks lists.

## 13:27-13:31 (day 640-691) End
- Day 683: satellite group B (split from 6 hungry hunters, rallied to empty ground) - "Group B has fallen from 5 to 4 animals: too few to hold together for long. Merge it into another group, or move it to better ground." (the "too few" path works).
- Season 11: #6 of 6 legacy and lineage.
- Final leaderboard day 691: PocketNomad10 653 (15.9%), LateComer10 279 (6.8%), GeneBroker10 331 (6.5%), Rulebender10 206 (5.3%), Herdmaster10 92 (1.8%), **ApexHunter10 71 (1.2%), 6th of 6 players**; wild 62.5%. Peak 112 alive (day 524), 3.7% (day 526), 3rd by head count around day 463-485.
- Watcher and scan loop stopped by PID; possessed animal released; hunt helper off. No page errors / console errors / failed requests in any of 5 UI runs.
