# Playtest 4: six AI players, after the playtest-3 fixes (2026-11-18)

Six AI agents played on one server (`--size 128 --seed 29 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 50–65 minutes each, game days
~15–830. Personas as before: herbivore swarm (Herdmaster4), predator
(ApexHunter4), breeder/trader (GeneBroker4), phone newcomer on an emulated
iPhone 13 (PocketNomad4), late joiner who registered 25 minutes in
(LateComer4), exploit tester (Rulebender4). Full logs:
`docs/playtest-4-logs/`; a few screenshots: `docs/screenshots/playtest-4/`.
The server stayed up throughout; no page errors, the only console error a
400 from the builder (Low 12).

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| LateComer4 | 25.9% (2,076 alive, at the hard cap) | Founded day 320 with a cold-blooded, live-bearing, brumating, toxic, honest-coloured flier; took the lead 23 wall minutes later; won era 2 |
| Herdmaster4 | 21–26% (2,006 alive) | One warm hibernating egg-laying herbivore lineage all game; won era 1 (41.8%) and seasons 2–11 |
| PocketNomad4 | 1.9% | Four lineages; starter lost 44% its first winter then hunted; a flier copy survived winters flat |
| GeneBroker4 | 0.4% | Season-1 crown; 6 cross-player matings completed; own territory starved its lineage |
| Rulebender4 | 0.3% (era 1 #2, 8.3%) | Retired on purpose to test re-founding; then hunted by a player predator |
| ApexHunter4 | 0.1% | Five predator lineages, four collapsed by starvation; 46 attack-order kills |

## Playtest-3 fixes confirmed working

Grace no longer restarts on re-founding within 2× the grace; capped at 60
game days, end alert (mostly) and "End my grace now"; player predators now
hunt (47 of 69 animals killed in 12 days); a kill feeds the hunter;
"looks poisonous" learning visible; burrowed prey flagged and untargetable;
thirsty animals drink (57 of 72 parched were drinking); cross-player matings
complete (rendezvous with direction/distance, escort); promised/pregnant dam
rules; market compatibility wording; reinforcement rolling window and
confirmation; re-founding soon after a retire halves founders; leaks closed
(sight checks first, coarse body plans, history capped, set_view narrowed);
nameplate/species-name filter refuses bidi, zero-width, markup, homoglyphs,
"Server Admin"; nameplates displayed; species naming UI; possession ends a
minute after disconnect; API players not counted offline; lineage warnings
(hunger, territory starving, near cap, grazed bare); dormancy toll at the
documented ~10% a winter, staggered waking, "winter sleep" cause, honest trait
text; founders' free first winter and winter provision save winter foundings;
founding ramp explained; five-eyes isolation closed (5 eyes ≈ 80–90%
fertile); toxins help no longer recommends drab toxins; phone fixes (notices
vs Tools, one-row chooser, top edge, Follow taps, renown pill, tour text,
gravid, Split at 4); designer warnings (drought at any size, upkeep, wings,
catastrophe); migration arrival radius ≤ 8, dormant-migrate warning, lake
targets moved ashore, lake claims refused for swimmers; title counts agree.

## Findings, by priority

### High

1. **The cold-blooded brumating toxic flier still dominates**, and a late
   joiner wins with it (LateComer4: 62 founders on day 320 → hard cap, 1st
   in 23 wall minutes). Brumators rouse to bask in mild winter weather and keep
   breeding (370 → 571 and 1,211 → 1,895 over two winters, zero non-old-age
   deaths); the toll only applies while dormant and births cover it at the cap;
   cold blood makes it sterile with every warm lineage (no dilution).
2. **Lineage names are unfiltered** (`design.rs` checks length/control chars
   only). "Server Admin" + RTL override is public in `/api/leaderboard/history`
   (no login), market lists, `/api/species`, achievements; it renders reversed
   in the Family tab; the founding species takes the lineage name, bypassing
   the species-name filter and fee.
3. **Founding placement still kills**: founders on deep water / lake ice that
   melts days later (13 of 18 standing in deep water; 10 → 2 for a burrower in a
   drought), a NE corner ringed by water and the tundra rim (21 of 27 starved in
   ~30 s real time), 4 of 50 drowned on day 1, a dry mountain corner.
4. **Player predators still starve out** (4 of 5 lineages; pure carnivore 45 → 9
   in 40 days with 100+ prey in view); one kill buys 1–2 days; only constant
   attack orders (every 15 s) sustain a band. **Toxic prey makes the role
   unplayable**: 4 of 5 rivals toxic; after learning, 65% still eat it on their
   own; hunters die within a day (36 toxin deaths).
5. **Big groups drift back after migrating**: the settle circle is the full
   rally radius (≤ 24 tiles), which can contain the origin (738-strong group
   "reached" with median 8.9 tiles, 3 days later median 22.9 and centre 16 tiles
   back); arrival still reported with medians 16–20 tiles out for 500+ groups;
   a 70-tile migration silently did nothing (no reached/timeout alert); 40
   fliers stalled after 25 of 70 tiles.
6. **Starter lineages are still filed under the wild Steppe antelope** (all
   starter lineages by ~day 180–216): no founding credit, no naming; the
   builder's 6% fertile estimate vs the market's 15%; the leading player can
   rename the initial wild species for everyone.
7. **Dormant animals die of thirst** (Rulebender4: 593 → 366 in one winter, up
   to 35 thirst deaths a day while 314 of 366 slept). (Possibly the same lineage
   bedding down parched before the thirst-30 rule; recheck.)

### Medium

8. **Territory `food_supports` is wildly optimistic** (uses peak regrowth; rated
   839/455/3,418 for herds of 200/97/250 that starved inside; r12 for 600 bare
   in 5 days). `standing_days` is computed but never warned on. Claim cooldown
   (~25 days) exceeds the time a big herd strips a territory (~15 days).
9. **Re-roll still possible**: the half-founders window is one season
   (60 days = 5 real minutes), and a lineage that dies out (not retired)
   re-founds at full size. The designer and `POST /api/lineage` show/charge the
   full count and never say "half"; the milestone alert isn't pushed live.
10. **Rendezvous problems**: the walk heads straight for the partner across
    lakes (drownings); a possessed animal doesn't walk, so a buyer can lure a
    rival's animal anywhere; agreements stall in winter (both asleep, UI says
    walking); one male can hold several agreements with one target.
11. **Dam-lock still cheap**: 10% cancel fee (1 renown on a cheap listing); a
    0-fee listing holds for free; the 30-minute deadline ≈ 375 game days.
12. **Nameplate filter gaps**: "Game Master", "GM", "Mod", "Owner", "Support",
    "Sysop", "wild", "Herdmaster4 (alt)" accepted; error text odd.
13. **Incubated eggs chill more than hatch** (351 chilled vs 304 born; 7,711
    chilled over a lineage), year-round, unexplained; the Eggs line mixes
    current clutches with lifetime losses.
14. **Season survival credit for players without animals** (survival #1 with
    no animals for 55 of 60 days; four survival titles from one catastrophe).
15. **Phone**: bottom map edge under the hint strip/Possess (edgePad.bottom 0);
    "Here:" chooser 737 px wide on a 390 px screen (× off-screen); two expanded
    notices cover 36% of the map.
16. **Possession blocks hibernation** (possessed animal stays awake and starves);
    possessed starving alert repeats every ~3 s; possessing a second animal
    silently releases the first.
17. **Reinforcements land unprotected** (8 of 10 eaten in 5 days, no info);
    clones of founders inherit generation 0 (free first winter).
18. **Winter advice wrong for cold designs** (recommends hibernation; should say
    brumation); no mention of the founders' free first winter.
19. `possess_attack` refused targets 0.4–2.7 tiles away as out of sight; hunger
    readings swing within a day (14 → 87 → 55).
20. `/api/spectate` without login is a map-wide radar of player herds.
21. Retire transfer re-tags the retiree's alleles to the receiver: colluding
    players can gift legacy share.
22. Territory/rally: claims anywhere (over a rival's herd, 47 tiles away, a far
    claim becomes home range with nobody within 80 tiles); a rally/claim silently
    replaces the group's order; cancelling a rally leaves no order; rally food
    ignores prey for carnivores; dry rally in a drought had no water warning.
23. Legacy share swings 7–13 points within ~60 days between two capped lineages.
24. Pace: "food for about 2 days" ≈ 10 real seconds at playtest speed.
25. The 1-renown fee floor makes 1-renown sales pay 0; the market title is still
    unwinnable in a 5-minute season; small players have no renown sink.
26. Client: Pixi "Cannot read properties of null (reading 'alphaMode')" page
    error once in the mobile e2e (sprite cache evicting a texture still drawn).

### Low

Builder fires `POST /api/design/compatibility` with an empty name (400) on load,
so the steep-bank water warning can't show until a name is typed; wings warning
printed twice; starter preview "small" for 4.2 kg; "The starting design is a
sturdy herbivore…" on carnivores; abandoned eggs "chilled: no parent was
warming them"; Mates outcomes replace "drowned" with "no longer alive (or out
of sight)"; real-time estimate wrong after reload (≈ 43 vs 30 min); design page
"Severe winter under way (1 days left)" after it ended; listing cards say "After
the other player agrees"; nameplate bought without confirmation; nameplates read
"Rulebender4Game Master" in text; "Litters are still full size" at ×0.9;
near-cap alert repeats every 3 days and floods the list; garbled "58% of
breeding alleles among the forest yours"; Seasons "Species founded: Nobody yet"
and World "Species 0" despite named player species; named species still offers
"Name it…"; builder vs server fertility estimates disagree; migrate water
warning for fliers; grace-end alert not seen by one player and client toast +
server alert duplicate for End grace; "Once-possessed" for an animal possessed
at death; carnivores shown dormant at night; "Request mating…" offered for a
sterile rival; plain click while possessing walks instead of selects; day
numbering (top bar 1-based year vs API 0-based; summary stamped a day later;
royalty ledger a day ahead; season day ranges differ between alert and API);
`settle.expires_tick` sent as u64::MAX; u64::MAX fee → generic bad_message; NaN in
a design → raw serde error; negative claim radius silently commits a 3-tile
territory and cooldown; API default body size 0.3 vs client starter 0.5; tour
reappears in every new browser context; Lineage tab stale after retire;
"4 about to lay" vs 3 gravid rows; grazed-bare alert for a cleared territory;
held-at-home shows "rally" and never expires; designer forgets the previous
design after a retire; limit-reached doesn't say when the next reinforcement is
possible; docs/game-design.md says pregnant dams can't be listed (they can, as
busy); death label "cold, heat or winter sleep" in charts.
