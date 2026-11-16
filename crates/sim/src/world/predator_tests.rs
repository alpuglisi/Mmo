//! The carnivore-diet rules and predator brakes (`Balance::carnivore_diet`;
//! *Predators that live on meat* and *Predators in their niches* in the
//! README): holes found in review (`docs/predator-review.md`).

use super::{Clutch, Intent, World};
use crate::genome::Gene;
use crate::math::Vec2;
use crate::organism::{Action, DeathCause, Sex};
use crate::rng::{rng_for, stream};
use crate::{presets, WorldConfig};

/// The pack hunters' lineage in `niche_world`.
const PACK: u32 = 4;

fn niche(seed: u64) -> World {
    let mut config = WorldConfig { seed, width: 64, height: 64, ..Default::default() };
    config.balance = config.balance.carnivore_diet();
    presets::niche_world(config, 1)
}

/// `n` grown, breed-ready pack hunters of `sex`, other than `skip`.
fn adults(w: &mut World, sex: Sex, n: usize, skip: &[usize]) -> Vec<usize> {
    let picked: Vec<usize> = (0..w.organisms.len())
        .filter(|&i| w.organisms[i].lineage == PACK && w.organisms[i].sex == sex && !skip.contains(&i))
        .take(n)
        .collect();
    assert_eq!(picked.len(), n);
    for &i in &picked {
        let o = &mut w.organisms[i];
        (o.growth, o.matured, o.breed_ready_tick, o.pregnancy) = (1.0, true, 0, None);
        (o.needs.hunger, o.needs.breeding, o.health) = (20.0, 90.0, 100.0);
    }
    picked
}

/// Everyone else far away, so only the organisms placed matter.
fn clear_area(w: &mut World, keep: &[usize]) {
    for i in 0..w.organisms.len() {
        if !keep.contains(&i) {
            w.organisms[i].pos = Vec2::new(60.0, 60.0);
        }
    }
}

/// Two pairs in one territory reach each other in the same tick: both
/// females decided while the territory was free, but once the first
/// conceives the second may not (one litter at a time per territory).
#[test]
fn a_territory_holds_one_litter_even_within_a_tick() {
    let mut w = niche(3);
    w.config.balance.condition_litter = 0.0;
    let f = adults(&mut w, Sex::Female, 2, &[]);
    let m = adults(&mut w, Sex::Male, 2, &[]);
    let all = [f[0], f[1], m[0], m[1]];
    clear_area(&mut w, &all);
    let (a, b) = (Vec2::new(20.5, 20.5), Vec2::new(23.5, 20.5));
    (w.organisms[f[0]].pos, w.organisms[m[0]].pos) = (a, a + Vec2::new(0.3, 0.0));
    (w.organisms[f[1]].pos, w.organisms[m[1]].pos) = (b, b + Vec2::new(0.3, 0.0));
    assert!(super::apply::territory_radius(&w.config.balance, &w.organisms[f[1]]) > 3.0);
    w.grid.rebuild(&w.organisms);
    assert!(!w.territory_taken(&w.organisms[f[0]]) && !w.territory_taken(&w.organisms[f[1]]));
    // Make every courtship accepted and fertile, so only the rule decides.
    let model = w.organisms[f[0]].clone();
    for &i in &all {
        let o = &mut w.organisms[i];
        (o.genome, o.pheno, o.traits, o.morph) = (model.genome.clone(), model.pheno.clone(), model.traits, model.morph);
    }
    let mut rng = rng_for(3, stream::APPLY, 0, 0);
    for (mi, fi) in [(m[0], f[0]), (m[1], f[1])] {
        let to = w.organisms[fi].pos;
        let intent = Intent { action: Action::SeekMate, target: Some(to), target_org: Some(fi), ..Intent::idle() };
        w.apply(mi, &intent, &mut rng);
    }
    assert!(w.organisms[f[0]].pregnancy.is_some(), "the first pair should conceive");
    assert!(w.organisms[f[1]].pregnancy.is_none(), "a second litter in a taken territory");
}

/// An egg-laying carnivore's clutch is her litter until it hatches: the
/// territory stays taken while it lies there.
#[test]
fn a_clutch_takes_the_territory_too() {
    let mut w = niche(3);
    let f = adults(&mut w, Sex::Female, 2, &[]);
    clear_area(&mut w, &f);
    let at = Vec2::new(20.5, 20.5);
    w.organisms[f[0]].pos = at;
    w.organisms[f[1]].pos = at + Vec2::new(3.0, 0.0);
    w.grid.rebuild(&w.organisms);
    assert!(!w.territory_taken(&w.organisms[f[1]]));
    let g = w.organisms[f[0]].genome.clone();
    let clutch = Clutch {
        pos: at,
        owner: w.organisms[f[0]].owner,
        mother: w.organisms[f[0]].id,
        father: 0,
        generation: 1,
        lineage: PACK,
        eggs: vec![(*g).clone(), (*g).clone()],
        hatch_tick: 1000,
        guardian: None,
        warm_blooded: true,
        chill: 0.0,
        overheat: 0.0,
        newborn_growth: 0.1,
        egg_mass: 0.05,
    };
    w.clutch_at.insert(w.map.tile_coords(at), w.clutches.len());
    w.clutches.push(clutch);
    assert!(w.territory_taken(&w.organisms[f[1]]), "eggs of her lineage lie in her territory");
    // Not with the rule off.
    w.config.balance.breeding_territory = 0.0;
    assert!(!w.territory_taken(&w.organisms[f[1]]));
}

/// Prey switching's daily draw is part of the world's randomness: two
/// worlds of different seeds don't give up on the same prey on the same
/// days for the same hunters.
#[test]
fn prey_switching_draws_depend_on_the_seed() {
    let draws = |seed: u64| {
        let mut w = niche(seed);
        w.config.balance.prey_switching = 1e6; // every prey is rare
        let hunters: Vec<usize> = (0..w.organisms.len()).filter(|&i| w.organisms[i].lineage == PACK).collect();
        let mut out = Vec::new();
        for day in 0..20u64 {
            w.tick = day * w.config.ticks_per_day as u64;
            for &h in &hunters {
                let mut o = w.organisms[h].clone();
                o.pheno.0[Gene::Pickiness.index()] = 0.25; // half the time
                out.push(w.gives_up_on(&o, 0));
            }
        }
        out
    };
    let (a, b) = (draws(1), draws(2));
    let given_up = a.iter().filter(|&&x| x).count();
    assert!(given_up > a.len() / 4 && given_up < 3 * a.len() / 4, "{given_up} of {}", a.len());
    let same = a.iter().zip(&b).filter(|(x, y)| x == y).count();
    assert!(same < a.len() * 3 / 4, "{same} of {} draws equal across seeds", a.len());
}

/// A provider sees its hungry young even in a crowd: past the nearest
/// `decide::FOCUS` neighbors (other pack members around the den).
#[test]
fn a_provider_finds_its_young_in_a_crowd() {
    let mut w = niche(5);
    let father = adults(&mut w, Sex::Male, 1, &[])[0];
    let mother = adults(&mut w, Sex::Female, 1, &[])[0];
    let den = Vec2::new(30.5, 30.5);
    {
        let o = &mut w.organisms[father];
        (o.pos, o.fat, o.needs.hunger, o.needs.thirst, o.needs.energy) = (den, 40.0, 10.0, 0.0, 100.0);
        o.needs.breeding = 0.0;
        o.pheno.0[Gene::PackInstinct.index()] = 0.9;
        // Grown but not yet hunting (`decide`'s `hunter`), so the pup gets
        // no close look as possible prey either.
        o.growth = 0.6;
    }
    let (g, owner, fid, mid) =
        (w.organisms[father].genome.clone(), w.organisms[father].owner, w.organisms[father].id, w.organisms[mother].id);
    w.organisms[mother].pos = Vec2::new(5.0, 5.0);
    let mut rng = rng_for(5, 0, 0, 0);
    let pup = w.spawn_newborn(g, owner, PACK, den + Vec2::new(3.0, 0.0), 1, mid, fid, 0.3, &mut rng);
    let pup = w.index[&pup];
    w.organisms[pup].needs.hunger = 90.0;
    // A crowd of 40 grown, fed pack members (no prey, no mates) packed
    // closer to the father than his pup.
    let crowd: Vec<usize> = (0..40)
        .map(|_| {
            let g = w.organisms[mother].genome.clone();
            let id = w.spawn_newborn(g, owner, PACK, den, 1, 0, 0, 1.0, &mut rng);
            w.index[&id]
        })
        .collect();
    assert!(crowd.len() > super::decide::FOCUS);
    let mut keep = crowd.clone();
    keep.extend([father, mother, pup]);
    clear_area(&mut w, &keep);
    for (k, &i) in crowd.iter().enumerate() {
        let o = &mut w.organisms[i];
        let a = k as f32 * 0.7;
        o.pos = den + Vec2::new(1.5 * a.cos(), 1.5 * a.sin());
        (o.growth, o.matured, o.pregnancy, o.needs.hunger, o.needs.breeding) = (1.0, true, None, 10.0, 0.0);
        o.breed_ready_tick = u64::MAX;
    }
    w.grid.rebuild(&w.organisms);
    let intent = w.decide(father);
    assert_eq!(
        (intent.action, intent.target_org),
        (Action::Scavenge, Some(pup)),
        "the father should take food to his starving pup"
    );
}

/// Carrion (`Balance::carrion`) leaves the body of every death but
/// predation and drowning: also a hunter poisoned by toxic prey.
#[test]
fn a_poisoned_hunter_leaves_carrion() {
    let mut w = niche(7);
    w.config.balance.attack_power = 0.0; // the strike never lands
    let h = adults(&mut w, Sex::Male, 1, &[])[0];
    let prey = (0..w.organisms.len()).find(|&i| w.organisms[i].lineage == 0).unwrap();
    clear_area(&mut w, &[h, prey]);
    let at = Vec2::new(20.5, 20.5);
    w.organisms[h].pos = at;
    w.organisms[h].health = 0.01;
    w.organisms[prey].pos = at + Vec2::new(0.3, 0.0);
    w.organisms[prey].pheno.0[Gene::Toxins.index()] = 0.9;
    let before = w.map.tile_at(at).meat;
    let intent = Intent { action: Action::Hunt, target: Some(at), target_org: Some(prey), ..Intent::idle() };
    let mut rng = rng_for(7, stream::APPLY, 0, 0);
    w.apply(h, &intent, &mut rng);
    assert_eq!(w.organisms[h].death, Some(DeathCause::Toxin));
    let body = w.organisms[h].mass() * w.config.balance.meat_per_kg;
    let left = w.map.tile_at(at).meat - before;
    assert!((left - body).abs() < 1e-3, "carcass {left}, body {body}");
}

/// With every rule on, a run doesn't depend on the thread count, and a
/// snapshot taken mid-run resumes exactly as the run goes on.
#[cfg(feature = "serde")]
#[test]
fn carnivore_rules_are_deterministic_and_resume_from_snapshots() {
    let fingerprint = |w: &World| {
        let orgs: Vec<_> = w
            .organisms
            .iter()
            .map(|o| (o.id, o.alive, o.pos, o.needs.hunger, o.fat, o.pregnancy.is_some(), o.forage_target))
            .collect();
        let meat: f64 = w.map.tiles.iter().map(|t| t.meat as f64).sum();
        format!("{orgs:?} {meat} {:?}", w.counters)
    };
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| {
            let mut w = niche(11);
            w.run_days(6);
            let snap = w.snapshot();
            w.run_days(6);
            let mut r = World::restore(&snap, Some(w.map.terrain().clone())).unwrap();
            r.run_days(6);
            (fingerprint(&w), fingerprint(&r), w.counters.kills)
        })
    };
    let (a, ra, kills) = run(1);
    let (b, _, _) = run(4);
    assert!(kills > 0, "the test should see kills");
    assert_eq!(a, b, "thread count changed the run");
    assert_eq!(a, ra, "the restored world went another way");
}
