# What changed since playtest 1

A one-page summary of three AI playtests (`docs/playtest-1.md`, `-2.md`,
`-3.md`) and the fixes between them. Each ran four to six AI players on one
server at playtest speed (a game day ≈ 5 s, a year ≈ 10 min) for about an
hour. Current rules: `docs/game-design.md`; details and measurements:
README (*Playtest 1: ecology fixes*, *Founders, the soft cap and the market*,
*Predators against players*, *Playtest 2: competitive balance*).

## At a glance

| | Playtest 1 | Playtest 2 | Playtest 3 |
| --- | --- | --- | --- |
| Players | 4 | 6 (with a late joiner and an exploit tester) | 6 (same roles) |
| Winner's build | Big swimming, hibernating grazer (a second lineage of 124 founders) | Egg-laying, toxic-but-drab, hibernating burrower founded late in winter | Cold-blooded, brumating, toxic 4 kg flier with extra eyes |
| Winner's head count | 1,500 (the hard cap) | ~3,000 (the 2× ceiling) | 2,100 (the 1.4× ceiling) |
| Winner's share | 18.7% | 26–40% | 22–31% |
| Player trades | 0 | some (rivals accepted paid offers) | 0 of 21 cross-player matings completed |
| Player predators | couldn't hunt players at all | starved or were poisoned | collapsed in minutes; never out of grace |
| Server | stayed up | stayed up | the container restarted mid-test; restored from its save with no game data lost |

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

## What playtest 3 found, being worked on in round 4

- **Grace restarts on re-founding** after ~40 game days, giving a fresh
  30 minutes; with most lineages graced, attack orders were nearly unusable.
- **Predators still can't play:** species collapse in 5–10 minutes and a kill
  doesn't save a starving hunter.
- **Thirst beside water:** hunger outranks thirst, and steep banks block poor
  climbers from drinking.
- **Fatal founding spots:** dry corners after a drought, deserts, an
  unreachable peninsula, the north snowfield in winter.
- **Five eyes buys reproductive isolation** for 3 points (and 360° vision).
- **Cross-player matings never complete:** nothing brings the pair together;
  buying a listed female is a free dam-lock.
- **Big groups "arrive"** while their median member is still 24+ tiles out.
- **New dominant build:** cold-blooded, brumating, toxic flier (brumation has
  no toll and costs half).
- **Territory claims** check water only once, with no food check or re-check in
  a drought (one claim killed 84% of a lineage).
- Medium items: possession outliving the session, retire as a free re-roll,
  reinforcement cap resetting at the season boundary, nameplates not shown
  and names unfiltered, remaining information leaks, lineage-level warnings,
  builder gaps, phone UI overlaps.

These are marked **changing** in `docs/game-design.md` until round 4 lands.
