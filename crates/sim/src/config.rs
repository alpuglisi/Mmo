//! World configuration. Rates elsewhere are expressed per simulated day and
//! scaled by `dt_days()`, so changing the tick rate keeps the balance.

use crate::genome::MutationParams;

#[derive(Clone, Debug)]
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
    /// (no hunting, no fleeing, grouping).
    pub species_distance: f32,
    /// Genetic distance above which organisms will not court each other.
    pub mate_distance: f32,
    /// `k` in the directed-mating override cost `k * max(0, mismatch - 1)^2`.
    pub override_k: f32,
    /// Hard cap on living organisms per player.
    pub lineage_cap: usize,
    /// Hard cap on the wild ecosystem (all wild species together).
    pub wild_cap: usize,
    pub mating_slots_per_season: u32,
    /// Upper bound on perception radius in tiles (also bounds neighbor queries).
    pub max_perception: f32,
    pub balance: Balance,
}

/// Tunable rates that shape the ecosystem. Kept together so the sweep tool
/// (`evo-sim sweep`) can search over them.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// a daily rate of `(kin nearby / larval_competition)^4` until they
    /// mature, so unpredated fast breeders settle below the hard cap. Uses the
    /// same kin count as `crowding`. 0 disables.
    pub larval_competition: f32,
    /// Daily background death hazard of adults at pace of life 1
    /// (`Traits::pace_of_life`: small bodies with big litters live fast and
    /// die young), so unpredated fast breeders don't pile up adults across
    /// years. Toxic frogs get about 0.6% a day, voles 0.2%, predators and
    /// grazers almost nothing. 0 disables.
    pub adult_mortality: f32,
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
            pregnancy_cost: 0.74,
            meat_per_kg: 5.75,
            flee_weight: 1.33,
            ambush: 0.4,
            crowding: 0.03,
            toxin_damage: 48.0,
            larval_competition: 18.0,
            adult_mortality: 0.011,
        }
    }
}

impl Balance {
    pub const FIELD_COUNT: usize = 19;

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
            ("pregnancy_cost", &mut self.pregnancy_cost),
            ("meat_per_kg", &mut self.meat_per_kg),
            ("flee_weight", &mut self.flee_weight),
            ("ambush", &mut self.ambush),
            ("crowding", &mut self.crowding),
            ("toxin_damage", &mut self.toxin_damage),
            ("larval_competition", &mut self.larval_competition),
            ("adult_mortality", &mut self.adult_mortality),
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
            mate_distance: 0.3,
            override_k: 1.0,
            lineage_cap: 1500,
            wild_cap: 5000,
            mating_slots_per_season: 6,
            max_perception: 8.0,
            balance: Balance::default(),
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
}
