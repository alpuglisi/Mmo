//! The parallel apply phase (`WorldConfig::parallel_apply`, on by default).
//!
//! `World::apply` runs one organism's whole turn in three parts: `start`
//! (take on the intent, move, plan routes), `act` (eat, hunt, mate, drink,
//! raid, nurse: everything that reaches other organisms, tiles, clutches
//! and caches) and `settle` (dormancy, needs, death rolls, disease, growth),
//! then `finish` (kill, credit parents, give birth). `start` and `settle`
//! touch nothing but the organism itself, which is where most of the time
//! goes (movement and A* route planning, needs), so they can run on every
//! core; the rest stays sequential.
//!
//! One tick, given the decisions and a seeded shuffled order:
//!
//! 1. **Chase pairs.** An organism whose decision targets another (a hunter
//!    and its prey, a suitor and its mate) is *engaged*, and so is its
//!    target. Engaged organisms take their whole turn with `apply`, in
//!    their place in the shuffled order (sweep 3), exactly as in the
//!    sequential apply, so pursuit, strikes, escapes and courtship see each
//!    other in the same random order as before. Everything below concerns
//!    the free (not engaged) organisms.
//! 2. **Start sweep** (parallel): free organisms take on their intent and
//!    move. Route planning uses a per-thread search scratch; search
//!    counters are summed afterwards.
//! 3. **Act sweep** (sequential, shuffled order): free organisms `act`,
//!    engaged ones `apply` (move, act, settle, finish). One shared random
//!    stream, as before. A young being nursed sees a free mother whose turn
//!    comes after its own where she stood before moving, as it would in
//!    the sequential apply (without this, mothers would always have walked
//!    off first and young of caring species would miss meals: herd grazers
//!    ran 10–25% lower in the balance sweep).
//! 4. **Settle sweep** (parallel): free survivors `settle`, each with its
//!    own random stream `(seed, SETTLE, tick, organism id)`.
//! 5. **Finish sweep** (sequential, shuffled order): deaths, parent credit
//!    and births of the free organisms, drawing from the shared stream.
//!
//! The parallel sweeps run each organism on its own (held outside
//! `World::organisms` with `std::mem::take`, so the rules can't reach
//! another organism by mistake; they would panic) against a world that
//! nothing writes meanwhile; the sequential sweeps don't depend on how the
//! parallel ones were split up. So a run is reproducible from its seed and
//! independent of thread count.
//!
//! What differs from the sequential apply: a free organism moves before any
//! organism acts, and settles after all have acted, instead of taking its
//! whole turn at its place in the order (so, e.g., a clutch guardian or a
//! prey's defending mother may already have moved when a raider or hunter
//! acts); its settle draws come from its own stream. Worlds therefore diverge from sequential runs of the same seed
//! (within the usual seed-to-seed spread; see README "Benchmark").
//! `parallel_apply = false` restores the sequential apply exactly.
//!
//! Why not region colouring (process same-coloured map regions in
//! parallel, each with its own buffers): the rules mutate through
//! `&mut World` everywhere (tiles, other organisms, clutches, caches,
//! counters, births), so every rule would have to move onto a split-borrow
//! view of the world, and organisms moving, bolting or being targeted across
//! region borders would need a deferral pass of their own. Splitting the
//! turn instead keeps the rules as they are; the sequential remainder is
//! about a quarter of the apply phase (`PhaseTimes::apply_serial`). If that
//! becomes the bottleneck, the act sweep is the part to split by region.

use std::cell::RefCell;

use rayon::prelude::*;

use super::apply::{drink_from, PathCount, Settled};
use super::{Intent, World};
use crate::design::BirthType;
use crate::math::Vec2;
use crate::organism::{Action, Organism};
use crate::pathfinding::PathScratch;
use crate::rng::{rng_for, stream, SimRng};

thread_local! {
    /// Route-search buffers of this worker thread (a search's result
    /// doesn't depend on what the buffers held before).
    static SCRATCH: RefCell<PathScratch> = RefCell::new(PathScratch::default());
}

/// Turns ahead in the act sweep whose organisms are fetched into the cache
/// early: the sweep visits organisms in random order, so each turn would
/// otherwise start with a wait on memory.
const PREFETCH: usize = 6;

/// Ask the CPU to bring what `p` points to into the cache. A hint only: it
/// changes nothing but timing.
#[inline(always)]
fn prefetch<T>(p: *const T) {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: prefetching is a hint; it never faults or changes memory.
    unsafe {
        std::arch::x86_64::_mm_prefetch::<{ std::arch::x86_64::_MM_HINT_T0 }>(p.cast());
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = p;
}

/// What a free organism's `act` would do, decided before the act sweep.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Local {
    /// It may reach beyond itself: it acts in turn.
    No,
    /// Dead already: it doesn't act.
    Dead,
    /// Nothing at all.
    Nothing,
    /// Drink where it stands (`apply::drink_from`), nothing else.
    Drink,
}

/// Whether `act` (with `o` as it stands when the act sweep begins) could
/// only drink or do nothing, and which. Reaching beyond the organism is
/// eating (plants, meat, caches, clutches), gathering, the migration hazard
/// (the shared random stream), chasing a target and nursing. What `act`
/// reads of the organism is its own and only its own act changes it during
/// the sweep, except a mother's hunger (nursing), so a mother who may nurse
/// is never local (`par_apply`).
fn local_act(o: &Organism, intent: &Intent) -> Local {
    use Action::*;
    let a = intent.action;
    if intent.target_org.is_some() || matches!(a, Graze | Cache | Scavenge | Hunt | Migrate) {
        return Local::No;
    }
    // Below 30 hunger it eats nothing on the side and raids no cache or
    // clutch; a dormant animal does neither anyway.
    if a != Dormant && !(o.needs.hunger < 30.0) {
        return Local::No;
    }
    // A dependent young that is hungry or thirsty may be nursed.
    let dependent = !o.matured && o.growth < 0.7 && o.traits.birth != BirthType::AbandonedEggs;
    if dependent && (o.needs.hunger >= 10.0 || o.needs.thirst >= 10.0) {
        return Local::No;
    }
    let sideline = !matches!(a, Flee | Graze | Scavenge | Hunt | Dormant | Sentry);
    if a == Drink || (sideline && o.needs.thirst > 30.0) {
        Local::Drink
    } else {
        Local::Nothing
    }
}

impl World {
    /// Organisms that chase another one this tick and those chased (hunter
    /// and prey, suitor and mate), indexed like the intents. Their turns
    /// keep the sequential apply's interleaving.
    pub(super) fn engaged(&self, intents: &[Intent]) -> Vec<bool> {
        let mut engaged = vec![false; intents.len()];
        for (i, intent) in intents.iter().enumerate() {
            if let Some(j) = intent.target_org {
                engaged[i] = true;
                engaged[j] = true;
            }
        }
        engaged
    }

    /// The mother that may nurse young `i` this tick (`nurse`), if any,
    /// among the first `n` organisms. A superset: eating and drinking before
    /// the nursing only lower hunger and thirst.
    fn nursing_mother(&self, i: usize, n: usize) -> Option<usize> {
        let o = &self.organisms[i];
        let needy = o.needs.hunger >= 10.0 || o.needs.thirst >= 10.0;
        if !needy || o.matured || o.growth >= 0.7 || o.traits.birth == BirthType::AbandonedEggs {
            return None;
        }
        o.mother.and_then(|m| self.index.get(&m)).copied().filter(|&m| m < n)
    }

    /// The apply phase on all cores (module docs). `order` is the shuffled
    /// order of the organisms that existed when they decided; `rng` the
    /// shared apply stream.
    pub(super) fn par_apply(&mut self, intents: &[Intent], order: &[usize], rng: &mut SimRng) {
        let n = intents.len();
        let engaged = self.engaged(intents);
        self.phase_times.engaged += engaged.iter().filter(|&&e| e).count() as u64;
        self.phase_times.applied += n as u64;
        let free = |i: usize, alive: bool| alive && !engaged[i];
        let mut rank = vec![0; n];
        for (r, &i) in order.iter().enumerate() {
            rank[i] = r;
        }
        let start_pos: Vec<Vec2> = self.organisms[..n].iter().map(|o| o.pos).collect();

        // Start sweep: take on the intent and move.
        let mut orgs = std::mem::take(&mut self.organisms);
        let this = &*self;
        let started: Vec<(f32, PathCount)> = orgs[..n]
            .par_iter_mut()
            .enumerate()
            .map(|(i, o)| {
                let mut paths = PathCount::default();
                if !free(i, o.alive) {
                    return (0.0, paths);
                }
                let moved = SCRATCH.with(|s| this.start(o, &intents[i], None, &mut s.borrow_mut(), &mut paths));
                (moved, paths)
            })
            .collect();
        self.organisms = orgs;
        for (_, paths) in &started {
            paths.add_to(&mut self.counters);
        }

        // Act sweep, local part: a free organism whose act can only drink or
        // do nothing touches nothing but itself, and nothing in the
        // sequential part reads what it changes, so it acts here, on all
        // cores, as it would have at its turn (`local_act`). Mothers who may
        // nurse this tick act in turn: nursing raises their hunger.
        let this = &*self;
        let mothers: Vec<Option<usize>> = (0..n)
            .into_par_iter()
            .map(|i| if this.organisms[i].alive { this.nursing_mother(i, n) } else { None })
            .collect();
        let mut nursing = vec![false; n];
        for &m in mothers.iter().flatten() {
            nursing[m] = true;
        }
        let local: Vec<Local> = (0..n)
            .into_par_iter()
            .map(|i| {
                let o = &this.organisms[i];
                if !o.alive {
                    Local::Dead
                } else if engaged[i] || nursing[i] {
                    Local::No
                } else {
                    local_act(o, &intents[i])
                }
            })
            .collect();
        let dt = self.config.dt_days();
        let mut orgs = std::mem::take(&mut self.organisms);
        let map = &self.map;
        orgs[..n].par_iter_mut().zip(&local).for_each(|(o, l)| {
            if *l == Local::Drink {
                drink_from(map, dt, o);
            }
        });
        self.organisms = orgs;
        let mut acted: Vec<bool> = local.iter().map(|&l| matches!(l, Local::Nothing | Local::Drink)).collect();

        // Act sweep, sequential part: everything that reaches beyond the
        // organism, in the shuffled order.
        let clock = std::time::Instant::now();
        let turns: Vec<usize> = order.iter().copied().filter(|&i| local[i] == Local::No).collect();
        for (k, &i) in turns.iter().enumerate() {
            if let Some(&next) = turns.get(k + PREFETCH) {
                // The parts of the organism a turn reads first.
                let o = &self.organisms[next];
                prefetch(&o.needs);
                prefetch(&o.traits);
                prefetch(&o.pos);
                prefetch(&o.mother);
            }
            if engaged[i] {
                self.apply(i, &intents[i], rng);
            } else {
                // Nursing needs the mother within reach. A free mother whose
                // turn comes later hasn't moved yet in the sequential apply:
                // the young sees her where she stood.
                let later = mothers[i].filter(|&m| !engaged[m] && rank[m] > rank[i]);
                let moved_to = later.map(|m| std::mem::replace(&mut self.organisms[m].pos, start_pos[m]));
                acted[i] = self.act(i, &intents[i], started[i].0, rng);
                if let (Some(m), Some(p)) = (later, moved_to) {
                    self.organisms[m].pos = p;
                }
            }
        }
        let mut serial = clock.elapsed().as_secs_f64();

        // Settle sweep: needs, death rolls, disease, growth.
        let this = &*self;
        let burrow: Vec<(f32, f32)> = (0..n)
            .into_par_iter()
            .map(|i| if acted[i] { this.burrow_inputs(&this.organisms[i]) } else { (0.0, 0.0) })
            .collect();
        let (seed, tick) = (self.config.seed, self.tick);
        let mut orgs = std::mem::take(&mut self.organisms);
        let this = &*self;
        // Each settled organism's outcome, and whether `finish` has anything
        // to do with it (a death, a young come of age, a birth due): nothing
        // in the finish sweep changes that for another organism.
        let settled: Vec<(Settled, bool)> = orgs[..n]
            .par_iter_mut()
            .enumerate()
            .map(|(i, o)| {
                if !acted[i] || !o.alive {
                    return (Settled::default(), false);
                }
                let mut rng = rng_for(seed, stream::SETTLE, tick, o.id);
                let s = this.settle(o, &intents[i], started[i].0, burrow[i], &mut rng);
                let due = matches!(&o.pregnancy, Some(p) if tick >= p.due_tick);
                (s, s.death.is_some() || s.matured || due)
            })
            .collect();
        self.organisms = orgs;

        // Finish sweep: deaths, parent credit, births.
        let clock = std::time::Instant::now();
        for &i in order {
            if let (s, true) = settled[i] {
                self.finish(i, s, rng);
            }
        }
        serial += clock.elapsed().as_secs_f64();
        self.phase_times.apply_serial += serial;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::organism::{Action, DeathCause};
    use crate::{presets, WorldConfig};

    fn world(seed: u64, parallel_apply: bool) -> World {
        presets::standard_world(WorldConfig { seed, width: 64, height: 64, parallel_apply, ..Default::default() }, 1)
    }

    fn run_on(threads: usize, w: &mut World, days: u32) {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| w.run_days(days));
    }

    /// Everything a report or the next tick could see.
    fn fingerprint(w: &World) -> String {
        let orgs: Vec<_> = w
            .organisms
            .iter()
            .map(|o| {
                (o.id, o.alive, o.pos, o.needs.hunger, o.needs.energy, o.health, o.fat, o.growth, o.offspring_raised)
            })
            .collect();
        let food: f64 = w.map.tiles.iter().map(|t| t.veg as f64 + t.meat as f64).sum();
        format!("{orgs:?} {:?} {food} {}", w.counters, w.clutches.len())
    }

    #[test]
    fn parallel_apply_is_independent_of_thread_count() {
        for parallel in [true, false] {
            let mut a = world(9, parallel);
            let mut b = world(9, parallel);
            run_on(1, &mut a, 8);
            run_on(4, &mut b, 8);
            assert!(
                a.counters.births + a.counters.hatched > 0 && a.counters.kills > 0,
                "the test should see births and kills"
            );
            assert_eq!(fingerprint(&a), fingerprint(&b), "parallel_apply = {parallel}");
        }
    }

    #[test]
    fn parallel_and_sequential_apply_both_run() {
        let mut par = world(5, true);
        let mut seq = world(5, false);
        par.run_days(2);
        seq.run_days(2);
        assert!(par.phase_times.applied > 0 && seq.phase_times.applied == 0);
        // Different schedules, different worlds.
        assert_ne!(fingerprint(&par), fingerprint(&seq));
    }

    /// Two hunters strike at the same prey from either side of it in one
    /// tick (where a region-split apply would put them in neighboring
    /// regions). Chasers and their prey take their turns whole and in order,
    /// so the prey dies once, is counted once, leaves one carcass, and the
    /// second hunter finds it dead.
    #[test]
    fn two_hunters_one_prey() {
        for seed in 0..8 {
            let mut config = WorldConfig { seed, width: 48, height: 48, ..Default::default() };
            config.balance.escape = 0.0;
            config.balance.attack_power = 1e3;
            config.balance.fight_damage = 0.0;
            let mut w = presets::standard_world(config, 1);
            let prey = w.organisms.iter().position(|o| o.lineage == 1).unwrap();
            let hunters: Vec<usize> =
                w.organisms.iter().enumerate().filter(|(_, o)| o.lineage == 3).map(|(i, _)| i).take(2).collect();
            let at = Vec2::new(16.0, 16.5);
            w.organisms[prey].pos = at;
            w.organisms[hunters[0]].pos = at + Vec2::new(-0.3, 0.0);
            w.organisms[hunters[1]].pos = at + Vec2::new(0.3, 0.0);
            for &h in &hunters {
                let o = &mut w.organisms[h];
                (o.growth, o.matured, o.needs.energy, o.needs.hunger, o.health) = (1.0, true, 90.0, 60.0, 100.0);
            }
            // Everyone else stays put, so these are the tick's only strikes.
            let n = w.organisms.len();
            let mut intents = vec![Intent::idle(); n];
            for &h in &hunters {
                intents[h] = Intent {
                    action: Action::Hunt,
                    target: Some(at),
                    target_org: Some(prey),
                    sprint: true,
                    ..Intent::idle()
                };
            }
            let meat_before: f32 = w.map.tiles.iter().map(|t| t.meat).sum();
            let eaten_before = w.counters.deaths.get(&DeathCause::Predation).copied().unwrap_or(0);
            let kills_before = w.counters.kills;
            let mut order: Vec<usize> = (0..n).collect();
            order.rotate_left(seed as usize * 7 % n);
            let mut rng = rng_for(seed, stream::APPLY, 0, 0);
            w.par_apply(&intents, &order, &mut rng);
            assert!(!w.organisms[prey].alive, "seed {seed}: prey survived");
            assert!(hunters.iter().all(|&h| w.organisms[h].alive));
            assert_eq!(w.counters.kills - kills_before, 1, "seed {seed}");
            let eaten = w.counters.deaths.get(&DeathCause::Predation).copied().unwrap_or(0);
            assert_eq!(eaten - eaten_before, 1, "seed {seed}");
            // One carcass, part of it eaten at once by the hunters.
            let carcass = w.organisms[prey].mass() * w.config.balance.meat_per_kg;
            let meat_after: f32 = w.map.tiles.iter().map(|t| t.meat).sum();
            assert!(meat_after - meat_before <= carcass + 1e-3 && meat_after > meat_before - 1e-3, "seed {seed}");
        }
    }

    /// A young is nursed by a free mother walking away exactly when the
    /// sequential apply would nurse it: when its turn comes before hers.
    #[test]
    fn nursing_sees_the_mother_as_the_sequential_apply_does() {
        let nursed = |parallel: bool, young_first: bool| {
            let mut w = presets::standard_world(
                WorldConfig { seed: 4, width: 48, height: 48, parallel_apply: parallel, ..Default::default() },
                1,
            );
            let mother = w.organisms.iter().position(|o| o.lineage == 1).unwrap();
            let (pos, mid, owner) = (Vec2::new(20.5, 20.5), w.organisms[mother].id, w.organisms[mother].owner);
            w.organisms[mother].pos = pos;
            w.organisms[mother].needs.hunger = 20.0;
            let g = w.organisms[mother].genome.clone();
            let mut rng = rng_for(4, 0, 0, 0);
            let id = w.spawn_newborn(g, owner, 1, pos + Vec2::new(-1.4, 0.0), 1, mid, 0, 0.2, &mut rng);
            let young = w.index[&id];
            w.organisms[young].needs.hunger = 25.0;
            let n = w.organisms.len();
            let mut intents = vec![Intent::idle(); n];
            intents[mother] =
                Intent { action: Action::Wander, target: Some(pos + Vec2::new(6.0, 0.0)), ..Intent::idle() };
            let mut order: Vec<usize> = (0..n).filter(|&k| k != mother && k != young).collect();
            order.extend(if young_first { [young, mother] } else { [mother, young] });
            let mut rng = rng_for(4, stream::APPLY, 0, 0);
            if parallel {
                w.par_apply(&intents, &order, &mut rng);
            } else {
                for &i in &order {
                    w.apply(i, &intents[i], &mut rng);
                }
            }
            // A meal is 300 hunger points a day, 6.25 a tick.
            w.organisms[young].needs.hunger < 22.0
        };
        assert!(nursed(false, true) && !nursed(false, false), "the test needs the order to matter");
        assert!(nursed(true, true));
        assert!(!nursed(true, false));
    }

    /// Births, hatchings and deaths are each counted once: every organism
    /// ever inserted is a founder or counted as born, and every one no
    /// longer alive has exactly one counted death.
    #[test]
    fn births_and_deaths_counted_once() {
        let mut w = world(9, true);
        let founders: u64 = w.lineages.iter().map(|l| l.founders as u64).sum();
        run_on(4, &mut w, 8);
        let c = &w.counters;
        let born: u64 = c.born.values().sum();
        assert!(born > 0 && c.kills > 0);
        assert_eq!(born, c.births + c.hatched);
        let inserted = w.next_id - 1;
        assert_eq!(inserted, founders + born);
        let deaths: u64 = c.deaths.values().sum();
        assert_eq!(deaths, inserted - w.alive_count() as u64);
        let by_lineage: u64 = c.deaths_by_lineage.values().sum();
        assert_eq!(by_lineage, deaths);
        assert!(c.kills <= c.deaths[&DeathCause::Predation]);
        // Ids are unique, and the living are those the owner counts hold.
        let mut ids: Vec<_> = w.organisms.iter().map(|o| o.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), w.organisms.len());
        assert_eq!(w.organisms.iter().filter(|o| o.alive).count(), w.alive_count());
    }
}
