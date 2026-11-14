# Evolution MMO — Game Design

Nov 12, 2026 · @Al

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

## Player controls

Players never set behavior directly. They act through three levers; everything else comes from genes.

| Lever | What the player does | Notes |
| --- | --- | --- |
| Possession | Take direct control of one organism: move, forage, hunt, fight, scout, court | Returns to genetic behavior when released. A small bonus (perception, reaction) rewards active play without dominating |
| Directed matings | Pair two organisms, including with another player's organism | Limited slots per season. Other player must consent and may set a price. The organisms must physically meet, so the trip carries risk. Stud marketplace for trading genetics |
| Group commands | Order migration, claim or abandon territory, split a group | How strongly a group follows them is genetic: the obedience gene weighs orders against instinct, and obedient animals are tamer (slower to flee) |

**Directed-mating override cost.** When a directed mating falls outside an organism's genetic mate preferences, it costs more the further outside it is. See Genetics.

**A typical session:** check the genetics dashboard and alerts, arrange a trade, possess an organism and escort it across the map to the meeting, log off while the lineage carries on.

## Genetics

Behavior and mate preference are inherited and mutate like physical traits. To change how a lineage acts, a player has to breed for it.

**Behavior genes** set weights such as aggression, boldness, foraging range, group tolerance, wanderlust and pickiness. Each organism's AI makes decisions from its own weights, so even siblings act differently.

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

**Balancing picky vs. flexible lineages.** Flexible preferences make a lineage cheap to steer, so pickiness needs real benefits:

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
| Carnivore + pack instinct | Cooperative hunting | Can take large prey (needs \~4+ carriers nearby) | Carriers |
| Toxins + bright coloration | Warning coloration | Predators avoid the population | Everyone in the area, including look-alikes |
| Claws or burrowing + group tolerance | Shared burrows | Shelter from weather and predators | Carriers |
| Keen hearing + vocal ability | Alarm calls | Group-wide early warning | Carriers |
| Herbivore + large size + group tolerance | Herd defense | Predators can't single out individuals | Group members |
| Migration + group tolerance | Group navigation | Safer, more accurate migration | Group members |
| Hibernation + group tolerance | Communal dens | Shared body heat cuts fat loss | Den members |
| Caching + group tolerance | Shared caches | Larger stockpile than individual hoards | Group members (free-rider risk) |
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
| Vulnerability | Young protected inside parent | Eggs can be raided, chilled or overheated |
| Mobility | Parent keeps moving | Tied to nest until hatching |
| Fits with | Migration, warm-blooded | Cold-blooded, colonies |

**3. Offspring per birth.** Few (1–2): larger, high survival, long care, slow growth. Many (8+): fragile, most die young, fast recovery and more variation. Food cost, recovery time and care per young scale with the number. Only offspring that reach breeding age add to the legacy score.

**4. Physical traits**

| Body area | Traits |
| --- | --- |
| Body | Body size, muscle mass, fat stores |
| Limbs | Long legs, claws, hooves, webbed feet, tail |
| Locomotion | Swimming, wings (flight), gliding flaps, climbing, burrowing |
| Covering | Fur, scales, armor plates |
| Head | Fangs, large eyes, large ears, keen nose |
| Other | Diet (carnivore, herbivore, omnivore, scavenger), camouflage, toxins |
| Instinct genes | Boldness, aggression, wanderlust, group tolerance, obedience, mate preference and pickiness |

**Stats layer.** Physical traits set stats; stats feed needs and decide how well behaviors succeed. Most traits carry a built-in cost.

| Stat | Raised by | Lowered by | Affects |
| --- | --- | --- | --- |
| Speed | Long legs, hooves, muscle | Body size, fat, armor, cold-blooded (in cold), low energy | Fleeing, hunting, migration; sprinting drains energy |
| Strength | Body size, muscle, claws, fangs | Low energy | Fighting, taking large prey |
| Endurance | Long legs, hooves, fat reserves | Body size | Energy drain from activity |
| Agility | Claws, tail | Hooves, armor | Fleeing, catching nimble prey |
| Perception | Large eyes, large ears, keen nose | Breeding drive | Spotting threats, food, mates |
| Stealth | Camouflage | Body size | Avoiding predators, ambushing prey |
| Defense | Body size, scales, armor, toxins | — | Surviving attacks |
| Metabolism | Body size, muscle, warm-blooded | Cold-blooded | Hunger and thirst rates |
| Insulation | Fur, fat | Large ears (shed heat) | Temperature |

**Founders.** Starting organisms vary around the design rather than being clones, so selection has material from day one. A larger founding population costs points, so group-trait builds pay to start above threshold.

## Seasons and biomes

The map has distinct biomes, changing weather and seasons, so each organism picks one primary seasonal strategy, plus optional add-ons.

| Strategy | How it works | Benefit | Cost or risk |
| --- | --- | --- | --- |
| Hibernation | Dormant through winter on stored fat | Almost no winter food needed; safe from weather | Must fatten in fall; den can be found; no winter breeding |
| Migration | Travels between seasonal ranges | Always in good conditions | Dangerous journeys; needs navigation and stamina |
| Year-round active | No seasonal change | Breeds and forages all year; holds territory | Faces winter head-on; needs cold traits or a mild biome |
| Estivation | Dormant through summer heat or drought | Survives deserts and dry seasons | Loses the summer |

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
| Hunger | Time, activity | Size, warm-blooded, litter size, pregnancy | Energy drops, then starvation |
| Thirst | Time, heat | Desert adaptations, size | Faster decline than hunger |
| Energy | Movement, fighting, cold | Endurance; cold-blooded in cold | Slower, weaker, can't flee |
| Temperature | Weather, biome | Fur, basking, thermoregulation | Lethargy, energy drain, death in extremes |
| Safety | Nearby threats | Boldness, group size, camouflage | Stress, flight |
| Breeding drive | Season, maturity | Timed breeding, offspring count | Seeks mates |
| Social (group species) | Time alone | Group tolerance | Stress, wandering |

**Action selection.** Each tick, an organism scores possible actions by the urgency of the related need, weighted by its behavior genes; the highest score wins. Hungry organisms prioritize food over everything except immediate danger. A bold organism keeps foraging near a predator; a timid one flees and goes hungry.

**Cascades.** Unmet hunger drains energy, low energy cuts speed and endurance, and slow organisms get caught and fail hunts, which worsens hunger. Bad seasons can spiral, which is where selection bites.

**Breeding season.** High breeding drive lowers perception and threat response and raises risk-taking, making the season dangerous. Showy displays attract mates and predators alike; sentry genes and short timed-breeding windows offset the risk.

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
2. Build the Rust server around the proven rules. *(Started: `crates/server`, see the README.)*
3. Build the TypeScript client against the server's API. *(Started: `client/`, PixiJS + Vite, see the README.)*

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
- **Population limits:** food-based carrying capacity, plus a hard cap per lineage.
- **Parallel regions:** map split into chunks updated across all cores.
- **More worlds, not bigger ones:** scale by adding independent worlds (shards), each with its own ecosystem, seasons and leaderboard.

**Next step:** build to the prototype target, measure real per-organism tick cost, then set launch numbers.

## Open questions

- [ ] Dominant designs: how to keep any single build from taking over (planned next)
- [ ] Duplicate mutations: separate tags per origin, or credit only whoever had it first? *(Currently separate tags per origin: with continuous alleles, "first" needs a rule for when two alleles count as the same. See README, Suggested next steps.)*
- [ ] Seasonal strategy: fixed at design, or an evolvable gene?
- [x] Group commands (migration, splitting): keep as player levers, or make genetic? *(Both: the commands stay player levers, and an obedience gene sets how strongly a group heeds them against its own instincts. Obedience is paid for in tameness: an obedient animal flees a threat later. See README, Groups and territory.)*
- [ ] Point costs for each trait and for founding population size
- [ ] Tick rate, and whether the world runs real-time or slow and async
- [x] Offline protection: home ranges or surviving founder groups *(Built as surviving founder groups: for up to 12 hours after logout, a lineage down to its last 12 members suffers a quarter of the kills and half the starvation and thirst; organisms in a trade or on the market are exempt. See README, Offline protection.)*
- [ ] Server compute: full simulation near players, statistical models elsewhere
