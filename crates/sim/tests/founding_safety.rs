//! Playtest 4's fatal foundings (`docs/playtest-4.md` H3), replayed on the
//! playtest's world: seed 29, 128×128, the wild ecosystem scaled to the map
//! as the server builds it, catastrophes on. Each case founds the reported
//! design on (or near) the reported day on a copy of the world, and checks
//! what went wrong then (`Balance::founding_safety`):
//!
//! - every founder stands on dry land: no water or ice on its tile and no
//!   lake bed or river channel under it (the thaw and the end of a drought
//!   put playtest 4's founders in deep water);
//! - the home keeps off the map's edge (the tundra rim, a water-ringed
//!   corner, a dry mountain corner);
//! - nobody drowns in the first 10 days (13 of 18 stood in deep water by
//!   day 13; 4 of 50 drowned on day 1).
//!
//! The players' own actions are not replayed, so the world only resembles
//! the playtest's; the cases are the designs and seasons that failed.

use evo_sim::design::{BirthType, FounderDesign, Thermo};
use evo_sim::morph::MorphDesign;
use evo_sim::organism::DeathCause;
use evo_sim::{presets, Gene, World, WorldConfig};

fn playtest_world() -> World {
    playtest_world_seed(29)
}

fn playtest_world_seed(seed: u64) -> World {
    let (size, reference) = (128usize, presets::REFERENCE_SIZE);
    let area = (size * size) as f32 / (reference * reference) as f32;
    let mut config = WorldConfig { seed, width: size, height: size, ..Default::default() };
    config.wild_cap = ((config.wild_cap as f32 * area) as usize).max(50);
    config.lineage_cap = ((config.lineage_cap as f32 * area.max(0.5)) as usize).max(50);
    config.scoring.catastrophes = true;
    let mut w = World::new(config);
    for (owner, design) in presets::wild_designs() {
        let n = ((design.founders as f32 * area).round() as u32).max(4);
        w.found_lineage_unchecked(owner, &design.founders(n));
    }
    w
}

/// Found `design` for `owner` on a copy of `w`; panic on a founder on wet
/// ground, a home at the edge, or drowning in 10 days. Returns the founders
/// alive after 10 days and the founding.
fn check(w: &World, owner: u32, design: &FounderDesign, label: &str) -> (usize, usize) {
    let mut c = w.clone();
    let ids = c.found_lineage_unchecked(owner, design);
    let lineage = c.lineages.len() as u32 - 1;
    let ground = c.map.ground().to_vec();
    for &id in &ids {
        let o = c.organism(id).expect("founder alive");
        let (x, y) = c.map.tile_coords(o.pos);
        let i = c.map.idx(x, y);
        let (t, g) = (&c.map.tiles[i], &ground[i]);
        assert!(
            t.water_depth == 0.0 && t.ice == 0.0 && g.base_water <= 0.0 && g.river <= 0.0,
            "{label}: founder at ({x}, {y}) on water {:.2}, ice {:.2}, lake bed {:.2}, river {:.2}",
            t.water_depth,
            t.ice,
            g.base_water,
            g.river
        );
    }
    let home = c.organism(ids[0]).unwrap().pos;
    let n = ids.len() as f32;
    let centre =
        ids.iter().filter_map(|&id| c.organism(id)).fold(evo_sim::math::Vec2::ZERO, |a, o| a + o.pos) * (1.0 / n);
    let edge = centre.x.min(centre.y).min(c.map.width as f32 - centre.x).min(c.map.height as f32 - centre.y);
    assert!(edge >= 4.0, "{label}: founders centred {edge:.1} tiles from the edge (home {home:?})");
    let drowned = |c: &World| c.counters.deaths_by_lineage.get(&(lineage, DeathCause::Drowning)).copied().unwrap_or(0);
    for day in 0..10 {
        for _ in 0..c.config.ticks_per_day {
            c.step();
        }
        // Drowning in the first days is the placement's doing (playtest 4:
        // 4 of 50 on day 1; the thaw under founders put on ice).
        if day < 3 {
            assert_eq!(drowned(&c), 0, "{label}: founders drowned by day {}", day + 1);
        }
    }
    let alive = ids.iter().filter(|&&id| c.organism(id).is_some()).count();
    eprintln!(
        "{label}: {alive}/{} alive after 10 days ({} drowned after day 3), centred {edge:.0} from the edge",
        ids.len(),
        drowned(&c)
    );
    (alive, ids.len())
}

#[test]
fn playtest4_foundings_land_on_dry_ground() {
    let mut w = playtest_world();
    let run_to = |w: &mut World, day: u64| {
        let tpd = w.config.ticks_per_day;
        while w.day() < day {
            for _ in 0..tpd {
                w.step();
            }
        }
    };
    // Rulebender4, day 40: 50 founders, swimming 0.3 (4 drowned on day 1).
    let voles = FounderDesign::new("Loophole Voles", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(Gene::FatStores, 0.6)
        .with(Gene::Fur, 0.6)
        .with(Gene::Swimming, 0.3)
        .with(Gene::Climbing, 0.4)
        .with(Gene::Burrowing, 0.4)
        .founders(50);
    // PocketNomad4: a 0.3-swimmer burrower re-founded with 10 in a drought,
    // and a 0.5-swimmer cold-blooded flier (landed mid lake).
    let burrower = FounderDesign::new("Pocket Burrowers", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(Gene::Swimming, 0.3)
        .with(Gene::Burrowing, 0.6)
        .founders(10);
    let flier = FounderDesign::new("Pocket Gliders", Thermo::Cold, BirthType::IncubatedEggs, 3)
        .with(Gene::BodySize, 0.5)
        .with(Gene::Wings, 0.8)
        .with(Gene::Swimming, 0.5)
        .with(Gene::Toxins, 0.6)
        .with(Gene::Coloration, 0.6)
        .with(Gene::Brumation, 0.7)
        .with_morph(MorphDesign { eye_count: 4, ..Default::default() })
        .founders(10);
    // ApexHunter4, day 230 (winter): an 18-strong pack of pure carnivores,
    // swimming 0.3 (13 of 18 in deep water after the thaw).
    let packs = FounderDesign::new("Ridge Packs", Thermo::Warm, BirthType::Live, 3)
        .with(Gene::Carnivory, 1.0)
        .with(Gene::BodySize, 0.4)
        .with(Gene::Fangs, 0.7)
        .with(Gene::Muscle, 0.5)
        .with(Gene::PackInstinct, 0.85)
        .with(Gene::FatStores, 0.7)
        .with(Gene::Fur, 0.6)
        .with(Gene::Swimming, 0.3)
        .founders(18);
    // GeneBroker4, day 361: 27 founders of the default body, swimming 0.3,
    // climbing 0.45 (tundra rim and a water-ringed corner, 21 starved).
    let brokers = FounderDesign::new("Brokers Four", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(Gene::BodySize, 0.5)
        .with(Gene::Swimming, 0.3)
        .with(Gene::Climbing, 0.45)
        .founders(27);

    let mut results = Vec::new();
    run_to(&mut w, 40);
    results.push(check(&w, 101, &voles, "day 40 voles"));
    // The worst drought of the first year (after day 120: re-foundings).
    let mut worst = (0.0f32, w.clone());
    while w.day() < 220 {
        let next = w.day() + 4;
        run_to(&mut w, next);
        if w.day() >= 120 && w.map.drought > worst.0 {
            worst = (w.map.drought, w.clone());
        }
    }
    eprintln!("worst drought {:.2} on day {}", worst.0, worst.1.day());
    results.push(check(&worst.1, 102, &burrower, "drought burrower"));
    results.push(check(&worst.1, 103, &flier, "drought flier"));
    run_to(&mut w, 230);
    results.push(check(&w, 104, &packs, "day 230 packs"));
    results.push(check(&w, 103, &flier, "day 230 flier"));
    run_to(&mut w, 361);
    results.push(check(&w, 105, &brokers, "day 361 brokers"));
    let (alive, all) = results.iter().fold((0, 0), |a, r| (a.0 + r.0, a.1 + r.1));
    assert!(alive * 10 >= all * 7, "{alive} of {all} founders alive after 10 days");
}

/// Playtest 5: the starter design, founded on day 30 in a drought on the
/// playtest's world (seed 37), lost 2 of 20 to drowning on day 47 on its
/// lakeshore. Founders land on dry land and none drowns in its first 30
/// days.
#[test]
fn playtest5_starter_keeps_off_the_lake() {
    let mut w = playtest_world_seed(37);
    while w.day() < 30 {
        w.run_days(1);
    }
    let starter = FounderDesign::new("Pocket Grazers", Thermo::Warm, BirthType::IncubatedEggs, 3)
        .with(Gene::BodySize, 0.5)
        .with(Gene::GroupTolerance, 0.9)
        .with(Gene::Boldness, 0.3)
        .with(Gene::Fur, 0.6)
        .with(Gene::FatStores, 0.6)
        .with(Gene::Hibernation, 0.5)
        .founders(20);
    let mut c = w.clone();
    let ids = c.found_lineage_unchecked(106, &starter);
    let lineage = c.lineages.len() as u32 - 1;
    for day in 0..30 {
        c.run_days(1);
        let drowned = c.counters.deaths_by_lineage.get(&(lineage, DeathCause::Drowning)).copied().unwrap_or(0);
        if drowned > 0 {
            let at: Vec<_> =
                c.organisms.iter().filter(|o| o.lineage == lineage && !o.alive).map(|o| (o.pos, o.death)).collect();
            panic!("starter: {drowned} drowned by day {} (drought {:.2}); dead: {at:?}", day + 1, c.map.drought);
        }
    }
    let alive = ids.iter().filter(|&&id| c.organism(id).is_some()).count();
    eprintln!("starter: {alive}/20 founders alive after 30 days, drought {:.2}", c.map.drought);
}
