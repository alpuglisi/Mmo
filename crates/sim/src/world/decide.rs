//! Utility-AI action selection ("Action selection" in the design doc).
//!
//! Each organism scores its possible actions by the urgency of the related
//! need, weighted by its own behavior genes, and the highest score wins. This
//! phase only reads the world, so it runs in parallel.

use std::cell::Cell;

use rand::Rng;

use super::spatial::{ALERT, ARMED, COURTING, FEMALE};
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
    genes: Genes,
    radius: f32,
    alarm: f32,
    /// Sentry benefit where the viewer stands, if it is a group member:
    /// how much a kin sentry's warning counts.
    sentry: f32,
    coop: f32,
    hunter: bool,
    can_breed: bool,
    my_power: f32,
    /// Striking power without the pack bonus: whoever grabs prey that
    /// fights back takes the bite alone.
    fight_power: f32,
    /// Lowest injury discount the viewer accepts (`1 - hunger * carnivory`).
    risk_floor: f32,
    /// Mass above which an armed neighbor can threaten the viewer
    /// (`can_threaten`).
    threat_floor: f32,
    /// Mass below which the viewer would take prey (`max_prey_mass`).
    prey_cap: f32,
    female: bool,
    /// Seeds per-neighbor draws, so they don't depend on visit order.
    salt: u64,
}

/// What an organism has learned from the neighbors it looked at closely.
struct Perception {
    threat: f32,
    threat_pos: Vec2,
    threat_from: usize,
    /// Strongest warning heard from a kin sentry, and where its threat is.
    /// Kept apart from `threat` so sentries never relay each other's
    /// warnings in a loop.
    warning: f32,
    warning_pos: Vec2,
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

/// Mean expression difference over the distance genes: `Genome::distance`, from the
/// phenotype (which holds exactly the expressed values) so it reads `Gene::COUNT`
/// contiguous floats instead of 76 scattered alleles.
fn genetic_distance(a: &Organism, b: &Organism) -> f32 {
    use crate::genome::{DISTANCE_GENES, IN_DISTANCE};
    let diffs = a.pheno.0.iter().zip(&b.pheno.0).zip(IN_DISTANCE);
    diffs.map(|((x, y), counts)| if counts { (x - y).abs() } else { 0.0 }).sum::<f32>() / DISTANCE_GENES
}

/// Slots in [`Genes`]: the distance genes, padded to whole groups of four.
pub(super) const GENE_SLOTS: usize = (crate::genome::DISTANCE_GENES as usize).div_ceil(4) * 4;

/// The expressed values of the genes that count toward [`genetic_distance`],
/// in gene order, then zeros: what kinship tests read of an organism, packed
/// next to its neighbors' in the spatial grid.
pub(super) type Genes = [f32; GENE_SLOTS];

/// Gene index of each slot of [`Genes`] (padding slots repeat nothing).
const GENE_OF_SLOT: [usize; crate::genome::DISTANCE_GENES as usize] = {
    let mut out = [0; crate::genome::DISTANCE_GENES as usize];
    let (mut k, mut n) = (0, 0);
    while k < Gene::COUNT {
        if crate::genome::IN_DISTANCE[k] {
            out[n] = k;
            n += 1;
        }
        k += 1;
    }
    out
};

/// Slot in [`Genes`] of each of `mating::BODY_PLAN_GENES`, in that order.
const PLAN_SLOTS: [usize; mating::BODY_PLAN_GENES.len()] = {
    let mut out = [usize::MAX; mating::BODY_PLAN_GENES.len()];
    let mut i = 0;
    while i < out.len() {
        let mut n = 0;
        while n < GENE_OF_SLOT.len() {
            if GENE_OF_SLOT[n] == mating::BODY_PLAN_GENES[i] as usize {
                out[i] = n;
            }
            n += 1;
        }
        assert!(out[i] != usize::MAX, "body-plan genes count toward the distance");
        i += 1;
    }
    out
};

/// [`Genes`] of a phenotype.
pub(super) fn genes_of(p: &crate::genome::Phenotype) -> Genes {
    let mut out = [0.0; GENE_SLOTS];
    for (slot, &k) in out.iter_mut().zip(&GENE_OF_SLOT) {
        *slot = p.0[k];
    }
    out
}

/// `mating::body_plan_gap` from [`Genes`] (the same value, bit for bit: the
/// same differences, folded in the same order).
fn body_plan_gap(a: &Genes, b: &Genes) -> f32 {
    PLAN_SLOTS.iter().map(|&k| (a[k] - b[k]).abs()).fold(0.0, f32::max)
}

/// Bounds on [`genetic_distance`] from a quick sum that adds the same terms
/// in another order (four running sums, which vectorize, instead of one
/// chain). Reordering a sum of `Gene::COUNT` non-negative terms moves it by
/// less than `Gene::COUNT * f32::EPSILON` of itself, a few millionths;
/// `SLACK` is far wider. Every test made of the distance is monotone in it,
/// so if it comes out the same at both bounds it is the test's exact result.
struct DistanceBounds {
    lo: f32,
    hi: f32,
}

impl DistanceBounds {
    const SLACK: f32 = 1e-4;

    fn new(a: &Genes, b: &Genes) -> Self {
        use crate::genome::DISTANCE_GENES;
        let mut lanes = [0.0f32; 4];
        for (a, b) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            for (l, lane) in lanes.iter_mut().enumerate() {
                *lane += (a[l] - b[l]).abs();
            }
        }
        let sum = (lanes[0] + lanes[1]) + (lanes[2] + lanes[3]);
        DistanceBounds { lo: sum * (1.0 - Self::SLACK) / DISTANCE_GENES, hi: sum * (1.0 + Self::SLACK) / DISTANCE_GENES }
    }

    /// `pass(genetic_distance(a, b))`, for a `pass` that can only go from
    /// true to false as the distance grows: decided from the bounds when
    /// they agree, from the exact distance (rarely needed) when they don't.
    fn test(&self, a: &Organism, b: &Organism, pass: impl Fn(f32) -> bool) -> bool {
        if pass(self.hi) {
            true
        } else if !pass(self.lo) {
            false
        } else {
            pass(genetic_distance(a, b))
        }
    }
}

/// Carnivory above which an organism hunts live prey (`choose_food`).
pub(crate) const HUNTER_CARNIVORY: f32 = 0.45;

/// Grown and carnivorous enough to hunt: able to threaten something. The
/// grid's `ARMED` flag prunes far threats with this, so it must stay in step
/// with `can_threaten`.
pub(crate) fn is_armed(o: &Organism) -> bool {
    o.traits.carnivory > HUNTER_CARNIVORY && o.growth > 0.5 && !o.dormancy.is_deep()
}

/// Can `predator` threaten `prey` at all? Anything that would hunt the
/// prey counts, omnivores included.
pub(crate) fn can_threaten(predator: &Organism, prey: &Organism) -> bool {
    is_armed(predator) && predator.mass() > 0.3 * prey.mass()
}

/// How hard `o` fights back when attacked: a meat eater turns on its
/// attacker with fangs and claws, a plant eater mostly tries to get away.
/// Follows the taxed diet (`Traits::meat_efficiency` with
/// `Balance::generalist_tax`): a generalist is a half-hearted fighter.
pub(crate) fn fight_back(o: &Organism, tax: f32) -> f32 {
    o.power() * o.traits.meat_efficiency(tax)
}

/// Factor on a hunter's prey score for the risk of being hurt by prey that
/// fights back: `(1 - risk)^(injury_aversion * (2 - carnivory))`, where
/// `risk` is the prey's share of the two fighters' power. Omnivores, which
/// can eat plants instead, weigh the risk more than strict carnivores.
/// Never above 1, so the far-neighbor prey bound in `decide` still holds.
pub(crate) fn injury_discount(hunter: &Organism, own_power: f32, danger: f32, aversion: f32) -> f32 {
    if danger <= 0.0 || aversion <= 0.0 {
        return 1.0;
    }
    let risk = danger / (own_power + danger + 1e-3);
    (1.0 - risk).powf(aversion * (2.0 - hunter.traits.carnivory))
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

    /// Shared-burrow benefit for `o`: a carrier where the trait holds, or
    /// dependent young of a carrier mother (raised in her burrow).
    pub(crate) fn shared_burrow_bonus(&self, o: &Organism) -> f32 {
        if self.config.balance.shared_burrow <= 0.0 {
            return 0.0;
        }
        let t = GroupTrait::SharedBurrows;
        let carrier = t.is_carrier(&o.pheno)
            || (!o.matured
                && o.growth < 0.7
                && o.mother.and_then(|m| self.index.get(&m)).is_some_and(|&mi| t.is_carrier(&self.organisms[mi].pheno)));
        if carrier {
            self.group.benefit(t, o.pos)
        } else {
            0.0
        }
    }

    /// Effective burrowing: the organism's own, deepened by a shared burrow
    /// (`Balance::shared_burrow`). Above 0.5 it can shelter on dry ground.
    pub(crate) fn burrow_depth(&self, o: &Organism) -> f32 {
        let b = o.traits.body.burrow;
        let shared = self.shared_burrow_bonus(o);
        if shared <= 0.0 {
            return b;
        }
        b + self.config.balance.shared_burrow * shared * (1.0 - b).max(0.0)
    }

    /// Sentry benefit for a group member (`GroupTrait::Sentries` eligible).
    pub(crate) fn sentry_bonus(&self, o: &Organism) -> f32 {
        if self.config.balance.sentry_share > 0.0 && GroupTrait::Sentries.is_eligible(&o.pheno) {
            self.group.benefit(GroupTrait::Sentries, o.pos)
        } else {
            0.0
        }
    }

    /// Is sentry carrier `o` on duty this tick? Shifts last two hours; each
    /// carrier draws its own (seeded by id and shift), a `sentry_share` of
    /// them on average.
    pub(crate) fn on_watch_duty(&self, o: &Organism) -> bool {
        let share = self.config.balance.sentry_share;
        if share <= 0.0 || !o.matured || !GroupTrait::Sentries.is_carrier(&o.pheno) {
            return false;
        }
        let shift = self.tick / (2 * self.config.ticks_per_hour());
        let roll = (splitmix64(o.id ^ splitmix64(shift ^ 0x5e47_e1e5)) >> 40) as f32 / (1u64 << 24) as f32;
        roll < share
    }

    pub(crate) fn herd_bonus(&self, o: &Organism) -> f32 {
        if GroupTrait::HerdDefense.is_carrier(&o.pheno) && o.kin_nearby >= 3 {
            self.group.benefit(GroupTrait::HerdDefense, o.pos)
        } else {
            0.0
        }
    }

    /// Resistance and power a juvenile's guardians add when a hunter strikes
    /// at it: its mother, if it is still dependent (growth < 0.7) and she is
    /// within reach, or else (whichever is more) the herd it keeps with
    /// (`herd_guard`). Scaled by parental care, which falls steeply with
    /// litter size (doc: few offspring get long care, many die young).
    pub(crate) fn guardian(&self, young: &Organism) -> (f32, f32) {
        let k = self.config.balance.parent_defense;
        if k <= 0.0 || young.matured || young.traits.birth == crate::BirthType::AbandonedEggs {
            return (0.0, 0.0);
        }
        let care = k * (1.0 - young.traits.litter_fraction()).powi(2);
        let mut guard = (0.0, 0.0);
        if young.growth < 0.7 {
            if let Some(&mi) = young.mother.and_then(|m| self.index.get(&m)) {
                let mother = &self.organisms[mi];
                if mother.alive && mother.pos.dist(young.pos) <= GUARD_REACH {
                    let rs = mother.resistance() * (1.0 + 1.5 * self.herd_bonus(mother));
                    guard = (care * rs, care * mother.power());
                }
            }
        }
        let (rs, power) = self.herd_guard(young, care);
        (guard.0.max(rs), guard.1.max(power))
    }

    /// What the herd adds to a young member's defense (`herd_guard`): where
    /// herd defense holds and kin are around, the adults close ranks around
    /// the calves, sub-adults and orphans among them, as a parent would
    /// (`care`), with the resistance and power of a grown member of its kind
    /// and the herd's own bonus. Predators can't single out the young either.
    fn herd_guard(&self, young: &Organism, care: f32) -> (f32, f32) {
        let share = self.config.balance.herd_guard;
        let herd = self.herd_bonus(young);
        if share <= 0.0 || herd <= 0.0 {
            return (0.0, 0.0);
        }
        let st = &young.traits.stats;
        let adult_rs = st.defense + 0.3 * st.agility;
        let adult_power = st.strength + 0.3 * st.agility;
        let k = share * care * herd;
        (k * adult_rs * (1.0 + 1.5 * herd), k * adult_power)
    }

    pub(crate) fn decide(&self, i: usize) -> Intent {
        let o = &self.organisms[i];
        if !o.alive {
            return Intent::idle();
        }
        // Hibernating, brumating or estivating: no perception, no choices.
        if o.dormancy.is_deep() {
            return Intent { action: Action::Dormant, kin_nearby: o.kin_nearby, ..Intent::idle() };
        }
        let mut rng = rng_for(self.config.seed, stream::DECIDE, self.tick, o.id);
        let p = &o.pheno;
        let n = &o.needs;
        let cfg = &self.config;

        // --- Perception ---------------------------------------------------
        let alarm = if GroupTrait::AlarmCalls.is_carrier(p) { self.group.benefit(GroupTrait::AlarmCalls, o.pos) } else { 0.0 };
        let possessed = if o.possessed_by.is_some() { 1.2 } else { 1.0 };
        // Group members where sentries hold breed under watch; a sentry on
        // watch scans harder and isn't distracted by breeding.
        let sentry = self.sentry_bonus(o);
        let watching = o.action == Action::Sentry;
        let vigil = if watching { 1.0 + cfg.balance.sentry_vigilance } else { 1.0 };
        // Breeding drive lowers perception (doc: "Breeding season").
        let distraction = if watching { 0.0 } else { 0.3 * n.breeding / 100.0 * (1.0 - sentry) };
        // ... and threat response: an animal in high breeding drive flees
        // later (`breeding_recklessness`). A short timed-breeding window
        // keeps the reckless period short; sentries offset it as well.
        let reckless = if watching { 0.0 } else { cfg.balance.breeding_recklessness * n.breeding / 100.0 * (1.0 - sentry) };
        let perception = o.traits.stats.perception * (1.0 + 0.5 * alarm) * possessed * vigil * (1.0 - distraction);
        let radius = (2.0 + cfg.balance.perception_range * perception).min(cfg.max_perception);

        let coop = self.coop_bonus(o);
        let hunter = o.traits.carnivory > HUNTER_CARNIVORY && o.growth > 0.6;
        let can_breed = o.can_breed(self.tick) && self.may_conceive(o);
        let my_power = o.power() * (1.0 + coop) * cfg.balance.attack_power;

        let fight_power = o.power() * cfg.balance.attack_power;
        let risk_floor = 1.0 - n.hunger / 100.0 * o.traits.carnivory;
        let (threat_floor, prey_cap, female) = (0.3 * o.mass(), max_prey_mass(o, coop), o.sex == Sex::Female);
        let v = Viewer {
            o,
            genes: genes_of(p),
            radius,
            alarm,
            sentry,
            coop,
            hunter,
            can_breed,
            my_power,
            fight_power,
            risk_floor,
            threat_floor,
            prey_cap,
            female,
            salt: rng.gen(),
        };
        let mut seen = Perception {
            threat: 0.0,
            threat_pos: o.pos,
            threat_from: usize::MAX,
            warning: 0.0,
            warning_pos: o.pos,
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
            self.grid.query(o.pos, radius, |slot, d2| {
                if self.grid.entry(slot).idx as usize != i {
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
                self.look(&v, &mut seen, slot, d2.sqrt(), true);
            }
            // Farther neighbors get a close look only if they could be the
            // biggest threat, the best prey or the best mate; each test is an
            // upper bound on what the look would score, so the outcome is
            // the same as looking at all of them. Their kin are estimated.
            for &(d2, slot) in &near[focus..] {
                let e = self.grid.entry(slot);
                let d = d2.sqrt();
                let threat = e.flags & ARMED != 0 && e.mass > v.threat_floor;
                let prey = hunter && e.mass < v.prey_cap && seen.prey.is_none_or(|(s, _)| e.mass.powf(0.75) * MAX_CHASE >= s * (1.0 + d));
                let mate = can_breed
                    && e.flags & COURTING != 0
                    && (e.flags & FEMALE != 0) != v.female
                    && seen.mate.is_none_or(|(s, _)| 1.0 >= s * (1.0 + d));
                let warning = sentry > 0.0 && e.flags & ALERT != 0;
                if threat || prey || mate || warning {
                    self.look(&v, &mut seen, slot, d, false);
                }
                let r = &mut rest[usize::from(e.lineage != o.lineage)];
                r.0 += 1;
                r.1 += e.pos;
            }
            buf.set(near);
        });
        let Perception { threat: seen_threat, threat_pos: seen_pos, prey, mate, .. } = seen;
        // A sentry's warning counts like a threat seen first hand, from
        // wherever the sentry saw it.
        let (threat, threat_pos) = if seen.warning > seen_threat { (seen.warning, seen.warning_pos) } else { (seen_threat, seen_pos) };

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
        // Torpid on a cold night: only a close threat rouses it.
        if o.dormancy == crate::organism::Dormancy::Torpor && threat < super::seasonal::TORPOR_ALERT {
            intent.action = Action::Dormant;
            return intent;
        }

        // --- Player overrides ----------------------------------------------
        // A possessed organism follows its move order unless danger or a
        // severe need overrides it (then it does what that need demands, and
        // goes back to the order afterwards). Once there, it is on its own
        // again (`World::settle_orders` drops the order).
        let mut urgent = None;
        if let (Some(_), Some(target)) = (o.possessed_by, o.order) {
            urgent = self.order_override(o, threat);
            if urgent.is_none() && o.pos.dist(target) > super::commands::ORDER_ARRIVAL {
                intent.action = Action::Ordered;
                intent.target = Some(target);
                intent.sprint = o.needs.energy > 40.0;
                return intent;
            }
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
        // Carriers of shared burrows can go to ground with modest burrowing.
        let can_dig = self.map.tile_at(o.pos).water_depth == 0.0 && self.burrow_depth(o) > 0.5;
        let flee_weight = cfg.balance.flee_weight * if can_dig { cfg.balance.burrow_caution } else { 1.0 };
        // Seasonal strategies: fattening before a dormancy, migrating
        // between seasonal ranges.
        let fatten = self.fat_room(o) / self.fat_capacity(o).max(1e-6);
        let season_dest = self.seasonal_destination(o, juvenile_care);
        let ordered = self.migrate_orders.contains_key(&o.owner) && o.matured;
        // A sentry on duty with its group around stands watch unless hungry,
        // thirsty or courting wins: some stay alert while others breed. A
        // migrator on its seasonal journey keeps going instead.
        let watch = if kin >= 2 && season_dest.is_none() && self.on_watch_duty(o) { 0.7 * sentry * (1.0 - hunger).powi(2) } else { 0.0 };

        let mut options: [(Action, f32); 14] = [
            (Action::Flee, threat * (1.3 - boldness) * flee_weight * (1.0 - reckless)),
            (Action::Graze, hunger.powf(1.5) * 1.2 + if hunger > 0.8 { 0.4 } else { 0.0 } + 0.4 * fatten),
            (Action::Drink, thirst.powf(1.5) * 1.3 + if thirst > 0.8 { 0.5 } else { 0.0 }),
            (Action::Rest, tired * tired * 1.1),
            (Action::Bask, if o.traits.warm { 0.0 } else { (1.0 - o.activity) * 0.7 }),
            // A threat it has seen puts courting on hold (`courtship_caution`);
            // breeding drive still blunts the flight itself (`reckless`).
            (Action::SeekMate, if can_breed {
                n.breeding / 100.0 * 0.9 * (1.0 - 0.7 * hunger) * (1.0 - cfg.balance.courtship_caution * threat.min(1.0))
            } else {
                0.0
            }),
            (Action::Group, match kin_centroid {
                // Comfortable spacing grows with group size, so herds spread to graze.
                Some(c) if o.pos.dist(c) > 1.5 + 4.0 * (1.0 - p[Gene::GroupTolerance]) + 0.8 * (kin as f32).sqrt() => {
                    (p[Gene::GroupTolerance] * 0.25 + n.social / 100.0 * 0.3) * (1.0 - hunger)
                }
                _ => 0.0,
            }),
            (Action::Wander, 0.05 + 0.15 * p[Gene::Wanderlust]),
            (Action::Migrate, if season_dest.is_some() { 0.6 } else if ordered { 0.55 } else { 0.0 }),
            (Action::Cache, self.cache_utility(o)),
            (Action::FollowParent, match mother_pos {
                Some(m) if juvenile_care && o.pos.dist(m) > 1.2 => 0.6,
                _ => 0.0,
            }),
            (Action::Guard, if o.nest.is_some() { 0.6 } else { 0.0 }),
            (Action::Sentry, watch),
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
        let action = urgent.unwrap_or(action);

        // An exhausted hunter rests before chasing again; otherwise hunger keeps
        // it chasing at a crawl until it starves.
        let meat_here = self.map.tile_at(o.pos).meat * o.traits.carnivory > 1.0;
        let action = if action == Action::Graze && o.traits.carnivory > 0.6 && n.energy < 30.0 && !meat_here && threat < 0.3 {
            Action::Rest
        } else {
            action
        };

        // A sentry warns its group of a threat it saw itself, on watch or
        // while fleeing from what it spotted there (never a relayed warning).
        let raising = action == Action::Sentry || (action == Action::Flee && (watching || o.alert.is_some()));
        if raising && seen_threat > 0.0 {
            intent.alert = Some((seen_pos, seen_threat));
        }

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
            // Navigating with its group, it keeps up with the group.
            Action::Migrate => {
                intent.target = match season_dest {
                    Some(d) => Some(self.migration_target(o, d, kin_centroid)),
                    None => self.migrate_orders.get(&o.owner).copied(),
                }
            }
            Action::Cache => self.choose_cache_spot(o, &mut intent, &mut rng),
            Action::FollowParent => intent.target = mother_pos,
            Action::Guard => intent.target = o.nest,
            // A sentry stands watch where it is.
            Action::Hunt | Action::Scavenge | Action::Sentry | Action::Ordered | Action::Dormant => {}
        }
        intent
    }

    /// What, if anything, overrides a possessed organism's move order: a
    /// threat at `Balance::possess_danger` (flee), or hunger or thirst at
    /// `Balance::possess_urgency` (eat or drink, the worse of the two). An
    /// organism already eating or drinking for it keeps at it until the need
    /// is down to `ORDER_RESUME`, so it doesn't turn back after one sip.
    pub(super) fn order_override(&self, o: &Organism, threat: f32) -> Option<Action> {
        use super::commands::ORDER_RESUME;
        let b = &self.config.balance;
        if threat >= b.possess_danger {
            return Some(Action::Flee);
        }
        let n = &o.needs;
        let drinking = o.action == Action::Drink && n.thirst > ORDER_RESUME;
        let eating = matches!(o.action, Action::Graze | Action::Hunt | Action::Scavenge) && n.hunger > ORDER_RESUME;
        if drinking || eating {
            return Some(if drinking { Action::Drink } else { Action::Graze });
        }
        let worst = n.thirst.max(n.hunger);
        if worst >= b.possess_urgency {
            return Some(if n.thirst >= n.hunger { Action::Drink } else { Action::Graze });
        }
        None
    }

    /// A close look at the neighbor in grid slot `slot`, `d` tiles away: is
    /// it kin, a threat, prey, a mate? `count` adds it to the kin tally.
    /// Kinship comes from the grid's summary of it ([`Genes`] and `Entry`);
    /// the organism itself is read only for what kin can't settle.
    fn look(&self, v: &Viewer, seen: &mut Perception, slot: u32, d: f32, count: bool) {
        let (o, radius, alarm, coop) = (v.o, v.radius, v.alarm, v.coop);
        let cfg = &self.config;
        let e = self.grid.entry(slot);
        let j = e.idx as usize;
        let q = &self.organisms[j];
        let genes = self.grid.genes(slot);
        // Kinship and mate distance are monotone in the genetic distance
        // (`DistanceBounds`), as long as `species_distance` is positive.
        let gd = DistanceBounds::new(&v.genes, genes);
        let gap = body_plan_gap(&v.genes, genes);
        let (sd, limit) = (cfg.species_distance, cfg.kin_body_plan_gap);
        let kin = if sd > 0.0 {
            gd.test(o, q, |gd| mating::is_kin_by_gap(gap, gd, sd, limit))
        } else {
            mating::is_kin_by_gap(gap, genetic_distance(o, q), sd, limit)
        };
        // A kin sentry's warning reaches group members where sentries hold.
        if kin && v.sentry > 0.0 && e.flags & ALERT != 0 {
            if let Some((at, level)) = q.alert {
                let heard = level * v.sentry;
                if heard > seen.warning {
                    seen.warning = heard;
                    seen.warning_pos = at;
                }
            }
        }
        if count {
            let lineage = usize::from(e.lineage != o.lineage);
            seen.looked[lineage] += 1;
            if kin {
                seen.kin_sum += e.pos;
                seen.kin += 1;
                seen.looked_kin[lineage] += 1;
            }
        }
        // A stranger matters only as a threat or as prey; the grid's summary
        // tells whether it could be either (`can_threaten`, `max_prey_mass`)
        // without reading the organism.
        let could_threaten = e.flags & ARMED != 0 && e.mass > v.threat_floor;
        let could_be_prey = v.hunter && e.mass < v.prey_cap;
        if !kin && (could_threaten || could_be_prey) {
            // Sheltered burrowers and stealthy organisms are harder to see.
            // A shared burrow hides its sleepers better. A deep sleeper's den
            // hides it less than a burrow; a dormant burrower in its burrow
            // gets whichever hides it better.
            let burrow = if q.sheltered { 0.5 + 0.3 * (self.burrow_depth(q) - q.traits.body.burrow) } else { 0.0 };
            let den = if q.dormancy.is_deep() { cfg.balance.den_concealment } else { 0.0 };
            let hidden = burrow.max(den);
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
                    matches!(traverse(&o.traits.body, self.map.tile_at(q.pos), &self.map.weather_at(q.pos), false), Traverse::Passable { .. })
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
                    // Prey that fights back is only worth the risk to a hungry
                    // hunter, and less so to one that could graze instead.
                    let injury = injury_discount(o, v.fight_power, fight_back(q, cfg.balance.generalist_tax), cfg.balance.injury_aversion);
                    if roll >= avoid && (guard == 0.0 || my_power > q.resistance() + guard) && injury >= v.risk_floor {
                        let odds = my_power / (my_power + q.resistance() + guard + 1e-3);
                        let chase = if q.traits.stats.speed * q.activity < o.traits.stats.speed * o.activity { MAX_CHASE } else { 0.6 };
                        // `injury <= 1` and `odds < 1`, so the far-neighbor bound
                        // `mass^0.75 * MAX_CHASE / (1 + d)` still holds.
                        let score = q.mass().powf(0.75) / (1.0 + d) * odds * chase * injury;
                        if beats(score, j, seen.prey) {
                            seen.prey = Some((score, j));
                        }
                    }
                }
            }
        }
        // `COURTING` is implied by `can_breed` and the drive, and spares
        // reading the organism when it is clear.
        if v.can_breed
            && e.flags & COURTING != 0
            && (e.flags & FEMALE != 0) != v.female
            && q.can_breed(self.tick)
            && gd.test(o, q, |gd| gd < cfg.mate_distance)
            && q.needs.breeding > 20.0
            && mating::fertility_from_gap(gap) > 0.0
        {
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
        // Food values use what the gut extracts (`Traits::plant_efficiency`):
        // a generalist values both foods less than a specialist would.
        let tax = self.config.balance.generalist_tax;
        let (plant_eff, c) = (o.traits.plant_efficiency(tax), o.traits.meat_efficiency(tax));
        let here = self.map.tile_at(o.pos);
        // Browse in the canopy only counts if the body can reach it.
        let (body, share) = (&o.traits.body, self.config.balance.canopy_share);
        let veg = |t: &crate::terrain::Tile| t.reachable_veg(body, share);

        // Value of each option in rough "food I can get" terms.
        let hunt_value = prey.map(|(s, _)| s * 4.0 * c).unwrap_or(0.0);
        let meat_here = here.meat * c;
        let graze_here = veg(here) * plant_eff;

        if meat_here > 1.0 && meat_here >= graze_here {
            intent.action = Action::Scavenge;
            return;
        }
        if let Some((_, j)) = prey {
            if hunt_value > graze_here * 0.05 && o.traits.carnivory > HUNTER_CARNIVORY {
                let q = &self.organisms[j];
                intent.action = Action::Hunt;
                intent.target = Some(q.pos);
                intent.target_org = Some(j);
                intent.sprint = o.pos.dist(q.pos) < 5.0 && o.needs.energy > 25.0;
                return;
            }
        }
        if o.traits.carnivory > 0.7 {
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
        // A cacher lives off its hoard in the lean seasons.
        if let Some(c) = self.cache_to_eat(o, bite) {
            intent.target = (o.pos.dist(c) > 0.5).then_some(c);
            return;
        }
        // Keep heading for the patch chosen earlier while it still has food.
        if let Some(t) = o.forage_target {
            let tile = self.map.tile_at(t);
            if veg(tile) * plant_eff + tile.meat * c >= bite {
                intent.target = Some(t);
                return;
            }
        }
        match self.find_tile(o, rng, 2.0 * bite, |t| veg(t) * plant_eff + t.meat * c) {
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
            if !matches!(traverse(&o.traits.body, t, &self.map.weather_at(cand), false), Traverse::Passable { .. }) {
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

    /// Gathering for the cache: go back if strayed too far from it, else
    /// gather here if there is food, else find a food tile nearby.
    fn choose_cache_spot(&self, o: &Organism, intent: &mut Intent, rng: &mut SimRng) {
        let eff = 1.0 - o.traits.carnivory;
        let bite = 25.0 * o.food_per_hunger();
        if let Some(c) = self.cache_home(o).filter(|&c| o.pos.dist(c) > super::seasonal::CACHE_RANGE - 1.0) {
            intent.target = Some(c);
        } else if self.map.tile_at(o.pos).veg * eff > 0.5 * bite {
            intent.target = None;
        } else if let Some(t) = self.find_tile(o, rng, 2.0 * bite, |t| t.veg * eff) {
            intent.target = Some(t);
        } else {
            self.wander(o, intent, rng, 5.0);
        }
    }

    fn wander(&self, o: &Organism, intent: &mut Intent, rng: &mut SimRng, range: f32) {
        // A target the body can't stand on is never reached, and would pin the
        // organism against the shore forever (starving next to the water).
        let usable = |t: Vec2| matches!(traverse(&o.traits.body, self.map.tile_at(t), &self.map.weather_at(t), false), Traverse::Passable { .. });
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
        let blocked = |w: &World, p: Vec2| traverse(&w.organisms[i].traits.body, w.map.tile_at(p), &w.map.weather_at(p), false) == Traverse::Blocked;
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

    /// A burrowing omnivore (carnivory 0.5) hunts voles, so a vole must see
    /// it as a threat even beyond its nearest `FOCUS` neighbors, where only
    /// the grid's `ARMED` flag decides whether it gets a close look.
    #[test]
    fn crowd_does_not_hide_a_burrowing_omnivore() {
        let mut w = presets::standard_world(WorldConfig { seed: 4, width: 48, height: 48, ..Default::default() }, 1);
        let vole = w.organisms.iter().position(|o| o.lineage == 0 && o.matured).unwrap();
        let burrower = w.organisms.iter().position(|o| o.lineage == 7 && o.growth > 0.5).unwrap();
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
        let b = &mut w.organisms[burrower];
        b.traits.carnivory = 0.5;
        b.sheltered = false;
        b.pos = center + Vec2::new(2.0, 0.0);
        assert!(can_threaten(&w.organisms[burrower], &w.organisms[vole]));
        w.grid.rebuild(&w.organisms);
        let intent = w.decide(vole);
        assert!(intent.threat > 0.0, "burrower unseen");
    }

    /// Predators are risky prey: an omnivore shies away from a fox more than
    /// a strict carnivore of equal power would, and nobody fears a grazer.
    #[test]
    fn hunters_weigh_injury_from_prey_that_fights_back() {
        let w = presets::standard_world(WorldConfig { seed: 3, width: 48, height: 48, ..Default::default() }, 1);
        let pick = |lineage: u32| &w.organisms[w.organisms.iter().position(|o| o.lineage == lineage).unwrap()];
        let (fox, pack, burrower) = (pick(3), pick(4), pick(7));
        // The most herbivorous grazer founder (founders vary around the design).
        let grazer = w.organisms.iter().filter(|o| o.lineage == 5).min_by(|a, b| a.traits.carnivory.total_cmp(&b.traits.carnivory)).unwrap();
        let k = w.config.balance.injury_aversion;
        let fight_back = |o| fight_back(o, w.config.balance.generalist_tax);
        assert!(fight_back(grazer) < 0.05 * fight_back(fox));
        // Founder variation can leave a grazer a few hundredths of carnivory.
        assert!(injury_discount(pack, pack.power(), fight_back(grazer), k) > 0.8);
        let from_burrower = injury_discount(burrower, 0.5, fight_back(fox), k);
        let from_carnivore = injury_discount(pack, 0.5, fight_back(fox), k);
        assert!(from_burrower < 0.5 * from_carnivore, "{from_burrower} vs {from_carnivore}");
        // Even a starving omnivore leaves the fox alone: its risk tolerance
        // tops out at its carnivory (`1 - hunger * carnivory`).
        assert!(from_burrower < 1.0 - burrower.traits.carnivory, "{from_burrower}");
        // A predator that burrows can threaten what it would hunt.
        assert!(can_threaten(burrower, pick(0)));
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

    /// The quick kinship tests (packed genes, distance bounds) give exactly
    /// what the genome-based ones give, over every pair in a mixed world
    /// with some drift, the bounds bracketing the exact distance.
    #[test]
    fn packed_kinship_tests_match_the_genome() {
        let mut w = presets::standard_world(WorldConfig { seed: 5, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(6);
        let cfg = &w.config;
        let alive: Vec<&Organism> = w.organisms.iter().filter(|o| o.alive).collect();
        let (mut kin, mut strangers) = (0, 0);
        for a in &alive {
            let ga = genes_of(&a.pheno);
            for b in &alive {
                let gb = genes_of(&b.pheno);
                let gap = body_plan_gap(&ga, &gb);
                assert_eq!(gap.to_bits(), mating::body_plan_gap(&a.genome, &b.genome).to_bits());
                let exact = genetic_distance(a, b);
                let bounds = DistanceBounds::new(&ga, &gb);
                assert!(bounds.lo <= exact && exact <= bounds.hi, "{} <= {exact} <= {}", bounds.lo, bounds.hi);
                let is_kin = bounds.test(a, b, |gd| mating::is_kin_by_gap(gap, gd, cfg.species_distance, cfg.kin_body_plan_gap));
                assert_eq!(is_kin, mating::is_kin(&a.genome, &b.genome, exact, cfg.species_distance, cfg.kin_body_plan_gap));
                assert_eq!(bounds.test(a, b, |gd| gd < cfg.mate_distance), exact < cfg.mate_distance);
                if is_kin {
                    kin += 1;
                } else {
                    strangers += 1;
                }
            }
        }
        assert!(kin > 100 && strangers > 100, "kin {kin}, strangers {strangers}");
    }
}
