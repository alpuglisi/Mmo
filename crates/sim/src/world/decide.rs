//! Utility-AI action selection ("Action selection" in the design doc).
//!
//! Each organism scores its possible actions by the urgency of the related
//! need, weighted by its own behavior genes, and the highest score wins. This
//! phase only reads the world, so it runs in parallel.

use std::cell::Cell;

use rand::Rng;

use super::spatial::{ARMED, COURTING, FEMALE};
use super::{Intent, World};
use crate::genome::Gene;
use crate::group::GroupTrait;
use crate::mating;
use crate::math::Vec2;
use crate::organism::{Action, Organism, Sex};
use crate::rng::{rng_for, splitmix64, stream, SimRng};
use crate::terrain::{traverse, Traverse};

/// How close a mother must be to her young to defend it.
const GUARD_REACH: f32 = 2.5;

/// Neighbors an organism looks at closely each tick, nearest first. Beyond
/// these it looks only at possible threats, prey and mates that could beat
/// what it has already found, and estimates how many of the rest are kin.
/// Sets cost, not behavior: up to `FOCUS` neighbors everything is exact.
const FOCUS: usize = 32;

/// Chase multiplier for prey slower than the hunter (the best case).
const MAX_CHASE: f32 = 1.4;

thread_local! {
    /// Neighbor buffer reused across `decide` calls on a thread: squared
    /// distance and grid slot.
    static SCAN: Cell<Vec<(f32, u32)>> = const { Cell::new(Vec::new()) };
}

/// The deciding organism and what it brings to a close look at a neighbor.
struct Viewer<'a> {
    o: &'a Organism,
    radius: f32,
    alarm: f32,
    coop: f32,
    hunter: bool,
    can_breed: bool,
    my_power: f32,
    /// Seeds per-neighbor draws, so they don't depend on visit order.
    salt: u64,
}

/// What an organism has learned from the neighbors it looked at closely.
struct Perception {
    threat: f32,
    threat_pos: Vec2,
    threat_from: usize,
    prey: Option<(f32, usize)>,
    mate: Option<(f32, usize)>,
    kin: u32,
    kin_sum: Vec2,
    /// Of the nearest `FOCUS`, how many by [same lineage, other] and how
    /// many of those were kin.
    looked: [u32; 2],
    looked_kin: [u32; 2],
}

/// Does `(score, index)` beat the best so far? Ties go to the lower index, so
/// the choice never depends on visit order.
fn beats(score: f32, j: usize, best: Option<(f32, usize)>) -> bool {
    best.is_none_or(|(s, k)| score > s || (score == s && j < k))
}

/// Mean expression difference over all genes: `Genome::distance`, from the
/// phenotype (which holds exactly the expressed values) so it reads 38
/// contiguous floats instead of 76 scattered alleles.
fn genetic_distance(a: &Organism, b: &Organism) -> f32 {
    a.pheno.0.iter().zip(&b.pheno.0).map(|(x, y)| (x - y).abs()).sum::<f32>() / Gene::COUNT as f32
}

/// Grown and carnivorous: able to threaten something.
pub(crate) fn is_armed(o: &Organism) -> bool {
    o.traits.carnivory > 0.5 && o.growth > 0.5
}

/// Can `predator` threaten `prey` at all?
pub(crate) fn can_threaten(predator: &Organism, prey: &Organism) -> bool {
    is_armed(predator) && predator.mass() > 0.3 * prey.mass()
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

    /// Resistance and power a dependent juvenile's mother adds when she is
    /// within reach of it. Scaled by parental care, which falls steeply with
    /// litter size (doc: few offspring get long care, many die young).
    pub(crate) fn guardian(&self, young: &Organism) -> (f32, f32) {
        let k = self.config.balance.parent_defense;
        if k <= 0.0 || young.matured || young.growth >= 0.7 || young.traits.birth == crate::BirthType::AbandonedEggs {
            return (0.0, 0.0);
        }
        let Some(&mi) = young.mother.and_then(|m| self.index.get(&m)) else {
            return (0.0, 0.0);
        };
        let mother = &self.organisms[mi];
        if !mother.alive || mother.pos.dist(young.pos) > GUARD_REACH {
            return (0.0, 0.0);
        }
        let care = k * (1.0 - young.traits.litter_fraction()).powi(2);
        let rs = mother.resistance() * (1.0 + 1.5 * self.herd_bonus(mother));
        (care * rs, care * mother.power())
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

        let v = Viewer { o, radius, alarm, coop, hunter, can_breed, my_power, salt: rng.gen() };
        let mut seen = Perception {
            threat: 0.0,
            threat_pos: o.pos,
            threat_from: usize::MAX,
            prey: None,
            mate: None,
            kin: 0,
            kin_sum: Vec2::ZERO,
            looked: [0; 2],
            looked_kin: [0; 2],
        };
        // Neighbors beyond the nearest `FOCUS`, by [same lineage, other
        // lineage]: count and summed position.
        let mut rest = [(0u32, Vec2::ZERO); 2];
        SCAN.with(|buf| {
            let mut near = buf.take();
            near.clear();
            self.grid.query(o.pos, radius, |slot, e, d2| {
                if e.idx as usize != i {
                    near.push((d2, slot));
                }
            });
            if near.len() > FOCUS {
                // The nearest `FOCUS`, ties by organism index (litter-mates
                // share a spot), so the set never depends on grid order.
                let idx = |slot: u32| self.grid.entry(slot).idx;
                near.select_nth_unstable_by(FOCUS, |a, b| a.0.total_cmp(&b.0).then_with(|| idx(a.1).cmp(&idx(b.1))));
            }
            let focus = near.len().min(FOCUS);
            for &(d2, slot) in &near[..focus] {
                self.look(&v, &mut seen, self.grid.entry(slot).idx as usize, d2.sqrt(), true);
            }
            // Farther neighbors get a close look only if they could be the
            // biggest threat, the best prey or the best mate; each test is an
            // upper bound on what the look would score, so the outcome is
            // the same as looking at all of them. Their kin are estimated.
            let threat_floor = 0.3 * o.mass();
            let prey_cap = max_prey_mass(o, coop);
            let female = o.sex == Sex::Female;
            for &(d2, slot) in &near[focus..] {
                let e = self.grid.entry(slot);
                let d = d2.sqrt();
                let threat = e.flags & ARMED != 0 && e.mass > threat_floor;
                let prey = hunter && e.mass < prey_cap && seen.prey.is_none_or(|(s, _)| e.mass.powf(0.75) * MAX_CHASE >= s * (1.0 + d));
                let mate = can_breed
                    && e.flags & COURTING != 0
                    && (e.flags & FEMALE != 0) != female
                    && seen.mate.is_none_or(|(s, _)| 1.0 >= s * (1.0 + d));
                if threat || prey || mate {
                    self.look(&v, &mut seen, e.idx as usize, d, false);
                }
                let r = &mut rest[usize::from(e.lineage != o.lineage)];
                r.0 += 1;
                r.1 += e.pos;
            }
            buf.set(near);
        });
        let Perception { threat, threat_pos, prey, mate, .. } = seen;

        // Kin beyond the nearest `FOCUS`: at the kin rate among the nearest,
        // by lineage (with none of a kind among them, one's own lineage
        // counts as kin and others don't).
        let rate = |k: usize, unseen: f32| if seen.looked[k] > 0 { seen.looked_kin[k] as f32 / seen.looked[k] as f32 } else { unseen };
        let (own, other) = (rate(0, 1.0), rate(1, 0.0));
        let kin_f = seen.kin as f32 + own * rest[0].0 as f32 + other * rest[1].0 as f32;
        let kin = kin_f.round().min(u16::MAX as f32) as u16;
        let kin_sum = seen.kin_sum + rest[0].1 * own + rest[1].1 * other;
        let kin_centroid = if kin > 0 { Some(kin_sum * (1.0 / kin_f)) } else { None };
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
        // A burrower on dry ground escapes by going to ground, which costs it
        // nothing, so it takes cover at lower threat than a runner would flee.
        let can_dig = o.traits.body.burrow > 0.5 && self.map.tile_at(o.pos).water_depth == 0.0;
        let flee_weight = cfg.balance.flee_weight * if can_dig { cfg.balance.burrow_caution } else { 1.0 };

        let mut options: [(Action, f32); 12] = [
            (Action::Flee, threat * (1.3 - boldness) * flee_weight),
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
            // Going to ground: stay put, sheltered (see `update_needs`) and
            // unseen by hunters.
            Action::Flee if can_dig => {
                intent.action = Action::Rest;
            }
            Action::Flee => {
                let away = (o.pos - threat_pos).norm();
                let away = if away == Vec2::ZERO { Vec2::new(1.0, 0.0) } else { away };
                intent.target = Some(self.map.clamp(o.pos + away * 4.0));
                // Dependent young run to their mother, who defends them.
                if let Some(m) = mother_pos.filter(|&m| juvenile_care && o.pos.dist(m) < radius) {
                    intent.target = Some(self.map.clamp(m + away));
                }
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

    /// A close look at neighbor `j`, `d` tiles away: is it kin, a threat,
    /// prey, a mate? `count` adds it to the kin tally.
    fn look(&self, v: &Viewer, seen: &mut Perception, j: usize, d: f32, count: bool) {
        let (o, radius, alarm, coop) = (v.o, v.radius, v.alarm, v.coop);
        let cfg = &self.config;
        let q = &self.organisms[j];
        let gd = genetic_distance(o, q);
        let kin = mating::is_kin(&o.genome, &q.genome, gd, cfg.species_distance, cfg.kin_body_plan_gap);
        if count {
            let lineage = usize::from(q.lineage != o.lineage);
            seen.looked[lineage] += 1;
            if kin {
                seen.kin_sum += q.pos;
                seen.kin += 1;
                seen.looked_kin[lineage] += 1;
            }
        }
        if !kin {
            // Sheltered burrowers and stealthy organisms are harder to see.
            let hidden = if q.sheltered { 0.5 } else { 0.0 };
            let detect = radius * (1.0 - cfg.balance.stealth_effect * q.traits.stats.stealth - hidden).max(0.25 + 0.3 * alarm);
            if d < detect {
                if can_threaten(q, o) {
                    let odds = q.power() / (q.power() + o.resistance() + 1e-3);
                    let level = (1.0 - d / detect) * odds * 2.0;
                    if level > seen.threat || (level == seen.threat && level > 0.0 && j < seen.threat_from) {
                        seen.threat = level;
                        seen.threat_pos = q.pos;
                        seen.threat_from = j;
                    }
                }
                let reachable = || {
                    matches!(traverse(&o.traits.body, self.map.tile_at(q.pos), &self.weather, false), Traverse::Passable { .. })
                };
                if v.hunter && q.mass() < max_prey_mass(o, coop) && !q.sheltered && reachable() {
                    // Learned avoidance of bright prey where warning coloration holds.
                    let avoid = if q.pheno[Gene::Coloration] > 0.6 {
                        if o.bright_aversion {
                            1.0
                        } else {
                            self.group.benefit(GroupTrait::WarningColoration, q.pos)
                        }
                    } else {
                        0.0
                    };
                    // A lone hunter won't take on a guarding parent it can't beat.
                    let guard = self.guardian(q).0;
                    let my_power = v.my_power;
                    let roll = (splitmix64(v.salt ^ q.id) >> 40) as f32 / (1u64 << 24) as f32;
                    if roll >= avoid && (guard == 0.0 || my_power > q.resistance() + guard) {
                        let odds = my_power / (my_power + q.resistance() + guard + 1e-3);
                        let chase = if q.traits.stats.speed * q.activity < o.traits.stats.speed * o.activity { MAX_CHASE } else { 0.6 };
                        let score = q.mass().powf(0.75) / (1.0 + d) * odds * chase;
                        if beats(score, j, seen.prey) {
                            seen.prey = Some((score, j));
                        }
                    }
                }
            }
        }
        if v.can_breed && q.sex != o.sex && q.can_breed(self.tick) && gd < cfg.mate_distance && q.needs.breeding > 20.0 && mating::body_plan_fertility(&o.genome, &q.genome) > 0.0 {
            let m = mating::mismatch(&o.pheno, q.display());
            if m <= 1.0 {
                let score = 1.0 / ((1.0 + m) * (1.0 + d));
                if beats(score, j, seen.mate) {
                    seen.mate = Some((score, j));
                }
            }
        }
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
        // A target the body can't stand on is never reached, and would pin the
        // organism against the shore forever (starving next to the water).
        let usable = |t: Vec2| matches!(traverse(&o.traits.body, self.map.tile_at(t), &self.weather, false), Traverse::Passable { .. });
        if let Some(t) = o.wander_target {
            if o.pos.dist(t) > 0.5 && usable(t) {
                intent.target = Some(t);
                return;
            }
        }
        let mut t = o.pos;
        for _ in 0..4 {
            t = self.map.clamp(o.pos + Vec2::new(rng.gen_range(-range..range), rng.gen_range(-range..range)));
            if usable(t) {
                break;
            }
        }
        intent.target = Some(t);
        intent.new_wander = Some(t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{presets, terrain::Map, WorldConfig};

    #[test]
    fn wander_drops_a_target_the_body_cannot_reach() {
        let mut w = presets::standard_world(WorldConfig { seed: 9, width: 48, height: 48, ..Default::default() }, 1);
        let i = w.organisms.iter().position(|o| o.lineage == 4).unwrap();
        let blocked = |w: &World, p: Vec2| traverse(&w.organisms[i].traits.body, w.map.tile_at(p), &w.weather, false) == Traverse::Blocked;
        let water = (0..48 * 48).map(|k| Map::tile_center(k % 48, k / 48)).find(|&p| blocked(&w, p)).expect("map has deep water");
        w.organisms[i].wander_target = Some(water);
        let mut intent = Intent::idle();
        let mut rng = rng_for(1, stream::DECIDE, 0, 0);
        w.wander(&w.organisms[i], &mut intent, &mut rng, 6.0);
        assert_ne!(intent.target, Some(water));
        assert!(intent.new_wander.is_some());
    }

    /// A crowd far bigger than `FOCUS` neither hides a fox just outside it
    /// nor caps how many kin are counted.
    #[test]
    fn crowd_neither_hides_a_predator_nor_caps_kin() {
        let mut w = presets::standard_world(WorldConfig { seed: 4, width: 48, height: 48, ..Default::default() }, 1);
        let vole = w.organisms.iter().position(|o| o.lineage == 0 && o.matured).unwrap();
        let fox = w.organisms.iter().position(|o| o.lineage == 3 && o.growth > 0.5).unwrap();
        for o in w.organisms.iter_mut() {
            o.pos = Vec2::new(1.0, 1.0);
        }
        let center = Vec2::new(24.0, 24.0);
        let template = w.organisms[vole].clone();
        for k in 0..150u64 {
            let mut c = template.clone();
            c.id = 1_000_000 + k;
            c.pos = center + Vec2::new(1.8 * (k as f32 / 150.0).sqrt(), 0.0).rotate(2.4 * k as f32);
            w.organisms.push(c);
        }
        w.organisms[vole].pos = center;
        w.organisms[fox].pos = center + Vec2::new(2.0, 0.0);
        w.grid.rebuild(&w.organisms);
        let intent = w.decide(vole);
        assert!(intent.threat > 0.0, "fox unseen");
        assert_eq!(intent.kin_nearby, 150);
    }

    /// An antelope calf next to its mother borrows her resistance; away from
    /// her, or once grown, it is on its own. Big litters get little care.
    #[test]
    fn mother_guards_her_dependent_young() {
        let w = presets::standard_world(WorldConfig { seed: 3, width: 48, height: 48, ..Default::default() }, 1);
        let pick = |w: &World, lineage: u32| w.organisms.iter().position(|o| o.lineage == lineage).unwrap();
        let guard_of = |w: &World, mother: usize, at: Vec2, growth: f32| {
            let mut calf = w.organisms[mother].clone();
            calf.mother = Some(w.organisms[mother].id);
            calf.matured = false;
            calf.growth = growth;
            calf.pos = at;
            w.guardian(&calf).0
        };
        let m = pick(&w, 1);
        let at = w.organisms[m].pos;
        let near = guard_of(&w, m, at, 0.3);
        assert!(near > 0.2, "near {near}");
        assert_eq!(guard_of(&w, m, at + Vec2::new(GUARD_REACH + 1.0, 0.0), 0.3), 0.0);
        assert_eq!(guard_of(&w, m, at, 0.8), 0.0);
        let vole = pick(&w, 0);
        let vat = w.organisms[vole].pos;
        let vole_care = guard_of(&w, vole, vat, 0.3) / w.organisms[vole].resistance();
        assert!(vole_care < 0.5 * near / w.organisms[m].resistance(), "vole care {vole_care}");
    }
}
