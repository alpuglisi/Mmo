//! Mating-transmitted disease and hidden-recessive deaths ("Genetics:
//! balancing picky vs flexible lineages" in the design doc).
//!
//! Pathogens appear at matings (more often between founding lineages), pass
//! between mates and from nursing mothers to their young (and back), drain the
//! health of the acutely sick and cut fertility. An acute infection ends in
//! immunity or, sometimes, lifelong symptomless carriage. Lineages that mate
//! widely meet more pathogens; picky ones avoid sick mates, whose drained
//! condition dulls their display.
//!
//! Draws come from their own stream (`stream::GENETICS`, per tick and
//! organism), so disease changes a world only through infections, never by
//! shifting other systems' random draws.

use rand::Rng;

use super::World;
use crate::organism::{Infection, OrgId, Organism};
use crate::rng::{rng_for, stream, SimRng};

/// New-pathogen multiplier for matings between founding lineages.
pub const FOREIGN_PATHOGEN: f32 = 4.0;

impl World {
    /// The disease stream for organism `i` this tick; `salt` (1–3) separates
    /// the rules drawing on the same tick (0 is births' load mutations).
    fn disease_rng(&self, i: usize, salt: u64) -> SimRng {
        self.disease_rng_for(self.organisms[i].id, salt)
    }

    fn disease_rng_for(&self, id: OrgId, salt: u64) -> SimRng {
        rng_for(self.config.seed, stream::GENETICS, self.tick, (id << 2) | salt)
    }

    /// Infect a susceptible organism (no effect otherwise).
    pub(crate) fn infect(&mut self, i: usize) {
        let o = &mut self.organisms[i];
        if o.alive && o.infection == Infection::Susceptible {
            o.infection = Infection::Acute;
            *self.counters.infections.entry(o.lineage).or_default() += 1;
        }
    }

    /// Fertility multiplier from the mates' infections.
    pub(crate) fn disease_fertility(&self, a: usize, b: usize) -> f32 {
        let keep = 1.0 - self.config.balance.disease_fertility.clamp(0.0, 1.0);
        [a, b].iter().filter(|&&k| self.organisms[k].infection.infectious()).fold(1.0, |f, _| f * keep)
    }

    /// A mating's contact (natural or directed, conceived or not): a new
    /// pathogen may appear, and an infectious mate may infect the other.
    /// Draws nothing while neither can happen.
    pub(crate) fn mating_contact(&mut self, a: usize, b: usize) {
        let bal = self.config.balance;
        if bal.disease_emergence > 0.0 {
            let rng = &mut self.disease_rng(a, 1);
            let foreign = self.organisms[a].lineage != self.organisms[b].lineage;
            let p = bal.disease_emergence * if foreign { FOREIGN_PATHOGEN } else { 1.0 };
            if rng.gen::<f32>() < p {
                self.infect(if rng.gen::<bool>() { a } else { b });
                return;
            }
        }
        self.contact(a, b, bal.disease_transmission);
    }

    /// One contact between `a` and `b`: if exactly one is infectious and the
    /// other susceptible, transmission with chance `p`.
    pub(crate) fn contact(&mut self, a: usize, b: usize, p: f32) {
        let (ia, ib) = (self.organisms[a].infection, self.organisms[b].infection);
        let target = match (ia.infectious(), ib.infectious()) {
            (true, false) if ib == Infection::Susceptible => b,
            (false, true) if ia == Infection::Susceptible => a,
            _ => return,
        };
        if p > 0.0 && self.disease_rng(a, 2).gen::<f32>() < p {
            self.infect(target);
        }
    }

    /// The course of an acute infection: each day a chance to end it, in
    /// immunity or chronic carriage. (Its health drain is in `update_needs`.)
    #[cfg(test)]
    pub(crate) fn disease_course(&mut self, i: usize) {
        let mut orgs = std::mem::take(&mut self.organisms);
        self.disease_course_of(&mut orgs[i]);
        self.organisms = orgs;
    }

    /// `disease_course` for an organism held outside `organisms` (apply's
    /// settle part).
    pub(crate) fn disease_course_of(&self, o: &mut Organism) {
        if o.infection != Infection::Acute {
            return;
        }
        let bal = self.config.balance;
        let rng = &mut self.disease_rng_for(o.id, 3);
        if rng.gen::<f32>() < bal.disease_recovery * self.config.dt_days() {
            o.infection = if rng.gen::<f32>() < bal.disease_chronic { Infection::Chronic } else { Infection::Immune };
        }
    }

    /// Homozygous harmful recessives: a daily death hazard per affected locus.
    #[cfg(test)]
    pub(crate) fn recessive_death(&self, i: usize, rng: &mut impl Rng) -> bool {
        self.recessive_death_of(&self.organisms[i], rng)
    }

    pub(crate) fn recessive_death_of(&self, o: &Organism, rng: &mut impl Rng) -> bool {
        if o.recessive_load == 0 {
            return false;
        }
        let hazard = self.config.balance.recessive_hazard * o.recessive_load as f32;
        hazard > 0.0 && rng.gen::<f32>() < hazard * self.config.dt_days()
    }
}

#[cfg(test)]
mod tests {
    use crate::genome::Allele;
    use crate::organism::{DeathCause, Infection, Sex};
    use crate::presets;
    use crate::world::commands::DirectedMating;
    use crate::world::World;
    use crate::WorldConfig;

    fn world(seed: u64) -> World {
        presets::standard_world(WorldConfig { seed, width: 48, height: 48, ..Default::default() }, 1)
    }

    /// A ready female and male of `lineage`, side by side.
    fn pair(w: &mut World, lineage: u32) -> (usize, usize) {
        let f = w.organisms.iter().position(|o| o.lineage == lineage && o.sex == Sex::Female).unwrap();
        let m = w.organisms.iter().position(|o| o.lineage == lineage && o.sex == Sex::Male).unwrap();
        w.organisms[m].pos = w.organisms[f].pos;
        for k in [f, m] {
            let o = &mut w.organisms[k];
            (o.matured, o.growth, o.breed_ready_tick, o.pregnancy) = (true, 1.0, 0, None);
        }
        (f, m)
    }

    #[test]
    fn matings_pass_infection_between_mates() {
        let mut w = world(1);
        w.config.balance.disease_transmission = 1.0;
        w.config.balance.disease_emergence = 0.0;
        let (f, m) = pair(&mut w, 1);
        w.organisms[m].infection = Infection::Chronic;
        w.mating_contact(f, m);
        assert_eq!(w.organisms[f].infection, Infection::Acute);
        assert_eq!(w.counters.infections[&1], 1);
        // Immune organisms can't be reinfected; uninfected pairs stay clean.
        let (f2, m2) = pair(&mut w, 0);
        w.organisms[f2].infection = Infection::Immune;
        w.organisms[m2].infection = Infection::Acute;
        w.mating_contact(f2, m2);
        assert_eq!(w.organisms[f2].infection, Infection::Immune);
        let (f3, m3) = pair(&mut w, 2);
        w.mating_contact(f3, m3);
        assert!(!w.organisms[f3].infection.infectious() && !w.organisms[m3].infection.infectious());
    }

    #[test]
    fn directed_matings_transmit_and_infection_costs_fertility() {
        let mut w = world(2);
        w.config.balance.disease_transmission = 1.0;
        w.config.balance.disease_emergence = 0.0;
        let lineage = w.lineages.iter().position(|l| l.owner != crate::WILD).unwrap() as u32;
        let (f, m) = pair(&mut w, lineage);
        let clean = w.disease_fertility(f, m);
        w.organisms[m].infection = Infection::Chronic;
        let player = w.organisms[f].owner;
        let (a, b) = (w.organisms[f].id, w.organisms[m].id);
        let out = w.directed_mating(DirectedMating { player, a, b, partner_consent: true }).unwrap();
        assert_eq!(w.organisms[f].infection, Infection::Acute);
        assert_eq!(clean, 1.0);
        assert!(out.fertility <= 1.0 - w.config.balance.disease_fertility + 1e-6, "{}", out.fertility);
    }

    #[test]
    fn new_pathogens_appear_more_often_between_lineages() {
        let mut w = world(3);
        w.config.balance.disease_emergence = 0.05;
        w.config.balance.disease_transmission = 0.0;
        let count = |w: &mut World, a: usize, b: usize| {
            let mut n = 0;
            for _ in 0..2000 {
                w.organisms[a].infection = Infection::Susceptible;
                w.organisms[b].infection = Infection::Susceptible;
                w.mating_contact(a, b);
                w.tick += 1;
                n += w.organisms[a].infection.infectious() as u32 + w.organisms[b].infection.infectious() as u32;
            }
            n
        };
        let (f, m) = pair(&mut w, 1);
        let (_, m5) = pair(&mut w, 5);
        let within = count(&mut w, f, m);
        let across = count(&mut w, f, m5);
        assert!(within > 50 && across > 2 * within, "within {within}, across {across}");
    }

    #[test]
    fn acute_infection_drains_health_until_it_kills() {
        let mut w = world(4);
        w.config.balance.disease_recovery = 0.0;
        w.config.balance.disease_damage = 50.0;
        let i = w.organisms.iter().position(|o| o.lineage == 1).unwrap();
        w.infect(i);
        let id = w.organisms[i].id;
        let start = w.organisms[i].health;
        w.run_days(1);
        let o = w.organisms.iter().find(|o| o.id == id).unwrap();
        assert!(!o.alive || o.health < start - 20.0, "health {}", o.health);
        // Day by day: the dead are removed at the start of the next day.
        for _ in 0..2 {
            if !w.organisms.iter().any(|o| o.id == id && o.alive) {
                break;
            }
            w.run_days(1);
        }
        let o = w.organisms.iter().find(|o| o.id == id).unwrap();
        if o.death != Some(DeathCause::Predation) {
            assert_eq!(o.death, Some(DeathCause::Disease));
        }
    }

    #[test]
    fn acute_infections_end_in_immunity_or_chronic_carriage() {
        let mut w = world(5);
        w.config.balance.disease_recovery = 1e6;
        w.config.balance.disease_chronic = 0.5;
        let mut seen = [0; 2];
        for i in 0..w.organisms.len() {
            w.infect(i);
            w.disease_course(i);
            match w.organisms[i].infection {
                Infection::Immune => seen[0] += 1,
                Infection::Chronic => seen[1] += 1,
                other => panic!("{other:?}"),
            }
        }
        assert!(seen[0] > 10 && seen[1] > 10, "{seen:?}");
    }

    #[test]
    fn nursing_spreads_infection_to_young() {
        let mut w = world(6);
        w.config.balance.disease_kin = 1e6;
        let (mother, _) = pair(&mut w, 1);
        w.organisms[mother].infection = Infection::Chronic;
        let (pos, mid) = (w.organisms[mother].pos, w.organisms[mother].id);
        let g = w.organisms[mother].genome.clone();
        let mut rng = crate::rng::rng_for(6, 0, 0, 0);
        let young = w.spawn_newborn(g, w.organisms[mother].owner, 1, pos, 1, mid, 0, 0.2, &mut rng);
        let y = w.index[&young];
        w.organisms[y].needs.hunger = 50.0;
        w.nurse(y);
        assert_eq!(w.organisms[y].infection, Infection::Acute);
    }

    #[test]
    fn homozygous_recessives_die_young_and_carriers_do_not() {
        let mut w = world(7);
        w.config.balance.recessive_hazard = 1e6;
        let i = w.organisms.iter().position(|o| o.lineage == 1).unwrap();
        let (owner, pos) = (w.organisms[i].owner, w.organisms[i].pos);
        let mut carrier = crate::Genome::clone(&w.organisms[i].genome);
        carrier.set_load(0, false, Allele::harmful(9));
        let mut affected = carrier.clone();
        affected.set_load(0, true, Allele::harmful(9));
        let mut rng = crate::rng::rng_for(7, 0, 0, 0);
        let c = w.spawn_newborn(carrier, owner, 1, pos, 1, 0, 0, 0.2, &mut rng);
        let a = w.spawn_newborn(affected, owner, 1, pos, 1, 0, 0, 0.2, &mut rng);
        let (c, a) = (w.index[&c], w.index[&a]);
        assert_eq!((w.organisms[c].recessive_load, w.organisms[a].recessive_load), (0, 1));
        assert!(!w.recessive_death(c, &mut rng));
        assert!(w.recessive_death(a, &mut rng));
    }
}
