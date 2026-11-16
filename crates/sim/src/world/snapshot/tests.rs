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
/// design shrank; seed 23 its second pregnancy when the morphology rules
/// came on by default).
fn busy_world() -> World {
    let mut config = WorldConfig { seed: 29, width: 96, height: 96, days_per_year: 16, ..Default::default() };
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
        let mut g = crate::Genome::inherit(a, a, 9, mutation, 0.0, &mut rng);
        if i % 2 == 0 {
            g.set_morph(odd_morph(&g, 9));
        }
        let o = &mut w.organisms[i];
        o.genome = Arc::new(g);
        o.refresh_derived();
        // As `World::insert` and a restore do: the body plan's effects.
        crate::morph_effects::apply(&mut o.traits, &o.pheno, &o.morph, &w.config.balance);
    }
}

/// A non-default morphology segment for `g`: one allele of each discrete
/// locus changed (hidden, wild type dominant) and the eyes, legs and tail
/// shape changed in both (shown), the changes tagged `owner`.
fn odd_morph(g: &crate::Genome, owner: crate::PlayerId) -> crate::morph::MorphGenes {
    use crate::morph::{MorphAllele, MorphLocus};
    let mut m = *g.morph();
    for l in MorphLocus::ALL {
        let p = &mut m.discrete[l.index()];
        let other = l.value_at(l.position(l.default_value()) + 1);
        p.b = MorphAllele { value: other, origin: owner };
        if matches!(l, MorphLocus::EyeCount | MorphLocus::LegCount | MorphLocus::TailShape) {
            p.a = p.b;
        }
    }
    m.continuous[0].a.value = 0.9;
    m
}

fn order_possessed(w: &mut World) {
    let o = possessed(w).expect("possessed organism alive");
    let (id, pos) = (o.id, o.pos);
    let target = w.map.clamp(pos + Vec2::new(12.0, -9.0));
    w.order_move(1, id, target).unwrap();
}

/// Genomes a snapshot writes against their parents' (organisms with both
/// parents earlier in the list, eggs with both parents in the world, and
/// mostly inherited from them), and those it writes whole.
fn by_parents(w: &World) -> (usize, usize, usize) {
    let at: std::collections::HashMap<_, _> = w.organisms.iter().enumerate().map(|(k, o)| (o.id, k)).collect();
    let (mut orgs, mut eggs, mut whole) = (0, 0, 0);
    for (k, o) in w.organisms.iter().enumerate() {
        let parent =
            |id: Option<u64>| id.and_then(|id| at.get(&id)).filter(|&&p| p < k).map(|&p| &w.organisms[p].genome);
        match (parent(o.mother), parent(o.father)) {
            (Some(m), Some(f)) if o.genome.to_wire_by_parents(m, f).is_some() => orgs += 1,
            _ => whole += 1,
        }
    }
    for c in &w.clutches {
        let parent = |id: u64| at.get(&id).map(|&p| &w.organisms[p].genome);
        for e in &c.eggs {
            match (parent(c.mother), parent(c.father)) {
                (Some(m), Some(f)) if e.to_wire_by_parents(m, f).is_some() => eggs += 1,
                _ => whole += 1,
            }
        }
    }
    (orgs, eggs, whole)
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
        assert_eq!(x.morph, y.morph, "morphology of {}", x.id);
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
    // Into winter (day 12 of 16), mid-hour, with a clutch laid.
    let k = 13 * tpd + 29;
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
    // Genomes written against their parents' (organisms and eggs) and whole.
    let (orgs, eggs, whole) = by_parents(&a);
    eprintln!("genomes by parents: {orgs} organisms, {eggs} eggs; {whole} whole");
    assert!(orgs > 0 && eggs > 0 && whole > 0, "want genomes by parents and whole: {orgs}, {eggs}, {whole}");

    // Player 4's main group answers a rally.
    let rp = a.organisms.iter().find(|o| o.alive && o.owner == 4 && o.group == 0).expect("player 4 alive").pos;
    a.rally_group(4, 0, rp).unwrap();

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
    assert!(b.rally(4, 0).is_some() && b.rally(4, 0) == a.rally(4, 0));
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

/// The morphology rules as they were before they were on by default
/// (version 24): body plan effects, directional vision, step mutation and
/// morphology alleles in the legacy tally all off, which is what snapshots
/// older than each of them read; and roaming hunters eating nothing, as
/// before version 25 (`Balance::roaming_forage`); and the carnivore-diet
/// rules and the predator brakes off, as before versions 26 and 27
/// ([`carnivore_diet_off`]). Worlds that
/// stand for older saves use them, so a restore from an older version
/// compares equal.
fn old_morph_rules(mut config: WorldConfig) -> WorldConfig {
    config.balance = config.balance.morphology_off();
    config.balance.roaming_forage = crate::serde_util::before_v25::OLD_VALUE;
    carnivore_diet_off(&mut config.balance);
    config
}

/// The carnivore-diet tunables as snapshots before version 26 read them,
/// and the predator brakes as snapshots before version 27 read them.
fn carnivore_diet_off(b: &mut crate::Balance) {
    let old = crate::serde_util::before_v26::OLD_VALUE;
    b.carcass_keep = old;
    b.carnivore_fat = old;
    b.carcass_scent = old;
    b.carrion = old;
    b.provisioning = old;
    b.breeding_territory = old;
    predator_brakes_off(b);
}

/// The predator brakes as snapshots before version 27 read them.
fn predator_brakes_off(b: &mut crate::Balance) {
    let old = crate::serde_util::before_v27::OLD_VALUE;
    b.prey_switching = old;
    b.condition_litter = old;
}

/// A small world, 1 day and 7 ticks in, under the old morphology rules
/// ([`old_morph_rules`]).
fn fixture_world() -> World {
    let mut w = World::new(old_morph_rules(WorldConfig { seed: 3, width: 32, height: 32, ..Default::default() }));
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
    w.config.balance.migration_onset = crate::serde_util::before_v16::OLD_MIGRATION_ONSET;
    w
}

/// `w` as a snapshot from before the morphology segment restores it: every
/// organism with the default morphology, tagged like its anchor genes.
fn without_morph(w: World) -> World {
    World::restore(&w.snapshot_v22(), Some(w.map.terrain().clone())).unwrap()
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
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
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
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
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
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
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

/// Give a world restored from a snapshot older than version 20 the
/// version 20 parameters (`Balance::fang_grazing`, `scale_cover`) of `w`,
/// which it read as 0.
fn with_v20_params(r: &mut World, w: &World) {
    let off = crate::serde_util::before_v20::OLD_VALUE;
    assert_eq!((r.config.balance.fang_grazing, r.config.balance.scale_cover), (off, off));
    r.config.balance.fang_grazing = w.config.balance.fang_grazing;
    r.config.balance.scale_cover = w.config.balance.scale_cover;
}

/// Version 14 snapshots (before the rally command) restore with no rally
/// standing, and the rest of the world as it was.
#[test]
fn version_14_snapshots_restore_without_rallies() {
    let mut w = without_morph(fixture_world());
    let plain = w.snapshot();
    let p = w.organisms.iter().find(|o| o.alive).unwrap();
    let (owner, group, pos) = (p.owner, p.group, p.pos);
    w.rally_group(owner, group, pos).unwrap();
    let old = w.snapshot_v14();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!((h.version, h.flags), (14, FLAG_LZ4 | FLAG_TILES_APART));
    assert!(h.restorable());
    let mut r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    assert_eq!(r.rally(owner, group), None);
    // Version 14 also predates `migration_onset` (version 16).
    r.config.balance.migration_onset = w.config.balance.migration_onset;
    with_v20_params(&mut r, &w);
    assert!(r.snapshot() == plain);
    assert!(r.snapshot_v14() == old);
}

/// Version 15 snapshots (before `Balance::migration_onset`) restore with it
/// at 1, the rule they ran under, and are otherwise the same world.
#[test]
fn version_15_snapshots_restore_without_migration_onset() {
    let mut w = without_morph(fixture_world());
    w.config.balance.migration_onset = 0.3;
    let old = w.snapshot_v15();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 15);
    assert!(h.restorable());
    // Without the parameter the payload is 4 bytes (one f32) shorter, and
    // 12 more without the home range's (version 17).
    assert_eq!(h.raw_len + 4 + 12, SnapshotHeader::read(&w.snapshot_v17()).unwrap().raw_len);
    let mut r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    with_v20_params(&mut r, &w);
    assert_eq!(r.config.balance.migration_onset, crate::serde_util::before_v16::OLD_MIGRATION_ONSET);
    w.config.balance.migration_onset = r.config.balance.migration_onset;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
}

/// Version 16 snapshots (before `Balance::home_range`) restore with its
/// defaults, and are otherwise the same world.
#[test]
fn version_16_snapshots_restore_without_the_home_range() {
    let mut w = without_morph(fixture_world());
    w.config.balance.home_range.predation = 0.9;
    let old = w.snapshot_v16();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 16);
    assert!(h.restorable());
    // Without the parameters the payload is 12 bytes (three f32) shorter.
    assert_eq!(h.raw_len + 12, SnapshotHeader::read(&w.snapshot_v17()).unwrap().raw_len);
    let mut r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    with_v20_params(&mut r, &w);
    assert_eq!(r.config.balance.home_range, crate::config::HomeRangeBalance::default());
    w.config.balance.home_range = r.config.balance.home_range;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
    assert!(r.snapshot_v16() == old);
}

/// A world laying eggs and raising young, for the genomes written against
/// their parents': day 9 of a small world, under the old morphology rules.
fn laying_world() -> World {
    let config = old_morph_rules(WorldConfig { seed: 5, width: 64, height: 64, ..Default::default() });
    let mut w = presets::standard_world(config, 1);
    w.run_days(9);
    w
}

/// Version 17 snapshots (every genome written whole) still restore, to the
/// same world; written against their parents, the genomes of young and eggs
/// take a fraction of the space.
#[test]
fn version_17_snapshots_restore_with_whole_genomes() {
    let w = without_morph(laying_world());
    let (orgs, eggs, whole) = by_parents(&w);
    assert!(orgs > 0 && eggs > 0 && whole > 0, "want genomes by parents and whole: {orgs}, {eggs}, {whole}");
    let old = w.snapshot_v17();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!((h.version, h.flags), (17, FLAG_LZ4 | FLAG_TILES_APART));
    assert!(h.restorable());
    let mut r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    with_v20_params(&mut r, &w);
    assert_same_caches(&w, &r);
    assert!(r.clutches.iter().zip(&w.clutches).all(|(a, b)| a.eggs == b.eggs));
    let new = w.snapshot();
    assert!(r.snapshot() == new);
    assert!(r.snapshot_v17() == old);
    // Smaller (more so in bigger worlds, where eggs are a third of a save).
    let (o, n) = (h.raw_len, SnapshotHeader::read(&new).unwrap().raw_len);
    eprintln!("raw payload: version 17 {o} B, version 18 {n} B; stored {} B -> {} B", old.len(), new.len());
    assert!(n < o && new.len() < old.len());
}

/// Version 18 snapshots (before `Balance::lactation` and
/// `Balance::warm_upkeep`) restore with both at 0, the rules they ran under,
/// and are otherwise the same world.
#[test]
fn version_18_snapshots_restore_without_warm_costs() {
    let mut w = without_morph(fixture_world());
    w.config.balance.lactation = 0.7;
    w.config.balance.warm_upkeep = 0.25;
    let old = w.snapshot_v18();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 18);
    assert!(h.restorable());
    // Without the two parameters the payload is 8 bytes (two f32s) shorter
    // than version 19's.
    assert_eq!(h.raw_len + 8, SnapshotHeader::read(&w.snapshot_v19()).unwrap().raw_len);
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    let off = crate::serde_util::before_v19::OLD_VALUE;
    assert_eq!((r.config.balance.lactation, r.config.balance.warm_upkeep), (off, off));
    w.config.balance.lactation = off;
    w.config.balance.warm_upkeep = off;
    // Version 18 predates version 20's parameters too.
    w.config.balance.fang_grazing = crate::serde_util::before_v20::OLD_VALUE;
    w.config.balance.scale_cover = crate::serde_util::before_v20::OLD_VALUE;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
    assert!(r.snapshot_v18() == old);
}

/// Version 19 snapshots (before `Balance::fang_grazing` and
/// `Balance::scale_cover`) restore with both at 0, the rules they ran under,
/// and are otherwise the same world.
#[test]
fn version_19_snapshots_restore_without_trait_costs() {
    let w = without_morph(fixture_world());
    assert!(w.config.balance.fang_grazing > 0.0 && w.config.balance.scale_cover > 0.0);
    let old = w.snapshot_v19();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 19);
    assert!(h.restorable());
    // Without the two parameters the payload is 8 bytes (two f32s) shorter.
    assert_eq!(h.raw_len + 8, SnapshotHeader::read(&w.snapshot_v20()).unwrap().raw_len);
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    let off = crate::serde_util::before_v20::OLD_VALUE;
    assert_eq!((r.config.balance.fang_grazing, r.config.balance.scale_cover), (off, off));
    let mut w = w;
    w.config.balance.fang_grazing = off;
    w.config.balance.scale_cover = off;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
    assert!(r.snapshot_v20() == w.snapshot_v20());
    assert!(r.snapshot_v19() == old);
}

/// Version 20 snapshots (before the `Balance::morph_*` tunables) restore
/// with all four at 0, the rules they ran under, and are otherwise the same
/// world.
#[test]
fn version_20_snapshots_restore_without_morph_effects() {
    let mut w = without_morph(fixture_world());
    w.config.balance.morph_effects = 1.0;
    w.config.balance.morph_tail_drag = 0.5;
    w.config.balance.morph_tail_chill = 0.5;
    w.config.balance.morph_long_legs = 0.5;
    let old = w.snapshot_v20();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 20);
    assert!(h.restorable());
    // Without the four parameters the payload is 16 bytes shorter.
    assert_eq!(h.raw_len + 16, SnapshotHeader::read(&w.snapshot_v21()).unwrap().raw_len);
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    let off = crate::serde_util::before_v21::OLD_VALUE;
    let b = &r.config.balance;
    assert_eq!((b.morph_effects, b.morph_tail_drag, b.morph_tail_chill, b.morph_long_legs), (off, off, off, off));
    w.config.balance.morph_effects = off;
    w.config.balance.morph_tail_drag = off;
    w.config.balance.morph_tail_chill = off;
    w.config.balance.morph_long_legs = off;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
    assert!(r.snapshot_v20() == old);
}

/// `r` (restored from a snapshot older than version 22, which keeps no
/// headings) facing the way `w`'s organisms face, so the two compare equal.
fn headings_from(w: &World, mut r: World) -> World {
    assert!(r.organisms.iter().all(|o| o.heading == crate::vision::birth_heading(o.id)));
    for (a, b) in r.organisms.iter_mut().zip(&w.organisms) {
        a.heading = b.heading;
    }
    r
}

/// Version 21 snapshots (before `Balance::directional_vision`,
/// `Balance::vision_scan` and organisms' headings) restore with directional
/// vision off and every organism facing its birth heading, and are
/// otherwise the same world. Headings round-trip in version 22.
#[test]
fn version_21_snapshots_restore_without_headings() {
    let mut w = without_morph(fixture_world());
    assert!(w.organisms.iter().any(|o| o.heading != crate::vision::birth_heading(o.id)), "nobody moved");
    w.config.balance.directional_vision = 0.6;
    w.config.balance.vision_scan = 0.3;
    let new = w.snapshot();
    let r = World::restore(&new, Some(w.map.terrain().clone())).unwrap();
    assert!(r.organisms.iter().zip(&w.organisms).all(|(a, b)| a.heading == b.heading));
    let old = w.snapshot_v21();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 21);
    assert!(h.restorable());
    // Without the two parameters (f32s) and the headings (two f32s each),
    // against a version 22 snapshot.
    let alive = w.organisms.len() as u64;
    assert_eq!(h.raw_len + 8 + 8 * alive, SnapshotHeader::read(&w.snapshot_v22()).unwrap().raw_len);
    let r = headings_from(&w, World::restore(&old, Some(w.map.terrain().clone())).unwrap());
    assert_eq!(r.config.balance.directional_vision, 0.0);
    assert_eq!(r.config.balance.vision_scan, crate::serde_util::before_v22::scan::OLD_VALUE);
    w.config.balance.directional_vision = 0.0;
    w.config.balance.vision_scan = 1.0;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot() == w.snapshot());
    assert!(r.snapshot_v21() == old);
}

/// Version 23 snapshots (before `Balance::morph_legacy`) restore with
/// morphology alleles left out of the legacy tally, and are otherwise the
/// same world.
#[test]
fn version_23_snapshots_restore_without_morph_legacy() {
    let mut w = fixture_world();
    w.config.balance.morph_legacy = 1.0;
    let old = w.snapshot_v23();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 23);
    assert!(h.restorable());
    // Without the parameter (an f32), against a version 24 snapshot.
    assert_eq!(h.raw_len + 4, SnapshotHeader::read(&w.snapshot_v24()).unwrap().raw_len);
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.morph_legacy, crate::serde_util::before_v24::OLD_VALUE);
    assert!(!r.config.balance.morph_in_legacy());
    w.config.balance.morph_legacy = crate::serde_util::before_v24::OLD_VALUE;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot_v27() == w.snapshot_v27());
    assert!(r.snapshot_v23() == old);
}

/// Version 24 snapshots (before `Balance::roaming_forage`) restore with
/// roaming hunters eating nothing, and are otherwise the same world.
#[test]
fn version_24_snapshots_restore_without_roaming_forage() {
    let mut w = fixture_world();
    w.config.balance.roaming_forage = 70.0;
    let old = w.snapshot_v24();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 24);
    assert!(h.restorable());
    // Without the parameter (an f32), against a version 25 snapshot.
    assert_eq!(h.raw_len + 4, SnapshotHeader::read(&w.snapshot_v25()).unwrap().raw_len);
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.roaming_forage, crate::serde_util::before_v25::OLD_VALUE);
    w.config.balance.roaming_forage = crate::serde_util::before_v25::OLD_VALUE;
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot_v27() == w.snapshot_v27());
    assert!(r.snapshot_v24() == old);
}

/// Version 25 snapshots (before the carnivore-diet tunables) restore with
/// every one of those rules off, and are otherwise the same world.
#[test]
fn version_25_snapshots_restore_without_carnivore_diet() {
    let mut w = fixture_world();
    let on = crate::Balance::default().carnivore_diet();
    w.config.balance.carcass_keep = on.carcass_keep;
    w.config.balance.carnivore_fat = on.carnivore_fat;
    w.config.balance.carcass_scent = on.carcass_scent;
    w.config.balance.carrion = on.carrion;
    w.config.balance.provisioning = on.provisioning;
    w.config.balance.breeding_territory = on.breeding_territory;
    let old = w.snapshot_v25();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 25);
    assert!(h.restorable());
    // Without the six parameters (f32 each), against a version 26 snapshot.
    assert_eq!(h.raw_len + 6 * 4, SnapshotHeader::read(&w.snapshot_v26()).unwrap().raw_len);
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.carrion, crate::serde_util::before_v26::OLD_VALUE);
    assert_eq!(r.config.balance.breeding_territory, crate::serde_util::before_v26::OLD_VALUE);
    carnivore_diet_off(&mut w.config.balance);
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot_v27() == w.snapshot_v27());
    assert!(r.snapshot_v25() == old);
}

/// Version 26 snapshots (before the predator brakes) restore with the
/// brakes off and the carnivore-diet values they stored, and are otherwise
/// the same world.
#[test]
fn version_26_snapshots_restore_without_predator_brakes() {
    let mut w = fixture_world();
    let on = crate::Balance::default().carnivore_diet();
    assert!(on.prey_switching > 0.0 && on.condition_litter > 0.0);
    w.config.balance.prey_switching = on.prey_switching;
    w.config.balance.condition_litter = on.condition_litter;
    let old = w.snapshot_v26();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 26);
    assert!(h.restorable());
    // Without the two parameters (f32 each), against a version 27 snapshot.
    assert_eq!(h.raw_len + 2 * 4, SnapshotHeader::read(&w.snapshot_v27()).unwrap().raw_len);
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.prey_switching, crate::serde_util::before_v27::OLD_VALUE);
    assert_eq!(r.config.balance.condition_litter, crate::serde_util::before_v27::OLD_VALUE);
    predator_brakes_off(&mut w.config.balance);
    assert_eq!(r.config.balance, w.config.balance);
    assert!(r.snapshot_v27() == w.snapshot_v27());
    assert!(r.snapshot_v26() == old);
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

/// Version 19 to 22 snapshots (before the morphology segment) restore
/// with the default morphology for every organism (and egg, and pregnancy's
/// father), its origin tags copied from the anchor genes, and the rest of
/// the world as it was; restored again they give the same world.
#[test]
fn version_19_to_22_snapshots_restore_with_default_morphology() {
    use crate::genome::Gene;
    use crate::morph::{MorphGene, MorphGenes, MorphLocus};
    let mut w = laying_world();
    // Some organisms carry other morphology: the old form drops it.
    for o in w.organisms.iter_mut().step_by(3) {
        let mut g = (*o.genome).clone();
        g.set_morph(odd_morph(&g, 7));
        o.genome = Arc::new(g);
        o.refresh_derived();
    }
    assert!(w.organisms.iter().any(|o| o.morph.leg_count != 4));
    type OldForm = fn(&World) -> Vec<u8>;
    let old_forms: [(u32, OldForm); 4] =
        [(19, World::snapshot_v19), (20, World::snapshot_v20), (21, World::snapshot_v21), (22, World::snapshot_v22)];
    for (version, old_form) in old_forms {
        let old = old_form(&w);
        let h = SnapshotHeader::read(&old).unwrap();
        assert_eq!(h.version, version);
        assert!(h.restorable());
        // Smaller than a version 23 snapshot: no morphology segments.
        assert!(h.raw_len < SnapshotHeader::read(&w.snapshot()).unwrap().raw_len);
        let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
        assert_eq!(r.organisms.len(), w.organisms.len());
        let check = |g: &crate::Genome, was: &crate::Genome| {
            let m = g.morph();
            assert_eq!(*m, MorphGenes::from_genes(g.loci()));
            for l in MorphLocus::ALL {
                let (p, anchor) = (
                    &m.discrete[l.index()],
                    g.locus(match l {
                        MorphLocus::EyeCount => Gene::Eyes,
                        MorphLocus::EarCount | MorphLocus::EarShape => Gene::Ears,
                        MorphLocus::LegCount => Gene::LegLength,
                        MorphLocus::TailShape => Gene::Tail,
                        MorphLocus::HornForm => Gene::Armor,
                        MorphLocus::Pattern => Gene::Camouflage,
                    }),
                );
                assert_eq!((p.a.value, p.b.value), (l.default_value(), l.default_value()));
                assert_eq!((p.a.origin, p.b.origin), (anchor.a.origin, anchor.b.origin));
            }
            let snout = &m.continuous[MorphGene::SnoutLength.index()];
            assert_eq!(snout.a.origin, g.locus(Gene::Nose).a.origin);
            assert_eq!(g.loci(), was.loci());
            assert_eq!(g.load(), was.load());
        };
        for (a, b) in r.organisms.iter().zip(&w.organisms) {
            check(&a.genome, &b.genome);
            assert_eq!((a.morph.eye_count, a.morph.leg_count, a.morph.ear_count), (2, 4, 2));
            assert!((a.morph.eye_azimuth - 60.0).abs() < 1e-3);
            if let (Some(p), Some(q)) = (&a.pregnancy, &b.pregnancy) {
                check(&p.father, &q.father);
            }
        }
        for (c, d) in r.clutches.iter().zip(&w.clutches) {
            for (a, b) in c.eggs.iter().zip(&d.eggs) {
                check(a, b);
            }
        }
        let again = World::restore(&r.snapshot(), Some(w.map.terrain().clone())).unwrap();
        assert!(again.snapshot() == r.snapshot());
        assert!(old_form(&r) == old);
    }
}

/// Organisms with designed morphology round-trip through a version 23
/// snapshot (whole and against their parents) and continue exactly.
#[test]
fn morphology_round_trips_and_continues() {
    use crate::morph::{EarShape, MorphDesign, TailShape};
    let mut w = World::new(WorldConfig { seed: 4, width: 48, height: 48, ..Default::default() });
    let mut designs = presets::wild_designs().into_iter();
    let (owner, d) = designs.next().unwrap();
    let six = MorphDesign {
        eye_count: 3,
        eye_azimuth: 100.0,
        ear_count: 4,
        ear_shape: EarShape::Long,
        leg_count: 6,
        tail_shape: TailShape::Prehensile,
        snout_length: 0.8,
        horn_size: 0.7,
        horn_form: crate::morph::HornForm::Branched,
        wing_aspect: 0.2,
        pattern: crate::morph::Pattern::Spots,
    };
    w.found_lineage_unchecked(owner, &d.founders(12).with_morph(six));
    let (owner, d) = designs.next().unwrap();
    w.found_lineage_unchecked(owner, &d.founders(12));
    w.run_days(8);
    assert!(w.organisms.iter().any(|o| o.alive && o.morph.leg_count == 6 && o.morph.eye_count == 3));
    assert!(w.organisms.iter().any(|o| o.generation > 0), "want young written against their parents");
    let bytes = w.snapshot();
    let mut r = World::restore(&bytes, Some(w.map.terrain().clone())).unwrap();
    assert_same_caches(&w, &r);
    assert!(r.snapshot() == bytes);
    for _ in 0..(2 * w.config.ticks_per_day) {
        w.step();
        r.step();
    }
    assert_same(&w, &r);
}

/// A version 27 snapshot (before the Phase 3 loci) restores with every
/// organism's core morphology as it was and the Phase 3 loci at their
/// defaults, tagged like their anchor genes; the tunables read as 0.
#[test]
fn snapshots_before_v29_restore_without_the_phase3_loci() {
    use crate::genome::Gene;
    use crate::morph::{HornForm, MorphDesign, MorphGene, MorphLocus, Pattern};
    let mut w = World::new(WorldConfig { seed: 5, width: 48, height: 48, ..Default::default() });
    let mut designs = presets::wild_designs().into_iter();
    let (owner, d) = designs.next().unwrap();
    let m = MorphDesign {
        leg_count: 6,
        horn_size: 0.9,
        horn_form: HornForm::Straight,
        pattern: Pattern::Stripes,
        ..Default::default()
    };
    w.found_lineage_unchecked(owner, &d.founders(12).with_morph(m));
    w.run_days(4);
    let old = w.snapshot_v27();
    let h = SnapshotHeader::read(&old).unwrap();
    assert_eq!(h.version, 27);
    assert!(h.raw_len < SnapshotHeader::read(&w.snapshot()).unwrap().raw_len);
    let r = World::restore(&old, Some(w.map.terrain().clone())).unwrap();
    assert_eq!(r.config.balance.morph_horns, crate::serde_util::before_v29::OLD_VALUE);
    assert_eq!(r.config.balance.morph_pattern, crate::serde_util::before_v29::OLD_VALUE);
    for (a, b) in r.organisms.iter().zip(&w.organisms) {
        let (ma, mb) = (a.genome.morph(), b.genome.morph());
        assert_eq!(ma.discrete[..MorphLocus::CORE], mb.discrete[..MorphLocus::CORE]);
        assert_eq!(ma.continuous[..MorphGene::CORE], mb.continuous[..MorphGene::CORE]);
        assert_eq!(a.morph.leg_count, b.morph.leg_count);
        assert!(!a.morph.horns_present && a.morph.pattern == Pattern::Plain);
        let horn = &ma.continuous[MorphGene::HornSize.index()];
        assert_eq!(horn.a.origin, a.genome.locus(Gene::Armor).a.origin);
        assert_eq!(a.genome.loci(), b.genome.loci());
    }
    assert!(w.organisms.iter().any(|o| o.morph.horns_present && o.morph.pattern == Pattern::Stripes));
    assert!(r.snapshot_v27() == old);
}
