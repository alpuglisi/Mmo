//! The sequential half of a tick: movement, eating, predation, mating, needs,
//! death, birth and growth.

use rand::Rng;

use super::{Clutch, Intent, World};
use crate::design::BirthType;
use crate::genome::{Gene, Genome, PlayerId};
use crate::math::Vec2;
use crate::mating;
use crate::organism::{Action, DeathCause, Dormancy, Infection, OrgId, Organism, Pregnancy, Sex};
use crate::pathfinding::{enter_cost, Outcome, Path, PathScratch, MAX_TILE_SPEED};
use crate::rng::{rng_for, stream, SimRng};
use crate::terrain::{glide, traverse, wind_effect, Map, Traverse, Weather};

/// How far (tiles) a mother looks for a nest site for eggs she leaves.
const NEST_SEARCH: usize = 3;
/// Carnivory above which an organism is an obligate carnivore: it roams
/// for prey rather than grazing (`decide::choose_food`), and the
/// carnivore-diet rules of version 26 apply to it (gorging on meat,
/// carcass scent, provisioning, breeding territories).
pub(crate) const OBLIGATE: f32 = 0.7;

/// How far the carnivore-diet rules (gorging, carcass scent, provisioning,
/// breeding territories) apply to `o`, from its carnivory gene: 0 for a
/// plant eater up to `decide::HUNTER_CARNIVORY` (0.45), rising evenly to 1
/// for an obligate carnivore (`OBLIGATE`, 0.7, and above). Before version
/// 27 the rules applied in full above 0.7 and not at all below; a
/// flexible mesopredator (the red fox at 0.65) now gets most of them, a
/// burrowing omnivore (0.5) a fifth.
pub(crate) fn meat_reliance(o: &Organism) -> f32 {
    let lo = super::decide::HUNTER_CARNIVORY;
    ((o.traits.carnivory - lo) / (OBLIGATE - lo)).clamp(0.0, 1.0)
}

/// Most fat (hunger points) `o` banks from meat in any season
/// (`Balance::carnivore_fat`): the global scale times its meat reliance
/// times `0.5 + fat stores`, so the fat stores gene sets how much of a kill
/// a carnivore can bank (60 at fat stores 0.5) and selection can tune it.
pub(crate) fn gorge_capacity(bal: &crate::Balance, o: &Organism) -> f32 {
    bal.carnivore_fat * meat_reliance(o) * (0.5 + o.pheno[Gene::FatStores])
}

/// Share of a litter a mother carries to term for her condition
/// (`Balance::condition_litter`): `1 − condition_litter × flex × (1 −
/// nourished)`, `nourished = (90 − hunger + fat) / 60` clamped to 0–1 (fat
/// carries a hungry mother: the fat stores gene), `flex = min(1, 2 × litter
/// fraction)` (the litter size gene: big litters shrink, a single young
/// can't). 1 with the rule off.
pub(crate) fn litter_condition(bal: &crate::Balance, mother: &Organism) -> f32 {
    let k = bal.condition_litter;
    if k <= 0.0 {
        return 1.0;
    }
    let nourished = ((90.0 - mother.needs.hunger + mother.fat) / 60.0).clamp(0.0, 1.0);
    let flex = (2.0 * mother.traits.litter_fraction()).min(1.0);
    (1.0 - k * flex * (1.0 - nourished)).clamp(0.0, 1.0)
}

/// How much of a provider father `o` is (`Balance::provisioning`): the
/// higher of its pack instinct and group tolerance, so social carnivores
/// (pack hunters, family-living foxes) bring food to the den and loners
/// rarely do. Scales both his urge to provision and how much he brings.
pub(crate) fn provision_care(o: &Organism) -> f32 {
    o.pheno[Gene::PackInstinct].max(o.pheno[Gene::GroupTolerance])
}

/// A carnivore female's breeding-territory radius in tiles
/// (`Balance::breeding_territory` times its meat reliance): larger the
/// more aggressive, smaller the more tolerant of its own kind, `1 +
/// aggression − group tolerance` (from a quarter to twice the global
/// radius). A lone, aggressive hunter keeps a big territory; a tolerant
/// pack shares its range.
pub(crate) fn territory_radius(bal: &crate::Balance, o: &Organism) -> f32 {
    let temper = (1.0 + o.pheno[Gene::Aggression] - o.pheno[Gene::GroupTolerance]).clamp(0.25, 2.0);
    bal.breeding_territory * meat_reliance(o) * temper
}

/// Most hunger points an organism can eat per day (so a meal takes a few ticks).
pub(crate) const MAX_INTAKE_PER_DAY: f32 = 1200.0;
const MAX_DRINK_PER_DAY: f32 = 1500.0;
/// Energy per tile walked at endurance 1.0.
const ENERGY_PER_TILE: f32 = 0.6;
const MATING_REACH: f32 = 1.2;
/// Sprints shorter than this (tiles) steer greedily instead of planning.
const PATH_SHORT: f32 = 4.0;
/// Chance per tick of flight in a storm that a gust injures a flyer, and
/// the health and energy it costs.
const BUFFET_CHANCE: f32 = 0.05;
const BUFFET_DAMAGE: f32 = 10.0;

/// Air temperature at which a basking cold-blooded body gets no more
/// warmth from the sun (`Balance::cold_heat`).
const COLD_SHADE: f32 = 30.0;
/// Body temperature past which a cold-blooded body slows and loses water
/// faster (`Balance::cold_heat`).
const COLD_SLOW: f32 = 32.0;
/// Body temperature that starts heat stress for a cold-blooded body under
/// `Balance::cold_heat` (harm past 8 more: 34 + 8 = 42 °C; 38 + 8 = 46
/// without it).
const COLD_HEAT_LIMIT: f32 = 34.0;
/// Extra hunger per unit of toxins (`Balance::toxin_upkeep`).
const TOXIN_UPKEEP: f32 = 0.4;
/// Extra hunger of a flier on the move, a share of `Balance::hunger_rate`
/// per unit of wings at 4 kg (`Balance::flight_upkeep`).
const FLIGHT_HUNGER: f32 = 0.45;
/// Extra thirst of a flier on the move, a share of `Balance::thirst_rate`
/// per unit of wings (`Balance::flight_upkeep`).
const FLIGHT_THIRST: f32 = 0.35;

/// Hunger multiplier from a cold-blooded body's temperature: a sluggish body
/// (low `activity`) idles cheaply; above full activity (22 °C) every 10 °C
/// costs `heat_burn` more (`Balance::cold_heat_burn`), so the food discount
/// of cold blood is largest where the body is cool and slow.
/// Typical activity cost over a day of grazing (resting 0.8, walking 1.2,
/// standing 1.0 in [`World::update_needs`]-style accounting): what a held
/// herd's day averages.
pub(crate) const MEAN_ACTIVITY: f32 = 1.1;

impl World {
    /// Hunger `o` gains in a typical day where it stands now: the base rate
    /// with the multipliers that don't depend on the moment's action
    /// (toxin upkeep, metabolism, the meat gut's discount, crowding by kin
    /// and by the lineage's size, warm-body upkeep and cold, pregnancy,
    /// body size) and [`MEAN_ACTIVITY`]. Food estimates use it; playtest 7's
    /// 600-animal herds ate about twice the bare rate (crowding), so a
    /// claim "for 65 days" was eaten in days.
    pub(crate) fn steady_hunger_per_day(&self, o: &Organism) -> f32 {
        let bal = &self.config.balance;
        let toxin_cost = 1.0 + TOXIN_UPKEEP * o.pheno[Gene::Toxins] * bal.toxin_upkeep;
        let diet = 1.0 - bal.carnivore_hunger_discount * o.traits.meat_efficiency(bal.generalist_tax);
        let crowd = 1.0 + bal.crowding * (o.kin_nearby as f32 - 8.0).max(0.0) * (1.0 - o.pheno[Gene::GroupTolerance]);
        let crowd = crowd * self.lineage_crowding(o.lineage)[1];
        let thermo = if o.traits.warm {
            (1.0 + bal.warm_upkeep) * (1.0 + 0.04 * o.needs.cold)
        } else {
            cold_blooded_thermo(o.activity, 22.0, 0.0)
        };
        let pregnant = if o.pregnancy.is_some() {
            1.0 + bal.pregnancy_cost * o.traits.litter_burden * litter_condition(bal, o)
        } else {
            1.0
        };
        let size_burn = if bal.size_burn == 0.0 { 1.0 } else { o.traits.body.mass.powf(-bal.size_burn) };
        bal.hunger_rate
            * toxin_cost
            * o.traits.stats.metabolism
            * diet
            * crowd
            * thermo
            * pregnant
            * size_burn
            * MEAN_ACTIVITY
    }
}

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

/// Drinking where it stands, if it can: touches nothing but the organism
/// (the act sweep runs these on all cores, `par_apply`).
pub(super) fn drink_from(map: &Map, dt_days: f32, o: &mut Organism) {
    if !map.can_drink_at(o.pos) {
        return;
    }
    let amount = MAX_DRINK_PER_DAY * dt_days;
    o.needs.thirst = (o.needs.thirst - amount).max(0.0);
    o.last_water = Some(o.pos);
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
        // Under a player's migration order, it keeps off ice that would
        // drown it if it melted (a frozen lake looks like a short cut).
        // So does an animal walking to an agreed mating (`set_rendezvous`).
        let wary = (intent.action == Action::Migrate
            && !self.migrate_orders.is_empty()
            && self.migrate_orders.contains_key(&(o.owner, o.group)))
            || (intent.action == Action::Rally && self.in_rendezvous(o));
        match intent.target {
            Some(target) => self.step_toward(o, chase.unwrap_or(target), intent.sprint, frac, wary, scratch, paths),
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
            self.leave_carrion(i, DeathCause::Journey);
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
            Action::Scavenge => {
                // With a target organism: a father bringing food to his
                // family (`Balance::provisioning`).
                if let Some(j) = intent.target_org {
                    self.provision(i, j);
                }
                self.eat_meat(i, 1.0);
            }
            Action::Hunt => {
                if let Some(j) = intent.target_org {
                    self.attack(i, j, rng);
                } else if self.organisms[i].needs.hunger > self.config.balance.roaming_forage {
                    // Starving roaming forage (`Balance::roaming_forage`, off
                    // by default): no prey in sight, so nibble plants as on
                    // any other errand.
                    self.eat_plants(i, 0.3);
                }
                // A roaming hunter drinks where it passes water (with the
                // carnivore-diet rules, `Balance::carcass_scent`).
                if intent.target_org.is_none()
                    && self.config.balance.carcass_scent > 0.0
                    && self.organisms[i].needs.thirst > 30.0
                {
                    self.drink(i);
                }
                // A hunter killed in its strike eats nothing
                // (`Balance::strike_fixes`).
                if self.organisms[i].alive || self.config.balance.strike_fixes <= 0.0 {
                    self.eat_meat(i, 1.0);
                }
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
            // A hunter poisoned by its prey (`attack`) lies where it fell,
            // once its turn is over (without `Balance::strike_fixes` it
            // still strikes and eats in it).
            if self.organisms[i].death == Some(DeathCause::Toxin) {
                self.leave_carrion(i, DeathCause::Toxin);
            }
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
            // A possessed animal at water drinks from a lower thirst: the
            // player walked it there (`Balance::drink_priority`).
            let o = &self.organisms[i];
            let at_order = o.possessed_by.is_some() && self.config.balance.possess_drink > 0.0;
            let sip = if at_order { 10.0 } else { 30.0 };
            if o.needs.thirst > sip && intent.action != Action::Drink {
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
    pub(super) fn settle(
        &self,
        o: &mut Organism,
        intent: &Intent,
        moved: f32,
        burrow: (f32, f32),
        rng: &mut SimRng,
    ) -> Settled {
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
            self.leave_carrion(i, cause);
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

    /// Carrion (`Balance::carrion`): the body of organism `i`, dead of
    /// `cause`, is left as a carcass where it fell, a starved one at half;
    /// not after predation (the hunter leaves the carcass) or drowning.
    pub(super) fn leave_carrion(&mut self, i: usize, cause: DeathCause) {
        let carrion = self.config.balance.carrion;
        // (A hunter killed fighting its prey left no carcass as predation
        // and still doesn't.)
        if carrion > 0.0 && !matches!(cause, DeathCause::Drowning | DeathCause::Predation | DeathCause::FoughtBack) {
            let o = &self.organisms[i];
            let lean = if cause == DeathCause::Starvation { 0.5 } else { 1.0 };
            let meat = o.mass() * self.config.balance.meat_per_kg * carrion * lean;
            let pos = o.pos;
            self.map.tile_at_mut(pos).meat += meat;
        }
    }

    /// Move organism `i` toward `target` (see `step_toward`), `frac` of a
    /// tick's movement.
    pub(super) fn move_toward(&mut self, i: usize, target: Vec2, sprint: bool, frac: f32) -> f32 {
        self.move_toward_with(i, target, sprint, frac, false)
    }

    /// [`Self::move_toward`], keeping off ice traps if `wary` (as under a
    /// migration order).
    pub(super) fn move_toward_with(&mut self, i: usize, target: Vec2, sprint: bool, frac: f32, wary: bool) -> f32 {
        let mut orgs = std::mem::take(&mut self.organisms);
        let mut scratch = std::mem::take(&mut self.path_scratch);
        let mut paths = PathCount::default();
        let moved = self.step_toward(&mut orgs[i], target, sprint, frac, wary, &mut scratch, &mut paths);
        self.organisms = orgs;
        self.path_scratch = scratch;
        paths.add_to(&mut self.counters);
        moved
    }

    /// How far `o` moves in a tick on open ground (tiles, before the terrain's
    /// own speed factor): its genes, size, condition, a sprint (if it has
    /// the energy) and the possession bonus. Clients use it to predict a
    /// possessed organism's walk between state messages.
    pub fn move_speed(&self, o: &Organism, sprint: bool) -> f32 {
        let b = &self.config.balance;
        let mut speed = b.base_speed * o.traits.stats.speed * o.activity * (0.5 + 0.5 * o.growth);
        if o.pregnancy.is_some() && o.traits.birth == BirthType::Live {
            speed *= 0.7;
        }
        if o.needs.energy < 20.0 {
            speed *= 0.5;
        }
        if o.needs.hunger > 90.0 {
            speed *= 0.7;
        }
        if sprint && o.needs.energy > 15.0 {
            speed *= b.sprint;
        }
        // Small possession bonus (reaction), per the design doc.
        if o.possessed_by.is_some() {
            speed *= 1.1;
        }
        speed
    }

    /// Steer `o` toward `target`, following or planning a route (A* in
    /// `scratch`, searches counted in `paths`) when the way is blocked, by
    /// `frac` of a tick's movement (1 except for possession sub-steps).
    /// A `wary` walker treats ice it would drown under as blocked
    /// ([`ice_trap`]). Touches nothing but `o`, so the parallel apply can
    /// run it for many organisms at once. Returns the distance moved.
    #[allow(clippy::too_many_arguments)]
    fn step_toward(
        &self,
        o: &mut Organism,
        target: Vec2,
        sprint: bool,
        frac: f32,
        wary: bool,
        scratch: &mut PathScratch,
        paths: &mut PathCount,
    ) -> f32 {
        let from = o.pos;
        let moved = self.step_toward_inner(o, target, sprint, frac, wary, scratch, paths);
        // Facing follows the last movement (`Organism::heading`).
        if moved > 0.0 {
            let dir = (o.pos - from).norm();
            if dir != Vec2::ZERO {
                o.heading = dir;
            }
        }
        moved
    }

    /// [`Self::step_toward`] without the heading update.
    #[allow(clippy::too_many_arguments)]
    fn step_toward_inner(
        &self,
        o: &mut Organism,
        target: Vec2,
        sprint: bool,
        frac: f32,
        wary: bool,
        scratch: &mut PathScratch,
        paths: &mut PathCount,
    ) -> f32 {
        let to = target - o.pos;
        let d = to.len();
        if d < 0.05 {
            return 0.0;
        }
        let st = &o.traits.stats;
        let sprinting = sprint && o.needs.energy > 15.0;
        let speed = self.move_speed(o, sprint);
        let step = (speed * frac).min(d);
        let dir = to * (1.0 / d);
        let body = o.traits.body;
        let pos = o.pos;
        let endurance = st.endurance;
        let air = self.map.weather_at(pos);
        let fly = body.can_fly && o.needs.energy > 25.0 && !air.grounds(&body);
        // Stuck on a tile the body can't use (e.g. ice melted underneath):
        // struggle toward the target regardless.
        let stranded = !fly && self.map.traverse_at(&body, pos, false) == Traverse::Blocked;
        let effort = Effort { sprint: if sprinting { 2.5 } else { 1.0 }, endurance };

        // Walkers follow a route planned earlier, or plan one when the direct
        // step is blocked (flyers only meet storms, which ground everything).
        // Short sprints (fleeing, the last lunge at prey) stay greedy.
        let pathing = !stranded && !fly && self.config.balance.path_nodes >= 1.0;
        let wary = wary && !fly;
        let trap = |p: Vec2| wary && ice_trap(&body, self.map.tile_at(p));
        if pathing {
            if let Some(moved) = self.follow_path(o, target, step, effort, wary) {
                return moved;
            }
        }
        let can_plan = pathing && !(sprinting && d < PATH_SHORT) && !self.next_to(pos, target);

        for (k, &angle) in [0.0f32, 0.6, -0.6, 1.2, -1.2, 1.9, -1.9].iter().enumerate() {
            let dd = dir.rotate(angle);
            let dest = self.map.clamp(pos + dd * step);
            let rule = if stranded {
                Traverse::Passable { speed: 0.3, energy: 2.0 }
            } else if trap(dest) {
                Traverse::Blocked
            } else {
                let rule = self.map.traverse_at(&body, dest, fly);
                // Flyers ride or fight the wind; gliders launch from height.
                let lift = if fly {
                    Some(wind_effect(&body, &air, dd))
                } else {
                    glide(&body, self.map.ground_at(pos), self.map.ground_at(dest), &air, dd)
                };
                match (rule, lift) {
                    (Traverse::Passable { speed, energy }, Some((ls, le))) => {
                        Traverse::Passable { speed: speed * ls, energy: energy * le }
                    }
                    _ => rule,
                }
            };
            if let Traverse::Passable { speed: s, energy: e } = rule {
                let dist = (step * s.min(MAX_TILE_SPEED)).min(d);
                let new_pos = self.map.clamp(pos + dd * dist);
                // The end point can fall short of the checked tile (or cut a
                // corner): never step onto a tile the body can't use.
                if stranded || (self.map.traverse_at(&body, new_pos, fly) != Traverse::Blocked && !trap(new_pos)) {
                    o.pos = new_pos;
                    o.needs.energy = (o.needs.energy - effort.cost(dist, e)).max(0.0);
                    if fly && air.storm() {
                        self.buffet(o, frac);
                    }
                    return dist;
                }
            }
            // The way straight ahead is blocked: find a way around.
            if k == 0 && can_plan && self.plan_path(o, target, wary, scratch, paths) {
                if let Some(moved) = self.follow_path(o, target, step, effort, wary) {
                    return moved;
                }
            }
        }
        0.0
    }

    /// A flyer in a storm risks being thrown about: an injury (never fatal
    /// on its own) and lost energy, drawn from the wind stream.
    fn buffet(&self, o: &mut Organism, frac: f32) {
        let mut rng = rng_for(self.config.seed, stream::WIND, self.tick, o.id);
        if rng.gen::<f32>() < BUFFET_CHANCE * frac {
            o.health = (o.health - BUFFET_DAMAGE).max(1.0);
            o.needs.energy = (o.needs.energy - BUFFET_DAMAGE).max(0.0);
        }
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
    fn plan_path(
        &self,
        o: &mut Organism,
        target: Vec2,
        wary: bool,
        scratch: &mut PathScratch,
        paths: &mut PathCount,
    ) -> bool {
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
            |g, t| {
                if wary && ice_trap(&body, t) {
                    return None;
                }
                enter_cost(traverse(&body, g, t, &Weather::still(t.wind), false))
            },
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
    fn follow_path(&self, o: &mut Organism, target: Vec2, step: f32, effort: Effort, wary: bool) -> Option<f32> {
        let path = o.path.as_mut()?;
        let first = path.next_tile()?;
        let pos = o.pos;
        let body = o.traits.body;
        let w = self.map.width;
        let center = |k: u32| Map::tile_center(k as usize % w, k as usize / w);
        let rule = |k: u32| {
            let t = &self.map.tiles[k as usize];
            if wary && ice_trap(&body, t) {
                return Traverse::Blocked;
            }
            traverse(&body, &self.map.ground()[k as usize], t, &Weather::still(t.wind), false)
        };
        let (cx, cy) = self.map.tile_coords(pos);
        let on_route = (first as usize % w).abs_diff(cx) <= 1 && (first as usize / w).abs_diff(cy) <= 1;
        let ahead = if on_route && path.serves(pos, target) { rule(first) } else { Traverse::Blocked };
        let Traverse::Passable { speed: mut s, energy: mut e } = ahead else {
            path.clear();
            return None;
        };
        // A glider on a route still glides where it has the height.
        if body.can_glide {
            let ground = self.map.ground();
            let (here, next) = (&ground[self.map.idx(cx, cy)], &ground[first as usize]);
            let dir = (center(first) - pos).norm();
            if let Some((ls, le)) = glide(&body, here, next, &self.map.weather_at(pos), dir) {
                (s, e) = (s * ls, e * le);
            }
        }
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
    /// keep eating past full and store the surplus as fat. Ripe fruit and
    /// seeds (`Map::fruit`, `Balance::fruit_crop`) come first: they are worth
    /// more per unit to any gut (`Traits::fruit_efficiency` times
    /// `Balance::fruit_value`), and leaves make up the rest.
    fn eat_plants(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let bal = &self.config.balance;
        let eff = o.traits.plant_efficiency(bal);
        let fat_room = self.fat_room(o);
        let room = o.needs.hunger + fat_room;
        let (pos, body) = (o.pos, o.traits.body);
        let (x, y) = self.map.tile_coords(pos);
        let k = self.map.idx(x, y);
        let fruit = self.map.fruit.get(k).copied().unwrap_or(0.0);
        let leaves = o.traits.carnivory <= 0.95 && eff > 0.0;
        if (!leaves && fruit <= 0.0) || room < 5.0 {
            return;
        }
        let mut want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(room);
        let fph = o.food_per_hunger();
        let mut gained = 0.0;
        if fruit > 0.0 {
            let fruit_eff = o.traits.fruit_efficiency() * bal.fruit_value();
            let got = (want * fph / fruit_eff).min(fruit);
            self.map.fruit[k] -= got;
            gained = got * fruit_eff / fph;
            want -= gained;
        }
        if leaves && want > 0.0 {
            let share = self.config.balance.canopy_share;
            let got = (want * fph / eff).min(self.map.tiles[k].reachable_veg(&self.map.ground()[k], &body, share));
            self.map.tiles[k].veg -= got;
            gained += got * eff / fph;
        }
        self.feed(i, gained, fat_room);
    }

    /// Eat carcass meat on the current tile, worth `Balance::meat_value`
    /// units of leaves a unit, then insects, worms and grubs (`Map::insects`,
    /// `Balance::insect_crop`) at the same value, digested as far as the
    /// gut is an omnivore's (`Traits::insect_efficiency`) and gathered at
    /// half the pace, for what it still has room for.
    fn eat_meat(&mut self, i: usize, rate: f32) {
        let o = &self.organisms[i];
        let bal = &self.config.balance;
        let eff = o.traits.meat_efficiency(bal.generalist_tax) * bal.meat_value();
        let bugs_eff = o.traits.insect_efficiency() * bal.meat_value();
        let fat_room = self.meat_fat_room(o);
        let room = o.needs.hunger + fat_room;
        if o.traits.carnivory < 0.2 || room < 5.0 {
            return;
        }
        let want = (MAX_INTAKE_PER_DAY * self.config.dt_days() * rate).min(room);
        let fph = o.food_per_hunger();
        let pos = o.pos;
        let (x, y) = self.map.tile_coords(pos);
        let k = self.map.idx(x, y);
        let tile = &mut self.map.tiles[k];
        let got = (want * fph / eff).min(tile.meat);
        tile.meat -= got;
        let mut gained = got * eff / fph;
        let bugs = self.map.insects.get(k).copied().unwrap_or(0.0);
        if bugs > 0.0 && bugs_eff > 0.0 {
            let more = (0.5 * want).min(want - gained);
            if more > 0.0 {
                let got = (more * fph / bugs_eff).min(bugs);
                self.map.insects[k] -= got;
                gained += got * bugs_eff / fph;
            }
        }
        self.feed(i, gained, fat_room);
    }

    fn drink(&mut self, i: usize) {
        let dt = self.config.dt_days();
        drink_from(&self.map, dt, &mut self.organisms[i]);
    }

    /// Hunter `i` killed in its fight with prey `j`: by a plant eater, a
    /// death fighting back (`DeathCause::FoughtBack`, not predation); by
    /// another hunter, predation as before.
    pub(super) fn hunter_killed_by(&mut self, i: usize, j: usize) {
        if self.organisms[j].traits.carnivory <= super::decide::HUNTER_CARNIVORY {
            self.kill_fighting_back(i, j);
        } else {
            self.kill_by(i, j);
        }
    }

    fn attack(&mut self, i: usize, j: usize, rng: &mut SimRng) {
        let (a, b) = (&self.organisms[i], &self.organisms[j]);
        // A hunter whose last prey broke free is still recovering.
        if !b.alive || a.pos.dist(b.pos) > self.config.balance.attack_reach || self.tick < a.strike_ready_tick {
            return;
        }
        // A newcomer's founding grace keeps other players' animals off it.
        if self.grace_spares(a, b) {
            return;
        }
        let escape = self.escape_chance(a, b);
        // Pack-mates striking beside it (`Balance::pack_hunt`), the more so
        // in a hunt a player leads (`decide::led_pack`).
        let bal = &self.config.balance;
        let led = self.attack_of(a) == Some(j);
        let strength = if led { super::decide::led_pack(bal) } else { bal.pack_hunt };
        let pack = 1.0 + strength * super::decide::PACK_POWER * f32::from(self.pack_allies(i, j, led));
        let pw = a.power() * (1.0 + self.coop_bonus(a)) * pack * self.config.balance.attack_power;
        let mut rs = b.resistance() * (1.0 + 1.5 * self.herd_bonus(b));
        // A parent standing over its dependent young fights for it.
        let (guard_rs, guard_power) = self.guardian(b);
        rs += guard_rs;
        // Prey that hasn't noticed the hunter is caught off guard.
        rs *= 1.0 - self.ambush_on(b);
        let mut p = pw * pw / (pw * pw + rs * rs + 1e-6);
        // Prey that made it into its burrow is mostly out of reach.
        if b.sheltered {
            p *= (1.0 - self.config.balance.burrow_shelter * self.burrow_depth(b)).max(0.0);
        }
        // A climber up in the canopy is mostly out of reach of ground hunters.
        p *= 1.0 - crate::terrain::canopy_refuge(self.map.ground_at(b.pos), &b.traits.body, &a.traits.body);
        let tox = b.pheno[Gene::Toxins];
        if tox > 0.3 {
            p *= 1.0 - 0.6 * tox;
        }
        p *= self.offline_predation_factor(b);
        // Depth judgment (`Balance::directional_vision`): binocular overlap
        // sharpens the strike (`Vision::depth_factor`, 1 at default eyes).
        let k = self.config.balance.directional_vision;
        if k > 0.0 {
            p = (p * a.traits.vision.depth_factor(k)).min(1.0);
        }
        let prey_power = b.power().max(guard_power);
        let prey_fights = super::decide::fight_back(b, self.config.balance.generalist_tax);
        let prey_bright = self.is_bright(b.pheno[Gene::Coloration]);
        // Whether the prey's toxin is its lineage's own (`Balance::toxin_source`):
        // one of its toxin alleles carries the lineage owner's origin.
        let own_toxin = self.config.balance.toxin_source <= 0.0 || {
            let owner = self.lineages.get(b.lineage as usize).map(|l| l.owner);
            let t = b.genome.locus(Gene::Toxins);
            owner.is_some_and(|w| t.a.origin == w || t.b.origin == w)
        };
        let (prey_lineage_of_b, prey_coloration) = (b.lineage, b.pheno[Gene::Coloration]);
        let prey_pos = b.pos;
        let prey_mass = b.mass();

        // Toxins hurt whoever bites, whether or not the attack succeeds.
        if tox > 0.3 {
            // A lineage that knows the toxin tastes and spits (`World::toxin_harm`).
            let harm = self.toxin_harm(self.organisms[i].lineage, prey_lineage_of_b);
            let a = &mut self.organisms[i];
            a.health -= self.config.balance.toxin_damage * tox * rng.gen_range(0.5..1.0) * harm;
            // A warning signal makes the lesson stick.
            if prey_bright {
                a.bright_aversion = true;
            }
            let (pred, prey_lineage, coloration) = (a.lineage, prey_lineage_of_b, prey_coloration);
            if own_toxin {
                self.learn_toxic(pred, prey_lineage, coloration);
            }
            let a = &mut self.organisms[i];
            if a.health <= 0.0 {
                self.kill(i, DeathCause::Toxin);
                // Dead, it strikes no more (`Balance::strike_fixes`).
                if self.config.balance.strike_fixes > 0.0 {
                    return;
                }
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
                self.hunter_killed_by(i, j);
                return;
            }
        }

        if rng.gen::<f32>() < p {
            let prey_lineage = self.organisms[j].lineage;
            self.kill_by(j, i);
            self.counters.kills += 1;
            self.organisms[i].search_image = Some(prey_lineage);
            self.map.tile_at_mut(prey_pos).meat += prey_mass * self.config.balance.meat_per_kg;
            // The hunter remembers its kill (`World::scent_carcass`).
            if self.config.balance.carcass_scent > 0.0 {
                self.organisms[i].forage_target = Some(prey_pos);
            }
            // The hunter eats in its turn (`act`); before
            // `Balance::strike_fixes` it also ate here, a second helping.
            if self.organisms[i].alive && self.config.balance.strike_fixes <= 0.0 {
                self.eat_meat(i, 1.0);
            }
        } else {
            // Held but not subdued: the struggle goes on next tick.
            self.counters.failed_hunts += 1;
            let a = &mut self.organisms[i];
            a.needs.energy = (a.needs.energy - 8.0).max(0.0);
            a.health -= prey_power * 8.0 * rng.gen::<f32>();
            if a.alive && a.health <= 0.0 {
                self.hunter_killed_by(i, j);
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
        // Snapping small prey: the hunter's snout (`morph_effects`; 1 by default).
        if b.mass() < 0.2 * a.mass() {
            q *= a.traits.morph.small_prey_escape;
        }
        (q * (1.0 - self.ambush_on(b))).min(0.95)
    }

    /// How far prey `b` is caught off guard by a strike (`Balance::ambush`):
    /// not at all while fleeing; fully if it had no threat in sight; and
    /// partly if it saw the hunter but kept drinking, resting or grazing
    /// with an eye on it (`alert_guard` of the penalty waived).
    pub(crate) fn ambush_on(&self, b: &Organism) -> f32 {
        let bal = &self.config.balance;
        if b.action == Action::Flee {
            0.0
        } else if b.needs.safety > 0.0 {
            bal.ambush * (1.0 - bal.alert_guard)
        } else {
            bal.ambush
        }
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
        // Two players' animals never mate naturally: the market's agreements
        // (`World::directed_mating`) are the way. Playtest 8's possessed
        // "stud raid" was barred; playtest 9 parked a fully fertile design in
        // a rival's herd and reached 45% rival genes in ~20 days.
        let players = a.owner != b.owner && a.owner != crate::WILD && b.owner != crate::WILD;
        if players {
            return;
        }
        let (f, m) = if a.sex == Sex::Female { (i, j) } else { (j, i) };
        // A breeding territory taken since the pair decided, by a female of
        // her lineage who conceived earlier this tick
        // (`Balance::breeding_territory`; false with the rule off).
        if self.territory_taken(&self.organisms[f]) {
            return;
        }
        let (female, male) = (&self.organisms[f], &self.organisms[m]);
        let mf = mating::mismatch(&female.pheno, male.display());
        let mm = mating::mismatch(&male.pheno, female.display());
        // Females choose; males are less picky.
        let accept_f = mf <= 1.0 || rng.gen::<f32>() < 0.05 * (-3.0 * (mf - 1.0)).exp();
        if !accept_f || mm > 1.5 {
            return;
        }
        let scale = mating::wild_gap_scale(&self.config.balance, female.owner, male.owner);
        let fert =
            mating::fertility_scaled(&female.genome, &male.genome, mating::Relief::of(&self.config.balance), scale)
                * mating::wild_fertility(&self.config.balance, female.owner, male.owner)
                * self.disease_fertility(f, m)
                * self.conception_boost(female);
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
        let father = self.organisms[m].genome.clone();
        let father_id = self.organisms[m].id;
        let father_lineage = self.organisms[m].lineage;
        let female = &mut self.organisms[f];
        female.pregnancy = Some(Pregnancy {
            father,
            father_id,
            father_lineage,
            fertility,
            due_tick: tick + (female.traits.gestation(self.config.balance.cold_gestation) * tpd) as u64,
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

    /// Father `i` feeds family member `j` (his pregnant mate or his
    /// dependent young) from his fat, at `Balance::provisioning` times the
    /// nursing rate, once next to it: a fox or wolf bringing food to the
    /// den. The fat comes from gorging on meat (`Balance::carnivore_fat`).
    fn provision(&mut self, i: usize, j: usize) {
        let (f, q) = (&self.organisms[i], &self.organisms[j]);
        if !q.alive || f.pos.dist(q.pos) > 1.5 || f.fat <= 0.0 {
            return;
        }
        let care = 2.0 * provision_care(f);
        let rate = 300.0 * self.config.balance.provisioning * care * self.config.dt_days();
        let ratio = q.food_per_hunger() / f.food_per_hunger();
        let amount = rate.min(q.needs.hunger).min(f.fat / ratio);
        let cost = amount * ratio;
        self.organisms[j].needs.hunger -= amount;
        self.organisms[i].fat -= cost;
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
        // A raider that has given up on a rare species passes its clutches
        // over too (`Balance::prey_switching`).
        if c.eggs.is_empty() || c.owner == o.owner || self.gives_up_on(o, c.lineage) {
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
        let bal = &self.config.balance;
        let eaten = food * o.traits.meat_efficiency(bal.generalist_tax) * bal.meat_value() / o.food_per_hunger();
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
    fn update_needs(
        &self,
        o: &mut Organism,
        intent: &Intent,
        moved: f32,
        dt: f32,
        burrow: (f32, f32),
    ) -> Option<DeathCause> {
        let daytime = self.is_daytime();
        let tick = self.tick;
        let pos = o.pos;
        let tile = self.map.tile_at(pos);
        let (tile_temp, open, water) = (tile.temp, self.map.ground_at(pos).is_open(), tile.water_depth);
        let body = o.traits.body;
        let drowning = !body.can_fly && self.map.traverse_at(&body, pos, false) == Traverse::Blocked;
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
            matches!(intent.action, Action::Rest | Action::Bask | Action::Guard | Action::Idle | Action::Dormant)
                && moved < 0.01;
        o.sheltered = resting && depth > 0.5 && water == 0.0;
        // Burrows buffer temperature toward 10 °C.
        let buffer = (0.6 + 0.6 * bal.shared_burrow * shared).min(0.95);
        let temp = if o.sheltered { tile_temp + (10.0 - tile_temp) * buffer } else { tile_temp };

        // A cold-blooded body's temperature follows the air (plus basking).
        let mut body_t = temp;
        let (cold, heat) = if o.traits.warm {
            o.activity = 1.0;
            // Scales displace fur's warmth (`Traits::insulation`, on top of the
            // body plan's ear and tail terms already in `stats.insulation`);
            // a bushy tail warms a resting body (`morph_effects`, 0 by default).
            let insulation = o.traits.insulation(bal.scale_cover)
                + coat_insulation
                + if resting { o.traits.morph.rest_insulation } else { 0.0 };
            let low = 12.0 - 30.0 * insulation;
            let high = 30.0 - 8.0 * insulation + o.traits.morph.heat_tolerance;
            ((low - temp).max(0.0), (temp - high).max(0.0))
        } else {
            let basking = daytime && open && matches!(intent.action, Action::Bask | Action::Rest | Action::Idle);
            let bask = if basking { 10.0 * (1.0 - o.traits.fur_mismatch - coat_mismatch).max(0.0) } else { 0.0 };
            // With `Balance::cold_heat` the sun's warmth fades as the air
            // nears 30 °C (it seeks shade), and a body past 32 °C slows.
            let hot = bal.cold_heat > 0.0;
            let bask = if hot { bask * ((COLD_SHADE - temp) / 8.0).clamp(0.0, 1.0) } else { bask };
            body_t = temp + bask;
            o.activity = ((body_t - 2.0) / 20.0).clamp(0.15, 1.0);
            if hot && body_t > COLD_SLOW {
                o.activity = o.activity.min((1.0 - (body_t - COLD_SLOW) / 12.0).max(0.15));
            }
            let limit = if hot { COLD_HEAT_LIMIT } else { 38.0 };
            ((6.0 - body_t).max(0.0), (body_t - limit).max(0.0))
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
            (1.0 + bal.warm_upkeep) * (1.0 + 0.04 * cold)
        } else {
            cold_blooded_thermo(o.activity, body_t, bal.cold_heat_burn)
        };
        // Pregnancy costs scale with the litter's total mass: the share of
        // it she would carry to term at her condition now
        // (`Balance::condition_litter`; all of it with the rule off), so a
        // starving mother resorbs part of her litter rather than starve.
        let pregnant = if o.pregnancy.is_some() {
            1.0 + bal.pregnancy_cost * o.traits.litter_burden * litter_condition(&bal, o)
        } else {
            1.0
        };
        let size = o.pheno[Gene::BodySize];
        let watch = if intent.action == Action::Sentry { 1.0 + bal.sentry_cost } else { 1.0 };
        let travel = (1.0 - bal.group_navigation / 3.0 * nav).max(0.0);
        // Carnivores eat big, infrequent meals: hunger rises more slowly. The
        // discount follows the meat gut (`Traits::meat_efficiency`), so a
        // generalist gets less of it than its carnivory alone would say.
        let diet = 1.0 - bal.carnivore_hunger_discount * o.traits.meat_efficiency(bal.generalist_tax);
        // An obligate carnivore's lean economy (`Balance::carnivore_energetics`),
        // in hard times only: a fed pack burns as before.
        let diet = diet
            * (1.0
                - OBLIGATE_DISCOUNT
                    * bal.carnivore_energetics
                    * obligacy(o.traits.carnivory)
                    * lean_times(o.needs.hunger));
        // A player's pack shares its kills: in lean times the band's meals
        // carry a hungry member (`Balance::player_packs`, by the meat gut).
        // (A founding-sized pack's first winter has the full room:
        // `World::first_winter_room`, `Balance::winter_larder`.)
        let diet = diet * (1.0 - player_pack_larder(&bal, o, self.pack_room(o.owner)));
        // Crowding: competition with many kin, felt by group-intolerant organisms.
        let crowd = 1.0 + bal.crowding * (o.kin_nearby as f32 - 8.0).max(0.0) * (1.0 - o.pheno[Gene::GroupTolerance]);
        // Competition in a lineage crowded past its soft cap
        // (`Balance::crowding_hunger`; 1 below it, when off and in the wild).
        let crowd = crowd * self.lineage_crowding(o.lineage)[1];
        // Small bodies burn through food and water faster (`Balance::size_burn`).
        let size_burn = if bal.size_burn == 0.0 { 1.0 } else { o.traits.body.mass.powf(-bal.size_burn) };
        // Making toxins costs food (`Balance::toxin_upkeep`).
        let toxin_cost = 1.0 + TOXIN_UPKEEP * o.pheno[Gene::Toxins] * bal.toxin_upkeep;
        let mut hunger = bal.hunger_rate
            * toxin_cost
            * st.metabolism
            * diet
            * crowd
            * thermo
            * activity_mult
            * pregnant
            * watch
            * travel
            * size_burn
            * dt;
        // A cold-blooded body saves water, less so in the heat
        // (`Balance::cold_heat`: +5% a degree of body heat past 32 °C).
        let cold_water = if o.traits.warm {
            1.0
        } else if bal.cold_heat > 0.0 {
            (0.7 + 0.05 * (body_t - COLD_SLOW).max(0.0)).min(1.3)
        } else {
            0.7
        };
        let mut thirst = bal.thirst_rate * (1.0 + 0.05 * heat) * (0.8 + 0.4 * size) * cold_water * size_burn * dt;
        // Flight is muscle work at any body temperature
        // (`Balance::flight_upkeep`), so a flier's low metabolism no longer
        // pays for it.
        if bal.flight_upkeep > 0.0 && o.traits.body.can_fly && moved > 0.01 {
            let wings = o.pheno[Gene::Wings] * bal.flight_upkeep;
            hunger += bal.hunger_rate * FLIGHT_HUNGER * wings * (o.traits.body.mass / 4.0).powf(0.3) * size_burn * dt;
            thirst += bal.thirst_rate * FLIGHT_THIRST * wings * (1.0 + 0.05 * heat) * size_burn * dt;
        }
        // A new founding's founders lose water more slowly in a drought, for
        // their first `FOUNDER_WATER_DAYS` (`Balance::founding_drought`):
        // time to find the water. Playtest 6's founders at 52% drought
        // died of thirst within days.
        thirst *= self.founder_water_relief(o);
        if o.dormancy != Dormancy::Awake {
            hunger *= dormant_rate;
            if deep {
                thirst *= dormant_rate;
                // Burning fat yields water (`Balance::dormant_water`).
                if o.fat > 0.0 {
                    thirst *= 1.0 - DORMANT_WATER * bal.dormant_water.clamp(0.0, 1.0);
                }
            }
        }
        // Stored fat pays for a dormancy, and for hard times.
        if o.fat > 0.0 && (deep || o.needs.hunger > 50.0) {
            let paid = hunger.min(o.fat);
            o.fat -= paid;
            hunger -= paid;
        }
        // Offline protection eases starvation and thirst (`world::offline`).
        let hardship = self.offline_hardship_factor(o);
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
            damage += harm(bal.starvation_damage * dt * hardship, DeathCause::Starvation);
        }
        if n.thirst >= 100.0 {
            damage += harm(40.0 * dt * hardship, DeathCause::Thirst);
        }
        if o.traits.warm && cold > 15.0 {
            damage += harm((cold - 15.0) * 1.5 * dt, DeathCause::Exposure);
        }
        if !o.traits.warm && cold > 10.0 && !o.sheltered {
            damage += harm((cold - 10.0) * 2.0 * dt, DeathCause::Exposure);
        }
        if heat > 8.0 {
            damage += harm((heat - 8.0) * 2.0 * dt, DeathCause::Heat);
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
            n.social = if o.kin_nearby == 0 {
                (n.social + 50.0 * gt * dt).min(100.0)
            } else {
                (n.social - 100.0 * dt).max(0.0)
            };
        }
        o.age_ticks += 1;

        if o.health <= 0.0 {
            Some(worst.map(|(_, c)| c).unwrap_or(DeathCause::Starvation))
        } else {
            None
        }
    }

    fn old_age_death(&self, o: &Organism, rng: &mut SimRng) -> bool {
        let lifespan = o.traits.lifespan_days;
        let frac = o.age_days(self.config.ticks_per_day) / lifespan;
        // Nobody passes 130% of its lifespan.
        if frac > 1.3 {
            return true;
        }
        let hazard = old_age_hazard(frac, lifespan, self.config.balance.age_spread);
        hazard > 0.0 && rng.gen::<f32>() < self.config.dt_days() * hazard
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
        let morph_rate = self.config.balance.morph_step_rate;
        let Some(preg) = self.organisms[i].pregnancy.take() else {
            return;
        };
        let mother = &self.organisms[i];
        let traits = mother.traits;
        let owner = mother.owner;
        let expected = traits.litter as f32
            * preg.fertility
            * mother.condition().clamp(0.3, 1.0)
            * self.breeding_suppression(i)
            * litter_condition(&self.config.balance, mother)
            // The soft lineage cap (`Balance::lineage_crowding`).
            * self.lineage_crowding(mother.lineage)[0];
        let mut count = expected.floor() as usize + usize::from(rng.gen::<f32>() < expected.fract());
        if !traits.birth.lays_eggs() {
            count = count.min(self.cap_room(mother.lineage));
        }
        // A warm-blooded mother nursing a litter doesn't conceive again
        // until it is (`Balance::lactation` of the way to being) weaned.
        let lactation = if traits.warm && traits.birth == BirthType::Live && count > 0 {
            self.config.balance.lactation * traits.weaning_days()
        } else {
            0.0
        };
        let mother = &mut self.organisms[i];
        mother.breed_ready_tick = tick + ((traits.recovery_days + lactation) * tpd) as u64;
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
                g.mutate_morph(owner, morph_rate);
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
                    self.spawn_newborn(
                        g,
                        owner,
                        lineage,
                        at,
                        generation,
                        mother_id,
                        preg.father_id,
                        traits.newborn_growth,
                        rng,
                    );
                }
                self.counters.births += count as u64;
            }
            BirthType::IncubatedEggs | BirthType::AbandonedEggs => {
                // Laying costs the clutch's mass in one go.
                mother.needs.hunger = (mother.needs.hunger + 60.0 * traits.litter_burden).min(100.0);
                let guardian = (traits.birth == BirthType::IncubatedEggs).then_some(mother_id);
                // A brooder makes her nest where she is and buffers it
                // herself; eggs left alone go to the best site nearby.
                let egg_mass = mother.traits.body.mass * 0.05;
                let pos = if guardian.is_some() {
                    mother.nest = Some(pos);
                    pos
                } else {
                    self.nest_site(i, traits.warm)
                };
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
                    overheat: 0.0,
                    newborn_growth: traits.newborn_growth,
                    egg_mass,
                });
            }
        }
    }

    /// Where a mother that leaves her eggs lays them: the passable tile
    /// within `NEST_SEARCH` tiles, without another clutch, whose
    /// microclimate (water and shade damp the day's heat and the season's
    /// swing) would harm her eggs least today, the nearest among equals.
    /// Abandoned eggs rely on this choice where a brooder relies on herself.
    pub(super) fn nest_site(&self, i: usize, warm_eggs: bool) -> Vec2 {
        let o = &self.organisms[i];
        let bal = self.config.balance;
        let (cx, cy) = self.map.tile_coords(o.pos);
        let r = NEST_SEARCH as i64;
        let mut best: Option<(f32, i64, Vec2)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx as i64 + dx, cy as i64 + dy);
                if x < 0 || y < 0 || x >= self.map.width as i64 || y >= self.map.height as i64 {
                    continue;
                }
                let (x, y) = (x as usize, y as usize);
                let here = dx == 0 && dy == 0;
                let p = if here { o.pos } else { Map::tile_center(x, y) };
                if !here && (!self.passable_for(o, p) || self.clutch_at.contains_key(&(x, y))) {
                    continue;
                }
                let k = self.map.idx(x, y);
                let g = &self.map.ground()[k];
                let stress = super::environment::nest_stress(
                    g,
                    &self.map.tiles[k],
                    g.base_temp + self.season_offset,
                    self.map.climate_temp,
                    warm_eggs,
                    &bal,
                );
                // Round so that a hair's difference doesn't send her away.
                let key = ((stress * 1000.0).round() / 1000.0, dx * dx + dy * dy, p);
                if best.is_none_or(|(s, d, _)| (key.0, key.1) < (s, d)) {
                    best = Some(key);
                }
            }
        }
        best.map_or(o.pos, |(_, _, p)| p)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_newborn(
        &mut self,
        genome: impl Into<std::sync::Arc<Genome>>,
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
        // A migrator's young share their mother's summer range; young are
        // born into their mother's group (if she is the same player's).
        if let Some(&mi) = self.index.get(&mother) {
            let m = &self.organisms[mi];
            o.range = m.range;
            if m.owner == owner {
                o.group = m.group;
            }
        }
        o.growth = growth;
        o.needs.hunger = 30.0;
        self.insert(o)
    }
}

/// Ice over water too deep for `body` to stand in or swim if the ice
/// went: a tile a migrating group keeps off, since it drowns there at the
/// melt. The water test is `terrain::passable`'s.
/// How much slower a deep sleeper with fat left gets thirsty
/// (`Balance::dormant_water`).
pub(crate) const DORMANT_WATER: f32 = 0.8;

/// Most an obligate carnivore's hunger slows (`Balance::carnivore_energetics`).
pub(crate) const OBLIGATE_DISCOUNT: f32 = 0.3;

/// Hunger a player's hungry pack carnivore is spared (`Balance::player_packs`)
/// at full meat diet and pack instinct: its band's kills carry it.
pub(crate) const PACK_LARDER: f32 = 0.7;
/// Hunger at which the band's kills start to carry a pack hunter
/// ([`PACK_LARDER`]); fully from `LARDER_FROM + LARDER_SPAN`.
pub(crate) const LARDER_FROM: f32 = 20.0;
/// Hunger over which the larder grows to full.
pub(crate) const LARDER_SPAN: f32 = 30.0;
/// A player's band up to this many animals shares every kill: the full
/// larder. Above it the share fades, to none at [`BAND_CROWD`]: a big
/// population competes for its kills rather than sharing them (a strong
/// pack design took 42% of six players' animals with the larder at every
/// size; fading out by 150, the sweep's sample pack hunters cost the
/// coexistence score more than by 100).
pub(crate) const BAND_SMALL: f32 = 60.0;
/// Head count at which a player's pack no longer shares as a band.
pub(crate) const BAND_CROWD: f32 = 100.0;
/// With `Balance::prey_larder`: prey per hunter (plant eaters not the
/// player's, per pack hunter of the player's) from which the larder is
/// full, and at which it is gone: a pack grows as far as the prey there is
/// carries it, and no further (playtest 6: a pack capped near 100 by the
/// band rule couldn't compete; playtest 3: a larder at any size let a
/// strong pack overshoot).
pub(crate) const PREY_FULL: f32 = 40.0;
/// Prey per hunter at which the larder is gone ([`PREY_FULL`]). At 20 and 8
/// a strong pack overshot (ApexHunter2's wolves took 33% of six players'
/// animals).
pub(crate) const PREY_NONE: f32 = 20.0;

/// The share of hunger `o` is spared as a player's pack hunter whose
/// owner has `room` for it (0–1, `World::refresh_pack_room`: by the prey
/// per hunter with `Balance::prey_larder`, else by the band's size): in
/// lean times its band's kills carry it ([`PACK_LARDER`], from hunger
/// [`LARDER_FROM`] to full at `LARDER_FROM + LARDER_SPAN`), by its meat gut
/// and pack instinct. A fed one is spared nothing; 0 for wild animals,
/// loners and with the rule off.
pub(crate) fn player_pack_larder(bal: &crate::Balance, o: &Organism, room: f32) -> f32 {
    if !super::decide::player_pack(bal, o) {
        return 0.0;
    }
    let lean = ((o.needs.hunger - LARDER_FROM) / LARDER_SPAN).clamp(0.0, 1.0);
    let small = room.clamp(0.0, 1.0);
    PACK_LARDER
        * lean
        * small
        * bal.player_packs.clamp(0.0, 1.0)
        * o.traits.meat_efficiency(bal.generalist_tax)
        * super::decide::pack_instinct(o)
}

/// How hard the times are for the lean economy (`Balance::carnivore_energetics`):
/// 0 at hunger 40 or below, 1 at 80 and above.
pub(crate) fn lean_times(hunger: f32) -> f32 {
    ((hunger - 40.0) / 40.0).clamp(0.0, 1.0)
}

/// The lean economy's share in a cohort, which has no hunger of its own.
pub(crate) const COHORT_LEAN: f32 = 0.5;

/// How obligate a carnivore is: 0 at carnivory 0.7 or below, 1 at 0.95 and
/// above.
pub(crate) fn obligacy(carnivory: f32) -> f32 {
    ((carnivory - 0.7) / 0.25).clamp(0.0, 1.0)
}

pub(super) fn ice_trap(body: &crate::stats::Body, t: &crate::terrain::Tile) -> bool {
    t.ice > 0.0
        && t.water_depth > 0.0
        && t.water_depth > body.height.max(0.1) * 0.8
        && body.swim < 0.25 + 0.5 * t.current
}

/// Daily old-age death hazard at `frac` of a `lifespan_days` lifespan (up
/// to 130%, past which every animal dies). With `spread` 0, the old rule:
/// `4 (frac − 0.8)` from 80%, so a cohort dies within days of 84%. With a
/// spread (`Balance::age_spread`, the standard deviation of the age at
/// death over the lifespan), a hazard rising linearly from an earlier onset
/// at a rate set by the lifespan: ages at death then follow a Rayleigh
/// curve with mean 85% of the lifespan and that standard deviation, so a
/// boom cohort dies over months.
pub(crate) fn old_age_hazard(frac: f32, lifespan_days: f32, spread: f32) -> f32 {
    if spread <= 0.0 {
        return if frac > 0.8 { 4.0 * (frac - 0.8) } else { 0.0 };
    }
    // Rayleigh: sd = 0.655 σ, mean = onset + 1.2533 σ.
    let sigma = spread / 0.655;
    let onset = 0.85 - 1.2533 * sigma;
    if frac <= onset {
        0.0
    } else {
        (frac - onset) / (sigma * sigma * lifespan_days.max(1.0))
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
            crate::stats::derive(
                &Genome::uniform(
                    &crate::FounderDesign::new("x", crate::Thermo::Cold, BirthType::AbandonedEggs, litter)
                        .with(Gene::BodySize, size)
                        .values(),
                    1,
                )
                .phenotype(),
            )
        };
        let frog = traits(10, 0.12);
        let pair = traits(2, 0.5);
        let k = crate::Balance::default().larval_competition;
        let (frog_h, pair_h) =
            (larval_hazard(20, frog.pace_of_life(0.12), k), larval_hazard(20, pair.pace_of_life(0.5), k));
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
        // Summed over a few maps: how crowded the founders stay depends on
        // the ground around their home.
        let deaths_on = |seed: u64, competition: f32| {
            let mut config = WorldConfig { seed, width: 48, height: 48, ..Default::default() };
            config.balance.larval_competition = competition;
            // All 80 in one crowd, as founded before `Balance::founding_food`.
            config.balance.founding_food = 0.0;
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
        let deaths = |competition: f32| (4..8).map(|seed| deaths_on(seed, competition)).sum::<u64>();
        let (with, without) = (deaths(18.0), deaths(0.0));
        assert!(with > without + 20, "deaths with competition {with}, without {without}");
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
            father: o.genome.clone(),
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

    /// Old age with a spread (`Balance::age_spread`): ages at death average
    /// about 85% of the lifespan with that standard deviation; without it,
    /// the old rule packs them within days of 84%.
    #[test]
    fn old_age_deaths_spread_with_age_spread() {
        let stats = |spread: f32| {
            let (lifespan, dt) = (240.0f32, 0.05f32);
            // Survival over time, deaths by day: mean and sd of the age at death.
            let (mut alive, mut m1, mut m2, mut t) = (1.0f64, 0.0f64, 0.0f64, 0.0f32);
            while t < 1.3 * lifespan && alive > 1e-9 {
                let h = old_age_hazard(t / lifespan, lifespan, spread) as f64 * dt as f64;
                let d = alive * h.min(1.0);
                alive -= d;
                m1 += d * (t / lifespan) as f64;
                m2 += d * ((t / lifespan) as f64).powi(2);
                t += dt;
            }
            m1 += alive * 1.3;
            m2 += alive * 1.69;
            (m1, (m2 - m1 * m1).max(0.0).sqrt())
        };
        let (mean0, sd0) = stats(0.0);
        assert!((mean0 - 0.84).abs() < 0.02 && sd0 < 0.04, "{mean0} {sd0}");
        let (mean, sd) = stats(0.12);
        assert!((mean - 0.85).abs() < 0.02 && (sd - 0.12).abs() < 0.02, "{mean} {sd}");
    }

    /// The soft lineage cap (`Balance::lineage_crowding`): a player lineage
    /// at its soft cap still breeds, at half pace; its hunger rises past it;
    /// juveniles count at `crowding_juveniles`; the hard ceiling is
    /// `crowding_ceiling` times the cap. Off, the old hard wall is back and
    /// the wild is never crowded.
    #[test]
    fn player_lineages_crowd_softly_toward_a_higher_ceiling() {
        let mut w = presets::standard_world(WorldConfig { seed: 3, width: 64, height: 64, ..Default::default() }, 1);
        let herd = w.lineages.iter().position(|l| l.name == "Herd grazers").unwrap() as u32;
        let owner = w.lineages[herd as usize].owner;
        let n = w.organisms.iter().filter(|o| o.alive && o.owner == owner).count();
        assert!(w.organisms.iter().filter(|o| o.owner == owner).all(|o| o.matured));
        w.config.lineage_cap = n;
        let ceiling = (n as f32 * w.config.balance.crowding_ceiling).round() as usize;
        assert_eq!(w.player_ceiling(), ceiling);
        assert_eq!(w.cap_usage(herd), (n, ceiling));
        w.refresh_crowding();
        let [litter, hunger] = w.lineage_crowding(herd);
        assert!((litter - 0.5).abs() < 1e-4 && (hunger - 1.0).abs() < 1e-6, "{litter} {hunger}");
        let wild = w.lineages.iter().position(|l| l.owner == crate::WILD).unwrap() as u32;
        assert_eq!(w.lineage_crowding(wild), [1.0, 1.0]);
        // Past the soft cap: fewer young, hungrier.
        let [l2, h2] = w.crowding_effect(1.3 * n as f32);
        let want = 1.0 + 0.3 * w.config.balance.crowding_hunger;
        assert!(l2 < 0.2 && (h2 - want).abs() < 1e-4, "{l2} {h2}");
        // Juveniles count a quarter.
        let i = w.organisms.iter().position(|o| o.owner == owner).unwrap();
        w.organisms[i].matured = false;
        let c = w.crowding_counts()[&owner];
        assert!((c - (n as f32 - 0.75)).abs() < 1e-4);
        // At the soft cap a mother still gives birth.
        let mother = w.organisms.iter().position(|o| o.alive && o.lineage == herd && o.sex == Sex::Female).unwrap();
        make_pregnant(&mut w, mother);
        let mut rng = crate::rng::rng_for(1, 0, 0, 0);
        let before = w.counters.births;
        for _ in 0..4 {
            let m = w
                .organisms
                .iter()
                .position(|o| o.alive && o.lineage == herd && o.sex == Sex::Female && o.pregnancy.is_none())
                .unwrap();
            make_pregnant(&mut w, m);
            w.give_birth(m, &mut rng);
        }
        w.give_birth(mother, &mut rng);
        assert!(w.counters.births > before, "births go on at the soft cap");
        // Off: the hard wall at the cap.
        w.config.balance.lineage_crowding = 0.0;
        w.refresh_crowding();
        assert_eq!(w.lineage_crowding(herd), [1.0, 1.0]);
        assert_eq!(w.player_ceiling(), n);
    }

    /// Playtest 9: a herd held under the cap in era 1 ran away in era 2 as
    /// the wild shrank. With `Balance::world_crowding` a player lineage
    /// also crowds at that share of the world's crowding count (the
    /// world's taken as at least the soft cap), whichever comes first.
    #[test]
    fn a_lineage_crowds_at_its_share_of_the_world() {
        let w = World::new(WorldConfig { width: 32, height: 32, lineage_cap: 1500, ..Default::default() });
        let share = w.config.balance.world_crowding;
        assert_eq!(share, 0.2);
        // 600 of 2,000: 1.5 times its share, crowded hard though under the cap.
        let [litter, hunger] = w.crowding_effect_in(600.0, 2000.0);
        // Breeding only (`Balance::share_litters`, playtest 10): hunger
        // comes from the soft cap alone.
        assert!(litter < 0.1 && hunger == 1.0, "{litter} {hunger}");
        let mut hungry = w.clone();
        hungry.config.balance.share_litters = 0.0;
        assert!(hungry.crowding_effect_in(600.0, 2000.0)[1] > 1.2);
        assert!(w.crowding_effect(600.0)[0] > 0.99);
        // At its share: half the litter, as at the soft cap.
        let [at, _] = w.crowding_effect_in(400.0, 2000.0);
        assert!((at - 0.5).abs() < 1e-4, "{at}");
        // A small lineage in a small world: the world counts as the soft cap.
        assert!(w.crowding_effect_in(150.0, 150.0)[0] > 0.98);
        let mut off = w.clone();
        off.config.balance.world_crowding = 0.0;
        assert_eq!(off.crowding_effect_in(600.0, 2000.0), off.crowding_effect(600.0));
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

    /// A warm-blooded live bearer conceives again only once her litter is
    /// `lactation` of the way to weaning, on top of her recovery; a
    /// cold-blooded live bearer recovers as before.
    #[test]
    fn nursing_mothers_wait_for_weaning() {
        let mut w = wild_world(5000);
        w.config.balance.breeding_density = 0.0;
        w.config.balance.lactation = 0.5;
        let cold = crate::FounderDesign::new("cold", crate::Thermo::Cold, BirthType::Live, 2);
        w.found_lineage_unchecked(1, &cold.founders(10));
        let tpd = w.config.ticks_per_day as f32;
        let mut rng = crate::rng::rng_for(1, 0, 0, 0);
        let mut ready_after = |w: &mut World, lineage: u32| {
            let i = w.organisms.iter().position(|o| o.lineage == lineage && o.sex == Sex::Female).unwrap();
            make_pregnant(w, i);
            let born = w.counters.births;
            w.give_birth(i, &mut rng);
            assert!(w.counters.births > born);
            let o = &w.organisms[i];
            (o.breed_ready_tick - w.tick, o.traits)
        };
        let (antelope, t) = ready_after(&mut w, 1);
        assert!(t.warm && t.weaning_days() > 5.0, "weaning {} days", t.weaning_days());
        assert_eq!(antelope, ((t.recovery_days + 0.5 * t.weaning_days()) * tpd) as u64);
        let (cold, t) = ready_after(&mut w, 2);
        assert!(!t.warm);
        assert_eq!(cold, (t.recovery_days * tpd) as u64);
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
        // Founders vary in group tolerance, so look at the colony's mean.
        let colony: Vec<usize> = (0..w.organisms.len()).filter(|&i| w.organisms[i].lineage == 0).collect();
        let crowded = colony.iter().map(|&i| w.breeding_suppression(i)).sum::<f32>() / colony.len() as f32;
        assert!(crowded < 0.7, "mean suppression in a colony of 40: {crowded}");
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

    /// Prey that saw the hunter but kept drinking is only partly caught off
    /// guard (`alert_guard`); prey that saw nothing fully; fleeing prey not
    /// at all.
    #[test]
    fn prey_with_a_threat_in_sight_is_partly_ambushed() {
        let mut w = presets::standard_world(WorldConfig { seed: 3, width: 48, height: 48, ..Default::default() }, 1);
        let mut o = w.organisms.iter().find(|o| o.lineage == 1).unwrap().clone();
        let ambush = w.config.balance.ambush;
        (o.action, o.needs.safety) = (Action::Drink, 0.0);
        assert_eq!(w.ambush_on(&o), ambush);
        o.needs.safety = 40.0;
        assert!((w.ambush_on(&o) - ambush * (1.0 - w.config.balance.alert_guard)).abs() < 1e-6);
        o.action = Action::Flee;
        assert_eq!(w.ambush_on(&o), 0.0);
        w.config.balance.alert_guard = 0.0;
        (o.action, o.needs.safety) = (Action::Rest, 40.0);
        assert_eq!(w.ambush_on(&o), ambush, "alert_guard 0: as before");
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

    /// A breeding female of `lineage` in the standard world, about to lay.
    fn layer(lineage: u32, birth: BirthType) -> (World, usize) {
        let mut w = presets::standard_world(WorldConfig { seed: 3, width: 64, height: 64, ..Default::default() }, 1);
        w.config.balance.breeding_density = 0.0;
        let i = w.organisms.iter().position(|o| o.lineage == lineage && o.sex == Sex::Female).unwrap();
        assert_eq!(w.organisms[i].traits.birth, birth);
        (w.organisms[i].matured, w.organisms[i].growth) = (true, 1.0);
        make_pregnant(&mut w, i);
        (w, i)
    }

    /// Hold the tile under `pos` at a steady `temp` (its annual mean too, so
    /// a sheltered site damps nothing).
    fn set_climate(w: &mut World, pos: Vec2, temp: f32) {
        let climate = w.map.climate_temp;
        w.map.tile_at_mut(pos).temp = temp;
        let (x, y) = w.map.tile_coords(pos);
        let k = w.map.idx(x, y);
        w.map.edit_terrain().ground[k].base_temp = temp - climate;
    }

    /// Doc: eggs "can be raided, chilled or overheated". Abandoned
    /// cold-blooded eggs in steady 40 °C heat cook within a day
    /// (`egg_overheat`); without the rule they would sit there unharmed.
    #[test]
    fn abandoned_eggs_overheat_in_the_heat() {
        for rate in [0.02, 0.0] {
            let (mut w, i) = layer(6, BirthType::AbandonedEggs);
            w.config.balance.egg_overheat = rate;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.mother == id).expect("a clutch");
            let (pos, lineage, n) = (w.clutches[k].pos, w.clutches[k].lineage, w.clutches[k].eggs.len() as u64);
            w.clutches[k].hatch_tick = u64::MAX;
            set_climate(&mut w, pos, 40.0);
            for _ in 0..24 {
                w.update_clutches(&mut rng);
            }
            let e = w.counters.eggs[&lineage];
            assert_eq!(e.chilled, 0);
            if rate > 0.0 {
                assert_eq!(e.overheated, n, "6 °C over the ceiling for a day kills the clutch");
                assert!(w.clutches.iter().all(|c| c.mother != id));
            } else {
                assert_eq!(e.overheated, 0);
                assert!(w.clutches.iter().any(|c| c.mother == id && c.overheat == 0.0));
            }
        }
    }

    /// Egg predators search where clutches are dense (`nest_predation`): a
    /// lone abandoned clutch is never found, one among eight others within
    /// two tiles is eaten whole within days, and a brooder's clutch in the
    /// same crowd is guarded.
    #[test]
    fn crowded_abandoned_clutches_are_found_by_egg_predators() {
        for (rate, crowded, guarded) in
            [(0.06, false, false), (0.06, true, false), (0.0, true, false), (0.06, true, true)]
        {
            let (mut w, i) = layer(6, BirthType::AbandonedEggs);
            w.config.balance.nest_predation = rate;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.mother == id).expect("a clutch");
            w.clutches[k].hatch_tick = u64::MAX;
            if guarded {
                w.clutches[k].guardian = Some(id);
            }
            // Away from the map's edge.
            let (cx, cy) = (32, 32);
            let pos = Map::tile_center(cx, cy);
            w.clutch_at.remove(&w.map.tile_coords(w.clutches[k].pos));
            w.clutch_at.insert((cx, cy), k);
            w.clutches[k].pos = pos;
            let lineage = w.clutches[k].lineage;
            for (dx, dy) in [(-1i64, -1i64), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                let (x, y) = ((cx as i64 + 2 * dx) as usize, (cy as i64 + dy) as usize);
                let at = Map::tile_center(x, y);
                set_climate(&mut w, at, 20.0);
                if crowded {
                    let mut c = w.clutches[k].clone();
                    (c.pos, c.mother, c.guardian) = (at, u64::MAX, None);
                    w.clutch_at.insert((x, y), w.clutches.len());
                    w.clutches.push(c);
                }
            }
            set_climate(&mut w, pos, 20.0);
            if crowded {
                assert_eq!(w.clutches_near(pos), 8);
            }
            for _ in 0..24 * 10 {
                if guarded {
                    w.organisms[i].pos = pos;
                }
                w.update_clutches(&mut rng);
            }
            let e = w.counters.eggs[&lineage];
            assert_eq!((e.chilled, e.overheated), (0, 0));
            let survives = w.clutches.iter().any(|c| c.mother == id);
            assert_eq!(
                survives,
                rate == 0.0 || !crowded || guarded,
                "rate {rate}, crowded {crowded}, guarded {guarded}"
            );
            assert_eq!(e.raided > 0, rate > 0.0 && crowded, "rate {rate}, crowded {crowded}");
        }
    }

    /// A sitting parent shades her eggs in the heat, at a cost in water,
    /// but only by `BROOD_SHADE`: at 42 °C warm-blooded eggs stay at their
    /// 39 °C ceiling under her and cook without her.
    #[test]
    fn brooders_shade_their_eggs_at_a_cost() {
        for sitting in [true, false] {
            let (mut w, i) = layer(7, BirthType::IncubatedEggs);
            // The brooding itself: a big clutch's cover is tested apart.
            w.config.balance.clutch_cover = 0.0;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.guardian == Some(id)).expect("a guarded clutch");
            let pos = w.clutches[k].pos;
            w.clutches[k].hatch_tick = u64::MAX;
            set_climate(&mut w, pos, 42.0);
            if !sitting {
                w.organisms[i].pos = pos + Vec2::new(3.0, 0.0);
            }
            w.organisms[i].needs.thirst = 0.0;
            for _ in 0..24 {
                w.update_clutches(&mut rng);
            }
            let thirst = w.organisms[i].needs.thirst;
            match w.clutches.iter().find(|c| c.guardian == Some(id)) {
                Some(c) => {
                    assert!(sitting && c.overheat == 0.0 && c.chill == 0.0);
                    assert!(thirst > 0.0, "shading costs water");
                }
                None => assert!(!sitting && thirst == 0.0),
            }
        }
    }

    /// Clutch cover (`Balance::clutch_cover`): a sitting parent keeps a
    /// clutch of 4 warm on a cool nest, while a bigger clutch chills under
    /// her; with the rule off both stay warm.
    #[test]
    fn a_big_clutch_chills_under_its_parent() {
        let chill = |eggs: usize, cover: f32| {
            let (mut w, i) = layer(7, BirthType::IncubatedEggs);
            w.config.balance.clutch_cover = cover;
            // The averaging rule `clutch_split` replaces.
            w.config.balance.clutch_split = 0.0;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.guardian == Some(id)).expect("a guarded clutch");
            let pos = w.clutches[k].pos;
            w.clutches[k].hatch_tick = u64::MAX;
            let egg = w.clutches[k].eggs[0].clone();
            w.clutches[k].eggs = vec![egg; eggs];
            set_climate(&mut w, pos, 10.0);
            w.organisms[i].pos = pos;
            w.update_clutches(&mut rng);
            w.clutches.iter().find(|c| c.guardian == Some(id)).map_or(f32::MAX, |c| c.chill)
        };
        assert_eq!(chill(4, 2.1), 0.0);
        assert!(chill(6, 2.1) > 0.0);
        assert_eq!(chill(6, 0.0), 0.0);
    }

    /// Playtest 8: a warm incubated design with litter 8 hatched nothing.
    /// With `Balance::clutch_split` a brooding parent keeps 4 eggs warm on
    /// a cool nest and the rest chill one by one: a clutch of 8 keeps its
    /// 4, while the old averaging chilled all 8.
    #[test]
    fn a_big_clutch_keeps_the_eggs_its_parent_warms() {
        let run = |eggs: usize, split: f32| {
            let (mut w, i) = layer(7, BirthType::IncubatedEggs);
            w.config.balance.clutch_split = split;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.guardian == Some(id)).expect("a guarded clutch");
            let pos = w.clutches[k].pos;
            w.clutches[k].hatch_tick = u64::MAX;
            let egg = w.clutches[k].eggs[0].clone();
            w.clutches[k].eggs = vec![egg; eggs];
            set_climate(&mut w, pos, 10.0);
            for h in 0..24 * 10 {
                w.tick = h;
                w.organisms[i].pos = pos;
                w.organisms[i].alive = true;
                w.organisms[i].needs.hunger = 0.0;
                w.organisms[i].needs.thirst = 0.0;
                w.update_clutches(&mut rng);
            }
            let left = w.clutches.iter().find(|c| c.guardian == Some(id)).map_or(0, |c| c.eggs.len());
            let chilled = w.counters.eggs.values().map(|e| e.chilled).sum::<u64>();
            (left, chilled)
        };
        assert_eq!(run(4, 1.0), (4, 0));
        assert_eq!(run(8, 1.0), (4, 4));
        let (left, chilled) = run(8, 0.0);
        assert_eq!(left, 0, "the old averaging: the whole clutch chills");
        assert_eq!(chilled, 8);
    }

    /// Attended eggs (`Balance::egg_attendance`): a clutch whose brooding
    /// parent is off the nest but near chills a fifth as fast as one whose
    /// parent is far away.
    #[test]
    fn attended_eggs_chill_slower() {
        let chill = |away: f32, attendance: f32| {
            let (mut w, i) = layer(7, BirthType::IncubatedEggs);
            w.config.balance.egg_attendance = attendance;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.guardian == Some(id)).expect("a guarded clutch");
            let pos = w.clutches[k].pos;
            w.clutches[k].hatch_tick = u64::MAX;
            set_climate(&mut w, pos, 5.0);
            w.organisms[i].pos = pos + Vec2::new(away, 0.0);
            w.update_clutches(&mut rng);
            w.clutches.iter().find(|c| c.guardian == Some(id)).map_or(f32::MAX, |c| c.chill)
        };
        let (near, far, off) = (chill(5.0, 1.0), chill(30.0, 1.0), chill(5.0, 0.0));
        assert!(near > 0.0 && (near - 0.2 * far).abs() < 1e-4 && (off - far).abs() < 1e-6, "{near} {far} {off}");
    }

    /// A warm-blooded brooder lifts her eggs by at most `brood_warmth`
    /// above the nest's air, and every degree costs her food
    /// (`brood_cost`): on a −10 °C nest the eggs chill under her, and she
    /// grows hungry faster the more she warms them.
    #[test]
    fn brooding_in_the_cold_is_limited_and_costly() {
        let mut hunger = Vec::new();
        for (warmth, temp) in [(24.0, -10.0), (100.0, -10.0), (24.0, 20.0)] {
            let (mut w, i) = layer(7, BirthType::IncubatedEggs);
            w.config.balance.brood_warmth = warmth;
            w.config.balance.clutch_cover = 0.0;
            let mut rng = crate::rng::rng_for(1, 0, 0, 0);
            w.give_birth(i, &mut rng);
            let id = w.organisms[i].id;
            let k = w.clutches.iter().position(|c| c.guardian == Some(id)).expect("a guarded clutch");
            let pos = w.clutches[k].pos;
            w.clutches[k].hatch_tick = u64::MAX;
            set_climate(&mut w, pos, temp);
            w.organisms[i].needs.hunger = 0.0;
            w.update_clutches(&mut rng);
            let c = w.clutches.iter().find(|c| c.guardian == Some(id)).unwrap();
            assert_eq!(c.chill > 0.0, warmth < 32.0 - temp, "warmth {warmth} at {temp} °C: chill {}", c.chill);
            hunger.push(w.organisms[i].needs.hunger);
        }
        // Warming by 24 °C costs twice warming by 12 °C; by 42 °C, more still.
        assert!((hunger[0] / hunger[2] - 2.0).abs() < 1e-3, "{hunger:?}");
        assert!(hunger[1] > hunger[0]);
    }

    /// Abandoned eggs rely on their mother's choice of site: in a hot
    /// summer she lays cold-blooded eggs in the shade of a canopy nearby
    /// rather than on the open ground where she stands.
    #[test]
    fn abandoned_eggs_go_to_the_coolest_site_in_summer() {
        let (mut w, i) = layer(6, BirthType::AbandonedEggs);
        let pos = w.organisms[i].pos;
        let (cx, cy) = w.map.tile_coords(pos);
        let here = w.map.tile_at(pos).clone();
        let ground = w.map.ground_at(pos).clone();
        let r = NEST_SEARCH as i64;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = ((cx as i64 + dx).clamp(0, 63) as usize, (cy as i64 + dy).clamp(0, 63) as usize);
                let idx = w.map.idx(x, y);
                w.map.tiles[idx] = crate::terrain::Tile { water_depth: 0.0, ..here.clone() };
                w.map.edit_terrain().ground[idx] =
                    crate::terrain::Ground { base_temp: 26.0, canopy: 0.0, ..ground.clone() };
            }
        }
        let shade = ((cx as i64 + 2).clamp(0, 63) as usize, cy);
        let idx = w.map.idx(shade.0, shade.1);
        w.map.edit_terrain().ground[idx].canopy = 1.0;
        assert!(w.passable_for(&w.organisms[i], Map::tile_center(shade.0, shade.1)));
        w.season_offset = 12.0 + w.map.climate_temp;
        assert_eq!(w.map.tile_coords(w.nest_site(i, false)), shade);
        // In a cool season the open ground where she stands will do.
        w.season_offset = -8.0 + w.map.climate_temp;
        assert_eq!(w.map.tile_coords(w.nest_site(i, false)), (cx, cy));
    }
}
