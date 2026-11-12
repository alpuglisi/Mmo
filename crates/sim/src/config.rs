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
    /// Food units left by a carcass per kg of body mass.
    pub meat_per_kg: f32,
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
            meat_per_kg: 6.0,
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
