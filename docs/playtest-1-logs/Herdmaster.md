# Herdmaster playtest log (herbivore swarm)

Started 11:53:22Z, server day "day":21

## 11:54Z (day 33) Founding
Design: warm-blooded, live birth, litter 4, **130 founders**, body 0.5, fur .6, fat .6, hibernation .5, sentry .5, legs .2, ears .3, sociability .9, boldness .35, pickiness .15, mate tolerances .9, obedience .6. 99.7/100 pts.
- Register -> design -> found worked smoothly through the UI. Budget readout live and clear.
- OBSERVATION/BALANCE: 130 founders (77 pts) gave an **instant 12.4-13.4% legacy share** on the next snapshot, and 64 of the founders were *already pregnant* at founding (Lineage tab "Pregnant 64" seconds after founding). Founders at 0.7 pt each look like by far the best buy for share: a newcomer can buy ~1/8 of the world's alleles instantly. Weighted share is 0.0% though.
- UX (minor): design preview text says "small warm-blooded herbivore" while body size slider says "medium (adult ≈ 4.2 kg)" at 0.50. Inconsistent size wording.
- UX: after founding, the camera did not centre on my herd: the herd sat at the right edge of the map view, half hidden behind the sidebar (shot Herdmaster-04-founded.png). Follow not on by default.
- Season table "By world season" shows my Mean 4.5% for days 31-33 though I founded on day 33 - fine but mean is dragged down by days before I existed (may be unfair for late joiners in seasonal ranking).
- Console: only SwiftShader "GPU stall due to ReadPixels" warnings (headless), no errors.
- World: summer, drought 94%.
## 11:54-11:57Z (day 38-68) First herd collapses
- Day 42: 102 births in one day (64 founders were pregnant at founding). Pop peaked ~243 (day 43).
- Then steady losses every day: eaten 2-14/day + thirst 1-7/day, **zero births days 44-55 and 60-68**. Adults 130 -> 39 by day 68, alive 58. Share 13.4% -> 2.3%.
- Day 54 census: 92 organisms of GeneBroker (player 4, omnivore carnivory 0.46, size 0.49) were *inside* my herd's view window; 59 of my 168 were fleeing at that moment. A rival player's omnivore swarm parked on my herd appears to be the main "eaten" cause. No way in the UI to see *who/what* ate an organism: death alerts just say "was eaten" (UX: killer species/owner would help a lot).
- Thirst deaths in a herd parked beside a lake/river in 87-94% drought - herd stayed at the NE corner (117,19), grazed it to veg ~1/9 while lush land with water was 30 tiles south. Herbivore AI does not leave a grazed-out, dry patch by itself (sociability .9 keeps them together?).
- UX: alerts feed gets one "Founder #X was eaten" line per founder: with 130 founders the feed and toasts are a death spam ("and 7 more like it" helps a bit in toasts).
- Achievement oddity: "Species founded: achievement bonus +100 renown" on day 46 - /api/species lists my lineage "Herdmaster Grazers" as origin "split", parent Steppe antelope. So simply founding a lineage that resembles a wild species earns the "species founded" achievement and +100 renown a couple of weeks later. Free renown.
- Renown income comes from legacy share (+5..9/day). Market empty so far.
- /api/lineage/stats `vitals` returns the current day twice (e.g. two entries for day 68: the full one and an empty partial). The dashboard text says "the last bar is today so far" so maybe intentional, but the duplicated day key is confusing for API users.
- Day 68 (11:57Z): ordered **Migrate** to (118,50) via toolbar Migrate… + map click: worked, toast "Adults migrating toward (118, 50)". 
## 11:57-11:59Z (day 74-82)
- **BUG (Matings tab): the "My adult" <select> resets to the first organism a few seconds after you pick one** (the panel re-renders on state refresh). Repro: Matings tab -> pick the 6th adult in "My adult" -> wait ~3 s -> the select is back on the first adult (#1905). Verified by polling the select value every 0.5 s: 3034,3034,3034,3034,3034,1905. Consequence: I picked female #3037 + typed male #1917, pressed Send, and got `request_mating: organisms are the same sex (same_sex)` because the form had silently reverted to male #1905. A human who takes >3 s to type a partner id will send the wrong organism (could waste a mating slot / pair wrong animals).
- UX: Matings help text says "Pairs within your own lineage run at once", but they also must stand within 1.5 tiles (`too_far: organisms are 2.6 tiles apart; they must be within 1.5`). Not mentioned in the form; there's no hint of distance or "Escort"-like helper for own-lineage pairs.
- BUG/UX: a successful own-lineage mating toasts "Mating done within your lineage." but **Recent outcomes stays "None yet."** - no conception/fertility shown for own pairs.
- Toast "Founder #2003 drowned." while herd was migrating south: migration pathing into water?
- **Exploit / balance: "Species founded" achievement farmed for free.** World tab: my lineage was detected as a new species twice (day 46 "Herdmaster Grazers", merged back into Steppe antelope day 61, then day 66 "Herdmaster Grazers (2)"). Each counts (Species founded: Herdmaster 2; +100 renown each; Seasons tab "Species founded 1st"). A lineage that drifts across the species threshold repeatedly earns repeated achievements without any real speciation. The one that merged back still counts.
- Season 1 (days 1-60): I'm "Legacy 1st, 3.4%" and "Largest lineage 1st, 242" though I joined on day 33 and crashed. Late joiner with 130 founders can grab season titles cheaply.
- Sex ratio after the crash: 10 adult females vs 15 males; only 3 non-pregnant females. Pregnant females seemed to be eaten disproportionately.
- Seasons tab "Largest lineage 134 organisms" for me while I have ~45 alive (it's presumably peak/mean in season; label doesn't say which).
(Note: game runs ~6 s/day; ~10 game days pass per wall minute.)
## 11:59-12:03Z (day 90-110) Winter drownings, lineage nearly extinct
- **Migration target turned into a death trap.** My standing Migrate order to (118,50) (picked on day 68 where my veg/water scan showed a land block with some water) persisted into winter; the target ring was drawn *in the lake* (shot Herdmaster-10-winter.png, pink cross in water). Days 90-93: 4+3+1+1 **drowned**. The client lets you drop a migration target on/near water with no warning, and the order never expires (unlike rally's 3 days), so a forgotten order keeps pulling the herd into water/ice. Suggest: refuse or warn on water targets, snap to nearest land, or expire migrations on arrival.
- Map rendering: visible rectangular chunk seams - square patches with different detail/snow (top-left white snow block with a hard straight edge, darker square patches) in Herdmaster-10-winter.png. Looks like detail chunks vs base texture mismatch on winter snow.
- Matings form kept a stale partner id and showed red "Can't preview: organism 3336 not found" long after that organism died - fine but could clear itself.
- Income: +2 renown/day floor even at 0.0% share (fine). Renown piles up (529) with nothing to spend it on: the market had a single listing all game so far (GeneBroker stud #2040, 8 renown).
- Day 108: down to 4 then 2 animals (both adults, one dormant female far north at (115,5), one male fleeing at the map edge (128,62)). Lineage effectively dead; share 0.2%, rank 4/4.
- Strategy verdict for design 1: 130 founders bought a huge but *brief* share. Problems: medium body (0.5) gets eaten by wild + GeneBroker omnivores, no swimming, herd stayed on a grazed-out corner in a 90% drought, and my migration order drowned the rest. Can't re-found until extinct (409 already_founded), so a 2-animal remnant locks me out of re-founding.
- BALANCE/DESIGN: because founding buys share so cheaply, a rational player wants their failing lineage to die quickly to re-roll 100 fresh points; nothing stops walking the last few into a lake via possession (didn't do it). Consider letting players abandon/re-found with a cooldown, or not rewarding founders so heavily.
## 12:03-12:05Z (day 123-152) Extinct -> re-found "Herdmaster Bison"
- Lineage died out on day ~123 (start of spring). Lineage tab shows a clear "Your lineage has died out ... Start a new lineage. Your renown, records and titles stay." Good flow.
- Minor wrong number: after extinction, Lineage tab shows "Newest generation 0" (lineage had reached gen 2).
- UX: "Start a new lineage" opens the designer reset to the starter design (litter 2, 20 founders, empty name): my previous design isn't offered/prefilled. Re-entering 18 sliders by hand is tedious; a "start from my last design" button would help.
- Design 2: litter 3, 124 founders, **body 0.75** (free, for herd defence), fur .6, fat .6, hibernation .5, **swimming .3**, sentry .5, ears .4, voice .4 (alarm calls), legs .3, boldness .3, aggression .6, sociability 1.0, roaming .6, foraging range .7, pickiness .1, obedience .7. 99.7 pts.
- Founding took ~10 game days of wall-time in the designer (~1 min); founders arrived at (29,16) NW, on early-spring ground already grazed to veg 1-5/9. **9 founders starved/thirsted within the first 2 days** (day 140-141), and 49+24 wild red foxes were in view. Founders placed on poor ground starve immediately - pick_home should consider current veg/hunger state of the founders.
- 52 of 113 founders pregnant at once again (instant births).
- Day 147: **Split** (toolbar) -> toast "Group A (main) split: 93 went to Group B, 93 stayed", then **Rally…** Group B at (44,30) and Group A at (12,28) (lush blocks with water). Both rallies confirmed with toasts and coloured rings on the map (shot Herdmaster-15-rally.png). Group cards show "obedient (obedience 0.70)". Works nicely.
- Minor layout: the group <select> in the toolbar truncates to "Group A (mair".
- Wild population keeps exploding (3.4k breeding adults, 4.5k alive), so player shares are tiny: GeneBroker 7.8%, me 3.1%.
## 12:05-12:08Z (day 152-188)
- Listed 3 founder males as studs (fixed fee 3, expiry 20 d) via Market tab: toasts "#14807 listed for 3 renown." Worked. No buyers.
- **BUG (Market, My listings): "N days left" never counts down and is inconsistent at listing time.** Three studs listed within ~3 s with "Expires after 20 days" showed "20 / 18 / 17 days left"; 15 game days later they still showed 20/18/17. Then on day ~174 the toast said "Listing #5 of #14809 expired." while the card still read "18 days left". Server expiry was right (20 d), the client countdown is frozen/stale.
- Day 172: herd healthy, 321 alive (births 35/day on day 163). Rallies lapsed after 3 days as documented; groups had drifted only part-way to the rally points.
- Inspector: a pregnant female (#14810) showed **Hunger 100** while her action was "graze"; she starved within a minute. Body plan reads "medium warm-blooded herbivore" at body size 0.75 (size words look one step low: 0.5 was "small").
- Tried Possess on her -> she died before the move order; nothing confusing in UI, but no toast for "your possessed organism died"? (didn't see one).
- Day 182-186: starvation + thirst wave (5-11/day) — drought 77% in autumn, both groups had grazed their areas to veg 0-2/9. 108 wild omnivores (red fox) in view. Share now 4.2% (weighted 2.4%), rank 2/4 behind GeneBroker 13.1%.
- Day 188: per-group **Migrate**: Group A -> (20,53), Group B -> (42,45) (checked target tiles are land, veg 8/9). Toast "Group B adults migrating toward (42, 45)". Will clear orders on arrival (lesson from the drownings).
- UX: no minimap/overview of pasture quality; I had to pull /api/terrain/dynamic and compute veg myself to find grazing. A "grazing" or "food" map overlay would make group commands meaningful for a human.
## 12:08-12:11Z (day 188-226) Migration vs hibernation; winter
- Migrations to (20,53)/(42,45) were barely followed: in 30 game days group centroids moved ~5 tiles. Only a handful of adults show action "migrate" at any time (obedience 0.7!). Then winter started (~day 210) and 60/85 and 63/81 went **dormant wherever they stood** - scattered over 30 tiles of grazed-out ground (e.g. (1,33) to (32,23)), while the few awake ones did reach the targets. So migration as a player lever is very weak vs instincts (drink/graze/seek mate), and hibernation silently overrides it. UX: group card says "migrating toward 20, 53" even though 95% of the group is asleep - no indication orders are paused.
- Juveniles die over winter (13 -> 9; most young of autumn starved): only 1-2 starvations/day among sleepers.
- Phone layout (390x844, same page resized): no horizontal overflow; bottom sheet + tab bar fine. The Tools menu drops down *behind/over* the open sheet so its last item(s) (Pin view) are covered by the sheet (Herdmaster-21-phone-tools.png). Thin light horizontal seam lines across the map at chunk boundaries (Herdmaster-20-phone-lineage.png, y≈210).
- API: `request_mating` to a wild organism is refused (`no_consent`), so no direct allele-pumping into the wild. Good.
- GeneBroker share climbing fast: 21.1% (877 alive) vs me 4.7% (152). Wild 73.8%.
## 12:11-12:15Z (day 226-290) Spring 2 (year index 2)
- Herd survived winter with 140 adults (34 pregnant at waking), births 8-18/day early spring; lineage longevity achievement (+20 renown) at 1 year.
- "Whose genes": 5.6% of my organisms' alleles are now wild - natural interbreeding with wild Steppe antelope (pickiness .1).
- Day 245: sent Group A -> (40,72), Group B -> (84,70) (chosen from my veg scan: "veg 8").
- **Group A got stuck for ~30 days at the foot of a mountain and lost ~20 animals**: (40,72) is inside a big mountain massif (biome 7). The terrain API reports veg 8/9 on mountain tiles, and the client happily accepts a Migrate target on impassable mountain with no warning. The group card kept saying "migrating toward 40, 72" while members milled at the base dying of thirst/starvation. Expected: refuse/snap targets on impassable terrain, or show "unreachable" on the card. (Shot Herdmaster-24-groupA-stuck.png; also shows big rectangular texture seams at chunk edges.)
- Group B reached (82,68) on day ~260 then drifted back NW ~12 tiles to an area grazed to 0 while the order still stood (most showing action "migrate") - unclear why the order doesn't hold them at the target.
- Day 288: re-targeted Group A to (13,51). Share 5.4% (weighted 3.5%), GeneBroker dropped to 17.0% (735). Rank 2/4.
- Renown 1343 with nothing to spend on (only one market listing exists all game).
## 12:15-12:17Z (day 290-322) Four groups
- North regrew to veg 9 over the winter. Day ~305: **Split** both groups (toasts "Group A (main) split: 46 went to Group C, 46 stayed", "Group B split: 48 went to Group D, 49 stayed") and sent the 4 groups to 4 separate pastures: C->(20,36), A->(44,36), D->(90,68), B->(114,40). All orders accepted (per-group Migrate via the toolbar group menu). Split clears the parent's migration order, as documented, so you must re-issue it.
- GeneBroker's single stud listing vanished; market empty again. Listed a stud at **auction** (reserve 5, 3 days): toast "#16589 auctioned from 5 renown."; card "closes in 2.7 days" immediately after listing (should be ~3.0), and ~5 game days later it still said "closes in 12 h" - same stale-clock symptom as the fixed-fee "days left".
- Day 320 (mid-autumn, 10 days before winter): 43/67, 31/61 already **dormant**. Hibernation 0.5 starts very early, eating ~40 of 120 days per year; groups that were still walking to pasture fall asleep en route.
- A status loop now rewrites Herdmaster.status every 2 min.
## 12:21Z (day 330-366) Severe-winter catastrophe, spring 4
- Day 331 catastrophe "severe winter" (red top-bar banner "Severe winter: every tile is about 10 °C colder than a normal winter · 18 days left" - clear). My herd: 263 -> 157 by day 363 (starvation, plus **7 drowned in the spring melt days 360-362**). Wild crashed 2981 -> 1381, GeneBroker 744 -> 425. Relative share went UP: 5.7% -> 8.6% (weighted 8.4%). Hibernators ride out catastrophes well relative to others.
- Auction result arrived as an alert: "Auction #9 of #16589 closed without a bid: the lot is yours again." Clear. (The auction countdown was roughly right; the fixed-fee "days left" is the frozen one.)
- "Founder #X died of old age" alerts trickle in - fine.
- **Groups don't stay together**: Group B's 34 members were spread from (35,14) to (119,31) - across the whole map. The group centroid in the group card/Center button is meaningless when that happens. Group cohesion with sociability 1.0 is weak once orders + hibernation interleave.
- Early spring: many adults at hunger 100 (veg ~0-2 everywhere north). South greens first (rows 64-104 veg 7-8 while the north is 0-2). Sent A->(16,70), C->(12,86), D->(104,82), B->(116,70) after checking biome (avoid mountain 7 / desert 5; the terrain API's veg value is high on desert/mountain tiles too, which is misleading).
## 12:26Z (day 366-400) Southern pastures, possession test
- Era 1 (days 0-359) ended: GeneBroker 25.4%, me 8.3% (2nd), ApexHunter 0.5%, PocketNomad 0.0%. Niche leaders: GeneBroker marsh + omnivore; wild everything else.
- All 4 groups reached the southern targets within ~15 days this time (no obstacles). Hunger fell to 22-40, births 5-15/day. Day 393: 291 alive, share 7.1% (weighted 6.6%), 2nd. PocketNomad suddenly at 6.0% (185 alive) - looks like a re-found with many founders (same instant-share effect).
- Cleared all migrations with group menu "All" + Clear: worked (all migration null), but **no toast/feedback** for the bulk clear (single-group Migrate gives a toast).
- **Possession in a herd is fiddly**: with Possess active, clicking the map near other animals *selects* the clicked animal instead of giving a move order; the inspector then shows the other animal and the **Release button disappears** (the possessed one stays possessed, off-screen in the list). Clicking a spot with no animal within ~2 tiles did issue the order (action "ordered") and the organism covered ~12 tiles in about a second (possessed speed 1.46 tiles/tick at 10 ticks/s), then went straight back to genetic behaviour (seek_mate/drink) while still possessed. To release, I had to find it in Lineage > My organisms. Suggest: while possessing, a map click should always be a move order (select with a modifier), and keep a persistent "Release #id" control visible.
- Inspector button "Possess (releases #48065)" is a nice touch.
## 12:28Z (day 400-455) Boom summer, share 10%
- Southern pastures + no orders = boom: 291 -> **677 alive by day 446** (353 adults, 56 pregnant), share 10.3% (weighted 10.4%). Gap to GeneBroker (18.4%) halved. Clearly the right play was "find the green belt (south greens first), get there, then stop giving orders".
- Season 7 title: "Survival 1st of 3 · 1 catastrophe" for me.
- **Wrong data in World tab species tree**: my extinct first lineage's split species "Herdmaster Grazers (2) · merged into Steppe antelope · **PocketNomad 100% of alleles** · founded by Herdmaster". PocketNomad never had anything to do with it; looks like a stale or reused species/lineage id when computing the dominant owner of a merged species.
- Balance observation: "Red fox · merged into Brokers (2)" - GeneBroker's omnivores interbred with the wild red fox until the wild species merged into theirs. My Bison likewise merged into "Steppe antelope (wild 51% of alleles)". Designing close to a wild species lets you absorb it and spread your alleles through wild bodies; GeneBroker's share (18%) exceeds its population share partly this way. Players can't see this mechanic anywhere in the UI except indirectly.
- Mating form: Playwright selectOption on #mating-mine timed out for a female id that was in state a moment earlier - the select's options are rebuilt every refresh (same root cause as the reset bug).
- Market "List one of my adults" select offers 173 organisms while Matings "My adult" offers 383 adults - different filters with no explanation (probably excludes pregnant/busy).
## 12:30Z (day 484) RANK 1
- Woke from winter at 903 alive, 721 adults, **297 pregnant**; **share 17.5% (weighted 20.0%) - rank 1/4**, ahead of GeneBroker 16.5%. Winter losses were tiny this time (hibernating juveniles survived; south is milder).
- New catastrophe "flood_year" days 480-503 - watching for drownings.
## 12:33Z (day 488-508) Hit the lineage cap
- Day 491-494: **+55, +248, +174, +61 births** in four days -> alive pinned at **1500 = lineage_cap** (1500 x area 1.0 on this 128² map) by day 493 and stays at 1498-1500 while 200-300 females remain "pregnant" and births drop to 1-10/day. Share plateaus ~18% (weighted ~20%), rank 1/4.
- **UX: the cap is invisible.** Nothing in the Lineage/Genetics tabs says the lineage is at its hard cap or that births are being suppressed; it just looks like the herd mysteriously stopped breeding with 300 pregnant females. Suggest "1500 / 1500 (lineage cap)" on the Alive tile.
- BALANCE: the cap counts juveniles (669 of 1498 are young), so at the cap a fast breeder wastes slots on young that don't score; and once every serious player is at the cap, the leaderboard is decided by wild interbreeding/market spread rather than play. The cap also makes re-founding with 124 founders + litter 3 strictly better than careful play early on.
- Flood-year catastrophe (days 480-503): 11 drowned total - mild.
## 12:36Z (day 510-560) Holding #1
- Listed 5 more studs at 1 renown / 60 days (toasts OK). Only other listing on the market: PocketNomad stud #51155 "herbivore, furred" while World tab calls PocketNomad's species "Nomad Brokers · omnivore" (diet label mismatch between market summary and species tree?). No buyers for any of my studs all game: the market is essentially unused by the AI players; renown (4932) is useless to me.
- **Territory test** (Group C, toolbar Territory…): hints "Click the center of Group C's territory (Esc cancels)." then "Click the edge ... (radius up to 24 tiles)"; clicking 30 tiles out clamped to 24: toast "Group C holds a territory of radius 24.0 at (15, 69)". Disc drawn (Herdmaster-31-territory.png). Group state shows home_range true, protection_left_min 720. **Abandon territory** (group card) worked but gave **no toast** (Claim gives one).
- **Rendering bug, very visible zoomed out: snow is drawn as big hard-edged white rectangles** (chunk-sized squares) rather than following terrain (Herdmaster-31-territory.png top of map). Also, zoomed out, sprites overflow past the map's left/right edges.
- Market card for another player's stud shows an enabled-looking blue "Buy for 1" with the note "pick one of your adults" next to it when no adult is picked (should look disabled).
- Day 559: alive 1500 (cap), adults 1285, share **20.8% (weighted 22.0%) rank 1/4**; GeneBroker 16.4%, ApexHunter growing (976 alive). First "disease" death seen.
- Page reload (session remembered): back to connected game in ~2 s, groups intact, tour not re-shown. Good. Still **zero console errors / failed requests** in the whole session so far (only SwiftShader GPU-stall warnings).
- **Family tab "Founding" stage is wrong after time passes**: earlier it read "Founding · Day 137 · 122 animals alive · As designed" (correct: founded day ~137 with 124). At day 566 it reads "Founding · Day 209 · 205 animals alive · As designed" - the first stage label now points at a later sample (history window truncated?) but is still called Founding.
## 12:41Z (day 604-616) Spring 6
- Out of winter at 1324 alive / 1034 adults / 396 pregnant: **22.5% share (weighted 27.1%), rank 1/4**. Back at the 1500 cap by day ~614; at the cap births ≈ deaths (e.g. "Day 614: 22 born, 22 died (11 starved, 5 eaten, 5 thirst)").
- Alert preferences / push cards render fine; "Enable on this device" correctly disabled with "Push notifications are turned off on this server."
## 12:50-13:04Z (day 720-889) Era 2 won; wrap-up
- **Era 2 (days 361-720) final: Herdmaster #1, 20.8%** (GeneBroker 12.8%, ApexHunter 2.6%, PocketNomad 1.4%); niche leader tundra 49%. 5 legacy crowns / 15 titles by season 13. World tab: "Steppe antelope · ~1690 adults · Herdmaster 57% of alleles" - my lineage absorbed the wild antelope species.
- Day 889 (13:03Z): alive 1500 (cap), 1188 adults, share 18.7% (weighted 18.4%), rank 1/4. ApexHunter (carnivore) at 1372 alive and "eaten" deaths creeping up (4-9/day).
- Server stayed up the whole session; no console errors, no failed requests in ~70 minutes.

## Summary of what worked
1. Founders are the best buy (0.7 pt each): 120-130 founders give instant share and ~half arrive pregnant.
2. Design 2 (body 0.75 free, swimming .3, hibernation .5, fur/fat .6, sentry/ears/voice, litter 3) survived; design 1 (body .5, no swimming) died to predators + drought + a migration order that pulled it into a lake.
3. Group play that mattered: Split into 4 groups, Migrate each to green pasture (south greens first in spring), then **clear orders** and let them graze. Long-standing orders, rallies and territories hurt.
4. Once at the 1500 lineage cap, nothing a player does moves share much; spread into the wild via interbreeding did the rest.
