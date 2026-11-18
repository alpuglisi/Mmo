# What changed since playtest 1

A one-page summary of four AI playtests (`docs/playtest-1.md` to `-4.md`)
and the fixes between them. Each ran four to six AI players on one
server at playtest speed (a game day ≈ 5 s, a year ≈ 10 min) for about an
hour. Current rules: `docs/game-design.md`; details and measurements:
README (*Playtest 1: ecology fixes*, *Founders, the soft cap and the market*,
*Predators against players*, *Playtest 2: competitive balance*, *Playtest 2:
predators, toxins and drowning*, *Playtest 3: balance*, *Playtest 3: founding
spots, steep banks and winter eggs*, and the economy's *Playtest 3 changes*).

## At a glance

| | Playtest 1 | Playtest 2 | Playtest 3 | Playtest 4 |
| --- | --- | --- | --- | --- |
| Players | 4 | 6 (with a late joiner and an exploit tester) | 6 (same roles) | 6 (same roles) |
| Winner's build | Big swimming, hibernating grazer (a second lineage of 124 founders) | Egg-laying, toxic-but-drab, hibernating burrower founded late in winter | Cold-blooded, brumating, toxic 4 kg flier with extra eyes | The same kind of flier, live-bearing and honestly coloured, founded by the late joiner on day 320 |
| Winner's head count | 1,500 (the hard cap) | ~3,000 (the 2× ceiling) | 2,100 (the 1.4× ceiling) | 2,076 (the hard cap); runner-up 2,006 |
| Winner's share | 18.7% | 26–40% | 22–31% | 25.9%; runner-up 21–26% |
| Player trades | 0 | some (rivals accepted paid offers) | 0 of 21 cross-player matings completed | matings completed (6 for the broker) |
| Player predators | couldn't hunt players at all | starved or were poisoned | collapsed in minutes; never out of grace | hunted (46 attack-order kills) but 4 of 5 lineages starved |
| Server | stayed up | stayed up | the container restarted mid-test; restored from its save with no game data lost | stayed up |

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

## What playtest 4 found, being worked on in round 5

- **The brumating toxic flier still dominates**, and a late joiner won with
  it: brumators wake to bask in mild winter weather and keep breeding, and
  cold blood keeps it sterile with every warm lineage.
- **Lineage names are unfiltered** ("Server Admin" with a direction
  override showed publicly), and a founding species takes that name.
- **Founding placement still kills:** deep water, lake ice that melts, a
  corner ringed by water.
- **Player predators still starve out,** and toxic prey (chosen by most
  players) makes the role nearly unplayable.
- **Big groups drift back** after migrating; arrival still reported far out
  for 500+ groups; one migration ended silently.
- **Starter lineages still absorbed** into the wild Steppe antelope.
- **Dormant animals died of thirst** in one lineage (to recheck).
- Medium items: optimistic territory food estimate and long claim cooldown,
  half-founders window too short and not shown, rendezvous walking across
  lakes or stalling in winter, cheap dam-lock, gaps in the title filter,
  incubated eggs chilling more than hatch, survival credit for players with
  no animals, phone overlaps, possession blocking hibernation, unprotected
  reinforcements, winter advice for cold designs, a map-wide spectate view,
  retire transfers gifting share, territory claims anywhere.

These are marked **changing** in `docs/game-design.md` until round 5 lands.
