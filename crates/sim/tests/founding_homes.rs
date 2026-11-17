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
        let config = WorldConfig { seed, width: size, height: size, ..Default::default() };
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
/// fox in its own niche*): the search is unchanged, the fox's homes moved.
const EXPECTED: u64 = 0xfe77_2129_f5d8_f371;
