# Evolution MMO — Game Design

Nov 12, 2026 · @Al · updated Nov 18, 2026 to match the game after playtests 1–4

> **Status.** This document describes the game as built after the fixes from
> playtests 1–3, as confirmed in playtest 4 (`docs/playtest-4.md`).
> Rules marked **changing** are being reworked in round 5 after playtest 4;
> their current behaviour is described, but don't build on it. A one-page
> history is in `docs/changelog-playtests.md`.

## Concept

A persistent multiplayer world where each player designs a founding organism and then steers its lineage through breeding, not direct editing. The simulation runs on the server whether or not the player is online.

- **Inspiration:** Niche: A Genetics Survival Game, scaled up from one tribe on a turn-based map to a shared, always-running world.
- **Core idea:** Behavior, body and mate preference are all genetic. Players change their lineage only through directed matings, possessing individual organisms, and group commands.
- **Shared world:** Biomes, changing weather and seasons; every lineage eats from the same food web.
- **Social structures vary:** Lineages may live as cohesive tribes, in pairs, or solitary and dispersed across the map. A player manages a lineage, not necessarily a tribe.

## Scoring: genetic legacy

A player's score is their share of alleles, in the living breeding-age population, that carry their origin tag. Genes count wherever they live, including in rivals' organisms.

1. **Tag alleles at creation.** Founder genes carry the designing player's ID. A new mutation carries the ID of whoever owned the organism it appeared in.
2. **Tags never change.** They pass to offspring through any mating, whoever owns the offspring.
3. **Snapshot regularly** (for example once per simulated day). Count only living organisms old enough to breed.
4. **Weight by success (optional).** Multiply each organism's alleles by a fitness factor, such as offspring raised to adulthood, so junk genes earn nothing.

```latex
\text{score}_p = \frac{\text{alleles tagged } p \text{ in breeding adults}}{\text{all alleles in breeding adults}}
```

Example: 10,000 breeding adults × 50 genes × 2 alleles = 1,000,000 alleles. 80,000 tagged to you = 8%.

**Supporting rules**

- **Seasonal leaderboards:** Snapshot and rank at the end of each era, possibly right after a catastrophe.
- **Secondary achievements (prestige only):** Species founded, lineage longevity, niche dominance, surviving catastrophes.
- **Anti-exploit:** Offspring cost food and parent time, and only breeding adults count, so flooding the world with genes doesn't pay.
- **Genes, not heads.** A lineage that interbreeds with a wild species or another player's lineage carries their alleles, so its share falls below its head count; alleles a player's studs spread into other lineages count for that player. This is intended, and the genetics dashboard shows the arithmetic: the lineage's head share, the part of the share at home and abroad, and whose alleles the lineage carries (README, *Whose genes score*).
- **Season titles** count each day the mean of raw and weighted share, so a fresh founding (founders that have raised nothing yet) counts half until its young grow up, and founding big late in a season doesn't take the title.

**Why trade** (playtest 1 had zero trades: accepting a rival's stud only dilutes you). Renown, earned only in play, now pays and buys things:

- **Breeder's royalty:** renown a day for a player's alleles living outside their own lineage, so siring into rivals' dams pays the breeder beyond the share it already counts for.
- **Renown shop (sinks):** extra directed-mating slots for the season (30 renown, doubling, at most 6), and reinforcements for a small lineage (10 young adults copied from its own, landing on the best-fed ground near it; 150 renown, doubling; only while at most a fifth of the soft cap is alive; at most two within a ranked season's length per player, a rolling window that a season boundary doesn't reset, retired lineages included): a large lineage spreads its genes by paying others' dams, a small one earns renown by hosting them and spends it on catching up. Leaders get cosmetic sinks only: a nameplate (shown with the player's name) and naming a species their lineage founded or now mostly makes up. Nameplate titles and species names are filtered: plain letters, digits and a little punctuation, no direction overrides, zero-width characters, markup or look-alike letters, and nothing passing for the server or another player. **Changing:** lineage names aren't filtered (and a founding species takes its lineage's name, so it skips the species-name filter); the title filter still accepts staff-like words ("Game Master", "Mod", "Support"); reinforcements land unprotected, and clones of founders count as founders (a free first winter).
- **Offers live in real time:** a request waits 10 minutes for an answer and an accepted agreement 30 minutes for the meeting (then the requester forfeits a tenth), not until the next weather season; offers survive season changes and server restarts, and the sender is warned 2 minutes before a request expires. An offer can go to a whole lineage, whose owner picks which animal takes it (or the nearest free one). A bought listing that lapses or is cancelled forfeits 10% of the price. **Changing:** that still makes holding a rival's dam cheap (1 renown on a cheap listing, nothing on a free one, and 30 real minutes is hundreds of game days at playtest speed).
- **Agreed pairs walk to meet:** once accepted, both animals head for each other on their own (breaking off for danger, thirst or hunger); both players see roughly where the partner is and may possess theirs to hurry. Accepting with your own pregnant female is refused with the reason; a pregnant female can still be listed but shows as busy; a female promised in an open agreement can't get a fixed listing and is held from natural mating. **Changing:** the walk heads straight for the partner, across lakes; a possessed animal doesn't walk, so a buyer can make a rival's animal come to it; pairs asleep in winter stall while the UI says "walking"; one male can hold several agreements with one target.
- **Retiring** a lineage removes its animals, cohorts and eggs from the world at once (not killed, not turned wild), ends its orders, and lets the player found again; renown, ledger and records stay. An animal at least a quarter of whose alleles are one other living player's joins that player's lineage instead. Re-founding after a retirement starts on a founding ramp (a fresh founding's share counts in gradually), and re-founding within a ranked season's length of retiring founds half the founders (after an extinction, the full design). **Changing:** the half-founders window is one season (5 real minutes at playtest speed), a lineage that dies out re-founds at full size, and the designer shows and charges the full count; the transfer moves animals that are mostly the retiree's and re-tags its alleles to the receiver, so colluding players can gift share.
- **Titles need substance:** the stud market needs 2 sales to 2 different buyers in the season; a survival title nobody failed goes to no one. **Changing:** players with no animals still get survival credit, and the market title is out of reach in a short season.
- **Market fee:** 10% of every payout is destroyed.
- **Compatibility shown:** body plans 0.30 apart (diet, warmth, birth mode, size) are sterile; the market lists which lineages can breed with yours.

## Player controls

Players never set behavior directly. They act through three levers; everything else comes from genes.

| Lever | What the player does | Notes |
| --- | --- | --- |
| Possession | Take direct control of one organism: move, forage, hunt, fight, scout, court | Returns to genetic behavior when released. A small bonus (perception, reaction) rewards active play without dominating |
| Directed matings | Pair two organisms, including with another player's organism | Limited slots per season. Other player must consent and may set a price. The organisms must physically meet, so the trip carries risk. Stud marketplace for trading genetics |
| Group commands | Order migration, claim or abandon territory, split a group | How strongly a group follows them is genetic: the obedience gene weighs orders against instinct, and obedient animals are tamer (slower to flee) |

**Attack orders** (player against player). While possessing an animal, a player can order it to attack another player's animal in sight (at most 8 tiles off). The order follows the usual strike rules and is refused on a target too heavy for the attacker and the pack-mates around it, on one in its burrow, and while either lineage is under its founding grace; the victim's owner gets an alert (at most one per attacking lineage every 5 minutes). A possessed pack leader draws its hungry pack-mates into the hunt, and a kill on an order feeds the hunter (a starving hunter's hunger falls below 30 within two days). Prey down in its burrow is shown as burrowed and can't be targeted. Territory holders mob larger intruders when they outnumber them. Wild predators hunt everyone as before.

**Toxic prey is learned.** Each predator lineage learns which prey lineages poisoned it; bright warning colours teach about three times faster than a drab coat, a hunter passes up prey only as far as it recognises it (so drab toxic prey is still mostly taken), and a taught predator takes less harm when it bites anyway. Players see "This animal looks poisonous to you", and an attack order on such prey warns before it goes through.

**Changing** (playtest 4): player predators still starve out (4 of 5 lineages; one kill buys a day or two), and toxic prey, which most players chose, makes the role nearly unplayable; some attack orders on targets a few tiles away were refused as out of sight.

**Founding grace.** A lineage founded into a running world can't be attacked by other players (orders or their animals' own hunting) for its first 20 game days, and at least 30 minutes of real time, whichever is longer, but never more than 60 game days. The grace is two-sided (its own animals don't hunt players and it can't order attacks), shows in the client with time left, ends with an alert, and graced lineages are hidden from the spectator map. Wild predators still hunt it. Re-founding doesn't start a new grace: a founding within twice the grace's whole length of the previous one continues the old grace. A player can end their grace early ("End my grace now", for a predator's owner who wants prey); it doesn't come back by re-founding.

**Possession** ends a minute after the player's connection drops, so an animal isn't left standing until it starves. **Changing:** a possessed animal stays awake instead of hibernating and starves; possessing a second animal silently releases the first.

**Lineage warnings.** Besides catastrophe and drought alerts, the server warns a player when their lineage is starving or parched in numbers, when a territory has no drinking water left or is grazed bare (with the day's deaths), and when the lineage nears the hard cap.

**Group commands in detail.**

- **Migration:** a target off the map is refused; one on deep water or a cliff moves to the nearest standable tile within 8 tiles, else it is refused. Members avoid ice they'd drown under at the melt, and break off to drink or eat when a need passes 60. A target in a lake moves ashore. The order ends when the group's *median* member is within the arrival radius (at most 8 tiles), then the group settles there for 15 days; a migration that hasn't gathered by its deadline (3 days plus a day per 3 tiles, 4–30 days) ends with an alert. Ordering a group whose members are mostly dormant warns that they won't follow. The route is drawn on the map. **Changing:** big groups drift back after arriving (the settle circle is up to 24 tiles across and can contain where they came from), groups of 500+ are still reported arrived with their median 16–20 tiles out, and one long migration ended with neither an arrival nor a timeout alert.
- **Territory and rally:** a claim or rally over a circle more than half water the group can't stand in is refused (`mostly_water`; a swimmer's lake doesn't count as land). The reply reports the circle's water share, drinkable tiles, about how many of the group its plants feed, and its distance from the group, with warnings for little water, a centre in water, too little food for the group, or a claim more than 30 tiles away. Every day the server checks standing territories and alerts once when one has no drinking water left or is grazed bare. Thirsty members leave the circle to drink at thirst 60. Rallies hold for 3 days. **Changing:** the food estimate is far too optimistic (it uses peak regrowth: herds rated at several times their size starved inside), the claim cooldown is longer than a big herd takes to strip a territory, claims are accepted anywhere (over a rival's herd, far from the group), and a claim or rally silently replaces the group's current order.
- **Retire:** see *Why trade* above.

**Directed-mating override cost.** When a directed mating falls outside an organism's genetic mate preferences, it costs more the further outside it is. See Genetics.

**A typical session:** check the genetics dashboard and alerts, arrange a trade, possess an organism and escort it across the map to the meeting, log off while the lineage carries on.

## Genetics

Behavior and mate preference are inherited and mutate like physical traits. To change how a lineage acts, a player has to breed for it.

**Behavior genes** set weights such as aggression, boldness, foraging range, sociability, roaming and mate choosiness. Each organism's AI makes decisions from its own weights, so even siblings act differently.

**Mate-preference genes** set, for each trait an organism cares about, a preferred value and a tolerance. Organisms choose mates on their own using these genes, and preference genes and trait genes can reinforce each other over generations.

**Override cost for directed matings**

1. For each trait, mismatch = |candidate's value − preferred value| ÷ tolerance.
2. Combine traits into one mismatch score (sum, or root-sum-of-squares so one big miss counts heavily). Score ≤ 1 means the organism would accept anyway.
3. Cost rises steeply above 1:

```latex
\text{cost} = k \cdot \max(0,\ \text{mismatch} - 1)^2
```

- **Possible costs:** food or extra mating slots; a failure chance that grows with mismatch; fewer or weaker offspring; a longer, exposed courtship.
- **Cross-player matings:** check both organisms and use the larger mismatch.

**Balancing picky vs. flexible lineages.** Flexible preferences make a lineage cheap to steer, so mate choosiness needs real benefits:

- **Honest signals (core):** Visible traits correlate with hidden quality, so picky organisms get fitter offspring.
- **Outbreeding depression (core):** Large genetic distance between mates risks lower fertility, weaker bodies or sterility. This also drives speciation.
- **Mating-transmitted disease:** Lineages that mate widely pick up and carry pathogens.
- **Hidden recessives:** Harmful recessives enter flexible lineages unseen, possibly planted by rivals.
- **Dilution:** Outside matings replace half of each offspring's genes.
- **Critical-mass group traits:** Loose breeding can dilute a group trait below its threshold. See next section.

## Critical-mass group traits

Some traits pay off only when enough of the local population carries them. Certain combinations of designed traits unlock them.

| Prerequisite traits | Group trait | Payoff at critical mass | Who benefits |
| --- | --- | --- | --- |
| Carnivore + pack hunting | Cooperative hunting | Can take large prey (needs \~4+ carriers nearby) | Carriers |
| Toxins + bright coloration | Warning coloration | Predators avoid the population | Everyone in the area, including look-alikes |
| Claws or burrowing + sociability | Shared burrows | Shelter from weather and predators | Carriers |
| Keen hearing (ear size) + voice | Alarm calls | Group-wide early warning | Carriers |
| Herbivore + large size + sociability | Herd defense | Predators can't single out individuals | Group members |
| Migration + sociability | Group navigation | Safer, more accurate migration | Group members |
| Hibernation + sociability | Communal dens | Shared body heat cuts fat loss | Den members |
| Food caching + sociability | Shared caches | Larger stockpile than individual hoards | Group members (free-rider risk) |
| Group species + sentry gene | Sentries | Some stay alert while others breed | Group members |

**Implementation.** Measure each trait's frequency among breeding adults per region or social group, and apply the benefit on an S-shaped curve:

```latex
\text{benefit} = \frac{1}{1 + e^{-k(\text{freq} - \text{threshold})}}
```

**Strategic effects**

- Picky lineages protect their group traits; one generation of loose breeding can drop a trait below threshold.
- Rivals can sabotage by flooding an area with non-carriers.
- Area-wide benefits invite free riders, creating rise-and-collapse cycles.
- A new group trait must be built up to threshold through inbreeding or coordinated directed matings.
- Tribes are where group traits work, which balances the dispersed playstyle.

## Organism design

At game start the player designs a founder genome on an equal point budget. Every trait stays available; after launch, the design changes only through breeding, mutation and selection.

**Design order**

1. **Warm-blooded or cold-blooded**
2. **How offspring are born:** live birth or incubated eggs (possible third option: eggs laid and left)
3. **Average offspring per birth** (slider)
4. **Remaining traits** from the categories below

**1. Thermoregulation**

|  | Warm-blooded | Cold-blooded |
| --- | --- | --- |
| Food needs | High, constant | Low |
| Activity | Any weather, day or night | Sluggish in cold; needs warmth |
| Biomes | Anywhere | Thrives warm, struggles cold |
| Growth | Fast | Slow, can live longer |

Thermoregulation does not lock or unlock traits. Each trait has an affinity (warm-leaning, cold-leaning, neutral); taking a mismatched trait imposes a substantial, trait-specific penalty, and multiple mismatches stack.

| Trait | Affinity | Mismatch penalty |
| --- | --- | --- |
| Hibernation | Warm | Cold-blooded can't heat itself to wake; many never wake in spring |
| Brumation | Cold | Warm-blooded burns through fat reserves while dormant |
| Thick fur | Warm | Cold-blooded can't bask; stays sluggish |
| Basking | Cold | Warm-blooded overheats; basking time wasted |
| Sustained endurance | Warm | Cold-blooded exhausts fast, long recovery |
| Abandoned eggs | Cold | Warm-blooded eggs chill without a parent |
| Venom | Neutral | None |

Optional: mismatch penalties are partly genetic, so selection can slowly reduce them over many generations.

**2. Birth type**

|  | Live birth | Incubated eggs |
| --- | --- | --- |
| Parent cost | Pregnant parent slower, eats more | Quick to lay; eggs must be guarded and kept warm |
| Vulnerability | Young protected inside parent | Eggs can be raided, chilled or overheated (lost eggs are reported by cause; **changing:** in playtest 4 more incubated eggs chilled than hatched, year-round) |
| Mobility | Parent keeps moving | Tied to nest until hatching |
| Fits with | Migration, warm-blooded | Cold-blooded, colonies |

Birth mode is part of the body plan, but counts for less than the other body-plan traits when deciding fertility: an egg layer is a little fertile with a close live bearer (and vice versa) instead of sterile with every one of them, so choosing eggs is not a free wall against hybrid dilution (playtest 2). Kinship and species boundaries still use the full difference.

**Body size** is free only in a middle band (from the default up to 3.6 kg). Smaller bodies cost points (more breeders on the same food), and so do bigger ones: a big body outgrows most predators, which its food need alone did not offset (playtest 2: player herbivores of 29–54 kg that no predator could take).

**The starter design** (the builder's default) lays 3 incubated eggs and is its own species, only somewhat fertile with the wild Steppe antelope. **Changing:** in playtest 4 every starter lineage was still filed under the Steppe antelope by about day 200 (no founding credit, no naming), and the builder's fertility estimate (6%) disagreed with the market's (15%).

**Where founders land.** In a running world the founding search judges each spot as it is now: water the design can walk to that lasts a drought, no ice over deep water, enough standing plants for the founders placed there, no snow or summer heat the design can't bear, map edges marked down in the cold season, and no spot near where the owner's earlier founding died out. A founding in winter is provisioned for it, egg layers wait for spring to breed, and founders sleep their first winter free of the dormancy toll. **Changing:** placement still killed foundings in playtest 4 (founders standing in deep water or on lake ice that melted days later, a corner ringed by water, a dry mountain corner).

**Eyes and the body plan.** Each step in eye count adds 0.1 to the body-plan gap for kinship but only 0.04 for fertility (playtest 3: five eyes had bought full reproductive isolation, and 360° vision, for 3 points; an extra eye now costs 3 points).

**Interbreeding check.** The builder asks the server which living lineages a design's founders could breed with: founders sampled with their individual variation against sampled adults of each lineage, giving a fertility range (not the best pair), the share of fertile pairs and a drift risk (how close a sterile pairing is to becoming fertile as lineages drift). Player lineages are never labelled wild.

**3. Offspring per birth.** Few (1–2): larger, high survival, long care, slow growth. Many (8+): fragile, most die young, fast recovery and more variation. Food cost, recovery time and care per young scale with the number. Only offspring that reach breeding age add to the legacy score.

**4. Physical traits**

| Body area | Traits |
| --- | --- |
| Body | Body size, muscle mass, body fat |
| Limbs | Leg length, claws, hooves, foot width (up to webbed feet), tail length |
| Locomotion | Swimming, wings (flight), gliding flaps, climbing, burrowing |
| Covering | Fur, scales, armor |
| Head | Fang size, eye size, ear size, sense of smell |
| Other | Diet (carnivore, herbivore, omnivore, scavenger), camouflage, toxicity, colour, voice |
| Instinct genes | Boldness, aggression, roaming, sociability, foraging range, pack hunting, obedience, mate preference and mate choosiness |

**Stats layer.** Physical traits set stats; stats feed needs and decide how well behaviors succeed. Most traits carry a built-in cost. Costs are contextual where the body makes them so: fur on a cold-blooded body blocks basking, hooves slip on ice unless claws grip, scales take skin a furred body would cover with fur, and long fangs crop plants badly, costing a grazer much and a hunter little. A trace of fangs (below 0.1) is no fangs at all.

| Stat | Raised by | Lowered by | Affects |
| --- | --- | --- | --- |
| Speed | Long legs, hooves, muscle | Body size, fat, armor, cold-blooded (in cold), low energy | Fleeing, hunting, migration; sprinting drains energy |
| Strength | Body size, muscle, claws, fangs | Low energy | Fighting, taking large prey |
| Endurance | Long legs, hooves, fat reserves | Body size | Energy drain from activity |
| Agility | Claws, tail | Hooves, armor | Fleeing, catching nimble prey |
| Perception | Eye size, ear size, sense of smell | Breeding drive | Spotting threats, food, mates |
| Stealth | Camouflage | Body size | Avoiding predators, ambushing prey |
| Defense | Body size, scales, armor, toxins | — | Surviving attacks |
| Metabolism | Body size, muscle, warm-blooded | Cold-blooded | Hunger and thirst rates |
| Insulation | Fur, body fat | Ear size (big ears shed heat), scales over fur | Temperature |

**Founders.** Starting organisms vary around the design rather than being clones, so selection has material from day one. A larger founding population costs points, so group-trait builds pay to start above threshold.

## Seasons and biomes

The map has distinct biomes, changing weather and seasons, so each organism picks one primary seasonal strategy, plus optional add-ons.

| Strategy | How it works | Benefit | Cost or risk |
| --- | --- | --- | --- |
| Hibernation | Dormant through winter on stored fat | Almost no winter food needed; safe from weather | Must fatten in fall; den can be found; no winter breeding; about 1 in 10 sleepers die in the den over a winter (more of the young and the lean; brumators too; founders sleep their first winter free); after waking, days of recovery before breeding; wakers spread over the first days of spring; dearer to design |
| Migration | Travels between seasonal ranges | Always in good conditions | Dangerous journeys; needs navigation and stamina |
| Year-round active | No seasonal change | Breeds and forages all year; holds territory | Faces winter head-on; needs cold traits or a mild biome |
| Estivation | Dormant through summer heat or drought | Survives deserts and dry seasons | Loses the summer |

**Brumation** (the cold-blooded counterpart of hibernation) has the 6-day waking lag and, since playtest 3, the same winter toll (about 1 in 10 a winter), and costs 10 points a unit against hibernation's 12 (it was 6). Playtest 3's winner was a cold-blooded, brumating, toxic flier that stayed flat through winters; in the scripted replay it is weaker under the new rules but still the strongest design (README, *Playtest 3: balance*). **Changing:** it won playtest 4 too, from a late start (62 founders on day 320 to the hard cap in 23 real minutes): brumators rouse to bask in mild winter weather and keep breeding, the toll applies only while dormant, and cold blood makes it sterile with every warm lineage. Dormant animals also died of thirst in numbers in one lineage.

**Optional add-ons:** food caching (raidable), seasonal coat (energy cost per molt), fat storage (slower), timed breeding (spring births, narrow window), short-term torpor (cheaper, weaker than hibernation).

**Effects on gene flow**

- **Migrators** pass through many ranges and meet many mates: natural gene spreaders, with higher disease and outbreeding risk.
- **Hibernators** are isolated part of each year: suited to picky, group-trait builds.
- **Year-round residents** hold the map while others are away and meet whoever returns.

## Movement and terrain

Traits produce physical properties; each tile has conditions; comparing the two makes a tile impassable, costly or advantaged for that organism.

**Derived physical properties**

| Property | Comes from | Matters for |
| --- | --- | --- |
| Height | Body size, leg length | Wading depth, deep snow, reaching high food, seeing over tall grass |
| Weight | Size, muscle, fat, armor | Sinking in mud and snow, breaking thin ice, flight |
| Ground pressure | Weight ÷ foot area (wide paws and webbed feet spread weight) | Mud, snow, sand |
| Buoyancy | Fat (floats); armor and dense muscle (sink) | Swimming, drowning risk |
| Wing loading | Weight ÷ wing area | Flight or gliding; wind tolerance |
| Grip | Claws, hooves, pads | Ice, rock, slopes, climbing |
| Body width | Size, armor | Dense forest, burrows |

**Tile conditions:** water depth, snow depth, ice thickness, mud or sand, slope, vegetation density, canopy height, wind, temperature.

**Outcomes**

1. **Impassable:** physically impossible (deep water without swimming, cliffs without climbing or flight). Pathfinding avoids it; if forced (fleeing, possession), risk of injury or death.
2. **Costly:** reduced speed and extra energy drain.
3. **Advantaged:** faster or safer than normal, such as a swimmer in a river or a climber in canopy out of reach of ground predators.

**Example rules**

| Condition | Helps | Hinders or blocks |
| --- | --- | --- |
| Deep water | Swimmers, buoyant bodies | Non-swimmers blocked; armor sinks |
| Shallow water | Tall organisms (wade) | Short organisms must swim |
| Deep snow | Tall bodies, wide feet, low ground pressure | Short legs, heavy narrow hooves |
| Ice | Claws, light bodies | Hooves slip; heavy bodies break thin ice |
| Mud or marsh | Wide or webbed feet, light bodies | Heavy, narrow-footed organisms bog down |
| Dense forest | Small, narrow, climbing bodies | Large bodies slowed; flyers can't maneuver |
| Open plains | Long legs, hooves, flyers | Small bodies exposed (stealth drops) |
| Cliffs, steep slopes | Climbers, flyers, strong grip | Others blocked or slowed |
| Desert sand | Wide feet, desert adaptation | Heavy bodies sink; thirst rises |
| High wind, storms | Heavy, low-profile bodies | Flyers grounded; gliders can't launch |

**Seasons and weather reshape the map**

- **Winter:** frozen lakes become bridges, opening migration routes and predator access; snow favors tall, wide-footed lineages.
- **Spring melt:** swollen rivers block weak swimmers and split populations, which can drive speciation.
- **Storms:** ground flyers and make open terrain dangerous.
- **Drought:** water and marsh shrink, crowding organisms at remaining sources.

**Design effects:** terrain isolates populations, so gene flow follows geography; seasonal bridges give directed matings and invasions a calendar; biome specialists dominate at home but struggle to leave, which works against dominant designs.

Locomotion traits carry built-in limits: wings cap weight and size through wing loading; gliding is cheaper than flight but needs height to launch.

## Biological needs and behavior

Needs turn genes into behavior: traits set how fast needs rise, behavior genes set how an organism responds, and the environment decides whether needs can be met.

| Need (0–100) | Rises from | Traits that change it | If unmet |
| --- | --- | --- | --- |
| Hunger | Time, activity | Size, warm-blooded, offspring per birth, pregnancy | Energy drops, then starvation |
| Thirst | Time, heat | Desert adaptations, size | Faster decline than hunger |
| Energy | Movement, fighting, cold | Endurance; cold-blooded in cold | Slower, weaker, can't flee |
| Temperature | Weather, biome | Fur, basking, thermoregulation | Lethargy, energy drain, death in extremes |
| Safety | Nearby threats | Boldness, group size, camouflage | Stress, flight |
| Breeding drive | Season, maturity | Timed breeding, offspring count | Seeks mates |
| Social (group species) | Time alone | Sociability | Stress, wandering |

**Food.** Foods differ in value. Leaves and grass are plentiful but poor; meat is rich (worth 1.8 units of leaves per unit); ripe fruit, seeds and nuts are rich (1.5) but seasonal, ripening from early summer to a peak in early autumn and lasting, as fallen nuts and seeds, into the winter, most in moist forest; insects, worms and grubs (small meat, worth what meat is but slow to gather) come out whenever the ground is above freezing, most in marsh and wet ground. How much of each food an animal gets depends on its diet gene: herbivores digest leaves, carnivores meat, omnivores and flexible hunters (a fox) do well on fruit and invertebrates, which a full carnivore's gut handles poorly and a herbivore's not at all. An animal eats what is worth most to it, so a fox lives on voles in spring, on berries in autumn, and on carcasses in winter.

**Predators.** Carcasses keep for days and natural deaths leave carrion, so one kill feeds a family and scavengers. Meat eaters gorge into fat, follow their nose to carcasses and bring food to a pregnant mate or young. A carnivore female breeds only when no other litter of her lineage is within her territory (wide for a lone, aggressive hunter, narrow for a tolerant pack), hunters overlook prey that has grown rare, and a starving mother carries fewer young. How strongly each applies comes from the animal's genes (diet, temper, group tolerance, pack instinct, fat stores, pickiness, litter size), so selection tunes them.

**Action selection.** Each tick, an organism scores possible actions by the urgency of the related need, weighted by its behavior genes; the highest score wins. Hungry organisms prioritize food over everything except immediate danger. Thirst is the exception: a parched animal (past thirst 85) with water within 6 tiles drinks before it eats, and a thirsty animal that can't drink where it stands heads for the nearest shore it can actually walk down to (steep banks stop poor climbers). A sleeper beds down for the winter only once it has drunk. A bold organism keeps foraging near a predator; a timid one flees and goes hungry.

**Cascades.** Unmet hunger drains energy, low energy cuts speed and endurance, and slow organisms get caught and fail hunts, which worsens hunger. Bad seasons can spiral, which is where selection bites.

**Breeding season.** High breeding drive lowers perception and threat response and raises risk-taking, making the season dangerous. Showy displays attract mates and predators alike; sentry genes and short timed-breeding windows offset the risk.

**Ageing.** Old age kills over a spread of ages (about 85% of the lifespan on average, give or take an eighth), so a cohort born in one boom does not die out in the same week.

**Other modifiers.** Pregnant and nursing parents need more food; egg-carrying parents are slower. Injury and disease may be added as needs that reduce other stats until healed.

## Technology

The server is written entirely in Rust; web and mobile share one TypeScript client. The server is authoritative, and clients only display state and send commands.

| Layer | Choice | Role |
| --- | --- | --- |
| Simulation | Rust, ECS (Bevy ECS, headless) | Ticks every organism: genome, needs, decisions, terrain checks |
| Networking and API | Rust: Tokio, Axum | WebSocket connections, logins, trades, marketplace |
| Database | PostgreSQL via sqlx | Accounts, lineage history, world snapshots |
| Client | TypeScript, PixiJS or Phaser | 2D map rendering and UI in the browser |
| Mobile apps | Capacitor | Wraps the same client as iOS and Android apps |
| Client–server link | WebSockets | Live world updates and player commands |

**Build order**

1. Prototype simulation rules (genetics, needs, terrain) with no players. *(Done: `crates/sim`; balance still being tuned.)*
2. Build the Rust server around the proven rules. *(Well along: `crates/server`, see the README.)*
3. Build the TypeScript client against the server's API. *(Well along: `client/`, PixiJS + Vite, with a phone layout and Capacitor wrappers; see the README. Three AI playtests so far: `docs/playtest-1.md` to `-3.md`.)*

The simulation and account services could later split into separate Rust services if scale requires it.

## Scale and performance

Server load scales with organism count: players × organisms per lineage, plus a wild ecosystem sized to the map. The per-lineage cap is the main lever on how many players a world can hold.

**Estimates (to verify by benchmark).** A careful Rust ECS tick per organism (needs, utility-AI decision, neighbor check, movement) is roughly 2–5 µs. At 1 tick per second that is about 200k–500k organisms per core before overhead; plan on about a third of that after terrain, weather, breeding and networking. A 16-core server could plausibly run a few hundred thousand active organisms.

| Stage | Concurrent players | Map (tiles) | Player organisms | Wild ecosystem |
| --- | --- | --- | --- | --- |
| Prototype | 10–50 | 256 × 256 | \~5k | \~20k |
| Launch | 100–200 | 512 × 512 | 20k–40k (\~200 breeding adults per lineage) | \~100k |
| Stretch | 500 | 1024 × 1024 | \~100k | \~300k |

**Strategies to control load**

- **Slow world clock:** about 1 tick per second or slower; possessed organisms tick faster. (Implemented as movement sub-steps: a possessed organism following a move order moves in quarter-tick steps between world ticks and new orders apply at the next sub-step; decisions and needs stay on the world clock. See README, Server.)
- **Level of detail:** full agents near active players; distant populations run as statistical models (birth, death, gene frequencies) and switch back to agents when a player arrives.
- **Slow environment updates:** weather, vegetation regrowth and snow depth update per simulated hour, chunk by chunk.
- **Population limits:** food-based carrying capacity, plus a soft cap per player lineage (1,500 on the reference map, scaled by area): as a lineage's adults (juveniles count a quarter) approach the cap its litters shrink (half at the cap) and past it its members get hungrier (steeply: a sixth of the litters at 1.3×), up to a hard safety ceiling at 1.4 times the cap (2,100). Playtest 3's winner sat at that ceiling, and in playtest 4 two lineages did (**changing:** their shares swung 7–13 points within about 60 days). Food, not the cap, is the real limiter, and a lineage at the cap grows its share by spreading its genes into others (README, *Soft lineage cap*). Wild species keep hard caps.
- **Parallel regions:** map split into chunks updated across all cores.
- **More worlds, not bigger ones:** scale by adding independent worlds (shards), each with its own ecosystem, seasons and leaderboard.

**Next step:** build to the prototype target, measure real per-organism tick cost, then set launch numbers.

## Open questions

- [ ] Dominant designs: how to keep any single build from taking over. *(Each playtest found one: big founding swarms (1), egg-laying toxic-but-drab hibernating burrowers (2, answered by the dormancy toll, egg hybridisation, body-size cost and a lower ceiling), a cold-blooded brumating toxic flier with extra eyes (3, answered by eye-gap relief, the toll on brumators and dearer brumation), and the same flier again in playtest 4, won by a late joiner (being answered in round 5).)*
- [ ] Duplicate mutations: separate tags per origin, or credit only whoever had it first? *(Currently separate tags per origin: with continuous alleles, "first" needs a rule for when two alleles count as the same. See README, Suggested next steps.)*
- [ ] Seasonal strategy: fixed at design, or an evolvable gene?
- [x] Group commands (migration, splitting): keep as player levers, or make genetic? *(Both: the commands stay player levers, and an obedience gene sets how strongly a group heeds them against its own instincts. Obedience is paid for in tameness: an obedient animal flees a threat later. See README, Groups and territory.)*
- [ ] Point costs for each trait and for founding population size *(Founders: 0.7 points each up to 50, then each 0.07 dearer than the last, so a whole budget buys 88 founders instead of 160; playtest 1's 130–141-founder swarms took an instant 5–13% share. See README, Point costs.)*
- [ ] Tick rate, and whether the world runs real-time or slow and async
- [x] Offline protection: home ranges or surviving founder groups *(Built as surviving founder groups: for up to 12 hours after logout, a lineage down to its last 12 members suffers a quarter of the kills and half the starvation and thirst; organisms in a trade or on the market are exempt. Home ranges too: the territory of a player's largest group shields the members inside it from hunters while they are away (half the kill chance, shared out above 20 residents, from 10 minutes after logout, fading after 12 hours; starvation untouched). See README, Offline protection.)*
- [ ] Server compute: full simulation near players, statistical models elsewhere
