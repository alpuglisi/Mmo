//! Point-cost calibration harness. Prints one CSV row per player lineage per
//! world, for analysis elsewhere (see "Point costs" in the README).
//!
//! ```text
//! cargo run --release --example calibrate -- marginal   [options]
//! cargo run --release --example calibrate -- tournament [options]
//! cargo run --release --example calibrate -- costs
//! ```
//!
//! - `marginal`: the four sample player designs are the templates. Each world
//!   founds the wild ecosystem plus one variant of every template: the design
//!   unchanged (`control`), one costed trait raised by `--delta` (capped at
//!   1), `--extra-founders` more founders, the litter one larger or smaller,
//!   or body size lowered by half of `--delta` (`smaller`). Options rotate across worlds so every template meets every
//!   option once per seed, next to a different mix of rivals. Variants are
//!   founded without the budget check, so the measured gain is the value of
//!   the change itself; `trait_cost` should be proportional to it.
//! - `tournament`: four random designs per world that each spend the whole
//!   budget under the current costs. Tests whether any archetype or trait
//!   wins out of proportion to its price.
//! - `costs`: print the current cost table.
//!
//! `--morph` adds the body plan (`docs/morphology.md` §4). In `marginal` it
//! adds one option per morphology change: eye count +1 (a midline eye), +2
//! (an extra pair) and −1, leg pairs +1, −1 and −2, ear pairs ±1, each
//! alternative ear shape, a tail of length 0.4 in each shape (`tail_thin` is
//! the baseline of the other tail shapes, since the templates have no tail),
//! `eye_azimuth` ±40° and `snout_length` ±0.4. Their `delta` is structures
//! added (eyes, or pairs), degrees, snout units, or 0 for a shape. In
//! `tournament` each random design gets a random body plan, paid for before
//! the traits. Run it with the morphology tunables on (`--set
//! morph_effects=1 --set morph_tail_drag=1 --set morph_tail_chill=1 --set
//! morph_long_legs=1 --set directional_vision=1`): prices only matter when
//! the effects do. The design column then ends with the body plan
//! (`m:eyes=… m:azimuth=… m:ears=… m:ear_shape=… m:legs=… m:tail_shape=…
//! m:snout=…`) whenever it differs from the default.
//!
//! Options: `--seeds N` (marginal; seeds `--seed0`..), `--worlds N`
//! (tournament), `--days N`, `--size N`, `--delta F`, `--extra-founders N`,
//! `--seed0 N`, `--world0 N` (tournament: first world index, to split a
//! tournament across processes; world `w` uses seed `seed0 + w`), `--only NAME,...` (marginal options to run), `--set NAME=V`
//! (override a `Balance` parameter, repeatable), `--swap-tail FROM=TO`
//! (tournament: the same designs, with every FROM tail shape drawn turned
//! into TO, for a paired test of one free shape against another), `--deaths` (print each
//! player lineage's deaths by cause to stderr: `deaths,seed,slot,cause=n,...`).
//!
//! Columns: `mode,seed,world,slot,template,option,delta,points,share_final,
//! share_mean,alive,adults,design`. `share_final` is the mean legacy share
//! over the last 10 days, `share_mean` over every day after day 10.

use evo_sim::design::{
    trait_cost, EarShape, MorphDesign, TailShape, BASE_FOUNDERS, COST_COLD_BLOODED, COST_PER_EXTRA_EAR_PAIR,
    COST_PER_EXTRA_EYE, COST_PER_EXTRA_FOUNDER, COST_PER_EXTRA_LEG_PAIR, COST_PER_EXTRA_OFFSPRING,
    COST_PER_SMALLER_BODY, DESIGN_BUDGET, MAX_LITTER,
};
use evo_sim::organism::DeathCause;
use evo_sim::rng::rng_for;
use evo_sim::{presets, Balance, BirthType, FounderDesign, Gene, PlayerId, Thermo, World, WorldConfig};
use rand::seq::SliceRandom;
use rand::Rng;

struct Opts {
    seeds: u64,
    worlds: u64,
    days: u32,
    size: usize,
    delta: f32,
    extra_founders: u32,
    seed0: u64,
    world0: u64,
    only: Option<Vec<String>>,
    balance: Balance,
    deaths: bool,
    morph: bool,
    swap_tail: Option<(TailShape, TailShape)>,
}

fn parse(args: &[String]) -> Opts {
    let mut o = Opts {
        seeds: 4,
        worlds: 20,
        days: 90,
        size: 96,
        delta: 0.3,
        extra_founders: 16,
        seed0: 1,
        world0: 0,
        only: None,
        balance: Balance::default(),
        deaths: false,
        morph: false,
        swap_tail: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| panic!("{a} needs a value")).clone();
        match a.as_str() {
            "--seeds" => o.seeds = v().parse().unwrap(),
            "--worlds" => o.worlds = v().parse().unwrap(),
            "--days" => o.days = v().parse().unwrap(),
            "--size" => o.size = v().parse().unwrap(),
            "--delta" => o.delta = v().parse().unwrap(),
            "--extra-founders" => o.extra_founders = v().parse().unwrap(),
            "--seed0" => o.seed0 = v().parse().unwrap(),
            "--world0" => o.world0 = v().parse().unwrap(),
            "--only" => o.only = Some(v().split(',').map(str::to_string).collect()),
            "--deaths" => o.deaths = true,
            "--morph" => o.morph = true,
            "--swap-tail" => {
                let s = v();
                let shape = |k: &str| TailShape::from_key(k).unwrap_or_else(|| panic!("unknown tail shape {k}"));
                let (a, b) = s.split_once('=').unwrap_or_else(|| panic!("--swap-tail expects FROM=TO, got {s}"));
                o.swap_tail = Some((shape(a), shape(b)));
            }
            "--set" => {
                let s = v();
                let (name, x) = s.split_once('=').unwrap_or_else(|| panic!("--set expects NAME=VALUE, got {s}"));
                assert!(o.balance.set(name, x.parse().unwrap()), "unknown balance parameter {name}");
            }
            _ => panic!("unknown option {a}"),
        }
    }
    o
}

fn costed_genes() -> Vec<Gene> {
    Gene::ALL.iter().copied().filter(|&g| trait_cost(g) > 0.0).collect()
}

fn describe(d: &FounderDesign) -> String {
    let mut s = format!("{:?}/{:?}/{}x/{}f", d.thermo, d.birth, d.litter_size, d.founders);
    for &(g, v) in &d.traits {
        s.push_str(&format!(" {}={:.2}", g.label().replace(' ', "_"), v));
    }
    let m = &d.morph;
    if *m != MorphDesign::default() {
        s.push_str(&format!(
            " m:eyes={} m:azimuth={:.0} m:ears={} m:ear_shape={} m:legs={} m:tail_shape={} m:snout={:.2}",
            m.eye_count,
            m.eye_azimuth,
            m.ear_count,
            m.ear_shape.key(),
            m.leg_count,
            m.tail_shape.key(),
            m.snout_length
        ));
    }
    s
}

/// Run a world with the given player designs and return each player's
/// `(share_final, share_mean, alive, adults)`.
fn run(seed: u64, o: &Opts, designs: &[FounderDesign]) -> Vec<(f64, f64, usize, usize)> {
    let config = WorldConfig { seed, width: o.size, height: o.size, balance: o.balance, ..Default::default() };
    let mut world: World = presets::wild_world(config, 1);
    let first = world.lineages.len();
    for (k, d) in designs.iter().enumerate() {
        world.found_lineage_unchecked(k as PlayerId + 1, d);
    }
    // The world keeps only its last few legacy snapshots: collect them.
    let mut legacy: Vec<evo_sim::legacy::LegacySnapshot> = Vec::new();
    for _ in 0..o.days {
        world.run_days(1);
        if let Some(l) = world.latest_legacy().filter(|l| legacy.last().map(|h| h.day) != Some(l.day)) {
            legacy.push(l.clone());
        }
    }
    if o.deaths {
        for k in 0..designs.len() {
            let mut line = format!("deaths,{seed},{k}");
            for cause in DeathCause::ALL {
                let n = world.counters.deaths_by_lineage.get(&((first + k) as u32, cause)).copied().unwrap_or(0);
                line.push_str(&format!(",{}={n}", cause.label()));
            }
            let l = (first + k) as u32;
            let c = &world.counters;
            let e = c.eggs.get(&l).copied().unwrap_or_default();
            line.push_str(&format!(
                ",born={},juvenile={},laid={},hatched={},chilled={},overheated={},raided={}",
                c.born.get(&l).copied().unwrap_or(0),
                c.juvenile_deaths.get(&l).copied().unwrap_or(0),
                e.laid,
                e.hatched,
                e.chilled,
                e.overheated,
                e.raided
            ));
            eprintln!("{line}");
        }
    }
    let counts = world.lineage_counts();
    let tail = legacy.len().saturating_sub(10);
    (0..designs.len())
        .map(|k| {
            let p = k as PlayerId + 1;
            let mean = |from: usize| {
                let s = &legacy[from.min(legacy.len())..];
                s.iter().map(|l| l.share(p)).sum::<f64>() / s.len().max(1) as f64
            };
            let c = counts[first + k];
            (mean(tail), mean(10), c.alive, c.adults)
        })
        .collect()
}

#[derive(Clone)]
enum Variant {
    Control,
    Trait(Gene),
    Founders,
    LitterUp,
    LitterDown,
    /// Body size lowered by half of `--delta` (shrinking is free).
    Smaller,
    /// One morphology change (`--morph`), see [`MorphOpt`].
    Morph(MorphOpt),
}

/// The morphology options of `marginal --morph` (`docs/morphology.md` §4):
/// one count step, an alternative shape, or a placement moved.
#[derive(Clone, Copy)]
enum MorphOpt {
    /// Eye count changed by this many (+1 from 2 is a midline eye, +2 an
    /// extra pair, −1 a single midline eye).
    Eyes(i8),
    /// Leg count changed by this many (legs come in pairs).
    Legs(i8),
    /// Ear count changed by this many (ears come in pairs).
    Ears(i8),
    EarShape(EarShape),
    /// A tail at least [`TAIL_PROBE`] long with this shape. The templates
    /// have no tail, so a shape's own effect is `tail_X − tail_thin`.
    Tail(TailShape),
    /// `eye_azimuth` moved by this many degrees.
    Azimuth(f32),
    /// `snout_length` moved by this much.
    Snout(f32),
}

/// Tail length the tail-shape options give (see [`MorphOpt::Tail`]).
const TAIL_PROBE: f32 = 0.4;

impl MorphOpt {
    const ALL: [MorphOpt; 20] = [
        MorphOpt::Eyes(1),
        MorphOpt::Eyes(2),
        MorphOpt::Eyes(-1),
        MorphOpt::Legs(2),
        MorphOpt::Legs(-2),
        MorphOpt::Legs(-4),
        MorphOpt::Ears(2),
        MorphOpt::Ears(-2),
        MorphOpt::EarShape(EarShape::Pointed),
        MorphOpt::EarShape(EarShape::Long),
        MorphOpt::EarShape(EarShape::Fan),
        MorphOpt::Tail(TailShape::Thin),
        MorphOpt::Tail(TailShape::Bushy),
        MorphOpt::Tail(TailShape::Fin),
        MorphOpt::Tail(TailShape::Club),
        MorphOpt::Tail(TailShape::Prehensile),
        MorphOpt::Azimuth(-40.0),
        MorphOpt::Azimuth(40.0),
        MorphOpt::Snout(-0.4),
        MorphOpt::Snout(0.4),
    ];

    fn name(self) -> String {
        match self {
            MorphOpt::Eyes(k) => format!("eyes{k:+}"),
            MorphOpt::Legs(k) => format!("legs{k:+}"),
            MorphOpt::Ears(k) => format!("ears{k:+}"),
            MorphOpt::EarShape(e) => format!("ear_{}", e.key()),
            MorphOpt::Tail(t) => format!("tail_{}", t.key()),
            MorphOpt::Azimuth(a) => format!("azimuth{a:+.0}"),
            MorphOpt::Snout(x) => format!("snout{x:+.1}"),
        }
    }

    fn step(v: u8, k: i8, lo: u8, hi: u8) -> u8 {
        (v as i16 + k as i16).clamp(lo as i16, hi as i16) as u8
    }

    /// The changed design and the change: structures added (eyes, or pairs
    /// of legs or ears), degrees, snout units, or 0 for a shape.
    fn apply(self, d: &FounderDesign) -> (FounderDesign, f32) {
        let m0 = d.morph;
        let mut m = m0;
        let mut v = d.clone();
        let delta = match self {
            MorphOpt::Eyes(k) => {
                m.eye_count = Self::step(m0.eye_count, k, 1, 6);
                m.eye_count as f32 - m0.eye_count as f32
            }
            MorphOpt::Legs(k) => {
                m.leg_count = Self::step(m0.leg_count, k, 0, 8);
                (m.leg_count as f32 - m0.leg_count as f32) / 2.0
            }
            MorphOpt::Ears(k) => {
                m.ear_count = Self::step(m0.ear_count, k, 0, 4);
                (m.ear_count as f32 - m0.ear_count as f32) / 2.0
            }
            MorphOpt::EarShape(e) => {
                m.ear_shape = e;
                0.0
            }
            MorphOpt::Tail(t) => {
                m.tail_shape = t;
                v = v.with(Gene::Tail, d.get(Gene::Tail).max(TAIL_PROBE));
                0.0
            }
            MorphOpt::Azimuth(a) => {
                m.eye_azimuth = (m0.eye_azimuth + a).clamp(0.0, 180.0);
                m.eye_azimuth - m0.eye_azimuth
            }
            MorphOpt::Snout(x) => {
                m.snout_length = (m0.snout_length + x).clamp(0.0, 1.0);
                m.snout_length - m0.snout_length
            }
        };
        (v.with_morph(m), delta)
    }
}

impl Variant {
    fn name(&self) -> String {
        match self {
            Variant::Control => "control".into(),
            Variant::Trait(g) => g.label().replace(' ', "_"),
            Variant::Founders => "founders".into(),
            Variant::LitterUp => "litter+1".into(),
            Variant::LitterDown => "litter-1".into(),
            Variant::Smaller => "smaller".into(),
            Variant::Morph(m) => m.name(),
        }
    }

    /// The changed design and the change in the varied quantity.
    fn apply(&self, d: &FounderDesign, o: &Opts) -> (FounderDesign, f32) {
        let v = match self {
            Variant::Control => d.clone(),
            Variant::Trait(g) => d.clone().with(*g, (d.get(*g) + o.delta).min(1.0)),
            Variant::Founders => d.clone().founders(d.founders + o.extra_founders),
            Variant::LitterUp => FounderDesign { litter_size: (d.litter_size + 1).min(MAX_LITTER), ..d.clone() },
            Variant::LitterDown => FounderDesign { litter_size: d.litter_size.saturating_sub(1).max(1), ..d.clone() },
            Variant::Smaller => d.clone().with(Gene::BodySize, (d.get(Gene::BodySize) - 0.5 * o.delta).max(0.02)),
            Variant::Morph(m) => return m.apply(d),
        };
        let delta = match self {
            Variant::Control => 0.0,
            Variant::Trait(g) => v.get(*g) - d.get(*g),
            Variant::Smaller => v.get(Gene::BodySize) - d.get(Gene::BodySize),
            Variant::Founders => o.extra_founders as f32,
            Variant::LitterUp | Variant::LitterDown => v.litter_size as f32 - d.litter_size as f32,
            Variant::Morph(_) => unreachable!("returned above"),
        };
        (v, delta)
    }
}

fn marginal(o: &Opts) {
    let templates = presets::player_designs();
    let mut variants =
        vec![Variant::Control, Variant::Founders, Variant::LitterUp, Variant::LitterDown, Variant::Smaller];
    variants.extend(costed_genes().into_iter().map(Variant::Trait));
    if o.morph {
        variants.extend(MorphOpt::ALL.into_iter().map(Variant::Morph));
    }
    if let Some(only) = &o.only {
        variants.retain(|v| only.contains(&v.name()));
    }
    let n = variants.len();
    for s in 0..o.seeds {
        let seed = o.seed0 + s;
        for w in 0..n {
            let picks: Vec<(FounderDesign, String, f32)> = templates
                .iter()
                .enumerate()
                .map(|(t, d)| {
                    let v = &variants[(w + t * (n / 4 + 1) + s as usize * 3) % n];
                    let (design, delta) = v.apply(d, o);
                    (design, v.name(), delta)
                })
                .collect();
            let designs: Vec<FounderDesign> = picks.iter().map(|p| p.0.clone()).collect();
            let out = run(seed, o, &designs);
            for (t, ((design, name, delta), r)) in picks.iter().zip(out).enumerate() {
                println!(
                    "marginal,{seed},{w},{t},{},{name},{delta:.3},{:.1},{:.5},{:.5},{},{},{}",
                    templates[t].name,
                    design.point_cost(),
                    r.0,
                    r.1,
                    r.2,
                    r.3,
                    describe(design)
                );
            }
        }
    }
}

/// A random body plan (`tournament --morph`): each count is the default
/// half the time and otherwise uniform over the other allowed values; shapes
/// are uniform; `eye_azimuth` and `snout_length` are uniform over their
/// ranges. Half the bodies also get a tail of random length 0.1–0.8 (paid
/// from the budget like any trait), since random designs rarely buy one and
/// a tail shape needs a tail. Drawn from its own stream.
fn random_morph(rng: &mut impl Rng) -> (MorphDesign, f32) {
    let d = MorphDesign::default();
    let mut count = |default: u8, values: &[u8]| -> u8 {
        if rng.gen_bool(0.5) {
            default
        } else {
            let others: Vec<u8> = values.iter().copied().filter(|&v| v != default).collect();
            *others.choose(rng).unwrap()
        }
    };
    let eye_count = count(d.eye_count, &[1, 2, 3, 4, 5, 6]);
    let ear_count = count(d.ear_count, &[0, 2, 4]);
    let leg_count = count(d.leg_count, &[0, 2, 4, 6, 8]);
    let tail = if rng.gen_bool(0.5) { rng.gen_range(0.1..0.8) } else { 0.0 };
    let m = MorphDesign {
        eye_count,
        ear_count,
        leg_count,
        ear_shape: *EarShape::ALL.choose(rng).unwrap(),
        tail_shape: *TailShape::ALL.choose(rng).unwrap(),
        eye_azimuth: rng.gen_range(0.0..=180.0),
        snout_length: rng.gen_range(0.0..=1.0),
    };
    (m, tail)
}

/// A random design that spends the whole budget under the current costs,
/// with the body plan `morph` and at least `tail` of tail (paid first).
fn random_design(rng: &mut impl Rng, name: String, morph: MorphDesign, tail: f32) -> FounderDesign {
    use Gene::*;
    let thermo = if rng.gen_bool(0.5) { Thermo::Warm } else { Thermo::Cold };
    let birth = *[BirthType::Live, BirthType::IncubatedEggs, BirthType::AbandonedEggs].choose(rng).unwrap();
    let litter = rng.gen_range(1..=MAX_LITTER);
    let founders = rng.gen_range(10..=60);
    let mut d = FounderDesign::new(name, thermo, birth, litter).founders(founders).with_morph(morph);
    d = d.with(Carnivory, *[0.0, 0.1, 0.5, 0.85].choose(rng).unwrap());
    for g in [Boldness, Aggression, Wanderlust, GroupTolerance, ForagingRange, PackInstinct, Pickiness] {
        d = d.with(g, rng.gen_range(0.1..0.9));
    }
    d = d.with(BodySize, rng.gen_range(0.05..0.75));
    if tail > 0.0 {
        d = d.with(Tail, tail);
    }
    // Shed founders, then litter, until the fixed choices fit.
    while d.point_cost() > DESIGN_BUDGET * 0.7 && d.founders > BASE_FOUNDERS {
        d.founders -= 1;
    }
    while d.point_cost() > DESIGN_BUDGET * 0.7 && d.litter_size > 1 {
        d.litter_size -= 1;
    }
    // Move body size toward its default in case either side is priced.
    while d.point_cost() > DESIGN_BUDGET * 0.7 && (d.get(BodySize) - 0.3).abs() > 0.03 {
        let v = d.get(BodySize) - 0.05 * (d.get(BodySize) - 0.3).signum();
        d = d.with(BodySize, v);
    }
    let mut pool: Vec<Gene> = costed_genes().into_iter().filter(|&g| g != BodySize).collect();
    pool.shuffle(rng);
    pool.truncate(rng.gen_range(3..=8));
    let weights: Vec<f32> = pool.iter().map(|_| rng.gen_range(0.2..1.0)).collect();
    // Spend what's left in proportion to the weights, re-spending what traits
    // capped at 1 couldn't absorb.
    for _ in 0..4 {
        let left = DESIGN_BUDGET - d.point_cost();
        let open: Vec<usize> = (0..pool.len()).filter(|&i| d.get(pool[i]) < 1.0).collect();
        let total: f32 = open.iter().map(|&i| weights[i]).sum();
        if left < 0.5 || total == 0.0 {
            break;
        }
        for &i in &open {
            let g = pool[i];
            let add = left * weights[i] / total / trait_cost(g);
            let v = (d.get(g) + add).min(1.0);
            d = d.with(g, v);
        }
    }
    while d.point_cost() > DESIGN_BUDGET {
        let g = pool[rng.gen_range(0..pool.len())];
        let v = (d.get(g) - 0.01).max(0.0);
        d = d.with(g, v);
    }
    debug_assert!(d.validate().is_ok());
    d
}

fn tournament(o: &Opts) {
    for w in o.world0..o.world0 + o.worlds {
        let seed = o.seed0 + w;
        let mut rng = rng_for(seed, 0xca1, w, 0);
        let designs: Vec<FounderDesign> = (0..4)
            .map(|k| {
                let (morph, tail) =
                    if o.morph { random_morph(&mut rng_for(seed, 0xca2, w, k)) } else { (MorphDesign::default(), 0.0) };
                let mut d = random_design(&mut rng, format!("R{w}.{k}"), morph, tail);
                if let Some((from, to)) = o.swap_tail {
                    if d.morph.tail_shape == from {
                        d.morph.tail_shape = to;
                    }
                }
                d
            })
            .collect();
        let out = run(seed, o, &designs);
        for (k, (d, r)) in designs.iter().zip(out).enumerate() {
            println!(
                "tournament,{seed},{w},{k},{},random,0,{:.1},{:.5},{:.5},{},{},{}",
                d.name,
                d.point_cost(),
                r.0,
                r.1,
                r.2,
                r.3,
                describe(d)
            );
        }
    }
}

fn costs() {
    println!(
        "budget {DESIGN_BUDGET}, base founders {BASE_FOUNDERS}, per extra founder {COST_PER_EXTRA_FOUNDER}, per extra offspring {COST_PER_EXTRA_OFFSPRING}, per unit smaller body {COST_PER_SMALLER_BODY}, cold-blooded {COST_COLD_BLOODED}"
    );
    for g in costed_genes() {
        println!("{:<14} {:5.1}", g.label(), trait_cost(g));
    }
    println!(
        "morphology: per eye beyond 2 {COST_PER_EXTRA_EYE}, per leg pair beyond 2 {COST_PER_EXTRA_LEG_PAIR}, second ear pair {COST_PER_EXTRA_EAR_PAIR}; shapes, placement and snout free"
    );
}

fn main() {
    rayon::ThreadPoolBuilder::new().num_threads(1).build_global().unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first() else {
        eprintln!("usage: calibrate marginal|tournament|costs [options]");
        return;
    };
    let o = parse(&args[1..]);
    match mode.as_str() {
        "marginal" => marginal(&o),
        "tournament" => tournament(&o),
        "costs" => costs(),
        _ => eprintln!("unknown mode {mode}"),
    }
}
