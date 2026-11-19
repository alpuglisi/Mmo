# Playtest 9: six AI players, after the playtest-8 fixes (2026-11-19)

Six AI agents played on one server (`--size 128 --seed 53 --tick-ms 100`:
a game day ≈ 5 s, a year ≈ 10 min) for about 44–64 minutes each, game days
~11–800, on main at cbbb6dd (all of round 9: retire transfers, grace carry-over,
squats, migrate warnings, landing text, client and server Lows, seller-at-fault
market rules, stud-raid bar, name filter rewrite, clutch split, estivation
advice, group alerts, pasture restoration, survival rules, founder youth,
founding search away from rivals). Personas as before. Full logs:
`docs/playtest-9-logs/`; screenshots: `docs/screenshots/playtest-9/`. The
server stayed up; no page errors or failed requests (only the expected 400s
for refused names).

## Result

| Player | Final share | Notes |
| --- | --- | --- |
| Herdmaster9 | 22–39% (709–1,772, swinging) | Playtest 8's warm small incubated-egg hibernating toxic bright herd, litter 8; held to 5.9% mean in era 1 (peak 571), then 24% mean in era 2 as the wild shrank 3,800 → 1,250; won both eras and seasons 1–10 |
| PocketNomad9 | 9.8% (535) | Starter and a first re-found eaten by the pack; a burrowing warm egg design grew 10 → 535; 2nd at the end |
| LateComer9 | 8.2–14% (733 peak) | Cold brumating toxic flier on era 2's first day; top player around days 610–623; era 2 #3 (6.6% mean), hunted 9–14 a day by the pack |
| ApexHunter9 | 3.9–11.5% (261 peak) | Live-bearing pure carnivore pack, era 1 #2, at times 2nd by share; carnivore + 3 biome niche prizes |
| Rulebender9 | 4.2% with 19 alive | Four lineages; most of its share is genes in other lineages |
| GeneBroker9 | 0.3% | Two lineages eaten by the pack; no trade completed all game |

## Playtest-8 fixes confirmed working

Predators can compete (pack 240–261 against herds of 300–1,700; hungry hunters
take bright toxic prey and pay in toxin deaths; attack warning scales with
hunger; honest prey warning; attacks refused for sleepers); large clutches
partly hatch (4 of 8) with a designer warning, chill warning, egg alerts name
the cause; estivation advice states its costs; group alerts count the group's
own deaths; View group on the densest cluster; territory kept during a
migration; migrate warnings (dormant, rival territory, lasting water, shared
journey food, amber toast); landing distance text; grace carries over through
re-foundings within 375 days of the last founding; retire transfers capped and
re-tagged wild; name filter accepts all 20 playtest-8 false positives and
refuses the listed bypasses, animal names checked; `/api/assign` masked;
metrics command counts hidden; seller-at-fault code; re-found fee and debt
shown; restore_pasture works; designer starts from the server's last design and
keeps its name; tour once; disabled buttons explain themselves; "while you were
away" only after 5 minutes; founders land young; survey price rule; Market naming
picker; shop refuses players without a lineage; niche prizes go to players.

## Findings, by priority

### High

1. **The warm egg herd runs away again in era 2.** Held in era 1 (5.9% mean),
   it rebounds every spring once the wild shrinks (302 → 1,211, 265 → 1,764; 39%
   of breeding alleles by day 786); only winter brakes it (−60–70% a year);
   24% era-2 mean, every season and niche prize to the leader.
2. **Re-founded and late lineages still land beside rival packs and herds**
   (1–2 tiles from ApexHunter9's pack for three players, with no grace; inside
   Herdmaster9's herd; 1 tile from GeneBroker9's animals; one founding 9 tiles
   from water, 21 of 60 dead of thirst). Retired lineages are not covered by the
   avoid-last-home rule; a scattered pack escapes the group-centre scoring.
3. **A fresh grace after re-founding**: grace carry-over is measured from the
   previous founding, the re-found window from the lineage's end; a lineage older
   than 375 days retires and re-founds with half founders, the fee, and a brand-new
   99-day grace stretched through winter — the retire panel says the opposite.
4. **Genes cross into a rival's herd by standing in it**: natural mating between
   two players' animals is barred only while possessed (2 days); a fully fertile
   design parked in a rival herd reached 45% rival genes in ~20 days; crossed genes
   keep scoring and paying royalties for a retired/extinct owner (Rulebender9 at 4.4%
   with 0 animals).
5. **A pinned view keeps watching after the player's animals leave or die**
   (`set_view` checked only at pin time; up to 8 connections = 8 cameras).
6. **Wild interbreeding still booms some designs**: a warm live body-0.25 design
   58% fertile with the wild Meadow vole (19% vole genes, 35 → 393 in 80 days);
   body-0.5 live grazer 77% fertile with Steppe antelope; Herdmaster9's herd
   hybridised with voles into a "Herdmaster9 II" species.

### Medium

7. **Survival**: credited at a season rollover in the middle of a catastrophe and
   never taken back (Rulebender9 kept 19%); public survivor lists
   (`/api/catastrophes`, achievements, World tab) ignore the keep-half rule; text
   still says "a quarter"; a benign flood year ("801 more organisms") gives all six
   players Survival #1.
8. **Names**: lineage names containing a player's name pass ("Herdmaster9x", "The
   Herdmaster9", "Herdmaster9 herd"), near-copies of rival/wild names ("Ironjaws",
   "Emberwool Drovers", "Red fox pack"); species names "Herdmaster9 II" accepted and
   permanent; nameplate "Real Herdmaster"; account names may hold another's name plus
   2 letters ("TheHerdmaster9"); name checks run before "already founded" (free probe).
9. **Grace and placement for newcomers**: no "grace ends in N days, a pack is near"
   warning; the large-group takeover ignores a newcomer's grace.
10. **Market**: still thin (no trade completed in 50 minutes; 8 of 12 offers failed to
    predation); a buyer can walk its animal into the seller's pack and collect; a
    sleeping seller can be blamed after the 30-day pause cap while hibernation lasts
    ~45 days; listings don't show dormancy; "moved away" counts fleeing or drinking;
    retire transfer re-tags the receiver's own alleles wild.
11. **Journey estimate** ignores the group's current hunger/thirst and water along a
    dry route (153% food promised, 65% too hungry/thirsty); migrating a day before
    winter has no bed-down warning (a rally does).
12. **Reinforcements** land inside rival territories, beside packs, and on the map
    edge; the leader buys cheap reinforcements every spring after winter drops it
    under 300.
13. **restore_pasture**: no preview, always group 0, works in midwinter and feeds
    rivals' groups in the circle; an ally can buy it cheaply on the leader's ground;
    sets plants to the tile maximum regardless of season/drought; "restored 5386.2998"
    has no unit. Renown still piles up (1,280–4,500 unspent).
14. **Leaks**: `/api/metrics` opened−closed connections and login hash counts, `applied`
    world-wide; `/api/worlds` players online without login; migrate replies
    (`others_on_the_way`) as a radar; listings reveal herd position to a 16-tile cell;
    `same_sex` error reveals a rival animal's sex.
15. **Warnings**: a satellite group dies with no group alert; "learned they are
    poisonous" fires when a lineage's grace ends, before any bite; toast stack covers
    the second row of map tools (desktop) and hides toasts on phones; claim reply
    "feeds 235" then "no longer feeds it" two ticks later; "outgrown" right after a claim.
16. **Packs scatter over the whole map** despite pack instinct 1.0.

### Low

"1 tiles"; "The megadrought is over" with "Drought: 96% dry" in one tick; a new
lineage shows the retired lineage's last order and group state; "Group A (main)" with
0 members; "dying off slowly" for a 45% crash; carnivore advice worded for grazers;
prey count doesn't mention graced animals; overlap refusal clause broken; stud
listings show exact position; biome niche won at 0.96%; voles refused as `sheltered`
without `burrowed`; whole-lineage view centred on the mean; float noise in
preview_mating, listings, compatibility and migrate replies; offers accepted at 0%
fertility; per-founder eaten alerts (42); retire confirm doesn't quote the next fee;
animal names lost on death without a word; reinforcement unlock day off by one;
reinforce-blocked text omits the 300-alive limit; survey balance vs state; `/api/eras`
order by a mean it doesn't show; designer re-runs compatibility 19× in 3 idle minutes;
group membership drifts after a migration; re-found cost "21.0" vs server 31.9;
listing-closed alerts don't name the killer; owed debt has no ledger entry; season 1's
crown rewards founding first; eggs-failing alert repeats through good seasons; water
click gives two warnings; rival-territory warning 27 tiles away without the distance;
shop price drift between quote and purchase; "Possessed … parched" 5 s after an order;
species tree draws separate foundings as splits.
