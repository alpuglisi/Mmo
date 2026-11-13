//! The sequential half of a tick: movement, eating, predation, mating, needs,
//! death, birth and growth.

use rand::Rng;

use super::{Clutch, Intent, World};
use crate::design::BirthType;
use crate::genome::{Gene, Genome, PlayerId};
use crate::mating;
use crate::math::Vec2;
use crate::organism::{Action, DeathCause, Dormancy, Infection, OrgId, Organism, Pregnancy, Sex};
use crate::pathfinding::{enter_cost, Outcome, Path, PathScratch, MAX_TILE_SPEED};
use crate::rng::{rng_for, stream, SimRng};
use crate::terrain::{traverse, Map, Traverse, Weather};

/// Most hunger points an organism can eat per day (so a meal takes a few ticks).
pub(crate) const MAX_INTAKE_PER_DAY: f32 = 1200.0;
const MAX_DRINK_PER_DAY: f32 = 1500.0;
/// Energy per tile walked at endurance 1.0.
const ENERGY_PER_TILE: f32 = 0.6;
const MATING_REACH: f32 = 1.2;
/// Sprints shorter than this (tiles) steer greedily instead of planning.
const PATH_SHORT: f32 = 4.0;

/// Hunger multiplier from a cold-blooded body's temperature: a sluggish body
/// (low `activity`) idles cheaply; above full activity (22 °C) every 10 °C
/// costs `heat_burn` more (`Balance::cold_heat_burn`), so the food discount
/// of cold blood is largest where the body is cool and slow.
pub(crate) fn cold_blooded_thermo(activity: f32, body_t: f32, heat_burn: f32) -> f32 {
    (0.4 + 0.6 * activity) * (1.0 + heat_burn * (body_t - 22.0).max(0.0) / 10.0)
}

/// Daily death hazard of an uncared-for juvenile with `kin` of its own kind
/// nearby: `(kin * sqrt(pace) / competition)^4`, `pace` being its kind's pace
/// of life (`Traits::pace_of_life`). Steep, because perceived kin saturate in
/// a crowd. Many small hatchlings compete hardest (doc: "many: fragile, most
/// die young"); the few, larger young of small clutches and big bodies hold
/// their own (toxic frogs, pace 0.52: 9 kin 6% a day, 25 kin 330%; a
/// two-egg clutch of a mid-sized body, pace 0.02, barely notices a crowd).
pub(crate) fn larval_hazard(kin: u16, pace: f32, competition: f32) -> f32 {
    if competition <= 0.0 {
        return 0.0;
    }
    (kin as f32 * pace.max(0.0).sqrt() / competition).powi(4)
}

/// What walking costs an organism this tick.
#[derive(Clone, Copy)]
struct Effort {
    sprint: f32,
    endurance: f32,
}

impl Effort {
    /// Energy for `dist` tiles on terrain with energy multiplier `e`.
    fn cost(self, dist: f32, e: f32) -> f32 {
        dist * ENERGY_PER_TILE * e * self.sprint / self.endurance
    }
}

/// Path searches run during movement (`World::step_toward`), added to the
/// world's counters after a sweep.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PathCount {
    pub searches: u64,
    pub failures: u64,
}

impl PathCount {
    pub fn add_to(self, c: &mut super::Counters) {
        c.path_searches += self.searches;
        c.path_failures += self.failures;
    }
}

/// What `World::settle` leaves for `World::finish`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Settled {
    pub death: Option<DeathCause>,
    /// Matured this tick: credit the parents.
    pub matured: bool,
}

impl World {
    /// Apply organism `i`'s decision in one go: take on the intent, move,
    /// act, then settle needs, death, growth and birth. The parallel apply
    /// (`world::par_apply`) runs the same parts as separate sweeps for
    /// organisms that neither chase nor are chased.
    pub(crate) fn apply(&mut self, i: usize, intent: &Intent, rng: &mut SimRng) {
        if !self.organisms[i].alive {
            return;
        }
        // Movement (chasing a live target follows it).
        let chase = match intent.target_org {
            Some(j) if self.organisms[j].alive => Some(self.organisms[j].pos),
            _ => None,
        };
        let mut orgs = std::mem::take(&mut self.organisms);
        let mut scratch = std::mem::take(&mut self.path_scratch);
        let mut paths = PathCount::default();
        let moved = self.start(&mut orgs[i], intent, chase, &mut scratch, &mut paths);
        self.organisms = orgs;
        self.path_scratch = scratch;
        paths.add_to(&mut self.counters);
        if !self.act(i, intent, moved, rng) {
            return;
        }
        let burrow = self.burrow_inputs(&self.organisms[i]);
        let mut orgs = std::mem::take(&mut self.organisms);
        let settled = self.settle(&mut orgs[i], intent, moved, burrow, rng);
        self.organisms = orgs;
        self.finish(i, settled, rng);
    }

    /// First part of `apply`, touching only organism `o` (held outside
    /// `organisms`): take on the intent and move, toward `chase` (the live
    /// position of a chased organism) if given. Returns the distance moved.
    pub(super) fn start(
        &self,
        o: &mut Organism,
        intent: &Intent,
        chase: Option<Vec2>,
        scratch: &mut PathScratch,
        paths: &mut PathCount,
    ) -> f32 {
        o.action = intent.action;
        o.target = intent.target;
        o.kin_nearby = intent.kin_nearby;
        o.needs.safety = (intent.threat * 100.0).min(100.0);
        o.alert = intent.alert;
        if let Some(w) = intent.new_wander {
            o.wander_target = Some(w);
        }
        if let Some(f) = intent.new_forage {
            o.forage_target = f;
        }
        // A possessed organism's order moves it in sub-steps
        // (`World::substep`); the tick's own move is the last of them.
        let frac = if intent.action == Action::Ordered { self.substep_frac() } else { 1.0 };
        match intent.target {
            Some(target) => self.step_toward(o, chase.unwrap_or(target), intent.sprint, frac, scratch, paths),
            None => 0.0,
        }
    }

    /// Middle part of `apply`: the action's effect and everything else that
    /// reaches other organisms, tiles, clutches and caches (eating, hunting,
    /// mating, drinking, raiding, nursing). False if the organism died.
    pub(super) fn act(&mut self, i: usize, intent: &Intent, moved: f32, rng: &mut SimRng) -> bool {
        if !self.organisms[i].alive {
            return false;
        }
        if intent.action == Action::Migrate && self.journey_death(i, moved, rng) {
            self.kill(i, DeathCause::Journey);
            return false;
        }

        // The chosen action's effect.
        match intent.action {
            Action::Graze => {
                self.eat_plants(i, 1.0);
                self.eat_meat(i, 1.0);
                self.eat_cache(i, false);
            }
            Action::Cache => self.gather(i),
            Action::Scavenge => self.eat_meat(i, 1.0),
            Action::Hunt => {
                if let Some(j) = intent.target_org {
                    self.attack(i, j, rng);
                }
                self.eat_meat(i, 1.0);
            }
            Action::Drink => self.drink(i),
            Action::SeekMate => {
                if let Some(j) = intent.target_org {
                    self.try_mate(i, j, rng);
                }
            }
            _ => {}
        }
        if !self.organisms[i].alive {
            return false;
        }

        // Opportunistic eating and drinking while doing something else.
        let dormant = intent.action == Action::Dormant;
        // A dormant animal doesn't eat; a sentry on watch doesn't forage:
        // that is the price of the watch.
        if !matches!(
            intent.action,
            Action::Flee | Action::Graze | Action::Scavenge | Action::Hunt | Action::Dormant | Action::Sentry
        ) {
            let o = &self.organisms[i];
            if o.needs.hunger > 30.0 {
                self.eat_plants(i, 0.3);
                self.eat_meat(i, 0.3);
            }
            if self.organisms[i].needs.thirst > 30.0 && intent.action != Action::Drink {
                self.drink(i);
            }
        }
        if !dormant {
            self.eat_cache(i, true);
            self.raid_clutch(i, rng);
        }
        self.nurse(i);
        true
    }

    /// Shared-burrow inputs of `update_needs` (shared bonus, burrow depth),
    /// read before `settle` because they look at the mother.
    pub(super) fn burrow_inputs(&self, o: &Organism) -> (f32, f32) {
        (self.shared_burrow_bonus(o), self.burrow_depth(o))
    }

    /// Last part of `apply` touching only organism `o` (held outside
    /// `organisms`): dormancy, needs, the death rolls, the course of a
    /// disease and growth. Deaths, parent credit and birth are left to
    /// `finish`.
    pub(super) fn settle(&self, o: &mut Organism, intent: &Intent, moved: f32, burrow: (f32, f32), rng: &mut SimRng) -> Settled {
        let dead = |cause| Settled { death: Some(cause), matured: false };
        if let Some(cause) = self.seasonal_tick(o, intent, moved, rng) {
            return dead(cause);
        }
        if let Some(cause) = self.update_needs(o, intent, moved, self.config.dt_days(), burrow) {
            return dead(cause);
        }
        if self.old_age_death(o, rng) {
            return dead(DeathCause::OldAge);
        }
        if self.larval_competition_death(o, rng) {
            return dead(DeathCause::Starvation);
        }
        if self.attrition_death(o, rng) {
            return dead(DeathCause::Attrition);
        }
        if self.recessive_death_of(o, rng) {
            return dead(DeathCause::Recessive);
        }
        self.disease_course_of(o);
        Settled { death: None, matured: self.grow(o) }
    }

    /// Resolve what `settle` left: death, credit to the parents of a young
    /// that just matured, birth when due.
    pub(super) fn finish(&mut self, i: usize, settled: Settled, rng: &mut SimRng) {
        if let Some(cause) = settled.death {
            self.kill(i, cause);
            return;
        }
        if settled.matured {
            // Credit parents: only offspring raised to breeding age count.
            let o = &self.organisms[i];
            let parents = [o.mother, o.father];
            for &k in parents.iter().flatten().filter_map(|p| self.index.get(p)) {
                self.organisms[k].offspring_raised += 1;
            }
        }
        if matches!(&self.organisms[i].pregnancy, Some(p) if self.tick >= p.due_tick) {
            self.give_birth(i, rng);
        }
    }

    /// Move organism `i` toward `target` (see `step_toward`), `frac` of a
    /// tick's movement.
    pub(super) fn move_toward(&mut self, i: usize, target: Vec2, sprint: bool, frac: f32) -> f32 {
        let mut orgs = std::mem::take(&mut self.organisms);
        let mut scratch = std::mem::take(&mut self.path_scratch);
        let mut paths = PathCount::default();
        let moved = self.step_toward(&mut orgs[i], target, sprint, frac, &mut scratch, &mut paths);
        self.organisms = orgs;
        self.path_scratch = scratch;
        paths.add_to(&mut self.counters);
        moved
    }

    /// Steer `o` toward `target`, following or planning a route (A* in
    /// `scratch`, searches counted in `paths`) when the way is blocked, by
    /// `frac` of a tick's movement (1 except for possession sub-steps).
    /// Touches nothing but `o`, so the parallel apply can run it for many
    /// organisms at once. Returns the distance moved.
    fn step_toward(
        &self,
        o: &mut Organism,
        target: Vec2,
        sprint: bool,
        frac: f32,
        scratch: &mut PathScratch,
        paths: &mut PathCount,
    ) -> f32 {
        let to = target - o.pos;
        let d = to.len();
        if d < 0.05 {
            return 0.0;
        }
        let st = &o.traits.stats;
        let mut speed = self.config.balance.base_speed * st.speed * o.activity * (0.5 + 0.5 * o.growth);
        if o.pregnancy.is_some() && o.traits.birth == BirthType::Live {
            speed *= 0.7;
        }
        if o.needs.energy < 20.0 {
            speed *= 0.5;
        }
        if o.needs.hunger > 90.0 {
            speed *= 0.7;
        }
        let sprinting = sprint && o.needs.energy > 15.0;
        if sprinting {
            speed *= self.config.balance.sprint;
        }
        // Small possession bonus (reaction), per the design doc.
        if o.possessed_by.is_some() {
            speed *= 1.1;
        }
        let step = (speed * frac).min(d);
        let dir = to * (1.0 / d);
        let body = o.traits.body;
        let pos = o.pos;
        let endurance = st.endurance;
        let fly = body.can_fly && o.needs.energy > 25.0 && !self.map.weather_at(pos).grounds_flyers();
        // Stuck on a tile the body can't use (e.g. ice melted underneath):
        // struggle toward the target regardless.
        let stranded = !fly && traverse(&body, self.map.tile_at(pos), &self.map.weather_at(pos), false) == Traverse::Blocked;
        let effort = Effort { sprint: if sprinting { 2.5 } else { 1.0 }, endurance };

        // Walkers follow a route planned earlier, or plan one when the direct
        // step is blocked (flyers only meet storms, which ground everything).
        // Short sprints (fleeing, the last lunge at prey) stay greedy.
        let pathing = !stranded && !fly && self.config.balance.path_nodes >= 1.0;
        if pathing {
            if let Some(moved) = self.follow_path(o, target, step, effort) {
                return moved;
            }
        }
        let can_plan = pathing && !(sprinting && d < PATH_SHORT) && !self.next_to(pos, target);

        for (k, &angle) in [0.0f32, 0.6, -0.6, 1.2, -1.2, 1.9, -1.9].iter().enumerate() {
            let dd = dir.rotate(angle);
            let dest = self.map.clamp(pos + dd * step);
            let rule = if stranded {
                Traverse::Passable { speed: 0.3, energy: 2.0 }
            } else {
                traverse(&body, self.map.tile_at(dest), &self.map.weather_at(dest), fly)
            };
            if let Traverse::Passable { speed: s, energy: e } = rule {
                let dist = (step * s.min(MAX_TILE_SPEED)).min(d);
                let new_pos = self.map.clamp(pos + dd * dist);
                // The end point can fall short of the checked tile (or cut a
                // corner): never step onto a tile the body can't use.
                if stranded || traverse(&body, self.map.tile_at(new_pos), &self.map.weather_at(new_pos), fly) != Traverse::Blocked {
                    o.pos = new_pos;
                    o.needs.energy = (o.needs.energy - effort.cost(dist, e)).max(0.0);
                    return dist;
                }
            }
            // The way straight ahead is blocked: find a way around.
            if k == 0 && can_plan && self.plan_path(o, target, scratch, paths) {
                if let Some(moved) = self.follow_path(o, target, step, effort) {
                    return moved;
                }
            }
        }
        0.0
    }

    /// Is `target` on the tile under `pos` or a neighboring one? Close enough
    /// that steering straight at it is all a route could do.
    fn next_to(&self, pos: Vec2, target: Vec2) -> bool {
        let (ax, ay) = self.map.tile_coords(pos);
        let (bx, by) = self.map.tile_coords(target);
        ax.abs_diff(bx) <= 1 && ay.abs_diff(by) <= 1
    }

    /// Search a route from organism `i` to `target` for its own body and keep
    /// it on the organism. A failed search is kept too, so it isn't repeated
    /// until the terrain changes (on the hour) or the target moves. True if
    /// there is a route to follow, possibly partial.
    fn plan_path(&self, o: &mut Organism, target: Vec2, scratch: &mut PathScratch, paths: &mut PathCount) -> bool {
        let epoch = self.tick / self.config.ticks_per_hour();
        if let Some(p) = &o.path {
            if p.failed && p.epoch == epoch && p.serves(o.pos, target) {
                return false;
            }
        }
        let (body, from) = (o.traits.body, o.pos);
        // Reuse the old route's allocation.
        let mut path = o.path.take().unwrap_or_else(|| Box::new(Path::new(target)));
        let budget = self.config.balance.path_nodes as usize;
        let outcome = scratch.search(
            &self.map,
            from,
            target,
            budget,
            |t| enter_cost(traverse(&body, t, &Weather { wind: t.wind }, false)),
            &mut path.waypoints,
        );
        path.goal = target;
        path.epoch = epoch;
        path.partial = outcome == Outcome::Partial;
        path.failed = outcome == Outcome::None;
        paths.searches += 1;
        paths.failures += u64::from(path.failed);
        let ok = !path.failed;
        o.path = Some(path);
        ok
    }

    /// Move organism `i` along its route to `target`, up to `step` tiles
    /// (scaled by the speed of the tile ahead). `None`, with the route
    /// dropped, if it has no route that still serves the target, has been
    /// pushed off it, or the terrain ahead has closed (ice melted); the
    /// caller then steers greedily, re-planning if the direct way is blocked.
    fn follow_path(&self, o: &mut Organism, target: Vec2, step: f32, effort: Effort) -> Option<f32> {
        let path = o.path.as_mut()?;
        let first = path.next_tile()?;
        let pos = o.pos;
        let body = o.traits.body;
        let w = self.map.width;
        let center = |k: u32| Map::tile_center(k as usize % w, k as usize / w);
        let rule = |k: u32| {
            let t = &self.map.tiles[k as usize];
            traverse(&body, t, &Weather { wind: t.wind }, false)
        };
        let (cx, cy) = self.map.tile_coords(pos);
        let on_route = (first as usize % w).abs_diff(cx) <= 1 && (first as usize / w).abs_diff(cy) <= 1;
        let ahead = if on_route && path.serves(pos, target) { rule(first) } else { Traverse::Blocked };
        let Traverse::Passable { speed: s, energy: e } = ahead else {
            path.clear();
            return None;
        };
        let total = step * s.min(MAX_TILE_SPEED);
        let mut left = total;
        let mut at = pos;
        while let Some(k) = path.next_tile() {
            if k != first && rule(k) == Traverse::Blocked {
                break;
            }
            let c = center(k);
            let len = at.dist(c);
            if len > left {
                at = at + (c - at) * (left / len);
                left = 0.0;
                break;
            }
            at = c;
            left -= len;
            path.waypoints.pop();
        }
        let dist = total - left;
        o.pos = self.map.clamp(at);
        o.needs.energy = (o.needs.energy - effort.cost(dist, e)).max(0.0);
        Some(dist)
    }

    /// Graze the current tile. Fattening organisms (`seasonal::fat_room`)
    /// keep eating past full and store the surplus as fat.
    fn eat_plants(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let eff = o.traits.plant_efficiency(self.config.balance.generalist_tax);
        let fat_room = self.fat_room(o);
        let room = o.needs.hunger + fat_room;
        if o.traits.carnivory > 0.95 || eff <= 0.0 || room < 5.0 {
            return;
        }
        let want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(room);
        let fph = o.food_per_hunger();
        let (pos, body) = (o.pos, o.traits.body);
        let share = self.config.balance.canopy_share;
        let tile = self.map.tile_at_mut(pos);
        let got = (want * fph / eff).min(tile.reachable_veg(&body, share));
        tile.veg -= got;
        self.feed(i, got * eff / fph, fat_room);
    }

    fn eat_meat(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let eff = o.traits.meat_efficiency(self.config.balance.generalist_tax);
        let fat_room = self.fat_room(o);
        let room = o.needs.hunger + fat_room;
        if o.traits.carnivory < 0.2 || room < 5.0 {
            return;
        }
        let want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(room);
        let fph = o.food_per_hunger();
        let pos = o.pos;
        let tile = self.map.tile_at_mut(pos);
        let got = (want * fph / eff).min(tile.meat);
        tile.meat -= got;
        self.feed(i, got * eff / fph, fat_room);
    }

    fn drink(&mut self, i: usize) {
        let pos = self.organisms[i].pos;
        if !self.map.can_drink_at(pos) {
            return;
        }
        let amount = MAX_DRINK_PER_DAY * self.config.dt_days();
        let o = &mut self.organisms[i];
        o.needs.thirst = (o.needs.thirst - amount).max(0.0);
        o.last_water = Some(pos);
    }

    fn attack(&mut self, i: usize, j: usize, rng: &mut SimRng) {
        let (a, b) = (&self.organisms[i], &self.organisms[j]);
        // A hunter whose last prey broke free is still recovering.
        if !b.alive || a.pos.dist(b.pos) > self.config.balance.attack_reach || self.tick < a.strike_ready_tick {
            return;
        }
        let escape = self.escape_chance(a, b);
        let pw = a.power() * (1.0 + self.coop_bonus(a)) * self.config.balance.attack_power;
        let mut rs = b.resistance() * (1.0 + 1.5 * self.herd_bonus(b));
        // A parent standing over its dependent young fights for it.
        let (guard_rs, guard_power) = self.guardian(b);
        rs += guard_rs;
        // Prey that hasn't noticed the hunter is caught off guard.
        if b.action != Action::Flee {
            rs *= 1.0 - self.config.balance.ambush;
        }
        let mut p = pw * pw / (pw * pw + rs * rs + 1e-6);
        // Prey that made it into its burrow is mostly out of reach.
        if b.sheltered {
            p *= (1.0 - self.config.balance.burrow_shelter * self.burrow_depth(b)).max(0.0);
        }
        // A climber up in the canopy is mostly out of reach of ground hunters.
        p *= 1.0 - crate::terrain::canopy_refuge(self.map.tile_at(b.pos), &b.traits.body, &a.traits.body);
        let tox = b.pheno[Gene::Toxins];
        if tox > 0.3 {
            p *= 1.0 - 0.6 * tox;
        }
        let prey_power = b.power().max(guard_power);
        let prey_fights = super::decide::fight_back(b, self.config.balance.generalist_tax);
        let prey_bright = b.pheno[Gene::Coloration] > 0.6;
        let prey_pos = b.pos;
        let prey_mass = b.mass();

        // Toxins hurt whoever bites, whether or not the attack succeeds.
        if tox > 0.3 {
            let a = &mut self.organisms[i];
            a.health -= self.config.balance.toxin_damage * tox * rng.gen_range(0.5..1.0);
            // A warning signal makes the lesson stick.
            if prey_bright {
                a.bright_aversion = true;
            }
            if a.health <= 0.0 {
                self.kill(i, DeathCause::Toxin);
            }
        }

        // The prey may twist away before the hunter gets a grip: a contest of
        // speed and agility. It then bolts while the hunter recovers, so an
        // encounter is a contest rather than a strike re-rolled every tick.
        if rng.gen::<f32>() < escape {
            self.counters.failed_hunts += 1;
            let window = (self.config.balance.escape_window * self.config.ticks_per_day as f32 / 24.0).round();
            let a = &mut self.organisms[i];
            a.needs.energy = (a.needs.energy - 8.0).max(0.0);
            a.strike_ready_tick = self.tick + window as u64;
            let from = a.pos;
            // A burrower stays in its burrow; anything else gets a burst of distance.
            if !self.organisms[j].sheltered {
                let away = (prey_pos - from).norm();
                let away = if away == Vec2::ZERO { Vec2::new(1.0, 0.0) } else { away };
                self.move_toward(j, self.map.clamp(prey_pos + away * 4.0), true, 1.0);
            }
            return;
        }

        // Prey that fights back hurts the hunter once they come to grips,
        // whether or not the hunter wins.
        if prey_fights > 0.0 && self.config.balance.fight_damage > 0.0 {
            let a = &mut self.organisms[i];
            a.health -= self.config.balance.fight_damage * prey_fights * rng.gen::<f32>();
            if a.alive && a.health <= 0.0 {
                self.kill_by(i, j);
                return;
            }
        }

        if rng.gen::<f32>() < p {
            self.kill_by(j, i);
            self.counters.kills += 1;
            self.map.tile_at_mut(prey_pos).meat += prey_mass * self.config.balance.meat_per_kg;
            if self.organisms[i].alive {
                self.eat_meat(i, 1.0);
            }
        } else {
            // Held but not subdued: the struggle goes on next tick.
            self.counters.failed_hunts += 1;
            let a = &mut self.organisms[i];
            a.needs.energy = (a.needs.energy - 8.0).max(0.0);
            a.health -= prey_power * 8.0 * rng.gen::<f32>();
            if a.alive && a.health <= 0.0 {
                self.kill_by(i, j);
            }
            let b = &mut self.organisms[j];
            b.needs.energy = (b.needs.energy - 5.0).max(0.0);
        }
    }

    /// Chance that prey `b` slips away from hunter `a`'s strike: the prey's
    /// share of their combined speed and agility, scaled by `Balance::escape`.
    /// Prey caught off guard (not fleeing) reacts late, like its resistance.
    pub(crate) fn escape_chance(&self, a: &Organism, b: &Organism) -> f32 {
        let (pursue, evade) = (a.evasion(), b.evasion());
        let mut q = self.config.balance.escape * 2.0 * evade / (evade + pursue + 1e-6);
        if b.action != Action::Flee {
            q *= 1.0 - self.config.balance.ambush;
        }
        q.min(0.95)
    }

    fn try_mate(&mut self, i: usize, j: usize, rng: &mut SimRng) {
        let tick = self.tick;
        let (a, b) = (&self.organisms[i], &self.organisms[j]);
        if !b.alive || a.sex == b.sex || !a.can_breed(tick) || !b.can_breed(tick) || a.pos.dist(b.pos) > MATING_REACH {
            return;
        }
        if !self.may_conceive(a) || !self.may_conceive(b) {
            return;
        }
        let (f, m) = if a.sex == Sex::Female { (i, j) } else { (j, i) };
        let (female, male) = (&self.organisms[f], &self.organisms[m]);
        let mf = mating::mismatch(&female.pheno, male.display());
        let mm = mating::mismatch(&male.pheno, female.display());
        // Females choose; males are less picky.
        let accept_f = mf <= 1.0 || rng.gen::<f32>() < 0.05 * (-3.0 * (mf - 1.0)).exp();
        if !accept_f || mm > 1.5 {
            return;
        }
        let fert = mating::fertility(&female.genome, &male.genome) * self.disease_fertility(f, m) * self.conception_boost(female);
        self.mating_contact(f, m);
        if fert <= 0.0 {
            // Courtship without conception.
            let half_day = self.config.ticks_per_day as u64 / 2;
            self.organisms[f].breed_ready_tick = tick + half_day;
            self.organisms[f].needs.breeding *= 0.5;
            return;
        }
        self.conceive(f, m, fert);
    }

    pub(crate) fn conceive(&mut self, f: usize, m: usize, fertility: f32) {
        let tpd = self.config.ticks_per_day as f32;
        let tick = self.tick;
        let father = Box::new(self.organisms[m].genome.clone());
        let father_id = self.organisms[m].id;
        let father_lineage = self.organisms[m].lineage;
        let female = &mut self.organisms[f];
        female.pregnancy = Some(Pregnancy {
            father,
            father_id,
            father_lineage,
            fertility,
            due_tick: tick + (female.traits.gestation_days * tpd) as u64,
        });
        female.needs.breeding = 0.0;
        if female.lineage != father_lineage {
            self.counters.cross_lineage_matings += 1;
        }
        let male = &mut self.organisms[m];
        male.needs.breeding = 0.0;
        male.breed_ready_tick = tick + (tpd / 2.0) as u64;
        self.counters.matings += 1;
    }

    /// Juveniles of caring species are fed by a nearby mother (close contact:
    /// infection can pass either way, `Balance::disease_kin`).
    pub(crate) fn nurse(&mut self, i: usize) {
        let o = &self.organisms[i];
        let needy = o.needs.hunger >= 10.0 || o.needs.thirst >= 10.0;
        if o.matured || o.growth >= 0.7 || o.traits.birth == BirthType::AbandonedEggs || !needy {
            return;
        }
        let Some(&mi) = o.mother.and_then(|m| self.index.get(&m)) else {
            return;
        };
        let mother = &self.organisms[mi];
        if !mother.alive || mother.needs.hunger > 80.0 || mother.pos.dist(o.pos) > 1.5 {
            return;
        }
        let amount = (300.0 * self.config.dt_days()).min(o.needs.hunger);
        let cost = amount * o.food_per_hunger() / mother.food_per_hunger() * 1.2;
        let live = o.traits.birth == BirthType::Live;
        let o = &mut self.organisms[i];
        o.needs.hunger -= amount;
        if live {
            o.needs.thirst = (o.needs.thirst - amount).max(0.0);
        }
        self.organisms[mi].needs.hunger = (self.organisms[mi].needs.hunger + cost).min(100.0);
        self.contact(i, mi, (self.config.balance.disease_kin * self.config.dt_days()).min(1.0));
    }

    fn raid_clutch(&mut self, i: usize, rng: &mut SimRng) {
        let o = &self.organisms[i];
        if o.traits.carnivory < 0.3 || o.needs.hunger < 35.0 || !o.matured {
            return;
        }
        let key = self.map.tile_coords(o.pos);
        let Some(&k) = self.clutch_at.get(&key) else {
            return;
        };
        let c = &self.clutches[k];
        if c.eggs.is_empty() || c.owner == o.owner {
            return;
        }
        // A present guardian may drive the raider off.
        if let Some(&gi) = c.guardian.and_then(|g| self.index.get(&g)) {
            let g = &self.organisms[gi];
            if g.alive && g.pos.dist(c.pos) < 1.5 && rng.gen::<f32>() < g.power() / (g.power() + o.power()) {
                return;
            }
        }
        let n = c.eggs.len().min(3);
        let food = n as f32 * c.egg_mass * self.config.balance.meat_per_kg;
        let eaten = food * o.traits.meat_efficiency(self.config.balance.generalist_tax) / o.food_per_hunger();
        let c = &mut self.clutches[k];
        c.eggs.truncate(c.eggs.len() - n);
        self.counters.eggs_lost += n as u64;
        self.counters.eggs.entry(c.lineage).or_default().raided += n as u64;
        let o = &mut self.organisms[i];
        o.needs.hunger = (o.needs.hunger - eaten).max(0.0);
    }

    /// Needs, temperature, health. Returns a cause of death if health ran out.
    ///
    /// `burrow` is `burrow_inputs`: a shared burrow lets carriers with modest
    /// burrowing shelter, and is deeper and better insulated than a lone
    /// burrow.
    fn update_needs(&self, o: &mut Organism, intent: &Intent, moved: f32, dt: f32, burrow: (f32, f32)) -> Option<DeathCause> {
        let daytime = self.is_daytime();
        let tick = self.tick;
        let pos = o.pos;
        let tile = self.map.tile_at(pos);
        let (tile_temp, open, water) = (tile.temp, tile.is_open(), tile.water_depth);
        let body = o.traits.body;
        let drowning = !body.can_fly && traverse(&body, tile, &self.map.weather_at(pos), false) == Traverse::Blocked;
        let (shared, depth) = burrow;

        let bal = self.config.balance;
        let dormant_rate = self.dormant_rate(o);
        // Group navigation: travelling with its group costs a migrant less.
        let nav = if intent.action == Action::Migrate { self.navigation_bonus(o) } else { 0.0 };
        // Seasonal coat and timed breeding (`seasonal`).
        let (coat_insulation, coat_mismatch) = self.coat_effect(o);
        let drive = self.breeding_drive_rate(o);
        let st = o.traits.stats;
        let deep = o.dormancy.is_deep();
        let torpid = o.dormancy == Dormancy::Torpor;
        let resting =
            matches!(intent.action, Action::Rest | Action::Bask | Action::Guard | Action::Idle | Action::Dormant) && moved < 0.01;
        o.sheltered = resting && depth > 0.5 && water == 0.0;
        // Burrows buffer temperature toward 10 °C.
        let buffer = (0.6 + 0.6 * bal.shared_burrow * shared).min(0.95);
        let temp = if o.sheltered { tile_temp + (10.0 - tile_temp) * buffer } else { tile_temp };

        // A cold-blooded body's temperature follows the air (plus basking).
        let mut body_t = temp;
        let (cold, heat) = if o.traits.warm {
            o.activity = 1.0;
            let insulation = st.insulation + coat_insulation;
            let low = 12.0 - 30.0 * insulation;
            let high = 30.0 - 8.0 * insulation;
            ((low - temp).max(0.0), (temp - high).max(0.0))
        } else {
            let basking = daytime && open && matches!(intent.action, Action::Bask | Action::Rest | Action::Idle);
            let bask = if basking { 10.0 * (1.0 - o.traits.fur_mismatch - coat_mismatch).max(0.0) } else { 0.0 };
            body_t = temp + bask;
            o.activity = ((body_t - 2.0) / 20.0).clamp(0.15, 1.0);
            ((6.0 - body_t).max(0.0), (body_t - 38.0).max(0.0))
        };
        // A deep sleeper in its den is safe from the weather.
        let (cold, heat) = if deep {
            o.activity = 0.15;
            (0.0, 0.0)
        } else {
            if torpid {
                o.activity = o.activity.min(0.4);
            }
            (cold, heat)
        };
        o.needs.cold = cold;
        o.needs.heat = heat;

        let activity_mult = if resting {
            0.8
        } else if intent.sprint && moved > 0.0 {
            2.0
        } else if moved > 0.0 {
            1.2
        } else {
            1.0
        };
        let thermo = if torpid {
            1.0
        } else if o.traits.warm {
            1.0 + 0.04 * cold
        } else {
            cold_blooded_thermo(o.activity, body_t, bal.cold_heat_burn)
        };
        // Pregnancy costs scale with the litter's total mass.
        let pregnant = if o.pregnancy.is_some() { 1.0 + bal.pregnancy_cost * o.traits.litter_burden } else { 1.0 };
        let size = o.pheno[Gene::BodySize];
        let watch = if intent.action == Action::Sentry { 1.0 + bal.sentry_cost } else { 1.0 };
        let travel = (1.0 - bal.group_navigation / 3.0 * nav).max(0.0);
        // Carnivores eat big, infrequent meals: hunger rises more slowly. The
        // discount follows the meat gut (`Traits::meat_efficiency`), so a
        // generalist gets less of it than its carnivory alone would say.
        let diet = 1.0 - bal.carnivore_hunger_discount * o.traits.meat_efficiency(bal.generalist_tax);
        // Crowding: competition with many kin, felt by group-intolerant organisms.
        let crowd = 1.0 + bal.crowding * (o.kin_nearby as f32 - 8.0).max(0.0) * (1.0 - o.pheno[Gene::GroupTolerance]);
        // Small bodies burn through food and water faster (`Balance::size_burn`).
        let size_burn = if bal.size_burn == 0.0 { 1.0 } else { o.traits.body.mass.powf(-bal.size_burn) };
        let mut hunger =
            bal.hunger_rate * st.metabolism * diet * crowd * thermo * activity_mult * pregnant * watch * travel * size_burn * dt;
        let mut thirst =
            bal.thirst_rate * (1.0 + 0.05 * heat) * (0.8 + 0.4 * size) * if o.traits.warm { 1.0 } else { 0.7 } * size_burn * dt;
        if o.dormancy != Dormancy::Awake {
            hunger *= dormant_rate;
            if deep {
                thirst *= dormant_rate;
            }
        }
        // Stored fat pays for a dormancy, and for hard times.
        if o.fat > 0.0 && (deep || o.needs.hunger > 50.0) {
            let paid = hunger.min(o.fat);
            o.fat -= paid;
            hunger -= paid;
        }
        let n = &mut o.needs;
        n.hunger += hunger;
        n.thirst += thirst;
        n.energy += if resting { 120.0 * if o.sheltered { 1.2 } else { 1.0 } } else { 15.0 } * dt;
        if n.hunger >= 90.0 {
            n.energy -= 30.0 * dt;
        }
        n.hunger = n.hunger.min(100.0);
        n.thirst = n.thirst.min(100.0);
        n.energy = n.energy.clamp(0.0, 100.0);

        // Health
        let mut worst: Option<(f32, DeathCause)> = None;
        let mut harm = |amount: f32, cause: DeathCause| {
            if amount > 0.0 && worst.is_none_or(|(w, _)| amount > w) {
                worst = Some((amount, cause));
            }
            amount
        };
        let mut damage = 0.0;
        if n.hunger >= 100.0 {
            damage += harm(bal.starvation_damage * dt, DeathCause::Starvation);
        }
        if n.thirst >= 100.0 {
            damage += harm(40.0 * dt, DeathCause::Thirst);
        }
        if o.traits.warm && cold > 15.0 {
            damage += harm((cold - 15.0) * 1.5 * dt, DeathCause::Exposure);
        }
        if !o.traits.warm && cold > 10.0 && !o.sheltered {
            damage += harm((cold - 10.0) * 2.0 * dt, DeathCause::Exposure);
        }
        if heat > 8.0 {
            damage += harm((heat - 8.0) * 2.0 * dt, DeathCause::Exposure);
        }
        if drowning {
            damage += harm(150.0 * dt, DeathCause::Drowning);
        }
        if o.infection == Infection::Acute {
            damage += harm(bal.disease_damage * dt, DeathCause::Disease);
        }
        o.health -= damage;
        if damage == 0.0 && n.hunger < 70.0 && n.thirst < 70.0 {
            o.health = (o.health + 15.0 * dt).min(100.0);
        }

        // Drives
        let can_breed = o.can_breed(tick);
        let n = &mut o.needs;
        if drive < 0.0 {
            n.breeding = (n.breeding + drive * dt).max(0.0);
        } else if can_breed && n.hunger < 70.0 {
            n.breeding = (n.breeding + drive * dt).min(100.0);
        } else if !o.matured {
            n.breeding = 0.0;
        }
        let gt = o.pheno[Gene::GroupTolerance];
        if gt > 0.3 {
            n.social = if o.kin_nearby == 0 { (n.social + 50.0 * gt * dt).min(100.0) } else { (n.social - 100.0 * dt).max(0.0) };
        }
        o.age_ticks += 1;

        if o.health <= 0.0 {
            Some(worst.map(|(_, c)| c).unwrap_or(DeathCause::Starvation))
        } else {
            None
        }
    }

    fn old_age_death(&self, o: &Organism, rng: &mut SimRng) -> bool {
        let frac = o.age_days(self.config.ticks_per_day) / o.traits.lifespan_days;
        if frac < 0.8 {
            return false;
        }
        // Hazard rises from 0 at 80% of lifespan; nobody passes 130%.
        frac > 1.3 || rng.gen::<f32>() < self.config.dt_days() * 4.0 * (frac - 0.8)
    }

    /// Young left to fend for themselves (abandoned eggs) compete with their
    /// own kind for food and shelter, and the losers starve: the doc's "many
    /// offspring: most die young", made density dependent.
    fn larval_competition_death(&self, o: &Organism, rng: &mut SimRng) -> bool {
        if o.matured || o.traits.birth != BirthType::AbandonedEggs || o.growth >= self.config.balance.larval_stage {
            return false;
        }
        let pace = o.traits.pace_of_life(o.pheno[Gene::BodySize]);
        let hazard = larval_hazard(o.kin_nearby, pace, self.config.balance.larval_competition);
        hazard > 0.0 && rng.gen::<f32>() < hazard * self.config.dt_days()
    }

    /// Adults of fast-living species wear out (disease, accidents, the cost
    /// of breeding) whether or not anything eats them: the doc's "live fast,
    /// die young" for big litters, so adults don't pile up across years.
    fn attrition_death(&self, o: &Organism, rng: &mut SimRng) -> bool {
        if !o.matured {
            return false;
        }
        let hazard = self.config.balance.adult_mortality * o.traits.pace_of_life(o.pheno[Gene::BodySize]);
        hazard > 0.0 && rng.gen::<f32>() < hazard * self.config.dt_days()
    }

    /// Density-dependent breeding: in a crowd of its own species a mother
    /// carries fewer young to term (embryo resorption, suppressed breeding),
    /// `1 / (1 + (d / breeding_density)^4)`. `d` is her species' members per
    /// tile around her, felt as `crowding` is (scaled by 1 - group tolerance)
    /// and most by fast breeders (scaled by the square root of pace of life:
    /// voles 0.4, foxes 0.19, grazers ~0), whose numbers food alone reins in
    /// only after they overshoot. Young of abandoned eggs face larval
    /// competition instead, so their parents are exempt.
    fn breeding_suppression(&self, i: usize) -> f32 {
        let o = &self.organisms[i];
        let k = self.config.balance.breeding_density;
        if k <= 0.0 || o.traits.birth == BirthType::AbandonedEggs {
            return 1.0;
        }
        let size = o.pheno[Gene::BodySize];
        let d = self.density.per_tile(o.pos, o.lineage)
            * (1.0 - o.pheno[Gene::GroupTolerance])
            * o.traits.pace_of_life(size).sqrt();
        1.0 / (1.0 + (d / k).powi(4))
    }

    /// Growth toward maturity; true when `o` matures this tick (its parents
    /// are credited in `finish`).
    fn grow(&self, o: &mut Organism) -> bool {
        let tpd = self.config.ticks_per_day;
        if o.matured {
            return false;
        }
        let progress = (o.age_days(tpd) / o.traits.maturity_days).min(1.0);
        o.growth = o.traits.newborn_growth + (1.0 - o.traits.newborn_growth) * progress;
        if progress >= 1.0 {
            o.matured = true;
            o.growth = 1.0;
            return true;
        }
        false
    }

    fn give_birth(&mut self, i: usize, rng: &mut SimRng) {
        let tick = self.tick;
        let tpd = self.config.ticks_per_day as f32;
        let mutation = self.config.mutation;
        let load_rate = self.config.balance.recessive_mutation;
        let Some(preg) = self.organisms[i].pregnancy.take() else {
            return;
        };
        let mother = &self.organisms[i];
        let traits = mother.traits;
        let owner = mother.owner;
        let expected = traits.litter as f32 * preg.fertility * mother.condition().clamp(0.3, 1.0) * self.breeding_suppression(i);
        let mut count = expected.floor() as usize + usize::from(rng.gen::<f32>() < expected.fract());
        if !traits.birth.lays_eggs() {
            count = count.min(self.cap_room(mother.lineage));
        }
        let mother = &mut self.organisms[i];
        mother.breed_ready_tick = tick + (traits.recovery_days * tpd) as u64;
        if count == 0 {
            return;
        }
        if preg.father_lineage != mother.lineage {
            *self.counters.hybrids.entry((mother.lineage, preg.father_lineage)).or_default() += count as u64;
        }
        let mut load_rng = rng_for(self.config.seed, stream::GENETICS, tick, mother.id << 2);
        let genomes: Vec<Genome> = (0..count)
            .map(|_| {
                let mut g = Genome::inherit_genes(&mother.genome, &preg.father, owner, mutation, rng);
                g.inherit_load(&mother.genome, &preg.father, owner, load_rate, &mut load_rng);
                g
            })
            .collect();
        let (pos, mother_id, generation, lineage) = (mother.pos, mother.id, mother.generation + 1, mother.lineage);
        match traits.birth {
            BirthType::Live => {
                for g in genomes {
                    let jitter = Vec2::new(rng.gen_range(-0.4..0.4), rng.gen_range(-0.4..0.4));
                    let at = self.map.clamp(pos + jitter);
                    let at = if self.passable_for(&self.organisms[i], at) { at } else { pos };
                    self.spawn_newborn(g, owner, lineage, at, generation, mother_id, preg.father_id, traits.newborn_growth, rng);
                }
                self.counters.births += count as u64;
            }
            BirthType::IncubatedEggs | BirthType::AbandonedEggs => {
                // Laying costs the clutch's mass in one go.
                mother.needs.hunger = (mother.needs.hunger + 60.0 * traits.litter_burden).min(100.0);
                let guardian = (traits.birth == BirthType::IncubatedEggs).then_some(mother_id);
                if guardian.is_some() {
                    mother.nest = Some(pos);
                }
                let egg_mass = mother.traits.body.mass * 0.05;
                self.counters.eggs.entry(lineage).or_default().laid += count as u64;
                self.clutch_at.insert(self.map.tile_coords(pos), self.clutches.len());
                self.clutches.push(Clutch {
                    pos,
                    owner,
                    mother: mother_id,
                    father: preg.father_id,
                    generation,
                    lineage,
                    eggs: genomes,
                    hatch_tick: tick + (traits.incubation_days * tpd) as u64,
                    guardian,
                    warm_blooded: traits.warm,
                    chill: 0.0,
                    newborn_growth: traits.newborn_growth,
                    egg_mass,
                });
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_newborn(
        &mut self,
        genome: Genome,
        owner: PlayerId,
        lineage: u32,
        pos: Vec2,
        generation: u32,
        mother: OrgId,
        father: OrgId,
        growth: f32,
        rng: &mut SimRng,
    ) -> OrgId {
        let sex = if rng.gen::<bool>() { Sex::Female } else { Sex::Male };
        let mut o = Organism::new(0, owner, genome, sex, pos);
        o.generation = generation;
        o.lineage = lineage;
        *self.counters.born.entry(lineage).or_default() += 1;
        o.mother = Some(mother);
        o.father = Some(father);
        // A migrator's young share their mother's summer range.
        if let Some(&mi) = self.index.get(&mother) {
            o.range = self.organisms[mi].range;
        }
        o.growth = growth;
        o.needs.hunger = 30.0;
        self.insert(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organism::Needs;
    use crate::{presets, WorldConfig};

    #[test]
    fn larval_hazard_rises_steeply_with_kin() {
        assert_eq!(larval_hazard(30, 0.5, 0.0), 0.0);
        assert!(larval_hazard(5, 1.0, 18.0) < 0.01);
        assert!((larval_hazard(18, 1.0, 18.0) - 1.0).abs() < 1e-6);
        assert!(larval_hazard(30, 1.0, 18.0) > 5.0);
        // Fast-living kinds (big clutches of small young) compete hardest.
        let traits = |litter: u32, size: f32| {
            crate::stats::derive(&Genome::uniform(&crate::FounderDesign::new("x", crate::Thermo::Cold, BirthType::AbandonedEggs, litter).with(Gene::BodySize, size).values(), 1).phenotype())
        };
        let frog = traits(10, 0.12);
        let pair = traits(2, 0.5);
        let k = crate::Balance::default().larval_competition;
        let (frog_h, pair_h) = (larval_hazard(20, frog.pace_of_life(0.12), k), larval_hazard(20, pair.pace_of_life(0.5), k));
        assert!(frog_h > 0.5, "frog {frog_h}");
        assert!(pair_h < 0.01, "pair {pair_h}");
    }

    /// Cold blood is cheapest when the body is cool and sluggish; a hot body
    /// burns more than one at full activity.
    #[test]
    fn cold_blooded_hunger_follows_body_temperature() {
        let burn = crate::Balance::default().cold_heat_burn;
        assert!(burn > 0.0);
        let at = |t: f32| cold_blooded_thermo(((t - 2.0) / 20.0).clamp(0.15, 1.0), t, burn);
        assert!(at(5.0) < at(15.0) && at(15.0) < at(22.0) && at(22.0) < at(32.0));
        assert!((at(22.0) - 1.0).abs() < 1e-6);
        assert!((at(32.0) - (1.0 + burn)).abs() < 1e-5);
        // Without the burn, the old curve: flat above full activity.
        assert_eq!(cold_blooded_thermo(1.0, 35.0, 0.0), 1.0);
    }

    /// Hunger relieved per unit of vegetation grazed: full for a herbivore,
    /// half for an untaxed omnivore (the old linear rule), clearly less than
    /// half under the default generalist tax.
    #[test]
    fn generalists_get_less_from_grazing() {
        let relief_per_veg = |carnivory: f32, tax: f32| {
            let mut w = wild_world(5000);
            w.config.balance.generalist_tax = tax;
            let i = w.organisms.iter().position(|o| o.lineage == 0 && o.matured).unwrap();
            let o = &mut w.organisms[i];
            o.traits.carnivory = carnivory;
            o.traits.seasonal.winter = None;
            o.traits.seasonal.estivation = 0.0;
            o.needs.hunger = 80.0;
            let pos = o.pos;
            w.map.tile_at_mut(pos).veg = 1000.0;
            w.eat_plants(i, 0.1);
            let eaten = 1000.0 - w.map.tile_at(pos).veg;
            assert!(eaten > 0.0);
            (80.0 - w.organisms[i].needs.hunger) / eaten
        };
        let herbivore = relief_per_veg(0.0, 0.0);
        let linear = relief_per_veg(0.5, 0.0) / herbivore;
        assert!((linear - 0.5).abs() < 1e-3, "{linear}");
        let tax = crate::Balance::default().generalist_tax;
        let taxed = relief_per_veg(0.5, tax) / herbivore;
        assert!((taxed - 0.5 * (1.0 - tax)).abs() < 1e-3 && taxed < 0.4, "{taxed}");
    }

    /// Small bodies burn more hunger points per day than large ones under
    /// `size_burn`; without it hunger points don't depend on size.
    #[test]
    fn small_bodies_burn_faster() {
        let config = WorldConfig { seed: 9, width: 32, height: 32, ..Default::default() };
        let mut w = World::new(config);
        let d = crate::FounderDesign::new("d", crate::Thermo::Warm, BirthType::Live, 2);
        w.found_lineage_unchecked(1, &d.founders(1));
        let hunger_after = |w: &mut World, kg: f32, size_burn: f32| {
            w.config.balance.size_burn = size_burn;
            let o = &mut w.organisms[0];
            o.traits.body.mass = kg;
            o.needs.hunger = 0.0;
            let mut o = w.organisms[0].clone();
            w.update_needs(&mut o, &Intent { action: Action::Rest, ..Intent::idle() }, 0.0, 0.1, (0.0, 0.0));
            o.needs.hunger
        };
        let burn = crate::Balance::default().size_burn;
        assert!(burn > 0.0);
        // 0.36 kg (body size 0.1) against 6.3 kg (0.6).
        let ratio = hunger_after(&mut w, 0.36, burn) / hunger_after(&mut w, 6.3, burn);
        assert!((ratio - (0.36f32 / 6.3).powf(-burn)).abs() < 1e-3 && ratio > 1.5, "{ratio}");
        let (small, large) = (hunger_after(&mut w, 0.36, 0.0), hunger_after(&mut w, 6.3, 0.0));
        assert!((small - large).abs() < 1e-4 * large, "{small} vs {large}");
    }

    /// A crowd of froglets thins out; the same crowd without competition doesn't.
    #[test]
    fn crowded_froglets_die_young() {
        let deaths = |competition: f32| {
            let mut config = WorldConfig { seed: 5, width: 48, height: 48, ..Default::default() };
            config.balance.larval_competition = competition;
            let mut w = World::new(config);
            let frogs = presets::player_designs().into_iter().find(|d| d.name == "Toxic frogs").unwrap();
            w.found_lineage_unchecked(1, &frogs.founders(80));
            for o in &mut w.organisms {
                o.matured = false;
                o.age_ticks = 0;
                o.growth = o.traits.newborn_growth;
            }
            w.run_days(2);
            w.counters.deaths_by_lineage.get(&(0, DeathCause::Starvation)).copied().unwrap_or(0)
        };
        let (with, without) = (deaths(18.0), deaths(0.0));
        assert!(with > without + 10, "deaths with competition {with}, without {without}");
    }

    fn wild_world(wild_cap: usize) -> World {
        let config = WorldConfig { seed: 7, width: 48, height: 48, wild_cap, ..Default::default() };
        let mut w = World::new(config);
        let wild = presets::wild_designs();
        w.found_lineage_unchecked(crate::WILD, &wild[0].1.clone().founders(40)); // voles
        w.found_lineage_unchecked(crate::WILD, &wild[1].1.clone().founders(10)); // antelope
        w
    }

    fn make_pregnant(w: &mut World, i: usize) {
        let o = &mut w.organisms[i];
        o.pregnancy = Some(Pregnancy {
            father: Box::new(o.genome.clone()),
            father_id: o.id,
            father_lineage: o.lineage,
            fertility: 1.0,
            due_tick: 0,
        });
        o.needs.hunger = 0.0;
        o.health = 100.0;
    }

    /// Each wild species gets its share of the wild cap (by founders: 40
    /// voles and 10 antelope split 100 as 80/20): voles at their share can't
    /// breed while antelope with room left can.
    #[test]
    fn wild_cap_is_split_per_species() {
        let mut w = wild_world(100);
        w.config.balance.breeding_density = 0.0;
        assert_eq!(w.cap_usage(0), (40, 80));
        w.config.wild_cap = 50;
        assert_eq!(w.cap_usage(0), (40, 40));
        assert_eq!(w.cap_room(1), 0);
        // Five antelope die: room for antelope, still none for voles.
        let dead: Vec<usize> = (0..w.organisms.len()).filter(|&i| w.organisms[i].lineage == 1).take(5).collect();
        for i in dead {
            w.kill(i, DeathCause::Predation);
        }
        assert_eq!(w.cap_room(1), 5);
        let vole = w.organisms.iter().position(|o| o.lineage == 0 && o.sex == Sex::Female).unwrap();
        make_pregnant(&mut w, vole);
        let mut rng = crate::rng::rng_for(1, 0, 0, 0);
        w.give_birth(vole, &mut rng);
        assert_eq!(w.counters.births, 0);
        let antelope = w.organisms.iter().position(|o| o.alive && o.lineage == 1 && o.sex == Sex::Female).unwrap();
        make_pregnant(&mut w, antelope);
        w.give_birth(antelope, &mut rng);
        let born = w.counters.births as usize;
        assert!(born >= 1);
        assert_eq!(w.cap_usage(1), (5 + born, 10));
    }

    /// An incubating parent recovers from the end of her brood, as a live
    /// bearer does from the birth (`brood_recovery`): she can't court on the
    /// nest, and after the hatch she needs her full recovery time again.
    #[test]
    fn brooders_recover_from_the_hatch() {
        let mut w = presets::standard_world(WorldConfig { seed: 3, width: 64, height: 64, ..Default::default() }, 1);
        w.config.balance.breeding_density = 0.0;
        let i = w.organisms.iter().position(|o| o.lineage == 7 && o.sex == Sex::Female).unwrap();
        assert_eq!(w.organisms[i].traits.birth, BirthType::IncubatedEggs);
        (w.organisms[i].matured, w.organisms[i].growth) = (true, 1.0);
        make_pregnant(&mut w, i);
        let mut rng = crate::rng::rng_for(1, 0, 0, 0);
        w.give_birth(i, &mut rng);
        let k = w.clutches.iter().position(|c| c.guardian == Some(w.organisms[i].id)).expect("a guarded clutch");
        assert!(w.organisms[i].nest.is_some());
        let recovery = (w.organisms[i].traits.recovery_days * w.config.ticks_per_day as f32) as u64;
        // Long past her recovery from laying, still on the nest: no courting.
        w.tick += recovery + 10;
        assert!(!w.organisms[i].can_breed(w.tick));
        // The eggs hatch: free of the nest, but recovering from now.
        w.clutches[k].hatch_tick = w.tick;
        w.update_clutches(&mut rng);
        let o = &w.organisms[i];
        assert!(o.nest.is_none());
        assert_eq!(o.breed_ready_tick, w.tick + recovery);
        assert!(!o.can_breed(w.tick) && o.can_breed(w.tick + recovery));
    }

    /// A mother in a dense colony of her own species carries fewer young.
    #[test]
    fn crowded_mothers_bear_smaller_litters() {
        let mut w = wild_world(5000);
        w.config.balance.breeding_density = 0.15;
        let center = Vec2::new(4.0, 4.0);
        for o in w.organisms.iter_mut().filter(|o| o.lineage == 0) {
            o.pos = center;
        }
        let lone = w.organisms.iter().position(|o| o.lineage == 1).unwrap();
        w.organisms[lone].pos = Vec2::new(40.0, 40.0);
        w.density.rebuild(&w.organisms, w.lineages.len());
        let crowded = w.breeding_suppression(0);
        assert!(crowded < 0.7, "suppression in a colony of 40: {crowded}");
        assert!(w.breeding_suppression(lone) > 0.99);
        w.config.balance.breeding_density = 0.0;
        assert_eq!(w.breeding_suppression(0), 1.0);
    }

    /// A fox on a fleeing adult antelope: fast, alert, grown and rested prey
    /// slips strikes more often than a calf, a tired or an unwary animal.
    #[test]
    fn escape_is_a_contest_of_speed_and_agility() {
        let w = presets::standard_world(WorldConfig { seed: 3, width: 48, height: 48, ..Default::default() }, 1);
        let pick = |lineage: u32| {
            let mut o = w.organisms.iter().find(|o| o.lineage == lineage).unwrap().clone();
            (o.growth, o.matured, o.action, o.needs.energy) = (1.0, true, Action::Flee, 80.0);
            o
        };
        let (fox, adult) = (pick(3), pick(1));
        let q = w.escape_chance(&fox, &adult);
        assert!(q > 0.4 && q < 0.95, "adult {q}");
        let calf = Organism { growth: 0.3, matured: false, ..adult.clone() };
        let tired = Organism { needs: Needs { energy: 10.0, ..adult.needs }, ..adult.clone() };
        let unwary = Organism { action: Action::Graze, ..adult.clone() };
        for (what, o) in [("calf", calf), ("tired", tired), ("unwary", unwary)] {
            let qo = w.escape_chance(&fox, &o);
            assert!(qo < q - 0.05, "{what} {qo} vs adult {q}");
        }
    }

    /// Prey that slips a strike bolts, and the hunter can't strike again
    /// until it recovers: no re-roll next tick.
    #[test]
    fn slipped_strike_opens_an_escape_window() {
        let mut config = WorldConfig { seed: 3, width: 48, height: 48, ..Default::default() };
        config.balance.escape = 10.0; // capped at 0.95
        let mut w = presets::standard_world(config, 1);
        let fox = w.organisms.iter().position(|o| o.lineage == 3).unwrap();
        let prey = w.organisms.iter().position(|o| o.lineage == 1).unwrap();
        for o in [fox, prey] {
            let o = &mut w.organisms[o];
            (o.growth, o.matured, o.action, o.needs.energy, o.mother) = (1.0, true, Action::Flee, 80.0, None);
        }
        w.organisms[fox].pos = w.organisms[prey].pos + Vec2::new(0.6, 0.0);
        let mut rng = crate::rng::rng_for(3, crate::rng::stream::APPLY, 0, 0);
        w.attack(fox, prey, &mut rng);
        assert!(w.organisms[prey].alive);
        assert_eq!(w.counters.failed_hunts, 1);
        assert!(w.organisms[fox].strike_ready_tick > w.tick);
        assert!(w.organisms[fox].pos.dist(w.organisms[prey].pos) > 0.6, "prey bolted");
        // Back in reach at once, but the fox is still recovering.
        w.organisms[fox].pos = w.organisms[prey].pos + Vec2::new(0.6, 0.0);
        for _ in 0..5 {
            w.attack(fox, prey, &mut rng);
        }
        assert!(w.organisms[prey].alive);
        assert_eq!((w.counters.failed_hunts, w.counters.kills), (1, 0));
    }
}
