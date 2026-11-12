//! Utility-AI action selection ("Action selection" in the design doc).
//!
//! Each organism scores its possible actions by the urgency of the related
//! need, weighted by its own behavior genes, and the highest score wins. This
//! phase only reads the world, so it runs in parallel.

use rand::Rng;

use super::{Intent, World};
use crate::genome::Gene;
use crate::group::GroupTrait;
use crate::mating;
use crate::math::Vec2;
use crate::organism::{Action, Organism};
use crate::rng::{rng_for, stream, SimRng};
use crate::terrain::{traverse, Traverse};

/// Can `predator` threaten `prey` at all?
pub(crate) fn can_threaten(predator: &Organism, prey: &Organism) -> bool {
    predator.traits.carnivory > 0.5 && predator.growth > 0.5 && predator.mass() > 0.3 * prey.mass()
}

/// Largest prey a hunter will take, with cooperative hunting.
pub(crate) fn max_prey_mass(hunter: &Organism, coop: f32) -> f32 {
    hunter.mass() * (1.5 + 3.0 * coop)
}

impl World {
    pub(crate) fn coop_bonus(&self, o: &Organism) -> f32 {
        if GroupTrait::CooperativeHunting.is_carrier(&o.pheno) {
            self.group.benefit(GroupTrait::CooperativeHunting, o.pos)
        } else {
            0.0
        }
    }

    pub(crate) fn herd_bonus(&self, o: &Organism) -> f32 {
        if GroupTrait::HerdDefense.is_carrier(&o.pheno) && o.kin_nearby >= 3 {
            self.group.benefit(GroupTrait::HerdDefense, o.pos)
        } else {
            0.0
        }
    }

    pub(crate) fn decide(&self, i: usize) -> Intent {
        let o = &self.organisms[i];
        if !o.alive {
            return Intent::idle();
        }
        let mut rng = rng_for(self.config.seed, stream::DECIDE, self.tick, o.id);
        let p = &o.pheno;
        let n = &o.needs;
        let cfg = &self.config;

        // --- Perception ---------------------------------------------------
        let alarm = if GroupTrait::AlarmCalls.is_carrier(p) { self.group.benefit(GroupTrait::AlarmCalls, o.pos) } else { 0.0 };
        let possessed = if o.possessed_by.is_some() { 1.2 } else { 1.0 };
        // Breeding drive lowers perception (doc: "Breeding season").
        let perception = o.traits.stats.perception * (1.0 + 0.5 * alarm) * possessed * (1.0 - 0.3 * n.breeding / 100.0);
        let radius = (2.0 + cfg.balance.perception_range * perception).min(cfg.max_perception);

        let coop = self.coop_bonus(o);
        let hunter = o.traits.carnivory > 0.45 && o.growth > 0.6;
        let can_breed = o.can_breed(self.tick);
        let my_power = o.power() * (1.0 + coop) * cfg.balance.attack_power;

        let mut threat: f32 = 0.0;
        let mut threat_pos = o.pos;
        let mut prey: Option<(f32, usize)> = None;
        let mut mate: Option<(f32, usize)> = None;
        let mut kin_sum = Vec2::ZERO;
        let mut kin = 0u16;
        let mut seen = 0;

        self.grid.query(o.pos, radius, |j| {
            if j == i || seen > 48 {
                return;
            }
            let q = &self.organisms[j];
            let d = o.pos.dist(q.pos);
            if d > radius {
                return;
            }
            seen += 1;
            let gd = o.genome.distance(&q.genome);
            if gd < cfg.species_distance {
                kin_sum += q.pos;
                kin += 1;
            } else {
                // Sheltered burrowers and stealthy organisms are harder to see.
                let hidden = if q.sheltered { 0.5 } else { 0.0 };
                let detect = radius * (1.0 - cfg.balance.stealth_effect * q.traits.stats.stealth - hidden).max(0.25 + 0.3 * alarm);
                if d < detect {
                    if can_threaten(q, o) {
                        let odds = q.power() / (q.power() + o.resistance() + 1e-3);
                        let level = (1.0 - d / detect) * odds * 2.0;
                        if level > threat {
                            threat = level;
                            threat_pos = q.pos;
                        }
                    }
                    let reachable = || {
                        matches!(traverse(&o.traits.body, self.map.tile_at(q.pos), &self.weather, false), Traverse::Passable { .. })
                    };
                    if hunter && q.mass() < max_prey_mass(o, coop) && !q.sheltered && reachable() {
                        // Learned avoidance of bright prey where warning coloration holds.
                        let avoid = if q.pheno[Gene::Coloration] > 0.6 {
                            self.group.benefit(GroupTrait::WarningColoration, q.pos)
                        } else {
                            0.0
                        };
                        if rng.gen::<f32>() >= avoid {
                            let odds = my_power / (my_power + q.resistance() + 1e-3);
                            let chase = if q.traits.stats.speed * q.activity < o.traits.stats.speed * o.activity { 1.4 } else { 0.6 };
                            let score = q.mass().powf(0.75) / (1.0 + d) * odds * chase;
                            if prey.is_none_or(|(s, _)| score > s) {
                                prey = Some((score, j));
                            }
                        }
                    }
                }
            }
            if can_breed && q.sex != o.sex && q.can_breed(self.tick) && gd < cfg.mate_distance && q.needs.breeding > 20.0 {
                let m = mating::mismatch(&o.pheno, q.display());
                if m <= 1.0 {
                    let score = 1.0 / ((1.0 + m) * (1.0 + d));
                    if mate.is_none_or(|(s, _)| score > s) {
                        mate = Some((score, j));
                    }
                }
            }
        });

        let kin_centroid = if kin > 0 { Some(kin_sum * (1.0 / kin as f32)) } else { None };
        let mut intent = Intent { kin_nearby: kin, threat, ..Intent::idle() };

        // --- Player overrides ----------------------------------------------
        if let (Some(_), Some(target)) = (o.possessed_by, o.order) {
            intent.action = Action::Ordered;
            intent.target = Some(target);
            intent.sprint = o.needs.energy > 40.0;
            return intent;
        }

        // --- Utilities -----------------------------------------------------
        let hunger = n.hunger / 100.0;
        let thirst = n.thirst / 100.0;
        let tired = 1.0 - n.energy / 100.0;
        let boldness = p[Gene::Boldness];
        let mother_pos = self.parent_pos(o);
        let juvenile_care = !o.matured && o.growth < 0.7 && o.traits.birth != crate::BirthType::AbandonedEggs;

        let mut options: [(Action, f32); 12] = [
            (Action::Flee, threat * (1.3 - boldness) * cfg.balance.flee_weight),
            (Action::Graze, hunger.powf(1.5) * 1.2 + if hunger > 0.8 { 0.4 } else { 0.0 }),
            (Action::Drink, thirst.powf(1.5) * 1.3 + if thirst > 0.8 { 0.5 } else { 0.0 }),
            (Action::Rest, tired * tired * 1.1),
            (Action::Bask, if o.traits.warm { 0.0 } else { (1.0 - o.activity) * 0.7 }),
            (Action::SeekMate, if can_breed { n.breeding / 100.0 * 0.9 * (1.0 - 0.7 * hunger) } else { 0.0 }),
            (Action::Group, match kin_centroid {
                // Comfortable spacing grows with group size, so herds spread to graze.
                Some(c) if o.pos.dist(c) > 1.5 + 4.0 * (1.0 - p[Gene::GroupTolerance]) + 0.8 * (kin as f32).sqrt() => {
                    (p[Gene::GroupTolerance] * 0.25 + n.social / 100.0 * 0.3) * (1.0 - hunger)
                }
                _ => 0.0,
            }),
            (Action::Wander, 0.05 + 0.15 * p[Gene::Wanderlust]),
            (Action::Migrate, if self.migrate_orders.contains_key(&o.owner) && o.matured { 0.55 } else { 0.0 }),
            (Action::FollowParent, match mother_pos {
                Some(m) if juvenile_care && o.pos.dist(m) > 1.2 => 0.6,
                _ => 0.0,
            }),
            (Action::Guard, if o.nest.is_some() { 0.6 } else { 0.0 }),
            (Action::Idle, 0.0),
        ];
        // Aggressive organisms hunt a little before they're hungry.
        if hunter && prey.is_some() {
            options[1].1 += 0.15 * p[Gene::Aggression];
        }
        let (action, _) = options
            .iter()
            .copied()
            .fold((Action::Idle, f32::MIN), |best, opt| if opt.1 > best.1 { opt } else { best });

        // An exhausted hunter rests before chasing again; otherwise hunger keeps
        // it chasing at a crawl until it starves.
        let meat_here = self.map.tile_at(o.pos).meat * o.traits.carnivory > 1.0;
        let action = if action == Action::Graze && o.traits.carnivory > 0.6 && n.energy < 30.0 && !meat_here && threat < 0.3 {
            Action::Rest
        } else {
            action
        };

        // --- Targets -------------------------------------------------------
        intent.action = action;
        match action {
            Action::Flee => {
                let away = (o.pos - threat_pos).norm();
                let away = if away == Vec2::ZERO { Vec2::new(1.0, 0.0) } else { away };
                intent.target = Some(self.map.clamp(o.pos + away * 4.0));
                intent.sprint = true;
            }
            Action::Graze => self.choose_food(o, prey, &mut intent, &mut rng),
            Action::Drink => {
                if self.map.can_drink_at(o.pos) {
                    intent.target = None;
                } else if let Some(w) = self.map.find_water(o.pos, 3) {
                    intent.target = Some(w);
                } else if let Some(w) = self.map.toward_water(o.pos) {
                    intent.target = Some(w);
                } else if let Some(w) = o.last_water {
                    intent.target = Some(w);
                } else {
                    self.wander(o, &mut intent, &mut rng, 8.0);
                }
            }
            Action::Rest | Action::Bask | Action::Idle => {
                // Cold-blooded organisms bask in the open.
                if action == Action::Bask && !self.map.tile_at(o.pos).is_open() {
                    self.wander(o, &mut intent, &mut rng, 3.0);
                    intent.action = Action::Bask;
                }
            }
            Action::SeekMate => {
                if let Some((_, j)) = mate {
                    intent.target = Some(self.organisms[j].pos);
                    intent.target_org = Some(j);
                } else if let Some(c) = kin_centroid {
                    intent.target = Some(c);
                } else {
                    self.wander(o, &mut intent, &mut rng, 6.0);
                }
            }
            Action::Group => intent.target = kin_centroid,
            Action::Wander => self.wander(o, &mut intent, &mut rng, 2.0 + 8.0 * p[Gene::Wanderlust]),
            Action::Migrate => intent.target = self.migrate_orders.get(&o.owner).copied(),
            Action::FollowParent => intent.target = mother_pos,
            Action::Guard => intent.target = o.nest,
            Action::Hunt | Action::Scavenge | Action::Ordered => {}
        }
        intent
    }

    fn parent_pos(&self, o: &Organism) -> Option<Vec2> {
        let m = self.index.get(&o.mother?)?;
        let mother = &self.organisms[*m];
        mother.alive.then_some(mother.pos)
    }

    /// Hunger: graze, hunt or scavenge depending on diet and what's around.
    fn choose_food(&self, o: &Organism, prey: Option<(f32, usize)>, intent: &mut Intent, rng: &mut SimRng) {
        let c = o.traits.carnivory;
        let plant_eff = 1.0 - c;
        let here = self.map.tile_at(o.pos);

        // Value of each option in rough "food I can get" terms.
        let hunt_value = prey.map(|(s, _)| s * 4.0 * c).unwrap_or(0.0);
        let meat_here = here.meat * c;
        let graze_here = here.veg * plant_eff;

        if meat_here > 1.0 && meat_here >= graze_here {
            intent.action = Action::Scavenge;
            return;
        }
        if let Some((_, j)) = prey {
            if hunt_value > graze_here * 0.05 && c > 0.45 {
                let q = &self.organisms[j];
                intent.action = Action::Hunt;
                intent.target = Some(q.pos);
                intent.target_org = Some(j);
                intent.sprint = o.pos.dist(q.pos) < 5.0 && o.needs.energy > 25.0;
                return;
            }
        }
        if plant_eff < 0.3 {
            // Carnivore with nothing in sight: look for carcasses, else roam.
            if let Some(t) = self.find_tile(o, rng, 1.0, |t| t.meat * c) {
                intent.action = Action::Scavenge;
                intent.target = Some(t);
            } else {
                self.wander(o, intent, rng, 6.0);
                intent.action = Action::Hunt;
            }
            return;
        }
        // A tile is worth eating from if it holds at least half a bite.
        let bite = 25.0 * o.food_per_hunger();
        if graze_here > 0.5 * bite {
            intent.target = None;
            return;
        }
        // Keep heading for the patch chosen earlier while it still has food.
        if let Some(t) = o.forage_target {
            let tile = self.map.tile_at(t);
            if tile.veg * plant_eff + tile.meat * c >= bite {
                intent.target = Some(t);
                return;
            }
        }
        match self.find_tile(o, rng, 2.0 * bite, |t| t.veg * plant_eff + t.meat * c) {
            Some(t) => {
                intent.target = Some(t);
                intent.new_forage = Some(Some(t));
            }
            None => {
                intent.new_forage = Some(None);
                self.wander(o, intent, rng, 5.0);
            }
        }
    }

    /// Sample nearby tiles and return the best by `value`, discounted by
    /// distance. Foraging range sets how far the organism looks.
    fn find_tile(
        &self,
        o: &Organism,
        rng: &mut SimRng,
        min_value: f32,
        value: impl Fn(&crate::terrain::Tile) -> f32,
    ) -> Option<Vec2> {
        let near = 2.0 + 6.0 * o.pheno[Gene::ForagingRange];
        let mut best: Option<(f32, Vec2)> = None;
        // Look close first; if the area is stripped, look much further.
        for k in 0..16 {
            if k == 8 && best.is_some() {
                break;
            }
            let r = if k < 8 { near } else { near * 3.0 };
            let cand = self.map.clamp(o.pos + Vec2::new(rng.gen_range(-r..r), rng.gen_range(-r..r)));
            let t = self.map.tile_at(cand);
            if !matches!(traverse(&o.traits.body, t, &self.weather, false), Traverse::Passable { .. }) {
                continue;
            }
            let v = value(t);
            let score = v / (1.0 + 0.3 * o.pos.dist(cand));
            if v >= min_value && best.is_none_or(|(s, _)| score > s) {
                best = Some((score, cand));
            }
        }
        best.map(|(_, p)| p)
    }

    fn wander(&self, o: &Organism, intent: &mut Intent, rng: &mut SimRng, range: f32) {
        if let Some(t) = o.wander_target {
            if o.pos.dist(t) > 0.5 {
                intent.target = Some(t);
                return;
            }
        }
        let t = self.map.clamp(o.pos + Vec2::new(rng.gen_range(-range..range), rng.gen_range(-range..range)));
        intent.target = Some(t);
        intent.new_wander = Some(t);
    }
}
