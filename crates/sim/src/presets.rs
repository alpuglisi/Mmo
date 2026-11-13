//! Sample founder designs: a wild ecosystem and four player strategies that
//! exercise different rules (pack hunting, herds, toxic eggs, burrowers).

use crate::config::WorldConfig;
use crate::design::{BirthType, FounderDesign, Thermo};
use crate::genome::{Gene, PlayerId, WILD};
use crate::world::World;

/// The reference map size the preset founder counts and caps are tuned for.
pub const REFERENCE_SIZE: usize = 128;

/// A world with the wild ecosystem and the four sample player lineages.
///
/// Wild founders and population caps scale with map area. Player lineages are
/// founded at their designed size (validated against the point budget) unless
/// `scale` is above 1, which multiplies every founding population and skips
/// the budget check (used for benchmarks).
pub fn standard_world(mut config: WorldConfig, scale: u32) -> World {
    let area = (config.width * config.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    config.wild_cap = ((config.wild_cap as f32 * area * scale as f32) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5) * scale as f32) as usize).max(50);
    let mut world = World::new(config);
    let grow = |n: u32| ((n as f32 * area * scale as f32).round() as u32).max(4);
    for (owner, design) in wild_designs() {
        let n = grow(design.founders);
        world.found_lineage_unchecked(owner, &design.founders(n));
    }
    for (k, design) in player_designs().into_iter().enumerate() {
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
                .with(Carnivory, 0.85)
                .with(LegLength, 0.5)
                .with(Nose, 0.6)
                .with(Ears, 0.5)
                .with(Fangs, 0.5)
                .with(Claws, 0.3)
                .with(Fur, 0.5)
                .with(Camouflage, 0.3)
                .with(GroupTolerance, 0.2)
                .founders(24),
        ),
    ]
}

/// Four example player designs, all within the point budget.
pub fn player_designs() -> Vec<FounderDesign> {
    use Gene::*;
    vec![
        FounderDesign::new("Pack hunters", Thermo::Warm, BirthType::Live, 4)
            .with(BodySize, 0.42)
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
            .founders(50),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_designs_fit_the_budget() {
        for d in player_designs() {
            assert!(d.validate().is_ok(), "{}: {:?} (cost {:.1})", d.name, d.validate(), d.point_cost());
        }
    }
}
