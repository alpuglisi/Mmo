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
pub fn standard_world(config: WorldConfig, scale: u32) -> World {
    let mut world = wild_world(config, scale);
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

/// Owner of the extra migrating lineage in [`extended_world`].
pub const MIGRANT_OWNER: PlayerId = 5;

/// [`standard_world`] plus the migrating sample design ([`migrant_design`])
/// as a fifth player lineage (owner [`MIGRANT_OWNER`], the last lineage).
/// The standard world's eight lineages keep their indices, so scores over
/// them stay comparable; the extra lineage is scored on its own.
pub fn extended_world(config: WorldConfig, scale: u32) -> World {
    let mut world = standard_world(config, scale);
    let design = migrant_design();
    if scale <= 1 {
        world.found_lineage(MIGRANT_OWNER, &design).expect("preset designs fit the budget");
    } else {
        let n = (design.founders * scale).max(4);
        world.found_lineage_unchecked(MIGRANT_OWNER, &design.founders(n));
    }
    world
}

/// A world with only the wild ecosystem, its founders and the population caps
/// scaled to map area as in [`standard_world`]. Player lineages are added by
/// the caller (the cost calibration example founds its own designs here).
pub fn wild_world(mut config: WorldConfig, scale: u32) -> World {
    let area = (config.width * config.height) as f32 / (REFERENCE_SIZE * REFERENCE_SIZE) as f32;
    config.wild_cap = ((config.wild_cap as f32 * area * scale as f32) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5) * scale as f32) as usize).max(50);
    let mut world = World::new(config);
    let grow = |n: u32| ((n as f32 * area * scale as f32).round() as u32).max(4);
    for (owner, design) in wild_designs() {
        let n = grow(design.founders);
        world.found_lineage_unchecked(owner, &design.founders(n));
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
                .with(Carnivory, 0.85)
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
        for d in player_designs().into_iter().chain([migrant_design()]) {
            assert!(d.validate().is_ok(), "{}: {:?} (cost {:.1})", d.name, d.validate(), d.point_cost());
        }
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
    /// eight, group-navigation carriers, and in autumn their herd heads
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
        // Into autumn: the herd is on its way, navigating as a group.
        let (mut migrating, mut navigating) = (0, 0);
        for _ in 0..70 {
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
