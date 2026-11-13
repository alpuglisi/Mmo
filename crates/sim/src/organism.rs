//! A single organism: genome, derived traits, needs and life-cycle state.

use crate::genome::{Gene, Genome, Phenotype, PlayerId};
use crate::mating::Display;
use crate::math::Vec2;
use crate::pathfinding::Path;
use crate::stats::{derive, Traits};

pub type OrgId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sex {
    Female,
    Male,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeathCause {
    Starvation,
    Thirst,
    Exposure,
    Predation,
    Toxin,
    Drowning,
    OldAge,
    /// Background adult mortality of fast-living species (disease, accidents).
    Attrition,
    /// Homozygous for a harmful recessive (`Balance::recessive_hazard`).
    Recessive,
    /// An acute mating-transmitted infection drained its health.
    Disease,
}

impl DeathCause {
    pub const ALL: [DeathCause; 10] = [
        DeathCause::Starvation,
        DeathCause::Thirst,
        DeathCause::Exposure,
        DeathCause::Predation,
        DeathCause::Toxin,
        DeathCause::Drowning,
        DeathCause::OldAge,
        DeathCause::Attrition,
        DeathCause::Recessive,
        DeathCause::Disease,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DeathCause::Starvation => "starved",
            DeathCause::Thirst => "thirst",
            DeathCause::Exposure => "exposure",
            DeathCause::Predation => "eaten",
            DeathCause::Toxin => "toxin",
            DeathCause::Drowning => "drowned",
            DeathCause::OldAge => "old age",
            DeathCause::Attrition => "attrition",
            DeathCause::Recessive => "recessive",
            DeathCause::Disease => "disease",
        }
    }
}

/// Mating-transmitted disease state ("Genetics: balancing picky vs flexible
/// lineages" in the design doc).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Infection {
    #[default]
    Susceptible,
    /// Sick: loses health daily (`Balance::disease_damage`) until it recovers.
    Acute,
    /// Lifelong symptomless carrier: infectious, less fertile.
    Chronic,
    /// Recovered and cleared the pathogen.
    Immune,
}

impl Infection {
    pub fn infectious(self) -> bool {
        matches!(self, Infection::Acute | Infection::Chronic)
    }
}

/// Needs on a 0–100 scale ("Biological needs and behavior" in the doc).
/// Energy is the one need where high is good.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Needs {
    pub hunger: f32,
    pub thirst: f32,
    pub energy: f32,
    /// Degrees outside the comfort range.
    pub cold: f32,
    pub heat: f32,
    pub safety: f32,
    pub breeding: f32,
    pub social: f32,
}

impl Default for Needs {
    fn default() -> Self {
        Self { hunger: 20.0, thirst: 20.0, energy: 80.0, cold: 0.0, heat: 0.0, safety: 0.0, breeding: 0.0, social: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pregnancy {
    pub father: Box<Genome>,
    pub father_id: OrgId,
    /// The father's lineage (offspring take the mother's).
    pub father_lineage: u32,
    pub fertility: f32,
    pub due_tick: u64,
}

/// What the organism is doing this tick (from the utility AI or a player).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Idle,
    Graze,
    Hunt,
    Scavenge,
    Drink,
    Rest,
    Bask,
    Flee,
    SeekMate,
    Group,
    Wander,
    Migrate,
    FollowParent,
    Guard,
    Ordered,
}

#[derive(Clone, Debug)]
pub struct Organism {
    pub id: OrgId,
    pub owner: PlayerId,
    pub generation: u32,
    /// Index into `World::lineages`: the founding population this descends
    /// from (through the mother).
    pub lineage: u32,
    pub sex: Sex,
    pub genome: Genome,
    pub pheno: Phenotype,
    pub traits: Traits,
    pub pos: Vec2,
    pub age_ticks: u32,
    /// Fraction of adult size, 0–1.
    pub growth: f32,
    pub matured: bool,
    pub needs: Needs,
    pub health: f32,
    pub alive: bool,
    pub death: Option<DeathCause>,
    pub mother: Option<OrgId>,
    pub father: Option<OrgId>,
    pub pregnancy: Option<Pregnancy>,
    pub breed_ready_tick: u64,
    /// Offspring that reached breeding age (the legacy fitness weight).
    pub offspring_raised: u32,
    /// Position of a clutch this organism is incubating.
    pub nest: Option<Vec2>,
    pub last_water: Option<Vec2>,
    pub wander_target: Option<Vec2>,
    /// Remembered food patch, kept across interruptions (e.g. a drink).
    pub forage_target: Option<Vec2>,
    pub action: Action,
    /// Where the current action is heading, if anywhere.
    pub target: Option<Vec2>,
    /// Cold-blooded activity level from body temperature, 0.15–1.
    pub activity: f32,
    /// Conspecifics seen last tick (herd defense, social need).
    pub kin_nearby: u16,
    pub sheltered: bool,
    /// Learned after being poisoned by brightly colored prey: avoid bright prey.
    pub bright_aversion: bool,
    /// Tick from which this organism can strike again: prey that broke free
    /// leaves the hunter recovering for a while (`Balance::escape_window`).
    pub strike_ready_tick: u64,
    pub possessed_by: Option<PlayerId>,
    pub order: Option<Vec2>,
    /// Load loci homozygous for a harmful recessive (cached at birth).
    pub recessive_load: u32,
    pub infection: Infection,
    /// Route to `target` around terrain the body can't cross, if the direct
    /// way was blocked (`World::move_toward`).
    pub path: Option<Box<Path>>,
}

impl Organism {
    pub fn new(id: OrgId, owner: PlayerId, genome: Genome, sex: Sex, pos: Vec2) -> Self {
        let pheno = genome.phenotype();
        let traits = derive(&pheno);
        let recessive_load = genome.homozygous_load();
        Self {
            id,
            owner,
            generation: 0,
            lineage: 0,
            sex,
            genome,
            pheno,
            traits,
            pos,
            age_ticks: 0,
            growth: traits.newborn_growth,
            matured: false,
            needs: Needs::default(),
            health: 100.0,
            alive: true,
            death: None,
            mother: None,
            father: None,
            pregnancy: None,
            breed_ready_tick: 0,
            offspring_raised: 0,
            nest: None,
            last_water: None,
            wander_target: None,
            forage_target: None,
            action: Action::Idle,
            target: None,
            activity: 1.0,
            kin_nearby: 0,
            sheltered: false,
            bright_aversion: false,
            strike_ready_tick: 0,
            possessed_by: None,
            order: None,
            recessive_load,
            infection: Infection::Susceptible,
            path: None,
        }
    }

    pub fn age_days(&self, ticks_per_day: u32) -> f32 {
        self.age_ticks as f32 / ticks_per_day as f32
    }

    /// Current mass: growth is the fraction of adult mass.
    pub fn mass(&self) -> f32 {
        self.traits.body.mass * self.growth.max(0.03)
    }

    /// Food units per hunger point: metabolic mass^0.75.
    pub fn food_per_hunger(&self) -> f32 {
        0.04 * self.mass().powf(0.75)
    }

    pub fn condition(&self) -> f32 {
        (self.health / 100.0) * (1.0 - self.needs.hunger / 200.0)
    }

    pub fn display(&self) -> Display {
        Display {
            coloration: self.pheno[Gene::Coloration] * self.condition(),
            body_size: self.pheno[Gene::BodySize] * self.growth,
        }
    }

    pub fn can_breed(&self, tick: u64) -> bool {
        self.alive && self.matured && self.pregnancy.is_none() && tick >= self.breed_ready_tick
    }

    pub fn is_breeding_adult(&self) -> bool {
        self.alive && self.matured
    }

    fn vigor(&self) -> f32 {
        (0.4 + 0.6 * self.growth) * (0.5 + 0.5 * self.needs.energy / 100.0) * self.activity.max(0.3)
    }

    /// Attack power before group-trait bonuses.
    pub fn power(&self) -> f32 {
        self.traits.stats.strength * self.vigor() + 0.3 * self.traits.stats.agility
    }

    /// Ability to get away from (or run down) another animal in a struggle:
    /// running speed, as slowed by youth and cold, plus agility, both sapped
    /// by exhaustion.
    pub fn evasion(&self) -> f32 {
        let st = &self.traits.stats;
        (st.speed * self.activity * (0.5 + 0.5 * self.growth) + st.agility) * (0.4 + 0.6 * self.needs.energy / 100.0)
    }

    /// Resistance to attack before group-trait bonuses.
    pub fn resistance(&self) -> f32 {
        self.traits.stats.defense * (0.4 + 0.6 * self.growth)
            + 0.3 * self.traits.stats.agility * (self.needs.energy / 100.0)
    }
}
