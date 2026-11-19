# What changed since playtest 1

A summary of seven AI playtests (`docs/playtest-1.md` to `-7.md`)
and the fixes between them. Each ran four to six AI players on one
server at playtest speed (a game day ≈ 5 s, a year ≈ 10 min) for about an
hour. Current rules: `docs/game-design.md`; details and measurements:
README (*Playtest 1: ecology fixes*, *Founders, the soft cap and the market*,
*Predators against players*, *Playtest 2: competitive balance*, *Playtest 2:
predators, toxins and drowning*, *Playtest 3: balance*, *Playtest 3: founding
spots, steep banks and winter eggs*, the three *Playtest 4* sections, and
the *Playtest 5* and *Playtest 6* sections, and the economy's *Playtest 3–6 changes*).

## At a glance

| | Playtest 1 | Playtest 2 | Playtest 3 | Playtest 4 | Playtest 5 | Playtest 6 | Playtest 7 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Players | 4 | 6 (with a late joiner and an exploit tester) | 6 (same roles) | 6 (same roles) | 6 (same roles) | 6 (same roles) | 6 (same roles) |
| Winner's build | Big swimming, hibernating grazer (a second lineage of 124 founders) | Egg-laying, toxic-but-drab, hibernating burrower founded late in winter | Cold-blooded, brumating, toxic 4 kg flier with extra eyes | The same kind of flier, live-bearing and honestly coloured, founded by the late joiner on day 320 | The same class: cold, brumating, toxic, bright fliers, played by 4 of 6 players | Warm, small, incubated-egg, hibernating, toxic bright herd; a late cold flier first by share at the end | No runaway; the late cold flier won era 2, a copy of the warm egg-laying herd led at day 654 |
| Winner's head count | 1,500 (the hard cap) | ~3,000 (the 2× ceiling) | 2,100 (the 1.4× ceiling) | 2,076 (the hard cap); runner-up 2,006 | 2,090 and 2,100 (two at the cap); third 1,600 | the hard cap from ~day 200; late flier 1,719 | 1,130–1,560 and 1,740 |
| Winner's share | 18.7% | 26–40% | 22–31% | 25.9%; runner-up 21–26% | 22.6–24.6%; fliers held 56–72% of player alleles | 28.6–58.8%, every legacy crown; late flier 18–29.5% | 17.6–27.1%; shares spread 6–27% |
| Player trades | 0 | some (rivals accepted paid offers) | 0 of 21 cross-player matings completed | matings completed (6 for the broker) | 0 completed sales (most players sterile with each other) | sales, purchases and a substitute completed; few listings | none completed; offers unanswered |
| Player predators | couldn't hunt players at all | starved or were poisoned | collapsed in minutes; never out of grace | hunted (46 attack-order kills) but 4 of 5 lineages starved | packs held 60–223 animals without orders; 63 attack kills | a pure carnivore held 60–100 but couldn't grow; 69 kills from 185 orders | a live-bearing pack held 63–84 for 300 days; hungry hunters still avoided bright toxic prey |
| Server | stayed up | stayed up | the container restarted mid-test; restored from its save with no game data lost | stayed up | stayed up | stayed up | stayed up |

## After playtest 1

- **Founders cost more:** 0.7 points each up to 50, then each dearer than the
  last, so a full budget buys 88 founders instead of 160.
- **Soft lineage cap** instead of a hard wall: litters shrink as a lineage
  nears 1,500 and members get hungrier past it.
- **A reason to trade:** breeder's royalty, a renown shop (mating slots,
  reinforcements for small lineages), a market fee, titles that need several
  sales, compatibility shown on the market.
- **Retire a lineage** instead of drowning it; a founding ramp so re-founding
  doesn't spike the score.
- **Migrations** refuse off-map and water targets, avoid melting ice, break off
  to drink, end on arrival, and show their route.
- **Attack orders and pack hunts** for predators against players, with a
  20-day founding grace for newcomers.
- **Ecology:** parched burrowers leave to drink; large foundings are split and
  placed by food actually standing.
- **Client and security:** visible catastrophe notices, food layer, founding
  warnings, panels that keep input, friendly errors, map image behind login,
  look-alike names refused, request rate limits and blocking.

Playtest 2 confirmed most of these and found trades finally happening.

## After playtest 2

- **Egg-laying is no longer a free wall:** an egg layer is ~10% fertile with a
  close live bearer instead of sterile with all of them.
- **Winter has a cost for sleepers:** a dormancy toll on warm hibernators and a
  6-day breeding lag after waking.
- **Big bodies cost points** (free only from 1.1 to 3.6 kg), so player
  herbivores are huntable again; hibernation and burrowing cost more.
- **Lower, steeper cap:** hard ceiling 1.4× the cap (2,100), down from 2×.
- **Old age is spread out,** so a boom cohort doesn't die in one week.
- **Honest compatibility:** sampled pairs of real animals, a fertility range,
  share of fertile pairs and a drift risk; player species never called wild.
- **Offers in real time:** 10 minutes to answer, 30 minutes to meet; nothing
  expires at a season change, and offers survive restarts.
- **Grace made visible and two-sided,** lasting at least 30 real minutes.
- **Reinforcements capped** at two a season (retired lineages count), price
  doubling.
- **Water checks** on territory and rally claims; drought alerts and banners,
  also on phones; possessed animals drink.
- **Leaks closed:** genomes in the leaderboard history need login and show
  rivals coarsely, spectate hides graced lineages, out-of-sight mating
  previews refused.

Playtest 3 confirmed these: the playtest-2 winner's build no longer dominated
(the toll cost it ~25–30% a winter).

## After playtest 3 (round 4)

Playtest 3 found grace restarting on re-founding, predators collapsing,
thirst beside water, fatal founding spots, five eyes buying isolation,
cross-player matings never completing, big groups "arriving" far out, a new
dominant flier, and territory claims that starved herds. Round 4:

- **Grace:** re-founding continues the old grace instead of starting a new
  one; capped at 60 game days; an alert when it ends; "End my grace now".
- **Predators:** a kill on an order feeds the hunter; predators learn which
  prey is toxic (bright colours teach faster, drab toxic prey is still mostly
  taken) and players see "looks poisonous"; burrowed prey is shown and can't
  be targeted.
- **Drinking:** parched animals beside water drink before they eat; thirsty
  animals head for a shore they can walk down to.
- **Founding spots** judged as they are now: lasting water, no lake ice,
  standing food, snow and heat, map edges in winter, away from earlier
  failed foundings. Founders sleep their first winter free; winter egg
  layers wait for spring.
- **Winter:** the toll is ~10% a winter as documented, brumators pay it too,
  brumation costs 10 points a unit (was 6), hibernators wake over 10 days.
- **Eyes:** an eye step counts much less for fertility (5 eyes ≈ fertile
  again) and costs 3 points.
- **Starter** lays 3 incubated eggs, meant to be its own species.
- **Matings:** agreed pairs walk to meet, with direction and distance shown;
  pregnant and promised dam rules; the accepter picks their animal.
- **Economy:** reinforcements in a rolling window; re-founding soon after a
  retire founds half; nameplates shown; nameplate and species names
  filtered; species naming in the client.
- **Groups:** migration arrival within 8 tiles; lake targets moved ashore;
  territory and rally claims report food and distance, refuse a swimmer's
  lake, and are checked daily for water and grazing.
- **Warnings:** lineage alerts (starving, territory dry or bare, near cap);
  designer warnings (drought at any size, upkeep, useless wings,
  catastrophes).
- **Other:** possession ends a minute after disconnect; API players not
  counted offline; more leaks closed; phone layout fixes.

Playtest 4 confirmed most of these: predators hunted, matings completed,
the toll ran at its documented rate.

## After playtest 4 (round 5)

Playtest 4 found the brumating toxic flier still dominant, unfiltered lineage names, founding spots that still killed, player predators starving out, big groups drifting back, starters absorbed into the antelope, and sleepers dying of thirst. Round 5:

- **Cold-blooded winter rest:** no winter breeding for cold-blooded animals,
  awake or asleep; a brumator awake in winter pays the toll.
- **Toxic avoidance holds:** decided once a day per prey lineage (drab 70%,
  bright up to 100%), so a known toxic prey isn't eaten a few ticks later.
- **Attended eggs** chill slower while the parent is nearby.
- **Player-led species:** a cluster mostly of one player lineage keeps or
  founds its own species; the initial wild species can't be renamed.
- **Weighted share** counts offspring raised up to 4.
- **Player packs** carried by their band's kills while small (up to 60
  animals); sleepers burning fat draw water from it; a possessed animal can
  bed down lean; attack orders reach 12 tiles.
- **Groups:** a migration's target becomes a home circle (at most 12 tiles)
  with a forage bound, so big groups stay; every migration ends with a
  reason; territory food from what grows now, standing-days and prey
  warnings; far claims migrate first; overlapping claims refused.
- **Founding safety:** dry land only, room around the home, away from edges,
  20 days of food; reinforcements land beside the herd.
- **Economy:** one name filter for every name; half founders for ~30 real
  minutes after a retire or an extinction, shown in the designer; retired
  alleles go wild (no gifting share); meeting points on dry land, one
  agreement per animal, a wait while both sleep; dams at least 5 renown,
  holds at most 30 days, a 25% forfeit; provisions for small lineages.
- **Other:** the spectator map hides players' herds; survival credit only
  for lineages founded before a catastrophe; consistent day numbering;
  winter advice by thermoregulation; "Why are my animals dying?" help.

Playtest 5 confirmed most of these: big groups stayed home, player packs
lived, founders landed on dry land, sleepers kept their water.

## After playtest 5 (round 6)

Playtest 5 found cold, brumating, toxic, bright fliers dominant as a class, the market walled off by cold/warm sterility, warm abandoned eggs never hatching, sleeping hibernators drowning, name-filter gaps, free market holds and species credit flipping. Round 6:

- **Cold fliers reined in:** warm and cold blood about a third as fertile
  (not sterile); flight costs food and water whatever the body temperature;
  learned toxic avoidance capped at 85%; cold bodies slow, dry out and
  overheat in hot weather.
- **Species per owner:** a copy of a rival's design is its own species, small
  player lineages stay out of wild species, a named species keeps its namer.
- **Re-founding** in the window prices founders double.
- **Water:** sleepers bed down on dry ground off floodplains and wake when
  water reaches them; a flood alert; thirsty animals look farther for a shore
  they can reach.
- **Eggs:** brooding parents keep their nests against orders, weaned young
  follow, no late clutches; the designer warns about warm abandoned eggs.
- **Groups:** ordered migrants outrank grazing; group alerts kept over
  summaries; destination checks; outgrown and prey-short alerts; overlap
  refusals no longer name the owner and start the cooldown.
- **Warnings:** being hunted (naming the hunters), heat (its own death cause),
  hunger advice for sleeping herds.
- **Fairness:** eras ranked by the mean; survival needs animals on 80% of
  the season; grace explained at founding, and small foundings keep it
  through their first winter; offline protection after 10 idle minutes; the
  hall of fame counts every season.
- **Market:** a buyer whose animal dies may send a substitute (no refund
  otherwise, unless the loss wasn't theirs); 5-renown minimum for every
  listing; pending requests don't block; the deadline waits while either
  sleeps; provisions scale with the lineage.
- **Other:** names checked run together ("G.M.", "SupportTeam", "the
  wild"); oversized or malformed input gets a plain answer; in-page
  dialogs; the server remembers the last design.

Playtest 6 confirmed most of these: no sleepers drowned through four floods,
the market reopened, cold fliers lost animals to drought and predators.

## After playtest 6 (round 7)

Playtest 6 found the warm egg-laying hibernator dominant, retire and
re-found free again, long migrations starving big herds, droughts wiping out
foundings, name and market holes, and predators unable to grow. Round 7:

- **Re-founding** in the window founds half the founders whatever the
  budget, and after a retirement costs 100 renown, doubling.
- **Balance:** toxins cost food; hungry hunters heed toxin lessons less; a
  poisoning teaches only about the toxin's own lineage; a sitting parent
  covers about 3 eggs; species credit only for genuine speciation.
- **Predators:** the pack larder scales with prey per hunter; the prey check
  counts only edible, catchable prey; plant eaters take no attack orders; a
  hunter killed by prey "fought back".
- **Founding and newcomers:** drought foundings land by lasting water with
  watered founders; no winter clutches; small foundings keep their grace
  through their first spring.
- **Groups:** journey estimates for migrations; drinking spots instead of
  water tiles; calibrated food checks; public territory outlines; overlap
  checked at the clamped radius and cheap to refuse; outgrown territories
  movable at once.
- **Market:** steered deaths not excused; hold fees prorated by sleep;
  meeting point on the seller's side; hold fees pay the market fee; no fee
  below 10; the title needs 2 buyers; a survey to buy.
- **Warnings and fairness:** size-scaled warnings, thirst told as thirst,
  cold warning, hunters named; niche prizes by mean; an extinct-lineage
  banner; offline protection capped at 900 game days and dated from leaving.
- **Names and leaks:** dotted and CamelCase staff words, "Wild" names and
  wild species' names refused, strict account names; rivals' body plans and
  masses only roughly.
- **Client:** no native dialogs; inline name refusal; one Release; pinned
  view shown.

Playtest 7 confirmed most of these: no runaway, drought foundings survived,
re-founding cost something, the territory radar closed.

## What playtest 7 found, being worked on in round 8

- **Hungry hunters still never take warning-coloured toxic prey:** hunger
  and the 85% cap touch only the learned lesson, not the warning-colour
  benefit or each animal's lasting aversion, and every player herbivore was
  toxic and bright.
- **The same two classes still take the top** (warm small egg-laying toxic
  hibernators and the late cold toxic flier); a late joiner founding on an
  era's first day won it for the third playtest in a row; herbivore design
  space is narrow.
- **Predators can't compete** (a pure carnivore at 60–84 against herds of
  500–1,300).
- Medium items: slow die-offs unflagged (on a build before the size-scaled
  warnings); territory and destination food 2–5× too high and territories
  trapping herds; names with digits or run-together lowercase; market holes
  (refund after a forfeit, steered deaths outside a day, retiring to void a
  sale, the re-found fee capped at the balance); `preview_mating` leaking
  gene gaps; late joiners landing in the most crowded corner; designer
  redraw lags; egg losses in autumn and spring; split groups claiming blind;
  offline protection up to ~75 real minutes at playtest speed; survivor
  achievements after near-wipeouts; renown still piling up.

These are marked **changing** in `docs/game-design.md` until round 8 lands.
