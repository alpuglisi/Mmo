//! The sequential half of a tick: movement, eating, predation, mating, needs,
//! death, birth and growth.

use rand::Rng;

use super::{Clutch, Intent, World};
use crate::design::BirthType;
use crate::genome::{Gene, Genome, PlayerId};
use crate::mating;
use crate::math::Vec2;
use crate::organism::{Action, DeathCause, Dormancy, OrgId, Organism, Pregnancy, Sex};
use crate::rng::SimRng;
use crate::terrain::{traverse, Traverse};

/// Most hunger points an organism can eat per day (so a meal takes a few ticks).
pub(crate) const MAX_INTAKE_PER_DAY: f32 = 1200.0;
const MAX_DRINK_PER_DAY: f32 = 1500.0;
/// Energy per tile walked at endurance 1.0.
const ENERGY_PER_TILE: f32 = 0.6;
const MATING_REACH: f32 = 1.2;

/// Daily death hazard of an uncared-for juvenile with `kin` of its own kind
/// nearby: `(kin / competition)^4`. Steep, because perceived kin saturate in
/// a crowd: a lone clutch of siblings barely notices (9 kin at the default:
/// 6% a day), a crowd is lethal (25 kin: 370% a day).
pub(crate) fn larval_hazard(kin: u16, competition: f32) -> f32 {
    if competition <= 0.0 {
        return 0.0;
    }
    (kin as f32 / competition).powi(4)
}

impl World {
    pub(crate) fn apply(&mut self, i: usize, intent: &Intent, rng: &mut SimRng) {
        if !self.organisms[i].alive {
            return;
        }
        let dt = self.config.dt_days();
        {
            let o = &mut self.organisms[i];
            o.action = intent.action;
            o.target = intent.target;
            o.kin_nearby = intent.kin_nearby;
            o.needs.safety = (intent.threat * 100.0).min(100.0);
            if let Some(w) = intent.new_wander {
                o.wander_target = Some(w);
            }
            if let Some(f) = intent.new_forage {
                o.forage_target = f;
            }
        }

        // Movement (chasing a live target follows it).
        let mut moved = 0.0;
        if let Some(target) = intent.target {
            let target = match intent.target_org {
                Some(j) if self.organisms[j].alive => self.organisms[j].pos,
                _ => target,
            };
            moved = self.move_toward(i, target, intent.sprint);
        }
        if intent.action == Action::Migrate && self.journey_death(i, moved, rng) {
            self.kill(i, DeathCause::Journey);
            return;
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
            return;
        }

        // Opportunistic eating and drinking while doing something else.
        let dormant = intent.action == Action::Dormant;
        if !dormant && !matches!(intent.action, Action::Flee | Action::Graze | Action::Scavenge | Action::Hunt) {
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

        if let Some(cause) = self.seasonal_tick(i, intent, moved, rng) {
            self.kill(i, cause);
            return;
        }
        let cause = self.update_needs(i, intent, moved, dt);
        if let Some(cause) = cause {
            self.kill(i, cause);
            return;
        }
        if self.old_age_death(i, rng) {
            self.kill(i, DeathCause::OldAge);
            return;
        }
        if self.larval_competition_death(i, rng) {
            self.kill(i, DeathCause::Starvation);
            return;
        }
        if self.attrition_death(i, rng) {
            self.kill(i, DeathCause::Attrition);
            return;
        }
        self.grow(i);
        if matches!(&self.organisms[i].pregnancy, Some(p) if self.tick >= p.due_tick) {
            self.give_birth(i, rng);
        }
    }

    fn move_toward(&mut self, i: usize, target: Vec2, sprint: bool) -> f32 {
        let o = &self.organisms[i];
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
        let step = speed.min(d);
        let dir = to * (1.0 / d);
        let body = o.traits.body;
        let pos = o.pos;
        let endurance = st.endurance;
        let fly = body.can_fly && o.needs.energy > 25.0 && !self.weather.grounds_flyers();
        // Stuck on a tile the body can't use (e.g. ice melted underneath):
        // struggle toward the target regardless.
        let stranded = !fly && traverse(&body, self.map.tile_at(pos), &self.weather, false) == Traverse::Blocked;

        for &angle in &[0.0f32, 0.6, -0.6, 1.2, -1.2, 1.9, -1.9] {
            let dd = dir.rotate(angle);
            let dest = self.map.clamp(pos + dd * step);
            let rule = if stranded {
                Traverse::Passable { speed: 0.3, energy: 2.0 }
            } else {
                traverse(&body, self.map.tile_at(dest), &self.weather, fly)
            };
            if let Traverse::Passable { speed: s, energy: e } = rule {
                let dist = (step * s.min(1.5)).min(d);
                let new_pos = self.map.clamp(pos + dd * dist);
                // The end point can fall short of the checked tile (or cut a
                // corner): never step onto a tile the body can't use.
                if !stranded
                    && traverse(&body, self.map.tile_at(new_pos), &self.weather, fly) == Traverse::Blocked
                {
                    continue;
                }
                let cost = dist * ENERGY_PER_TILE * e * if sprinting { 2.5 } else { 1.0 } / endurance;
                let o = &mut self.organisms[i];
                o.pos = new_pos;
                o.needs.energy = (o.needs.energy - cost).max(0.0);
                return dist;
            }
        }
        0.0
    }

    /// Graze the current tile. Fattening organisms (`seasonal::fat_room`)
    /// keep eating past full and store the surplus as fat.
    fn eat_plants(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let eff = 1.0 - o.traits.carnivory;
        let fat_room = self.fat_room(o);
        let room = o.needs.hunger + fat_room;
        if eff < 0.05 || room < 5.0 {
            return;
        }
        let want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(room);
        let fph = o.food_per_hunger();
        let pos = o.pos;
        let tile = self.map.tile_at_mut(pos);
        let got = (want * fph / eff).min(tile.veg);
        tile.veg -= got;
        self.feed(i, got * eff / fph, fat_room);
    }

    fn eat_meat(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let eff = o.traits.carnivory;
        let fat_room = self.fat_room(o);
        let room = o.needs.hunger + fat_room;
        if eff < 0.2 || room < 5.0 {
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
            p *= 1.0 - self.config.balance.burrow_shelter * b.traits.body.burrow;
        }
        let tox = b.pheno[Gene::Toxins];
        if tox > 0.3 {
            p *= 1.0 - 0.6 * tox;
        }
        let prey_power = b.power().max(guard_power);
        let prey_fights = super::decide::fight_back(b);
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
                self.move_toward(j, self.map.clamp(prey_pos + away * 4.0), true);
            }
            return;
        }

        // Prey that fights back hurts the hunter once they come to grips,
        // whether or not the hunter wins.
        if prey_fights > 0.0 && self.config.balance.fight_damage > 0.0 {
            let a = &mut self.organisms[i];
            a.health -= self.config.balance.fight_damage * prey_fights * rng.gen::<f32>();
            if a.alive && a.health <= 0.0 {
                self.kill(i, DeathCause::Predation);
                return;
            }
        }

        if rng.gen::<f32>() < p {
            self.kill(j, DeathCause::Predation);
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
                self.kill(i, DeathCause::Predation);
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
        let (f, m) = if a.sex == Sex::Female { (i, j) } else { (j, i) };
        let (female, male) = (&self.organisms[f], &self.organisms[m]);
        let mf = mating::mismatch(&female.pheno, male.display());
        let mm = mating::mismatch(&male.pheno, female.display());
        // Females choose; males are less picky.
        let accept_f = mf <= 1.0 || rng.gen::<f32>() < 0.05 * (-3.0 * (mf - 1.0)).exp();
        if !accept_f || mm > 1.5 {
            return;
        }
        let fert = mating::fertility(&female.genome, &male.genome);
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

    /// Juveniles of caring species are fed by a nearby mother.
    fn nurse(&mut self, i: usize) {
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
        let eaten = food * o.traits.carnivory / o.food_per_hunger();
        let c = &mut self.clutches[k];
        c.eggs.truncate(c.eggs.len() - n);
        self.counters.eggs_lost += n as u64;
        let o = &mut self.organisms[i];
        o.needs.hunger = (o.needs.hunger - eaten).max(0.0);
    }

    /// Needs, temperature, health. Returns a cause of death if health ran out.
    fn update_needs(&mut self, i: usize, intent: &Intent, moved: f32, dt: f32) -> Option<DeathCause> {
        let daytime = self.is_daytime();
        let breed_factor = self.season().breeding_factor();
        let tick = self.tick;
        let pos = self.organisms[i].pos;
        let tile = self.map.tile_at(pos);
        let (tile_temp, open, water) = (tile.temp, tile.is_open(), tile.water_depth);
        let body = self.organisms[i].traits.body;
        let drowning = !body.can_fly && traverse(&body, tile, &self.weather, false) == Traverse::Blocked;

        let bal = self.config.balance;
        let dormant_rate = self.dormant_rate(&self.organisms[i]);
        let o = &mut self.organisms[i];
        let st = o.traits.stats;
        let deep = o.dormancy.is_deep();
        let torpid = o.dormancy == Dormancy::Torpor;
        let resting =
            matches!(intent.action, Action::Rest | Action::Bask | Action::Guard | Action::Idle | Action::Dormant) && moved < 0.01;
        o.sheltered = resting && body.burrow > 0.5 && water == 0.0;
        // Burrows buffer temperature toward 10 °C.
        let temp = if o.sheltered { tile_temp + (10.0 - tile_temp) * 0.6 } else { tile_temp };

        let (cold, heat) = if o.traits.warm {
            o.activity = 1.0;
            let low = 12.0 - 30.0 * st.insulation;
            let high = 30.0 - 8.0 * st.insulation;
            ((low - temp).max(0.0), (temp - high).max(0.0))
        } else {
            let basking = daytime && open && matches!(intent.action, Action::Bask | Action::Rest | Action::Idle);
            let bask = if basking { 10.0 * (1.0 - o.traits.fur_mismatch) } else { 0.0 };
            let body_t = temp + bask;
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
            0.4 + 0.6 * o.activity
        };
        // Pregnancy costs scale with the litter's total mass.
        let pregnant = if o.pregnancy.is_some() { 1.0 + bal.pregnancy_cost * o.traits.litter_burden } else { 1.0 };
        let size = o.pheno[Gene::BodySize];
        // Carnivores eat big, infrequent meals: hunger rises more slowly.
        let diet = 1.0 - bal.carnivore_hunger_discount * o.traits.carnivory;
        // Crowding: competition with many kin, felt by group-intolerant organisms.
        let crowd = 1.0 + bal.crowding * (o.kin_nearby as f32 - 8.0).max(0.0) * (1.0 - o.pheno[Gene::GroupTolerance]);
        let mut hunger = bal.hunger_rate * st.metabolism * diet * crowd * thermo * activity_mult * pregnant * dt;
        let mut thirst = bal.thirst_rate * (1.0 + 0.05 * heat) * (0.8 + 0.4 * size) * if o.traits.warm { 1.0 } else { 0.7 } * dt;
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
        o.health -= damage;
        if damage == 0.0 && n.hunger < 70.0 && n.thirst < 70.0 {
            o.health = (o.health + 15.0 * dt).min(100.0);
        }

        // Drives
        let can_breed = o.can_breed(tick);
        let n = &mut o.needs;
        if can_breed && n.hunger < 70.0 {
            n.breeding = (n.breeding + 30.0 * breed_factor * dt).min(100.0);
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

    fn old_age_death(&self, i: usize, rng: &mut SimRng) -> bool {
        let o = &self.organisms[i];
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
    fn larval_competition_death(&self, i: usize, rng: &mut SimRng) -> bool {
        let o = &self.organisms[i];
        if o.matured || o.traits.birth != BirthType::AbandonedEggs {
            return false;
        }
        let hazard = larval_hazard(o.kin_nearby, self.config.balance.larval_competition);
        hazard > 0.0 && rng.gen::<f32>() < hazard * self.config.dt_days()
    }

    /// Adults of fast-living species wear out (disease, accidents, the cost
    /// of breeding) whether or not anything eats them: the doc's "live fast,
    /// die young" for big litters, so adults don't pile up across years.
    fn attrition_death(&self, i: usize, rng: &mut SimRng) -> bool {
        let o = &self.organisms[i];
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

    fn grow(&mut self, i: usize) {
        let tpd = self.config.ticks_per_day;
        let o = &mut self.organisms[i];
        if o.matured {
            return;
        }
        let progress = (o.age_days(tpd) / o.traits.maturity_days).min(1.0);
        o.growth = o.traits.newborn_growth + (1.0 - o.traits.newborn_growth) * progress;
        if progress >= 1.0 {
            o.matured = true;
            o.growth = 1.0;
            // Credit parents: only offspring raised to breeding age count.
            let parents = [o.mother, o.father];
            for &k in parents.iter().flatten().filter_map(|p| self.index.get(p)) {
                self.organisms[k].offspring_raised += 1;
            }
        }
    }

    fn give_birth(&mut self, i: usize, rng: &mut SimRng) {
        let tick = self.tick;
        let tpd = self.config.ticks_per_day as f32;
        let mutation = self.config.mutation;
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
        let genomes: Vec<Genome> =
            (0..count).map(|_| Genome::inherit(&mother.genome, &preg.father, owner, mutation, rng)).collect();
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
        assert_eq!(larval_hazard(30, 0.0), 0.0);
        assert!(larval_hazard(5, 18.0) < 0.01);
        assert!((larval_hazard(18, 18.0) - 1.0).abs() < 1e-6);
        assert!(larval_hazard(30, 18.0) > 5.0);
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
