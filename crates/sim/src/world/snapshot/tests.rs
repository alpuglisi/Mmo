use super::*;
use crate::chronicle::CatastropheKind;
use crate::math::Vec2;
use crate::organism::{Dormancy, Infection};
use crate::presets;

/// Player 1's possessed organism (it keeps its region active under LOD).
fn possessed(w: &World) -> Option<&crate::organism::Organism> {
    w.organisms.iter().find(|o| o.alive && o.possessed_by == Some(1))
}

/// A world exercising as much state as possible: level of detail with a
/// focus, catastrophes (an epidemic early on), short years so winter
/// dormancy comes quickly, a possessed organism with a move order, daily
/// species detection.
fn busy_world() -> World {
    let mut config = WorldConfig { seed: 21, width: 96, height: 96, days_per_year: 16, ..Default::default() };
    config.lod.enabled = true;
    config.scoring.catastrophes = true;
    config.scoring.catastrophe_first_year = 0;
    config.scoring.species_interval_days = 1;
    let mut w = presets::standard_world(config, 1);
    w.set_focus(1, vec![Vec2::new(30.0, 30.0)]);
    w.schedule_catastrophe(CatastropheKind::Epidemic, 1, Some(4));
    let id = w.organisms.iter().find(|o| o.owner == 1).unwrap().id;
    w.possess(1, id).unwrap();
    w
}

fn order_possessed(w: &mut World) {
    let o = possessed(w).expect("possessed organism alive");
    let (id, pos) = (o.id, o.pos);
    let target = w.map.clamp(pos + Vec2::new(12.0, -9.0));
    w.order_move(1, id, target).unwrap();
}

/// Everything a divergence would show up in first, for a readable failure.
fn assert_same(a: &World, b: &World) {
    assert_eq!(a.tick(), b.tick());
    assert_eq!(a.organisms.len(), b.organisms.len(), "organism count");
    for (x, y) in a.organisms.iter().zip(&b.organisms) {
        assert_eq!(x.id, y.id);
        assert_eq!((x.alive, x.pos, x.action, x.dormancy), (y.alive, y.pos, y.action, y.dormancy), "organism {}", x.id);
        assert_eq!(x.needs, y.needs, "needs of {}", x.id);
        assert_eq!(x.health, y.health, "health of {}", x.id);
    }
    assert_eq!(a.next_id, b.next_id);
    assert_eq!(format!("{:?}", a.counters), format!("{:?}", b.counters), "counters");
    assert_eq!(a.legacy, b.legacy, "legacy");
    assert_eq!(a.clutches.len(), b.clutches.len());
    assert_eq!(a.population_counts(), b.population_counts(), "population incl. cohorts");
    assert_eq!(a.chronicle.catastrophes, b.chronicle.catastrophes);
    assert_eq!(a.chronicle.species.species, b.chronicle.species.species);
    // And the whole state, byte for byte (hash maps are written sorted).
    assert!(a.snapshot() == b.snapshot(), "snapshots differ");
}

#[test]
fn restored_world_continues_exactly() {
    let tpd = 48u64;
    let mut a = busy_world();
    assert_eq!(a.config.ticks_per_day as u64, tpd);
    // Into winter (day 12 of 16), mid-hour.
    let k = 13 * tpd + 5;
    while a.tick() < k {
        if a.tick().is_multiple_of(100) {
            order_possessed(&mut a);
        }
        a.step();
    }
    order_possessed(&mut a);
    // Between ticks, part way through the possession sub-steps.
    assert_eq!(a.substep().len(), 1, "want the possessed organism to move in a sub-step");

    // The snapshot holds the state the test is meant to cover.
    let alive = || a.organisms.iter().filter(|o| o.alive);
    let pregnant = alive().filter(|o| o.pregnancy.is_some()).count();
    let infected = alive().filter(|o| o.infection != Infection::Susceptible).count();
    let dormant = alive().filter(|o| o.dormancy != Dormancy::Awake).count();
    let caching = alive().filter(|o| o.cache > 0.0).count();
    let pathing = alive().filter(|o| o.path.is_some()).count();
    let cohorts = a.cohorts().count();
    eprintln!(
        "tick {k}: {} agents, {pregnant} pregnant, {} clutches, {infected} infected, {dormant} dormant, \
         {caching} caching, {pathing} with paths, {cohorts} cohorts, {} catastrophes, {} species",
        a.alive_count(),
        a.clutches.len(),
        a.chronicle.catastrophes.len(),
        a.chronicle.species.species.len()
    );
    assert!(pregnant > 0 && !a.clutches.is_empty(), "want a pregnancy and a clutch");
    assert!(infected > 0, "want disease");
    assert!(dormant > 0, "want dormancy");
    assert!(caching > 0, "want caches");
    assert!(cohorts > 0, "want LOD cohorts");
    assert!(a.chronicle.catastrophes.iter().any(|c| c.started), "want a catastrophe");
    assert!(possessed(&a).is_some_and(|o| o.order.is_some()), "want a possessed organism with an order");

    let bytes = a.snapshot();
    let mut b = World::restore(&bytes).expect("restore");
    // A restored world snapshots to the same bytes.
    assert!(b.snapshot() == bytes, "round trip changes the snapshot");
    // A clone (what a server encodes off its world thread) snapshots to the
    // same bytes and continues the same way.
    let mut c = a.clone();
    assert!(c.snapshot() == bytes, "a clone snapshots differently");

    let n = k + 2 * tpd + 17;
    while a.tick() < n {
        a.step();
        b.step();
        c.step();
    }
    assert_same(&a, &b);
    assert_same(&a, &c);
}

#[test]
fn header_and_errors() {
    let mut w = World::new(WorldConfig { seed: 3, width: 32, height: 32, ..Default::default() });
    for (owner, design) in presets::wild_designs().into_iter().take(2) {
        w.found_lineage_unchecked(owner, &design.founders(6));
    }
    w.run_days(1);
    let bytes = w.snapshot();
    let h = SnapshotHeader::read(&bytes).unwrap();
    assert_eq!((h.version, h.tick, h.organisms), (SNAPSHOT_VERSION, w.tick(), w.organisms.len() as u64));
    assert_eq!(h.config_hash, w.config.snapshot_hash());

    // Another format version is refused, with both versions named.
    let mut other = bytes.clone();
    other[8..12].copy_from_slice(&(SNAPSHOT_VERSION + 1).to_le_bytes());
    assert_eq!(
        World::restore(&other).err(),
        Some(SnapshotError::Version { found: SNAPSHOT_VERSION + 1, expected: SNAPSHOT_VERSION })
    );
    // Corruption and truncation are caught by the checksum.
    let mut corrupt = bytes.clone();
    let mid = corrupt.len() / 2;
    corrupt[mid] ^= 0x55;
    assert_eq!(World::restore(&corrupt).err(), Some(SnapshotError::Checksum));
    assert_eq!(World::restore(&bytes[..bytes.len() - 10]).err(), Some(SnapshotError::Checksum));
    assert_eq!(World::restore(b"not a snapshot").err(), Some(SnapshotError::BadHeader));

    // Config compatibility: balance may change, the map may not.
    let mut tuned = w.config.clone();
    tuned.balance.generalist_tax += 0.1;
    assert!(w.config.snapshot_compatible(&tuned));
    assert_ne!(w.config.snapshot_hash(), tuned.snapshot_hash());
    let resized = WorldConfig { width: 64, ..w.config.clone() };
    assert!(!w.config.snapshot_compatible(&resized));
}
