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
    /// Possessed organisms tick faster ("Slow world clock" in the design
    /// doc): each tick is split into this many sub-steps for the movement of
    /// possessed organisms following a move order (`World::substep`), so a
    /// server can move them, and apply new orders, between world ticks.
    /// Each sub-step covers `1 / possess_substeps` of a tick's movement,
    /// so possession is no faster overall. 1 turns sub-steps off. Only
    /// possessed organisms are affected.
    pub possess_substeps: u32,
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
    /// Hysteresis (snapshot version 42; 0 or 1 turns each off, as in older
    /// worlds). A split counts (named, announced, credited) only once its
    /// cluster has been detected this many times in a row, each time with at
    /// least `species_min_new` samples, and then has at least
    /// `species_confirm_adults` estimated adults; until then it is a
    /// candidate and its members are unclassified. Detections are
    /// `species_interval_days` apart, so the default 4 is 20 days.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v42::list"))]
    pub species_confirm: u32,
    /// Estimated adults a candidate needs to be confirmed as a species.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v42::list"))]
    pub species_confirm_adults: f32,
    /// A species merges into another (goes extinct, merged) only after
    /// continuing as the same cluster as a larger one this many detections
    /// in a row; until then it stays alive with no adults of its own.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v42::list"))]
    pub species_merge_confirm: u32,
    /// A new cluster that matches a species which merged away within this
    /// many days is that species again (same id and name, no new
    /// announcement or credit), not a new one. 0: never re-used.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v42::list"))]
    pub species_revive_days: u32,
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
            species_confirm: 4,
            species_confirm_adults: 12.0,
            species_merge_confirm: 4,
            species_revive_days: 360,
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
    /// Vigilance at water and at rest: a threat in sight scales the urges to
    /// drink and to rest by `1 - threat_caution × threat` (threat capped at
    /// 1), so immediate danger outranks them (doc: "Hungry organisms
    /// prioritize food over everything except immediate danger"). The
    /// parched still drink: the extra urge above thirst 80 is kept. 0
    /// disables.
    pub threat_caution: f32,
    /// Share of the `ambush` penalty (on resistance and on the chance to
    /// slip a strike) waived for prey that had a threat in sight
    /// (`Needs::safety` > 0) but wasn't fleeing: drinking, resting or
    /// grazing with an eye on the hunter, it is not wholly caught off guard.
    /// 0: anything not fleeing is fully ambushed, as before.
    pub alert_guard: f32,
    /// Foraging on stripped ground. A forager whose tile is bare first
    /// steps to the best of its 8 neighboring tiles with a bite on it,
    /// instead of passing such remnants by for a sampled patch further out;
    /// if nothing is found within 3× its foraging reach either, it samples
    /// once more at `pasture_search` × its reach (a herd moving on to fresh
    /// pasture) before it wanders. 0 disables both.
    pub pasture_search: f32,
    /// Prey switching by search image: chance that a hunter overlooks prey
    /// of another kind (lineage) than its last kill (`Organism::search_image`),
    /// drawn per hunter, prey and day. Hunters mostly kill the common prey,
    /// so most carry its image and a rare kind is taken less than its
    /// encounter rate alone would have it. 0 disables.
    pub search_image: f32,
    /// How many times as long a cold-blooded live bearer carries her young
    /// as a warm-blooded one of the same size and litter
    /// (`Traits::gestation`). Cold-blooded eggs take 1.4× as long, but a
    /// live-bearing ectotherm basks and keeps her embryos near her active
    /// temperature (why reptiles of cold climates bear live young). At 1.4,
    /// the old rule, cold-blooded live bearers won 15–17% of tournaments
    /// (fair 25%) and cold-blooded egg layers that leave their eggs 37–39%.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v12"))]
    pub cold_gestation: f32,
    /// Egg predation on nesting grounds: hourly, an unguarded clutch (left
    /// by its mother) is found and eaten whole with a daily chance of
    /// `nest_predation` for each other clutch, of any kind, within 2 tiles
    /// of it (doc: eggs "can be raided"). Egg predators search where eggs
    /// are dense, so layers that leave many clutches close together lose
    /// the most; a brooder's clutch is guarded. Aimed at cold-blooded
    /// abandoned eggs, whose eggs rarely chill and whose mothers lay again
    /// within days: without it they won 35–38% of tournaments (fair 25%).
    /// 0 disables.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v13"))]
    pub nest_predation: f32,
    /// The price of obedience (`Gene::Obedience`, group commands as genes):
    /// tameness. An animal's flight from a threat is weighted by
    /// `1 − tameness × (obedience − 0.5)`, so the biddable flee later and
    /// the headstrong sooner; at the design default (0.5) nothing changes.
    /// 0 makes obedience free.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v14"))]
    pub tameness: f32,
    /// When a migrator leaves for its winter range: in autumn, once plant
    /// regrowth at its summer range (`terrain::growth_warmth` of the
    /// seasonal mean there) has fallen to this share of the full rate, so
    /// the winter range's pasture is saved for the cold season instead of
    /// being grazed down through autumn; in winter it goes regardless. 1
    /// leaves at the first day of autumn (the old rule).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v16"))]
    pub migration_onset: f32,
    /// The home-range form of offline protection (`world::offline`): how a
    /// claimed territory shields its residents while their owner is away.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v17"))]
    pub home_range: HomeRangeBalance,
    /// Lactation: after a litter, a warm-blooded live bearer conceives again
    /// only once her young are `lactation` of the way to weaning (growth 0.7,
    /// when nursing and a mother's guard end; `Traits::weaning_days`), on
    /// top of her recovery: milk is an endotherm's long, costly care, and
    /// lactating mammals don't come into heat. Cold-blooded live bearers and
    /// egg layers don't nurse that way and are unaffected. 0 disables (the
    /// default). Tried at 1 against warm-blooded live bearers' tournament
    /// lead: it slows the wild mammals they compete with as much, so their
    /// wins didn't fall and incubators gained (README, *Warm-blooded live
    /// bearers*).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v19"))]
    pub lactation: f32,
    /// Extra upkeep of endothermy: a warm-blooded body's hunger is
    /// `1 + warm_upkeep` times the base (doc: warm-blooded food needs
    /// "high, constant"), on top of its extra burn in the cold; the
    /// same for level-of-detail cohorts. 0 is the old rule (and the
    /// default: see *Endothermy upkeep* in the README's Balance section).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v19"))]
    pub warm_upkeep: f32,
    /// Long fangs crop and chew plants badly: plant food is worth
    /// `1 - fang_grazing × fang size × (1 - carnivory)` of its value
    /// (`Traits::plant_efficiency`), so sabres cost a grazer the most and a
    /// hunter little (nothing for a pure carnivore). 0 is the old rule.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v20"))]
    pub fang_grazing: f32,
    /// Scales leave less skin for fur: a warm-blooded body loses
    /// `scale_cover × scales × fur warmth` of insulation (`Traits::insulation`),
    /// so scales cost a furred mammal warmth and a naked or cold-blooded body
    /// nothing. 0 is the old rule.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v20"))]
    pub scale_cover: f32,
    /// Strength of the body plan's count, shape and snout effects
    /// (`morph_effects`: ear count and shape, leg count, tail shape, snout
    /// length). Neutral at the default body whatever its value; 0 leaves
    /// every body as `stats::derive` made it; 1 is the spec's formulas and
    /// the default (README, *Body plan effects* and *Morphology on by
    /// default*). Saves from before version 21 read 0.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v21"))]
    pub morph_effects: f32,
    /// Tail drag: burrowing ×(1 − 0.2·T·k) and `0.2·T·k` of speed lost per
    /// unit of vegetation density, for tail length `T`. 0 is off; 1 is the
    /// spec's value and the default.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v21"))]
    pub morph_tail_drag: f32,
    /// Heat loss of a thin or fin tail: insulation −0.05·T·k. 0 is off; 1 is
    /// the spec's value and the default.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v21"))]
    pub morph_tail_chill: f32,
    /// Long legs are tippy (stability ×(1 − 0.3·LegLength·k)) and dig worse
    /// (burrowing −0.2·LegLength·k). 0 is off; 1 is the spec's value and
    /// the default.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v21"))]
    pub morph_long_legs: f32,
    /// Directional vision (`crate::vision`; README, *Directional vision*):
    /// eyes see fields around the organism's heading (its last movement).
    /// At `k` (0–1) a neighbor in a blind sector is noticed at
    /// `1 - k * visual_share` of the distance, where `visual_share` is the
    /// sight channel's part of perception, `(0.10 + 0.3 Eyes) / perception`
    /// (hearing and smell still work; see `vision_scan` for an animal
    /// standing still), and a hunter's kill chance is multiplied by
    /// `1 + 0.35 k (min(binocular, 90) - 20) / 90` (depth judgment; 1 at
    /// the default eyes). 0 is off and leaves every result as before the
    /// mechanic; 1 is the default (README, *Morphology on by default*).
    /// Saves from before version 22 read 0.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v22"))]
    pub directional_vision: f32,
    /// Scanning under directional vision: an organism standing still (no
    /// movement target) looks around, so its sight in a blind sector is
    /// `0.5 × vision_scan × coverage` instead of 0 (`vision::detect_factor`).
    /// Inert while `directional_vision` is 0.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v22::scan"))]
    pub vision_scan: f32,
    /// Probability per allele per generation that a discrete morphology
    /// allele (eye, ear or leg count, ear or tail shape; `crate::morph`)
    /// mutates one step. The default is the spec's 0.0005
    /// (`morph::RECOMMENDED_STEP_RATE`); 0 runs bit for bit as before the
    /// morphology loci (an expressed mutant count changes kinship and
    /// fertility). Snapshots before version 23 read 0.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v23"))]
    pub morph_step_rate: f32,
    /// Whether the morphology segment's alleles count in the legacy tally
    /// (`LegacySnapshot`, cohorts included; above 0.5 is on): the spec counts
    /// them like any allele (`docs/morphology.md`, decisions 5, 13 and 16;
    /// 1, the default). Niche leaders count them the same way. Snapshots
    /// before version 24 read 0 (not counted).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v24"))]
    pub morph_legacy: f32,
    /// Starving roaming forage: a hunter roaming under `Action::Hunt` with no
    /// prey in sight nibbles plants at 0.3 of a full graze, as it does while
    /// courting, resting or drinking, once its hunger is above this. Meat
    /// covers a few percent of a hunter's day and plants the rest, and a
    /// roaming hunter otherwise eats nothing; pregnant and nursing mothers,
    /// who can't court, roam the most and starve first. 100 is off (hunger
    /// never exceeds it): the default, and what snapshots before version 25
    /// read. Tried as the fix for predators dying out over the years: at 70
    /// both predators survive 3 years on 4 of 4 seeds but drive the voles
    /// out on 2; at 75–85 they still die out within 4 years. See
    /// *Predators over the years* in the README's Balance section.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v25"))]
    pub roaming_forage: f32,
    // The carnivore-diet rules (version 26; *Predators that live on meat*
    // and *Predators in their niches* in the README's Balance section).
    // Each is off at 0, what snapshots before version 26 read
    // (`serde_util::before_v26`); on by default from version 31, with the
    // food values (*Food by its value* in the README).
    // `Balance::carnivore_diet` turns them all on at their tested values
    // and `carnivore_diet_off` off. The tunables are
    // global scales: how far each rule applies
    // to an animal comes from its genes (`world::apply::meat_reliance` from
    // carnivory: none up to 0.45, in full from 0.7; and the genes named
    // below), so selection can tune the behaviours. Carrion and the slower
    // rot feed every meat eater.
    /// Share of a carcass's rot prevented: tile meat loses `0.05 × (1 −
    /// carcass_keep)` an hour above 15 °C and `0.02 × (1 − carcass_keep)`
    /// below (0: 70% and 38% a day, as before; 0.75: 26% and 11%), so one
    /// kill feeds the killer, its family and other scavengers for days.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v26"))]
    pub carcass_keep: f32,
    /// Gorging: a carnivore banks meat beyond its hunger as fat (up to this
    /// many hunger points times its meat reliance times `0.5 + fat stores`)
    /// in any season, not only before a dormancy, and keeps eating at a
    /// carcass it knows of while it has room. Fat pays for hunger above 50
    /// as for any animal, so a kill tides it over the days without one (and
    /// the winter), and a provider feeds its family from it (`provisioning`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v26"))]
    pub carnivore_fat: f32,
    /// Carcass scent (tiles, times meat reliance, times 0.5 + nose): a hungry
    /// obligate carnivore with no prey in sight goes to the best carcass
    /// within this range before roaming (a flexible hunter, carnivory 0.45
    /// to 0.7, when off a good patch of plants), remembers its own last kill
    /// and returns to it, and a pregnant one goes to her mate's last kill.
    /// With it a roaming hunter also drinks where it passes water. Before,
    /// it found carcasses only by sampling eight random tiles and neither
    /// drank nor ate while roaming.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v26"))]
    pub carcass_scent: f32,
    /// Carrion: share of a body's meat (`meat_per_kg` per kg) left as a
    /// carcass by a death other than predation or drowning (half for a
    /// starved one). Before, only kills left meat; winter deaths are now
    /// the scavengers' winter food.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v26"))]
    pub carrion: f32,
    /// Provisioning: a fed carnivore (hunger below 50, fat 5 or more, not
    /// pregnant) takes food to its pregnant mate or dependent young in sight
    /// when they are hungry (above 40), feeding them from its fat at this
    /// times twice its care (`world::apply::provision_care`: the higher of
    /// pack instinct and group tolerance) times the nursing rate (300 hunger
    /// points a day); by its pack instinct it also feeds the other pregnant
    /// females and dependent young of its kind (helpers at the den).
    /// Mothers, who can't court and so never nibbled on the way, starved
    /// first.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v26"))]
    pub provisioning: f32,
    /// Breeding territory (tiles, times meat reliance, times `1 + aggression
    /// − group tolerance` clamped to 0.25–2): a carnivore female doesn't
    /// breed, and isn't courted, while a pregnant female or an unweaned young
    /// of her own lineage is within this range, so each predator is limited
    /// by its own numbers (one litter at a time per territory) and ends a
    /// boom before the prey run out. (Before version 27 she only stopped
    /// courting, and males still bred her: the territory leaked.)
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v26"))]
    pub breeding_territory: f32,
    // The predator brakes (version 27; *Predators in their niches* in the
    // README's Balance section), on by default from version 31. Global scales and switches:
    // how strongly each animal applies them comes from its genes. Off at 0,
    // what snapshots before version 27 read (`serde_util::before_v27`).
    /// Prey switching (a type-III functional response): a hunter gives up on
    /// a prey species that has grown rare, overlooking it for the day with
    /// chance `min(1, 2 × pickiness) × (1 − r²)`, where `r` is the species'
    /// head count over `prey_switching` times its founding count (capped at
    /// 1). Hunting effort follows the common prey, so a predator can't hunt
    /// a rare one to the last animal, and the pickiness gene sets how
    /// readily each hunter switches.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v27"))]
    pub prey_switching: f32,
    /// Litters follow the mother's condition: a mother carries
    /// `1 − condition_litter × flex × (1 − nourished)` of her litter to term
    /// (on top of the condition factor every birth has), where `nourished`
    /// is `(90 − hunger + fat) / 60` (0–1: fat stores, laid down by the fat
    /// stores gene, carry a hungry mother through) and `flex` is `min(1, 2 ×
    /// litter fraction)` (the litter size gene: a big litter can shrink, a
    /// single young can't). The pregnancy's food cost follows the same share,
    /// so a starving mother resorbs part of her litter rather than starve.
    /// Predator numbers follow their food.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v27"))]
    pub condition_litter: f32,
    /// Horns (`morph_extra`, Phase 3 morphology): scales their defense,
    /// fight-back and display and their mass, upkeep and snagging in dense
    /// vegetation. 0 turns horns cosmetic.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v29"))]
    pub morph_horns: f32,
    /// Wing shape (`morph_extra`): narrow wings fly faster and further but
    /// turn worse and snag in forest; broad wings take off easier. 0 turns
    /// wing aspect cosmetic.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v29"))]
    pub morph_wing_shape: f32,
    /// Coat patterns (`morph_extra`): a pattern adds stealth on matching
    /// ground (spots in forest, stripes in grass and marsh, mottling on rock
    /// and tundra) and costs a little where it stands out. 0 turns patterns
    /// cosmetic.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v29"))]
    pub morph_pattern: f32,
    /// Strike fixes (version 30; on at 1, the default): a hunter killed in
    /// its own strike (by its prey's toxins or by prey that fought back)
    /// does nothing more that turn, where before it could still bring the
    /// prey down and eat; and a kill is eaten once in the hunter's turn,
    /// where before the hunter ate a full helping at the kill and another
    /// right after. 0 is the old behaviour, what snapshots before version
    /// 30 read (`serde_util::before_v30`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v30"))]
    pub strike_fixes: f32,
    /// Staggered decisions (an approximation for speed; 2 by default: briefly
    /// 1 after the food values cost the voles on fresh seeds, then 2 again
    /// once a kept intent also required no danger in sight, README
    /// *Performance*): at 2 or more, a calm organism decides afresh only every
    /// `decide_interval` ticks (staggered by id) and keeps its last intent
    /// in between, its chase target's position updated
    /// (`World::reused_intent`). Every organism decides on the day's first
    /// tick, and so does every hunter and any organism that is possessed, fleeing, courting,
    /// provisioning or defending, that saw a threat, that has an armed
    /// organism able to threaten it or a sentry's warning within sight
    /// (`SpatialGrid::danger_near`), or whose target died.
    /// 0 or 1: every organism decides every tick, what snapshots before
    /// version 32 read (`serde_util::before_v32`). About 13–15% off a tick
    /// in crowds; the paired comparison and predator survival are in the
    /// README (Performance); about 8% with the danger check.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v32"))]
    pub decide_interval: f32,
    // Food values (version 31; *Food by its value* in the README's Balance
    // section). Off at 0, what snapshots before version 31 read
    // (`serde_util::before_v31`): every food worth the same per unit. On by
    // default (`Balance::food_values`).
    /// Extra food value of meat over leafy plants: a unit of meat (carcass,
    /// eggs) is worth `1 + meat_energy` units of leaves before digestion
    /// (meat is 80–90% digestible, leaves 20–60%). Digestion still follows
    /// the Carnivory gene (`Traits::meat_efficiency`). With the richer food,
    /// `carnivore_hunger_discount` (a slower hunger standing in for it) can
    /// go.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v31"))]
    pub meat_energy: f32,
    /// The seasonal fruit and seed layer: scale on each tile's crop of
    /// fruit, seeds and nuts (`terrain::fruit_capacity`: most in moist
    /// forest, some in marsh, grassland seeds and tundra berries), which
    /// ripens from late summer, peaks in early autumn and rots or is eaten
    /// through the winter (`terrain::fruit_season`). 0: no fruit layer.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v31"))]
    pub fruit_crop: f32,
    /// Extra food value of fruit and seeds over leaves: a unit is worth
    /// `1 + fruit_energy` before digestion. Fruit is digested well by
    /// herbivores, omnivores and flexible hunters alike, poorly only by
    /// full carnivores (`stats::fruit_efficiency`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v31"))]
    pub fruit_energy: f32,
    /// The invertebrate layer: scale on each tile's insects, worms and grubs
    /// (`terrain::insect_capacity`: most in marsh and moist ground), out
    /// whenever the ground is above freezing (none in winter, a spring
    /// flush) and back within days of being eaten.
    /// Small meat, at the value of meat (`meat_energy`), digested as far as
    /// the gut is an omnivore's or a flexible hunter's
    /// (`Traits::insect_efficiency`), gathered at half the pace of other
    /// food (small items), on any errand a meat eater makes, roaming for
    /// prey included. 0: no invertebrates.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v31"))]
    pub insect_crop: f32,
    /// How sharply prey switching (`prey_switching`) spares a rare prey: a
    /// hunter still hunts a prey species at `r` of its threshold with chance
    /// `r^switching_power` (times its pickiness's share), so at 4 a species
    /// at half its threshold is hunted at a sixteenth of the rate, at 2 a
    /// quarter. Hunting effort then collapses on a declining prey well
    /// before it is gone, even when an abundant other prey keeps many
    /// hunters fed (apparent competition). 0 reads as 2, the square of
    /// before version 31.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v31"))]
    pub switching_power: f32,
    // Playtest 1 ecology fixes (version 34; *Playtest 1: ecology fixes* in
    // the README's Balance section). Off at 0, what snapshots before version
    // 34 read (`serde_util::before_v34`); on by default.
    /// Desperate animals take risks: a burrower that would go to ground from
    /// a threat (a free escape, so it hid even while dying) does so less as
    /// its hunger or thirst passes 0.7, by `desperate_risk ×
    /// smoothstep(0.7, 1, max(hunger, thirst))`, and goes to drink or eat
    /// instead. Before, parched and starving burrowers sat in their burrows
    /// in sight of water until they died ("rest" at hunger or thirst 100).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v34"))]
    pub desperate_risk: f32,
    /// Founding by food: a lineage founded into a running world (after its
    /// first tick) picks homes by the food still standing around them (the
    /// home's plant score times the share of the vegetation left within 4
    /// tiles), not by what the ground could grow, so founders don't land on
    /// grazed-out ground; and a player's founding of more than
    /// `World::FOUNDING_GROUP` (48) is placed in groups of at most that
    /// many, each at its own home, spread over enough pasture for their
    /// number. Before, a player's founders all landed at one home however
    /// many, and 80–140 founders lost most of their number in weeks.
    /// Founding at world creation is unchanged.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v34"))]
    pub founding_food: f32,
    // Lineage crowding: the soft cap (version 35; *Soft lineage cap* in the
    // README). Off at 0, what snapshots before version 35 read
    // (`serde_util::before_v35`): a player's lineage stops being born into
    // at `WorldConfig::lineage_cap`, the old hard wall.
    /// Steepness of a player lineage's crowding: its litters are multiplied
    /// by `1 / (1 + x^lineage_crowding)`, where `x` is the lineage's
    /// crowding count (adults, plus juveniles at `crowding_juveniles`) over
    /// `WorldConfig::lineage_cap`, now the soft cap. At the soft cap a
    /// lineage breeds at half its pace, at 1.5× it at a sixth. Wild lineages
    /// keep their hard caps. 0: off (the hard cap at `lineage_cap`).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v35"))]
    pub lineage_crowding: f32,
    /// The hard safety ceiling on a player lineage while crowding is on, as
    /// a multiple of `lineage_cap` (every member counts, juveniles too, as
    /// the old cap counted them). Values below 1 read as 1.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v35"))]
    pub crowding_ceiling: f32,
    /// How much a juvenile counts toward its lineage's crowding, against 1
    /// for an adult: the young eat, but neither breed nor score.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v35"))]
    pub crowding_juveniles: f32,
    /// Competition within a crowded lineage: its members' hunger rises by
    /// this times how far the crowding count is past the soft cap
    /// (`x - 1`, none below it), so food, not the cap, is what stops it.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v35"))]
    pub crowding_hunger: f32,
    // Predators against players (version 36; *Predators against players*
    // in the README). Off at 0, what snapshots before version 36 read
    // (`serde_util::before_v36`).
    /// Pack hunts: a pack hunter takes prey above the solo limit
    /// (`decide::max_prey_mass`) with pack-mates around, and strikes harder
    /// for each pack-mate hunting the same prey beside it
    /// (`decide::pack_scale`, `World::pack_allies`). 0: off, every hunter
    /// sizes prey and strikes as if alone. The default, 0.25, keeps the
    /// wild balance (at 1, steppe antelope and herd grazers ran at half
    /// their numbers in the balance sweep).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36"))]
    pub pack_hunt: f32,
    /// Territory defence against larger intruders: territory holders that
    /// outnumber a stranger stronger than one of them chase it too, and a
    /// defender's scare counts its kin around it (`Action::Defend`). 0: off,
    /// a stronger stranger is left alone.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36"))]
    pub territory_harass: f32,
    /// Days after a player lineage is founded into a running world (after
    /// tick 0) during which other players can't order attacks on it
    /// (`World::possess_attack`) and their animals don't hunt it, so a
    /// newcomer's founding site can't be camped. Wild predators still hunt
    /// it. 0: no grace.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v36"))]
    pub attack_grace: f32,
    // Competitive balance after playtest 2 (version 43; *Playtest 2:
    // competitive balance* in the README). Off at 0, what snapshots before
    // version 43 read (`serde_util::before_v43`).
    /// How much less birth mode counts in the body-plan gap for fertility
    /// (`mating::fertility_gap`): the birth-mode difference is weighted by
    /// `1 − egg_hybrid`. At 0.2, adjacent birth modes (0.35 apart) are 0.28
    /// apart, so an egg layer is a little fertile with close live bearers
    /// instead of sterile with all of them (playtest 2: an egg-laying
    /// lineage was never diluted). Kinship and species keep the full gap.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v43"))]
    pub egg_hybrid: f32,
    /// Spread of the age at which animals die of old age, as a standard
    /// deviation over the lifespan (`old_age_hazard`): the hazard starts
    /// earlier and rises more slowly, so a boom cohort dies over months
    /// instead of days. 0: the old rule (from 80% of the lifespan, nearly
    /// all within a few days of 84%).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v43"))]
    pub age_spread: f32,
    /// Chance that a winter sleeper dies over a whole winter's sleep (den
    /// collapse, cold snaps, disease), 1.5× in its first year and up to
    /// 1.5× again as its fat runs out, brumators too, founders free in
    /// their first winter (`Balance::winter_toll` on, the default; with it
    /// off, the version 43 rule: this chance per season asleep, 2× and 2×,
    /// warm hibernators only). 0: winter sleep is safe.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v43"))]
    pub dormancy_toll: f32,
    /// Days after waking from a winter sleep before a hibernator or
    /// brumator can breed (it rebuilds condition first), so sleeping
    /// through winter costs part of the breeding year. 0: none.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v43"))]
    pub emergence_lag: f32,
    // Playtest 2 fixes (version 44; *Playtest 2: predators, toxins and
    // drowning* in the README). Off at 0, what snapshots before version 44
    // read (`serde_util::before_v44`).
    /// Drinking first when parched beside water: past thirst 0.85 with
    /// water within 6 tiles, drinking gains up to `1.5 × drink_priority`
    /// (all of it at 0.95), more than hunger, fattening and courting give,
    /// so a starving, parched animal next to a lake drinks rather than
    /// grazes or scavenges (playtest 3). 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44"))]
    pub drink_priority: f32,
    /// Possessed animals drink: one with water within 8 tiles breaks off its
    /// order to drink at thirst 60 (instead of `possess_urgency`), and one at
    /// water drinks from thirst 10. Only possessed animals. 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44"))]
    pub possess_drink: f32,
    /// Learned avoidance of toxic prey: each time a predator is poisoned by
    /// its prey, its lineage's lesson about the prey's lineage grows by
    /// `0.15 × toxic_learning × (1 + 2 × coloration)` (warning colours
    /// teach about three times faster than a drab coat), up to 1, and fades
    /// with a 60-day time constant (`World::toxic_lesson`). Hunters pass
    /// prey up with the lesson's probability as far as they can recognise
    /// it, `0.3 + 0.7 × coloration` (a drab coat never buys full
    /// protection), and a lineage that knows a toxin takes less harm biting
    /// it, `1 − 0.7 × lesson` (`world::toxic`). 0: off (only the
    /// bright-prey aversion of a poisoned individual).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44"))]
    pub toxic_learning: f32,
    /// Dry ground for sleeping, founding and hunting: no dormancy on ice
    /// over water deep enough to drown the sleeper at the melt (a sleeper on
    /// such ice wakes), founders land only on dry ground away from deep
    /// water (`World::found_lineage_grouped`), and a hunter that swims
    /// poorly (below 0.5) doesn't chase prey standing in water deeper than
    /// it can wade. 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44"))]
    pub dry_sites: f32,
    /// Obligate carnivores' energetics: a hungry obligate carnivore's
    /// hunger rises up to `0.3 × carnivore_energetics` slower (none at
    /// carnivory 0.7 or below, all of it at 0.95 and above; none at hunger
    /// 40 or below, all of it at 80 and above: a lean economy in hard
    /// times, so a fed pack burns as before). Playtest 2's player
    /// carnivores needed a fifth of their weight in prey a day and starved.
    /// 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44"))]
    pub carnivore_energetics: f32,
    /// Player-led pack hunts on big prey: in a hunt a player leads (an
    /// attack order and the pack-mates who join it) the prey mass limit
    /// grows by `led_pack` per pack-mate around, up to 8 (`decide::LED_PACK_MAX`),
    /// times pack instinct, and up to 8 pack-mates at the prey strike
    /// beside the leader: a full pack takes prey several times one hunter's
    /// weight. 0: off (the wild rule, at most 4 pack-mates).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v44"))]
    pub led_pack: f32,
    // Competitive balance after playtest 3 (version 46; *Playtest 3:
    // balance* in the README). Off at 0, what snapshots before version 46
    // read (`serde_util::before_v46`).
    /// How much less an eye-count step counts in the body-plan gap for
    /// fertility (`morph::morph_fertility_gap`): weighted `1 − eye_hybrid`.
    /// At 0.6, 2 → 5 eyes is 0.12 apart (fully fertile) instead of 0.30
    /// (sterile with every two-eyed animal, playtest 3's 3-point isolation).
    /// Leg steps and kinship keep the full gap.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v46"))]
    pub eye_hybrid: f32,
    /// The winter toll as documented (`seasonal::dormancy_toll_rate`): on
    /// (1), `dormancy_toll` is the chance of dying over a whole winter's
    /// sleep (spread over two seasons, since sleepers bed down in autumn),
    /// 1.5× in an animal's first year and up to 1.5× again with no fat;
    /// brumators pay it like hibernators; founders sleep their first winter
    /// free (the winter provision); a sleeper beds down only once it has
    /// drunk (thirst under 30), and the deaths are counted as
    /// `DeathCause::WinterSleep`. Off (0): the version 43 toll (a season's
    /// toll per season asleep, 2× first year, 2× with no fat, warm
    /// hibernators only: ~1% a day, 25–30% a winter in playtest 3).
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v46"))]
    pub winter_toll: f32,
    /// Days over which a warm hibernator's spring waking is spread (each
    /// animal its own day, from its id), so a lineage doesn't wake at once
    /// on the same pasture and starve. 0: all wake on the first spring day.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v46"))]
    pub wake_spread: f32,
    // Competitive balance after playtest 4 (version 49; *Playtest 4:
    // balance* in the README). Off at 0, what snapshots before version 49
    // read (`serde_util::before_v49`).
    /// Winter rest for cold-blooded animals: in winter their breeding drive
    /// drains instead of building, even when they wake to bask on mild days,
    /// and a brumator awake in winter pays the winter toll as if asleep
    /// (out in the cold). Playtest 4: roused brumators kept breeding through
    /// winters (370 → 571, 1,211 → 1,895) with no deaths but old age. 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v49"))]
    pub winter_rest: f32,
    /// Strong learned avoidance of toxic prey: once a hunter's lineage has
    /// learned a toxin (`World::toxic_lesson`), it recognises the prey
    /// `0.7 + 0.3 × coloration` of the time (drab 70%, bright 90%+) instead of
    /// `0.3 + 0.7 × coloration`. Playtest 4: 65% of hunters still ate known
    /// toxic prey and died. 0: the version 44 recognition.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v49"))]
    pub toxic_avoidance: f32,
    /// Eggs keep warm while their brooding parent is alive but off the
    /// nest: an attended clutch (its guardian alive, within 12 tiles) chills
    /// at `1 − 0.8 × egg_attendance` of the rate of a lost one (0.6 at the
    /// default 0.5; at 1 the egg-laying hibernator took 17 of 40 titles in
    /// the dominance replay). Playtest 4: incubated clutches chilled more
    /// than they hatched (351 chilled vs 304 born) because parents left to
    /// eat and drink. 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v49"))]
    pub egg_attendance: f32,
    /// Player species identity: a cluster most of whose adults belong to
    /// one player lineage that wasn't among a wild species' founders isn't
    /// filed under that initial wild species; it is (or becomes) the player
    /// lineage's own species. Playtest 4: every starter lineage ended up
    /// "Steppe antelope" (no founding credit, no naming). 0: off.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v49"))]
    pub player_species: f32,
    /// Most offspring raised an adult counts for in the weighted legacy
    /// share (`LegacySnapshot::weighted`): alleles × min(raised, cap). The
    /// weight was the lifetime count, so an old, prolific cohort dominated
    /// the weighted share and its die-off swung it by 7–13 points within 60
    /// days (playtest 4). 0: uncapped.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v49"))]
    pub raised_cap: f32,
    // Playtest 3 founding and drinking fixes (version 47). Off at 0, what
    // snapshots before version 47 read (`serde_util::before_v47`).
    /// Thirsty animals head for a shore spot they can walk to and drink
    /// from (`World::reachable_drink_spot`) before the nearest water: a lake
    /// below a steep bank no longer holds non-climbers at the top of it
    /// until they die of thirst. 0: the nearest water, reachable or not.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v47"))]
    pub reachable_water: f32,
    /// The founding search (`World::pick_home`) judges the spot as it is
    /// now: water the design can reach and that will last a drought, snow
    /// and frozen water in the cold season, plants standing for the founders,
    /// map edges in the cold season, and the homes where the player's
    /// earlier foundings died (avoided). 0: the old search.
    #[cfg_attr(feature = "serde", serde(with = "crate::serde_util::before_v47"))]
    pub founding_conditions: f32,
}

impl Balance {
    /// Whether the legacy tally counts morphology alleles
    /// (`morph_legacy` above 0.5).
    pub fn morph_in_legacy(&self) -> bool {
        self.morph_legacy > 0.5
    }

    /// These rules with every morphology mechanic off, as before they were
    /// on by default (snapshot version 24): the four body plan tunables,
    /// directional vision, step mutation and morphology alleles in the
    /// legacy tally. Default-designed worlds then run bit for bit as before
    /// the morphology work (`vision_scan` is inert with vision off).
    pub fn morphology_off(mut self) -> Self {
        self.morph_effects = 0.0;
        self.morph_tail_drag = 0.0;
        self.morph_tail_chill = 0.0;
        self.morph_long_legs = 0.0;
        self.directional_vision = 0.0;
        self.morph_step_rate = 0.0;
        self.morph_legacy = 0.0;
        self.morph_horns = 0.0;
        self.morph_wing_shape = 0.0;
        self.morph_pattern = 0.0;
        self
    }

    /// These rules with the carnivore-diet rules and the predator brakes on
    /// at their tested values (the defaults from version 31: *Predators that live on meat*
    /// and *Predators in their niches* in the README's Balance section),
    /// tuned with the founder designs of `presets::niche_world`: carcasses
    /// rot a quarter as fast, carnivores gorge into fat, smell and remember
    /// carcasses, providers feed their family, natural deaths leave carrion,
    /// one litter at a time holds a breeding territory (12 tiles at the
    /// neutral temper), hunters give up on rare prey, and litters follow the
    /// mother's condition.
    pub fn carnivore_diet(mut self) -> Self {
        self.carcass_keep = 0.75;
        self.carnivore_fat = 60.0;
        self.carcass_scent = 8.0;
        self.carrion = 1.0;
        self.provisioning = 1.0;
        self.breeding_territory = 12.0;
        self.prey_switching = 1.0;
        self.condition_litter = 1.0;
        self.switching_power = 4.0;
        self
    }

    /// These rules with the carnivore-diet rules and the predator brakes
    /// (`prey_switching`, `condition_litter`) off: the rules of before
    /// version 31 (with `food_values_off`).
    pub fn carnivore_diet_off(mut self) -> Self {
        self.carcass_keep = 0.0;
        self.carnivore_fat = 0.0;
        self.carcass_scent = 0.0;
        self.carrion = 0.0;
        self.provisioning = 0.0;
        self.breeding_territory = 0.0;
        self.prey_switching = 0.0;
        self.condition_litter = 0.0;
        self.switching_power = 0.0;
        self
    }

    /// These rules with the food values on at their tested values (*Food
    /// by its value* in the README's Balance section): meat richer than
    /// leaves in place of the carnivores' slower hunger, and the seasonal
    /// fruit and seed and invertebrate layers (the defaults from version 31).
    pub fn food_values(mut self) -> Self {
        self.meat_energy = 0.8;
        self.fruit_crop = 1.0;
        self.fruit_energy = 0.5;
        self.insect_crop = 1.0;
        self.carnivore_hunger_discount = 0.0;
        self
    }

    /// These rules with the food values off, as before version 31: every
    /// food worth the same per unit, no fruit layer, and full carnivores'
    /// hunger 44% slower in its place.
    pub fn food_values_off(mut self) -> Self {
        self.meat_energy = 0.0;
        self.fruit_crop = 0.0;
        self.fruit_energy = 0.0;
        self.insect_crop = 0.0;
        self.carnivore_hunger_discount = 0.44;
        self
    }

    /// Food value of a unit of meat relative to leaves (`meat_energy`).
    pub fn meat_value(&self) -> f32 {
        1.0 + self.meat_energy
    }

    /// Food value of a unit of fruit relative to leaves (`fruit_energy`).
    pub fn fruit_value(&self) -> f32 {
        1.0 + self.fruit_energy
    }
}

/// How a shielded home range softens predation (`World::set_home_ranges`).
/// Inert until the caller shields a range: the simulation never does.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HomeRangeBalance {
    /// Factor on the kill chance of a strike against a resident of a range
    /// at full strength (and on the exposure of a resident cohort to
    /// hunting cohorts). 1 turns the home range off.
    pub predation: f32,
    /// Residents shielded in full: above this many, the same softening is
    /// spread over all of them (each gets `residents / count` of it), so a
    /// big herd gains little more than a small one.
    pub residents: f32,
    /// Lowest kill-chance factor when the home range and the surviving-core
    /// protection both apply (their product, floored here). Either rule
    /// alone is never raised by it.
    pub stack_floor: f32,
}

impl Default for HomeRangeBalance {
    fn default() -> Self {
        Self { predation: 0.5, residents: 20.0, stack_floor: 0.2 }
    }
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
            carnivore_hunger_discount: 0.0,
            generalist_tax: 0.5,
            cold_heat_burn: 1.0,
            size_burn: 0.25,
            pregnancy_cost: 0.74,
            meat_per_kg: 5.75,
            flee_weight: 1.33,
            ambush: 0.4,
            crowding: 0.03,
            toxin_damage: 48.0,
            larval_competition: 9.5,
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
            threat_caution: 1.0,
            alert_guard: 0.5,
            pasture_search: 6.0,
            search_image: 0.7,
            cold_gestation: 1.0,
            nest_predation: 0.06,
            tameness: 0.4,
            migration_onset: 0.2,
            home_range: HomeRangeBalance::default(),
            lactation: 0.0,
            warm_upkeep: 0.0,
            fang_grazing: 0.5,
            scale_cover: 0.5,
            morph_effects: 1.0,
            morph_tail_drag: 1.0,
            morph_tail_chill: 1.0,
            morph_long_legs: 1.0,
            directional_vision: 1.0,
            vision_scan: 1.0,
            morph_step_rate: crate::morph::RECOMMENDED_STEP_RATE,
            morph_legacy: 1.0,
            roaming_forage: 100.0,
            morph_horns: 1.0,
            morph_wing_shape: 1.0,
            morph_pattern: 1.0,
            strike_fixes: 1.0,
            decide_interval: 2.0,
            carcass_keep: 0.75,
            carnivore_fat: 60.0,
            carcass_scent: 8.0,
            carrion: 1.0,
            provisioning: 1.0,
            breeding_territory: 12.0,
            prey_switching: 1.0,
            condition_litter: 1.0,
            meat_energy: 0.8,
            fruit_crop: 1.0,
            fruit_energy: 0.5,
            insect_crop: 1.0,
            switching_power: 4.0,
            desperate_risk: 1.0,
            founding_food: 1.0,
            lineage_crowding: 6.0,
            crowding_ceiling: 1.4,
            crowding_juveniles: 0.25,
            crowding_hunger: 0.6,
            pack_hunt: 0.25,
            territory_harass: 1.0,
            attack_grace: 20.0,
            egg_hybrid: 0.2,
            age_spread: 0.12,
            dormancy_toll: 0.10,
            emergence_lag: 6.0,
            drink_priority: 1.0,
            possess_drink: 1.0,
            toxic_learning: 1.0,
            dry_sites: 1.0,
            carnivore_energetics: 1.0,
            led_pack: 0.5,
            eye_hybrid: 0.6,
            winter_toll: 1.0,
            wake_spread: 10.0,
            winter_rest: 1.0,
            toxic_avoidance: 1.0,
            egg_attendance: 0.5,
            player_species: 1.0,
            raised_cap: 4.0,
            reachable_water: 1.0,
            founding_conditions: 1.0,
        }
    }
}

impl Balance {
    pub const FIELD_COUNT: usize = 149;

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
            ("threat_caution", &mut self.threat_caution),
            ("alert_guard", &mut self.alert_guard),
            ("pasture_search", &mut self.pasture_search),
            ("search_image", &mut self.search_image),
            ("cold_gestation", &mut self.cold_gestation),
            ("nest_predation", &mut self.nest_predation),
            ("tameness", &mut self.tameness),
            ("migration_onset", &mut self.migration_onset),
            ("home_range_predation", &mut self.home_range.predation),
            ("home_range_residents", &mut self.home_range.residents),
            ("offline_stack_floor", &mut self.home_range.stack_floor),
            ("lactation", &mut self.lactation),
            ("warm_upkeep", &mut self.warm_upkeep),
            ("fang_grazing", &mut self.fang_grazing),
            ("scale_cover", &mut self.scale_cover),
            ("morph_effects", &mut self.morph_effects),
            ("morph_tail_drag", &mut self.morph_tail_drag),
            ("morph_tail_chill", &mut self.morph_tail_chill),
            ("morph_long_legs", &mut self.morph_long_legs),
            ("directional_vision", &mut self.directional_vision),
            ("vision_scan", &mut self.vision_scan),
            ("morph_step_rate", &mut self.morph_step_rate),
            ("morph_legacy", &mut self.morph_legacy),
            ("roaming_forage", &mut self.roaming_forage),
            ("carcass_keep", &mut self.carcass_keep),
            ("carnivore_fat", &mut self.carnivore_fat),
            ("carcass_scent", &mut self.carcass_scent),
            ("carrion", &mut self.carrion),
            ("provisioning", &mut self.provisioning),
            ("breeding_territory", &mut self.breeding_territory),
            ("prey_switching", &mut self.prey_switching),
            ("condition_litter", &mut self.condition_litter),
            ("morph_horns", &mut self.morph_horns),
            ("morph_wing_shape", &mut self.morph_wing_shape),
            ("morph_pattern", &mut self.morph_pattern),
            ("strike_fixes", &mut self.strike_fixes),
            ("decide_interval", &mut self.decide_interval),
            ("meat_energy", &mut self.meat_energy),
            ("fruit_crop", &mut self.fruit_crop),
            ("fruit_energy", &mut self.fruit_energy),
            ("insect_crop", &mut self.insect_crop),
            ("switching_power", &mut self.switching_power),
            ("desperate_risk", &mut self.desperate_risk),
            ("founding_food", &mut self.founding_food),
            ("lineage_crowding", &mut self.lineage_crowding),
            ("crowding_ceiling", &mut self.crowding_ceiling),
            ("crowding_juveniles", &mut self.crowding_juveniles),
            ("crowding_hunger", &mut self.crowding_hunger),
            ("pack_hunt", &mut self.pack_hunt),
            ("territory_harass", &mut self.territory_harass),
            ("attack_grace", &mut self.attack_grace),
            ("egg_hybrid", &mut self.egg_hybrid),
            ("age_spread", &mut self.age_spread),
            ("dormancy_toll", &mut self.dormancy_toll),
            ("emergence_lag", &mut self.emergence_lag),
            ("drink_priority", &mut self.drink_priority),
            ("possess_drink", &mut self.possess_drink),
            ("toxic_learning", &mut self.toxic_learning),
            ("dry_sites", &mut self.dry_sites),
            ("carnivore_energetics", &mut self.carnivore_energetics),
            ("led_pack", &mut self.led_pack),
            ("eye_hybrid", &mut self.eye_hybrid),
            ("winter_toll", &mut self.winter_toll),
            ("wake_spread", &mut self.wake_spread),
            ("winter_rest", &mut self.winter_rest),
            ("toxic_avoidance", &mut self.toxic_avoidance),
            ("egg_attendance", &mut self.egg_attendance),
            ("player_species", &mut self.player_species),
            ("raised_cap", &mut self.raised_cap),
            ("reachable_water", &mut self.reachable_water),
            ("founding_conditions", &mut self.founding_conditions),
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
            possess_substeps: 4,
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
