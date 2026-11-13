//! World configuration. Rates elsewhere are expressed per simulated day and
//! scaled by `dt_days()`, so changing the tick rate keeps the balance.

use crate::genome::MutationParams;

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorldConfig {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    /// Must be a multiple of 24 so environment updates land on whole hours.
    pub ticks_per_day: u32,
    /// Four equal seasons, starting with spring.
    pub days_per_year: u32,
    pub mutation: MutationParams,
    /// Genetic distance under which organisms treat each other as their own kind
    /// (no hunting, no fleeing, grouping), when their body plans match exactly.
    /// See [`crate::mating::is_kin`].
    pub species_distance: f32,
    /// Largest difference in any body-plan gene (thermoregulation, birth mode,
    /// body size, diet) at which organisms with identical other genes are still
    /// kin. Body-plan and whole-genome differences add up: kin need
    /// `distance / species_distance + body_gap / kin_body_plan_gap < 1`.
    pub kin_body_plan_gap: f32,
    /// Genetic distance above which organisms will not court each other.
    pub mate_distance: f32,
    /// `k` in the directed-mating override cost `k * max(0, mismatch - 1)^2`.
    pub override_k: f32,
    /// Hard cap on living organisms per player (`World::cap_usage`).
    pub lineage_cap: usize,
    /// Hard cap on the wild ecosystem (all wild species together), split
    /// between the wild species in proportion to their founding populations
    /// (`World::cap_usage`).
    pub wild_cap: usize,
    pub mating_slots_per_season: u32,
    /// Upper bound on perception radius in tiles (also bounds neighbor queries).
    pub max_perception: f32,
    pub balance: Balance,
    /// Level of detail: full agents near focus points, statistical cohorts
    /// elsewhere (`world::lod`). Off by default.
    pub lod: LodConfig,
    /// Run the apply phase on all cores (`world::par_apply`); still
    /// reproducible and independent of thread count. `false` restores the
    /// fully sequential apply, whose worlds differ from parallel ones of
    /// the same seed.
    pub parallel_apply: bool,
    /// Species detection, eras, catastrophes and achievements
    /// (`species`, `chronicle`, `world::chronicle`). Catastrophes are off by
    /// default; detection and records never change the simulation.
    pub scoring: ScoringConfig,
}

/// Species detection, eras and catastrophes ("Scoring: genetic legacy",
/// supporting rules, in the design doc).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ScoringConfig {
    /// Days between species detections (0 disables detection).
    pub species_interval_days: u32,
    /// Most breeding adults sampled per detection, spread over (lineage,
    /// 16-tile region) strata (cohorts are strata of their own).
    pub species_max_samples: usize,
    /// Most samples per stratum.
    pub species_per_stratum: usize,
    /// Two samples are linked (same species) when they would recognize each
    /// other as kin (`mating::is_kin`) and their fertility
    /// (`mating::fertility`) is at least this.
    pub species_min_fertility: f32,
    /// Samples a cluster needs to count as a new species (split or founding).
    pub species_min_new: usize,
    /// Detections a species may go undetected before it is declared extinct.
    pub species_extinct_after: u32,
    /// Representative genomes kept per species to match clusters across
    /// detections (farthest-point picks, so every branch is covered).
    pub species_reps: usize,
    /// Era length in years. Each era ends with a final legacy ranking and
    /// niche-dominance awards.
    pub era_years: u32,
    /// Random catastrophes (severe winter, megadrought, epidemic, flood
    /// year). Off by default so balance runs are unaffected; the server
    /// turns them on.
    pub catastrophes: bool,
    /// Chance per year (from year `catastrophe_first_year`) that a
    /// catastrophe strikes during it.
    pub catastrophe_chance: f32,
    /// No random catastrophe before this year (0-based), so the first
    /// year is always quiet.
    pub catastrophe_first_year: u32,
    /// Strength multiplier on catastrophe effects.
    pub catastrophe_severity: f32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            species_interval_days: 5,
            species_max_samples: 1200,
            species_per_stratum: 8,
            species_min_fertility: 0.5,
            species_min_new: 4,
            species_extinct_after: 2,
            species_reps: 12,
            era_years: 3,
            catastrophes: false,
            catastrophe_chance: 0.4,
            catastrophe_first_year: 1,
            catastrophe_severity: 1.0,
        }
    }
}

/// Level-of-detail settings (`world::lod`, "Scale and performance" in the
/// design doc). Regions are the 16-tile environment chunks. With `enabled`
/// false (the default) nothing here is read and a world runs exactly as
/// without LOD.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LodConfig {
    pub enabled: bool,
    /// A region is active (full agents) when any part of it is within this
    /// many tiles of a focus point (`World::set_focus`) or a possessed
    /// organism.
    pub active_radius: f32,
    /// Extra distance beyond `active_radius` before an active region
    /// collapses into cohorts, so a focus moving back and forth doesn't
    /// thrash regions.
    pub hysteresis: f32,
    /// Genomes kept per cohort. Up to this many members the pool holds
    /// every member's genome exactly; above it, a uniform sample, and
    /// expanded organisms get recombinants of two sampled genomes.
    pub pool_size: f32,
    /// Share of a region's reachable plant food its grazing cohorts can eat
    /// in a day (they only graze where they are).
    pub graze_fraction: f32,
    /// Daily background death hazard of juveniles (orphans, exposure,
    /// accidents) in cohorts.
    pub juvenile_mortality: f32,
    /// Prey per tile at which a predator cohort catches half of what it
    /// needs (Holling type II functional response).
    pub prey_half_density: f32,
    /// Plant-digesting efficiency (1 - carnivory) from which a cohort can
    /// live on plants alone; below it plants cover at most
    /// `efficiency / plant_diet` of its need (agents that digest plants
    /// poorly only graze in passing, between kills).
    pub plant_diet: f32,
    /// Share of a kill's carcass (`Balance::meat_per_kg` per kg) the killer
    /// eats.
    pub carcass_use: f32,
    /// Daily chance per member of moving to a neighboring region, at
    /// wanderlust 0.75; tripled by hunger, quadrupled without water.
    pub migration: f32,
    /// Days an adult female spends finding a mate per breeding cycle, divided
    /// by the season's breeding factor (long in winter).
    pub mate_search_days: f32,
    /// How much denser than the region average organisms live where they
    /// gather (agents clump in groups), for density-dependent breeding and
    /// larval competition.
    pub clump: f32,
}

impl Default for LodConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            active_radius: 40.0,
            hysteresis: 16.0,
            pool_size: 64.0,
            graze_fraction: 0.5,
            juvenile_mortality: 0.01,
            prey_half_density: 0.1,
            plant_diet: 0.1,
            carcass_use: 1.0,
            migration: 0.01,
            mate_search_days: 3.0,
            clump: 1.0,
        }
    }
}

impl LodConfig {
    pub const FIELD_COUNT: usize = 11;

    /// The numeric fields by name (everything but `enabled`).
    pub fn fields_mut(&mut self) -> [(&'static str, &mut f32); Self::FIELD_COUNT] {
        [
            ("active_radius", &mut self.active_radius),
            ("hysteresis", &mut self.hysteresis),
            ("pool_size", &mut self.pool_size),
            ("graze_fraction", &mut self.graze_fraction),
            ("juvenile_mortality", &mut self.juvenile_mortality),
            ("prey_half_density", &mut self.prey_half_density),
            ("plant_diet", &mut self.plant_diet),
            ("carcass_use", &mut self.carcass_use),
            ("migration", &mut self.migration),
            ("mate_search_days", &mut self.mate_search_days),
            ("clump", &mut self.clump),
        ]
    }

    /// Set a field by name. Returns false if there is no such field.
    pub fn set(&mut self, name: &str, value: f32) -> bool {
        match self.fields_mut().into_iter().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                *v = value;
                true
            }
            None => false,
        }
    }
}

/// Tunable rates that shape the ecosystem. Kept together so the sweep tool
/// (`evo-sim sweep`) can search over them.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Balance {
    /// Hunger points per day at metabolism 1.0.
    pub hunger_rate: f32,
    /// Thirst points per day for a mid-sized warm-blooded body.
    pub thirst_rate: f32,
    /// Health lost per day while starving.
    pub starvation_damage: f32,
    /// Logistic regrowth rate of vegetation per day.
    pub veg_regrowth: f32,
    /// Tiles per tick at speed 1.0.
    pub base_speed: f32,
    /// Speed multiplier while sprinting (fleeing, closing on prey).
    pub sprint: f32,
    /// Distance in tiles at which an attack can land.
    pub attack_reach: f32,
    /// How much stealth shrinks the distance at which an organism is seen.
    pub stealth_effect: f32,
    /// Perception radius in tiles per unit of the perception stat.
    pub perception_range: f32,
    /// Multiplier on attack power (predation success).
    pub attack_power: f32,
    /// Hunger-rate reduction for full carnivores (big, infrequent meals).
    pub carnivore_hunger_discount: f32,
    /// Share of food value an even omnivore (carnivory 0.5) loses for
    /// keeping both guts (`stats::generalist_factor`): plants yield
    /// `(1 - carnivory)` and meat `carnivory` of their value, times
    /// `1 - tax * (4c(1-c))^3`. The tax fades fast toward either specialist.
    /// 0 is the old linear rule (an omnivore gets half of each, free to
    /// switch).
    pub generalist_tax: f32,
    /// Extra hunger for a cold-blooded body per 10 °C of body temperature
    /// above full activity (22 °C): a warm ectotherm burns more, a cool one
    /// stays cheap but sluggish.
    pub cold_heat_burn: f32,
    /// Extra mass-specific burn of small bodies: hunger and thirst points
    /// rise at `(adult kg)^-k` times the base rate (food per hunger point
    /// already scales with mass^0.75). Small bodies run hot and fast (shrews
    /// eat all day) and carry smaller reserves, so they need more food and
    /// water per kg and starve or dry out sooner; large bodies outlast them.
    /// 0 keeps food needs on Kleiber's mass^0.75.
    pub size_burn: f32,
    /// Extra hunger rate per unit of litter burden while pregnant.
    pub pregnancy_cost: f32,
    /// Food units left by a carcass per kg of body mass.
    pub meat_per_kg: f32,
    /// Weight of the flee utility (how readily prey runs).
    pub flee_weight: f32,
    /// Fraction of resistance lost by prey that isn't fleeing (ambush).
    pub ambush: f32,
    /// Extra hunger per kin nearby beyond 8, scaled by (1 - group tolerance):
    /// density dependence that keeps fast breeders below the hard cap.
    pub crowding: f32,
    /// Health a predator loses biting fully toxic prey.
    pub toxin_damage: f32,
    /// Larval competition: young of abandoned eggs (no parental care) die at
    /// a daily rate of `(kin nearby * sqrt(pace of life) / larval_competition)^4`
    /// until they mature (or reach `larval_stage`), so unpredated fast
    /// breeders settle below the hard cap. Uses the same kin count as
    /// `crowding`; pace of life (`Traits::pace_of_life`) makes it hit big
    /// clutches of small young hardest, as `breeding_density` does for the
    /// other birth modes. 0 disables.
    pub larval_competition: f32,
    /// Growth (share of adult size) up to which young of abandoned eggs face
    /// larval competition; past it they hold their own like any sub-adult.
    /// 1 keeps it until maturity.
    pub larval_stage: f32,
    /// Daily background death hazard of adults at pace of life 1
    /// (`Traits::pace_of_life`: small bodies with big litters live fast and
    /// die young), so unpredated fast breeders don't pile up adults across
    /// years. Toxic frogs get about 0.6% a day, voles 0.2%, predators and
    /// grazers almost nothing. 0 disables.
    pub adult_mortality: f32,
    /// Fraction of attacks a sheltered burrower escapes, per unit of
    /// burrowing (burrowing 0.8 at 1.0 turns away 80% of attacks).
    pub burrow_shelter: f32,
    /// Flee-weight multiplier for burrowers that can go to ground (hiding is
    /// cheap, so they take cover at lower threat than runners flee).
    pub burrow_caution: f32,
    /// Share of a nearby mother's resistance added to her dependent young's
    /// when a predator attacks it, times `(1 - litter fraction)^2` (so
    /// single young are guarded fully, big litters barely). 0 disables.
    pub parent_defense: f32,
    /// Herd guard: share of a parent's protection (`parent_defense`) a herd
    /// gives each of its young members, calves, sub-adults and orphans
    /// alike, where herd defense holds (`GroupTrait::HerdDefense`, 3+ kin
    /// around): they get the resistance and power of a grown member of their
    /// kind times `herd_guard × care × herd benefit` against a strike (the
    /// better of this and the mother's own guard). 0 disables.
    pub herd_guard: f32,
    /// Density-dependent breeding: live young and incubated eggs per birth
    /// are scaled by `1 / (1 + (d / breeding_density)^4)`, where `d` is the
    /// mother's species' members per tile in her 8×8-tile cell times
    /// (1 - group tolerance) times the square root of pace of life
    /// (`Traits::pace_of_life`). Colonies of fast breeders stop growing where
    /// they are dense instead of booming into the hard cap; slow breeders
    /// barely feel it. Abandoned-egg young face `larval_competition`
    /// instead. 0 disables.
    pub breeding_density: f32,
    /// Chance that alert prey slips a strike and bolts, when prey and hunter
    /// are evenly matched in speed and agility (`Organism::evasion`). Scaled
    /// by the prey's share of the contest, `2 * evade / (evade + pursue)`,
    /// cut by `ambush` for prey caught off guard, capped at 0.95. Only a
    /// strike the prey doesn't slip goes on to the power-vs-resistance roll.
    /// 0 disables (every strike is a power roll, re-rolled each tick).
    pub escape: f32,
    /// Hours a hunter needs to recover after its prey broke free before it
    /// can strike anything again: the prey's head start.
    pub escape_window: f32,
    /// Founding: distance in tiles within which organisms that could eat a
    /// new group count against its home (more the closer they are).
    pub home_buffer: f32,
    /// How strongly a hunter shies away from prey that fights back
    /// (`decide::fight_back`: power times the taxed meat share, so mostly other
    /// predators). Its prey score is scaled by
    /// `(1 - risk)^(injury_aversion * (2 - carnivory))`, `risk` being the
    /// prey's share of its own plus the striker's power (no pack bonus: the
    /// bite lands on one animal). It skips prey whose factor is below
    /// `1 - hunger * carnivory`, so only a hungry hunter takes on another
    /// predator, and an omnivore, which can graze instead, hardly ever does.
    /// 0 disables.
    pub injury_aversion: f32,
    /// Most health a hunter loses per strike that comes to grips with prey
    /// that fights back, per unit of the prey's `fight_back`, win or lose.
    /// 0 disables.
    pub fight_damage: f32,
    /// Hidden recessives: probability per functional load allele per birth
    /// that it mutates into a harmful recessive (tagged with the newborn's
    /// owner). Carriers show nothing; see `recessive_hazard`. 0 disables.
    pub recessive_mutation: f32,
    /// Daily death hazard per load locus at which an organism is homozygous
    /// for a harmful recessive (from birth, so most die young). 0 disables.
    pub recessive_hazard: f32,
    /// Mating-transmitted disease: chance per mating that one partner picks
    /// up a new pathogen, ×4 when the mates come from different founding
    /// lineages (unfamiliar pathogens). 0 disables.
    pub disease_emergence: f32,
    /// Chance per mating (natural or directed) that an infectious partner
    /// (acute or chronic) infects a susceptible one.
    pub disease_transmission: f32,
    /// Health lost per day during an acute infection. It blocks the usual
    /// regeneration, so long infections kill, and it dulls condition (and
    /// with it display coloration and litter size).
    pub disease_damage: f32,
    /// Daily chance that an acute infection ends: the organism becomes immune
    /// or, with chance `disease_chronic`, a chronic carrier.
    pub disease_recovery: f32,
    /// Chance that an ending acute infection leaves a lifelong carrier:
    /// symptomless and still infectious, with the fertility cost.
    pub disease_chronic: f32,
    /// Fertility lost per infectious parent (acute or chronic) at a mating.
    pub disease_fertility: f32,
    /// Daily transmission chance between a nursing mother and her young when
    /// one of them is infectious (close kin).
    pub disease_kin: f32,
    /// Most tiles one path search may expand (`pathfinding`). Movement
    /// steers straight at its target and plans a route only when the direct
    /// step is blocked; a search that runs out of budget heads for the tile
    /// it got closest to and plans again from there. 0 disables pathfinding
    /// (greedy steering only).
    pub path_nodes: f32,
    /// How strongly regional wetness moves the level of shallow lakes, marsh
    /// and rivers: level = 1 + response × (wetness − 0.5) / 0.38, so a
    /// drought dries them out and a wet spell fills them. 0 keeps them fixed.
    pub water_response: f32,
    /// River level rise per unit of snowmelt at the river's headwaters
    /// (spring melt swells rivers so they block weak swimmers). 0 disables.
    pub snowmelt_swell: f32,
    /// Share of plant regrowth that follows regional wetness (drought slows
    /// it, wet spells speed it up). 0 disables.
    pub rain_growth: f32,
    /// Extra chill in °C on open tiles during a storm (all tiles get 3 °C).
    pub storm_exposure: f32,
    /// Share of a forest tile's plant food that grows in the canopy, out of
    /// reach of short bodies that can't climb. 0 disables.
    pub canopy_share: f32,
    // --- Seasonal strategies (`world::seasonal`) ---
    /// Seasonal mean temperature (°C, no day/night swing) below which a
    /// hibernator or brumator goes dormant in autumn and winter. Brumators
    /// (and hibernators that can't heat themselves) wake when it is 2 °C
    /// warmer again; warm-blooded hibernators sleep until spring.
    pub dormancy_temp: f32,
    /// Seasonal mean temperature (°C) above which an estivator goes dormant
    /// in summer; it wakes when it is 2 °C cooler or summer ends.
    pub estivation_temp: f32,
    /// Hunger and thirst rate while hibernating, brumating or estivating, as
    /// a fraction of the resting rate, at full gene expression (twice this
    /// just above the expression threshold).
    pub dormant_metabolism: f32,
    /// Chance that a cold-blooded hibernator (affinity mismatch) never wakes
    /// from its winter dormancy.
    pub dormancy_mismatch: f32,
    /// Dormant-metabolism multiplier for a warm-blooded brumator (affinity
    /// mismatch: it keeps burning fat while dormant).
    pub brumation_mismatch: f32,
    /// Most fat (hunger points) laid down before a dormancy, times
    /// `1 + fat stores`. Fat pays for hunger while dormant or above 50 hunger.
    pub fat_capacity: f32,
    /// How much a dormant animal's den hides it from passing hunters (as
    /// stealth does; a sheltered burrower hides 0.5).
    pub den_concealment: f32,
    /// Distance of a migrator's winter range south of its summer range, as a
    /// fraction of the map height at full gene expression.
    pub migration_range: f32,
    /// Death chance per tile travelled on a seasonal migration, times
    /// `1.5 - endurance` and cut by group navigation (`group_navigation`).
    pub migration_risk: f32,
    /// Hunger-rate multiplier while in torpor on a cold night, at full gene
    /// expression (cold costs nothing extra while torpid).
    pub torpor_metabolism: f32,
    /// Most food a cacher hoards, in its own hunger points, at full gene
    /// expression.
    pub cache_capacity: f32,
    /// Shared burrows (`GroupTrait::SharedBurrows`): how far a shared burrow
    /// deepens a carrier's shelter at full benefit. Its effective burrowing
    /// is `burrowing + shared_burrow * benefit * (1 - burrowing)`, which sets
    /// whether it can shelter at all (above 0.5), how many attacks fail
    /// (`burrow_shelter`) and how well it hides; the burrow's temperature
    /// buffering rises from 60% toward 10 °C by up to `0.6 * shared_burrow`.
    /// 0 disables.
    pub shared_burrow: f32,
    /// Sentries (`GroupTrait::Sentries`): share of two-hour shifts in which a
    /// sentry carrier in a group is on duty and stands watch, if no more
    /// urgent need wins. 0 disables.
    pub sentry_share: f32,
    /// Perception boost for a sentry on watch (it ignores breeding drive).
    pub sentry_vigilance: f32,
    /// Extra hunger rate while on watch (an alert posture, no foraging).
    pub sentry_cost: f32,
    /// Communal dens (`GroupTrait::CommunalDens`): how much shared body heat
    /// cuts a winter sleeper's hunger and thirst rate (so its fat lasts
    /// longer) at full benefit, for a carrier with 3 or more dormant carrier
    /// den mates within a few tiles (fewer mates, less). Solitary sleepers
    /// get nothing. 0 disables.
    pub communal_den: f32,
    /// Group navigation (`GroupTrait::GroupNavigation`): how much travelling
    /// with kin cuts a carrier's journey risk at full benefit (where the
    /// trait holds at its summer range or where it is). Navigators also keep
    /// together, scan wider for the best winter pasture, and travel on
    /// `group_navigation / 3` less hunger. 0 disables.
    pub group_navigation: f32,
    /// Shared caches (`GroupTrait::SharedCaches`): capacity of a group's
    /// regional cache in full individual hoards (`cache_capacity`). Where the
    /// trait holds, carriers gather into it; any plant-eating group member of
    /// the lineage in the region may eat from it, free riders included, and
    /// it spoils at half the rate of a private hoard. 0 disables.
    pub shared_cache: f32,
    /// Hunger or thirst (0-100) at which a possessed organism breaks off its
    /// move order to eat or drink; it goes back to the order once the need
    /// is down to 30. Below this the order is followed whatever the needs.
    pub possess_urgency: f32,
    /// Threat level (as perceived, about 0-2) at which a possessed organism
    /// breaks off its move order to flee or take cover.
    pub possess_danger: f32,
    /// Seasonal coat: hunger points a full molt costs at full gene
    /// expression (one into the winter coat in autumn, one back in spring).
    pub molt_cost: f32,
    /// Seasonal coat: insulation a warm-blooded body gains in its full
    /// winter coat at full gene expression. Its heat tolerance drops with
    /// it (comfort ceiling `30 - 8 × insulation`).
    pub coat_insulation: f32,
    /// Timed breeding: length of the breeding window from the start of
    /// spring, as a share of the season.
    pub breeding_window: f32,
    /// Timed breeding: extra fertility of a mating in the window at full
    /// gene expression (better conception; twins in single-young species).
    pub timed_conception: f32,
    /// Breeding-season risk: how much full breeding drive lowers the urge to
    /// flee a threat (on top of the perception it costs), offset by
    /// sentries. 0 disables.
    pub breeding_recklessness: f32,
    /// How far a threat it has seen (perceived level, capped at 1) cuts an
    /// animal's urge to court: courting is `1 - courtship_caution × threat`
    /// as urgent. Without it, full breeding drive outranked fleeing from
    /// all but the closest hunters, so prey in breeding condition (most
    /// adults of year-round breeders that hadn't found a mate) died
    /// courting. 0 disables.
    pub courtship_caution: f32,
    /// Share of her recovery time (`Traits::recovery_days`) an incubating
    /// parent starts over when her clutch hatches: young leave a live
    /// bearer's body at birth but an incubator's care at hatching, so both
    /// recover from the end of the brood (doc: eggs tie the parent to the
    /// nest). Without it a brooder whose recovery ran out while she sat on
    /// the eggs could breed again the day they hatched, cycling almost twice
    /// as fast as a live bearer. 0 restores recovery from laying.
    pub brood_recovery: f32,
    /// Egg overheating (doc: eggs "can be raided, chilled or overheated"):
    /// damage per hour per °C an egg sits above its ceiling (39 °C for
    /// warm-blooded eggs, 34 °C for cold-blooded ones); the clutch dies
    /// at 1. Eggs track their nest's microclimate: water and shade damp the
    /// air's swing toward the tile's annual mean. A sitting parent shades
    /// her eggs by at most 3 °C; abandoned eggs rely on the site their
    /// mother picks. 0 disables overheating.
    pub egg_overheat: f32,
    /// Most °C a sitting warm-blooded parent can lift her eggs above the
    /// nest's air toward 32 °C (a cold-blooded one manages 4). Below
    /// `32 - brood_warmth` warm-blooded eggs cool even under a parent.
    /// Large values restore the old fixed 32 °C.
    pub brood_warmth: f32,
    /// What brooding costs the parent: extra hunger per hour for each 10 °C
    /// she warms her eggs, as a share of her hourly hunger, and extra thirst
    /// the same way for each 10 °C she shades them. The doc's "eggs must be
    /// guarded and kept warm": a brooder in a cold place eats much more and
    /// must leave the nest to feed more often. 0 makes brooding free.
    pub brood_cost: f32,
}

impl Default for Balance {
    /// Tuned by `evo-sim sweep` (2 rounds × 16 trials × 3 seeds, 96×96 map,
    /// 120 days), rounded. Re-run the sweep after changing rules.
    fn default() -> Self {
        Self {
            hunger_rate: 57.0,
            thirst_rate: 36.5,
            starvation_damage: 33.5,
            veg_regrowth: 0.35,
            base_speed: 0.82,
            sprint: 2.45,
            attack_reach: 1.2,
            stealth_effect: 0.4,
            perception_range: 4.6,
            attack_power: 0.75,
            carnivore_hunger_discount: 0.44,
            generalist_tax: 0.5,
            cold_heat_burn: 1.0,
            size_burn: 0.25,
            pregnancy_cost: 0.74,
            meat_per_kg: 5.75,
            flee_weight: 1.33,
            ambush: 0.4,
            crowding: 0.03,
            toxin_damage: 48.0,
            larval_competition: 13.0,
            larval_stage: 1.0,
            adult_mortality: 0.011,
            burrow_shelter: 1.0,
            burrow_caution: 2.0,
            parent_defense: 1.0,
            herd_guard: 1.0,
            breeding_density: 0.03,
            escape: 0.6,
            escape_window: 1.0,
            home_buffer: 28.0,
            injury_aversion: 6.0,
            fight_damage: 30.0,
            recessive_mutation: 0.002,
            recessive_hazard: 0.08,
            disease_emergence: 0.004,
            disease_transmission: 0.4,
            disease_damage: 3.0,
            disease_recovery: 0.08,
            disease_chronic: 0.2,
            disease_fertility: 0.3,
            disease_kin: 0.2,
            path_nodes: 1500.0,
            water_response: 1.0,
            snowmelt_swell: 1.0,
            rain_growth: 0.5,
            storm_exposure: 3.0,
            canopy_share: 0.3,
            dormancy_temp: 6.0,
            estivation_temp: 27.0,
            dormant_metabolism: 0.05,
            dormancy_mismatch: 0.5,
            brumation_mismatch: 6.0,
            fat_capacity: 50.0,
            den_concealment: 0.3,
            migration_range: 0.35,
            migration_risk: 0.002,
            torpor_metabolism: 0.4,
            cache_capacity: 80.0,
            shared_burrow: 0.5,
            sentry_share: 0.25,
            sentry_vigilance: 0.5,
            sentry_cost: 0.2,
            communal_den: 0.5,
            group_navigation: 0.6,
            shared_cache: 6.0,
            possess_urgency: 85.0,
            possess_danger: 0.6,
            molt_cost: 12.0,
            coat_insulation: 0.3,
            breeding_window: 0.4,
            timed_conception: 0.3,
            breeding_recklessness: 0.15,
            courtship_caution: 1.0,
            brood_recovery: 1.0,
            egg_overheat: 0.02,
            brood_warmth: 20.0,
            brood_cost: 1.0,
        }
    }
}

impl Balance {
    pub const FIELD_COUNT: usize = 78;

    /// Every field by name, for search tools and reports.
    pub fn fields(&self) -> [(&'static str, f32); Self::FIELD_COUNT] {
        let mut copy = *self;
        copy.fields_mut().map(|(n, v)| (n, *v))
    }

    pub fn fields_mut(&mut self) -> [(&'static str, &mut f32); Self::FIELD_COUNT] {
        [
            ("hunger_rate", &mut self.hunger_rate),
            ("thirst_rate", &mut self.thirst_rate),
            ("starvation_damage", &mut self.starvation_damage),
            ("veg_regrowth", &mut self.veg_regrowth),
            ("base_speed", &mut self.base_speed),
            ("sprint", &mut self.sprint),
            ("attack_reach", &mut self.attack_reach),
            ("stealth_effect", &mut self.stealth_effect),
            ("perception_range", &mut self.perception_range),
            ("attack_power", &mut self.attack_power),
            ("carnivore_hunger_discount", &mut self.carnivore_hunger_discount),
            ("generalist_tax", &mut self.generalist_tax),
            ("cold_heat_burn", &mut self.cold_heat_burn),
            ("size_burn", &mut self.size_burn),
            ("pregnancy_cost", &mut self.pregnancy_cost),
            ("meat_per_kg", &mut self.meat_per_kg),
            ("flee_weight", &mut self.flee_weight),
            ("ambush", &mut self.ambush),
            ("crowding", &mut self.crowding),
            ("toxin_damage", &mut self.toxin_damage),
            ("larval_competition", &mut self.larval_competition),
            ("larval_stage", &mut self.larval_stage),
            ("adult_mortality", &mut self.adult_mortality),
            ("burrow_shelter", &mut self.burrow_shelter),
            ("burrow_caution", &mut self.burrow_caution),
            ("parent_defense", &mut self.parent_defense),
            ("herd_guard", &mut self.herd_guard),
            ("breeding_density", &mut self.breeding_density),
            ("escape", &mut self.escape),
            ("escape_window", &mut self.escape_window),
            ("home_buffer", &mut self.home_buffer),
            ("injury_aversion", &mut self.injury_aversion),
            ("fight_damage", &mut self.fight_damage),
            ("recessive_mutation", &mut self.recessive_mutation),
            ("recessive_hazard", &mut self.recessive_hazard),
            ("disease_emergence", &mut self.disease_emergence),
            ("disease_transmission", &mut self.disease_transmission),
            ("disease_damage", &mut self.disease_damage),
            ("disease_recovery", &mut self.disease_recovery),
            ("disease_chronic", &mut self.disease_chronic),
            ("disease_fertility", &mut self.disease_fertility),
            ("disease_kin", &mut self.disease_kin),
            ("path_nodes", &mut self.path_nodes),
            ("water_response", &mut self.water_response),
            ("snowmelt_swell", &mut self.snowmelt_swell),
            ("rain_growth", &mut self.rain_growth),
            ("storm_exposure", &mut self.storm_exposure),
            ("canopy_share", &mut self.canopy_share),
            ("dormancy_temp", &mut self.dormancy_temp),
            ("estivation_temp", &mut self.estivation_temp),
            ("dormant_metabolism", &mut self.dormant_metabolism),
            ("dormancy_mismatch", &mut self.dormancy_mismatch),
            ("brumation_mismatch", &mut self.brumation_mismatch),
            ("fat_capacity", &mut self.fat_capacity),
            ("den_concealment", &mut self.den_concealment),
            ("migration_range", &mut self.migration_range),
            ("migration_risk", &mut self.migration_risk),
            ("torpor_metabolism", &mut self.torpor_metabolism),
            ("cache_capacity", &mut self.cache_capacity),
            ("shared_burrow", &mut self.shared_burrow),
            ("sentry_share", &mut self.sentry_share),
            ("sentry_vigilance", &mut self.sentry_vigilance),
            ("sentry_cost", &mut self.sentry_cost),
            ("communal_den", &mut self.communal_den),
            ("group_navigation", &mut self.group_navigation),
            ("shared_cache", &mut self.shared_cache),
            ("possess_urgency", &mut self.possess_urgency),
            ("possess_danger", &mut self.possess_danger),
            ("molt_cost", &mut self.molt_cost),
            ("coat_insulation", &mut self.coat_insulation),
            ("breeding_window", &mut self.breeding_window),
            ("timed_conception", &mut self.timed_conception),
            ("breeding_recklessness", &mut self.breeding_recklessness),
            ("courtship_caution", &mut self.courtship_caution),
            ("brood_recovery", &mut self.brood_recovery),
            ("egg_overheat", &mut self.egg_overheat),
            ("brood_warmth", &mut self.brood_warmth),
            ("brood_cost", &mut self.brood_cost),
        ]
    }

    /// Set a field by name. Returns false if there is no such field.
    pub fn set(&mut self, name: &str, value: f32) -> bool {
        match self.fields_mut().into_iter().find(|(n, _)| *n == name) {
            Some((_, v)) => {
                *v = value;
                true
            }
            None => false,
        }
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            width: 128,
            height: 128,
            ticks_per_day: 48,
            days_per_year: 120,
            mutation: MutationParams { rate: 0.004, sd: 0.07 },
            species_distance: 0.12,
            kin_body_plan_gap: 0.25,
            mate_distance: 0.3,
            override_k: 1.0,
            lineage_cap: 1500,
            wild_cap: 5000,
            mating_slots_per_season: 6,
            max_perception: 8.0,
            balance: Balance::default(),
            lod: LodConfig::default(),
            parallel_apply: true,
            scoring: ScoringConfig::default(),
        }
    }
}

impl WorldConfig {
    pub fn dt_days(&self) -> f32 {
        1.0 / self.ticks_per_day as f32
    }

    pub fn ticks_per_hour(&self) -> u64 {
        (self.ticks_per_day / 24).max(1) as u64
    }

    pub fn cap_for(&self, owner: crate::genome::PlayerId) -> usize {
        if owner == crate::genome::WILD {
            self.wild_cap
        } else {
            self.lineage_cap
        }
    }

    pub fn days_per_season(&self) -> u32 {
        (self.days_per_year / 4).max(1)
    }

    /// Era length in days (`ScoringConfig::era_years` years, at least one day).
    pub fn era_days(&self) -> u64 {
        (self.scoring.era_years.max(1) as u64 * self.days_per_year as u64).max(1)
    }
}
