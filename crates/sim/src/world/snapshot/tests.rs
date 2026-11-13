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
    let mut config = WorldConfig { seed: 22, width: 96, height: 96, days_per_year: 16, ..Default::default() };
    config.lod.enabled = true;
    config.scoring.catastrophes = true;
    config.scoring.catastrophe_first_year = 0;
    config.scoring.species_interval_days = 1;
    let mut w = presets::standard_world(config, 1);
    w.set_focus(1, vec![Vec2::new(30.0, 30.0)]);
    w.schedule_catastrophe(CatastropheKind::Epidemic, 1, Some(4));
    let id = w.organisms.iter().find(|o| o.owner == 1).unwrap().id;
    w.possess(1, id).unwrap();
    // Player 2 splits its herd: one half holds ground, the other migrates.
    w.split_group(2, 0, None).unwrap();
    let c = w.organisms.iter().find(|o| o.owner == 2 && o.group == 1).unwrap().pos;
    w.claim_territory(2, 1, c, 6.0).unwrap();
    w.command_migrate_group(2, 0, Vec2::new(10.0, 10.0)).unwrap();
    w
}

/// Give some organisms mutations tagged by a third player, so their
/// genomes carry several origin tags.
fn tag_mutations(w: &mut World) {
    let mut rng = crate::rng::rng_for(21, 99, 0, 0);
    let mutation = crate::genome::MutationParams { rate: 0.1, sd: 0.01 };
    for i in (0..w.organisms.len()).step_by(5) {
        let a = &w.organisms[i].genome;
        let g = crate::Genome::inherit(a, a, 9, mutation, 0.0, &mut rng);
        w.organisms[i].genome = Arc::new(g);
        w.organisms[i].refresh_derived();
    }
}

fn order_possessed(w: &mut World) {
    let o = possessed(w).expect("possessed organism alive");
    let (id, pos) = (o.id, o.pos);
    let target = w.map.clamp(pos + Vec2::new(12.0, -9.0));
    w.order_move(1, id, target).unwrap();
}

/// Pregnancies whose father genome is shared with an organism in the world
/// (written as an index), and those with their own copy (written in full).
fn shared_fathers(w: &World) -> (usize, usize) {
    let genomes: std::collections::HashSet<_> = w.organisms.iter().map(|o| Arc::as_ptr(&o.genome)).collect();
    let fathers = w.organisms.iter().filter_map(|o| o.pregnancy.as_ref());
    fathers.fold((0, 0), |(s, n), p| if genomes.contains(&Arc::as_ptr(&p.father)) { (s + 1, n) } else { (s, n + 1) })
}

/// The genome-derived caches a snapshot leaves out came back exactly.
fn assert_same_caches(a: &World, b: &World) {
    assert_eq!(a.organisms.len(), b.organisms.len());
    for (x, y) in a.organisms.iter().zip(&b.organisms) {
        assert_eq!(x.genome, y.genome, "genome of {}", x.id);
        let bits = |p: &crate::genome::Phenotype| p.0.map(f32::to_bits);
        assert_eq!(bits(&x.pheno), bits(&y.pheno), "phenotype of {}", x.id);
        assert_eq!(format!("{:?}", x.traits), format!("{:?}", y.traits), "traits of {}", x.id);
        assert_eq!(x.recessive_load, y.recessive_load, "recessive load of {}", x.id);
        assert_eq!(x.path, y.path, "path of {}", x.id);
        assert_eq!(x.pregnancy, y.pregnancy, "pregnancy of {}", x.id);
    }
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
    assert_same_caches(a, b);
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
    assert!(pathing > 0, "want paths");
    assert!(alive().any(|o| o.genome.harmful_alleles() > 0), "want harmful recessives");
    tag_mutations(&mut a);
    // Fathers of pregnancies by a re-tagged genome no longer share it.
    let fathers = shared_fathers(&a);
    assert!(fathers.0 > 0 && fathers.1 > 0, "want shared and own father genomes: {fathers:?}");
    assert!(a.organisms.iter().any(|o| o.genome.loci().iter().any(|l| l.a.origin != l.b.origin)), "want mixed tags");

    let bytes = a.snapshot();
    let mut b = World::restore(&bytes, Some(a.map.terrain().clone())).expect("restore");
    assert!(Arc::ptr_eq(a.map.terrain(), b.map.terrain()));
    // The caches left out of the snapshot (phenotype, traits, recessive
    // load) are recomputed on restore to the bit; paths are kept. Father
    // genomes come back shared with the fathers.
    assert_same_caches(&a, &b);
    assert_eq!(shared_fathers(&b), fathers);
    assert_eq!(b.groups(2), a.groups(2), "groups and their orders");
    assert!(b.territory(2, 1).is_some() && b.migration_order(2, 0).is_some());
    // A restored world snapshots to the same bytes.
    assert!(b.snapshot() == bytes, "round trip changes the snapshot");
    // A clone (what a server encodes off its world thread) snapshots to the
    // same bytes and continues the same way.
    let mut c = a.clone();
    assert!(c.snapshot() == bytes, "a clone snapshots differently");
    // The clone shares the static terrain instead of copying it, and the
    // genomes (immutable) too.
    assert!(Arc::ptr_eq(a.map.terrain(), c.map.terrain()));
    assert!(a.organisms.iter().zip(&c.organisms).all(|(x, y)| Arc::ptr_eq(&x.genome, &y.genome)));
    // From the stored terrain blob, too.
    let stored = Arc::new(Terrain::decode(&a.map.terrain().encode()).unwrap());
    let mut d = World::restore(&bytes, Some(stored)).expect("restore from a stored terrain");

    let n = k + 2 * tpd + 17;
    while a.tick() < n {
        a.step();
        b.step();
        c.step();
        d.step();
    }
    assert_same(&a, &b);
    assert_same(&a, &c);
    assert_same(&a, &d);
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
    assert_eq!(h.terrain, Some(w.map.terrain().hash()));
    let terrain = || Some(w.map.terrain().clone());

    // Another format version is refused, with both versions named.
    let mut other = bytes.clone();
    other[8..12].copy_from_slice(&(SNAPSHOT_VERSION + 1).to_le_bytes());
    assert_eq!(
        World::restore(&other, terrain()).err(),
        Some(SnapshotError::Version { found: SNAPSHOT_VERSION + 1, expected: SNAPSHOT_VERSION })
    );
    // Corruption and truncation are caught by the checksum.
    let mut corrupt = bytes.clone();
    let mid = corrupt.len() / 2;
    corrupt[mid] ^= 0x55;
    assert_eq!(World::restore(&corrupt, terrain()).err(), Some(SnapshotError::Checksum));
    assert_eq!(World::restore(&bytes[..bytes.len() - 10], terrain()).err(), Some(SnapshotError::Checksum));
    assert_eq!(World::restore(b"not a snapshot", None).err(), Some(SnapshotError::BadHeader));

    // Config compatibility: balance may change, the map may not.
    let mut tuned = w.config.clone();
    tuned.balance.generalist_tax += 0.1;
    assert!(w.config.snapshot_compatible(&tuned));
    assert_ne!(w.config.snapshot_hash(), tuned.snapshot_hash());
    let resized = WorldConfig { width: 64, ..w.config.clone() };
    assert!(!w.config.snapshot_compatible(&resized));
}

/// A small world, 1 day and 7 ticks in.
fn fixture_world() -> World {
    let mut w = World::new(WorldConfig { seed: 3, width: 32, height: 32, ..Default::default() });
    for (owner, design) in presets::wild_designs().into_iter().take(2) {
        w.found_lineage_unchecked(owner, &design.founders(6));
    }
    w.run_days(1);
    for _ in 0..7 {
        w.step();
    }
    w
}

#[test]
fn terrain_is_stored_apart_and_must_be_the_right_one() {
    let w = fixture_world();
    let bytes = w.snapshot();
    let hash = w.map.terrain().hash();
    // No terrain, or another map's: refused, naming the one it needs.
    assert_eq!(World::restore(&bytes, None).err(), Some(SnapshotError::MissingTerrain { hash }));
    let other = World::new(WorldConfig { seed: 4, width: 32, height: 32, ..Default::default() });
    let found = other.map.terrain().hash();
    assert_ne!(found, hash);
    assert_eq!(
        World::restore(&bytes, Some(other.map.terrain().clone())).err(),
        Some(SnapshotError::WrongTerrain { expected: hash, found })
    );

    // The terrain blob round-trips, hash included, and restores the world.
    let blob = w.map.terrain().encode();
    assert_eq!(Terrain::encoded_hash(&blob), Ok(hash));
    let t = Terrain::decode(&blob).unwrap();
    assert!(t == **w.map.terrain());
    assert_eq!(t.hash(), hash);
    let r = World::restore(&bytes, Some(Arc::new(t))).unwrap();
    assert!(r.snapshot() == bytes);
    // Corrupt, truncated, foreign and future blobs are refused.
    let mut corrupt = blob.clone();
    let mid = corrupt.len() / 2;
    corrupt[mid] ^= 0x55;
    assert_eq!(Terrain::decode(&corrupt).err(), Some(SnapshotError::Checksum));
    assert_eq!(Terrain::decode(&blob[..blob.len() - 3]).err(), Some(SnapshotError::Checksum));
    assert_eq!(Terrain::decode(&bytes).err(), Some(SnapshotError::BadHeader));
    let mut future = blob.clone();
    future[8..12].copy_from_slice(&(TERRAIN_VERSION + 1).to_le_bytes());
    assert_eq!(
        Terrain::decode(&future).err(),
        Some(SnapshotError::Version { found: TERRAIN_VERSION + 1, expected: TERRAIN_VERSION })
    );

    // Editing the terrain unshares it and changes its hash.
    let mut edited = w.clone();
    edited.map.edit_terrain().ground[0].elevation += 0.01;
    assert_ne!(edited.map.terrain().hash(), hash);
    assert!(!Arc::ptr_eq(edited.map.terrain(), w.map.terrain()));
    assert_eq!(w.map.terrain().hash(), hash);
}

/// Snapshots of older versions (terrain embedded until version 5; no
/// lakes, floodplains or wind direction before 6; full organisms before 7)
/// are refused, naming the version, even when the header is readable.
#[test]
fn older_snapshots_are_refused_by_version() {
    let bytes = fixture_world().snapshot();
    // Version 6 has the terrain hash in its header.
    let mut b = bytes.clone();
    b[8..12].copy_from_slice(&6u32.to_le_bytes());
    let h = SnapshotHeader::read(&b).unwrap();
    assert_eq!((h.version, h.terrain), (6, Some(fixture_world().map.terrain().hash())));
    assert_eq!(
        World::restore(&b, Some(fixture_world().map.terrain().clone())).err(),
        Some(SnapshotError::Version { found: 6, expected: SNAPSHOT_VERSION })
    );
    for old in [4u32, 5] {
        let mut b = bytes.clone();
        b[8..12].copy_from_slice(&old.to_le_bytes());
        // Their header has no terrain hash.
        let h = SnapshotHeader::read(&b).unwrap();
        assert_eq!((h.version, h.terrain), (old, None));
        assert_eq!(
            World::restore(&b, None).err(),
            Some(SnapshotError::Version { found: old, expected: SNAPSHOT_VERSION })
        );
    }
}
