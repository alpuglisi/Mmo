//! Eras, catastrophes and secondary achievements ("Scoring: genetic legacy",
//! supporting rules: seasonal leaderboards at the end of each era, possibly
//! right after a catastrophe; prestige-only achievements).
//!
//! The world keeps one [`Chronicle`] (`World::chronicle`), updated in the
//! daily routine (`world::chronicle`):
//!
//! - **Eras** last `ScoringConfig::era_years` years. When one ends, its final
//!   legacy ranking (the day-boundary snapshot) and the niche leaders are
//!   recorded as an [`EraRecord`].
//! - **Catastrophes** ([`CatastropheKind`]) are built on existing systems: a
//!   severe winter lowers every tile's temperature, a megadrought or a flood
//!   year biases the regional rain (soil wetness, lakes, marsh and rivers
//!   follow), an epidemic infects a share of the population with the
//!   mating-transmitted disease. With `ScoringConfig::catastrophes` on, each
//!   year from `catastrophe_first_year` (default 1, so the first year is
//!   always quiet) has a `catastrophe_chance` (default 0.4) of one; it starts
//!   in the season it belongs to. `World::schedule_catastrophe` forces one.
//!   Random ones are off by default so balance runs (120 days) are untouched.
//! - **Achievements** (prestige only, never part of the score):
//!   species founded (a speciation credited by `species`), lineage longevity
//!   (a player lineage alive for each whole year since founding), niche
//!   dominance (the largest allele share among a diet or biome niche's
//!   breeding adults at an era's end, at least [`NICHE_MIN_SHARE`] and ahead
//!   of the wild ecosystem), surviving a catastrophe (a lineage alive at its
//!   start and still alive at its end).

use std::collections::BTreeMap;

use crate::genome::PlayerId;
use crate::species::SpeciesTracker;

/// Smallest allele share in a niche that earns niche dominance.
pub const NICHE_MIN_SHARE: f64 = 0.25;
/// Fewest breeding adults a niche needs for a dominance award.
pub const NICHE_MIN_ADULTS: f64 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CatastropheKind {
    /// A winter far colder than normal (every tile `-10 °C × severity`).
    SevereWinter,
    /// Rain fails for a summer and half an autumn: soil dries, lakes, marsh
    /// and streams shrink, plants stop growing.
    Megadrought,
    /// A virulent strain: a share of the susceptible population falls sick
    /// at once and more every day (`world::disease`).
    Epidemic,
    /// A spring of relentless rain: soaked soil, swollen lakes, marsh and
    /// rivers that block weak swimmers.
    FloodYear,
}

impl CatastropheKind {
    pub const ALL: [CatastropheKind; 4] =
        [CatastropheKind::SevereWinter, CatastropheKind::Megadrought, CatastropheKind::Epidemic, CatastropheKind::FloodYear];

    pub fn label(self) -> &'static str {
        match self {
            CatastropheKind::SevereWinter => "severe_winter",
            CatastropheKind::Megadrought => "megadrought",
            CatastropheKind::Epidemic => "epidemic",
            CatastropheKind::FloodYear => "flood_year",
        }
    }

    pub fn from_label(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.label() == s)
    }

    /// Season it starts in (0 spring … 3 winter) and its length in seasons.
    pub fn timing(self) -> (u32, f32) {
        match self {
            CatastropheKind::SevereWinter => (3, 1.0),
            CatastropheKind::Megadrought => (1, 1.5),
            CatastropheKind::Epidemic => (0, 0.5),
            CatastropheKind::FloodYear => (0, 0.75),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Catastrophe {
    pub id: u32,
    pub kind: CatastropheKind,
    pub severity: f32,
    pub start_day: u64,
    /// First day after it (planned length).
    pub end_day: u64,
    pub started: bool,
    pub ended: bool,
    /// Population (agents and cohort members) when it started and ended.
    pub population_before: usize,
    pub population_after: Option<usize>,
    /// Players with a living lineage at its start, and those still alive at
    /// its end (each survivor earns an achievement).
    pub players_before: Vec<PlayerId>,
    pub survivors: Vec<PlayerId>,
}

impl Catastrophe {
    pub fn is_active(&self) -> bool {
        self.started && !self.ended
    }
}

/// A player's allele share at an era's end.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EraRank {
    pub player: PlayerId,
    pub share: f64,
    pub weighted_share: f64,
}

/// The leader of one niche at an era's end.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NicheLeader {
    /// `diet:herbivore` / `diet:omnivore` / `diet:carnivore` or
    /// `biome:<name>`.
    pub niche: String,
    pub player: PlayerId,
    pub share: f64,
    /// Breeding adults in the niche.
    pub adults: f64,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EraRecord {
    pub era: u64,
    pub first_day: u64,
    pub last_day: u64,
    /// Every origin tag with alleles in the final snapshot, best first (the
    /// wild ecosystem included as player 0).
    pub ranking: Vec<EraRank>,
    pub breeding_adults: usize,
    pub niches: Vec<NicheLeader>,
    /// Catastrophes that started in the era.
    pub catastrophes: Vec<u32>,
    pub species_alive: usize,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AchievementKind {
    SpeciesFounded { species: u32 },
    /// A lineage alive `years` whole years after founding.
    LineageLongevity { lineage: u32, years: u32 },
    NicheDominance { era: u64, niche: String, share: f64 },
    SurvivedCatastrophe { catastrophe: u32 },
}

impl AchievementKind {
    pub fn label(&self) -> &'static str {
        match self {
            AchievementKind::SpeciesFounded { .. } => "species_founded",
            AchievementKind::LineageLongevity { .. } => "lineage_longevity",
            AchievementKind::NicheDominance { .. } => "niche_dominance",
            AchievementKind::SurvivedCatastrophe { .. } => "survived_catastrophe",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Achievement {
    pub day: u64,
    pub player: PlayerId,
    pub kind: AchievementKind,
}

/// When a lineage was first seen alive and when it died out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LineageLife {
    pub founded_day: u64,
    pub extinct_day: Option<u64>,
    /// Whole years of longevity already awarded.
    pub years_awarded: u32,
}

impl LineageLife {
    /// Days alive up to `today` (or to its extinction).
    pub fn days(&self, today: u64) -> u64 {
        self.extinct_day.unwrap_or(today).saturating_sub(self.founded_day)
    }
}

/// Everything the scoring extras record.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Chronicle {
    pub species: SpeciesTracker,
    pub eras: Vec<EraRecord>,
    pub catastrophes: Vec<Catastrophe>,
    pub achievements: Vec<Achievement>,
    /// Per lineage (indexed like `World::lineages`).
    pub lineages: Vec<LineageLife>,
    /// Year whose random catastrophe has been drawn.
    pub(crate) planned_year: Option<u64>,
}

impl Chronicle {
    /// The catastrophe under way, if any.
    pub fn active_catastrophe(&self) -> Option<&Catastrophe> {
        self.catastrophes.iter().find(|c| c.is_active())
    }

    /// Achievements per player, by kind label.
    pub fn achievement_counts(&self) -> BTreeMap<PlayerId, BTreeMap<&'static str, u32>> {
        let mut out: BTreeMap<PlayerId, BTreeMap<&'static str, u32>> = BTreeMap::new();
        for a in &self.achievements {
            *out.entry(a.player).or_default().entry(a.kind.label()).or_default() += 1;
        }
        out
    }

    pub(crate) fn award(&mut self, day: u64, player: PlayerId, kind: AchievementKind) {
        self.achievements.push(Achievement { day, player, kind });
    }
}
