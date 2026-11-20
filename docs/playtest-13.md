# Playtest 13: six AI players, after the playtest-12 fixes (2026-11-20)

Six AI agents played on one server (`--size 128 --seed 71 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) on main at a668cdd (all of round 13:
recent-peak share reference and a litter brake past a quarter of the legacy,
two niche prizes per player per era, era score = day-weighted mean + last
tenth, cohesion with a home/territory, pregnant pack females fed first,
instinct grace, one need-alert budget with graded declines and a weekly
hunted view, migration-end alerts, reordered/two-swap/blend name checks,
territory warnings only in sight with a per-player order cooldown, leak
fixes, unfit-animal offer refusals, pair shield for pending offers, one-tap
listing, restore at the quoted centre with a 50-food minimum, escrow payout
ledger line, own-reply toast pinned, landing toast once, a deeper-sleeping
starter, client and server Lows). **The container restarted at about 01:00
UTC, ~51–53 minutes in (game day ~630–640), and took the server with it**;
every player's log survived and each wrote its report from it. The first late
joiner (LateComer13) quit after 7 minutes without registering; a re-run
(LateComer13b) founded on era 2's first day and played ~22 minutes. Full logs:
`docs/playtest-13-logs/`; screenshots: `docs/screenshots/playtest-13/`. No page
errors or failed requests.

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| Rulebender13 | 33.8% (975) | An exact copy of the cold live-bearing toxic bright flier: won era 1 (score 21.8%, 30.4% at its end), seasons 2–8, Survivor |
| LateComer13b | 22.2% (819) | A **flightless** copy (toxins/colour 0.95, brumation, carnivory 0.34) founded on era 2's first day: #2 within ~100 days |
| Herdmaster13 | 5.0% (265; peak 624, 17.2%) | Warm incubated-egg toxic hibernating herd: #1 on day 567 after a severe winter, then a summer thirst crash; era 1 #3 |
| ApexHunter13 | 1.8% (90; peak 192) | Pack peaked only by eating the largest wild antelope herd (988 → 167); crashes every winter; carnivore niche |
| PocketNomad13 | 1.1% (~60; peak 306) | **Kept the starter all game**: winter starvation fixed, but 60–80% lost each year to drought and a pack eating its sleepers; season 1 crown |
| GeneBroker13 | 0.5% (~25; peak 287) | Starter design; era 1 #2 (3.2%), tundra niche; **10 cross-player trades**, 2 fee-free listing sales (one via "List it for 10") |

The two copies of the cold toxic bright brumator held 55.6% of the world;
wings don't matter (the flightless copy reached #2).

## Playtest-12 fixes confirmed working

Need-alert budget (one loss alert every 2–3 days with "Also:"), graded declines,
weekly hunted view naming the hunter; migration arrival/end alerts; restore at the
quoted centre (exact 4,096 / 4,778 for two players; ±4–9% for others), poor-value
hint, nothing in winter, `quote_changed` worked; landing toast once; own-command
reply pinned on the phone (one-line replies); unfit animals (old ≥ 90%, asleep,
starving) refused for offers with reasons; offers show ages; lineage offers say
"halfway"; outgoing offers show the distance; listing positions as 32-tile regions;
meet_note without placement; escrow payout line; shop fee-free text; "List it for 10";
reordered names, two-swap names and blends with rival lineage names refused; per-player
migrate/rally cooldown, territory warnings only in sight (Rulebender13); anonymous
metrics zeroed, spectate hides player lineages; recent-peak share engages in droughts
(shares summed to 69–71%); the starter's winter-sleep starvation fixed (winter losses
13–30%, almost no starvation, even through a severe winter); instinct grace held (no
bite until the pack's grace ended); toxin deaths in decline alerts; era score computed
as designed; retire dialog count; Merge/Clear/not-now/Ranks fixes; group names capitalised.

## Findings, by priority

### High

1. **The leader's share still runs past 30% every winter.** Young mature over winter
   while wild adults die, so the world's breeding total stays flat and the recent-peak
   reference never engages (shares sum to ~100% in winter); the 25% litter brake can't
   act because nothing breeds in winter. Rulebender13: 19.3% → 30.2% (days 212–240),
   20.3% → 30.1% (320–362), 27.9% → 33.8% (464–636), each while its head count fell. A
   severe winter (wild 2,594 → 680) hands the world to sleeping player herds.
2. **The cold toxic bright brumator dominates** (55.6% of the world for two copies; wings
   irrelevant: toxins + colour + brumation + cold blood carry it). Warm herds boom each
   spring and crash each summer/drought; the flier holds a flat ~900–1,100.
3. **Sleepers are a larder for packs**: the starter (toxic 0.5, warning colours) is eaten
   2–5 a day for ~50 days each winter (~30, then ~95), and toxicity doesn't deter; the
   flightless flier copy lost 55 sleepers in 21 days. Sleepers can't be moved; offline
   home-range protection only applies while the owner is offline.
4. **The era score's "last tenth" always falls in winter** (an era is 3 years), exactly
   when the leader's share spikes; it's still labelled "Era mean" in the UI and API.

### Medium

5. **Pack**: peaks only by wiping out the biggest wild herd, then starves to 75–90 and
   crashes each winter; the adult sex skew is back (36–45% female; 5 of 20 animals older
   than 108 days female; founder females died at 4× the male rate — females die more in
   crashes); scatters with no order; most wild prey near a pack are unreachable burrowing
   voles and every player herd is toxic (prey checks "feeds 1–17 of 65–89"); 50 of 60
   attack orders on above-ground small prey refused as `sheltered`.
6. **Founding/grace text contradicts instinct grace** ("even one your animals start on
   their own" in the founding reply; End-grace panel "yours can't attack theirs").
7. **Reinforcements land 34 tiles away** when the group is inside/next to rival ground and
   die alone (8 of 10 in 24 days, 150 renown wasted); the 300-alive rule locks out a
   crashing newcomer ("you had 306" while down to 69).
8. **Names**: reversed names with a short extra word or run together ("Nomad Pocket II" —
   now a permanent species name listed above its target's — "Nomadpocket", "Comer Late II",
   "Master Herd Jr", "Masterherd", "The Master Herd", "Apex the Hunter"); two swaps split
   across words with an extra word ("Pokcet Nomda II").
9. **Market fitness**: the offer gate's 90% old-age line fires too late (old-age hazard from
   ~62% of lifespan, mean death at 85%: six deals failed to old age at 61–88%); buy_listing
   and the accepter's pick (respond_mating / lineage_candidates) run no fitness check
   (dormant buyer accepted; candidates at hunger 100 / thirst 92 / 92% lifespan listed);
   listings have no age/fitness check; a substitute keeps the original deadline (47 s
   left, whole fee forfeit); an old-age death mid-agreement forfeits the hold fee; the same
   animal can sit in two pending offers to one lineage (price escrowed twice); lineage-offer
   `apart` counts unsuitable adults (said 16, walks were 40–72); meeting points still slide
   ~90% to one side.
10. **A kept territory is dropped when a migration arrives** (reply promised it'd be held;
    claim cooldown keeps running) — seen by two players; a far claim interrupted by winter
    sleep loses both territories with only half an explanation.
11. **Groups drain into the main group** after splits (A 195 → 435 while B 159 → 24), and
    "Group B has fallen from 97 to 46 adults" reads as deaths.
12. **A claim 18–28 tiles from a hungry group is never reached** (loosened hold forages in
    place; reply said "feeds 100" with no warning).
13. **Catastrophe survivors**: the epidemic lists `survivors: []` and the World tab says "0
    of 5 player lineages survived" when all five lived (none was "hit").
14. **Alerts**: "dying off slowly" graded on net change while 40% died gross (also 23–31%
    falls); 5 "crashing" toasts in 57 s with shifting baselines on one phone; eggs-failing
    every 3–4 days; territory water/grazed alerts every ~8 days while asleep, outside the
    fold; decline advice "fatten it before the cold" in midwinter.
15. **Wake-up time wrong** in the migrate reply ("wake in spring, about 119 days from now"
    on day 597; they woke on day 601).
16. **Phone**: a two-line reply pins the side line and the main line is pushed out; the
    designer's interbreeding check says no player lineage would be fertile, the Market later
    says fully fertile (matings conceived); timed breeding founded in summer gives no young
    for ~90 days with no designer warning; heat deaths for a fur-and-fat design with no
    designer heat warning.
17. **Big-group overclaim takes a small player's territory**; graced squatters can't be
    displaced (2-animal squats on key water for 100+ days).

### Low

Grace days still public (rounded) in anonymous `/api/world`; pending lineage offer `apart`
recomputed live (ranging by possessing the offered animal), `too_far` gives the distance;
`/api/leaderboard/history` body values to 3 decimals; `/api/spectate` lets anyone copy the
leading class (no login) — may be intended; mating updates while paused asleep flap 68↔72
every few seconds; "Meeting point predators are near every point…" garbled; "too_many_requests"
wording/ticks wrong; Provision feeds sleepers though the alert says they can't be fed; water
alert on a frozen lake shore; restorable food volatile in early spring; possessed animal left
stranded after a mating; leaderboard shares sum to 69–88% with no note; "13 of the 47 animals
holding territories" with 86; hidden designer text; daily summary day off by one (text vs
field); "founded on day 361" vs 360; species diet label at 0.34; shop `market_fee_percent` 10;
two restore prices (shop vs group); `/api/eras` era score as `mean_share`; survey charges with no
confirm over the API and the button stays live; provisions at the limit quote 40/20; flier "4.1
tiles a day"; rally+cancel free probe; overlap refusals don't name the territory; "Generation 2"
from reinforcements; "defend" behaviour unexplained; phone leaderboard row wraps; one species
with two names (Market vs World); restore centres on the animals, not the territory; long news
toasts vanish in 6 s; Ranks names cut off at 390 px; "Cleared: migration" doesn't name the group;
"outgrown its territory" at 68 of 69 fed and right after a claim; "accepted" repeated per
substitute; offer card age 118 vs API 138; survey results only in the Market; Tools menu left
open makes Territory "active"; restore "next costs 400" after the season reset; renown piles up.
