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
/// species detection. The seed is one whose world has every kind of state
/// the test asserts (seed 22 lost its LOD cohorts when the pack-hunter
/// design shrank).
fn busy_world() -> World {
    let mut config = WorldConfig { seed: 23, width: 96, height: 96, days_per_year: 16, ..Default::default() };
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
/// genomes carry several origin tags. Every fifth organism is re-tagged, and
/// the father of the first pregnancy, so that some pregnancy's father genome
/// is no longer shared with anyone in the world.
fn tag_mutations(w: &mut World) {
    let mut rng = crate::rng::rng_for(21, 99, 0, 0);
    let mutation = crate::genome::MutationParams { rate: 0.1, sd: 0.01 };
    let first_father = w
        .organisms
        .iter()
        .filter_map(|o| o.pregnancy.as_ref())
        .find_map(|p| w.organisms.iter().position(|f| Arc::ptr_eq(&f.genome, &p.father)));
    let every_fifth = (0..w.organisms.len()).step_by(5).filter(|&i| Some(i) != first_father);
    for i in first_father.into_iter().chain(every_fifth) {
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
    // Fed, watered and awake, so no need overrides its order.
    let i = a.organisms.iter().position(|o| o.alive && o.possessed_by == Some(1)).expect("possessed organism alive");
    let o = &mut a.organisms[i];
    (o.needs.hunger, o.needs.thirst, o.needs.energy, o.needs.safety) = (10.0, 10.0, 90.0, 0.0);
    o.dormancy = Dormancy::Awake;
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

/// `w` as a snapshot from before `Gene::Obedience` restores it: every
/// genome's obedience locus at the design default with its wanderlust
/// alleles' tags (pregnancies' fathers and cohort pools included), species
/// representatives at the default, and `tameness` at its default.
fn without_obedience(w: &World) -> World {
    let mut w = World::restore(&w.snapshot_v13(), Some(w.map.terrain().clone())).unwrap();
    w.config.balance.tameness = crate::config::Balance::default().tameness;
    w
}

/// Version 10 snapshots (the tiles inside the payload, as builds before
/// version 11 wrote them, no `Balance::cold_gestation` and no obedience)
/// still restore, to the same world, with the rule they ran under.
#[test]
fn version_10_snapshots_still_restore() {
    let mut w = fixture_world();
    w.config.balance.cold_gestation = crate::serde_util::before_v12::OLD_COLD_GESTATION;
    w.config.balance.nest_predation = crate::serde_util::before_v13::OLD_NEST_PREDATION;
    let w = without_obedience(&w);
    let old = w.snapshot_v10();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!((h.version, h.flags), (10, FLAG_LZ4));
    assert!(h.restorable());
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert!(r.map.tiles == w.map.tiles);
    assert!(r.snapshot() == w.snapshot());
    // A newer snapshot is smaller: the tiles compress better apart.
    assert!(w.snapshot().len() < old.len());
}

/// Version 12 snapshots (no `Balance::nest_predation`) still restore, to
/// the same world, with the rule they ran under (no egg predation on
/// nesting grounds).
#[test]
fn version_12_snapshots_still_restore() {
    let mut w = fixture_world();
    w.config.balance.nest_predation = crate::serde_util::before_v13::OLD_NEST_PREDATION;
    let w = without_obedience(&w);
    let old = w.snapshot_v12();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 12);
    assert!(h.restorable());
    // Without the parameter the payload is 4 bytes (one f32) shorter than
    // a version 13 one.
    assert_eq!(h.raw_len + 4, SnapshotHeader::read(&w.snapshot_v13()).unwrap().raw_len);
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.nest_predation, 0.0);
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
}

/// Version 13 snapshots (before `Gene::Obedience` and `Balance::tameness`)
/// restore with every obedience locus at the design default, tagged like
/// the organism's wanderlust alleles, so their organisms behave as before;
/// restored again they give the same world.
#[test]
fn version_13_snapshots_restore_with_default_obedience() {
    use crate::genome::Gene;
    let mut w = fixture_world();
    // Some organisms carry other obedience alleles: the old form drops them.
    for o in w.organisms.iter_mut().step_by(3) {
        let mut g = (*o.genome).clone();
        g.set_gene_alleles(Gene::Obedience, 0.9, 0.1);
        o.genome = Arc::new(g);
        o.refresh_derived();
    }
    w.config.balance.tameness = 0.9;
    let old = w.snapshot_v13();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!((h.version, h.flags), (13, FLAG_LZ4 | FLAG_TILES_APART));
    assert!(h.restorable());
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.tameness, crate::config::Balance::default().tameness);
    assert_eq!(r.organisms.len(), w.organisms.len());
    for (a, b) in r.organisms.iter().zip(&w.organisms) {
        let (o, wl) = (a.genome.locus(Gene::Obedience), a.genome.locus(Gene::Wanderlust));
        assert_eq!((o.a.value, o.b.value), (0.5, 0.5));
        assert_eq!((o.a.origin, o.b.origin), (wl.a.origin, wl.b.origin));
        assert_eq!(a.pheno[Gene::Obedience], 0.5);
        for &g in Gene::ALL.iter().filter(|&&g| g != Gene::Obedience) {
            assert_eq!(a.genome.locus(g), b.genome.locus(g), "{g:?} of {}", a.id);
        }
    }
    let again = World::restore(&r.snapshot(), Some(w.map.terrain().clone())).unwrap();
    assert!(again.snapshot() == r.snapshot());
    assert!(r.snapshot_v13() == old);
}

/// The tile planes round-trip every bit pattern (negative zero, NaN,
/// subnormals), on maps whose width is not a power of two.
#[test]
fn tiles_round_trip_bit_exactly() {
    let odd = [0.0f32, -0.0, f32::NAN, f32::from_bits(1), f32::MAX, -1.5, 0.1];
    for (width, height) in [(1, 1), (7, 5), (13, 1), (1, 9)] {
        let tiles: Vec<Tile> = (0..width * height)
            .map(|i| {
                let b: [u32; TILE_FIELDS] =
                    std::array::from_fn(|f| odd[(i * 3 + f) % odd.len()].to_bits() ^ (i as u32 * 0x9e37));
                tile_from_bits(b)
            })
            .collect();
        let back = decode_tiles(&encode_tiles(&tiles, width), width, height).unwrap();
        assert!(tiles.iter().zip(&back).all(|(a, b)| tile_bits(a) == tile_bits(b)), "{width}x{height}");
        // Another map size is refused.
        assert!(decode_tiles(&encode_tiles(&tiles, width), width + 1, height).is_err());
    }
}
