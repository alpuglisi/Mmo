//! Derived effects of the body plan: ears, legs, tail shape and snout
//! (`docs/morphology.md` §3, work package D; vision is package V's).
//!
//! Everything here is computed once per organism, at birth, from its
//! phenotype and its [`BodyPlan`] (the contract's expressed count, shape and
//! placement parameters; sizes stay the existing `Ears`, `LegLength`, `Tail`
//! genes). [`derive`] gives every derived number and stat change as a
//! [`MorphEffects`]; [`apply`] folds them into the organism's [`Traits`]
//! (stats and body) and fills the few context terms the tick reads
//! ([`MorphContext`]).
//!
//! Two rules make this safe to merge before calibration:
//!
//! - **Neutral at the default body.** At the default morphology (2 round
//!   ears, 4 legs, a thin tail, snout 0.5) every multiplier is exactly 1 and
//!   every additive term exactly 0, whatever the size genes, so applying the
//!   effects to a default body changes no bit.
//! - **Behind tunables that start at 0.** [`Balance::morph_effects`] scales
//!   every count/shape/snout effect; the three new downsides of today's genes
//!   (tail drag, tail chill, long-leg tippiness and digging) have their own
//!   knobs. With all four at 0 (the default) [`apply`] returns at once.
//!
//! Hearing is omnidirectional, so it is what fills visual blind spots; the
//! vision package reads [`MorphContext::hearing`] and
//! [`MorphContext::localization`] and places a threat detected by hearing
//! alone with [`heard_bearing`].

use crate::config::Balance;
use crate::genome::{Gene, Phenotype};
use crate::stats::Traits;

/// Ear shape (contract `ear_shape`). Dominance order: round > pointed > long
/// > fan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EarShape {
    Round,
    Pointed,
    Long,
    Fan,
}

impl EarShape {
    pub const ALL: [EarShape; 4] = [EarShape::Round, EarShape::Pointed, EarShape::Long, EarShape::Fan];

    pub fn name(self) -> &'static str {
        match self {
            EarShape::Round => "round",
            EarShape::Pointed => "pointed",
            EarShape::Long => "long",
            EarShape::Fan => "fan",
        }
    }

    /// Hearing gain.
    pub fn gain(self) -> f32 {
        match self {
            EarShape::Round => 1.0,
            EarShape::Pointed => 1.1,
            EarShape::Long => 1.2,
            EarShape::Fan => 1.3,
        }
    }

    /// Base localization `L0` of one pair.
    pub fn localization(self) -> f32 {
        match self {
            EarShape::Round => 0.4,
            EarShape::Pointed => 0.7,
            EarShape::Long => 0.6,
            EarShape::Fan => 0.5,
        }
    }

    /// Radiating area per unit of ear size and pair, `A_s`.
    pub fn area(self) -> f32 {
        match self {
            EarShape::Round => 1.0,
            EarShape::Pointed => 1.1,
            EarShape::Long => 1.5,
            EarShape::Fan => 2.0,
        }
    }
}

/// Tail shape (contract `tail_shape`). Dominance order: thin > bushy >
/// prehensile > fin > club.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TailShape {
    Thin,
    Bushy,
    Fin,
    Club,
    Prehensile,
}

impl TailShape {
    pub const ALL: [TailShape; 5] =
        [TailShape::Thin, TailShape::Bushy, TailShape::Fin, TailShape::Club, TailShape::Prehensile];

    pub fn name(self) -> &'static str {
        match self {
            TailShape::Thin => "thin",
            TailShape::Bushy => "bushy",
            TailShape::Fin => "fin",
            TailShape::Club => "club",
            TailShape::Prehensile => "prehensile",
        }
    }

    /// Agility per unit of tail length (today's thin tail: 0.3).
    pub fn agility(self) -> f32 {
        match self {
            TailShape::Thin => 0.3,
            TailShape::Bushy | TailShape::Prehensile => 0.2,
            TailShape::Fin => 0.1,
            TailShape::Club => 0.05,
        }
    }
}

/// Foot type, derived from the existing `Hooves`, `Claws` and `FootWidth`
/// genes (contract: "derived"): the strongest of the three when it reaches
/// 0.4, else plain. A readout only: the genes themselves drive the rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foot {
    Plain,
    Hooves,
    Claws,
    Webbed,
}

impl Foot {
    pub fn of(p: &Phenotype) -> Foot {
        let (h, c, w) = (p[Gene::Hooves], p[Gene::Claws], p[Gene::FootWidth]);
        let top = h.max(c).max(w);
        if top < 0.4 {
            Foot::Plain
        } else if top == h {
            Foot::Hooves
        } else if top == c {
            Foot::Claws
        } else {
            Foot::Webbed
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Foot::Plain => "plain",
            Foot::Hooves => "hooves",
            Foot::Claws => "claws",
            Foot::Webbed => "webbed",
        }
    }
}

/// The contract's expressed body-plan parameters (`docs/morphology-contract.md`)
/// that this package reads. Sizes are the existing genes (`Eyes`, `Ears`,
/// `LegLength`, `Tail`), read from the phenotype.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPlan {
    /// 1–6 (read by the vision package, carried here for the readout).
    pub eye_count: u8,
    /// Degrees, 0–180 (vision package).
    pub eye_azimuth: f32,
    /// 0, 2 or 4.
    pub ear_count: u8,
    pub ear_shape: EarShape,
    /// 0, 2, 4, 6 or 8.
    pub leg_count: u8,
    pub tail_present: bool,
    pub tail_shape: TailShape,
    /// 0 (flat face) – 1 (long muzzle).
    pub snout_length: f32,
}

impl BodyPlan {
    /// The default (wild-type) body plan, with a tail.
    pub const DEFAULT: BodyPlan = BodyPlan {
        eye_count: 2,
        eye_azimuth: 60.0,
        ear_count: 2,
        ear_shape: EarShape::Round,
        leg_count: 4,
        tail_present: true,
        tail_shape: TailShape::Thin,
        snout_length: 0.5,
    };

    /// Leg pairs, `n = leg_count / 2` (capped at 4).
    pub fn leg_pairs(&self) -> u8 {
        (self.leg_count / 2).min(4)
    }
}

/// Whether a `Tail` gene value grows a tail (contract: `Tail` ≥ 0.05).
pub fn tail_present(tail: f32) -> bool {
    tail >= 0.05
}

/// THE accessor for an organism's body plan. Every rule in this module
/// reads morphology through it.
///
/// Until the genome package's morphology loci land, every organism has the
/// contract defaults (2 eyes at 60°, 2 round ears, 4 legs, a thin tail when
/// `Tail` ≥ 0.05, snout 0.5). Swapping in the expressed `Morphology` is a
/// change to this function only.
pub fn body_plan(p: &Phenotype) -> BodyPlan {
    BodyPlan { tail_present: tail_present(p[Gene::Tail]), ..BodyPlan::DEFAULT }
}

/// The tunables, as one value (`Balance::morph_*`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    /// Every count, shape and snout effect (`Balance::morph_effects`).
    pub effects: f32,
    /// Tail drag in burrows and dense vegetation (`Balance::morph_tail_drag`).
    pub tail_drag: f32,
    /// Heat loss of thin and fin tails (`Balance::morph_tail_chill`).
    pub tail_chill: f32,
    /// Long legs are tippy and dig worse (`Balance::morph_long_legs`).
    pub long_legs: f32,
}

impl Knobs {
    /// The spec's formulas at full strength (the readout and the fixture).
    pub const FULL: Knobs = Knobs { effects: 1.0, tail_drag: 1.0, tail_chill: 1.0, long_legs: 1.0 };
    pub const OFF: Knobs = Knobs { effects: 0.0, tail_drag: 0.0, tail_chill: 0.0, long_legs: 0.0 };

    pub fn of(b: &Balance) -> Knobs {
        Knobs {
            effects: b.morph_effects,
            tail_drag: b.morph_tail_drag,
            tail_chill: b.morph_tail_chill,
            long_legs: b.morph_long_legs,
        }
    }

    pub fn is_off(&self) -> bool {
        *self == Knobs::OFF
    }
}

/// What the tick reads of the body plan beyond stats and body, stored in
/// [`Traits::morph`]. Neutral (0, or 1 for factors) unless [`apply`] ran with
/// the tunables on; `hearing`, `smell` and `localization` are always set.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MorphContext {
    /// Hearing channel of perception (perception units; today's share is
    /// `0.03 + 0.3·Ears`). Omnidirectional: it detects in visual blind
    /// spots. Included in `Stats::perception`.
    pub hearing: f32,
    /// Smell channel of perception (`0.02 + 0.3·Nose`, times the snout
    /// factor). Included in `Stats::perception`.
    pub smell: f32,
    /// Ear localization `L`, 0–0.9 (0 with no ears). See [`heard_bearing`].
    pub localization: f32,
    /// °C added to a warm body's heat tolerance (fan ears, long snout).
    pub heat_tolerance: f32,
    /// Insulation added while resting (a warm body's bushy tail).
    pub rest_insulation: f32,
    /// Added to how hard it fights back when attacked (club tail).
    pub fight_back: f32,
    /// Factor on the escape chance of prey lighter than 0.2× this hunter's
    /// mass (snout length).
    pub small_prey_escape: f32,
}

impl MorphContext {
    /// At birth, before [`apply`]: today's channels and no effects.
    pub fn at_birth(p: &Phenotype) -> MorphContext {
        let plan = body_plan(p);
        MorphContext {
            hearing: 0.03 + 0.3 * p[Gene::Ears],
            smell: 0.02 + 0.3 * p[Gene::Nose],
            localization: localization(&plan),
            heat_tolerance: 0.0,
            rest_insulation: 0.0,
            fight_back: 0.0,
            small_prey_escape: 1.0,
        }
    }
}

/// Ear localization `L = L0(shape) + 0.2·[4 ears]`, 0 with no ears.
pub fn localization(plan: &BodyPlan) -> f32 {
    match plan.ear_count {
        0 => 0.0,
        c => plan.ear_shape.localization() + if c >= 4 { 0.2 } else { 0.0 },
    }
}

/// Largest bearing error (degrees) of a threat detected by hearing alone:
/// `70°·(1 − L)`.
pub fn bearing_error_deg(localization: f32) -> f32 {
    70.0 * (1.0 - localization)
}

/// Interface for the vision package: where an animal thinks a threat is when
/// it heard it but no eye covers its sector. `true_bearing` is in degrees
/// (any convention), `u` a deterministic draw in [-1, 1] supplied by the
/// caller (its own random stream). Returns `true_bearing + u·70°·(1 − L)`;
/// the animal flees from that bearing.
pub fn heard_bearing(true_bearing: f32, localization: f32, u: f32) -> f32 {
    true_bearing + u.clamp(-1.0, 1.0) * bearing_error_deg(localization)
}

/// Gait factor `g` and belly/leg stability `S0` by leg pairs (spec table).
fn gait_and_s0(pairs: u8) -> (f32, f32) {
    match pairs {
        0 => (0.6, 1.0),
        1 => (0.95, 0.45),
        2 => (1.0, 0.7),
        3 => (0.92, 0.85),
        _ => (0.85, 0.95),
    }
}

/// Stability of the default body (`S_ref`).
pub const STABILITY_REF: f32 = 0.7;

/// Every derived number and stat change of one body plan (at the given
/// knob strengths; [`Knobs::FULL`] for the spec's formulas). Additive
/// changes are `d_*`; factors are 1 and changes 0 at the default body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MorphEffects {
    // Ears.
    pub hearing: f32,
    pub localization: f32,
    pub bearing_error_deg: f32,
    /// Radiating area `A = Ears·(c/2)·A_s`.
    pub radiating_area: f32,
    // Legs.
    pub gait: f32,
    /// `S = S0(n)·(1 − 0.3·LegLength) + 0.15·T·[biped]`.
    pub stability: f32,
    /// `S_ref / S`: factor on ice slip and the slope speed penalty.
    pub stability_factor: f32,
    /// Effective foot area: `foot_area·n/2`, a belly of 1.5 when legless.
    pub foot_area: f32,
    pub ground_pressure_factor: f32,
    pub mass_factor: f32,
    /// The `Wings` gene (mass feeds wing loading).
    pub wings: f32,
    pub climb_factor: f32,
    // Tail.
    /// Tail length `T` (0 without a tail).
    pub tail: f32,
    /// Factor on burrowing from tail drag (`1 − 0.2·T`).
    pub burrow_factor: f32,
    // Snout.
    pub smell: f32,
    /// Factor on the fang term of strength (fang size, 0 below
    /// `stats::FANG_THRESHOLD`), `1.3 − 0.6·s`.
    pub bite_factor: f32,
    /// Binocular overlap the snout hides, `40°·max(0, s − 0.5)`
    /// (`vision::snout_occlusion`, which the eye geometry uses too).
    pub snout_occlusion_deg: f32,
    // Context terms (see `MorphContext`).
    pub heat_tolerance: f32,
    pub rest_insulation: f32,
    pub fight_back: f32,
    pub small_prey_escape: f32,
    // Stat and body changes.
    pub d_speed: f32,
    pub d_strength: f32,
    pub d_endurance: f32,
    pub d_agility: f32,
    pub d_perception: f32,
    pub d_stealth: f32,
    pub d_defense: f32,
    pub d_insulation: f32,
    /// Change of `upkeep` (metabolism is `base·(1 + upkeep)`).
    pub d_upkeep: f32,
    pub d_metabolism: f32,
    pub d_height: f32,
    /// Effective width in dense vegetation (tail drag: `T/3` there costs
    /// `0.2·T·veg_density` of speed).
    pub d_width: f32,
    pub d_swim: f32,
    /// Added to climbing after `climb_factor`.
    pub d_climb: f32,
    pub d_burrow: f32,
    pub d_grip: f32,
}

/// The body plan's effects on a body whose traits (`base`, from
/// `stats::derive`) and phenotype `p` are given.
pub fn derive(p: &Phenotype, plan: &BodyPlan, k: Knobs, base: &Traits) -> MorphEffects {
    use Gene::*;
    let km = k.effects;
    let warm = base.warm;
    let ears = p[Ears];
    let ll = p[LegLength];
    let t = if plan.tail_present { p[Tail] } else { 0.0 };
    let shape = plan.tail_shape;
    let ds = plan.snout_length - 0.5;

    // Ears.
    let c = plan.ear_count;
    let f_c = match c {
        0 => 0.0,
        2 => 1.0,
        _ => 1.15,
    };
    let hearing_today = 0.03 + 0.3 * ears;
    let d_hearing = if c == 0 { 0.01 - hearing_today } else { 0.3 * ears * (plan.ear_shape.gain() * f_c - 1.0) };
    let localization = localization(plan);
    let radiating_area = ears * (c as f32 / 2.0) * plan.ear_shape.area();
    let d_area = radiating_area - ears;

    // Legs.
    let n = plan.leg_pairs();
    let biped = n == 1;
    let (gait, s0) = gait_and_s0(n);
    let leg_terms = 0.45 * ll + 0.25 * p[Hooves];
    let d_gait = if n == 0 { 0.6 * 0.45 - 0.45 - leg_terms } else { (gait - 1.0) * leg_terms };
    let s0 = STABILITY_REF + km * (s0 - STABILITY_REF);
    let stability = s0 * (1.0 - 0.3 * ll * k.long_legs) + if biped { km * 0.15 * t } else { 0.0 };
    let stability_factor = STABILITY_REF / stability;
    let d_armor = -0.35 * p[Armor] * (stability_factor.sqrt() - 1.0);
    let foot = base.body.foot_area;
    let full_foot = if n == 0 { 1.5 } else { foot * (n as f32 / 2.0) };
    let foot_area = foot + km * (full_foot - foot);
    let wings = p[Wings];
    let mut mass = 1.0 + 0.04 * (n as f32 - 2.0) * (0.5 + ll);
    if shape == TailShape::Club {
        mass *= 1.0 + 0.15 * t;
    }
    if n < 2 {
        // Wings that replace forelimbs (or a legless winged body) don't pay
        // the extra limb set the default four-legged winged body carries.
        mass /= 1.0 + 0.15 * wings;
    }
    let mass_factor = 1.0 + km * (mass - 1.0);
    let ground_pressure_factor = mass_factor.cbrt() * (foot / foot_area);
    let climb = match n {
        0 => 0.6,
        1 => 0.5,
        2 => 1.0,
        _ => 1.2,
    };
    let climb_factor = 1.0 + km * (climb - 1.0);
    let legless = if n == 0 { 1.0 } else { 0.0 };
    let pairs_beyond = (n as f32 - 2.0).max(0.0);

    // Upkeep: ears replace `0.1·Ears` by `0.1·Ears·c/2`; legs; extra-limb
    // wings relative to the default (4 legs + wings pays them); prehensile.
    let mut upkeep = 0.1 * ears * (c as f32 / 2.0 - 1.0) + (0.05 * (n as f32 - 2.0)).max(-0.05);
    if n < 2 {
        upkeep -= 0.1 * wings;
    }
    if shape == TailShape::Prehensile {
        upkeep += 0.05 * t;
    }
    let d_upkeep = km * upkeep;
    let base_metabolism = (0.6 + 0.3 * p[Muscle]) * if warm { 1.0 } else { 0.35 };

    // Tail shape.
    let tail_agility = (shape.agility() - 0.3) * t;
    let biped_balance = if biped && t < 0.2 { -0.15 * (1.0 - t / 0.2) } else { 0.0 };
    let (mut tail_speed, mut tail_stealth, mut tail_defense, mut swim, mut grip, mut climb_add) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut rest_insulation, mut fight_back) = (0.0, 0.0);
    match shape {
        TailShape::Thin => {}
        TailShape::Bushy => {
            if warm {
                rest_insulation = km * 0.15 * t;
            }
            tail_stealth = -0.05 * t;
        }
        TailShape::Fin => {
            swim = 0.4 * t;
            tail_speed = -0.05 * t;
        }
        TailShape::Club => {
            tail_defense = 0.3 * t;
            fight_back = km * 0.3 * t;
            tail_speed = -0.05 * t;
        }
        TailShape::Prehensile => {
            climb_add = 0.3 * t;
            grip = 0.15 * t;
        }
    }
    let chill = if matches!(shape, TailShape::Thin | TailShape::Fin) { -0.05 * t * k.tail_chill } else { 0.0 };
    let burrow_factor = 1.0 - 0.2 * t * k.tail_drag;
    let d_width = t * k.tail_drag / 3.0;

    // Snout.
    let nose = p[Nose];
    let smell_delta = 0.18 * nose * ds;
    let bite_factor = 1.0 - 0.6 * ds * km;

    let d_perception = km * (d_hearing + smell_delta);
    let heat_tolerance = if warm { km * (6.0 * d_area + 3.0 * ds) } else { 0.0 };
    let d_long_legs_dig = if n > 0 { -0.2 * ll * k.long_legs } else { 0.0 };

    MorphEffects {
        hearing: hearing_today + km * d_hearing,
        localization,
        bearing_error_deg: bearing_error_deg(localization),
        radiating_area,
        gait,
        stability,
        stability_factor,
        foot_area,
        ground_pressure_factor,
        mass_factor,
        wings,
        climb_factor,
        tail: t,
        burrow_factor,
        smell: 0.02 + 0.3 * nose + km * smell_delta,
        bite_factor,
        snout_occlusion_deg: crate::vision::snout_occlusion(plan.snout_length),
        heat_tolerance,
        rest_insulation,
        fight_back,
        small_prey_escape: 1.0 - km * 0.4 * ds,
        d_speed: km * (d_gait + tail_speed) + d_armor,
        d_strength: -km * 0.18 * base.fangs * ds,
        d_endurance: -km * 0.04 * pairs_beyond,
        d_agility: km * (tail_agility + biped_balance),
        d_perception,
        d_stealth: km * (-0.04 * ears * (plan.ear_shape.area() - 1.0) + tail_stealth + 0.1 * legless),
        d_defense: km * tail_defense + 0.1 * (stability - STABILITY_REF),
        d_insulation: -km * 0.15 * d_area * (1.0 - 0.4 * p[Fur]) + chill,
        d_upkeep,
        d_metabolism: base_metabolism * d_upkeep,
        d_height: km
            * if n == 0 {
                -0.3 * p[BodySize] - 0.4 * ll
            } else if biped {
                0.1 * ll
            } else {
                0.0
            },
        d_width,
        d_swim: km * (swim + 0.15 * legless),
        d_climb: km * climb_add,
        d_burrow: km * (0.2 * legless + 0.1 * ds) + d_long_legs_dig,
        d_grip: km * grip,
    }
}

impl MorphEffects {
    /// Fold these effects into `t` (its stats, body and context). Exact at
    /// the default body: every change there is `+ 0` or `× 1`, and grip,
    /// which can't be rewritten exactly, is left alone.
    pub fn apply_to(&self, t: &mut Traits) {
        let st = &mut t.stats;
        st.speed = (st.speed + self.d_speed).max(0.08);
        st.strength += self.d_strength;
        st.endurance = (st.endurance + self.d_endurance).max(0.1);
        st.agility = (st.agility + self.d_agility).max(0.05);
        st.perception += self.d_perception;
        st.stealth = (st.stealth + self.d_stealth).clamp(0.0, 1.0);
        st.defense += self.d_defense;
        st.metabolism += self.d_metabolism;
        st.insulation = (st.insulation + self.d_insulation).clamp(0.0, 1.0);

        let b = &mut t.body;
        b.mass *= self.mass_factor;
        b.ground_pressure *= self.ground_pressure_factor;
        b.foot_area = self.foot_area;
        if self.mass_factor != 1.0 {
            // Mass feeds wing loading; same thresholds as `stats::derive`.
            b.wing_loading *= self.mass_factor;
            b.can_fly = self.wings > 0.5 && b.wing_loading < 2.0;
            b.can_glide = self.wings > 0.25 && b.wing_loading < 6.0;
        }
        b.height = (b.height + self.d_height).max(0.05);
        b.width += self.d_width;
        b.swim = (b.swim + self.d_swim).clamp(0.0, 1.0);
        b.climb = b.climb * self.climb_factor + self.d_climb;
        b.burrow = ((b.burrow + self.d_burrow) * self.burrow_factor).max(0.0);
        // Stability scales ice slip (`hoof_slip·(1 − grip)`) and the slope
        // penalty (`(1 − grip)`), the only two uses of grip.
        if self.stability_factor != 1.0 || self.d_grip != 0.0 {
            let grip = (b.grip + self.d_grip).min(1.0);
            b.grip = 1.0 - (1.0 - grip) * self.stability_factor;
        }

        t.morph.hearing = self.hearing;
        t.morph.smell = self.smell;
        t.morph.localization = self.localization;
        t.morph.heat_tolerance = self.heat_tolerance;
        t.morph.rest_insulation = self.rest_insulation;
        t.morph.fight_back = self.fight_back;
        t.morph.small_prey_escape = self.small_prey_escape;
    }
}

/// Apply the body plan's effects to a newborn's (or restored organism's)
/// traits, at the strengths `bal` sets. Call once, on traits fresh from
/// `stats::derive`. With every `morph_*` tunable at 0 (the default) it
/// changes nothing.
pub fn apply(t: &mut Traits, p: &Phenotype, bal: &Balance) {
    let k = Knobs::of(bal);
    if k.is_off() {
        return;
    }
    apply_plan(t, p, &body_plan(p), k);
}

/// [`apply`] with an explicit body plan (tests, readouts, and the genome
/// package's hook until [`body_plan`] reads its loci).
pub fn apply_plan(t: &mut Traits, p: &Phenotype, plan: &BodyPlan, k: Knobs) {
    if k.is_off() {
        return;
    }
    derive(p, plan, k, t).apply_to(t);
    // The eyes' share of the changed perception, and the plan's eyes and
    // snout (`Balance::directional_vision`). The sight channel itself is
    // unchanged: hearing and smell carry the body plan's perception terms.
    let eyes = p[Gene::Eyes];
    t.vision = crate::vision::Vision::new(
        &crate::vision::EyeLayout::of_plan(plan, eyes),
        crate::vision::sight(eyes),
        t.stats.perception,
    );
}

#[cfg(test)]
mod tests;
