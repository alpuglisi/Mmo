//! Sample founder designs: a wild ecosystem and four player strategies that
//! exercise different rules (pack hunting, herds, toxic eggs, burrowers).

use crate::config::WorldConfig;
use crate::design::{BirthType, FounderDesign, Thermo};
use crate::genome::{Gene, PlayerId, WILD};
use crate::world::World;
use rand::Rng;

/// The reference map size the preset founder counts and caps are tuned for.
pub const REFERENCE_SIZE: usize = 128;

/// A world with the wild ecosystem and the four sample player lineages.
///
/// Wild founders and population caps scale with map area. Player lineages are
/// founded at their designed size (validated against the point budget) unless
/// `scale` is above 1, which multiplies every founding population and skips
/// the budget check (used for benchmarks).
pub fn standard_world(config: WorldConfig, scale: u32) -> World {
    standard_world_from(config, scale, wild_designs(), player_designs())
}

/// [`standard_world`] with the predators in their niches
/// ([`niche_wild_designs`], [`niche_player_designs`]): the red fox a small,
/// flexible mesopredator, the pack hunters a cooperative obligate carnivore
/// of the herds. The designs the carnivore-diet rules and predator brakes
/// (`Balance::carnivore_diet`) were tuned with (*Predators in their niches*
/// in the README); the lineages keep their indices.
pub fn niche_world(config: WorldConfig, scale: u32) -> World {
    standard_world_from(config, scale, niche_wild_designs(), niche_player_designs())
}

/// Changes to the sample designs (`evo-sim ... --design SPEC`): gene values
/// and founder counts per lineage over [`wild_designs`] and
/// [`player_designs`], to try design variants against the standard world.
/// The default changes nothing. A SPEC is comma-separated
/// `LINEAGE.GENE=V` or `LINEAGE.founders=N`: the lineage by its name with
/// spaces as underscores, case ignored (`red_fox`, `pack_hunters`), or
/// `fox` / `pack` for the two predators; the gene by its label with spaces
/// as underscores (`carnivory`, `body_size`, `group_tolerance`). A founder
/// count other than the design's skips the point budget check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesignTweaks {
    /// `(lineage name, gene, value)`.
    pub genes: Vec<(String, Gene, f32)>,
    /// `(lineage name, founders)`.
    pub founders: Vec<(String, u32)>,
}

impl DesignTweaks {
    /// Parse a SPEC (see [`DesignTweaks`]).
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut t = DesignTweaks::default();
        let names: Vec<String> = wild_designs()
            .into_iter()
            .map(|(_, d)| d.name)
            .chain(player_designs().into_iter().map(|d| d.name))
            .collect();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, v) = part.split_once('=').ok_or_else(|| format!("{part}: expected LINEAGE.GENE=V"))?;
            let (lineage, gene) =
                key.trim().split_once('.').ok_or_else(|| format!("{part}: expected LINEAGE.GENE=V"))?;
            let lineage = match lineage.to_lowercase().as_str() {
                "fox" => "red fox".to_string(),
                "pack" => "pack hunters".to_string(),
                other => other.replace('_', " "),
            };
            let name = names
                .iter()
                .find(|n| n.to_lowercase() == lineage)
                .ok_or_else(|| format!("{part}: no sample design named {lineage}"))?
                .clone();
            let v: f32 = v.trim().parse().map_err(|_| format!("{part}: not a number"))?;
            if gene == "founders" {
                t.founders.push((name, v.max(2.0).round() as u32));
            } else {
                let g = Gene::ALL
                    .iter()
                    .copied()
                    .find(|g| g.label().replace(' ', "_") == gene)
                    .ok_or_else(|| format!("{part}: unknown gene {gene} (spaces as underscores)"))?;
                t.genes.push((name, g, v.clamp(0.0, 1.0)));
            }
        }
        Ok(t)
    }

    pub fn is_empty(&self) -> bool {
        self.genes.is_empty() && self.founders.is_empty()
    }

    /// `design` with the changes that name it.
    pub fn apply(&self, design: FounderDesign) -> FounderDesign {
        let mut d = design;
        for (n, g, v) in &self.genes {
            if *n == d.name {
                d = d.with(*g, *v);
            }
        }
        for (n, f) in &self.founders {
            if *n == d.name {
                d = d.founders(*f);
            }
        }
        d
    }
}

/// [`standard_world`] with the sample designs changed as `tweaks` says (the
/// default: exactly [`standard_world`]).
pub fn standard_world_tweaked(config: WorldConfig, scale: u32, tweaks: &DesignTweaks) -> World {
    if tweaks.is_empty() {
        return standard_world(config, scale);
    }
    let wild = wild_designs().into_iter().map(|(o, d)| (o, tweaks.apply(d))).collect();
    let players: Vec<FounderDesign> = player_designs().into_iter().map(|d| tweaks.apply(d)).collect();
    let mut world = wild_world_from(config, scale, wild);
    for (k, design) in players.into_iter().enumerate() {
        let owner = k as PlayerId + 1;
        let n = if scale <= 1 { design.founders } else { (design.founders * scale).max(4) };
        if scale <= 1 && design.validate().is_ok() {
            world.found_lineage(owner, &design).expect("validated");
        } else {
            world.found_lineage_unchecked(owner, &design.founders(n));
        }
    }
    world
}

/// How [`niche_world_with`] founds the niche world: founder counts and the
/// founders' starting state (*First-year prey boom: founding* in the
/// README). The default founds exactly as [`niche_world`]. Predators are the
/// lineages designed at carnivory 0.6 or above (the fox and the pack
/// hunters), prey every other lineage. Population caps and the founding
/// counts the rules and scores measure against (`Lineage::founders`) stay
/// those of the designs, so only the animals placed change.
#[derive(Clone, Debug, PartialEq)]
pub struct NicheFounding {
    /// Wild predator founders (the red fox), times their designed count.
    pub wild_predators: f32,
    /// Player predator founders (the pack hunters), times their designed
    /// count.
    pub player_predators: f32,
    /// Prey founders, times their designed count.
    pub prey: f32,
    /// Predator founders' starting fat, in hunger points (capped at their
    /// fat capacity).
    pub predator_fat: f32,
    /// Predator founders' starting breeding drive is at least this.
    pub predator_drive: f32,
    /// Share of each prey lineage's founders that start as weaned juveniles
    /// (aged between weaning and maturity) instead of adults.
    pub prey_young: f32,
    /// Prey founders' starting breeding drive, times its default (a uniform
    /// draw below 40).
    pub prey_drive: f32,
    /// Predator founders per placement group, each group at its own home
    /// (0: as designed, 12 per group for wild lineages and one group for a
    /// player's). A breeding territory holds one litter at a time
    /// (`Balance::breeding_territory`), so founders clumped in one place
    /// breed as one territory until they spread.
    pub predator_group: f32,
}

impl Default for NicheFounding {
    fn default() -> Self {
        NicheFounding {
            wild_predators: 1.0,
            player_predators: 1.0,
            prey: 1.0,
            predator_fat: 0.0,
            predator_drive: 0.0,
            prey_young: 0.0,
            prey_drive: 1.0,
            predator_group: 0.0,
        }
    }
}

/// Designed carnivory from which a lineage counts as a predator in
/// [`NicheFounding`].
const FOUNDING_PREDATOR: f32 = 0.6;

impl NicheFounding {
    /// Parse `name=value,name=value` (the field names; e.g.
    /// `predators=1.5,predator_fat=30`; `predators` sets both predator
    /// counts) over the defaults.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut f = NicheFounding::default();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (n, v) = part.split_once('=').ok_or_else(|| format!("{part}: expected name=value"))?;
            let v: f32 = v.trim().parse().map_err(|_| format!("{part}: not a number"))?;
            let slot = match n.trim() {
                "predators" => {
                    (f.wild_predators, f.player_predators) = (v, v);
                    continue;
                }
                "wild_predators" => &mut f.wild_predators,
                "player_predators" => &mut f.player_predators,
                "prey" => &mut f.prey,
                "predator_fat" => &mut f.predator_fat,
                "predator_drive" => &mut f.predator_drive,
                "prey_young" => &mut f.prey_young,
                "prey_drive" => &mut f.prey_drive,
                "predator_group" => &mut f.predator_group,
                other => return Err(format!("unknown founding option {other}")),
            };
            *slot = v;
        }
        Ok(f)
    }

    fn is_default(&self) -> bool {
        *self == NicheFounding::default()
    }

    fn count(&self, design: &FounderDesign, wild: bool) -> u32 {
        let k = match (is_predator(design), wild) {
            (true, true) => self.wild_predators,
            (true, false) => self.player_predators,
            (false, _) => self.prey,
        };
        ((design.founders as f32 * k).round() as u32).max(2)
    }
}

fn is_predator(design: &FounderDesign) -> bool {
    design.get(Gene::Carnivory) >= FOUNDING_PREDATOR
}

/// [`niche_world`] founded as `founding` says (the default: exactly
/// [`niche_world`]). Founder counts other than the designs' skip the point
/// budget check.
pub fn niche_world_with(config: WorldConfig, scale: u32, founding: &NicheFounding) -> World {
    if founding.is_default() {
        return niche_world(config, scale);
    }
    let resize = |d: FounderDesign, wild: bool| {
        let n = founding.count(&d, wild);
        (d.founders, d.founders(n))
    };
    let wild: Vec<_> = niche_wild_designs().into_iter().map(|(o, d)| (o, resize(d, true))).collect();
    let players: Vec<_> = niche_player_designs().into_iter().map(|d| resize(d, false)).collect();
    let predator: Vec<bool> =
        wild.iter().map(|(_, (_, d))| d).chain(players.iter().map(|(_, d)| d)).map(is_predator).collect();
    let designed: Vec<u32> = wild.iter().map(|(_, (n, _))| *n).chain(players.iter().map(|(n, _)| *n)).collect();
    let group = |d: &FounderDesign, n: u32, wild: bool| match founding.predator_group {
        g if g >= 1.0 && is_predator(d) => g.round() as u32,
        _ if wild => 12,
        _ => n.max(1),
    };
    let mut world = wild_world_with(config, scale, wild.into_iter().map(|(o, (_, d))| (o, d)).collect(), group);
    for (k, (_, design)) in players.into_iter().enumerate() {
        let owner = k as PlayerId + 1;
        let n = if scale <= 1 { design.founders } else { (design.founders * scale).max(4) };
        world.found_lineage_grouped(owner, &design.clone().founders(n), group(&design, n, false));
    }
    // Caps and the founding counts the rules read stay the designs'.
    let area = (world.config.width * world.config.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    for (l, &n) in designed.iter().enumerate() {
        let wild = world.lineages[l].owner == WILD;
        world.lineages[l].founders = if wild {
            ((n as f32 * area * scale as f32).round() as u32).max(4)
        } else if scale <= 1 {
            n
        } else {
            (n * scale).max(4)
        };
    }
    let tpd = world.config.ticks_per_day as f32;
    let mut rng = crate::rng::rng_for(world.config.seed, crate::rng::stream::FOUNDING, 0xF0_0D, 0);
    for o in world.organisms.iter_mut().filter(|o| o.alive) {
        if predator[o.lineage as usize] {
            o.fat = founding.predator_fat.min(world.config.balance.fat_capacity * (1.0 + o.pheno[Gene::FatStores]));
            o.needs.breeding = o.needs.breeding.max(founding.predator_drive);
        } else {
            o.needs.breeding *= founding.prey_drive;
            if rng.gen::<f32>() < founding.prey_young {
                let wean = crate::stats::weaning_days(o.traits.maturity_days, o.traits.newborn_growth);
                let age = rng.gen_range(wean..o.traits.maturity_days.max(wean + 1e-3));
                let progress = (age / o.traits.maturity_days).min(1.0);
                o.age_ticks = (age * tpd) as u32;
                o.growth = o.traits.newborn_growth + (1.0 - o.traits.newborn_growth) * progress;
                o.matured = false;
                o.needs.breeding = 0.0;
            }
        }
    }
    world
}

/// The standard world's layout from the given wild and player designs (the
/// lineages keep the standard world's indices if the lists keep its order).
pub fn standard_world_from(
    config: WorldConfig,
    scale: u32,
    wild: Vec<(PlayerId, FounderDesign)>,
    players: Vec<FounderDesign>,
) -> World {
    let mut world = wild_world_from(config, scale, wild);
    for (k, design) in players.into_iter().enumerate() {
        let owner = k as PlayerId + 1;
        if scale <= 1 {
            world.found_lineage(owner, &design).expect("preset designs fit the budget");
        } else {
            let n = (design.founders * scale).max(4);
            world.found_lineage_unchecked(owner, &design.founders(n));
        }
    }
    world
}

/// Owner of the extra migrating lineage in [`extended_world`].
pub const MIGRANT_OWNER: PlayerId = 5;

/// [`standard_world`] plus the migrating sample design ([`migrant_design`])
/// as a fifth player lineage (owner [`MIGRANT_OWNER`], the last lineage).
/// The standard world's eight lineages keep their indices, so scores over
/// them stay comparable; the extra lineage is scored on its own.
pub fn extended_world(config: WorldConfig, scale: u32) -> World {
    extended_world_with(config, scale, &migrant_design())
}

/// [`extended_world`] with `design` as the fifth player lineage in place of
/// the migrating caribou (`evo-sim compare --migrant-gene` founds a variant
/// of it, e.g. the same herd without the migration gene).
pub fn extended_world_with(config: WorldConfig, scale: u32, design: &FounderDesign) -> World {
    let mut world = standard_world(config, scale);
    if scale <= 1 {
        world.found_lineage(MIGRANT_OWNER, design).expect("preset designs fit the budget");
    } else {
        let n = (design.founders * scale).max(4);
        world.found_lineage_unchecked(MIGRANT_OWNER, &design.clone().founders(n));
    }
    world
}

/// A world with only the wild ecosystem, its founders and the population caps
/// scaled to map area as in [`standard_world`]. Player lineages are added by
/// the caller (the cost calibration example founds its own designs here).
pub fn wild_world(config: WorldConfig, scale: u32) -> World {
    wild_world_from(config, scale, wild_designs())
}

fn wild_world_from(config: WorldConfig, scale: u32, wild: Vec<(PlayerId, FounderDesign)>) -> World {
    wild_world_with(config, scale, wild, |_, _, _| 12)
}

/// [`wild_world_from`] with each lineage's founders placed in groups of
/// `group(design, founders, true)` (12 in [`wild_world_from`]).
fn wild_world_with(
    mut config: WorldConfig,
    scale: u32,
    wild: Vec<(PlayerId, FounderDesign)>,
    group: impl Fn(&FounderDesign, u32, bool) -> u32,
) -> World {
    let area = (config.width * config.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    config.wild_cap = ((config.wild_cap as f32 * area * scale as f32) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5) * scale as f32) as usize).max(50);
    let mut world = World::new(config);
    let grow = |n: u32| ((n as f32 * area * scale as f32).round() as u32).max(4);
    for (owner, design) in wild {
        let n = grow(design.founders);
        world.found_lineage_grouped(owner, &design.clone().founders(n), group(&design, n, true));
    }
    world
}

/// The standard red fox's carnivory (0.85 before; *The fox in its own
/// niche* in the README).
pub const FOX_CARNIVORY: f32 = 0.6;

/// The wild ecosystem. Founded without the point budget.
pub fn wild_designs() -> Vec<(PlayerId, FounderDesign)> {
    use Gene::*;
    vec![
        (
            WILD,
            FounderDesign::new("Meadow vole", Thermo::Warm, BirthType::Live, 6)
                .with(BodySize, 0.12)
                .with(Ears, 0.4)
                .with(Nose, 0.3)
                .with(Burrowing, 0.6)
                .with(Claws, 0.3)
                .with(Fur, 0.5)
                .with(Camouflage, 0.4)
                .with(GroupTolerance, 0.4)
                // Voles hoard seeds for winter.
                .with(Caching, 0.8)
                .founders(160),
        ),
        (
            WILD,
            FounderDesign::new("Steppe antelope", Thermo::Warm, BirthType::Live, 1)
                .with(BodySize, 0.55)
                .with(LegLength, 0.7)
                .with(Hooves, 0.7)
                .with(Eyes, 0.5)
                .with(Ears, 0.5)
                .with(Fur, 0.4)
                .with(GroupTolerance, 0.8)
                .with(Boldness, 0.3)
                // A herd that takes turns on watch (`GroupTrait::Sentries`).
                .with(Vigilance, 0.7)
                .founders(70),
        ),
        (
            WILD,
            FounderDesign::new("Marsh newt", Thermo::Cold, BirthType::AbandonedEggs, 8)
                .with(BodySize, 0.08)
                .with(Swimming, 0.8)
                .with(FootWidth, 0.5)
                .with(Camouflage, 0.6)
                .with(Carnivory, 0.1)
                .with(Tail, 0.6)
                // Newts brumate through the cold.
                .with(Brumation, 0.8)
                .founders(100),
        ),
        (
            WILD,
            FounderDesign::new("Red fox", Thermo::Warm, BirthType::Live, 4)
                .with(BodySize, 0.32)
                // A flexible hunter that eats fruit, insects and plants
                // too, a full body-plan gap (`mating::BODY_PLAN_STERILE`,
                // 0.3) below the pack hunters' 0.9, so the two don't
                // interbreed (*The fox in its own niche* in the README). At
                // 0.85 they were one step apart and the fox took on the pack
                // hunters' genes; at 0.55 it died out more often.
                .with(Carnivory, FOX_CARNIVORY)
                .with(LegLength, 0.5)
                .with(Nose, 0.6)
                .with(Ears, 0.5)
                .with(Fangs, 0.5)
                .with(Claws, 0.3)
                .with(Fur, 0.5)
                .with(Camouflage, 0.3)
                .with(GroupTolerance, 0.2)
                // Solitary hunters: at the default pack instinct (0.5) half
                // the foxes carried cooperative hunting and, where it held,
                // took prey three times their size (adult antelope).
                .with(PackInstinct, 0.1)
                // Foxes molt into a thick winter coat.
                .with(SeasonalCoat, 0.8)
                .founders(24),
        ),
    ]
}

/// The red fox as a small, flexible mesopredator (version 27, *Predators in
/// their niches* in the README), for [`niche_world`]: carnivory 0.65 makes
/// it a hunter that also eats plants (a red fox lives on berries and fruit
/// in season as much as on voles), not an obligate carnivore on the pack
/// hunters' prey; shorter fangs crop plants better (`fang_grazing`). At 0.85
/// the pack hunters excluded it from the shared voles and newts by year 6.
/// Fat stores bank a kill (`Balance::carnivore_fat`), and a solitary,
/// aggressive temper holds a wide breeding territory, which keeps its
/// numbers in check (`world::apply::territory_radius`). A starting point:
/// evolution moves every one of these genes.
pub fn niche_fox() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Red fox", Thermo::Warm, BirthType::Live, 4)
        .with(BodySize, 0.32)
        .with(Carnivory, 0.65)
        .with(LegLength, 0.5)
        .with(Nose, 0.6)
        .with(Ears, 0.5)
        .with(Fangs, 0.3)
        .with(Claws, 0.3)
        .with(Fur, 0.5)
        .with(Camouflage, 0.3)
        .with(FatStores, 0.5)
        .with(GroupTolerance, 0.2)
        .with(Aggression, 0.7)
        .with(PackInstinct, 0.1)
        .with(SeasonalCoat, 0.8)
        .founders(24)
}

/// The pack hunters as a cooperative obligate carnivore of the herds
/// (version 27, *Predators in their niches* in the README), for
/// [`niche_world`]: 1.1 kg (body size 0.30; 0.8 kg at 0.22), so with its
/// pack it takes herd calves and antelope, which by weight are most of its
/// food (prey are scored by mass^0.75), and carnivory 0.95, a full body-plan
/// gap (0.3) from the niche fox's 0.65, so the two neither interbreed nor
/// count as one kind. Fat stores bank a kill. Bigger did not work: at
/// 0.36–0.45 (1.8–3 kg) small prey cover too little of a big hunter's day
/// and founding mothers starved (survived 6 years on 1–2 of 8 seeds).
/// Within the point budget.
pub fn niche_pack_hunters() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Pack hunters", Thermo::Warm, BirthType::Live, 4)
        .with(BodySize, 0.30)
        .with(Carnivory, 0.95)
        .with(PackInstinct, 0.9)
        .with(GroupTolerance, 0.8)
        .with(FatStores, 0.5)
        .with(LegLength, 0.55)
        .with(Muscle, 0.4)
        .with(Fangs, 0.5)
        .with(Nose, 0.5)
        .with(Ears, 0.4)
        .with(Fur, 0.5)
        .with(Vocal, 0.6)
        .founders(24)
}

/// [`wild_designs`] with the red fox as [`niche_fox`].
pub fn niche_wild_designs() -> Vec<(PlayerId, FounderDesign)> {
    let fox = niche_fox();
    wild_designs().into_iter().map(|(o, d)| if d.name == fox.name { (o, fox.clone()) } else { (o, d) }).collect()
}

/// [`player_designs`] with the pack hunters as [`niche_pack_hunters`].
pub fn niche_player_designs() -> Vec<FounderDesign> {
    let pack = niche_pack_hunters();
    player_designs().into_iter().map(|d| if d.name == pack.name { pack.clone() } else { d }).collect()
}

/// Four example player designs, all within the point budget.
pub fn player_designs() -> Vec<FounderDesign> {
    use Gene::*;
    vec![
        FounderDesign::new("Pack hunters", Thermo::Warm, BirthType::Live, 4)
            // Small (0.8 kg, weasel-like): voles and newts are its prey,
            // and a kill or a mouthful of plants covers about twice as much
            // of a small hunter's day (hunger per food unit ~ mass^-0.5). At
            // 0.42 (2.6 kg) meat covered 2 of 33 hunger points a day and
            // founding packs starved on 4 of 48 seeds. Not 0.32: fox-sized
            // hunters would count foxes as their own kind.
            .with(BodySize, 0.22)
            .with(Carnivory, 0.9)
            .with(PackInstinct, 0.9)
            .with(GroupTolerance, 0.8)
            .with(LegLength, 0.55)
            .with(Muscle, 0.4)
            .with(Fangs, 0.5)
            .with(Nose, 0.5)
            .with(Ears, 0.4)
            .with(Fur, 0.5)
            .with(Vocal, 0.6)
            .founders(24),
        FounderDesign::new("Herd grazers", Thermo::Warm, BirthType::Live, 2)
            .with(BodySize, 0.62)
            .with(LegLength, 0.6)
            .with(Hooves, 0.7)
            .with(FatStores, 0.4)
            .with(Eyes, 0.5)
            .with(Ears, 0.6)
            .with(Vocal, 0.6)
            .with(Fur, 0.5)
            .with(GroupTolerance, 0.9)
            .with(Boldness, 0.3)
            // Takes turns on watch (`GroupTrait::Sentries`).
            .with(Vigilance, 0.7)
            .founders(40),
        FounderDesign::new("Toxic frogs", Thermo::Cold, BirthType::AbandonedEggs, 10)
            .with(BodySize, 0.12)
            .with(Toxins, 0.8)
            .with(Coloration, 0.9)
            .with(Swimming, 0.7)
            .with(FootWidth, 0.6)
            .with(Carnivory, 0.1)
            .with(GroupTolerance, 0.6)
            .with(Pickiness, 0.7)
            .with(Brumation, 0.8)
            .founders(46),
        FounderDesign::new("Burrowing omnivores", Thermo::Warm, BirthType::IncubatedEggs, 4)
            .with(BodySize, 0.3)
            .with(Burrowing, 0.8)
            .with(Claws, 0.6)
            .with(Carnivory, 0.5)
            .with(Nose, 0.6)
            .with(Fur, 0.5)
            .with(FatStores, 0.5)
            // Tolerant enough to share burrows (`GroupTrait::SharedBurrows`).
            .with(GroupTolerance, 0.6)
            .with(Wanderlust, 0.3)
            // Badger-like: fattens in autumn, sleeps through hard winters.
            .with(Hibernation, 0.8)
            .founders(30),
    ]
}

/// A migrating herd: summers at its founding range, winters a third of the
/// map further south, and travels as a group (`GroupTrait::GroupNavigation`:
/// migration ≥ 0.5 with group tolerance > 0.5), so it runs less journey
/// risk, eats less on the way and keeps together. Within the point budget.
pub fn migrant_design() -> FounderDesign {
    use Gene::*;
    FounderDesign::new("Migrating caribou", Thermo::Warm, BirthType::Live, 2)
        // Large: at 0.55 (antelope size) pack hunters took the herd on
        // 3 of 8 test seeds.
        .with(BodySize, 0.72)
        .with(LegLength, 0.6)
        .with(Hooves, 0.6)
        .with(FatStores, 0.5)
        .with(Eyes, 0.5)
        .with(Ears, 0.6)
        .with(Vocal, 0.6)
        .with(Fur, 0.6)
        .with(GroupTolerance, 0.9)
        .with(Boldness, 0.3)
        .with(Migration, 0.8)
        .founders(36)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_designs_fit_the_budget() {
        for d in player_designs().into_iter().chain([migrant_design(), niche_pack_hunters()]) {
            assert!(d.validate().is_ok(), "{}: {:?} (cost {:.1})", d.name, d.validate(), d.point_cost());
        }
    }

    /// The default founding is the niche world; other founder counts place
    /// more or fewer animals but keep the caps and the founding counts the
    /// rules read, and the founders' starting state follows the options.
    #[test]
    fn niche_founding_changes_only_the_founders() {
        let config = || WorldConfig { seed: 9, width: 64, height: 64, ..Default::default() };
        let base = niche_world(config(), 1);
        let same = niche_world_with(config(), 1, &NicheFounding::default());
        assert_eq!(base.organisms.len(), same.organisms.len());
        assert!(base.organisms.iter().zip(&same.organisms).all(|(a, b)| a.pos == b.pos && a.genome == b.genome));

        let f = NicheFounding::parse("predators=2, predator_fat=40, prey_young=1, prey_drive=0").unwrap();
        assert_eq!((f.wild_predators, f.player_predators, f.predator_fat), (2.0, 2.0, 40.0));
        assert!(NicheFounding::parse("wolves=2").is_err());
        let w = niche_world_with(config(), 1, &f);
        let count = |w: &World, l: u32| w.organisms.iter().filter(|o| o.lineage == l).count();
        for l in 0..base.lineages.len() as u32 {
            assert_eq!(w.cap_usage(l).1, base.cap_usage(l).1, "cap of lineage {l}");
            assert_eq!(w.lineages[l as usize].founders, base.lineages[l as usize].founders);
            let (designed, placed) = (count(&base, l), count(&w, l));
            if l == 3 || l == 4 {
                assert!(
                    placed >= 2 * designed - 1 && placed <= 2 * designed + 1,
                    "lineage {l}: {placed} of {designed}"
                );
                assert!(w.organisms.iter().filter(|o| o.lineage == l).all(|o| o.fat == 40.0 && o.matured));
            } else {
                assert_eq!(placed, designed);
                assert!(w.organisms.iter().filter(|o| o.lineage == l).all(|o| !o.matured && o.needs.breeding == 0.0));
            }
        }
    }

    /// Predators founded in pairs are scattered over the map instead of in
    /// one pack: the same animals, at more homes.
    #[test]
    fn predators_founded_in_pairs_spread_out() {
        let config = || WorldConfig { seed: 4, width: 96, height: 96, ..Default::default() };
        let base = niche_world(config(), 1);
        let pairs = niche_world_with(config(), 1, &NicheFounding::parse("predator_group=2").unwrap());
        let spread = |w: &World| {
            let pack: Vec<_> = w.organisms.iter().filter(|o| o.lineage == 4).map(|o| o.pos).collect();
            let mut far = 0.0f32;
            for a in &pack {
                for b in &pack {
                    far = far.max(a.dist(*b));
                }
            }
            (pack.len(), far)
        };
        let ((n0, far0), (n1, far1)) = (spread(&base), spread(&pairs));
        assert_eq!(n0, n1);
        assert!(far0 < 15.0 && far1 > 30.0, "pack spread {far0:.1} as designed, {far1:.1} in pairs");
    }

    /// The standard fox and pack hunters are a full body-plan gap apart, so
    /// they can't interbreed (`mating::body_plan_fertility` nil between
    /// their designs).
    #[test]
    fn standard_predators_cannot_interbreed() {
        let fox = wild_designs().into_iter().map(|(_, d)| d).find(|d| d.name == "Red fox").unwrap();
        let pack = player_designs().into_iter().find(|d| d.name == "Pack hunters").unwrap();
        let gap = (fox.get(Gene::Carnivory) - pack.get(Gene::Carnivory)).abs();
        assert!(gap >= crate::mating::BODY_PLAN_STERILE - 1e-6, "carnivory gap {gap}");
        let genome = |d: &FounderDesign| {
            crate::genome::Genome::founder_with_morph(
                &d.values(),
                &d.morph,
                0,
                0.0,
                &mut crate::rng::rng_for(1, 1, 1, 1),
            )
        };
        assert!(crate::mating::body_plan_fertility(&genome(&fox), &genome(&pack)) < 1e-5);
    }

    /// Foxes hunt alone: none of them carries cooperative hunting, so they
    /// never take prey more than 1.5 times their size.
    #[test]
    fn foxes_are_solitary_hunters() {
        use crate::group::GroupTrait;
        let w = standard_world(WorldConfig { seed: 3, width: 64, height: 64, ..Default::default() }, 1);
        let foxes: Vec<_> = w.organisms.iter().filter(|o| o.lineage == 3).collect();
        assert!(!foxes.is_empty());
        assert!(foxes.iter().all(|o| !GroupTrait::CooperativeHunting.is_carrier(&o.pheno)));
        assert!(w.organisms.iter().any(|o| o.lineage == 4 && GroupTrait::CooperativeHunting.is_carrier(&o.pheno)));
    }

    /// The migrating caribou are a fifth player lineage after the standard
    /// eight, group-navigation carriers, and in late autumn their herd heads
    /// south together (`seasonal_destination`, `navigation_bonus`).
    #[test]
    fn caribou_migrate_south_as_a_group() {
        use crate::group::GroupTrait;
        use crate::organism::Action;
        let config = WorldConfig { seed: 5, width: 64, height: 64, ..Default::default() };
        let mut w = extended_world(config, 1);
        let l = w.lineages.len() as u32 - 1;
        assert_eq!(l, 8);
        assert_eq!(w.lineages[l as usize].owner, MIGRANT_OWNER);
        let herd: Vec<_> = w.organisms.iter().filter(|o| o.lineage == l).collect();
        assert!(herd.iter().all(|o| GroupTrait::GroupNavigation.is_carrier(&o.pheno)));
        let start = herd.iter().map(|o| o.pos.y).sum::<f32>() / herd.len() as f32;
        // Into late autumn, once its summer pasture has stopped growing
        // (`Balance::migration_onset`): the herd is on its way, navigating as
        // a group.
        let (mut migrating, mut navigating) = (0, 0);
        for _ in 0..90 {
            w.run_days(1);
            for o in w.organisms.iter().filter(|o| o.alive && o.lineage == l && o.action == Action::Migrate) {
                migrating += 1;
                navigating += usize::from(w.navigation_bonus(o) > 0.5);
            }
        }
        let alive: Vec<_> = w.organisms.iter().filter(|o| o.alive && o.lineage == l).collect();
        assert!(alive.len() >= 10, "{} caribou left", alive.len());
        let now = alive.iter().map(|o| o.pos.y).sum::<f32>() / alive.len() as f32;
        assert!(now > start + 5.0, "herd at {now:.1}, started at {start:.1}");
        assert!(migrating > 0 && navigating * 2 > migrating, "{navigating} of {migrating} navigating");
    }
}
