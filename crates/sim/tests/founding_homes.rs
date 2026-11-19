//! Founding's home search (`World::found_lineage`) picks the same homes as
//! before it was made cheap: a hash of every founder's position over many
//! worlds, founded empty and into grown worlds (threats and prey around),
//! against the value the earlier search gave.

use evo_sim::{presets, WorldConfig};

fn mix(h: &mut u64, v: u64) {
    *h = (*h ^ v).wrapping_mul(0x100_0000_01b3);
}

fn homes_hash() -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for seed in 1..=12u64 {
        let size = [64, 96, 128][seed as usize % 3];
        let mut config = WorldConfig { seed, width: size, height: size, ..Default::default() };
        // The search itself, without the playtest 1 fixes: founding by food
        // (`Balance::founding_food`) moves homes in grown worlds by design,
        // and `desperate_risk` changes the days the world grows first.
        config.balance.founding_food = 0.0;
        config.balance.desperate_risk = 0.0;
        // Pack hunts (version 36) change the days the world grows first too.
        config.balance.pack_hunt = 0.0;
        // Version 37: dry founding sites move homes by design; the others
        // change the days the world grows first.
        config.balance.dry_sites = 0.0;
        config.balance.drink_priority = 0.0;
        config.balance.toxic_learning = 0.0;
        config.balance.carnivore_energetics = 0.0;
        // Playtest 3 (version 47): the search judging current conditions
        // moves homes in grown worlds by design (tests in `world::mod`), and
        // thirsty animals finding a reachable shore changes the days the
        // world grows first.
        config.balance.founding_conditions = 0.0;
        // Playtest 4 (version 50): settling on dry land moves homes by design
        // (`tests/founding_safety.rs`).
        config.balance.founding_safety = 0.0;
        config.balance.reachable_water = 0.0;
        // Playtest 4 (version 48): pack larders and sleepers' metabolic
        // water change the days the world grows first.
        config.balance.player_packs = 0.0;
        config.balance.dormant_water = 0.0;
        config.balance.possessed_sleep = 0.0;
        // Playtest 5 (version 57): dry beds and the farther drink search
        // change the days the world grows first.
        config.balance.dry_beds = 0.0;
        config.balance.drink_reach = 0.0;
        // Playtest 6 (version 58): the prey-scaled pack larder.
        config.balance.prey_larder = 0.0;
        // Playtest 5 (version 52): brooders keeping their nests, young
        // following and escaping rising water change the days the world
        // grows first.
        config.balance.brood_care = 0.0;
        // Playtest 6 (version 58): drought foundings move homes by design;
        // watered founders and no winter clutches change the days the world
        // grows first.
        config.balance.founding_drought = 0.0;
        // Playtest 7 (version 68): rival-aware homes move foundings by
        // design; estivating through spring and autumn droughts changes
        // the days the world grows first.
        config.balance.founding_rivals = 0.0;
        // Playtest 6 balance (version 62): toxin upkeep, hungry hunters,
        // toxin source, speciation credit and clutch cover change how the
        // world grows before the sample players found.
        config.balance.toxin_upkeep = 0.0;
        config.balance.hungry_hunters = 0.0;
        config.balance.toxin_source = 0.0;
        config.balance.speciation_credit = 0.0;
        config.balance.clutch_cover = 0.0;
        // Founded empty: the wild ecosystem, then the sample players.
        let mut w = presets::standard_world(config, 1);
        for o in &w.organisms {
            mix(&mut h, o.pos.x.to_bits() as u64);
            mix(&mut h, o.pos.y.to_bits() as u64);
        }
        // Founded into a grown world: each sample design again, and the
        // niche predators, on copies of it.
        w.run_days(3);
        let mut designs = presets::player_designs();
        designs.extend([presets::niche_fox(), presets::niche_pack_hunters()]);
        for (k, design) in designs.iter().enumerate() {
            let mut c = w.clone();
            for id in c.found_lineage_unchecked(k as u32 + 1, design) {
                let o = c.organism(id).expect("founder alive");
                mix(&mut h, o.pos.x.to_bits() as u64);
                mix(&mut h, o.pos.y.to_bits() as u64);
            }
        }
    }
    h
}

#[test]
fn founding_picks_the_same_homes() {
    let h = homes_hash();
    eprintln!("homes hash {h:016x}");
    assert_eq!(h, EXPECTED, "founding chose different homes: {h:016x}");
}

/// From the home search before `World::passable_regions` used
/// `terrain::passable` and run labelling (commit c50ca26: 0x43e1_cbe2_a1a0_2b21),
/// re-recorded when the standard fox's design changed (carnivory 0.6, *The
/// fox in its own niche*): the search is unchanged, the fox's homes moved;
/// and again when the toxic frogs went from 46 to 42 founders (brumation's
/// price, playtest 3: 0xfe77_2129_f5d8_f371 before).
const EXPECTED: u64 = 0xf8fa_5a60_7113_5d10;
