//! Genome level: Wright–Fisher populations on the real genome code paths.
//!
//! A population is `N` genomes. Each generation draws `N` children, each from
//! a uniformly random mother and a uniformly random father (with
//! replacement, so selfing is allowed: every gene copy of a child is then a
//! uniform draw from the parents' `2N`, the textbook Wright–Fisher model),
//! through `Genome::inherit` (genes, obedience on its own stream, morphology
//! on its own stream, load loci) and `Genome::mutate_morph`, the same calls
//! the world makes at a birth. Population genetics theory then applies
//! exactly (or to the diffusion approximation, as stated per check).
//!
//! **Segments.** The genome's loci reach their random numbers by different
//! paths, so every result is reported per segment:
//!
//! - `genes`: the 46 genes before `Obedience`, on the caller's stream;
//! - `obedience`: its own stream, seeded by a hash of the child's other gene
//!   alleles (`Genome::own_rng`);
//! - `load`: the four hidden-recessive loci, on the caller's stream;
//! - `morph discrete` / `morph continuous`: the morphology segment, on a
//!   stream seeded by a hash of the child's gene alleles
//!   (`Genome::morph_rng`).
//!
//! **Markers.** Drift is measured on biallelic markers: a gene allele 0.25
//! or 0.75, a load allele functional or harmful, a discrete morphology
//! allele the default or one step from it. With mutation off a marker is
//! inherited exactly.

use evo_sim::genome::{Allele, Gene, Genome, MutationParams, PlayerId, DOMINANCE, LOAD_LOCI};
use evo_sim::morph::{MorphGene, MorphLocus};
use evo_sim::rng::{rng_for, SimRng};
use evo_sim::{Balance, WorldConfig};
use rand::seq::SliceRandom;
use rand::Rng;
use rayon::prelude::*;

use super::{ascii_plot, exact_check, mean_check, mean_se, num, prop_check, Check, Profile, Verdict, Z_STRICT};

/// Random stream of the harness (`rng_for(seed, STREAM, check, replicate)`).
pub const STREAM: u64 = 0x6765_6e65_7469_6373;

const OBED: usize = Gene::Obedience as usize;

/// Which code path a locus's randomness takes (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Genes,
    Obedience,
    Load,
    MorphD,
    MorphC,
}

impl Seg {
    pub const ALL: [Seg; 5] = [Seg::Genes, Seg::Obedience, Seg::Load, Seg::MorphD, Seg::MorphC];

    pub fn label(self) -> &'static str {
        match self {
            Seg::Genes => "genes",
            Seg::Obedience => "obedience",
            Seg::Load => "load",
            Seg::MorphD => "morph discrete",
            Seg::MorphC => "morph continuous",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One locus of the genome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    Gene(usize),
    Load(usize),
    MorphD(usize),
    MorphC(usize),
}

impl Site {
    pub fn seg(self) -> Seg {
        match self {
            Site::Gene(k) if k == OBED => Seg::Obedience,
            Site::Gene(_) => Seg::Genes,
            Site::Load(_) => Seg::Load,
            Site::MorphD(_) => Seg::MorphD,
            Site::MorphC(_) => Seg::MorphC,
        }
    }

    pub fn all() -> Vec<Site> {
        let mut v: Vec<Site> = (0..Gene::COUNT).map(Site::Gene).collect();
        v.extend((0..LOAD_LOCI).map(Site::Load));
        v.extend((0..MorphLocus::COUNT).map(Site::MorphD));
        v.extend((0..MorphGene::COUNT).map(Site::MorphC));
        v
    }

    fn label(self) -> String {
        match self {
            Site::Gene(k) => Gene::ALL[k].label().to_string(),
            Site::Load(k) => format!("load {k}"),
            Site::MorphD(k) => MorphLocus::ALL[k].key().to_string(),
            Site::MorphC(k) => MorphGene::ALL[k].key().to_string(),
        }
    }
}

/// The marker variant of a discrete locus: one step from the default.
pub fn variant(l: MorphLocus) -> u8 {
    match l {
        MorphLocus::EyeCount => 3,
        MorphLocus::EarCount => 4,
        MorphLocus::EarShape => 1,
        MorphLocus::LegCount => 6,
        MorphLocus::TailShape | MorphLocus::HornForm | MorphLocus::Pattern => 1,
    }
}

const LO: f32 = 0.25;
const HI: f32 = 0.75;

/// A marker's two alleles (`true` = the variant).
pub fn marker_get(g: &Genome, s: Site) -> (bool, bool) {
    match s {
        Site::Gene(k) => {
            let l = &g.loci()[k];
            (l.a.value > 0.5, l.b.value > 0.5)
        }
        Site::Load(k) => {
            let l = &g.load()[k];
            (l.a.is_harmful(), l.b.is_harmful())
        }
        Site::MorphD(k) => {
            let l = MorphLocus::ALL[k];
            let p = &g.morph().discrete[k];
            (p.a.value != l.default_value(), p.b.value != l.default_value())
        }
        Site::MorphC(k) => {
            let p = &g.morph().continuous[k];
            (p.a.value > 0.5, p.b.value > 0.5)
        }
    }
}

/// Set a marker's two alleles, keeping their origin tags.
pub fn marker_set(g: &mut Genome, s: Site, a: bool, b: bool) {
    let v = |x: bool| if x { HI } else { LO };
    match s {
        Site::Gene(k) => g.set_gene_alleles(Gene::ALL[k], v(a), v(b)),
        Site::Load(k) => {
            let l = g.load()[k];
            g.set_load(k, false, Allele { value: if a { 1.0 } else { 0.0 }, origin: l.a.origin });
            g.set_load(k, true, Allele { value: if b { 1.0 } else { 0.0 }, origin: l.b.origin });
        }
        Site::MorphD(k) => {
            let l = MorphLocus::ALL[k];
            let mut m = *g.morph();
            m.discrete[k].a.value = if a { variant(l) } else { l.default_value() };
            m.discrete[k].b.value = if b { variant(l) } else { l.default_value() };
            g.set_morph(m);
        }
        Site::MorphC(k) => {
            let mut m = *g.morph();
            m.continuous[k].a.value = v(a);
            m.continuous[k].b.value = v(b);
            g.set_morph(m);
        }
    }
}

/// Allele identities (exact value bits) at a site, for infinite-alleles
/// heterozygosity.
fn keys(g: &Genome, s: Site) -> (u32, u32) {
    match s {
        Site::Gene(k) => {
            let l = &g.loci()[k];
            (l.a.value.to_bits(), l.b.value.to_bits())
        }
        Site::Load(k) => {
            let l = &g.load()[k];
            (l.a.value.to_bits(), l.b.value.to_bits())
        }
        Site::MorphD(k) => {
            let p = &g.morph().discrete[k];
            (p.a.value as u32, p.b.value as u32)
        }
        Site::MorphC(k) => {
            let p = &g.morph().continuous[k];
            (p.a.value.to_bits(), p.b.value.to_bits())
        }
    }
}

/// Continuous allele values at a site (genes and morph continuous).
fn values(g: &Genome, s: Site) -> (f32, f32) {
    match s {
        Site::Gene(k) => (g.loci()[k].a.value, g.loci()[k].b.value),
        Site::MorphC(k) => (g.morph().continuous[k].a.value, g.morph().continuous[k].b.value),
        _ => unreachable!("not a continuous site"),
    }
}

/// Mutation rates of a run.
#[derive(Clone, Copy, Debug)]
pub struct Rates {
    pub mutation: MutationParams,
    pub load: f32,
    pub morph: f32,
}

impl Rates {
    pub const NONE: Rates = Rates { mutation: MutationParams { rate: 0.0, sd: 0.0 }, load: 0.0, morph: 0.0 };

    /// The world defaults (`WorldConfig::mutation`, `Balance::recessive_mutation`,
    /// `Balance::morph_step_rate`).
    pub fn defaults() -> Rates {
        let b = Balance::default();
        Rates { mutation: WorldConfig::default().mutation, load: b.recessive_mutation, morph: b.morph_step_rate }
    }
}

/// A birth, as the world makes it: inherit, then step-mutate the morphology.
pub fn child(m: &Genome, f: &Genome, owner: PlayerId, r: &Rates, rng: &mut SimRng) -> Genome {
    let mut c = Genome::inherit(m, f, owner, r.mutation, r.load, rng);
    c.mutate_morph(owner, r.morph);
    c
}

/// One Wright–Fisher generation (random parents, selfing allowed).
pub fn wf_generation(pop: &[Genome], owner: PlayerId, r: &Rates, rng: &mut SimRng) -> Vec<Genome> {
    let n = pop.len();
    (0..n)
        .map(|_| {
            let m = &pop[rng.gen_range(0..n)];
            let f = &pop[rng.gen_range(0..n)];
            child(m, f, owner, r, rng)
        })
        .collect()
}

/// Founders of a flat design (every gene 0.5, contract-default morphology),
/// alleles varied by `variation`.
pub fn founders(n: usize, origin: PlayerId, variation: f32, rng: &mut SimRng) -> Vec<Genome> {
    (0..n).map(|_| Genome::founder(&[0.5; Gene::COUNT], origin, variation, rng)).collect()
}

fn rng(seed: u64, check: u64, rep: u64) -> SimRng {
    rng_for(seed, STREAM, check, rep)
}

/// Results of the genome level.
pub struct Results {
    pub checks: Vec<Check>,
    pub sections: Vec<(String, String)>,
}

/// Every genome-level check (or those whose group name contains `only`).
/// `log` prints progress to stderr.
pub fn run_all(p: &Profile, seed: u64, only: Option<&str>, log: bool) -> Results {
    let mut r = Results { checks: Vec::new(), sections: Vec::new() };
    type Step = fn(&Profile, u64, &mut Results);
    let steps: [(&str, Step); 9] = [
        ("neutral drift", drift),
        ("mutation-drift equilibrium", equilibrium),
        ("morphology step mutation", morph_stationary),
        ("mutation rates", mutation_rates),
        ("dominance", dominance),
        ("additive inheritance", additive),
        ("recessive load", recessive_load),
        ("legacy tags", legacy),
        ("linkage", linkage),
    ];
    for (name, f) in steps {
        if only.is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let t = std::time::Instant::now();
        let before = r.checks.len();
        f(p, seed, &mut r);
        if log {
            let fails = r.checks[before..].iter().filter(|c| c.verdict == Verdict::Fail).count();
            eprintln!(
                "genetics: {name}: {} checks, {fails} failed ({:.1} s)",
                r.checks.len() - before,
                t.elapsed().as_secs_f64()
            );
        }
    }
    r
}

// ---------------------------------------------------------------------------
// Neutral drift

/// One drift replicate's record.
struct DriftRep {
    /// Per generation `t ≤ t_rec`, per segment: sum over marker sites of
    /// `2p(1-p)` and of `(p - p0)^2`.
    h: Vec<[f64; 5]>,
    dev2: Vec<[f64; 5]>,
    /// Per marker site: (segment, absorption generation, fixed?), or None if
    /// still segregating at `t_max`.
    absorbed: Vec<(Seg, Option<(u32, bool)>)>,
}

/// Markers at frequency `p0` (exactly `round(2N p0)` copies, shuffled) at the
/// chosen sites; run until every marker is fixed or lost (or `t_max`).
fn drift_rep(n: usize, p0: f64, sites: &[Site], rates: &Rates, t_rec: usize, t_max: usize, r: &mut SimRng) -> DriftRep {
    let mut pop = founders(n, 1, 0.05, r);
    let copies = (2.0 * n as f64 * p0).round() as usize;
    for &s in sites {
        let mut v: Vec<bool> = (0..2 * n).map(|k| k < copies).collect();
        v.shuffle(r);
        for (i, g) in pop.iter_mut().enumerate() {
            marker_set(g, s, v[2 * i], v[2 * i + 1]);
        }
    }
    let p0 = copies as f64 / (2 * n) as f64;
    let mut rep =
        DriftRep { h: Vec::new(), dev2: Vec::new(), absorbed: sites.iter().map(|s| (s.seg(), None)).collect() };
    let mut t = 0;
    loop {
        let mut h = [0.0; 5];
        let mut d = [0.0; 5];
        let mut open = 0;
        for (k, &s) in sites.iter().enumerate() {
            let c: usize = pop
                .iter()
                .map(|g| {
                    let (a, b) = marker_get(g, s);
                    a as usize + b as usize
                })
                .sum();
            let p = c as f64 / (2 * n) as f64;
            h[s.seg().index()] += 2.0 * p * (1.0 - p);
            d[s.seg().index()] += (p - p0) * (p - p0);
            if rep.absorbed[k].1.is_none() {
                if c == 0 || c == 2 * n {
                    rep.absorbed[k].1 = Some((t as u32, c == 2 * n));
                } else {
                    open += 1;
                }
            }
        }
        if t <= t_rec {
            rep.h.push(h);
            rep.dev2.push(d);
        }
        if (open == 0 && t >= t_rec) || t >= t_max {
            break;
        }
        pop = wf_generation(&pop, 2, rates, r);
        t += 1;
    }
    rep
}

/// Exact Wright–Fisher (2N gene copies, binomial sampling) absorption
/// quantities from `i` copies: (mean time to absorption, mean time to
/// fixation given fixation).
#[allow(clippy::needless_range_loop)] // dense linear algebra reads clearer with indices
pub fn wf_exact_times(n: usize, i: usize) -> (f64, f64) {
    let m = 2 * n;
    // ln C(m, j)
    let lnfact: Vec<f64> = (0..=m)
        .scan(0.0, |acc, k| {
            if k > 0 {
                *acc += (k as f64).ln();
            }
            Some(*acc)
        })
        .collect();
    let pmf = |x: f64, j: usize| -> f64 {
        if x <= 0.0 {
            return (j == 0) as u8 as f64;
        }
        if x >= 1.0 {
            return (j == m) as u8 as f64;
        }
        (lnfact[m] - lnfact[j] - lnfact[m - j] + j as f64 * x.ln() + (m - j) as f64 * (1.0 - x).ln()).exp()
    };
    let t = m - 1; // transient states 1..m-1
    let solve = |conditioned: bool| -> Vec<f64> {
        // (I - Q) tau = 1
        let mut a = vec![vec![0.0; t + 1]; t];
        for r in 0..t {
            let i = r + 1;
            let x = i as f64 / m as f64;
            for c in 0..t {
                let j = c + 1;
                let mut q = pmf(x, j);
                if conditioned {
                    q *= j as f64 / i as f64;
                }
                a[r][c] = if r == c { 1.0 - q } else { -q };
            }
            a[r][t] = 1.0;
        }
        // Gaussian elimination with partial pivoting.
        for col in 0..t {
            let piv = (col..t).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs())).unwrap();
            a.swap(col, piv);
            let d = a[col][col];
            for k in col..=t {
                a[col][k] /= d;
            }
            for r in 0..t {
                if r != col {
                    let f = a[r][col];
                    if f != 0.0 {
                        for k in col..=t {
                            a[r][k] -= f * a[col][k];
                        }
                    }
                }
            }
        }
        (0..t).map(|r| a[r][t]).collect()
    };
    let tau = solve(false);
    let tau1 = solve(true);
    (tau[i - 1], tau1[i - 1])
}

fn drift(p: &Profile, seed: u64, out: &mut Results) {
    let n = 50usize;
    let t_rec = 4 * n;
    let t_max = 60 * n;
    let all = Site::all();
    let bg_sites: Vec<Site> = all.iter().copied().filter(|s| matches!(s, Site::Load(_) | Site::MorphD(_))).collect();
    let bg_rates = Rates { mutation: Rates::defaults().mutation, load: 0.0, morph: 0.0 };
    // (label, p0, sites, rates, check id)
    let modes: [(&str, f64, &[Site], Rates, u64); 3] = [
        ("all loci as markers, no mutation", 0.5, &all, Rates::NONE, 1),
        ("all loci as markers, no mutation", 0.2, &all, Rates::NONE, 2),
        ("load + morph discrete markers, gene mutation on", 0.5, &bg_sites, bg_rates, 3),
    ];
    let mut plot_done = false;
    for (mode, p0, sites, rates, id) in modes {
        let reps: Vec<DriftRep> = (0..p.drift_reps as u64)
            .into_par_iter()
            .map(|k| drift_rep(n, p0, sites, &rates, t_rec, t_max, &mut rng(seed, id, k)))
            .collect();
        let copies = (2.0 * n as f64 * p0).round() as usize;
        let p0 = copies as f64 / (2 * n) as f64;
        let h0 = 2.0 * p0 * (1.0 - p0);
        let decay = |t: usize| (1.0 - 1.0 / (2.0 * n as f64)).powi(t as i32);
        let all_markers = id != 3;
        for seg in Seg::ALL {
            let count = sites.iter().filter(|s| s.seg() == seg).count();
            if count == 0 {
                continue;
            }
            // In the all-markers mode the hash-seeded segments lose their
            // randomness as the gene background fixes (see the report):
            // reported, not judged.
            let judged = !all_markers || matches!(seg, Seg::Genes | Seg::Load);
            let series = |t: usize, which: &dyn Fn(&DriftRep) -> f64| -> (f64, f64) {
                let xs: Vec<f64> = reps.iter().map(which).collect();
                let _ = t;
                mean_se(&xs)
            };
            let si = seg.index();
            if p0 == 0.5 || seg == Seg::Genes {
                for t in [n, 3 * n] {
                    let (m, se) = series(t, &|r: &DriftRep| r.h[t][si] / count as f64 / h0);
                    let mut c = mean_check(
                        "neutral drift",
                        format!("H_t/H_0 at t={t}, {} ({mode}, p0={p0})", seg.label()),
                        decay(t),
                        m,
                        se,
                        0.0,
                        format!(
                            "E[H_t] = H_0 (1 - 1/2N)^t, N={n}, {} replicates × {count} loci; H = 2p(1-p) of the population.",
                            p.drift_reps
                        ),
                    );
                    if !judged {
                        c.verdict = Verdict::Info;
                        c.detail.push_str(" Not judged: with every gene a marker and mutation off, the gene background fixes and this segment's hash-seeded stream stops varying (see 'Hash-seeded streams').");
                    }
                    out.checks.push(c);
                }
            }
            if seg == Seg::Genes && p0 == 0.5 {
                let t = 2 * n;
                let (m, se) = series(t, &|r: &DriftRep| r.dev2[t][si] / count as f64);
                out.checks.push(mean_check(
                    "neutral drift",
                    format!("Var(p_t) at t={t}, genes (p0={p0})"),
                    p0 * (1.0 - p0) * (1.0 - decay(t)),
                    m,
                    se,
                    0.0,
                    "Var(p_t) = p0 q0 [1 - (1 - 1/2N)^t].",
                ));
            }
            // Fixation probability and absorption times, per segment.
            let mut fixed = 0u64;
            let mut total = 0u64;
            let mut censored = 0u64;
            let (mut times, mut fix_times) = (Vec::new(), Vec::new());
            for r in &reps {
                for &(s, a) in &r.absorbed {
                    if s != seg {
                        continue;
                    }
                    match a {
                        Some((t, f)) => {
                            total += 1;
                            fixed += f as u64;
                            times.push(t as f64);
                            if f {
                                fix_times.push(t as f64);
                            }
                        }
                        None => censored += 1,
                    }
                }
            }
            let mut push = |mut c: Check| {
                if !judged {
                    c.verdict = Verdict::Info;
                }
                out.checks.push(c);
            };
            push(prop_check(
                "neutral drift",
                format!("fixation probability, {} ({mode}, p0={p0})", seg.label()),
                p0,
                fixed,
                total,
                0.0,
                format!("A neutral allele fixes with probability equal to its initial frequency. {censored} loci still segregating at t={t_max} (excluded)."),
            ));
            let (tau, tau1) = wf_exact_times(n, copies);
            let diff = -4.0 * n as f64 * (p0 * p0.ln() + (1.0 - p0) * (1.0 - p0).ln());
            let diff1 = -4.0 * n as f64 * (1.0 - p0) * (1.0 - p0).ln() / p0;
            let (m, se) = mean_se(&times);
            push(mean_check(
                "neutral drift",
                format!("mean time to absorption (gens), {} ({mode}, p0={p0})", seg.label()),
                tau,
                m,
                se,
                0.0,
                format!(
                    "Exact Wright–Fisher chain (2N={}); diffusion approximation -4N[p ln p + q ln q] = {} ({}N).",
                    2 * n,
                    num(diff),
                    num(diff / n as f64)
                ),
            ));
            let (m, se) = mean_se(&fix_times);
            push(mean_check(
                "neutral drift",
                format!("mean time to fixation given fixation, {} ({mode}, p0={p0})", seg.label()),
                tau1,
                m,
                se,
                0.0,
                format!("Exact Wright–Fisher chain; diffusion approximation -4N q ln q / p = {}.", num(diff1)),
            ));
        }
        if !plot_done {
            plot_done = true;
            let count = sites.iter().filter(|s| s.seg() == Seg::Genes).count() as f64;
            let xs: Vec<f64> = (0..=t_rec).step_by(4).map(|t| t as f64).collect();
            let obs: Vec<f64> = (0..=t_rec)
                .step_by(4)
                .map(|t| reps.iter().map(|r| r.h[t][0] / count).sum::<f64>() / reps.len() as f64)
                .collect();
            let exp: Vec<f64> = (0..=t_rec).step_by(4).map(|t| h0 * decay(t)).collect();
            let plot = ascii_plot(
                &format!(
                    "Mean heterozygosity 2p(1-p) of the gene markers vs generation (N={n}, p0=0.5, {} replicates)",
                    p.drift_reps
                ),
                &xs,
                &[("expected H0(1-1/2N)^t", '.', exp), ("observed", 'o', obs)],
                14,
            );
            out.sections.push(("Neutral drift: heterozygosity decay".into(), plot));
        }
    }
}

// ---------------------------------------------------------------------------
// Mutation–drift equilibrium (infinite alleles, additive variance)

struct EqRep {
    /// Per segment: mean over samples and sites of the unbiased
    /// heterozygosity and of the within-population allele variance.
    h: [f64; 5],
    v: [f64; 5],
}

fn equilibrium_rep(n: usize, varied: bool, burn: usize, gens: usize, every: usize, r: &mut SimRng) -> EqRep {
    let rates = Rates { mutation: Rates::defaults().mutation, load: 0.0, morph: 0.0 };
    let mut pop: Vec<Genome> = if varied {
        founders(n, 1, 0.05, r)
    } else {
        (0..n).map(|_| Genome::uniform(&[0.5; Gene::COUNT], 1)).collect()
    };
    // The core continuous loci only, as calibrated (horn size starts at the
    // clamp bound 0, where the unbounded-locus formula doesn't hold).
    let sites: Vec<Site> = Site::all()
        .into_iter()
        .filter(|s| match s {
            Site::Gene(_) => true,
            Site::MorphC(k) => *k < MorphGene::CORE,
            _ => false,
        })
        .collect();
    let (mut h, mut v, mut cnt) = ([0.0; 5], [0.0; 5], [0.0; 5]);
    let mut buf = Vec::with_capacity(2 * n);
    let two_n = (2 * n) as f64;
    for t in 0..burn + gens {
        if t >= burn && (t - burn).is_multiple_of(every) {
            for &s in &sites {
                buf.clear();
                let (mut sum, mut sum2) = (0.0f64, 0.0f64);
                for g in &pop {
                    let (a, b) = keys(g, s);
                    buf.push(a);
                    buf.push(b);
                    let (x, y) = values(g, s);
                    sum += (x + y) as f64;
                    sum2 += (x * x + y * y) as f64;
                }
                buf.sort_unstable();
                let mut hom = 0.0;
                let mut k = 0;
                while k < buf.len() {
                    let j = buf[k..].iter().take_while(|&&x| x == buf[k]).count();
                    hom += (j as f64 / two_n).powi(2);
                    k += j;
                }
                let i = s.seg().index();
                h[i] += two_n / (two_n - 1.0) * (1.0 - hom);
                let mean = sum / two_n;
                v[i] += sum2 / two_n - mean * mean;
                cnt[i] += 1.0;
            }
        }
        pop = wf_generation(&pop, 1, &rates, r);
    }
    for i in 0..5 {
        if cnt[i] > 0.0 {
            h[i] /= cnt[i];
            v[i] /= cnt[i];
        }
    }
    EqRep { h, v }
}

fn equilibrium(p: &Profile, seed: u64, out: &mut Results) {
    let m = Rates::defaults().mutation;
    let (mu, sd) = (m.rate as f64, m.sd as f64);
    let sizes: &[(usize, bool, u64)] =
        if p.name == "quick" { &[(50, true, 10)] } else { &[(50, true, 10), (50, false, 11), (100, true, 12)] };
    for &(n, varied, id) in sizes {
        let burn = 12 * n;
        let reps: Vec<EqRep> = (0..p.equilibrium_reps as u64)
            .into_par_iter()
            .map(|k| equilibrium_rep(n, varied, burn, p.equilibrium_gens, 10, &mut rng(seed, id, k)))
            .collect();
        let two_n = 2.0 * n as f64;
        let keep = (1.0 - mu).powi(2);
        let f_eq = keep / two_n / (1.0 - keep * (1.0 - 1.0 / two_n));
        let start = if varied { "founder-varied start" } else { "uniform start" };
        for seg in [Seg::Genes, Seg::Obedience, Seg::MorphC] {
            let i = seg.index();
            let (hm, hse) = mean_se(&reps.iter().map(|r| r.h[i]).collect::<Vec<_>>());
            out.checks.push(mean_check(
                "mutation-drift",
                format!("equilibrium heterozygosity, {} (N={n}, {start})", seg.label()),
                1.0 - f_eq,
                hm,
                hse,
                0.01,
                format!(
                    "Infinite alleles (every mutation a new value): exact WF recursion F' = (1-μ)²[1/2N + (1-1/2N)F], μ={mu}; 4Nμ/(1+4Nμ) = {}. Burn-in {burn} gens, {} gens sampled every 10, {} replicates. Tolerance 0.01 for value collisions at the clamp bounds.",
                    num(4.0 * n as f64 * mu / (1.0 + 4.0 * n as f64 * mu)),
                    p.equilibrium_gens,
                    p.equilibrium_reps
                ),
            ));
            let (vm, vse) = mean_se(&reps.iter().map(|r| r.v[i]).collect::<Vec<_>>());
            let v_eq = (two_n - 1.0) * mu * sd * sd;
            out.checks.push(mean_check(
                "mutation-drift",
                format!("equilibrium allele variance, {} (N={n}, {start})", seg.label()),
                v_eq,
                vm,
                vse,
                0.05 * v_eq,
                format!("Additive neutral locus: V' = (1-1/2N)(V + μσ²) ⇒ V_eq = (2N-1)μσ², σ={sd}. Tolerance 5% for clamping at [0, 1]."),
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Discrete morphology: step mutation, stationary distribution, validity

/// Positions on a locus's axis.
fn positions(l: MorphLocus) -> usize {
    if l.is_shape() {
        l.range().1 as usize + 1
    } else {
        l.position(l.range().1) as usize + 1
    }
}

/// Stationary distribution of the step chain: a random walk on a path
/// (interior ±1 equally, ends always inward) is stationary in proportion to
/// each position's degree.
pub fn path_stationary(k: usize) -> Vec<f64> {
    let deg: Vec<f64> = (0..k).map(|i| if i == 0 || i == k - 1 { 1.0 } else { 2.0 }).collect();
    let s: f64 = deg.iter().sum();
    deg.iter().map(|d| d / s).collect()
}

/// Every discrete allele and expressed value valid (odd legs, three ears,
/// seven eyes and unknown shapes are impossible).
fn invalid_morph(g: &Genome) -> u64 {
    let m = g.morph();
    let mut bad = 0;
    for l in MorphLocus::ALL {
        let pr = &m.discrete[l.index()];
        bad += !l.is_valid(pr.a.value) as u64 + !l.is_valid(pr.b.value) as u64 + !l.is_valid(m.expressed(l)) as u64;
    }
    let e = g.morphology();
    bad += !(1..=6).contains(&e.eye_count) as u64;
    bad += ![0, 2, 4].contains(&e.ear_count) as u64;
    bad += ![0, 2, 4, 6, 8].contains(&e.leg_count) as u64;
    bad
}

struct MorphRep {
    /// Per locus, per position: mean allele frequency over samples.
    freq: Vec<Vec<f64>>,
    invalid: u64,
    genomes: u64,
    /// Mean fraction carrying a non-default allele, and expressing one.
    carriers: f64,
    expressing: f64,
}

fn morph_rep(n: usize, rate: f32, burn: usize, gens: usize, every: usize, r: &mut SimRng) -> MorphRep {
    let rates = Rates { mutation: Rates::defaults().mutation, load: 0.0, morph: rate };
    let mut pop = founders(n, 1, 0.05, r);
    let mut freq: Vec<Vec<f64>> = MorphLocus::ALL.iter().map(|&l| vec![0.0; positions(l)]).collect();
    let (mut invalid, mut genomes, mut samples) = (0, 0, 0.0);
    let (mut carriers, mut expressing) = (0.0, 0.0);
    for t in 0..burn + gens {
        for g in &pop {
            invalid += invalid_morph(g);
        }
        genomes += pop.len() as u64;
        if t >= burn && (t - burn).is_multiple_of(every) {
            samples += 1.0;
            for l in MorphLocus::ALL {
                let f = &mut freq[l.index()];
                for g in &pop {
                    let pr = &g.morph().discrete[l.index()];
                    f[l.position(pr.a.value) as usize] += 1.0 / (2 * n) as f64;
                    f[l.position(pr.b.value) as usize] += 1.0 / (2 * n) as f64;
                }
            }
            let mut c = 0;
            let mut e = 0;
            for g in &pop {
                let m = g.morph();
                c += MorphLocus::ALL.iter().any(|&l| {
                    let pr = &m.discrete[l.index()];
                    pr.a.value != l.default_value() || pr.b.value != l.default_value()
                }) as usize;
                e += MorphLocus::ALL.iter().any(|&l| m.expressed(l) != l.default_value()) as usize;
            }
            carriers += c as f64 / n as f64;
            expressing += e as f64 / n as f64;
        }
        pop = wf_generation(&pop, 1, &rates, r);
    }
    for f in &mut freq {
        for x in f.iter_mut() {
            *x /= samples;
        }
    }
    MorphRep { freq, invalid, genomes, carriers: carriers / samples, expressing: expressing / samples }
}

fn morph_stationary(p: &Profile, seed: u64, out: &mut Results) {
    let n = 50;
    let rate = 0.05;
    let (burn, gens) = if p.name == "quick" { (600, 600) } else { (1500, 3000) };
    let reps_n = if p.name == "quick" { 12 } else { p.equilibrium_reps };
    let reps: Vec<MorphRep> =
        (0..reps_n as u64).into_par_iter().map(|k| morph_rep(n, rate, burn, gens, 10, &mut rng(seed, 20, k))).collect();
    let mut table = String::from("| Locus | Position (value) | Expected | Observed ± SE |\n|---|---|---|---|\n");
    for l in MorphLocus::ALL {
        let k = positions(l);
        let pi = path_stationary(k);
        let mut worst = 0.0f64;
        let mut obs = Vec::new();
        for (pos, &e) in pi.iter().enumerate() {
            let (m, se) = mean_se(&reps.iter().map(|r| r.freq[l.index()][pos]).collect::<Vec<_>>());
            worst = worst.max((m - e).abs() / se.max(0.005));
            obs.push(m);
            table.push_str(&format!(
                "| {} | {} ({}) | {} | {} ± {} |\n",
                l.key(),
                pos,
                l.value_at(pos as u8),
                num(e),
                num(m),
                num(se)
            ));
        }
        let fmt = |v: &[f64]| format!("[{}]", v.iter().map(|x| format!("{x:.3}")).collect::<Vec<_>>().join(", "));
        out.checks.push(Check {
            level: "genome",
            group: "morph step mutation",
            name: format!("stationary allele distribution, {}", l.key()),
            expected: fmt(&pi),
            observed: fmt(&obs),
            ci: format!("max |z| = {worst:.2}"),
            verdict: Verdict::strict(worst <= 4.5),
            detail: format!(
                "Under neutral drift the expected allele frequencies follow the mutation chain, so their long-run mean is its stationary distribution (∝ degree on the path of positions, ends move inward). Step rate raised to {rate} so the chain mixes; N={n}, burn-in {burn}, {gens} gens sampled every 10, {reps_n} replicates. Pass: every position within 4.5 SE (across replicates, floored at 0.005; several positions per locus)."
            ),
        });
    }
    out.sections.push(("Morphology step mutation: stationary allele distribution".into(), table));
    let invalid: u64 = reps.iter().map(|r| r.invalid).sum();
    let genomes: u64 = reps.iter().map(|r| r.genomes).sum();
    out.checks.push(exact_check(
        "morph step mutation",
        format!("impossible morphology values in {genomes} genomes (step rate {rate})"),
        0,
        invalid,
        "Every discrete allele and expressed value checked every generation: eyes 1–6, ears 0/2/4, legs 0/2/4/6/8, known shapes.",
    ));
    let (c, _) = mean_se(&reps.iter().map(|r| r.carriers).collect::<Vec<_>>());
    let (e, _) = mean_se(&reps.iter().map(|r| r.expressing).collect::<Vec<_>>());
    out.checks.push(Check {
        level: "genome",
        group: "morph step mutation",
        name: "carriers of a non-default allele vs expressing one".into(),
        expected: "expressing < carrying".into(),
        observed: format!("{} carry, {} express", num(c), num(e)),
        ci: "-".into(),
        verdict: Verdict::strict(e < c),
        detail: "The default is dominant, so variants hide in carriers.".into(),
    });
}

// ---------------------------------------------------------------------------
// Mutation rates, step spectrum and origin tags

#[derive(Default, Clone)]
struct MutCounts {
    alleles: [u64; 5],
    mutated: [u64; 5],
    /// Value steps of mutated continuous alleles: n, Σδ, Σδ².
    steps: [(f64, f64, f64); 5],
    /// Alleles whose tag disagrees with whether they mutated.
    tag_errors: u64,
}

impl MutCounts {
    fn add(&mut self, o: &MutCounts) {
        for i in 0..5 {
            self.alleles[i] += o.alleles[i];
            self.mutated[i] += o.mutated[i];
            self.steps[i].0 += o.steps[i].0;
            self.steps[i].1 += o.steps[i].1;
            self.steps[i].2 += o.steps[i].2;
        }
        self.tag_errors += o.tag_errors;
    }
}

/// A parent homozygous at every continuous locus, at a value of its own
/// (0.5 + N(0, 0.05²)): a mutated allele's source is then known exactly.
/// (A heterozygous parent leaves the source ambiguous: assigning a large
/// step to the nearer parental allele understates the step SD.) Every child
/// gets fresh parents, so gene backgrounds never repeat.
fn homozygous_parent(r: &mut SimRng) -> Genome {
    let mut g = Genome::founder(&[0.5; Gene::COUNT], 1, 0.0, r);
    for k in 0..Gene::COUNT {
        let v = 0.5 + 0.05 * evo_sim::rng::normal(r);
        g.set_gene_alleles(Gene::ALL[k], v, v);
    }
    let mut m = *g.morph();
    for c in &mut m.continuous {
        let v = 0.5 + 0.05 * evo_sim::rng::normal(r);
        (c.a.value, c.b.value) = (v, v);
    }
    g.set_morph(m);
    g
}

fn count_mutations(c: &Genome, m: &Genome, f: &Genome, owner: PlayerId, out: &mut MutCounts) {
    // Continuous sites: the child's `a` came from the mother, `b` from the father.
    for s in Site::all() {
        let i = s.seg().index();
        match s {
            Site::Gene(_) | Site::MorphC(_) => {
                let (ca, cb) = values(c, s);
                let tags = match s {
                    Site::Gene(k) => (c.loci()[k].a.origin, c.loci()[k].b.origin),
                    Site::MorphC(k) => (c.morph().continuous[k].a.origin, c.morph().continuous[k].b.origin),
                    _ => unreachable!(),
                };
                for (v, tag, parent) in [(ca, tags.0, m), (cb, tags.1, f)] {
                    let (pa, pb) = values(parent, s);
                    out.alleles[i] += 1;
                    let copied = v == pa || v == pb;
                    let mutated = tag == owner;
                    if mutated {
                        out.mutated[i] += 1;
                        let src = if (v - pa).abs() < (v - pb).abs() { pa } else { pb };
                        let d = (v - src) as f64;
                        out.steps[i].0 += 1.0;
                        out.steps[i].1 += d;
                        out.steps[i].2 += d * d;
                    }
                    if mutated == copied {
                        out.tag_errors += 1;
                    }
                }
            }
            Site::Load(k) => {
                let l = &c.load()[k];
                for a in [l.a, l.b] {
                    out.alleles[i] += 1;
                    let mutated = a.is_harmful();
                    out.mutated[i] += mutated as u64;
                    if mutated != (a.origin == owner) {
                        out.tag_errors += 1;
                    }
                }
            }
            Site::MorphD(k) => {
                let l = MorphLocus::ALL[k];
                let pr = &c.morph().discrete[k];
                for a in [pr.a, pr.b] {
                    out.alleles[i] += 1;
                    let mutated = a.value != l.default_value();
                    out.mutated[i] += mutated as u64;
                    if mutated != (a.origin == owner) {
                        out.tag_errors += 1;
                    }
                }
            }
        }
    }
}

fn mutation_rates(p: &Profile, seed: u64, out: &mut Results) {
    let rates = Rates::defaults();
    let chunks = 64u64;
    let per = p.mutation_children / chunks as usize;
    let total: MutCounts = (0..chunks)
        .into_par_iter()
        .map(|k| {
            let r = &mut rng(seed, 30, k);
            let mut c = MutCounts::default();
            for _ in 0..per {
                let (m, f) = (homozygous_parent(r), homozygous_parent(r));
                let ch = child(&m, &f, 2, &rates, r);
                count_mutations(&ch, &m, &f, 2, &mut c);
            }
            c
        })
        .reduce(MutCounts::default, |mut a, b| {
            a.add(&b);
            a
        });
    let children = per as u64 * chunks;
    for seg in Seg::ALL {
        let i = seg.index();
        let expected = match seg {
            Seg::Genes | Seg::Obedience | Seg::MorphC => rates.mutation.rate,
            Seg::Load => rates.load,
            Seg::MorphD => rates.morph,
        } as f64;
        out.checks.push(prop_check(
            "mutation rate",
            format!("mutations per allele per generation, {}", seg.label()),
            expected,
            total.mutated[i],
            total.alleles[i],
            0.0,
            format!("{children} children, each of two fresh parents; a mutation is an allele tagged with the child's owner."),
        ));
    }
    for seg in [Seg::Genes, Seg::Obedience, Seg::MorphC] {
        let (n, s1, s2) = total.steps[seg.index()];
        let mean = s1 / n;
        let sd = (s2 / n - mean * mean).sqrt();
        out.checks.push(mean_check(
            "mutation rate",
            format!("mutation step SD, {}", seg.label()),
            rates.mutation.sd as f64,
            sd,
            sd / (2.0 * n).sqrt(),
            0.0,
            format!(
                "Gaussian step N(0, σ²) of a mutated allele, σ={}; mean step {} (expected 0).",
                rates.mutation.sd,
                num(mean)
            ),
        ));
    }
    out.checks.push(exact_check(
        "mutation rate",
        "alleles whose origin tag disagrees with mutation status",
        0,
        total.tag_errors,
        "A mutated allele carries the child's owner's tag; an inherited one keeps its parent's tag (every allele of every child checked).",
    ));
    // Step spectrum at a raised rate: every value of every locus as the
    // homozygous parental value, rate 0.5.
    let spec_rates = Rates { mutation: rates.mutation, load: 0.0, morph: 0.5 };
    let n_spec = (p.mutation_children / 5).max(10_000);
    // [locus][from position][to position]
    #[allow(clippy::type_complexity)]
    let spectra: Vec<Vec<Vec<Vec<u64>>>> = (0..chunks)
        .into_par_iter()
        .map(|k| {
            let r = &mut rng(seed, 31, k);
            let mut t: Vec<Vec<Vec<u64>>> =
                MorphLocus::ALL.iter().map(|&l| vec![vec![0u64; positions(l)]; positions(l)]).collect();
            for trial in 0..n_spec / chunks as usize {
                let mut m = Genome::founder(&[0.5; Gene::COUNT], 1, 0.05, r);
                let mut f = Genome::founder(&[0.5; Gene::COUNT], 1, 0.05, r);
                for g in [&mut m, &mut f] {
                    let mut mg = *g.morph();
                    for l in MorphLocus::ALL {
                        let v = l.value_at((trial % positions(l)) as u8);
                        mg.discrete[l.index()].a.value = v;
                        mg.discrete[l.index()].b.value = v;
                    }
                    g.set_morph(mg);
                }
                let c = child(&m, &f, 2, &spec_rates, r);
                for l in MorphLocus::ALL {
                    let from = trial % positions(l);
                    let pr = &c.morph().discrete[l.index()];
                    for a in [pr.a, pr.b] {
                        let to = if l.is_valid(a.value) { l.position(a.value) as usize } else { usize::MAX };
                        if to == usize::MAX {
                            t[l.index()][from][from] += 1 << 40; // flags an invalid value
                        } else {
                            t[l.index()][from][to] += 1;
                        }
                    }
                }
            }
            t
        })
        .collect();
    let mut bad_moves = 0u64;
    let mut boundary_out = 0u64;
    let mut table =
        String::from("| Locus | From | Down | Stay | Up | Up share (interior) |\n|---|---|---|---|---|---|\n");
    for l in MorphLocus::ALL {
        let k = positions(l);
        let (mut up, mut moved) = (0u64, 0u64);
        for from in 0..k {
            let row: Vec<u64> = (0..k).map(|to| spectra.iter().map(|t| t[l.index()][from][to]).sum()).collect();
            let stay = row[from];
            if stay >= 1 << 40 {
                bad_moves += stay >> 40;
            }
            let down = if from > 0 { row[from - 1] } else { 0 };
            let upc = if from + 1 < k { row[from + 1] } else { 0 };
            for (to, &c) in row.iter().enumerate() {
                if to.abs_diff(from) > 1 {
                    bad_moves += c;
                }
            }
            if from == 0 {
                boundary_out += down; // impossible by construction (0)
            }
            if from > 0 && from + 1 < k {
                up += upc;
                moved += upc + down;
            } else if (from == 0 && upc == 0) || (from == k - 1 && down == 0) {
                boundary_out += 1; // an end that never moved inward
            }
            table.push_str(&format!(
                "| {} | {} | {down} | {} | {upc} | {} |\n",
                l.key(),
                l.value_at(from as u8),
                stay & ((1 << 40) - 1),
                if from > 0 && from + 1 < k { num(upc as f64 / (upc + down).max(1) as f64) } else { "-".into() }
            ));
        }
        if moved > 0 {
            out.checks.push(prop_check(
                "mutation rate",
                format!("step direction from interior values, {} (share up)", l.key()),
                0.5,
                up,
                moved,
                0.0,
                format!("Step rate 0.5, every interior value as the homozygous parental value; {n_spec} children."),
            ));
        }
    }
    out.checks.push(exact_check(
        "mutation rate",
        "step mutations that skip a value, leave the range or stay put at an end",
        0,
        bad_moves + boundary_out,
        "A discrete mutation moves one step (legs and ears two units) to an adjacent valid value; at an end it moves inward.",
    ));
    out.sections.push(("Morphology step-mutation spectrum (rate 0.5)".into(), table));
}

// ---------------------------------------------------------------------------
// Dominance: F2 ratios

/// The contract's dominance rule, written out independently of the code:
/// the value nearer the default wins (positions on the axis), the lower
/// value on a tie.
fn contract_dominant(l: MorphLocus, x: u8, y: u8) -> u8 {
    let pos = |v: u8| -> i32 {
        match l {
            // Dominance order thin > bushy > prehensile > fin > club
            // (codes thin 0, bushy 1, fin 2, club 3, prehensile 4).
            MorphLocus::TailShape => [0, 1, 3, 4, 2][v as usize],
            MorphLocus::EarShape | MorphLocus::HornForm | MorphLocus::Pattern => v as i32,
            MorphLocus::LegCount | MorphLocus::EarCount => v as i32 / 2,
            MorphLocus::EyeCount => v as i32,
        }
    };
    let d = |v: u8| (pos(v) - pos(l.default_value())).abs();
    if (d(x), x) <= (d(y), y) {
        x
    } else {
        y
    }
}

fn with_morph(mut g: Genome, l: MorphLocus, a: u8, b: u8) -> Genome {
    let mut m = *g.morph();
    m.discrete[l.index()].a.value = a;
    m.discrete[l.index()].b.value = b;
    g.set_morph(m);
    g
}

fn dominance(p: &Profile, seed: u64, out: &mut Results) {
    use MorphLocus::*;
    let crosses: [(MorphLocus, u8, u8); 12] = [
        (EyeCount, 2, 4),
        (LegCount, 4, 6),
        (EarCount, 2, 0),
        (TailShape, 0, 3),
        (EarShape, 0, 3),
        (EyeCount, 5, 3),
        (EyeCount, 3, 1),
        (LegCount, 6, 2),
        (LegCount, 8, 6),
        (TailShape, 3, 2),
        (TailShape, 4, 1),
        (EarShape, 2, 1),
    ];
    let rates = Rates { mutation: Rates::defaults().mutation, load: 0.0, morph: 0.0 };
    for (ci, &(l, x, y)) in crosses.iter().enumerate() {
        let dom = contract_dominant(l, x, y);
        let rec = if dom == x { y } else { x };
        let chunks = 16u64;
        let per = p.f2_children / chunks as usize;
        // (F1 not showing the dominant, F2 dominant, F2 total, mismatches of
        // genotype vs phenotype)
        let res: Vec<(u64, u64, u64, u64)> = (0..chunks)
            .into_par_iter()
            .map(|k| {
                let r = &mut rng(seed, 40 + ci as u64, k);
                let (mut f1_bad, mut dom_n, mut n, mut mism) = (0, 0, 0, 0);
                let p1 = |r: &mut SimRng| with_morph(Genome::founder(&[0.5; Gene::COUNT], 1, 0.05, r), l, x, x);
                let p2 = |r: &mut SimRng| with_morph(Genome::founder(&[0.5; Gene::COUNT], 1, 0.05, r), l, y, y);
                for _ in 0..per {
                    let (a1, a2, b1, b2) = (p1(r), p2(r), p1(r), p2(r));
                    let fa = child(&a1, &a2, 1, &rates, r);
                    let fb = child(&b2, &b1, 1, &rates, r);
                    f1_bad += (fa.morph().expressed(l) != dom) as u64 + (fb.morph().expressed(l) != dom) as u64;
                    let f2 = child(&fa, &fb, 1, &rates, r);
                    let e = f2.morph().expressed(l);
                    let pr = &f2.morph().discrete[l.index()];
                    let homo_rec = pr.a.value == rec && pr.b.value == rec;
                    mism += ((e == rec) != homo_rec) as u64;
                    dom_n += (e == dom) as u64;
                    n += 1;
                }
                (f1_bad, dom_n, n, mism)
            })
            .collect();
        let sum = |f: fn(&(u64, u64, u64, u64)) -> u64| res.iter().map(f).sum::<u64>();
        let (f1_bad, dom_n, n, mism) = (sum(|r| r.0), sum(|r| r.1), sum(|r| r.2), sum(|r| r.3));
        let kind =
            if x == l.default_value() || y == l.default_value() { "default vs variant" } else { "contract rule" };
        let mut c = prop_check(
            "dominance",
            format!("F2 share showing {}={dom} in {x}/{x} × {y}/{y} ({kind})", l.key(), x = x, y = y),
            0.75,
            dom_n,
            n,
            0.0,
            format!(
                "3:1 Mendelian ratio; {f1_bad} of {} F1 did not show the dominant {dom}, {mism} F2 whose phenotype disagreed with their genotype (recessive shows only when homozygous).",
                2 * n
            ),
        );
        if f1_bad > 0 || mism > 0 {
            c.verdict = Verdict::Fail;
        }
        out.checks.push(c);
    }
    // Gene dominance coefficients: 0.2/0.2 × 0.8/0.8 crosses, mutation off.
    for (gi, gene) in [Gene::Coloration, Gene::Fur, Gene::Camouflage, Gene::BodySize].into_iter().enumerate() {
        let h = DOMINANCE[gene.index()] as f64;
        let chunks = 16u64;
        let per = p.f2_children / chunks as usize;
        // (low-low, het, high-high, Σ expressed, max |het expression error|)
        let res: Vec<(u64, u64, u64, f64, f64)> = (0..chunks)
            .into_par_iter()
            .map(|k| {
                let r = &mut rng(seed, 60 + gi as u64, k);
                let mut acc = (0, 0, 0, 0.0, 0.0f64);
                let par = |r: &mut SimRng, v: f32| {
                    let mut g = Genome::founder(&[0.5; Gene::COUNT], 1, 0.05, r);
                    g.set_gene_alleles(gene, v, v);
                    g
                };
                for _ in 0..per {
                    let fa = child(&par(r, 0.2), &par(r, 0.8), 1, &Rates::NONE, r);
                    let fb = child(&par(r, 0.8), &par(r, 0.2), 1, &Rates::NONE, r);
                    let f2 = child(&fa, &fb, 1, &Rates::NONE, r);
                    let l = f2.locus(gene);
                    let e = f2.expr(gene) as f64;
                    acc.3 += e;
                    match (l.a.value > 0.5, l.b.value > 0.5) {
                        (false, false) => acc.0 += 1,
                        (true, true) => acc.2 += 1,
                        _ => {
                            acc.1 += 1;
                            acc.4 = acc.4.max((e - (0.5 + 0.3 * h)).abs());
                        }
                    }
                }
                acc
            })
            .collect();
        let lo: u64 = res.iter().map(|r| r.0).sum();
        let het: u64 = res.iter().map(|r| r.1).sum();
        let hi: u64 = res.iter().map(|r| r.2).sum();
        let n = lo + het + hi;
        let mean = res.iter().map(|r| r.3).sum::<f64>() / n as f64;
        let err = res.iter().map(|r| r.4).fold(0.0, f64::max);
        let mut c = prop_check(
            "dominance",
            format!("F2 heterozygote share, {} (h={h})", gene.label()),
            0.5,
            het,
            n,
            0.0,
            format!(
                "1:2:1 genotypes ({lo}:{het}:{hi}); heterozygote expression 0.5 + h·0.3 = {}, max error {err:.2e}.",
                num(0.5 + 0.3 * h)
            ),
        );
        if err > 1e-5 {
            c.verdict = Verdict::Fail;
        }
        out.checks.push(c);
        // Var of F2 mean: classes 0.2 / het / 0.8.
        let het_e = 0.5 + 0.3 * h;
        let exp_mean = 0.25 * 0.2 + 0.5 * het_e + 0.25 * 0.8;
        let var =
            0.25 * (0.2f64 - exp_mean).powi(2) + 0.5 * (het_e - exp_mean).powi(2) + 0.25 * (0.8f64 - exp_mean).powi(2);
        out.checks.push(mean_check(
            "dominance",
            format!("F2 mean expressed value, {} (h={h})", gene.label()),
            exp_mean,
            mean,
            (var / n as f64).sqrt(),
            0.0,
            "expressed = mean + h|a-b|/2: F2 mean = 0.5 + 0.15h.",
        ));
    }
}

// ---------------------------------------------------------------------------
// Additive loci: offspring mean = midparent, segregation variance

fn additive(p: &Profile, seed: u64, out: &mut Results) {
    let k_children = 4usize;
    let batches = 50u64;
    let per = (p.families / batches as usize).max(2);
    let sites: Vec<Site> = Site::all()
        .into_iter()
        .filter(|s| match s {
            Site::Gene(k) => DOMINANCE[*k] == 0.0,
            Site::MorphC(_) => true,
            _ => false,
        })
        .collect();
    let expr = |g: &Genome, s: Site| -> f64 {
        let (a, b) = values(g, s);
        0.5 * (a + b) as f64
    };
    // Per batch, per segment: Σd, n_d, Σd², Σexpected seg var, Σ within-family var, n_fam, Σ V_A/2
    #[derive(Default, Clone, Copy)]
    struct Acc {
        d: f64,
        nd: f64,
        d2: f64,
        exp_seg: f64,
        within: f64,
        nfam: f64,
        half_va: f64,
    }
    let res: Vec<[Acc; 5]> = (0..batches)
        .into_par_iter()
        .map(|b| {
            let r = &mut rng(seed, 70, b);
            let pool = founders(400, 1, 0.15, r);
            // V_A per site in this pool: variance of expressed genotypic values.
            let va: Vec<f64> = sites
                .iter()
                .map(|&s| {
                    let xs: Vec<f64> = pool.iter().map(|g| expr(g, s)).collect();
                    let m = xs.iter().sum::<f64>() / xs.len() as f64;
                    xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (xs.len() - 1) as f64
                })
                .collect();
            let mut acc = [Acc::default(); 5];
            for _ in 0..per {
                let i = r.gen_range(0..pool.len());
                let j = (i + r.gen_range(1..pool.len())) % pool.len();
                let (m, f) = (&pool[i], &pool[j]);
                let kids: Vec<Genome> = (0..k_children).map(|_| child(m, f, 1, &Rates::NONE, r)).collect();
                for (si, &s) in sites.iter().enumerate() {
                    let a = &mut acc[s.seg().index()];
                    let mid = 0.5 * (expr(m, s) + expr(f, s));
                    let (m1, m2) = values(m, s);
                    let (f1, f2) = values(f, s);
                    let seg_var = (((m1 - m2) as f64).powi(2) + ((f1 - f2) as f64).powi(2)) / 16.0;
                    let ys: Vec<f64> = kids.iter().map(|c| expr(c, s)).collect();
                    for &y in &ys {
                        a.d += y - mid;
                        a.d2 += (y - mid) * (y - mid);
                        a.nd += 1.0;
                        a.exp_seg += seg_var;
                    }
                    let ym = ys.iter().sum::<f64>() / ys.len() as f64;
                    a.within += ys.iter().map(|y| (y - ym) * (y - ym)).sum::<f64>() / (ys.len() - 1) as f64;
                    a.nfam += 1.0;
                    a.half_va += va[si] / 2.0;
                }
            }
            acc
        })
        .collect();
    for seg in [Seg::Genes, Seg::Obedience, Seg::MorphC] {
        let i = seg.index();
        let bias: Vec<f64> = res.iter().map(|a| a[i].d / a[i].nd).collect();
        let ratio: Vec<f64> = res.iter().map(|a| a[i].d2 / a[i].exp_seg).collect();
        let half: Vec<f64> = res.iter().map(|a| a[i].within / a[i].half_va).collect();
        let (m, se) = mean_se(&bias);
        out.checks.push(mean_check(
            "additive inheritance",
            format!("offspring − midparent, {} (additive loci)", seg.label()),
            0.0,
            m,
            se,
            0.0,
            format!(
                "{} families × {k_children} children from founder pools (allele SD 0.15), mutation off.",
                per as u64 * batches
            ),
        ));
        let (m, se) = mean_se(&ratio);
        out.checks.push(mean_check(
            "additive inheritance",
            format!("segregation variance / Mendelian expectation, {}", seg.label()),
            1.0,
            m,
            se,
            0.0,
            "Var(child − midparent) = [(m1−m2)² + (f1−f2)²]/16 for parents (m1,m2), (f1,f2).",
        ));
        let (m, se) = mean_se(&half);
        out.checks.push(mean_check(
            "additive inheritance",
            format!("within-family variance / (V_A/2), {}", seg.label()),
            1.0,
            m,
            se,
            0.0,
            "In a random-mating (Hardy–Weinberg) population full sibs vary by half the additive variance.",
        ));
    }
}

// ---------------------------------------------------------------------------
// Recessive load: mutation–selection balance, inbreeding

/// Deterministic equilibrium of a recessive under viability selection `s`
/// against homozygotes and one-way mutation `mu` in the gametes: adult
/// frequency `q` → gametes `q + mu(1-q)` → zygotes in Hardy–Weinberg →
/// survivors.
pub fn recessive_equilibrium(mu: f64, s: f64) -> f64 {
    let mut q = 0.0;
    for _ in 0..200_000 {
        let g = q + mu * (1.0 - q);
        q = g * (1.0 - s * g) / (1.0 - s * g * g);
    }
    q
}

struct LoadRep {
    /// Mean adult harmful frequency over the sampled generations.
    q: f64,
    /// Trajectory of q per generation.
    traj: Vec<f64>,
    pop: Vec<Genome>,
}

fn harmful_freq(pop: &[Genome]) -> [f64; LOAD_LOCI] {
    let mut q = [0.0; LOAD_LOCI];
    for g in pop {
        for (k, l) in g.load().iter().enumerate() {
            q[k] += (l.a.is_harmful() as u8 + l.b.is_harmful() as u8) as f64;
        }
    }
    q.map(|x| x / (2 * pop.len()) as f64)
}

/// Fractions of adults homozygous and heterozygous for the harmful allele at
/// load locus `k`.
fn genotype_freqs(pop: &[Genome], k: usize) -> (f64, f64) {
    let (mut hom, mut het) = (0usize, 0usize);
    for g in pop {
        match g.load()[k].a.is_harmful() as u8 + g.load()[k].b.is_harmful() as u8 {
            2 => hom += 1,
            1 => het += 1,
            _ => {}
        }
    }
    (hom as f64 / pop.len() as f64, het as f64 / pop.len() as f64)
}

fn load_rep(n: usize, s: f64, burn: usize, gens: usize, r: &mut SimRng, sel: &mut SimRng) -> LoadRep {
    let rates = Rates::defaults();
    let mut pop = founders(n, 1, 0.05, r);
    let (mut sum, mut cnt) = (0.0, 0.0);
    let mut traj = Vec::with_capacity(burn + gens);
    for t in 0..burn + gens {
        let mut next = Vec::with_capacity(n);
        while next.len() < n {
            let m = &pop[r.gen_range(0..n)];
            let f = &pop[r.gen_range(0..n)];
            let c = child(m, f, 1, &rates, r);
            let hom = c.homozygous_load();
            if hom == 0 || sel.gen::<f64>() < (1.0 - s).powi(hom as i32) {
                next.push(c);
            }
        }
        pop = next;
        let q = harmful_freq(&pop).iter().sum::<f64>() / LOAD_LOCI as f64;
        traj.push(q);
        if t >= burn {
            sum += q;
            cnt += 1.0;
        }
    }
    LoadRep { q: sum / cnt, traj, pop }
}

fn recessive_load(p: &Profile, seed: u64, out: &mut Results) {
    let rates = Rates::defaults();
    let mu = rates.load as f64;
    let n = p.load_n;
    let (burn, gens) = (150, if p.name == "quick" { 100 } else { 250 });
    let mut final_pops = Vec::new();
    for (si, s) in [1.0f64, 0.3].into_iter().enumerate() {
        let reps: Vec<LoadRep> = (0..p.load_reps as u64)
            .into_par_iter()
            .map(|k| load_rep(n, s, burn, gens, &mut rng(seed, 80 + si as u64, k), &mut rng(seed, 90 + si as u64, k)))
            .collect();
        let q_eq = recessive_equilibrium(mu, s);
        let (m, se) = mean_se(&reps.iter().map(|r| r.q).collect::<Vec<_>>());
        out.checks.push(mean_check(
            "recessive load",
            format!("harmful-allele frequency at mutation–selection balance (s={s}, N={n})"),
            q_eq,
            m,
            se,
            0.1 * q_eq,
            format!(
                "Deterministic equilibrium of q → q+μ(1−q) → selection 1−s on homozygotes (μ={mu}); sqrt(μ/s) = {}. Mean over gens {burn}–{}, {} replicates. Tolerance 10% for drift in a finite population (rare recessives are lost by drift, so a finite population sits a little below the deterministic value).",
                num((mu / s).sqrt()),
                burn + gens,
                p.load_reps
            ),
        ));
        if si == 0 {
            let len = burn + gens;
            let xs: Vec<f64> = (0..len).step_by(4).map(|t| t as f64).collect();
            let obs: Vec<f64> =
                (0..len).step_by(4).map(|t| reps.iter().map(|r| r.traj[t]).sum::<f64>() / reps.len() as f64).collect();
            let exp: Vec<f64> = (0..len).step_by(4).map(|_| q_eq).collect();
            out.sections.push((
                "Recessive load: approach to mutation–selection balance".into(),
                ascii_plot(
                    &format!("Mean harmful-allele frequency vs generation (s=1, N={n}, μ={mu})"),
                    &xs,
                    &[("deterministic equilibrium", '.', exp), ("observed", 'o', obs)],
                    12,
                ),
            ));
            final_pops = reps.into_iter().map(|r| r.pop).collect();
        }
    }
    // Game hazard mapping (information).
    let b = Balance::default();
    let days = 30.0;
    let s_game = 1.0 - (-(b.recessive_hazard as f64) * days).exp();
    out.checks.push(Check {
        level: "genome",
        group: "recessive load",
        name: "game's implied selection and balance".into(),
        expected: format!("s ≈ {}", num(s_game)),
        observed: format!("q̂ ≈ {}", num(recessive_equilibrium(mu, s_game))),
        ci: "-".into(),
        verdict: Verdict::Info,
        detail: format!(
            "The world kills a homozygote with a daily hazard recessive_hazard={} per affected locus, so s = 1 − exp(−hazard × days to breeding); {days} days gives s={}, and the balance q̂ above.",
            b.recessive_hazard,
            num(s_game)
        ),
    });
    // Inbreeding: full-sib mating vs outcrossing in the s=1 populations.
    let trials = if p.name == "quick" { 3_000 } else { 20_000 };
    let res: Vec<(f64, f64, f64, f64)> = final_pops
        .par_iter()
        .enumerate()
        .map(|(k, pop)| {
            let r = &mut rng(seed, 95, k as u64);
            let q = harmful_freq(pop);
            let (mut out_hom, mut sib_hom) = (0u64, 0u64);
            let pick2 = |r: &mut SimRng| {
                let i = r.gen_range(0..pop.len());
                (i, (i + r.gen_range(1..pop.len())) % pop.len())
            };
            for _ in 0..trials {
                let (i, j) = pick2(r);
                out_hom += child(&pop[i], &pop[j], 1, &rates, r).homozygous_load() as u64;
                let (i, j) = pick2(r);
                let s1 = child(&pop[i], &pop[j], 1, &rates, r);
                let s2 = child(&pop[i], &pop[j], 1, &rates, r);
                sib_hom += child(&s1, &s2, 1, &rates, r).homozygous_load() as u64;
            }
            let exp_out: f64 = q.iter().map(|&q| (q + mu * (1.0 - q)).powi(2)).sum();
            // A full-sib offspring's two alleles are (1/4) copies of one
            // grandparental allele (IBD: harmful with probability q), (1/4)
            // the two alleles of one grandparent, (1/2) alleles of the two
            // grandparents; each passed through two meioses of mutation.
            // The grandparents are survivors of selection, so the
            // same-grandparent pair is homozygous at the adults' (selected)
            // homozygote frequency, not q² as in Hardy–Weinberg.
            let exp_sib: f64 = (0..LOAD_LOCI)
                .map(|k| {
                    let q = q[k];
                    let (hom, het) = genotype_freqs(pop, k);
                    let g = q + 2.0 * mu * (1.0 - q);
                    0.25 * q + 0.25 * (hom + het * 2.0 * mu) + 0.5 * g * g
                })
                .sum();
            (out_hom as f64 / trials as f64, sib_hom as f64 / trials as f64, exp_out, exp_sib)
        })
        .collect();
    // Pooled over the populations: homozygous loci are rare events, so the
    // count is Poisson around its expectation (per-population q).
    let total = (trials * res.len()) as f64;
    let oo = res.iter().map(|r| r.0).sum::<f64>() * trials as f64 / total;
    let so = res.iter().map(|r| r.1).sum::<f64>() * trials as f64 / total;
    let eo = res.iter().map(|r| r.2).sum::<f64>() / res.len() as f64;
    let es = res.iter().map(|r| r.3).sum::<f64>() / res.len() as f64;
    let (ose, sse) = ((oo / total).sqrt(), (so / total).sqrt());
    out.checks.push(mean_check(
        "recessive load",
        "affected (homozygous) loci per outcrossed offspring",
        eo,
        oo,
        ose,
        0.05 * eo,
        format!("Σ q_g² over the 4 load loci (gamete frequency q_g = q + μ(1−q)); {trials} offspring per population × {} s=1 populations at balance; Poisson SE of the pooled count.", res.len()),
    ));
    out.checks.push(mean_check(
        "recessive load",
        "affected (homozygous) loci per full-sib-mated offspring",
        es,
        so,
        sse,
        0.05 * es,
        "Per locus ¼q (identical by descent) + ¼·P(a grandparent's two alleles both harmful) + ½g² (g = q + 2μ(1−q) after two meioses). The textbook q² + Fq(1−q) with F = ¼ assumes Hardy–Weinberg grandparents; under selection against homozygotes (s=1) none of them is homozygous, so the middle term is ≈ 0 instead of ¼q². Poisson SE.",
    ));
    out.checks.push(Check {
        level: "genome",
        group: "recessive load",
        name: "inbreeding raises homozygote deaths (sib / outcross)".into(),
        expected: format!("{} (> 1)", num(es / eo)),
        observed: num(so / oo),
        ci: format!("[{}, {}]", num((so - Z_STRICT * sse) / (oo + Z_STRICT * ose)), num((so + Z_STRICT * sse) / (oo - Z_STRICT * ose).max(1e-12))),
        verdict: Verdict::strict(so - Z_STRICT * sse > oo + Z_STRICT * ose),
        detail: "Full-sib mating exposes hidden recessives; pass when the sib rate exceeds the outcross rate beyond both CIs.".into(),
    });
}

// ---------------------------------------------------------------------------
// Legacy / origin tags

fn share_of(pop: &[Genome], owner: PlayerId, buf: &mut Vec<(PlayerId, u32)>) -> f64 {
    let (mut mine, mut all) = (0u64, 0u64);
    for g in pop {
        g.count_origins_with(buf, true);
        for &(o, c) in buf.iter() {
            all += c as u64;
            if o == owner {
                mine += c as u64;
            }
        }
    }
    mine as f64 / all as f64
}

fn legacy(p: &Profile, seed: u64, out: &mut Results) {
    let n = 50usize;
    let rates = Rates::defaults();
    let times = [n / 2, n, 2 * n, 4 * n];
    // Martingale: owners 1 (30%) and 2 (70%); a child belongs to its
    // mother's owner and its mutations are tagged with that owner.
    let res: Vec<(Vec<f64>, Vec<f64>)> = (0..p.legacy_reps as u64)
        .into_par_iter()
        .map(|k| {
            let r = &mut rng(seed, 100, k);
            let mut owners: Vec<PlayerId> = (0..n).map(|i| if i < 15 { 1 } else { 2 }).collect();
            let mut pop: Vec<Genome> =
                owners.iter().map(|&o| Genome::founder(&[0.5; Gene::COUNT], o, 0.05, r)).collect();
            let mut buf = Vec::new();
            let (mut shares, mut ind) = (Vec::new(), Vec::new());
            for t in 0..=4 * n {
                if times.contains(&t) {
                    shares.push(share_of(&pop, 1, &mut buf));
                    ind.push(owners.iter().filter(|&&o| o == 1).count() as f64 / n as f64);
                }
                let mut next = Vec::with_capacity(n);
                let mut next_owners = Vec::with_capacity(n);
                for _ in 0..n {
                    let (i, j) = (r.gen_range(0..n), r.gen_range(0..n));
                    next.push(child(&pop[i], &pop[j], owners[i], &rates, r));
                    next_owners.push(owners[i]);
                }
                pop = next;
                owners = next_owners;
            }
            (shares, ind)
        })
        .collect();
    for (ti, &t) in times.iter().enumerate() {
        let (m, se) = mean_se(&res.iter().map(|r| r.0[ti]).collect::<Vec<_>>());
        out.checks.push(mean_check(
            "legacy tags",
            format!("owner's allele share at t={t} (start 0.3)"),
            0.3,
            m,
            se,
            0.0,
            format!(
                "Neutral drift is a martingale: the expected share is preserved. Legacy tally with the morphology alleles (count_origins_with(.., true)); children belong to the mother's owner and mutations are tagged with it; N={n}, {} replicates, default mutation rates.",
                p.legacy_reps
            ),
        ));
    }
    let (m, se) = mean_se(&res.iter().map(|r| r.1[times.len() - 1]).collect::<Vec<_>>());
    out.checks.push(mean_check(
        "legacy tags",
        format!("owner's share of individuals at t={} (start 0.3)", 4 * n),
        0.3,
        m,
        se,
        0.0,
        "Ownership follows the maternal line: also a martingale.",
    ));
    // Tag decay under mutation: every individual owned by 1, every allele
    // tagged 2; mutations retag at each segment's rate.
    let res: Vec<Vec<f64>> = (0..p.legacy_reps as u64)
        .into_par_iter()
        .map(|k| {
            let r = &mut rng(seed, 101, k);
            let mut pop = founders(n, 2, 0.05, r);
            let mut buf = Vec::new();
            let mut shares = Vec::new();
            for t in 0..=4 * n {
                if times.contains(&t) {
                    shares.push(share_of(&pop, 2, &mut buf));
                }
                pop = wf_generation(&pop, 1, &rates, r);
            }
            shares
        })
        .collect();
    let (mu, ml, md) = (rates.mutation.rate as f64, rates.load as f64, rates.morph as f64);
    let (ng, nl, nd, nc) = (
        2.0 * Gene::COUNT as f64,
        2.0 * LOAD_LOCI as f64,
        2.0 * MorphLocus::COUNT as f64,
        2.0 * MorphGene::COUNT as f64,
    );
    for (ti, &t) in times.iter().enumerate() {
        let t_ = t as i32;
        let exp =
            (ng * (1.0 - mu).powi(t_) + nl * (1.0 - ml).powi(t_) + nd * (1.0 - md).powi(t_) + nc * (1.0 - mu).powi(t_))
                / (ng + nl + nd + nc);
        let (m, se) = mean_se(&res.iter().map(|r| r[ti]).collect::<Vec<_>>());
        out.checks.push(mean_check(
            "legacy tags",
            format!("designer's share after t={t} gens of mutation tagged to the owner"),
            exp,
            m,
            se,
            0.0,
            format!("E[share] = Σ_segments w (1−μ)^t: genes and continuous morph μ={mu}, load (functional → harmful) {ml}, discrete morph {md}. Drift leaves the expectation alone."),
        ));
    }
}

// ---------------------------------------------------------------------------
// Linkage: phase-known test cross

fn linkage(p: &Profile, seed: u64, out: &mut Results) {
    let sites = Site::all();
    let ns = sites.len();
    // Mother: every site heterozygous, phase known (a = variant).
    let mut mother = Genome::founder(&[0.5; Gene::COUNT], 1, 0.0, &mut rng(seed, 110, 0));
    for &s in &sites {
        marker_set(&mut mother, s, true, false);
    }
    // Father: heterozygous at the continuous sites with other values, so the
    // child's gene background varies on both sides; default elsewhere.
    let mut father = Genome::founder(&[0.5; Gene::COUNT], 1, 0.0, &mut rng(seed, 110, 1));
    for k in 0..Gene::COUNT {
        father.set_gene_alleles(Gene::ALL[k], 0.6, 0.4);
    }
    let mut fm = *father.morph();
    for c in &mut fm.continuous {
        (c.a.value, c.b.value) = (0.6, 0.4);
    }
    father.set_morph(fm);
    let chunks = 64u64;
    let per = p.linkage_children / chunks as usize;
    let res: Vec<(Vec<u64>, Vec<u64>)> = (0..chunks)
        .into_par_iter()
        .map(|k| {
            let r = &mut rng(seed, 111, k);
            let mut single = vec![0u64; ns];
            let mut same = vec![0u64; ns * ns];
            let mut bits = vec![false; ns];
            for _ in 0..per {
                let c = child(&mother, &father, 1, &Rates::NONE, r);
                for (i, &s) in sites.iter().enumerate() {
                    bits[i] = marker_get(&c, s).0;
                    single[i] += bits[i] as u64;
                }
                for i in 0..ns {
                    for j in i + 1..ns {
                        same[i * ns + j] += (bits[i] == bits[j]) as u64;
                    }
                }
            }
            (single, same)
        })
        .collect();
    let n = (per as u64 * chunks) as f64;
    let single: Vec<f64> = (0..ns).map(|i| res.iter().map(|r| r.0[i]).sum::<u64>() as f64 / n).collect();
    let se = (0.25 / n).sqrt();
    let (worst_i, worst_t) =
        single.iter().enumerate().map(|(i, &f)| (i, (f - 0.5).abs() / se)).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    // Bonferroni z at two-sided 0.001 over the loci / pairs.
    let z_bonf = |m: f64| -> f64 {
        // Inverse of the normal tail by bisection.
        let target = 0.001 / m / 2.0;
        let tail = |z: f64| 0.5 * erfc(z / std::f64::consts::SQRT_2);
        let (mut lo, mut hi) = (0.0, 10.0);
        for _ in 0..100 {
            let mid = 0.5 * (lo + hi);
            if tail(mid) > target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    };
    let zs = z_bonf(ns as f64);
    out.checks.push(Check {
        level: "genome",
        group: "linkage",
        name: format!("transmission ratio at each of {ns} loci (Mendel's first law)"),
        expected: "0.5".into(),
        observed: format!("worst {} = {}", sites[worst_i].label(), num(single[worst_i])),
        ci: format!("max |z| = {worst_t:.2} (limit {zs:.2})"),
        verdict: Verdict::strict(worst_t <= zs),
        detail: format!("{n} offspring of a phase-known heterozygote; Bonferroni limit at two-sided 0.001."),
    });
    let pairs = ns * (ns - 1) / 2;
    let zp = z_bonf(pairs as f64);
    let mut chi = 0.0;
    let mut worst = (0.0f64, 0, 0, 0.0);
    let mut by_seg: std::collections::BTreeMap<(usize, usize), (f64, f64)> = Default::default();
    for i in 0..ns {
        for j in i + 1..ns {
            let rf = res.iter().map(|r| r.1[i * ns + j]).sum::<u64>() as f64 / n;
            let z = (rf - 0.5) / se;
            chi += z * z;
            if z.abs() > worst.0 {
                worst = (z.abs(), i, j, rf);
            }
            let (a, b) = (sites[i].seg().index(), sites[j].seg().index());
            let e = by_seg.entry((a.min(b), a.max(b))).or_default();
            e.0 += (rf - 0.5).abs();
            e.1 += 1.0;
        }
    }
    out.checks.push(Check {
        level: "genome",
        group: "linkage",
        name: format!("parental-strand agreement for all {pairs} locus pairs (independent assortment)"),
        expected: "0.5 (unlinked)".into(),
        observed: format!("worst {}–{} = {}", sites[worst.1].label(), sites[worst.2].label(), num(worst.3)),
        ci: format!("max |z| = {:.2} (limit {zp:.2})", worst.0),
        verdict: Verdict::strict(worst.0 <= zp),
        detail: "Share of offspring that took the same maternal strand at both loci; 0.5 means a recombination fraction of 1/2 (no linkage), including across the hash-seeded segments.".into(),
    });
    let df = pairs as f64;
    out.checks.push(mean_check(
        "linkage",
        "Σ z² over all locus pairs (global independence)",
        df,
        chi,
        (2.0 * df).sqrt(),
        0.0,
        format!("Chi-square with {pairs} degrees of freedom (pairwise strand agreements are uncorrelated under independence): mean {pairs}, SD √(2·{pairs})."),
    ));
    let mut table = String::from("| Segment pair | Pairs | Mean abs(r − 0.5) |\n|---|---|---|\n");
    for ((a, b), (s, c)) in by_seg {
        table.push_str(&format!("| {} × {} | {c} | {} |\n", Seg::ALL[a].label(), Seg::ALL[b].label(), num(s / c)));
    }
    table.push_str(&format!(
        "\nExpected mean |r − 0.5| under independence: {} (n = {n}).\n",
        num(se * (2.0 / std::f64::consts::PI).sqrt())
    ));
    out.sections.push(("Linkage by segment".into(), table));
    hash_streams(p, seed, out);
}

/// Complementary error function (Numerical Recipes' erfcc, |error| < 1.2e-7).
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t
        * (-z * z - 1.265_512_23
            + t * (1.000_023_68
                + t * (0.374_091_96
                    + t * (0.096_784_18
                        + t * (-0.186_288_06
                            + t * (0.278_868_07
                                + t * (-1.135_203_98
                                    + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
            .exp();
    if x >= 0.0 {
        r
    } else {
        2.0 - r
    }
}

/// The obedience and morphology loci draw from streams seeded by a hash of
/// the child's gene alleles. In a gene background with no variation at all
/// (every parent homozygous for the same gene alleles), every child of the
/// same parental morphology alleles then gets the same draws. Reported for
/// information: it never arises under the default mutation rate.
fn hash_streams(p: &Profile, seed: u64, out: &mut Results) {
    let n = (p.f2_children / 4).max(2_000) as u64;
    let r = &mut rng(seed, 120, 0);
    let mut parent = Genome::uniform(&[0.5; Gene::COUNT], 1);
    parent = with_morph(parent, MorphLocus::EyeCount, 2, 4);
    for (label, rates) in [("mutation off", Rates::NONE), ("default mutation", Rates::defaults())] {
        let mut hom4 = 0u64;
        let mut genotypes = std::collections::BTreeSet::new();
        for _ in 0..n {
            let c = child(&parent, &parent, 1, &rates, r);
            let pr = &c.morph().discrete[MorphLocus::EyeCount.index()];
            genotypes.insert((pr.a.value, pr.b.value));
            hom4 += (pr.a.value == 4 && pr.b.value == 4) as u64;
        }
        out.checks.push(Check {
            level: "genome",
            group: "hash-seeded streams",
            name: format!("eye 2/4 × 2/4 with identical homozygous gene backgrounds ({label})"),
            expected: "0.25 eye 4/4 (Mendel)".into(),
            observed: format!("{} 4/4, {} distinct genotypes", num(hom4 as f64 / n as f64), genotypes.len()),
            ci: "-".into(),
            verdict: Verdict::Info,
            detail: format!(
                "{n} children of two genomes with identical, fully homozygous gene loci (Genome::uniform). The morphology stream is seeded from the child's gene alleles and the parents' morphology alleles, so without gene variation every child draws the same segregation; with default mutation only the children that carry a new gene mutation draw afresh. Founders vary (Genome::founder), and the mutation rate keeps every real lineage polymorphic, so this never arises in a world (see the ecology level's sibling-background measurement)."
            ),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_wf_times_match_diffusion_roughly() {
        let (tau, tau1) = wf_exact_times(50, 50);
        assert!((tau / (2.0 * 50.0 * 2.0f64.ln() * 2.0) - 1.0).abs() < 0.05, "{tau}");
        assert!(tau1 > tau);
        let (tau, _) = wf_exact_times(10, 1);
        assert!(tau > 0.0 && tau < 20.0);
    }

    #[test]
    fn path_stationary_is_stationary() {
        for k in [3, 4, 5, 6] {
            let pi = path_stationary(k);
            let mut next = vec![0.0; k];
            for i in 0..k {
                if i == 0 {
                    next[1] += pi[0];
                } else if i == k - 1 {
                    next[k - 2] += pi[i];
                } else {
                    next[i - 1] += pi[i] / 2.0;
                    next[i + 1] += pi[i] / 2.0;
                }
            }
            for i in 0..k {
                assert!((next[i] - pi[i]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn contract_rule_matches_the_code() {
        for l in MorphLocus::ALL {
            let (lo, hi) = l.range();
            for x in lo..=hi {
                for y in lo..=hi {
                    if l.is_valid(x) && l.is_valid(y) {
                        let pair = evo_sim::morph::MorphLocusPair {
                            a: evo_sim::morph::MorphAllele { value: x, origin: 0 },
                            b: evo_sim::morph::MorphAllele { value: y, origin: 0 },
                        };
                        assert_eq!(l.express(&pair), contract_dominant(l, x, y), "{l:?} {x} {y}");
                    }
                }
            }
        }
    }

    /// The CI-sized genome-level run: every strict check passes (fixed seed,
    /// so it never flakes).
    #[test]
    fn quick_genome_checks_pass() {
        let r = run_all(&Profile::QUICK, 1, None, false);
        let failed: Vec<String> = r
            .checks
            .iter()
            .filter(|c| c.verdict == Verdict::Fail)
            .map(|c| format!("{} / {}: expected {} observed {} {}", c.group, c.name, c.expected, c.observed, c.ci))
            .collect();
        assert!(failed.is_empty(), "failed checks:\n{}", failed.join("\n"));
        assert!(r.checks.iter().filter(|c| c.verdict == Verdict::Pass).count() > 40);
    }

    /// The long genome-level run (minutes): `cargo test --release -p
    /// evo-sim-cli -- --ignored long_genome_checks_pass`.
    #[test]
    #[ignore]
    fn long_genome_checks_pass() {
        let r = run_all(&Profile::LONG, 1, None, true);
        let failed: Vec<String> = r
            .checks
            .iter()
            .filter(|c| c.verdict == Verdict::Fail)
            .map(|c| format!("{} / {}: expected {} observed {} {}", c.group, c.name, c.expected, c.observed, c.ci))
            .collect();
        assert!(failed.is_empty(), "failed checks:\n{}", failed.join("\n"));
    }
}
