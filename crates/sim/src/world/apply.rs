//! The sequential half of a tick: movement, eating, predation, mating, needs,
//! death, birth and growth.

use rand::Rng;

use super::{Clutch, Intent, World};
use crate::design::BirthType;
use crate::genome::{Gene, Genome, PlayerId};
use crate::mating;
use crate::math::Vec2;
use crate::organism::{Action, DeathCause, OrgId, Organism, Pregnancy, Sex};
use crate::rng::SimRng;
use crate::terrain::{traverse, Traverse};

/// Most hunger points an organism can eat per day (so a meal takes a few ticks).
const MAX_INTAKE_PER_DAY: f32 = 1200.0;
const MAX_DRINK_PER_DAY: f32 = 1500.0;
/// Energy per tile walked at endurance 1.0.
const ENERGY_PER_TILE: f32 = 0.6;
const MATING_REACH: f32 = 1.2;

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

        // The chosen action's effect.
        match intent.action {
            Action::Graze => {
                self.eat_plants(i, 1.0);
                self.eat_meat(i, 1.0);
            }
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
        if !matches!(intent.action, Action::Flee | Action::Graze | Action::Scavenge | Action::Hunt) {
            let o = &self.organisms[i];
            if o.needs.hunger > 30.0 {
                self.eat_plants(i, 0.3);
                self.eat_meat(i, 0.3);
            }
            if self.organisms[i].needs.thirst > 30.0 && intent.action != Action::Drink {
                self.drink(i);
            }
        }
        self.raid_clutch(i, rng);
        self.nurse(i);

        let cause = self.update_needs(i, intent, moved, dt);
        if let Some(cause) = cause {
            self.kill(i, cause);
            return;
        }
        if self.old_age_death(i, rng) {
            self.kill(i, DeathCause::OldAge);
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
                let cost = dist * ENERGY_PER_TILE * e * if sprinting { 2.5 } else { 1.0 } / endurance;
                let o = &mut self.organisms[i];
                o.pos = new_pos;
                o.needs.energy = (o.needs.energy - cost).max(0.0);
                return dist;
            }
        }
        0.0
    }

    fn eat_plants(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let eff = 1.0 - o.traits.carnivory;
        if eff < 0.05 || o.needs.hunger < 5.0 {
            return;
        }
        let want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(o.needs.hunger);
        let fph = o.food_per_hunger();
        let pos = o.pos;
        let tile = self.map.tile_at_mut(pos);
        let got = (want * fph / eff).min(tile.veg);
        tile.veg -= got;
        self.organisms[i].needs.hunger -= got * eff / fph;
    }

    fn eat_meat(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let eff = o.traits.carnivory;
        if eff < 0.2 || o.needs.hunger < 5.0 {
            return;
        }
        let want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(o.needs.hunger);
        let fph = o.food_per_hunger();
        let pos = o.pos;
        let tile = self.map.tile_at_mut(pos);
        let got = (want * fph / eff).min(tile.meat);
        tile.meat -= got;
        self.organisms[i].needs.hunger -= got * eff / fph;
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
        if !b.alive || a.pos.dist(b.pos) > self.config.balance.attack_reach {
            return;
        }
        let pw = a.power() * (1.0 + self.coop_bonus(a)) * self.config.balance.attack_power;
        let mut rs = b.resistance() * (1.0 + 1.5 * self.herd_bonus(b));
        // Prey that hasn't noticed the hunter is caught off guard.
        if b.action != Action::Flee {
            rs *= 1.0 - self.config.balance.ambush;
        }
        let mut p = pw * pw / (pw * pw + rs * rs + 1e-6);
        let tox = b.pheno[Gene::Toxins];
        if tox > 0.3 {
            p *= 1.0 - 0.6 * tox;
        }
        let prey_power = b.power();
        let prey_pos = b.pos;
        let prey_mass = b.mass();

        // Toxins hurt whoever bites, whether or not the attack succeeds.
        if tox > 0.3 {
            let a = &mut self.organisms[i];
            a.health -= 60.0 * tox * rng.gen_range(0.5..1.0);
            if a.health <= 0.0 {
                self.kill(i, DeathCause::Toxin);
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
        let fert = mating::fertility(female.genome.distance(&male.genome));
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
        let female = &mut self.organisms[f];
        female.pregnancy = Some(Pregnancy {
            father,
            father_id,
            fertility,
            due_tick: tick + (female.traits.gestation_days * tpd) as u64,
        });
        female.needs.breeding = 0.0;
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
        let o = &mut self.organisms[i];
        let st = o.traits.stats;
        let resting = matches!(intent.action, Action::Rest | Action::Bask | Action::Guard | Action::Idle) && moved < 0.01;
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
        let thermo = if o.traits.warm { 1.0 + 0.04 * cold } else { 0.4 + 0.6 * o.activity };
        // Pregnancy costs scale with the litter's total mass.
        let pregnant = if o.pregnancy.is_some() { 1.0 + bal.pregnancy_cost * o.traits.litter_burden } else { 1.0 };
        let size = o.pheno[Gene::BodySize];
        let n = &mut o.needs;
        // Carnivores eat big, infrequent meals: hunger rises more slowly.
        let diet = 1.0 - bal.carnivore_hunger_discount * o.traits.carnivory;
        // Crowding: competition with many kin, felt by group-intolerant organisms.
        let crowd = 1.0 + bal.crowding * (o.kin_nearby as f32 - 8.0).max(0.0) * (1.0 - o.pheno[Gene::GroupTolerance]);
        n.hunger += bal.hunger_rate * st.metabolism * diet * crowd * thermo * activity_mult * pregnant * dt;
        n.thirst += bal.thirst_rate * (1.0 + 0.05 * heat) * (0.8 + 0.4 * size) * if o.traits.warm { 1.0 } else { 0.7 } * dt;
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
            damage += harm(150.0 * dt, DeathCause::Exposure);
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
        let expected = traits.litter as f32 * preg.fertility * mother.condition().clamp(0.3, 1.0);
        let mut count = expected.floor() as usize + usize::from(rng.gen::<f32>() < expected.fract());
        if !traits.birth.lays_eggs() {
            count = count.min(self.config.cap_for(owner).saturating_sub(self.owner_count(owner)));
        }
        let mother = &mut self.organisms[i];
        mother.breed_ready_tick = tick + (traits.recovery_days * tpd) as u64;
        if count == 0 {
            return;
        }
        let genomes: Vec<Genome> =
            (0..count).map(|_| Genome::inherit(&mother.genome, &preg.father, owner, mutation, rng)).collect();
        let (pos, mother_id, generation, lineage) = (mother.pos, mother.id, mother.generation + 1, mother.lineage);
        match traits.birth {
            BirthType::Live => {
                for g in genomes {
                    let jitter = Vec2::new(rng.gen_range(-0.4..0.4), rng.gen_range(-0.4..0.4));
                    let at = self.map.clamp(pos + jitter);
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
        o.mother = Some(mother);
        o.father = Some(father);
        o.growth = growth;
        o.needs.hunger = 30.0;
        self.insert(o)
    }
}
